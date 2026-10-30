//! The SSH login dialog: asks for a password, a key passphrase, or a
//! keyboard-interactive question while connecting.

use badterm_core::screen::Screen;
use badterm_core::session::{AuthPrompt, PromptKind};

use super::*;

const W: usize = 64;

pub struct LoginPrompt {
    pub title: String,
    pub instructions: String,
    pub question: String,
    /// Whether the answer is shown as typed.
    pub echo: bool,
    pub field: TextField,
    /// Whether "remember" is offered: passwords for directory entries only.
    pub can_remember: bool,
    pub remember: bool,
    /// Where a remembered password goes, for the label.
    pub keychain: bool,
}

pub enum LoginAction {
    None,
    Answer(String),
    Cancel,
}

impl LoginPrompt {
    pub fn new(prompt: &AuthPrompt, in_directory: bool, keychain: bool) -> LoginPrompt {
        LoginPrompt {
            title: prompt.title.clone(),
            instructions: prompt.instructions.clone(),
            question: prompt.prompt.clone(),
            echo: prompt.echo,
            field: TextField::new("", 500),
            can_remember: in_directory && prompt.kind == PromptKind::Password,
            remember: false,
            keychain,
        }
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods) -> LoginAction {
        match key {
            Key::Enter => return LoginAction::Answer(std::mem::take(&mut self.field.value)),
            Key::Escape => return LoginAction::Cancel,
            Key::Tab if self.can_remember => self.remember = !self.remember,
            _ => {
                self.field.handle_key(key, mods);
            }
        }
        LoginAction::None
    }

    /// Draw the dialog; returns the text cursor position.
    pub fn draw(&self, s: &mut Screen) -> (usize, usize) {
        let info: Vec<String> =
            self.instructions.lines().filter(|l| !l.trim().is_empty()).flat_map(|l| wrap(l, W - 6)).take(4).collect();
        let h = 7 + info.len() + usize::from(self.can_remember);
        let (x, y) = ((s.cols() - W) / 2, (s.rows().saturating_sub(h)) / 2);
        let t = theme::current();
        frame(s, x, y, W, h, &truncate(&self.title, W - 8), t.window.fg(), t.window.bg());
        let mut row = y + 2;
        for line in &info {
            label(s, x + 3, row, line, t.field_value.fg(), t.field_value.bg());
            row += 1;
        }
        text(s, x + 3, row, W - 6, &truncate(&self.question, W - 6), t.window.fg(), t.window.bg());
        let cursor = self.field.draw(s, x + 3, row + 1, W - 6, !self.echo, t.input.fg(), t.input.bg());
        row += 3;
        if self.can_remember {
            let mark = if self.remember { 'x' } else { ' ' };
            let place = if self.keychain { "in the keychain" } else { "in the directory file" };
            label(
                s,
                x + 3,
                row,
                &format!("[{mark}] Remember password {place}   Tab"),
                t.window_text.fg(),
                t.window_text.bg(),
            );
            row += 1;
        }
        label(s, x + 3, row, "Enter log in   Esc cancel", t.keys.fg(), t.keys.bg());
        cursor
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn sample(can_remember: bool) -> LoginPrompt {
        LoginPrompt {
            title: "SSH login to bbs.example.com".into(),
            instructions: String::new(),
            question: "Password for guest:".into(),
            echo: false,
            field: TextField::new("", 500),
            can_remember,
            remember: false,
            keychain: true,
        }
    }

    #[test]
    fn keys() {
        let m = Mods::default();
        let mut p = sample(true);
        for c in "pw".chars() {
            p.handle_key(Key::Char(c), m);
        }
        p.handle_key(Key::Tab, m);
        assert!(p.remember);
        assert!(matches!(p.handle_key(Key::Enter, m), LoginAction::Answer(a) if a == "pw"));
        assert!(matches!(p.handle_key(Key::Escape, m), LoginAction::Cancel));

        // Tab does nothing when remembering is not offered.
        let mut p = sample(false);
        p.handle_key(Key::Tab, m);
        assert!(!p.remember);
    }
}
