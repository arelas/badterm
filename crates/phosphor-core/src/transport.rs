//! Network transports. Each connection runs on its own background thread
//! and reports [`NetEvent`]s through a callback; the caller writes through
//! the [`Writer`] handed over in [`NetEvent::Connected`].
//!
//! - Telnet and raw connections use a plain TCP socket.
//! - SSH runs the `russh` client on a single-threaded Tokio runtime, opening
//!   one interactive shell channel with a PTY. Host keys are trusted on first
//!   use and pinned in Phosphor's own `known_hosts` file; a changed key
//!   refuses the connection.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::keys::{HashAlg, PublicKey, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use tokio::sync::mpsc;

use crate::dialer::{Entry, Protocol};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Rounds of keyboard-interactive prompts answered before giving up.
const MAX_KBD_INTERACTIVE_ROUNDS: usize = 5;

/// Events produced by a connection thread.
#[derive(Debug)]
pub enum NetEvent {
    /// Connected; carries the handle used for writing.
    Connected(Writer),
    Data(Vec<u8>),
    /// Something worth telling the user, e.g. a newly trusted host key.
    Notice(String),
    /// The connection ended, with an error message if it failed.
    Closed(Option<String>),
}

/// Settings for establishing a connection.
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    pub cols: u16,
    pub rows: u16,
    /// Where SSH host keys are pinned.
    pub known_hosts: PathBuf,
}

enum SshCmd {
    Data(Vec<u8>),
    Close,
}

/// The sending side of a connection.
pub struct Writer(WriterKind);

enum WriterKind {
    Tcp(TcpStream),
    Ssh(mpsc::UnboundedSender<SshCmd>),
}

impl fmt::Debug for Writer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            WriterKind::Tcp(_) => "Writer(TCP)",
            WriterKind::Ssh(_) => "Writer(SSH)",
        })
    }
}

impl Writer {
    pub fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match &mut self.0 {
            WriterKind::Tcp(s) => s.write_all(data),
            WriterKind::Ssh(tx) => tx
                .send(SshCmd::Data(data.to_vec()))
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "SSH connection closed")),
        }
    }

    /// Close the connection; the reading thread then reports `Closed`.
    pub fn shutdown(&self) {
        match &self.0 {
            WriterKind::Tcp(s) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            WriterKind::Ssh(tx) => {
                let _ = tx.send(SshCmd::Close);
            }
        }
    }
}

/// Connect to `entry` in a background thread, reporting through `sink`.
///
/// `sink` is called from the background thread; the GUI uses it to forward
/// events into its event loop.
pub fn spawn_connection<F>(entry: &Entry, opts: &ConnectOptions, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    match entry.protocol {
        Protocol::Telnet | Protocol::Raw => spawn_tcp(entry.host.clone(), entry.port, sink),
        Protocol::Ssh => {
            let params = SshParams {
                host: entry.host.clone(),
                port: entry.port,
                user: entry.username.clone(),
                password: entry.password.clone(),
                term: if entry.term_type.is_empty() { "ansi".into() } else { entry.term_type.clone() },
                cols: opts.cols,
                rows: opts.rows,
                known_hosts: opts.known_hosts.clone(),
            };
            spawn_ssh(params, sink);
        }
        Protocol::Rlogin => sink(NetEvent::Closed(Some("RLogin connections are not supported yet".into()))),
    }
}

fn spawn_tcp<F>(host: String, port: u16, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    thread::spawn(move || {
        let mut stream = match connect_tcp(&host, port) {
            Ok(s) => s,
            Err(e) => return sink(NetEvent::Closed(Some(format!("{e:#}")))),
        };
        match stream.try_clone() {
            Ok(w) => sink(NetEvent::Connected(Writer(WriterKind::Tcp(w)))),
            Err(e) => return sink(NetEvent::Closed(Some(e.to_string()))),
        }
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return sink(NetEvent::Closed(None)),
                Ok(n) => sink(NetEvent::Data(buf[..n].to_vec())),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return sink(NetEvent::Closed(Some(e.to_string()))),
            }
        }
    });
}

fn connect_tcp(host: &str, port: u16) -> Result<TcpStream> {
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

struct SshParams {
    host: String,
    port: u16,
    user: String,
    password: String,
    term: String,
    cols: u16,
    rows: u16,
    known_hosts: PathBuf,
}

/// A thread-safe wrapper around the caller's sink, so it can be shared with
/// the SSH handler.
#[derive(Clone)]
struct Emitter(Arc<dyn Fn(NetEvent) + Send + Sync>);

impl Emitter {
    fn emit(&self, ev: NetEvent) {
        (self.0)(ev)
    }
}

fn spawn_ssh<F>(params: SshParams, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    thread::spawn(move || {
        let sink = Mutex::new(sink);
        let emitter = Emitter(Arc::new(move |ev| (sink.lock().unwrap())(ev)));
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => return emitter.emit(NetEvent::Closed(Some(format!("starting SSH runtime: {e}")))),
        };
        let result = rt.block_on(run_ssh(params, emitter.clone()));
        emitter.emit(NetEvent::Closed(result.err().map(|e| format!("{e:#}"))));
    });
}

