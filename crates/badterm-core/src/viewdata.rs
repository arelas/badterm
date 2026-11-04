//! Viewdata BBSes (Prestel, and the teletext look): a 40x24 page of text
//! and 2x3 block "mosaic" graphics in eight colors, as a Prestel terminal
//! showed them, following CTerm's description of its Prestel mode.
//!
//! Colors and the like are *serial attributes*: control codes that take a
//! cell of their own and change the cells after them on the same row.
//! So, as a real decoder does, the page keeps the bytes as received and
//! each row is drawn again from left to right when anything changes;
//! overwriting a code part way along a row recolors the rest of it.
//!
//! The glyphs are the font with ID [`FONT`], which the front end makes:
//! teletext's alphanumerics at their own codes, drawn from the CP437 font,
//! the UK set's £, ½, ¼, ¾, ‖, ─, ■ and ÷ in the control-code slots (see
//! [`glyph`]), and the mosaics at 0x80 + the six bits of the pattern
//! (contiguous) and 0xC0 + them (separated).

use crate::ansi::TermEvent;
use crate::palette::Rgb;
use crate::screen::{Cell, Color, Screen, flags};

pub const COLS: usize = 40;
pub const ROWS: usize = 24;
/// The (BadTerm-only) font ID of the teletext glyphs.
pub const FONT: u8 = 250;

/// Black, red, green, yellow, blue, magenta, cyan, white.
pub const PALETTE: [Rgb; 8] = [0x000000, 0xFF0000, 0x00FF00, 0xFFFF00, 0x0000FF, 0xFF00FF, 0x00FFFF, 0xFFFFFF];

fn color(i: u8) -> Color {
    Color::Rgb(PALETTE[(i & 7) as usize])
}

/// The glyph (in the [`FONT`] font) for a teletext alphanumeric, 0x20-0x7F.
/// UK Prestel's set differs from ASCII in a few places.
pub fn glyph(c: u8) -> u8 {
    match c {
        b'#' => GLYPH_POUND,
        b'[' => 0x1B, // ←, as in CP437
        b'\\' => GLYPH_HALF,
        b']' => 0x1A, // →
        b'^' => 0x18, // ↑
        b'_' => b'#',
        b'`' => GLYPH_DASH,
        b'{' => GLYPH_QUARTER,
        b'|' => GLYPH_BARS,
        b'}' => GLYPH_THREE_QUARTERS,
        b'~' => GLYPH_DIVIDE,
        0x7F => GLYPH_BLOCK,
        c => c,
    }
}

/// Where the [`FONT`] font keeps the UK set's own characters.
pub const GLYPH_POUND: u8 = 0x01;
pub const GLYPH_HALF: u8 = 0x02;
pub const GLYPH_QUARTER: u8 = 0x03;
pub const GLYPH_BARS: u8 = 0x04;
pub const GLYPH_DASH: u8 = 0x05;
pub const GLYPH_BLOCK: u8 = 0x06;
pub const GLYPH_THREE_QUARTERS: u8 = 0x07;
pub const GLYPH_DIVIDE: u8 = 0x0E;

/// A byte that is a serial attribute, stored as its code minus 0x40.
fn is_code(b: u8) -> bool {
    b < 0x20
}

#[derive(Debug)]
pub struct Viewdata {
    /// What was received, by position: printable bytes, or attribute
    /// codes (0x00-0x1F).
    page: [[u8; COLS]; ROWS],
    x: usize,
    y: usize,
    /// ESC was the last byte: the next is an attribute.
    escape: bool,
}

impl Default for Viewdata {
    fn default() -> Viewdata {
        Viewdata { page: [[b' '; COLS]; ROWS], x: 0, y: 0, escape: false }
    }
}

impl Viewdata {
    pub fn start(&mut self, s: &mut Screen) {
        *self = Viewdata::default();
        s.look.fonts[0] = FONT;
        s.base_font = FONT;
        s.autowrap = false;
        s.ice_colors = false;
        self.draw(s);
    }

    /// Bytes from the BBS: the page changes, then the screen is drawn from it.
    pub fn feed(&mut self, s: &mut Screen, data: &[u8], events: &mut Vec<TermEvent>) {
        for &b in data {
            self.byte(s, b, events);
        }
        self.draw(s);
    }

