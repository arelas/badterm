//! SyncTERM's audio APCs (`ESC _ SyncTERM:A;... ESC \`), as CTerm
//! documents them: 256 slots of sound (16-bit stereo at 44.1 kHz) filled
//! from files in the cache or sent inline (WAV, AIFF, VOC and Ogg Opus are
//! decoded here), or synthesized; and 16 channels the BBS queues them on,
//! with fades, loops, crossfades and volume. Synchronet's doors play their
//! sound effects and music this way.
//!
//! The mixing is the front end's: queued sound goes out as [`AudioOp`]s.
//! What the BBS asks about (is a channel playing?) is answered from the
//! lengths of what was queued.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The sample rate of slots and channels.
pub const RATE: u32 = 44_100;
pub const SLOTS: usize = 256;
pub const CHANNELS: usize = 16;
/// The first channel the BBS may queue sound on; 0 is ANSI music, 1
/// sound effects (RIP), which are the terminal's own.
pub const FIRST_APC_CHANNEL: usize = 2;
/// APC channels start at -12 dB.
pub const BASE_DB: f32 = -12.0;
/// Longer sounds are cut off (ten minutes).
const MAX_FRAMES: usize = RATE as usize * 600;

/// Interleaved stereo frames (left, right) at [`RATE`].
pub type Pcm = Arc<[i16]>;

/// A sound in a slot or on a channel. Its length is known at once; its
/// frames once they're decoded, which for the slow formats (Opus, Vorbis,
/// FLAC) happens on a thread of its own, so the screen doesn't wait.
#[derive(Clone)]
pub struct Sound(Arc<SoundData>);

struct SoundData {
    frames: usize,
    pcm: std::sync::OnceLock<Pcm>,
}

impl Sound {
    pub fn ready(pcm: Pcm) -> Sound {
        let data = SoundData { frames: pcm.len() / 2, pcm: std::sync::OnceLock::new() };
        let _ = data.pcm.set(pcm);
        Sound(Arc::new(data))
    }

    /// A sound `frames` long, decoded by `decode` on another thread (to
    /// nothing, if that fails).
    fn decoded_later(frames: usize, decode: impl FnOnce() -> Option<Vec<i16>> + Send + 'static) -> Sound {
        let sound = Sound(Arc::new(SoundData { frames, pcm: std::sync::OnceLock::new() }));
        let fill = sound.0.clone();
        let spawned = std::thread::Builder::new().name("decode".into()).spawn(move || {
            let _ = fill.pcm.set(decode().unwrap_or_default().into());
        });
        if let Err(e) = spawned {
            log::warn!("decoding a sound: {e}");
            let _ = sound.0.pcm.set(Vec::new().into());
        }
        sound
    }

    /// Its length in frames (once decoded, the true one).
    pub fn frames(&self) -> usize {
        self.pcm().map_or(self.0.frames, |p| p.len() / 2)
    }

    /// Its frames, once decoded.
    pub fn pcm(&self) -> Option<&Pcm> {
        self.0.pcm.get()
    }

    /// Wait for it to be decoded (for tests and tools).
    pub fn wait(&self) -> &Pcm {
        loop {
            if let Some(p) = self.pcm() {
                return p;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl PartialEq for Sound {
    fn eq(&self, other: &Sound) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Sound({} frames{})", self.0.frames, if self.pcm().is_some() { "" } else { ", decoding" })
    }
}

/// What the front end's mixer is to do.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioOp {
    /// Play `sound` on `channel` after what is queued there (or, with
    /// `crossfade`, fade out what is playing while this fades in over
    /// `fade_in` frames). `volume` is in dB, added to the channel's.
    Queue {
        channel: usize,
        sound: Sound,
        fade_in: usize,
        fade_out: usize,
        crossfade: bool,
        looped: bool,
        volume: (f32, f32),
    },
    /// Stop a channel: at once, or fading out over `fade_out` frames.
    Flush { channel: usize, fade_out: usize },
    /// Set a channel's level (dB) now, or over `ramp` frames.
    Volume { channel: usize, left: Option<f32>, right: Option<f32>, ramp: usize },
}

/// When a channel will have played all that is queued.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Busy {
    #[default]
    Idle,
    Until(Instant),
    Looping,
}

pub struct Audio {
    slots: Vec<Option<Sound>>,
    busy: [Busy; CHANNELS],
    /// Channels to report on when they stop (`Update`).
    notify: [bool; CHANNELS],
}

impl Default for Audio {
    fn default() -> Audio {
        Audio { slots: vec![None; SLOTS], busy: [Busy::Idle; CHANNELS], notify: [false; CHANNELS] }
    }
}

/// A duration: milliseconds (`500`, `500ms`), frames (`22050f`), or with
/// `hz`, whole periods of it (`3p`). Anything else is 0.
fn duration(s: &str, hz: Option<f64>) -> usize {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let Ok(n) = s[..digits].parse::<u64>() else { return 0 };
    match &s[digits..] {
        "" | "ms" => (n * RATE as u64 / 1000) as usize,
        "f" => n as usize,
        "p" => hz.filter(|&f| f > 0.0).map_or(0, |f| (n as f64 * RATE as f64 / f + 0.5) as usize),
        _ => 0,
    }
}

/// A volume in dB: `-6dB`, or a percentage (0 is -60 dB).
fn volume(s: &str) -> f32 {
    let lower = s.to_ascii_lowercase();
    if let Some(db) = lower.strip_suffix("db") {
        return db.trim().parse().unwrap_or(0.0);
    }
    match s.trim().parse::<f32>() {
        Ok(v) if v <= 0.0 => -60.0,
        Ok(v) => 20.0 * (v.min(100.0) / 100.0).log10(),
        Err(_) => 0.0,
    }
}

/// `KEY=value` among an APC's fields.
fn field<'a>(fields: &[&'a str], key: &str) -> Option<&'a str> {
    fields.iter().find_map(|f| f.strip_prefix(key)?.strip_prefix('='))
}

