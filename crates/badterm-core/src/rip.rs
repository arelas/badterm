//! RIPscrip 1.54 graphics: the vector scenes, buttons and clickable mouse
//! regions of RIPterm-era BBSes, drawn on a 640x350 EGA canvas under the
//! 80x43 text screen.
//!
//! A RIP line starts with `!|` at the beginning of a line and holds commands
//! separated by `|`, each a command letter (with a level digit before it
//! for level 1 commands) and its parameters, mostly two-digit base-36
//! "MegaNums". A backslash at the end of a line continues it on the next;
//! `\|`, `\\` and `\!` are literal characters in text.
//!
//! Drawing follows the Borland BGI the format was built on: colors are
//! indices into a 16-entry palette of the EGA's 64 colors, lines have a
//! style and thickness, areas are filled with one of BGI's patterns, and
//! circles are corrected for the EGA's tall pixels. Text uses the 8x8 font,
//! scaled up for larger sizes; BGI's vector fonts (Borland's own files) are
//! drawn in it too.
//!
//! Left out: icon files (`1I`, `1W`), scene files and file queries (`1R`,
//! `1F`), variables (`1D`) and RIP's block-mode file transfers.

use crate::palette::Rgb;
use crate::screen::{Cell, Screen};

pub const WIDTH: usize = 640;
pub const HEIGHT: usize = 350;
/// The text screen over the canvas: 80x43 in the 8x8 font.
pub const COLS: usize = 80;
pub const ROWS: usize = 43;

/// What a RIP terminal answers to `ESC [ !`: RIPscrip, version 1.54.
pub const VERSION_REPLY: &[u8] = b"RIPSCRIP015400";

/// The longest RIP line kept, continuations included.
const MAX_LINE: usize = 64 * 1024;

static FONT_8X8: &[u8; 2048] = include_bytes!("../assets/vga_8x8.bin");

/// The EGA's default palette: indices into its 64 colors.
const DEFAULT_PALETTE: [u8; 16] = [0, 1, 2, 3, 4, 5, 20, 7, 56, 57, 58, 59, 60, 61, 62, 63];

/// BGI's fill patterns, 8x8, most significant bit leftmost.
const FILL_PATTERNS: [[u8; 8]; 12] = [
    [0x00; 8],
    [0xFF; 8],
    [0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00],
    [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80],
    [0xE0, 0xC1, 0x83, 0x07, 0x0E, 0x1C, 0x38, 0x70],
    [0xF0, 0x78, 0x3C, 0x1E, 0x0F, 0x87, 0xC3, 0xE1],
    [0xA5, 0xD2, 0x69, 0xB4, 0x5A, 0x2D, 0x96, 0x4B],
    [0xFF, 0x88, 0x88, 0x88, 0xFF, 0x88, 0x88, 0x88],
    [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81],
    [0xCC, 0x33, 0xCC, 0x33, 0xCC, 0x33, 0xCC, 0x33],
    [0x80, 0x00, 0x08, 0x00, 0x80, 0x00, 0x08, 0x00],
    [0x88, 0x00, 0x22, 0x00, 0x88, 0x00, 0x22, 0x00],
];

/// BGI's line styles: solid, dotted, center and dashed.
const LINE_STYLES: [u16; 4] = [0xFFFF, 0xCCCC, 0xFC78, 0xF8F8];

/// The EGA draws circles this much flatter, so they look round on a 4:3
/// screen of 640x350 (BGI's aspect ratio for it).
const ASPECT: f64 = 0.775;

/// One of the EGA's 64 colors (bits: secondary R G B, primary R G B).
pub fn ega_rgb(c: u8) -> Rgb {
    let level = |hi: u8, lo: u8| (((c >> hi) & 1) * 0xAA + ((c >> lo) & 1) * 0x55) as u32;
    (level(2, 5) << 16) | (level(1, 4) << 8) | level(0, 3)
}

/// A clickable area: when clicked, `text` is sent to the BBS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MouseRegion {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
    /// Show the area inverted while it is clicked.
    pub invert: bool,
    pub text: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ButtonStyle {
    width: usize,
    height: usize,
    bevel: usize,
    dfore: u8,
    dback: u8,
    bright: u8,
    dark: u8,
    surface: u8,
}

