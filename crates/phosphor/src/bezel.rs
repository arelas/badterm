//! Monitor bezels drawn around the screen: a few built-in ones in the style
//! of the CRT monitors and terminals BBSes were called from (original
//! designs, no makers' names or logos), and the user's own as PNG images
//! with a see-through screen area.
//!
//! A bezel is drawn for the window's size and cached: an ARGB image the
//! size of the window, with the screen area (where the terminal is drawn)
//! fully transparent. The terminal is drawn first and the bezel blended
//! over it, so an image's soft edges fall on the screen properly.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// A built-in bezel's look, in fractions of the screen opening's width.
struct Style {
    name: &'static str,
    /// Plastic color.
    body: (f64, f64, f64),
    side: f64,
    top: f64,
    /// The chin under the screen, with the controls.
    bottom: f64,
    corner: f64,
    /// The recessed rim around the glass.
    recess: f64,
    recess_color: (f64, f64, f64),
    led: (f64, f64, f64),
    /// Round knobs (brightness, contrast) rather than a row of buttons.
    knobs: bool,
    /// Ventilation slots in the chin.
    vents: bool,
    /// How much the plastic shines toward the top.
    gloss: f64,
}

const STYLES: [Style; 4] = [
    Style {
        name: "Beige, 1980s",
        body: (0.84, 0.80, 0.70),
        side: 0.10,
        top: 0.09,
        bottom: 0.20,
        corner: 0.05,
        recess: 0.035,
        recess_color: (0.62, 0.58, 0.49),
        led: (0.25, 0.95, 0.35),
        knobs: true,
        vents: true,
        gloss: 0.05,
    },
    Style {
        name: "Platinum, 1990s",
        body: (0.79, 0.79, 0.76),
        side: 0.07,
        top: 0.07,
        bottom: 0.13,
        corner: 0.035,
        recess: 0.025,
        recess_color: (0.58, 0.58, 0.56),
        led: (1.0, 0.65, 0.15),
        knobs: false,
        vents: false,
        gloss: 0.08,
    },
    Style {
        name: "Charcoal, late 1990s",
        body: (0.17, 0.17, 0.18),
        side: 0.06,
        top: 0.06,
        bottom: 0.11,
        corner: 0.06,
        recess: 0.02,
        recess_color: (0.08, 0.08, 0.09),
        led: (0.3, 0.85, 1.0),
        knobs: false,
        vents: false,
        gloss: 0.14,
    },
    Style {
        name: "Terminal",
        body: (0.27, 0.24, 0.20),
        side: 0.12,
        top: 0.11,
        bottom: 0.16,
        corner: 0.09,
        recess: 0.05,
        recess_color: (0.13, 0.12, 0.10),
        led: (0.2, 1.0, 0.4),
        knobs: true,
        vents: true,
        gloss: 0.06,
    },
];

/// The screen opening of a built-in bezel is 4:3, like a CRT's tube.
const SCREEN_ASPECT: f64 = 4.0 / 3.0;

pub enum Bezel {
    Builtin(usize),
    /// An image: 0xAARRGGBB pixels, and its screen area (x, y, w, h).
    Image {
        pixels: Vec<u32>,
        width: usize,
        height: usize,
        screen: (usize, usize, usize, usize),
    },
}

/// A bezel drawn for one window size.
pub struct Rendered {
    /// 0xAARRGGBB, the window's size; transparent over the screen.
    pub pixels: Vec<u32>,
    /// Where the terminal goes: (x, y, w, h).
    pub screen: (usize, usize, usize, usize),
}

/// The names of the bezels there are: the built-in ones, then the user's
/// (their PNG files' names) from `dir`.
pub fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = STYLES.iter().map(|s| s.name.to_string()).collect();
    out.extend(user_files(dir).into_iter().filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())));
    out
}

fn user_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
        .collect();
    out.sort();
    out
}