struct HostKeyCheck {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    emitter: Emitter,
    /// Why the key was rejected, reported instead of russh's generic error.
    problem: Arc<Mutex<Option<String>>>,
}

impl client::Handler for HostKeyCheck {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::from(cert.public_key().clone()),
        };
        match verify_host_key(&self.host, self.port, &key, &self.known_hosts) {
            Ok(notice) => {
                if let Some(n) = notice {
                    self.emitter.emit(NetEvent::Notice(n));
                }
                Ok(true)
            }
            Err(msg) => {
                *self.problem.lock().unwrap() = Some(msg);
                Ok(false)
            }
        }
    }
}

/// Trust-on-first-use host key check. Returns a notice when a new key was
/// recorded, or an error message when the key does not match the pinned one.
pub fn verify_host_key(host: &str, port: u16, key: &PublicKey, known_hosts: &Path) -> Result<Option<String>, String> {
    let fingerprint = key.fingerprint(HashAlg::Sha256);
    match check_known_hosts_path(host, port, key, known_hosts) {
        Ok(true) => Ok(None),
        Ok(false) => {
            learn_known_hosts_path(host, port, key, known_hosts)
                .map_err(|e| format!("saving host key to {}: {e}", known_hosts.display()))?;
            Ok(Some(format!("Trusted new SSH host key for {host}: {fingerprint}")))
        }
        Err(russh::keys::Error::KeyChanged { line }) => Err(format!(
            "The SSH host key for {host} has CHANGED (now {fingerprint}). Someone may be intercepting the \
             connection. If the BBS really changed its key, delete line {line} of {}.",
            known_hosts.display()
        )),
        Err(e) => Err(format!("reading {}: {e}", known_hosts.display())),
    }
}

async fn run_ssh(p: SshParams, emitter: Emitter) -> Result<()> {
    if p.user.is_empty() {
        bail!("SSH needs a user name: edit the entry and fill in \"User name\"");
    }
    let config = Arc::new(client::Config { keepalive_interval: Some(Duration::from_secs(30)), ..Default::default() });
    let problem = Arc::new(Mutex::new(None));
    let handler = HostKeyCheck {
        host: p.host.clone(),
        port: p.port,
        known_hosts: p.known_hosts.clone(),
        emitter: emitter.clone(),
        problem: problem.clone(),
    };
    let connect = client::connect(config, (p.host.as_str(), p.port), handler);
    let mut session = match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Err(_) => bail!("connecting to {}:{}: timed out", p.host, p.port),
        Ok(Err(e)) => {
            if let Some(msg) = problem.lock().unwrap().take() {
                bail!(msg);
            }
            return Err(anyhow!(e)).with_context(|| format!("connecting to {}:{}", p.host, p.port));
        }
        Ok(Ok(s)) => s,
    };

    authenticate(&mut session, &p.user, &p.password).await?;

    let channel = session.channel_open_session().await.context("opening SSH channel")?;
    channel
        .request_pty(false, &p.term, p.cols.into(), p.rows.into(), 0, 0, &[])
        .await
        .context("requesting a terminal")?;
    channel.request_shell(false).await.context("starting the shell")?;

    let (tx, mut rx) = mpsc::unbounded_channel();
    emitter.emit(NetEvent::Connected(Writer(WriterKind::Ssh(tx))));

    let (mut read, write) = channel.split();
    loop {
        tokio::select! {
            msg = read.wait() => match msg {
                Some(ChannelMsg::Data { data }) => emitter.emit(NetEvent::Data(data.to_vec())),
                Some(ChannelMsg::ExtendedData { data, .. }) => emitter.emit(NetEvent::Data(data.to_vec())),
                Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
            cmd = rx.recv() => match cmd {
                Some(SshCmd::Data(d)) => write.data(&d[..]).await.context("sending data")?,
                Some(SshCmd::Close) | None => break,
            },
        }
    }
    let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
    Ok(())
}

/// Try "none" (common for BBSes that run their own login), then password,
/// then keyboard-interactive answering every prompt with the stored
/// password.
async fn authenticate<H: client::Handler>(session: &mut client::Handle<H>, user: &str, password: &str) -> Result<()> {
    if session.authenticate_none(user).await?.success() {
        return Ok(());
    }
    if !password.is_empty() && session.authenticate_password(user, password).await?.success() {
        return Ok(());
    }
    let mut resp = session.authenticate_keyboard_interactive_start(user, None).await?;
    for _ in 0..MAX_KBD_INTERACTIVE_ROUNDS {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(()),
            KeyboardInteractiveAuthResponse::Failure { .. } => break,
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = vec![password.to_string(); prompts.len()];
                resp = session.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    if password.is_empty() {
        bail!("SSH login as \"{user}\" was refused; the BBS may need a password set in the entry");
    }
    bail!("SSH login as \"{user}\" was refused: check the user name and password")
}
