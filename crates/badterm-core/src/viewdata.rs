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

/// Which teletext terminal: a Prestel one, or a BBC Micro in Mode 7.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Prestel,
    /// The BBC Micro's Mode 7 (CTerm's "BEEB"): 40x25; attributes only as
    /// bytes 0x80-0x9F (ESC starts the BBC's own VDU codes); the screen
    /// scrolls rather than wrapping at its top and bottom; `#`, `_` and
    /// `` ` `` are swapped as the BBC's character set has them; and a
    /// double-height row's lower half is the row below's own text.
    Beeb,
}

/// BBC Mode 7 has a 25th row.
pub const BEEB_ROWS: usize = 25;

#[derive(Debug)]
pub struct Viewdata {
    pub kind: Kind,
    /// What was received, by position: printable bytes, or attribute
    /// codes (0x00-0x1F).
    page: Vec<[u8; COLS]>,
    x: usize,
    y: usize,
    /// ESC was the last byte: the next is an attribute (Prestel).
    escape: bool,
    /// Concealed text shown (Reveal).
    pub reveal: bool,
    /// The BBC's multi-byte codes: what's still to come (VDU 23 takes 9
    /// bytes, VDU 31, cursor position, 2), and what's come.
    vdu: (u8, usize, Vec<u8>),
}

impl Default for Viewdata {
    fn default() -> Viewdata {
        Viewdata::new(Kind::Prestel)
    }
}

impl Viewdata {
    pub fn new(kind: Kind) -> Viewdata {
        let rows = if kind == Kind::Beeb { BEEB_ROWS } else { ROWS };
        Viewdata {
            kind,
            page: vec![[b' '; COLS]; rows],
            x: 0,
            y: 0,
            escape: false,
            reveal: false,
            vdu: (0, 0, Vec::new()),
        }
    }

    fn rows(&self) -> usize {
        self.page.len()
    }

    pub fn start(&mut self, s: &mut Screen) {
        *self = Viewdata::new(self.kind);
        s.look.fonts[0] = FONT;
        s.base_font = FONT;
        s.autowrap = false;
        s.ice_colors = false;
        self.draw(s);
    }

    /// Show concealed text, or hide it again.
    pub fn toggle_reveal(&mut self, s: &mut Screen) -> bool {
        self.reveal = !self.reveal;
        self.draw(s);
        self.reveal
    }

