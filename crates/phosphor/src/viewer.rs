//! `phosphor --render`: draw an ANSI file to a PNG, as Phosphor would show
//! it. Used for the website's screenshots, and handy for art.

use std::path::Path;

use anyhow::{Context, Result};
use phosphor_core::{Cell, Terminal};

use crate::bezel::Bezel;
use crate::crt::CrtMode;
use crate::render::{Display, Font, Frame, Renderer, Scaling};

#[derive(Default)]
pub struct RenderOptions {
    pub crt: CrtMode,
    pub nine_dot: bool,
    pub aspect: bool,
    /// Only the final 80x25 screen (for captured BBS sessions) rather than
    /// the whole file as one tall picture (for art).
    pub screen_only: bool,
    /// The font to start in (a CTerm font ID), e.g. Topaz for Amiga art.
    pub font: u8,
    /// A monitor bezel to draw around the screen.
    pub bezel: Option<Bezel>,
}

/// The tallest picture drawn from one file.
const MAX_ROWS: usize = 2000;

/// Render `input` to `output`; returns the size of the image.
pub fn render_file(input: &Path, output: &Path, opts: RenderOptions) -> Result<(usize, usize)> {
    let data = std::fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let (content, sauce_ice) = split_sauce(&data);
    let ice = sauce_ice || content.windows(5).any(|w| w == b"\x1b[?33");

    let rows = if opts.screen_only { 25 } else { MAX_ROWS };
    let mut term = Terminal::new(80, rows, 0);
    term.screen.ice_colors = ice;
    term.screen.base_font = opts.font;
    term.screen.look.fonts[0] = opts.font;
    term.feed(content);
    let ice = term.screen.ice_colors;
    let used = if opts.screen_only {
        25
    } else {
        let last = (0..rows).rev().find(|&y| term.screen.row(y).iter().any(|c| !c.is_blank()));
        last.map_or(25, |y| (y + 1).max(25))
    };

    let lines: Vec<&[Cell]> = (0..used).map(|y| term.screen.row(y)).collect();
    let cell_w = if opts.nine_dot { 9 } else { 8 };
    let mut r = Renderer::new(Font::vga_8x16(), 80, used);
    r.set_layout(Font::vga_8x16(), cell_w, 80, used);
    // The art's own palette and fonts (CTerm sequences), if it sets any.
    let look = &term.screen.look;
    r.draw(&Frame { look, look_rows: used, ..Frame::plain(lines, None, ice) }, true, false);

    let (nw, nh) = (r.native_width(), r.native_height());
    let (w, h, pixels) = if let Some(bezel) = &opts.bezel {
        // The bezel, about as large as a 4:3 monitor at twice the size.
        let (w, h) = (nw * 2 + nw / 2, nw * 2);
        let frame = bezel.render(w, h);
        let (sw, sh) = (frame.screen.2.max(1) as f64, frame.screen.3.max(1) as f64);
        let pixel_aspect = sh / sw * nw as f64 / nh as f64;
        let mut out = vec![0; w * h];
        r.blit_in(&mut out, w, h, frame.screen, &Display { scaling: Scaling::Fill, pixel_aspect, crt: opts.crt });
        crate::bezel::composite(&mut out, &frame.pixels);
        (w, h, out)
    } else if opts.crt == CrtMode::Off && !opts.aspect {
        (nw, nh, r.pixels().to_vec())
    } else {
        // Effects need room: twice the size, and taller for 4:3.
        let pixel_aspect = if opts.aspect { nw as f64 / 400.0 * 0.75 } else { 1.0 };
        let (w, h) = (nw * 2, (nh as f64 * 2.0 * pixel_aspect).round() as usize);
        let mut out = vec![0; w * h];
        r.blit(&mut out, w, h, &Display { scaling: Scaling::Fill, pixel_aspect, crt: opts.crt });
        (w, h, out)
    };
    std::fs::write(output, crate::png::encode(&pixels, w, h))
        .with_context(|| format!("writing {}", output.display()))?;
    Ok((w, h))
}

/// The art without its SAUCE record (and the Ctrl-Z before it), and
/// whether SAUCE asks for iCE colors.
fn split_sauce(data: &[u8]) -> (&[u8], bool) {
    let mut ice = false;
    let mut end = data.len();
    if data.len() >= 128 && data[data.len() - 128..].starts_with(b"SAUCE00") {
        let record = &data[data.len() - 128..];
        // TFlags bit 0: "non-blink mode", that is, iCE colors.
        ice = record[105] & 1 != 0;
        end = data.len() - 128;
    }
    // Ctrl-Z marks the end of the art in files with SAUCE or comments.
    if let Some(z) = data[..end].iter().position(|&b| b == 0x1A) {
        end = z;
    }
    (&data[..end], ice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sauce_is_split_off() {
        let mut file = b"\x1b[1;31mART\x1b[0m".to_vec();
        let art_len = file.len();
        file.push(0x1A);
        let mut sauce = vec![b' '; 128];
        sauce[..7].copy_from_slice(b"SAUCE00");
        sauce[105] = 1;
        file.extend(&sauce);
        let (art, ice) = split_sauce(&file);
        assert_eq!(art.len(), art_len);
        assert!(ice);
        assert_eq!(split_sauce(b"plain"), (&b"plain"[..], false));
    }

    #[test]
    fn renders_a_file() {
        let dir = std::env::temp_dir().join(format!("phosphor-render-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ans = dir.join("a.ans");
        std::fs::write(&ans, b"\x1b[1;32mHello\r\n".repeat(30)).unwrap();
        let png = dir.join("a.png");
        // The whole file: 30 lines.
        assert_eq!(render_file(&ans, &png, RenderOptions::default()).unwrap(), (640, 30 * 16));
        let opts = RenderOptions { crt: CrtMode::Green, screen_only: true, ..RenderOptions::default() };
        assert_eq!(render_file(&ans, &png, opts).unwrap(), (1280, 800));
        let bezel = Some(Bezel::find("Beige, 1980s", &dir).unwrap());
        let opts = RenderOptions { screen_only: true, bezel, ..RenderOptions::default() };
        assert_eq!(render_file(&ans, &png, opts).unwrap(), (1600, 1280));
        assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
