//! Interoperability with lrzsz, the `sz`/`rz` most Unix BBSes run, and its
//! XMODEM and YMODEM programs (`sx`/`rx`, `sb`/`rb`).
//!
//! A fake telnet BBS bridges its socket to a real `sz` or `rz` process, so
//! these exercise everything between the wire and the saved file: telnet
//! escaping, transfer detection, the zmodem2 state machines and the cleanup
//! of ZMODEM's trailer from the screen.
//!
//! The binaries are looked up in `$BADTERM_LRZSZ_DIR`, then on `PATH`.
//! Without them the tests are skipped, unless `BADTERM_REQUIRE_LRZSZ` is
//! set (as in CI), which makes a missing lrzsz a failure.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use badterm_core::session::{NetEvent, Session, SessionEvent};
use badterm_core::telnet::Telnet;
use badterm_core::transfer::{Outcome, XyProtocol};

mod common;
use common::*;

fn lrzsz(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("BADTERM_LRZSZ_DIR").map(PathBuf::from).into_iter().collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Some(p) = dirs.iter().map(|d| d.join(name)).find(|p| p.is_file()) {
        return Some(p);
    }
    if std::env::var_os("BADTERM_REQUIRE_LRZSZ").is_some() {
        panic!("{name} not found; install lrzsz or set BADTERM_LRZSZ_DIR");
    }
    eprintln!("skipping: {name} (lrzsz) not found");
    None
}

/// A BBS that writes `before`, runs `program` with its stdio bridged to the
/// telnet connection, then writes `after`. Returns the program's exit status.
fn bbs(
    listener: TcpListener,
    mut program: Command,
    before: &'static [u8],
    after: &'static [u8],
) -> thread::JoinHandle<ExitStatus> {
    thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(before).unwrap();
        let mut child: Child =
            program.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();

        // Program output -> telnet-encoded -> socket.
        let mut out = child.stdout.take().unwrap();
        let mut to_sock = sock.try_clone().unwrap();
        let down = thread::spawn(move || {
            let telnet = Telnet::new("ANSI", 80, 25);
            let mut buf = [0u8; 8192];
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 || to_sock.write_all(&telnet.encode(&buf[..n])).is_err() {
                    break;
                }
            }
        });
        // Socket -> telnet-decoded -> program input.
        let mut input = child.stdin.take().unwrap();
        let mut from_sock = sock.try_clone().unwrap();
        thread::spawn(move || {
            let mut telnet = Telnet::new("ANSI", 80, 25);
            let mut buf = [0u8; 8192];
            while let Ok(n) = from_sock.read(&mut buf) {
                let (mut data, mut reply) = (Vec::new(), Vec::new());
                telnet.receive(&buf[..n], &mut data, &mut reply);
                if n == 0 || input.write_all(&data).is_err() {
                    break;
                }
            }
        });

        let status = wait_with_timeout(&mut child, Duration::from_secs(30));
        down.join().unwrap();
        let _ = sock.write_all(after);
        thread::sleep(Duration::from_millis(300));
        let _ = sock.shutdown(Shutdown::Both);
        status
    })
}

fn wait_with_timeout(child: &mut Child, limit: Duration) -> ExitStatus {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("lrzsz did not exit");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn listen() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

fn finished(events: &[SessionEvent]) -> Option<&Outcome> {
    events.iter().find_map(|e| match e {
        SessionEvent::TransferFinished(o) => Some(o),
        _ => None,
    })
}

fn screen_text(session: &Session) -> Vec<String> {
    (0..6).map(|y| row(session, y)).collect()
}

fn connect(port: u16, name: &str) -> (Session, mpsc::Receiver<NetEvent>, PathBuf) {
    let cfg = config(name);
    let downloads = cfg.download_dir.clone();
    let (s, rx) = dial(telnet_entry(port), cfg);
    (s, rx, downloads)
}

#[test]
fn download_batch_from_sz() {
    let Some(sz) = lrzsz("sz") else { return };
    let src = temp_dir("lrz-src");
    let (big, big_data) = test_file(&src, "BIGFILE.ZIP", 1_500_000);
    let (small, small_data) = test_file(&src, "readme 1st.txt", 3_000);
    let mut cmd = Command::new(sz);
    cmd.arg(&big).arg(&small);

    let (listener, port) = listen();
    let server = bbs(listener, cmd, b"Sending 2 files\r\n", b"\r\nBack at the menu");
    let (mut session, rx, downloads) = connect(port, "lrz-dl");
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, ev| {
        finished(ev).is_some() && s.transfer().is_none() && (0..6).any(|y| row(s, y).contains("menu"))
    });

    assert!(server.join().unwrap().success(), "sz failed");
    let saved = [downloads.join("BIGFILE.ZIP"), downloads.join("readme 1st.txt")];
    assert_eq!(finished(&events), Some(&Outcome::Completed(saved.to_vec())));
    assert_eq!(std::fs::read(&saved[0]).unwrap(), big_data);
    assert_eq!(std::fs::read(&saved[1]).unwrap(), small_data);

    // Only BBS text on screen: sz's "rz\r" prompt is overwritten in place,
    // and no ZMODEM header or "OO" trailer shows up.
    let text = screen_text(&session);
    assert_eq!(text[0], "Sending 2 files");
    assert!(text.iter().any(|l| l == "Back at the menu"), "{text:?}");
    for l in &text {
        assert!(!l.contains("**") && !l.contains("OO") && !l.contains("B00"), "protocol bytes on screen: {text:?}");
    }
}

