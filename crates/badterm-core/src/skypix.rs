//! Amiga BBSes with SkyPix (SkyLine's, later C-Net's): ANSI text and
//! drawing commands (`ESC [ n ; ... !`) on a 640x200 screen in 8 or 16
//! colors, as the "ANSI/SkyPix Documentation" (v1.12, 1994) describes them.
//!
//! Like the Amiga's screen, everything (text included) is drawn into one
//! bitmap of pens, which the front end shows like RIPscrip's graphics.
//! Text is in Topaz 8, at a pixel position: ANSI moves it a cell (8x8) at a
//! time, `ESC [ 19 ; x ; y !` anywhere. Amiga disk fonts (`ESC [ 10`) are
//! drawn as Topaz scaled to about their size, without a background, as the
//! Amiga draws fonts other than the console's.
//!
//! ANSI colors pick the pens that look like them in SkyPix's palette.
//! Samples and XMODEM transfers of brushes aren't supported: a transfer is
//! cancelled.

use std::time::{Duration, Instant};

use crate::palette::Rgb;

static TOPAZ: &[u8; 2048] = include_bytes!("../assets/topaz_8x8.bin");

pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 200;
pub const COLS: usize = 80;
pub const ROWS: usize = 25;

/// SkyPix's standard palette, 0x0RGB.
const PALETTE: [u16; 16] =
    [0x000, 0x11F, 0xDDD, 0xF00, 0x0F1, 0x3AF, 0xFF2, 0xC0E, 0x0B6, 0x0DD, 0x0AF, 0x07C, 0x00F, 0x70F, 0xC0E, 0xC08];
/// The pens ANSI's colors (black, red, green, yellow, blue, magenta, cyan,
/// white) are drawn in.
const ANSI_PENS: [u8; 8] = [0, 3, 4, 6, 1, 7, 5, 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi,
    /// A string after a command's `!`, up to the next `!`.
    Text,
}

/// A brush: a piece of the screen, by pen.
struct Brush {
    w: usize,
    h: usize,
    pixels: Vec<u8>,
}

pub struct Skypix {
    pixels: Vec<u8>,
    palette: [u16; 16],
    /// 8 or 16 colors.
    colors: u8,
    /// The text cursor (top left of the next character), in pixels.
    x: i32,
    y: i32,
    saved: (i32, i32),
    /// Pen A (foreground, and what's drawn with) and B (background).
    pen_a: u8,
    pen_b: u8,
    reverse: bool,
    /// The drawing pen.
    pen: (i32, i32),
    /// A disk font's scale; None for Topaz (drawn with its background).
    font: Option<i32>,
    brush: Option<Brush>,
    gadgets: Vec<(i32, i32, i32, i32)>,
    pub cursor_on: bool,
    state: State,
    params: Vec<u32>,
    /// Which parameters had digits (`ESC [ 15 ; !` has none).
    digits: Vec<bool>,
    have: bool,
    text: Vec<u8>,
    /// Bytes not read yet, while a delay (`14`) lasts.
    held: Vec<u8>,
    pub hold: Option<Instant>,
    pub replies: Vec<u8>,
    pub bells: usize,
}

impl Default for Skypix {
    fn default() -> Skypix {
        Skypix {
            pixels: vec![0; WIDTH * HEIGHT],
            palette: PALETTE,
            // The documentation says 8, but BBSes use all 16 from the start.
            colors: 16,
            x: 0,
            y: 0,
            saved: (0, 0),
            pen_a: 2,
            pen_b: 0,
            reverse: false,
            pen: (0, 0),
            font: None,
            brush: None,
            gadgets: Vec::new(),
            cursor_on: true,
            state: State::Ground,
            params: Vec::new(),
            digits: Vec::new(),
            have: false,
            text: Vec::new(),
            held: Vec::new(),
            hold: None,
            replies: Vec::new(),
            bells: 0,
        }
    }
}

