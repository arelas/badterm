//! Commodore 64 BBSes: PETSCII, on a 40x25 screen in the C64's colors;
//! and the Commodore 128's 80-column screen (its VDC chip): the same
//! PETSCII on 80x25, in the VDC's RGBI colors.
//!
//! Printable bytes become screen codes (the order the C64's characters are
//! stored in, which BadTerm's C64 fonts use; reverse video is the second
//! half of the font), and control bytes move the cursor, change the color,
//! switch between the upper case/graphics and lower/upper case sets, and so
//! on, as the C64's screen editor does.

use crate::ansi::TermEvent;
use crate::palette::Rgb;
use crate::screen::{Color, Screen};

pub const COLS: usize = 40;
pub const ROWS: usize = 25;
/// The CTerm font IDs of the two character sets (as SyncTERM numbers them).
pub const FONT_UPPER: u8 = 32;
pub const FONT_LOWER: u8 = 33;
/// The C128's, for its 80-column screen.
pub const C128_COLS: usize = 80;
pub const FONT_C128_UPPER: u8 = 34;
pub const FONT_C128_LOWER: u8 = 35;

/// The C64's 16 colors (the "Pepto" measurements).
pub const PALETTE: [Rgb; 16] = [
    0x000000, 0xFFFFFF, 0x68372B, 0x70A4B2, 0x6F3D86, 0x588D43, 0x352879, 0xB8C76F, 0x6F4F25, 0x433900, 0x9A6759,
    0x444444, 0x6C6C6C, 0x9AD284, 0x6C5EB5, 0x959595,
];
/// The C128's 80-column colors, in the C64's order (RGBI, as on a CGA
/// monitor): black, white, dark red, light cyan, light purple, dark
/// green, dark blue, light yellow, dark purple, brown, light red, dark
/// cyan, dark gray, light green, light blue, light gray.
pub const VDC_PALETTE: [Rgb; 16] = [
    0x000000, 0xFFFFFF, 0xAA0000, 0x55FFFF, 0xFF55FF, 0x00AA00, 0x0000AA, 0xFFFF55, 0xAA00AA, 0xAA5500, 0xFF5555,
    0x00AAAA, 0x555555, 0x55FF55, 0x5555FF, 0xAAAAAA,
];
/// Light blue text on black, as C64 terminal programs start.
const DEFAULT_FG: usize = 14;

pub fn color(i: usize) -> Color {
    Color::Rgb(PALETTE[i & 15])
}

/// The C64 screen editor's state.
#[derive(Debug, Default)]
pub struct Petscii {
    reverse: bool,
    /// The C128's 80-column screen, not the C64's.
    c128: bool,
}

impl Petscii {
    pub fn c128() -> Petscii {
        Petscii { reverse: false, c128: true }
    }

    fn color(&self, i: usize) -> Color {
        Color::Rgb(if self.c128 { VDC_PALETTE[i & 15] } else { PALETTE[i & 15] })
    }

    fn fonts(&self) -> (u8, u8) {
        if self.c128 { (FONT_C128_UPPER, FONT_C128_LOWER) } else { (FONT_UPPER, FONT_LOWER) }
    }

    /// A fresh C64 screen: cleared, colors and character set as at power-on.
    pub fn start(&mut self, s: &mut Screen) {
        self.reverse = false;
        // The C128 starts light cyan in 80 columns.
        s.pen.fg = self.color(if self.c128 { 3 } else { DEFAULT_FG });
        s.pen.bg = self.color(0);
        let (upper, _) = self.fonts();
        s.look.fonts[0] = upper;
        s.base_font = upper;
        s.autowrap = true;
        s.erase_display(2);
        s.move_to(0, 0);
    }

