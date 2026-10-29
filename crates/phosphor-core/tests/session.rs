//! End-to-end tests against fake BBSes on the loopback interface: telnet
//! negotiation, ZMODEM in both directions, and SSH.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use phosphor_core::session::{NetEvent, Session, SessionConfig, SessionEvent, State};
use phosphor_core::telnet::{DO, IAC, SB, SE, Telnet, WILL, opt};
use phosphor_core::transfer::{Download, Outcome, Transfer, Upload};
use phosphor_core::{Entry, Protocol, Screen};

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("phosphor-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(name: &str) -> SessionConfig {
    let dir = temp_dir(name);
    SessionConfig { scrollback: 100, known_hosts: dir.join("known_hosts"), download_dir: dir.join("downloads") }
}

/// Connect a session and return it with the channel its events arrive on.
fn dial(entry: Entry, cfg: SessionConfig) -> (Session, mpsc::Receiver<NetEvent>) {
    let session = Session::new(entry, 80, 25, cfg).unwrap();
    let (tx, rx) = mpsc::channel();
    session.connect(move |ev| {
        let _ = tx.send(ev);
    });
    (session, rx)
}

/// Pump network events into the session until `done` holds or time runs out.
fn pump_until(
    session: &mut Session,
    rx: &mpsc::Receiver<NetEvent>,
    events: &mut Vec<SessionEvent>,
    mut done: impl FnMut(&mut Session, &[SessionEvent]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(20)) {
            session.handle(ev);
        }
        session.tick();
        events.extend(session.take_events());
        if done(session, events) {
            return;
        }
    }
    panic!("timed out; screen line 0: {:?}, events: {events:?}", Screen::row_text(session.term.screen.row(0)));
}

fn row(session: &Session, y: usize) -> String {
    Screen::row_text(session.term.screen.row(y))
}

fn telnet_entry(port: u16) -> Entry {
    Entry { host: "127.0.0.1".into(), port, ..Entry::default() }
}

#[test]
fn telnet_session_round_trip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        sock.write_all(&[IAC, DO, opt::TTYPE]).unwrap();
        sock.write_all(&[IAC, SB, opt::TTYPE, 1, IAC, SE]).unwrap();
        sock.write_all(&[IAC, DO, opt::NAWS, IAC, WILL, opt::ECHO]).unwrap();
        sock.write_all(b"\x1b[2J\x1b[1;37;44mWelcome\x1b[0m\r\n\x1b[6n").unwrap();
        // Collect what the client sends until we have its CPR and keystroke.
        let mut got: Vec<u8> = Vec::new();
        let mut buf = [0u8; 256];
        while !(got.contains(&b'x') && got.windows(6).any(|w| w == b"\x1b[2;1R")) {
            let n = sock.read(&mut buf).unwrap();
            assert!(n > 0, "client closed early; got {got:?}");
            got.extend_from_slice(&buf[..n]);
        }
        got
    });

    let (mut session, rx) = dial(telnet_entry(port), config("telnet"));
    let mut sent_key = false;
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| {
        if !sent_key && row(s, 0) == "Welcome" {
            s.send(b"x");
            sent_key = true;
        }
        server.is_finished()
    });

    let got = server.join().unwrap();
    let contains = |needle: &[u8]| got.windows(needle.len()).any(|w| w == needle);
    let mut ttype = vec![IAC, SB, opt::TTYPE, 0];
    ttype.extend_from_slice(b"ANSI");
    ttype.extend_from_slice(&[IAC, SE]);
    assert!(contains(&ttype), "no TTYPE IS reply in {got:?}");
    assert!(contains(&[IAC, SB, opt::NAWS, 0, 80, 0, 25, IAC, SE]), "no NAWS in {got:?}");
    assert!(contains(&[IAC, DO, opt::ECHO]), "ECHO not accepted in {got:?}");
    assert!(contains(b"\x1b[2;1R"), "no cursor position report in {got:?}");
    session.hang_up();
    assert_eq!(session.state, State::Closed);
}

#[test]
fn connection_refused_reports_error() {
    // Bind then drop to get a port that is very likely closed.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (_session, rx) = dial(telnet_entry(port), config("refused"));
    match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
        NetEvent::Closed(Some(msg)) => assert!(msg.contains("connecting"), "{msg}"),
        other => panic!("expected failure, got {other:?}"),
    }
}

