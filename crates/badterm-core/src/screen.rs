//! The character-cell screen model: a grid of CP437 glyphs with attributes,
//! a cursor, a scrolling region and a scrollback buffer.

use std::collections::VecDeque;

use crate::palette::{self, Rgb, VGA};

/// A cell color. PC colors keep ANSI-BBS semantics (bold brightens the
/// foreground, blink brightens the background in iCE mode) by being resolved
/// at render time rather than when written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    /// VGA palette index 0-15.
    Pc(u8),
    /// xterm 256-color index.
    Xterm(u8),
    /// 24-bit color.
    Rgb(Rgb),
}

pub mod flags {
    pub const BOLD: u8 = 1 << 0;
    pub const BLINK: u8 = 1 << 1;
    pub const UNDERLINE: u8 = 1 << 2;
    pub const REVERSE: u8 = 1 << 3;
    pub const CONCEAL: u8 = 1 << 4;
    pub const FAINT: u8 = 1 << 5;
    /// Teletext double height: the top half of the glyph, stretched.
    pub const DOUBLE_TOP: u8 = 1 << 6;
    /// And the bottom half, on the row below.
    pub const DOUBLE_BOTTOM: u8 = 1 << 7;
}

pub const DEFAULT_FG: Color = Color::Pc(7);
pub const DEFAULT_BG: Color = Color::Pc(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// CP437 glyph index.
    pub ch: u8,
    pub fg: Color,
    pub bg: Color,
    pub flags: u8,
    /// Pixel graphics over the cell (sixel and the like): an ID in the
    /// screen's [`Tiles`], or 0 for none. Writing to the cell replaces it.
    pub tile: u32,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: b' ', fg: DEFAULT_FG, bg: DEFAULT_BG, flags: 0, tile: 0 }
    }
}

impl Cell {
    pub fn new(ch: u8, fg: Color, bg: Color) -> Self {
        Cell { ch, fg, bg, flags: 0, tile: 0 }
    }

    pub fn is_blank(&self) -> bool {
        (self.ch == b' ' || self.ch == 0) && self.bg == DEFAULT_BG && self.flags & flags::REVERSE == 0 && self.tile == 0
    }

    /// Resolve to concrete (foreground, background, blinks) colors.
    ///
    /// With `ice` set, the blink attribute selects a high-intensity background
    /// instead of blinking text, as on a VGA with blink disabled.
    pub fn resolve(&self, ice: bool) -> (Rgb, Rgb, bool) {
        self.resolve_with(ice, Look::standard())
    }

    /// [`Cell::resolve`] with the palette and modes the BBS has set.
    pub fn resolve_with(&self, ice: bool, look: &Look) -> (Rgb, Rgb, bool) {
        let bold = self.flags & flags::BOLD != 0 && !look.bright_plain;
        let blink = self.flags & flags::BLINK != 0;
        let mut fg = match self.fg {
            Color::Pc(i) if bold && i < 8 => look.rgb(Color::Pc(i + 8)),
            c => look.rgb(c),
        };
        let mut bg = match self.bg {
            Color::Pc(i) if blink && ice && i < 8 => look.rgb(Color::Pc(i + 8)),
            c => look.rgb(c),
        };
        if self.flags & flags::REVERSE != 0 {
            std::mem::swap(&mut fg, &mut bg);
        }
        if self.flags & flags::CONCEAL != 0 {
            fg = bg;
        }
        (fg, bg, blink && !ice && !look.blink_steady)
    }
}

/// What a BBS can change about how cells are drawn, beyond their own
/// attributes: CTerm's palette (`OSC 4`) and font slots (`CSI n;m SP D`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    /// In xterm order: the 16 ANSI colors (black, red, green, ...), the
    /// 6x6x6 cube and the grays.
    pub palette: [Rgb; 256],
    /// CTerm font IDs for the normal, bright, blink and bright-blink slots.
    pub fonts: [u8; 4],
    /// `CSI ?31h`: bright text is drawn in font slot 1.
    pub bright_font: bool,
    /// `CSI ?32h`: bold does not brighten the color.
    pub bright_plain: bool,
    /// `CSI ?34h`: blinking text is drawn in font slot 2.
    pub blink_font: bool,
    /// `CSI ?35h`: blink does not make text blink.
    pub blink_steady: bool,
    /// Fonts the BBS has loaded (CTerm's loadable fonts), by slot.
    pub loaded: Vec<LoadedFont>,
}

