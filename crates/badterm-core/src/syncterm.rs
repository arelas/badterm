//! SyncTERM's Application Program Commands (`ESC _ SyncTERM:... ESC \`),
//! as CTerm documents them: a cache of files the BBS stores on this
//! machine, pictures (PPM and JPEG XL, from the cache or sent inline)
//! drawn anywhere on the screen with clipping, flips, zoom and masks, and
//! copying and pasting pixels. Synchronet's graphical doors (SyncDoom and
//! the like) draw with these.
//!
//! Pixels are drawn over the screen's cells (see
//! [`Screen::draw_pixels`]), so text written later replaces them, as in
//! SyncTERM.

use std::path::{Path, PathBuf};

use data_encoding::{BASE64, BASE64_NOPAD};

use crate::screen::Screen;
use crate::sixel::{Image, solid};

/// Larger pictures are refused.
const MAX_SIDE: usize = 4096;
/// The most the cache for one BBS may hold.
const MAX_CACHE: u64 = 256 << 20;

/// Which pixels to draw: a bitmap, set where the source shows.
#[derive(Clone, Debug, Default)]
struct Mask {
    width: usize,
    height: usize,
    bits: Vec<bool>,
}

impl Mask {
    fn get(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.bits[y * self.width + x]
    }
}

#[derive(Default)]
pub struct SyncTerm {
    /// Where this BBS's files are cached; no cache without one.
    pub cache_dir: Option<PathBuf>,
    /// The two pixel buffers (`B=0`, `B=1`).
    buffers: [Option<Image>; 2],
    /// The mask buffer (`MBUF`).
    mask: Option<Mask>,
    /// Sound slots and what the channels are playing.
    pub audio: crate::audio::Audio,
    /// Sound for the front end to play.
    pub audio_ops: Vec<crate::audio::AudioOp>,
    /// `A;Wait`: read nothing more from the BBS until then.
    pub hold: Option<std::time::Instant>,
}

/// An APC's fields, after `SyncTERM:X;`.
struct Args<'a> {
    fields: Vec<&'a str>,
}

impl<'a> Args<'a> {
    /// `NAME=value`, as a number.
    fn num(&self, name: &str) -> Option<i64> {
        self.text(name).and_then(|v| v.trim().parse().ok())
    }

    fn text(&self, name: &str) -> Option<&'a str> {
        self.fields.iter().find_map(|f| f.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v))
    }

    /// A flag with no value (`FX`).
    fn flag(&self, name: &str) -> bool {
        self.fields.contains(&name)
    }
}

pub(crate) fn decode_b64(data: &str) -> Option<Vec<u8>> {
    let data: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    BASE64.decode(data.as_bytes()).or_else(|_| BASE64_NOPAD.decode(data.trim_end_matches('=').as_bytes())).ok()
}

/// A cache file name the BBS gave: a plain name, no folders.
pub(crate) fn safe_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0') || c.is_control());
    ok.then_some(name)
}

/// glob(3)'s `*` and `?`.
fn glob(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some(b'*'), _) => glob(&pattern[1..], name) || !name.is_empty() && glob(pattern, &name[1..]),
        (Some(b'?'), Some(_)) => glob(&pattern[1..], &name[1..]),
        (Some(p), Some(n)) if p == n => glob(&pattern[1..], &name[1..]),
        _ => false,
    }
}