#[derive(Clone, Copy, Debug)]
struct Viewport {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

const FULL: Viewport = Viewport { x0: 0, y0: 0, x1: WIDTH - 1, y1: HEIGHT - 1 };

/// A text block being laid out (`1T` ... `1E`).
struct TextBlock {
    x0: i32,
    x1: i32,
    y: i32,
}

pub struct Rip {
    pixels: Vec<u8>,
    /// Palette entry -> EGA color.
    palette: [u8; 16],
    color: u8,
    fill_color: u8,
    fill: [u8; 8],
    line_style: u16,
    thick: bool,
    xor: bool,
    view: Viewport,
    /// The drawing position (for text), relative to the viewport.
    pos: (i32, i32),
    font_size: usize,
    vertical: bool,
    button: ButtonStyle,
    regions: Vec<MouseRegion>,
    clipboard: Option<(usize, usize, Vec<u8>)>,
    text_block: Option<TextBlock>,
    /// The text window, in text cells (x0, y0, x1, y1).
    text_window: (usize, usize, usize, usize),
    /// The RIP line being read, from its `!`.
    line: Option<Vec<u8>>,
    /// A RIP line just ended with CR: its LF goes too.
    eat_lf: bool,
    /// Changed since the front end last looked.
    pub dirty: bool,
}

/// What [`Rip::byte`] did with a byte.
pub enum Feed {
    /// Not RIP: handle it as text.
    Pass,
    /// Taken.
    Done,
    /// It turned out not to be a RIP line: show these bytes as text.
    Text(Vec<u8>),
}

impl Default for Rip {
    fn default() -> Self {
        Rip {
            pixels: vec![0; WIDTH * HEIGHT],
            palette: DEFAULT_PALETTE,
            color: 15,
            fill_color: 15,
            fill: FILL_PATTERNS[1],
            line_style: 0xFFFF,
            thick: false,
            xor: false,
            view: FULL,
            pos: (0, 0),
            font_size: 1,
            vertical: false,
            button: ButtonStyle::default(),
            regions: Vec::new(),
            clipboard: None,
            text_block: None,
            text_window: (0, 0, COLS - 1, ROWS - 1),
            line: None,
            eat_lf: false,
            dirty: true,
        }
    }
}

impl Rip {
    /// The canvas: `WIDTH` x `HEIGHT` palette indices.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The colors of the 16 palette entries.
    pub fn palette(&self) -> [Rgb; 16] {
        self.palette.map(ega_rgb)
    }

    pub fn regions(&self) -> &[MouseRegion] {
        &self.regions
    }

    /// What clicking the canvas at (x, y) sends, if it is in a mouse region
    /// (the last defined wins, as they are drawn over each other).
    pub fn click(&self, x: usize, y: usize) -> Option<Vec<u8>> {
        self.regions
            .iter()
            .rev()
            .find(|r| (r.x0..=r.x1).contains(&x) && (r.y0..=r.y1).contains(&y))
            .map(|r| r.text.clone())
    }

    /// Take a byte at the text level: `at_line_start` says whether it
    /// begins a line, where a RIP line may start.
    pub fn byte(&mut self, s: &mut Screen, b: u8, at_line_start: bool) -> Feed {
        if std::mem::take(&mut self.eat_lf) && b == b'\n' {
            return Feed::Done;
        }
        let Some(line) = self.line.as_mut() else {
            if at_line_start && b == b'!' {
                self.line = Some(vec![b]);
                return Feed::Done;
            }
            return Feed::Pass;
        };
        // `!` must be followed by `|`; otherwise it was text.
        if line.len() == 1 && b != b'|' {
            let mut text = self.line.take().unwrap_or_default();
            text.push(b);
            return Feed::Text(text);
        }
        match b {
            b'\r' | b'\n' => {
                // A backslash before the end of the line continues it.
                if line.last() == Some(&b'\\') && !line.ends_with(b"\\\\") {
                    line.pop();
                    line.push(0);
                    return Feed::Done;
                }
                if line.last() == Some(&0) {
                    // The LF of a continued line's CR LF.
                    return Feed::Done;
                }
                let line = self.line.take().unwrap_or_default();
                self.run_line(s, &line);
                // The line's end is not shown: RIP lines are invisible.
                self.eat_lf = b == b'\r';
                Feed::Done
            }
            _ => {
                if line.last() == Some(&0) {
                    line.pop();
                }
                if line.len() < MAX_LINE {
                    line.push(b);
                }
                Feed::Done
            }
        }
    }

