//! XMODEM (checksum, CRC and 1K) and YMODEM (batch, and the streaming
//! YMODEM-G) file transfers, for the BBSes and doors that offer them.
//!
//! Like ZMODEM in [`crate::transfer`], each side is a state machine fed the
//! bytes from the remote, appending what it wants sent. Neither protocol
//! announces itself, so the user starts them: a download asks the sender to
//! begin (with 'C', NAK or 'G'), an upload waits for the receiver to.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Result, bail};

use crate::transfer::{self, Common, Direction, Outcome, Progress};

const SOH: u8 = 0x01;
const STX: u8 = 0x02;
const EOT: u8 = 0x04;
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const CAN: u8 = 0x18;
/// XMODEM pads the last block with this.
const SUB: u8 = 0x1A;

/// How often a receiver asks the sender to start, and how many times.
const START_EVERY: Duration = Duration::from_secs(3);
const START_TRIES: u32 = 20;
/// Retries of one block (or EOT) before giving up.
const MAX_RETRIES: u32 = 10;
/// Blocks sent per call while streaming (YMODEM-G).
const STREAM_BURST: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XyProtocol {
    Ymodem,
    /// YMODEM without acknowledgements, for error-free links.
    YmodemG,
    Xmodem1k,
    XmodemCrc,
    /// The original: 128-byte blocks with a checksum.
    Xmodem,
}

impl XyProtocol {
    pub const ALL: [XyProtocol; 5] =
        [XyProtocol::Ymodem, XyProtocol::YmodemG, XyProtocol::Xmodem1k, XyProtocol::XmodemCrc, XyProtocol::Xmodem];

    pub fn name(self) -> &'static str {
        match self {
            XyProtocol::Ymodem => "YMODEM",
            XyProtocol::YmodemG => "YMODEM-G",
            XyProtocol::Xmodem1k => "XMODEM-1K",
            XyProtocol::XmodemCrc => "XMODEM-CRC",
            XyProtocol::Xmodem => "XMODEM",
        }
    }

    /// YMODEM sends file names and sizes, and several files at once.
    pub fn batch(self) -> bool {
        matches!(self, XyProtocol::Ymodem | XyProtocol::YmodemG)
    }

    fn crc(self) -> bool {
        self != XyProtocol::Xmodem
    }

    fn streaming(self) -> bool {
        self == XyProtocol::YmodemG
    }

    fn block_size(self) -> usize {
        match self {
            XyProtocol::Xmodem | XyProtocol::XmodemCrc => 128,
            _ => 1024,
        }
    }

    /// What a receiver sends to start the sender.
    fn start_char(self) -> u8 {
        match self {
            XyProtocol::Xmodem => NAK,
            XyProtocol::YmodemG => b'G',
            _ => b'C',
        }
    }
}

