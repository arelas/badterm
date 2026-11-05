//! Built-in text-mode user interface, drawn with CP437 glyphs into a
//! [`Screen`] so it renders exactly like BBS content.

pub mod art;
pub mod dialer;
pub mod files;
pub mod help;
pub mod login;
pub mod macro_edit;
pub mod save;
pub mod settings;
pub mod splash;
pub mod theme;
pub mod transfer;

use badterm_core::cp437;
use std::path::{Path, PathBuf};

use badterm_core::screen::{Cell, Color, Screen};

use crate::input::{Key, Mods};

/// The modifier for BadTerm's own hotkeys, as shown in help text.
pub const HOTKEY: &str = if cfg!(target_os = "macos") { "Cmd" } else { "Alt" };

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

/// A centered box with a title and a message, wrapped to fit.
pub fn message_box(s: &mut Screen, title: &str, msg: &str) {
    let lines = wrap(msg, 60);
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(title.len() + 4) + 6;
    let h = lines.len() + 4;
    let x = s.cols().saturating_sub(w) / 2;
    let y = s.rows().saturating_sub(h) / 2;
    let t = theme::current();
    frame(s, x, y, w, h, title, t.window.fg(), t.window.bg());
    for (i, l) in lines.iter().enumerate() {
        label(s, x + 3, y + 2 + i, l, t.window.fg(), t.window.bg());
    }
}

/// A question in a box: the message (if any), then the choices on a row of
/// their own, each key highlighted, e.g. `Y Exit    N Stay`.
pub fn confirm_box(s: &mut Screen, title: &str, msg: &str, choices: &[(&str, &str)]) {
    let lines = if msg.is_empty() { Vec::new() } else { wrap(msg, 56) };
    let gap = "    ";
    let row_len: usize = choices.iter().map(|(k, what)| k.chars().count() + 1 + what.chars().count()).sum::<usize>()
        + gap.len() * choices.len().saturating_sub(1);
    let text_w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(row_len).max(title.len() + 4);
    let w = text_w + 8;
    // The border, a blank row, the message and another blank row (if
    // there is a message), the choices, a blank row and the border.
    let h = if lines.is_empty() { 5 } else { lines.len() + 6 };
    let x = s.cols().saturating_sub(w) / 2;
    let y = s.rows().saturating_sub(h) / 2;
    let t = theme::current();
    let (fg, bg) = (t.window.fg(), t.window.bg());
    frame(s, x, y, w, h, title, fg, bg);
    for (i, l) in lines.iter().enumerate() {
        label(s, x + (w - l.chars().count()) / 2, y + 2 + i, l, fg, bg);
    }
    let row_y = if lines.is_empty() { y + 2 } else { y + 3 + lines.len() };
    let mut cx = x + (w - row_len) / 2;
    for (i, (key, what)) in choices.iter().enumerate() {
        if i > 0 {
            cx += gap.len();
        }
        label(s, cx, row_y, key, t.keys.fg(), bg);
        cx += key.chars().count() + 1;
        label(s, cx, row_y, what, fg, bg);
        cx += what.chars().count();
    }
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
        label(s, tx, y, &t, theme::current().window_title.fg(), bg);
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

/// Word-wrap `s` into lines of at most `width` characters.
pub fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&truncate(word, width));
    }
    lines.push(cur);
    lines
}

/// Shorten `s` to `n` characters, marking the cut with `»`.
pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('\u{00BB}');
        t
    }
}

/// Expand a leading `~/` to the home directory.
pub fn expand_home(p: &str) -> PathBuf {
    if let Some(home) = directories::BaseDirs::new() {
        if p == "~" {
            return home.home_dir().to_path_buf();
        }
        if let Some(rest) = p.strip_prefix("~/") {
            return home.home_dir().join(rest);
        }
    }
    PathBuf::from(p)
}

/// A path as someone would type it: `~/...` under the home folder.
pub fn tidy_path(p: &Path) -> String {
    if let Some(home) = directories::BaseDirs::new()
        && let Ok(rest) = p.strip_prefix(home.home_dir())
        && !rest.as_os_str().is_empty()
    {
        return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
    }
    p.display().to_string()
}

/// [`tidy_path`], or just the part after `base`, for a path inside it.
pub fn tidy_path_in(p: &Path, base: &Path) -> String {
    match p.strip_prefix(base) {
        Ok(rest) if !rest.as_os_str().is_empty() => rest.display().to_string(),
        _ => tidy_path(p),
    }
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
