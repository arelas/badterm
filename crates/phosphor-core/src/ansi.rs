//! ANSI-BBS / ANSI X3.64 terminal emulation.
//!
//! [`Terminal`] consumes the byte stream from the remote system, drives a
//! [`Screen`], and collects any replies (cursor position reports, etc.) that
//! must be sent back.

use serde::{Deserialize, Serialize};

use crate::cp437;
use crate::music::{self, Tone};
use crate::palette::{ANSI_TO_VGA, rgb};
use crate::screen::{Cell, Color, DEFAULT_BG, DEFAULT_FG, Screen, flags};

/// Character set used to interpret incoming bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    #[default]
    Cp437,
    Utf8,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Cp437 => "CP437",
            Encoding::Utf8 => "UTF-8",
        }
    }
}

const MAX_PARAMS: usize = 32;
const MAX_STRING: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// ESC followed by an intermediate byte, e.g. `ESC ( B`; waiting for the final byte.
    EscIntermediate,
    Csi,
    /// OSC: collected for the window title.
    Osc,
    /// DCS, SOS, PM, APC: ignored until ST.
    IgnoreString,
    /// Saw ESC inside a string; `\` terminates it.
    StringEsc,
    /// An ANSI music string, up to Ctrl-N.
    Music,
}

/// Things the front end may want to react to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermEvent {
    Bell,
    Title(String),
    /// ANSI music to play.
    Music(Vec<Tone>),
}