    fn run_line(&mut self, s: &mut Screen, line: &[u8]) {
        // Split on unescaped bars; the first piece is the `!`.
        let mut cmds: Vec<Vec<u8>> = vec![Vec::new()];
        let mut i = 1;
        while i < line.len() {
            match line[i] {
                b'\\' if i + 1 < line.len() => {
                    cmds.last_mut().expect("never empty").push(line[i + 1]);
                    i += 1;
                }
                b'|' => cmds.push(Vec::new()),
                c => cmds.last_mut().expect("never empty").push(c),
            }
            i += 1;
        }
        for cmd in cmds.iter().filter(|c| !c.is_empty()) {
            self.command(s, cmd);
        }
        self.dirty = true;
    }

    fn command(&mut self, s: &mut Screen, cmd: &[u8]) {
        let (level, op, args) = match cmd {
            [d @ b'1'..=b'9', op, args @ ..] => (d - b'0', *op, args),
            [op, args @ ..] => (0, *op, args),
            [] => return,
        };
        let mut a = Args(args);
        match (level, op) {
            (0, b'w') => {
                let (x0, y0, x1, y1) = (a.n2(), a.n2(), a.n2(), a.n2());
                let (x1, y1) = (x1.clamp(x0, COLS - 1), y1.clamp(y0, ROWS - 1));
                self.text_window = (x0.min(COLS - 1), y0.min(ROWS - 1), x1, y1);
                s.set_scroll_region(self.text_window.1, self.text_window.3);
                s.move_to(self.text_window.0, self.text_window.1);
            }
            (0, b'v') => {
                let (x0, y0, x1, y1) = (a.n2(), a.n2(), a.n2(), a.n2());
                self.view = Viewport {
                    x0: x0.min(WIDTH - 1),
                    y0: y0.min(HEIGHT - 1),
                    x1: x1.clamp(x0.min(WIDTH - 1), WIDTH - 1),
                    y1: y1.clamp(y0.min(HEIGHT - 1), HEIGHT - 1),
                };
            }
            (0, b'*') => self.reset(s),
            (0, b'e') => {
                let (x0, y0, x1, y1) = self.text_window;
                s.fill_rect(x0, y0, x1 - x0 + 1, y1 - y0 + 1, Cell { ch: b' ', ..s.pen });
                s.move_to(x0, y0);
            }
            (0, b'E') => {
                let v = self.view;
                for y in v.y0..=v.y1 {
                    self.pixels[y * WIDTH + v.x0..=y * WIDTH + v.x1].fill(0);
                }
            }
            (0, b'g') => {
                let (x, y) = (a.n2(), a.n2());
                s.move_to(self.text_window.0 + x, self.text_window.1 + y);
            }
            (0, b'H') => s.move_to(self.text_window.0, self.text_window.1),
            (0, b'>') => s.erase_line(0),
            (0, b'c') => self.color = a.n2() as u8 & 15,
            (0, b'Q') => {
                for i in 0..16 {
                    self.palette[i] = a.n2() as u8 & 63;
                }
            }
            (0, b'a') => {
                let (i, c) = (a.n2(), a.n2());
                self.palette[i & 15] = c as u8 & 63;
            }
            (0, b'W') => self.xor = a.n2() == 1,
            (0, b'm') => self.pos = (a.i2(), a.i2()),
            (0, b'T') => {
                let text = a.rest().to_vec();
                self.text(&text);
            }
            (0, b'@') => {
                self.pos = (a.i2(), a.i2());
                let text = a.rest().to_vec();
                self.text(&text);
            }
            (0, b'Y') => {
                let (_font, dir, size) = (a.n2(), a.n2(), a.n2());
                self.vertical = dir == 1;
                self.font_size = size.clamp(1, 10);
            }
            (0, b'X') => {
                let (x, y) = (a.i2(), a.i2());
                self.plot(x, y, self.color);
            }
            (0, b'L') => {
                let (x0, y0, x1, y1) = (a.i2(), a.i2(), a.i2(), a.i2());
                self.line(x0, y0, x1, y1);
            }
            (0, b'R') => {
                let (x0, y0, x1, y1) = (a.i2(), a.i2(), a.i2(), a.i2());
                self.polyline(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], true);
            }
            (0, b'B') => {
                let (x0, y0, x1, y1) = (a.i2(), a.i2(), a.i2(), a.i2());
                self.bar(x0, y0, x1, y1);
            }
            (0, b'C') => {
                let (x, y, r) = (a.i2(), a.i2(), a.i2());
                let pts = arc_points(x, y, 0, 360, r as f64, r as f64 * ASPECT);
                self.polyline(&pts, false);
            }
            (0, b'O') => {
                let (x, y, st, end, xr, yr) = (a.i2(), a.i2(), a.i2(), a.i2(), a.i2(), a.i2());
                let pts = arc_points(x, y, st, end, xr as f64, yr as f64);
                self.polyline(&pts, false);
            }
            (0, b'o') => {
                let (x, y, xr, yr) = (a.i2(), a.i2(), a.i2(), a.i2());
                let pts = arc_points(x, y, 0, 360, xr as f64, yr as f64);
                self.fill_polygon(&pts);
                self.polyline(&pts, false);
            }
            (0, b'A') => {
                let (x, y, st, end, r) = (a.i2(), a.i2(), a.i2(), a.i2(), a.i2());
                let pts = arc_points(x, y, st, end, r as f64, r as f64 * ASPECT);
                self.polyline(&pts, false);
            }
            (0, b'V') => {
                let (x, y, st, end, xr, yr) = (a.i2(), a.i2(), a.i2(), a.i2(), a.i2(), a.i2());
                let pts = arc_points(x, y, st, end, xr as f64, yr as f64);
                self.polyline(&pts, false);
            }
            (0, b'I') => {
                let (x, y, st, end, r) = (a.i2(), a.i2(), a.i2(), a.i2(), a.i2());
                self.pie(x, y, st, end, r as f64, r as f64 * ASPECT);
            }
            (0, b'i') => {
                let (x, y, st, end, xr, yr) = (a.i2(), a.i2(), a.i2(), a.i2(), a.i2(), a.i2());
                self.pie(x, y, st, end, xr as f64, yr as f64);
            }
            (0, b'Z') => {
                let p: Vec<(f64, f64)> = (0..4).map(|_| (a.i2() as f64, a.i2() as f64)).collect();
                let n = a.n2().max(1);
                let pts: Vec<(i32, i32)> = (0..=n)
                    .map(|i| {
                        let t = i as f64 / n as f64;
                        let u = 1.0 - t;
                        let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        let x = b0 * p[0].0 + b1 * p[1].0 + b2 * p[2].0 + b3 * p[3].0;
                        let y = b0 * p[0].1 + b1 * p[1].1 + b2 * p[2].1 + b3 * p[3].1;
                        (x.round() as i32, y.round() as i32)
                    })
                    .collect();
                self.polyline(&pts, false);
            }
            (0, b'P' | b'p' | b'l') => {
                let n = a.n2();
                let pts: Vec<(i32, i32)> = (0..n).map(|_| (a.i2(), a.i2())).collect();
                if op == b'p' {
                    self.fill_polygon(&pts);
                }
                self.polyline(&pts, op != b'l');
            }
            (0, b'F') => {
                let (x, y, border) = (a.i2(), a.i2(), a.n2() as u8);
                self.flood(x, y, border & 15);
            }
            (0, b'=') => {
                let (style, user, thick) = (a.n2(), a.n4(), a.n2());
                self.line_style = LINE_STYLES.get(style).copied().unwrap_or(user as u16);
                self.thick = thick >= 3;
            }
            (0, b'S') => {
                let (pattern, color) = (a.n2(), a.n2());
                self.fill = FILL_PATTERNS.get(pattern).copied().unwrap_or(FILL_PATTERNS[1]);
                self.fill_color = color as u8 & 15;
            }
            (0, b's') => {
                for row in self.fill.iter_mut() {
                    *row = a.n2() as u8;
                }
                self.fill_color = a.n2() as u8 & 15;
            }
            (0, b'#') => {}
            (1, b'M') => {
                let (_num, x0, y0, x1, y1, clk, _clr) = (a.n2(), a.n2(), a.n2(), a.n2(), a.n2(), a.n1(), a.n1());
                a.skip(5);
                let text = host_command(a.rest());
                self.region(x0, y0, x1, y1, clk == 1, text);
            }
            (1, b'K') => self.regions.clear(),
            (1, b'T') => {
                let (x0, y0, x1) = (a.i2(), a.i2(), a.i2());
                self.text_block = Some(TextBlock { x0, x1, y: y0 });
            }
            (1, b't') => {
                let _justify = a.n1();
                let text = a.rest().to_vec();
                if let Some(tb) = self.text_block.as_mut() {
                    let (x0, x1, y) = (tb.x0, tb.x1, tb.y);
                    tb.y += 8 * self.font_size as i32;
                    let room = ((x1 - x0).max(0) as usize / (8 * self.font_size)).max(1);
                    self.pos = (x0, y);
                    self.text(&text[..text.len().min(room)]);
                }
            }
            (1, b'E') => self.text_block = None,
            (1, b'C') => {
                let (x0, y0, x1, y1) = (a.i2(), a.i2(), a.i2(), a.i2());
                self.copy_to_clipboard(x0, y0, x1, y1);
            }
            (1, b'P') => {
                let (x, y, mode) = (a.i2(), a.i2(), a.n2());
                self.paste(x, y, mode);
            }
            (1, b'G') => {
                let (x0, y0, x1, y1) = (a.i2(), a.i2(), a.i2(), a.i2());
                a.skip(2);
                let dest = a.i2();
                self.copy_rows(x0, y0, x1, y1, dest);
            }
            (1, b'B') => {
                self.button = ButtonStyle {
                    width: a.n2(),
                    height: a.n2(),
                    bevel: {
                        // The orientation and flags (icons, hot keys,
                        // radio groups) are not used.
                        a.skip(6);
                        a.n2()
                    },
                    dfore: a.n2() as u8 & 15,
                    dback: a.n2() as u8 & 15,
                    bright: a.n2() as u8 & 15,
                    dark: a.n2() as u8 & 15,
                    surface: a.n2() as u8 & 15,
                };
            }
            (1, b'U') => {
                let (x0, y0, x1, y1, _hotkey, _flags) = (a.i2(), a.i2(), a.i2(), a.i2(), a.n2(), a.n1());
                a.skip(1);
                let text = a.rest().to_vec();
                self.draw_button(x0, y0, x1, y1, &text);
            }
            _ => log::debug!("unhandled RIP command {level}{}", op as char),
        }
    }