/// Run a ZMODEM transfer on the BBS side of a telnet connection: decode
/// what the client sends, encode what the transfer produces.
fn bbs_transfer(sock: &mut TcpStream, mut transfer: Transfer) -> Outcome {
    let mut telnet = Telnet::new("ANSI", 80, 25);
    sock.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    let mut buf = vec![0u8; 16 * 1024];
    let mut out = Vec::new();
    transfer.feed(&[], &mut out);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !transfer.is_finished() && Instant::now() < deadline {
        if !out.is_empty() {
            sock.write_all(&telnet.encode(&out)).unwrap();
            out.clear();
        }
        match sock.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let (mut data, mut reply) = (Vec::new(), Vec::new());
                telnet.receive(&buf[..n], &mut data, &mut reply);
                transfer.feed(&data, &mut out);
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                transfer.tick(Instant::now(), &mut out);
            }
            Err(e) => panic!("server read: {e}"),
        }
    }
    if !out.is_empty() {
        sock.write_all(&telnet.encode(&out)).unwrap();
    }
    transfer.finish().0
}

fn test_file(dir: &std::path::Path, name: &str, len: usize) -> (PathBuf, Vec<u8>) {
    // Pseudo-random bytes, so IAC (0xFF), CR and ZMODEM control bytes all occur.
    let data: Vec<u8> = (0..len as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8).collect();
    let path = dir.join(name);
    std::fs::write(&path, &data).unwrap();
    (path, data)
}

#[test]
fn zmodem_download_over_telnet() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let src = temp_dir("zdl-src");
    let (file, contents) = test_file(&src, "ARTPACK.ZIP", 200_000);

    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(b"Starting download\r\n").unwrap();
        let outcome = bbs_transfer(&mut sock, Transfer::Upload(Upload::new(vec![file]).unwrap()));
        sock.write_all(b"\r\nBack at the menu").unwrap();
        thread::sleep(Duration::from_millis(500));
        outcome
    });

    let cfg = config("zdl");
    let downloads = cfg.download_dir.clone();
    let (mut session, rx) = dial(telnet_entry(port), cfg);
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| {
        s.transfer().is_none() && (1..4).any(|y| row(s, y).contains("menu"))
    });

    assert!(matches!(server.join().unwrap(), Outcome::Completed(_)));
    assert!(events.contains(&SessionEvent::TransferStarted), "{events:?}");
    let saved = downloads.join("ARTPACK.ZIP");
    assert!(events.contains(&SessionEvent::TransferFinished(Outcome::Completed(vec![saved.clone()]))), "{events:?}");
    assert_eq!(std::fs::read(&saved).unwrap(), contents);
    // Text before the transfer is shown and no protocol bytes leak onto the screen.
    let rows: Vec<String> = (0..4).map(|y| row(&session, y)).collect();
    assert_eq!(rows, ["Starting download", "", "Back at the menu", ""]);
}

#[test]
fn zmodem_upload_requested_by_bbs() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bbs_dir = temp_dir("zul-bbs");
    let local = temp_dir("zul-local");
    let (file, contents) = test_file(&local, "upload.txt", 70_000);

    let dir = bbs_dir.clone();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(b"Begin your upload\r\n").unwrap();
        bbs_transfer(&mut sock, Transfer::Download(Download::new(dir).unwrap()))
    });

    let (mut session, rx) = dial(telnet_entry(port), config("zul"));
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |_, ev| ev.contains(&SessionEvent::UploadRequested));
    assert!(session.upload_requested());
    session.start_upload(vec![file.clone()]).unwrap();
    pump_until(&mut session, &rx, &mut events, |_, ev| {
        ev.iter().any(|e| matches!(e, SessionEvent::TransferFinished(_)))
    });

    assert!(events.contains(&SessionEvent::TransferFinished(Outcome::Completed(vec![file]))), "{events:?}");
    assert_eq!(server.join().unwrap(), Outcome::Completed(vec![bbs_dir.join("upload.txt")]));
    assert_eq!(std::fs::read(bbs_dir.join("upload.txt")).unwrap(), contents);
    assert_eq!(row(&session, 0), "Begin your upload");
}

mod ssh {
    use std::sync::Arc;

    use russh::keys::PrivateKey;
    use russh::keys::ssh_key::private::Ed25519Keypair;
    use russh::server::{self, Auth, Msg, Server as _, Session as ServerSession};
    use russh::{Channel, ChannelId};

    use super::*;

    #[derive(Clone)]
    struct Bbs;

