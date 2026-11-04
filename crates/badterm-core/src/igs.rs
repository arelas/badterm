//! Atari ST BBSes with IGS, Larry Mears's "Instant Graphics and Sound"
//! (version 2.19): VT-52 text and GEM VDI drawing on the ST's own screen,
//! 640x200 in four colors (medium resolution, where BBSes start), 320x200
//! in sixteen or 640x400 in two.
//!
//! `G#` gets IGS's attention; then a command letter, a `>` to chain
//! another command after it (anything else ends the chain), and numbers,
//! each ended by any one character that isn't a digit. The parsing follows
//! IG's own source (IG217.C, and IG 2.19's notes): numbers of up to four
//! digits, `_` and line breaks inside a number skipped, `x`, `y`, `r`, `R`,
//! `+n`, `-n` and `!n` for loops and random values.
//!
//! Everything, text included, is drawn into a bitmap of color registers,
//! which the front end shows like RIPscrip's graphics. Fill patterns, hatches,
//! line styles and marker shapes are the VDI's, as EmuTOS (GPL 2 or later)
//! defines them. Sound (the sound chip's effects, chip music and MIDI) is
//! read past and not played.

use std::time::{Duration, Instant};

use crate::palette::Rgb;

static FONT8: &[u8; 2048] = include_bytes!("../assets/vga_8x8.bin");
static FONT16: &[u8; 4096] = include_bytes!("../assets/vga_8x16.bin");
/// BadTerm's own 6x6 font (see assets/make_font6.py), for 8-point text.
static FONT6: &[u8; 1536] = include_bytes!("../assets/font6x6.bin");

pub const MAX_WIDTH: usize = 640;
pub const MAX_HEIGHT: usize = 400;
/// The text screen the front end lays out (the bitmap is stretched over it).
pub const COLS: usize = 80;
pub const ROWS: usize = 25;
/// What `G#?>0:` answers.
const VERSION: &[u8] = b"2.19";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    Low,
    Medium,
    High,
}

impl Res {
    pub fn size(self) -> (usize, usize) {
        match self {
            Res::Low => (320, 200),
            Res::Medium => (640, 200),
            Res::High => (640, 400),
        }
    }

    fn colors(self) -> u8 {
        match self {
            Res::Low => 16,
            Res::Medium => 4,
            Res::High => 2,
        }
    }

    /// The text cell: 8x8, or 8x16 in high resolution.
    fn cell_h(self) -> usize {
        if self == Res::High { 16 } else { 8 }
    }

    /// The color register a VDI pen draws in.
    fn register(self, pen: u8) -> u8 {
        const LOW: [u8; 16] = [0, 15, 1, 2, 4, 6, 3, 5, 7, 8, 9, 10, 12, 14, 11, 13];
        match self {
            Res::Low => LOW[(pen & 15) as usize],
            Res::Medium => [0, 3, 1, 2].get(pen as usize).copied().unwrap_or(3),
            Res::High => (pen != 0) as u8,
        }
    }

    /// A circle's height for its width: the screen's pixels aren't square.
    fn aspect(self) -> f64 {
        match self {
            Res::Low => 338.0 / 372.0,
            Res::Medium => 169.0 / 372.0,
            Res::High => 1.0,
        }
    }

    fn number(self) -> u8 {
        match self {
            Res::Low => 0,
            Res::Medium => 1,
            Res::High => 2,
        }
    }
}

/// The ST's colors are 3 bits a channel: 0x0RGB.
type StColor = u16;

/// The desktop's (TOS's) colors, by register.
const DESKTOP: [StColor; 16] =
    [0x777, 0x700, 0x070, 0x770, 0x007, 0x707, 0x077, 0x555, 0x333, 0x733, 0x373, 0x773, 0x337, 0x737, 0x377, 0x000];
const DESKTOP_MEDIUM: [StColor; 4] = [0x777, 0x700, 0x070, 0x000];
/// IG 2.18's own palette, by register.
const IG_PALETTE: [StColor; 16] =
    [0x777, 0x700, 0x070, 0x000, 0x007, 0x770, 0x077, 0x546, 0x532, 0x743, 0x043, 0x030, 0x333, 0x210, 0x134, 0x000];

fn rgb(c: StColor) -> Rgb {
    let v = |n: u16| ((n & 7) as u32 * 255 + 3) / 7;
    v(c >> 8) << 16 | v(c >> 4) << 8 | v(c)
}

// The VDI's patterns (from EmuTOS's vdi_fill.c and vdi_line.c).
const ROM_UD_PATTERN: [u16; 16] = [
    0x07E0, 0x0FF0, 0x1FD8, 0x1808, 0x1808, 0x1008, 0x1E78, 0x1348, 0x1108, 0x0810, 0x0B70, 0x0650, 0x07A0, 0x1E20,
    0x1BC0, 0x1800,
];
const DITHER: [u16; 32] = [
    0x0000, 0x4444, 0x0000, 0x1111, 0x0000, 0x5555, 0x0000, 0x5555, 0x8888, 0x5555, 0x2222, 0x5555, 0xAAAA, 0x5555,
    0xAAAA, 0x5555, 0xAAAA, 0xDDDD, 0xAAAA, 0x7777, 0xAAAA, 0xFFFF, 0xAAAA, 0xFFFF, 0xEEEE, 0xFFFF, 0xBBBB, 0xFFFF,
    0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF,
];
const OEMPAT: [u16; 128] = [
    0xFFFF, 0x8080, 0x8080, 0x8080, 0xFFFF, 0x0808, 0x0808, 0x0808, 0x2020, 0x4040, 0x8080, 0x4141, 0x2222, 0x1414,
    0x0808, 0x1010, 0x0000, 0x0000, 0x1010, 0x2828, 0x0000, 0x0000, 0x0101, 0x8282, 0x0202, 0x0202, 0xAAAA, 0x5050,
    0x2020, 0x2020, 0xAAAA, 0x0505, 0x4040, 0x8080, 0x0000, 0x0808, 0x0404, 0x0202, 0x0000, 0x2020, 0x6606, 0xC6C6,
    0xD8D8, 0x1818, 0x8181, 0x8DB1, 0x0C33, 0x6000, 0x0000, 0x0000, 0x0400, 0x0000, 0x0010, 0x0000, 0x8000, 0x0000,
    0xF8F8, 0x6C6C, 0xC6C6, 0x8F8F, 0x1F1F, 0x3636, 0x6363, 0xF1F1, 0xAAAA, 0x0000, 0x8888, 0x1414, 0x2222, 0x4141,
    0x8888, 0x0000, 0x0808, 0x0000, 0xAAAA, 0x0000, 0x0808, 0x0000, 0x8888, 0x0000, 0x7777, 0x9898, 0xF8F8, 0xF8F8,
    0x7777, 0x8989, 0x8F8F, 0x8F8F, 0x8080, 0x8080, 0x4141, 0x3E3E, 0x0808, 0x0808, 0x1414, 0xE3E3, 0x8181, 0x4242,
    0x2424, 0x1818, 0x0606, 0x0101, 0x8080, 0x8080, 0xF0F0, 0xF0F0, 0xF0F0, 0xF0F0, 0x0F0F, 0x0F0F, 0x0F0F, 0x0F0F,
    0x0808, 0x1C1C, 0x3E3E, 0x7F7F, 0xFFFF, 0x7F7F, 0x3E3E, 0x1C1C, 0x1111, 0x2222, 0x4444, 0xFFFF, 0x8888, 0x4444,
    0x2222, 0xFFFF,
];
const HATCH0: [u16; 48] = [
    0x0101, 0x0202, 0x0404, 0x0808, 0x1010, 0x2020, 0x4040, 0x8080, 0x6060, 0xC0C0, 0x8181, 0x0303, 0x0606, 0x0C0C,
    0x1818, 0x3030, 0x4242, 0x8181, 0x8181, 0x4242, 0x2424, 0x1818, 0x1818, 0x2424, 0x8080, 0x8080, 0x8080, 0x8080,
    0x8080, 0x8080, 0x8080, 0x8080, 0xFFFF, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0xFFFF, 0x8080,
    0x8080, 0x8080, 0x8080, 0x8080, 0x8080, 0x8080,
];
const HATCH1: [u16; 96] = [
    0x0001, 0x0002, 0x0004, 0x0008, 0x0010, 0x0020, 0x0040, 0x0080, 0x0100, 0x0200, 0x0400, 0x0800, 0x1000, 0x2000,
    0x4000, 0x8000, 0x8003, 0x0007, 0x000E, 0x001C, 0x0038, 0x0070, 0x00E0, 0x01C0, 0x0380, 0x0700, 0x0E00, 0x1C00,
    0x3800, 0x7000, 0xE000, 0xC001, 0x8001, 0x4002, 0x2004, 0x1008, 0x0810, 0x0420, 0x0240, 0x0180, 0x0180, 0x0240,
    0x0420, 0x0810, 0x1008, 0x2004, 0x4002, 0x8001, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000,
    0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0x8000, 0xFFFF, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0xFFFF, 0x8080, 0x8080, 0x8080,
    0x8080, 0x8080, 0x8080, 0x8080, 0xFFFF, 0x8080, 0x8080, 0x8080, 0x8080, 0x8080, 0x8080, 0x8080,
];
const LINE_STYLES: [u16; 6] = [0xFFFF, 0xFFF0, 0xC0C0, 0xFF18, 0xFF00, 0xF191];
/// Polymarkers (EmuTOS's vdi_marker.c): polylines of points, in units of
/// a 7-pixel marker.
const MARKERS: [&[&[(i32, i32)]]; 6] = [
    &[&[(0, 0), (0, 0)]],
    &[&[(0, -3), (0, 3)], &[(-4, 0), (4, 0)]],
    &[&[(0, -3), (0, 3)], &[(3, 2), (-3, -2)], &[(3, -2), (-3, 2)]],
    &[&[(-4, -3), (4, -3), (4, 3), (-4, 3), (-4, -3)]],
    &[&[(-4, -3), (4, 3)], &[(-4, 3), (4, -3)]],
    &[&[(-4, 0), (0, -3), (4, 0), (0, 3), (-4, 0)]],
];

