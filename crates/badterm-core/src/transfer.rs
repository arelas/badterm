//! File transfers: ZMODEM here, XMODEM and YMODEM in [`crate::xmodem`], and
//! Kermit in [`crate::kermit`].
//!
//! ZMODEM transfers drive the caller-owned state machines of the
//! `zmodem2` crate with file I/O, progress tracking and timeouts.
//!
//! A [`Transfer`] is fed raw (post-telnet) bytes from the remote and appends
//! the bytes it wants sent to an output buffer, so it works the same over
//! telnet, raw TCP and SSH.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use zmodem2::{Action, Event, FileInfo, Position, Receiver, Sender};

pub use crate::kermit::Kermit;
pub use crate::xmodem::{XyProtocol, XyReceive, XySend};

/// `ZPAD ZPAD ZDLE 'B' "00"`: the hex ZRQINIT header a sender (`sz`) opens with.
pub const ZRQINIT: &[u8] = b"**\x18B00";
/// `ZPAD ZPAD ZDLE 'B' "01"`: the hex ZRINIT header a waiting receiver (`rz`) sends.
pub const ZRINIT: &[u8] = b"**\x18B01";
/// Ten CANs then ten backspaces: makes the other end abandon a session.
pub const CANCEL: &[u8] = b"\x18\x18\x18\x18\x18\x18\x18\x18\x18\x18\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08";

/// Wait this long for input before nudging the other side.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);
/// Give up after this many consecutive timeouts.
pub(crate) const MAX_TIMEOUTS: u32 = 6;
pub(crate) const MAX_FILE_SIZE: u64 = u32::MAX as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Download,
    Upload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Finished; lists the files saved (download) or sent (upload).
    Completed(Vec<PathBuf>),
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub direction: Direction,
    pub file_name: String,
    pub file_bytes: u64,
    pub file_size: Option<u64>,
    pub files_done: usize,
    /// Files still waiting to be sent (uploads only).
    pub files_queued: usize,
    pub file_started: Instant,
    /// Where a resumed download picked up, if it was resumed.
    pub resumed_at: Option<u64>,
}

impl Progress {
    pub(crate) fn new(direction: Direction) -> Progress {
        Progress {
            direction,
            file_name: String::new(),
            file_bytes: 0,
            file_size: None,
            files_done: 0,
            files_queued: 0,
            file_started: Instant::now(),
            resumed_at: None,
        }
    }

    /// Average speed of the current file in bytes per second (of what
    /// came this time, for a resumed download).
    pub fn rate(&self) -> f64 {
        let secs = self.file_started.elapsed().as_secs_f64();
        let bytes = self.file_bytes.saturating_sub(self.resumed_at.unwrap_or(0));
        if secs > 0.0 { bytes as f64 / secs } else { 0.0 }
    }
}

/// State shared by both directions.
pub(crate) struct Common {
    pub(crate) pending: Vec<u8>,
    pub(crate) progress: Progress,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) last_input: Instant,
    pub(crate) timeouts: u32,
    pub(crate) cancelled: bool,
}

impl Common {
    pub(crate) fn new(direction: Direction) -> Common {
        Common {
            pending: Vec::new(),
            progress: Progress::new(direction),
            outcome: None,
            last_input: Instant::now(),
            timeouts: 0,
            cancelled: false,
        }
    }

    pub(crate) fn input(&mut self, data: &[u8]) {
        self.pending.extend_from_slice(data);
        self.last_input = Instant::now();
        self.timeouts = 0;
    }

    /// True when the timeout elapsed; sets a failure once too many have.
    fn timed_out(&mut self, now: Instant) -> bool {
        if self.outcome.is_some() || now.duration_since(self.last_input) < TIMEOUT {
            return false;
        }
        self.last_input = now;
        self.timeouts += 1;
        if self.timeouts >= MAX_TIMEOUTS {
            self.outcome = Some(Outcome::Failed("the other side stopped responding".into()));
        }
        true
    }

    pub(crate) fn aborted(&mut self) {
        let outcome = if self.cancelled { Outcome::Cancelled } else { Outcome::Failed("cancelled by the BBS".into()) };
        self.outcome.get_or_insert(outcome);
    }

