//! Software rasterizer: draws character cells with a bitmap font into a
//! native-resolution frame, then scales it into the window buffer, with an
//! optional CRT look (see [`crate::crt`]).

use phosphor_core::screen::{Cell, flags};

use crate::crt::{Crt, CrtMode};
use crate::selection::Selection;

/// An 8-pixel-wide bitmap font with 256 CP437 glyphs.
#[derive(Clone, Copy)]
pub struct Font {
    pub height: usize,
    data: &'static [u8],
}

impl Font {
    pub fn vga_8x16() -> Font {
        Font { height: 16, data: include_bytes!("../assets/fonts/vga_8x16.bin") }
    }

    /// For 50-line modes.
    pub fn vga_8x8() -> Font {
        Font { height: 8, data: include_bytes!("../assets/fonts/vga_8x8.bin") }
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
}

/// How a frame is presented.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub scaling: Scaling,
    /// Height of a frame pixel relative to its width: 1.0 for square
    /// pixels, about 1.2 to show 640x400 at the 4:3 shape of a CRT.
    pub pixel_aspect: f64,
    pub crt: CrtMode,
}

impl Default for Display {
    fn default() -> Display {
        Display { scaling: Scaling::Integer, pixel_aspect: 1.0, crt: CrtMode::Off }
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
                let (mut fg, mut bg, blinks) = cell.resolve(frame.ice);
                if frame.selection.is_some_and(|s| s.contains(x, y)) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let hide = blinks && !blink_on;
                let cursor_here = cursor_on && frame.cursor == Some((x, y));
                // A VGA's ninth column repeats the eighth for the line-drawing
                // glyphs, so their lines join up; otherwise it is background.
                let extend = (0xC0..=0xDF).contains(&cell.ch);
                for gy in 0..fh {
                    let mut bits = if hide { 0 } else { self.font.row(cell.ch, gy) as u16 };
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
                }
            }
        }
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
        let (nw, nh) = (self.native_width(), self.native_height());
        let pa = display.pixel_aspect.max(0.1);
        let fit = (width as f64 / nw as f64).min(height as f64 / (nh as f64 * pa));
        let scale = match display.scaling {
            Scaling::Integer if fit >= 1.0 => fit.floor(),
            _ => fit,
        };
        let dw = ((nw as f64 * scale).round() as usize).clamp(1, width);
        let dh = ((nh as f64 * scale * pa).round() as usize).clamp(1, height);
        let ox = (width - dw) / 2;
        let oy = (height - dh) / 2;
        out.fill(0);
        if display.crt == CrtMode::Off {
            scale_into(&self.native, nw, nh, out, width, (ox, oy, dw, dh));
        } else {
            self.crt.present(display.crt, &self.native, nw, nh, out, width, (ox, oy, dw, dh));
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
    use phosphor_core::{Directory, Entry, Terminal};

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

    /// Renders sample frames to PPM images for visual inspection:
    /// `PHOSPHOR_SNAPSHOT_DIR=/tmp/shots cargo test -p phosphor snapshot -- --ignored`
    #[test]
    #[ignore]
    fn snapshot() {
        let dir = std::path::PathBuf::from(std::env::var("PHOSPHOR_SNAPSHOT_DIR").expect("set PHOSPHOR_SNAPSHOT_DIR"));
        std::fs::create_dir_all(&dir).unwrap();

        let mut t = Terminal::new(80, 25, 100);
        let mut ans = b"\x1b[2J\x1b[1;33;44m \xc9\xcd\xcd\xcd\xcd\xbb Phosphor test \x1b[0m\r\n".to_vec();
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
        r.draw(&Frame { rows, cursor: Some(t.screen.cursor()), ice: true, selection: None }, true, true);
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
        let mut over = phosphor_core::Screen::new(80, 25, 0);
        for y in 0..25 {
            for (x, c) in t.screen.row(y).iter().enumerate() {
                *over.cell_mut(x, y).unwrap() = *c;
            }
        }
        let progress = phosphor_core::transfer::Progress {
            direction: phosphor_core::transfer::Direction::Download,
            file_name: "ACID-0897.ZIP".into(),
            file_bytes: 1_400_000,
            file_size: Some(3_100_000),
            files_done: 1,
            files_queued: 0,
            file_started: std::time::Instant::now() - std::time::Duration::from_secs(4),
        };
        crate::ui::transfer::draw_progress(&mut over, &progress);
        let rows: Vec<&[Cell]> = (0..25).map(|y| over.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true, selection: None }, true, true);
        write_ppm(&mut r, &dir.join("transfer.ppm"));

        let mut prompt = crate::ui::transfer::UploadPrompt::new(true);
        prompt.field.value = "~/Uploads/MYART.ANS".into();
        prompt.field.cursor = prompt.field.value.len();
        let cursor = prompt.draw(&mut over);
        let rows: Vec<&[Cell]> = (0..25).map(|y| over.row(y)).collect();
        r.draw(&Frame { rows, cursor: Some(cursor), ice: true, selection: None }, true, true);
        write_ppm(&mut r, &dir.join("upload.ppm"));

        let mut ui = DialerUi::new(dir.clone(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
        let mut s = phosphor_core::Screen::new(80, 25, 0);
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true, selection: None }, true, true);
        write_ppm(&mut r, &dir.join("dialer.ppm"));

        ui.confirm_hang_up("Example BBS", "another.example.com");
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true, selection: None }, true, true);
        write_ppm(&mut r, &dir.join("hangup.ppm"));

        let idle = crate::app::idle_screen();
        let rows: Vec<&[Cell]> = (0..25).map(|y| idle.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true, selection: None }, true, true);
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
        r.draw(&Frame { rows, cursor: Some(cursor), ice: true, selection: None }, true, true);
        write_ppm(&mut r, &dir.join("login.ppm"));

        // A mouse selection over the terminal, and the same screen as a PNG.
        let rows: Vec<&[Cell]> = (0..25).map(|y| t.screen.row(y)).collect();
        let selection = Some(crate::selection::Selection { anchor: (20, 0), head: (9, 4) });
        r.draw(&Frame { rows, cursor: None, ice: true, selection }, true, true);
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
        r.draw(&Frame { rows, cursor: None, ice: true, selection: None }, true, true);
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
        crt.draw(&Frame { rows, cursor: None, ice: true, selection: None }, true, true);
        for (mode, name) in [
            (crate::crt::CrtMode::Off, "aspect.png"),
            (crate::crt::CrtMode::Color, "crt-color.png"),
            (crate::crt::CrtMode::Green, "crt-green.png"),
            (crate::crt::CrtMode::Amber, "crt-amber.png"),
        ] {
            let display = Display { scaling: Scaling::Fill, pixel_aspect: 720.0 / 400.0 * 0.75, crt: mode };
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
        r50.draw(&Frame { rows, cursor: None, ice: true, selection: None }, true, true);
        write_png(
            &mut r50,
            &Display { scaling: Scaling::Fill, pixel_aspect: 1.2, crt: crate::crt::CrtMode::Off },
            "80x50.png",
        );

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
