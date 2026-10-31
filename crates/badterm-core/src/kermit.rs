//! Kermit file transfers, as C-Kermit, G-Kermit and Kermit-95 speak them,
//! in both directions.
//!
//! Packets are `SOH LEN SEQ TYPE DATA CHECK CR`, every field but the mark
//! made printable (`tochar(x) = x + 32`). The two sides trade their
//! parameters in the Send-Init (`S`) packet and its acknowledgement; this
//! side offers long packets (up to [`MAX_LONG`] bytes), CRC-16 block checks
//! (type 3), `~` run-length compression and file attributes (for the size),
//! and takes whatever the other side can do. No sliding windows: each
//! packet waits for its ACK, which is fine on a telnet or SSH link.
//!
//! A download starts by itself when the BBS's Kermit sends its `S` packet
//! (see [`find_send_init`]); an upload is started by the user once the
//! BBS's Kermit is receiving.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::transfer::{self, Common, Direction, Outcome, Progress};

const SOH: u8 = 0x01;
const CR: u8 = 0x0D;
/// The longest packet offered (its data and check).
pub const MAX_LONG: usize = 4000;
/// How long to wait for a packet before resending (or NAKing).
const TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RETRIES: u32 = 10;
/// A receiver waiting for the sender's Send-Init gives up after this many
/// timeouts.
const START_TRIES: u32 = 15;

/// Capability bits.
const CAP_LONG: u8 = 2;
const CAP_ATTRS: u8 = 8;

fn tochar(x: u8) -> u8 {
    x + 32
}

fn unchar(c: u8) -> u8 {
    c.wrapping_sub(32)
}

fn ctl(c: u8) -> u8 {
    c ^ 64
}

/// The Kermit block checks: 1 (6-bit sum), 2 (12-bit sum), 3 (CRC-16).
fn block_check(kind: u8, data: &[u8]) -> Vec<u8> {
    let sum: u32 = data.iter().map(|&b| b as u32).sum();
    match kind {
        2 => vec![tochar(((sum >> 6) & 63) as u8), tochar((sum & 63) as u8)],
        3 => {
            let crc = crc16_kermit(data);
            vec![tochar(((crc >> 12) & 0x0F) as u8), tochar(((crc >> 6) & 0x3F) as u8), tochar((crc & 0x3F) as u8)]
        }
        _ => vec![tochar(((sum + ((sum & 192) >> 6)) & 63) as u8)],
    }
}

/// CRC-16/KERMIT: CCITT, reflected (0x8408), starting at 0.
pub fn crc16_kermit(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0x8408 } else { crc >> 1 };
        }
    }
    crc
}

/// Where a Send-Init packet starts in `data`, if there is one: what a BBS's
/// Kermit sends when told to send a file.
pub fn find_send_init(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w[0] == SOH && (b' '..=b'~').contains(&w[1]) && w[2] == b' ' && w[3] == b'S')
}

/// The parameters one side announces in its Send-Init.
#[derive(Clone, Debug)]
struct Params {
    /// The longest packet it can take, short (up to 94) and long.
    maxl: usize,
    maxlx: Option<usize>,
    /// Seconds before it wants to be timed out.
    time: u8,
    npad: u8,
    padc: u8,
    eol: u8,
    qctl: u8,
    /// 'Y' (will if asked), 'N' (won't), or the prefix it wants.
    qbin: u8,
    chkt: u8,
    /// The repeat prefix, or ' ' for none.
    rept: u8,
    capas: u8,
}