    /// `RIP_RESET_WINDOWS`: everything back as it starts.
    fn reset(&mut self, s: &mut Screen) {
        *self = Rip { line: self.line.take(), eat_lf: self.eat_lf, ..Rip::default() };
        s.set_scroll_region(0, s.rows() - 1);
        s.erase_display(2);
        s.move_to(0, 0);
    }

    /// Set a pixel, at viewport coordinates, clipped to the viewport.
    fn plot(&mut self, x: i32, y: i32, c: u8) {
        let v = self.view;
        let (x, y) = (x + v.x0 as i32, y + v.y0 as i32);
        if x < v.x0 as i32 || y < v.y0 as i32 || x > v.x1 as i32 || y > v.y1 as i32 {
            return;
        }
        let p = &mut self.pixels[y as usize * WIDTH + x as usize];
        *p = if self.xor { *p ^ c } else { c };
    }

    fn get(&self, x: i32, y: i32) -> Option<u8> {
        let v = self.view;
        let (x, y) = (x + v.x0 as i32, y + v.y0 as i32);
        (x >= v.x0 as i32 && y >= v.y0 as i32 && x <= v.x1 as i32 && y <= v.y1 as i32)
            .then(|| self.pixels[y as usize * WIDTH + x as usize])
    }

    /// A pixel of the fill pattern: the fill color where its bit is set,
    /// the background (0) elsewhere.
    fn fill_at(&mut self, x: i32, y: i32) {
        let v = self.view;
        let bit = self.fill[(y + v.y0 as i32).rem_euclid(8) as usize] & (0x80 >> (x + v.x0 as i32).rem_euclid(8));
        let c = if bit != 0 { self.fill_color } else { 0 };
        self.plot(x, y, c);
    }

