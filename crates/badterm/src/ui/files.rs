//! A file picker in text mode: folders first, then files with their sizes;
//! Enter opens a folder or chooses a file, Space marks several, Backspace
//! goes up, and Tab switches to typing a path.

use std::path::{Path, PathBuf};

use badterm_core::screen::Screen;

use super::transfer::format_bytes;
use super::*;
use crate::input::{Key, Mods};

pub enum PickerAction {
    None,
    /// The files chosen (at least one).
    Chosen(Vec<PathBuf>),
}

struct Item {
    name: String,
    dir: bool,
    size: u64,
}

pub struct FilePicker {
    dir: PathBuf,
    items: Vec<Item>,
    selected: usize,
    top: usize,
    /// Choosing several at once (uploads).
    multi: bool,
    marked: Vec<PathBuf>,
    /// Typing a path instead of browsing.
    typing: Option<TextField>,
    error: Option<String>,
    /// Only files whose names end in one of these (lower case), if any.
    extensions: &'static [&'static str],
}

impl FilePicker {
    pub fn new(dir: PathBuf, multi: bool, extensions: &'static [&'static str]) -> FilePicker {
        let mut p = FilePicker {
            dir: PathBuf::new(),
            items: Vec::new(),
            selected: 0,
            top: 0,
            multi,
            marked: Vec::new(),
            typing: None,
            error: None,
            extensions,
        };
        p.open(dir);
        p
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Show `dir`, or say why it can't be read (and stay put).
    fn open(&mut self, dir: PathBuf) {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                self.error = Some(format!("{}: {e}", dir.display()));
                if self.dir.as_os_str().is_empty() {
                    self.dir = dir;
                }
                return;
            }
        };
        let mut items: Vec<Item> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    return None;
                }
                // Follow links, so a link to a folder is a folder.
                let meta = std::fs::metadata(e.path()).ok()?;
                let dir = meta.is_dir();
                let lower = name.to_ascii_lowercase();
                if !dir && !self.extensions.is_empty() && !self.extensions.iter().any(|x| lower.ends_with(x)) {
                    return None;
                }
                Some(Item { name, dir, size: meta.len() })
            })
            .collect();
        items.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        if dir.parent().is_some() {
            items.insert(0, Item { name: "..".into(), dir: true, size: 0 });
        }
        // Coming up out of a folder: select it.
        let came_from = self.dir.file_name().map(|n| n.to_string_lossy().into_owned());
        self.selected = came_from
            .filter(|_| self.dir.parent() == Some(dir.as_path()))
            .and_then(|n| items.iter().position(|i| i.name == n))
            .unwrap_or(0);
        self.items = items;
        self.top = 0;
        self.dir = dir;
        self.error = None;
    }

    fn up(&mut self) {
        if let Some(parent) = self.dir.parent() {
            self.open(parent.to_path_buf());
        }
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods) -> PickerAction {
        if let Some(field) = &mut self.typing {
            match key {
                Key::Escape | Key::Tab => self.typing = None,
                Key::Enter => {
                    let path = expand_home(field.value.trim().trim_matches('"'));
                    self.typing = None;
                    if path.is_dir() {
                        self.open(path);
                    } else if path.is_file() {
                        return PickerAction::Chosen(vec![path]);
                    } else {
                        self.error = Some(format!("{} does not exist", path.display()));
                    }
                }
                _ => {
                    field.handle_key(key, mods);
                }
            }
            return PickerAction::None;
        }
        let n = self.items.len();
        match key {
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Down => self.selected = (self.selected + 1).min(n.saturating_sub(1)),
            Key::PageUp => self.selected = self.selected.saturating_sub(10),
            Key::PageDown => self.selected = (self.selected + 10).min(n.saturating_sub(1)),
            Key::Home => self.selected = 0,
            Key::End => self.selected = n.saturating_sub(1),
            Key::Backspace => self.up(),
            Key::Tab => {
                let mut start = self.dir.to_string_lossy().into_owned();
                if !start.ends_with(std::path::MAIN_SEPARATOR) {
                    start.push(std::path::MAIN_SEPARATOR);
                }
                self.typing = Some(TextField::new(&start, 1000));
            }
            Key::Char(' ') if self.multi => {
                if let Some(item) = self.items.get(self.selected).filter(|i| !i.dir) {
                    let path = self.dir.join(&item.name);
                    match self.marked.iter().position(|p| *p == path) {
                        Some(i) => {
                            self.marked.remove(i);
                        }
                        None => self.marked.push(path),
                    }
                    self.selected = (self.selected + 1).min(n.saturating_sub(1));
                }
            }
            Key::Enter => {
                let Some(item) = self.items.get(self.selected) else { return PickerAction::None };
                if item.name == ".." {
                    self.up();
                } else if item.dir {
                    self.open(self.dir.join(&item.name));
                } else if !self.marked.is_empty() {
                    return PickerAction::Chosen(std::mem::take(&mut self.marked));
                } else {
                    return PickerAction::Chosen(vec![self.dir.join(&item.name)]);
                }
            }
            // Jump to the next name starting with the letter typed.
            Key::Char(c) if c.is_alphanumeric() => {
                let c = c.to_ascii_lowercase();
                let starts = |i: &Item| i.name.to_lowercase().starts_with(c);
                if let Some(i) = (self.selected + 1..n).chain(0..=self.selected).find(|&i| starts(&self.items[i])) {
                    self.selected = i;
                }
            }
            _ => {}
        }
        PickerAction::None
    }

    /// Draw the list in (x, y, w, h), inside a window drawn by the caller.
    /// Returns the cursor while a path is being typed.
    pub fn draw(&mut self, s: &mut Screen, x: usize, y: usize, w: usize, h: usize) -> Option<(usize, usize)> {
        let t = theme::current();
        let path = self.dir.to_string_lossy();
        let shown: String = if path.chars().count() > w {
            format!("\u{2026}{}", path.chars().skip(path.chars().count() + 1 - w).collect::<String>())
        } else {
            path.into_owned()
        };
        text(s, x, y, w, &shown, t.window_title.fg(), t.window_title.bg());
        if let Some(field) = &self.typing {
            text(s, x, y + 1, w, "Type a folder or file, Enter to go there:", t.window_text.fg(), t.window_text.bg());
            return Some(field.draw(s, x, y + 2, w, false, t.input.fg(), t.input.bg()));
        }
        let rows = h.saturating_sub(1);
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + rows {
            self.top = self.selected + 1 - rows;
        }
        for row in 0..rows {
            let yy = y + 1 + row;
            let Some(item) = self.items.get(self.top + row) else {
                text(s, x, yy, w, "", t.entries.fg(), t.entries.bg());
                continue;
            };
            let marked = !item.dir && self.marked.contains(&self.dir.join(&item.name));
            let mark = if marked { '\u{221A}' } else { ' ' };
            let size = if item.dir { "<folder>".to_string() } else { format_bytes(item.size) };
            let name_w = w.saturating_sub(13);
            let line = format!("{mark}{:<name_w$} {size:>10}", truncate(&item.name, name_w));
            let a = if self.top + row == self.selected { t.selected } else { t.entries };
            text(s, x, yy, w, &line, a.fg(), a.bg());
        }
        if self.items.is_empty() {
            text(s, x, y + 1, w, " (empty)", t.entries.fg(), t.entries.bg());
        }
        if let Some(e) = &self.error {
            text(s, x, y + h - 1, w, &truncate(e, w), t.error.fg(), t.error.bg());
        }
        None
    }

    /// How many files are marked.
    pub fn marked(&self) -> usize {
        self.marked.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(p: &mut FilePicker, k: Key) -> PickerAction {
        p.handle_key(k, Mods::default())
    }

    #[test]
    fn browse_mark_and_choose() {
        let dir = std::env::temp_dir().join(format!("badterm-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["b.zip", "A.txt", ".hidden"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        std::fs::write(dir.join("sub").join("inner.ans"), b"x").unwrap();

        let mut p = FilePicker::new(dir.clone(), true, &[]);
        let names: Vec<&str> = p.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["..", "sub", "A.txt", "b.zip"]);
        // Into the folder and back out, which selects it again.
        key(&mut p, Key::Down);
        key(&mut p, Key::Enter);
        assert_eq!(p.dir(), dir.join("sub"));
        key(&mut p, Key::Backspace);
        assert_eq!(p.items[p.selected].name, "sub");
        // Mark both files; Enter sends the marked ones.
        key(&mut p, Key::Char('a'));
        key(&mut p, Key::Char(' '));
        key(&mut p, Key::Char(' '));
        assert_eq!(p.marked(), 2);
        let mut s = Screen::new(80, 25, 0);
        p.draw(&mut s, 2, 2, 60, 15);
        match key(&mut p, Key::Enter) {
            PickerAction::Chosen(v) => assert_eq!(v, vec![dir.join("A.txt"), dir.join("b.zip")]),
            PickerAction::None => panic!(),
        }
        // Typing a path.
        key(&mut p, Key::Tab);
        for c in "sub/inner.ans".chars() {
            key(&mut p, Key::Char(c));
        }
        match key(&mut p, Key::Enter) {
            PickerAction::Chosen(v) => assert_eq!(v, vec![dir.join("sub").join("inner.ans")]),
            PickerAction::None => panic!(),
        }
        // Only some kinds of file.
        let p = FilePicker::new(dir.join("sub"), false, &[".zip"]);
        assert_eq!(p.items.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
