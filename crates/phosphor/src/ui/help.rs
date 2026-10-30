//! The help screen (Alt-Z, as in Telix): every hotkey on one page. Any key
//! closes it.

use phosphor_core::screen::{Cell, Screen};

use super::*;

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

/// (key, what it does): the key after the modifier in the first two.
const CALLS: [(&str, &str); 9] = [
    ("D", "Directory"),
    ("H", "Hang up"),
    ("R", "Call again"),
    ("L", "Send login"),
    ("U", "Upload files"),
    ("C", "Capture"),
    ("S", "Save screen"),
    ("V", "Paste"),
    ("X", "Exit"),
];

const SCREEN: [(&str, &str); 9] = [
    ("B", "Scrollback"),
    ("P", "Phosphor mode"),
    ("A", "4:3 / square"),
    ("W", "9 or 8 dots"),
    ("Enter", "Full screen"),
    ("I", "iCE colors"),
    ("E", "CP437 / UTF-8"),
    ("M", "ANSI music"),
    ("O", "Settings"),
];

const DIRECTORY: [(&str, &str); 8] = [
    ("Enter", "Call"),
    ("Ins / E", "Add / edit"),
    ("Del", "Remove"),
    ("P", "Pin to top"),
    ("/ or F3", "Find"),
    ("S", "Sort"),
    ("Q", "Quick connect"),
    ("I / X", "Import/export"),
];

pub fn draw(s: &mut Screen) {
    let t = theme::current();
    s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', t.help.fg(), t.help.bg()));
    let title = format!(" Phosphor {} \u{2502} Help", env!("CARGO_PKG_VERSION"));
    text(s, 0, 0, COLS, &title, t.title_bar.fg(), t.title_bar.bg());

    let (x, y, w, h) = (1, 1, COLS - 2, 23);
    frame(s, x, y, w, h, "Keys", t.window.fg(), t.window.bg());
    // `kw`: the width of the key column.
    let column = |s: &mut Screen, cx: usize, kw: usize, heading: &str, prefix: &str, keys: &[(&str, &str)]| {
        label(s, cx, y + 2, heading, t.window_title.fg(), t.window_title.bg());
        for (i, (key, what)) in keys.iter().enumerate() {
            let k = if prefix.is_empty() { key.to_string() } else { format!("{prefix}-{key}") };
            label(s, cx, y + 4 + i * 2, &k, t.keys.fg(), t.keys.bg());
            label(s, cx + kw, y + 4 + i * 2, what, t.window.fg(), t.window.bg());
        }
    };
    column(s, x + 2, 10, "Calls", HOTKEY, &CALLS);
    column(s, x + 26, 11, "Screen", HOTKEY, &SCREEN);
    column(s, x + 53, 9, "Dialing directory", "", &DIRECTORY);
    let note = "Mouse: drag to copy (Shift on release: as ANSI). Drop files to upload.";
    label(s, x + 2, y + h - 2, note, t.window_text.fg(), t.window_text.bg());
    text(s, 1, 24, COLS - 2, "Any key returns", t.help.fg(), t.help.bg());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_on_one_screen() {
        let mut s = Screen::new(COLS, ROWS, 0);
        draw(&mut s);
        let rows: Vec<String> = (0..ROWS).map(|y| Screen::row_text(s.row(y))).collect();
        assert!(rows.iter().any(|r| r.contains(&format!("{HOTKEY}-H")) && r.contains("Hang up")), "{rows:#?}");
        // Columns don't run into each other or off the screen.
        // (Columns at 3, 27 and 54; the border is at 78.)
        for (_, what) in &CALLS {
            assert!(3 + 10 + what.len() < 27, "{what}");
        }
        for (_, what) in &SCREEN {
            assert!(27 + 11 + what.len() < 54, "{what}");
        }
        for (key, what) in &DIRECTORY {
            assert!(key.len() < 9 && 54 + 9 + what.len() < 78, "{what}");
        }
    }
}