/// A font a BBS sent: 256 glyphs, 8 pixels wide and `height` high.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadedFont {
    pub slot: u8,
    pub height: u8,
    pub data: &'static [u8],
}

/// CTerm's built-in fonts are 0-42; loaded ones go in the slots after.
pub const FIRST_LOADABLE_FONT: u8 = 43;

/// The slot a directory entry's own font file goes in.
pub const ENTRY_FONT: u8 = 255;

impl Default for Look {
    fn default() -> Look {
        Look::standard().clone()
    }
}

impl Look {
    /// The VGA palette and font, with no CTerm modes set.
    pub fn standard() -> &'static Look {
        static STANDARD: std::sync::LazyLock<Look> = std::sync::LazyLock::new(|| Look {
            palette: std::array::from_fn(|i| palette::xterm256(i as u8)),
            fonts: [0; 4],
            bright_font: false,
            bright_plain: false,
            blink_font: false,
            blink_steady: false,
            loaded: Vec::new(),
        });
        &STANDARD
    }

    /// Load a font into a slot: 4096 bytes is 8x16, 3584 8x14, 2048 8x8.
    /// False if the slot is a built-in one or the size is none of those.
    pub fn load_font(&mut self, slot: u8, data: &[u8]) -> bool {
        let height = match data.len() {
            4096 => 16,
            3584 => 14,
            2048 => 8,
            _ => return false,
        };
        if slot < FIRST_LOADABLE_FONT {
            return false;
        }
        let font = LoadedFont { slot, height, data: intern(data) };
        self.loaded.retain(|f| f.slot != slot);
        self.loaded.push(font);
        true
    }

    /// A loaded font, by its slot (the ID `CSI n ; slot SP D` selects).
    pub fn loaded_font(&self, slot: u8) -> Option<LoadedFont> {
        self.loaded.iter().find(|f| f.slot == slot).copied()
    }

    pub fn rgb(&self, c: Color) -> Rgb {
        match c {
            // VGA order to xterm order.
            Color::Pc(i) => {
                let i = i & 15;
                let ansi = palette::ANSI_TO_VGA.iter().position(|&v| v == i & 7).unwrap_or(0) as u8;
                self.palette[(ansi + (i & 8)) as usize]
            }
            Color::Xterm(i) => self.palette[i as usize],
            Color::Rgb(rgb) => rgb,
        }
    }

    /// The CTerm font a cell is drawn in.
    pub fn font_for(&self, cell: &Cell) -> u8 {
        let bright = self.bright_font && cell.flags & flags::BOLD != 0;
        let blink = self.blink_font && cell.flags & flags::BLINK != 0;
        self.fonts[bright as usize | (blink as usize) << 1]
    }
}

/// Font data kept for the rest of the run, once for each different font
/// (BBSes send the same few again and again).
fn intern(data: &[u8]) -> &'static [u8] {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static FONTS: Mutex<Option<HashSet<&'static [u8]>>> = Mutex::new(None);
    let mut fonts = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    let set = fonts.get_or_insert_with(HashSet::new);
    if let Some(&have) = set.get(data) {
        return have;
    }
    let kept: &'static [u8] = Box::leak(data.to_vec().into_boxed_slice());
    set.insert(kept);
    kept
}

pub fn color_rgb(c: Color) -> Rgb {
    match c {
        Color::Pc(i) => VGA[(i & 15) as usize],
        Color::Xterm(i) => palette::xterm256(i),
        Color::Rgb(rgb) => rgb,
    }
}