impl Params {
    fn ours() -> Params {
        Params {
            maxl: 94,
            maxlx: Some(MAX_LONG),
            time: TIMEOUT.as_secs() as u8,
            npad: 0,
            padc: 0,
            eol: CR,
            qctl: b'#',
            qbin: b'Y',
            chkt: 3,
            rept: b'~',
            capas: CAP_LONG | CAP_ATTRS,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let maxlx = self.maxlx.unwrap_or(0);
        vec![
            tochar(self.maxl as u8),
            tochar(self.time),
            tochar(self.npad),
            ctl(self.padc),
            tochar(self.eol),
            self.qctl,
            self.qbin,
            b'0' + self.chkt,
            self.rept,
            tochar(self.capas),
            tochar(1),
            tochar((maxlx / 95) as u8),
            tochar((maxlx % 95) as u8),
        ]
    }

    /// Read the other side's; fields it leaves out take their defaults.
    fn decode(d: &[u8]) -> Params {
        let at = |i: usize| d.get(i).copied();
        let mut p = Params {
            maxl: at(0).map_or(80, |c| unchar(c).clamp(10, 94) as usize),
            maxlx: None,
            time: at(1).map_or(5, unchar),
            npad: at(2).map_or(0, unchar),
            padc: at(3).map_or(0, ctl),
            eol: at(4).map(unchar).filter(|&c| c > 0).unwrap_or(CR),
            qctl: at(5).filter(|&c| c != b' ').unwrap_or(b'#'),
            qbin: at(6).filter(|&c| c != b' ').unwrap_or(b'N'),
            chkt: at(7).map(|c| c.wrapping_sub(b'0')).filter(|c| (1..=3).contains(c)).unwrap_or(1),
            rept: at(8).unwrap_or(b' '),
            capas: 0,
        };
        // Capability bytes go on while their low bit is set.
        let mut i = 9;
        while let Some(c) = at(i) {
            if i == 9 {
                p.capas = unchar(c);
            }
            i += 1;
            if unchar(c) & 1 == 0 {
                break;
            }
        }
        // Then the window size, and the long packet length.
        if p.capas & CAP_LONG != 0 {
            let len = match (at(i + 1), at(i + 2)) {
                (Some(a), Some(b)) => unchar(a) as usize * 95 + unchar(b) as usize,
                _ => 500,
            };
            p.maxlx = Some(if len == 0 { 500 } else { len });
        }
        p
    }
}

/// What both sides agreed on, from one side's point of view.
#[derive(Clone, Debug)]
struct Link {
    /// The longest packet the other side takes from us.
    send_max: usize,
    long: bool,
    chkt: u8,
    /// The prefixes: ours for sending, theirs for reading what they send.
    our_qctl: u8,
    their_qctl: u8,
    qbin: Option<u8>,
    rept: Option<u8>,
    attrs: bool,
    eol: u8,
    npad: u8,
    padc: u8,
}

impl Link {
    /// Before the Send-Init exchange: the basics.
    fn initial() -> Link {
        Link {
            send_max: 80,
            long: false,
            chkt: 1,
            our_qctl: b'#',
            their_qctl: b'#',
            qbin: None,
            rept: None,
            attrs: false,
            eol: CR,
            npad: 0,
            padc: 0,
        }
    }

    fn agree(ours: &Params, theirs: &Params) -> Link {
        let long = ours.capas & theirs.capas & CAP_LONG != 0;
        let qbin = match (ours.qbin, theirs.qbin) {
            (b'Y', c) | (c, b'Y') if c != b'Y' && c != b'N' && is_prefix(c) => Some(c),
            (a, b) if a == b && is_prefix(a) => Some(a),
            _ => None,
        };
        let rept = (ours.rept == theirs.rept && is_prefix(ours.rept)).then_some(ours.rept);
        Link {
            send_max: if long { theirs.maxlx.unwrap_or(500).min(MAX_LONG) } else { theirs.maxl },
            long,
            chkt: if ours.chkt == theirs.chkt { ours.chkt } else { 1 },
            our_qctl: ours.qctl,
            their_qctl: theirs.qctl,
            qbin,
            rept,
            attrs: ours.capas & theirs.capas & CAP_ATTRS != 0,
            eol: theirs.eol,
            npad: theirs.npad,
            padc: theirs.padc,
        }
    }

    /// A packet on the wire. Send-Init packets and their ACKs are always
    /// checked with type 1.
    fn packet(&self, seq: u8, kind: u8, data: &[u8], chkt: u8) -> Vec<u8> {
        let chk_len = chkt as usize;
        let mut body = Vec::with_capacity(data.len() + 10);
        let short_len = data.len() + 2 + chk_len;
        if short_len <= 94 {
            body.push(tochar(short_len as u8));
            body.push(tochar(seq));
            body.push(kind);
        } else {
            let ext = data.len() + chk_len;
            body.extend_from_slice(&[tochar(0), tochar(seq), kind, tochar((ext / 95) as u8), tochar((ext % 95) as u8)]);
            body.extend(block_check(1, &body));
        }
        body.extend_from_slice(data);
        let chk = block_check(chkt, &body);
        let mut out = vec![self.padc; self.npad as usize];
        out.push(SOH);
        out.extend(body);
        out.extend(chk);
        out.push(self.eol);
        out
    }

