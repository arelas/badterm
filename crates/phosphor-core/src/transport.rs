//! Network transports. Each connection runs on its own background thread
//! and reports [`NetEvent`]s through a callback; the caller writes through
//! the [`Writer`] handed over in [`NetEvent::Connected`].
//!
//! - Telnet, raw and RLogin connections use a plain TCP socket; RLogin
//!   first sends its login record (see [`rlogin_handshake`]).
//! - Telnet over TLS: see [`crate::tls`].
//! - SSH runs the `russh` client on a single-threaded Tokio runtime, opening
//!   one interactive shell channel with a PTY. Host keys are trusted on first
//!   use and pinned in Phosphor's own `known_hosts` file; a changed key
//!   refuses the connection. Login tries "none", then public keys (the
//!   entry's key file, the SSH agent, the usual `~/.ssh/id_*` files), then
//!   keyboard-interactive or password, asking the user through
//!   [`NetEvent::Prompt`] for anything not stored.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use russh::MethodKind;
use russh::client::{self, AuthResult, KeyboardInteractiveAuthResponse};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use tokio::sync::{mpsc, oneshot};

use crate::dialer::{Entry, Protocol};

pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Rounds of keyboard-interactive prompts answered before giving up.
const MAX_KBD_INTERACTIVE_ROUNDS: usize = 5;
/// Times the user is asked for a password (or passphrase) before giving up.
const MAX_PROMPTED_ATTEMPTS: usize = 3;
/// Key files tried from the SSH directory, in OpenSSH's order.
const DEFAULT_KEY_FILES: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];
const LOGIN_CANCELLED: &str = "Login cancelled";

/// Events produced by a connection thread.
#[derive(Debug)]
pub enum NetEvent {
    /// Connected; carries the handle used for writing.
    Connected(Writer),
    Data(Vec<u8>),
    /// Something worth telling the user, e.g. a newly trusted host key.
    Notice(String),
    /// The login needs an answer from the user.
    Prompt(AuthPrompt),
    /// The connection ended, with an error message if it failed.
    Closed(Option<String>),
}

/// What an [`AuthPrompt`] asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// The account password (worth offering to remember).
    Password,
    /// The passphrase of a private key file.
    Passphrase,
    /// A question from the server's keyboard-interactive login, such as a
    /// one-time code.
    Interactive,
}

/// A question the SSH login needs the user to answer. Answer it with
/// [`AuthPrompt::answer`]; dropping it unanswered cancels the login.
pub struct AuthPrompt {
    pub kind: PromptKind,
    /// A heading, e.g. "SSH login to bbs.example.com".
    pub title: String,
    /// Extra text from the server, or why the question is asked again.
    pub instructions: String,
    pub prompt: String,
    /// Whether the answer may be shown as it is typed.
    pub echo: bool,
    reply: oneshot::Sender<Option<String>>,
}

impl AuthPrompt {
    /// Reply with the answer, or `None` to cancel the login.
    pub fn answer(self, answer: Option<String>) {
        let _ = self.reply.send(answer);
    }
}

impl fmt::Debug for AuthPrompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthPrompt")
            .field("kind", &self.kind)
            .field("title", &self.title)
            .field("instructions", &self.instructions)
            .field("prompt", &self.prompt)
            .field("echo", &self.echo)
            .finish()
    }
}

/// Settings for establishing a connection.
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    pub cols: u16,
    pub rows: u16,
    /// Where SSH host keys are pinned.
    pub known_hosts: PathBuf,
    /// Whether to offer the SSH agent's keys.
    pub ssh_agent: bool,
    /// Where to look for default SSH keys (usually `~/.ssh`).
    pub ssh_key_dir: Option<PathBuf>,
}

/// What a [`Writer`] asks a connection task (SSH or TLS) to do.
pub(crate) enum Cmd {
    Data(Vec<u8>),
    Close,
}

/// The sending side of a connection.
pub struct Writer(pub(crate) WriterKind);

pub(crate) enum WriterKind {
    Tcp(TcpStream),
    /// To a task that owns the connection (SSH, TLS).
    Channel(mpsc::UnboundedSender<Cmd>),
}