/// Netpbm's header: the magic, then numbers, with `#` comments. Returns
/// the numbers and where the data starts.
fn pnm_header(data: &[u8], count: usize) -> Option<(Vec<usize>, usize)> {
    let mut i = 2;
    let mut nums = Vec::new();
    while nums.len() < count {
        while i < data.len() && (data[i].is_ascii_whitespace() || data[i] == b'#') {
            if data[i] == b'#' {
                while i < data.len() && data[i] != b'\n' {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        let start = i;
        while i < data.len() && data[i].is_ascii_digit() {
            i += 1;
        }
        nums.push(std::str::from_utf8(&data[start..i]).ok()?.parse().ok()?);
    }
    // One whitespace byte ends the header.
    Some((nums, i + 1))
}

/// The numbers of a plain (text) Netpbm file's data.
fn plain_numbers(data: &[u8]) -> impl Iterator<Item = usize> + '_ {
    data.split(|b| b.is_ascii_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| std::str::from_utf8(t).ok()?.parse().ok())
}

/// A PPM picture, raw (P6) or plain (P3).
fn decode_ppm(data: &[u8]) -> Option<Image> {
    let raw = match data.get(..2)? {
        b"P6" => true,
        b"P3" => false,
        _ => return None,
    };
    let (h, start) = pnm_header(data, 3)?;
    let (width, height, max) = (h[0], h[1], h[2].clamp(1, 255));
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let scale = |v: usize| (v.min(max) * 255 / max) as u32;
    let n = width * height;
    let samples: Vec<usize> = if raw {
        data.get(start..start + n * 3)?.iter().map(|&b| b as usize).collect()
    } else {
        plain_numbers(data.get(start - 1..)?).take(n * 3).collect()
    };
    if samples.len() < n * 3 {
        return None;
    }
    let pixels = samples.chunks(3).map(|c| solid(scale(c[0]) << 16 | scale(c[1]) << 8 | scale(c[2]))).collect();
    Some(Image { width, height, pixels })
}

/// A PBM bitmap, raw (P4) or plain (P1): 1 (black) is set.
fn decode_pbm(data: &[u8]) -> Option<Mask> {
    let raw = match data.get(..2)? {
        b"P4" => true,
        b"P1" => false,
        _ => return None,
    };
    let (h, start) = pnm_header(data, 2)?;
    let (width, height) = (h[0], h[1]);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let bits = if raw {
        // Rows are padded to whole bytes.
        let row = width.div_ceil(8);
        let d = data.get(start..start + row * height)?;
        (0..height).flat_map(|y| (0..width).map(move |x| d[y * row + x / 8] & (0x80 >> (x % 8)) != 0)).collect()
    } else {
        let bits: Vec<bool> = data
            .get(start - 1..)?
            .iter()
            .filter(|b| matches!(b, b'0' | b'1'))
            .take(width * height)
            .map(|&b| b == b'1')
            .collect();
        if bits.len() < width * height {
            return None;
        }
        bits
    };
    Some(Mask { width, height, bits })
}

/// A JPEG XL picture, in sRGB.
fn decode_jxl(data: &[u8]) -> Option<Image> {
    use jxl_oxide::{EnumColourEncoding, JxlImage, RenderingIntent};
    let mut image = JxlImage::builder().read(std::io::Cursor::new(data)).ok()?;
    let (width, height) = (image.width() as usize, image.height() as usize);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    image.request_color_encoding(EnumColourEncoding::srgb(RenderingIntent::Relative));
    let render = image.render_frame(0).ok()?;
    let mut stream = render.stream_no_alpha();
    let (w, h, ch) = (stream.width() as usize, stream.height() as usize, stream.channels() as usize);
    let mut buf = vec![0u8; w * h * ch];
    stream.write_to_buffer(&mut buf);
    let pixels = buf
        .chunks(ch.max(1))
        .map(|c| match *c {
            [r, g, b, ..] => solid((r as u32) << 16 | (g as u32) << 8 | b as u32),
            [v, ..] => solid((v as u32) * 0x010101),
            [] => solid(0),
        })
        .collect();
    Some(Image { width: w, height: h, pixels })
}

impl SyncTerm {
    /// Handle an APC string (without `ESC _` and `ESC \`). Replies go in
    /// `replies`. Returns false if it isn't SyncTERM's.
    pub fn apc(&mut self, data: &[u8], screen: &mut Screen, replies: &mut Vec<u8>) -> bool {
        let Ok(text) = std::str::from_utf8(data) else { return false };
        let Some(rest) = text.strip_prefix("SyncTERM:") else { return false };
        let mut fields: Vec<&str> = rest.split(';').collect();
        let kind = fields.remove(0);
        let verb = if fields.is_empty() { "" } else { fields.remove(0) };
        match (kind, verb) {
            ("C", "S") => self.store(&fields),
            ("C", "L") => self.list(fields.first().copied().unwrap_or("*"), replies),
            ("C", "DrawPPM" | "DrawPPMBlob" | "DrawJXL" | "DrawJXLBlob") => {
                let Some((&last, opts)) = fields.split_last() else { return true };
                let args = Args { fields: opts.to_vec() };
                if let Some(img) = self.load(verb, last) {
                    self.draw(&img, &args, screen);
                }
            }
            ("C", "LoadPPM" | "LoadPPMBlob" | "LoadJXL" | "LoadJXLBlob") => {
                let Some((&last, opts)) = fields.split_last() else { return true };
                let args = Args { fields: opts.to_vec() };
                let b = args.num("B").unwrap_or(0).clamp(0, 1) as usize;
                self.buffers[b] = self.load(verb, last);
            }
            // A cached font into a font slot.
            ("C", "SetFont") => {
                let (Some(slot), Some(name)) = (fields.first(), fields.get(1)) else { return true };
                if let (Ok(slot), Some(data)) = (slot.trim().parse::<u8>(), self.read_cached(name))
                    && !screen.look.load_font(slot, &data)
                {
                    log::debug!("not loading {name} ({} bytes) into font slot {slot}", data.len());
                }
            }
            ("C", "LoadPBM" | "LoadPBMBlob") => {
                let Some(&last) = fields.last() else { return true };
                let bytes = if verb == "LoadPBMBlob" { decode_b64(last) } else { self.read_cached(last) };
                self.mask = bytes.and_then(|b| decode_pbm(&b));
            }
            ("P", "Copy") => {
                let args = Args { fields };
                let b = args.num("B").unwrap_or(0).clamp(0, 1) as usize;
                self.buffers[b] = Some(copy(screen, &args));
            }
            ("P", "Paste") => {
                let args = Args { fields };
                let b = args.num("B").unwrap_or(0).clamp(0, 1) as usize;
                if let Some(img) = self.buffers[b].take() {
                    self.draw(&img, &args, screen);
                    self.buffers[b] = Some(img);
                }
            }
            // JPEG XL is always there.
            ("Q", "JXL") => replies.extend_from_slice(b"\x1b[=1;1-n"),
            ("A", _) => {
                let mut all = vec![verb];
                all.extend(&fields);
                if let Some(until) = self.audio.apc(&all, self.cache_dir.as_ref(), &mut self.audio_ops) {
                    // At most a minute, in case of a sound that never ends.
                    self.hold = Some(until.min(std::time::Instant::now() + std::time::Duration::from_secs(60)));
                }
            }
            // Sound files: some formats (see audio::format_supported).
            ("Q", "libsndfile") => replies.extend_from_slice(b"\x1b[=7;100;1n"),
            ("Q", "libsndfileFormat") => {
                let id = |i: usize| crate::audio::parse_id(fields.get(i).copied().unwrap_or("0"));
                let (pm, ps) = (id(0), id(1));
                let ok = crate::audio::format_supported(pm, ps) as u8;
                replies.extend_from_slice(format!("\x1b[=7;101;{pm};{ps};{ok}n").as_bytes());
            }
            _ => log::debug!("unhandled SyncTERM APC {kind};{verb}"),
        }
        true
    }

    /// A picture by verb: a cached file's name, or a base64 blob.
    fn load(&self, verb: &str, arg: &str) -> Option<Image> {
        let bytes = if verb.ends_with("Blob") { decode_b64(arg)? } else { self.read_cached(arg)? };
        if verb.contains("JXL") { decode_jxl(&bytes) } else { decode_ppm(&bytes) }
    }

    fn cache_path(&self, name: &str) -> Option<PathBuf> {
        Some(self.cache_dir.as_ref()?.join(safe_name(name)?))
    }

    fn read_cached(&self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.cache_path(name)?).ok()
    }

    /// `C;S;name;base64`: store a file in the cache.
    fn store(&self, fields: &[&str]) {
        let (Some(name), Some(data)) = (fields.first(), fields.get(1)) else { return };
        let (Some(path), Some(bytes)) = (self.cache_path(name), decode_b64(data)) else {
            return log::warn!("not caching {name:?}: a bad name or data");
        };
        let dir = path.parent().unwrap_or(Path::new("."));
        let used: u64 = std::fs::read_dir(dir)
            .map(|d| d.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum())
            .unwrap_or(0);
        if used + bytes.len() as u64 > MAX_CACHE {
            return log::warn!("not caching {name}: the cache for this BBS is full");
        }
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, &bytes)) {
            log::warn!("caching {}: {e}", path.display());
        }
    }

    /// `C;L[;pattern]`: the cached files and their MD5 sums.
    fn list(&self, pattern: &str, replies: &mut Vec<u8>) {
        let mut out = b"\x1b_SyncTERM:C;L\n".to_vec();
        if let Some(dir) = &self.cache_dir {
            let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
                .filter(|(n, _)| glob(pattern.as_bytes(), n.as_bytes()))
                .collect();
            names.sort();
            for (name, path) in names {
                if let Ok(bytes) = std::fs::read(&path) {
                    out.extend_from_slice(format!("{name}\t{:x}\n", md5::compute(&bytes)).as_bytes());
                }
            }
        }
        out.extend_from_slice(b"\x1b\\");
        replies.extend_from_slice(&out);
    }

    /// Draw `img` as DrawPPM's options say.
    fn draw(&self, img: &Image, args: &Args, screen: &mut Screen) {
        let num = |n: &str, d: i64| args.num(n).unwrap_or(d);
        let sx = num("SX", 0).clamp(0, img.width as i64) as usize;
        let sy = num("SY", 0).clamp(0, img.height as i64) as usize;
        let sw = num("SW", (img.width - sx) as i64).clamp(0, (img.width - sx) as i64) as usize;
        let sh = num("SH", (img.height - sy) as i64).clamp(0, (img.height - sy) as i64) as usize;
        let (zx, zy) = (num("ZX", 1).clamp(1, 64) as usize, num("ZY", 1).clamp(1, 64) as usize);
        let (fx, fy) = (args.flag("FX"), args.flag("FY"));
        let (dx, dy) = (num("DX", 0) as isize, num("DY", 0) as isize);
        let (mx, my) = (num("MX", 0).max(0) as usize, num("MY", 0).max(0) as usize);
        let mask = if args.flag("MBUF") {
            self.mask.clone()
        } else if let Some(file) = args.text("MFILE") {
            self.read_cached(file).and_then(|b| decode_pbm(&b))
        } else if let Some(bits) = args.text("MASK") {
            let (w, h) = (num("MW", 0).max(0) as usize, num("MH", 0).max(0) as usize);
            decode_b64(bits).filter(|_| w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE).map(|b| Mask {
                width: w,
                height: h,
                bits: (0..w * h).map(|i| b.get(i / 8).is_some_and(|byte| byte & (0x80 >> (i % 8)) != 0)).collect(),
            })
        } else {
            None
        };
        if sw == 0 || sh == 0 {
            return;
        }
        screen.draw_pixels((dx, dy, sw * zx, sh * zy), |x, y| {
            let (i, j) = ((x - dx) as usize / zx, (y - dy) as usize / zy);
            let (i, j) = (if fx { sw - 1 - i } else { i }, if fy { sh - 1 - j } else { j });
            if mask.as_ref().is_some_and(|m| !m.get(mx + i, my + j)) {
                return 0;
            }
            img.pixels[(sy + j) * img.width + sx + i]
        });
    }
}