/// A number as IG reads it, before a loop gives the letters their values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Num(i32),
    /// `x`: the loop's value.
    X,
    /// `y`: the loop's value counted the other way.
    Y,
    /// `r` and `R`: random, in their ranges.
    Rand,
    RandBig,
    /// `+n`, `-n`, `!n`: the loop's value plus n, minus n, n minus it.
    Plus(i32),
    Minus(i32),
    Bang(i32),
}

/// The bytes ran out before the command did: wait for more.
#[derive(Debug)]
struct Incomplete;

type Res2<T> = Result<T, Incomplete>;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn next(&mut self) -> Res2<u8> {
        let b = *self.data.get(self.pos).ok_or(Incomplete)?;
        self.pos += 1;
        Ok(b)
    }

    /// A number (see the module's notes).
    fn token(&mut self) -> Res2<Tok> {
        let (mut num, mut digits) = (0i32, 0);
        let mut sign: Option<u8> = None;
        loop {
            match self.next()? {
                d @ b'0'..=b'9' => {
                    num = num * 10 + (d - b'0') as i32;
                    digits += 1;
                }
                // Line breaks may split a number (IG 2.18), and `_` marks
                // where one does.
                b'\r' | b'\n' | b'_' => {}
                c @ (b'x' | b'y' | b'r' | b'R') if digits == 0 => {
                    self.next()?;
                    return Ok(match c {
                        b'x' => Tok::X,
                        b'y' => Tok::Y,
                        b'r' => Tok::Rand,
                        _ => Tok::RandBig,
                    });
                }
                c @ (b'+' | b'-' | b'!') if digits == 0 && sign.is_none() => sign = Some(c),
                _ => break,
            }
        }
        if digits > 4 {
            num = 0;
        }
        Ok(match sign {
            Some(b'+') => Tok::Plus(num),
            Some(b'-') => Tok::Minus(num),
            Some(_) => Tok::Bang(num),
            None => Tok::Num(num),
        })
    }

    /// Text up to `@` (line breaks dropped), at most 128 characters.
    fn text(&mut self) -> Res2<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            match self.next()? {
                b'@' => return Ok(out),
                b'\r' | b'\n' => {}
                b if out.len() < 128 => out.push(b),
                _ => return Ok(out),
            }
        }
    }
}

/// A running `&` loop: IG 2.17's value() for looping, ported as it is.
struct Loop {
    params: Vec<Tok>,
    at: usize,
    /// lp1 (the value), lp2 (to), lp3 (step), lp5 (from).
    value: i32,
    to: i32,
    step: i32,
    from: i32,
    /// `|` (XOR stepping) or `@` (W reads its text each time).
    kind: u8,
    x_count: i32,
    first: i32,
    second: i32,
    was: i32,
    /// No `x`-like parameters: the value steps once a pass.
    plain: bool,
    running: bool,
}

impl Loop {
    fn next(&mut self) -> Tok {
        if self.at == 0 {
            self.x_count = 0;
        }
        let t = self.params[self.at];
        let relative = !matches!(t, Tok::Num(_) | Tok::Rand | Tok::RandBig);
        let mut out = t;
        if relative {
            self.x_count += 1;
            if self.second == 0 {
                self.was = self.value;
            }
            if self.x_count < 2 && self.first != 0 {
                self.value += self.step;
                self.second += 1;
                if self.second == 1 && self.kind == b'|' {
                    self.value = self.was;
                }
                if self.second == 2 && self.kind == b'|' {
                    self.second = 0;
                }
            }
            self.first += 1;
            out = Tok::Num(match t {
                Tok::X => self.value,
                Tok::Y => match self.from.cmp(&self.to) {
                    std::cmp::Ordering::Less => self.to - self.value,
                    std::cmp::Ordering::Greater => self.from - self.value,
                    std::cmp::Ordering::Equal => 0,
                },
                Tok::Plus(n) => n + self.value,
                Tok::Minus(n) => self.value - n,
                Tok::Bang(n) => n - self.value,
                t => return t,
            });
        }
        if self.plain && self.at == 0 {
            self.value += self.step;
        }
        let last = self.at == self.params.len() - 1;
        if last && (self.step >= 1 && self.value >= self.to || self.step < 0 && self.value <= self.to) {
            self.running = false;
        }
        self.at = (self.at + 1) % self.params.len();
        out
    }
}

#[derive(Clone, Copy)]
struct LineAttr {
    pen: u8,
    style: u8,
    width: i32,
    /// 0 square, 1 arrow, 2 round: (start, end).
    ends: (u8, u8),
}

#[derive(Clone, Copy)]
struct FillAttr {
    pen: u8,
    /// 0 hollow, 1 solid, 2 pattern, 3 hatch, 4 user.
    interior: u8,
    style: u8,
    perimeter: bool,
}

#[derive(Clone, Copy)]
struct TextAttr {
    pen: u8,
    effects: u8,
    points: i32,
    /// Quarter turns.
    rotation: u8,
}

#[derive(Clone, Copy)]
struct Attrs {
    line: LineAttr,
    fill: FillAttr,
    text: TextAttr,
    marker: (u8, u8, i32),
    /// 1 replace, 2 transparent, 3 XOR, 4 reverse transparent.
    mode: u8,
}

impl Default for Attrs {
    fn default() -> Attrs {
        Attrs {
            line: LineAttr { pen: 1, style: 1, width: 1, ends: (0, 0) },
            fill: FillAttr { pen: 1, interior: 1, style: 1, perimeter: true },
            text: TextAttr { pen: 1, effects: 0, points: 9, rotation: 0 },
            marker: (1, 1, 1),
            mode: 1,
        }
    }
}

/// A mouse zone: its rectangle (x0, y0, x1, y1) and what it sends.
type Zone = (i32, i32, i32, i32, Vec<u8>);

/// What the VT-52 escape parser is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Esc {
    None,
    Start,
    Row,
    Col(u8),
    Fg,
    Bg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Text,
    /// After `G#`, or a `>`: a command letter comes next.
    Command,
}

pub struct Igs {
    pub res: Res,
    pixels: Vec<u8>,
    /// The blit buffer, screen sized.
    memory: Vec<u8>,
    memory_rect: (i32, i32, i32, i32),
    regs: [StColor; 16],
    attrs: Attrs,
    user_patterns: [[u16; 16]; 8],
    /// Where `D` draws from.
    pen_at: (i32, i32),
    hollow: bool,
    scaling: i32,
    rand: (i32, i32),
    rand_big: (i32, i32),
    seed: u32,
    // VT-52.
    col: usize,
    row: usize,
    fg: u8,
    bg: u8,
    reverse: bool,
    wrap: bool,
    pub cursor_on: bool,
    destructive_bs: bool,
    saved: (usize, usize),
    esc: Esc,
    // Parsing.
    mode: Mode,
    /// A `G` that may start `G#`.
    g: bool,
    buf: Vec<u8>,
    /// IG commands stored by `X 9`, and when they're to run.
    stored: Vec<u8>,
    run_stored: Option<Vec<u8>>,
    /// `t` and `q`: read nothing more until then.
    pub hold: Option<Instant>,
    looping: Option<Loop>,
    last_text: Vec<u8>,
    // The mouse.
    zones: Vec<Option<Zone>>,
    /// `<` with a mouse zone: (send a CR after, transmit it).
    awaiting_zone: Option<(bool, bool)>,
    /// What the BBS should get (replies, zones clicked).
    pub replies: Vec<u8>,
    pub bells: usize,
}

