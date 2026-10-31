//! Dialing out through a modem: a fake Hayes modem on the other side of a
//! pseudo-terminal answers the init string and the dial, connects, passes
//! data, and drops the carrier. Unix only (pseudo-terminals).
#![cfg(unix)]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use badterm_core::dialer::{Entry, Protocol};
use badterm_core::modem::{FlowControl, ModemConfig};
use badterm_core::session::{ConnectOptions, NetEvent, spawn_connection};
use serialport::{SerialPort, TTYPort};

/// A modem that answers each command line with `answer(line)`, then, once
/// connected, sends `banner`, echoes what it gets in capitals until "bye",
/// and hangs up.
fn fake_modem(
    mut port: TTYPort,
    answer: fn(&str) -> &'static str,
    banner: &'static [u8],
) -> thread::JoinHandle<Vec<String>> {
    thread::spawn(move || {
        port.set_timeout(Duration::from_secs(10)).unwrap();
        let mut commands = Vec::new();
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            if port.read(&mut byte).unwrap_or(0) == 0 {
                return commands;
            }
            // Echo, as modems do.
            port.write_all(&byte).unwrap();
            if byte[0] != b'\r' {
                line.push(byte[0]);
                continue;
            }
            let cmd = String::from_utf8_lossy(&line).trim().to_string();
            line.clear();
            commands.push(cmd.clone());
            let reply = answer(&cmd);
            port.write_all(format!("\r\n{reply}\r\n").as_bytes()).unwrap();
            if reply.starts_with("CONNECT") {
                break;
            }
            if reply != "OK" {
                // Stay on the line until the reply has been read.
                thread::sleep(Duration::from_millis(500));
                return commands;
            }
        }
        port.write_all(banner).unwrap();
        let mut got = Vec::new();
        while !got.ends_with(b"bye") {
            let mut buf = [0u8; 64];
            let n = port.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
            port.write_all(&buf[..n].to_ascii_uppercase()).unwrap();
        }
        thread::sleep(Duration::from_millis(200));
        port.write_all(b"\r\nNO CARRIER\r\n").unwrap();
        thread::sleep(Duration::from_millis(500));
        commands
    })
}

fn dial(device: String, number: &str) -> mpsc::Receiver<NetEvent> {
    let entry = Entry { protocol: Protocol::Modem, host: number.into(), port: 0, ..Entry::default() };
    let opts = ConnectOptions {
        cols: 80,
        rows: 25,
        known_hosts: std::env::temp_dir().join("badterm-modem-known_hosts"),
        ssh_agent: false,
        ssh_key_dir: None,
        modem: ModemConfig { device, speed: 115_200, flow: FlowControl::None, ..ModemConfig::default() },
    };
    let (tx, rx) = mpsc::channel();
    spawn_connection(&entry, &opts, move |ev| {
        let _ = tx.send(ev);
    });
    rx
}

fn next(rx: &mpsc::Receiver<NetEvent>) -> NetEvent {
    rx.recv_timeout(Duration::from_secs(20)).expect("an event from the modem thread")
}

#[test]
fn dials_connects_and_notices_the_carrier_drop() {
    let (master, slave) = TTYPort::pair().unwrap();
    let modem = fake_modem(
        master,
        |cmd| match cmd {
            "ATZ" => "OK",
            "ATDT555-1234" => "CONNECT 33600",
            _ => "ERROR",
        },
        b"Welcome to the BBS\r\n",
    );
    let rx = dial(slave.name().unwrap(), "555-1234");
    let mut shown = Vec::new();
    let mut writer = loop {
        match next(&rx) {
            NetEvent::Data(d) => shown.extend(d),
            NetEvent::Connected(w) => break w,
            NetEvent::Notice(_) => {}
            other => panic!("before connecting: {other:?}"),
        }
    };
    // The dialing showed on the terminal, as it would have.
    let text = String::from_utf8_lossy(&shown).into_owned();
    assert!(text.contains("ATDT555-1234") && text.contains("CONNECT 33600"), "{text:?}");
    writer.write_all(b"hello bye").unwrap();
    let mut after = Vec::new();
    let closed = loop {
        match next(&rx) {
            NetEvent::Data(d) => after.extend(d),
            NetEvent::Closed(why) => break why,
            other => panic!("while connected: {other:?}"),
        }
    };
    // The banner may come with the CONNECT, or after it.
    shown.extend(after);
    let text = String::from_utf8_lossy(&shown).into_owned();
    assert!(text.contains("Welcome to the BBS") && text.contains("HELLO BYE"), "{text:?}");
    assert_eq!(closed, None, "the carrier dropped: a normal end");
    assert_eq!(modem.join().unwrap(), ["ATZ", "ATDT555-1234"]);
    drop(slave);
}

#[test]
fn a_busy_line_is_reported_for_redialing() {
    let (master, slave) = TTYPort::pair().unwrap();
    let modem = fake_modem(master, |cmd| if cmd == "ATZ" { "OK" } else { "BUSY" }, b"");
    let rx = dial(slave.name().unwrap(), "5551234");
    let why = loop {
        match next(&rx) {
            NetEvent::Closed(why) => break why,
            NetEvent::Connected(_) => panic!("a busy line connected"),
            _ => {}
        }
    };
    // "connecting to" is what makes a session worth redialing.
    assert_eq!(why.as_deref(), Some("connecting to 5551234: the line is busy (BUSY)"));
    modem.join().unwrap();
    drop(slave);
}
