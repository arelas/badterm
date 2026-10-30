//! A live connection to a BBS: protocol decoding, terminal state, ZMODEM
//! transfers and session capture. Networking lives in [`crate::transport`].

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::ansi::{TermEvent, Terminal};
use crate::dialer::Entry;
use crate::telnet::Telnet;
use crate::transfer::{self, Download, Outcome, TrailerFilter, Transfer, Upload};
pub use crate::transport::{AuthPrompt, ConnectOptions, NetEvent, PromptKind, Writer, spawn_connection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Connecting,
    Connected,
    Closed,
}

/// Where a session keeps the files it produces.
#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub scrollback: usize,
    pub known_hosts: PathBuf,
    pub download_dir: PathBuf,
    /// Offer the SSH agent's keys when logging in.
    pub ssh_agent: bool,
    /// Where default SSH keys (`id_ed25519`, ...) are looked for.
    pub ssh_key_dir: Option<PathBuf>,
}

/// Things the front end should react to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// The connection is up (for SSH, the login succeeded).
    Connected,
    Term(TermEvent),
    Notice(String),
    /// The BBS is waiting to receive files (it sent ZRINIT); ask the user
    /// what to upload, then call [`Session::start_upload`] or
    /// [`Session::decline_upload`].
    UploadRequested,
    /// The login has a question for the user: see [`Session::auth_prompt`],
    /// and reply with [`Session::answer_prompt`].
    AuthPrompt,
    TransferStarted,
    TransferFinished(Outcome),
}

pub struct Session {
    pub entry: Entry,
    pub term: Terminal,
    pub state: State,
    /// Why the session closed, if it failed.
    pub error: Option<String>,
    config: SessionConfig,
    telnet: Option<Telnet>,
    writer: Option<Writer>,
    capture: Option<(PathBuf, BufWriter<File>)>,
    transfer: Option<Transfer>,
    /// Bytes received since the BBS asked for an upload, held until the user
    /// picks files (they include the ZRINIT the sender needs to see).
    upload_wait: Option<Vec<u8>>,
    auth_prompt: Option<AuthPrompt>,
    /// The end of the last data chunk, so a ZMODEM header split across two
    /// reads is still recognized.
    scan_tail: Vec<u8>,
    /// Hides ZMODEM's closing bytes from the terminal after a transfer.
    trailer: Option<TrailerFilter>,
    started: Instant,
    connected_at: Option<Instant>,
    events: Vec<SessionEvent>,
}

impl Session {
    pub fn new(entry: Entry, cols: usize, rows: usize, config: SessionConfig) -> Session {
        let mut term = Terminal::new(cols, rows, config.scrollback);
        term.encoding = entry.encoding;
        let telnet = entry.protocol.is_telnet().then(|| Telnet::new(&entry.term_type, cols as u16, rows as u16));
        Session {
            entry,
            term,
            state: State::Connecting,
            error: None,
            config,
            telnet,
            writer: None,
            capture: None,
            transfer: None,
            upload_wait: None,
            auth_prompt: None,
            scan_tail: Vec::new(),
            trailer: None,
            started: Instant::now(),
            connected_at: None,
            events: Vec::new(),
        }
    }

    /// Start connecting in the background; events arrive through `sink` and
    /// must be passed to [`Session::handle`].
    pub fn connect<F>(&self, sink: F)
    where
        F: Fn(NetEvent) + Send + 'static,
    {
        let opts = ConnectOptions {
            cols: self.term.screen.cols() as u16,
            rows: self.term.screen.rows() as u16,
            known_hosts: self.config.known_hosts.clone(),
            ssh_agent: self.config.ssh_agent,
            ssh_key_dir: self.config.ssh_key_dir.clone(),
        };
        spawn_connection(&self.entry, &opts, sink);
    }

