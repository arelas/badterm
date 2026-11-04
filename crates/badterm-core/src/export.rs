//! Turning screen contents back into files or clipboard text: plain text,
//! and ANSI as BBS art viewers and editors expect it (CP437 bytes for `.ans`
//! files, or Unicode for pasting).

use crate::codepage::CodePage;
use crate::cp437;
use crate::palette::ANSI_TO_VGA;
use crate::screen::{Cell, Color, DEFAULT_BG, DEFAULT_FG, flags};

/// Plain text, one line per row, trailing blanks and blank lines trimmed.
pub fn text(lines: &[&[Cell]]) -> String {
    text_in(lines, None)
}

/// [`text`], from a screen in a code page other than CP437.
pub fn text_in(lines: &[&[Cell]], codepage: Option<&CodePage>) -> String {
    let mut out: Vec<String> = lines
        .iter()
        .map(|row| row.iter().map(|c| glyph(c.ch, codepage)).collect::<String>().trim_end().to_string())
        .collect();
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// Options for [`ansi`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AnsiOptions {
    /// The art uses iCE colors (blink means bright background): start with
    /// `CSI ?33h` so viewers that know it switch modes.
    pub ice: bool,
    /// Write glyphs as UTF-8 instead of CP437 bytes (for the clipboard).
    pub unicode: bool,
    /// The code page the screen is in, if not CP437 (for `unicode`).
    pub codepage: Option<&'static CodePage>,
}

