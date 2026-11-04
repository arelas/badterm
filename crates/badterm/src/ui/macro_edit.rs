//! Editing keyboard macros: the settings' (for every BBS) and a directory
//! entry's own, which take the place of the settings' ones they fill.

use badterm_core::screen::Screen;

use super::*;
use crate::input::{Key, Mods};
use crate::macros::COUNT;

pub enum EditAction {
    None,
    /// A macro changed (saved as it is typed).
    Changed,
    Close,
}

pub struct MacroEditor {
    line: usize,
    field: TextField,
}

impl MacroEditor {
    pub fn new(values: &[String]) -> MacroEditor {
        MacroEditor { line: 0, field: TextField::new(values.first().map_or("", String::as_str), 200) }
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods, values: &mut Vec<String>) -> EditAction {
        values.resize(COUNT, String::new());
        let next = match key {
            Key::Escape => return EditAction::Close,
            Key::Up => (self.line + COUNT - 1) % COUNT,
            Key::Down | Key::Enter | Key::Tab => (self.line + 1) % COUNT,
            _ => {
                self.field.handle_key(key, mods);
                values[self.line] = self.field.value.clone();
                return EditAction::Changed;
            }
        };
        self.line = next;
        self.field = TextField::new(&values[next], 200);
        EditAction::None
    }

    /// Draw it, titled `title`. `fallback`: the settings' macros, shown
    /// where an entry has none of its own.
    pub fn draw(&self, s: &mut Screen, title: &str, values: &[String], fallback: Option<&[String]>) -> (usize, usize) {
        let t = theme::current();
        let (x, y, w) = (3, 2, COLS_80 - 6);
        frame(s, x, y, w, 21, title, t.window.fg(), t.window.bg());
        let mut cursor = (0, 0);
        for i in 0..COUNT {
            let yy = y + 2 + i;
            let focused = i == self.line;
            let name_attr = if focused { t.keys } else { t.field_name };
            let k = format!("{HOTKEY}-{}", crate::macros::key_name(i));
            text(s, x + 2, yy, 7, &format!("{k:>6}"), name_attr.fg(), name_attr.bg());
            let own = values.get(i).map(String::as_str).unwrap_or("");
            let room = w - 12;
            if focused {
                cursor = self.field.draw(s, x + 10, yy, room, false, t.input.fg(), t.input.bg());
                if own.is_empty()
                    && let Some(g) = fallback.and_then(|f| f.get(i)).filter(|g| !g.is_empty())
                {
                    let shown = truncate(&format!("(Settings: {g})"), room - 1);
                    label(s, x + 11, yy, &shown, t.note.fg(), t.input.bg());
                }
            } else if own.is_empty()
                && let Some(g) = fallback.and_then(|f| f.get(i)).filter(|g| !g.is_empty())
            {
                text(s, x + 10, yy, room, &truncate(&format!("(Settings: {g})"), room), t.note.fg(), t.window.bg());
            } else {
                text(s, x + 10, yy, room, &truncate(own, room), t.field_value.fg(), t.field_value.bg());
            }
        }
        let mut help = vec![
            "Sent to the BBS when you press the key. In a macro:",
            "  ^M  Enter          ^[  Esc        ^A to ^Z  other control keys",
            "  ~   wait 1/2 sec   ^^  a ^        ^~  a ~",
            "  {user}  {password}   the dialing directory entry's",
        ];
        help.push(if fallback.is_some() {
            "This BBS's own; where one is empty, the one in Settings is sent."
        } else {
            "For example:  {user}^M~{password}^M   logs in."
        });
        for (i, l) in help.iter().enumerate() {
            label(s, x + 2, y + 14 + i, l, t.window_text.fg(), t.window_text.bg());
        }
        cursor
    }
}

const COLS_80: usize = 80;