#[test]
fn upload_batch_to_rz() {
    let Some(rz) = lrzsz("rz") else { return };
    let inbox = temp_dir("lrz-inbox");
    let local = temp_dir("lrz-local");
    let (a, a_data) = test_file(&local, "art.ans", 250_000);
    let (b, b_data) = test_file(&local, "notes.txt", 10);
    let mut cmd = Command::new(rz);
    cmd.current_dir(&inbox);

    let (listener, port) = listen();
    let server = bbs(listener, cmd, b"Start your upload now\r\n", b"\r\nThanks!");
    let (mut session, rx, _) = connect(port, "lrz-ul");
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |_, ev| ev.contains(&SessionEvent::UploadRequested));
    session.start_upload(vec![a.clone(), b.clone()]).unwrap();
    pump_until(&mut session, &rx, &mut events, |_, ev| finished(ev).is_some());

    assert_eq!(finished(&events), Some(&Outcome::Completed(vec![a, b])));
    assert!(server.join().unwrap().success(), "rz failed");
    assert_eq!(std::fs::read(inbox.join("art.ans")).unwrap(), a_data);
    assert_eq!(std::fs::read(inbox.join("notes.txt")).unwrap(), b_data);
}

#[test]
fn cancelling_a_download_stops_sz() {
    let Some(sz) = lrzsz("sz") else { return };
    let src = temp_dir("lrz-cancel-src");
    let (big, _) = test_file(&src, "HUGE.ISO", 30_000_000);
    let mut cmd = Command::new(sz);
    cmd.arg(&big);

    let (listener, port) = listen();
    let server = bbs(listener, cmd, b"", b"\r\nTransfer aborted");
    let (mut session, rx, _) = connect(port, "lrz-cancel");
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |s, _| s.transfer().is_some_and(|t| t.progress().file_bytes > 0));
    session.cancel_transfer();
    events.extend(session.take_events());
    assert_eq!(finished(&events), Some(&Outcome::Cancelled));

    // sz must notice the cancel and give up rather than run to completion.
    let started = Instant::now();
    pump_until(&mut session, &rx, &mut events, |_, _| server.is_finished());
    assert!(!server.join().unwrap().success(), "sz should exit with an error after a cancel");
    assert!(started.elapsed() < Duration::from_secs(15));
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn an_interrupted_download_resumes() {
    let Some(sz) = lrzsz("sz") else { return };
    let src = temp_dir("lrz-resume-src");
    let (file, data) = test_file(&src, "PART2.ZIP", 400_000);
    let mut cmd = Command::new(sz);
    cmd.arg(&file);

    let (listener, port) = listen();
    let server = bbs(listener, cmd, b"", b"");
    let (mut session, rx, downloads) = connect(port, "lrz-resume");
    // The first 150,000 bytes came down before the line dropped.
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("PART2.ZIP.part"), &data[..150_000]).unwrap();
    let mut events = Vec::new();
    let mut resumed = None;
    pump_until(&mut session, &rx, &mut events, |s, ev| {
        if let Some(t) = s.transfer() {
            resumed = resumed.or(t.progress().resumed_at);
        }
        finished(ev).is_some()
    });
    assert!(server.join().unwrap().success(), "sz failed");
    let saved = downloads.join("PART2.ZIP");
    assert_eq!(finished(&events), Some(&Outcome::Completed(vec![saved.clone()])));
    assert_eq!(resumed, Some(150_000));
    assert_eq!(std::fs::read(&saved).unwrap(), data);
    assert!(!downloads.join("PART2.ZIP.part").exists());
    let _ = std::fs::remove_dir_all(&src);
}

// XMODEM and YMODEM, against lrzsz's sx/sb (send) and rx/rb (receive).