    /// Bytes left after the session ended, to hand back to the terminal.
    fn leftover(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

/// Downloads are written under this suffix until they are complete, so an
/// interrupted one can be resumed when the BBS sends the file again.
pub const PARTIAL_SUFFIX: &str = ".part";

pub struct Download {
    rx: Receiver,
    dir: PathBuf,
    /// The file being written (`name.part`), and its name once complete.
    file: Option<(BufWriter<File>, PathBuf, String)>,
    /// Where to ask the sender to start the file just announced, once the
    /// receiver has room to say so.
    accept: Option<u32>,
    saved: Vec<PathBuf>,
    c: Common,
}

impl Download {
    pub fn new(dir: PathBuf) -> Result<Download> {
        // TCP and SSH are reliable and flow-controlled, so let the sender
        // stream without per-buffer acknowledgements.
        let mut rx = Receiver::with_flow_control(0, true).map_err(|e| anyhow::anyhow!("{e}"))?;
        // Decide where each file starts: a partial one is resumed.
        rx.set_manual_file_accept(true);
        Ok(Download { rx, dir, file: None, accept: None, saved: Vec::new(), c: Common::new(Direction::Download) })
    }

    fn fail(&mut self, msg: String) {
        self.c.outcome.get_or_insert(Outcome::Failed(msg));
        let _ = self.rx.abort();
    }

    /// Open the file the sender announced, as `name.part`: appending to
    /// one left by an interrupted download of it (when its size says there
    /// is more to come), or new. Returns where the sender should start.
    fn open(&mut self, wire_name: &[u8], size: Option<u64>) -> Option<u32> {
        self.close();
        let name = sanitize_file_name(wire_name);
        let part = self.dir.join(format!("{name}{PARTIAL_SUFFIX}"));
        let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let resume = size.is_some_and(|s| have > 0 && have < s && have <= u32::MAX as u64);
        let result = fs::create_dir_all(&self.dir)
            .and_then(|()| if resume { fs::OpenOptions::new().append(true).open(&part) } else { File::create(&part) });
        match result {
            Ok(f) => {
                let offset = if resume { have } else { 0 };
                self.file = Some((BufWriter::new(f), part, name.clone()));
                let p = &mut self.c.progress;
                p.file_name = name;
                p.file_size = size;
                p.file_bytes = offset;
                p.resumed_at = resume.then_some(offset);
                p.file_started = Instant::now();
                Some(offset as u32)
            }
            Err(e) => {
                self.fail(format!("creating {} in {}: {e}", part.display(), self.dir.display()));
                None
            }
        }
    }

    /// Finish writing the current file. A complete one gets its own name
    /// (never replacing a file); an incomplete one stays as `name.part`.
    fn close_file(&mut self, complete: bool) -> Option<PathBuf> {
        let (mut f, part, name) = self.file.take()?;
        if let Err(e) = f.flush() {
            self.fail(format!("writing {}: {e}", part.display()));
            return None;
        }
        drop(f);
        if !complete {
            return None;
        }
        match unique_path(&self.dir, &name).and_then(|dest| fs::rename(&part, &dest).map(|()| dest)) {
            Ok(dest) => Some(dest),
            Err(e) => {
                self.fail(format!("saving {name}: {e}"));
                None
            }
        }
    }

    fn close(&mut self) {
        self.close_file(false);
    }

    fn pump(&mut self, out: &mut Vec<u8>) {
        loop {
            match self.rx.poll() {
                Action::WriteWire(b) => {
                    out.extend_from_slice(b);
                    let n = b.len();
                    self.rx.wire_written(n);
                }
                Action::WriteFile(b) => {
                    let n = b.len();
                    let written = match &mut self.file {
                        Some((f, _, _)) => f.write_all(b),
                        None => Ok(()),
                    };
                    if let Err(e) = written {
                        self.fail(format!("writing {}: {e}", self.c.progress.file_name));
                        continue;
                    }
                    self.c.progress.file_bytes += n as u64;
                    if let Err(e) = self.rx.file_written(n) {
                        log::debug!("zmodem file_written: {e}");
                    }
                }
                Action::Event(Event::FileStarted(info)) => {
                    let name = info.name.to_vec();
                    let size = info.size.map(|s| u64::from(s.get()));
                    self.accept = self.open(&name, size);
                }
                Action::Event(Event::FileCompleted) => {
                    let path = self.close_file(true);
                    self.saved.extend(path);
                    self.c.progress.files_done += 1;
                }
                Action::Event(Event::SessionCompleted) => {
                    self.close();
                    self.c.outcome.get_or_insert(Outcome::Completed(self.saved.clone()));
                }
                Action::Event(Event::Aborted) => {
                    self.close();
                    self.c.aborted();
                }
                Action::Idle => {
                    if let Some(offset) = self.accept {
                        match self.rx.accept_file_at(offset) {
                            Ok(()) => {
                                self.accept = None;
                                continue;
                            }
                            // Outgoing bytes first.
                            Err(zmodem2::Error::Backpressure) => {}
                            Err(e) => {
                                self.accept = None;
                                self.fail(format!("starting {}: {e}", self.c.progress.file_name));
                            }
                        }
                    }
                    if self.c.outcome.is_some() || self.c.pending.is_empty() {
                        break;
                    }
                    match self.rx.submit_wire(&self.c.pending) {
                        Ok(0) => break,
                        Ok(n) => {
                            self.c.pending.drain(..n);
                        }
                        Err(e) => {
                            // The receiver has asked for a resend; drop the damaged input.
                            log::debug!("zmodem receive error: {e}");
                            self.c.pending.clear();
                        }
                    }
                }
                _ => break,
            }
        }
    }
}

pub struct Upload {
    tx: Sender,
    queue: VecDeque<PathBuf>,
    file: Option<(File, PathBuf)>,
    sent: Vec<PathBuf>,
    /// Start the next file (or finish) once the outgoing buffer drains.
    advance: bool,
    finishing: bool,
    buf: Vec<u8>,
    c: Common,
}

impl Upload {
    pub fn new(paths: Vec<PathBuf>) -> Result<Upload> {
        if paths.is_empty() {
            bail!("no files to upload");
        }
        for p in &paths {
            check_upload(p)?;
        }
        let mut tx = Sender::new().map_err(|e| anyhow::anyhow!("{e}"))?;
        tx.set_streaming_window(usize::MAX);
        let mut up = Upload {
            tx,
            queue: paths.into(),
            file: None,
            sent: Vec::new(),
            advance: false,
            finishing: false,
            buf: Vec::new(),
            c: Common::new(Direction::Upload),
        };
        up.start_next();
        Ok(up)
    }

    /// Add a file to the batch. Fails once the last file has been sent.
    pub fn enqueue(&mut self, path: PathBuf) -> Result<()> {
        if self.finishing || self.c.outcome.is_some() {
            bail!("the upload is already finishing");
        }
        check_upload(&path)?;
        self.queue.push_back(path);
        self.c.progress.files_queued = self.queue.len();
        Ok(())
    }

    fn fail(&mut self, msg: String) {
        self.c.outcome.get_or_insert(Outcome::Failed(msg));
        self.tx.abort();
    }

    fn start_next(&mut self) {
        let Some(path) = self.queue.pop_front() else {
            self.finishing = true;
            if let Err(e) = self.tx.finish() {
                self.fail(format!("finishing upload: {e}"));
            }
            return;
        };
        self.c.progress.files_queued = self.queue.len();
        let opened = File::open(&path).and_then(|f| f.metadata().map(|m| (f, m)));
        let (f, meta) = match opened {
            Ok(v) => v,
            Err(e) => return self.fail(format!("opening {}: {e}", path.display())),
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let size = meta.len();
        let mut info = FileInfo::new(name.as_bytes(), Some(Position::new(size as u32)));
        if let Some(t) = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()) {
            info = info.with_modified(t.as_secs() as u32);
        }
        if let Err(e) = self.tx.start_file(info) {
            return self.fail(format!("starting {name}: {e}"));
        }
        let p = &mut self.c.progress;
        p.file_name = name;
        p.file_size = Some(size);
        p.file_bytes = 0;
        p.file_started = Instant::now();
        self.file = Some((f, path));
    }

    fn read_chunk(&mut self, offset: u64, max_len: usize) -> std::io::Result<usize> {
        let Some((f, _)) = &mut self.file else { return Ok(0) };
        self.buf.resize(max_len, 0);
        f.seek(SeekFrom::Start(offset))?;
        f.read(&mut self.buf)
    }

    fn pump(&mut self, out: &mut Vec<u8>) {
        loop {
            match self.tx.poll() {
                Action::WriteWire(b) => {
                    out.extend_from_slice(b);
                    let n = b.len();
                    self.tx.wire_written(n);
                }
                Action::ReadFile { offset, max_len } => {
                    let offset = u64::from(offset.get());
                    match self.read_chunk(offset, max_len) {
                        Ok(0) => self.fail(format!("{} got shorter while sending", self.c.progress.file_name)),
                        Ok(n) => {
                            if let Err(e) = self.tx.submit_file(&self.buf[..n]) {
                                log::debug!("zmodem submit_file: {e}");
                            }
                            self.c.progress.file_bytes = offset + n as u64;
                        }
                        Err(e) => self.fail(format!("reading {}: {e}", self.c.progress.file_name)),
                    }
                }
                Action::Event(Event::FileCompleted) => {
                    if let Some((_, path)) = self.file.take() {
                        self.sent.push(path);
                    }
                    self.c.progress.files_done += 1;
                    self.advance = true;
                }
                Action::Event(Event::SessionCompleted) => {
                    self.c.outcome.get_or_insert(Outcome::Completed(self.sent.clone()));
                }
                Action::Event(Event::Aborted) => self.c.aborted(),
                Action::Event(_) => {}
                Action::Idle => {
                    if self.advance && self.c.outcome.is_none() {
                        self.advance = false;
                        self.start_next();
                        continue;
                    }
                    if self.c.outcome.is_some() || self.c.pending.is_empty() {
                        break;
                    }
                    match self.tx.submit_wire(&self.c.pending) {
                        Ok(0) => break,
                        Ok(n) => {
                            self.c.pending.drain(..n);
                        }
                        Err(e) => {
                            log::debug!("zmodem send error: {e}");
                            self.c.pending.clear();
                        }
                    }
                }
                _ => break,
            }
        }
    }
}

pub(crate) fn check_upload(p: &Path) -> Result<()> {
    let meta = fs::metadata(p).with_context(|| format!("{}", p.display()))?;
    if !meta.is_file() {
        bail!("{} is not a file", p.display());
    }
    if meta.len() > MAX_FILE_SIZE {
        bail!("{} is larger than the 4 GiB limit", p.display());
    }
    Ok(())
}

pub enum Transfer {
    Download(Download),
    Upload(Upload),
    /// XMODEM or YMODEM.
    XyDownload(XyReceive),
    XyUpload(XySend),
    /// Either way.
    Kermit(Kermit),
}

impl Transfer {
    fn parts(&mut self) -> &mut Common {
        match self {
            Transfer::Download(d) => &mut d.c,
            Transfer::Upload(u) => &mut u.c,
            Transfer::XyDownload(d) => &mut d.c,
            Transfer::XyUpload(u) => &mut u.c,
            Transfer::Kermit(k) => &mut k.c,
        }
    }

    fn pump(&mut self, out: &mut Vec<u8>) {
        match self {
            Transfer::Download(d) => d.pump(out),
            Transfer::Upload(u) => u.pump(out),
            Transfer::XyDownload(d) => d.pump(out),
            Transfer::XyUpload(u) => u.pump(out),
            Transfer::Kermit(k) => k.pump(out),
        }
    }

    /// The protocol's name, for the progress window.
    pub fn name(&self) -> &'static str {
        match self {
            Transfer::Download(_) | Transfer::Upload(_) => "ZMODEM",
            Transfer::XyDownload(d) => d.protocol().name(),
            Transfer::XyUpload(u) => u.protocol().name(),
            Transfer::Kermit(_) => "Kermit",
        }
    }

    /// Start a side that speaks first (a Kermit sender): call once.
    pub fn start(&mut self, out: &mut Vec<u8>) {
        if let Transfer::Kermit(k) = self {
            k.start(out);
        }
    }

    /// Feed bytes received from the remote; bytes to send are appended to `out`.
    pub fn feed(&mut self, data: &[u8], out: &mut Vec<u8>) {
        self.parts().input(data);
        self.pump(out);
    }

    /// Call periodically to drive timeouts and retries.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<u8>) {
        // X/YMODEM keep their own time: a receiver asks every few seconds.
        match self {
            Transfer::XyDownload(d) => {
                d.tick(now, out);
                return self.pump(out);
            }
            Transfer::XyUpload(u) => {
                u.tick(now, out);
                return self.pump(out);
            }
            Transfer::Kermit(k) => {
                k.tick(now, out);
                return self.pump(out);
            }
            _ => {}
        }
        if !self.parts().timed_out(now) {
            return;
        }
        let gave_up = self.parts().outcome.is_some();
        match self {
            Transfer::Download(d) if gave_up => {
                let _ = d.rx.abort();
            }
            Transfer::Download(d) => {
                let _ = d.rx.timeout();
            }
            Transfer::Upload(u) if gave_up => u.tx.abort(),
            Transfer::Upload(u) => {
                let _ = u.tx.timeout();
            }
            Transfer::XyDownload(_) | Transfer::XyUpload(_) | Transfer::Kermit(_) => {}
        }
        self.pump(out);
    }

