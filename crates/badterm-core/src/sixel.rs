//! Sixel graphics: `DCS p1 ; p2 q <data> ST`, as DEC's VT340 drew them and
//! SyncTERM (CTerm) and xterm do today. Decoded here to an [`Image`], which
//! the screen places at the cursor (see [`crate::screen::Screen::place_image`]).
//!
//! Each data character `?`..`~` is six pixels in a column (bit 0 at the
//! top) in the current color; `!n` repeats the next one, `#n` picks a color
//! and `#n;2;r;g;b` (or `;1;h;l;s`) defines one, `$` returns to the start
//! of the six-pixel band and `-` starts the next band. `"pan;pad;w;h` sets
//! the pixels' shape and the picture's size.

use crate::palette::Rgb;

/// A picture: 0xFFRRGGBB pixels, 0 where nothing was drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

/// An opaque pixel.
pub const fn solid(rgb: Rgb) -> u32 {
    0xFF00_0000 | (rgb & 0x00FF_FFFF)
}

/// Larger pictures are cut off (no screen is anywhere near this size).
const MAX_SIZE: usize = 2048;

/// The VT340's default palette, in percent.
const DEFAULT: [(u32, u32, u32); 16] = [
    (0, 0, 0),
    (20, 20, 80),
    (80, 13, 13),
    (20, 80, 20),
    (80, 20, 80),
    (20, 80, 80),
    (80, 80, 20),
    (53, 53, 53),
    (26, 26, 26),
    (33, 33, 60),
    (60, 26, 26),
    (33, 60, 33),
    (60, 33, 60),
    (33, 60, 60),
    (60, 60, 33),
    (80, 80, 80),
];

fn percent(r: u32, g: u32, b: u32) -> Rgb {
    let c = |v: u32| v.min(100) * 255 / 100;
    c(r) << 16 | c(g) << 8 | c(b)
}

/// DEC's HLS: hue 0 is blue, 120 red, 240 green; lightness and saturation
/// in percent.
fn hls(h: u32, l: u32, s: u32) -> Rgb {
    let h = ((h + 240) % 360) as f64;
    let (l, s) = (l.min(100) as f64 / 100.0, s.min(100) as f64 / 100.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let v = |f: f64| (((f + m) * 255.0).round().clamp(0.0, 255.0)) as u32;
    v(r) << 16 | v(g) << 8 | v(b)
}

/// The numbers after a control character: digits and `;`, stopping at
/// anything else. Returns them and how many bytes they took.
fn numbers(data: &[u8]) -> (Vec<u32>, usize) {
    let mut out = Vec::new();
    let mut cur: Option<u32> = None;
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            d @ b'0'..=b'9' => cur = Some(cur.unwrap_or(0).saturating_mul(10).saturating_add((d - b'0') as u32)),
            b';' => out.push(cur.take().unwrap_or(0)),
            _ => break,
        }
        i += 1;
    }
    if let Some(v) = cur {
        out.push(v);
    }
    (out, i)
}

