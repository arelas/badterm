//! The settings screen (Alt-O): display, sound and calling options, and the
//! interface colors, either a ready-made scheme or each part picked from a
//! grid of the 16 x 8 color combinations, as in QModem and Telix.

use std::path::PathBuf;

use phosphor_core::screen::{Cell, Screen};

use super::theme::{self, Attr, SCHEMES, Theme};
use super::*;
use crate::crt::CrtMode;
use crate::input::{Key, Mods};
use crate::settings::{Music, Settings};

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

/// What the app should do after a key.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    /// The settings changed: apply and save them.
    Changed,
    Close,
    /// Import the theme file at this path.
    Import(PathBuf),
    /// Save the current colors as a theme with this name.
    Export(String),
}

const ITEMS: [&str; 11] = [
    "Phosphor mode",
    "Character cells",
    "Pixel shape",
    "Monitor bezel",
    "ANSI music",
    "Redial busy BBSes",
    "Check for updates",
    "Color scheme",
    "Edit colors...",
    "Import a theme...",
    "Save as a theme...",
];
const I_CRT: usize = 0;
const I_NINE_DOT: usize = 1;
const I_ASPECT: usize = 2;
const I_BEZEL: usize = 3;
const I_MUSIC: usize = 4;
const I_REDIAL: usize = 5;
const I_UPDATES: usize = 6;
const I_SCHEME: usize = 7;
const I_COLORS: usize = 8;
const I_IMPORT: usize = 9;
const I_EXPORT: usize = 10;
/// Items that open something rather than having a value.
const ACTIONS: [usize; 3] = [I_COLORS, I_IMPORT, I_EXPORT];

/// Rows of the color grid: the eight background colors (bright ones blink
/// on a real VGA, so the interface leaves them out, as Telix did).
const GRID_BGS: u8 = 8;

#[derive(Default)]
enum Mode {
    #[default]
    Menu,
    /// Choosing which part to color.
    Parts,
    /// Choosing its colors on the grid; `before` is restored by Esc.
    Grid {
        before: Attr,
    },
    /// Asking for a theme file to import, or a name to save one as.
    Prompt(usize, TextField),
    Message(String),
}

#[derive(Default)]
pub struct SettingsUi {
    item: usize,
    part: usize,
    mode: Mode,
    /// The user's themes, after the built-in schemes in the list.
    themes: Vec<(String, Theme)>,
    /// The bezels to choose from, by name.
    bezels: Vec<String>,
}

impl SettingsUi {
    /// Start at the menu, as the screen opens, with the themes and bezels
    /// there are now.
    pub fn open(&mut self, themes: Vec<(String, Theme)>, bezels: Vec<String>) {
        self.mode = Mode::Menu;
        self.themes = themes;
        self.bezels = bezels;
    }

    /// Show the outcome of an import or export.
    pub fn show_message(&mut self, msg: String) {
        self.mode = Mode::Message(msg);
    }

    pub fn set_themes(&mut self, themes: Vec<(String, Theme)>) {
        self.themes = themes;
    }