    pub fn handle(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Connected(w) => {
                self.writer = Some(w);
                self.state = State::Connected;
                self.connected_at = Some(Instant::now());
                self.events.push(SessionEvent::Connected);
            }
            NetEvent::Data(bytes) => self.receive(&bytes),
            NetEvent::Notice(msg) => self.events.push(SessionEvent::Notice(msg)),
            NetEvent::Prompt(p) => {
                self.auth_prompt = Some(p);
                self.events.push(SessionEvent::AuthPrompt);
            }
            NetEvent::Closed(err) => {
                self.auth_prompt = None;
                self.state = State::Closed;
                self.error = err;
                self.writer = None;
                self.end_transfer(Some(Outcome::Failed("the connection closed".into())));
            }
        }
    }

    fn receive(&mut self, bytes: &[u8]) {
        let data = match &mut self.telnet {
            Some(t) => {
                let (mut data, mut reply) = (Vec::with_capacity(bytes.len()), Vec::new());
                t.receive(bytes, &mut data, &mut reply);
                if !reply.is_empty() {
                    self.write_raw(&reply);
                }
                data
            }
            None => bytes.to_vec(),
        };
        self.process(data);
    }

    /// Route decoded data to an active transfer, or detect a new one, or
    /// show it on the terminal.
    fn process(&mut self, mut data: Vec<u8>) {
        if let Some(f) = &mut self.trailer
            && !f.strip(&mut data)
        {
            self.trailer = None;
        }
        if let Some(t) = &mut self.transfer {
            let mut out = Vec::new();
            t.feed(&data, &mut out);
            self.send(&out);
            self.end_transfer(None);
            return;
        }
        if let Some(buf) = &mut self.upload_wait {
            buf.extend_from_slice(&data);
            return;
        }

        let mut scan = std::mem::take(&mut self.scan_tail);
        let already_shown = scan.len();
        scan.extend_from_slice(&data);
        if let Some(i) = transfer::find(&scan, transfer::ZRQINIT) {
            self.show(&scan[already_shown.min(i)..i]);
            match Download::new(self.config.download_dir.clone()) {
                Ok(d) => {
                    self.transfer = Some(Transfer::Download(d));
                    self.events.push(SessionEvent::TransferStarted);
                    self.process(scan[i..].to_vec());
                }
                Err(e) => self.events.push(SessionEvent::Notice(format!("Cannot start download: {e:#}"))),
            }
            return;
        }
        if let Some(i) = transfer::find(&scan, transfer::ZRINIT) {
            self.show(&scan[already_shown.min(i)..i]);
            self.upload_wait = Some(scan[i..].to_vec());
            self.events.push(SessionEvent::UploadRequested);
            return;
        }
        let keep = scan.len().min(transfer::ZRQINIT.len() - 1);
        self.scan_tail = scan[scan.len() - keep..].to_vec();
        self.show(&data);
    }

    fn show(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if let Some((_, f)) = &mut self.capture {
            let _ = f.write_all(data);
        }
        self.term.feed(data);
        let replies = self.term.take_replies();
        if !replies.is_empty() {
            self.send(&replies);
        }
        self.events.extend(self.term.take_events().into_iter().map(SessionEvent::Term));
    }

    /// If the transfer finished (or `force` gives a reason to stop it),
    /// report the outcome and return its trailing bytes to the terminal.
    fn end_transfer(&mut self, force: Option<Outcome>) {
        let done = self.transfer.as_ref().is_some_and(Transfer::is_finished);
        if !done && force.is_none() {
            return;
        }
        let Some(t) = self.transfer.take() else { return };
        let (outcome, leftover) = t.finish();
        let outcome = if done { outcome } else { force.unwrap_or(outcome) };
        self.events.push(SessionEvent::TransferFinished(outcome));
        self.scan_tail.clear();
        self.trailer = Some(TrailerFilter::new());
        self.process(leftover);
    }

    pub fn take_events(&mut self) -> Vec<SessionEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn transfer(&self) -> Option<&Transfer> {
        self.transfer.as_ref()
    }

    /// The login question waiting for an answer, if any.
    pub fn auth_prompt(&self) -> Option<&AuthPrompt> {
        self.auth_prompt.as_ref()
    }

    /// Answer the waiting login question; `None` cancels the login.
    pub fn answer_prompt(&mut self, answer: Option<String>) {
        if let Some(p) = self.auth_prompt.take() {
            p.answer(answer);
        }
    }

    /// Whether the BBS is waiting for the user to choose files to upload.
    pub fn upload_requested(&self) -> bool {
        self.upload_wait.is_some()
    }

    /// Send files with ZMODEM. If a batch is already being sent, the files
    /// are added to it.
    pub fn start_upload(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        if let Some(t) = &mut self.transfer {
            for p in paths {
                t.enqueue(p)?;
            }
            return Ok(());
        }
        let up = Upload::new(paths)?;
        self.transfer = Some(Transfer::Upload(up));
        self.events.push(SessionEvent::TransferStarted);
        // Replay what the BBS sent while waiting (its ZRINIT), if anything.
        let waiting = self.upload_wait.take().unwrap_or_default();
        self.process(waiting);
        Ok(())
    }

    /// The user chose not to upload: make the BBS's receiver give up.
    pub fn decline_upload(&mut self) {
        if self.upload_wait.take().is_some() {
            self.send(transfer::CANCEL);
        }
    }

    pub fn cancel_transfer(&mut self) {
        if let Some(t) = &mut self.transfer {
            let mut out = Vec::new();
            t.cancel(&mut out);
            self.send(&out);
            self.end_transfer(None);
        }
    }

    /// Call periodically (a few times a second) to drive transfer timeouts.
    pub fn tick(&mut self) {
        if let Some(t) = &mut self.transfer {
            let mut out = Vec::new();
            t.tick(Instant::now(), &mut out);
            self.send(&out);
            self.end_transfer(None);
        }
    }

    /// Send application data (keystrokes, paste, transfer data) to the remote.
    pub fn send(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let encoded = match &self.telnet {
            Some(t) => t.encode(data),
            None => data.to_vec(),
        };
        self.write_raw(&encoded);
    }

    fn write_raw(&mut self, data: &[u8]) {
        if let Some(w) = &mut self.writer
            && let Err(e) = w.write_all(data)
        {
            self.error = Some(e.to_string());
            self.hang_up();
        }
    }

    pub fn hang_up(&mut self) {
        self.answer_prompt(None);
        if let Some(w) = self.writer.take() {
            w.shutdown();
        }
        self.state = State::Closed;
        self.stop_capture();
    }

    /// Time since connecting (or since dialing, while still connecting).
    pub fn elapsed(&self) -> Duration {
        self.connected_at.unwrap_or(self.started).elapsed()
    }

    pub fn download_dir(&self) -> &Path {
        &self.config.download_dir
    }

    pub fn capture_path(&self) -> Option<&Path> {
        self.capture.as_ref().map(|(p, _)| p.as_path())
    }

    /// Start recording received data (after telnet decoding) to a file.
    pub fn start_capture(&mut self, path: PathBuf) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let f = File::options()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        self.capture = Some((path, BufWriter::new(f)));
        Ok(())
    }

    pub fn stop_capture(&mut self) {
        if let Some((_, mut f)) = self.capture.take() {
            let _ = f.flush();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.hang_up();
    }
}