/// Decode a sixel string: `params` are the DCS parameters before `q`,
/// `data` what follows it. Unset pixels are `background` (opaque) unless
/// `p2` is 1, when they stay see-through.
pub fn decode(params: &[u32], data: &[u8], background: Rgb) -> Image {
    let p1 = params.first().copied().unwrap_or(0);
    let transparent = params.get(1).copied() == Some(1);
    // Pixel height from p1 (deprecated; raster attributes override it).
    let mut aspect = match p1 {
        2 => 5,
        3 | 4 => 3,
        7..=9 => 1,
        _ => 2,
    };
    let mut palette: [Rgb; 256] = std::array::from_fn(|i| {
        let (r, g, b) = DEFAULT[i % 16];
        percent(r, g, b)
    });
    let mut color = palette[0];
    let mut rows: Vec<Vec<u32>> = Vec::new();
    let (mut x, mut band) = (0usize, 0usize);
    let (mut width, mut height) = (0usize, 0usize);
    let mut raster = (0usize, 0usize);
    let mut drawn = false;

    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        i += 1;
        match c {
            b'?'..=b'~' | b'!' => {
                let (count, bits) = if c == b'!' {
                    let (n, used) = numbers(&data[i..]);
                    i += used;
                    let Some(&ch) = data.get(i).filter(|ch| (b'?'..=b'~').contains(ch)) else { continue };
                    i += 1;
                    (n.first().copied().unwrap_or(1).max(1) as usize, ch - b'?')
                } else {
                    (1, c - b'?')
                };
                drawn = true;
                let count = count.min(MAX_SIZE.saturating_sub(x));
                if bits != 0 {
                    for bit in 0..6 {
                        if bits & (1 << bit) == 0 {
                            continue;
                        }
                        for dy in 0..aspect {
                            let y = (band * 6 + bit) * aspect + dy;
                            if y >= MAX_SIZE {
                                continue;
                            }
                            if rows.len() <= y {
                                rows.resize_with(y + 1, Vec::new);
                            }
                            let row = &mut rows[y];
                            if row.len() < x + count {
                                row.resize(x + count, 0);
                            }
                            row[x..x + count].fill(solid(color));
                        }
                    }
                }
                x += count;
                width = width.max(x);
                height = height.max(((band + 1) * 6 * aspect).min(MAX_SIZE));
            }
            b'#' => {
                let (n, used) = numbers(&data[i..]);
                i += used;
                let Some(&reg) = n.first() else { continue };
                let reg = (reg as usize).min(255);
                if n.len() >= 5 {
                    palette[reg] = match n[1] {
                        1 => hls(n[2], n[3], n[4]),
                        _ => percent(n[2], n[3], n[4]),
                    };
                }
                color = palette[reg];
            }
            b'"' => {
                let (n, used) = numbers(&data[i..]);
                i += used;
                if !drawn {
                    let (pan, pad) = (n.first().copied().unwrap_or(1), n.get(1).copied().unwrap_or(1).max(1));
                    aspect = ((pan as f64 / pad as f64).round() as usize).clamp(1, 10);
                    raster = (
                        n.get(2).copied().unwrap_or(0).min(MAX_SIZE as u32) as usize,
                        n.get(3).copied().unwrap_or(0).min(MAX_SIZE as u32) as usize,
                    );
                }
            }
            b'$' => x = 0,
            b'-' => {
                x = 0;
                band += 1;
            }
            _ => {}
        }
    }

    // A raster size that was given is the least the picture covers.
    if !transparent {
        width = width.max(raster.0);
        height = height.max(raster.1);
    }
    let fill = if transparent { 0 } else { solid(background) };
    let mut pixels = vec![fill; width * height];
    for (y, row) in rows.iter().enumerate().take(height) {
        for (x, &p) in row.iter().enumerate().take(width) {
            if p != 0 {
                pixels[y * width + x] = p;
            }
        }
    }
    Image { width, height, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_colors_and_repeats() {
        // A 3x12 picture: red column, then two green columns, over two bands.
        let img = decode(&[0, 1], b"#1;2;100;0;0#2;2;0;100;0\"1;1#1~#2!2~-#1~#2!2~", 0);
        assert_eq!((img.width, img.height), (3, 12));
        assert_eq!(img.pixels[0], solid(0xFF0000));
        assert_eq!(img.pixels[1], solid(0x00FF00));
        assert_eq!(img.pixels[11 * 3 + 2], solid(0x00FF00));
    }

    #[test]
    fn passes_and_transparency() {
        // Bit 0 red, then over the same band ($) bit 1 blue; bits 2-5 unset.
        let img = decode(&[0, 1], b"\"1;1#1;2;100;0;0@$#2;2;0;0;100A", 0);
        assert_eq!(img.pixels[..6], [solid(0xFF0000), solid(0x0000FF), 0, 0, 0, 0]);
        // Without p2 = 1, unset pixels are the background.
        let img = decode(&[], b"\"1;1;2;6@", 0x123456);
        assert_eq!((img.width, img.height), (2, 6));
        assert_eq!(img.pixels[1], solid(0x123456));
        assert_eq!(img.pixels[0], solid(0));
    }

    #[test]
    fn pixel_shape() {
        // The default (p1 = 0) is pixels two high.
        let img = decode(&[], b"~", 0);
        assert_eq!((img.width, img.height), (1, 12));
        let img = decode(&[9], b"~", 0);
        assert_eq!(img.height, 6);
    }

    #[test]
    fn random_input_is_harmless() {
        let mut seed = 0x1234_5678u32;
        let alphabet = b"?@AB~!#$-\";0123456789;;;";
        for _ in 0..200 {
            let data: Vec<u8> = (0..2000)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    alphabet[seed as usize % alphabet.len()]
                })
                .collect();
            let img = decode(&[0, 0], &data, 0);
            assert!(img.width <= MAX_SIZE && img.height <= MAX_SIZE);
            assert_eq!(img.pixels.len(), img.width * img.height);
        }
        // A huge repeat or raster size is cut off.
        let img = decode(&[9], b"\"1;1;99999;99999!999999~", 0);
        assert_eq!((img.width, img.height), (MAX_SIZE, MAX_SIZE));
    }

    #[test]
    fn hls_colors() {
        assert_eq!(hls(0, 50, 100), 0x0000FF);
        assert_eq!(hls(120, 50, 100), 0xFF0000);
        assert_eq!(hls(240, 50, 100), 0x00FF00);
        assert_eq!(hls(0, 100, 0), 0xFFFFFF);
    }
}
