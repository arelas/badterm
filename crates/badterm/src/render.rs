//! Software rasterizer: draws character cells with a bitmap font into a
//! native-resolution frame, then scales it into the window buffer, with an
//! optional CRT look (see [`crate::crt`]).

use badterm_core::screen::{Cell, Color, Look, Tiles, flags};

use crate::crt::{Crt, CrtMode, Curvature, Glow, Scanlines};
use crate::selection::Selection;

/// An 8-pixel-wide bitmap font with 256 glyphs, indexed by byte (CP437 for
/// the VGA fonts, ISO-8859-1 for the Amiga ones).
#[derive(Clone, Copy)]
pub struct Font {
    pub height: usize,
    data: &'static [u8],
    /// The CTerm font ID.
    pub id: u8,
}

/// The fonts BadTerm has besides CP437, by CTerm font ID (as SyncTERM
/// numbers them), with their names. All are 8x16.
pub const EXTRA_FONTS: [(u8, &str); 6] = [
    (42, "Topaz"),
    (40, "Topaz Plus"),
    (41, "MicroKnight"),
    (39, "MicroKnight Plus"),
    (37, "P0T-NOoDLE"),
    (38, "mO'sOul"),
];

impl Font {
    pub fn vga_8x16() -> Font {
        Font { height: 16, data: include_bytes!("../assets/fonts/vga_8x16.bin"), id: 0 }
    }

    /// For 50-line modes.
    pub fn vga_8x8() -> Font {
        Font { height: 8, data: include_bytes!("../assets/fonts/vga_8x8.bin"), id: 0 }
    }

    /// The font with this CTerm ID, at the height of `base` (the screen
    /// mode's font). IDs BadTerm lacks, and every font in 8-pixel modes
    /// (the Amiga fonts are 8x16 only), give `base`.
    pub fn cterm(id: u8, base: Font) -> Font {
        // The C64 and Atari fonts are 8x8 whatever the mode: their screens
        // are laid out for them (see `Font::eight_only`).
        let eight: Option<&'static [u8]> = match id {
            32 => Some(include_bytes!("../assets/fonts/c64_upper.bin")),
            33 => Some(include_bytes!("../assets/fonts/c64_lower.bin")),
            36 => Some(include_bytes!("../assets/fonts/atari.bin")),
            _ => None,
        };
        if let Some(data) = eight {
            return Font { height: 8, data, id };
        }
        let data: &'static [u8] = match (id, base.height) {
            (42, 16) => include_bytes!("../assets/fonts/amiga/topaz_a1200.bin"),
            (40, 16) => include_bytes!("../assets/fonts/amiga/topazplus_a1200.bin"),
            (41, 16) => include_bytes!("../assets/fonts/amiga/microknight.bin"),
            (39, 16) => include_bytes!("../assets/fonts/amiga/microknightplus.bin"),
            (37, 16) => include_bytes!("../assets/fonts/amiga/p0t-noodle.bin"),
            (38, 16) => include_bytes!("../assets/fonts/amiga/mosoul.bin"),
            _ => return base,
        };
        Font { height: 16, data, id }
    }

    /// Fonts that exist only at 8x8 (the C64's and Atari's): a screen in
    /// one is laid out in 8x8 cells.
    pub fn eight_only(id: u8) -> bool {
        matches!(id, 32 | 33 | 36)
    }

    fn row(&self, ch: u8, y: usize) -> u8 {
        self.data[ch as usize * self.height + y]
    }
}

/// How the native frame is fitted to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
    /// The largest whole-number scale that fits, so every pixel is exact.
    Integer,
    /// As large as fits (keeping the aspect ratio), for fullscreen. Pixels
    /// stay sharp: only the seams between them, where a screen pixel
    /// straddles two frame pixels, are blended.
    Fill,
    /// The whole area, whatever its shape.
    Stretch,
}

/// How a frame is presented.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub scaling: Scaling,
    /// Height of a frame pixel relative to its width: 1.0 for square
    /// pixels, about 1.2 to show 640x400 at the 4:3 shape of a CRT.
    pub pixel_aspect: f64,
    pub crt: CrtMode,
    pub glow: Glow,
    pub scanlines: Scanlines,
    pub curvature: Curvature,
}

impl Default for Display {
    fn default() -> Display {
        Display {
            scaling: Scaling::Integer,
            pixel_aspect: 1.0,
            crt: CrtMode::Off,
            glow: Glow::Light,
            scanlines: Scanlines::Medium,
            curvature: Curvature::Off,
        }
    }
}

/// One frame to draw: a list of rows of cells plus cursor state.
pub struct Frame<'a> {
    pub rows: Vec<&'a [Cell]>,
    /// Cursor cell, if it should be shown.
    pub cursor: Option<(usize, usize)>,
    pub ice: bool,
    /// Text selected with the mouse, drawn in inverse.
    pub selection: Option<Selection>,
    /// The palette and fonts for the first `look_rows` rows (the BBS's
    /// screen); the rest (the status bar) are drawn in the standard look.
    pub look: &'a Look,
    pub look_rows: usize,
    /// The look of the other rows: BadTerm's own screens and status bar,
    /// in the color theme's palette.
    pub ui_look: &'a Look,
    /// RIPscrip graphics under the BBS's screen: they show through cells
    /// left blank on black.
    pub graphics: Option<Graphics<'a>>,
    /// Pixel graphics (sixel) the BBS's cells show, by [`Cell::tile`].
    pub tiles: Option<&'a Tiles>,
}

/// A picture in 16 palette colors, stretched over the BBS's screen.
#[derive(Clone, Copy)]
pub struct Graphics<'a> {
    pub pixels: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub palette: [u32; 16],
}

impl<'a> Frame<'a> {
    /// Rows drawn in the standard look.
    pub fn plain(rows: Vec<&'a [Cell]>, cursor: Option<(usize, usize)>, ice: bool) -> Frame<'a> {
        Frame {
            rows,
            cursor,
            ice,
            selection: None,
            look: Look::standard(),
            look_rows: 0,
            ui_look: Look::standard(),
            graphics: None,
            tiles: None,
        }
    }
}

pub struct Renderer {
    font: Font,
    /// Cell width: 8, or 9 as on a VGA in text mode.
    cell_w: usize,
    cols: usize,
    rows: usize,
    native: Vec<u32>,
    crt: Crt,
}

impl Renderer {
    pub fn new(font: Font, cols: usize, rows: usize) -> Renderer {
        let mut r = Renderer { font, cell_w: 8, cols: 0, rows: 0, native: Vec::new(), crt: Crt::default() };
        r.set_layout(font, 8, cols, rows);
        r
    }