impl fmt::Debug for Writer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            WriterKind::Tcp(_) => "Writer(TCP)",
            WriterKind::Channel(_) => "Writer(channel)",
        })
    }
}

impl Writer {
    pub fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match &mut self.0 {
            WriterKind::Tcp(s) => s.write_all(data),
            WriterKind::Channel(tx) => tx
                .send(Cmd::Data(data.to_vec()))
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "connection closed")),
        }
    }

    /// Close the connection; the reading thread then reports `Closed`.
    pub fn shutdown(&self) {
        match &self.0 {
            WriterKind::Tcp(s) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            WriterKind::Channel(tx) => {
                let _ = tx.send(Cmd::Close);
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
        Protocol::Telnet | Protocol::Raw => spawn_tcp(entry.host.clone(), entry.port, None, sink),
        Protocol::Rlogin => spawn_tcp(entry.host.clone(), entry.port, Some(rlogin_handshake(entry)), sink),
        Protocol::TelnetS => {
            crate::tls::spawn_tls(entry.host.clone(), entry.port, tls_pins_path(&opts.known_hosts), sink)
        }
        Protocol::Ssh => {
            let params = SshParams {
                host: entry.host.clone(),
                port: entry.port,
                user: entry.username.clone(),
                password: entry.password.clone(),
                key_file: (!entry.key_file.trim().is_empty()).then(|| expand_home(entry.key_file.trim())),
                agent: opts.ssh_agent,
                key_dir: opts.ssh_key_dir.clone(),
                term: if entry.term_type.is_empty() { "ansi".into() } else { entry.term_type.clone() },
                cols: opts.cols,
                rows: opts.rows,
                known_hosts: opts.known_hosts.clone(),
            };
            spawn_ssh(params, sink);
        }
    }
}

/// Where TLS certificates are pinned: beside SSH's `known_hosts`.
pub fn tls_pins_path(known_hosts: &Path) -> PathBuf {
    known_hosts.with_file_name("tls_known_hosts")
}

/// RLogin's opening record (RFC 1282): client user name, server user name
/// and terminal type/speed, each ending in a NUL, after a NUL. As SyncTERM
/// does, BBSes get the password as the client user name and the user name
/// as the server user name, so they can log the caller straight in.
pub fn rlogin_handshake(entry: &Entry) -> Vec<u8> {
    let term = if entry.term_type.is_empty() { "ansi-bbs".to_string() } else { entry.term_type.to_ascii_lowercase() };
    let mut out = vec![0];
    for field in [entry.password.as_str(), entry.username.as_str(), &format!("{term}/115200")] {
        out.extend_from_slice(field.as_bytes());
        out.push(0);
    }
    out
}

/// A TCP connection, sending `handshake` first (RLogin), in which case the
/// server's one-byte NUL reply is not passed on.
fn spawn_tcp<F>(host: String, port: u16, handshake: Option<Vec<u8>>, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    thread::spawn(move || {
        let mut stream = match connect_tcp(&host, port) {
            Ok(s) => s,
            Err(e) => return sink(NetEvent::Closed(Some(format!("{e:#}")))),
        };
        let mut expect_ack = false;
        if let Some(h) = &handshake {
            if let Err(e) = stream.write_all(h) {
                return sink(NetEvent::Closed(Some(format!("sending the RLogin login: {e}"))));
            }
            expect_ack = true;
        }
        match stream.try_clone() {
            Ok(w) => sink(NetEvent::Connected(Writer(WriterKind::Tcp(w)))),
            Err(e) => return sink(NetEvent::Closed(Some(e.to_string()))),
        }
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return sink(NetEvent::Closed(None)),
                Ok(n) => {
                    let mut data = &buf[..n];
                    if expect_ack {
                        expect_ack = false;
                        data = data.strip_prefix(&[0]).unwrap_or(data);
                    }
                    if !data.is_empty() {
                        sink(NetEvent::Data(data.to_vec()));
                    }
                }
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
    key_file: Option<PathBuf>,
    agent: bool,
    key_dir: Option<PathBuf>,
    term: String,
    cols: u16,
    rows: u16,
    known_hosts: PathBuf,
}

/// A thread-safe wrapper around the caller's sink, so it can be shared with
/// the SSH handler.
#[derive(Clone)]
pub(crate) struct Emitter(Arc<dyn Fn(NetEvent) + Send + Sync>);

impl Emitter {
    /// Wrap a sink that is only `Send`, for sharing between tasks.
    pub(crate) fn new<F: Fn(NetEvent) + Send + 'static>(sink: F) -> Emitter {
        let sink = Mutex::new(sink);
        Emitter(Arc::new(move |ev| (sink.lock().unwrap_or_else(|e| e.into_inner()))(ev)))
    }

    pub(crate) fn emit(&self, ev: NetEvent) {
        (self.0)(ev)
    }
}

fn spawn_ssh<F>(params: SshParams, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    thread::spawn(move || {
        let emitter = Emitter::new(sink);
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

    Login { session: &mut session, p: &p, emitter: &emitter }.run().await?;

    let channel = session.channel_open_session().await.context("opening SSH channel")?;
    channel
        .request_pty(false, &p.term, p.cols.into(), p.rows.into(), 0, 0, &[])
        .await
        .context("requesting a terminal")?;
    channel.request_shell(false).await.context("starting the shell")?;

    let (tx, mut rx) = mpsc::unbounded_channel();
    emitter.emit(NetEvent::Connected(Writer(WriterKind::Channel(tx))));

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
                Some(Cmd::Data(d)) => write.data(&d[..]).await.context("sending data")?,
                Some(Cmd::Close) | None => break,
            },
        }
    }
    let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
    Ok(())
}

/// Expand a leading `~/` to the home directory.
fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(p),
    }
}