impl Default for Igs {
    fn default() -> Igs {
        let mut igs = Igs {
            res: Res::Medium,
            pixels: Vec::new(),
            memory: Vec::new(),
            memory_rect: (0, 0, 0, 0),
            regs: DESKTOP,
            attrs: Attrs::default(),
            user_patterns: [ROM_UD_PATTERN; 8],
            pen_at: (0, 0),
            hollow: false,
            scaling: 0,
            rand: (0, 199),
            rand_big: (0, 199),
            seed: 0x1234_5678,
            col: 0,
            row: 0,
            fg: 3,
            bg: 0,
            reverse: false,
            wrap: true,
            cursor_on: true,
            destructive_bs: false,
            saved: (0, 0),
            esc: Esc::None,
            mode: Mode::Text,
            g: false,
            buf: Vec::new(),
            stored: Vec::new(),
            run_stored: None,
            hold: None,
            looping: None,
            last_text: Vec::new(),
            zones: vec![None; 48],
            awaiting_zone: None,
            replies: Vec::new(),
            bells: 0,
        };
        igs.set_res(Res::Medium);
        igs.desktop_palette();
        igs
    }
}

impl Igs {
    pub fn width(&self) -> usize {
        self.res.size().0
    }

    pub fn height(&self) -> usize {
        self.res.size().1
    }

