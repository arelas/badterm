//! Software rasterizer: draws character cells with a bitmap font into a
//! native-resolution frame, then scales it into the window buffer.

use phosphor_core::screen::{Cell, flags};

/// An 8-pixel-wide bitmap font with 256 CP437 glyphs.
pub struct Font {
    pub height: usize,
    data: &'static [u8],
}

pub const WIDTH: usize = 8;

impl Font {
    pub fn vga_8x16() -> Font {
        Font { height: 16, data: include_bytes!("../assets/fonts/vga_8x16.bin") }
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

/// One frame to draw: a list of rows of cells plus cursor state.
pub struct Frame<'a> {
    pub rows: Vec<&'a [Cell]>,
    /// Cursor cell, if it should be shown.
    pub cursor: Option<(usize, usize)>,
    pub ice: bool,
}

pub struct Renderer {
    pub font: Font,
    cols: usize,
    rows: usize,
    native: Vec<u32>,
}

impl Renderer {
    pub fn new(font: Font, cols: usize, rows: usize) -> Renderer {
        let mut r = Renderer { font, cols: 0, rows: 0, native: Vec::new() };
        r.set_grid(cols, rows);
        r
    }

    pub fn set_grid(&mut self, cols: usize, rows: usize) {
        if (cols, rows) != (self.cols, self.rows) {
            self.cols = cols;
            self.rows = rows;
            self.native = vec![0; self.native_width() * self.native_height()];
        }
    }

    pub fn native_width(&self) -> usize {
        self.cols * WIDTH
    }

    pub fn native_height(&self) -> usize {
        self.rows * self.font.height
    }

    /// Rasterize `frame` at native resolution. `blink_on` is the phase of
    /// blinking text, `cursor_on` the phase of the cursor.
    pub fn draw(&mut self, frame: &Frame, blink_on: bool, cursor_on: bool) {
        let nw = self.native_width();
        let fh = self.font.height;
        let blank = Cell::default();
        for y in 0..self.rows {
            let row = frame.rows.get(y).copied().unwrap_or(&[]);
            for x in 0..self.cols {
                let cell = row.get(x).unwrap_or(&blank);
                let (fg, bg, blinks) = cell.resolve(frame.ice);
                let hide = blinks && !blink_on;
                let cursor_here = cursor_on && frame.cursor == Some((x, y));
                for gy in 0..fh {
                    let mut bits = if hide { 0 } else { self.font.row(cell.ch, gy) };
                    if cell.flags & flags::UNDERLINE != 0 && gy == fh - 1 && !hide {
                        bits = 0xFF;
                    }
                    // VGA-style underline cursor on the bottom two scan lines.
                    if cursor_here && gy >= fh - 2 {
                        bits = 0xFF;
                    }
                    let base = (y * fh + gy) * nw + x * WIDTH;
                    for gx in 0..WIDTH {
                        self.native[base + gx] = if bits & (0x80 >> gx) != 0 { fg } else { bg };
                    }
                }
            }
        }
    }