impl Skypix {
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn palette(&self) -> [Rgb; 16] {
        std::array::from_fn(|i| {
            let c = self.palette[if self.colors == 8 { i & 7 } else { i }] as u32;
            let ch = |shift: u32| ((c >> shift) & 15) * 0x11;
            (ch(8) << 16) | (ch(4) << 8) | ch(0)
        })
    }

    /// The text cursor, as a cell of the 80x25 screen.
    pub fn cursor(&self) -> (usize, usize) {
        ((self.x.max(0) / 8).min(COLS as i32 - 1) as usize, (self.y.max(0) / 8).min(ROWS as i32 - 1) as usize)
    }

    fn mask(&self) -> u8 {
        self.colors - 1
    }

    pub fn feed(&mut self, data: &[u8]) {
        if self.hold.is_some() {
            self.held.extend_from_slice(data);
            return;
        }
        for (i, &b) in data.iter().enumerate() {
            self.byte(b);
            if self.hold.is_some() {
                self.held.extend_from_slice(&data[i + 1..]);
                return;
            }
        }
    }

    /// Carry on after a delay, once its time has come.
    pub fn poll(&mut self, now: Instant) {
        if self.hold.is_some_and(|t| now >= t) {
            self.hold = None;
            let held = std::mem::take(&mut self.held);
            self.feed(&held);
        }
    }