    /// The bitmap: a color register a pixel.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn palette(&self) -> [Rgb; 16] {
        std::array::from_fn(|i| rgb(self.regs[i]))
    }

    /// Where the VT-52 cursor is, on the front end's 80x25 text screen.
    pub fn cursor(&self) -> (usize, usize) {
        match self.res {
            Res::Low => (self.col * 2, self.row),
            Res::Medium => (self.col, self.row),
            Res::High => (self.col, self.row / 2),
        }
    }

    fn text_size(&self) -> (usize, usize) {
        (self.width() / 8, self.height() / self.res.cell_h())
    }

    fn set_res(&mut self, res: Res) {
        self.res = res;
        let (w, h) = res.size();
        self.pixels = vec![0; w * h];
        self.memory = vec![0; w * h];
        let (cols, rows) = self.text_size();
        self.col = self.col.min(cols - 1);
        self.row = self.row.min(rows - 1);
        let mask = res.colors() - 1;
        self.fg &= mask;
        self.bg &= mask;
        if res == Res::Medium {
            self.fg = 3;
        }
    }

    fn desktop_palette(&mut self) {
        self.regs = DESKTOP;
        match self.res {
            Res::Medium => self.regs[..4].copy_from_slice(&DESKTOP_MEDIUM),
            Res::High => self.regs[..2].copy_from_slice(&[0x777, 0x000]),
            Res::Low => {}
        }
    }

    fn random(&mut self, (lo, hi): (i32, i32)) -> i32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        lo + (self.seed % ((hi - lo).max(0) as u32 + 1)) as i32
    }

    /// Bytes from the BBS.
    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        let mut buf = std::mem::take(&mut self.buf);
        let mut pos = 0;
        while pos < buf.len() && self.hold.is_none() {
            match self.mode {
                Mode::Text => {
                    let b = buf[pos];
                    pos += 1;
                    self.text_byte(b);
                }
                Mode::Command => {
                    let mut r = Reader { data: &buf, pos };
                    match self.command(&mut r) {
                        Ok(chain) => {
                            pos = r.pos;
                            self.mode = if chain { Mode::Command } else { Mode::Text };
                            // X 9: run the stored commands next.
                            if let Some(run) = self.run_stored.take() {
                                buf.splice(pos..pos, run);
                            }
                        }
                        Err(Incomplete) => {
                            self.looping = None;
                            break;
                        }
                    }
                }
            }
        }
        buf.drain(..pos);
        // A command that never ends shouldn't swallow everything after it.
        if buf.len() > 1 << 20 {
            buf.clear();
            self.mode = Mode::Text;
        }
        self.buf = buf;
    }

    /// Carry on after a pause (`t`, `q`), once its time has come.
    pub fn poll(&mut self, now: Instant) {
        if self.hold.is_some_and(|t| now >= t) {
            self.hold = None;
            self.feed(&[]);
        }
    }

    // ---- VT-52 text ----

    fn text_byte(&mut self, b: u8) {
        if self.g {
            self.g = false;
            if b == b'#' && self.esc == Esc::None {
                self.mode = Mode::Command;
                return;
            }
            self.vt52(b'G');
        }
        if b == b'G' && self.esc == Esc::None {
            self.g = true;
            return;
        }
        self.vt52(b);
    }

    fn vt52(&mut self, b: u8) {
        let (cols, rows) = self.text_size();
        match self.esc {
            Esc::Start => {
                self.esc = Esc::None;
                self.vt52_escape(b);
                return;
            }
            Esc::Row => {
                self.esc = Esc::Col(b.saturating_sub(32));
                return;
            }
            Esc::Col(r) => {
                self.esc = Esc::None;
                self.row = (r as usize).min(rows - 1);
                self.col = (b.saturating_sub(32) as usize).min(cols - 1);
                return;
            }
            Esc::Fg => {
                self.esc = Esc::None;
                self.fg = b & (self.res.colors() - 1);
                return;
            }
            Esc::Bg => {
                self.esc = Esc::None;
                self.bg = b & (self.res.colors() - 1);
                return;
            }
            Esc::None => {}
        }
        match b {
            0x07 => self.bells += 1,
            0x08 => {
                self.col = self.col.saturating_sub(1);
                if self.destructive_bs {
                    self.glyph_cell(b' ');
                }
            }
            0x09 => self.col = ((self.col / 8 + 1) * 8).min(cols - 1),
            0x0A..=0x0C => self.line_feed(),
            0x0D => self.col = 0,
            0x1B => self.esc = Esc::Start,
            0x00..=0x1F => {}
            _ => {
                if self.col >= cols {
                    if !self.wrap {
                        self.col = cols - 1;
                    } else {
                        self.col = 0;
                        self.line_feed();
                    }
                }
                self.glyph_cell(b);
                self.col += 1;
                if self.col >= cols && !self.wrap {
                    self.col = cols - 1;
                }
            }
        }
    }

    fn vt52_escape(&mut self, b: u8) {
        let (cols, rows) = self.text_size();
        match b {
            b'A' => self.row = self.row.saturating_sub(1),
            b'B' => self.row = (self.row + 1).min(rows - 1),
            b'C' => self.col = (self.col + 1).min(cols - 1),
            b'D' => self.col = self.col.saturating_sub(1),
            b'E' => {
                self.clear_text_rows(0, rows);
                (self.col, self.row) = (0, 0);
            }
            b'H' => (self.col, self.row) = (0, 0),
            b'I' => {
                if self.row == 0 {
                    self.scroll_text(0, false);
                } else {
                    self.row -= 1;
                }
            }
            b'J' => {
                self.clear_line_from(self.col);
                self.clear_text_rows(self.row + 1, rows);
            }
            b'K' => self.clear_line_from(self.col),
            b'L' => {
                self.scroll_text(self.row, false);
                self.col = 0;
            }
            b'M' => {
                self.scroll_text(self.row, true);
                self.col = 0;
            }
            b'Y' => self.esc = Esc::Row,
            b'b' => self.esc = Esc::Fg,
            b'c' => self.esc = Esc::Bg,
            b'd' => {
                self.clear_text_rows(0, self.row);
                self.clear_line_to(self.col);
            }
            b'e' => self.cursor_on = true,
            b'f' => self.cursor_on = false,
            b'j' => self.saved = (self.col, self.row),
            b'k' => (self.col, self.row) = self.saved,
            b'l' => self.clear_text_rows(self.row, self.row + 1),
            b'o' => self.clear_line_to(self.col),
            b'p' => self.reverse = true,
            b'q' => self.reverse = false,
            b'v' => self.wrap = true,
            b'w' => self.wrap = false,
            _ => {}
        }
    }

    fn line_feed(&mut self) {
        let rows = self.text_size().1;
        if self.row + 1 >= rows {
            self.scroll_text(0, true);
        } else {
            self.row += 1;
        }
    }

    /// Scroll the text rows from `top` down: up a row (`up`), or down,
    /// leaving a blank row.
    fn scroll_text(&mut self, top: usize, up: bool) {
        let (w, h) = self.res.size();
        let ch = self.res.cell_h();
        let (start, end) = (top * ch * w, h * w);
        if start >= end {
            return;
        }
        if up {
            self.pixels.copy_within(start + ch * w..end, start);
            self.pixels[end - ch * w..end].fill(self.bg);
        } else {
            self.pixels.copy_within(start..end - ch * w, start + ch * w);
            self.pixels[start..start + ch * w].fill(self.bg);
        }
    }

    fn clear_text_rows(&mut self, from: usize, to: usize) {
        let w = self.width();
        let ch = self.res.cell_h();
        let (a, b) = ((from * ch * w).min(self.pixels.len()), (to * ch * w).min(self.pixels.len()));
        if a < b {
            self.pixels[a..b].fill(self.bg);
        }
    }

    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        let w = self.width();
        let ch = self.res.cell_h();
        for y in row * ch..(row + 1) * ch {
            let line = &mut self.pixels[y * w..(y + 1) * w];
            line[(from * 8).min(w)..(to * 8).min(w)].fill(self.bg);
        }
    }

    fn clear_line_from(&mut self, col: usize) {
        let cols = self.text_size().0;
        self.clear_cells(self.row, col, cols);
    }

    fn clear_line_to(&mut self, col: usize) {
        self.clear_cells(self.row, 0, col + 1);
    }

    /// A character at the cursor, in the text colors.
    fn glyph_cell(&mut self, ch: u8) {
        let w = self.width();
        let h = self.res.cell_h();
        let (fg, bg) = if self.reverse { (self.bg, self.fg) } else { (self.fg, self.bg) };
        let (x0, y0) = (self.col * 8, self.row * h);
        for gy in 0..h {
            let bits = glyph_row(ch, gy, h);
            let row = &mut self.pixels[(y0 + gy) * w + x0..][..8];
            for (gx, p) in row.iter_mut().enumerate() {
                *p = if bits & (0x80 >> gx) != 0 { fg } else { bg };
            }
        }
    }

    // ---- Drawing ----

    fn mask(&self) -> u8 {
        self.res.colors() - 1
    }

    /// A pixel in the writing mode: `on` is the pattern's bit there.
    fn plot(&mut self, x: i32, y: i32, on: bool, color: u8, mode: u8) {
        let (w, h) = self.res.size();
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            return;
        }
        let p = &mut self.pixels[y as usize * w + x as usize];
        match mode {
            2 if on => *p = color,
            3 if on => *p ^= self.res.colors() - 1,
            4 if !on => *p = color,
            1 => *p = if on { color } else { 0 },
            _ => {}
        }
    }

    fn reg(&self, pen: u8) -> u8 {
        self.res.register(pen)
    }

    /// The fill pattern's bit at a pixel.
    fn pattern_bit(&self, x: i32, y: i32) -> bool {
        let f = self.attrs.fill;
        let fi = f.style.max(1) as usize - 1;
        let word = match f.interior {
            0 => return false,
            1 => return true,
            2 if fi < 8 => DITHER[fi * 4 + (y & 3) as usize],
            2 => OEMPAT[((fi - 8).min(15)) * 8 + (y & 7) as usize],
            3 if fi < 6 => HATCH0[fi * 8 + (y & 7) as usize],
            3 => HATCH1[((fi - 6).min(5)) * 16 + (y & 15) as usize],
            _ => self.user_patterns[fi.min(7)][(y & 15) as usize],
        };
        word & (0x8000 >> (x & 15)) != 0
    }

    /// A horizontal span of the fill.
    fn fill_span(&mut self, x0: i32, x1: i32, y: i32) {
        if self.attrs.fill.interior == 0 {
            return;
        }
        let color = self.reg(self.attrs.fill.pen);
        let mode = self.attrs.mode;
        for x in x0.max(0)..=x1.min(self.width() as i32 - 1) {
            let on = self.pattern_bit(x, y);
            self.plot(x, y, on, color, mode);
        }
    }

    /// A filled polygon (even-odd), and its outline if the perimeter is on.
    fn polygon(&mut self, pts: &[(i32, i32)], perimeter: bool) {
        if pts.is_empty() {
            return;
        }
        let (ymin, ymax) = pts.iter().fold((i32::MAX, i32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        let h = self.height() as i32;
        for y in ymin.max(0)..=ymax.min(h - 1) {
            let mut xs: Vec<i32> = Vec::new();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                let (lo, hi) = if a.1 < b.1 { (a, b) } else { (b, a) };
                if y >= lo.1 && y < hi.1 || y == ymax && hi.1 == ymax && lo.1 != hi.1 {
                    let t = (y - lo.1) as f64 / (hi.1 - lo.1) as f64;
                    xs.push((lo.0 as f64 + t * (hi.0 - lo.0) as f64).round() as i32);
                }
            }
            xs.sort_unstable();
            for pair in xs.chunks(2) {
                if let [a, b] = pair {
                    self.fill_span(*a, *b, y);
                }
            }
        }
        if perimeter {
            let color = self.reg(self.attrs.fill.pen);
            let mut closed = pts.to_vec();
            closed.push(pts[0]);
            for seg in closed.windows(2) {
                self.thin_line(seg[0], seg[1], color, 0xFFFF, self.attrs.mode);
            }
        }
    }

    /// A one-pixel line in a style.
    fn thin_line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), color: u8, style: u16, mode: u8) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        let mut bit = 0u32;
        loop {
            let on = style.rotate_left(bit) & 0x8000 != 0;
            // Lines draw their pattern's zero bits only when replacing.
            if on || mode == 1 || mode == 4 {
                self.plot(x, y, on, color, mode);
            }
            bit += 1;
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

    /// A line in the line attributes: wide lines are filled polygons,
    /// with square, round or arrow ends.
    fn line(&mut self, a: (i32, i32), b: (i32, i32)) {
        let l = self.attrs.line;
        let color = self.reg(l.pen);
        let style = match l.style {
            1..=6 => LINE_STYLES[l.style as usize - 1],
            _ => {
                let p = self.user_patterns[6];
                p[0]
            }
        };
        if l.width <= 1 {
            self.thin_line(a, b, color, style, self.attrs.mode);
            self.line_ends(a, b, color, 1);
            return;
        }
        let saved = self.attrs.fill;
        self.attrs.fill = FillAttr { pen: l.pen, interior: 1, style: 1, perimeter: false };
        let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let half = l.width as f64 / 2.0;
        // Perpendicular, with y squashed for the screen's pixel shape.
        let (px, py) = (-dy / len * half, dx / len * half * self.res.aspect());
        let q = |p: (i32, i32), s: f64| ((p.0 as f64 + px * s).round() as i32, (p.1 as f64 + py * s).round() as i32);
        self.polygon(&[q(a, 1.0), q(b, 1.0), q(b, -1.0), q(a, -1.0)], false);
        self.line_ends(a, b, color, l.width);
        self.attrs.fill = saved;
    }

    fn line_ends(&mut self, a: (i32, i32), b: (i32, i32), color: u8, width: i32) {
        let ends = self.attrs.line.ends;
        for (end, (at, from)) in [(ends.0, (a, b)), (ends.1, (b, a))] {
            match end {
                1 => self.arrow(at, from, color, width),
                2 if width > 1 => {
                    let r = width / 2;
                    let saved = self.attrs.fill;
                    self.attrs.fill = FillAttr { pen: self.attrs.line.pen, interior: 1, style: 1, perimeter: false };
                    let pts = self.ellipse_points(at, r, (r as f64 * self.res.aspect()) as i32, 0, 3600);
                    self.polygon(&pts, false);
                    self.attrs.fill = saved;
                }
                _ => {}
            }
        }
    }

    fn arrow(&mut self, tip: (i32, i32), from: (i32, i32), _color: u8, width: i32) {
        let (dx, dy) = ((tip.0 - from.0) as f64, (tip.1 - from.1) as f64);
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let size = (8 + 2 * width) as f64;
        let (ux, uy) = (dx / len, dy / len);
        let back = (tip.0 as f64 - ux * size, tip.1 as f64 - uy * size);
        let side = size / 2.0;
        let p1 = ((back.0 - uy * side) as i32, (back.1 + ux * side * self.res.aspect()) as i32);
        let p2 = ((back.0 + uy * side) as i32, (back.1 - ux * side * self.res.aspect()) as i32);
        let saved = self.attrs.fill;
        self.attrs.fill = FillAttr { pen: self.attrs.line.pen, interior: 1, style: 1, perimeter: false };
        self.polygon(&[tip, p1, p2], false);
        self.attrs.fill = saved;
    }

    /// Points around an ellipse from `start` to `end`, tenths of a degree
    /// counter-clockwise from three o'clock (the VDI's way).
    fn ellipse_points(&self, c: (i32, i32), rx: i32, ry: i32, start: i32, end: i32) -> Vec<(i32, i32)> {
        let mut span = end - start;
        if span <= 0 {
            span += 3600;
        }
        let steps = (rx.max(ry) / 4).clamp(32, 128);
        (0..=steps)
            .map(|i| {
                let a = (start as f64 + span as f64 * i as f64 / steps as f64).to_radians() / 10.0;
                ((c.0 as f64 + rx as f64 * a.cos()).round() as i32, (c.1 as f64 - ry as f64 * a.sin()).round() as i32)
            })
            .collect()
    }

    fn polyline(&mut self, pts: &[(i32, i32)]) {
        for seg in pts.windows(2) {
            self.line(seg[0], seg[1]);
        }
    }

    /// A filled shape: interior, then its outline if the perimeter's on.
    fn shape(&mut self, pts: &[(i32, i32)]) {
        self.polygon(pts, self.attrs.fill.perimeter);
    }

    fn rounded_box(&mut self, (x0, y0, x1, y1): (i32, i32, i32, i32), filled: bool) {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (y0, y1) = (y0.min(y1), y0.max(y1));
        let rx = ((x1 - x0) / 2).min(8);
        let ry = (((y1 - y0) / 2) as f64).min(8.0 * self.res.aspect()) as i32;
        let mut pts = Vec::new();
        for (cx, cy, a0) in
            [(x1 - rx, y0 + ry, 0), (x0 + rx, y0 + ry, 900), (x0 + rx, y1 - ry, 1800), (x1 - rx, y1 - ry, 2700)]
        {
            pts.extend(self.ellipse_points((cx, cy), rx, ry, a0, a0 + 900));
        }
        if filled {
            self.shape(&pts);
        } else {
            pts.push(pts[0]);
            self.polyline(&pts);
        }
    }

    /// Fill the area of one color around a point (`F`).
    fn flood(&mut self, x: i32, y: i32) {
        let (w, h) = self.res.size();
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            return;
        }
        let target = self.pixels[y as usize * w + x as usize];
        let mut seen = vec![false; w * h];
        let mut stack = vec![(x as usize, y as usize)];
        let mut spans = Vec::new();
        while let Some((x, y)) = stack.pop() {
            if seen[y * w + x] || self.pixels[y * w + x] != target {
                continue;
            }
            let (mut a, mut b) = (x, x);
            while a > 0 && !seen[y * w + a - 1] && self.pixels[y * w + a - 1] == target {
                a -= 1;
            }
            while b + 1 < w && !seen[y * w + b + 1] && self.pixels[y * w + b + 1] == target {
                b += 1;
            }
            for i in a..=b {
                seen[y * w + i] = true;
                for ny in [y.wrapping_sub(1), y + 1] {
                    if ny < h && !seen[ny * w + i] && self.pixels[ny * w + i] == target {
                        stack.push((i, ny));
                    }
                }
            }
            spans.push((a as i32, b as i32, y as i32));
        }
        for (a, b, y) in spans {
            self.fill_span(a, b, y);
        }
    }

    fn marker(&mut self, x: i32, y: i32) {
        let (pen, kind, size) = self.attrs.marker;
        let color = self.reg(pen);
        let shape = MARKERS[(kind.clamp(1, 6) - 1) as usize];
        let scale = if kind == 1 { 0.0 } else { size.max(1) as f64 };
        for poly in shape {
            let pts: Vec<(i32, i32)> =
                poly.iter().map(|&(dx, dy)| (x + (dx as f64 * scale) as i32, y + (dy as f64 * scale) as i32)).collect();
            for seg in pts.windows(2) {
                self.thin_line(seg[0], seg[1], color, 0xFFFF, self.attrs.mode);
            }
        }
    }

    /// Text at a point (its baseline), as `W` writes it.
    fn write_text(&mut self, x: i32, y: i32, text: &[u8]) {
        let t = self.attrs.text;
        let color = self.reg(t.pen);
        // The system font's sizes, as the ST has them: 6x6 at 8 points,
        // 8x8 at 9, 8x16 at 10, and twice those at 16, 18 and 20.
        let (cell, scale) = match t.points {
            ..=8 => (6, 1),
            9 => (8, 1),
            10..=15 => (16, 1),
            16..=17 => (6, 2),
            18..=19 => (8, 2),
            _ => (16, 2),
        };
        let ascent = match cell {
            6 => 4,
            8 => 6,
            _ => 13,
        } * scale;
        let (cw, ch) = (cell.min(8) * scale, cell * scale);
        for (i, &c) in text.iter().enumerate() {
            for gy in 0..ch {
                let mut bits = glyph_row(c, (gy / scale) as usize, cell as usize) as u32;
                if t.effects & 1 != 0 {
                    bits |= bits >> 1;
                }
                if t.effects & 2 != 0 && gy % 2 == 1 {
                    bits &= 0x55;
                }
                if t.effects & 8 != 0 && gy == ascent + scale {
                    bits = 0xFF;
                }
                let skew = if t.effects & 4 != 0 { (ch - gy) / 4 } else { 0 };
                for gx in 0..cw {
                    let on = bits & (0x80 >> (gx / scale)) != 0;
                    if !on && self.attrs.mode != 1 {
                        continue;
                    }
                    // Along the text and down from its top, turned.
                    let (ax, ay) = ((i as i32) * cw + gx + skew, gy - ascent);
                    let (px, py) = match t.rotation % 4 {
                        1 => (x + ay, y - ax),
                        2 => (x - ax, y - ay),
                        3 => (x - ay, y + ax),
                        _ => (x + ax, y + ay),
                    };
                    self.plot(px, py, on, color, self.attrs.mode);
                }
            }
        }
    }

    /// `G`: copy pixels between the screen and the blit buffer, combining
    /// them with one of the 16 logic operations.
    fn blit(&mut self, from_memory: bool, src: (i32, i32, i32, i32), to_memory: bool, dst: (i32, i32), op: u8) {
        let (w, h) = self.res.size();
        let (x0, y0, x1, y1) = (src.0.min(src.2), src.1.min(src.3), src.0.max(src.2), src.1.max(src.3));
        let source: Vec<u8> = if from_memory { self.memory.clone() } else { self.pixels.clone() };
        let mask = self.mask();
        for y in 0..=(y1 - y0) {
            for x in 0..=(x1 - x0) {
                let (sx, sy, dx, dy) = (x0 + x, y0 + y, dst.0 + x, dst.1 + y);
                if sx < 0 || sy < 0 || dx < 0 || dy < 0 {
                    continue;
                }
                let (sx, sy, dx, dy) = (sx as usize, sy as usize, dx as usize, dy as usize);
                if sx >= w || sy >= h || dx >= w || dy >= h {
                    continue;
                }
                let s = source[sy * w + sx];
                let target = if to_memory { &mut self.memory } else { &mut self.pixels };
                let d = target[dy * w + dx];
                let v = match op {
                    0 => 0,
                    1 => s & d,
                    2 => s & !d,
                    4 => !s & d,
                    5 => d,
                    6 => s ^ d,
                    7 => s | d,
                    8 => !(s | d),
                    9 => !(s ^ d),
                    10 => !d,
                    11 => s | !d,
                    12 => !s,
                    13 => !s | d,
                    14 => !(s & d),
                    15 => 0xFF,
                    _ => s,
                };
                target[dy * w + dx] = v & mask;
            }
        }
    }

    fn clear_screen(&mut self) {
        self.pixels.fill(0);
    }
}

