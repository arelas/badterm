//! A file picker in text mode, for every place BadTerm asks for a file or
//! folder: folders first, then files with their sizes. Enter opens a folder
//! or chooses a file, Backspace goes up, and Tab switches to typing a path.
//!
//! It picks in one of three ways:
//! - **opening** a file that exists (Space marks several, for uploads)
//! - **choosing a folder**: only folders are listed, and the first line,
//!   "Use this folder", chooses the one shown
//! - **saving**: the folders and the files already there, over a name to
//!   save as. Tab moves between the list and the name; picking a file takes
//!   its name; saving over a file that exists takes a second Enter.
//!
//! Choosing a folder and saving can also make a new folder (F7).

use std::path::{Path, PathBuf};

use badterm_core::screen::Screen;

use super::transfer::format_bytes;
use super::*;
use crate::input::{Key, Mods};

pub enum PickerAction {
    None,
    /// The files chosen (at least one), or the folder, or the file to save as.
    Chosen(Vec<PathBuf>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    Open,
    Folder,
    Save,
}

/// The first line when choosing a folder: the folder shown.
const THIS_FOLDER: &str = ".";

struct Item {
    name: String,
    dir: bool,
    size: u64,
}

/// What the line being typed is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Typing {
    Path,
    NewFolder,
}

pub struct FilePicker {
    dir: PathBuf,
    items: Vec<Item>,
    selected: usize,
    top: usize,
    purpose: Purpose,
    /// Choosing several at once (uploads).
    multi: bool,
    marked: Vec<PathBuf>,
    /// Typing a path, or a new folder's name, instead of browsing.
    typing: Option<(Typing, TextField)>,
    error: Option<String>,
    /// Only files whose names end in one of these (lower case), if any.
    extensions: &'static [&'static str],
    /// Saving: the name to save as, and whether keys go to it (not the list).
    name: TextField,
    on_name: bool,
    /// Saving: a file that exists, asked about once; Enter again replaces it.
    replace: Option<PathBuf>,
    /// The folder shown isn't made yet (choosing a folder, or saving): it
    /// will be, when used.
    missing: bool,
}

