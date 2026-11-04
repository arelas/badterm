//! The help screen (Alt-Z, as in Telix): every key on one page, grouped by
//! what it is for. Any key closes it.

use badterm_core::screen::{Cell, Screen};

use super::*;

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

/// A group of keys under a heading. In keys, `@` stands for the hotkey
/// modifier (Alt, or Cmd on macOS).
struct Group {
    heading: &'static str,
    keys: &'static [(&'static str, &'static str)],
}

const LEFT: [Group; 3] = [
    Group {
        heading: "Calling",
        keys: &[
            ("@-D", "Dialing directory"),
            ("@-R", "Call again, or redial now"),
            ("@-L", "Send login (name, password)"),
            ("@-1..0", "Send a keyboard macro"),
            ("@-H", "Hang up"),
            ("@-X", "Exit BadTerm"),
        ],
    },
    Group {
        heading: "Files",
        keys: &[
            ("@-U", "Upload: Z/Y/XMODEM, Kermit"),
            ("@-G", "Download: Y/XMODEM, Kermit"),
            ("", "(ZMODEM, Kermit: automatic)"),
            ("@-C", "Capture session to a file"),
            ("@-S", "Save scrollback or screen"),
            ("@-F", "View ANSI art"),
        ],
    },
    Group {
        heading: "Scrollback and clipboard",
        keys: &[
            ("@-B", "Scrollback (or Shift-PgUp)"),
            ("Drag", "Copy (Shift: as ANSI)"),
            ("@-V", "Paste (or Shift-Ins)"),
        ],
    },
];

const RIGHT: [Group; 2] = [
    Group {
        heading: "Settings and display",
        keys: &[
            ("@-O", "Settings and colors"),
            ("@-P", "Phosphor mode (CRT look)"),
            ("@-A", "Aspect ratio"),
            ("@-W", "9-dot or 8-dot characters"),
            ("@-Enter", "Full screen"),
            ("@-I", "iCE colors or blinking"),
            ("@-E", "CP437 or UTF-8"),
            ("@-K", "Reveal (Viewdata)"),
            ("@-M", "Sound and music"),
        ],
    },
    Group {
        heading: "In the dialing directory",
        keys: &[
            ("Enter", "Call"),
            ("Ins/E/Del", "Add / edit / remove"),
            ("P", "Pin to the top"),
            ("/ or F3", "Find"),
            ("S / G", "Sort / one group"),
            ("Q", "Quick connect"),
            ("I / X", "Import / export"),
            ("O", "Settings"),
        ],
    },
];

/// The key column's width; descriptions follow it.
const KEY_W: usize = 10;

pub fn draw(s: &mut Screen) {
    let t = theme::current();
    s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', t.help.fg(), t.help.bg()));
    let title = format!(" BadTerm {} \u{2502} Help", env!("CARGO_PKG_VERSION"));
    text(s, 0, 0, COLS, &title, t.title_bar.fg(), t.title_bar.bg());

    frame(s, 0, 1, COLS, ROWS - 2, "Keys", t.window.fg(), t.window.bg());
    column(s, 2, &LEFT);
    column(s, 41, &RIGHT);

    let footer = format!("{HOTKEY}-O Settings and colors   Any other key returns");
    label(s, 1, 24, &footer, t.help_key.fg(), t.help_key.bg());
}

/// Draw groups down from the top of the frame, a blank line between them.
fn column(s: &mut Screen, x: usize, groups: &[Group]) {
    let t = theme::current();
    let mut y = 3;
    for g in groups {
        label(s, x, y, g.heading, t.window_title.fg(), t.window_title.bg());
        y += 1;
        for (key, what) in g.keys {
            label(s, x + 1, y, &key.replace('@', HOTKEY), t.keys.fg(), t.keys.bg());
            label(s, x + 1 + KEY_W, y, what, t.window.fg(), t.window.bg());
            y += 1;
        }
        y += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_on_one_screen() {
        let mut s = Screen::new(COLS, ROWS, 0);
        draw(&mut s);
        let rows: Vec<String> = (0..ROWS).map(|y| Screen::row_text(s.row(y))).collect();
        assert!(rows.iter().any(|r| r.contains(&format!("{HOTKEY}-O")) && r.contains("Settings")), "{rows:#?}");
        for (x, groups) in [(2, &LEFT[..]), (41, &RIGHT[..])] {
            // Inside the frame (rows 1 to 23, columns 0 to 79), and not
            // into the next column.
            let lines: usize = groups.iter().map(|g| g.keys.len() + 2).sum::<usize>() - 1;
            assert!(3 + lines <= 23, "{lines} lines");
            for g in groups {
                for (key, what) in g.keys {
                    assert!(key.replace('@', "Cmd").len() < KEY_W, "{key}");
                    assert!(x + 1 + KEY_W + what.len() <= if x == 2 { 40 } else { 78 }, "{what}");
                }
            }
        }
    }
}
