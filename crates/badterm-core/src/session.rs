//! A live connection to a BBS: protocol decoding, terminal state, ZMODEM
//! transfers and session capture. Networking lives in [`crate::transport`].

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::ansi::{TermEvent, Terminal};
use crate::autologin::{AutoLogin, Step};
use crate::dialer::Entry;
use crate::screen::Screen;
use crate::telnet::Telnet;
use crate::transfer::{
    self, Download, Kermit, Outcome, TrailerFilter, Transfer, Upload, XyProtocol, XyReceive, XySend,
};
pub use crate::transport::{AuthPrompt, ConnectOptions, NetEvent, PromptKind, Writer, spawn_connection};

/// How long the BBS must be quiet before the text at the cursor is taken
/// for a prompt.
const PROMPT_QUIET: Duration = Duration::from_millis(250);

/// How soon after answering a BBS that hangs up may be called busy.
const BUSY_WINDOW: Duration = Duration::from_secs(30);

/// Whether the screen says the BBS has no free line.
fn says_busy(screen: &Screen) -> bool {
    const WORDS: [&str; 6] = ["busy", "no free node", "all nodes", "all lines", "try again later", "too many users"];
    (0..screen.rows()).any(|y| {
        let line = Screen::row_text(screen.row(y)).to_ascii_lowercase();
        WORDS.iter().any(|w| line.contains(w))
    })
}

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
    /// The modem, for modem entries.
    pub modem: crate::modem::ModemConfig,
}

/// Things the front end should react to.
#[derive(Clone, Debug, PartialEq)]
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
    capture: Option<Capture>,
    transfer: Option<Transfer>,
    /// The transfer is a RIP one whose RIP files are shown as they come.
    rip_shows: bool,
    /// The transfer is SkyPix's, of a brush or sample of this mode.
    skypix_load: Option<u32>,
    /// Bytes received since the BBS asked for an upload, held until the user
    /// picks files (they include the ZRINIT the sender needs to see).
    upload_wait: Option<Vec<u8>>,
    auth_prompt: Option<AuthPrompt>,
    login: AutoLogin,
    /// When data last arrived, until a login prompt check has looked at it.
    quiet_since: Option<Instant>,
    /// Baud rate emulation: received data waiting to be shown, how many
    /// bytes may go now, and when that was worked out.
    paced: VecDeque<u8>,
    pace_credit: f64,
    pace_at: Instant,
    /// The end of the last data chunk, so a ZMODEM header split across two
    /// reads is still recognized.
    scan_tail: Vec<u8>,
    /// Hides ZMODEM's closing bytes from the terminal after a transfer.
    trailer: Option<TrailerFilter>,
    started: Instant,
    connected_at: Option<Instant>,
    events: Vec<SessionEvent>,
    /// A copy of what is shown, for a script, while it is Some.
    pub tap: Option<Vec<u8>>,
}