fn number(fields: &[&str], key: &str) -> Option<usize> {
    field(fields, key)?.trim().parse().ok()
}

impl Audio {
    /// An `A;...` APC: `fields` after `SyncTERM:A;`. Sound to play goes
    /// in `ops`; `hold` is set for `Wait` to how long to stop reading.
    pub fn apc(&mut self, fields: &[&str], cache: Option<&PathBuf>, ops: &mut Vec<AudioOp>) -> Option<Instant> {
        let (&verb, rest) = fields.split_first()?;
        let slot = number(rest, "S").filter(|&s| s < SLOTS);
        let channel = number(rest, "C").filter(|&c| c < CHANNELS);
        match verb {
            "Load" | "LoadBlob" => {
                // The fields after the S=: a file name, or the data.
                let data = rest.iter().rfind(|f| !f.starts_with("S="))?;
                let bytes = if verb == "Load" {
                    let name = crate::syncterm::safe_name(data)?;
                    std::fs::read(cache?.join(name)).ok()?
                } else {
                    crate::syncterm::decode_b64(data)?
                };
                match decode(&bytes) {
                    Some(sound) => self.slots[slot?] = Some(sound),
                    None => log::info!("audio {verb}: can't decode it"),
                }
            }
            "Synth" => {
                let shape = field(rest, "W").and_then(Shape::parse)?;
                let hz: f64 = field(rest, "F").and_then(|f| f.trim().parse().ok()).unwrap_or(0.0);
                let frames = duration(field(rest, "T")?, Some(hz)).min(MAX_FRAMES);
                if frames > 0 {
                    self.slots[slot?] = Some(Sound::ready(synth(shape, hz, frames).into()));
                }
            }
            "Copy" => {
                let to = number(rest, "D").filter(|&d| d < SLOTS)?;
                self.slots[to] = self.slots[slot?].clone();
            }
            "Queue" => {
                let channel = channel.filter(|&c| c >= FIRST_APC_CHANNEL)?;
                let sound = self.slots[slot?].take().filter(|s| s.frames() > 0)?;
                let both = field(rest, "V").map(volume).unwrap_or(0.0);
                let left = field(rest, "VL").map(volume).unwrap_or(both);
                let right = field(rest, "VR").map(volume).unwrap_or(both);
                let fade_in = field(rest, "I").map_or(0, |d| duration(d, None));
                let crossfade = rest.contains(&"X");
                let looped = rest.contains(&"L");
                let now = Instant::now();
                let length = Duration::from_secs_f64(sound.frames() as f64 / RATE as f64);
                self.busy[channel] = match (looped, self.busy[channel]) {
                    (true, _) => Busy::Looping,
                    (false, Busy::Until(t)) if !crossfade && t > now => Busy::Until(t + length),
                    _ => Busy::Until(now + length),
                };
                ops.push(AudioOp::Queue {
                    channel,
                    sound,
                    fade_in,
                    fade_out: field(rest, "O").map_or(0, |d| duration(d, None)),
                    crossfade,
                    looped,
                    volume: (left, right),
                });
            }
            "Flush" => {
                let channel = channel?;
                let fade_out = field(rest, "O").map_or(0, |d| duration(d, None));
                self.busy[channel] = match fade_out {
                    0 => Busy::Idle,
                    f => Busy::Until(Instant::now() + Duration::from_secs_f64(f as f64 / RATE as f64)),
                };
                ops.push(AudioOp::Flush { channel, fade_out });
            }
            "Volume" => {
                let channel = channel?;
                let both = field(rest, "V").map(volume);
                let left = field(rest, "VL").map(volume).or(both);
                let right = field(rest, "VR").map(volume).or(both);
                if left.is_some() || right.is_some() {
                    let ramp = field(rest, "T").map_or(0, |d| duration(d, None));
                    ops.push(AudioOp::Volume { channel, left, right, ramp });
                }
            }
            "Wait" => {
                if let Busy::Until(t) = self.busy[channel?] {
                    return Some(t);
                }
            }
            "Update" => self.notify[channel?] = true,
            _ => log::debug!("unhandled audio APC {verb}"),
        }
        None
    }