    pub fn byte(&mut self, s: &mut Screen, b: u8, events: &mut Vec<TermEvent>) {
        let pal = if self.c128 { VDC_PALETTE } else { PALETTE };
        let ink = |s: &mut Screen, i: usize| s.pen.fg = Color::Rgb(pal[i & 15]);
        let (upper, lower) = self.fonts();
        match b {
            0x05 => ink(s, 1),
            0x07 => events.push(TermEvent::Bell),
            0x0D | 0x8D => {
                s.carriage_return();
                s.line_feed();
                self.reverse = false;
            }
            0x0E => s.look.fonts[0] = lower,
            0x8E => s.look.fonts[0] = upper,
            0x11 => s.move_by(0, 1),
            0x91 => s.move_by(0, -1),
            0x1D => right(s),
            0x9D => left(s),
            0x12 => self.reverse = true,
            0x92 => self.reverse = false,
            0x13 => s.move_to(0, 0),
            0x93 => {
                s.erase_display(2);
                s.move_to(0, 0);
            }
            // DEL: back one, closing up the line.
            0x14 => {
                if s.cursor().0 > 0 {
                    s.move_by(-1, 0);
                    s.delete_chars(1);
                }
            }
            0x94 => s.insert_chars(1),
            0x1C => ink(s, 2),
            0x1E => ink(s, 5),
            0x1F => ink(s, 6),
            0x81 => ink(s, 8),
            0x90 => ink(s, 0),
            0x95 => ink(s, 9),
            0x96 => ink(s, 10),
            0x97 => ink(s, 11),
            0x98 => ink(s, 12),
            0x99 => ink(s, 13),
            0x9A => ink(s, 14),
            0x9B => ink(s, 15),
            0x9C => ink(s, 4),
            0x9E => ink(s, 7),
            0x9F => ink(s, 3),
            _ => {
                if let Some(code) = screen_code(b) {
                    s.put(if self.reverse { code | 0x80 } else { code });
                }
            }
        }
    }
}

/// Cursor right, on to the next line at the end of one.
fn right(s: &mut Screen) {
    let (x, y) = s.cursor();
    if x + 1 < s.cols() {
        s.move_by(1, 0);
    } else if y + 1 < s.rows() {
        s.move_to(0, y + 1);
    } else {
        s.carriage_return();
        s.line_feed();
    }
}

/// Cursor left, back to the end of the line above at the start of one.
fn left(s: &mut Screen) {
    let (x, y) = s.cursor();
    if x > 0 {
        s.move_by(-1, 0);
    } else if y > 0 {
        s.move_to(s.cols() - 1, y - 1);
    }
}

/// The screen code of a printable PETSCII byte; None for the controls.
pub fn screen_code(b: u8) -> Option<u8> {
    Some(match b {
        0x20..=0x3F => b,
        0x40..=0x5F => b - 0x40,
        0x60..=0x7F => b - 0x20,
        0xA0..=0xBF => b - 0x40,
        0xC0..=0xFE => b - 0x80,
        0xFF => 0x5E,
        _ => return None,
    })
}

/// What a typed character sends to a C64 BBS. In the C64's default case,
/// unshifted letters show as capitals, so lower-case letters send those
/// codes and capitals the shifted ones.
pub fn encode_char(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 0x41),
        'A'..='Z' => Some(c as u8 - b'A' + 0xC1),
        ' '..='@' | '[' | ']' => Some(c as u8),
        '\u{a3}' => Some(0x5C),
        '\u{2191}' | '^' => Some(0x5E),
        '\u{2190}' | '_' => Some(0x5F),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::Screen;

    fn run(bytes: &[u8]) -> (Screen, Petscii) {
        let mut s = Screen::new(COLS, ROWS, 0);
        let mut p = Petscii::default();
        p.start(&mut s);
        let mut ev = Vec::new();
        for &b in bytes {
            p.byte(&mut s, b, &mut ev);
        }
        (s, p)
    }

    #[test]
    fn text_colors_and_reverse() {
        // "HI" in white, then reversed red "A", then a newline.
        let (s, _) = run(b"\x05HI\x1c\x12A\x92\x0dX");
        let row = s.row(0);
        assert_eq!((row[0].ch, row[1].ch), (0x08, 0x09));
        assert_eq!(row[0].fg, color(1));
        assert_eq!((row[2].ch, row[2].fg), (0x01 | 0x80, color(2)));
        assert_eq!(s.row(1)[0].ch, 0x18);
        assert_eq!(s.cursor(), (1, 1));
    }

    #[test]
    fn cursor_clear_and_charsets() {
        let (s, _) = run(b"ABC\x9d\x9d\x14\x13\x11\x1d*\x0e");
        // On B, DEL takes the A before it and closes up the line.
        assert_eq!((s.row(0)[0].ch, s.row(0)[1].ch), (0x02, 0x03));
        assert_eq!(s.row(1)[1].ch, b'*');
        assert_eq!(s.look.fonts[0], FONT_LOWER);
        let (s, _) = run(b"ABC\x93");
        assert!(s.row(0).iter().all(|c| c.ch == b' '));
        assert_eq!(s.cursor(), (0, 0));
        // Graphics: shifted bytes and their copies land on the same glyph.
        assert_eq!(screen_code(0xC1), screen_code(0x61));
        assert_eq!(screen_code(0xA0), Some(0x60));
        assert_eq!((encode_char('a'), encode_char('A'), encode_char('1')), (Some(0x41), Some(0xC1), Some(b'1')));
    }
}