#[derive(Clone, Copy, Debug)]
struct SavedCursor {
    x: usize,
    y: usize,
    pen: Cell,
}

pub struct Screen {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    x: usize,
    y: usize,
    /// Set when a glyph was written to the last column; the wrap happens on the
    /// next printable character (VT100 "last column flag").
    pending_wrap: bool,
    /// Attributes applied to newly written glyphs (`ch` is unused).
    pub pen: Cell,
    saved: Option<SavedCursor>,
    scroll_top: usize,
    scroll_bottom: usize,
    pub autowrap: bool,
    pub cursor_visible: bool,
    pub ice_colors: bool,
    /// The palette and fonts the BBS has chosen.
    pub look: Look,
    /// The font (CTerm ID) a reset returns to: the entry's choice.
    pub base_font: u8,
    /// Whether a reset leaves iCE colors on: the entry's choice.
    pub base_ice: bool,
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_limit: usize,
    /// The pixel graphics cells show (see [`Cell::tile`]).
    pub tiles: Tiles,
}

impl Screen {
    pub fn new(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Screen {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            x: 0,
            y: 0,
            pending_wrap: false,
            pen: Cell::default(),
            saved: None,
            scroll_top: 0,
            scroll_bottom: rows - 1,
            autowrap: true,
            cursor_visible: true,
            ice_colors: false,
            look: Look::default(),
            base_font: 0,
            base_ice: false,
            scrollback: VecDeque::new(),
            scrollback_limit,
            tiles: Tiles::default(),
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    pub fn row(&self, y: usize) -> &[Cell] {
        &self.cells[y * self.cols..(y + 1) * self.cols]
    }

    pub fn cell_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        if x < self.cols && y < self.rows { Some(&mut self.cells[y * self.cols + x]) } else { None }
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Line `i` of the scrollback, 0 being the oldest.
    pub fn scrollback_line(&self, i: usize) -> &[Cell] {
        &self.scrollback[i]
    }

    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
    }

    /// Full reset (RIS): clears the screen and modes but keeps scrollback.
    pub fn reset(&mut self) {
        self.pen = Cell::default();
        self.cells.fill(Cell::default());
        self.x = 0;
        self.y = 0;
        self.pending_wrap = false;
        self.saved = None;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.autowrap = true;
        self.cursor_visible = true;
        self.ice_colors = self.base_ice;
        // A reset keeps the fonts the BBS loaded, as CTerm does.
        let loaded = std::mem::take(&mut self.look.loaded);
        self.look = Look { loaded, ..Look::default() };
        self.look.fonts[0] = self.base_font;
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        let mut cells = vec![Cell::default(); cols * rows];
        for y in 0..rows.min(self.rows) {
            let n = cols.min(self.cols);
            cells[y * cols..y * cols + n].copy_from_slice(&self.row(y)[..n]);
        }
        self.cells = cells;
        self.cols = cols;
        self.rows = rows;
        self.x = self.x.min(cols - 1);
        self.y = self.y.min(rows - 1);
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.pending_wrap = false;
    }

    /// The cell used when erasing: a space in the current colors (ANSI.SYS
    /// and ANSI-BBS erase with the current background).
    fn blank(&self) -> Cell {
        Cell { ch: b' ', flags: self.pen.flags & (flags::BLINK | flags::REVERSE), ..self.pen }
    }

    /// Write a glyph at the cursor and advance.
    pub fn put(&mut self, ch: u8) {
        if self.pending_wrap {
            self.pending_wrap = false;
            self.x = 0;
            self.line_feed();
        }
        let idx = self.y * self.cols + self.x;
        self.cells[idx] = Cell { ch, ..self.pen };
        if self.x + 1 < self.cols {
            self.x += 1;
        } else if self.autowrap {
            self.pending_wrap = true;
        }
    }

    pub fn carriage_return(&mut self) {
        self.x = 0;
        self.pending_wrap = false;
    }

    /// Move down one line, scrolling the region if at its bottom.
    pub fn line_feed(&mut self) {
        self.pending_wrap = false;
        if self.y == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.y + 1 < self.rows {
            self.y += 1;
        }
    }

    /// Move up one line, scrolling the region down if at its top (ESC M).
    pub fn reverse_index(&mut self) {
        self.pending_wrap = false;
        if self.y == self.scroll_top {
            self.scroll_down(1);
        } else if self.y > 0 {
            self.y -= 1;
        }
    }

    pub fn backspace(&mut self) {
        self.pending_wrap = false;
        self.x = self.x.saturating_sub(1);
    }

    pub fn tab(&mut self) {
        self.pending_wrap = false;
        self.x = ((self.x / 8 + 1) * 8).min(self.cols - 1);
    }

    pub fn back_tab(&mut self, n: usize) {
        for _ in 0..n {
            self.x = self.x.saturating_sub(1) / 8 * 8;
        }
        self.pending_wrap = false;
    }

    /// Absolute move, 0-based, clamped to the screen.
    pub fn move_to(&mut self, x: usize, y: usize) {
        self.x = x.min(self.cols - 1);
        self.y = y.min(self.rows - 1);
        self.pending_wrap = false;
    }

    pub fn move_by(&mut self, dx: isize, dy: isize) {
        let x = (self.x as isize + dx).max(0) as usize;
        let y = (self.y as isize + dy).max(0) as usize;
        self.move_to(x, y);
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let bottom = bottom.min(self.rows - 1);
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows - 1;
        }
        self.move_to(0, 0);
    }