/// ANSI with SGR attributes, one CRLF-terminated line per row. Trailing
/// blanks on each line and trailing blank lines are left out.
pub fn ansi(lines: &[&[Cell]], opts: AnsiOptions) -> Vec<u8> {
    let mut out = Vec::new();
    if opts.ice {
        out.extend_from_slice(b"\x1b[?33h");
    }
    out.extend_from_slice(b"\x1b[0m");
    let last = lines.iter().rposition(|row| row.iter().any(|c| !blank(c))).map_or(0, |i| i + 1);
    let default = Cell::default();
    for row in &lines[..last] {
        let end = row.iter().rposition(|c| !blank(c)).map_or(0, |i| i + 1);
        let mut pen = default;
        for c in &row[..end] {
            if !same_attrs(c, &pen) {
                sgr(c, &mut out);
                pen = *c;
            }
            if opts.unicode {
                let mut buf = [0; 4];
                out.extend_from_slice(glyph(c.ch, opts.codepage).encode_utf8(&mut buf).as_bytes());
            } else {
                out.push(safe_byte(c.ch));
            }
        }
        if !same_attrs(&pen, &default) {
            out.extend_from_slice(b"\x1b[0m");
        }
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// A glyph as Unicode: in the code page, if there is one and it has a
/// character there (ISO-8859's 0x80-0x9F are controls: CP437's glyph then).
fn glyph(ch: u8, codepage: Option<&CodePage>) -> char {
    match codepage.map(|cp| cp.decode(ch)) {
        // A C64 or Atari screen: every code is a character.
        Some(c) if codepage.is_some_and(|cp| cp.screen_codes()) => {
            if c == '\u{a0}' {
                ' '
            } else {
                c
            }
        }
        _ if ch == 0 => ' ',
        Some(c) if ch >= 0x80 && !c.is_control() && c != '\u{fffd}' => c,
        _ => cp437::to_unicode(ch),
    }
}

/// CP437 glyphs that would act as control codes in a file are written as
/// spaces: NUL, BEL, BS, TAB, LF, FF, CR, SUB (end of file before a SAUCE
/// record) and ESC.
fn safe_byte(ch: u8) -> u8 {
    match ch {
        0 | 7 | 8 | 9 | 10 | 12 | 13 | 26 | 27 => b' ',
        c => c,
    }
}

fn blank(c: &Cell) -> bool {
    c.is_blank() && c.flags & (flags::UNDERLINE | flags::BLINK) == 0
}

fn same_attrs(a: &Cell, b: &Cell) -> bool {
    a.fg == b.fg && a.bg == b.bg && a.flags == b.flags
}

/// Write the attributes of `c` from scratch (`CSI 0;...m`).
fn sgr(c: &Cell, out: &mut Vec<u8>) {
    let mut p: Vec<String> = vec!["0".into()];
    let f = c.flags;
    // Bright PC colors are written the classic way, as bold, since that is
    // what BBS art viewers understand.
    let bright_fg = matches!(c.fg, Color::Pc(i) if i >= 8);
    for (bit, code) in
        [(flags::BOLD, "1"), (flags::FAINT, "2"), (flags::UNDERLINE, "4"), (flags::BLINK, "5"), (flags::REVERSE, "7")]
    {
        if f & bit != 0 || bit == flags::BOLD && bright_fg {
            p.push(code.into());
        }
    }
    if f & flags::CONCEAL != 0 {
        p.push("8".into());
    }
    match c.fg {
        DEFAULT_FG => {}
        Color::Pc(i) | Color::Ui(i) => p.push(format!("{}", 30 + vga_to_ansi(i & 7))),
        Color::Xterm(i) => p.push(format!("38;5;{i}")),
        Color::Rgb(rgb) => p.push(format!("38;2;{};{};{}", rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF)),
    }
    match c.bg {
        DEFAULT_BG => {}
        Color::Pc(i) | Color::Ui(i) if i >= 8 => p.push(format!("{}", 100 + vga_to_ansi(i & 7))),
        Color::Pc(i) | Color::Ui(i) => p.push(format!("{}", 40 + vga_to_ansi(i & 7))),
        Color::Xterm(i) => p.push(format!("48;5;{i}")),
        Color::Rgb(rgb) => p.push(format!("48;2;{};{};{}", rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF)),
    }
    out.extend_from_slice(format!("\x1b[{}m", p.join(";")).as_bytes());
}

fn vga_to_ansi(i: u8) -> u8 {
    // The mapping swaps red and blue (and cyan and brown), so it is its own
    // inverse.
    ANSI_TO_VGA[i as usize & 7]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::Terminal;

    fn rows(t: &Terminal) -> Vec<&[Cell]> {
        (0..t.screen.rows()).map(|y| t.screen.row(y)).collect()
    }

    #[test]
    fn plain_text() {
        let mut t = Terminal::new(20, 5, 0);
        t.feed(b"\x1b[1;31mHello\x1b[0m  \r\n\r\n\xc9\xcd\xbb\r\n");
        assert_eq!(text(&rows(&t)), "Hello\n\n\u{2554}\u{2550}\u{2557}\n");
    }

    #[test]
    fn ansi_round_trips_through_the_terminal() {
        let mut t = Terminal::new(40, 6, 0);
        t.feed(b"\x1b[1;33;44mBold yellow on blue\x1b[0m plain\r\n");
        t.feed(b"\x1b[5;31;47mblink\x1b[0m \x1b[38;5;196mxterm\x1b[38;2;1;2;3m rgb\x1b[0m\r\n");
        t.feed(b"\x1b[92mbright\x1b[0m \x1b[104mbg\x1b[0m \x1b[4munder\x1b[7mrev\x1b[0m\r\n");
        t.feed(b"\x1b[41m  \x1b[0m\x1b[30C\xdb\x1b[K");
        let data = ansi(&rows(&t), AnsiOptions::default());

        let mut back = Terminal::new(40, 6, 0);
        back.feed(&data);
        for y in 0..6 {
            for x in 0..40 {
                let (a, b) = (t.screen.row(y)[x], back.screen.row(y)[x]);
                assert_eq!(a.ch, b.ch, "glyph at {x},{y}");
                assert_eq!(a.resolve(false), b.resolve(false), "colors at {x},{y}");
            }
        }
    }

    #[test]
    fn options_and_safety() {
        let mut t = Terminal::new(10, 2, 0);
        t.screen.put(0x1B);
        t.screen.put(0x01);
        let raw = ansi(&rows(&t), AnsiOptions { ice: true, unicode: false, codepage: None });
        assert_eq!(raw, b"\x1b[?33h\x1b[0m \x01\r\n");
        let uni = ansi(&rows(&t), AnsiOptions { ice: false, unicode: true, codepage: None });
        assert_eq!(String::from_utf8(uni).unwrap(), "\x1b[0m\u{2190}\u{263A}\r\n");
        // Nothing at all.
        assert_eq!(ansi(&rows(&Terminal::new(10, 2, 0)), AnsiOptions::default()), b"\x1b[0m");
    }

    #[test]
    fn copying_from_a_code_page() {
        let mut t = Terminal::new(20, 2, 0);
        t.feed(&[0x8F, 0xE0, 0xA8, 0xA2, 0xA5, 0xE2, b' ', 0xC9, 0xCD]);
        let cp866 = crate::codepage::for_font(25);
        assert_eq!(text_in(&rows(&t), cp866), "Привет ╔═\n");
        // ISO-8859's controls (0x80-0x9F) are copied as CP437's glyphs.
        let mut t = Terminal::new(4, 1, 0);
        t.feed(&[0x81, 0xE9]);
        assert_eq!(text_in(&rows(&t), crate::codepage::for_font(24)), "üé\n");
    }

    #[test]
    fn copying_a_c64_screen() {
        let mut t = Terminal::new(40, 2, 0);
        t.encoding = crate::ansi::Encoding::Petscii;
        t.start_emulation();
        // "HELLO @" and a spade, in the upper-case set; reversed "OK".
        t.feed(b"HELLO @\xc1 \x12OK\x92\r");
        assert_eq!(text_in(&rows(&t), t.text_codepage()), "HELLO @\u{2660} OK\n");
        // The lower-case set: unshifted letters are small.
        t.feed(b"\x0eHello");
        assert_eq!(text_in(&rows(&t), t.text_codepage()).lines().nth(1), Some("hELLO"));
    }
}
