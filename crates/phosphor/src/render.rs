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
    /// preserving aspect ratio and preferring whole-number scale factors so
    /// pixels stay crisp. Returns the destination rectangle (x, y, w, h).
    pub fn blit(&self, out: &mut [u32], width: usize, height: usize) -> (usize, usize, usize, usize) {
        let (nw, nh) = (self.native_width(), self.native_height());
        let fit = (width as f64 / nw as f64).min(height as f64 / nh as f64);
        let scale = if fit >= 1.0 { fit.floor() } else { fit };
        let dw = ((nw as f64 * scale) as usize).clamp(1, width);
        let dh = ((nh as f64 * scale) as usize).clamp(1, height);
        let ox = (width - dw) / 2;
        let oy = (height - dh) / 2;
        out.fill(0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_core::{Directory, Entry, Terminal};

    use crate::ui::dialer::DialerUi;

    fn write_ppm(r: &Renderer, path: &std::path::Path) {
        let (w, h) = (r.native_width() * 2, r.native_height() * 2);
        let mut out = vec![0u32; w * h];
        r.blit(&mut out, w, h);
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
        let mut ui = DialerUi::new(dir.clone());
        let mut s = phosphor_core::Screen::new(80, 25, 0);
        ui.draw(&mut s, &d);
        let rows: Vec<&[Cell]> = (0..25).map(|y| s.row(y)).collect();
        r.draw(&Frame { rows, cursor: None, ice: true }, true, true);
        write_ppm(&r, &dir.join("dialer.ppm"));
    }
}
