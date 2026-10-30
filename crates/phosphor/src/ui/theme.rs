//! The colors of Phosphor's own screens: every part of the interface has a
//! foreground and background from the 16-color palette, chosen from a few
//! schemes or one by one in the settings, as in QModem and Telix.

use std::cell::Cell;

use phosphor_core::screen::Color;
use serde::{Deserialize, Serialize};

/// A foreground (0-15) and background (0-15) in VGA attribute order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attr {
    pub fg: u8,
    pub bg: u8,
}

impl Attr {
    pub const fn new(fg: u8, bg: u8) -> Attr {
        Attr { fg, bg }
    }

    pub fn fg(self) -> Color {
        Color::Pc(self.fg & 15)
    }

    pub fn bg(self) -> Color {
        Color::Pc(self.bg & 15)
    }
}

macro_rules! theme {
    ($($field:ident: $name:literal),* $(,)?) => {
        /// The interface colors.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Theme {
            $(pub $field: Attr,)*
        }

        impl Theme {
            /// Each part's name, for the color editor, in its order.
            pub const NAMES: &[&str] = &[$($name),*];

            pub fn get(&self, i: usize) -> Attr {
                [$(self.$field),*][i]
            }

            pub fn set(&mut self, i: usize, a: Attr) {
                let mut n = 0;
                $(
                    if n == i {
                        self.$field = a;
                    }
                    n += 1;
                )*
                let _ = n;
            }
        }
    };
}

theme! {
    title_bar: "Title bar",
    border: "Directory border",
    headings: "Column headings",
    entries: "Directory entries",
    selected: "Selected entry",
    info: "Information line",
    help: "Help line",
    help_key: "Help line keys",
    status: "Status bar",
    status_alt: "Status bar, scrollback",
    window: "Window",
    window_title: "Window title",
    window_text: "Window text",
    field_name: "Field name",
    field_value: "Field value",
    input: "Input field",
    keys: "Key hints",
    note: "Notes",
    error: "Errors",
}

// VGA attribute colors.
const BLACK: u8 = 0;
const BLUE: u8 = 1;
const GREEN: u8 = 2;
const CYAN: u8 = 3;
const RED: u8 = 4;
const BROWN: u8 = 6;
const GRAY: u8 = 7;
const DARK_GRAY: u8 = 8;
const LIGHT_GREEN: u8 = 10;
const LIGHT_CYAN: u8 = 11;
const LIGHT_RED: u8 = 12;
const YELLOW: u8 = 14;
const WHITE: u8 = 15;

const fn a(fg: u8, bg: u8) -> Attr {
    Attr::new(fg, bg)
}