/// A row of a glyph, 6, 8 or 16 high (CP437 at 8 and 16; ASCII at 6,
/// where anything else comes from the 8x8 font).
fn glyph_row(c: u8, y: usize, h: usize) -> u8 {
    match h {
        16 => FONT16[c as usize * 16 + y.min(15)],
        6 if (0x20..0x7F).contains(&c) => FONT6[c as usize * 6 + y.min(5)],
        6 => FONT8[c as usize * 8 + (y * 8 / 6).min(7)],
        _ => FONT8[c as usize * 8 + y.min(7)],
    }
}

// The command interpreter, in its own impl for its size.
impl Igs {
    /// A number, from the stream or a running loop, scaled by `g` if it's
    /// a coordinate (`axis` 1 for x, 2 for y).
    fn val(&mut self, r: &mut Reader, axis: u8) -> Res2<i32> {
        let tok = match self.looping.as_mut() {
            Some(lp) if lp.running || lp.at != 0 => lp.next(),
            _ => r.token()?,
        };
        let mut n = match tok {
            Tok::Num(n) | Tok::Plus(n) | Tok::Bang(n) => n,
            Tok::Minus(n) => -n,
            Tok::Rand => self.random(self.rand),
            Tok::RandBig => self.random(self.rand_big),
            Tok::X | Tok::Y => 0,
        };
        if self.scaling == 1 && axis > 0 && n > 0 {
            let max = if axis == 1 { self.width() - 1 } else { self.height() - 1 } as f64;
            n = (n as f64 * max / 9999.0).round() as i32;
        } else if self.scaling == 2 && axis == 2 && self.res == Res::High {
            n *= 2;
        }
        Ok(n)
    }