impl Session {
    pub fn new(entry: Entry, cols: usize, rows: usize, config: SessionConfig) -> Session {
        let login = AutoLogin::new(&entry);
        let mut term = Terminal::new(cols, rows, config.scrollback);
        term.encoding = entry.encoding;
        term.screen.base_font = entry.font;
        term.screen.base_ice = entry.ice.unwrap_or(false);
        term.screen.ice_colors = term.screen.base_ice;
        term.screen.look.fonts[0] = entry.font;
        // A C64 or Atari system: its colors and font.
        term.start_emulation();
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
            rip_shows: false,
            skypix_load: None,
            upload_wait: None,
            auth_prompt: None,
            login,
            quiet_since: None,
            paced: VecDeque::new(),
            pace_credit: 0.0,
            pace_at: Instant::now(),
            scan_tail: Vec::new(),
            trailer: None,
            started: Instant::now(),
            connected_at: None,
            events: Vec::new(),
            tap: None,
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
            modem: self.config.modem.clone(),
        };
        spawn_connection(&self.entry, &opts, sink);
    }

    pub fn handle(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Connected(w) => {
                self.writer = Some(w);
                self.state = State::Connected;
                self.connected_at = Some(Instant::now());
                self.login.connected();
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
        // Transfers are not slowed down.
        if self.entry.baud == 0 || self.transfer.is_some() && self.paced.is_empty() {
            self.process(data);
            self.quiet_since = Some(Instant::now());
        } else {
            if self.paced.is_empty() {
                self.pace_at = Instant::now();
            }
            self.paced.extend(data);
            self.pump();
        }
    }

    /// Whether received data is waiting for baud rate emulation: call
    /// [`Session::pump`] often (every frame) while it is.
    pub fn pacing(&self) -> bool {
        !self.paced.is_empty()
    }

    /// Show as much paced data as the emulated baud rate allows by now: ten
    /// bits per byte, as with a modem's start and stop bits.
    pub fn pump(&mut self) {
        if self.paced.is_empty() {
            return;
        }
        let now = Instant::now();
        self.pace_credit += now.duration_since(self.pace_at).as_secs_f64() * self.entry.baud as f64 / 10.0;
        self.pace_at = now;
        let n = (self.pace_credit as usize).min(self.paced.len());
        if n == 0 {
            return;
        }
        self.pace_credit -= n as f64;
        let chunk: Vec<u8> = self.paced.drain(..n).collect();
        self.process(chunk);
        // A transfer that started in that chunk gets the rest at once.
        if self.transfer.is_some() {
            let rest: Vec<u8> = self.paced.drain(..).collect();
            self.process(rest);
        }
        if self.paced.is_empty() {
            self.pace_credit = 0.0;
        }
        self.quiet_since = Some(now);
    }

    /// Answer a login prompt, once the BBS has been quiet for a moment (so
    /// half a line is not taken for a prompt).
    fn check_login_prompt(&mut self) {
        let Some(t) = self.quiet_since else { return };
        if t.elapsed() < PROMPT_QUIET || self.transfer.is_some() || self.upload_wait.is_some() {
            return;
        }
        self.quiet_since = None;
        let screen = &self.term.screen;
        let (x, y) = screen.cursor();
        let row = screen.row(y);
        // A C64's or Atari's screen holds screen codes, not ASCII.
        let prompt = match self.term.text_codepage().filter(|cp| cp.screen_codes()) {
            Some(cp) => row[..x.min(row.len())].iter().map(|c| cp.decode(c.ch)).collect(),
            None => Screen::row_text(&row[..x.min(row.len())]),
        };
        if let Some(step) = self.login.answer(&prompt) {
            self.send_login_step(step);
        }
    }

    /// A click on the RIP graphics: send what it sends. False if it sends
    /// nothing (now: a question for the user may come first).
    pub fn rip_press(&mut self, x: usize, y: usize) -> bool {
        let out = self.term.rip_press(x, y);
        if let Some(text) = &out {
            self.send(text);
        }
        self.events.extend(self.term.take_events().into_iter().map(SessionEvent::Term));
        out.is_some()
    }

    /// Answer RIPscrip's question for a text variable (None: cancelled),
    /// sending what was waiting for it.
    pub fn answer_rip(&mut self, answer: Option<String>) {
        self.term.rip_answer(answer);
        let replies = self.term.take_replies();
        if !replies.is_empty() {
            self.send(&replies);
        }
        self.events.extend(self.term.take_events().into_iter().map(SessionEvent::Term));
    }

    /// Send the next part of the stored login (Alt-L): the user name, then
    /// the password. False when there is nothing (more) to send.
    pub fn send_login(&mut self) -> bool {
        match self.login.next_step() {
            Some(step) => {
                self.send_login_step(step);
                true
            }
            None => false,
        }
    }

    /// Whether the entry has a user name or password to log in with.
    pub fn has_login(&self) -> bool {
        self.login.has_login()
    }

    fn send_login_step(&mut self, step: Step) {
        let (text, what) = match step {
            Step::User(u) => (u, "user name"),
            Step::Password(p) => (p, "password"),
        };
        let bytes = self.term.encoding.encode_text(&format!("{text}\r"));
        self.send(&bytes);
        self.events.push(SessionEvent::Notice(format!("Sent your {what}")));
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
        // A Kermit sending: its Send-Init packet.
        if let Some(i) = crate::kermit::find_send_init(&scan) {
            self.show(&scan[already_shown.min(i)..i]);
            self.transfer = Some(Transfer::Kermit(Kermit::receive(self.config.download_dir.clone())));
            self.events.push(SessionEvent::TransferStarted);
            self.process(scan[i..].to_vec());
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
        if let Some(c) = &mut self.capture {
            c.write(data, self.term.encoding);
        }
        if let Some(t) = &mut self.tap {
            t.extend_from_slice(data);
        }
        self.term.feed(data);
        let replies = self.term.take_replies();
        if !replies.is_empty() {
            self.send(&replies);
        }
        self.events.extend(self.term.take_events().into_iter().map(SessionEvent::Term));
        if let Some((block, rest)) = self.term.take_rip_block() {
            self.start_rip_block(&block);
            self.process(rest);
        }
        if let Some((t, rest)) = self.term.take_skypix_transfer() {
            // To the BBS's folder in the cache (BRU:, on the Amiga).
            let dir = match &self.term.syncterm.cache_dir {
                Some(c) => c.join("SkyPix"),
                None => self.config.download_dir.clone(),
            };
            let name = transfer::sanitize_file_name(t.name.as_bytes());
            let started = std::fs::create_dir_all(&dir)
                .map_err(anyhow::Error::from)
                .and_then(|()| self.start_xy_download_to(XyProtocol::XmodemCrc, dir, Some(name)));
            match started {
                Ok(()) => self.skypix_load = Some(t.mode),
                Err(e) => {
                    self.send(transfer::CANCEL);
                    self.events.push(SessionEvent::Notice(format!("SkyPix transfer: {e:#}")));
                }
            }
            self.process(rest);
        }
    }

    /// A file transfer RIPscrip started: files go to (or come from) the
    /// BBS's RIP folder.
    fn start_rip_block(&mut self, b: &crate::rip::BlockTransfer) {
        let dir = match &self.term.syncterm.cache_dir {
            Some(c) => c.join("RIP"),
            None => self.config.download_dir.clone(),
        };
        let xy = match b.protocol {
            0 => Some(XyProtocol::Xmodem),
            1 => Some(XyProtocol::XmodemCrc),
            2 | 3 => Some(XyProtocol::Xmodem1k),
            5 => Some(XyProtocol::Ymodem),
            6 => Some(XyProtocol::YmodemG),
            _ => None,
        };
        let started: Result<()> = (|| {
            if b.upload {
                // Only a file in the RIP folder, by its name.
                let path = b.name.as_ref().map(|n| dir.join(n)).filter(|p| p.is_file());
                let Some(path) = path else { anyhow::bail!("the BBS asked for a file that isn't here") };
                match (b.protocol, xy) {
                    (_, Some(p)) => self.start_xy_upload(p, vec![path]),
                    (4, _) => self.start_kermit_upload(vec![path]),
                    _ => {
                        self.transfer = Some(Transfer::Upload(Upload::new(vec![path])?));
                        self.events.push(SessionEvent::TransferStarted);
                        Ok(())
                    }
                }
            } else {
                std::fs::create_dir_all(&dir)?;
                match (b.protocol, xy) {
                    (_, Some(p)) => self.start_xy_download_to(p, dir.clone(), b.name.clone()),
                    (4, _) => {
                        self.transfer = Some(Transfer::Kermit(Kermit::receive(dir.clone())));
                        self.events.push(SessionEvent::TransferStarted);
                        Ok(())
                    }
                    _ => {
                        self.transfer = Some(Transfer::Download(Download::new(dir.clone())?));
                        self.events.push(SessionEvent::TransferStarted);
                        Ok(())
                    }
                }
            }
        })();
        match started {
            Ok(()) => self.rip_shows = !b.upload && b.shows(),
            Err(e) => {
                // As RIPterm refuses: ten CANs.
                self.send(&[0x18; 10]);
                self.events.push(SessionEvent::Notice(format!("RIP file transfer: {e:#}")));
            }
        }
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
        // RIP files a RIP transfer brought, to be shown.
        let show: Vec<PathBuf> = match &outcome {
            Outcome::Completed(files) if std::mem::take(&mut self.rip_shows) => {
                files.iter().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("rip"))).cloned().collect()
            }
            _ => Vec::new(),
        };
        self.rip_shows = false;
        // A SkyPix brush or sample, for it to use.
        if let (Some(mode), Outcome::Completed(files)) = (self.skypix_load.take(), &outcome)
            && let Some(data) = files.first().and_then(|f| std::fs::read(f).ok())
            && let Some(sky) = self.term.skypix_mut()
        {
            sky.load(mode, &data);
        }
        self.events.push(SessionEvent::TransferFinished(outcome));
        self.scan_tail.clear();
        self.trailer = Some(TrailerFilter::new());
        for f in show {
            match std::fs::read(&f) {
                Ok(data) => self.show(&data),
                Err(e) => log::warn!("showing {}: {e}", f.display()),
            }
        }
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

    /// Send files with XMODEM or YMODEM, once the BBS's receiver starts.
    pub fn start_xy_upload(&mut self, proto: XyProtocol, paths: Vec<PathBuf>) -> Result<()> {
        if self.transfer.is_some() {
            anyhow::bail!("a transfer is already in progress");
        }
        let up = XySend::new(proto, paths)?;
        self.upload_wait = None;
        self.transfer = Some(Transfer::XyUpload(up));
        self.events.push(SessionEvent::TransferStarted);
        Ok(())
    }

    /// Receive with XMODEM or YMODEM: asks the BBS's sender to start.
    /// `name` is what an XMODEM file is saved as (YMODEM sends names).
    pub fn start_xy_download(&mut self, proto: XyProtocol, name: Option<String>) -> Result<()> {
        self.start_xy_download_to(proto, self.config.download_dir.clone(), name)
    }

    fn start_xy_download_to(&mut self, proto: XyProtocol, dir: PathBuf, name: Option<String>) -> Result<()> {
        if self.transfer.is_some() {
            anyhow::bail!("a transfer is already in progress");
        }
        let mut t = Transfer::XyDownload(XyReceive::new(proto, dir, name));
        let mut out = Vec::new();
        t.tick(Instant::now(), &mut out);
        self.transfer = Some(t);
        self.send(&out);
        self.events.push(SessionEvent::TransferStarted);
        Ok(())
    }

    /// Send files with Kermit to the BBS's Kermit, once it is receiving.
    pub fn start_kermit_upload(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        if self.transfer.is_some() {
            anyhow::bail!("a transfer is already in progress");
        }
        let mut t = Transfer::Kermit(Kermit::send(paths)?);
        let mut out = Vec::new();
        t.start(&mut out);
        self.upload_wait = None;
        self.transfer = Some(t);
        self.send(&out);
        self.events.push(SessionEvent::TransferStarted);
        Ok(())
    }

    /// Receive with Kermit: for a BBS whose Kermit is already sending (one
    /// that starts afterwards is picked up by itself).
    pub fn start_kermit_download(&mut self) -> Result<()> {
        if self.transfer.is_some() {
            anyhow::bail!("a transfer is already in progress");
        }
        self.transfer = Some(Transfer::Kermit(Kermit::receive(self.config.download_dir.clone())));
        self.events.push(SessionEvent::TransferStarted);
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
        self.pump();
        // Sound reports, and reading again after waiting for a sound.
        self.term.poll(Instant::now());
        let replies = self.term.take_replies();
        if !replies.is_empty() {
            self.send(&replies);
        }
        self.events.extend(self.term.take_events().into_iter().map(SessionEvent::Term));
        if let Some(t) = &mut self.transfer {
            let mut out = Vec::new();
            t.tick(Instant::now(), &mut out);
            self.send(&out);
            self.end_transfer(None);
        }
        self.check_login_prompt();
    }

    /// When [`Session::tick`] next has something to do for the terminal.
    pub fn next_tick(&self) -> Option<Instant> {
        self.term.next_poll()
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

    /// Whether a call that has ended is worth dialing again: the connection
    /// could not be made (refused, timed out), or the BBS hung up soon after
    /// answering, saying it is busy. Not for failed logins, changed host
    /// keys and the like.
    pub fn should_redial(&self) -> bool {
        if self.state != State::Closed {
            return false;
        }
        match self.connected_at {
            None => self.error.as_deref().is_some_and(|e| e.contains("connecting to")),
            Some(t) => t.elapsed() < BUSY_WINDOW && says_busy(&self.term.screen),
        }
    }

    /// Whether the call got through (it may have ended since).
    pub fn was_connected(&self) -> bool {
        self.connected_at.is_some()
    }

    /// Time since connecting (or since dialing, while still connecting).
    pub fn elapsed(&self) -> Duration {
        self.connected_at.unwrap_or(self.started).elapsed()
    }

    pub fn download_dir(&self) -> &Path {
        &self.config.download_dir
    }

    pub fn capture_path(&self) -> Option<&Path> {
        self.capture.as_ref().map(|c| c.path.as_path())
    }

    /// Start recording received data (after telnet decoding) to a file: all
    /// of it, or for a `.txt` file only the text (see [`CaptureKind`]).
    pub fn start_capture(&mut self, path: PathBuf) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let f = File::options()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let kind = CaptureKind::for_path(&path);
        self.capture = Some(Capture { path, file: BufWriter::new(f), kind, text: TextState::Ground });
        Ok(())
    }

    pub fn stop_capture(&mut self) {
        if let Some(mut c) = self.capture.take() {
            let _ = c.file.flush();
        }
    }
}