/// `P;Copy`: the screen's pixels in a rectangle.
fn copy(screen: &Screen, args: &Args) -> Image {
    let (w, h) = screen.size_px();
    let x = args.num("X").unwrap_or(0).clamp(0, w as i64) as usize;
    let y = args.num("Y").unwrap_or(0).clamp(0, h as i64) as usize;
    let cw = args.num("W").unwrap_or((w - x) as i64).clamp(0, (w - x) as i64) as usize;
    let ch = args.num("H").unwrap_or((h - y) as i64).clamp(0, (h - y) as i64) as usize;
    let mut pixels = Vec::with_capacity(cw * ch);
    for yy in y..y + ch {
        for xx in x..x + cw {
            pixels.push(solid(screen.pixel_at(xx, yy)));
        }
    }
    Image { width: cw, height: ch, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apc(st: &mut SyncTerm, screen: &mut Screen, s: &str) -> Vec<u8> {
        let mut replies = Vec::new();
        assert!(st.apc(s.as_bytes(), screen, &mut replies));
        replies
    }

    /// A 2x2 PPM: red, green / blue, white.
    const PPM: &[u8] = b"P6\n# a comment\n2 2\n255\n\xff\x00\x00\x00\xff\x00\x00\x00\xff\xff\xff\xff";

    #[test]
    fn pictures_decode() {
        let img = decode_ppm(PPM).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.pixels, [solid(0xFF0000), solid(0x00FF00), solid(0x0000FF), solid(0xFFFFFF)]);
        let plain = decode_ppm(b"P3 2 1 15  15 0 0  0 0 15").unwrap();
        assert_eq!(plain.pixels, [solid(0xFF0000), solid(0x0000FF)]);
        let m = decode_pbm(b"P4\n10 2\n\xc0\x40\x00\x00").unwrap();
        assert_eq!((m.width, m.height), (10, 2));
        assert!(m.get(0, 0) && m.get(1, 0) && !m.get(2, 0) && m.get(9, 0) && !m.get(0, 1));
        let m = decode_pbm(b"P1 3 1 1 0 1").unwrap();
        assert_eq!(m.bits, [true, false, true]);
        assert!(decode_ppm(b"P6 99999 1 255 x").is_none());
    }

    #[test]
    fn cache_draw_zoom_flip_and_mask() {
        let dir = std::env::temp_dir().join(format!("badterm-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = SyncTerm { cache_dir: Some(dir.clone()), ..Default::default() };
        let mut s = Screen::new(80, 25, 0);
        let b64 = BASE64.encode(PPM);
        apc(&mut st, &mut s, &format!("SyncTERM:C;S;pic.ppm;{b64}"));
        // Bad names aren't stored.
        apc(&mut st, &mut s, &format!("SyncTERM:C;S;../evil;{b64}"));
        let list = String::from_utf8(apc(&mut st, &mut s, "SyncTERM:C;L;*.ppm")).unwrap();
        assert_eq!(list, format!("\x1b_SyncTERM:C;L\npic.ppm\t{:x}\n\x1b\\", md5::compute(PPM)));
        assert!(!dir.parent().unwrap().join("evil").exists());

        // Zoomed 2x at (10, 20), flipped across: green, red / white, blue.
        apc(&mut st, &mut s, "SyncTERM:C;DrawPPM;DX=10;DY=20;ZX=2;ZY=2;FX;pic.ppm");
        assert_eq!(s.pixel_at(10, 20), 0x00FF00);
        assert_eq!(s.pixel_at(11, 21), 0x00FF00);
        assert_eq!(s.pixel_at(12, 20), 0xFF0000);
        assert_eq!(s.pixel_at(10, 22), 0xFFFFFF);
        assert_eq!(s.pixel_at(14, 20), 0, "the background beyond");

        // Inline, off the top left edge, through a mask: only (1,1) shows.
        let mask = BASE64.encode(&[0b0001_0000]);
        apc(&mut st, &mut s, &format!("SyncTERM:C;DrawPPMBlob;DX=-1;DY=-1;MW=2;MH=2;MASK={mask};{b64}"));
        assert_eq!(s.pixel_at(0, 0), 0xFFFFFF);
        assert_eq!(s.pixel_at(1, 0), 0);

        // Copy a piece of the screen and paste it elsewhere.
        apc(&mut st, &mut s, "SyncTERM:P;Copy;B=1;X=10;Y=20;W=4;H=4");
        apc(&mut st, &mut s, "SyncTERM:P;Paste;B=1;DX=100;DY=100");
        assert_eq!(s.pixel_at(100, 100), 0x00FF00);
        assert_eq!(s.pixel_at(103, 103), 0x0000FF);

        // Text written over a cell replaces its pixels.
        s.move_to(1, 1);
        s.put(b'x');
        assert_eq!(s.pixel_at(10, 20), 0);

        assert_eq!(apc(&mut st, &mut s, "SyncTERM:Q;JXL"), b"\x1b[=1;1-n");
        // A cached font into a font slot.
        apc(&mut st, &mut s, &format!("SyncTERM:C;S;my.fnt;{}", BASE64.encode(&[0x3Cu8; 2048])));
        apc(&mut st, &mut s, "SyncTERM:C;SetFont;50;my.fnt");
        assert_eq!(s.look.loaded_font(50).map(|f| f.height), Some(8));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globbing() {
        assert!(glob(b"*", b"a.ppm") && glob(b"*.ppm", b"a.ppm") && glob(b"a?ppm", b"a.ppm"));
        assert!(!glob(b"*.jxl", b"a.ppm") && !glob(b"a", b"ab"));
    }

    /// A JPEG XL file: `BADTERM_JXL=pic.jxl cargo test -p badterm-core jxl -- --ignored`
    #[test]
    #[ignore]
    fn jxl_decodes() {
        let data = std::fs::read(std::env::var("BADTERM_JXL").expect("set BADTERM_JXL")).unwrap();
        let img = decode_jxl(&data).expect("decodes");
        eprintln!("{}x{}", img.width, img.height);
        assert!(img.pixels.iter().any(|&p| p != solid(0)));
    }
}
