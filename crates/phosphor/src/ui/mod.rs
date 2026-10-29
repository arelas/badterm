//! Built-in text-mode user interface, drawn with CP437 glyphs into a
//! [`Screen`] so it renders exactly like BBS content.

pub mod dialer;
pub mod transfer;

use phosphor_core::cp437;
use std::path::PathBuf;

use phosphor_core::screen::{Cell, Color, Screen};

use crate::input::{Key, Mods};

pub const BLACK: Color = Color::Pc(0);
pub const BLUE: Color = Color::Pc(1);
pub const CYAN: Color = Color::Pc(3);
pub const GRAY: Color = Color::Pc(7);
pub const DARK_GRAY: Color = Color::Pc(8);
pub const LIGHT_CYAN: Color = Color::Pc(11);
pub const YELLOW: Color = Color::Pc(14);
pub const WHITE: Color = Color::Pc(15);
pub const LIGHT_RED: Color = Color::Pc(12);

/// Draw `text` at (x, y), truncated or space-padded to `width` cells.
pub fn text(s: &mut Screen, x: usize, y: usize, width: usize, text: &str, fg: Color, bg: Color) {
    let mut bytes = cp437::encode(text);
    bytes.resize(width, b' ');
    s.draw_text(x, y, &bytes, fg, bg);
}

/// Draw `text` without padding.
pub fn label(s: &mut Screen, x: usize, y: usize, text: &str, fg: Color, bg: Color) {
    s.draw_text(x, y, &cp437::encode(text), fg, bg);
}

/// Draw a double-line box with a filled interior and an optional title.
pub fn frame(s: &mut Screen, x: usize, y: usize, w: usize, h: usize, title: &str, fg: Color, bg: Color) {
    s.fill_rect(x, y, w, h, Cell::new(b' ', fg, bg));
    let put = |s: &mut Screen, xx, yy, ch| s.draw_text(xx, yy, &[ch], fg, bg);
    for xx in x + 1..x + w - 1 {
        put(s, xx, y, 0xCD);
        put(s, xx, y + h - 1, 0xCD);
    }
    for yy in y + 1..y + h - 1 {
        put(s, x, yy, 0xBA);
        put(s, x + w - 1, yy, 0xBA);
    }
    put(s, x, y, 0xC9);
    put(s, x + w - 1, y, 0xBB);
    put(s, x, y + h - 1, 0xC8);
    put(s, x + w - 1, y + h - 1, 0xBC);
    if !title.is_empty() {
        let t = format!(" {title} ");
        let tx = x + (w.saturating_sub(t.chars().count())) / 2;
        label(s, tx, y, &t, YELLOW, bg);
    }
}

/// Draw a horizontal divider across a frame at row `y` (╟───╢).
pub fn divider(s: &mut Screen, x: usize, y: usize, w: usize, fg: Color, bg: Color) {
    let mut line = vec![0xC4; w];
    line[0] = 0xC7;
    line[w - 1] = 0xB6;
    s.draw_text(x, y, &line, fg, bg);
}

/// A single-line text input.
#[derive(Clone, Debug, Default)]
pub struct TextField {
    pub value: String,
    /// Cursor position in characters.
    pub cursor: usize,
    pub max_len: usize,
}

impl TextField {
    pub fn new(value: &str, max_len: usize) -> TextField {
        TextField { value: value.to_string(), cursor: value.chars().count(), max_len }
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.value.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(self.value.len())
    }

    /// Returns true if the key was consumed.
    pub fn handle_key(&mut self, key: Key, mods: Mods) -> bool {
        let len = self.value.chars().count();
        match key {
            Key::Char(c) if !mods.ctrl && !c.is_control() => {
                if len < self.max_len {
                    let i = self.byte_index(self.cursor);
                    self.value.insert(i, c);
                    self.cursor += 1;
                }
            }
            // Ctrl+U clears, as in many line editors.
            Key::Char('u') if mods.ctrl => {
                self.value.clear();
                self.cursor = 0;
            }
            Key::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let i = self.byte_index(self.cursor);
                self.value.remove(i);
            }
            Key::Delete if self.cursor < len => {
                let i = self.byte_index(self.cursor);
                self.value.remove(i);
            }
            Key::Left => self.cursor = self.cursor.saturating_sub(1),
            Key::Right => self.cursor = (self.cursor + 1).min(len),
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = len,
            Key::Backspace | Key::Delete => {}
            _ => return false,
        }
        true
    }

    /// Draw the field in a `width`-cell box, scrolling horizontally to keep
    /// the cursor visible. Returns the screen position of the cursor.
    pub fn draw(
        &self,
        s: &mut Screen,
        x: usize,
        y: usize,
        width: usize,
        secret: bool,
        fg: Color,
        bg: Color,
    ) -> (usize, usize) {
        let start = (self.cursor + 1).saturating_sub(width);
        let shown: String = if secret {
            "*".repeat(self.value.chars().count().saturating_sub(start).min(width))
        } else {
            self.value.chars().skip(start).take(width).collect()
        };
        text(s, x, y, width, &shown, fg, bg);
        (x + self.cursor - start, y)
    }
}

/// Expand a leading `~/` to the home directory.
pub fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = directories::BaseDirs::new()
    {
        return home.home_dir().join(rest);
    }
    PathBuf::from(p)
}

/// Format a Unix timestamp as YYYY-MM-DD (UTC).
pub fn format_date(ts: u64) -> String {
    // Civil-from-days, after Howard Hinnant's algorithm.
    let z = (ts / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(951_782_400), "2000-02-29");
        assert_eq!(format_date(1_793_232_000), "2026-10-29");
    }

    #[test]
    fn text_field_editing() {
        let m = Mods::default();
        let mut f = TextField::new("hllo", 10);
        f.handle_key(Key::Home, m);
        f.handle_key(Key::Right, m);
        f.handle_key(Key::Char('e'), m);
        assert_eq!(f.value, "hello");
        f.handle_key(Key::End, m);
        f.handle_key(Key::Backspace, m);
        assert_eq!(f.value, "hell");
    }
}