    /// ANSI music started on channel 0, playing for `length`.
    pub fn music(&mut self, length: Duration) {
        let now = Instant::now();
        self.busy[0] = match self.busy[0] {
            Busy::Until(t) if t > now => Busy::Until(t + length),
            _ => Busy::Until(now + length),
        };
    }

    fn playing(&self, channel: usize, now: Instant) -> bool {
        match self.busy[channel] {
            Busy::Idle => false,
            Busy::Until(t) => t > now,
            Busy::Looping => true,
        }
    }

    /// The audio state report (`CSI = 7 [; channel] n`).
    pub fn report(&self, channel: Option<usize>) -> String {
        let now = Instant::now();
        match channel {
            Some(c) => format!("\x1b[=7;{c};{}n", (c < CHANNELS && self.playing(c, now)) as u8),
            None => {
                let pairs: String =
                    (0..CHANNELS).filter(|&c| self.playing(c, now)).map(|c| format!(";{c};1")).collect();
                format!("\x1b[=7{pairs}n")
            }
        }
    }

    /// Reports for channels that were asked about (`Update`) and have
    /// stopped since.
    pub fn poll(&mut self, now: Instant, replies: &mut Vec<u8>) {
        for c in 0..CHANNELS {
            if self.notify[c] && !self.playing(c, now) {
                self.notify[c] = false;
                replies.extend_from_slice(format!("\x1b[=7;{c};0n").as_bytes());
            }
        }
    }

    /// When the next report may be due.
    pub fn next_due(&self) -> Option<Instant> {
        (0..CHANNELS)
            .filter(|&c| self.notify[c])
            .filter_map(|c| match self.busy[c] {
                Busy::Until(t) => Some(t),
                _ => None,
            })
            .min()
    }
}

/// Whether a libsndfile format (major `pm`, subtype `ps`) is one BadTerm
/// decodes.
pub fn format_supported(pm: u32, ps: u32) -> bool {
    const WAV: u32 = 1;
    const AIFF: u32 = 2;
    const VOC: u32 = 8;
    const WAVEX: u32 = 19;
    const FLAC: u32 = 23;
    const OGG: u32 = 32;
    const VORBIS: u32 = 96;
    const OPUS: u32 = 100;
    const MPEG: u32 = 35;
    match pm {
        WAV | WAVEX => matches!(ps, 2..=7),
        AIFF => matches!(ps, 1..=4 | 6),
        VOC => matches!(ps, 2 | 5),
        FLAC => matches!(ps, 1..=3),
        OGG => matches!(ps, VORBIS | OPUS),
        // Layers I, II and III.
        MPEG => matches!(ps, 128..=130),
        _ => false,
    }
}

/// Parse a number as the doc allows: decimal or `0x` hex.
pub fn parse_id(s: &str) -> u32 {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16).unwrap_or(0),
        None => s.parse().unwrap_or(0),
    }
}

/// SyncTERM's tone shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Sine,
    Saw,
    Square,
    SineHarm,
    SineSaw,
    SineSawChord,
    SineSawHarm,
    Silence,
}

impl Shape {
    fn parse(s: &str) -> Option<Shape> {
        Some(match s.trim() {
            "SIN" => Shape::Sine,
            "SAW" => Shape::Saw,
            "SQ" => Shape::Square,
            "SINE_HARM" => Shape::SineHarm,
            "SINE_SAW" => Shape::SineSaw,
            "SINE_SAW_CHORD" => Shape::SineSawChord,
            "SINE_SAW_HARM" => Shape::SineSawHarm,
            "SILENCE" => Shape::Silence,
            _ => return None,
        })
    }
}