    /// Bytes from the BBS: the page changes, then the screen is drawn from it.
    pub fn feed(&mut self, s: &mut Screen, data: &[u8], events: &mut Vec<TermEvent>) {
        for &b in data {
            match self.kind {
                Kind::Prestel => self.byte(s, b, events),
                Kind::Beeb => self.beeb_byte(s, b, events),
            }
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
        let rows = self.rows();
        match b {
            0x07 => events.push(TermEvent::Bell),
            0x08 => self.back(),
            0x09 => self.forward(),
            0x0A => self.y = (self.y + 1) % rows,
            0x0B => self.y = (self.y + rows - 1) % rows,
            0x0C => self.clear(),
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

    /// A byte to a BBC Micro in Mode 7.
    fn beeb_byte(&mut self, s: &mut Screen, b: u8, events: &mut Vec<TermEvent>) {
        // The rest of a VDU 23 (cursor on or off) or VDU 31 (position).
        if self.vdu.1 > 0 {
            self.vdu.2.push(b);
            self.vdu.1 -= 1;
            if self.vdu.1 == 0 {
                let args = std::mem::take(&mut self.vdu.2);
                match self.vdu.0 {
                    23 if args.first() == Some(&1) => s.cursor_visible = args.get(1).copied().unwrap_or(1) != 0,
                    28 => {
                        let (x, y) = (args[0].saturating_sub(0x20) as usize, args[1].saturating_sub(0x20) as usize);
                        if x < COLS && y < self.rows() {
                            (self.x, self.y) = (x, y);
                        }
                    }
                    _ => {}
                }
            }
            return;
        }
        let b = if b >= 0xA0 { b & 0x7F } else { b };
        match b {
            0x07 => events.push(TermEvent::Bell),
            0x08 => self.beeb_back(),
            0x09 => self.beeb_forward(),
            0x0A => self.beeb_down(),
            0x0B => {
                if self.y == 0 {
                    self.page.rotate_right(1);
                    self.page[0] = [b' '; COLS];
                } else {
                    self.y -= 1;
                }
            }
            0x0C => self.clear(),
            0x0D => self.x = 0,
            0x17 => self.vdu = (23, 9, Vec::new()),
            0x1C => self.vdu = (28, 2, Vec::new()),
            0x1E => (self.x, self.y) = (0, 0),
            // A destructive backspace.
            0x7F => {
                self.beeb_back();
                self.page[self.y][self.x] = b' ';
            }
            // The BBC's #, _ and ` are where the teletext chip has #, the
            // long dash and £.
            0x23 => self.beeb_put(0x5F),
            0x5F => self.beeb_put(0x60),
            0x60 => self.beeb_put(0x23),
            0x20..=0x7E => self.beeb_put(b),
            0x80..=0x9F => self.beeb_put(b - 0x80),
            _ => {}
        }
    }

    fn beeb_put(&mut self, b: u8) {
        self.page[self.y][self.x] = b;
        self.beeb_forward();
    }

    /// Down a row, scrolling the page up at the bottom.
    fn beeb_down(&mut self) {
        if self.y + 1 == self.rows() {
            self.page.rotate_left(1);
            let last = self.rows() - 1;
            self.page[last] = [b' '; COLS];
        } else {
            self.y += 1;
        }
    }

    fn beeb_forward(&mut self) {
        self.x += 1;
        if self.x == COLS {
            self.x = 0;
            self.beeb_down();
        }
    }

    fn beeb_back(&mut self) {
        if self.x > 0 {
            self.x -= 1;
            return;
        }
        self.x = COLS - 1;
        if self.y == 0 {
            self.page.rotate_right(1);
            self.page[0] = [b' '; COLS];
        } else {
            self.y -= 1;
        }
    }

    fn clear(&mut self) {
        self.page.fill([b' '; COLS]);
        (self.x, self.y) = (0, 0);
        self.reveal = false;
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
            self.y = (self.y + 1) % self.rows();
        }
    }

    fn back(&mut self) {
        if self.x == 0 {
            self.x = COLS - 1;
            self.y = (self.y + self.rows() - 1) % self.rows();
        } else {
            self.x -= 1;
        }
    }

    /// Draw the page on the screen, row by row.
    pub fn draw(&self, s: &mut Screen) {
        let rows = self.rows();
        let mut above: Option<[Cell; COLS]> = None;
        for y in 0..rows.min(s.rows()) {
            let (row, top) = draw_row(&self.page[y], self.reveal);
            let cells = match above.take() {
                // The row under a double-height row shows bottom halves:
                // of the row above's cells on Prestel, of its own on a BBC.
                Some(top_cells) if self.kind == Kind::Prestel => top_cells.map(|c| {
                    if c.flags & flags::DOUBLE_TOP != 0 {
                        Cell { flags: (c.flags & !flags::DOUBLE_TOP) | flags::DOUBLE_BOTTOM, ..c }
                    } else {
                        Cell { ch: b' ', flags: c.flags & !flags::BLINK, ..c }
                    }
                }),
                Some(_) => row.map(|c| {
                    if c.flags & flags::DOUBLE_TOP != 0 {
                        Cell { flags: (c.flags & !flags::DOUBLE_TOP) | flags::DOUBLE_BOTTOM, ..c }
                    } else {
                        c
                    }
                }),
                None => {
                    if top && y + 1 < rows {
                        above = Some(row);
                    }
                    if top && y + 1 == rows {
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
fn draw_row(page: &[u8; COLS], reveal: bool) -> ([Cell; COLS], bool) {
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
        if conceal && !reveal {
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

/// A typed character as a BBC Micro sends it: its `£` is 0x60.
pub fn beeb_encode_char(c: char) -> Option<u8> {
    match c {
        '£' => Some(0x60),
        '\r' => Some(0x0D),
        c if c.is_ascii() && !c.is_ascii_control() => Some(c as u8),
        _ => None,
    }
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
    fn reveal_shows_concealed_text() {
        let mut s = Screen::new(COLS, ROWS, 0);
        let mut v = Viewdata::default();
        v.start(&mut s);
        v.feed(&mut s, b"\x1bXsecret", &mut Vec::new());
        assert!(s.row(0)[1].flags & flags::CONCEAL != 0);
        assert!(v.toggle_reveal(&mut s));
        assert!(s.row(0)[1].flags & flags::CONCEAL == 0);
        // A new page hides it again.
        v.feed(&mut s, b"\x0c\x1bXagain", &mut Vec::new());
        assert!(s.row(0)[1].flags & flags::CONCEAL != 0);
    }

    #[test]
    fn bbc_mode_7() {
        let mut s = Screen::new(COLS, BEEB_ROWS, 0);
        let mut v = Viewdata::new(Kind::Beeb);
        v.start(&mut s);
        // Attributes as C1 bytes (red text); ESC isn't one; # is #.
        v.feed(&mut s, b"\x81Hi #", &mut Vec::new());
        assert_eq!((s.row(0)[1].ch, s.row(0)[1].fg), (b'H', color(1)));
        assert_eq!(s.row(0)[4].ch, b'#');
        // VDU 31 (here 28, with 0x20 added) moves; the bottom row scrolls.
        v.feed(&mut s, b"\x1c\x20\x38last\n", &mut Vec::new());
        assert_eq!(Screen::row_text(s.row(23)), "last");
        assert_eq!(Screen::row_text(s.row(24)), "");
        assert_eq!(s.cursor(), (4, 24));
        // Double height: the row below shows the lower halves of its own text.
        v.feed(&mut s, b"\x0c\x8dBIG\r\n\x8dBIG", &mut Vec::new());
        assert!(s.row(1)[1].flags & flags::DOUBLE_BOTTOM != 0);
        assert_eq!(s.row(1)[1].ch, b'B');
        // VDU 23,1,0;0;0;0; hides the cursor.
        v.feed(&mut s, &[23, 1, 0, 0, 0, 0, 0, 0, 0, 0], &mut Vec::new());
        assert!(!s.cursor_visible);
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