impl Bezel {
    /// The bezel called `name`: built in, or `dir/name.png`.
    pub fn find(name: &str, dir: &Path) -> Result<Bezel> {
        if let Some(i) = STYLES.iter().position(|s| s.name == name) {
            return Ok(Bezel::Builtin(i));
        }
        let path = user_files(dir)
            .into_iter()
            .find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == name))
            .with_context(|| format!("no bezel called {name} in {}", dir.display()))?;
        Bezel::load(&path)
    }

    /// A PNG bezel. Its screen area is the see-through part around the
    /// middle of the image.
    pub fn load(path: &Path) -> Result<Bezel> {
        let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().with_context(|| format!("reading {}", path.display()))?;
        let mut buf = vec![0; reader.output_buffer_size().context("image too large")?];
        let info = reader.next_frame(&mut buf).with_context(|| format!("reading {}", path.display()))?;
        let (width, height) = (info.width as usize, info.height as usize);
        let channels = info.color_type.samples();
        let pixels: Vec<u32> = buf[..info.buffer_size()]
            .chunks(channels)
            .map(|p| {
                let (r, g, b, a) = match *p {
                    [g] => (g, g, g, 255),
                    [g, a] => (g, g, g, a),
                    [r, g, b] => (r, g, b, 255),
                    [r, g, b, a, ..] => (r, g, b, a),
                    _ => (0, 0, 0, 255),
                };
                (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
            })
            .collect();
        let screen = find_screen(&pixels, width, height)
            .with_context(|| format!("{} has no see-through screen area in the middle", path.display()))?;
        Ok(Bezel::Image { pixels, width, height, screen })
    }

    /// Draw the bezel for a `w` x `h` window.
    pub fn render(&self, w: usize, h: usize) -> Rendered {
        match self {
            Bezel::Builtin(i) => render_builtin(&STYLES[*i], w, h),
            Bezel::Image { pixels, width, height, screen } => render_image(pixels, *width, *height, *screen, w, h),
        }
    }
}

/// The see-through rectangle around the image's middle: as far as the
/// middle row and column stay transparent.
fn find_screen(pixels: &[u32], w: usize, h: usize) -> Result<(usize, usize, usize, usize)> {
    let clear = |x: usize, y: usize| pixels[y * w + x] >> 24 < 128;
    let (cx, cy) = (w / 2, h / 2);
    if w < 8 || h < 8 || !clear(cx, cy) {
        bail!("not see-through in the middle");
    }
    let left = (0..cx).rev().find(|&x| !clear(x, cy)).map_or(0, |x| x + 1);
    let right = (cx..w).find(|&x| !clear(x, cy)).unwrap_or(w);
    let top = (0..cy).rev().find(|&y| !clear(cx, y)).map_or(0, |y| y + 1);
    let bottom = (cy..h).find(|&y| !clear(cx, y)).unwrap_or(h);
    Ok((left, top, right - left, bottom - top))
}

/// Fit an image of `iw` x `ih` into the window, centered: (x, y, scale).
fn fit(iw: f64, ih: f64, w: usize, h: usize) -> (f64, f64, f64) {
    let scale = (w as f64 / iw).min(h as f64 / ih);
    ((w as f64 - iw * scale) / 2.0, (h as f64 - ih * scale) / 2.0, scale)
}

fn render_image(
    src: &[u32],
    sw: usize,
    sh: usize,
    screen: (usize, usize, usize, usize),
    w: usize,
    h: usize,
) -> Rendered {
    let (ox, oy, scale) = fit(sw as f64, sh as f64, w, h);
    let mut pixels = vec![0xFF00_0000; w * h];
    for y in 0..h {
        let sy = (y as f64 + 0.5 - oy) / scale - 0.5;
        if sy < -0.5 || sy > sh as f64 - 0.5 {
            continue;
        }
        for x in 0..w {
            let sx = (x as f64 + 0.5 - ox) / scale - 0.5;
            if sx < -0.5 || sx > sw as f64 - 0.5 {
                continue;
            }
            pixels[y * w + x] = bilinear(src, sw, sh, sx, sy);
        }
    }
    let (x, y, sw2, sh2) = screen;
    let at = |v: usize, o: f64| (o + v as f64 * scale).round().max(0.0) as usize;
    let sx = at(x, ox);
    let sy = at(y, oy);
    let screen = (sx, sy, at(x + sw2, ox).min(w) - sx, at(y + sh2, oy).min(h) - sy);
    Rendered { pixels, screen }
}

fn bilinear(src: &[u32], w: usize, h: usize, x: f64, y: f64) -> u32 {
    let (x, y) = (x.clamp(0.0, (w - 1) as f64), y.clamp(0.0, (h - 1) as f64));
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let mut out = 0;
    for shift in [24, 16, 8, 0] {
        let c = |px: usize, py: usize| ((src[py * w + px] >> shift) & 0xFF) as f64;
        let top = c(x0, y0) * (1.0 - fx) + c(x1, y0) * fx;
        let bottom = c(x0, y1) * (1.0 - fx) + c(x1, y1) * fx;
        out |= ((top * (1.0 - fy) + bottom * fy).round() as u32) << shift;
    }
    out
}

