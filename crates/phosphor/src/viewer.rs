//! ANSI art files: loaded with their SAUCE details, for the viewer (Alt-F)
//! and for `phosphor --render`, which draws one to a PNG as Phosphor would
//! show it (the website's screenshots come from it).

use std::path::Path;

use anyhow::{Context, Result};
use phosphor_core::sauce::{self, Sauce};
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
    /// How strong phosphor mode's scanlines are.
    pub scanlines: crate::crt::Scanlines,
}

/// The tallest picture drawn from one file.
const MAX_ROWS: usize = 2000;

/// A piece of art, played into a terminal as tall as it needs.
pub struct Art {
    pub term: Terminal,
    /// The rows it uses (at least a screen's worth).
    pub rows: usize,
    pub sauce: Option<Sauce>,
}

/// Play `data` into a terminal: as wide as SAUCE says (80 otherwise), in
/// its font (unless `font` picks one) and with iCE colors if SAUCE or the
/// art asks. `screen_only` keeps just the last 80x25 screen, as of a
/// captured BBS session.
pub fn load(data: &[u8], font: u8, screen_only: bool) -> Art {
    let (content, sauce) = sauce::split(data);
    let ice = sauce.as_ref().is_some_and(|s| s.ice) || content.windows(5).any(|w| w == b"\x1b[?33");
    let cols = match sauce.as_ref().map(|s| s.width) {
        Some(w @ 40..=255) if !screen_only => w as usize,
        _ => 80,
    };
    let font = if font != 0 { font } else { sauce.as_ref().map_or(0, Sauce::cterm_font) };
    let rows = if screen_only { 25 } else { MAX_ROWS };
    let mut term = Terminal::new(cols, rows, 0);
    term.screen.ice_colors = ice;
    term.screen.base_ice = ice;
    term.screen.base_font = font;
    term.screen.look.fonts[0] = font;
    term.feed(content);
    let used = if screen_only {
        25
    } else {
        let last = (0..rows).rev().find(|&y| term.screen.row(y).iter().any(|c| !c.is_blank()));
        last.map_or(25, |y| (y + 1).max(25))
    };
    Art { term, rows: used, sauce }
}

/// Render `input` to `output`; returns the size of the image.
pub fn render_file(input: &Path, output: &Path, opts: RenderOptions) -> Result<(usize, usize)> {
    let data = std::fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let Art { term, rows: used, .. } = load(&data, opts.font, opts.screen_only);
    let ice = term.screen.ice_colors;
    let cols = term.screen.cols();

    let lines: Vec<&[Cell]> = (0..used).map(|y| term.screen.row(y)).collect();
    let cell_w = if opts.nine_dot { 9 } else { 8 };
    let mut r = Renderer::new(Font::vga_8x16(), cols, used);
    r.set_layout(Font::vga_8x16(), cell_w, cols, used);
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
        r.blit_in(
            &mut out,
            w,
            h,
            frame.screen,
            &Display { scaling: Scaling::Fill, pixel_aspect, crt: opts.crt, scanlines: opts.scanlines },
        );
        crate::bezel::composite(&mut out, &frame.pixels);
        (w, h, out)
    } else if opts.crt == CrtMode::Off && !opts.aspect {
        (nw, nh, r.pixels().to_vec())
    } else {
        // Effects need room: twice the size, and taller for 4:3.
        let pixel_aspect = if opts.aspect { nw as f64 / 400.0 * 0.75 } else { 1.0 };
        let (w, h) = (nw * 2, (nh as f64 * 2.0 * pixel_aspect).round() as usize);
        let mut out = vec![0; w * h];
        r.blit(
            &mut out,
            w,
            h,
            &Display { scaling: Scaling::Fill, pixel_aspect, crt: opts.crt, scanlines: opts.scanlines },
        );
        (w, h, out)
    };
    std::fs::write(output, crate::png::encode(&pixels, w, h))
        .with_context(|| format!("writing {}", output.display()))?;
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sauce_sets_width_font_and_ice() {
        let mut file = b"\x1b[1;31mART\x1b[0m".to_vec();
        file.push(0x1A);
        let mut r = vec![0u8; 128];
        r[..7].copy_from_slice(b"SAUCE00");
        r[94] = 1;
        r[96..98].copy_from_slice(&132u16.to_le_bytes());
        r[105] = 1;
        r[106..120].copy_from_slice(b"Amiga Topaz 2+");
        file.extend(r);
        let art = load(&file, 0, false);
        assert_eq!(art.term.screen.cols(), 132);
        assert!(art.term.screen.ice_colors);
        assert_eq!(art.term.screen.look.fonts[0], 40);
        assert_eq!(art.rows, 25);
        assert_eq!(load(&file, 42, false).term.screen.look.fonts[0], 42);
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
