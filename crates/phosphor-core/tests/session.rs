//! End-to-end test against a fake telnet BBS on the loopback interface.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use phosphor_core::session::{NetEvent, Session, State, spawn_connection};
use phosphor_core::telnet::{DO, IAC, SB, SE, WILL, opt};
use phosphor_core::{Entry, Screen};

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

    let entry = Entry { host: "127.0.0.1".into(), port, ..Entry::default() };
    let mut session = Session::new(entry, 80, 25, 100).unwrap();
    let (tx, rx) = mpsc::channel();
    spawn_connection("127.0.0.1".into(), port, move |ev| {
        let _ = tx.send(ev);
    });

    // Pump network events until the server has seen everything it waits for.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut sent_key = false;
    while !server.is_finished() && Instant::now() < deadline {
        let Ok(ev) = rx.recv_timeout(Duration::from_millis(50)) else { continue };
        session.handle(ev);
        if !sent_key && Screen::row_text(session.term.screen.row(0)) == "Welcome" {
            session.send(b"x");
            sent_key = true;
        }
    }
    assert!(sent_key, "welcome screen never arrived");
    assert!(sent_key, "welcome screen never arrived");

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
    let (tx, rx) = mpsc::channel();
    spawn_connection("127.0.0.1".into(), port, move |ev| {
        let _ = tx.send(ev);
    });
    match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
        NetEvent::Closed(Some(msg)) => assert!(msg.contains("connecting"), "{msg}"),
        other => panic!("expected failure, got {other:?}"),
    }
}