    /// A line in the line style, three pixels wide when thick.
    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let steep = -dy > dx;
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        let mut n = 0u32;
        loop {
            if self.line_style & (0x8000 >> (n % 16)) != 0 {
                self.plot(x, y, self.color);
                if self.thick {
                    let (ox, oy) = if steep { (1, 0) } else { (0, 1) };
                    self.plot(x - ox, y - oy, self.color);
                    self.plot(x + ox, y + oy, self.color);
                }
            }
            n += 1;
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

    fn polyline(&mut self, pts: &[(i32, i32)], closed: bool) {
        for w in pts.windows(2) {
            self.line(w[0].0, w[0].1, w[1].0, w[1].1);
        }
        if closed && pts.len() > 2 {
            let (a, b) = (pts[pts.len() - 1], pts[0]);
            self.line(a.0, a.1, b.0, b.1);
        }
    }

    fn bar(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        for y in y0.min(y1)..=y0.max(y1) {
            for x in x0.min(x1)..=x0.max(x1) {
                self.fill_at(x, y);
            }
        }
    }

    /// Fill a polygon (even-odd, by scan lines) with the fill pattern.
    fn fill_polygon(&mut self, pts: &[(i32, i32)]) {
        if pts.len() < 3 {
            return;
        }
        let (top, bottom) = pts.iter().fold((i32::MAX, i32::MIN), |(t, b), &(_, y)| (t.min(y), b.max(y)));
        for y in top..=bottom {
            let yc = y as f64 + 0.5;
            let mut xs: Vec<f64> = Vec::new();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                let (ay, by) = (a.1 as f64, b.1 as f64);
                if (ay <= yc) != (by <= yc) {
                    xs.push(a.0 as f64 + (yc - ay) / (by - ay) * (b.0 - a.0) as f64);
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks(2) {
                if let [l, r] = pair {
                    for x in l.round() as i32..=r.round() as i32 {
                        self.fill_at(x, y);
                    }
                }
            }
        }
    }

    fn pie(&mut self, x: i32, y: i32, st: i32, end: i32, xr: f64, yr: f64) {
        let mut pts = vec![(x, y)];
        pts.extend(arc_points(x, y, st, end, xr, yr));
        self.fill_polygon(&pts);
        self.polyline(&pts, true);
    }

    /// Flood the area around (x, y) up to the border color with the fill
    /// pattern.
    fn flood(&mut self, x: i32, y: i32, border: u8) {
        let v = self.view;
        let (w, h) = ((v.x1 - v.x0 + 1) as i32, (v.y1 - v.y0 + 1) as i32);
        if self.get(x, y).is_none_or(|c| c == border) {
            return;
        }
        let mut seen = vec![false; (w * h) as usize];
        let mut stack = vec![(x, y)];
        while let Some((px, py)) = stack.pop() {
            if px < 0 || py < 0 || px >= w || py >= h {
                continue;
            }
            let i = (py * w + px) as usize;
            if seen[i] || self.get(px, py) == Some(border) {
                continue;
            }
            // Fill this run of the line, then look above and below it.
            let mut l = px;
            while l > 0 && !seen[(py * w + l - 1) as usize] && self.get(l - 1, py) != Some(border) {
                l -= 1;
            }
            let mut r = px;
            while r + 1 < w && !seen[(py * w + r + 1) as usize] && self.get(r + 1, py) != Some(border) {
                r += 1;
            }
            for xx in l..=r {
                seen[(py * w + xx) as usize] = true;
                self.fill_at(xx, py);
                stack.push((xx, py - 1));
                stack.push((xx, py + 1));
            }
        }
    }

    /// Draw text at the drawing position in the 8x8 font, scaled by the
    /// font size, leaving the background; moves the position past it.
    fn text(&mut self, text: &[u8]) {
        let k = self.font_size as i32;
        for &ch in text {
            let (x0, y0) = self.pos;
            for gy in 0..8 {
                let bits = FONT_8X8[ch as usize * 8 + gy as usize];
                for gx in 0..8 {
                    if bits & (0x80 >> gx) == 0 {
                        continue;
                    }
                    for sy in 0..k {
                        for sx in 0..k {
                            // Vertical text runs upwards, rotated.
                            let (px, py) = if self.vertical {
                                (x0 + gy * k + sy, y0 - gx * k - sx)
                            } else {
                                (x0 + gx * k + sx, y0 + gy * k + sy)
                            };
                            self.plot(px, py, self.color);
                        }
                    }
                }
            }
            if self.vertical {
                self.pos.1 -= 8 * k;
            } else {
                self.pos.0 += 8 * k;
            }
        }
    }

    fn region(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, invert: bool, text: Vec<u8>) {
        let (x1, y1) = (x1.max(x0).min(WIDTH - 1), y1.max(y0).min(HEIGHT - 1));
        self.regions.push(MouseRegion { x0, y0, x1, y1, invert, text });
    }

    fn copy_to_clipboard(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let (w, h) = ((x1 - x0 + 1).max(0) as usize, (y1 - y0 + 1).max(0) as usize);
        let data = (0..h as i32)
            .flat_map(|dy| (0..w as i32).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| self.get(x0 + dx, y0 + dy).unwrap_or(0))
            .collect();
        self.clipboard = Some((w, h, data));
    }

    /// Put the clipboard at (x, y): copied, XORed, ORed, ANDed or inverted.
    fn paste(&mut self, x: i32, y: i32, mode: usize) {
        let Some((w, h, data)) = self.clipboard.clone() else { return };
        let xor = self.xor;
        self.xor = false;
        for dy in 0..h {
            for dx in 0..w {
                let (px, py) = (x + dx as i32, y + dy as i32);
                let (src, dst) = (data[dy * w + dx], self.get(px, py).unwrap_or(0));
                let c = match mode {
                    1 => dst ^ src,
                    2 => dst | src,
                    3 => dst & src,
                    4 => !src & 15,
                    _ => src,
                };
                self.plot(px, py, c);
            }
        }
        self.xor = xor;
    }

    /// `1G`: copy lines y0..=y1 (columns x0..=x1) to start at line `dest`.
    fn copy_rows(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, dest: i32) {
        let clipboard = self.clipboard.take();
        self.copy_to_clipboard(x0, y0, x1, y1);
        self.paste(x0, dest, 0);
        self.clipboard = clipboard;
    }

    /// `1U`: a button in the current style, with a mouse region when it has
    /// a host command. Its text is `icon<>label<>command`.
    fn draw_button(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, text: &[u8]) {
        let st = self.button;
        let (x1, y1) = if st.width > 0 && st.height > 0 {
            (x0 + st.width as i32 - 1, y0 + st.height as i32 - 1)
        } else {
            (x1, y1)
        };
        let mut parts = split_on(text, b"<>");
        let (_icon, label, command) =
            (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
        let saved = (self.fill, self.fill_color, self.color, self.line_style, self.thick, self.xor);
        (self.fill, self.line_style, self.thick, self.xor) = (FILL_PATTERNS[1], 0xFFFF, false, false);
        let b = st.bevel as i32;
        // The bevel: light along the top and left, dark along the others.
        if b > 0 {
            self.fill_color = st.bright;
            self.fill_polygon(&[(x0 - b, y0 - b), (x1 + b, y0 - b), (x1, y0), (x0, y0), (x0, y1), (x0 - b, y1 + b)]);
            self.fill_color = st.dark;
            self.fill_polygon(&[(x1 + b, y0 - b), (x1 + b, y1 + b), (x0 - b, y1 + b), (x0, y1), (x1, y1), (x1, y0)]);
        }
        self.fill_color = st.surface;
        self.bar(x0, y0, x1, y1);
        // The label, centered, with its shadow one pixel down and right.
        let k = self.font_size as i32;
        let (lw, lh) = (label.len() as i32 * 8 * k, 8 * k);
        let (lx, ly) = (x0 + (x1 - x0 + 1 - lw) / 2, y0 + (y1 - y0 + 1 - lh) / 2);
        let vertical = std::mem::replace(&mut self.vertical, false);
        if st.dback != st.surface {
            self.color = st.dback;
            self.pos = (lx + 1, ly + 1);
            self.text(label);
        }
        self.color = st.dfore;
        self.pos = (lx, ly);
        self.text(label);
        self.vertical = vertical;
        (self.fill, self.fill_color, self.color, self.line_style, self.thick, self.xor) = saved;
        if !command.is_empty() {
            let v = self.view;
            let at = |p: i32, o: usize| (p + o as i32).max(0) as usize;
            self.region(at(x0, v.x0), at(y0, v.y0), at(x1, v.x0), at(y1, v.y0), true, host_command(command));
        }
    }
}

/// Points along an elliptical arc from `st` to `end` degrees, counter-
/// clockwise from three o'clock as in BGI.
fn arc_points(cx: i32, cy: i32, st: i32, end: i32, xr: f64, yr: f64) -> Vec<(i32, i32)> {
    let end = if end <= st { end + 360 } else { end };
    let steps = ((end - st) as f64 * (xr.max(yr) / 20.0).max(1.0) / 3.0).ceil().clamp(4.0, 720.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = (st as f64 + (end - st) as f64 * i as f64 / steps as f64).to_radians();
            ((cx as f64 + xr * a.cos()).round() as i32, (cy as f64 - yr * a.sin()).round() as i32)
        })
        .collect()
}

/// A host command's text, with `^M`-style control characters made real.
fn host_command(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        match (text[i], text.get(i + 1)) {
            (b'^', Some(&c)) if (b'@'..=b'_').contains(&c.to_ascii_uppercase()) => {
                out.push(c.to_ascii_uppercase() - b'@');
                i += 2;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn split_on<'a>(text: &'a [u8], sep: &'a [u8]) -> impl Iterator<Item = &'a [u8]> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let r = rest?;
        match r.windows(sep.len()).position(|w| w == sep) {
            Some(i) => {
                rest = Some(&r[i + sep.len()..]);
                Some(&r[..i])
            }
            None => {
                rest = None;
                Some(r)
            }
        }
    })
}

/// A command's parameters: MegaNums (base 36) and the text after them.
struct Args<'a>(&'a [u8]);

impl Args<'_> {
    fn digits(&mut self, n: usize) -> usize {
        let (head, tail) = self.0.split_at(n.min(self.0.len()));
        self.0 = tail;
        head.iter().fold(0, |v, &c| {
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'A'..=b'Z' => c - b'A' + 10,
                b'a'..=b'z' => c - b'a' + 10,
                _ => 0,
            };
            v * 36 + d as usize
        })
    }

    fn n1(&mut self) -> usize {
        self.digits(1)
    }

    fn n2(&mut self) -> usize {
        self.digits(2)
    }

    fn n4(&mut self) -> usize {
        self.digits(4)
    }

    fn i2(&mut self) -> i32 {
        self.n2() as i32
    }

    fn skip(&mut self, n: usize) {
        self.0 = &self.0[n.min(self.0.len())..];
    }

    fn rest(&self) -> &[u8] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::{Encoding, Terminal};

    fn term(data: &[u8]) -> Terminal {
        let mut t = Terminal::new(COLS, ROWS, 0);
        t.encoding = Encoding::Rip;
        t.start_emulation();
        t.feed(data);
        t
    }

    fn px(t: &Terminal, x: usize, y: usize) -> u8 {
        t.rip().expect("RIP is on").pixels()[y * WIDTH + x]
    }

    #[test]
    fn megnums_and_escapes() {
        let mut a = Args(b"0A1Z00hello\\|");
        assert_eq!((a.n2(), a.n2(), a.n2()), (10, 71, 0));
        assert_eq!(host_command(b"go^m"), b"go\r");
        assert_eq!(split_on(b"a<>b<>c", b"<>").collect::<Vec<_>>(), [b"a" as &[u8], b"b", b"c"]);
    }

    #[test]
    fn draws_lines_bars_and_text_beside_ansi() {
        // Yellow (14) line, a red bar, then text on the text screen.
        let t = term(b"!|c0E|L00000A00|S0104|B0A0A0F0F\r\nHello\r\n");
        assert_eq!((px(&t, 0, 0), px(&t, 10, 0), px(&t, 11, 0)), (14, 14, 0));
        assert_eq!((px(&t, 12, 12), px(&t, 16, 16)), (4, 0));
        assert_eq!(Screen::row_text(t.screen.row(0)).trim_end(), "Hello");
        // `!` not followed by `|` is just text.
        let t = term(b"!Hi\r\n");
        assert_eq!(Screen::row_text(t.screen.row(0)).trim_end(), "!Hi");
    }

    #[test]
    fn continuation_fill_and_reset() {
        // A rectangle, continued over a line break, then flood its inside.
        let t = term(b"!|c0F|R0A0A1414|S01\\\r\n02|F0F0F0F\r\n");
        assert_eq!((px(&t, 10, 10), px(&t, 15, 15), px(&t, 39, 39), px(&t, 50, 50)), (15, 2, 2, 0));
        let t = term(b"!|c0F|L00000Z00|*\r\n");
        assert_eq!(px(&t, 5, 0), 0);
    }

    #[test]
    fn buttons_and_mouse_regions() {
        let t = term(b"!|1B000000000002000F0F0807000000000000|1U0A0A1E140000<>OK<>yes^M\r\n");
        let rip = t.rip().unwrap();
        assert_eq!(rip.click(15, 15), Some(b"yes\r".to_vec()));
        assert_eq!(rip.click(100, 100), None);
        // The surface is light gray (7), the bevel white above and left.
        assert_eq!(px(&t, 11, 19), 7);
        assert_eq!(px(&t, 9, 12), 15);
        let t = term(b"!|1M00000010101100000say hi\r\n");
        assert_eq!(t.rip().unwrap().click(5, 5), Some(b"say hi".to_vec()));
        let t = term(b"!|1M00000010101100000say hi\r\n!|1K\r\n");
        assert_eq!(t.rip().unwrap().click(5, 5), None);
    }

    #[test]
    fn answers_the_rip_query() {
        let mut t = term(b"\x1b[!");
        assert_eq!(t.take_replies(), VERSION_REPLY);
        assert_eq!(ega_rgb(63), 0xFFFFFF);
        assert_eq!(ega_rgb(20), 0xAA5500);
    }
}
