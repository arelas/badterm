//! The ANSI viewer (Alt-F): a piece of art, scrolled a screen at a time,
//! in its own width, font and colors, with its SAUCE details.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use phosphor_core::screen::{Cell, Screen};

use super::*;
use crate::input::Key;
use crate::viewer::{self, Art};

/// The kinds of file the viewer's picker shows.
pub const EXTENSIONS: &[&str] = &[".ans", ".asc", ".ice", ".nfo", ".diz", ".txt", ".art", ".drk", ".avt"];

/// Rows of art shown at once.
pub const ROWS: usize = 25;

pub enum ArtAction {
    None,
    Close,
    /// Choose another file.
    Open,
}

pub struct ArtView {
    pub art: Art,
    pub path: PathBuf,
    top: usize,
    /// Showing the SAUCE details.
    info: bool,
    /// The rows on show, with the details drawn over them.
    page: Screen,
}

impl ArtView {
    pub fn open(path: &Path, font: u8) -> Result<ArtView> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let art = viewer::load(&data, font, false);
        let cols = art.term.screen.cols();
        let mut v = ArtView { art, path: path.to_path_buf(), top: 0, info: false, page: Screen::new(cols, ROWS, 0) };
        v.refresh();
        Ok(v)
    }

    pub fn cols(&self) -> usize {
        self.art.term.screen.cols()
    }

    fn max_top(&self) -> usize {
        self.art.rows.saturating_sub(ROWS)
    }

    pub fn handle_key(&mut self, key: Key) -> ArtAction {
        let max = self.max_top();
        match key {
            Key::Escape | Key::Char('q') | Key::Char('Q') => return ArtAction::Close,
            Key::Char('o') | Key::Char('O') => return ArtAction::Open,
            Key::Char('i') | Key::Char('I') | Key::Tab => self.info = !self.info,
            Key::Up => self.top = self.top.saturating_sub(1),
            Key::Down | Key::Enter => self.top = (self.top + 1).min(max),
            Key::PageUp => self.top = self.top.saturating_sub(ROWS - 1),
            Key::PageDown | Key::Char(' ') => self.top = (self.top + ROWS - 1).min(max),
            Key::Home => self.top = 0,
            Key::End => self.top = max,
            _ => return ArtAction::None,
        }
        self.refresh();
        ArtAction::None
    }

    /// Scroll by `lines` (the mouse wheel: up is positive).
    pub fn scroll(&mut self, lines: isize) {
        self.top = (self.top as isize - lines).clamp(0, self.max_top() as isize) as usize;
        self.refresh();
    }

    fn refresh(&mut self) {
        let sc = &self.art.term.screen;
        for y in 0..ROWS {
            let blank = vec![Cell::default(); sc.cols()];
            let row = if self.top + y < sc.rows() { sc.row(self.top + y) } else { &blank };
            for (x, c) in row.iter().enumerate() {
                if let Some(cell) = self.page.cell_mut(x, y) {
                    *cell = *c;
                }
            }
        }
        if self.info {
            self.draw_info();
        }
    }

    /// What is on screen: the art's rows, perhaps with the details over them.
    pub fn rows(&self) -> Vec<&[Cell]> {
        (0..ROWS).map(|y| self.page.row(y)).collect()
    }

    pub fn status(&self) -> String {
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let what = match &self.art.sauce {
            Some(s) if !s.title.is_empty() && !s.author.is_empty() => format!("{} by {}", s.title, s.author),
            Some(s) if !s.title.is_empty() => s.title.clone(),
            _ => name,
        };
        let shown = (self.top + ROWS).min(self.art.rows);
        format!(
            " {} \u{2502} {}-{} of {} \u{2502} I info  O open  Esc close",
            truncate(&what, 40),
            self.top + 1,
            shown,
            self.art.rows
        )
    }

    fn draw_info(&mut self) {
        let t = theme::current();
        let mut lines: Vec<(String, String)> = Vec::new();
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        lines.push(("File".into(), name));
        match &self.art.sauce {
            None => lines.push(("SAUCE".into(), "none: no title or artist recorded".into())),
            Some(s) => {
                for (k, v) in [("Title", &s.title), ("Artist", &s.author), ("Group", &s.group)] {
                    if !v.is_empty() {
                        lines.push((k.into(), v.clone()));
                    }
                }
                if let Some(d) = s.date_text() {
                    lines.push(("Date".into(), d));
                }
                match (s.width, s.lines) {
                    (0, _) => {}
                    (w, 0) => lines.push(("Size".into(), format!("{w} columns"))),
                    (w, h) => lines.push(("Size".into(), format!("{w} x {h}"))),
                }
                if !s.font.is_empty() {
                    lines.push(("Font".into(), s.font.clone()));
                }
                lines.push(("iCE colors".into(), if s.ice { "yes" } else { "no" }.into()));
                for (i, c) in s.comments.iter().take(6).enumerate() {
                    lines.push((if i == 0 { "Comments".into() } else { String::new() }, c.clone()));
                }
            }
        }
        lines.push(("Lines".into(), self.art.rows.to_string()));
        let w = 70.min(self.page.cols() - 2);
        let h = lines.len() + 4;
        let (x, y) = ((self.page.cols() - w) / 2, (ROWS.saturating_sub(h)) / 2);
        let p = &mut self.page;
        frame(p, x, y, w, h, "SAUCE", t.window.fg(), t.window.bg());
        for (i, (k, v)) in lines.iter().enumerate() {
            text(p, x + 2, y + 2 + i, 12, &format!("{k:>11}"), t.field_name.fg(), t.field_name.bg());
            text(p, x + 15, y + 2 + i, w - 17, &truncate(v, w - 17), t.field_value.fg(), t.field_value.bg());
        }
    }
}