    /// How much data fits in one packet for the other side.
    fn room(&self) -> usize {
        if self.long { self.send_max - self.chkt as usize - 7 } else { self.send_max - 2 - self.chkt as usize }
    }

    /// Encode as much of `src` as fits in `room` bytes; returns the encoded
    /// data and how many bytes of `src` it holds.
    fn encode(&self, src: &[u8], room: usize) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(room);
        let mut i = 0;
        while i < src.len() {
            let c = src[i];
            let mut run = 1;
            if self.rept.is_some() {
                while i + run < src.len() && src[i + run] == c && run < 94 {
                    run += 1;
                }
            }
            let one = self.encode_byte(c);
            let unit = match self.rept {
                Some(r) if run >= 3 => {
                    let mut u = vec![r, tochar(run as u8)];
                    u.extend_from_slice(&one);
                    u
                }
                _ => {
                    run = 1;
                    one
                }
            };
            if out.len() + unit.len() > room {
                break;
            }
            out.extend(unit);
            i += run;
        }
        (out, i)
    }

    fn encode_byte(&self, mut c: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(3);
        if let Some(q) = self.qbin
            && c & 0x80 != 0
        {
            out.push(q);
            c &= 0x7F;
        }
        let low = c & 0x7F;
        if low < 32 || low == 127 {
            out.extend_from_slice(&[self.our_qctl, ctl(c)]);
        } else if low == self.our_qctl || Some(low) == self.qbin || Some(low) == self.rept {
            out.extend_from_slice(&[self.our_qctl, c]);
        } else {
            out.push(c);
        }
        out
    }

    /// Decode a packet's data from the other side.
    fn decode(&self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        while i < data.len() {
            let mut count = 1;
            if let Some(r) = self.rept
                && data[i] == r
                && i + 1 < data.len()
            {
                count = unchar(data[i + 1]) as usize;
                i += 2;
            }
            let mut high = 0;
            if let Some(q) = self.qbin
                && data.get(i) == Some(&q)
            {
                high = 0x80;
                i += 1;
            }
            let Some(&b) = data.get(i) else { break };
            i += 1;
            let c = if b == self.their_qctl {
                let Some(&q) = data.get(i) else { break };
                i += 1;
                if (0x3F..=0x5F).contains(&(q & 0x7F)) { ctl(q) } else { q }
            } else {
                b
            };
            out.extend(std::iter::repeat_n(c | high, count));
        }
        out
    }
}

/// A prefix character Kermit allows (`!` to `>`, or `` ` `` to `~`).
fn is_prefix(c: u8) -> bool {
    (33..=62).contains(&c) || (96..=126).contains(&c)
}

/// A packet read from the other side.
#[derive(Debug, PartialEq, Eq)]
struct Packet {
    seq: u8,
    kind: u8,
    data: Vec<u8>,
}