    fn byte(&mut self, b: u8) {
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => {
                self.state = if b == b'[' { State::Csi } else { State::Ground };
                self.params.clear();
                self.digits.clear();
                self.have = false;
            }
            State::Csi => match b {
                b'0'..=b'9' => {
                    if self.params.is_empty() {
                        self.params.push(0);
                        self.digits.push(false);
                    }
                    if let Some(d) = self.digits.last_mut() {
                        *d = true;
                    }
                    let p = self.params.last_mut().unwrap();
                    *p = p.saturating_mul(10).saturating_add((b - b'0') as u32);
                    self.have = true;
                }
                b';' => {
                    if self.params.is_empty() {
                        self.params.push(0);
                        self.digits.push(false);
                    }
                    if self.params.len() < 32 {
                        self.params.push(0);
                        self.digits.push(false);
                    }
                }
                b'!' => {
                    // Commands with a string: the font's name, a file's.
                    if matches!(self.params.first(), Some(0 | 10 | 16)) && !self.closes_font() {
                        self.text.clear();
                        self.state = State::Text;
                    } else {
                        self.state = State::Ground;
                        self.command(&[]);
                    }
                }
                0x40..=0x7E => {
                    self.state = State::Ground;
                    self.csi(b);
                }
                0x20..=0x3F => {}
                _ => self.state = State::Ground,
            },
            State::Text => {
                if b == b'!' || self.text.len() > 256 {
                    self.state = State::Ground;
                    let text = std::mem::take(&mut self.text);
                    self.command(&text);
                } else {
                    self.text.push(b);
                }
            }
        }
    }

    /// `ESC [ 10 ; 0 !` goes back to Topaz, with no name after it.
    fn closes_font(&self) -> bool {
        self.params.first() == Some(&10) && self.params.get(1).copied().unwrap_or(0) == 0
    }

    /// Whether parameter `i` had digits.
    fn given(&self, i: usize) -> bool {
        self.digits.get(i).copied().unwrap_or(false)
    }

    fn p(&self, i: usize) -> i32 {
        self.params.get(i).copied().unwrap_or(0) as i32
    }

    fn ground(&mut self, b: u8) {
        let (cw, ch) = self.cell();
        match b {
            0x07 => self.bells += 1,
            0x08 => self.x = (self.x - cw).max(0),
            0x09 => self.x = ((self.x / 64 + 1) * 64).min(WIDTH as i32 - cw),
            0x0A => {
                // The Amiga's LF is a new line (to its start); a disk font
                // goes down a line on CR instead, and ignores LF.
                if self.font.is_none() {
                    self.x = 0;
                    self.line_feed();
                }
            }
            0x0C => self.clear(),
            0x0D => {
                self.x = 0;
                if self.font.is_some() {
                    self.line_feed();
                }
            }
            0x1B => self.state = State::Escape,
            0x00..=0x1F => {}
            _ => {
                // Topaz wraps; a disk font is cut off at the edge.
                if self.x + cw > WIDTH as i32 && self.font.is_none() {
                    self.x = 0;
                    self.line_feed();
                }
                self.glyph(b);
                self.x += cw;
            }
        }
        let _ = ch;
    }

    /// The character cell: Topaz's 8x8, or a disk font's scaled one.
    fn cell(&self) -> (i32, i32) {
        let s = self.font.unwrap_or(1);
        (8 * s, 8 * s)
    }

    fn line_feed(&mut self) {
        let ch = self.cell().1;
        self.y += ch;
        if self.y + ch > HEIGHT as i32 {
            let up = (self.y + ch - HEIGHT as i32) as usize;
            self.scroll(up);
            self.y -= up as i32;
        }
    }

    fn scroll(&mut self, rows: usize) {
        let rows = rows.min(HEIGHT);
        self.pixels.copy_within(rows * WIDTH.., 0);
        let bg = self.pen_b;
        self.pixels[(HEIGHT - rows) * WIDTH..].fill(bg);
    }

    fn clear(&mut self) {
        let bg = self.pen_b;
        self.pixels.fill(bg);
        (self.x, self.y) = (0, 0);
    }

    fn set(&mut self, x: i32, y: i32, pen: u8) {
        if x >= 0 && y >= 0 && (x as usize) < WIDTH && (y as usize) < HEIGHT {
            self.pixels[y as usize * WIDTH + x as usize] = pen & self.mask();
        }
    }

    fn get(&self, x: i32, y: i32) -> Option<u8> {
        (x >= 0 && y >= 0 && (x as usize) < WIDTH && (y as usize) < HEIGHT)
            .then(|| self.pixels[y as usize * WIDTH + x as usize])
    }

    /// A character at the cursor: with its background in Topaz, without
    /// in a disk font.
    fn glyph(&mut self, c: u8) {
        let s = self.font.unwrap_or(1);
        let (fg, bg) = if self.reverse { (self.pen_b, self.pen_a) } else { (self.pen_a, self.pen_b) };
        for gy in 0..8 * s {
            let bits = TOPAZ[c as usize * 8 + (gy / s) as usize];
            for gx in 0..8 * s {
                let on = bits & (0x80 >> (gx / s)) != 0;
                if on {
                    self.set(self.x + gx, self.y + gy, fg);
                } else if self.font.is_none() {
                    self.set(self.x + gx, self.y + gy, bg);
                }
            }
        }
    }

    fn csi(&mut self, f: u8) {
        let (cw, ch) = self.cell();
        let n = self.p(0).max(1);
        match f {
            b'A' => self.y = (self.y - n * ch).max(0),
            b'B' => self.y = (self.y + n * ch).min(HEIGHT as i32 - ch),
            b'C' => self.x = (self.x + n * cw).min(WIDTH as i32 - cw),
            b'D' => self.x = (self.x - n * cw).max(0),
            b'H' | b'f' => {
                let (row, col) = (self.p(0).max(1), self.p(1).max(1));
                self.x = ((col - 1) * 8).min(WIDTH as i32 - 8);
                self.y = ((row - 1) * 8).min(HEIGHT as i32 - 8);
            }
            b'J' => match self.p(0) {
                0 => {
                    self.fill_rect(self.x, self.y, WIDTH as i32 - 1, self.y + ch - 1, self.pen_b);
                    self.fill_rect(0, self.y + ch, WIDTH as i32 - 1, HEIGHT as i32 - 1, self.pen_b);
                }
                1 => {
                    self.fill_rect(0, 0, WIDTH as i32 - 1, self.y - 1, self.pen_b);
                    self.fill_rect(0, self.y, self.x + cw - 1, self.y + ch - 1, self.pen_b);
                }
                // The whole screen, and home, as ANSI-BBS does.
                _ => self.clear(),
            },
            b'K' => {
                let (a, b) = match self.p(0) {
                    0 => (self.x, WIDTH as i32 - 1),
                    1 => (0, self.x + cw - 1),
                    _ => (0, WIDTH as i32 - 1),
                };
                self.fill_rect(a, self.y, b, self.y + ch - 1, self.pen_b);
            }
            b'm' => self.sgr(),
            b's' => self.saved = (self.x, self.y),
            b'u' => (self.x, self.y) = self.saved,
            b'n' if self.p(0) == 6 => {
                let reply = format!("\x1b[{};{}R", self.y / 8 + 1, self.x / 8 + 1);
                self.replies.extend_from_slice(reply.as_bytes());
            }
            b'h' | b'l' if self.params.first() == Some(&25) => self.cursor_on = f == b'h',
            _ => {}
        }
    }

    fn sgr(&mut self) {
        if self.params.is_empty() {
            self.params.push(0);
        }
        for i in 0..self.params.len() {
            match self.params[i] {
                0 => {
                    self.pen_a = 2;
                    self.pen_b = 0;
                    self.reverse = false;
                }
                7 => self.reverse = true,
                27 => self.reverse = false,
                p @ 30..=37 => self.pen_a = ANSI_PENS[(p - 30) as usize],
                39 => self.pen_a = 2,
                p @ 40..=47 => self.pen_b = ANSI_PENS[(p - 40) as usize],
                49 => self.pen_b = 0,
                _ => {}
            }
        }
    }

    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, pen: u8) {
        let (x0, x1) = (x0.min(x1).max(0), x0.max(x1).min(WIDTH as i32 - 1));
        let (y0, y1) = (y0.min(y1).max(0), y0.max(y1).min(HEIGHT as i32 - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, pen);
            }
        }
    }

    fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.set(x, y, self.pen_a);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// An ellipse around (cx, cy) with radii a and b: its outline, or filled.
    fn ellipse(&mut self, cx: i32, cy: i32, a: i32, b: i32, filled: bool) {
        let (a, b) = (a.max(0), b.max(0));
        if a == 0 || b == 0 {
            self.line((cx - a, cy - b), (cx + a, cy + b));
            return;
        }
        let pen = self.pen_a;
        let half = |dy: i32| (a as f64 * (1.0 - (dy as f64 / b as f64).powi(2)).max(0.0).sqrt()).round() as i32;
        let mut prev = half(-b);
        for dy in -b..=b {
            let w = half(dy);
            if filled {
                for x in cx - w..=cx + w {
                    self.set(x, cy + dy, pen);
                }
            } else {
                // Each side from the row above's edge to this one's, so
                // the outline has no gaps where it's steep or flat.
                let (near, far) = (w.min(prev), w.max(prev));
                for x in near..=far {
                    self.set(cx - x, cy + dy, pen);
                    self.set(cx + x, cy + dy, pen);
                }
            }
            prev = w;
        }
        if !filled {
            // The flat top and bottom.
            for x in -half(-b + 1).max(0)..=half(-b + 1) {
                self.set(cx + x, cy - b, pen);
                self.set(cx + x, cy + b, pen);
            }
        }
    }

    /// Flood fill in pen A: mode 0 up to pixels in pen A (the outline),
    /// mode 1 over the color under (x, y).
    fn flood(&mut self, mode: i32, x: i32, y: i32) {
        let Some(under) = self.get(x, y) else { return };
        let pen = self.pen_a & self.mask();
        let fills = |p: u8| if mode == 0 { p != pen } else { p == under };
        if mode != 0 && under == pen || !fills(under) {
            return;
        }
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            if !self.get(x, y).is_some_and(fills) {
                continue;
            }
            let (mut a, mut b) = (x, x);
            while self.get(a - 1, y).is_some_and(fills) {
                a -= 1;
            }
            while self.get(b + 1, y).is_some_and(fills) {
                b += 1;
            }
            for i in a..=b {
                self.set(i, y, pen);
            }
            for i in a..=b {
                for ny in [y - 1, y + 1] {
                    if self.get(i, ny).is_some_and(fills) {
                        stack.push((i, ny));
                    }
                }
            }
        }
    }

    /// A SkyPix command (`ESC [ n ; ... !`, and its string if it has one).
    fn command(&mut self, text: &[u8]) {
        let p = |i: usize| self.p(i);
        match p(0) {
            1 => self.set(p(1), p(2), self.pen_a),
            2 => {
                let to = (p(1), p(2));
                self.line(self.pen, to);
                self.pen = to;
            }
            3 => self.flood(p(1), p(2), p(3)),
            4 => self.fill_rect(p(1), p(2), p(3), p(4), self.pen_a),
            5 => self.ellipse(p(1), p(2), p(3), p(4), false),
            6 => {
                let (x, y, w, h) = (p(1), p(2), p(3).clamp(0, WIDTH as i32), p(4).clamp(0, HEIGHT as i32));
                let pixels = (0..h)
                    .flat_map(|j| (0..w).map(move |i| (x + i, y + j)))
                    .map(|(x, y)| self.get(x, y).unwrap_or(0))
                    .collect();
                self.brush = Some(Brush { w: w as usize, h: h as usize, pixels });
            }
            7 => self.blit(p(1), p(2), p(3), p(4), p(5), p(6), p(7) as u8, p(8) as u8),
            8 => self.pen = (p(1), p(2)),
            10 => {
                let name = String::from_utf8_lossy(text).to_ascii_lowercase();
                let size = p(1);
                // Topaz 8 (and back to it) is the console's own font.
                let font = if size == 0 || name.starts_with("topaz") && size <= 9 {
                    None
                } else {
                    Some(((size + 4) / 8).max(1))
                };
                // Back to Topaz, the text stays on the same baseline.
                if let (Some(s), None) = (self.font, font) {
                    self.y = (self.y + 8 * s - 8).min(HEIGHT as i32 - 8);
                }
                self.font = font;
            }
            11 => {
                for i in 0..16 {
                    self.palette[i] = (self.p(i + 1) as u16) & 0xFFF;
                }
            }
            12 => self.palette = PALETTE,
            13 => self.ellipse(p(1), p(2), p(3), p(4), true),
            14 => self.hold = Some(Instant::now() + Duration::from_millis(p(1).clamp(0, 6000) as u64 * 1000 / 60)),
            // With no number, the default pen.
            15 if !self.given(1) => self.pen_a = 2,
            15 => self.pen_a = p(1) as u8 & 15,
            // An XMODEM transfer of a brush or sample: refused.
            16 => self.replies.extend_from_slice(&[0x18, 0x18, 0x18]),
            17 => {
                self.colors = if p(1) == 2 { 16 } else { 8 };
                self.palette = PALETTE;
            }
            18 if !self.given(1) => self.pen_b = 0,
            18 => self.pen_b = p(1) as u8 & 15,
            19 => (self.x, self.y) = (p(1).clamp(0, WIDTH as i32 - 1), p(2).clamp(0, HEIGHT as i32 - 1)),
            22 => {
                let (x0, y0, x1, y1) = (p(3), p(4), p(5), p(6));
                for (a, b) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))] {
                    self.line(a, b);
                }
                self.gadgets.push((x0, y0, x1, y1));
            }
            // The end of SkyPix: back to plain ANSI's look.
            99 => {
                self.palette = PALETTE;
                self.font = None;
                self.pen_a = 2;
                self.pen_b = 0;
            }
            _ => {}
        }
    }

    /// Copy the brush (a piece of it) to the screen, through the Amiga
    /// blitter's minterm (192 copies) and plane mask.
    #[allow(clippy::too_many_arguments)]
    fn blit(&mut self, sx: i32, sy: i32, dx: i32, dy: i32, w: i32, h: i32, minterm: u8, mask: u8) {
        let Some(brush) = self.brush.take() else { return };
        for j in 0..h.max(0) {
            for i in 0..w.max(0) {
                let (bx, by) = (sx + i, sy + j);
                if bx < 0 || by < 0 || bx as usize >= brush.w || by as usize >= brush.h {
                    continue;
                }
                let b = brush.pixels[by as usize * brush.w + bx as usize];
                let Some(c) = self.get(dx + i, dy + j) else { continue };
                let mut out = 0u8;
                for plane in 0..4 {
                    let (bb, cb) = ((b >> plane) & 1, (c >> plane) & 1);
                    let bit = if mask & (1 << plane) != 0 { (minterm >> (4 + (bb << 1 | cb))) & 1 } else { cb };
                    out |= bit << plane;
                }
                self.set(dx + i, dy + j, out);
            }
        }
        self.brush = Some(brush);
    }

    /// A left click at a point of the screen: SkyPix's controller return.
    pub fn click(&self, x: i32, y: i32) -> Vec<u8> {
        format!("\x1b[21;1;{x};{y}!").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sky(data: &[u8]) -> Skypix {
        let mut s = Skypix::default();
        s.feed(data);
        s
    }

    fn at(s: &Skypix, x: usize, y: usize) -> u8 {
        s.pixels[y * WIDTH + x]
    }

    #[test]
    fn drawing_commands() {
        // Red (pen 3): a pixel, a line from the pen, a box; then a filled
        // ellipse in green.
        let s =
            sky(b"\x1b[15;3!\x1b[1;5;5!\x1b[8;10;10!\x1b[2;20;10!\x1b[4;30;30;40;40!\x1b[15;4!\x1b[13;100;100;10;5!");
        assert_eq!(at(&s, 5, 5), 3);
        assert_eq!((at(&s, 10, 10), at(&s, 20, 10), at(&s, 21, 10)), (3, 3, 0));
        assert_eq!(at(&s, 35, 35), 3);
        assert_eq!((at(&s, 100, 100), at(&s, 109, 100), at(&s, 112, 100)), (4, 4, 0));
    }

    #[test]
    fn flood_fill_modes() {
        // An outline in pen 3, then fill inside it up to the outline.
        let mut s = sky(b"\x1b[15;3!\x1b[5;50;50;20;10!\x1b[3;0;50;50!");
        assert_eq!((at(&s, 50, 50), at(&s, 75, 50)), (3, 0));
        // Mode 1: repaint the color under the point.
        s.feed(b"\x1b[15;4!\x1b[3;1;50;50!");
        assert_eq!(at(&s, 50, 50), 4);
    }

    #[test]
    fn ansi_text_in_pens() {
        // ANSI red is pen 3; the text's cell is drawn in pen B.
        let s = sky(b"\x1b[31;45mA\x1b[2;1HB");
        let pens: Vec<u8> = (0..8).flat_map(|y| (0..8).map(move |x| (x, y))).map(|(x, y)| at(&s, x, y)).collect();
        assert!(pens.contains(&3) && pens.contains(&7), "{pens:?}");
        assert!((8..16).any(|y| (0..8).any(|x| at(&s, x, y) == 3)), "B on the next row");
        assert_eq!(s.cursor(), (1, 1));
    }

    #[test]
    fn fonts_position_and_brushes() {
        // A disk font at a pixel position: bigger, no background.
        let s = sky(b"\x1b[18;1!\x1b[10;16!garnet.font!\x1b[19;100;50!I");
        assert!((50..66).any(|y| (100..116).any(|x| at(&s, x, y) == 2)));
        assert_eq!(at(&s, 100, 50), 0, "no background drawn");
        assert_eq!(s.cursor(), (14, 6));
        // Grab a brush and copy it elsewhere.
        let s = sky(b"\x1b[15;5!\x1b[4;0;0;3;3!\x1b[6;0;0;4;4!\x1b[7;0;0;200;100;4;4;192;255!");
        assert_eq!((at(&s, 202, 102), at(&s, 205, 102)), (5, 0));
        // The palette: 0xRGB, red highest.
        let s = sky(b"\x1b[11;3840;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0!");
        assert_eq!(s.palette()[0], 0xFF0000);
    }
}
