//! Amiga disk fonts, for SkyPix's `ESC [ 10` (see [`crate::skypix`]):
//! `name.font` and a `name/` folder holding a file per size, each an
//! AmigaDOS load file whose code hunk holds the font's `DiskFontHeader`
//! and `TextFont`. The glyphs are one bitmap, a bit per pixel, with each
//! character's place and width, and (for proportional fonts) its spacing
//! and kerning.

use std::path::Path;

/// A font of one size.
#[derive(Debug, Clone)]
pub struct AmigaFont {
    pub height: usize,
    pub baseline: usize,
    /// The width of a character of a fixed-width font.
    x_size: i32,
    lo: u8,
    hi: u8,
    modulo: usize,
    data: Vec<u8>,
    /// (bit offset, width) of each character from `lo` to `hi`, then the
    /// glyph for the characters the font lacks.
    loc: Vec<(usize, usize)>,
    space: Option<Vec<i32>>,
    kern: Option<Vec<i32>>,
}

const HUNK_HEADER: u32 = 0x3F3;
const HUNK_CODE: u32 = 0x3E9;
const HUNK_DATA: u32 = 0x3EA;
const FPF_PROPORTIONAL: u8 = 0x20;

fn long(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn word(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

/// The first code (or data) hunk of an AmigaDOS load file.
fn first_hunk(file: &[u8]) -> Option<&[u8]> {
    if long(file, 0)? != HUNK_HEADER {
        return None;
    }
    // Resident library names (none, in a font), then the hunk table.
    let mut at = 4;
    loop {
        let n = long(file, at)? as usize;
        at += 4;
        if n == 0 {
            break;
        }
        at += n * 4;
    }
    let (first, last) = (long(file, at + 4)?, long(file, at + 8)?);
    at += 12 + (last.checked_sub(first)? as usize + 1) * 4;
    loop {
        let kind = long(file, at)? & 0x3FFF_FFFF;
        let n = long(file, at + 4)? as usize * 4;
        if kind == HUNK_CODE || kind == HUNK_DATA {
            return file.get(at + 8..at + 8 + n);
        }
        // Anything else first (a name, debug data): skip it.
        at += 8 + n;
    }
}

impl AmigaFont {
    /// A font file of one size (`name/size`).
    pub fn parse(file: &[u8]) -> Option<AmigaFont> {
        let h = first_hunk(file)?;
        // After `moveq #-1,d0; rts`: the DiskFontHeader (its node, file ID
        // 0x0F80, revision, segment and name), then the TextFont.
        if word(h, 18)? != 0x0F80 {
            return None;
        }
        let tf = 58;
        let height = word(h, tf + 20)? as usize;
        let flags = *h.get(tf + 23)?;
        let x_size = word(h, tf + 24)? as i32;
        let baseline = word(h, tf + 26)? as usize;
        let (lo, hi) = (*h.get(tf + 32)?, *h.get(tf + 33)?);
        let char_data = long(h, tf + 34)? as usize;
        let modulo = word(h, tf + 38)? as usize;
        let (char_loc, char_space, char_kern) =
            (long(h, tf + 40)? as usize, long(h, tf + 44)? as usize, long(h, tf + 48)? as usize);
        if hi < lo || !(1..=255).contains(&height) {
            return None;
        }
        let n = (hi - lo) as usize + 2;
        let data = h.get(char_data..char_data + modulo * height)?.to_vec();
        let loc = (0..n)
            .map(|i| Some((word(h, char_loc + i * 4)? as usize, word(h, char_loc + i * 4 + 2)? as usize)))
            .collect::<Option<Vec<_>>>()?;
        let table = |at: usize| -> Option<Vec<i32>> {
            (at != 0).then(|| (0..n).map(|i| word(h, at + i * 2).map_or(0, |w| w as i16 as i32)).collect())
        };
        let space = if flags & FPF_PROPORTIONAL != 0 { table(char_space) } else { None };
        Some(AmigaFont { height, baseline, x_size, lo, hi, modulo, data, loc, space, kern: table(char_kern) })
    }

    /// The font `name` (with or without `.font`) at about `size` lines,
    /// from the `fonts` folder: the size asked for, or the nearest.
    pub fn find(fonts: &Path, name: &str, size: usize) -> Option<AmigaFont> {
        let stem = name.trim().trim_end_matches(".font");
        if stem.is_empty() || stem.contains(['/', '\\']) || stem.contains("..") {
            return None;
        }
        // The folder, in any case.
        let dir = std::fs::read_dir(fonts).ok()?.flatten().map(|e| e.path()).find(|p| {
            p.is_dir() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(stem))
        })?;
        let sizes: Vec<usize> =
            std::fs::read_dir(&dir).ok()?.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect();
        let best = sizes.into_iter().min_by_key(|&s: &usize| s.abs_diff(size))?;
        AmigaFont::parse(&std::fs::read(dir.join(best.to_string())).ok()?)
    }

    fn index(&self, c: u8) -> usize {
        if (self.lo..=self.hi).contains(&c) { (c - self.lo) as usize } else { (self.hi - self.lo) as usize + 1 }
    }

    /// How far the pen moves after `c`.
    pub fn advance(&self, c: u8) -> i32 {
        let i = self.index(c);
        match &self.space {
            Some(space) => space[i] + self.kern.as_ref().map_or(0, |k| k[i]),
            None => self.x_size,
        }
    }

    /// The pixels `c` sets: (x, y) from the top left of its cell.
    pub fn pixels(&self, c: u8) -> impl Iterator<Item = (i32, i32)> + '_ {
        let i = self.index(c);
        let (offset, width) = self.loc[i];
        let kern = self.kern.as_ref().map_or(0, |k| k[i]);
        (0..self.height).flat_map(move |y| {
            (0..width).filter_map(move |x| {
                let bit = offset + x;
                let byte = *self.data.get(y * self.modulo + bit / 8)?;
                (byte & (0x80 >> (bit % 8)) != 0).then_some((x as i32 + kern, y as i32))
            })
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny proportional font: 'A' is a 3x4 block, 'I' a 1x4 bar.
    pub(crate) fn sample_font() -> Vec<u8> {
        let (lo, hi, height) = (b'A', b'I', 4usize);
        let n = (hi - lo) as usize + 2;
        let modulo = 2;
        // Bits: 'A' at 0 (3 wide), 'I' at 3 (1 wide); others nothing.
        let data = vec![0xF0, 0x00, 0xF0, 0x00, 0xF0, 0x00, 0xF0, 0x00];
        let mut h = vec![0x70, 0xFF, 0x4E, 0x75];
        h.extend([0u8; 14]); // node
        h.extend(0x0F80u16.to_be_bytes());
        h.extend([0u8; 2 + 4 + 32]);
        assert_eq!(h.len(), 58);
        let tf = h.len();
        h.extend([0u8; 20]);
        h.extend((height as u16).to_be_bytes());
        h.extend([0, FPF_PROPORTIONAL]);
        h.extend(8u16.to_be_bytes()); // x size
        h.extend(3u16.to_be_bytes()); // baseline
        h.extend([0u8; 4]);
        h.extend([lo, hi]);
        let at = tf + 52;
        let (data_at, loc_at) = (at, at + data.len());
        let space_at = loc_at + n * 4;
        h.extend((data_at as u32).to_be_bytes());
        h.extend((modulo as u16).to_be_bytes());
        h.extend((loc_at as u32).to_be_bytes());
        h.extend((space_at as u32).to_be_bytes());
        h.extend(0u32.to_be_bytes()); // no kerning
        assert_eq!(h.len(), at);
        h.extend(&data);
        for i in 0..n {
            let (off, w): (u16, u16) = match i {
                0 => (0, 3),
                8 => (3, 1),
                _ => (0, 0),
            };
            h.extend(off.to_be_bytes());
            h.extend(w.to_be_bytes());
        }
        for i in 0..n {
            h.extend((if i == 8 { 2i16 } else { 4 }).to_be_bytes());
        }
        while h.len() % 4 != 0 {
            h.push(0);
        }
        let mut f = Vec::new();
        for v in [HUNK_HEADER, 0, 1, 0, 0, (h.len() / 4) as u32, HUNK_CODE, (h.len() / 4) as u32] {
            f.extend(v.to_be_bytes());
        }
        f.extend(&h);
        f.extend(0x3F2u32.to_be_bytes());
        f
    }

    #[test]
    fn reads_a_disk_font() {
        let f = AmigaFont::parse(&sample_font()).unwrap();
        assert_eq!((f.height, f.baseline), (4, 3));
        assert_eq!((f.advance(b'A'), f.advance(b'I'), f.advance(b'z')), (4, 2, 4));
        assert_eq!(f.pixels(b'A').count(), 12);
        assert_eq!(f.pixels(b'I').collect::<Vec<_>>(), [(0, 0), (0, 1), (0, 2), (0, 3)]);
        // From a fonts folder, the nearest size.
        let dir = std::env::temp_dir().join(format!("badterm-amigafont-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Tiny")).unwrap();
        std::fs::write(dir.join("Tiny/4"), sample_font()).unwrap();
        assert_eq!(AmigaFont::find(&dir, "tiny.font", 8).unwrap().height, 4);
        assert!(AmigaFont::find(&dir, "../tiny.font", 8).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