/// A tone, as SyncTERM's `xptone_makewave` makes it: the "noisy" shapes
/// keep that code's 8-bit arithmetic (wrap-arounds and all), so they
/// sound the same; the ends are softened over a few zero crossings so
/// they don't click.
fn synth(shape: Shape, hz: f64, frames: usize) -> Vec<i16> {
    if hz <= 0.0 || shape == Shape::Silence {
        return vec![0; frames * 2];
    }
    use std::f64::consts::{PI, TAU};
    let inc = TAU * hz / RATE as f64;
    // C's double to unsigned char, as compilers do it: through an int.
    let u8c = |v: f64| (v as i64) as u8;
    let mut mono: Vec<i16> = (0..frames)
        .map(|i| {
            let i = i as f64;
            let pos = (inc * i) % TAU;
            let noisy = |u: u8| (((u as i32) - 128) << 8) as i16;
            match shape {
                Shape::Sine => (pos.sin() * 32767.0) as i16,
                Shape::Saw => (32767.0 - (pos / TAU) * 65535.0) as i16,
                Shape::Square => {
                    if pos < PI {
                        32767
                    } else {
                        -32768
                    }
                }
                Shape::SineHarm => noisy(u8c((inc * 3.0 * i).sin() * 16.0)),
                Shape::SineSaw => noisy(u8c(((pos.sin() * 127.0 + 128.0) + ((TAU - pos) * 40.5)) / 2.0)),
                Shape::SineSawChord | Shape::SineSawHarm => {
                    let (a, b) = if shape == Shape::SineSawChord { (0.5, 1.0 / 3.0) } else { (2.0, 3.0) };
                    let mut u = u8c(((pos.sin() * 64.0 + 128.0) + ((TAU - pos) * 6.2)) / 2.0);
                    u = u8c(u as f64 + (inc * a * i).sin() * 24.0);
                    u = u8c(u as f64 + (inc * b * i).sin() * 16.0);
                    noisy(u)
                }
                Shape::Silence => 0,
            }
        })
        .collect();
    soften(&mut mono, hz);
    mono.iter().flat_map(|&v| [v, v]).collect()
}

/// `xptone_makewave`'s click prevention: drop the last partial wave and
/// ramp the first and last few zero crossings.
fn soften(mono: &mut [i16], hz: f64) {
    let n = mono.len();
    if n < 4 {
        return;
    }
    let mid = n / 2;
    let end_high = mono[n - 1] > 0;
    if mono[n - 1] != 0 {
        let mut i = n - 1;
        while i > mid {
            if end_high && mono[i] < 0 || !end_high && mono[i] > 0 {
                break;
            }
            mono[i] = 0;
            i -= 1;
        }
    }
    let crossings_wanted = (hz / 100.0) as i32;
    if crossings_wanted == 0 {
        return;
    }
    let mut crossings = 0;
    for i in 0..n - 1 {
        if ((mono[i] < 0 && mono[i + 1] >= 0) || (mono[i] > 0 && mono[i + 1] <= 0)) && i > 2 {
            crossings += 1;
            if crossings >= crossings_wanted {
                break;
            }
        }
        let d = crossings_wanted - crossings;
        mono[i] = (mono[i] as i32 / (d * d)) as i16;
    }
    crossings = 0;
    for i in (1..n).rev() {
        if ((mono[i] < 0 && mono[i - 1] >= 0) || (mono[i] > 0 && mono[i - 1] <= 0)) && i > 2 {
            crossings += 1;
            if crossings >= crossings_wanted {
                break;
            }
        }
        let d = crossings_wanted - crossings;
        mono[i] = (mono[i] as i32 / (d * d)) as i16;
    }
}

/// A sound file, as stereo frames at [`RATE`]: WAV, AIFF and VOC at once,
/// Ogg (Opus or Vorbis), FLAC and MP3 on a thread (their length read first).
pub fn decode(data: &[u8]) -> Option<Sound> {
    let (rate, channels, samples) = if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WAVE") {
        decode_wav(data)?
    } else if data.starts_with(b"FORM") && matches!(data.get(8..12), Some(b"AIFF" | b"AIFC")) {
        decode_aiff(data)?
    } else if data.starts_with(b"Creative Voice File\x1a") {
        decode_voc(data)?
    } else if data.starts_with(b"OggS") || data.starts_with(b"fLaC") || nanomp3::detect(data) {
        let (frames, decode) = later(data)?;
        let frames = frames.min(MAX_FRAMES);
        return Some(Sound::decoded_later(frames, move || {
            let (rate, channels, samples) = decode()?;
            Some(to_stereo(rate, channels, &samples))
        }));
    } else {
        return None;
    };
    Some(Sound::ready(to_stereo(rate, channels, &samples).into()))
}

/// What a decoder makes: (rate, channels, interleaved samples in -1..1).
type Decoded = (u32, usize, Vec<f32>);