#[test]
fn download_ymodem_batch_from_sb() {
    let Some(sb) = lrzsz("sb") else { return };
    let src = temp_dir("lrz-sb-src");
    let (big, big_data) = test_file(&src, "GAME.ZIP", 300_000);
    let (small, small_data) = test_file(&src, "file_id.diz", 333);
    let mut cmd = Command::new(sb);
    cmd.arg(&big).arg(&small);

    let (listener, port) = listen();
    let server = bbs(listener, cmd, b"", b"\r\nBack at the menu");
    let (mut session, rx, downloads) = connect(port, "lrz-sb");
    session.start_xy_download(XyProtocol::Ymodem, None).unwrap();
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |_, ev| finished(ev).is_some());

    let saved = [downloads.join("GAME.ZIP"), downloads.join("file_id.diz")];
    assert_eq!(finished(&events), Some(&Outcome::Completed(saved.to_vec())));
    assert!(server.join().unwrap().success(), "sb failed");
    assert_eq!(std::fs::read(&saved[0]).unwrap(), big_data);
    assert_eq!(std::fs::read(&saved[1]).unwrap(), small_data);
}

#[test]
fn download_xmodem_from_sx() {
    let Some(sx) = lrzsz("sx") else { return };
    let src = temp_dir("lrz-sx-src");
    // 1K blocks with CRC, then the original 128-byte blocks with a checksum.
    for (proto, args) in [(XyProtocol::Xmodem1k, &["-k"][..]), (XyProtocol::Xmodem, &[][..])] {
        let (file, data) = test_file(&src, "DOOR.EXE", 20_000);
        let mut cmd = Command::new(&sx);
        cmd.args(args).arg(&file);
        let (listener, port) = listen();
        let server = bbs(listener, cmd, b"", b"");
        let (mut session, rx, downloads) = connect(port, &format!("lrz-sx-{}", proto.name()));
        session.start_xy_download(proto, Some("door.exe".into())).unwrap();
        let mut events = Vec::new();
        pump_until(&mut session, &rx, &mut events, |_, ev| finished(ev).is_some());
        let saved = downloads.join("door.exe");
        assert_eq!(finished(&events), Some(&Outcome::Completed(vec![saved.clone()])), "{}", proto.name());
        assert!(server.join().unwrap().success(), "sx failed ({})", proto.name());
        // XMODEM pads the last block; BadTerm cuts the padding. This data
        // doesn't end in SUB, so the file comes back exactly.
        assert_eq!(std::fs::read(&saved).unwrap(), data, "{}", proto.name());
    }
}

#[test]
fn upload_ymodem_batch_to_rb() {
    let Some(rb) = lrzsz("rb") else { return };
    let inbox = temp_dir("lrz-rb-inbox");
    let local = temp_dir("lrz-rb-local");
    let (a, a_data) = test_file(&local, "upload.zip", 200_000);
    let (b, b_data) = test_file(&local, "note.txt", 5);
    let mut cmd = Command::new(rb);
    cmd.current_dir(&inbox);

    let (listener, port) = listen();
    let server = bbs(listener, cmd, b"", b"\r\nThanks!");
    let (mut session, rx, _) = connect(port, "lrz-rb");
    session.start_xy_upload(XyProtocol::Ymodem, vec![a.clone(), b.clone()]).unwrap();
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |_, ev| finished(ev).is_some());

    assert_eq!(finished(&events), Some(&Outcome::Completed(vec![a, b])));
    assert!(server.join().unwrap().success(), "rb failed");
    assert_eq!(std::fs::read(inbox.join("upload.zip")).unwrap(), a_data);
    assert_eq!(std::fs::read(inbox.join("note.txt")).unwrap(), b_data);
}

#[test]
fn upload_xmodem_to_rx() {
    let Some(rx_bin) = lrzsz("rx") else { return };
    let inbox = temp_dir("lrz-rx-inbox");
    let local = temp_dir("lrz-rx-local");
    let (a, a_data) = test_file(&local, "art.ans", 10_000);
    let mut cmd = Command::new(rx_bin);
    cmd.arg("-c").arg(inbox.join("art.ans"));

    let (listener, port) = listen();
    let server = bbs(listener, cmd, b"", b"");
    let (mut session, rx, _) = connect(port, "lrz-rx");
    session.start_xy_upload(XyProtocol::Xmodem1k, vec![a.clone()]).unwrap();
    let mut events = Vec::new();
    pump_until(&mut session, &rx, &mut events, |_, ev| finished(ev).is_some());

    assert_eq!(finished(&events), Some(&Outcome::Completed(vec![a])));
    assert!(server.join().unwrap().success(), "rx failed");
    // XMODEM's receiver keeps the padding of the last block.
    let got = std::fs::read(inbox.join("art.ans")).unwrap();
    assert_eq!(&got[..a_data.len()], &a_data[..]);
    assert!(got[a_data.len()..].iter().all(|&b| b == 0x1A), "{} bytes of padding", got.len() - a_data.len());
}