/// CRC-16/XMODEM: polynomial 0x1021, starting at 0.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// A block on the wire: SOH (128 bytes) or STX (1024), its number twice,
/// the data, and a CRC or checksum.
fn block(num: u8, data: &[u8], crc: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 5);
    out.push(if data.len() == 1024 { STX } else { SOH });
    out.push(num);
    out.push(!num);
    out.extend_from_slice(data);
    if crc {
        out.extend_from_slice(&crc16(data).to_be_bytes());
    } else {
        out.push(data.iter().fold(0u8, |a, &b| a.wrapping_add(b)));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RState {
    /// Waiting for YMODEM's block 0: a file's name and size, or the end.
    Header,
    Data,
    Done,
}

/// Receiving files.
pub struct XyReceive {
    proto: XyProtocol,
    dir: PathBuf,
    /// XMODEM sends no name; this is what the file is saved as.
    name: String,
    state: RState,
    expected: u8,
    /// No block of the current file yet: keep asking the sender to start.
    starting: bool,
    last_ask: Option<Instant>,
    asks: u32,
    retries: u32,
    file: Option<(BufWriter<File>, PathBuf)>,
    /// YMODEM's file size, to cut the padding off the last block.
    remaining: Option<u64>,
    /// XMODEM's last block, held back until EOT so its padding can go.
    held: Vec<u8>,
    eots: u32,
    saved: Vec<PathBuf>,
    pub(crate) c: Common,
}

impl XyReceive {
    pub fn new(proto: XyProtocol, dir: PathBuf, name: Option<String>) -> XyReceive {
        let name =
            name.map(|n| transfer::sanitize_file_name(n.as_bytes())).unwrap_or_else(|| "xmodem-download".to_string());
        XyReceive {
            proto,
            dir,
            name,
            state: if proto.batch() { RState::Header } else { RState::Data },
            expected: 1,
            starting: true,
            last_ask: None,
            asks: 0,
            retries: 0,
            file: None,
            remaining: None,
            held: Vec::new(),
            eots: 0,
            saved: Vec::new(),
            c: Common::new(Direction::Download),
        }
    }

    pub(crate) fn progress(&self) -> &Progress {
        &self.c.progress
    }

    pub fn protocol(&self) -> XyProtocol {
        self.proto
    }

    fn fail(&mut self, msg: impl Into<String>, out: &mut Vec<u8>) {
        if self.c.outcome.is_none() {
            out.extend_from_slice(transfer::CANCEL);
            self.c.outcome = Some(Outcome::Failed(msg.into()));
        }
        self.drop_file();
    }

    /// A partly received file is removed.
    fn drop_file(&mut self) {
        if let Some((_, path)) = self.file.take() {
            let _ = fs::remove_file(path);
        }
    }

    fn ask(&mut self, out: &mut Vec<u8>) {
        out.push(self.proto.start_char());
        self.last_ask = Some(Instant::now());
        self.starting = true;
    }

    /// Drive timeouts: ask the sender to start (again), or to resend.
    pub(crate) fn tick(&mut self, now: Instant, out: &mut Vec<u8>) {
        if self.c.outcome.is_some() || self.state == RState::Done {
            return;
        }
        if self.starting {
            if self.last_ask.is_none_or(|t| now.duration_since(t) >= START_EVERY) {
                self.asks += 1;
                if self.asks > START_TRIES {
                    return self.fail("the sender never started", out);
                }
                self.ask(out);
            }
            return;
        }
        if now.duration_since(self.c.last_input) >= transfer::TIMEOUT {
            self.c.last_input = now;
            self.retries += 1;
            if self.retries > MAX_RETRIES || self.proto.streaming() {
                return self.fail("the sender stopped", out);
            }
            out.push(NAK);
        }
    }

    pub(crate) fn pump(&mut self, out: &mut Vec<u8>) {
        if self.state == RState::Done && self.c.outcome.is_none() {
            self.c.outcome = Some(Outcome::Completed(self.saved.clone()));
        }
        while self.c.outcome.is_none() && !self.c.pending.is_empty() {
            let first = self.c.pending[0];
            match first {
                SOH | STX => {
                    let size = if first == SOH { 128 } else { 1024 };
                    let need = 3 + size + if self.proto.crc() { 2 } else { 1 };
                    if self.c.pending.len() < need {
                        return;
                    }
                    let raw: Vec<u8> = self.c.pending.drain(..need).collect();
                    let (num, inv, data) = (raw[1], raw[2], &raw[3..3 + size]);
                    let good = num == !inv
                        && if self.proto.crc() {
                            raw[3 + size..] == crc16(data).to_be_bytes()
                        } else {
                            raw[3 + size] == data.iter().fold(0u8, |a, &b| a.wrapping_add(b))
                        };
                    if !good {
                        if self.proto.streaming() {
                            return self.fail("a damaged block (YMODEM-G cannot resend)", out);
                        }
                        out.push(NAK);
                        continue;
                    }
                    self.retries = 0;
                    self.block(num, data.to_vec(), out);
                }
                EOT => {
                    self.c.pending.remove(0);
                    self.eot(out);
                }
                CAN => {
                    if self.c.pending.len() < 2 {
                        return;
                    }
                    if self.c.pending[1] == CAN {
                        self.drop_file();
                        self.c.aborted();
                        return;
                    }
                    self.c.pending.remove(0);
                }
                // Noise between blocks.
                _ => {
                    self.c.pending.remove(0);
                }
            }
        }
        if self.state == RState::Done && self.c.outcome.is_none() {
            self.c.outcome = Some(Outcome::Completed(self.saved.clone()));
        }
    }

    fn block(&mut self, num: u8, data: Vec<u8>, out: &mut Vec<u8>) {
        let streaming = self.proto.streaming();
        match self.state {
            RState::Header if num == 0 => {
                self.starting = false;
                out.push(ACK);
                // The end of the batch: an empty name.
                if data[0] == 0 {
                    self.state = RState::Done;
                    return;
                }
                let name_end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                let name = transfer::sanitize_file_name(&data[..name_end]);
                let rest = &data[(name_end + 1).min(data.len())..];
                let info = String::from_utf8_lossy(&rest[..rest.iter().position(|&b| b == 0).unwrap_or(rest.len())])
                    .into_owned();
                let size = info.split_whitespace().next().and_then(|s| s.parse::<u64>().ok());
                if let Err(e) = self.open(&name, size) {
                    return self.fail(e.to_string(), out);
                }
                self.state = RState::Data;
                self.expected = 1;
                // Now for the data.
                self.ask(out);
            }
            RState::Header => {
                if !streaming {
                    out.push(NAK);
                }
            }
            // The sender missed our ACK of the header and sent it again.
            RState::Data if num == 0 && self.proto.batch() && self.expected == 1 => {
                out.push(ACK);
                self.ask(out);
            }
            RState::Data if num == self.expected => {
                if self.starting && !self.proto.batch() {
                    let name = self.name.clone();
                    if let Err(e) = self.open(&name, None) {
                        return self.fail(e.to_string(), out);
                    }
                }
                self.starting = false;
                self.expected = self.expected.wrapping_add(1);
                if let Err(e) = self.write(data) {
                    return self.fail(e, out);
                }
                if !streaming {
                    out.push(ACK);
                }
            }
            RState::Data if num == self.expected.wrapping_sub(1) => {
                // A repeat of the last block: our ACK was lost.
                if !streaming {
                    out.push(ACK);
                }
            }
            RState::Data => self.fail(format!("blocks out of order (got {num}, wanted {})", self.expected), out),
            RState::Done => {}
        }
    }

    fn open(&mut self, name: &str, size: Option<u64>) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = transfer::unique_path(&self.dir, name)?;
        let f = File::create(&path)?;
        self.file = Some((BufWriter::new(f), path));
        self.remaining = size;
        self.held.clear();
        self.eots = 0;
        let p = &mut self.c.progress;
        p.file_name = name.to_string();
        p.file_size = size;
        p.file_bytes = 0;
        p.file_started = Instant::now();
        Ok(())
    }

    fn write(&mut self, mut data: Vec<u8>) -> Result<(), String> {
        if let Some(left) = &mut self.remaining {
            data.truncate((*left).min(data.len() as u64) as usize);
            *left -= data.len() as u64;
        } else {
            // XMODEM: write the block before this one; this one may be the
            // last, with padding to cut.
            std::mem::swap(&mut self.held, &mut data);
        }
        self.c.progress.file_bytes += data.len() as u64;
        match &mut self.file {
            Some((f, path)) => f.write_all(&data).map_err(|e| format!("writing {}: {e}", path.display())),
            None => Ok(()),
        }
    }

    fn eot(&mut self, out: &mut Vec<u8>) {
        if self.state != RState::Data || self.file.is_none() && self.proto.batch() {
            // No file under way (an empty YMODEM file has one): just
            // acknowledge it.
            out.push(ACK);
            return;
        }
        if self.file.is_none() {
            // An empty XMODEM file is just an EOT.
            let name = self.name.clone();
            if let Err(e) = self.open(&name, None) {
                return self.fail(e.to_string(), out);
            }
            self.starting = false;
        }
        // NAK the first EOT, as YMODEM asks, in case it was line noise.
        if !self.proto.streaming() && self.eots == 0 {
            self.eots = 1;
            out.push(NAK);
            return;
        }
        out.push(ACK);
        if let Err(e) = self.finish_file() {
            return self.fail(e, out);
        }
        if self.proto.batch() {
            self.state = RState::Header;
            self.ask(out);
        } else {
            self.state = RState::Done;
        }
    }

    fn finish_file(&mut self) -> Result<(), String> {
        let mut last = std::mem::take(&mut self.held);
        while last.last() == Some(&SUB) {
            last.pop();
        }
        self.c.progress.file_bytes += last.len() as u64;
        if let Some((mut f, path)) = self.file.take() {
            f.write_all(&last).and_then(|()| f.flush()).map_err(|e| format!("writing {}: {e}", path.display()))?;
            self.saved.push(path);
        }
        self.c.progress.files_done += 1;
        Ok(())
    }

    pub(crate) fn cancel(&mut self, out: &mut Vec<u8>) {
        self.c.cancelled = true;
        out.extend_from_slice(transfer::CANCEL);
        self.drop_file();
        self.c.outcome.get_or_insert(Outcome::Cancelled);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SState {
    /// Waiting for the receiver's 'C', NAK or 'G' to send a header (YMODEM)
    /// or the first block (XMODEM).
    Start,
    HeaderAck,
    /// The header was acknowledged; waiting for the go-ahead for data.
    DataStart,
    /// A block is out; waiting for its ACK.
    BlockAck,
    Streaming,
    EotAck,
    /// YMODEM: waiting to send the empty header that ends the batch.
    EndStart,
    EndAck,
    Done,
}

/// Sending files.
pub struct XySend {
    proto: XyProtocol,
    queue: VecDeque<PathBuf>,
    file: Option<(File, PathBuf)>,
    size: u64,
    sent_bytes: u64,
    sent: Vec<PathBuf>,
    state: SState,
    num: u8,
    /// The last block (or EOT) sent, for resending.
    last: Vec<u8>,
    crc: bool,
    streaming: bool,
    retries: u32,
    pub(crate) c: Common,
}

impl XySend {
    pub fn new(proto: XyProtocol, paths: Vec<PathBuf>) -> Result<XySend> {
        if paths.is_empty() {
            bail!("no files to upload");
        }
        if !proto.batch() && paths.len() > 1 {
            bail!("{} sends one file at a time; YMODEM sends several", proto.name());
        }
        for p in &paths {
            transfer::check_upload(p)?;
        }
        let mut c = Common::new(Direction::Upload);
        c.progress.files_queued = paths.len();
        Ok(XySend {
            proto,
            queue: paths.into(),
            file: None,
            size: 0,
            sent_bytes: 0,
            sent: Vec::new(),
            state: SState::Start,
            num: 0,
            last: Vec::new(),
            crc: proto.crc(),
            streaming: false,
            retries: 0,
            c,
        })
    }

    pub(crate) fn progress(&self) -> &Progress {
        &self.c.progress
    }

    pub fn protocol(&self) -> XyProtocol {
        self.proto
    }

    pub fn enqueue(&mut self, path: PathBuf) -> Result<()> {
        if !self.proto.batch() {
            bail!("{} sends one file at a time", self.proto.name());
        }
        if matches!(self.state, SState::EndStart | SState::EndAck | SState::Done) {
            bail!("the upload is already finishing");
        }
        transfer::check_upload(&path)?;
        self.queue.push_back(path);
        self.c.progress.files_queued = self.queue.len();
        Ok(())
    }

    fn fail(&mut self, msg: impl Into<String>, out: &mut Vec<u8>) {
        if self.c.outcome.is_none() {
            out.extend_from_slice(transfer::CANCEL);
            self.c.outcome = Some(Outcome::Failed(msg.into()));
        }
    }

    fn send(&mut self, bytes: Vec<u8>, out: &mut Vec<u8>) {
        out.extend_from_slice(&bytes);
        self.last = bytes;
    }

    /// Open the next file, if any.
    fn next_file(&mut self) -> Result<bool, String> {
        let Some(path) = self.queue.pop_front() else { return Ok(false) };
        self.c.progress.files_queued = self.queue.len();
        let f = File::open(&path).map_err(|e| format!("opening {}: {e}", path.display()))?;
        self.size = f.metadata().map(|m| m.len()).unwrap_or(0);
        self.sent_bytes = 0;
        let p = &mut self.c.progress;
        p.file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        p.file_size = Some(self.size);
        p.file_bytes = 0;
        p.file_started = Instant::now();
        self.file = Some((f, path));
        Ok(true)
    }

    /// YMODEM's block 0: name, size, modification time.
    fn header(&self) -> Vec<u8> {
        let (f, path) = self.file.as_ref().expect("a file is open");
        let mut data = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into_bytes();
        data.push(0);
        let mtime = f
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        data.extend_from_slice(format!("{} {mtime:o} 0", self.size).as_bytes());
        data.push(0);
        let len = if data.len() <= 128 { 128 } else { 1024 };
        data.resize(len, 0);
        block(0, &data, true)
    }

    /// The next data block, or None at the end of the file.
    fn data_block(&mut self) -> Result<Option<Vec<u8>>, String> {
        let left = self.size.saturating_sub(self.sent_bytes);
        if left == 0 {
            return Ok(None);
        }
        // 1K blocks need CRC; a short end goes in a 128-byte block.
        let size = if self.crc && self.proto.block_size() == 1024 && left > 128 { 1024 } else { 128 };
        let mut data = vec![0; size];
        let (f, path) = self.file.as_mut().expect("a file is open");
        let mut n = 0;
        while n < size {
            match f.read(&mut data[n..]) {
                Ok(0) => break,
                Ok(k) => n += k,
                Err(e) => return Err(format!("reading {}: {e}", path.display())),
            }
        }
        if n == 0 {
            return Err(format!("{} got shorter while sending", path.display()));
        }
        data[n..].fill(SUB);
        self.sent_bytes += n as u64;
        self.c.progress.file_bytes = self.sent_bytes;
        self.num = self.num.wrapping_add(1);
        Ok(Some(block(self.num, &data, self.crc)))
    }

    /// Send the next block, or EOT once the file is done.
    fn next_block(&mut self, out: &mut Vec<u8>) {
        match self.data_block() {
            Ok(Some(b)) => {
                self.send(b, out);
                self.state = if self.streaming { SState::Streaming } else { SState::BlockAck };
            }
            Ok(None) => {
                self.send(vec![EOT], out);
                self.state = SState::EotAck;
            }
            Err(e) => self.fail(e, out),
        }
    }

    /// The receiver asked to start: with CRC ('C'), a checksum (NAK) or
    /// streaming ('G').
    fn started_with(&mut self, b: u8) {
        self.crc = b != NAK;
        self.streaming = b == b'G';
    }

    pub(crate) fn pump(&mut self, out: &mut Vec<u8>) {
        if self.state == SState::Start && self.file.is_none() && self.c.outcome.is_none() {
            match self.next_file() {
                Ok(true) => {}
                Ok(false) => self.state = SState::EndStart,
                Err(e) => return self.fail(e, out),
            }
        }
        while self.c.outcome.is_none() {
            if self.state == SState::Streaming {
                // Stream a burst, unless the receiver has something to say.
                if !self.c.pending.is_empty() && self.c.pending.contains(&CAN) {
                    // Fall through to read it.
                } else {
                    for _ in 0..STREAM_BURST {
                        self.next_block(out);
                        if self.state != SState::Streaming {
                            break;
                        }
                    }
                    if self.state == SState::Streaming {
                        return;
                    }
                    continue;
                }
            }
            if self.c.pending.is_empty() {
                return;
            }
            let b = self.c.pending.remove(0);
            if b == CAN {
                if self.c.pending.first() == Some(&CAN) {
                    self.c.aborted();
                    return;
                }
                continue;
            }
            let go = matches!(b, b'C' | b'G' | NAK);
            match self.state {
                SState::Start if go => {
                    self.started_with(b);
                    self.num = 0;
                    if self.proto.batch() {
                        let h = self.header();
                        self.send(h, out);
                        self.state = SState::HeaderAck;
                    } else {
                        self.next_block(out);
                    }
                }
                SState::HeaderAck if b == ACK => self.state = SState::DataStart,
                // Some receivers go straight on without the ACK (YMODEM-G).
                SState::HeaderAck | SState::DataStart if b == b'C' || b == b'G' => {
                    self.started_with(b);
                    self.next_block(out);
                }
                SState::HeaderAck if b == NAK => self.resend(out),
                SState::BlockAck if b == ACK => {
                    self.retries = 0;
                    self.next_block(out);
                }
                SState::BlockAck | SState::EotAck if b == NAK => self.resend(out),
                SState::EotAck if b == ACK => {
                    self.retries = 0;
                    if let Some((_, path)) = self.file.take() {
                        self.sent.push(path);
                    }
                    self.c.progress.files_done += 1;
                    if self.proto.batch() {
                        self.state = SState::Start;
                        match self.next_file() {
                            Ok(true) => {}
                            Ok(false) => self.state = SState::EndStart,
                            Err(e) => return self.fail(e, out),
                        }
                    } else {
                        self.state = SState::Done;
                        self.c.outcome = Some(Outcome::Completed(self.sent.clone()));
                    }
                }
                SState::EndStart if go => {
                    self.send(block(0, &[0; 128], true), out);
                    self.state = SState::EndAck;
                }
                SState::EndAck if b == ACK => {
                    self.state = SState::Done;
                    self.c.outcome = Some(Outcome::Completed(self.sent.clone()));
                }
                SState::EndAck if b == NAK => self.resend(out),
                _ => {}
            }
        }
    }

    fn resend(&mut self, out: &mut Vec<u8>) {
        self.retries += 1;
        if self.retries > MAX_RETRIES {
            return self.fail("too many errors", out);
        }
        out.extend_from_slice(&self.last);
    }

    pub(crate) fn tick(&mut self, now: Instant, out: &mut Vec<u8>) {
        if self.c.outcome.is_some() || now.duration_since(self.c.last_input) < transfer::TIMEOUT {
            return;
        }
        self.c.last_input = now;
        match self.state {
            SState::Start | SState::EndStart => {
                self.c.timeouts += 1;
                if self.c.timeouts >= transfer::MAX_TIMEOUTS {
                    self.fail("the receiver never started", out);
                }
            }
            SState::Streaming => {}
            _ => self.resend(out),
        }
    }

    pub(crate) fn cancel(&mut self, out: &mut Vec<u8>) {
        self.c.cancelled = true;
        out.extend_from_slice(transfer::CANCEL);
        self.c.outcome.get_or_insert(Outcome::Cancelled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("badterm-xy-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Shuttle bytes between a sender and a receiver until both finish.
    fn run(tx: &mut XySend, rx: &mut XyReceive) {
        let mut to_rx: Vec<u8> = Vec::new();
        let mut to_tx: Vec<u8> = Vec::new();
        rx.tick(Instant::now(), &mut to_tx);
        for _ in 0..100_000 {
            if tx.c.outcome.is_some() && rx.c.outcome.is_some() {
                return;
            }
            let mut out = Vec::new();
            tx.c.input(&std::mem::take(&mut to_tx));
            tx.pump(&mut out);
            to_rx.extend(out);
            let mut out = Vec::new();
            rx.c.input(&std::mem::take(&mut to_rx));
            rx.pump(&mut out);
            to_tx.extend(out);
        }
        panic!("stuck: tx {:?} {:?}, rx {:?} {:?}", tx.state, tx.c.outcome, rx.state, rx.c.outcome);
    }

    #[test]
    fn crc16_check_value() {
        assert_eq!(crc16(b"123456789"), 0x31C3);
    }

    #[test]
    fn every_protocol_round_trips() {
        let src = temp_dir("src");
        // Sizes on and around the block boundaries, including a file ending
        // in SUB bytes, which only YMODEM (knowing the size) keeps.
        let files: Vec<(PathBuf, Vec<u8>)> = [(0usize, 0u8), (1, 7), (128, 9), (1025, 3), (5000, SUB)]
            .iter()
            .enumerate()
            .map(|(i, &(len, fill))| {
                let mut data: Vec<u8> = (0..len as u32).map(|j| (j * 31 + i as u32) as u8).collect();
                if fill == SUB && len > 0 {
                    data[len - 3..].fill(SUB);
                }
                let p = src.join(format!("file{i}.bin"));
                fs::write(&p, &data).unwrap();
                (p, data)
            })
            .collect();
        for proto in XyProtocol::ALL {
            let dst = temp_dir(proto.name());
            let one = if proto == XyProtocol::Xmodem { 0 } else { 3 };
            let paths: Vec<PathBuf> =
                if proto.batch() { files.iter().map(|(p, _)| p.clone()).collect() } else { vec![files[one].0.clone()] };
            let mut tx = XySend::new(proto, paths.clone()).unwrap();
            let mut rx = XyReceive::new(proto, dst.clone(), Some("got.bin".into()));
            run(&mut tx, &mut rx);
            assert_eq!(tx.c.outcome, Some(Outcome::Completed(paths.clone())), "{}", proto.name());
            let Some(Outcome::Completed(saved)) = rx.c.outcome.clone() else { panic!("{}", proto.name()) };
            assert_eq!(saved.len(), paths.len(), "{}", proto.name());
            for (path, (_, data)) in saved.iter().zip(files.iter().skip(if proto.batch() { 0 } else { one })) {
                let got = fs::read(path).unwrap();
                assert_eq!(&got, data, "{} {}", proto.name(), path.display());
            }
            if !proto.batch() {
                assert_eq!(saved[0].file_name().unwrap(), "got.bin");
            }
        }
    }

    #[test]
    fn damaged_blocks_are_resent() {
        let src = temp_dir("noise");
        let data: Vec<u8> = (0..3000u32).map(|i| (i * 7) as u8).collect();
        fs::write(src.join("a.bin"), &data).unwrap();
        let dst = temp_dir("noise-out");
        let mut tx = XySend::new(XyProtocol::Ymodem, vec![src.join("a.bin")]).unwrap();
        let mut rx = XyReceive::new(XyProtocol::Ymodem, dst, None);
        let mut to_rx: Vec<u8> = Vec::new();
        let mut to_tx = Vec::new();
        rx.tick(Instant::now(), &mut to_tx);
        let mut flipped = false;
        for _ in 0..10_000 {
            let mut out = Vec::new();
            tx.c.input(&std::mem::take(&mut to_tx));
            tx.pump(&mut out);
            // Damage the first data block once.
            if !flipped && out.len() > 1000 && out[0] == STX && out[1] == 1 {
                out[500] ^= 0xFF;
                flipped = true;
            }
            to_rx.extend(out);
            let mut out = Vec::new();
            rx.c.input(&std::mem::take(&mut to_rx));
            rx.pump(&mut out);
            to_tx.extend(out);
            if tx.c.outcome.is_some() && rx.c.outcome.is_some() {
                break;
            }
        }
        assert!(flipped);
        let Some(Outcome::Completed(saved)) = rx.c.outcome.clone() else { panic!("{:?}", rx.c.outcome) };
        assert_eq!(fs::read(&saved[0]).unwrap(), data);
    }

    #[test]
    fn xmodem_takes_one_file_and_cancel_is_heard() {
        let src = temp_dir("one");
        fs::write(src.join("a"), b"a").unwrap();
        fs::write(src.join("b"), b"b").unwrap();
        assert!(XySend::new(XyProtocol::Xmodem, vec![src.join("a"), src.join("b")]).is_err());
        let mut rx = XyReceive::new(XyProtocol::XmodemCrc, temp_dir("cancel"), None);
        let mut out = Vec::new();
        rx.c.input(&[CAN, CAN, CAN]);
        rx.pump(&mut out);
        assert!(matches!(rx.c.outcome, Some(Outcome::Failed(_))));
    }
}
