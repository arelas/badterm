//! Playing ANSI music: a background thread turns tones into a square wave,
//! like a PC speaker, and feeds the system's audio output (ALSA on Linux,
//! loaded at run time; CoreAudio or WASAPI through cpal elsewhere).
//!
//! If there is no audio output, music is silently skipped.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use phosphor_core::music::Tone;

/// Quiet: a square wave at full scale is harsh.
const VOLUME: f32 = 0.12;
/// Fade in and out over this long, so notes don't click.
const FADE_MS: u32 = 3;
/// Samples synthesized at a time, so a Stop is noticed quickly.
const CHUNK_MS: u32 = 20;

enum Cmd {
    Play(Vec<Tone>),
    Stop,
}

/// The music player. The audio thread and output start on first use.
#[derive(Default)]
pub struct Player {
    tx: Option<Sender<Cmd>>,
}

impl Player {
    /// Queue tones after whatever is playing.
    pub fn play(&mut self, tones: Vec<Tone>) {
        let tx = self.tx.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            thread::Builder::new()
                .name("music".into())
                .spawn(move || run(rx))
                .map_err(|e| log::warn!("starting the music thread: {e}"))
                .ok();
            tx
        });
        let _ = tx.send(Cmd::Play(tones));
    }

    /// Stop playing and forget anything queued.
    pub fn stop(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Stop);
        }
    }
}

/// Where samples go: blocking writes of mono samples in -1..1.
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
    let mut synth = Synth::new(out.rate());
    let mut queue: VecDeque<Tone> = VecDeque::new();
    loop {
        // Wait for something to play.
        if queue.is_empty() {
            match rx.recv() {
                Ok(Cmd::Play(t)) => queue.extend(t),
                Ok(Cmd::Stop) => continue,
                Err(_) => return,
            }
        }
        loop {
            match rx.try_recv() {
                Ok(Cmd::Play(t)) => queue.extend(t),
                Ok(Cmd::Stop) => queue.clear(),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if let Some(tone) = queue.front_mut() {
            let ms = tone.ms.min(CHUNK_MS);
            let samples = synth.render(*tone, ms);
            tone.ms -= ms;
            if tone.ms == 0 {
                queue.pop_front();
                synth.next_note();
            }
            out.write(&samples);
        }
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
    //! ALSA through `dlopen`, so neither building nor running Phosphor needs
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
                log::info!("no sound for ANSI music: {e}");
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
                // Mono 16-bit at 44.1 kHz, resampled by ALSA if need be,
                // with 100 ms of buffering.
                let err = set_params(pcm, FORMAT_S16_LE, ACCESS_RW_INTERLEAVED, 1, RATE, 1, 100_000);
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
                // SAFETY: `rest` is valid for `rest.len()` mono frames.
                let n = unsafe { (self.writei)(self.pcm, rest.as_ptr().cast(), rest.len() as c_ulong) };
                if n >= 0 {
                    done += n as usize;
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
                log::info!("no sound for ANSI music: {e}");
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
                        let v = T::from_sample(buf.pop_front().unwrap_or(0.0));
                        frame.fill(v);
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
            // Keep about 100 ms queued, so Stop takes effect quickly.
            let limit = self.rate as usize / 10;
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
