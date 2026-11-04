//! Playing a BBS's sound: ANSI music, which a square wave plays like a PC
//! speaker, and SyncTERM's audio channels (samples and tones the BBS
//! queues, with fades, loops and volume), mixed together by a background
//! thread that feeds the system's audio output (ALSA on Linux, loaded at
//! run time; CoreAudio or WASAPI through cpal elsewhere).
//!
//! If there is no audio output, sound is silently skipped.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use badterm_core::audio::{self, AudioOp, BASE_DB, CHANNELS, FIRST_APC_CHANNEL, Sound};
use badterm_core::music::Tone;

/// Quiet: a square wave at full scale is harsh.
const VOLUME: f32 = 0.12;
/// Fade in and out over this long, so notes don't click.
const FADE_MS: u32 = 3;
/// Samples mixed at a time, so a change is heard quickly.
const CHUNK_MS: u32 = 20;

enum Cmd {
    Play(Vec<Tone>),
    Audio(AudioOp),
    Stop,
}

/// The sound player. The audio thread and output start on first use.
#[derive(Default)]
pub struct Player {
    tx: Option<Sender<Cmd>>,
}

impl Player {
    fn send(&mut self, cmd: Cmd) {
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            thread::Builder::new()
                .name("sound".into())
                .spawn(move || run(rx))
                .map_err(|e| log::warn!("starting the sound thread: {e}"))
                .ok();
            tx
        });
        let _ = tx.send(cmd);
    }

    /// Queue tones after whatever music is playing.
    pub fn play(&mut self, tones: Vec<Tone>) {
        self.send(Cmd::Play(tones));
    }

    /// A SyncTERM audio command.
    pub fn audio(&mut self, op: AudioOp) {
        self.send(Cmd::Audio(op));
    }

    /// Stop everything and forget anything queued.
    pub fn stop(&mut self) {
        if self.tx.is_some() {
            self.send(Cmd::Stop);
        }
    }
}

/// Where samples go: blocking writes of interleaved stereo samples in -1..1.
trait Output {
    fn rate(&self) -> u32;
    fn write(&mut self, samples: &[f32]);
}