    /// Abort the transfer, telling the other side to stop.
    pub fn cancel(&mut self, out: &mut Vec<u8>) {
        self.parts().cancelled = true;
        match self {
            Transfer::Download(d) => {
                let _ = d.rx.abort();
            }
            Transfer::Upload(u) => u.tx.abort(),
            Transfer::XyDownload(d) => return d.cancel(out),
            Transfer::XyUpload(u) => return u.cancel(out),
            Transfer::Kermit(k) => return k.cancel(out),
        }
        self.pump(out);
        // The abort is final whether or not the other side acknowledges it.
        self.parts().outcome.get_or_insert(Outcome::Cancelled);
    }

    pub fn progress(&self) -> &Progress {
        match self {
            Transfer::Download(d) => &d.c.progress,
            Transfer::Upload(u) => &u.c.progress,
            Transfer::XyDownload(d) => d.progress(),
            Transfer::XyUpload(u) => u.progress(),
            Transfer::Kermit(k) => k.progress(),
        }
    }

    pub fn is_finished(&self) -> bool {
        match self {
            Transfer::Download(d) => d.c.outcome.is_some(),
            Transfer::Upload(u) => u.c.outcome.is_some(),
            Transfer::XyDownload(d) => d.c.outcome.is_some(),
            Transfer::XyUpload(u) => u.c.outcome.is_some(),
            Transfer::Kermit(k) => k.c.outcome.is_some(),
        }
    }