pub struct Terminal {
    pub screen: Screen,
    pub encoding: Encoding,
    /// Treat `ESC [ M` with no parameter as the start of ANSI music, as
    /// many BBS doors expect, instead of "delete one line". `ESC [ N` and
    /// `ESC [ |` always start music.
    pub music_on_csi_m: bool,
    state: State,
    string_is_osc: bool,
    private: u8,
    intermediate: u8,
    params: Vec<u32>,
    param_present: bool,
    osc: Vec<u8>,
    utf8_buf: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,
    replies: Vec<u8>,
    events: Vec<TermEvent>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        Terminal {
            screen: Screen::new(cols, rows, scrollback),
            encoding: Encoding::Cp437,
            music_on_csi_m: false,
            state: State::Ground,
            string_is_osc: false,
            private: 0,
            intermediate: 0,
            params: Vec::with_capacity(MAX_PARAMS),
            param_present: false,
            osc: Vec::new(),
            utf8_buf: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
            replies: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Bytes the terminal wants sent to the remote (e.g. CPR replies).
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub fn take_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn feed(&mut self, data: &[u8]) {
        for &b in data {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        // CAN and SUB abort any sequence in progress (ECMA-48).
        if self.state != State::Ground && (b == 0x18 || b == 0x1A) {
            self.state = State::Ground;
            return;
        }
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::EscIntermediate => self.state = State::Ground,
            State::Csi => self.csi(b),
            State::Osc | State::IgnoreString => match b {
                0x07 if self.state == State::Osc => self.finish_string(),
                0x1B => self.state = State::StringEsc,
                _ if self.state == State::Osc && self.osc.len() < MAX_STRING => self.osc.push(b),
                _ => {}
            },
            State::Music => match b {
                0x0E => {
                    self.state = State::Ground;
                    let tones = music::parse(&self.osc);
                    if !tones.is_empty() {
                        self.events.push(TermEvent::Music(tones));
                    }
                }
                _ if self.osc.len() < MAX_STRING => self.osc.push(b),
                // Too long to be music: give up on it.
                _ => self.state = State::Ground,
            },
            State::StringEsc => {
                if b == b'\\' {
                    self.finish_string();
                } else {
                    // Not ST: abandon the string and treat this as a new escape.
                    self.state = State::Escape;
                    self.escape(b);
                }
            }
        }
    }

    fn ground(&mut self, b: u8) {
        if self.encoding == Encoding::Utf8 && self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8_buf[self.utf8_len] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_need {
                    let ch = std::str::from_utf8(&self.utf8_buf[..self.utf8_len]).ok().and_then(|s| s.chars().next());
                    self.utf8_need = 0;
                    self.screen.put(ch.and_then(cp437::from_unicode).unwrap_or(b'?'));
                }
                return;
            }
            // Truncated sequence: show a replacement and handle this byte normally.
            self.utf8_need = 0;
            self.screen.put(b'?');
        }
        match b {
            0x00 | 0x7F => {}
            0x07 => self.events.push(TermEvent::Bell),
            0x08 => self.screen.backspace(),
            0x09 => self.screen.tab(),
            0x0A => self.screen.line_feed(),
            0x0C => self.screen.erase_display(2),
            0x0D => self.screen.carriage_return(),
            0x1B => self.state = State::Escape,
            _ => match self.encoding {
                // Like ANSI.SYS, every other byte is a glyph, including the
                // "graphic" control characters (☺, ♥, ►, ...).
                Encoding::Cp437 => self.screen.put(b),
                Encoding::Utf8 => match b {
                    0x01..=0x1F => {}
                    0x20..=0x7E => self.screen.put(b),
                    0xC2..=0xDF => self.utf8_start(b, 2),
                    0xE0..=0xEF => self.utf8_start(b, 3),
                    0xF0..=0xF4 => self.utf8_start(b, 4),
                    _ => self.screen.put(b'?'),
                },
            },
        }
    }

    fn utf8_start(&mut self, b: u8, need: usize) {
        self.utf8_buf[0] = b;
        self.utf8_len = 1;
        self.utf8_need = need;
    }

    fn escape(&mut self, b: u8) {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.state = State::Csi;
                self.private = 0;
                self.intermediate = 0;
                self.params.clear();
                self.param_present = false;
            }
            b']' => {
                self.state = State::Osc;
                self.string_is_osc = true;
                self.osc.clear();
            }
            b'P' | b'X' | b'^' | b'_' => {
                self.state = State::IgnoreString;
                self.string_is_osc = false;
            }
            0x20..=0x2F => self.state = State::EscIntermediate,
            b'7' => self.screen.save_cursor(),
            b'8' => self.screen.restore_cursor(),
            b'c' => self.screen.reset(),
            b'D' => self.screen.line_feed(),
            b'E' => {
                self.screen.carriage_return();
                self.screen.line_feed();
            }
            b'M' => self.screen.reverse_index(),
            0x1B => self.state = State::Escape,
            _ => log::debug!("unhandled ESC {:?}", b as char),
        }
    }

    fn finish_string(&mut self) {
        self.state = State::Ground;
        if !self.string_is_osc {
            return;
        }
        // OSC 0 / OSC 2: set window title.
        let s = String::from_utf8_lossy(&self.osc).into_owned();
        if let Some((code, title)) = s.split_once(';')
            && (code == "0" || code == "2")
        {
            self.events.push(TermEvent::Title(title.to_string()));
        }
    }

    fn csi(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                if self.params.is_empty() {
                    self.params.push(0);
                }
                let p = self.params.last_mut().unwrap();
                *p = p.saturating_mul(10).saturating_add((b - b'0') as u32);
                self.param_present = true;
            }
            b';' | b':' => {
                if self.params.is_empty() {
                    self.params.push(0);
                }
                if self.params.len() < MAX_PARAMS {
                    self.params.push(0);
                }
                self.param_present = true;
            }
            0x3C..=0x3F => {
                if !self.param_present && self.private == 0 {
                    self.private = b;
                }
            }
            0x20..=0x2F => self.intermediate = b,
            0x40..=0x7E => {
                self.state = State::Ground;
                self.dispatch_csi(b);
            }
            // C0 controls are executed even in the middle of a sequence.
            0x00..=0x1F => {
                if b == 0x1B {
                    self.state = State::Escape;
                } else {
                    self.ground(b);
                }
            }
            _ => self.state = State::Ground,
        }
    }

    /// Parameter `i`, with 0 or missing replaced by `default`.
    fn p(&self, i: usize, default: u32) -> u32 {
        match self.params.get(i) {
            Some(&v) if v != 0 => v,
            _ => default,
        }
    }

    fn dispatch_csi(&mut self, b: u8) {
        if self.intermediate != 0 {
            // e.g. CTerm font selection `CSI n ; m SP D`; not yet supported.
            log::debug!("unhandled CSI with intermediate {:?} {:?}", self.intermediate as char, b as char);
            return;
        }
        match self.private {
            0 => {}
            b'?' => return self.dec_mode(b),
            _ => {
                log::debug!("unhandled private CSI {:?}{:?} {:?}", self.private as char, b as char, self.params);
                return;
            }
        }
        let no_params = self.params.is_empty();
        if b == b'|' || no_params && (b == b'N' || b == b'M' && self.music_on_csi_m) {
            self.state = State::Music;
            self.osc.clear();
            return;
        }
        let n = self.p(0, 1) as usize;
        let s = &mut self.screen;
        match b {
            b'@' => s.insert_chars(n),
            b'A' => s.move_by(0, -(n as isize)),
            b'B' | b'e' => s.move_by(0, n as isize),
            b'C' | b'a' => s.move_by(n as isize, 0),
            b'D' => s.move_by(-(n as isize), 0),
            b'E' => {
                s.move_by(0, n as isize);
                s.carriage_return();
            }
            b'F' => {
                s.move_by(0, -(n as isize));
                s.carriage_return();
            }
            b'G' | b'`' => {
                let y = s.cursor().1;
                s.move_to(n - 1, y);
            }
            b'd' => {
                let x = s.cursor().0;
                s.move_to(x, n - 1);
            }
            b'H' | b'f' => {
                let (row, col) = (self.p(0, 1) as usize, self.p(1, 1) as usize);
                self.screen.move_to(col - 1, row - 1);
            }
            b'J' => s.erase_display(self.params.first().copied().unwrap_or(0)),
            b'K' => s.erase_line(self.params.first().copied().unwrap_or(0)),
            b'L' => s.insert_lines(n),
            b'M' => s.delete_lines(n),
            b'P' => s.delete_chars(n),
            b'S' => s.scroll_up(n),
            b'T' => s.scroll_down(n),
            b'X' => s.erase_chars(n),
            b'Z' => s.back_tab(n),
            // Device Attributes: answer as a plain VT100. Claiming CTerm would
            // invite font and palette sequences that are not supported yet.
            b'c' if self.params.first().is_none_or(|&v| v == 0) => self.replies.extend_from_slice(b"\x1b[?1;0c"),
            b'm' => self.sgr(),
            b'n' => match self.params.first().copied().unwrap_or(0) {
                5 => self.replies.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let (x, y) = self.screen.cursor();
                    self.replies.extend_from_slice(format!("\x1b[{};{}R", y + 1, x + 1).as_bytes());
                }
                _ => {}
            },
            b'r' => {
                let rows = self.screen.rows() as u32;
                let (top, bottom) = (self.p(0, 1), self.p(1, rows));
                self.screen.set_scroll_region(top as usize - 1, bottom as usize - 1);
            }
            b's' => s.save_cursor(),
            b'u' => s.restore_cursor(),
            // CTerm/PabloDraw 24-bit color: CSI 0;r;g;b t (bg) / CSI 1;r;g;b t (fg).
            b't' if self.params.len() == 4 => {
                let c = Color::Rgb(rgb(self.params[1] as u8, self.params[2] as u8, self.params[3] as u8));
                match self.params[0] {
                    0 => self.screen.pen.bg = c,
                    1 => self.screen.pen.fg = c,
                    _ => {}
                }
            }
            b'h' | b'l' => {}
            _ => log::debug!("unhandled CSI {:?} {:?}", b as char, self.params),
        }
    }

    fn dec_mode(&mut self, b: u8) {
        let on = match b {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        for &mode in &self.params {
            match mode {
                7 => self.screen.autowrap = on,
                25 => self.screen.cursor_visible = on,
                // CTerm: blink attribute selects bright background (iCE colors).
                33 => self.screen.ice_colors = on,
                _ => log::debug!("unhandled DEC mode {mode} {on}"),
            }
        }
    }

    fn sgr(&mut self) {
        if self.params.is_empty() {
            self.params.push(0);
        }
        let pen = &mut self.screen.pen;
        let mut i = 0;
        while i < self.params.len() {
            let p = self.params[i];
            match p {
                0 => *pen = Cell { ch: b' ', ..Cell::default() },
                1 => pen.flags = (pen.flags | flags::BOLD) & !flags::FAINT,
                2 => pen.flags = (pen.flags | flags::FAINT) & !flags::BOLD,
                4 => pen.flags |= flags::UNDERLINE,
                5 | 6 => pen.flags |= flags::BLINK,
                7 => pen.flags |= flags::REVERSE,
                8 => pen.flags |= flags::CONCEAL,
                21 | 22 => pen.flags &= !(flags::BOLD | flags::FAINT),
                24 => pen.flags &= !flags::UNDERLINE,
                25 => pen.flags &= !flags::BLINK,
                27 => pen.flags &= !flags::REVERSE,
                28 => pen.flags &= !flags::CONCEAL,
                30..=37 => pen.fg = Color::Pc(ANSI_TO_VGA[(p - 30) as usize]),
                39 => pen.fg = DEFAULT_FG,
                40..=47 => pen.bg = Color::Pc(ANSI_TO_VGA[(p - 40) as usize]),
                49 => pen.bg = DEFAULT_BG,
                90..=97 => pen.fg = Color::Pc(ANSI_TO_VGA[(p - 90) as usize] + 8),
                100..=107 => pen.bg = Color::Pc(ANSI_TO_VGA[(p - 100) as usize] + 8),
                38 | 48 => {
                    let color = match self.params.get(i + 1) {
                        Some(5) => {
                            let c = self.params.get(i + 2).map(|&v| Color::Xterm(v as u8));
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = (i + 4 < self.params.len()).then(|| {
                                let v = &self.params[i + 2..i + 5];
                                Color::Rgb(rgb(v[0] as u8, v[1] as u8, v[2] as u8))
                            });
                            i += 4;
                            c
                        }
                        _ => None,
                    };
                    if let Some(c) = color {
                        if p == 38 { pen.fg = c } else { pen.bg = c }
                    }
                }
                _ => log::debug!("unhandled SGR {p}"),
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term() -> Terminal {
        Terminal::new(80, 25, 100)
    }

    fn line(t: &Terminal, y: usize) -> String {
        Screen::row_text(t.screen.row(y))
    }

    #[test]
    fn cursor_position_and_text() {
        let mut t = term();
        t.feed(b"\x1b[3;5HHi");
        assert_eq!(line(&t, 2), "    Hi");
        assert_eq!(t.screen.cursor(), (6, 2));
    }

    #[test]
    fn sgr_colors() {
        let mut t = term();
        t.feed(b"\x1b[1;31;44mX");
        let c = t.screen.row(0)[0];
        assert_eq!(c.fg, Color::Pc(4));
        assert_eq!(c.bg, Color::Pc(1));
        assert!(c.flags & flags::BOLD != 0);
        t.feed(b"\x1b[0;38;2;10;20;30mY");
        assert_eq!(t.screen.row(0)[1].fg, Color::Rgb(0x0A141E));
        t.feed(b"\x1b[38;5;196mZ");
        assert_eq!(t.screen.row(0)[2].fg, Color::Xterm(196));
    }

    #[test]
    fn cursor_position_report() {
        let mut t = term();
        t.feed(b"\x1b[10;20H\x1b[6n");
        assert_eq!(t.take_replies(), b"\x1b[10;20R");
    }

    #[test]
    fn device_attributes() {
        let mut t = term();
        t.feed(b"\x1b[c\x1b[0c");
        assert_eq!(t.take_replies(), b"\x1b[?1;0c\x1b[?1;0c");
        // Secondary DA (`CSI > c`) and `CSI 1 c` are not answered.
        t.feed(b"\x1b[>c\x1b[1c");
        assert!(t.take_replies().is_empty());
    }

    #[test]
    fn ice_mode_toggle() {
        let mut t = term();
        t.feed(b"\x1b[?33h");
        assert!(t.screen.ice_colors);
        t.feed(b"\x1b[?33l");
        assert!(!t.screen.ice_colors);
    }

    #[test]
    fn cp437_glyphs_and_utf8() {
        let mut t = term();
        t.feed(&[0xDB, 0x03]);
        assert_eq!(t.screen.row(0)[0].ch, 0xDB);
        assert_eq!(t.screen.row(0)[1].ch, 0x03);
        t.encoding = Encoding::Utf8;
        t.feed("█é".as_bytes());
        assert_eq!(t.screen.row(0)[2].ch, 0xDB);
        assert_eq!(t.screen.row(0)[3].ch, 0x82);
    }

    #[test]
    fn osc_title_and_strings_ignored() {
        let mut t = term();
        t.feed(b"\x1b]0;My BBS\x07\x1bPjunk\x1b\\ok");
        assert_eq!(t.take_events(), vec![TermEvent::Title("My BBS".into())]);
        assert_eq!(line(&t, 0), "ok");
    }

    #[test]
    fn ansi_art_line_of_exactly_80_columns() {
        let mut t = term();
        let mut data = vec![b'#'; 80];
        data.extend_from_slice(b"\r\nnext");
        t.feed(&data);
        assert_eq!(line(&t, 1), "next");
    }

    #[test]
    fn save_restore_cursor() {
        let mut t = term();
        t.feed(b"\x1b[5;5H\x1b[s\x1b[1;1H\x1b[uX");
        assert_eq!(t.screen.row(4)[4].ch, b'X');
    }

    #[test]
    fn ansi_music() {
        let mut t = term();
        t.feed(b"Hi\x1b[NMLO3A\x0e there\x1b[|MLO3A8\x0e");
        assert_eq!(line(&t, 0), "Hi there");
        let a = Tone { hz: 440, ms: 500 };
        assert_eq!(t.take_events(), [TermEvent::Music(vec![a]), TermEvent::Music(vec![Tone { ms: 250, ..a }])]);

        // ESC [ M is delete-line unless music is wanted there.
        t.feed(b"\x1b[2;1Hx\x1b[1;1H\x1b[M");
        assert_eq!(line(&t, 0), "x");
        t.music_on_csi_m = true;
        t.feed(b"\x1b[MMLO3A\x0e\x1b[1M");
        assert_eq!(t.take_events(), [TermEvent::Music(vec![a])]);
        assert_eq!(line(&t, 0), "");
    }
}
