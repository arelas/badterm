//! RLogin and telnet over TLS, against small local servers.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use phosphor_core::session::SessionEvent;
use phosphor_core::telnet::{DO, IAC, WILL, opt};
use phosphor_core::transport::tls_pins_path;
use phosphor_core::{Entry, Protocol};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

mod common;
use common::*;

#[test]
fn rlogin_sends_its_login_and_hides_the_reply() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 256];
        while got.iter().filter(|&&b| b == 0).count() < 4 {
            let n = sock.read(&mut buf).unwrap();
            assert!(n > 0, "client closed early; got {got:?}");
            got.extend_from_slice(&buf[..n]);
        }
        // The NUL that accepts the login, then the BBS, in one packet.
        sock.write_all(b"\0Welcome back\r\n").unwrap();
        // Bytes telnet would treat as commands pass through untouched.
        sock.write_all(&[IAC, DO, opt::ECHO]).unwrap();
        thread::sleep(Duration::from_millis(300));
        got
    });

    let entry = Entry {
        host: "127.0.0.1".into(),
        port,
        protocol: Protocol::Rlogin,
        username: "sysop".into(),
        password: "secret".into(),
        ..Entry::default()
    };
    let (mut session, rx) = dial(entry, config("rlogin"));
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| row(s, 0) == "Welcome back");
    let got = server.join().unwrap();
    assert_eq!(got, b"\0secret\0sysop\0ansi/115200\0");
}

/// A TLS server with a fresh self-signed certificate for "localhost", that
/// greets each caller and asks for its terminal type.
fn tls_bbs() -> (u16, thread::JoinHandle<Vec<u8>>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let chain = vec![CertificateDer::from(cert.cert.der().to_vec())];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let conn = rustls::ServerConnection::new(Arc::new(config)).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, sock);
        let mut got = Vec::new();
        // A client that refuses the certificate never finishes the handshake.
        if tls.write_all(b"Secure BBS\r\n").and_then(|()| tls.write_all(&[IAC, DO, opt::TTYPE])).is_err() {
            return got;
        }
        let _ = tls.flush();
        let mut buf = [0u8; 256];
        while !got.windows(3).any(|w| w == [IAC, WILL, opt::TTYPE]) {
            match tls.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
            }
        }
        got
    });
    (port, server)
}

#[test]
fn telnet_over_tls_trusts_a_self_signed_certificate_once() {
    let cfg = config("tls");
    let (port, server) = tls_bbs();
    let entry = Entry { host: "localhost".into(), port, protocol: Protocol::TelnetS, ..Entry::default() };

    let (mut session, rx) = dial(entry.clone(), cfg.clone());
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| row(s, 0) == "Secure BBS");
    let got = server.join().unwrap();
    // Telnet runs inside the TLS connection.
    assert!(got.windows(3).any(|w| w == [IAC, WILL, opt::TTYPE]), "{got:?}");
    let notice = events.iter().find_map(|e| match e {
        SessionEvent::Notice(n) => Some(n.clone()),
        _ => None,
    });
    assert!(notice.as_deref().is_some_and(|n| n.contains("self-signed")), "{events:?}");
    let pins = std::fs::read_to_string(tls_pins_path(&cfg.known_hosts)).unwrap();
    assert!(pins.starts_with(&format!("localhost:{port} sha256:")), "{pins}");
    session.hang_up();

    // The same system with a different certificate is refused. (The new
    // server has a new port, so pin the old certificate under it.)
    let (port2, server) = tls_bbs();
    std::fs::write(tls_pins_path(&cfg.known_hosts), pins.replace(&format!(":{port} "), &format!(":{port2} "))).unwrap();
    let (mut session, rx) = dial(Entry { port: port2, ..entry }, cfg);
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| s.error.is_some());
    assert!(session.error.as_deref().unwrap().contains("CHANGED"), "{:?}", session.error);
    let _ = server.join();
}

#[test]
fn auto_login_answers_the_prompts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 256];
        let mut wait_for = |sock: &mut std::net::TcpStream, needle: &[u8], got: &mut Vec<u8>| {
            while !got.windows(needle.len()).any(|w| w == needle) {
                let n = sock.read(&mut buf).unwrap();
                assert!(n > 0, "client closed early; got {got:?}");
                got.extend_from_slice(&buf[..n]);
            }
        };
        // A pause too short to count: "Last login:" is not a prompt.
        sock.write_all(b"Welcome!\r\nLast login:").unwrap();
        thread::sleep(Duration::from_millis(50));
        sock.write_all(b" yesterday\r\nLogin: ").unwrap();
        wait_for(&mut sock, b"sysop\r", &mut got);
        sock.write_all(b"\r\nPassword: ").unwrap();
        wait_for(&mut sock, b"secret\r", &mut got);
        sock.write_all(b"\r\nHello, sysop.\r\n").unwrap();
        thread::sleep(Duration::from_millis(300));
        got
    });

    let entry = Entry { username: "sysop".into(), password: "secret".into(), ..telnet_entry(port) };
    let (mut session, rx) = dial(entry, config("autologin"));
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| (0..25).any(|y| row(s, y) == "Hello, sysop."));
    let got = server.join().unwrap();
    let text = String::from_utf8_lossy(&got);
    assert_eq!(text.matches("sysop").count(), 1, "{got:?}");
    assert!(text.find("sysop").unwrap() < text.find("secret").unwrap(), "{got:?}");
}

#[test]
fn busy_and_unreachable_calls_are_worth_redialing() {
    // A BBS that answers, says it is full and hangs up.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(b"Sorry, all nodes are busy. Please try again later.\r\n").unwrap();
    });
    let (mut session, rx) = dial(telnet_entry(port), config("busy"));
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| s.state == phosphor_core::session::State::Closed);
    server.join().unwrap();
    assert!(session.should_redial());

    // One that hangs up after a normal session is not.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(b"Goodbye!\r\n").unwrap();
    });
    let (mut session, rx) = dial(telnet_entry(port), config("bye"));
    pump_until(&mut session, &rx, &mut events, |s, _| s.state == phosphor_core::session::State::Closed);
    server.join().unwrap();
    assert!(!session.should_redial());

    // Nothing listening.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (mut session, rx) = dial(telnet_entry(port), config("down"));
    pump_until(&mut session, &rx, &mut events, |s, _| s.state == phosphor_core::session::State::Closed);
    assert!(session.should_redial(), "{:?}", session.error);
}