    pub fn save_cursor(&mut self) {
        self.saved = Some(SavedCursor { x: self.x, y: self.y, pen: self.pen });
    }

    pub fn restore_cursor(&mut self) {
        if let Some(s) = self.saved {
            self.move_to(s.x, s.y);
            self.pen = s.pen;
        }
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        if self.scrollback_limit == 0 {
            return;
        }
        if self.scrollback.len() >= self.scrollback_limit {
            self.scrollback.pop_front();
        }
        self.scrollback.push_back(line);
    }

    /// Scroll the region up `n` lines. Lines leaving the top of a full-screen
    /// region go to scrollback.
    pub fn scroll_up(&mut self, n: usize) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = n.min(bottom - top + 1);
        if top == 0 && bottom == self.rows - 1 {
            for y in 0..n {
                let line = self.row(y).to_vec();
                self.push_scrollback(line);
            }
        }
        let c = self.cols;
        self.cells.copy_within((top + n) * c..(bottom + 1) * c, top * c);
        let blank = self.blank();
        self.cells[(bottom + 1 - n) * c..(bottom + 1) * c].fill(blank);
    }

    pub fn scroll_down(&mut self, n: usize) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        let n = n.min(bottom - top + 1);
        let c = self.cols;
        self.cells.copy_within(top * c..(bottom + 1 - n) * c, (top + n) * c);
        let blank = self.blank();
        self.cells[top * c..(top + n) * c].fill(blank);
    }

    /// ED: 0 = cursor to end, 1 = start to cursor, 2/3 = whole screen.
    ///
    /// Clearing the whole screen saves its content to scrollback first, so
    /// BBS screens that are wiped between menus can still be reviewed, and,
    /// like ANSI.SYS, homes the cursor.
    pub fn erase_display(&mut self, mode: u32) {
        let blank = self.blank();
        let pos = self.y * self.cols + self.x;
        match mode {
            0 => self.cells[pos..].fill(blank),
            1 => self.cells[..=pos].fill(blank),
            _ => {
                let used = (0..self.rows).rev().find(|&y| self.row(y).iter().any(|c| !c.is_blank()));
                if let Some(last) = used {
                    for y in 0..=last {
                        let line = self.row(y).to_vec();
                        self.push_scrollback(line);
                    }
                }
                self.cells.fill(blank);
                self.move_to(0, 0);
            }
        }
        self.pending_wrap = false;
    }

    /// EL: 0 = cursor to end of line, 1 = start to cursor, 2 = whole line.
    pub fn erase_line(&mut self, mode: u32) {
        let blank = self.blank();
        let start = self.y * self.cols;
        let (a, b) = match mode {
            0 => (self.x, self.cols),
            1 => (0, self.x + 1),
            _ => (0, self.cols),
        };
        self.cells[start + a..start + b].fill(blank);
        self.pending_wrap = false;
    }

    /// ECH: blank `n` cells from the cursor without moving it.
    pub fn erase_chars(&mut self, n: usize) {
        let blank = self.blank();
        let start = self.y * self.cols;
        let end = (self.x + n).min(self.cols);
        self.cells[start + self.x..start + end].fill(blank);
    }

    /// ICH: insert `n` blanks at the cursor, shifting the rest of the line right.
    pub fn insert_chars(&mut self, n: usize) {
        let blank = self.blank();
        let start = self.y * self.cols;
        let n = n.min(self.cols - self.x);
        let line = &mut self.cells[start..start + self.cols];
        line.copy_within(self.x..self.cols - n, self.x + n);
        line[self.x..self.x + n].fill(blank);
    }

    /// DCH: delete `n` cells at the cursor, shifting the rest of the line left.
    pub fn delete_chars(&mut self, n: usize) {
        let blank = self.blank();
        let start = self.y * self.cols;
        let n = n.min(self.cols - self.x);
        let line = &mut self.cells[start..start + self.cols];
        line.copy_within(self.x + n.., self.x);
        let cols = self.cols;
        line[cols - n..].fill(blank);
    }

    /// IL: insert `n` lines at the cursor row (within the scroll region).
    pub fn insert_lines(&mut self, n: usize) {
        if self.y < self.scroll_top || self.y > self.scroll_bottom {
            return;
        }
        let top = self.scroll_top;
        self.scroll_top = self.y;
        self.scroll_down(n);
        self.scroll_top = top;
        self.x = 0;
    }

    /// DL: delete `n` lines at the cursor row (within the scroll region).
    pub fn delete_lines(&mut self, n: usize) {
        if self.y < self.scroll_top || self.y > self.scroll_bottom {
            return;
        }
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        // Temporarily narrow the region so scroll_up never feeds scrollback.
        self.scroll_top = self.y;
        let n = n.min(bottom - self.y + 1);
        let c = self.cols;
        self.cells.copy_within((self.y + n) * c..(bottom + 1) * c, self.y * c);
        let blank = self.blank();
        self.cells[(bottom + 1 - n) * c..(bottom + 1) * c].fill(blank);
        self.scroll_top = top;
        self.x = 0;
    }

    /// Write a string of CP437 glyphs directly at a position without moving
    /// the cursor; used by the built-in text-mode UI.
    pub fn draw_text(&mut self, x: usize, y: usize, text: &[u8], fg: Color, bg: Color) {
        for (i, &ch) in text.iter().enumerate() {
            if let Some(c) = self.cell_mut(x + i, y) {
                *c = Cell::new(ch, fg, bg);
            }
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, cell: Cell) {
        for yy in y..(y + h).min(self.rows) {
            for xx in x..(x + w).min(self.cols) {
                self.cells[yy * self.cols + xx] = cell;
            }
        }
    }

    /// The size of a cell in pixels, for pixel graphics: an 8x16 VGA font,
    /// or 8x8 on screens of 43 rows or more (as BadTerm draws them).
    pub fn cell_px(&self) -> (usize, usize) {
        (8, if self.rows >= 43 { 8 } else { 16 })
    }

    /// The screen's size in pixels, for pixel graphics.
    pub fn size_px(&self) -> (usize, usize) {
        let (w, h) = self.cell_px();
        (self.cols * w, self.rows * h)
    }

    /// Put a picture on the screen with its top left at the cursor's cell,
    /// as sixel does: with `scrolling`, the screen scrolls to make room
    /// and the cursor ends on the row below the picture's last full band,
    /// at the column it started in; without, it goes at the top left of
    /// the screen, what doesn't fit is cut off and the cursor stays put.
    pub fn place_image(&mut self, img: &crate::sixel::Image, scrolling: bool) {
        if img.width == 0 || img.height == 0 {
            return;
        }
        let (tw, th) = self.cell_px();
        let cx = self.x;
        let rows = img.height.div_ceil(th);
        let cols = img.width.div_ceil(tw);
        if !scrolling {
            for r in 0..rows.min(self.rows) {
                self.tile_row(img, r, 0, r, cols);
            }
            return;
        }
        self.pending_wrap = false;
        let last = img.height / th;
        for r in 0..=last {
            if r > 0 {
                self.line_feed();
            }
            if r < rows {
                let y = self.y;
                self.tile_row(img, r, cx, y, cols);
            }
        }
        self.x = cx;
    }

    /// Row `r` of a picture's tiles onto screen row `y`, from column `x0`.
    fn tile_row(&mut self, img: &crate::sixel::Image, r: usize, x0: usize, y: usize, cols: usize) {
        let (tw, th) = self.cell_px();
        let (left, top) = ((x0 * tw) as isize, (y * th) as isize);
        let rect = (left, top, cols * tw, th);
        self.draw_pixels(rect, |px, py| {
            let (ix, iy) = ((px - left) as usize, (py - top) as usize + r * th);
            if ix < img.width && iy < img.height { img.pixels[iy * img.width + ix] } else { 0 }
        });
    }

    /// Draw pixels over the screen in `rect` (x, y, w, h, in screen pixels;
    /// it may go off the screen, and that part is clipped): `pixel(x, y)`
    /// is each one's color, 0xFFRRGGBB, or 0 to leave it as it is.
    pub fn draw_pixels(&mut self, rect: (isize, isize, usize, usize), pixel: impl Fn(isize, isize) -> u32) {
        let (tw, th) = self.cell_px();
        let (sw, sh) = self.size_px();
        let (x0, y0) = (rect.0.max(0) as usize, rect.1.max(0) as usize);
        let x1 = ((rect.0 + rect.2 as isize).max(0) as usize).min(sw);
        let y1 = ((rect.1 + rect.3 as isize).max(0) as usize).min(sh);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for cy in y0 / th..y1.div_ceil(th) {
            for cx in x0 / tw..x1.div_ceil(tw) {
                let idx = cy * self.cols + cx;
                let mut px = match self.tiles.get(self.cells[idx].tile) {
                    Some(t) if (t.width, t.height) == (tw, th) => t.pixels.to_vec(),
                    _ => vec![0; tw * th],
                };
                let mut any = false;
                for ty in 0..th {
                    let y = cy * th + ty;
                    if y < y0 || y >= y1 {
                        continue;
                    }
                    for tx in 0..tw {
                        let x = cx * tw + tx;
                        if x < x0 || x >= x1 {
                            continue;
                        }
                        let p = pixel(x as isize, y as isize);
                        if p != 0 {
                            px[ty * tw + tx] = p;
                            any = true;
                        }
                    }
                }
                if !any {
                    continue;
                }
                if self.tiles.full() {
                    let mut used = std::collections::HashSet::new();
                    used.extend(self.cells.iter().map(|c| c.tile));
                    used.extend(self.scrollback.iter().flatten().map(|c| c.tile));
                    self.tiles.collect(&used);
                }
                self.cells[idx].tile = self.tiles.add(Tile { width: tw, height: th, pixels: px.into() });
            }
        }
    }

    /// The color of a screen pixel, as pixel operations see it: the
    /// cell's pixel graphics, or else its background (text isn't drawn
    /// here, so it reads as background).
    pub fn pixel_at(&self, x: usize, y: usize) -> crate::palette::Rgb {
        let (tw, th) = self.cell_px();
        let Some(cell) = self.cells.get((y / th) * self.cols + x / tw).filter(|_| x / tw < self.cols) else {
            return 0;
        };
        if let Some(t) = self.tiles.get(cell.tile) {
            let p = t.pixels.get((y % th) * t.width + x % tw).copied().unwrap_or(0);
            if p != 0 {
                return p & 0x00FF_FFFF;
            }
        }
        cell.resolve_with(self.ice_colors, &self.look).1
    }

    /// Plain-text contents of a row, trailing blanks trimmed.
    pub fn row_text(row: &[Cell]) -> String {
        let s: String = row.iter().map(|c| crate::cp437::to_unicode(c.ch.max(b' '))).collect();
        s.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &Screen, y: usize) -> String {
        Screen::row_text(s.row(y))
    }

    #[test]
    fn delayed_wrap_at_last_column() {
        let mut s = Screen::new(4, 3, 10);
        for &b in b"abcd" {
            s.put(b);
        }
        assert_eq!(s.cursor(), (3, 0));
        s.carriage_return();
        s.line_feed();
        s.put(b'e');
        assert_eq!(text(&s, 0), "abcd");
        assert_eq!(text(&s, 1), "e");
    }

    #[test]
    fn scrolling_feeds_scrollback() {
        let mut s = Screen::new(4, 2, 10);
        for &b in b"ab" {
            s.put(b);
            s.carriage_return();
            s.line_feed();
        }
        assert_eq!(s.scrollback_len(), 1);
        assert_eq!(Screen::row_text(s.scrollback_line(0)), "a");
        assert_eq!(text(&s, 0), "b");
    }

    #[test]
    fn clear_screen_saves_to_scrollback() {
        let mut s = Screen::new(4, 3, 10);
        s.put(b'x');
        s.erase_display(2);
        assert_eq!(s.scrollback_len(), 1);
        assert_eq!(s.cursor(), (0, 0));
    }

    #[test]
    fn insert_delete_chars() {
        let mut s = Screen::new(5, 1, 0);
        for &b in b"abcde" {
            s.put(b);
        }
        s.move_to(1, 0);
        s.delete_chars(2);
        assert_eq!(text(&s, 0), "ade");
        s.insert_chars(1);
        assert_eq!(text(&s, 0), "a de");
    }

    #[test]
    fn ice_colors_resolve() {
        let c = Cell { ch: b'A', fg: Color::Pc(1), bg: Color::Pc(4), flags: flags::BOLD | flags::BLINK, tile: 0 };
        assert_eq!(c.resolve(false), (VGA[9], VGA[4], true));
        assert_eq!(c.resolve(true), (VGA[9], VGA[12], false));
    }
}

