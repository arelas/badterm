//! "Phosphor mode": the look of a CRT. Bright pixels glow, scanlines show
//! between rows of pixels, the glass curves and darkens toward its edges, and
//! optionally the colors become a green or amber monochrome monitor.
//!
//! It works on the native frame and the scaled result in software. The
//! curvature, scanline and vignette factors for every output pixel are
//! computed once per window size and cached, so a frame costs one pass for
//! the glow at native size, one scaling pass and one lookup per pixel.

use crate::render::scale_into;

pub use phosphor_core::dialer::CrtMode;

/// The phosphor color of a monochrome monitor.
fn tint(mode: CrtMode) -> Option<(u32, u32, u32)> {
    match mode {
        CrtMode::Green => Some((51, 255, 102)),
        CrtMode::Amber => Some((255, 176, 0)),
        CrtMode::Off | CrtMode::Color => None,
    }
}

/// How far the glass bulges: 0 is flat. Slight, so text stays straight.
const CURVE: f64 = 0.012;
/// How dark the edges get.
const VIGNETTE: f64 = 0.2;
/// Darkest point between scanlines: light, so they texture the picture
/// rather than stripe it.
const SCANLINE_DEPTH: f64 = 0.25;
/// How much of the blurred image is added back as glow, in 256ths.
const GLOW: u32 = 150;
/// Brightness boost to make up for the darkening, in 256ths.
const GAIN: f64 = 1.12;

#[derive(Default)]
pub struct Crt {
    /// What `map` and `shade` were built for.
    key: (usize, usize, usize, usize, usize),
    /// For each pixel of the output rectangle, the pixel of `flat` to show,
    /// or `u32::MAX` for none (beyond the curved edge).
    map: Vec<u32>,
    /// And its brightness, in 256ths.
    shade: Vec<u16>,
    /// The native frame with tint and glow.
    lit: Vec<u32>,
    blur: Vec<u32>,
    tmp: Vec<u32>,
    /// `lit`, scaled to the output rectangle.
    flat: Vec<u32>,
}

impl Crt {
    pub fn present(
        &mut self,
        mode: CrtMode,
        native: &[u32],
        nw: usize,
        nh: usize,
        out: &mut [u32],
        stride: usize,
        rect: (usize, usize, usize, usize),
    ) {
        let (ox, oy, dw, dh) = rect;
        self.lit.clear();
        match tint(mode) {
            Some(tint) => self.lit.extend(native.iter().map(|&px| monochrome(px, tint))),
            None => self.lit.extend_from_slice(native),
        }
        self.glow(nw, nh);

        self.flat.resize(dw * dh, 0);
        scale_into(&self.lit, nw, nh, &mut self.flat, dw, (0, 0, dw, dh));

        let key = (nw, nh, dw, dh, 0);
        if key != self.key || self.map.len() != dw * dh {
            self.build(nh, dw, dh);
            self.key = key;
        }
        for y in 0..dh {
            let row = &mut out[(oy + y) * stride + ox..][..dw];
            let (map, shade) = (&self.map[y * dw..][..dw], &self.shade[y * dw..][..dw]);
            for ((d, &m), &s) in row.iter_mut().zip(map).zip(shade) {
                *d = if m == u32::MAX { 0 } else { brighten(self.flat[m as usize], s as u32) };
            }
        }
    }

    /// Add a blurred copy of `lit` back onto itself.
    fn glow(&mut self, w: usize, h: usize) {
        self.blur.clone_from(&self.lit);
        self.tmp.resize(w * h, 0);
        // Two box blurs approximate a Gaussian.
        for _ in 0..2 {
            box_blur(&self.blur, &mut self.tmp, w, h, 2, true);
            box_blur(&self.tmp, &mut self.blur, w, h, 2, false);
        }
        for (px, &b) in self.lit.iter_mut().zip(&self.blur) {
            let ch = |s: u32| {
                let v = ((*px >> s) & 0xFF) + ((b >> s) & 0xFF) * GLOW / 256;
                v.min(255) << s
            };
            *px = ch(16) | ch(8) | ch(0);
        }
    }