/// Draw a built-in bezel: shaded plastic around a 4:3 opening.
fn render_builtin(s: &Style, w: usize, h: usize) -> Rendered {
    // In units of the opening's width.
    let (bw, bh) = (1.0 + 2.0 * s.side, 1.0 / SCREEN_ASPECT + s.top + s.bottom);
    let (ox, oy, scale) = fit(bw, bh, w, h);
    let px = |u: f64| u * scale;
    let screen = (
        (ox + px(s.side)).round() as usize,
        (oy + px(s.top)).round() as usize,
        px(1.0).round() as usize,
        px(1.0 / SCREEN_ASPECT).round() as usize,
    );
    let mut pixels = vec![0xFF00_0000; w * h];
    let (sx, sy, sw, sh) = (screen.0 as f64, screen.1 as f64, screen.2 as f64, screen.3 as f64);
    let (body_w, body_h) = (px(bw), px(bh));
    let radius = px(s.corner);
    let recess = px(s.recess);
    // The glass's own rim, black, curving into the plastic.
    let glass_r = px(0.03);
    let edge = px(0.012).max(1.0);
    let chin_y = sy + sh + recess;
    let chin_h = oy + body_h - chin_y;

    for y in 0..h {
        let fy = y as f64 + 0.5;
        if fy < oy || fy > oy + body_h {
            continue;
        }
        for x in 0..w {
            let fx = x as f64 + 0.5;
            if fx < ox || fx > ox + body_w {
                continue;
            }
            // The screen opening itself (with rounded corners): see-through.
            let in_glass = rounded(fx - sx, fy - sy, sw, sh, glass_r);
            if in_glass > 0.0 {
                // Fade out over a pixel, so the edge is smooth.
                let a = (1.0 - in_glass.min(1.0)) * 255.0;
                pixels[y * w + x] = (a as u32) << 24;
                continue;
            }
            let outer = rounded(fx - ox, fy - oy, body_w, body_h, radius);
            if outer <= 0.0 {
                continue;
            }
            let ly = (fy - oy) / body_h;
            let mut c = s.body;
            // Light from above: brighter toward the top, a gloss band, and
            // a bevel at the outer edge.
            let mut light = 1.0 + s.gloss * (1.0 - ly) - 0.06 * ly;
            // The bevel: lit where the nearest edge is the top or left,
            // shadowed at the bottom and right.
            let (to_left, to_top) = (fx - ox, fy - oy);
            let (to_right, to_bottom) = (ox + body_w - fx, oy + body_h - fy);
            if outer < edge {
                light *= if to_left.min(to_top) < to_right.min(to_bottom) { 1.12 } else { 0.78 };
            }
            // The recess around the glass: darker, shadowed at the top.
            let around =
                rounded(fx - (sx - recess), fy - (sy - recess), sw + 2.0 * recess, sh + 2.0 * recess, glass_r + recess);
            if around > 0.0 {
                c = s.recess_color;
                let depth = ((fy - (sy - recess)) / (sh + 2.0 * recess)).clamp(0.0, 1.0);
                light = 0.85 + 0.25 * depth;
            }
            // The chin's controls.
            if around <= 0.0 && fy > chin_y && chin_h > 0.0 {
                let cy = (fy - chin_y) / chin_h;
                let cxu = (fx - ox) / body_w;
                if let Some((cc, l)) = control(s, cxu, cy, body_w, chin_h) {
                    c = cc;
                    light = l;
                }
            }
            // A faint texture, as on molded plastic.
            let grain = hash(x as u32, y as u32) * 0.025 - 0.0125;
            let shade = |v: f64| ((v * light + grain).clamp(0.0, 1.0) * 255.0).round() as u32;
            let a = (outer.min(1.0) * 255.0) as u32;
            pixels[y * w + x] = a << 24 | shade(c.0) << 16 | shade(c.1) << 8 | shade(c.2);
        }
    }
    Rendered { pixels, screen }
}

/// What is at (u, v) on the chin (both 0 to 1 across it), if anything: the
/// power LED, knobs or buttons, vents, and a blank badge plate.
fn control(s: &Style, u: f64, v: f64, body_w: f64, chin_h: f64) -> Option<((f64, f64, f64), f64)> {
    let aspect = body_w / chin_h;
    // Distance from (cu, cv) in chin-height units.
    let dist = |cu: f64, cv: f64| (((u - cu) * aspect).powi(2) + (v - cv).powi(2)).sqrt();
    let dark = (s.body.0 * 0.55, s.body.1 * 0.55, s.body.2 * 0.55);
    // Power LED, at the right.
    let d = dist(0.93, 0.5);
    if d < 0.07 {
        return Some((s.led, 1.0 + (0.07 - d) * 6.0));
    }
    if s.knobs {
        for cu in [0.80, 0.86] {
            let d = dist(cu, 0.5);
            if d < 0.16 {
                // A knob, lit from above.
                let lit = 1.0 + (0.5 - v) * 0.6;
                return Some((s.body, if d > 0.13 { 0.7 } else { lit }));
            }
        }
    } else {
        for i in 0..5 {
            let cu = 0.62 + i as f64 * 0.05;
            if ((u - cu) * aspect).abs() < 0.12 && (v - 0.5).abs() < 0.08 {
                return Some((dark, if v < 0.5 { 1.25 } else { 1.0 }));
            }
        }
    }
    if s.vents && (0.06..0.30).contains(&u) {
        let slot = ((u - 0.06) * aspect / 0.12).fract();
        if slot < 0.45 && (v - 0.5).abs() < 0.22 {
            return Some((dark, 0.75));
        }
    }
    // The badge plate, left of center: blank, as a maker's name would go.
    if (0.40..0.56).contains(&u) && (v - 0.5).abs() < 0.14 {
        let rim = (v - 0.5).abs() > 0.11 || u < 0.405 || u > 0.555;
        return Some((s.body, if rim { 0.88 } else { 0.95 }));
    }
    None
}