    fn byte(&mut self, s: &mut Screen, b: u8, events: &mut Vec<TermEvent>) {
        let b = if b >= 0xA0 { b & 0x7F } else { b };
        if self.escape {
            self.escape = false;
            // ESC and a code: a serial attribute (other ESC sequences,
            // Prestel's memory programming, are ignored).
            if (0x40..=0x5F).contains(&b) {
                self.put(b - 0x40);
            }
            return;
        }
        match b {
            0x07 => events.push(TermEvent::Bell),
            0x08 => self.back(),
            0x09 => self.forward(),
            0x0A => self.y = (self.y + 1) % ROWS,
            0x0B => self.y = (self.y + ROWS - 1) % ROWS,
            0x0C => {
                self.page = [[b' '; COLS]; ROWS];
                (self.x, self.y) = (0, 0);
            }
            0x0D => self.x = 0,
            0x11 => s.cursor_visible = true,
            0x14 => s.cursor_visible = false,
            0x1B => self.escape = true,
            0x1E => (self.x, self.y) = (0, 0),
            0x20..=0x7F => self.put(b),
            // Attributes sent as C1 bytes.
            0x80..=0x9F => self.put(b - 0x80),
            _ => {}
        }
    }

    fn put(&mut self, b: u8) {
        self.page[self.y][self.x] = b;
        self.forward();
    }

    /// The next position: rows wrap, and the bottom row wraps to the top.
    fn forward(&mut self) {
        self.x += 1;
        if self.x == COLS {
            self.x = 0;
            self.y = (self.y + 1) % ROWS;
        }
    }

    fn back(&mut self) {
        if self.x == 0 {
            self.x = COLS - 1;
            self.y = (self.y + ROWS - 1) % ROWS;
        } else {
            self.x -= 1;
        }
    }

    /// Draw the page on the screen, row by row.
    pub fn draw(&self, s: &mut Screen) {
        let mut above: Option<[Cell; COLS]> = None;
        for y in 0..ROWS.min(s.rows()) {
            let (row, top) = draw_row(&self.page[y]);
            let cells = match above.take() {
                // The row under a double-height row shows its bottom half.
                Some(top_cells) => top_cells.map(|c| {
                    if c.flags & flags::DOUBLE_TOP != 0 {
                        Cell { flags: (c.flags & !flags::DOUBLE_TOP) | flags::DOUBLE_BOTTOM, ..c }
                    } else {
                        Cell { ch: b' ', flags: c.flags & !flags::BLINK, ..c }
                    }
                }),
                None => {
                    if top && y + 1 < ROWS {
                        above = Some(row);
                    }
                    if top && y + 1 == ROWS {
                        row.map(|c| Cell { flags: c.flags & !flags::DOUBLE_TOP, ..c })
                    } else {
                        row
                    }
                }
            };
            for (x, c) in cells.iter().enumerate() {
                if let Some(cell) = s.cell_mut(x, y) {
                    *cell = *c;
                }
            }
        }
        s.move_to(self.x, self.y);
    }
}

/// One row of the page as cells, applying its serial attributes from the
/// left; and whether it has double height in it.
fn draw_row(page: &[u8; COLS]) -> ([Cell; COLS], bool) {
    let (mut fg, mut bg) = (7u8, 0u8);
    let (mut mosaic, mut separated, mut hold, mut flash, mut conceal, mut double) =
        (false, false, false, false, false, false);
    let mut held = b' ';
    let mut any_double = false;
    let mut cells = [Cell::default(); COLS];
    for (x, &b) in page.iter().enumerate() {
        let ch = if is_code(b) {
            // Before: these take effect in the code's own cell.
            match b {
                0x09 => flash = false,
                0x0C => {
                    double = false;
                    hold = false;
                    held = b' ';
                }
                0x18 => conceal = true,
                0x19 => separated = false,
                0x1A => separated = true,
                0x1C => bg = 0,
                0x1D => bg = fg,
                0x1E => hold = true,
                _ => {}
            }
            if hold && mosaic { held } else { b' ' }
        } else if mosaic && (b & 0x20 != 0) {
            // A 2x3 block: bits 0-4 and 6 of the byte are its six cells.
            let pattern = (b & 0x1F) | ((b & 0x40) >> 1);
            let g = if separated { 0xC0 } else { 0x80 } | pattern;
            held = g;
            g
        } else {
            glyph(b)
        };
        let mut f = 0;
        if flash {
            f |= flags::BLINK;
        }
        if conceal {
            f |= flags::CONCEAL;
        }
        if double {
            f |= flags::DOUBLE_TOP;
        }
        cells[x] = Cell { ch, fg: color(fg), bg: color(bg), flags: f, tile: 0 };
        if is_code(b) {
            // After: from the next cell on.
            match b {
                0x01..=0x07 => {
                    fg = b;
                    mosaic = false;
                    conceal = false;
                    hold = false;
                    held = b' ';
                }
                0x11..=0x17 => {
                    fg = b - 0x10;
                    mosaic = true;
                    conceal = false;
                }
                0x08 => flash = true,
                0x0D => {
                    double = true;
                    any_double = true;
                    hold = false;
                    held = b' ';
                }
                0x1F => {
                    hold = false;
                    held = b' ';
                }
                _ => {}
            }
        }
    }
    (cells, any_double)
}

