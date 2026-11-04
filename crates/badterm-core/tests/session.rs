//! End-to-end tests against fake BBSes on the loopback interface: telnet
//! negotiation, ZMODEM in both directions, and SSH.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use badterm_core::session::{NetEvent, Session, SessionEvent, State};
use badterm_core::telnet::{DO, IAC, SB, SE, Telnet, WILL, opt};
use badterm_core::transfer::{Download, Outcome, Transfer, Upload};
use badterm_core::{Entry, Protocol};

mod common;
use common::*;

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
fn logs_in_to_a_c64_bbs() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        // Clear the screen, then the prompt in PETSCII's capitals.
        sock.write_all(b"\x93\x05WELCOME\x0dLOGIN: ").unwrap();
        let mut got: Vec<u8> = Vec::new();
        let mut buf = [0u8; 256];
        while !got.contains(&0x0D) {
            let n = sock.read(&mut buf).unwrap();
            assert!(n > 0, "client closed early; got {got:?}");
            got.extend_from_slice(&buf[..n]);
        }
        got
    });
    let mut entry = telnet_entry(port);
    entry.encoding = badterm_core::Encoding::Petscii;
    entry.username = "sysop".into();
    let (mut session, rx) = dial(entry, config("c64login"));
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |_, _| server.is_finished());
    let got = server.join().unwrap();
    // The name in the C64's default case: unshifted letters.
    let at = got.iter().position(|&b| b == 0x53).expect("the user name");
    assert_eq!(&got[at..at + 6], b"SYSOP\x0d");
    session.hang_up();
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

    use std::borrow::Cow;

    use badterm_core::session::{PromptKind, SessionConfig};
    use russh::keys::ssh_key::private::Ed25519Keypair;
    use russh::keys::ssh_key::{Cipher, Kdf, LineEnding};
    use russh::keys::{PrivateKey, PublicKey};
    use russh::server::{self, Auth, Msg, Response, Server as _, Session as ServerSession, VerifiedPublicKey};
    use russh::{Channel, ChannelId, MethodKind, MethodSet};

    use super::*;

    /// A fake SSH BBS. The password "secret" always works for "sysop"; it can
    /// also accept a public key, or ask a keyboard-interactive question.
    #[derive(Clone, Default)]
    struct Bbs {
        client_key: Option<PublicKey>,
        /// Ask for a verification code (answer "1234") instead of a password.
        code: bool,
    }

    impl server::Server for Bbs {
        type Handler = Self;
        fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
            self.clone()
        }
    }

    impl server::Handler for Bbs {
        type Error = russh::Error;

        async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
            if user == "sysop" && password == "secret" && !self.code {
                return Ok(Auth::Accept);
            }
            // Keep offering passwords after a wrong one, as OpenSSH does
            // (russh's default is to stop).
            let methods = [MethodKind::PublicKey, MethodKind::Password, MethodKind::KeyboardInteractive];
            Ok(Auth::Reject { proceed_with_methods: Some(MethodSet::from(&methods[..])), partial_success: false })
        }

        async fn auth_publickey(&mut self, user: &str, key: VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
            let ok = user == "sysop" && self.client_key.as_ref() == Some(key.public_key);
            if ok { Ok(Auth::Accept) } else { Ok(Auth::reject()) }
        }

        async fn auth_keyboard_interactive<'a>(
            &'a mut self,
            _: &str,
            _: &str,
            response: Option<Response<'a>>,
        ) -> Result<Auth, Self::Error> {
            if !self.code {
                return Ok(Auth::reject());
            }
            match response {
                None => Ok(Auth::Partial {
                    name: Cow::Borrowed(""),
                    instructions: Cow::Borrowed("Check your authenticator app."),
                    prompts: Cow::Owned(vec![(Cow::Borrowed("Verification code: "), true)]),
                }),
                Some(mut r) => {
                    let ok = r.next().as_deref() == Some(&b"1234"[..]);
                    if ok { Ok(Auth::Accept) } else { Ok(Auth::reject()) }
                }
            }
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
        start_bbs(seed, Bbs::default())
    }

    fn start_bbs(seed: u8, bbs: Bbs) -> u16 {
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
                let mut bbs = bbs;
                let _ = bbs.run_on_socket(config, &listener).await;
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

    /// Wait for the login to ask something; returns (kind, instructions, prompt, echo).
    fn next_prompt(session: &mut Session, rx: &mpsc::Receiver<NetEvent>) -> (PromptKind, String, String, bool) {
        let mut events = Vec::new();
        pump_until(session, rx, &mut events, |s, _| s.auth_prompt().is_some() || s.state == State::Closed);
        let p = session.auth_prompt().unwrap_or_else(|| panic!("no prompt; closed with {:?}", session.error));
        (p.kind, p.instructions.clone(), p.prompt.clone(), p.echo)
    }

    fn welcomed(session: &mut Session, rx: &mpsc::Receiver<NetEvent>) {
        let mut events = Vec::new();
        pump_until(session, rx, &mut events, |s, _| row(s, 1) == "Welcome to SSH BBS" || s.state == State::Closed);
        assert_eq!(session.state, State::Connected, "{:?}", session.error);
        assert!(events.contains(&SessionEvent::Connected), "{events:?}");
    }

    #[test]
    fn wrong_password_asks_again() {
        let port = start_server(3);
        let (mut session, rx) = dial(ssh_entry(port, "wrong"), config("ssh-bad"));
        let (kind, why, prompt, echo) = next_prompt(&mut session, &rx);
        assert_eq!((kind, prompt.as_str(), echo), (PromptKind::Password, "Password for sysop:", false));
        assert!(why.contains("not accepted"), "{why}");
        session.answer_prompt(Some("secret".into()));
        welcomed(&mut session, &rx);
    }

    #[test]
    fn missing_password_is_asked_for() {
        let port = start_server(5);
        let (mut session, rx) = dial(ssh_entry(port, ""), config("ssh-ask"));
        let (kind, why, ..) = next_prompt(&mut session, &rx);
        assert_eq!((kind, why.as_str()), (PromptKind::Password, ""));
        session.answer_prompt(Some("secret".into()));
        welcomed(&mut session, &rx);
    }

    #[test]
    fn cancelling_the_prompt_ends_the_login() {
        let port = start_server(6);
        let (mut session, rx) = dial(ssh_entry(port, ""), config("ssh-cancel"));
        next_prompt(&mut session, &rx);
        session.answer_prompt(None);
        assert_eq!(closed_error(&mut session, &rx), "Login cancelled");
    }

    #[test]
    fn repeated_wrong_passwords_are_refused() {
        let port = start_server(7);
        let (mut session, rx) = dial(ssh_entry(port, ""), config("ssh-refused"));
        for _ in 0..3 {
            next_prompt(&mut session, &rx);
            session.answer_prompt(Some("nope".into()));
        }
        let err = closed_error(&mut session, &rx);
        assert!(err.contains("was refused"), "{err}");
    }

    #[test]
    fn keyboard_interactive_questions_are_shown() {
        let port = start_bbs(8, Bbs { code: true, ..Bbs::default() });
        // The stored password does not answer a question that echoes.
        let (mut session, rx) = dial(ssh_entry(port, "secret"), config("ssh-kbd"));
        let (kind, why, prompt, echo) = next_prompt(&mut session, &rx);
        assert_eq!((kind, prompt.as_str(), echo), (PromptKind::Interactive, "Verification code:", true));
        assert_eq!(why, "Check your authenticator app.");
        session.answer_prompt(Some("1234".into()));
        welcomed(&mut session, &rx);
    }

    fn client_key(seed: u8) -> PrivateKey {
        PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]))
    }

    fn write_key(path: &std::path::Path, key: &PrivateKey, passphrase: Option<&str>) {
        let key = match passphrase {
            Some(p) => {
                key.encrypt_with(Cipher::Aes256Ctr, Kdf::Bcrypt { salt: vec![7; 16], rounds: 4 }, 42, p).unwrap()
            }
            None => key.clone(),
        };
        key.write_openssh_file(path, LineEnding::LF).unwrap();
    }

    fn key_bbs(seed: u8, key: &PrivateKey) -> u16 {
        start_bbs(seed, Bbs { client_key: Some(key.public_key().clone()), ..Bbs::default() })
    }

    #[test]
    fn key_file_from_the_entry() {
        let key = client_key(20);
        let port = key_bbs(9, &key);
        let cfg = config("ssh-keyfile");
        let path = cfg.known_hosts.with_file_name("bbs_key");
        write_key(&path, &key, None);
        let entry = Entry { key_file: path.to_string_lossy().into(), ..ssh_entry(port, "") };
        let (mut session, rx) = dial(entry, cfg);
        welcomed(&mut session, &rx);
    }

    #[test]
    fn encrypted_key_file_asks_for_its_passphrase() {
        let key = client_key(21);
        let port = key_bbs(10, &key);
        let cfg = config("ssh-passphrase");
        let path = cfg.known_hosts.with_file_name("bbs_key");
        write_key(&path, &key, Some("hunter2"));
        let entry = Entry { key_file: path.to_string_lossy().into(), ..ssh_entry(port, "") };
        let (mut session, rx) = dial(entry, cfg);

        let (kind, why, prompt, _) = next_prompt(&mut session, &rx);
        assert_eq!((kind, why.as_str()), (PromptKind::Passphrase, ""));
        assert!(prompt.starts_with("Passphrase for ") && prompt.ends_with("bbs_key:"), "{prompt}");
        session.answer_prompt(Some("wrong".into()));
        let (kind, why, ..) = next_prompt(&mut session, &rx);
        assert_eq!(kind, PromptKind::Passphrase);
        assert!(why.contains("not accepted"), "{why}");
        session.answer_prompt(Some("hunter2".into()));
        welcomed(&mut session, &rx);
    }

    fn with_key_dir(name: &str) -> SessionConfig {
        let cfg = config(name);
        let dir = cfg.known_hosts.with_file_name("dot-ssh");
        std::fs::create_dir_all(&dir).unwrap();
        SessionConfig { ssh_key_dir: Some(dir), ..cfg }
    }

    #[test]
    fn default_key_files_are_tried() {
        let key = client_key(22);
        let port = key_bbs(11, &key);
        let cfg = with_key_dir("ssh-default-key");
        // An unrelated key first, as people often have several.
        write_key(&cfg.ssh_key_dir.as_ref().unwrap().join("id_ed25519"), &client_key(23), None);
        write_key(&cfg.ssh_key_dir.as_ref().unwrap().join("id_rsa"), &key, None);
        let (mut session, rx) = dial(ssh_entry(port, ""), cfg);
        welcomed(&mut session, &rx);
    }

    #[test]
    fn encrypted_default_keys_are_skipped() {
        let key = client_key(24);
        let port = key_bbs(12, &key);
        let cfg = with_key_dir("ssh-default-encrypted");
        write_key(&cfg.ssh_key_dir.as_ref().unwrap().join("id_ed25519"), &key, Some("hunter2"));
        // No passphrase question: straight to the password.
        let (mut session, rx) = dial(ssh_entry(port, ""), cfg);
        let (kind, ..) = next_prompt(&mut session, &rx);
        assert_eq!(kind, PromptKind::Password);
        session.answer_prompt(Some("secret".into()));
        welcomed(&mut session, &rx);
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