    /// The name of the scheme or theme the colors are, if any.
    fn scheme_name<'a>(&'a self, colors: &Theme) -> Option<&'a str> {
        colors.scheme_name().or_else(|| self.themes.iter().find(|(_, t)| t == colors).map(|(n, _)| n.as_str()))
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods, settings: &mut Settings) -> Action {
        match &mut self.mode {
            Mode::Menu => self.menu_key(key, settings),
            Mode::Message(_) => {
                self.mode = Mode::Menu;
                Action::None
            }
            Mode::Prompt(item, field) => match key {
                Key::Escape => {
                    self.mode = Mode::Menu;
                    Action::None
                }
                Key::Enter => {
                    let value = field.value.trim().to_string();
                    let item = *item;
                    self.mode = Mode::Menu;
                    match item {
                        _ if value.is_empty() => Action::None,
                        I_IMPORT => Action::Import(PathBuf::from(value)),
                        _ => Action::Export(value),
                    }
                }
                _ => {
                    field.handle_key(key, mods);
                    Action::None
                }
            },
            Mode::Parts => {
                let n = Theme::NAMES.len();
                match key {
                    Key::Up => self.part = (self.part + n - 1) % n,
                    Key::Down => self.part = (self.part + 1) % n,
                    Key::Home => self.part = 0,
                    Key::End => self.part = n - 1,
                    Key::Enter | Key::Right => self.mode = Mode::Grid { before: settings.colors.get(self.part) },
                    Key::Escape | Key::Left => self.mode = Mode::Menu,
                    _ => {}
                }
                Action::None
            }
            Mode::Grid { before } => {
                let before = *before;
                let mut a = settings.colors.get(self.part);
                match key {
                    Key::Left => a.fg = (a.fg + 15) % 16,
                    Key::Right => a.fg = (a.fg + 1) % 16,
                    Key::Up => a.bg = (a.bg + GRID_BGS - 1) % GRID_BGS,
                    Key::Down => a.bg = (a.bg + 1) % GRID_BGS,
                    Key::Enter => {
                        self.mode = Mode::Parts;
                        return Action::None;
                    }
                    Key::Escape => {
                        a = before;
                        self.mode = Mode::Parts;
                    }
                    _ => return Action::None,
                }
                settings.colors.set(self.part, a);
                theme::set_current(settings.colors);
                Action::Changed
            }
        }
    }

    fn menu_key(&mut self, key: Key, settings: &mut Settings) -> Action {
        let n = ITEMS.len();
        let back = key == Key::Left;
        match key {
            Key::Up => self.item = (self.item + n - 1) % n,
            Key::Down | Key::Tab => self.item = (self.item + 1) % n,
            Key::Home => self.item = 0,
            Key::End => self.item = n - 1,
            Key::Escape => return Action::Close,
            Key::Enter if self.item == I_COLORS => self.mode = Mode::Parts,
            Key::Enter if self.item == I_IMPORT => self.mode = Mode::Prompt(I_IMPORT, TextField::new("", 400)),
            Key::Enter if self.item == I_EXPORT => {
                let name = self.scheme_name(&settings.colors).unwrap_or("My colors").to_string();
                let name = if settings.colors.scheme_name().is_some() { format!("My {name}") } else { name };
                self.mode = Mode::Prompt(I_EXPORT, TextField::new(&name, 60));
            }
            _ if ACTIONS.contains(&self.item) => {}
            Key::Left | Key::Right | Key::Char(' ') | Key::Enter => {
                self.change(back, settings);
                return Action::Changed;
            }
            _ => {}
        }
        Action::None
    }

    /// Step the current item to its next (or, `back`, previous) value.
    fn change(&self, back: bool, s: &mut Settings) {
        let step = |i: usize, n: usize| if back { (i + n - 1) % n } else { (i + 1) % n };
        match self.item {
            I_CRT => {
                let all = CrtMode::ALL;
                let i = all.iter().position(|&m| m == s.crt).unwrap_or(0);
                s.crt = all[step(i, all.len())];
            }
            I_NINE_DOT => s.nine_dot = !s.nine_dot,
            I_ASPECT => s.aspect = !s.aspect,
            I_BEZEL => {
                // "None", then each bezel.
                let i = self.bezels.iter().position(|b| *b == s.bezel).map_or(0, |i| i + 1);
                let next = step(i, self.bezels.len() + 1);
                s.bezel = if next == 0 { String::new() } else { self.bezels[next - 1].clone() };
            }
            I_MUSIC => {
                let all = [Music::Off, Music::On, Music::All];
                let i = all.iter().position(|&m| m == s.music).unwrap_or(0);
                s.music = all[step(i, all.len())];
            }
            I_REDIAL => s.redial = !s.redial,
            I_UPDATES => s.check_updates = !s.check_updates,
            I_SCHEME => {
                // The built-in schemes, then the user's themes; from custom
                // colors, the first step goes to the first scheme.
                let all: Vec<Theme> =
                    SCHEMES.iter().map(|(_, t)| *t).chain(self.themes.iter().map(|(_, t)| *t)).collect();
                s.colors = match all.iter().position(|t| *t == s.colors) {
                    Some(i) => all[step(i, all.len())],
                    None => all[0],
                };
                theme::set_current(s.colors);
            }
            _ => {}
        }
    }

    /// Draw the screen; returns the text cursor, if a prompt shows one.
    /// `own_crt`: the connected BBS's own phosphor mode; `update`: a newer
    /// version, if a check found one.
    pub fn draw(
        &self,
        s: &mut Screen,
        settings: &Settings,
        own_crt: Option<&str>,
        update: Option<&str>,
    ) -> Option<(usize, usize)> {
        let t = theme::current();
        s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', t.help.fg(), t.help.bg()));
        let title = format!(" Phosphor {} \u{2502} Settings", env!("CARGO_PKG_VERSION"));
        text(s, 0, 0, COLS, &title, t.title_bar.fg(), t.title_bar.bg());
        let mut cursor = None;
        match &self.mode {
            Mode::Menu => self.draw_menu(s, settings, own_crt, update),
            Mode::Parts | Mode::Grid { .. } => self.draw_colors(s, settings),
            Mode::Prompt(item, field) => {
                self.draw_menu(s, settings, own_crt, update);
                let (title, hint) = if *item == I_IMPORT {
                    ("Import a Theme", "A Phosphor theme file (.toml), from anywhere")
                } else {
                    ("Save as a Theme", "A name for these colors, to share or keep")
                };
                let (x, y, w) = (6, 9, 68);
                frame(s, x, y, w, 6, title, t.window.fg(), t.window.bg());
                label(s, x + 2, y + 1, hint, t.window_text.fg(), t.window_text.bg());
                cursor = Some(field.draw(s, x + 2, y + 3, w - 4, false, t.input.fg(), t.input.bg()));
            }
            Mode::Message(msg) => {
                self.draw_menu(s, settings, own_crt, update);
                let lines = wrap(msg, 60);
                let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) + 6;
                let (x, y) = ((COLS - w) / 2, 10);
                frame(s, x, y, w, lines.len() + 4, "Themes", t.window.fg(), t.window.bg());
                for (i, l) in lines.iter().enumerate() {
                    label(s, x + 3, y + 2 + i, l, t.window.fg(), t.window.bg());
                }
            }
        }
        let help = match self.mode {
            Mode::Menu => "\u{2191}\u{2193} Choose   \u{2190}\u{2192} Change   Enter Open   Esc Done",
            Mode::Parts => "\u{2191}\u{2193} Choose a part   Enter Pick its colors   Esc Back",
            Mode::Grid { .. } => "\u{2190}\u{2192} Foreground   \u{2191}\u{2193} Background   Enter Keep   Esc Undo",
            Mode::Prompt(..) => "Enter OK   Esc Cancel",
            Mode::Message(_) => "Press any key",
        };
        text(s, 1, 24, COLS - 2, help, t.help.fg(), t.help.bg());
        cursor
    }

    fn draw_menu(&self, s: &mut Screen, settings: &Settings, own_crt: Option<&str>, update: Option<&str>) {
        let t = theme::current();
        let (w, h) = (60, ITEMS.len() + 6);
        let (x, y) = ((COLS - w) / 2, 3);
        frame(s, x, y, w, h, "Settings", t.window.fg(), t.window.bg());
        for (i, name) in ITEMS.iter().enumerate() {
            let yy = y + 2 + i + usize::from(i >= I_SCHEME);
            let focused = i == self.item;
            let name_attr = if focused { t.keys } else { t.field_name };
            text(s, x + 2, yy, 20, &format!("{name:>19}"), name_attr.fg(), name_attr.bg());
            if ACTIONS.contains(&i) {
                continue;
            }
            let a = if focused { t.input } else { t.field_value };
            let v = format!("\u{25C4} {:<22} \u{25BA}", self.value(i, settings));
            text(s, x + 23, yy, 26, &v, a.fg(), a.bg());
        }
        let note = match (self.item, own_crt, update) {
            (I_UPDATES, _, Some(v)) => format!("Phosphor {v} is out: {}", phosphor_core::update::DOWNLOAD_PAGE),
            (I_UPDATES, _, None) => "When on, asks git.badcluster.dev for the newest version".to_string(),
            (I_CRT, Some(own), _) => format!("This BBS has its own phosphor mode: {own}"),
            (I_BEZEL, ..) => "Your own: a PNG with a see-through screen, in bezels/".to_string(),
            (I_SCHEME | I_IMPORT | I_EXPORT, ..) => {
                "Themes are kept in the themes folder (phosphor --paths)".to_string()
            }
            _ => String::new(),
        };
        label(s, x + 2, y + h - 2, &note, t.note.fg(), t.note.bg());
    }

    /// An item's current value, as shown.
    fn value(&self, item: usize, s: &Settings) -> String {
        match item {
            I_CRT => s.crt.name().to_string(),
            I_NINE_DOT => if s.nine_dot { "9 dots, as on a VGA" } else { "8 dots" }.into(),
            I_ASPECT => if s.aspect { "4:3, as on a CRT" } else { "Square" }.into(),
            I_BEZEL if s.bezel.is_empty() => "None".into(),
            I_BEZEL => s.bezel.clone(),
            I_MUSIC => match s.music {
                Music::Off => "Off",
                Music::On => "On",
                Music::All => "On, including ESC [ M",
            }
            .into(),
            I_REDIAL => if s.redial { "On" } else { "Off" }.into(),
            I_UPDATES => if s.check_updates { "On, at each start" } else { "Off" }.into(),
            I_SCHEME => self.scheme_name(&s.colors).unwrap_or("Custom").into(),
            _ => String::new(),
        }
    }

    fn draw_colors(&self, s: &mut Screen, settings: &Settings) {
        let t = theme::current();
        let picking = matches!(self.mode, Mode::Grid { .. });

        // The parts, each named in its own colors.
        let (lx, ly, lw) = (1, 2, 36);
        frame(s, lx, ly, lw, Theme::NAMES.len() + 2, "Parts", t.window.fg(), t.window.bg());
        for (i, name) in Theme::NAMES.iter().enumerate() {
            let a = settings.colors.get(i);
            let yy = ly + 1 + i;
            let mark = if i == self.part { '\u{25BA}' } else { ' ' };
            label(s, lx + 1, yy, &mark.to_string(), t.keys.fg(), t.window.bg());
            text(s, lx + 3, yy, 24, &format!(" {name}"), a.fg(), a.bg());
            label(s, lx + 28, yy, &format!("{:>2}/{}", a.fg, a.bg), t.window_text.fg(), t.window_text.bg());
        }

        // The grid: foreground across, background down.
        let (gx, gy) = (40, 2);
        let title = if picking { "Pick colors" } else { "Colors" };
        frame(s, gx, gy, 16 * 2 + 4, GRID_BGS as usize + 2, title, t.window.fg(), t.window.bg());
        let chosen = settings.colors.get(self.part);
        for bg in 0..GRID_BGS {
            for fg in 0..16u8 {
                let a = Attr::new(fg, bg);
                let glyph = if a == chosen { "\u{25C4}\u{25BA}" } else { "Aa" };
                label(s, gx + 2 + fg as usize * 2, gy + 1 + bg as usize, glyph, a.fg(), a.bg());
            }
        }

        // A sample window in the colors as they are now.
        let (px, py, pw) = (40, 13, 36);
        frame(s, px, py, pw, 9, "Sample", t.window.fg(), t.window.bg());
        label(s, px + 2, py + 1, "Window text", t.window_text.fg(), t.window_text.bg());
        text(s, px + 2, py + 3, 10, "     Name", t.field_name.fg(), t.field_name.bg());
        text(s, px + 13, py + 3, 18, "The Vortex BBS", t.field_value.fg(), t.field_value.bg());
        text(s, px + 2, py + 4, 10, "  Address", t.keys.fg(), t.keys.bg());
        text(s, px + 13, py + 4, 18, "vortexbbs.com_", t.input.fg(), t.input.bg());
        label(s, px + 2, py + 5, "(a note)", t.note.fg(), t.note.bg());
        label(s, px + 2, py + 6, "Address is required", t.error.fg(), t.error.bg());
        label(s, px + 2, py + 7, "Enter save   Esc cancel", t.keys.fg(), t.keys.bg());
        let name = self.scheme_name(&settings.colors).unwrap_or("custom");
        label(s, 1, 23, &format!(" Scheme: {name}"), t.info.fg(), t.info.bg());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl SettingsUi {
        fn handle_key_test(&mut self, key: Key, s: &mut Settings) -> Action {
            self.handle_key(key, Mods::default(), s)
        }
    }

    #[test]
    fn themes_bezels_import_and_export() {
        let mut ui = SettingsUi::default();
        let mut mine = Theme::default();
        mine.window.bg = 2;
        ui.open(vec![("Mine".into(), mine)], vec!["Beige".into(), "Platinum".into()]);
        let mut s = Settings::default();
        ui.item = I_BEZEL;
        ui.handle_key_test(Key::Right, &mut s);
        assert_eq!(s.bezel, "Beige");
        ui.handle_key_test(Key::Left, &mut s);
        ui.handle_key_test(Key::Left, &mut s);
        assert_eq!(s.bezel, "Platinum");
        // The user's theme comes after the built-in schemes.
        ui.item = I_SCHEME;
        ui.handle_key_test(Key::Left, &mut s);
        assert_eq!(s.colors, mine);
        assert_eq!(ui.value(I_SCHEME, &s), "Mine");
        // Export offers the theme's name; import takes a path.
        ui.item = I_EXPORT;
        ui.handle_key_test(Key::Enter, &mut s);
        assert_eq!(ui.handle_key_test(Key::Enter, &mut s), Action::Export("Mine".into()));
        ui.item = I_IMPORT;
        ui.handle_key_test(Key::Enter, &mut s);
        for c in "a.toml".chars() {
            ui.handle_key_test(Key::Char(c), &mut s);
        }
        let mut screen = Screen::new(COLS, ROWS, 0);
        assert!(ui.draw(&mut screen, &s, None, None).is_some());
        assert_eq!(ui.handle_key_test(Key::Enter, &mut s), Action::Import("a.toml".into()));
        ui.show_message("Imported".into());
        ui.draw(&mut screen, &s, None, None);
        assert_eq!(ui.handle_key_test(Key::Char('x'), &mut s), Action::None);
        assert_eq!(ui.handle_key_test(Key::Escape, &mut s), Action::Close);
        theme::set_current(Theme::default());
    }

    #[test]
    fn menu_changes_settings() {
        let mut ui = SettingsUi::default();
        let mut s = Settings::default();
        assert_eq!(ui.handle_key_test(Key::Right, &mut s), Action::Changed);
        assert_eq!(s.crt, CrtMode::Color);
        assert_eq!(ui.handle_key_test(Key::Left, &mut s), Action::Changed);
        assert_eq!(s.crt, CrtMode::Off);
        for _ in 0..I_SCHEME {
            ui.handle_key_test(Key::Down, &mut s);
        }
        ui.handle_key_test(Key::Right, &mut s);
        assert_eq!(s.colors, SCHEMES[1].1);
        assert_eq!(theme::current(), SCHEMES[1].1);
        assert_eq!(ui.handle_key_test(Key::Escape, &mut s), Action::Close);
        theme::set_current(Theme::default());
    }

    #[test]
    fn color_editor_picks_and_undoes() {
        let mut ui = SettingsUi { item: I_COLORS, ..SettingsUi::default() };
        let mut s = Settings::default();
        ui.handle_key_test(Key::Enter, &mut s);
        ui.handle_key_test(Key::Down, &mut s); // the directory border
        ui.handle_key_test(Key::Enter, &mut s);
        let before = s.colors.border;
        assert_eq!(ui.handle_key_test(Key::Right, &mut s), Action::Changed);
        assert_eq!(s.colors.border.fg, (before.fg + 1) % 16);
        ui.handle_key_test(Key::Up, &mut s);
        assert_eq!(s.colors.border.bg, (before.bg + 7) % 8);
        assert_eq!(s.colors.scheme_name(), None);
        let mut screen = Screen::new(COLS, ROWS, 0);
        ui.draw(&mut screen, &s, None, None);
        // Esc puts it back; then Esc leaves the editor, and the screen.
        ui.handle_key_test(Key::Escape, &mut s);
        assert_eq!(s.colors.border, before);
        ui.handle_key_test(Key::Escape, &mut s);
        assert_eq!(ui.handle_key_test(Key::Escape, &mut s), Action::Close);
        ui.draw(&mut screen, &s, Some("amber screen"), Some("9.9.9"));
        theme::set_current(Theme::default());
    }
}
