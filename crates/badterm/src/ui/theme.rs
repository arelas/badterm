//! The colors of BadTerm's own screens: every part of the interface has a
//! foreground and background from the 16-color palette, chosen from a few
//! schemes or one by one in the settings, as in QModem and Telix. A theme
//! may also bring its own 16 colors (see [`Palette`]), as the Bad Cluster
//! scheme does.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use badterm_core::screen::{Color, Look};
use serde::{Deserialize, Serialize};

/// A foreground (0-15) and background (0-15) in VGA attribute order.
/// Theme files write them as color names (`fg = "light cyan"`); numbers
/// are read too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attr {
    #[serde(with = "color_name")]
    pub fg: u8,
    #[serde(with = "color_name")]
    pub bg: u8,
}

/// The 16 colors' names, in VGA attribute order.
pub const COLOR_NAMES: [&str; 16] = [
    "black",
    "blue",
    "green",
    "cyan",
    "red",
    "magenta",
    "brown",
    "light gray",
    "dark gray",
    "light blue",
    "light green",
    "light cyan",
    "light red",
    "light magenta",
    "yellow",
    "white",
];

mod color_name {
    use serde::Serializer;
    use serde::de::{self, Deserializer, Visitor};

    use super::COLOR_NAMES;

    pub fn serialize<S: Serializer>(c: &u8, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(COLOR_NAMES[(*c & 15) as usize])
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = u8;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a color name such as \"light cyan\", or 0-15")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<u8, E> {
                u8::try_from(v).ok().filter(|&v| v < 16).ok_or_else(|| E::custom(format!("color {v} is not 0-15")))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<u8, E> {
                self.visit_u64(u64::try_from(v).map_err(|_| E::custom("negative color"))?)
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<u8, E> {
                let want = v.trim().to_ascii_lowercase().replace(['-', '_'], " ").replace("grey", "gray");
                let want = if want == "gray" { "light gray".to_string() } else { want };
                COLOR_NAMES
                    .iter()
                    .position(|n| *n == want)
                    .map(|i| i as u8)
                    .ok_or_else(|| E::custom(format!("unknown color {v:?}")))
            }
        }
        // Numbers or names; `deserialize_any` lets TOML and JSON say which.
        d.deserialize_any(V)
    }
}

impl Attr {
    pub const fn new(fg: u8, bg: u8) -> Attr {
        Attr { fg, bg }
    }

    /// In BadTerm's own colors (the scheme's palette), wherever drawn.
    pub fn fg(self) -> Color {
        Color::Ui(self.fg & 15)
    }

    pub fn bg(self) -> Color {
        Color::Ui(self.bg & 15)
    }
}

/// A theme's own 16 colors, in VGA attribute order, replacing the VGA's on
/// BadTerm's screens (not the BBS's). Theme files write them as
/// `"#RRGGBB"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette(pub [u32; 16]);