    fn point(&mut self, r: &mut Reader) -> Res2<(i32, i32)> {
        Ok((self.val(r, 1)?, self.val(r, 2)?))
    }

    /// One command, its letter next in `r`. Returns whether a chained
    /// command follows.
    fn command(&mut self, r: &mut Reader) -> Res2<bool> {
        let mut key = r.next()?;
        // A new G# where a chained command was expected starts over.
        while key == b'G' && r.data.get(r.pos) == Some(&b'#') {
            r.pos += 1;
            key = r.next()?;
        }
        let chain = r.next()? == b'>';
        if key == b'&' {
            self.loop_command(r)?;
        } else {
            self.run(key, r)?;
        }
        Ok(chain)
    }

    /// `&`: run a command (or a "chain gang" of them) many times.
    fn loop_command(&mut self, r: &mut Reader) -> Res2<()> {
        let (from, to, step, _delay) = (r.token()?, r.token()?, r.token()?, r.token()?);
        let num = |t: Tok| if let Tok::Num(n) = t { n } else { 0 };
        let (from, to, step) = (num(from), num(to), num(step));
        let mut step = if from > to { -step } else { step };
        if step == 0 {
            step = 1;
        }
        let key = r.next()?;
        let gang = if key == b'>' { Some(r.text()?) } else { None };
        let kind = r.next()?;
        let count = (num(r.token()?)).clamp(1, 2048) as usize;
        let mut params = Vec::with_capacity(count);
        for _ in 0..count {
            params.push(r.token()?);
        }
        let plain = !params.iter().any(|t| !matches!(t, Tok::Num(_) | Tok::Rand | Tok::RandBig));
        self.looping = Some(Loop {
            params,
            at: 0,
            value: from,
            to,
            step,
            from,
            kind,
            x_count: 0,
            first: 0,
            second: 0,
            was: from,
            plain,
            running: true,
        });
        let mut guard = 0;
        loop {
            let cmd = match &gang {
                Some(g) => {
                    let i = self.val(r, 0)?.clamp(0, 127) as usize;
                    g.get(i).copied().unwrap_or(b' ')
                }
                None => key,
            };
            // W reads its text from the stream each time only with `@`.
            self.run(cmd, r)?;
            guard += 1;
            let done = self.looping.as_ref().is_none_or(|lp| !lp.running && lp.at == 0);
            if done || guard > 100_000 {
                break;
            }
        }
        self.looping = None;
        Ok(())
    }

    fn in_loop(&self) -> bool {
        self.looping.is_some()
    }