impl FilePicker {
    /// Opening a file that exists (several, with `multi`), of these kinds
    /// (lower-case endings), or any; starting in `dir` (or the nearest
    /// folder above it that exists).
    pub fn new(dir: PathBuf, multi: bool, extensions: &'static [&'static str]) -> FilePicker {
        let mut p = FilePicker::blank(Purpose::Open, extensions);
        p.multi = multi;
        p.open(existing(&dir));
        p
    }

    /// Choosing a folder, starting at `dir`, which may not be made yet.
    pub fn folder(dir: &Path) -> FilePicker {
        let mut p = FilePicker::blank(Purpose::Folder, &[]);
        p.open(dir.to_path_buf());
        p
    }

    /// Saving a file, as `path` to start with: the list starts in its folder
    /// (which saving makes, if it isn't made yet), the name is its name.
    pub fn save(path: &Path, extensions: &'static [&'static str]) -> FilePicker {
        let mut p = FilePicker::blank(Purpose::Save, extensions);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        p.name = TextField::new(&name, 255);
        p.open(path.parent().unwrap_or(path).to_path_buf());
        p
    }

    fn blank(purpose: Purpose, extensions: &'static [&'static str]) -> FilePicker {
        FilePicker {
            dir: PathBuf::new(),
            items: Vec::new(),
            selected: 0,
            top: 0,
            purpose,
            multi: false,
            marked: Vec::new(),
            typing: None,
            error: None,
            extensions,
            name: TextField::new("", 255),
            on_name: purpose == Purpose::Save,
            replace: None,
            missing: false,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A path or folder name is being typed: Esc goes back to the list,
    /// not out of the picker.
    pub fn is_typing(&self) -> bool {
        self.typing.is_some()
    }

    /// Show `dir`, or say why it can't be read (and stay put).
    fn open(&mut self, dir: PathBuf) {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            // Choosing a folder, or saving into one, that isn't made yet: it
            // can still be chosen (or saved in), and is made then.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && self.purpose != Purpose::Open => {
                let mut items = Vec::new();
                if self.purpose == Purpose::Folder {
                    items.push(Item { name: THIS_FOLDER.into(), dir: true, size: 0 });
                }
                if dir.parent().is_some_and(|p| !p.as_os_str().is_empty()) {
                    items.push(Item { name: "..".into(), dir: true, size: 0 });
                }
                (self.items, self.selected, self.top) = (items, 0, 0);
                (self.dir, self.error, self.replace, self.missing) = (dir, None, None, true);
                return;
            }
            Err(e) => {
                self.error = Some(format!("{}: {e}", dir.display()));
                if self.dir.as_os_str().is_empty() {
                    self.dir = dir;
                }
                return;
            }
        };
        let folders_only = self.purpose == Purpose::Folder;
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
                if !dir && folders_only {
                    return None;
                }
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
        if folders_only {
            items.insert(0, Item { name: THIS_FOLDER.into(), dir: true, size: 0 });
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
        self.replace = None;
        self.missing = false;
    }

    fn up(&mut self) {
        if let Some(parent) = self.dir.parent() {
            self.open(parent.to_path_buf());
        }
    }

    /// Where saving now would write: the name, in the folder shown (or the
    /// path typed as the name, if it is one).
    fn target(&self) -> Option<PathBuf> {
        let name = self.name.value.trim().trim_matches('"');
        if name.is_empty() {
            return None;
        }
        let typed = expand_home(name);
        Some(if typed.is_absolute() { typed } else { self.dir.join(typed) })
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods) -> PickerAction {
        if let Some((kind, field)) = &mut self.typing {
            match key {
                Key::Escape | Key::Tab => self.typing = None,
                Key::Enter => {
                    let (kind, value) = (*kind, field.value.trim().trim_matches('"').to_string());
                    self.typing = None;
                    match kind {
                        Typing::Path => return self.go_to(expand_home(&value)),
                        Typing::NewFolder => self.new_folder(&value),
                    }
                }
                _ => {
                    field.handle_key(key, mods);
                }
            }
            return PickerAction::None;
        }
        if key == Key::F(7) && self.purpose != Purpose::Open {
            self.typing = Some((Typing::NewFolder, TextField::new("", 255)));
            return PickerAction::None;
        }
        if self.purpose == Purpose::Save && self.on_name {
            return self.name_key(key, mods);
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
            Key::Tab if self.purpose == Purpose::Save => self.on_name = true,
            Key::Tab => {
                let mut start = self.dir.to_string_lossy().into_owned();
                if !start.ends_with(std::path::MAIN_SEPARATOR) {
                    start.push(std::path::MAIN_SEPARATOR);
                }
                self.typing = Some((Typing::Path, TextField::new(&start, 1000)));
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
                if item.name == THIS_FOLDER && self.purpose == Purpose::Folder {
                    return PickerAction::Chosen(vec![self.dir.clone()]);
                } else if item.name == ".." {
                    self.up();
                } else if item.dir {
                    self.open(self.dir.join(&item.name));
                } else if self.purpose == Purpose::Save {
                    // Take its name; Enter again saves over it (and asks).
                    self.name = TextField::new(&item.name.clone(), 255);
                    self.on_name = true;
                } else if !self.marked.is_empty() {
                    return PickerAction::Chosen(std::mem::take(&mut self.marked));
                } else {
                    return PickerAction::Chosen(vec![self.dir.join(&item.name)]);
                }
            }
            // Jump to the next name starting with the letter typed.
            Key::Char(c) if c.is_alphanumeric() && n > 0 => {
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

    /// Keys while the name to save as has them.
    fn name_key(&mut self, key: Key, mods: Mods) -> PickerAction {
        match key {
            Key::Tab | Key::Down | Key::Up | Key::PageDown | Key::PageUp => self.on_name = false,
            Key::Enter => {
                let Some(target) = self.target() else {
                    self.error = Some("Type a name to save as".into());
                    return PickerAction::None;
                };
                if target.is_dir() {
                    // A folder typed as the name: go there.
                    self.name = TextField::new("", 255);
                    self.open(target);
                } else if target.exists() && self.replace.as_ref() != Some(&target) {
                    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.error = Some(format!("{} is there already: Enter again to replace it", truncate(&name, 40)));
                    self.replace = Some(target);
                } else if let Some(parent) = target.parent().filter(|p| !p.is_dir())
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    // A folder not made yet is made now.
                    self.error = Some(format!("Could not make {}: {e}", parent.display()));
                } else {
                    return PickerAction::Chosen(vec![target]);
                }
            }
            _ => {
                let before = self.name.value.clone();
                self.name.handle_key(key, mods);
                if self.name.value != before {
                    self.replace = None;
                    self.error = None;
                }
            }
        }
        PickerAction::None
    }

    /// A path typed: open a folder, or choose a file (or, choosing a
    /// folder, any folder, made later if it doesn't exist yet).
    fn go_to(&mut self, path: PathBuf) -> PickerAction {
        match self.purpose {
            Purpose::Folder if path.is_dir() || !path.exists() => PickerAction::Chosen(vec![path]),
            _ if path.is_dir() => {
                self.open(path);
                PickerAction::None
            }
            Purpose::Open if path.is_file() => PickerAction::Chosen(vec![path]),
            Purpose::Save => {
                self.name = TextField::new(&path.to_string_lossy(), 1000);
                self.on_name = true;
                self.name_key(Key::Enter, Mods::default())
            }
            _ => {
                self.error = Some(format!("{} does not exist", path.display()));
                PickerAction::None
            }
        }
    }

    /// Make a folder in the one shown, and go into it.
    fn new_folder(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        let path = self.dir.join(name);
        match std::fs::create_dir_all(&path) {
            Ok(()) => self.open(path),
            Err(e) => self.error = Some(format!("Could not make {name}: {e}")),
        }
    }

    /// Draw the list in (x, y, w, h), inside a window drawn by the caller.
    /// Returns the cursor while something is being typed.
    pub fn draw(&mut self, s: &mut Screen, x: usize, y: usize, w: usize, h: usize) -> Option<(usize, usize)> {
        let t = theme::current();
        let path = self.dir.to_string_lossy();
        let shown: String = if path.chars().count() > w {
            format!("\u{2026}{}", path.chars().skip(path.chars().count() + 1 - w).collect::<String>())
        } else {
            path.into_owned()
        };
        text(s, x, y, w, &shown, t.window_title.fg(), t.window_title.bg());
        if let Some((kind, field)) = &self.typing {
            let prompt = match (kind, self.purpose) {
                (Typing::NewFolder, _) => "A name for the new folder, Enter to make it:",
                (_, Purpose::Folder) => "Type a folder, Enter to choose it:",
                (_, Purpose::Save) => "Type a folder, or a file to save as:",
                _ => "Type a folder or file, Enter to go there:",
            };
            text(s, x, y + 1, w, prompt, t.window_text.fg(), t.window_text.bg());
            return Some(field.draw(s, x, y + 2, w, false, t.input.fg(), t.input.bg()));
        }
        // Saving: the list stops two rows short, for the name.
        let saving = self.purpose == Purpose::Save;
        let rows = h.saturating_sub(if saving { 3 } else { 1 });
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + rows {
            self.top = self.selected + 1 - rows;
        }
        let list_has_keys = !(saving && self.on_name);
        for row in 0..rows {
            let yy = y + 1 + row;
            let Some(item) = self.items.get(self.top + row) else {
                text(s, x, yy, w, "", t.entries.fg(), t.entries.bg());
                continue;
            };
            let marked = !item.dir && self.marked.contains(&self.dir.join(&item.name));
            let mark = if marked { '\u{221A}' } else { ' ' };
            let name_w = w.saturating_sub(13);
            let line = if item.name == THIS_FOLDER {
                format!("{mark}{:<name_w$} {:>10}", "\u{25BA} Use this folder", "")
            } else {
                let size = if item.dir { "<folder>".to_string() } else { format_bytes(item.size) };
                format!("{mark}{:<name_w$} {size:>10}", truncate(&item.name, name_w))
            };
            let a = if self.top + row == self.selected && list_has_keys { t.selected } else { t.entries };
            text(s, x, yy, w, &line, a.fg(), a.bg());
        }
        if self.missing {
            let yy = y + 1 + self.items.len();
            let what =
                if saving { " (a new folder: made when you save)" } else { " (a new folder: made when it's used)" };
            text(s, x, yy, w, what, t.note.fg(), t.note.bg());
        } else if self.items.is_empty() {
            text(s, x, y + 1, w, " (empty)", t.entries.fg(), t.entries.bg());
        }
        let mut cursor = None;
        if saving {
            let yy = y + h - 2;
            let a = if self.on_name { t.keys } else { t.field_name };
            label(s, x, yy, "Save as:", a.fg(), a.bg());
            let a = if self.on_name { t.input } else { t.field_value };
            let pos = self.name.draw(s, x + 9, yy, w.saturating_sub(9), false, a.fg(), a.bg());
            if self.on_name {
                cursor = Some(pos);
            }
        }
        if let Some(e) = &self.error {
            text(s, x, y + h - 1, w, &truncate(e, w), t.error.fg(), t.error.bg());
        }
        cursor
    }

    /// What the keys do now, for the line under the picker; `enter` is what
    /// Enter does with a file (opening) or the name (saving).
    pub fn keys(&self, enter: &str) -> String {
        if self.typing.is_some() {
            return "Enter OK   Esc back to the list".into();
        }
        match self.purpose {
            Purpose::Open => format!("Enter {enter}  Bksp up  Tab type a path  Esc cancel"),
            Purpose::Folder => "Enter open/choose  Bksp up  F7 new folder  Tab type a path  Esc cancel".into(),
            Purpose::Save if self.on_name => format!("Enter {enter}  Tab the folders  F7 new folder  Esc cancel"),
            Purpose::Save => "Enter open/pick  Bksp up  Tab the name  F7 new folder  Esc cancel".into(),
        }
    }

    /// The picker in a window of its own, centred over `s`, titled `title`,
    /// with `note` (what to choose) under the title and the keys at the
    /// bottom. Returns the cursor while something is being typed.
    pub fn window(&mut self, s: &mut Screen, title: &str, note: &str, enter: &str) -> Option<(usize, usize)> {
        let (w, h) = (74, 21);
        let (x, y) = ((s.cols().saturating_sub(w)) / 2, (s.rows().saturating_sub(h)) / 2);
        let t = theme::current();
        frame(s, x, y, w, h, title, t.window.fg(), t.window.bg());
        label(s, x + 2, y + 1, &truncate(note, w - 4), t.window_text.fg(), t.window_text.bg());
        let cursor = self.draw(s, x + 2, y + 3, w - 4, h - 6);
        label(s, x + 2, y + h - 2, &truncate(&self.keys(enter), w - 4), t.keys.fg(), t.keys.bg());
        cursor
    }

    /// How many files are marked.
    pub fn marked(&self) -> usize {
        self.marked.len()
    }
}

/// `dir`, or the nearest folder above it that exists (the home folder if
/// none does).
fn existing(dir: &Path) -> PathBuf {
    let mut d = dir;
    loop {
        if d.is_dir() {
            return d.to_path_buf();
        }
        match d.parent() {
            Some(p) if !p.as_os_str().is_empty() => d = p,
            _ => return expand_home("~"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(p: &mut FilePicker, k: Key) -> PickerAction {
        p.handle_key(k, Mods::default())
    }

    fn typed(p: &mut FilePicker, text: &str) {
        for c in text.chars() {
            key(p, Key::Char(c));
        }
    }

    fn chosen(a: PickerAction) -> Vec<PathBuf> {
        match a {
            PickerAction::Chosen(v) => v,
            PickerAction::None => panic!("nothing chosen"),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("badterm-picker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["b.zip", "A.txt", ".hidden"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        std::fs::write(dir.join("sub").join("inner.ans"), b"x").unwrap();
        dir
    }

    #[test]
    fn browse_mark_and_choose() {
        let dir = scratch("open");
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
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("A.txt"), dir.join("b.zip")]);
        // Typing a path.
        key(&mut p, Key::Tab);
        typed(&mut p, "sub/inner.ans");
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("sub").join("inner.ans")]);
        // Only some kinds of file; a letter in an empty list does nothing.
        let mut p = FilePicker::new(dir.join("sub"), false, &[".zip"]);
        assert_eq!(p.items.len(), 1);
        p.items.clear();
        key(&mut p, Key::Char('z'));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn choose_a_folder() {
        let dir = scratch("folder");
        // A folder that isn't made yet can be chosen as it is, or left by "..".
        let mut p = FilePicker::folder(&dir.join("not").join("yet"));
        assert_eq!(p.dir(), dir.join("not").join("yet"));
        assert!(p.missing);
        let mut s = Screen::new(80, 25, 0);
        p.window(&mut s, "Downloads", "", "choose");
        let rows: Vec<String> = (0..25).map(|y| Screen::row_text(s.row(y))).collect();
        assert!(rows.iter().any(|r| r.contains("a new folder")), "{rows:#?}");
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("not").join("yet")]);
        key(&mut p, Key::Down);
        key(&mut p, Key::Enter);
        assert_eq!(p.dir(), dir.join("not"));
        key(&mut p, Key::Backspace);
        assert_eq!(p.dir(), dir);
        assert!(!p.missing);
        let names: Vec<&str> = p.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, [THIS_FOLDER, "..", "sub"], "folders only");
        // Into sub; its first line chooses it.
        key(&mut p, Key::End);
        key(&mut p, Key::Enter);
        assert_eq!(p.dir(), dir.join("sub"));
        assert_eq!(p.selected, 0);
        p.window(&mut s, "Downloads", "Where files go", "choose");
        let rows: Vec<String> = (0..25).map(|y| Screen::row_text(s.row(y))).collect();
        assert!(rows.iter().any(|r| r.contains("Use this folder")), "{rows:#?}");
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("sub")]);
        // A new folder (F7), which it goes into.
        key(&mut p, Key::F(7));
        typed(&mut p, "new");
        key(&mut p, Key::Enter);
        assert_eq!(p.dir(), dir.join("sub").join("new"));
        assert!(p.dir().is_dir());
        // A typed folder is chosen even before it exists.
        key(&mut p, Key::Tab);
        typed(&mut p, "later");
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("sub").join("new").join("later")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_as() {
        let dir = scratch("save");
        let mut p = FilePicker::save(&dir.join("sub").join("export.lst"), &[]);
        assert_eq!(p.dir(), dir.join("sub"));
        // Enter saves the name it started with, in the folder shown.
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("sub").join("export.lst")]);
        // Over to the list, up a folder, and pick a file: its name is taken,
        // and saving over it takes a second Enter.
        key(&mut p, Key::Tab);
        key(&mut p, Key::Backspace);
        assert_eq!(p.dir(), dir);
        key(&mut p, Key::Char('a'));
        key(&mut p, Key::Enter);
        assert!(p.on_name);
        assert_eq!(p.name.value, "A.txt");
        assert!(matches!(key(&mut p, Key::Enter), PickerAction::None));
        assert!(p.error.as_deref().is_some_and(|e| e.contains("replace")));
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("A.txt")]);
        // Typing changes the name (and the question goes).
        key(&mut p, Key::End);
        typed(&mut p, ".bak");
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("A.txt.bak")]);
        // A folder typed as the name goes there; an empty name says so.
        p.name = TextField::new("sub", 255);
        key(&mut p, Key::Enter);
        assert_eq!(p.dir(), dir.join("sub"));
        assert!(matches!(key(&mut p, Key::Enter), PickerAction::None));
        assert!(p.error.is_some());
        let mut s = Screen::new(80, 25, 0);
        assert!(p.window(&mut s, "Export", "", "save").is_some(), "the name has the cursor");
        // Saving into a folder that isn't made yet makes it.
        let mut p = FilePicker::save(&dir.join("new").join("deep").join("x.ans"), &[]);
        assert!(p.missing);
        assert_eq!(chosen(key(&mut p, Key::Enter)), vec![dir.join("new").join("deep").join("x.ans")]);
        assert!(dir.join("new").join("deep").is_dir());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
