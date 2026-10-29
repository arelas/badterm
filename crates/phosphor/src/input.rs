//! Keyboard handling: translation of winit key events into a small key model,
//! and encoding of keys into the byte sequences BBS software expects.

use phosphor_core::{Encoding, cp437};
use winit::event::KeyEvent;
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Cmd on macOS, the Windows key elsewhere.
    pub logo: bool,
}

impl From<ModifiersState> for Mods {
    fn from(m: ModifiersState) -> Self {
        Mods { ctrl: m.control_key(), alt: m.alt_key(), shift: m.shift_key(), logo: m.super_key() }
    }
}

impl Mods {
    /// The modifier used for client hotkeys: Alt, or Cmd on macOS (where
    /// Option composes characters).
    pub fn hotkey(&self) -> bool {
        self.alt || (cfg!(target_os = "macos") && self.logo)
    }
}

pub fn translate(event: &KeyEvent) -> Option<Key> {
    Some(match &event.logical_key {
        WKey::Named(n) => match n {
            NamedKey::Enter => Key::Enter,
            NamedKey::Escape => Key::Escape,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Tab => Key::Tab,
            NamedKey::Space => Key::Char(' '),
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Insert => Key::Insert,
            NamedKey::Delete => Key::Delete,
            NamedKey::F1 => Key::F(1),
            NamedKey::F2 => Key::F(2),
            NamedKey::F3 => Key::F(3),
            NamedKey::F4 => Key::F(4),
            NamedKey::F5 => Key::F(5),
            NamedKey::F6 => Key::F(6),
            NamedKey::F7 => Key::F(7),
            NamedKey::F8 => Key::F(8),
            NamedKey::F9 => Key::F(9),
            NamedKey::F10 => Key::F(10),
            NamedKey::F11 => Key::F(11),
            NamedKey::F12 => Key::F(12),
            _ => return None,
        },
        WKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                // Multi-character results (dead-key compositions) arrive via `text`.
                return event.text.as_ref().and_then(|t| t.chars().next()).map(Key::Char);
            }
            Key::Char(c)
        }
        _ => return None,
    })
}

/// Encode a key for the remote system using ANSI-BBS conventions.
pub fn encode(key: Key, mods: Mods, encoding: Encoding) -> Vec<u8> {
    let seq: &[u8] = match key {
        Key::Char(c) => {
            if mods.ctrl {
                return match c.to_ascii_lowercase() {
                    c @ 'a'..='z' => vec![c as u8 - b'a' + 1],
                    '@' | ' ' | '2' => vec![0],
                    '[' | '3' => vec![0x1B],
                    '\\' | '4' => vec![0x1C],
                    ']' | '5' => vec![0x1D],
                    '^' | '6' => vec![0x1E],
                    '_' | '-' | '7' => vec![0x1F],
                    _ => vec![],
                };
            }
            return match encoding {
                Encoding::Cp437 => cp437::from_unicode(c).map(|b| vec![b]).unwrap_or_default(),
                Encoding::Utf8 => c.to_string().into_bytes(),
            };
        }
        Key::Enter => b"\r",
        Key::Escape => b"\x1b",
        Key::Backspace => b"\x08",
        Key::Tab if mods.shift => b"\x1b[Z",
        Key::Tab => b"\t",
        Key::Up => b"\x1b[A",
        Key::Down => b"\x1b[B",
        Key::Right => b"\x1b[C",
        Key::Left => b"\x1b[D",
        Key::Home => b"\x1b[H",
        Key::End => b"\x1b[K",
        Key::Insert => b"\x1b[@",
        Key::Delete => b"\x7f",
        Key::PageUp => b"\x1b[V",
        Key::PageDown => b"\x1b[U",
        Key::F(1) => b"\x1bOP",
        Key::F(2) => b"\x1bOQ",
        Key::F(3) => b"\x1bOR",
        Key::F(4) => b"\x1bOS",
        Key::F(5) => b"\x1b[15~",
        Key::F(6) => b"\x1b[17~",
        Key::F(7) => b"\x1b[18~",
        Key::F(8) => b"\x1b[19~",
        Key::F(9) => b"\x1b[20~",
        Key::F(10) => b"\x1b[21~",
        Key::F(11) => b"\x1b[23~",
        Key::F(12) => b"\x1b[24~",
        Key::F(_) => b"",
    };
    seq.to_vec()
}