    fn run(&mut self, key: u8, r: &mut Reader) -> Res2<()> {
        match key {
            b'A' => {
                let (t, p, b) = (self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?);
                let f = &mut self.attrs.fill;
                f.interior = if (0..=4).contains(&t) { t as u8 } else { 0 };
                if f.interior == 4 && p >= 8 {
                    // 8: random sand in slot 0; 9 and up: its default.
                    self.user_patterns[0] = if p == 8 {
                        std::array::from_fn(|_| {
                            self.seed ^= self.seed << 13;
                            self.seed ^= self.seed >> 17;
                            self.seed ^= self.seed << 5;
                            (self.seed & self.seed >> 7) as u16
                        })
                    } else {
                        ROM_UD_PATTERN
                    };
                    self.attrs.fill.style = 1;
                } else {
                    self.attrs.fill.style =
                        if self.attrs.fill.interior == 4 { (p + 1).clamp(1, 8) as u8 } else { p.clamp(1, 24) as u8 };
                }
                self.attrs.fill.perimeter = b != 0;
            }
            b'B' => {
                let (a, b, round) = (self.point(r)?, self.point(r)?, self.val(r, 0)?);
                if round != 0 {
                    self.rounded_box((a.0, a.1, b.0, b.1), true);
                } else {
                    self.shape(&[(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1)]);
                }
            }
            b'C' => {
                let (which, pen) = (self.val(r, 0)?, self.val(r, 0)?);
                let pen = if (0..=15).contains(&pen) { pen as u8 } else { 0 };
                match which {
                    0 => self.attrs.marker.0 = pen,
                    1 => self.attrs.line.pen = pen,
                    2 => self.attrs.fill.pen = pen,
                    3 => self.attrs.text.pen = pen,
                    _ => self.attrs.marker.0 = pen,
                }
            }
            b'D' => {
                let to = self.point(r)?;
                let from = self.pen_at;
                self.line(from, to);
                self.pen_at = to;
            }
            b'E' => {
                let (fx, size, rot) = (self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?);
                self.attrs.text.effects = fx as u8;
                self.attrs.text.points = size.clamp(8, 20);
                self.attrs.text.rotation = if (0..=4).contains(&rot) { rot as u8 } else { 0 };
            }
            b'F' => {
                let (x, y) = self.point(r)?;
                self.flood(x, y);
            }
            b'f' | b'z' => {
                let n = self.val(r, 0)?.clamp(0, 128) as usize;
                let mut pts = Vec::with_capacity(n);
                for _ in 0..n {
                    pts.push(self.point(r)?);
                }
                if key == b'f' {
                    self.shape(&pts);
                } else if n >= 2 {
                    self.polyline(&pts);
                }
            }
            b'G' => self.grab(r)?,
            b'g' => {
                let v = self.val(r, 0)?;
                self.scaling = if (0..=2).contains(&v) { v } else { 0 };
            }
            b'H' => {
                self.hollow = self.val(r, 0)? != 0;
                if self.hollow {
                    self.attrs.fill.interior = 0;
                    self.attrs.mode = 2;
                    self.attrs.fill.perimeter = true;
                } else {
                    self.attrs.fill.interior = 1;
                    self.attrs.mode = 1;
                    self.attrs.fill.perimeter = false;
                }
            }
            b'I' => match self.val(r, 0)? {
                1 => self.desktop_palette(),
                2 => self.attrs = Attrs::default(),
                3 => self.regs = IG_PALETTE,
                4 => self.desktop_palette(),
                5 => {
                    if self.res == Res::Low {
                        self.set_res(Res::Medium);
                    }
                }
                _ => {
                    self.desktop_palette();
                    self.attrs = Attrs::default();
                }
            },
            b'J' | b'Y' => {
                let (c, rx, ry) = (self.point(r)?, self.val(r, 1)?, self.val(r, 2)?);
                let (a0, a1) = (self.val(r, 0)? * 10, self.val(r, 0)? * 10);
                let pts = self.ellipse_points(c, rx, ry, a0, a1);
                if key == b'J' {
                    self.polyline(&pts);
                } else {
                    let mut pie = vec![c];
                    pie.extend(pts);
                    self.shape(&pie);
                }
            }
            b'K' | b'V' => {
                let (c, rad) = (self.point(r)?, self.val(r, 1)?);
                let (a0, a1) = (self.val(r, 0)? * 10, self.val(r, 0)? * 10);
                let ry = (rad as f64 * self.res.aspect()).round() as i32;
                let pts = self.ellipse_points(c, rad, ry, a0, a1);
                if key == b'K' {
                    self.polyline(&pts);
                } else {
                    let mut pie = vec![c];
                    pie.extend(pts);
                    self.shape(&pie);
                }
            }
            b'k' => match self.val(r, 0)? {
                0 => self.cursor_on = false,
                1 => self.cursor_on = true,
                2 => self.destructive_bs = true,
                _ => self.destructive_bs = false,
            },
            b'L' => {
                let (a, b) = (self.point(r)?, self.point(r)?);
                self.line(a, b);
                self.pen_at = b;
            }
            b'M' => {
                let m = self.val(r, 0)?;
                self.attrs.mode = if (1..=4).contains(&m) { m as u8 } else { 1 };
            }
            b'N' => self.noise(r)?,
            b'n' => {
                for _ in 0..6 {
                    self.val(r, 0)?;
                }
            }
            b'O' => {
                let (c, rad) = (self.point(r)?, self.val(r, 1)?);
                let ry = (rad as f64 * self.res.aspect()).round() as i32;
                let pts = self.ellipse_points(c, rad, ry, 0, 3600);
                self.shape(&pts);
            }
            b'P' => {
                let p = self.point(r)?;
                self.marker(p.0, p.1);
                self.pen_at = p;
            }
            b'Q' => {
                let (c, rx, ry) = (self.point(r)?, self.val(r, 1)?, self.val(r, 2)?);
                let pts = self.ellipse_points(c, rx, ry, 0, 3600);
                self.shape(&pts);
            }
            b'q' => {
                let n = self.val(r, 0)?;
                if (1..=180).contains(&n) && !self.in_loop() {
                    self.hold = Some(Instant::now() + Duration::from_millis(n as u64 * 1000 / 60));
                }
            }
            b'R' => {
                let (res, pal) = (self.val(r, 0)?, self.val(r, 0)?);
                let want = match res {
                    0 => Res::Low,
                    1 => Res::Medium,
                    _ => self.res,
                };
                if self.res != Res::High && want != self.res {
                    self.set_res(want);
                }
                match pal {
                    1 | 3 => self.desktop_palette(),
                    2 => self.regs = IG_PALETTE,
                    _ => {}
                }
            }
            b'S' => {
                let pen = self.val(r, 0)?;
                let (cr, cg, cb) = (self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?);
                let pen = if (0..=15).contains(&pen) { pen as u8 } else { 0 };
                let c = |v: i32| v.clamp(0, 7) as u16;
                let reg = self.reg(pen) as usize;
                self.regs[reg] = c(cr) << 8 | c(cg) << 4 | c(cb);
            }
            b's' => match self.val(r, 0)? {
                0 => self.vt52_escape(b'E'),
                1 => self.vt52_escape(b'd'),
                2 => self.vt52_escape(b'J'),
                3 => self.clear_screen(),
                4 => {
                    self.clear_screen();
                    (self.col, self.row) = (0, 0);
                }
                _ => {
                    self.reverse = false;
                    self.bg = 0;
                    self.fg = 3 & self.mask();
                    self.vt52_escape(b'E');
                }
            },
            b'T' => {
                let which = self.val(r, 0)?;
                if !(1..=2).contains(&which) {
                    return Ok(());
                }
                let kind = self.val(r, 0)?;
                let size = self.val(r, 0)?;
                if which == 1 {
                    self.attrs.marker.1 = if (1..=6).contains(&kind) { kind as u8 } else { 1 };
                    self.attrs.marker.2 = if (1..=8).contains(&size) { size } else { 1 };
                } else {
                    let kind = if (1..=7).contains(&kind) { kind as u8 } else { 1 };
                    if (1..42).contains(&size) {
                        self.attrs.line.style = kind;
                        self.attrs.line.width = if kind > 1 { 1 } else { size };
                    }
                    let ends = match size {
                        0 => Some((0, 0)),
                        50 => Some((1, 1)),
                        51 => Some((1, 0)),
                        52 => Some((0, 1)),
                        53 => Some((1, 2)),
                        54 => Some((2, 1)),
                        60 => Some((2, 2)),
                        61 => Some((2, 0)),
                        62 => Some((0, 2)),
                        63 => Some((2, 1)),
                        64 => Some((1, 2)),
                        _ => None,
                    };
                    if let Some(e) = ends {
                        self.attrs.line.ends = e;
                    }
                }
            }
            b't' => {
                let n = self.val(r, 0)?;
                if n > 0 && !self.in_loop() {
                    self.hold = Some(Instant::now() + Duration::from_secs(n.min(30) as u64));
                }
            }
            b'U' => {
                let (a, b, filled) = (self.point(r)?, self.point(r)?, self.val(r, 0)?);
                self.rounded_box((a.0, a.1, b.0, b.1), filled != 0);
            }
            b'W' => {
                let p = self.point(r)?;
                let reads = self.looping.as_ref().is_none_or(|lp| lp.kind == b'@');
                if reads {
                    self.last_text = r.text()?;
                }
                let text = self.last_text.clone();
                self.write_text(p.0, p.1, &text);
            }
            b'X' => self.extended(r)?,
            b'Z' => {
                let (a, b) = (self.point(r)?, self.point(r)?);
                let (x0, x1, y0, y1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
                for y in y0..=y1 {
                    self.fill_span(x0, x1, y);
                }
            }
            b'b' => match self.val(r, 0)? {
                20 => {
                    for _ in 0..6 {
                        self.val(r, 0)?;
                    }
                }
                22 | 23 => {
                    self.val(r, 0)?;
                }
                _ => {}
            },
            b'<' => {
                let (cr, kind, show) = (self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?);
                if (2..=10).contains(&kind) {
                    self.awaiting_zone = Some((cr != 0, show < 2));
                }
            }
            b'?' => {
                let q = self.val(r, 0)?;
                match q {
                    0 => self.replies.extend_from_slice(VERSION),
                    1 => {
                        self.val(r, 0)?;
                        let (c, rw) = (self.col as u8 + 32, self.row as u8 + 32);
                        self.replies.extend_from_slice(&[c, rw, 32]);
                    }
                    2 => {
                        self.val(r, 0)?;
                        self.replies.extend_from_slice(b"0,0,0:");
                    }
                    3 => self.replies.extend_from_slice(&[b'0' + self.res.number(), b':']),
                    _ => {}
                }
            }
            // The modified VT-52 commands.
            b'c' => {
                let (which, reg) = (self.val(r, 0)?, self.val(r, 0)?);
                let reg = reg as u8 & self.mask();
                if which == 0 {
                    self.bg = reg;
                } else {
                    self.fg = reg;
                }
            }
            b'd' => {
                for _ in 0..self.val(r, 0)?.clamp(0, 50) {
                    self.vt52_escape(b'M');
                }
            }
            b'i' => {
                let (kind, n) = (self.val(r, 0)?, self.val(r, 0)?);
                for _ in 0..n.clamp(0, 50) {
                    if kind == 0 {
                        self.vt52_escape(b'I');
                    } else {
                        self.vt52_escape(b'L');
                    }
                }
            }
            b'l' => match self.val(r, 0)? {
                0 => {
                    self.vt52_escape(b'l');
                    self.col = 0;
                }
                1 => self.vt52_escape(b'o'),
                _ => self.vt52_escape(b'K'),
            },
            b'm' => {
                let (dir, n) = (self.val(r, 0)?, self.val(r, 0)?);
                let esc = match dir {
                    1 => b'A',
                    2 => b'B',
                    3 => b'C',
                    4 => b'D',
                    _ => b'H',
                };
                for _ in 0..n.clamp(1, 80) {
                    self.vt52_escape(esc);
                }
            }
            b'p' => {
                let (c, rw) = (self.val(r, 0)?, self.val(r, 0)?);
                let (cols, rows) = self.text_size();
                self.col = (c.max(0) as usize).min(cols - 1);
                self.row = (rw.max(0) as usize).min(rows - 1);
            }
            b'r' => {
                if self.val(r, 0)? == 0 {
                    self.saved = (self.col, self.row);
                } else {
                    (self.col, self.row) = self.saved;
                }
            }
            b'v' => self.reverse = self.val(r, 0)? != 0,
            b'w' => self.wrap = self.val(r, 0)? != 0,
            _ => log::debug!("unknown IGS command {:?}", key as char),
        }
        Ok(())
    }

    fn grab(&mut self, r: &mut Reader) -> Res2<()> {
        let (kind, op) = (self.val(r, 0)?, self.val(r, 0)?);
        let op = if (0..=15).contains(&op) { op as u8 } else { 3 };
        let a = self.point(r)?;
        match kind {
            0 => {
                let b = self.point(r)?;
                let d = self.point(r)?;
                self.blit(false, (a.0, a.1, b.0, b.1), false, d, op);
            }
            1 => {
                let b = self.point(r)?;
                self.memory_rect = (a.0, a.1, b.0, b.1);
                self.blit(false, self.memory_rect, true, (a.0.min(b.0), a.1.min(b.1)), op);
            }
            2 => self.blit(true, self.memory_rect, false, a, op),
            _ => {
                let b = self.point(r)?;
                let d = self.point(r)?;
                self.blit(true, (a.0, a.1, b.0, b.1), kind == 4, d, op);
            }
        }
        Ok(())
    }

    /// `N`: MIDI and sound chip data, read past.
    fn noise(&mut self, r: &mut Reader) -> Res2<()> {
        match self.val(r, 0)? {
            0 | 1 | 3 | 4 => {
                let n = self.val(r, 0)?.clamp(0, 10_001) as usize;
                for _ in 0..n {
                    r.next()?;
                }
            }
            6 => {
                self.val(r, 0)?;
                self.val(r, 0)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn extended(&mut self, r: &mut Reader) -> Res2<()> {
        match self.val(r, 0)? {
            // Spray paint: markers at random in a rectangle.
            0 => {
                let (a, rx, ry, n) = (self.point(r)?, self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?);
                for _ in 0..n.clamp(0, 9999) {
                    let x = a.0 + self.random((0, rx.clamp(0, 255)));
                    let y = a.1 + self.random((0, ry.clamp(0, 255)));
                    self.marker(x, y);
                }
            }
            // A color register, as the ST stores it (0x0RGB, in decimal).
            1 => {
                let (reg, v) = (self.val(r, 0)?, self.val(r, 0)?);
                self.regs[(reg & 15) as usize] = (v as u16) & 0x777;
            }
            2 => {
                let (a, b) = (self.val(r, 0)?, self.val(r, 0)?);
                if a == b {
                    let c = self.val(r, 0)?;
                    self.rand_big = (b, c);
                } else {
                    self.rand = (a, b);
                }
            }
            // The right mouse button's text: read past.
            3 => match self.val(r, 0)? {
                1 => {
                    self.val(r, 0)?;
                }
                2 => {
                    let (_on, _cr, len) = (self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?);
                    for _ in 0..len.clamp(0, 80) + 1 {
                        r.next()?;
                    }
                }
                _ => {}
            },
            // A mouse zone, and what it sends.
            4 => {
                let id = self.val(r, 0)?;
                if id >= 9997 {
                    if id == 9999 {
                        self.zones = vec![None; 48];
                    }
                    return Ok(());
                }
                let (a, b, len) = (self.point(r)?, self.point(r)?, self.val(r, 0)?);
                let mut text = Vec::new();
                for _ in 0..len.clamp(0, 80) {
                    text.push(r.next()?);
                }
                r.next()?;
                if let Some(z) = self.zones.get_mut(id.clamp(0, 47) as usize) {
                    *z = Some((a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1), text));
                }
            }
            5 => {
                if matches!(self.val(r, 0)?, 2 | 3) {
                    self.val(r, 0)?;
                    self.val(r, 0)?;
                }
            }
            6 => {
                self.val(r, 0)?;
            }
            // A user fill pattern: 16 rows of 16 characters, X for a bit.
            7 => {
                let slot = self.val(r, 0)?.clamp(0, 7) as usize;
                let mut rows = [0u16; 16];
                for row in &mut rows {
                    let text = r.text()?;
                    *row =
                        text.iter().take(16).enumerate().fold(0, |acc, (i, &c)| {
                            if c.eq_ignore_ascii_case(&b'X') { acc | 0x8000 >> i } else { acc }
                        });
                }
                self.user_patterns[slot] = rows;
            }
            // Rotating color registers: done at once (no animation yet).
            8 => {
                let (a, b, times, _delay) = (self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?, self.val(r, 0)?);
                let (lo, hi) = (a.min(b).clamp(0, 15) as usize, a.max(b).clamp(0, 15) as usize);
                if times > 0 && lo < hi {
                    let n = times as usize % (hi - lo + 1);
                    if a < b {
                        self.regs[lo..=hi].rotate_right(n);
                    } else {
                        self.regs[lo..=hi].rotate_left(n);
                    }
                }
            }
            // Store IG commands to run later.
            9 => match self.val(r, 0)? {
                0 => {
                    let mut stored = Vec::new();
                    while !stored.ends_with(b"||}") {
                        stored.push(r.next()?);
                    }
                    stored.truncate(stored.len() - 3);
                    self.stored = stored;
                }
                1 => self.run_stored = Some(self.stored.clone()),
                _ => self.stored.clear(),
            },
            10 => self.pen_at = self.point(r)?,
            _ => {}
        }
        Ok(())
    }

    /// A click at a point of the bitmap: what to send the BBS, if a zone
    /// was being waited for.
    pub fn click(&mut self, x: i32, y: i32) -> Option<Vec<u8>> {
        let (cr, send) = self.awaiting_zone?;
        let hit = self.zones[..47]
            .iter()
            .flatten()
            .find(|z| x >= z.0 && x <= z.2 && y >= z.1 && y <= z.3)
            .or(self.zones[47].as_ref())?;
        let mut out = if send { hit.4.clone() } else { Vec::new() };
        self.awaiting_zone = None;
        if cr && send {
            out.push(b'\r');
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn igs(data: &[u8]) -> Igs {
        let mut g = Igs::default();
        g.feed(data);
        g
    }

    fn at(g: &Igs, x: usize, y: usize) -> u8 {
        g.pixels[y * g.width() + x]
    }

    #[test]
    fn numbers_as_ig_reads_them() {
        let mut r = Reader { data: b"12,7:_\r\n34,1\r\n5\r\n2,x,+10,-3,!99,r,12345,", pos: 0 };
        let toks: Vec<Tok> = (0..9).map(|_| r.token().unwrap()).collect();
        assert_eq!(
            toks,
            [
                Tok::Num(12),
                Tok::Num(7),
                Tok::Num(34),
                Tok::Num(152),
                Tok::X,
                Tok::Plus(10),
                Tok::Minus(3),
                Tok::Bang(99),
                Tok::Rand,
            ]
        );
        assert_eq!(r.token().unwrap(), Tok::Num(0), "five digits are too many");
        assert!(Reader { data: b"12", pos: 0 }.token().is_err());
    }

    #[test]
    fn lines_boxes_and_chains() {
        // A solid black box (pen 1 is register 3 in medium resolution),
        // then a line in red (pen 2, register 1), chained; then text.
        let g = igs(b"G#A>1,1,0:B>10,10,20,20,0:C>1,2:L 0,50,100,50:ok");
        assert_eq!(at(&g, 15, 15), 3);
        assert_eq!(at(&g, 25, 15), 0);
        assert_eq!(at(&g, 50, 50), 1);
        // The text after the chain is text: "ok" at the cursor, in black.
        assert!((0..8).any(|y| (0..16).any(|x| at(&g, x, y) == 3)));
    }

    #[test]
    fn commands_split_across_reads() {
        let mut g = Igs::default();
        g.feed(b"G#C>2,2:B>0,0,");
        assert_eq!(at(&g, 5, 5), 0);
        g.feed(b"10,10,0:");
        assert_eq!(at(&g, 5, 5), 1);
    }

    #[test]
    fn loops_step_and_read_data() {
        // Vertical lines at x = 0, 10, 20, 30 from a loop.
        let g = igs(b"G#C>1,1:&>0,30,10,0,L,4,x,0,x,9:");
        for x in [0, 10, 20, 30] {
            assert_eq!(at(&g, x, 5), 3, "x={x}");
        }
        assert_eq!(at(&g, 5, 5), 0);
        assert_eq!(at(&g, 40, 5), 0);
        // READ DATA: the same command with constant parameters, twice.
        let g = igs(b"G#C>1,1:&>0,0,1,0,L,8,100,0,100,9,200,0,200,9:");
        assert_eq!((at(&g, 100, 5), at(&g, 200, 5)), (3, 3));
    }

    #[test]
    fn flood_fill_and_patterns() {
        // An outlined box, then flood its inside with pattern 4 (50%).
        let g = igs(b"G#A>0,1,1:B>0,0,40,20,0:A>2,4,0:F>10,10:");
        assert_eq!(at(&g, 0, 0), 3, "the border");
        let inside: Vec<u8> = (8..12).map(|x| at(&g, x, 10)).collect();
        assert!(inside.contains(&3) && inside.contains(&0), "{inside:?}");
        assert_eq!(at(&g, 50, 10), 0, "outside");
    }

    #[test]
    fn vt52_text_and_queries() {
        let mut g = igs(b"\x1bY\x22\x25X\x1bb\x01Y");
        // "X" at column 5, row 2, black; "Y" after it in red.
        assert!((16..24).any(|y| (40..48).any(|x| at(&g, x, y) == 3)));
        assert!((16..24).any(|y| (48..56).any(|x| at(&g, x, y) == 1)));
        g.feed(b"G#?>0:?>3:");
        assert_eq!(g.replies, b"2.191:");
        // Low resolution: sixteen colors.
        g.feed(b"G#R>0,0:C>2,15:Z>0,0,9,9:");
        assert_eq!((g.width(), at(&g, 5, 5)), (320, 13));
    }

    #[test]
    fn mouse_zones() {
        let mut g = igs(b"G#X>4,1,0,0,99,99,3,f/L:X>4,47,0,0,0,0,1,Q:<>1,2,1:");
        assert_eq!(g.click(50, 50), Some(b"f/L\r".to_vec()));
        assert_eq!(g.click(50, 50), None, "only while waiting");
        g.feed(b"G#< 0,2,1:");
        assert_eq!(g.click(300, 150), Some(b"Q".to_vec()));
    }

    #[test]
    fn pauses_hold_the_stream() {
        let mut g = igs(b"G#t>1:C>2,2:Z>0,0,9,9:");
        assert!(g.hold.is_some());
        assert_eq!(at(&g, 5, 5), 0);
        g.poll(Instant::now() + Duration::from_secs(2));
        assert_eq!(at(&g, 5, 5), 1);
    }
}
