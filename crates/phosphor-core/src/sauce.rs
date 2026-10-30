//! SAUCE, the record at the end of most ANSI art: title, artist, group,
//! date, the width it was drawn at, its font and whether it uses iCE
//! colors, plus any comment lines (<https://www.acid.org/info/sauce/sauce.htm>).

use crate::cp437;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sauce {
    pub title: String,
    pub author: String,
    pub group: String,
    /// CCYYMMDD.
    pub date: String,
    /// Columns (character art), 0 if not given.
    pub width: u16,
    /// Lines, 0 if not given.
    pub lines: u16,
    /// The blink bit is a bright background ("non-blink mode").
    pub ice: bool,
    /// 9-pixel letter spacing asked for.
    pub nine_dot: bool,
    /// The font's name, e.g. "IBM VGA" or "Amiga Topaz 2+".
    pub font: String,
    pub comments: Vec<String>,
}

impl Sauce {
    /// The date as YYYY-MM-DD, if it looks like one.
    pub fn date_text(&self) -> Option<String> {
        let d = self.date.trim();
        (d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]))
    }

    /// The CTerm font ID for the font named, if Phosphor has it (0 for the
    /// IBM fonts).
    pub fn cterm_font(&self) -> u8 {
        let f = self.font.to_ascii_lowercase();
        match () {
            _ if f.contains("topaz") && f.contains('+') => 40,
            _ if f.contains("topaz") => 42,
            _ if f.contains("microknight") && f.contains('+') => 39,
            _ if f.contains("microknight") => 41,
            _ if f.contains("noodle") => 37,
            _ if f.contains("mosoul") || f.contains("mo'soul") => 38,
            _ => 0,
        }
    }
}

/// Split `data` into the art (without the SAUCE record, its comments, and
/// the Ctrl-Z before them) and the record, if there is one.
pub fn split(data: &[u8]) -> (&[u8], Option<Sauce>) {
    let mut end = data.len();
    let mut sauce = None;
    if data.len() >= 128 && data[data.len() - 128..].starts_with(b"SAUCE00") {
        let r = &data[data.len() - 128..];
        end = data.len() - 128;
        let field = |a: usize, b: usize| cp437::decode(&r[a..b]).trim_end_matches(['\0', ' ']).to_string();
        let u16_at = |a: usize| u16::from_le_bytes([r[a], r[a + 1]]);
        let (data_type, flags) = (r[94], r[105]);
        let mut s = Sauce {
            title: field(7, 42),
            author: field(42, 62),
            group: field(62, 82),
            date: field(82, 90),
            font: field(106, 128),
            ..Sauce::default()
        };
        // Character and binary-text art have widths and the flags.
        if matches!(data_type, 1 | 5) {
            s.width = if data_type == 5 { u16::from(r[95]) * 2 } else { u16_at(96) };
            s.lines = if data_type == 5 { 0 } else { u16_at(98) };
            s.ice = flags & 1 != 0;
            s.nine_dot = (flags >> 1) & 3 == 2;
        }
        // Comments: "COMNT" and 64 bytes per line, just before the record.
        let n = r[104] as usize;
        let block = 5 + 64 * n;
        if n > 0 && end >= block && data[end - block..].starts_with(b"COMNT") {
            s.comments = data[end - block + 5..end]
                .chunks(64)
                .map(|c| cp437::decode(c).trim_end_matches(['\0', ' ']).to_string())
                .collect();
            end -= block;
        }
        sauce = Some(s);
    }
    // Ctrl-Z ends the art in files with SAUCE or comments.
    if let Some(z) = data[..end].iter().position(|&b| b == 0x1A) {
        end = z;
    }
    (&data[..end], sauce)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sauce_record(title: &str, author: &str, width: u16, flags: u8, font: &str, comments: u8) -> Vec<u8> {
        let mut r = vec![0u8; 128];
        r[..7].copy_from_slice(b"SAUCE00");
        r[7..7 + title.len()].copy_from_slice(title.as_bytes());
        r[42..42 + author.len()].copy_from_slice(author.as_bytes());
        r[82..90].copy_from_slice(b"19960314");
        r[94] = 1;
        r[95] = 1;
        r[96..98].copy_from_slice(&width.to_le_bytes());
        r[98..100].copy_from_slice(&25u16.to_le_bytes());
        r[104] = comments;
        r[105] = flags;
        r[106..106 + font.len()].copy_from_slice(font.as_bytes());
        r
    }

    #[test]
    fn reads_the_record_and_comments() {
        let mut file = b"\x1b[1;31mART\x1b[0m".to_vec();
        let art_len = file.len();
        file.push(0x1A);
        file.extend_from_slice(b"COMNT");
        let mut line = b"Greets to the scene".to_vec();
        line.resize(64, b' ');
        file.extend_from_slice(&line);
        file.extend(sauce_record("Big Logo", "artist", 80, 1 | 4, "Amiga Topaz 2+", 1));
        let (art, sauce) = split(&file);
        assert_eq!(art.len(), art_len);
        let s = sauce.unwrap();
        assert_eq!((s.title.as_str(), s.author.as_str(), s.width, s.lines), ("Big Logo", "artist", 80, 25));
        assert!(s.ice && s.nine_dot);
        assert_eq!(s.cterm_font(), 40);
        assert_eq!(s.date_text().as_deref(), Some("1996-03-14"));
        assert_eq!(s.comments, vec!["Greets to the scene".to_string()]);
        assert_eq!(split(b"plain"), (&b"plain"[..], None));
    }
}
