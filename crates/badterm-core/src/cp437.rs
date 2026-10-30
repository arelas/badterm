//! IBM code page 437 <-> Unicode conversion.
//!
//! Glyph indices on screen are always CP437 bytes; these tables are used for
//! UTF-8 sessions, keyboard input and copying text out of the terminal.

/// Unicode equivalent of every CP437 glyph, including the "graphic" control
/// range 0x00-0x1F shown by the PC video BIOS.
#[rustfmt::skip]
pub const TO_UNICODE: [char; 256] = [
    '\u{0000}', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼',
    '►', '◄', '↕', '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼',
    ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?',
    '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '\\', ']', '^', '_',
    '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '{', '|', '}', '~', '⌂',
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å',
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ',
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»',
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐',
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧',
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀',
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩',
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{00A0}',
];

/// Convert a CP437 glyph to Unicode.
pub fn to_unicode(b: u8) -> char {
    TO_UNICODE[b as usize]
}

/// Convert a Unicode scalar to its CP437 glyph, if one exists.
///
/// ASCII maps to itself; a few common look-alikes (e.g. U+00B7 vs U+2219) are
/// accepted in both directions.
pub fn from_unicode(c: char) -> Option<u8> {
    if (c as u32) < 0x80 {
        return Some(c as u8);
    }
    match c {
        '\u{00B7}' => return Some(0xFA),
        '\u{03B2}' => return Some(0xE1),
        '\u{03A3}' => return Some(0xE4),
        '\u{220E}' | '\u{25A0}' => return Some(0xFE),
        _ => {}
    }
    TO_UNICODE[0x80..]
        .iter()
        .position(|&u| u == c)
        .map(|i| (i + 0x80) as u8)
        .or_else(|| TO_UNICODE[1..0x20].iter().position(|&u| u == c).map(|i| (i + 1) as u8))
}

/// Decode a CP437 byte string into a Unicode `String`.
pub fn decode(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| to_unicode(b)).collect()
}

/// Encode a string as CP437, replacing unmappable characters with `?`.
pub fn encode(s: &str) -> Vec<u8> {
    s.chars().map(|c| from_unicode(c).unwrap_or(b'?')).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_high_half() {
        for b in 0x80..=0xFFu8 {
            assert_eq!(from_unicode(to_unicode(b)), Some(b), "byte {b:#x}");
        }
    }

    #[test]
    fn ascii_identity() {
        assert_eq!(encode("Hello"), b"Hello");
        assert_eq!(decode(&[0xDB, 0xB0]), "█░");
    }
}