/// What a capture keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureKind {
    /// Everything the BBS sent, ANSI codes and all (`.ans`), to view later.
    Raw,
    /// Only the text, as UTF-8 (`.txt`): a log to read or search.
    Text,
}

impl CaptureKind {
    pub fn for_path(path: &Path) -> CaptureKind {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("log") => CaptureKind::Text,
            _ => CaptureKind::Raw,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextState {
    Ground,
    Escape,
    Csi,
    /// An OSC or other string, up to BEL or ST.
    String,
}

struct Capture {
    path: PathBuf,
    file: BufWriter<File>,
    kind: CaptureKind,
    text: TextState,
}

impl Capture {
    fn write(&mut self, data: &[u8], encoding: crate::ansi::Encoding) {
        if self.kind == CaptureKind::Raw {
            let _ = self.file.write_all(data);
            return;
        }
        let utf8 = encoding == crate::ansi::Encoding::Utf8;
        let mut out = Vec::with_capacity(data.len());
        for &b in data {
            self.text = match (self.text, b) {
                (_, 0x18 | 0x1A) => TextState::Ground,
                (TextState::Ground, 0x1B) => TextState::Escape,
                (TextState::Ground, b'\n' | b'\t') => {
                    out.push(b);
                    TextState::Ground
                }
                (TextState::Ground, 0x0C) => {
                    out.extend_from_slice(b"\n\n");
                    TextState::Ground
                }
                (TextState::Ground, 0x00..=0x1F | 0x7F) => TextState::Ground,
                (TextState::Ground, _) if utf8 || b < 0x80 => {
                    out.push(b);
                    TextState::Ground
                }
                (TextState::Ground, _) => {
                    let mut buf = [0; 4];
                    out.extend_from_slice(crate::cp437::to_unicode(b).encode_utf8(&mut buf).as_bytes());
                    TextState::Ground
                }
                (TextState::Escape, b'[') => TextState::Csi,
                (TextState::Escape, b']' | b'P' | b'X' | b'^' | b'_') => TextState::String,
                (TextState::Escape, 0x20..=0x2F) => TextState::Escape,
                (TextState::Escape, _) => TextState::Ground,
                (TextState::Csi, 0x40..=0x7E) => TextState::Ground,
                (TextState::Csi, _) => TextState::Csi,
                (TextState::String, 0x07) => TextState::Ground,
                (TextState::String, 0x1B) => TextState::Escape,
                (TextState::String, _) => TextState::String,
            };
        }
        let _ = self.file.write_all(&out);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.hang_up();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::Encoding;

    #[test]
    fn text_captures_keep_only_the_words() {
        let path = std::env::temp_dir().join(format!("badterm-capture-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(CaptureKind::for_path(&path), CaptureKind::Text);
        assert_eq!(CaptureKind::for_path(Path::new("x.ans")), CaptureKind::Raw);
        let f = File::create(&path).unwrap();
        let mut c =
            Capture { path: path.clone(), file: BufWriter::new(f), kind: CaptureKind::Text, text: TextState::Ground };
        // Colors, a title, CR LF and a CP437 box character, split mid-sequence.
        c.write(b"\x1b[1;3", Encoding::Cp437);
        c.write(b"1mHello\x1b]2;title\x07\r\n\xc4\xc4 bye\x07", Encoding::Cp437);
        c.file.flush().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Hello\n\u{2500}\u{2500} bye");
        let _ = std::fs::remove_file(&path);
    }
}