/// The picker the viewer opens with: a full page, like the settings.
pub fn draw_picker(s: &mut Screen, picker: &mut files::FilePicker) -> Option<(usize, usize)> {
    let t = theme::current();
    let cols = s.cols();
    s.fill_rect(0, 0, cols, s.rows(), Cell::new(b' ', t.help.fg(), t.help.bg()));
    let title = format!(" Phosphor {} \u{2502} View ANSI Art", env!("CARGO_PKG_VERSION"));
    text(s, 0, 0, cols, &title, t.title_bar.fg(), t.title_bar.bg());
    frame(s, 2, 2, cols - 4, 21, "Choose a file", t.window.fg(), t.window.bg());
    let cursor = picker.draw(s, 4, 3, cols - 8, 18);
    text(s, 1, 24, cols - 2, "Enter view   Bksp up   Tab type a path   Esc cancel", t.help.fg(), t.help.bg());
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolls_and_shows_details() {
        let dir = std::env::temp_dir().join(format!("phosphor-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tall.ans");
        let mut data = Vec::new();
        for i in 0..60 {
            data.extend_from_slice(format!("\x1b[1;3{}mLine {i}\r\n", i % 7 + 1).as_bytes());
        }
        std::fs::write(&path, &data).unwrap();
        let mut v = ArtView::open(&path, 0).unwrap();
        assert_eq!(v.art.rows, 60);
        assert_eq!(Screen::row_text(v.rows()[0]), "Line 0");
        v.handle_key(Key::PageDown);
        assert_eq!(Screen::row_text(v.rows()[0]), "Line 24");
        v.handle_key(Key::End);
        assert_eq!(Screen::row_text(v.rows()[0]), "Line 35");
        assert!(v.status().contains("36-60 of 60"));
        v.handle_key(Key::Char('i'));
        let text: Vec<String> = v.rows().iter().map(|r| Screen::row_text(r)).collect();
        assert!(text.iter().any(|l| l.contains("SAUCE") || l.contains("none")), "{text:#?}");
        assert!(matches!(v.handle_key(Key::Escape), ArtAction::Close));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
