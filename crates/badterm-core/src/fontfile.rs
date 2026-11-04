//! Font files from disk, for a directory entry's font: raw DOS fonts
//! (`.fnt`, `.f08`, `.f14`, `.f16`: 256 glyphs of 8 pixels a row, one byte
//! per row) and Linux console fonts (`.psf`, versions 1 and 2), 8 pixels
//! wide. They become 8x8, 8x14 or 8x16 fonts, as CTerm's loaded fonts are
//! (see [`crate::screen::Look::load_font`]); other heights are stretched
//! to the nearest of those.

use anyhow::{Result, bail};

/// The extensions of the files read.
pub const EXTENSIONS: &[&str] = &["fnt", "f08", "f14", "f16", "psf", "psfu"];

/// The 256 glyphs, ready for [`crate::screen::Look::load_font`].
pub fn parse(data: &[u8]) -> Result<Vec<u8>> {
    let (height, glyphs) = if data.starts_with(&[0x36, 0x04]) && data.len() >= 4 {
        // PSF 1: mode, then the bytes (and rows) per glyph.
        let h = data[3] as usize;
        (h, &data[4..])
    } else if data.starts_with(&[0x72, 0xB5, 0x4A, 0x86]) && data.len() >= 32 {
        let word = |i: usize| u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let (header, count, size, h, w) = (word(8), word(16), word(20), word(24), word(28));
        if w > 8 {
            bail!("the font is {w} pixels wide; BadTerm's cells are 8");
        }
        if size != h || count < 256 {
            bail!("not a font of 256 or more glyphs of one byte a row");
        }
        (h, data.get(header..).unwrap_or_default())
    } else if !data.is_empty() && data.len().is_multiple_of(256) {
        (data.len() / 256, data)
    } else {
        bail!("not a font file BadTerm reads (.fnt, .f08, .f14, .f16 or .psf)");
    };
    if !(4..=32).contains(&height) || glyphs.len() < 256 * height {
        bail!("not a font file BadTerm reads (glyphs {height} rows high)");
    }
    let to = match height {
        ..=10 => 8,
        11..=14 => 14,
        _ => 16,
    };
    Ok((0..256).flat_map(|c| (0..to).map(move |y| glyphs[c * height + y * height / to])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_and_psf() {
        let raw: Vec<u8> = (0..256 * 16).map(|i| (i % 251) as u8).collect();
        assert_eq!(parse(&raw).unwrap(), raw);
        let mut psf1 = vec![0x36, 0x04, 0, 14];
        psf1.extend(std::iter::repeat_n(0x18, 256 * 14));
        assert_eq!(parse(&psf1).unwrap().len(), 256 * 14);
        // PSF 2, 512 glyphs 8x12: the first 256, stretched to 14 rows.
        let mut psf2 = vec![0x72, 0xB5, 0x4A, 0x86];
        for v in [0u32, 32, 0, 512, 12, 12, 8] {
            psf2.extend(v.to_le_bytes());
        }
        psf2.extend((0..512 * 12).map(|i| (i / 12) as u8));
        let f = parse(&psf2).unwrap();
        assert_eq!((f.len(), f[14 * 65]), (256 * 14, 65));
        // 9 pixels wide, or not a font.
        psf2[28] = 9;
        assert!(parse(&psf2).is_err());
        assert!(parse(b"hello").is_err());
    }
}