/// A slow format's length in frames at [`RATE`], from its headers, and
/// the work of decoding it.
#[allow(clippy::type_complexity)]
fn later(data: &[u8]) -> Option<(usize, Box<dyn FnOnce() -> Option<Decoded> + Send>)> {
    let at_rate = |n: u64, rate: u32| (n as f64 * RATE as f64 / rate.max(1) as f64) as usize;
    if data.starts_with(b"fLaC") {
        let reader = claxon::FlacReader::new(std::io::Cursor::new(data.to_vec())).ok()?;
        let info = reader.streaminfo();
        let frames = at_rate(info.samples.unwrap_or(0), info.sample_rate);
        return Some((frames, Box::new(move || decode_flac(reader))));
    }
    if !data.starts_with(b"OggS") {
        // MP3 (or MP1, MP2): its length from a scan of its frames.
        let reader = nanomp3::SliceReader::<f32>::new(data);
        let rate = reader.sample_rate();
        let frames = at_rate(reader.total_samples().unwrap_or(0), rate);
        if rate == 0 || frames == 0 {
            return None;
        }
        let owned = data.to_vec();
        return Some((frames, Box::new(move || decode_mp3(&owned))));
    }
    // Ogg: the stream's codec from its first packet, its length from the
    // last page's granule position.
    let mut reader = ogg::PacketReader::new(std::io::Cursor::new(data));
    let head = reader.read_packet().ok()??.data;
    let mut last = 0;
    while let Ok(Some(p)) = reader.read_packet() {
        last = p.absgp_page();
    }
    let owned = data.to_vec();
    if head.starts_with(b"OpusHead") {
        let pre_skip = u16le(&head, 10)? as u64;
        Some((at_rate(last.saturating_sub(pre_skip), 48_000), Box::new(move || decode_opus(&owned))))
    } else if head.starts_with(b"\x01vorbis") {
        let rate = u32le(&head, 12)?;
        Some((at_rate(last, rate), Box::new(move || decode_vorbis(owned))))
    } else {
        None
    }
}

fn decode_mp3(data: &[u8]) -> Option<Decoded> {
    let mut reader = nanomp3::SliceReader::<f32>::new(data);
    let channels = reader.channels().map_or(2, |c| c as usize);
    let rate = reader.sample_rate();
    let mut samples = Vec::new();
    let mut buf = vec![0f32; 4608];
    while let Ok(n @ 1..) = reader.read(&mut buf) {
        samples.extend_from_slice(&buf[..n]);
        if samples.len() > MAX_FRAMES * channels {
            break;
        }
    }
    Some((rate, channels, samples))
}

fn decode_flac(mut reader: claxon::FlacReader<std::io::Cursor<Vec<u8>>>) -> Option<Decoded> {
    let info = reader.streaminfo();
    let scale = 1.0 / (1u64 << (info.bits_per_sample.clamp(1, 32) - 1)) as f32;
    let mut samples = Vec::new();
    for s in reader.samples() {
        samples.push(s.ok()? as f32 * scale);
        if samples.len() > MAX_FRAMES * 2 * info.channels as usize / 2 {
            break;
        }
    }
    Some((info.sample_rate, info.channels as usize, samples))
}

fn decode_vorbis(data: Vec<u8>) -> Option<Decoded> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(data)).ok()?;
    let (rate, channels) = (reader.ident_hdr.audio_sample_rate, reader.ident_hdr.audio_channels as usize);
    let mut samples = Vec::new();
    while let Ok(Some(packet)) = reader.read_dec_packet_itl() {
        samples.extend(packet.iter().map(|&s| s as f32 / 32768.0));
        if samples.len() > MAX_FRAMES * channels.max(1) {
            break;
        }
    }
    Some((rate, channels, samples))
}

/// Samples in -1..1 (interleaved, `channels` of them) at `rate`, as
/// stereo 16-bit at [`RATE`] (linear interpolation).
fn to_stereo(rate: u32, channels: usize, samples: &[f32]) -> Vec<i16> {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    let frame = |i: usize| -> (f32, f32) {
        let f = &samples[i.min(frames - 1) * channels..][..channels];
        (f[0], if channels > 1 { f[1] } else { f[0] })
    };
    if frames == 0 || rate == 0 {
        return Vec::new();
    }
    let out_frames = ((frames as u64 * RATE as u64 / rate as u64) as usize).min(MAX_FRAMES);
    let step = rate as f64 / RATE as f64;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = i as f64 * step;
        let (j, t) = (pos as usize, (pos.fract()) as f32);
        let ((l0, r0), (l1, r1)) = (frame(j), frame(j + 1));
        let s = |a: f32, b: f32| ((a + (b - a) * t) * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
        out.push(s(l0, l1));
        out.push(s(r0, r1));
    }
    out
}