impl Serialize for Palette {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(16))?;
        for c in self.0 {
            seq.serialize_element(&format!("#{c:06X}"))?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Palette {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Palette, D::Error> {
        use serde::de::Error;
        let list = Vec::<String>::deserialize(d)?;
        if list.len() != 16 {
            return Err(D::Error::custom(format!("a palette has 16 colors, not {}", list.len())));
        }
        let mut out = [0u32; 16];
        for (slot, text) in out.iter_mut().zip(&list) {
            let hex = text.trim().trim_start_matches('#');
            *slot = u32::from_str_radix(hex, 16)
                .ok()
                .filter(|_| hex.len() == 6)
                .ok_or_else(|| D::Error::custom(format!("{text:?} is not a #RRGGBB color")))?;
        }
        Ok(Palette(out))
    }
}

macro_rules! theme {
    ($($field:ident: $name:literal),* $(,)?) => {
        /// The interface colors.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Theme {
            $(pub $field: Attr,)*
            /// Its own colors for the 16 VGA ones, if it has them.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub palette: Option<Palette>,
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
const MAGENTA: u8 = 5;
const LIGHT_BLUE: u8 = 9;
const LIGHT_MAGENTA: u8 = 13;
const YELLOW: u8 = 14;
const WHITE: u8 = 15;

const fn a(fg: u8, bg: u8) -> Attr {
    Attr::new(fg, bg)
}

/// Bad Cluster's colors (badcluster.dev), in the VGA's 16 places: the void
/// and panels for the darks, copper for brown and yellow, teal for cyan,
/// and its warm white and grays.
const BAD_CLUSTER: Palette = Palette([
    0x0E1015, // black: the void
    0x161922, // blue: a panel
    0x2A4D49, // green: dim teal
    0x46B3A6, // cyan: teal
    0x8C3B2E, // red
    0x1C2029, // magenta: a raised panel
    0xD98E3E, // brown: copper
    0x8B90A0, // light gray: dim ink
    0x565C6B, // dark gray: faint ink
    0x262B36, // light blue: a rule
    0x8FD6A8, // light green
    0x6FF0DD, // light cyan: glowing teal
    0xFF6B5C, // light red
    0x6B4F2A, // light magenta: dim copper
    0xFFB35C, // yellow: glowing copper
    0xE9E7E2, // white: ink
]);

/// Dark Dracula: the Dracula colors (draculatheme.com) on pure black, as
/// "Dracula Dark" has them, in the VGA's 16 places: black surfaces and
/// panels, Dracula's selection and comment colors, its purple (a shade
/// darker) for the accent (yellow and brown), and its foreground for white.
const DRACULA: Palette = Palette([
    0x000000, // black: the surface
    0x0A0A0A, // blue: an inner panel
    0x44475A, // green: the selection
    0x6272A4, // cyan: muted (Dracula's comment)
    0xFF5555, // red: danger
    0x0D0D0D, // magenta: the page
    0x7A52BD, // brown: the accent, dimmed
    0xC6C6C0, // light gray: dim text
    0x282A36, // dark gray: a border
    0x343746, // light blue: a rule
    0x50FA7B, // light green
    0x8BE9FD, // light cyan
    0xFF6E6E, // light red
    0xFF79C6, // light magenta: pink
    0x9A6FE0, // yellow: the accent, Dracula's purple a shade darker
    0xF8F8F2, // white: the foreground
]);

/// A scheme with no fills: everything on black, the borders in the dark
/// shade of one color, text in its bright shade, and white for what is
/// chosen or typed. (VGA's dark blue is too dark to read as text.)
const fn unfilled(dark: u8, bright: u8) -> Theme {
    Theme {
        title_bar: a(bright, BLACK),
        border: a(dark, BLACK),
        headings: a(WHITE, BLACK),
        entries: a(bright, BLACK),
        selected: a(WHITE, BLACK),
        info: a(bright, BLACK),
        help: a(bright, BLACK),
        help_key: a(WHITE, BLACK),
        status: a(bright, BLACK),
        status_alt: a(WHITE, BLACK),
        window: a(dark, BLACK),
        window_title: a(WHITE, BLACK),
        window_text: a(bright, BLACK),
        field_name: a(bright, BLACK),
        field_value: a(WHITE, BLACK),
        input: a(WHITE, BLACK),
        keys: a(WHITE, BLACK),
        note: a(DARK_GRAY, BLACK),
        error: a(LIGHT_RED, BLACK),
        palette: None,
    }
}

/// The built-in schemes, by name; the first is the default.
pub const SCHEMES: [(&str, Theme); 14] = [
    (
        // The logo's colors (assets/logo/badterm.ans): the letters' cyan
        // ground for the bars, their light blue for the frames and light
        // green for the headings, and the gold of the ki around them for
        // what is chosen and what to press. On black, in the VGA's own colors.
        "BadTerm",
        Theme {
            title_bar: a(BLACK, CYAN),
            border: a(LIGHT_BLUE, BLACK),
            headings: a(LIGHT_GREEN, BLACK),
            entries: a(GRAY, BLACK),
            selected: a(BLACK, YELLOW),
            info: a(CYAN, BLACK),
            help: a(GRAY, BLACK),
            help_key: a(YELLOW, BLACK),
            status: a(BLACK, CYAN),
            status_alt: a(BLACK, YELLOW),
            window: a(LIGHT_BLUE, BLACK),
            window_title: a(YELLOW, BLACK),
            window_text: a(GRAY, BLACK),
            field_name: a(CYAN, BLACK),
            field_value: a(WHITE, BLACK),
            input: a(WHITE, BLUE),
            keys: a(YELLOW, BLACK),
            note: a(DARK_GRAY, BLACK),
            error: a(LIGHT_RED, BLACK),
            palette: None,
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
            palette: None,
        },
    ),
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
            palette: None,
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
            palette: None,
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
            palette: None,
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
            palette: None,
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
            palette: None,
        },
    ),
    (
        "Ocean",
        Theme {
            title_bar: a(WHITE, CYAN),
            border: a(LIGHT_BLUE, BLUE),
            headings: a(LIGHT_CYAN, BLUE),
            entries: a(LIGHT_CYAN, BLUE),
            selected: a(BLUE, LIGHT_CYAN),
            info: a(LIGHT_BLUE, BLACK),
            help: a(CYAN, BLACK),
            help_key: a(WHITE, BLACK),
            status: a(WHITE, CYAN),
            status_alt: a(BLUE, LIGHT_CYAN),
            window: a(WHITE, CYAN),
            window_title: a(YELLOW, CYAN),
            window_text: a(BLUE, CYAN),
            field_name: a(BLACK, CYAN),
            field_value: a(WHITE, CYAN),
            input: a(LIGHT_CYAN, BLUE),
            keys: a(YELLOW, CYAN),
            note: a(BLUE, CYAN),
            error: a(LIGHT_RED, CYAN),
            palette: None,
        },
    ),
    (
        "Crimson",
        Theme {
            title_bar: a(YELLOW, RED),
            border: a(LIGHT_RED, BLACK),
            headings: a(YELLOW, BLACK),
            entries: a(GRAY, BLACK),
            selected: a(WHITE, RED),
            info: a(LIGHT_RED, BLACK),
            help: a(DARK_GRAY, BLACK),
            help_key: a(YELLOW, BLACK),
            status: a(YELLOW, RED),
            status_alt: a(BLACK, LIGHT_RED),
            window: a(WHITE, RED),
            window_title: a(YELLOW, RED),
            window_text: a(GRAY, RED),
            field_name: a(GRAY, RED),
            field_value: a(WHITE, RED),
            input: a(YELLOW, BLACK),
            keys: a(YELLOW, RED),
            note: a(LIGHT_RED, RED),
            error: a(YELLOW, RED),
            palette: None,
        },
    ),
    (
        "Plum",
        Theme {
            title_bar: a(WHITE, MAGENTA),
            border: a(LIGHT_MAGENTA, BLACK),
            headings: a(LIGHT_CYAN, BLACK),
            entries: a(LIGHT_MAGENTA, BLACK),
            selected: a(BLACK, LIGHT_MAGENTA),
            info: a(LIGHT_CYAN, BLACK),
            help: a(MAGENTA, BLACK),
            help_key: a(LIGHT_CYAN, BLACK),
            status: a(WHITE, MAGENTA),
            status_alt: a(BLACK, LIGHT_CYAN),
            window: a(WHITE, MAGENTA),
            window_title: a(LIGHT_CYAN, MAGENTA),
            window_text: a(LIGHT_MAGENTA, MAGENTA),
            field_name: a(LIGHT_MAGENTA, MAGENTA),
            field_value: a(WHITE, MAGENTA),
            input: a(LIGHT_CYAN, BLACK),
            keys: a(YELLOW, MAGENTA),
            note: a(BLACK, MAGENTA),
            error: a(YELLOW, MAGENTA),
            palette: None,
        },
    ),
    ("Dark blue", unfilled(BLUE, LIGHT_BLUE)),
    ("Dark cyan", unfilled(CYAN, LIGHT_CYAN)),
    (
        "Bad Cluster",
        Theme {
            title_bar: a(YELLOW, MAGENTA),
            border: a(DARK_GRAY, BLACK),
            headings: a(BROWN, BLACK),
            entries: a(WHITE, BLACK),
            selected: a(LIGHT_CYAN, GREEN),
            info: a(GRAY, BLACK),
            help: a(DARK_GRAY, BLACK),
            help_key: a(CYAN, BLACK),
            status: a(BLACK, BROWN),
            status_alt: a(BLACK, CYAN),
            window: a(WHITE, BLUE),
            window_title: a(YELLOW, BLUE),
            window_text: a(GRAY, BLUE),
            field_name: a(CYAN, BLUE),
            field_value: a(WHITE, BLUE),
            input: a(LIGHT_CYAN, GREEN),
            keys: a(BROWN, BLUE),
            note: a(DARK_GRAY, BLUE),
            error: a(LIGHT_RED, BLUE),
            palette: Some(BAD_CLUSTER),
        },
    ),
    (
        "Dark Dracula",
        Theme {
            title_bar: a(YELLOW, MAGENTA),
            border: a(DARK_GRAY, BLACK),
            headings: a(BROWN, BLACK),
            entries: a(WHITE, BLACK),
            selected: a(WHITE, GREEN),
            info: a(CYAN, BLACK),
            help: a(CYAN, BLACK),
            help_key: a(YELLOW, BLACK),
            status: a(BLACK, YELLOW),
            status_alt: a(BLACK, LIGHT_MAGENTA),
            window: a(BROWN, BLUE),
            window_title: a(YELLOW, BLUE),
            window_text: a(GRAY, BLUE),
            field_name: a(CYAN, BLUE),
            field_value: a(WHITE, BLUE),
            input: a(WHITE, GREEN),
            keys: a(YELLOW, BLUE),
            note: a(CYAN, BLUE),
            error: a(RED, BLUE),
            palette: Some(DRACULA),
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

/// A theme file, for sharing colors: a name, who made it, and the colors
/// (each part's `fg` and `bg`, by name). Parts it leaves out keep the
/// default scheme's colors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeFile {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    pub colors: Theme,
}

impl ThemeFile {
    pub fn load(path: &Path) -> Result<ThemeFile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut file: ThemeFile = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        file.name = file.name.trim().to_string();
        if file.name.is_empty() {
            bail!("{} has no name", path.display());
        }
        Ok(file)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let header = "# A BadTerm color theme. Colors: black, blue, green, cyan, red, magenta,\n\
                      # brown, light gray, dark gray, light blue, light green, light cyan,\n\
                      # light red, light magenta, yellow, white. A theme may also bring\n\
                      # its own 16 colors for them: palette = [\"#RRGGBB\", ...], in that order.\n\n";
        std::fs::write(path, format!("{header}{}", toml::to_string_pretty(self)?))
            .with_context(|| format!("writing {}", path.display()))
    }
}

/// The themes in `dir` (its `.toml` files), by name. Unreadable files are
/// skipped, with a warning in the log.
pub fn user_themes(dir: &Path) -> Vec<(String, Theme)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .collect();
    paths.sort();
    let mut out: Vec<(String, Theme)> = Vec::new();
    for p in paths {
        match ThemeFile::load(&p) {
            Ok(f) if SCHEMES.iter().any(|(n, _)| n.eq_ignore_ascii_case(&f.name)) => {
                log::warn!("{}: {} is a built-in scheme's name", p.display(), f.name);
            }
            Ok(f) => out.push((f.name, f.colors)),
            Err(e) => log::warn!("{e:#}"),
        }
    }
    out
}

/// A file name for a theme called `name`.
pub fn file_name(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("{}.toml", if stem.is_empty() { "theme" } else { &stem })
}

thread_local! {
    static CURRENT: Cell<Theme> = Cell::new(Theme::default());
}

/// The colors the interface is drawn in now.
pub fn current() -> Theme {
    CURRENT.with(Cell::get)
}

/// How BadTerm's own screens are drawn: the standard look, with the
/// theme's own 16 colors if it has them.
pub fn ui_look() -> Look {
    let mut look = Look::standard().clone();
    if let Some(Palette(colors)) = current().palette {
        for (vga, rgb) in colors.into_iter().enumerate() {
            // The look is in xterm order; the palette in VGA order.
            let ansi = badterm_core::palette::ANSI_TO_VGA.iter().position(|&v| v as usize == vga & 7).unwrap_or(0);
            look.palette[ansi + (vga & 8)] = rgb;
        }
    }
    look
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
        assert_eq!(Theme::default().scheme_name(), Some("BadTerm"));
        // Every scheme is distinct, so its name can be shown.
        for (i, (_, a)) in SCHEMES.iter().enumerate() {
            for (_, b) in &SCHEMES[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn palettes() {
        let bc = SCHEMES.iter().find(|(n, _)| *n == "Bad Cluster").unwrap().1;
        set_current(bc);
        let look = ui_look();
        assert_eq!(look.rgb(Color::Pc(6)), 0xD98E3E);
        assert_eq!(look.rgb(Color::Pc(11)), 0x6FF0DD);
        assert_eq!(look.rgb(Color::Pc(0)), 0x0E1015);
        set_current(Theme::default());
        assert_eq!(ui_look(), *Look::standard());
        // Written as #RRGGBB, and read back.
        let file = ThemeFile { name: "Copper".into(), author: String::new(), colors: bc };
        let text = toml::to_string_pretty(&file).unwrap();
        assert!(text.contains("\"#D98E3E\""), "{text}");
        assert_eq!(toml::from_str::<ThemeFile>(&text).unwrap(), file);
        assert!(toml::from_str::<Theme>("palette = [\"#123456\"]").is_err());
    }

    /// Every theme offered for download on the website (`extras/themes`)
    /// loads, and none has a built-in scheme's name.
    #[test]
    fn extra_themes_load() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extras/themes");
        let mut n = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|x| x == "toml") {
                let file = ThemeFile::load(&path).unwrap();
                assert!(SCHEMES.iter().all(|(name, _)| !name.eq_ignore_ascii_case(&file.name)), "{}", file.name);
                assert!(!file.author.is_empty(), "{}", path.display());
                n += 1;
            }
        }
        assert!(n > 0, "no themes in {}", dir.display());
    }

    #[test]
    fn partial_toml_keeps_the_rest() {
        let t: Theme = toml::from_str("selected = { fg = 14, bg = 4 }").unwrap();
        assert_eq!(t.selected, Attr::new(14, 4));
        assert_eq!(t.window, Theme::default().window);
    }

    #[test]
    fn theme_files_round_trip_with_names() {
        let dir = std::env::temp_dir().join(format!("badterm-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let colors = Theme { selected: Attr::new(14, 4), ..Theme::default() };
        let file = ThemeFile { name: "Sunrise".into(), author: "sysop".into(), colors };
        let path = dir.join(file_name(&file.name));
        file.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("fg = \"yellow\"") && text.contains("bg = \"red\""), "{text}");
        assert_eq!(ThemeFile::load(&path).unwrap(), file);
        assert_eq!(user_themes(&dir), vec![("Sunrise".to_string(), colors)]);

        // Hand-written: names in any case, "grey", and parts left out.
        std::fs::write(
            dir.join("b.toml"),
            "name = \"Hand made\"\n[colors]\ntitle_bar = { fg = \"Light-Grey\", bg = 4 }\n",
        )
        .unwrap();
        std::fs::write(dir.join("broken.toml"), "name = 3").unwrap();
        let themes = user_themes(&dir);
        assert_eq!(themes.len(), 2);
        assert_eq!(themes[0].1.title_bar, Attr::new(7, 4));
        assert_eq!(themes[0].1.window, Theme::default().window);
        assert!(toml::from_str::<Attr>("fg = \"puce\"\nbg = 0").is_err());
        assert_eq!(file_name("  My Cool Theme! "), "my-cool-theme.toml");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
