//! A live connection to a BBS: networking, protocol decoding, terminal state
//! and session capture.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::ansi::{TermEvent, Terminal};
use crate::dialer::{Entry, Protocol};
use crate::telnet::Telnet;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Events produced by the network thread.
#[derive(Debug)]
pub enum NetEvent {
    /// Connected; carries the stream used for writing.
    Connected(TcpStream),
    Data(Vec<u8>),
    /// The connection ended, with an error message if it failed.
    Closed(Option<String>),
}

/// Resolve, connect and read in a background thread, reporting through `sink`.
///
/// `sink` is called from the background thread; the GUI uses it to forward
/// events into its event loop.
pub fn spawn_connection<F>(host: String, port: u16, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    thread::spawn(move || {
        let mut stream = match connect(&host, port) {
            Ok(s) => s,
            Err(e) => return sink(NetEvent::Closed(Some(format!("{e:#}")))),
        };
        match stream.try_clone() {
            Ok(w) => sink(NetEvent::Connected(w)),
            Err(e) => return sink(NetEvent::Closed(Some(e.to_string()))),
        }
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return sink(NetEvent::Closed(None)),
                Ok(n) => sink(NetEvent::Data(buf[..n].to_vec())),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return sink(NetEvent::Closed(Some(e.to_string()))),
            }
        }
    });
}

fn connect(host: &str, port: u16) -> Result<TcpStream> {
    let addrs: Vec<_> = (host, port).to_socket_addrs().with_context(|| format!("resolving {host}"))?.collect();
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        Some(e) => Err(e).with_context(|| format!("connecting to {host}:{port}")),
        None => bail!("{host} has no addresses"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Connecting,
    Connected,
    Closed,
}

pub struct Session {
    pub entry: Entry,
    pub term: Terminal,
    pub state: State,
    /// Why the session closed, if it failed.
    pub error: Option<String>,
    telnet: Option<Telnet>,
    writer: Option<TcpStream>,
    capture: Option<(PathBuf, BufWriter<File>)>,
    started: Instant,
    connected_at: Option<Instant>,
    events: Vec<TermEvent>,
}

impl Session {
    pub fn new(entry: Entry, cols: usize, rows: usize, scrollback: usize) -> Result<Session> {
        if !entry.protocol.supported() {
            bail!("{} connections are not supported yet", entry.protocol);
        }
        let mut term = Terminal::new(cols, rows, scrollback);
        term.encoding = entry.encoding;
        let telnet =
            (entry.protocol == Protocol::Telnet).then(|| Telnet::new(&entry.term_type, cols as u16, rows as u16));
        Ok(Session {
            entry,
            term,
            state: State::Connecting,
            error: None,
            telnet,
            writer: None,
            capture: None,
            started: Instant::now(),
            connected_at: None,
            events: Vec::new(),
        })
    }

    pub fn handle(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Connected(w) => {
                self.writer = Some(w);
                self.state = State::Connected;
                self.connected_at = Some(Instant::now());
            }
            NetEvent::Data(bytes) => self.receive(&bytes),
            NetEvent::Closed(err) => {
                self.state = State::Closed;
                self.error = err;
                self.writer = None;
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
        if let Some((_, f)) = &mut self.capture {
            let _ = f.write_all(&data);
        }
        self.term.feed(&data);
        let replies = self.term.take_replies();
        if !replies.is_empty() {
            self.send(&replies);
        }
        self.events.extend(self.term.take_events());
    }

    pub fn take_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    /// Send application data (keystrokes, paste) to the remote.
    pub fn send(&mut self, data: &[u8]) {
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
        if let Some(w) = self.writer.take() {
            let _ = w.shutdown(Shutdown::Both);
        }
        self.state = State::Closed;
        self.stop_capture();
    }

    /// Time since connecting (or since dialing, while still connecting).
    pub fn elapsed(&self) -> Duration {
        self.connected_at.unwrap_or(self.started).elapsed()
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