    impl server::Server for Bbs {
        type Handler = Self;
        fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
            Bbs
        }
    }

    impl server::Handler for Bbs {
        type Error = russh::Error;

        async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
            if user == "sysop" && password == "secret" { Ok(Auth::Accept) } else { Ok(Auth::reject()) }
        }

        async fn channel_open_session(
            &mut self,
            _: Channel<Msg>,
            reply: server::ChannelOpenHandle,
            _: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            Ok(())
        }

        #[allow(clippy::too_many_arguments)]
        async fn pty_request(
            &mut self,
            channel: ChannelId,
            term: &str,
            cols: u32,
            rows: u32,
            _: u32,
            _: u32,
            _: &[(russh::Pty, u32)],
            session: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            session.data(channel, format!("PTY {term} {cols}x{rows}\r\n").into_bytes())?;
            Ok(())
        }

        async fn shell_request(&mut self, channel: ChannelId, session: &mut ServerSession) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            session.data(channel, b"\x1b[1;33mWelcome to SSH BBS\x1b[0m\r\n".to_vec())?;
            Ok(())
        }

        async fn data(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut ServerSession,
        ) -> Result<(), Self::Error> {
            let reply = format!("you typed {}", String::from_utf8_lossy(data));
            session.data(channel, reply.into_bytes())?;
            Ok(())
        }
    }

    /// Start an SSH BBS whose host key is derived from `seed`.
    fn start_server(seed: u8) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let key = PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]));
                let config = Arc::new(server::Config {
                    keys: vec![key],
                    auth_rejection_time: Duration::from_millis(10),
                    auth_rejection_time_initial: Some(Duration::ZERO),
                    ..Default::default()
                });
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let _ = Bbs.run_on_socket(config, &listener).await;
            });
        });
        port
    }

    fn ssh_entry(port: u16, password: &str) -> Entry {
        Entry {
            host: "127.0.0.1".into(),
            port,
            protocol: Protocol::Ssh,
            username: "sysop".into(),
            password: password.into(),
            ..Entry::default()
        }
    }

    fn closed_error(session: &mut Session, rx: &mpsc::Receiver<NetEvent>) -> String {
        let mut events = Vec::new();
        pump_until(session, rx, &mut events, |s, _| s.state == State::Closed);
        session.error.clone().unwrap_or_default()
    }

    #[test]
    fn login_shell_and_host_key_pinning() {
        let port = start_server(1);
        let cfg = config("ssh");
        let known_hosts = cfg.known_hosts.clone();

        let (mut session, rx) = dial(ssh_entry(port, "secret"), cfg.clone());
        let mut events = Vec::new();
        pump_until(&mut session, &rx, &mut events, |s, _| row(s, 1) == "Welcome to SSH BBS");
        assert_eq!(session.state, State::Connected);
        assert_eq!(row(&session, 0), "PTY ANSI 80x25");
        assert!(events.iter().any(|e| matches!(e, SessionEvent::Notice(n) if n.contains("Trusted new SSH host key"))));
        let pinned = std::fs::read_to_string(&known_hosts).unwrap();
        assert!(pinned.contains(&format!("[127.0.0.1]:{port} ssh-ed25519 ")), "{pinned}");

        session.send(b"x");
        pump_until(&mut session, &rx, &mut events, |s, _| row(s, 2) == "you typed x");
        session.hang_up();

        // Reconnecting to the same key is silent.
        let (mut session, rx) = dial(ssh_entry(port, "secret"), cfg.clone());
        let mut events = Vec::new();
        pump_until(&mut session, &rx, &mut events, |s, _| row(s, 1) == "Welcome to SSH BBS");
        assert!(!events.iter().any(|e| matches!(e, SessionEvent::Notice(_))), "{events:?}");
        session.hang_up();

        // A different key on the same host:port is refused.
        let impostor = start_server(2);
        let moved = pinned.replace(&format!("]:{port} "), &format!("]:{impostor} "));
        std::fs::write(&known_hosts, moved).unwrap();
        let (mut session, rx) = dial(ssh_entry(impostor, "secret"), cfg);
        let err = closed_error(&mut session, &rx);
        assert!(err.contains("has CHANGED"), "{err}");
    }

    #[test]
    fn wrong_password_is_refused() {
        let port = start_server(3);
        let (mut session, rx) = dial(ssh_entry(port, "wrong"), config("ssh-bad"));
        let err = closed_error(&mut session, &rx);
        assert!(err.contains("was refused"), "{err}");
    }

    #[test]
    fn missing_user_name_is_explained() {
        let port = start_server(4);
        let entry = Entry { username: String::new(), ..ssh_entry(port, "") };
        let (mut session, rx) = dial(entry, config("ssh-nouser"));
        let err = closed_error(&mut session, &rx);
        assert!(err.contains("needs a user name"), "{err}");
    }
}