    /// Precompute where each output pixel samples from and how bright it is.
    fn build(&mut self, nh: usize, dw: usize, dh: usize) {
        self.map.resize(dw * dh, 0);
        self.shade.resize(dw * dh, 0);
        // Scanlines need about two output pixels per frame pixel to read as
        // lines rather than as moiré: full depth from twice the size.
        let scale_y = dh as f64 / nh as f64;
        let depth = ((scale_y - 1.2) / 0.8).clamp(0.0, 1.0) * SCANLINE_DEPTH;
        // Edges fade out over a couple of pixels, for a smooth curve.
        let fade_px = 2.0;
        for y in 0..dh {
            let v = (y as f64 + 0.5) / dh as f64 * 2.0 - 1.0;
            for x in 0..dw {
                let u = (x as f64 + 0.5) / dw as f64 * 2.0 - 1.0;
                // Barrel distortion: points move outward more toward the
                // corners, so the picture bulges and its edges curve.
                let su = u * (1.0 + CURVE * v * v);
                let sv = v * (1.0 + CURVE * u * u);
                let i = y * dw + x;
                let edge = (1.0 - su.abs()) * dw as f64 / 2.0;
                let edge = edge.min((1.0 - sv.abs()) * dh as f64 / 2.0);
                if edge <= 0.0 {
                    self.map[i] = u32::MAX;
                    self.shade[i] = 0;
                    continue;
                }
                let fx = ((su + 1.0) / 2.0 * dw as f64).clamp(0.0, dw as f64 - 1.0);
                let fy = ((sv + 1.0) / 2.0 * dh as f64).clamp(0.0, dh as f64 - 1.0);
                self.map[i] = (fy as usize * dw + fx as usize) as u32;
                // Brightest mid-row, darkest between rows.
                let t = (fy + 0.5) / scale_y;
                let scan = 1.0 - depth * (std::f64::consts::PI * t).cos().powi(2);
                let vignette = 1.0 - VIGNETTE * (su * su + sv * sv) / 2.0;
                let fade = (edge / fade_px).min(1.0);
                self.shade[i] = (scan * vignette * fade * GAIN * 256.0).round().clamp(0.0, 1023.0) as u16;
            }
        }
    }
}

/// Monochrome: the pixel's brightness, in the phosphor's color.
fn monochrome(px: u32, (r, g, b): (u32, u32, u32)) -> u32 {
    let luma = (((px >> 16) & 0xFF) * 77 + ((px >> 8) & 0xFF) * 150 + (px & 0xFF) * 29) >> 8;
    // A little gamma lift, so dark-gray text stays readable.
    let l = ((luma as f64 / 255.0).powf(0.8) * 255.0) as u32;
    ((r * l / 255) << 16) | ((g * l / 255) << 8) | (b * l / 255)
}

/// Scale a pixel's brightness by `s`/256, saturating.
fn brighten(px: u32, s: u32) -> u32 {
    let ch = |shift: u32| ((((px >> shift) & 0xFF) * s / 256).min(255)) << shift;
    ch(16) | ch(8) | ch(0)
}

/// One pass of a box blur of radius `r`, horizontal or vertical.
fn box_blur(src: &[u32], dst: &mut [u32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let at = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
    let n = (2 * r + 1) as u32;
    for line in 0..lines {
        let mut sum = [0u32; 3];
        let add = |sum: &mut [u32; 3], px: u32, sign: i32| {
            for (k, s) in [16, 8, 0].into_iter().enumerate() {
                let c = (px >> s) & 0xFF;
                sum[k] = if sign > 0 { sum[k] + c } else { sum[k] - c };
            }
        };
        // Edges repeat the end pixels.
        let get = |i: isize| src[at(line, i.clamp(0, len as isize - 1) as usize)];
        for i in -(r as isize)..=(r as isize) {
            add(&mut sum, get(i), 1);
        }
        for i in 0..len {
            dst[at(line, i)] = ((sum[0] / n) << 16) | ((sum[1] / n) << 8) | (sum[2] / n);
            add(&mut sum, get(i as isize + r as isize + 1), 1);
            add(&mut sum, get(i as isize - r as isize), -1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_cycle() {
        let mut m = CrtMode::Off;
        let names: Vec<_> = (0..4)
            .map(|_| {
                m = m.next();
                m.name()
            })
            .collect();
        assert_eq!(names, ["color CRT", "green screen", "amber screen", "off"]);
    }

    #[test]
    fn monochrome_keeps_brightness_order() {
        let tint = tint(CrtMode::Green).unwrap();
        let white = monochrome(0xFFFFFF, tint);
        assert_eq!(white, 0x33FF66);
        assert_eq!(monochrome(0, tint), 0);
        assert!(monochrome(0x555555, tint) & 0xFF00 < monochrome(0xAAAAAA, tint) & 0xFF00);
    }

    #[test]
    fn present_glows_and_curves() {
        // A white dot in the middle of a black frame.
        let (nw, nh) = (40, 25);
        let mut native = vec![0u32; nw * nh];
        native[12 * nw + 20] = 0xFFFFFF;
        let (w, h) = (160, 100);
        let mut out = vec![0x123456; w * h];
        let mut crt = Crt::default();
        crt.present(CrtMode::Color, &native, nw, nh, &mut out, w, (0, 0, w, h));
        // The dot's neighbourhood glows; the corners are past the curve.
        assert!(out[50 * w + 84] & 0xFF > 0, "glow");
        assert_eq!(out[0], 0);
        assert_eq!(out[w * h - 1], 0);
        // A second frame of the same size reuses the cached map.
        let map = crt.map.clone();
        crt.present(CrtMode::Green, &native, nw, nh, &mut out, w, (0, 0, w, h));
        assert_eq!(crt.map, map);
        let px = out[50 * w + 80];
        assert!(px >> 8 & 0xFF > px >> 16 & 0xFF, "green, not white: {px:06x}");
    }
}
