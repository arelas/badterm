//! Interoperability with C-Kermit, the Kermit most Unix BBSes and shells
//! offer, both ways. C-Kermit is run on one side of a pseudo-terminal (its
//! `-l` line), and BadTerm's Kermit on the other.
//!
//! `kermit` is looked up in `$BADTERM_KERMIT_DIR`, then on `PATH`. Without
//! it the tests are skipped, unless `BADTERM_REQUIRE_KERMIT` is set (as in
//! CI). Unix only: Windows has no pseudo-terminals of this kind.
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use badterm_core::transfer::{Kermit, Outcome, Transfer};
use serialport::{SerialPort, TTYPort};

fn kermit() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("BADTERM_KERMIT_DIR").map(PathBuf::from).into_iter().collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Some(p) = dirs.iter().map(|d| d.join("kermit")).find(|p| p.is_file()) {
        return Some(p);
    }
    if std::env::var_os("BADTERM_REQUIRE_KERMIT").is_some() {
        panic!("kermit not found; install C-Kermit (ckermit) or set BADTERM_KERMIT_DIR");
    }
    eprintln!("skipping: kermit (C-Kermit) not found");
    None
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("badterm-ck-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn test_data(len: usize) -> Vec<u8> {
    let mut v: Vec<u8> = (0..len as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8).collect();
    // Runs to compress, and every control character.
    v.extend(std::iter::repeat_n(b'x', 300));
    v.extend(0..=255u8);
    v
}

/// Run C-Kermit with `args` on a pseudo-terminal, and `transfer` on the
/// other end, until both are done.
fn run(args: &[&str], dir: &Path, mut transfer: Transfer) -> (Outcome, Child) {
    let Some(kermit) = kermit() else { panic!("checked by the caller") };
    let (mut master, slave) = TTYPort::pair().expect("a pseudo-terminal");
    let line = slave.name().expect("the pty's name");
    let child = Command::new(kermit)
        .args(["-Y", "-l", &line, "-b", "115200", "-i"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    master.set_timeout(Duration::from_millis(20)).unwrap();
    let mut out = Vec::new();
    transfer.start(&mut out);
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut buf = vec![0u8; 8192];
    while !transfer.is_finished() {
        assert!(Instant::now() < deadline, "the transfer did not finish");
        if !out.is_empty() {
            master.write_all(&std::mem::take(&mut out)).unwrap();
        }
        match master.read(&mut buf) {
            Ok(n) => transfer.feed(&buf[..n], &mut out),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => transfer.tick(Instant::now(), &mut out),
            Err(e) => panic!("reading the pty: {e}"),
        }
    }
    master.write_all(&out).unwrap();
    // Let C-Kermit see the last ACK before the pty goes.
    let mut child = child;
    let until = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() && Instant::now() < until {
        let _ = master.read(&mut buf);
    }
    drop(slave);
    (transfer.finish().0, child)
}

#[test]
fn download_from_c_kermit() {
    if kermit().is_none() {
        return;
    }
    let (src, dst) = (temp_dir("src"), temp_dir("dst"));
    let data = test_data(200_000);
    std::fs::write(src.join("GAME.ZIP"), &data).unwrap();
    std::fs::write(src.join("notes.txt"), b"line one\r\nline two\r\n").unwrap();
    let rx = Transfer::Kermit(Kermit::receive(dst.clone()));
    let (outcome, mut child) = run(&["-s", "GAME.ZIP", "notes.txt"], &src, rx);
    let _ = child.kill();
    // C-Kermit sends names in capitals, as DOS had them; they are kept.
    assert_eq!(outcome, Outcome::Completed(vec![dst.join("GAME.ZIP"), dst.join("NOTES.TXT")]));
    assert_eq!(std::fs::read(dst.join("GAME.ZIP")).unwrap(), data);
    assert_eq!(std::fs::read(dst.join("NOTES.TXT")).unwrap(), b"line one\r\nline two\r\n");
}

#[test]
fn upload_to_c_kermit() {
    if kermit().is_none() {
        return;
    }
    let (src, dst) = (temp_dir("usrc"), temp_dir("udst"));
    let data = test_data(150_000);
    std::fs::write(src.join("upload.bin"), &data).unwrap();
    let tx = Transfer::Kermit(Kermit::send(vec![src.join("upload.bin")]).unwrap());
    let (outcome, mut child) = run(&["-r"], &dst, tx);
    let status = child.wait().unwrap();
    assert_eq!(outcome, Outcome::Completed(vec![src.join("upload.bin")]));
    assert!(status.success(), "C-Kermit: {status}");
    assert_eq!(std::fs::read(dst.join("upload.bin")).unwrap(), data);
}