/// The built-in schemes, by name; the first is the default.
pub const SCHEMES: [(&str, Theme); 6] = [
    (
        "Classic blue",
        Theme {
            title_bar: a(WHITE, BLUE),
            border: a(LIGHT_CYAN, BLUE),
            headings: a(YELLOW, BLUE),
            entries: a(WHITE, BLUE),
            selected: a(BLACK, CYAN),
            info: a(LIGHT_CYAN, BLACK),
            help: a(GRAY, BLACK),
            help_key: a(YELLOW, BLACK),
            status: a(WHITE, BLUE),
            status_alt: a(BLACK, CYAN),
            window: a(WHITE, BLUE),
            window_title: a(YELLOW, BLUE),
            window_text: a(GRAY, BLUE),
            field_name: a(GRAY, BLUE),
            field_value: a(LIGHT_CYAN, BLUE),
            input: a(WHITE, BLACK),
            keys: a(YELLOW, BLUE),
            note: a(DARK_GRAY, BLUE),
            error: a(LIGHT_RED, BLUE),
        },
    ),
    (
        "Midnight",
        Theme {
            title_bar: a(BLACK, CYAN),
            border: a(CYAN, BLACK),
            headings: a(LIGHT_CYAN, BLACK),
            entries: a(GRAY, BLACK),
            selected: a(WHITE, BLUE),
            info: a(CYAN, BLACK),
            help: a(DARK_GRAY, BLACK),
            help_key: a(LIGHT_CYAN, BLACK),
            status: a(BLACK, CYAN),
            status_alt: a(WHITE, BLUE),
            window: a(WHITE, BLACK),
            window_title: a(LIGHT_CYAN, BLACK),
            window_text: a(GRAY, BLACK),
            field_name: a(CYAN, BLACK),
            field_value: a(WHITE, BLACK),
            input: a(YELLOW, BLUE),
            keys: a(LIGHT_CYAN, BLACK),
            note: a(DARK_GRAY, BLACK),
            error: a(LIGHT_RED, BLACK),
        },
    ),
    (
        "Gray windows",
        Theme {
            title_bar: a(BLACK, GRAY),
            border: a(WHITE, BLUE),
            headings: a(YELLOW, BLUE),
            entries: a(LIGHT_CYAN, BLUE),
            selected: a(BLACK, GREEN),
            info: a(BLACK, CYAN),
            help: a(BLACK, GRAY),
            help_key: a(RED, GRAY),
            status: a(BLACK, GRAY),
            status_alt: a(WHITE, GREEN),
            window: a(BLACK, GRAY),
            window_title: a(WHITE, GRAY),
            window_text: a(DARK_GRAY, GRAY),
            field_name: a(BLACK, GRAY),
            field_value: a(BLUE, GRAY),
            input: a(YELLOW, BLUE),
            keys: a(RED, GRAY),
            note: a(DARK_GRAY, GRAY),
            error: a(RED, GRAY),
        },
    ),
    (
        "Green phosphor",
        Theme {
            title_bar: a(BLACK, GREEN),
            border: a(GREEN, BLACK),
            headings: a(LIGHT_GREEN, BLACK),
            entries: a(GREEN, BLACK),
            selected: a(BLACK, LIGHT_GREEN),
            info: a(LIGHT_GREEN, BLACK),
            help: a(GREEN, BLACK),
            help_key: a(LIGHT_GREEN, BLACK),
            status: a(BLACK, GREEN),
            status_alt: a(BLACK, LIGHT_GREEN),
            window: a(LIGHT_GREEN, BLACK),
            window_title: a(BLACK, LIGHT_GREEN),
            window_text: a(GREEN, BLACK),
            field_name: a(GREEN, BLACK),
            field_value: a(LIGHT_GREEN, BLACK),
            input: a(BLACK, GREEN),
            keys: a(LIGHT_GREEN, BLACK),
            note: a(GREEN, BLACK),
            error: a(WHITE, BLACK),
        },
    ),
    (
        "Amber",
        Theme {
            title_bar: a(BLACK, BROWN),
            border: a(BROWN, BLACK),
            headings: a(YELLOW, BLACK),
            entries: a(BROWN, BLACK),
            selected: a(BLACK, YELLOW),
            info: a(YELLOW, BLACK),
            help: a(BROWN, BLACK),
            help_key: a(YELLOW, BLACK),
            status: a(BLACK, BROWN),
            status_alt: a(BLACK, YELLOW),
            window: a(YELLOW, BLACK),
            window_title: a(BLACK, YELLOW),
            window_text: a(BROWN, BLACK),
            field_name: a(BROWN, BLACK),
            field_value: a(YELLOW, BLACK),
            input: a(BLACK, BROWN),
            keys: a(YELLOW, BLACK),
            note: a(BROWN, BLACK),
            error: a(WHITE, BLACK),
        },
    ),
    (
        "Monochrome",
        Theme {
            title_bar: a(BLACK, GRAY),
            border: a(GRAY, BLACK),
            headings: a(WHITE, BLACK),
            entries: a(GRAY, BLACK),
            selected: a(BLACK, GRAY),
            info: a(WHITE, BLACK),
            help: a(GRAY, BLACK),
            help_key: a(WHITE, BLACK),
            status: a(BLACK, GRAY),
            status_alt: a(BLACK, WHITE),
            window: a(WHITE, BLACK),
            window_title: a(BLACK, GRAY),
            window_text: a(GRAY, BLACK),
            field_name: a(GRAY, BLACK),
            field_value: a(WHITE, BLACK),
            input: a(BLACK, GRAY),
            keys: a(WHITE, BLACK),
            note: a(DARK_GRAY, BLACK),
            error: a(WHITE, BLACK),
        },
    ),
];

impl Default for Theme {
    fn default() -> Theme {
        SCHEMES[0].1
    }
}

impl Theme {
    /// The name of the built-in scheme these colors are, if any.
    pub fn scheme_name(&self) -> Option<&'static str> {
        SCHEMES.iter().find(|(_, t)| t == self).map(|(n, _)| *n)
    }
}

thread_local! {
    static CURRENT: Cell<Theme> = Cell::new(Theme::default());
}

/// The colors the interface is drawn in now.
pub fn current() -> Theme {
    CURRENT.with(Cell::get)
}

pub fn set_current(t: Theme) {
    CURRENT.with(|c| c.set(t));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_and_names() {
        let mut t = Theme::default();
        assert_eq!(Theme::NAMES.len(), 19);
        assert_eq!(t.get(4), t.selected);
        t.set(4, Attr::new(14, 4));
        assert_eq!(t.selected, Attr::new(14, 4));
        assert_eq!(t.scheme_name(), None);
        assert_eq!(Theme::default().scheme_name(), Some("Classic blue"));
        // Every scheme is distinct, so its name can be shown.
        for (i, (_, a)) in SCHEMES.iter().enumerate() {
            for (_, b) in &SCHEMES[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn partial_toml_keeps_the_rest() {
        let t: Theme = toml::from_str("selected = { fg = 14, bg = 4 }").unwrap();
        assert_eq!(t.selected, Attr::new(14, 4));
        assert_eq!(t.window, Theme::default().window);
    }
}
