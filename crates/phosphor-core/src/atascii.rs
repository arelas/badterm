//! Atari 8-bit BBSes: ATASCII, on the 40x24 screen of the Atari's text
//! mode, light text on blue.
//!
//! Bytes 0x00-0x7F are characters (in ATASCII order, which Phosphor's Atari
//! font uses), the same with the top bit set are shown in inverse, and the
//! Atari's screen editor codes (EOL, cursor keys, clear, delete and insert)
//! are carried out. ESC shows the next byte as a character instead.

use crate::ansi::TermEvent;
use crate::screen::{Color, Screen};

pub const COLS: usize = 40;
pub const ROWS: usize = 24;
/// The CTerm font ID of the Atari font (as SyncTERM numbers it).
pub const FONT: u8 = 36;

/// The Atari's default text colors.
pub const BACKGROUND: Color = Color::Rgb(0x00_52_A5);
pub const TEXT: Color = Color::Rgb(0xA8_D0_FF);

#[derive(Debug, Default)]
pub struct Atascii {
    /// The last byte was ESC: show this one as it is.
    literal: bool,
}

impl Atascii {
    pub fn start(&mut self, s: &mut Screen) {
        self.literal = false;
        s.pen.fg = TEXT;
        s.pen.bg = BACKGROUND;
        s.look.fonts[0] = FONT;
        s.base_font = FONT;
        s.autowrap = true;
        s.erase_display(2);
        s.move_to(0, 0);
    }

    pub fn byte(&mut self, s: &mut Screen, b: u8, events: &mut Vec<TermEvent>) {
        if std::mem::take(&mut self.literal) {
            s.put(b);
            return;
        }
        match b {
            0x1B => self.literal = true,
            0x9B => {
                s.carriage_return();
                s.line_feed();
            }
            // The cursor keys wrap around within the screen, as on an Atari.
            0x1C => {
                let (x, y) = s.cursor();
                s.move_to(x, (y + s.rows() - 1) % s.rows());
            }
            0x1D => {
                let (x, y) = s.cursor();
                s.move_to(x, (y + 1) % s.rows());
            }
            0x1E => {
                let (x, y) = s.cursor();
                s.move_to((x + s.cols() - 1) % s.cols(), y);
            }
            0x1F => {
                let (x, y) = s.cursor();
                s.move_to((x + 1) % s.cols(), y);
            }
            0x7D => {
                s.erase_display(2);
                s.move_to(0, 0);
            }
            // Backspace: back one and blank it.
            0x7E => {
                if s.cursor().0 > 0 {
                    s.move_by(-1, 0);
                    s.erase_chars(1);
                }
            }
            0x7F => s.tab(),
            0x9C => s.delete_lines(1),
            0x9D => s.insert_lines(1),
            0xFD => events.push(TermEvent::Bell),
            0xFE => s.delete_chars(1),
            0xFF => s.insert_chars(1),
            // Tab stops: not kept; tabs go to the next multiple of 8.
            0x9E | 0x9F => {}
            _ => s.put(b),
        }
    }
}

/// What a typed character sends to an Atari BBS: ATASCII is ASCII for the
/// letters, digits and punctuation.
pub fn encode_char(c: char) -> Option<u8> {
    matches!(c, ' '..='z' | '|').then_some(c as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eol_inverse_and_editing() {
        let mut s = Screen::new(COLS, ROWS, 0);
        let mut a = Atascii::default();
        a.start(&mut s);
        let mut ev = Vec::new();
        for &b in b"HI\xc1\x9bAB\x7e\x1b\x1c\x9b\x7dX\xfd" {
            a.byte(&mut s, b, &mut ev);
        }
        // Cleared by 0x7D, then X.
        assert_eq!(s.row(0)[0].ch, b'X');
        assert_eq!(ev, vec![TermEvent::Bell]);

        let mut s = Screen::new(COLS, ROWS, 0);
        a.start(&mut s);
        for &b in b"HI\xc1\x9bAB\x7e\x1b\x1c" {
            a.byte(&mut s, b, &mut ev);
        }
        assert_eq!(&[s.row(0)[0].ch, s.row(0)[1].ch, s.row(0)[2].ch], b"HI\xc1");
        // B rubbed out, then ESC shows 0x1C (an arrow) instead of moving.
        assert_eq!(&[s.row(1)[0].ch, s.row(1)[1].ch], b"A\x1c");
        assert_eq!(s.row(0)[0].bg, BACKGROUND);
    }
}
