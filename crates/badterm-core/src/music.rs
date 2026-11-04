//! ANSI music: the BASIC `PLAY` language that BBSes send between `ESC [ M`
//! (or `ESC [ N`, `ESC [ |`) and Ctrl-N, turned into tones to play.
//!
//! Supported, as in GW-BASIC: notes `A`-`G` with `#`/`+` (sharp) or `-`
//! (flat), a length and dots; `O` octave, `<` and `>`; `L` default length;
//! `T` tempo; `P` pause; `N` note number; `MN`/`ML`/`MS` normal, legato and
//! staccato. `MF`/`MB` (foreground/background) don't apply to a terminal
//! and are ignored.

/// One sound: a square wave at `hz` (0 is silence) for `ms` milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tone {
    pub hz: u32,
    pub ms: u32,
}

/// Longest piece played from one string, so a runaway string can't keep
/// the speaker going for long.
const MAX_TOTAL_MS: u32 = 5 * 60 * 1000;

#[derive(Clone, Copy)]
enum Style {
    Normal,
    Legato,
    Staccato,
}

/// Parse a music string. Anything not understood is skipped.
pub fn parse(s: &[u8]) -> Vec<Tone> {
    let mut p = Parser { s, i: 0 };
    let (mut octave, mut length, mut tempo, mut style) = (4u32, 4u32, 120u32, Style::Normal);
    let mut out = Vec::new();
    let mut total = 0u32;
    while let Some(c) = p.next() {
        let tone_of = |key: Option<u32>, len: u32, dots: u32, style: Style, tempo: u32| {
            // A whole note is four beats.
            let mut ms = 240_000 / (tempo * len.max(1)) as u64;
            let mut extra = ms;
            for _ in 0..dots {
                extra /= 2;
                ms += extra;
            }
            let ms = ms as u32;
            let Some(key) = key else { return vec![Tone { hz: 0, ms }] };
            let on = match style {
                Style::Normal => ms * 7 / 8,
                Style::Legato => ms,
                Style::Staccato => ms * 3 / 4,
            };
            let mut v = vec![Tone { hz: frequency(key), ms: on }];
            if on < ms {
                v.push(Tone { hz: 0, ms: ms - on });
            }
            v
        };
        let tones = match c.to_ascii_uppercase() {
            n @ b'A'..=b'G' => {
                let mut key = octave as i32 * 12 + semitone(n);
                match p.peek() {
                    Some(b'#' | b'+') => {
                        p.i += 1;
                        key += 1;
                    }
                    Some(b'-') => {
                        p.i += 1;
                        key -= 1;
                    }
                    _ => {}
                }
                let len = p.number().filter(|&n| (1..=64).contains(&n)).unwrap_or(length);
                let dots = p.dots();
                tone_of(Some(key.clamp(0, 83) as u32), len, dots, style, tempo)
            }
            b'N' => match p.number() {
                Some(0) => tone_of(None, length, p.dots(), style, tempo),
                Some(n @ 1..=84) => tone_of(Some(n - 1), length, p.dots(), style, tempo),
                _ => Vec::new(),
            },
            b'P' => {
                let len = p.number().filter(|&n| (1..=64).contains(&n)).unwrap_or(length);
                tone_of(None, len, p.dots(), style, tempo)
            }
            b'O' => {
                if let Some(n) = p.number().filter(|&n| n <= 6) {
                    octave = n;
                }
                Vec::new()
            }
            b'>' => {
                octave = (octave + 1).min(6);
                Vec::new()
            }
            b'<' => {
                octave = octave.saturating_sub(1);
                Vec::new()
            }
            b'L' => {
                if let Some(n) = p.number().filter(|&n| (1..=64).contains(&n)) {
                    length = n;
                }
                Vec::new()
            }
            b'T' => {
                if let Some(n) = p.number().filter(|&n| (32..=255).contains(&n)) {
                    tempo = n;
                }
                Vec::new()
            }
            b'M' => {
                match p.next().map(|c| c.to_ascii_uppercase()) {
                    Some(b'N') => style = Style::Normal,
                    Some(b'L') => style = Style::Legato,
                    Some(b'S') => style = Style::Staccato,
                    _ => {}
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
        for t in tones {
            total = total.saturating_add(t.ms);
            if total > MAX_TOTAL_MS {
                return out;
            }
            // Join consecutive rests.
            match out.last_mut() {
                Some(Tone { hz: 0, ms }) if t.hz == 0 => *ms += t.ms,
                _ => out.push(t),
            }
        }
    }
    out
}

/// Semitones above C.
fn semitone(note: u8) -> i32 {
    match note {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        _ => 11,
    }
}

/// Frequency of key `n` (octave * 12 + semitone). In BASIC, octave 3 starts
/// at middle C, so A in octave 3 (key 45) is 440 Hz.
fn frequency(key: u32) -> u32 {
    (440.0 * 2f64.powf((key as f64 - 45.0) / 12.0)).round() as u32
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    /// The next byte that isn't a space.
    fn next(&mut self) -> Option<u8> {
        while let Some(c) = self.peek() {
            self.i += 1;
            if !c.is_ascii_whitespace() {
                return Some(c);
            }
        }
        None
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }

    fn dots(&mut self) -> u32 {
        let mut n = 0;
        while self.peek() == Some(b'.') {
            self.i += 1;
            n += 1;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(hz: u32, ms: u32) -> Tone {
        Tone { hz, ms }
    }

    #[test]
    fn notes_lengths_and_tempo() {
        // At 120 bpm a quarter note is 500 ms; normal style sounds 7/8.
        assert_eq!(parse(b"O3A"), [t(440, 437), t(0, 63)]);
        assert_eq!(parse(b"ML O3 A8"), [t(440, 250)]);
        assert_eq!(parse(b"MS T60 O3 A"), [t(440, 750), t(0, 250)]);
        // Dotted: 1.5 times.
        assert_eq!(parse(b"MLO3A4."), [t(440, 750)]);
        // Middle C, sharps and flats, octave changes.
        assert_eq!(parse(b"MLO3C"), [t(262, 500)]);
        assert_eq!(parse(b"MLO3C#D-"), [t(277, 500), t(277, 500)]);
        assert_eq!(parse(b"MLO3A>A<<A"), [t(440, 500), t(880, 500), t(220, 500)]);
    }

    #[test]
    fn rests_numbers_and_junk() {
        assert_eq!(parse(b"L8P4"), [t(0, 500)]);
        // N46 is A in octave 3; N0 is a rest. Rests in a row merge.
        assert_eq!(parse(b"MLN46N0P8"), [t(440, 500), t(0, 750)]);
        // Lower case works; unknown letters and out-of-range values are ignored.
        assert_eq!(parse(b"ml o3 a xyz O9 L99 T5"), [t(440, 500)]);
        assert!(parse(b"MFMB").is_empty());
    }

    #[test]
    fn long_strings_are_cut_off() {
        let s = format!("T32L1{}", "C".repeat(1000));
        let total: u32 = parse(s.as_bytes()).iter().map(|t| t.ms).sum();
        assert!(total <= MAX_TOTAL_MS);
    }
}