    /// Scale the native frame into `out` (a `width` x `height` buffer),
    /// centered and preserving the aspect ratio. Returns the destination
    /// rectangle (x, y, w, h).
    pub fn blit(&self, out: &mut [u32], width: usize, height: usize, scaling: Scaling) -> (usize, usize, usize, usize) {
        let (nw, nh) = (self.native_width(), self.native_height());
        let fit = (width as f64 / nw as f64).min(height as f64 / nh as f64);
        let scale = match scaling {
            Scaling::Integer if fit >= 1.0 => fit.floor(),
            _ => fit,
        };
        let dw = ((nw as f64 * scale).round() as usize).clamp(1, width);
        let dh = ((nh as f64 * scale).round() as usize).clamp(1, height);
        let ox = (width - dw) / 2;
        let oy = (height - dh) / 2;
        out.fill(0);
        // Upscaling by a fraction: sharp bilinear. Exact multiples (and
        // shrinking a too-small window) use plain nearest-neighbour.
        if dw > nw && dh > nh && (!dw.is_multiple_of(nw) || !dh.is_multiple_of(nh)) {
            let xs = taps(nw, dw);
            let ys = taps(nh, dh);
            for (dy, &(sy, wy)) in ys.iter().enumerate() {
                let row0 = &self.native[sy * nw..][..nw];
                let row1 = &self.native[(sy + 1).min(nh - 1) * nw..][..nw];
                let dst = &mut out[(oy + dy) * width + ox..][..dw];
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
            return (ox, oy, dw, dh);
        }
        let xmap: Vec<usize> = (0..dw).map(|dx| dx * nw / dw).collect();
        for dy in 0..dh {
            let src = &self.native[(dy * nh / dh) * nw..][..nw];
            let dst = &mut out[(oy + dy) * width + ox..][..dw];
            for (d, &sx) in dst.iter_mut().zip(&xmap) {
                *d = src[sx];
            }
        }
        (ox, oy, dw, dh)
    }
}

/// For each of `dst` output pixels covering `src` frame pixels (`dst` >
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
fn mix(a: u32, b: u32, w: u32) -> u32 {
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

    fn write_ppm(r: &Renderer, path: &std::path::Path) {
        let (w, h) = (r.native_width() * 2, r.native_height() * 2);
        let mut out = vec![0u32; w * h];
        r.blit(&mut out, w, h, Scaling::Integer);
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
        r.draw(&Frame { rows, cursor: Some(t.screen.cursor()), ice: true }, true, true);
        write_ppm(&r, &dir.join("terminal.ppm"));

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
        r.draw(&Frame { rows, cursor: None, ice: true }, true, true);
        write_ppm(&r, &dir.join("transfer.ppm"));

        let mut prompt = crate::ui::transfer::UploadPrompt::new(true);
        prompt.field.value = "~/Uploads/MYART.ANS".into();
        prompt.field.cursor = prompt.field.value.len();
        let cursor = prompt.draw(&mut over);
        let rows: Vec<&[Cell]> = (0..25).map(|y| over.row(y)).collect();
        r.draw(&Frame { rows, cursor: Some(cursor), ice: true }, true, true);
        write_ppm(&r, &dir.join("upload.ppm"));

        let mut ui = DialerUi::new(dir.clone(), std::rc::Rc::new(crate::secrets::tests::Memory::default()));
        let mut s = phosphor_core::Screen::new(80, 25, 0);
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true }, true, true);
        write_ppm(&r, &dir.join("dialer.ppm"));

        ui.confirm_hang_up("Example BBS", "another.example.com");
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true }, true, true);
        write_ppm(&r, &dir.join("hangup.ppm"));

        let idle = crate::app::idle_screen();
        let rows: Vec<&[Cell]> = (0..25).map(|y| idle.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true }, true, true);
        write_ppm(&r, &dir.join("idle.ppm"));

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
        r.draw(&Frame { rows, cursor: Some(cursor), ice: true }, true, true);
        write_ppm(&r, &dir.join("login.ppm"));

        // Fullscreen on a 1920x1080 monitor.
        let (w, h) = (1920, 1080);
        let mut out = vec![0u32; w * h];
        r.blit(&mut out, w, h, Scaling::Fill);
        let mut data = format!("P6 {w} {h} 255\n").into_bytes();
        for px in out {
            data.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
        std::fs::write(dir.join("fullscreen.ppm"), data).unwrap();
    }

    #[test]
    fn integer_scaling_is_exact() {
        let r = Renderer::new(Font::vga_8x16(), 80, 25);
        let (w, h) = (1920, 1080);
        let mut out = vec![1u32; w * h];
        // 640x400 fits twice; the rest is black border.
        assert_eq!(r.blit(&mut out, w, h, Scaling::Integer), (320, 140, 1280, 800));
        assert_eq!(r.blit(&mut out, w, h, Scaling::Fill), (96, 0, 1728, 1080));
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
