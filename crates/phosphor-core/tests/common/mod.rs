//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use phosphor_core::session::{NetEvent, Session, SessionConfig, SessionEvent};
use phosphor_core::{Entry, Screen};

pub fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("phosphor-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn config(name: &str) -> SessionConfig {
    let dir = temp_dir(name);
    SessionConfig {
        scrollback: 100,
        known_hosts: dir.join("known_hosts"),
        download_dir: dir.join("downloads"),
        // Never the developer's own agent or ~/.ssh keys.
        ssh_agent: false,
        ssh_key_dir: None,
    }
}

/// Connect a session and return it with the channel its events arrive on.
pub fn dial(entry: Entry, cfg: SessionConfig) -> (Session, mpsc::Receiver<NetEvent>) {
    let session = Session::new(entry, 80, 25, cfg).unwrap();
    let (tx, rx) = mpsc::channel();
    session.connect(move |ev| {
        let _ = tx.send(ev);
    });
    (session, rx)
}

/// Pump network events into the session until `done` holds or time runs out.
pub fn pump_until(
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

pub fn row(session: &Session, y: usize) -> String {
    Screen::row_text(session.term.screen.row(y))
}

pub fn telnet_entry(port: u16) -> Entry {
    Entry { host: "127.0.0.1".into(), port, ..Entry::default() }
}

pub fn test_file(dir: &Path, name: &str, len: usize) -> (PathBuf, Vec<u8>) {
    // Pseudo-random bytes, so IAC (0xFF), CR and ZMODEM control bytes all occur.
    let data: Vec<u8> = (0..len as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8).collect();
    let path = dir.join(name);
    std::fs::write(&path, &data).unwrap();
    (path, data)
}