/// A typed character as a Prestel keyboard sends it: its `#` key is
/// 0x5F, and `£` is 0x23. Enter is the `#` key, which ends input.
pub fn encode_char(c: char) -> Option<u8> {
    match c {
        '\r' | '#' => Some(0x5F),
        '£' => Some(0x23),
        c if c.is_ascii() && !c.is_ascii_control() => Some(c as u8),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(data: &[u8]) -> Screen {
        let mut s = Screen::new(COLS, ROWS, 0);
        let mut v = Viewdata::default();
        v.start(&mut s);
        v.feed(&mut s, data, &mut Vec::new());
        s
    }

    #[test]
    fn serial_attributes_take_a_cell_and_color_the_rest() {
        // Red text, then blue on a red background ("new background").
        let s = screen(b"\x1bAHi\x1bD\x1b]X");
        let row = s.row(0);
        assert_eq!(row[0].ch, b' ', "the code's own cell is a space");
        assert_eq!((row[1].ch, row[1].fg), (b'H', color(1)));
        assert_eq!(row[3].ch, b' ');
        // New background takes the current color (blue) in its own cell.
        assert_eq!(row[4].bg, color(4));
        assert_eq!((row[5].ch, row[5].fg, row[5].bg), (b'X', color(4), color(4)));
        // The next row starts over: white on black.
        assert_eq!(s.row(1)[0].fg, color(7));
    }

    #[test]
    fn mosaics_hold_and_separation() {
        // Mosaic green, a full block (0x7F) and the top-left cell (0x21);
        // then separated, and held over a color change.
        let s = screen(b"\x1bR\x7f!\x1bZ\x7f\x1b^\x1bS");
        let row = s.row(0);
        assert_eq!(row[1].ch, 0x80 | 0x3F);
        assert_eq!(row[2].ch, 0x80 | 0x01);
        assert_eq!(row[4].ch, 0xC0 | 0x3F, "separated");
        // Hold mosaics: the yellow code's cell repeats the last block.
        assert_eq!(row[6].ch, 0xC0 | 0x3F);
        // Capital letters stay letters in mosaic mode.
        let s = screen(b"\x1bQA");
        assert_eq!(s.row(0)[1].ch, b'A');
    }

    #[test]
    fn overwriting_a_code_recolors_the_row() {
        let mut s = Screen::new(COLS, ROWS, 0);
        let mut v = Viewdata::default();
        v.start(&mut s);
        v.feed(&mut s, b"\x1bAred text", &mut Vec::new());
        assert_eq!(s.row(0)[5].fg, color(1));
        // Back to the start of the row, and make that code green.
        v.feed(&mut s, b"\r\x1bB", &mut Vec::new());
        assert_eq!(s.row(0)[5].fg, color(2));
    }

    #[test]
    fn double_height_and_the_page_wrapping() {
        let s = screen(b"\x1bMBig\r\nsmall");
        assert!(s.row(0)[1].flags & flags::DOUBLE_TOP != 0);
        // The row under it shows the bottom halves, not its own text.
        assert_eq!(s.row(1)[1].ch, b'B');
        assert!(s.row(1)[1].flags & flags::DOUBLE_BOTTOM != 0);
        // Up from the top row is the bottom row; the UK set's £.
        let s = screen(b"\x0b#");
        assert_eq!(s.row(ROWS - 1)[0].ch, GLYPH_POUND);
        assert_eq!(s.cursor(), (1, ROWS - 1));
        // Writing the last cell goes on to the top.
        let mut data = vec![0x1E, 0x0B];
        data.extend([b'x'; COLS]);
        assert_eq!(screen(&data).cursor(), (0, 0));
    }
}