/// How a login attempt went.
enum Attempt {
    Success,
    /// Rejected; the server lists the methods still worth trying.
    Failed(Vec<MethodKind>),
}

impl From<AuthResult> for Attempt {
    fn from(r: AuthResult) -> Attempt {
        match r {
            AuthResult::Success => Attempt::Success,
            AuthResult::Failure { remaining_methods, .. } => Attempt::Failed(remaining_methods.to_vec()),
        }
    }
}

/// The SSH login sequence.
struct Login<'a, H: client::Handler> {
    session: &'a mut client::Handle<H>,
    p: &'a SshParams,
    emitter: &'a Emitter,
}

impl<H: client::Handler> Login<'_, H> {
    /// Try "none" (common for BBSes that run their own login), then public
    /// keys, then keyboard-interactive or password, prompting for whatever
    /// is not stored.
    async fn run(&mut self) -> Result<()> {
        let user = self.p.user.clone();
        let mut methods = match self.session.authenticate_none(&user).await?.into() {
            Attempt::Success => return Ok(()),
            Attempt::Failed(m) => m,
        };
        if methods.contains(&MethodKind::PublicKey) {
            match self.public_keys().await? {
                Some(Attempt::Success) => return Ok(()),
                Some(Attempt::Failed(m)) => methods = m,
                None => {}
            }
        }
        // Like OpenSSH, keyboard-interactive first: servers that offer it
        // (PAM, one-time codes) often refuse plain password logins. If it
        // already asked the user, the password method would only ask again.
        let mut asked = false;
        if methods.contains(&MethodKind::KeyboardInteractive) {
            let (attempt, a) = self.keyboard_interactive().await?;
            asked = a;
            match attempt {
                Attempt::Success => return Ok(()),
                Attempt::Failed(m) => methods = m,
            }
        }
        if !asked && methods.contains(&MethodKind::Password) {
            match self.password().await? {
                Attempt::Success => return Ok(()),
                Attempt::Failed(m) => methods = m,
            }
        }
        let offered: Vec<&str> = methods.iter().map(<&str>::from).collect();
        log::debug!("SSH login refused; server still offers {offered:?}");
        bail!("SSH login as \"{user}\" was refused: check the user name and password")
    }

    async fn ask(&self, kind: PromptKind, instructions: String, prompt: String, echo: bool) -> Option<String> {
        let (reply, answer) = oneshot::channel();
        let title = format!("SSH login to {}", self.p.host);
        self.emitter.emit(NetEvent::Prompt(AuthPrompt { kind, title, instructions, prompt, echo, reply }));
        answer.await.ok().flatten()
    }

    async fn ask_password(&self, retry: bool) -> Result<String> {
        let why = if retry { "That password was not accepted." } else { "" };
        let prompt = format!("Password for {}:", self.p.user);
        self.ask(PromptKind::Password, why.into(), prompt, false).await.context(LOGIN_CANCELLED)
    }

    /// The stored password once, then up to [`MAX_PROMPTED_ATTEMPTS`]
    /// typed ones.
    async fn password(&mut self) -> Result<Attempt> {
        let mut stored = (!self.p.password.is_empty()).then(|| self.p.password.clone());
        let (mut retry, mut asked) = (false, 0);
        let mut last = Attempt::Failed(vec![MethodKind::Password]);
        while asked < MAX_PROMPTED_ATTEMPTS {
            let pw = match stored.take() {
                Some(pw) => pw,
                None => {
                    asked += 1;
                    self.ask_password(retry).await?
                }
            };
            last = self.session.authenticate_password(&self.p.user, pw).await?.into();
            match &last {
                Attempt::Failed(m) if m.contains(&MethodKind::Password) => retry = true,
                _ => break,
            }
        }
        Ok(last)
    }

    /// Answer the server's questions. A lone hidden question is taken to be
    /// the password, so a stored one answers it the first time. Also returns
    /// whether the user was asked anything.
    async fn keyboard_interactive(&mut self) -> Result<(Attempt, bool)> {
        let user = self.p.user.clone();
        let mut stored = (!self.p.password.is_empty()).then(|| self.p.password.clone());
        let (mut retry, mut asked) = (false, false);
        let mut last = Attempt::Failed(vec![MethodKind::KeyboardInteractive]);
        // A stored password gets one try ahead of the typed ones.
        let attempts = MAX_PROMPTED_ATTEMPTS + usize::from(stored.is_some());
        'attempts: for _ in 0..attempts {
            let mut resp = self.session.authenticate_keyboard_interactive_start(&user, None).await?;
            for _ in 0..MAX_KBD_INTERACTIVE_ROUNDS {
                let (name, instructions, prompts) = match resp {
                    KeyboardInteractiveAuthResponse::Success => return Ok((Attempt::Success, asked)),
                    KeyboardInteractiveAuthResponse::Failure { remaining_methods, .. } => {
                        last = Attempt::Failed(remaining_methods.to_vec());
                        // Retry only after a wrong answer: one the user typed,
                        // or the stored password (the user is asked next time).
                        let used_stored = stored.is_none() && !self.p.password.is_empty();
                        if !remaining_methods.contains(&MethodKind::KeyboardInteractive) || !asked && !used_stored {
                            break 'attempts;
                        }
                        retry = true;
                        continue 'attempts;
                    }
                    KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => {
                        (name, instructions, prompts)
                    }
                };
                let mut answers = Vec::with_capacity(prompts.len());
                if let ([only], Some(pw)) = (prompts.as_slice(), stored.as_ref())
                    && !only.echo
                {
                    answers.push(pw.clone());
                    stored = None;
                } else {
                    let mut info = [name.trim(), instructions.trim()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if retry && info.is_empty() {
                        info = "That answer was not accepted.".into();
                    }
                    for q in &prompts {
                        let kind = if q.echo { PromptKind::Interactive } else { PromptKind::Password };
                        let text = if q.prompt.trim().is_empty() { "Password:" } else { q.prompt.trim() };
                        asked = true;
                        let answer = self.ask(kind, info.clone(), text.into(), q.echo).await;
                        answers.push(answer.context(LOGIN_CANCELLED)?);
                    }
                }
                resp = self.session.authenticate_keyboard_interactive_respond(answers).await?;
            }
            break;
        }
        Ok((last, asked))
    }

    /// The entry's key file, then the agent's keys, then the default key
    /// files. Returns None if no key was offered at all.
    async fn public_keys(&mut self) -> Result<Option<Attempt>> {
        let rsa_hash = self.session.best_supported_rsa_hash().await.ok().flatten().flatten();
        let mut offered: Vec<PublicKey> = Vec::new();
        let mut last = None;

        if let Some(path) = self.p.key_file.clone()
            && let Some(key) = self.load_key(&path, true).await?
        {
            offered.push(key.public_key().clone());
            let r = self.offer_key(key, rsa_hash).await?;
            if matches!(r, Attempt::Success) {
                return Ok(Some(r));
            }
            last = Some(r);
        }

        if self.p.agent
            && let Some(mut agent) = connect_agent().await
        {
            let identities = agent.request_identities().await.unwrap_or_else(|e| {
                log::debug!("listing SSH agent keys: {e}");
                Vec::new()
            });
            for id in identities {
                let AgentIdentity::PublicKey { key, .. } = id else { continue };
                if offered.contains(&key) {
                    continue;
                }
                offered.push(key.clone());
                let hash = if key.algorithm().is_rsa() { rsa_hash } else { None };
                match self.session.authenticate_publickey_with(&self.p.user, key, hash, &mut agent).await {
                    Ok(r) => match Attempt::from(r) {
                        Attempt::Success => return Ok(Some(Attempt::Success)),
                        failed => last = Some(failed),
                    },
                    Err(e) => log::debug!("SSH agent signing failed: {e}"),
                }
            }
        }

        if let Some(dir) = self.p.key_dir.clone() {
            for name in DEFAULT_KEY_FILES {
                let path = dir.join(name);
                if Some(&path) == self.p.key_file.as_ref() || !path.exists() {
                    continue;
                }
                // Encrypted default keys are skipped rather than asking for
                // their passphrase on every BBS that offers public keys;
                // load them into the agent, or name one in the entry.
                let Some(key) = self.load_key(&path, false).await? else { continue };
                if offered.contains(key.public_key()) {
                    continue;
                }
                offered.push(key.public_key().clone());
                match self.offer_key(key, rsa_hash).await? {
                    Attempt::Success => return Ok(Some(Attempt::Success)),
                    failed => last = Some(failed),
                }
            }
        }
        Ok(last)
    }

    async fn offer_key(&mut self, key: PrivateKey, rsa_hash: Option<HashAlg>) -> Result<Attempt> {
        let key = PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
        Ok(self.session.authenticate_publickey(&self.p.user, key).await?.into())
    }

    /// Load a private key. For the entry's own key (`ask`), an encrypted key
    /// prompts for its passphrase and problems are reported; otherwise they
    /// only skip the key.
    async fn load_key(&self, path: &Path, ask: bool) -> Result<Option<PrivateKey>> {
        let shown = path.display().to_string();
        let mut why = String::new();
        let mut passphrase: Option<String> = None;
        for _ in 0..=MAX_PROMPTED_ATTEMPTS {
            match russh::keys::load_secret_key(path, passphrase.as_deref()) {
                Ok(key) => return Ok(Some(key)),
                Err(e) if !ask => {
                    log::debug!("skipping SSH key {shown}: {e}");
                    return Ok(None);
                }
                Err(russh::keys::Error::KeyIsEncrypted) => {}
                // A wrong passphrase shows up as a decryption error.
                Err(russh::keys::Error::SshKey(_)) if passphrase.is_some() => {
                    why = "That passphrase was not accepted.".into();
                }
                Err(e) => {
                    self.emitter.emit(NetEvent::Notice(format!("Cannot use SSH key {shown}: {e}")));
                    return Ok(None);
                }
            }
            let prompt = format!("Passphrase for {shown}:");
            match self.ask(PromptKind::Passphrase, why.clone(), prompt, false).await {
                Some(p) => passphrase = Some(p),
                // Skip the key; other methods may still work.
                None => return Ok(None),
            }
        }
        Ok(None)
    }
}

/// Connect to the user's SSH agent, if one is running.
async fn connect_agent() -> Option<AgentClient<Box<dyn AgentStream + Send + Unpin>>> {
    #[cfg(unix)]
    {
        match AgentClient::connect_env().await {
            Ok(a) => Some(a.dynamic()),
            Err(e) => {
                log::debug!("no SSH agent: {e}");
                None
            }
        }
    }
    #[cfg(windows)]
    {
        // The Windows OpenSSH agent service, then PuTTY's Pageant.
        if let Ok(a) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
            return Some(a.dynamic());
        }
        match AgentClient::connect_pageant().await {
            Ok(a) => Some(a.dynamic()),
            Err(e) => {
                log::debug!("no SSH agent: {e}");
                None
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    None
}
