//! The "save scrollback" window (Alt-S): where to save, browsed for, and a
//! file name whose extension picks the format.

use std::path::{Path, PathBuf};

use badterm_core::screen::Screen;

use super::files::{FilePicker, PickerAction};
use crate::input::{Key, Mods};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    /// Scrollback and screen as ANSI (CP437), for art viewers and editors.
    Ansi,
    /// Scrollback and screen as plain text.
    Text,
    /// What is on screen now, as an image.
    Png,
}

impl SaveFormat {
    pub fn from_path(path: &Path) -> Option<SaveFormat> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "ans" | "ansi" => Some(SaveFormat::Ansi),
            "txt" | "asc" => Some(SaveFormat::Text),
            "png" => Some(SaveFormat::Png),
            _ => None,
        }
    }
}

/// What the save window decided.
pub enum SaveAction {
    None,
    Cancel,
    Save(PathBuf),
}

/// The save window: where to save, browsed for, and the name, whose
/// extension picks the format.
pub struct SavePrompt {
    picker: FilePicker,
}

impl SavePrompt {
    pub fn new(default: &Path) -> SavePrompt {
        SavePrompt { picker: FilePicker::save(default, &[".ans", ".ansi", ".txt", ".asc", ".png"]) }
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods) -> SaveAction {
        if key == Key::Escape && !self.picker.is_typing() {
            return SaveAction::Cancel;
        }
        match self.picker.handle_key(key, mods) {
            PickerAction::Chosen(paths) => SaveAction::Save(paths[0].clone()),
            PickerAction::None => SaveAction::None,
        }
    }

    /// Draw the window; returns the text cursor, while the name has it.
    pub fn draw(&mut self, s: &mut Screen) -> Option<(usize, usize)> {
        let note = ".ans or .txt: the scrollback and screen, .png: the screen as it is";
        self.picker.window(s, "Save Scrollback", note, "save")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_where_browsed_to() {
        let dir = std::env::temp_dir().join(format!("badterm-save-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("art")).unwrap();
        let mut p = SavePrompt::new(&dir.join("vortex.ans"));
        let mut s = Screen::new(80, 25, 0);
        assert!(p.draw(&mut s).is_some(), "the name has the cursor");
        // Into the art folder: the list, then Enter on it, then back to the name.
        p.handle_key(Key::Tab, Mods::default());
        p.handle_key(Key::Char('a'), Mods::default());
        p.handle_key(Key::Enter, Mods::default());
        p.handle_key(Key::Tab, Mods::default());
        match p.handle_key(Key::Enter, Mods::default()) {
            SaveAction::Save(path) => assert_eq!(path, dir.join("art").join("vortex.ans")),
            _ => panic!(),
        }
        assert!(matches!(p.handle_key(Key::Escape, Mods::default()), SaveAction::Cancel));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn format_follows_the_extension() {
        assert_eq!(SaveFormat::from_path(Path::new("a/b.ANS")), Some(SaveFormat::Ansi));
        assert_eq!(SaveFormat::from_path(Path::new("b.txt")), Some(SaveFormat::Text));
        assert_eq!(SaveFormat::from_path(Path::new("b.png")), Some(SaveFormat::Png));
        assert_eq!(SaveFormat::from_path(Path::new("b")), None);
    }
}