fn run(rx: Receiver<Cmd>) {
    let Some(mut out) = backend::open() else {
        // Keep draining commands so senders never notice.
        for _ in rx {}
        return;
    };
    let mut mixer = Mixer::new(out.rate());
    loop {
        // Wait for something to play.
        if mixer.idle() {
            match rx.recv() {
                Ok(cmd) => mixer.command(cmd),
                Err(_) => return,
            }
        }
        loop {
            match rx.try_recv() {
                Ok(cmd) => mixer.command(cmd),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if !mixer.idle() {
            let samples = mixer.render(out.rate() * CHUNK_MS / 1000);
            out.write(&samples);
        }
    }
}

fn db(v: f32) -> f32 {
    10f32.powf(v / 20.0)
}

/// A sound on a channel, playing.
struct Buf {
    /// Silent, and not started, until it's decoded.
    sound: Sound,
    /// Where it is, in frames at [`audio::RATE`] (fractional when the
    /// output's rate differs).
    pos: f64,
    fade_in: usize,
    fade_out: usize,
    looped: bool,
    /// Linear gains, left and right.
    gain: (f32, f32),
    /// Fading away over (frames, of how many), after a crossfade or flush.
    decay: Option<(usize, usize)>,
}

impl Buf {
    fn done(&self) -> bool {
        let ended = self.sound.pcm().is_some_and(|p| self.pos as usize >= p.len() / 2 && (!self.looped || p.is_empty()));
        self.decay.is_some_and(|(left, _)| left == 0) || ended
    }

    /// The next frame, `step` frames of the sound per output frame.
    fn next(&mut self, step: f64) -> (f32, f32) {
        // Not decoded yet: wait (a sound decoded to nothing is done).
        let Some(pcm) = self.sound.pcm().cloned() else { return (0.0, 0.0) };
        let n = pcm.len() / 2;
        if self.done() || n == 0 {
            return (0.0, 0.0);
        }
        let i = self.pos as usize % n;
        let t = self.pos.fract() as f32;
        let j = if i + 1 < n {
            i + 1
        } else if self.looped {
            0
        } else {
            i
        };
        let at = |k: usize, c: usize| pcm[k * 2 + c] as f32 / 32768.0;
        let (l, r) = (at(i, 0) + (at(j, 0) - at(i, 0)) * t, at(i, 1) + (at(j, 1) - at(i, 1)) * t);
        let mut g = 1.0;
        let played = self.pos as usize;
        if self.fade_in > 0 && played < self.fade_in {
            g *= played as f32 / self.fade_in as f32;
        }
        if self.fade_out > 0 && !self.looped && n - i < self.fade_out {
            g *= (n - i) as f32 / self.fade_out as f32;
        }
        if let Some((left, total)) = &mut self.decay {
            g *= *left as f32 / (*total).max(1) as f32;
            *left = left.saturating_sub(1);
        }
        self.pos += step;
        if self.looped && self.pos as usize >= n {
            self.pos -= n as f64;
        }
        (l * g * self.gain.0, r * g * self.gain.1)
    }
}

/// One of SyncTERM's channels.
struct Channel {
    queue: VecDeque<Buf>,
    /// Sounds fading out under the queue (crossfades, flushes).
    fading: Vec<Buf>,
    /// The level in dB, and a ramp toward another: (target, frames left).
    level: (f32, f32),
    ramp: Option<((f32, f32), usize)>,
}

impl Channel {
    fn new(level: f32) -> Channel {
        Channel { queue: VecDeque::new(), fading: Vec::new(), level: (level, level), ramp: None }
    }

    fn idle(&self) -> bool {
        self.queue.is_empty() && self.fading.is_empty()
    }
}

struct Mixer {
    /// Frames of sound per output frame.
    step: f64,
    music: VecDeque<Tone>,
    /// Music rendered but not mixed yet.
    music_buf: VecDeque<f32>,
    synth: Synth,
    channels: Vec<Channel>,
}

impl Mixer {
    fn new(rate: u32) -> Mixer {
        let channels =
            (0..CHANNELS).map(|c| Channel::new(if c >= FIRST_APC_CHANNEL { BASE_DB } else { 0.0 })).collect();
        Mixer {
            step: audio::RATE as f64 / rate as f64,
            music: VecDeque::new(),
            music_buf: VecDeque::new(),
            synth: Synth::new(rate),
            channels,
        }
    }

    fn idle(&self) -> bool {
        self.music.is_empty() && self.music_buf.is_empty() && self.channels.iter().all(Channel::idle)
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play(t) => self.music.extend(t),
            Cmd::Stop => {
                self.music.clear();
                self.music_buf.clear();
                for c in &mut self.channels {
                    c.queue.clear();
                    c.fading.clear();
                }
            }
            Cmd::Audio(op) => self.audio(op),
        }
    }

    fn audio(&mut self, op: AudioOp) {
        match op {
            AudioOp::Queue { channel, sound, fade_in, fade_out, crossfade, looped, volume } => {
                let Some(c) = self.channels.get_mut(channel) else { return };
                let buf =
                    Buf { sound, pos: 0.0, fade_in, fade_out, looped, gain: (db(volume.0), db(volume.1)), decay: None };
                if crossfade && let Some(mut head) = c.queue.pop_front() {
                    // What was playing fades out (looping on) as this fades in.
                    head.decay = Some((fade_in.max(1), fade_in.max(1)));
                    c.fading.push(head);
                    c.queue.push_front(buf);
                } else {
                    // A new sound ends a loop, after its current pass.
                    if let Some(head) = c.queue.front_mut() {
                        head.looped = false;
                    }
                    c.queue.push_back(buf);
                }
            }
            AudioOp::Flush { channel, fade_out } => {
                if channel == 0 {
                    self.music.clear();
                    self.music_buf.clear();
                }
                let Some(c) = self.channels.get_mut(channel) else { return };
                if fade_out > 0
                    && let Some(mut head) = c.queue.pop_front()
                {
                    head.decay = Some((fade_out, fade_out));
                    c.fading.push(head);
                }
                c.queue.clear();
                if fade_out == 0 {
                    c.fading.clear();
                }
            }
            AudioOp::Volume { channel, left, right, ramp } => {
                let Some(c) = self.channels.get_mut(channel) else { return };
                let target = (left.unwrap_or(c.level.0), right.unwrap_or(c.level.1));
                if ramp == 0 {
                    c.level = target;
                    c.ramp = None;
                } else {
                    c.ramp = Some((target, ramp));
                }
            }
        }
    }

    /// `frames` frames of everything playing, interleaved stereo.
    fn render(&mut self, frames: u32) -> Vec<f32> {
        let mut out = vec![0f32; frames as usize * 2];
        // ANSI music, on channel 0: rendered a note at a time, kept until used.
        while self.music_buf.len() < frames as usize {
            let Some(tone) = self.music.front_mut() else { break };
            let ms = tone.ms.min(CHUNK_MS);
            self.music_buf.extend(self.synth.render(*tone, ms));
            tone.ms -= ms;
            if tone.ms == 0 {
                self.music.pop_front();
                self.synth.next_note();
            }
        }
        let g0 = (db(self.channels[0].level.0), db(self.channels[0].level.1));
        let n = self.music_buf.len().min(frames as usize);
        for (i, m) in self.music_buf.drain(..n).enumerate() {
            out[i * 2] += m * g0.0;
            out[i * 2 + 1] += m * g0.1;
        }
        // The BBS's channels.
        let step = self.step;
        for c in &mut self.channels[FIRST_APC_CHANNEL..] {
            if c.idle() {
                continue;
            }
            for i in 0..frames as usize {
                if let Some((target, left)) = &mut c.ramp {
                    let k = 1.0 / *left as f32;
                    c.level.0 += (target.0 - c.level.0) * k;
                    c.level.1 += (target.1 - c.level.1) * k;
                    *left -= 1;
                    if *left == 0 {
                        c.level = *target;
                        c.ramp = None;
                    }
                }
                let (gl, gr) = (db(c.level.0), db(c.level.1));
                let (mut l, mut r) = (0.0, 0.0);
                for b in c.fading.iter_mut().chain(c.queue.front_mut()) {
                    let (bl, br) = b.next(step);
                    l += bl;
                    r += br;
                }
                out[i * 2] += l * gl;
                out[i * 2 + 1] += r * gr;
                if c.queue.front().is_some_and(Buf::done) {
                    c.queue.pop_front();
                }
            }
            c.fading.retain(|b| !b.done());
        }
        for s in &mut out {
            *s = s.clamp(-1.0, 1.0);
        }
        out
    }
}

/// Square-wave synthesis that keeps its phase across chunks of a note.
struct Synth {
    rate: u32,
    phase: f32,
    /// Samples into the current note, for the fades.
    pos: u32,
}

impl Synth {
    fn new(rate: u32) -> Synth {
        Synth { rate, phase: 0.0, pos: 0 }
    }

    fn next_note(&mut self) {
        self.pos = 0;
    }

    /// `ms` milliseconds of `tone`, which has `tone.ms` left (including these).
    fn render(&mut self, tone: Tone, ms: u32) -> Vec<f32> {
        let n = (self.rate as u64 * ms as u64 / 1000) as usize;
        if tone.hz == 0 {
            self.pos += n as u32;
            return vec![0.0; n];
        }
        let fade = (self.rate * FADE_MS / 1000).max(1) as f32;
        let left_total = (self.rate as u64 * tone.ms as u64 / 1000) as u32;
        let step = tone.hz as f32 / self.rate as f32;
        let start = self.pos;
        let samples = (0..n)
            .map(|i| {
                let level = if self.phase < 0.5 { VOLUME } else { -VOLUME };
                self.phase = (self.phase + step).fract();
                let from_start = (start + i as u32) as f32;
                let to_end = left_total.saturating_sub(i as u32) as f32;
                level * (from_start / fade).min(1.0) * (to_end / fade).min(1.0)
            })
            .collect();
        self.pos += n as u32;
        samples
    }
}

#[cfg(target_os = "linux")]
mod backend {
    //! ALSA through `dlopen`, so neither building nor running BadTerm needs
    //! the ALSA library: without it there is just no music.

    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};

    use libloading::{Library, Symbol};

    use super::Output;

    const RATE: u32 = 44_100;
    const STREAM_PLAYBACK: c_int = 0;
    const FORMAT_S16_LE: c_int = 2;
    const ACCESS_RW_INTERLEAVED: c_int = 3;

    type Open = unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int, c_int) -> c_int;
    type SetParams = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int;
    type Writei = unsafe extern "C" fn(*mut c_void, *const c_void, c_ulong) -> c_long;
    type Recover = unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int;
    type Close = unsafe extern "C" fn(*mut c_void) -> c_int;

    struct Alsa {
        // Keeps the library loaded for as long as the functions are used.
        _lib: Library,
        pcm: *mut c_void,
        writei: Writei,
        recover: Recover,
        close: Close,
    }

    pub(super) fn open() -> Option<Box<dyn Output>> {
        match Alsa::open() {
            Ok(a) => Some(Box::new(a)),
            Err(e) => {
                log::info!("no sound: {e}");
                None
            }
        }
    }

    impl Alsa {
        fn open() -> Result<Alsa, String> {
            // SAFETY: loading the system's libasound, whose initialisers
            // are safe to run, and looking up functions by their documented
            // C signatures.
            unsafe {
                let lib = Library::new("libasound.so.2").map_err(|e| e.to_string())?;
                let sym = |name: &[u8]| -> Result<*const (), String> {
                    let s: Symbol<*const ()> = lib.get(name).map_err(|e| e.to_string())?;
                    Ok(*s)
                };
                let open: Open = std::mem::transmute(sym(b"snd_pcm_open\0")?);
                let set_params: SetParams = std::mem::transmute(sym(b"snd_pcm_set_params\0")?);
                let writei: Writei = std::mem::transmute(sym(b"snd_pcm_writei\0")?);
                let recover: Recover = std::mem::transmute(sym(b"snd_pcm_recover\0")?);
                let close: Close = std::mem::transmute(sym(b"snd_pcm_close\0")?);
                let mut pcm = std::ptr::null_mut();
                let err = open(&mut pcm, c"default".as_ptr(), STREAM_PLAYBACK, 0);
                if err < 0 {
                    return Err(format!("snd_pcm_open failed ({err})"));
                }
                // Stereo 16-bit at 44.1 kHz, resampled by ALSA if need be,
                // with 100 ms of buffering.
                let err = set_params(pcm, FORMAT_S16_LE, ACCESS_RW_INTERLEAVED, 2, RATE, 1, 100_000);
                if err < 0 {
                    close(pcm);
                    return Err(format!("snd_pcm_set_params failed ({err})"));
                }
                Ok(Alsa { _lib: lib, pcm, writei, recover, close })
            }
        }
    }

    impl Output for Alsa {
        fn rate(&self) -> u32 {
            RATE
        }

        fn write(&mut self, samples: &[f32]) {
            let pcm: Vec<i16> = samples.iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect();
            let mut done = 0;
            while done < pcm.len() {
                let rest = &pcm[done..];
                // SAFETY: `rest` is valid for `rest.len() / 2` stereo frames.
                let n = unsafe { (self.writei)(self.pcm, rest.as_ptr().cast(), (rest.len() / 2) as c_ulong) };
                if n >= 0 {
                    done += n as usize * 2;
                } else if unsafe { (self.recover)(self.pcm, n as c_int, 1) } < 0 {
                    // Underruns are recovered from; anything else drops the chunk.
                    return;
                }
            }
        }
    }

    impl Drop for Alsa {
        fn drop(&mut self) {
            // SAFETY: `pcm` came from snd_pcm_open and is closed once.
            unsafe { (self.close)(self.pcm) };
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod backend {
    //! cpal: CoreAudio on macOS, WASAPI on Windows. The callback takes
    //! samples from a shared buffer that `write` fills, blocking while more
    //! than a little is waiting.

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

    use super::Output;

    struct Cpal {
        // Playing for as long as it lives.
        _stream: Stream,
        buffer: Arc<Mutex<VecDeque<f32>>>,
        rate: u32,
    }

    pub(super) fn open() -> Option<Box<dyn Output>> {
        match Cpal::open() {
            Ok(c) => Some(Box::new(c)),
            Err(e) => {
                log::info!("no sound: {e}");
                None
            }
        }
    }

    impl Cpal {
        fn open() -> Result<Cpal, String> {
            let device = cpal::default_host().default_output_device().ok_or("no audio output device")?;
            let supported = device.default_output_config().map_err(|e| e.to_string())?;
            let format = supported.sample_format();
            let config: StreamConfig = supported.into();
            let buffer = Arc::new(Mutex::new(VecDeque::new()));
            let stream = match format {
                SampleFormat::F32 => build::<f32>(&device, &config, buffer.clone()),
                SampleFormat::I16 => build::<i16>(&device, &config, buffer.clone()),
                SampleFormat::U16 => build::<u16>(&device, &config, buffer.clone()),
                SampleFormat::I32 => build::<i32>(&device, &config, buffer.clone()),
                other => return Err(format!("unsupported sample format {other}")),
            }?;
            stream.play().map_err(|e| e.to_string())?;
            Ok(Cpal { _stream: stream, buffer, rate: config.sample_rate })
        }
    }

    fn build<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        buffer: Arc<Mutex<VecDeque<f32>>>,
    ) -> Result<Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = config.channels as usize;
        device
            .build_output_stream(
                *config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let mut buf = buffer.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks_mut(channels.max(1)) {
                        let l = buf.pop_front().unwrap_or(0.0);
                        let r = buf.pop_front().unwrap_or(0.0);
                        match frame {
                            [only] => *only = T::from_sample((l + r) / 2.0),
                            [a, b, rest @ ..] => {
                                *a = T::from_sample(l);
                                *b = T::from_sample(r);
                                rest.fill(T::from_sample(0.0));
                            }
                            [] => {}
                        }
                    }
                },
                |e| log::warn!("audio: {e}"),
                None,
            )
            .map_err(|e| e.to_string())
    }

    impl Output for Cpal {
        fn rate(&self) -> u32 {
            self.rate
        }

        fn write(&mut self, samples: &[f32]) {
            self.buffer.lock().unwrap_or_else(|e| e.into_inner()).extend(samples);
            // Keep about 100 ms queued (stereo), so Stop takes effect quickly.
            let limit = self.rate as usize / 5;
            while self.buffer.lock().unwrap_or_else(|e| e.into_inner()).len() > limit {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod backend {
    pub(super) fn open() -> Option<Box<dyn super::Output>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(m: &mut Mixer, channel: usize, pcm: &[i16], crossfade: bool, looped: bool, fade_in: usize) {
        m.audio(AudioOp::Queue {
            channel,
            sound: Sound::ready(pcm.into()),
            fade_in,
            fade_out: 0,
            crossfade,
            looped,
            volume: (0.0, 0.0),
        });
    }

    #[test]
    fn channels_mix_at_their_levels() {
        let mut m = Mixer::new(audio::RATE);
        // 100 frames at half scale, left only.
        let pcm: Vec<i16> = (0..100).flat_map(|_| [16384, 0]).collect();
        queue(&mut m, 2, &pcm, false, false, 0);
        assert!(!m.idle());
        let out = m.render(150);
        // -12 dB: about a quarter of the half.
        assert!((out[0] - 0.5 * db(-12.0)).abs() < 1e-3, "{}", out[0]);
        assert_eq!(out[1], 0.0);
        assert_eq!(out[120 * 2], 0.0, "finished after 100 frames");
        assert!(m.idle());
        // The channel's level, set to 0 dB.
        m.audio(AudioOp::Volume { channel: 2, left: Some(0.0), right: None, ramp: 0 });
        queue(&mut m, 2, &pcm, false, false, 0);
        assert!((m.render(1)[0] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn resampled_to_the_outputs_rate() {
        // One second of 1 kHz at 44.1 kHz, played on a 48 kHz output.
        let pcm: Vec<i16> = (0..audio::RATE)
            .flat_map(|i| {
                let v = ((i as f64 * 1000.0 / audio::RATE as f64 * std::f64::consts::TAU).sin() * 16000.0) as i16;
                [v, v]
            })
            .collect();
        let mut m = Mixer::new(48_000);
        queue(&mut m, 2, &pcm, false, false, 0);
        let out = m.render(48_000);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let crossings = left.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((1995..=2001).contains(&crossings), "{crossings}");
    }

    #[test]
    fn loops_crossfades_and_flushes() {
        let mut m = Mixer::new(audio::RATE);
        m.audio(AudioOp::Volume { channel: 3, left: Some(0.0), right: Some(0.0), ramp: 0 });
        let a: Vec<i16> = (0..10).flat_map(|_| [8192, 8192]).collect();
        let b: Vec<i16> = (0..1000).flat_map(|_| [-8192, -8192]).collect();
        queue(&mut m, 3, &a, false, true, 0);
        let out = m.render(100);
        assert!(out.iter().all(|&s| (s - 0.25).abs() < 1e-3), "loops for as long as it's let");
        // Crossfade over 50 frames: a fades out as b fades in.
        queue(&mut m, 3, &b, true, false, 50);
        let out = m.render(100);
        assert!((out[0] - 0.25).abs() < 0.01);
        assert!(out[25 * 2].abs() < 0.01, "halfway, they cancel");
        assert!((out[60 * 2] + 0.25).abs() < 1e-3);
        // Flushed, it stops at once.
        m.audio(AudioOp::Flush { channel: 3, fade_out: 0 });
        assert!(m.idle());
    }

    #[test]
    fn square_wave_with_fades() {
        let mut s = Synth::new(8000);
        // 10 ms of 1 kHz: 80 samples, 8 per cycle.
        let tone = Tone { hz: 1000, ms: 10 };
        let v = s.render(tone, 10);
        assert_eq!(v.len(), 80);
        // Faded in from silence, full volume in the middle, faded out.
        assert_eq!(v[0], 0.0);
        assert_eq!(v[40].abs(), VOLUME);
        assert!(v[79].abs() < VOLUME / 4.0);
        // Half the cycle high, half low.
        assert_eq!(v[40..48].iter().filter(|&&x| x > 0.0).count(), 4);
        // Rests are silent.
        assert!(s.render(Tone { hz: 0, ms: 5 }, 5).iter().all(|&x| x == 0.0));
    }
}
