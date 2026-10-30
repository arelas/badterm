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