/// One cell's pixel graphics: 0xFFRRGGBB pixels, 0 where the cell's own
/// text and background show through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub width: usize,
    pub height: usize,
    pub pixels: Box<[u32]>,
}

/// The tiles the screen's cells (and scrollback) show, by ID. Ones no cell
/// shows any more are reused once there are many.
#[derive(Debug, Default)]
pub struct Tiles {
    store: Vec<Option<Tile>>,
    free: Vec<u32>,
    /// Look for unused tiles once there are this many.
    next_collect: usize,
}

impl Tiles {
    pub fn get(&self, id: u32) -> Option<&Tile> {
        id.checked_sub(1).and_then(|i| self.store.get(i as usize)).and_then(|t| t.as_ref())
    }

    fn full(&self) -> bool {
        self.free.is_empty() && self.store.len() >= self.next_collect.max(4096)
    }

    /// Free every tile not in `used`.
    fn collect(&mut self, used: &std::collections::HashSet<u32>) {
        self.free.clear();
        for (i, t) in self.store.iter_mut().enumerate() {
            let id = i as u32 + 1;
            if !used.contains(&id) {
                *t = None;
                self.free.push(id);
            }
        }
        self.next_collect = (self.store.len() - self.free.len()) * 2;
    }

    fn add(&mut self, tile: Tile) -> u32 {
        match self.free.pop() {
            Some(id) => {
                self.store[id as usize - 1] = Some(tile);
                id
            }
            None => {
                self.store.push(Some(tile));
                self.store.len() as u32
            }
        }
    }
}
