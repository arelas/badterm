//! The "save scrollback" prompt (Alt-S): a file name whose extension picks
//! the format.

use std::path::{Path, PathBuf};

use phosphor_core::screen::Screen;

use super::*;

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

pub struct SavePrompt {
    pub field: TextField,
}

impl SavePrompt {
    pub fn new(default: &Path) -> SavePrompt {
        SavePrompt { field: TextField::new(&default.to_string_lossy(), 1000) }
    }

    pub fn path(&self) -> PathBuf {
        expand_home(self.field.value.trim().trim_matches('"'))
    }

    /// Draw the prompt; returns the text cursor position.
    pub fn draw(&self, s: &mut Screen) -> (usize, usize) {
        let (x, y, w) = ((s.cols() - 70) / 2, 9, 70);
        frame(s, x, y, w, 7, "Save Scrollback", WHITE, BLUE);
        label(s, x + 2, y + 1, ".ans ANSI or .txt text: the scrollback and screen", GRAY, BLUE);
        label(s, x + 2, y + 2, ".png: an image of what is on screen now", GRAY, BLUE);
        let cursor = self.field.draw(s, x + 2, y + 4, w - 4, false, WHITE, BLACK);
        label(s, x + 2, y + 5, "Enter save   Esc cancel", YELLOW, BLUE);
        cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_follows_the_extension() {
        assert_eq!(SaveFormat::from_path(Path::new("a/b.ANS")), Some(SaveFormat::Ansi));
        assert_eq!(SaveFormat::from_path(Path::new("b.txt")), Some(SaveFormat::Text));
        assert_eq!(SaveFormat::from_path(Path::new("b.png")), Some(SaveFormat::Png));
        assert_eq!(SaveFormat::from_path(Path::new("b")), None);
    }
}