    /// Change the font, cell width (8 or 9) or grid size.
    pub fn set_layout(&mut self, font: Font, cell_w: usize, cols: usize, rows: usize) {
        self.font = font;
        self.cell_w = cell_w;
        self.cols = cols;
        self.rows = rows;
        let len = self.native_width() * self.native_height();
        if self.native.len() != len {
            self.native = vec![0; len];
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn native_width(&self) -> usize {
        self.cols * self.cell_w
    }

    pub fn native_height(&self) -> usize {
        self.rows * self.font.height
    }

    /// Rasterize `frame` at native resolution. `blink_on` is the phase of
    /// blinking text, `cursor_on` the phase of the cursor.
    pub fn draw(&mut self, frame: &Frame, blink_on: bool, cursor_on: bool) {
        let nw = self.native_width();
        let (fh, cw) = (self.font.height, self.cell_w);
        let blank = Cell::default();
        for y in 0..self.rows {
            let row = frame.rows.get(y).copied().unwrap_or(&[]);
            for x in 0..self.cols {
                let cell = row.get(x).unwrap_or(&blank);
                let look = if y < frame.look_rows { frame.look } else { frame.ui_look };
                let (mut fg, mut bg, blinks) = cell.resolve_with(frame.ice, look);
                let font = match look.font_for(cell) {
                    0 => self.font,
                    id => Font::cterm(id, self.font),
                };
                if frame.selection.is_some_and(|s| s.contains(x, y)) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let hide = blinks && !blink_on;
                let cursor_here = cursor_on && frame.cursor == Some((x, y));
                // A VGA's ninth column repeats the eighth for the line-drawing
                // glyphs, so their lines join up; otherwise it is background.
                let extend = font.id == 0 && (0xC0..=0xDF).contains(&cell.ch);
                // Graphics show through a black background.
                let graphics = frame.graphics.filter(|_| {
                    y < frame.look_rows
                        && cell.bg == Color::Pc(0)
                        && cell.flags & flags::REVERSE == 0
                        && bg == look.rgb(Color::Pc(0))
                });
                let tile = frame.tiles.filter(|_| cell.tile != 0 && y < frame.look_rows).and_then(|t| t.get(cell.tile));
                for gy in 0..fh {
                    let mut bits = if hide { 0 } else { font.row(cell.ch, gy) as u16 };
                    if cell.flags & flags::UNDERLINE != 0 && gy == fh - 1 && !hide {
                        bits = 0x1FF;
                    }
                    // VGA-style underline cursor on the bottom two scan lines.
                    if cursor_here && gy >= fh - 2 {
                        bits = 0x1FF;
                    }
                    let base = (y * fh + gy) * nw + x * cw;
                    for gx in 0..8 {
                        self.native[base + gx] = if bits & (0x80 >> gx) != 0 { fg } else { bg };
                    }
                    if cw == 9 {
                        let on = bits & 0x100 != 0 || extend && bits & 1 != 0;
                        self.native[base + 8] = if on { fg } else { bg };
                    }
                    if let Some(g) = graphics {
                        let (text_w, text_h) = (self.cols * cw, frame.look_rows * fh);
                        let gyy = ((y * fh + gy) * g.height / text_h).min(g.height - 1);
                        for gx in 0..cw {
                            let on = gx < 8 && bits & (0x80 >> gx) != 0 || gx == 8 && self.native[base + 8] == fg;
                            if !on {
                                let gxx = ((x * cw + gx) * g.width / text_w).min(g.width - 1);
                                self.native[base + gx] = g.palette[g.pixels[gyy * g.width + gxx] as usize & 15];
                            }
                        }
                    }
                    // Pixel graphics over the cell, scaled to it (a 9-dot
                    // cell repeats the eighth column); the cursor stays seen.
                    if let Some(t) = tile.filter(|_| !(cursor_here && gy >= fh - 2)) {
                        let row = &t.pixels[(gy * t.height / fh).min(t.height - 1) * t.width..][..t.width];
                        for gx in 0..cw {
                            let p = row[(gx * t.width / cw).min(t.width - 1)];
                            if p != 0 {
                                self.native[base + gx] = p & 0x00FF_FFFF;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Invert the top `rows` text rows, for a visual bell.
    pub fn flash(&mut self, rows: usize) {
        let n = (rows * self.font.height * self.native_width()).min(self.native.len());
        for p in &mut self.native[..n] {
            *p ^= 0x00FF_FFFF;
        }
    }

    /// Where a point of the native frame is in a picture `width` x `height`
    /// stretched over the top `rows` text rows, if it is over them.
    pub fn graphics_point(
        &self,
        x: usize,
        y: usize,
        rows: usize,
        width: usize,
        height: usize,
    ) -> Option<(usize, usize)> {
        let (text_w, text_h) = (self.native_width(), rows * self.font.height);
        (x < text_w && y < text_h).then(|| (x * width / text_w, y * height / text_h))
    }

    /// The frame drawn by the last [`Renderer::draw`], as 0RGB pixels.
    pub fn pixels(&self) -> &[u32] {
        &self.native
    }

    /// Scale the native frame into `out` (a `width` x `height` buffer),
    /// centered, keeping its shape (with `display.pixel_aspect` applied).
    /// Returns the destination rectangle (x, y, w, h).
    pub fn blit(
        &mut self,
        out: &mut [u32],
        width: usize,
        height: usize,
        display: &Display,
    ) -> (usize, usize, usize, usize) {
        self.blit_in(out, width, height, (0, 0, width, height), display)
    }

    /// [`Renderer::blit`] into the `area` (x, y, w, h) of `out`, such as a
    /// bezel's screen; the rest of `out` is cleared to black.
    pub fn blit_in(
        &mut self,
        out: &mut [u32],
        width: usize,
        height: usize,
        area: (usize, usize, usize, usize),
        display: &Display,
    ) -> (usize, usize, usize, usize) {
        let (ax, ay) = (area.0.min(width - 1), area.1.min(height - 1));
        let (aw, ah) = (area.2.clamp(1, width - ax), area.3.clamp(1, height - ay));
        let (nw, nh) = (self.native_width(), self.native_height());
        let pa = display.pixel_aspect.max(0.1);
        let fit = (aw as f64 / nw as f64).min(ah as f64 / (nh as f64 * pa));
        let scale = match display.scaling {
            Scaling::Integer if fit >= 1.0 => fit.floor(),
            _ => fit,
        };
        let (dw, dh) = match display.scaling {
            Scaling::Stretch => (aw, ah),
            _ => (
                ((nw as f64 * scale).round() as usize).clamp(1, aw),
                ((nh as f64 * scale * pa).round() as usize).clamp(1, ah),
            ),
        };
        let ox = ax + (aw - dw) / 2;
        let oy = ay + (ah - dh) / 2;
        out.fill(0);
        if display.crt == CrtMode::Off {
            scale_into(&self.native, nw, nh, out, width, (ox, oy, dw, dh));
        } else {
            self.crt.present(
                display.crt,
                display.glow,
                display.scanlines,
                display.curvature,
                &self.native,
                nw,
                nh,
                out,
                width,
                (ox, oy, dw, dh),
            );
        }
        (ox, oy, dw, dh)
    }
}

/// Scale a `nw` x `nh` image into the `rect` of `out` (`stride` pixels per
/// row). Upscaling uses sharp bilinear, which is exact at whole multiples;
/// shrinking a too-small window uses nearest-neighbour.
pub fn scale_into(
    src: &[u32],
    nw: usize,
    nh: usize,
    out: &mut [u32],
    stride: usize,
    rect: (usize, usize, usize, usize),
) {
    let (ox, oy, dw, dh) = rect;
    if dw >= nw && dh >= nh {
        let xs = taps(nw, dw);
        let ys = taps(nh, dh);
        for (dy, &(sy, wy)) in ys.iter().enumerate() {
            let row0 = &src[sy * nw..][..nw];
            let row1 = &src[(sy + 1).min(nh - 1) * nw..][..nw];
            let dst = &mut out[(oy + dy) * stride + ox..][..dw];
            for (d, &(sx, wx)) in dst.iter_mut().zip(&xs) {
                let x1 = (sx + 1).min(nw - 1);
                *d = if wy == 256 {
                    if wx == 256 { row0[sx] } else { mix(row0[sx], row0[x1], wx) }
                } else {
                    let top = if wx == 256 { row0[sx] } else { mix(row0[sx], row0[x1], wx) };
                    let bottom = if wx == 256 { row1[sx] } else { mix(row1[sx], row1[x1], wx) };
                    mix(top, bottom, wy)
                };
            }
        }
        return;
    }
    let xmap: Vec<usize> = (0..dw).map(|dx| dx * nw / dw).collect();
    for dy in 0..dh {
        let row = &src[(dy * nh / dh) * nw..][..nw];
        let dst = &mut out[(oy + dy) * stride + ox..][..dw];
        for (d, &sx) in dst.iter_mut().zip(&xmap) {
            *d = row[sx];
        }
    }
}

/// For each of `dst` output pixels covering `src` frame pixels (`dst` >=
/// `src`): the first frame pixel it covers, and that pixel's share of it in
/// 256ths (256 when it lies entirely within one frame pixel).
fn taps(src: usize, dst: usize) -> Vec<(usize, u32)> {
    (0..dst)
        .map(|d| {
            // The output pixel spans [d, d + 1) * src / dst in frame pixels;
            // in units of 1/dst that is [d * src, (d + 1) * src).
            let (start, end) = (d * src, (d + 1) * src);
            let first = start / dst;
            let boundary = (first + 1) * dst;
            if end <= boundary { (first, 256) } else { (first, ((boundary - start) * 256 / src) as u32) }
        })
        .collect()
}

/// Blend two 0RGB pixels: `w`/256 of `a`, the rest `b`.
pub fn mix(a: u32, b: u32, w: u32) -> u32 {
    let ch = |shift: u32| {
        let (ca, cb) = ((a >> shift) & 0xFF, (b >> shift) & 0xFF);
        ((ca * w + cb * (256 - w)) / 256) << shift
    };
    ch(16) | ch(8) | ch(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use badterm_core::{Directory, Entry, Terminal};

    use crate::ui::dialer::DialerUi;

    fn write_ppm(r: &mut Renderer, path: &std::path::Path) {
        let (w, h) = (r.native_width() * 2, r.native_height() * 2);
        let mut out = vec![0u32; w * h];
        r.blit(&mut out, w, h, &Display::default());
        let mut data = format!("P6 {w} {h} 255\n").into_bytes();
        for px in out {
            data.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
        std::fs::write(path, data).unwrap();
    }

    /// The dialing directory and settings in some of the color schemes, as PNGs:
    /// `BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm theme_snapshot -- --ignored`
    #[test]
    #[ignore]
    fn theme_snapshot() {
        use crate::ui::theme;
        let dir = std::path::PathBuf::from(std::env::var("BADTERM_SNAPSHOT_DIR").expect("set BADTERM_SNAPSHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["Bad Cluster", "Dark blue", "Dark cyan"] {
            let bc = theme::SCHEMES.iter().find(|(n, _)| *n == name).unwrap().1;
            let prefix = name.to_lowercase().replace(' ', "");
            theme::set_current(bc);
            let look = theme::ui_look();
            let mut d = Directory::default();
            d.merge(vec![
                Entry {
                    name: "Example BBS".into(),
                    comment: "Runs Synchronet".into(),
                    ..Entry::from_address("bbs.example.org").unwrap()
                },
                Entry::from_address("ssh://secure.example.net:2222").unwrap(),
                Entry::from_address("telnet://another.example.com:2323").unwrap(),
            ]);
            d.offer_defaults();
            d.sort();
            let mut r = Renderer::new(Font::vga_8x16(), 80, 25);
            let shot = |r: &mut Renderer, s: &badterm_core::Screen, cursor, name: &str| {
                let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
                r.draw(&Frame { ui_look: &look, ..Frame::plain(rows, cursor, true) }, true, true);
                let png = crate::png::encode(r.pixels(), r.native_width(), r.native_height());
                std::fs::write(dir.join(name), png).unwrap();
            };
            let mut ui = DialerUi::new(dir.clone(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
            let mut s = badterm_core::Screen::new(80, 25, 0);
            ui.draw(&mut s, &d);
            shot(&mut r, &s, None, &format!("{prefix}-dialer.png"));
            ui.handle_key(crate::input::Key::Char('e'), crate::input::Mods::default(), &mut d);
            let cursor = ui.draw(&mut s, &d);
            shot(&mut r, &s, cursor, &format!("{prefix}-edit.png"));
            let mut settings = crate::settings::Settings { colors: bc, ..Default::default() };
            settings.colors = bc;
            let mut sui = crate::ui::settings::SettingsUi::default();
            sui.open(Vec::new(), Vec::new(), vec![std::path::PathBuf::from("/home/sysop/Downloads"); 5]);
            let cursor = sui.draw(&mut s, &settings, None, None);
            shot(&mut r, &s, cursor, &format!("{prefix}-settings.png"));
            crate::ui::splash::draw(&mut s);
            shot(&mut r, &s, None, &format!("{prefix}-splash.png"));
        }
        theme::set_current(theme::Theme::default());
    }

    /// The yes/no questions, as PNGs:
    /// `BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm confirm_snapshot -- --ignored`
    #[test]
    #[ignore]
    fn confirm_snapshot() {
        let dir = std::path::PathBuf::from(std::env::var("BADTERM_SNAPSHOT_DIR").expect("set BADTERM_SNAPSHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = Directory::default();
        d.offer_defaults();
        let mut r = Renderer::new(Font::vga_8x16(), 80, 25);
        let shot = |r: &mut Renderer, s: &badterm_core::Screen, name: &str| {
            let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
            r.draw(&Frame::plain(rows, None, true), true, true);
            std::fs::write(dir.join(name), crate::png::encode(r.pixels(), r.native_width(), r.native_height()))
                .unwrap();
        };
        let mut ui = DialerUi::new(dir.clone(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
        let mut s = badterm_core::Screen::new(80, 25, 0);
        ui.draw(&mut s, &d);
        crate::ui::confirm_box(
            &mut s,
            "Exit BadTerm?",
            "This hangs up the call to The Vortex BBS.",
            &[("Y", "Exit"), ("N", "Stay")],
        );
        shot(&mut r, &s, "confirm-exit-call.png");
        ui.draw(&mut s, &d);
        crate::ui::confirm_box(&mut s, "Exit BadTerm?", "", &[("Y", "Exit"), ("N", "Stay")]);
        shot(&mut r, &s, "confirm-exit.png");
        ui.handle_key(crate::input::Key::Delete, crate::input::Mods::default(), &mut d);
        ui.draw(&mut s, &d);
        shot(&mut r, &s, "confirm-delete.png");
    }

    /// Previews of the themes and bezels offered on the website, from
    /// `extras/`, into `extras/previews`:
    /// `cargo test -p badterm make_extra_previews -- --ignored`
    #[test]
    #[ignore]
    fn make_extra_previews() {
        use crate::ui::theme;
        let extras = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extras");
        let out_dir = extras.join("previews");
        std::fs::create_dir_all(&out_dir).unwrap();
        let png = |pixels: &[u32], w: usize, h: usize, path: std::path::PathBuf| {
            let rgb: Vec<u8> = pixels.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
            let mut out = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
                enc.set_color(png::ColorType::Rgb);
                enc.set_compression(png::Compression::High);
                enc.write_header().unwrap().write_image_data(&rgb).unwrap();
            }
            std::fs::write(path, out).unwrap();
        };
        let mut d = Directory::default();
        d.merge(vec![
            Entry {
                name: "Example BBS".into(),
                comment: "Runs Synchronet".into(),
                ..Entry::from_address("bbs.example.org").unwrap()
            },
            Entry::from_address("ssh://secure.example.net:2222").unwrap(),
            Entry::from_address("telnet://another.example.com:2323").unwrap(),
        ]);
        d.offer_defaults();
        d.sort();
        let mut r = Renderer::new(Font::vga_8x16(), 80, 25);
        let mut s = badterm_core::Screen::new(80, 25, 0);
        let mut themes: Vec<_> = std::fs::read_dir(extras.join("themes")).unwrap().map(|e| e.unwrap().path()).collect();
        themes.sort();
        for path in themes.iter().filter(|p| p.extension().is_some_and(|x| x == "toml")) {
            theme::set_current(theme::ThemeFile::load(path).unwrap().colors);
            let look = theme::ui_look();
            let mut ui =
                DialerUi::new(std::env::temp_dir(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
            ui.draw(&mut s, &d);
            let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
            r.draw(&Frame { ui_look: &look, ..Frame::plain(rows, None, true) }, true, true);
            let stem = path.file_stem().unwrap().to_string_lossy();
            png(r.pixels(), r.native_width(), r.native_height(), out_dir.join(format!("theme-{stem}.png")));
        }
        // Each bezel around the splash screen, in the default colors.
        theme::set_current(theme::Theme::default());
        crate::ui::splash::draw(&mut s);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame::plain(rows, None, true), true, true);
        let mut bezels: Vec<_> = std::fs::read_dir(extras.join("bezels")).unwrap().map(|e| e.unwrap().path()).collect();
        bezels.sort();
        for path in bezels.iter().filter(|p| p.extension().is_some_and(|x| x == "png")) {
            let bezel = crate::bezel::Bezel::load(path).unwrap();
            let (w, h) = (640, 560);
            let frame = bezel.render(w, h);
            let mut out = vec![0u32; w * h];
            let display = Display { scaling: Scaling::Fill, crt: CrtMode::Color, ..Display::default() };
            r.blit_in(&mut out, w, h, frame.screen, &display);
            crate::bezel::composite(&mut out, &frame.pixels);
            let stem = path.file_stem().unwrap().to_string_lossy().to_lowercase().replace(' ', "-");
            png(&out, w, h, out_dir.join(format!("bezel-{stem}.png")));
        }
    }

    #[test]
    fn sixel_pictures_are_drawn_over_cells() {
        let mut t = Terminal::new(80, 25, 0);
        // An 8x6 red block at the top left, then text at row 2.
        t.feed(b"\x1bP0;1q\"1;1#1;2;100;0;0!8~\x1b\\\x1b[2;1HX");
        let lines: Vec<&[Cell]> = (0..25).map(|y| t.screen.row(y)).collect();
        for cw in [8, 9] {
            let mut r = Renderer::new(Font::vga_8x16(), 80, 25);
            r.set_layout(Font::vga_8x16(), cw, 80, 25);
            let frame =
                Frame { look_rows: 25, tiles: Some(&t.screen.tiles), ..Frame::plain(lines.clone(), None, true) };
            r.draw(&frame, true, true);
            let px = r.pixels();
            let nw = r.native_width();
            assert_eq!(px[0], 0xFF0000);
            assert_eq!(px[5 * nw + cw - 1], 0xFF0000, "the whole cell's width");
            assert_eq!(px[6 * nw], 0, "below the picture, the cell shows through");
        }
    }

    /// A sixel file drawn as BadTerm shows it, as a PNG:
    /// `BADTERM_SIXEL=pic.six BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm sixel_snapshot -- --ignored`
    #[test]
    #[ignore]
    fn sixel_snapshot() {
        let dir = std::path::PathBuf::from(std::env::var("BADTERM_SNAPSHOT_DIR").expect("set BADTERM_SNAPSHOT_DIR"));
        let data = std::fs::read(std::env::var("BADTERM_SIXEL").expect("set BADTERM_SIXEL")).unwrap();
        let mut t = Terminal::new(80, 25, 0);
        t.feed(&data);
        let lines: Vec<&[Cell]> = (0..25).map(|y| t.screen.row(y)).collect();
        let mut r = Renderer::new(Font::vga_8x16(), 80, 25);
        let frame =
            Frame { look_rows: 25, tiles: Some(&t.screen.tiles), ..Frame::plain(lines, Some(t.screen.cursor()), true) };
        r.draw(&frame, true, true);
        std::fs::write(dir.join("sixel.png"), crate::png::encode(r.pixels(), r.native_width(), r.native_height()))
            .unwrap();
    }

    /// The C64 and Atari screens shown on the website, as PNGs at 4x:
    /// `BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm retro_site_snapshot -- --ignored`
    #[test]
    #[ignore]
    fn retro_site_snapshot() {
        use badterm_core::Encoding;
        let dir = std::path::PathBuf::from(std::env::var("BADTERM_SNAPSHOT_DIR").expect("set BADTERM_SNAPSHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let shot = |enc: Encoding, bytes: &[u8], name: &str| {
            let (cols, rows) = enc.screen_size().unwrap();
            let mut t = Terminal::new(cols, rows, 0);
            t.encoding = enc;
            t.start_emulation();
            t.feed(bytes);
            let lines: Vec<&[Cell]> = (0..rows).map(|y| t.screen.row(y)).collect();
            let mut r = Renderer::new(Font::vga_8x8(), cols, rows);
            r.draw(&Frame { look: &t.screen.look, look_rows: rows, ..Frame::plain(lines, None, true) }, true, true);
            let (w, h) = (r.native_width() * 4, r.native_height() * 4);
            let mut out = vec![0u32; w * h];
            r.blit(&mut out, w, h, &Display::default());
            std::fs::write(dir.join(name), crate::png::encode(&out, w, h)).unwrap();
        };
        // A C64 BBS: PETSCII colors, reverse video and line graphics.
        let menu = [
            ("M", "MESSAGE BASES"),
            ("F", "FILE AREAS"),
            ("D", "ONLINE GAMES"),
            ("C", "CHAT WITH SYSOP"),
            ("G", "GOODBYE"),
        ];
        let mut c = b"\x93\x9e\x12    THE BREADBIN BBS  *  NODE 1    \x92\r".to_vec();
        c.push(0x12);
        for color in [0x1c, 0x81, 0x9e, 0x1e, 0x9f, 0x1f, 0x9c, 0x96, 0x99] {
            c.extend([color, b' ', b' ', b' ', b' ']);
        }
        c.extend(b"\x92\r\r\x9f");
        c.push(0xb0);
        c.extend([0xc0; 35]);
        c.extend([0xae, b'\r']);
        for (key, what) in menu {
            c.extend([0x9f, 0xdd]);
            c.extend(b" \x9e[");
            c.extend(key.bytes());
            c.extend(b"]\x05 ");
            c.extend(format!("{what:<30}").into_bytes());
            c.extend([0x9f, 0xdd, b'\r']);
        }
        c.push(0xad);
        c.extend([0xc0; 35]);
        c.extend([0xbd, b'\r', b'\r']);
        c.extend(b"\x99 LAST CALLER: \x05RETRO RICK\r\x9a CALLS TODAY: \x05 42\r\r\x9b COMMAND \x9e> \x05");
        shot(Encoding::Petscii, &c, "site-c64.png");

        // An Atari BBS: ATASCII inverse video and line graphics.
        let inverse = |text: &str| text.bytes().map(|b| b | 0x80).collect::<Vec<u8>>();
        let mut a = vec![0x7d];
        a.extend(inverse("     ATARI BBS EXPRESS!  BOARD 1     "));
        a.extend(b"\x9b\x9b\x11");
        a.extend([0x12; 35]);
        a.extend([0x05, 0x9b]);
        for (key, what) in [("M", "MESSAGES"), ("F", "FILES AND UPLOADS"), ("B", "BULLETINS"), ("G", "GOODBYE")] {
            a.push(b'|');
            a.extend(b" ");
            a.extend(inverse(key));
            a.extend(format!(" {what:<32}|").into_bytes());
            a.push(0x9b);
        }
        a.push(0x1a);
        a.extend([0x12; 35]);
        a.extend([0x03, 0x9b, 0x9b]);
        a.extend(b"LAST CALLER: ANTIC FAN\x9bTIME LEFT: 60 MINUTES\x9b\x9bCOMMAND? ");
        shot(Encoding::Atascii, &a, "site-atari.png");
    }

    /// A RIPscrip scene under ANSI text, as a PNG:
    /// `BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm rip_snapshot -- --ignored`
    #[test]
    #[ignore]
    fn rip_snapshot() {
        use badterm_core::rip;
        let dir = std::path::PathBuf::from(std::env::var("BADTERM_SNAPSHOT_DIR").expect("set BADTERM_SNAPSHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = Terminal::new(rip::COLS, rip::ROWS, 0);
        t.encoding = badterm_core::Encoding::Rip;
        t.start_emulation();
        // A sky, ground, sun, hills, a framed title and two buttons.
        let scene: &[&str] = &[
            "!|*|S0109|B0000HR6Q|S0102|B006QHR9P|c0E|S010E|o8Q1E0U0O",
            "!|c02|S010A|i5A6Q00503C1E|iDE6Q0050461Q|c0F|=00000003|R0A0A6E1O|c0B|Y00000300|@0O0FBADTERM",
            "!|Y00000100|c0F|@0O1BThe badass terminal|c0C|S0104|C9W3K0Y|F9W3K0C",
            "!|1B000000000002000F0F0807000000000000|1U0A8C32960000<>Files<>F^M|1U3M8C6E960000<>Goodbye<>G^M",
        ];
        for line in scene {
            t.feed(line.as_bytes());
            t.feed(b"\r\n");
        }
        t.feed(b"\x1b[38;1H\x1b[1;37;44m Text over graphics: ANSI on a blue bar \x1b[0m");
        let r_rip = t.rip().unwrap();
        let graphics =
            Some(Graphics { pixels: r_rip.pixels(), width: rip::WIDTH, height: rip::HEIGHT, palette: r_rip.palette() });
        let mut r = Renderer::new(Font::vga_8x8(), rip::COLS, rip::ROWS);
        let rows: Vec<&[Cell]> = (0..rip::ROWS).map(|y| t.screen.row(y)).collect();
        let frame = Frame { look_rows: rip::ROWS, graphics, ..Frame::plain(rows, None, true) };
        r.draw(&frame, true, true);
        let png = crate::png::encode(r.pixels(), r.native_width(), r.native_height());
        std::fs::write(dir.join("rip.png"), png).unwrap();
    }

    /// Renders sample frames to PPM images for visual inspection:
    /// `BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm snapshot -- --ignored`
    #[test]
    #[ignore]
    fn snapshot() {
        use crate::input::Key;
        let dir = std::path::PathBuf::from(std::env::var("BADTERM_SNAPSHOT_DIR").expect("set BADTERM_SNAPSHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();

        let mut t = Terminal::new(80, 25, 100);
        let mut ans = b"\x1b[2J\x1b[1;33;44m \xc9\xcd\xcd\xcd\xcd\xbb BadTerm test \x1b[0m\r\n".to_vec();
        for i in 0..8u8 {
            ans.extend_from_slice(format!("\x1b[0;{}m\u{0}", 30 + i).as_bytes());
            ans.extend_from_slice(&[0xDB, 0xDB, 0xB2, 0xB1, 0xB0]);
            ans.extend_from_slice(format!("\x1b[1;{}m", 30 + i).as_bytes());
            ans.extend_from_slice(&[0xDB, 0xDB, 0xB2, 0xB1, 0xB0]);
        }
        ans.extend_from_slice(b"\x1b[0m\r\n\r\n");
        for i in 0..8u8 {
            ans.extend_from_slice(format!("\x1b[0;5;{};30m iCE {i} ", 40 + i).as_bytes());
        }
        ans.extend_from_slice(b"\x1b[0m\r\n\r\n\x1b[38;2;255;128;0mTruecolor \x1b[38;5;45m256-color\x1b[0m ");
        ans.extend_from_slice(b"\x03\x04\x05\x06 \xdc\xdf\xdd\xde \xe0\xe1\xe2\xe3 \xab\xac\r\n");
        ans.extend_from_slice(b"\x1b[10;10H\x1b[4;37mUnderlined\x1b[0m at 10,10");
        t.feed(&ans);
        t.screen.ice_colors = true;

        let mut r = Renderer::new(Font::vga_8x16(), 80, 25);
        let rows: Vec<&[Cell]> = (0..25).map(|y| t.screen.row(y)).collect();
        r.draw(&Frame::plain(rows, Some(t.screen.cursor()), true), true, true);
        write_ppm(&mut r, &dir.join("terminal.ppm"));

        let mut d = Directory::default();
        d.merge(vec![
            Entry {
                name: "Example BBS".into(),
                comment: "Runs Synchronet".into(),
                ..Entry::from_address("bbs.example.org").unwrap()
            },
            Entry::from_address("ssh://secure.example.net:2222").unwrap(),
            Entry::from_address("telnet://another.example.com:2323").unwrap(),
        ]);
        d.offer_defaults();
        let mut over = badterm_core::Screen::new(80, 25, 0);
        for y in 0..25 {
            for (x, c) in t.screen.row(y).iter().enumerate() {
                *over.cell_mut(x, y).unwrap() = *c;
            }
        }
        let progress = badterm_core::transfer::Progress {
            direction: badterm_core::transfer::Direction::Download,
            file_name: "ACID-0897.ZIP".into(),
            file_bytes: 1_400_000,
            file_size: Some(3_100_000),
            files_done: 1,
            files_queued: 0,
            file_started: std::time::Instant::now() - std::time::Duration::from_secs(4),
            resumed_at: None,
        };
        crate::ui::transfer::draw_progress(&mut over, "ZMODEM", &progress);
        let rows: Vec<&[Cell]> = (0..25).map(|y| over.row(y)).collect();
        r.draw(&Frame::plain(rows, None, true), true, true);
        write_ppm(&mut r, &dir.join("transfer.ppm"));

        let mut prompt = crate::ui::transfer::UploadPrompt::new(false, std::env::current_dir().unwrap(), 1);
        let cursor = prompt.draw(&mut over);
        let rows: Vec<&[Cell]> = (0..25).map(|y| over.row(y)).collect();
        r.draw(&Frame::plain(rows, cursor, true), true, true);
        write_ppm(&mut r, &dir.join("upload.ppm"));

        let prompt = crate::ui::transfer::DownloadPrompt::new(3);
        let cursor = prompt.draw(&mut over);
        let rows: Vec<&[Cell]> = (0..25).map(|y| over.row(y)).collect();
        r.draw(&Frame::plain(rows, cursor, true), true, true);
        write_ppm(&mut r, &dir.join("download.ppm"));

        let mut ui = DialerUi::new(dir.clone(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
        let mut s = badterm_core::Screen::new(80, 25, 0);
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame::plain(rows, None, true), true, true);
        write_ppm(&mut r, &dir.join("dialer.ppm"));

        let mut edit = DialerUi::new(dir.clone(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
        let mut ed = d.clone();
        ed.entries[0].crt = Some(crate::crt::CrtMode::Green);
        edit.handle_key(crate::input::Key::Char('e'), crate::input::Mods::default(), &mut ed);
        let mut es = badterm_core::Screen::new(80, 25, 0);
        let cursor = edit.draw(&mut es, &ed);
        let rows: Vec<&[Cell]> = (0..25).map(|y| es.row(y)).collect();
        r.draw(&Frame::plain(rows, cursor, true), true, true);
        write_ppm(&mut r, &dir.join("edit.ppm"));

        ui.confirm_hang_up("Example BBS", "another.example.com");
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame::plain(rows, None, true), true, true);
        write_ppm(&mut r, &dir.join("hangup.ppm"));

        let idle = crate::app::idle_screen(Some("9.9.9"));
        let rows: Vec<&[Cell]> = (0..25).map(|y| idle.row(y)).collect();
        r.draw(&Frame::plain(rows, None, true), true, true);
        write_ppm(&mut r, &dir.join("idle.ppm"));

        for y in 0..25 {
            for (x, c) in t.screen.row(y).iter().enumerate() {
                *over.cell_mut(x, y).unwrap() = *c;
            }
        }
        let mut login = crate::ui::login::tests::sample(true);
        login.field.value = "hunter2".into();
        login.field.cursor = 7;
        login.remember = true;
        let cursor = login.draw(&mut over);
        let rows: Vec<&[Cell]> = (0..25).map(|y| over.row(y)).collect();
        r.draw(&Frame::plain(rows, Some(cursor), true), true, true);
        write_ppm(&mut r, &dir.join("login.ppm"));

        // A mouse selection over the terminal, and the same screen as a PNG.
        let rows: Vec<&[Cell]> = (0..25).map(|y| t.screen.row(y)).collect();
        let selection = Some(crate::selection::Selection { anchor: (20, 0), head: (9, 4) });
        r.draw(&Frame { selection, ..Frame::plain(rows, None, true) }, true, true);
        write_ppm(&mut r, &dir.join("selection.ppm"));
        let png = crate::png::encode(r.pixels(), r.native_width(), r.native_height());
        std::fs::write(dir.join("selection.png"), png).unwrap();

        // Searching the directory.
        let mut ui = DialerUi::new(dir.clone(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
        for c in "/exa".chars() {
            ui.handle_key(crate::input::Key::Char(c), crate::input::Mods::default(), &mut d);
        }
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame::plain(rows, None, true), true, true);
        write_ppm(&mut r, &dir.join("find.ppm"));

        // Phosphor mode, 9-dot cells and 4:3, fullscreen on 1920x1080.
        let write_png = |r: &mut Renderer, display: &Display, name: &str| {
            let (w, h) = (1920, 1080);
            let mut out = vec![0u32; w * h];
            r.blit(&mut out, w, h, display);
            std::fs::write(dir.join(name), crate::png::encode(&out, w, h)).unwrap();
        };
        let rows: Vec<&[Cell]> = (0..25).map(|y| t.screen.row(y)).collect();
        let mut crt = Renderer::new(Font::vga_8x16(), 80, 25);
        crt.set_layout(Font::vga_8x16(), 9, 80, 25);
        crt.draw(&Frame::plain(rows, None, true), true, true);
        for (mode, name) in [
            (crate::crt::CrtMode::Off, "aspect.png"),
            (crate::crt::CrtMode::Color, "crt-color.png"),
            (crate::crt::CrtMode::Green, "crt-green.png"),
            (crate::crt::CrtMode::Amber, "crt-amber.png"),
        ] {
            let display =
                Display { scaling: Scaling::Fill, pixel_aspect: 720.0 / 400.0 * 0.75, crt: mode, ..Display::default() };
            write_png(&mut crt, &display, name);
        }

        // 80x50 with the 8x8 font.
        let mut t50 = Terminal::new(80, 50, 0);
        t50.feed(&ans);
        for y in 10..50 {
            t50.feed(format!("\x1b[{};1H\x1b[1;3{}mLine {y} of 50 ", y + 1, y % 7 + 1).as_bytes());
            t50.feed(&[0xB0, 0xB1, 0xB2, 0xDB]);
        }
        let rows: Vec<&[Cell]> = (0..50).map(|y| t50.screen.row(y)).collect();
        let mut r50 = Renderer::new(Font::vga_8x8(), 80, 50);
        r50.draw(&Frame::plain(rows, None, true), true, true);
        write_png(&mut r50, &Display { scaling: Scaling::Fill, pixel_aspect: 1.2, ..Display::default() }, "80x50.png");

        // The settings menu, the color editor and help.
        let page = |r: &mut Renderer, s: &badterm_core::Screen, name: &str| {
            let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
            r.set_layout(Font::vga_8x16(), 8, 80, 25);
            r.draw(&Frame::plain(rows, None, true), true, true);
            write_ppm(r, &dir.join(name));
        };
        let mut settings = crate::settings::Settings::default();
        let mut sui = crate::ui::settings::SettingsUi::default();
        let mut ps = badterm_core::Screen::new(80, 25, 0);
        sui.draw(&mut ps, &settings, Some("Green screen"), None);
        page(&mut r, &ps, "settings.ppm");
        let mut fui = crate::ui::settings::SettingsUi::default();
        fui.open(Vec::new(), Vec::new(), vec![std::path::PathBuf::from("/home/sysop/Downloads/BadTerm"); 5]);
        let mut fs = crate::settings::Settings::default();
        fs.folders.captures = Some("/home/sysop/BBS/captures".into());
        for k in [Key::Tab, Key::Tab, Key::Down] {
            fui.handle_key(k, crate::input::Mods::default(), &mut fs);
        }
        fui.draw(&mut ps, &fs, None, None);
        page(&mut r, &ps, "folders.ppm");
        for k in [
            Key::Tab,
            Key::Tab,
            Key::Tab,
            Key::Down,
            Key::Enter,
            Key::Down,
            Key::Down,
            Key::Down,
            Key::Down,
            Key::Enter,
            Key::Right,
        ] {
            sui.handle_key(k, crate::input::Mods::default(), &mut settings);
        }
        crate::ui::theme::set_current(settings.colors);
        sui.draw(&mut ps, &settings, None, None);
        page(&mut r, &ps, "colors.ppm");
        let mut mui = crate::ui::settings::SettingsUi::default();
        let mut ms = crate::settings::Settings::default();
        ms.macros[0] = "{user}^M~{password}^M".into();
        ms.macros[2] = "/x^M".into();
        for k in [Key::Tab, Key::Down, Key::Down, Key::Down, Key::Down, Key::Enter] {
            mui.handle_key(k, crate::input::Mods::default(), &mut ms);
        }
        crate::ui::theme::set_current(Default::default());
        mui.draw(&mut ps, &ms, None, None);
        page(&mut r, &ps, "macros.ppm");
        crate::ui::theme::set_current(crate::ui::theme::SCHEMES[2].1);
        crate::ui::help::draw(&mut ps);
        page(&mut r, &ps, "help.ppm");
        crate::ui::theme::set_current(Default::default());

        crate::ui::splash::draw(&mut ps);
        page(&mut r, &ps, "splash.ppm");

        // A C64 (PETSCII) and an Atari (ATASCII) screen.
        let retro = |enc: badterm_core::Encoding, bytes: &[u8], name: &str, r: &mut Renderer| {
            let (cols, rows) = enc.screen_size().unwrap();
            let mut t = Terminal::new(cols, rows, 0);
            t.encoding = enc;
            t.start_emulation();
            t.feed(bytes);
            let lines: Vec<&[Cell]> = (0..rows).map(|y| t.screen.row(y)).collect();
            r.set_layout(Font::vga_8x8(), 8, cols, rows);
            r.draw(&Frame { look: &t.screen.look, look_rows: rows, ..Frame::plain(lines, None, true) }, true, true);
            write_ppm(r, &dir.join(name));
        };
        let mut c64 =
            b"\x93\x05    **** COMMODORE 64 BASIC V2 ****\r\r\x9a 64K RAM SYSTEM  38911 BASIC BYTES FREE\r\r".to_vec();
        c64.extend_from_slice(b"READY.\r\x1c\x12 RED \x92 \x1eGREEN \x1fBLUE \x9eYELLOW \x9fCYAN \x9cPURPLE\r\r\x05");
        c64.extend((0xA0u8..=0xBF).chain(0xC0..=0xDF));
        c64.extend_from_slice(b"\r\r\x9a\x0eLower case: Hello, World! \x8e");
        retro(badterm_core::Encoding::Petscii, &c64, "petscii.ppm", &mut r);
        let mut atari = b"\x7dREADY\x9bATARI BBS \xc1\xd4\xc1\xd2\xc9 (inverse)\x9b\x9b".to_vec();
        atari.extend(0x00u8..0x1B);
        atari.extend_from_slice(b"\x1b\x1c\x1b\x1d\x1b\x1e\x1b\x1f\x9b`abcdefg{|\x1b}\x1b~\x1b\x7f\x9b");
        retro(badterm_core::Encoding::Atascii, &atari, "atascii.ppm", &mut r);

        // The ANSI viewer, with its SAUCE box.
        let art_path = dir.join("viewer-test.ans");
        let mut art = ans.clone();
        art.push(0x1A);
        let mut rec = vec![0u8; 128];
        rec[..7].copy_from_slice(b"SAUCE00");
        rec[7..18].copy_from_slice(b"BadTerm art");
        rec[42..48].copy_from_slice(b"arelas");
        rec[82..90].copy_from_slice(b"20261030");
        rec[94] = 1;
        rec[96..98].copy_from_slice(&80u16.to_le_bytes());
        rec[105] = 1;
        rec[106..113].copy_from_slice(b"IBM VGA");
        art.extend(rec);
        std::fs::write(&art_path, &art).unwrap();
        let mut view = crate::ui::art::ArtView::open(&art_path, 0).unwrap();
        view.handle_key(Key::Char('i'));
        let rows = view.rows();
        let look = &view.art.term.screen.look;
        r.set_layout(Font::vga_8x16(), 8, 80, 25);
        r.draw(&Frame { look, look_rows: 25, ..Frame::plain(rows, None, true) }, true, true);
        write_ppm(&mut r, &dir.join("viewer.ppm"));

        // 132x37, with a dialog centered over it.
        let mut t132 = Terminal::new(132, 37, 0);
        t132.feed(&ans);
        for y in 10..37 {
            t132.feed(format!("\x1b[{};1H\x1b[0;3{}m{}", y + 1, y % 7 + 1, "132 columns. ".repeat(10)).as_bytes());
        }
        let prompt = crate::ui::save::SavePrompt::new(std::path::Path::new("/tmp/badterm-scrollback.ans"));
        let cursor = prompt.draw(&mut t132.screen);
        let rows: Vec<&[Cell]> = (0..37).map(|y| t132.screen.row(y)).collect();
        let mut r132 = Renderer::new(Font::vga_8x16(), 132, 37);
        r132.draw(&Frame::plain(rows, Some(cursor), true), true, true);
        write_ppm(&mut r132, &dir.join("132x37.ppm"));

        // Fullscreen on a 1920x1080 monitor.
        let (w, h) = (1920, 1080);
        let mut out = vec![0u32; w * h];
        r.blit(&mut out, w, h, &Display { scaling: Scaling::Fill, ..Display::default() });
        let mut data = format!("P6 {w} {h} 255\n").into_bytes();
        for px in out {
            data.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
        std::fs::write(dir.join("fullscreen.ppm"), data).unwrap();
    }

    #[test]
    fn integer_scaling_is_exact() {
        let mut r = Renderer::new(Font::vga_8x16(), 80, 25);
        let (w, h) = (1920, 1080);
        let mut out = vec![1u32; w * h];
        let fill = Display { scaling: Scaling::Fill, ..Display::default() };
        // 640x400 fits twice; the rest is black border.
        assert_eq!(r.blit(&mut out, w, h, &Display::default()), (320, 140, 1280, 800));
        assert_eq!(r.blit(&mut out, w, h, &fill), (96, 0, 1728, 1080));
        // 4:3: pixels 1.2 times as tall, so 640x400 shows as 1440x1080.
        assert_eq!(r.blit(&mut out, w, h, &Display { pixel_aspect: 1.2, ..fill }), (240, 0, 1440, 1080));
        // Stretch: the whole window, whatever the shape.
        let stretch = Display { scaling: Scaling::Stretch, ..Display::default() };
        assert_eq!(r.blit(&mut out, w, h, &stretch), (0, 0, w, h));
        // 9-dot cells: 720 wide.
        r.set_layout(Font::vga_8x16(), 9, 80, 25);
        assert_eq!(r.native_width(), 720);
    }

    #[test]
    fn fill_blends_only_the_seams() {
        // A 2.5x upscale: output pixels alternate between lying inside one
        // frame pixel and straddling two (half and half).
        assert_eq!(taps(2, 5), [(0, 256), (0, 256), (0, 128), (1, 256), (1, 256)]);
        // Exactly 3x never blends.
        assert!(taps(4, 12).iter().all(|&(_, w)| w == 256));
        assert_eq!(mix(0xFF0000, 0x0000FF, 128), 0x7F007F);
        assert_eq!(mix(0x123456, 0xABCDEF, 256), 0x123456);
    }
}