    /// The result and any bytes received after the session ended, which
    /// belong to the terminal.
    pub fn finish(mut self) -> (Outcome, Vec<u8>) {
        let c = self.parts();
        let leftover = c.leftover();
        let outcome = c.outcome.take().unwrap_or_else(|| Outcome::Failed("interrupted".into()));
        if let (Transfer::Download(d), Outcome::Failed(_) | Outcome::Cancelled) = (&mut self, &outcome) {
            d.close();
        }
        (outcome, leftover)
    }

    pub fn enqueue(&mut self, path: PathBuf) -> Result<()> {
        match self {
            Transfer::Upload(u) => u.enqueue(path),
            Transfer::XyUpload(u) => u.enqueue(path),
            Transfer::Kermit(k) => k.enqueue(path),
            Transfer::Download(_) | Transfer::XyDownload(_) => bail!("a download is in progress"),
        }
    }
}

/// Reduce a file name from the wire to a safe base name: no directories, no
/// control characters, never empty, "." or "..".
pub fn sanitize_file_name(wire: &[u8]) -> String {
    let s = String::from_utf8_lossy(wire);
    let base = s.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "download".to_string() } else { cleaned }
}

/// `dir/name`, or `dir/name (1).ext`, ... if that already exists.
pub(crate) fn unique_path(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let first = dir.join(name);
    if !first.exists() {
        return Ok(first);
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    for i in 1..10_000 {
        let p = dir.join(format!("{stem} ({i}){ext}"));
        if !p.exists() {
            return Ok(p);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("too many copies of {name}")))
}

/// Strips the tail end of a ZMODEM session from data that follows it: the
/// CR LF (often with the high bit set) and XON closing the last hex header,
/// and the "OO" ("over and out") that `sz` sends after its final ZFIN. These
/// can arrive in later packets than the end of the session, so the filter
/// stays armed across calls until real data shows up or it expires.
#[derive(Debug)]
pub struct TrailerFilter {
    until: Instant,
    o_left: u8,
}

impl TrailerFilter {
    const WINDOW: Duration = Duration::from_secs(2);

    pub fn new() -> TrailerFilter {
        TrailerFilter { until: Instant::now() + Self::WINDOW, o_left: 2 }
    }

    /// Remove trailer bytes from the start of `data`. Returns false once the
    /// filter is done and can be dropped.
    pub fn strip(&mut self, data: &mut Vec<u8>) -> bool {
        if Instant::now() > self.until {
            return false;
        }
        // Where the trailer ends, if it ends within this chunk.
        let mut end = None;
        for (n, &b) in data.iter().enumerate() {
            match b {
                b'\r' | b'\n' | 0x8D | 0x8A | 0x11 | 0x13 => {}
                b'O' if self.o_left > 0 => {
                    self.o_left -= 1;
                    if self.o_left == 0 {
                        // "OO" is the very last thing a sender writes.
                        end = Some(n + 1);
                        break;
                    }
                }
                _ => {
                    end = Some(n);
                    break;
                }
            }
        }
        match end {
            Some(cut) => {
                data.drain(..cut);
                false
            }
            None => {
                data.clear();
                true
            }
        }
    }
}

impl Default for TrailerFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the first occurrence of `needle` in `hay`.
pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("badterm-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Run an upload against a download in memory until both finish.
    fn run(up: &mut Transfer, down: &mut Transfer) {
        let (mut to_down, mut to_up) = (Vec::new(), Vec::new());
        up.feed(&[], &mut to_down);
        for _ in 0..100_000 {
            if up.is_finished() && down.is_finished() {
                return;
            }
            let data = std::mem::take(&mut to_down);
            down.feed(&data, &mut to_up);
            let data = std::mem::take(&mut to_up);
            up.feed(&data, &mut to_down);
        }
        panic!("transfer did not finish");
    }

    #[test]
    fn upload_to_download_round_trip() {
        let src = temp_dir("src");
        let dst = temp_dir("dst");
        let mut contents = Vec::new();
        for i in 0..300_000u32 {
            contents.push((i.wrapping_mul(2_654_435_761) >> 13) as u8);
        }
        // Include bytes that need escaping on the wire.
        contents.extend_from_slice(&[0x18, 0x11, 0x13, 0xFF, b'\r', 0]);
        fs::write(src.join("big.bin"), &contents).unwrap();
        fs::write(src.join("empty.txt"), b"").unwrap();
        fs::write(dst.join("big.bin"), b"already here").unwrap();

        let mut up = Transfer::Upload(Upload::new(vec![src.join("big.bin"), src.join("empty.txt")]).unwrap());
        let mut down = Transfer::Download(Download::new(dst.clone()).unwrap());
        run(&mut up, &mut down);

        let (up_outcome, _) = up.finish();
        let (down_outcome, _) = down.finish();
        assert_eq!(up_outcome, Outcome::Completed(vec![src.join("big.bin"), src.join("empty.txt")]));
        // The existing file was not overwritten.
        assert_eq!(down_outcome, Outcome::Completed(vec![dst.join("big (1).bin"), dst.join("empty.txt")]));
        assert_eq!(fs::read(dst.join("big (1).bin")).unwrap(), contents);
        assert_eq!(fs::read(dst.join("big.bin")).unwrap(), b"already here");
        assert_eq!(fs::read(dst.join("empty.txt")).unwrap(), b"");
    }

    #[test]
    fn cancel_sends_abort() {
        let dst = temp_dir("cancel");
        let mut down = Transfer::Download(Download::new(dst).unwrap());
        let mut out = Vec::new();
        down.cancel(&mut out);
        assert!(find(&out, &CANCEL[..10]).is_some());
        assert_eq!(down.finish().0, Outcome::Cancelled);
    }

    #[test]
    fn trailer_filter_spans_chunks() {
        let mut f = TrailerFilter::new();
        let mut a = b"\r\x8a".to_vec();
        assert!(f.strip(&mut a));
        assert!(a.is_empty());
        // The BBS's own line break after "OO" is kept.
        let mut b = b"OO\r\nMenu: ".to_vec();
        assert!(!f.strip(&mut b));
        assert_eq!(b, b"\r\nMenu: ");
        // Only two Os are ever eaten.
        let mut f = TrailerFilter::new();
        let mut c = b"OOOps".to_vec();
        f.strip(&mut c);
        assert_eq!(c, b"Ops");
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_file_name(b"../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name(b"C:\\DOS\\GAME.ZIP"), "GAME.ZIP");
        assert_eq!(sanitize_file_name(b".."), "download");
        assert_eq!(sanitize_file_name(b".hidden"), "hidden");
        assert_eq!(sanitize_file_name(b"a\x1bb?.txt"), "a_b_.txt");
    }
}