/// How far inside a rounded rectangle of `w` x `h` with corner radius `r`
/// the point (x, y) is, in pixels (0 or less: outside).
fn rounded(x: f64, y: f64, w: f64, h: f64, r: f64) -> f64 {
    let r = r.min(w / 2.0).min(h / 2.0);
    let dx = (r - x).max(x - (w - r)).max(0.0);
    let dy = (r - y).max(y - (h - r)).max(0.0);
    if dx > 0.0 && dy > 0.0 { r - (dx * dx + dy * dy).sqrt() } else { x.min(y).min(w - x).min(h - y) }
}

fn hash(x: u32, y: u32) -> f64 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h >> 8) as f64 / (1u32 << 24) as f64
}

/// Blend the bezel over the frame in `out` (0RGB).
pub fn composite(out: &mut [u32], bezel: &[u32]) {
    for (o, &b) in out.iter_mut().zip(bezel) {
        let a = b >> 24;
        if a == 0 {
            continue;
        }
        if a == 255 {
            *o = b & 0xFF_FFFF;
            continue;
        }
        let mix = |shift: u32| {
            let (f, s) = ((b >> shift) & 0xFF, (*o >> shift) & 0xFF);
            ((f * a + s * (255 - a)) / 255) << shift
        };
        *o = mix(16) | mix(8) | mix(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_bezels_leave_a_4_3_screen() {
        for (i, s) in STYLES.iter().enumerate() {
            let r = Bezel::Builtin(i).render(1280, 1024);
            let (x, y, w, h) = r.screen;
            assert!((w as f64 / h as f64 - 4.0 / 3.0).abs() < 0.01, "{}: {w}x{h}", s.name);
            assert!(x + w <= 1280 && y + h <= 1024);
            // See-through in the middle of the screen, solid plastic below it.
            assert_eq!(r.pixels[(y + h / 2) * 1280 + x + w / 2] >> 24, 0);
            assert_eq!(r.pixels[(y + h + (1024 - y - h) / 3) * 1280 + 640] >> 24, 255, "{}", s.name);
        }
    }

    #[test]
    fn image_bezels_find_their_screen() {
        let (w, h) = (100, 80);
        let mut pixels = vec![0xFF80_8080u32; w * h];
        for y in 10..60 {
            for x in 20..80 {
                pixels[y * w + x] = 0;
            }
        }
        assert_eq!(find_screen(&pixels, w, h).unwrap(), (20, 10, 60, 50));
        // Twice the size in the window: the screen scales with it.
        let r = render_image(&pixels, w, h, (20, 10, 60, 50), 200, 160);
        assert_eq!(r.screen, (40, 20, 120, 100));
        assert!(find_screen(&vec![0xFF00_0000; w * h], w, h).is_err());
    }

    #[test]
    fn png_round_trip() {
        let dir = std::env::temp_dir().join(format!("phosphor-bezel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // An RGBA image with a clear middle.
        let path = dir.join("My monitor.png");
        let (w, h) = (64, 48);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let clear = (8..56).contains(&x) && (6..40).contains(&y);
                rgba.extend_from_slice(&[200, 190, 170, if clear { 0 } else { 255 }]);
            }
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.write_header().unwrap().write_image_data(&rgba).unwrap();
        }
        std::fs::write(&path, out).unwrap();
        assert_eq!(names(&dir).last().unwrap(), "My monitor");
        match Bezel::find("My monitor", &dir).unwrap() {
            Bezel::Image { screen, .. } => assert_eq!(screen, (8, 6, 48, 34)),
            Bezel::Builtin(_) => panic!(),
        }
        assert!(Bezel::find("Nothing", &dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compositing() {
        let mut out = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF];
        composite(&mut out, &[0, 0xFF12_3456, 0x8000_0000]);
        assert_eq!(out, [0x00FF_0000, 0x0012_3456, 0x0000_007F]);
    }
}