fn u16le(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn u16be(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn u32be(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// Integer or float PCM samples to -1..1. `bits` 8 is unsigned (WAV,
/// VOC) when `unsigned8`.
fn pcm_samples(data: &[u8], bits: u16, float: bool, big_endian: bool, unsigned8: bool) -> Option<Vec<f32>> {
    let bytes = bits as usize / 8;
    if bytes == 0 || bytes > 8 {
        return None;
    }
    let limit = MAX_FRAMES * 2 * bytes;
    let data = &data[..data.len().min(limit) / bytes * bytes];
    Some(
        data.chunks_exact(bytes)
            .map(|c| {
                let mut b = [0u8; 8];
                if big_endian {
                    for (i, &v) in c.iter().rev().enumerate() {
                        b[i] = v;
                    }
                } else {
                    b[..bytes].copy_from_slice(c);
                }
                match (float, bytes) {
                    (true, 4) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    (true, 8) => f64::from_le_bytes(b) as f32,
                    (_, 1) if unsigned8 => (b[0] as f32 - 128.0) / 128.0,
                    (_, 1) => b[0] as i8 as f32 / 128.0,
                    (_, 2) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
                    (_, 3) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
                    (_, 4) => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
                    _ => 0.0,
                }
            })
            .collect(),
    )
}

fn decode_wav(d: &[u8]) -> Option<(u32, usize, Vec<f32>)> {
    let (mut at, mut fmt, mut data) = (12, None, None);
    while at + 8 <= d.len() {
        let size = u32le(d, at + 4)? as usize;
        let body = d.get(at + 8..(at + 8 + size).min(d.len()))?;
        match &d[at..at + 4] {
            b"fmt " => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        at += 8 + size + size % 2;
    }
    let (fmt, data) = (fmt?, data?);
    let mut tag = u16le(fmt, 0)?;
    let channels = u16le(fmt, 2)? as usize;
    let rate = u32le(fmt, 4)?;
    let bits = u16le(fmt, 14)?;
    if tag == 0xFFFE {
        // WAVE_FORMAT_EXTENSIBLE: the real format starts the sub-format GUID.
        tag = u16le(fmt, 24)?;
    }
    let float = match tag {
        1 => false,
        3 => true,
        _ => return None,
    };
    Some((rate, channels, pcm_samples(data, bits, float, false, true)?))
}

/// An 80-bit IEEE extended float (AIFF's sample rate).
fn extended(b: &[u8]) -> Option<f64> {
    let exp = (u16be(b, 0)? & 0x7FFF) as i32;
    let mant = u64::from_be_bytes(b.get(2..10)?.try_into().ok()?);
    Some(mant as f64 * 2f64.powi(exp - 16383 - 63))
}

fn decode_aiff(d: &[u8]) -> Option<(u32, usize, Vec<f32>)> {
    let aifc = &d[8..12] == b"AIFC";
    let (mut at, mut comm, mut ssnd) = (12, None, None);
    while at + 8 <= d.len() {
        let size = u32be(d, at + 4)? as usize;
        let body = d.get(at + 8..(at + 8 + size).min(d.len()))?;
        match &d[at..at + 4] {
            b"COMM" => comm = Some(body),
            b"SSND" => ssnd = Some(body),
            _ => {}
        }
        at += 8 + size + size % 2;
    }
    let (comm, ssnd) = (comm?, ssnd?);
    let channels = u16be(comm, 0)? as usize;
    let bits = u16be(comm, 6)?;
    let rate = extended(comm.get(8..18)?)? as u32;
    let (float, big) = match comm.get(18..22).filter(|_| aifc) {
        None | Some(b"NONE") | Some(b"twos") => (false, true),
        Some(b"sowt") => (false, false),
        Some(b"fl32") | Some(b"FL32") => (true, true),
        _ => return None,
    };
    let offset = u32be(ssnd, 0)? as usize;
    let data = ssnd.get(8 + offset..)?;
    let bits = if float { 32 } else { bits.div_ceil(8) * 8 };
    Some((rate, channels, pcm_samples(data, bits, float, big, false)?))
}

fn decode_voc(d: &[u8]) -> Option<(u32, usize, Vec<f32>)> {
    let mut at = u16le(d, 20)? as usize;
    let (mut rate, mut channels, mut bits) = (0u32, 1usize, 8u16);
    let mut samples = Vec::new();
    while let Some(&kind) = d.get(at) {
        if kind == 0 {
            break;
        }
        let size = u32le(d, at)? as usize >> 8;
        let body = d.get(at + 4..(at + 4 + size).min(d.len()))?;
        at += 4 + size;
        let pcm = match kind {
            // Sound data: a time constant and a codec (0: 8-bit unsigned).
            1 => {
                if rate == 0 {
                    rate = 1_000_000 / (256 - *body.first()? as u32);
                }
                (*body.get(1)? == 0).then(|| body.get(2..)).flatten()
            }
            // More of the same.
            2 => Some(body),
            // The newer header: rate, bits, channels, codec (0 or 4).
            9 => {
                rate = u32le(body, 0)?;
                bits = *body.get(4)? as u16;
                channels = *body.get(5)? as usize;
                matches!(u16le(body, 6)?, 0 | 4).then(|| body.get(12..)).flatten()
            }
            _ => None,
        };
        if let Some(p) = pcm {
            samples.extend(pcm_samples(p, bits, false, false, true)?);
            if samples.len() > MAX_FRAMES * 2 {
                break;
            }
        }
    }
    (rate > 0).then_some((rate, channels, samples))
}

/// Ogg Opus: decoded at 48 kHz, stereo.
fn decode_opus(d: &[u8]) -> Option<(u32, usize, Vec<f32>)> {
    let mut reader = ogg::PacketReader::new(std::io::Cursor::new(d));
    let head = reader.read_packet().ok()??;
    if !head.data.starts_with(b"OpusHead") || head.data.get(18).copied().unwrap_or(0) != 0 {
        // Not Opus, or a surround mapping.
        return None;
    }
    let pre_skip = u16le(&head.data, 10)? as usize;
    let gain = i16::from_le_bytes(head.data.get(16..18)?.try_into().ok()?);
    let gain = 10f32.powf(gain as f32 / 256.0 / 20.0);
    let mut decoder = rusty_opus::OpusDecoder::new(48_000, 2).ok()?;
    let mut buf = vec![0f32; 5760 * 2];
    let mut samples = Vec::new();
    let mut skipped = 0;
    while let Ok(Some(packet)) = reader.read_packet() {
        if packet.data.starts_with(b"OpusTags") {
            continue;
        }
        let Ok(n) = decoder.decode(&packet.data, 5760, &mut buf) else { continue };
        let mut frames = &buf[..n * 2];
        if skipped < pre_skip {
            let skip = (pre_skip - skipped).min(n);
            skipped += skip;
            frames = &frames[skip * 2..];
        }
        samples.extend(frames.iter().map(|s| s * gain));
        if samples.len() > MAX_FRAMES * 2 {
            break;
        }
    }
    Some((48_000, 2, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut f = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        f.extend(16u32.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(channels.to_le_bytes());
        f.extend(rate.to_le_bytes());
        f.extend((rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        f.extend((channels * bits / 8).to_le_bytes());
        f.extend(bits.to_le_bytes());
        f.extend(b"data");
        f.extend((data.len() as u32).to_le_bytes());
        f.extend(data);
        f
    }

    #[test]
    fn durations_and_volumes() {
        assert_eq!(duration("500", None), 22050);
        assert_eq!(duration("500ms", None), 22050);
        assert_eq!(duration("100f", None), 100);
        assert_eq!(duration("2p", Some(441.0)), 200);
        assert_eq!(duration("2p", None), 0);
        assert_eq!(duration("x", None), 0);
        assert_eq!(volume("-6.5dB"), -6.5);
        assert_eq!(volume("100"), 0.0);
        assert_eq!(volume("0"), -60.0);
        assert!((volume("50") + 6.02).abs() < 0.01);
    }

    #[test]
    fn files_decode_to_stereo_at_44k() {
        // 8-bit unsigned mono WAV at 22050 Hz: twice as many frames out.
        let pcm = decode(&wav(22050, 1, 8, &[128, 255, 0, 128])).unwrap().wait().clone();
        assert_eq!(pcm.len(), 8 * 2);
        assert_eq!(pcm[0], 0);
        assert_eq!(pcm[2], pcm[3], "mono goes to both sides");
        // Frame 1 is halfway between samples 0 and 1; frame 2 is sample 1.
        assert!((16000..16500).contains(&pcm[2]), "{}", pcm[2]);
        assert!(pcm[4] > 32000);
        // 16-bit stereo at the same rate is kept as it is.
        let pcm = decode(&wav(44100, 2, 16, &[0, 0x40, 0, 0xC0])).unwrap().wait().clone();
        assert_eq!(&pcm[..], &[16384, -16384]);
        // A VOC: an 8-bit block at 1 MHz / (256 - 233) = 43478 Hz.
        let mut voc = b"Creative Voice File\x1a\x1a\x00\x0a\x01\x29\x11".to_vec();
        voc.extend([1, 6, 0, 0, 233, 0, 128, 255, 128, 0, 0]);
        let pcm = decode(&voc).unwrap().wait().clone();
        assert!(pcm.len() >= 8 && pcm.iter().any(|&s| s > 30000));
        // AIFF: 16-bit big-endian mono at 44100 Hz.
        let mut comm = 1u16.to_be_bytes().to_vec();
        comm.extend(2u32.to_be_bytes());
        comm.extend(16u16.to_be_bytes());
        comm.extend([0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        let mut aiff = b"FORM\0\0\0\0AIFFCOMM".to_vec();
        aiff.extend((comm.len() as u32).to_be_bytes());
        aiff.extend(&comm);
        aiff.extend(b"SSND");
        aiff.extend(12u32.to_be_bytes());
        aiff.extend([0u8; 8]);
        aiff.extend([0x40, 0x00, 0xC0, 0x00]);
        assert_eq!(&decode(&aiff).unwrap().wait()[..], &[16384, 16384, -16384, -16384]);
        assert!(decode(b"not a sound").is_none());
    }

    #[test]
    fn slots_channels_and_reports() {
        let mut a = Audio::default();
        let mut ops = Vec::new();
        let apc = |a: &mut Audio, ops: &mut Vec<AudioOp>, s: &str| {
            let fields: Vec<&str> = s.split(';').collect();
            a.apc(&fields, None, ops)
        };
        apc(&mut a, &mut ops, "Synth;S=3;W=SQ;F=441;T=10p");
        apc(&mut a, &mut ops, "Copy;S=3;D=4");
        // Channels 0 and 1 are the terminal's own.
        apc(&mut a, &mut ops, "Queue;C=1;S=3");
        assert!(ops.is_empty());
        apc(&mut a, &mut ops, "Queue;C=2;S=3;V=50;I=5;L");
        let Some(AudioOp::Queue { channel: 2, sound, looped: true, fade_in: 220, volume, .. }) = ops.first() else {
            panic!("{ops:?}")
        };
        assert_eq!(sound.frames(), 1000);
        assert!((volume.0 + 6.02).abs() < 0.01);
        // The slot is emptied; its copy isn't.
        apc(&mut a, &mut ops, "Queue;C=3;S=3");
        assert_eq!(ops.len(), 1);
        apc(&mut a, &mut ops, "Queue;C=3;S=4");
        assert_eq!(a.report(None), "\x1b[=7;2;1;3;1n");
        assert_eq!(a.report(Some(5)), "\x1b[=7;5;0n");
        // Waiting on the 1000-frame tone holds about 23 ms.
        let until = apc(&mut a, &mut ops, "Wait;C=3").expect("a hold");
        assert!(until > Instant::now() && until < Instant::now() + Duration::from_millis(100));
        // Told when it stops.
        apc(&mut a, &mut ops, "Update;C=3");
        let mut replies = Vec::new();
        a.poll(Instant::now(), &mut replies);
        assert!(replies.is_empty());
        a.poll(Instant::now() + Duration::from_secs(1), &mut replies);
        assert_eq!(replies, b"\x1b[=7;3;0n");
        apc(&mut a, &mut ops, "Flush;C=2");
        assert_eq!(a.report(Some(2)), "\x1b[=7;2;0n");
        assert_eq!(ops.last(), Some(&AudioOp::Flush { channel: 2, fade_out: 0 }));
    }

    #[test]
    fn tones_match_synctermss_shapes() {
        let sq = synth(Shape::Square, 441.0, 1000);
        // Full scale in the middle, softened at the ends.
        assert_eq!(sq[425 * 2], 32767);
        assert_eq!(sq[475 * 2], -32768);
        assert!(sq[0].abs() < 32767 / 4);
        assert_eq!(synth(Shape::Silence, 441.0, 10), vec![0; 20]);
        let noisy = synth(Shape::SineSawHarm, 441.0, 1000);
        assert!(noisy.iter().any(|&v| v != 0));
        assert!(
            format_supported(32, 100)
                && format_supported(1, 2)
                && format_supported(23, 2)
                && format_supported(32, 96)
                && format_supported(35, 130)
                && !format_supported(23, 96)
        );
        assert_eq!(parse_id("0x20"), 32);
    }

    /// A sound file (Ogg Opus or Vorbis, FLAC, WAV...): `BADTERM_SOUND=a.opus cargo test -p badterm-core --release sound_file -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn sound_file_decodes() {
        let data = std::fs::read(std::env::var("BADTERM_SOUND").expect("set BADTERM_SOUND")).unwrap();
        let t = Instant::now();
        let sound = decode(&data).expect("decodes");
        let frames = sound.frames();
        let pcm = sound.wait().clone();
        eprintln!("length from the headers {frames}, decoded {}", pcm.len() / 2);
        let secs = pcm.len() as f64 / 2.0 / RATE as f64;
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let rms = (pcm.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt();
        // Zero crossings per second, left channel: music, not noise.
        let zc = pcm.chunks(2).collect::<Vec<_>>().windows(2).filter(|w| (w[0][0] < 0) != (w[1][0] < 0)).count();
        eprintln!(
            "{secs:.2} s, peak {peak}, rms {rms:.0}, {:.0} crossings/s, decoded in {:?}",
            zc as f64 / secs,
            t.elapsed()
        );
        if let Ok(out) = std::env::var("BADTERM_WAV_OUT") {
            let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
            let mut f = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
            f.extend(16u32.to_le_bytes());
            f.extend([1, 0, 2, 0]);
            f.extend(RATE.to_le_bytes());
            f.extend((RATE * 4).to_le_bytes());
            f.extend([4, 0, 16, 0]);
            f.extend(b"data");
            f.extend((bytes.len() as u32).to_le_bytes());
            f.extend(bytes);
            std::fs::write(out, f).unwrap();
        }
    }
}