/// Take the next packet from `buf`: Ok(None) until a whole one is there,
/// Err with its sequence number for a damaged one. Noise before it goes.
fn read_packet(buf: &mut Vec<u8>, chkt: u8) -> std::result::Result<Option<Packet>, Option<u8>> {
    let Some(start) = buf.iter().position(|&b| b == SOH) else {
        buf.clear();
        return Ok(None);
    };
    buf.drain(..start);
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = unchar(buf[1]) as usize;
    let (seq, kind) = (unchar(buf[2]), buf[3]);
    // Send-Init packets and ACKs to them are type 1.
    let chk_len = if kind == b'S' { 1 } else { chkt as usize };
    let (header, total) = if len == 0 {
        if buf.len() < 7 {
            return Ok(None);
        }
        if block_check(1, &buf[1..6]) != buf[6..7] {
            buf.drain(..1);
            return Err(Some(seq & 63));
        }
        (7, 7 + unchar(buf[4]) as usize * 95 + unchar(buf[5]) as usize)
    } else if len < 3 {
        buf.drain(..1);
        return Err(None);
    } else {
        (4, 2 + len)
    };
    if total > MAX_LONG + 16 || total < header + chk_len {
        buf.drain(..1);
        return Err(None);
    }
    if buf.len() < total {
        return Ok(None);
    }
    let (body, chk) = buf[1..total].split_at(total - 1 - chk_len);
    if block_check(chk_len as u8, body) != chk {
        // Only its mark goes: a good packet may follow inside it.
        buf.drain(..1);
        return Err(Some(seq & 63));
    }
    let raw: Vec<u8> = buf.drain(..total).collect();
    Ok(Some(Packet { seq: seq & 63, kind, data: raw[header..total - chk_len].to_vec() }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Receiving: waiting for the Send-Init. Sending: waiting for its ACK.
    Init,
    /// Receiving: waiting for a file header or the end of the batch.
    File,
    /// Receiving: the file's attributes or data. Sending: data under way.
    Data,
    /// Sending: the header (F), attributes (A), end of file (Z), or end of
    /// batch (B) is out, waiting for its ACK.
    Header,
    Attrs,
    Eof,
    Break,
    Done,
}

enum Role {
    Receive { dir: PathBuf, file: Option<(BufWriter<File>, PathBuf)>, saved: Vec<PathBuf> },
    Send { queue: VecDeque<PathBuf>, file: Option<(File, PathBuf)>, sent: Vec<PathBuf>, eof: bool, skip: bool },
}

/// A Kermit transfer, either way.
pub struct Kermit {
    role: Role,
    state: State,
    link: Link,
    seq: u8,
    /// The last packet sent, for resending.
    last: Vec<u8>,
    /// Read but not yet sent (sending), as packets go out.
    pending_data: Vec<u8>,
    retries: u32,
    last_heard: Instant,
    pub(crate) c: Common,
}

impl Kermit {
    /// Receive files into `dir`. Feed it the BBS's Send-Init packet (or
    /// wait for it).
    pub fn receive(dir: PathBuf) -> Kermit {
        Kermit::new(Role::Receive { dir, file: None, saved: Vec::new() }, Direction::Download)
    }

    /// Send files to a Kermit that is ready to receive.
    pub fn send(paths: Vec<PathBuf>) -> Result<Kermit> {
        if paths.is_empty() {
            bail!("no files to upload");
        }
        for p in &paths {
            if !p.is_file() {
                bail!("not a file: {}", p.display());
            }
        }
        let mut k = Kermit::new(
            Role::Send { queue: paths.into(), file: None, sent: Vec::new(), eof: false, skip: false },
            Direction::Upload,
        );
        k.c.progress.files_queued = k.queued();
        Ok(k)
    }

    fn new(role: Role, direction: Direction) -> Kermit {
        Kermit {
            role,
            state: State::Init,
            link: Link::initial(),
            seq: 0,
            last: Vec::new(),
            pending_data: Vec::new(),
            retries: 0,
            last_heard: Instant::now(),
            c: Common::new(direction),
        }
    }

    pub(crate) fn progress(&self) -> &Progress {
        &self.c.progress
    }

    pub fn enqueue(&mut self, path: PathBuf) -> Result<()> {
        match &mut self.role {
            Role::Send { queue, .. } => {
                queue.push_back(path);
                self.c.progress.files_queued = self.queued();
                Ok(())
            }
            Role::Receive { .. } => bail!("a download is in progress"),
        }
    }

    fn queued(&self) -> usize {
        match &self.role {
            Role::Send { queue, .. } => queue.len(),
            Role::Receive { .. } => 0,
        }
    }

    fn sending(&self) -> bool {
        matches!(self.role, Role::Send { .. })
    }

    /// The check type in use for the packets of the current state.
    fn chkt(&self) -> u8 {
        if self.state == State::Init { 1 } else { self.link.chkt }
    }

    fn put(&mut self, kind: u8, data: &[u8], out: &mut Vec<u8>) {
        let chkt = if kind == b'S' || self.state == State::Init { 1 } else { self.link.chkt };
        self.last = self.link.packet(self.seq, kind, data, chkt);
        out.extend_from_slice(&self.last);
        self.retries = 0;
        self.last_heard = Instant::now();
    }

    /// Acknowledge packet `seq` (receiving).
    fn ack(&mut self, seq: u8, data: &[u8], out: &mut Vec<u8>) {
        self.seq = seq;
        self.put(b'Y', data, out);
    }

    fn fail(&mut self, msg: impl Into<String>, out: &mut Vec<u8>) {
        if self.c.outcome.is_some() {
            return;
        }
        let msg = msg.into();
        let data = self.link.encode(msg.as_bytes(), 80).0;
        let p = self.link.packet(self.seq, b'E', &data, self.chkt());
        out.extend_from_slice(&p);
        self.drop_partial();
        self.c.outcome = Some(Outcome::Failed(msg));
    }

    fn drop_partial(&mut self) {
        if let Role::Receive { file, .. } = &mut self.role
            && let Some((_, path)) = file.take()
        {
            let _ = fs::remove_file(path);
        }
    }

    pub(crate) fn cancel(&mut self, out: &mut Vec<u8>) {
        self.c.cancelled = true;
        if self.c.outcome.is_none() {
            let p = self.link.packet(self.seq, b'E', b"Cancelled by the user", self.chkt());
            out.extend_from_slice(&p);
        }
        self.drop_partial();
        self.c.outcome.get_or_insert(Outcome::Cancelled);
    }

    /// The sender starts by itself: call once, before the first feed.
    pub fn start(&mut self, out: &mut Vec<u8>) {
        if self.sending() && self.state == State::Init && self.last.is_empty() {
            let data = Params::ours().encode();
            self.put(b'S', &data, out);
        }
    }

    pub(crate) fn tick(&mut self, now: Instant, out: &mut Vec<u8>) {
        if self.c.outcome.is_some() || self.state == State::Done {
            return;
        }
        if now.duration_since(self.last_heard) < TIMEOUT {
            return;
        }
        self.last_heard = now;
        self.retries += 1;
        let limit = if !self.sending() && self.state == State::Init { START_TRIES } else { MAX_RETRIES };
        if self.retries > limit {
            return self.fail("the other side stopped answering", out);
        }
        if self.sending() || !self.last.is_empty() {
            out.extend_from_slice(&self.last);
        } else {
            // A receiver that has heard nothing NAKs packet 0.
            let p = self.link.packet(0, b'N', &[], 1);
            out.extend_from_slice(&p);
        }
    }

    pub(crate) fn pump(&mut self, out: &mut Vec<u8>) {
        while self.c.outcome.is_none() && !self.c.pending.is_empty() {
            let chkt = self.chkt();
            match read_packet(&mut self.c.pending, chkt) {
                Ok(None) => break,
                Ok(Some(p)) => {
                    self.last_heard = Instant::now();
                    if p.kind == b'E' {
                        let msg = String::from_utf8_lossy(&self.link.decode(&p.data)).into_owned();
                        self.drop_partial();
                        self.c.outcome = Some(Outcome::Failed(format!("the other side stopped: {msg}")));
                        break;
                    }
                    if self.sending() { self.sender_got(p, out) } else { self.receiver_got(p, out) }
                }
                Err(seq) => {
                    // Damaged: ask again (receiving), or resend (sending).
                    if self.sending() {
                        out.extend_from_slice(&self.last);
                    } else {
                        let p = self.link.packet(seq.unwrap_or(self.seq.wrapping_add(1) & 63), b'N', &[], chkt);
                        out.extend_from_slice(&p);
                    }
                }
            }
        }
    }

    fn receiver_got(&mut self, p: Packet, out: &mut Vec<u8>) {
        let expected = if self.state == State::Init { 0 } else { (self.seq + 1) & 63 };
        if p.seq != expected {
            // A repeat of the one just acknowledged: acknowledge it again.
            if p.seq == self.seq && !self.last.is_empty() {
                out.extend_from_slice(&self.last);
            }
            return;
        }
        match (self.state, p.kind) {
            (State::Init, b'S') => {
                let theirs = Params::decode(&p.data);
                let ours = Params::ours();
                self.ack(p.seq, &ours.encode(), out);
                self.link = Link::agree(&ours, &theirs);
                self.state = State::File;
            }
            (State::File, b'F') => {
                let wire = self.link.decode(&p.data);
                let name = transfer::sanitize_file_name(&wire);
                match self.open(&name) {
                    Ok(path) => {
                        let used = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name);
                        let data = self.link.encode(used.as_bytes(), self.link.room()).0;
                        self.ack(p.seq, &data, out);
                        self.state = State::Data;
                    }
                    Err(e) => self.fail(format!("saving {name}: {e}"), out),
                }
            }
            // A text header (`X`), to show rather than save: not shown.
            (State::File, b'X') => self.ack(p.seq, &[], out),
            (State::File, b'B') => {
                self.ack(p.seq, &[], out);
                self.state = State::Done;
                if let Role::Receive { saved, .. } = &self.role {
                    self.c.outcome = Some(Outcome::Completed(saved.clone()));
                }
            }
            (State::Data, b'A') => {
                self.c.progress.file_size = attribute_size(&p.data);
                self.ack(p.seq, b"Y", out);
            }
            (State::Data, b'D') => {
                let data = self.link.decode(&p.data);
                if let Role::Receive { file: Some((f, path)), .. } = &mut self.role
                    && let Err(e) = f.write_all(&data)
                {
                    let msg = format!("writing {}: {e}", path.display());
                    return self.fail(msg, out);
                }
                self.c.progress.file_bytes += data.len() as u64;
                self.ack(p.seq, &[], out);
            }
            (State::Data, b'Z') => {
                // "D" in the data: the sender discarded the file.
                let discard = self.link.decode(&p.data).first() == Some(&b'D');
                if let Role::Receive { file, saved, .. } = &mut self.role
                    && let Some((mut f, path)) = file.take()
                {
                    if discard {
                        drop(f);
                        let _ = fs::remove_file(&path);
                    } else if let Err(e) = f.flush() {
                        let msg = format!("writing {}: {e}", path.display());
                        return self.fail(msg, out);
                    } else {
                        saved.push(path);
                        self.c.progress.files_done += 1;
                    }
                }
                self.ack(p.seq, &[], out);
                self.state = State::File;
            }
            (_, kind) => self.fail(format!("unexpected Kermit packet '{}'", kind as char), out),
        }
    }

    fn open(&mut self, name: &str) -> std::io::Result<PathBuf> {
        let Role::Receive { dir, file, .. } = &mut self.role else { unreachable!("receiving") };
        fs::create_dir_all(&*dir)?;
        let path = transfer::unique_path(dir, name)?;
        *file = Some((BufWriter::new(File::create(&path)?), path.clone()));
        let p = &mut self.c.progress;
        p.file_name = name.to_string();
        p.file_size = None;
        p.file_bytes = 0;
        p.file_started = Instant::now();
        Ok(path)
    }

    fn sender_got(&mut self, p: Packet, out: &mut Vec<u8>) {
        // A NAK for the next packet acknowledges this one.
        let acked = match p.kind {
            b'Y' => p.seq == self.seq,
            b'N' => p.seq == (self.seq + 1) & 63,
            _ => false,
        };
        if !acked {
            if p.kind == b'N' && p.seq == self.seq {
                self.retries += 1;
                if self.retries > MAX_RETRIES {
                    return self.fail("too many retries", out);
                }
                out.extend_from_slice(&self.last);
            }
            return;
        }
        let reply = if p.kind == b'Y' { p.data } else { Vec::new() };
        self.seq = (self.seq + 1) & 63;
        match self.state {
            State::Init => {
                self.link = Link::agree(&Params::ours(), &Params::decode(&reply));
                self.next_file(out);
            }
            State::Header if self.link.attrs => {
                let size = self.file_size();
                let digits = size.to_string();
                let mut data = vec![b'1', tochar(digits.len() as u8)];
                data.extend_from_slice(digits.as_bytes());
                self.state = State::Attrs;
                self.put(b'A', &data, out);
            }
            State::Header => self.start_data(out),
            State::Attrs => {
                // "N" in the reply: the receiver does not want the file.
                if reply.first() == Some(&b'N') {
                    self.skip_file(out);
                } else {
                    self.start_data(out);
                }
            }
            State::Data => {
                // "X" stops this file, "Z" the whole batch.
                match reply.first() {
                    Some(b'X') => self.skip_file(out),
                    Some(b'Z') => {
                        if let Role::Send { queue, .. } = &mut self.role {
                            queue.clear();
                        }
                        self.skip_file(out);
                    }
                    _ => self.send_data(out),
                }
            }
            State::Eof => {
                if let Role::Send { file, sent, skip, .. } = &mut self.role
                    && let Some((_, path)) = file.take()
                {
                    if !*skip {
                        sent.push(path);
                        self.c.progress.files_done += 1;
                    }
                    *skip = false;
                }
                self.next_file(out);
            }
            State::Break => {
                self.state = State::Done;
                if let Role::Send { sent, .. } = &self.role {
                    self.c.outcome = Some(Outcome::Completed(sent.clone()));
                }
            }
            State::File | State::Done => {}
        }
    }

    fn file_size(&self) -> u64 {
        match &self.role {
            Role::Send { file: Some((f, _)), .. } => f.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        }
    }

    /// Open the next file and send its header, or end the batch.
    fn next_file(&mut self, out: &mut Vec<u8>) {
        let Role::Send { queue, file, eof, .. } = &mut self.role else { return };
        let Some(path) = queue.pop_front() else {
            self.state = State::Break;
            return self.put(b'B', &[], out);
        };
        let f = match File::open(&path) {
            Ok(f) => f,
            Err(e) => return self.fail(format!("reading {}: {e}", path.display()), out),
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let size = f.metadata().map(|m| m.len()).ok();
        *file = Some((f, path));
        *eof = false;
        self.pending_data.clear();
        let p = &mut self.c.progress;
        p.file_name = name.clone();
        p.file_size = size;
        p.file_bytes = 0;
        p.file_started = Instant::now();
        p.files_queued = queue.len();
        let data = self.link.encode(name.as_bytes(), self.link.room()).0;
        self.state = State::Header;
        self.put(b'F', &data, out);
    }

    fn start_data(&mut self, out: &mut Vec<u8>) {
        self.state = State::Data;
        self.send_data(out);
    }

    /// Send the next data packet, or the end of the file.
    fn send_data(&mut self, out: &mut Vec<u8>) {
        let room = self.link.room();
        // Keep enough read ahead to fill a packet even with no compression.
        if let Role::Send { file: Some((f, path)), eof, .. } = &mut self.role
            && !*eof
            && self.pending_data.len() < room * 2
        {
            let mut buf = vec![0u8; room * 4];
            match f.read(&mut buf) {
                Ok(0) => *eof = true,
                Ok(n) => self.pending_data.extend_from_slice(&buf[..n]),
                Err(e) => {
                    let msg = format!("reading {}: {e}", path.display());
                    return self.fail(msg, out);
                }
            }
        }
        if self.pending_data.is_empty() {
            let at_end = matches!(self.role, Role::Send { eof: true, .. });
            if !at_end {
                // The read above got nothing yet but the file isn't done.
                return self.send_data(out);
            }
            self.state = State::Eof;
            return self.put(b'Z', &[], out);
        }
        let (data, used) = self.link.encode(&self.pending_data, room);
        self.pending_data.drain(..used);
        self.c.progress.file_bytes += used as u64;
        self.put(b'D', &data, out);
    }

    /// Stop sending this file: an end of file that says "discard".
    fn skip_file(&mut self, out: &mut Vec<u8>) {
        if let Role::Send { skip, .. } = &mut self.role {
            *skip = true;
        }
        self.pending_data.clear();
        self.state = State::Eof;
        self.put(b'Z', b"D", out);
    }
}

/// The file size from an attribute packet: `1` (bytes) or `!` (KiB).
fn attribute_size(data: &[u8]) -> Option<u64> {
    let mut i = 0;
    let mut size = None;
    while i + 1 < data.len() {
        let (tag, len) = (data[i], unchar(data[i + 1]) as usize);
        let value = data.get(i + 2..i + 2 + len)?;
        let n = std::str::from_utf8(value).ok().and_then(|s| s.trim().parse::<u64>().ok());
        match tag {
            b'1' => size = n.or(size),
            b'!' if size.is_none() => size = n.map(|k| k * 1024),
            _ => {}
        }
        i += 2 + len;
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("badterm-kermit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Run a sender against a receiver in memory, optionally damaging
    /// every `damage`-th chunk on the way to the receiver.
    fn run(tx: &mut Kermit, rx: &mut Kermit, damage: Option<usize>) {
        let (mut to_rx, mut to_tx) = (Vec::new(), Vec::new());
        tx.start(&mut to_rx);
        for n in 0..200_000 {
            if tx.c.outcome.is_some() && rx.c.outcome.is_some() {
                return;
            }
            let mut data = std::mem::take(&mut to_rx);
            if damage.is_some_and(|d| n % d == d - 1) && data.len() > 8 {
                let i = data.len() / 2;
                data[i] ^= 0x01;
            }
            rx.c.input(&data);
            rx.pump(&mut to_tx);
            let data = std::mem::take(&mut to_tx);
            tx.c.input(&data);
            tx.pump(&mut to_rx);
            if to_rx.is_empty() && to_tx.is_empty() {
                // Lost to damage: what a timeout would do.
                tx.last_heard = Instant::now() - TIMEOUT;
                tx.tick(Instant::now(), &mut to_rx);
            }
        }
        panic!("transfer did not finish");
    }

    #[test]
    fn checks_and_prefixes() {
        assert_eq!(crc16_kermit(b"123456789"), 0x2189);
        let link = Link { qbin: Some(b'&'), rept: Some(b'~'), ..Link::agree(&Params::ours(), &Params::ours()) };
        let src = b"ab\x01#\x80&~cccccc\xff\r\n".to_vec();
        let (enc, used) = link.encode(&src, 1000);
        assert_eq!(used, src.len());
        assert!(enc.iter().all(|&b| (32..127).contains(&b)), "{enc:?}");
        assert_eq!(link.decode(&enc), src);
        // A short room ends on a whole unit.
        let (part, n) = link.encode(&src, 5);
        assert_eq!(link.decode(&part), src[..n]);
        assert_eq!(attribute_size(b"#(20251031!\"12"), Some(12 * 1024));
        assert_eq!(attribute_size(b"1#300!\"12"), Some(300));
        assert_eq!(find_send_init(b"junk\x01) S~"), Some(4));
    }

    #[test]
    fn round_trip_two_files() {
        let (src, dst) = (temp_dir("src"), temp_dir("dst"));
        let mut big = Vec::new();
        for i in 0..150_000u32 {
            big.push((i.wrapping_mul(2_654_435_761) >> 13) as u8);
        }
        big.extend_from_slice(&[0; 500]);
        fs::write(src.join("big.bin"), &big).unwrap();
        fs::write(src.join("empty.txt"), b"").unwrap();
        let mut tx = Kermit::send(vec![src.join("big.bin"), src.join("empty.txt")]).unwrap();
        let mut rx = Kermit::receive(dst.clone());
        run(&mut tx, &mut rx, None);
        assert_eq!(tx.c.outcome, Some(Outcome::Completed(vec![src.join("big.bin"), src.join("empty.txt")])));
        assert_eq!(rx.c.outcome, Some(Outcome::Completed(vec![dst.join("big.bin"), dst.join("empty.txt")])));
        assert_eq!(fs::read(dst.join("big.bin")).unwrap(), big);
        assert_eq!(fs::read(dst.join("empty.txt")).unwrap(), b"");
        // Long packets with CRCs were agreed.
        assert!(tx.link.long && tx.link.chkt == 3 && tx.link.rept == Some(b'~'));
    }

    #[test]
    fn damaged_packets_are_resent() {
        let (src, dst) = (temp_dir("dsrc"), temp_dir("ddst"));
        let data: Vec<u8> = (0..40_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(src.join("a.dat"), &data).unwrap();
        let mut tx = Kermit::send(vec![src.join("a.dat")]).unwrap();
        let mut rx = Kermit::receive(dst.clone());
        run(&mut tx, &mut rx, Some(7));
        assert!(matches!(rx.c.outcome, Some(Outcome::Completed(_))), "{:?}", rx.c.outcome);
        assert_eq!(fs::read(dst.join("a.dat")).unwrap(), data);
    }

    #[test]
    fn cancel_sends_an_error_packet() {
        let mut rx = Kermit::receive(temp_dir("cancel"));
        let mut out = Vec::new();
        rx.cancel(&mut out);
        assert_eq!(out[0], SOH);
        assert_eq!(out[3], b'E');
        assert_eq!(rx.c.outcome, Some(Outcome::Cancelled));
    }
}
