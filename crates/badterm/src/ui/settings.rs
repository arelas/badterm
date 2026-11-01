//! The settings screen (Alt-O): display, sound and calling options, and the
//! interface colors, either a ready-made scheme or each part picked from a
//! grid of the 16 x 8 color combinations, as in QModem and Telix.

use std::path::PathBuf;

use badterm_core::screen::{Cell, Screen};

use super::theme::{self, Attr, SCHEMES, Theme};
use super::*;
use crate::crt::{CrtMode, Curvature, Glow, Scanlines};
use crate::input::{Key, Mods};
use badterm_core::modem::FlowControl;

use crate::settings::{Aspect, BellMode, Music, ScaleMode, Settings};

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

const ITEMS: [&str; 28] = [
    "Phosphor mode",
    "Scanlines",
    "Character cells",
    "Aspect ratio",
    "Monitor bezel",
    "ANSI music",
    "Redial busy BBSes",
    "Check for updates",
    "Keyboard macros...",
    "Color scheme",
    "Edit colors...",
    "Import a theme...",
    "Save as a theme...",
    "Downloads",
    "Captures",
    "Saved screens",
    "Uploads start in",
    "ANSI art starts in",
    "Curvature",
    "Scaling",
    "Splash screen",
    "Bell",
    "Serial port",
    "Speed",
    "Flow control",
    "Init string",
    "Dial command",
    "Glow",
];
const I_CRT: usize = 0;
const I_SCANLINES: usize = 1;
const I_NINE_DOT: usize = 2;
const I_ASPECT: usize = 3;
const I_BEZEL: usize = 4;
const I_MUSIC: usize = 5;
const I_REDIAL: usize = 6;
const I_UPDATES: usize = 7;
const I_MACROS: usize = 8;
const I_SCHEME: usize = 9;
const I_COLORS: usize = 10;
const I_IMPORT: usize = 11;
const I_EXPORT: usize = 12;
/// The folders, in `Folders::NAMES` order.
const I_FOLDER: usize = 13;
const I_CURVATURE: usize = 18;
const I_SCALING: usize = 19;
const I_SPLASH: usize = 20;
const I_BELL: usize = 21;
const I_PORT: usize = 22;
const I_SPEED: usize = 23;
const I_FLOW: usize = 24;
const I_INIT: usize = 25;
const I_DIAL: usize = 26;
const I_GLOW: usize = 27;
/// Items typed in, like the folders: Enter opens a prompt.
const TEXT_ITEMS: [usize; 3] = [I_PORT, I_INIT, I_DIAL];
/// Items that open something rather than having a value.
const ACTIONS: [usize; 4] = [I_MACROS, I_COLORS, I_IMPORT, I_EXPORT];

/// The tabs, and the items on each.
const TABS: [(&str, &[usize]); 5] = [
    ("Display", &[I_CRT, I_GLOW, I_SCANLINES, I_CURVATURE, I_NINE_DOT, I_ASPECT, I_SCALING, I_BEZEL]),
    ("General", &[I_SPLASH, I_MUSIC, I_BELL, I_REDIAL, I_UPDATES, I_MACROS]),
    ("Folders", &[I_FOLDER, I_FOLDER + 1, I_FOLDER + 2, I_FOLDER + 3, I_FOLDER + 4]),
    ("Colors", &[I_SCHEME, I_COLORS, I_IMPORT, I_EXPORT]),
    ("Modem", &[I_PORT, I_SPEED, I_FLOW, I_INIT, I_DIAL]),
];

fn is_folder(item: usize) -> bool {
    (I_FOLDER..I_FOLDER + 5).contains(&item)
}

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
    /// Editing the keyboard macros: which one, and its text.
    Macros(usize, TextField),
    Message(String),
}

#[derive(Default)]
pub struct SettingsUi {
    tab: usize,
    /// The item chosen on each tab (its position there).
    item_on: [usize; 5],
    /// The item chosen (its number), as `item_on` gives it.
    item: usize,
    part: usize,
    mode: Mode,
    /// The user's themes, after the built-in schemes in the list.
    themes: Vec<(String, Theme)>,
    /// The bezels to choose from, by name.
    bezels: Vec<String>,
    /// The folders BadTerm uses when none is chosen.
    folder_defaults: Vec<PathBuf>,
}

impl SettingsUi {
    /// Start at the menu, as the screen opens, with the themes and bezels
    /// there are now.
    pub fn open(&mut self, themes: Vec<(String, Theme)>, bezels: Vec<String>, folder_defaults: Vec<PathBuf>) {
        self.mode = Mode::Menu;
        self.themes = themes;
        self.bezels = bezels;
        self.folder_defaults = folder_defaults;
        self.item = TABS[self.tab].1[self.item_on[self.tab]];
    }

    fn select(&mut self, tab: usize, pos: usize) {
        self.tab = tab;
        self.item_on[tab] = pos;
        self.item = TABS[tab].1[pos];
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
            Mode::Menu => self.menu_key(key, mods, settings),
            Mode::Message(_) => {
                self.mode = Mode::Menu;
                Action::None
            }
            Mode::Macros(line, field) => {
                let n = crate::macros::COUNT;
                let next = match key {
                    Key::Escape => {
                        self.mode = Mode::Menu;
                        return Action::None;
                    }
                    Key::Up => (*line + n - 1) % n,
                    Key::Down | Key::Enter | Key::Tab => (*line + 1) % n,
                    _ => {
                        field.handle_key(key, mods);
                        settings.macros.resize(n, String::new());
                        settings.macros[*line] = field.value.clone();
                        return Action::Changed;
                    }
                };
                settings.macros.resize(n, String::new());
                self.mode = Mode::Macros(next, TextField::new(&settings.macros[next], 200));
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
                        // An empty folder is the default one.
                        _ if is_folder(item) => self.set_folder(item, &value, settings),
                        I_PORT => {
                            settings.modem.device = value;
                            Action::Changed
                        }
                        I_INIT => {
                            settings.modem.init = value;
                            Action::Changed
                        }
                        I_DIAL => {
                            settings.modem.dial = value;
                            Action::Changed
                        }
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

    /// Choose a folder: a path, or empty for the default. It is created
    /// if it does not exist.
    fn set_folder(&mut self, item: usize, value: &str, settings: &mut Settings) -> Action {
        let i = item - I_FOLDER;
        if value.is_empty() {
            settings.folders.set(i, None);
            return Action::Changed;
        }
        let path = expand_home(value);
        match std::fs::create_dir_all(&path) {
            Ok(()) => {
                settings.folders.set(i, Some(path));
                Action::Changed
            }
            Err(e) => {
                self.mode = Mode::Message(format!("Could not use {}: {e}", path.display()));
                Action::None
            }
        }
    }

    /// The folder an item shows: the chosen one, or the default.
    fn folder(&self, item: usize, settings: &Settings) -> (PathBuf, bool) {
        let i = item - I_FOLDER;
        match settings.folders.get(i) {
            Some(p) => (p.clone(), true),
            None => (self.folder_defaults.get(i).cloned().unwrap_or_default(), false),
        }
    }

    fn menu_key(&mut self, key: Key, mods: Mods, settings: &mut Settings) -> Action {
        let items = TABS[self.tab].1;
        let n = items.len();
        let pos = self.item_on[self.tab];
        let back = key == Key::Left;
        match key {
            Key::Up => self.select(self.tab, (pos + n - 1) % n),
            Key::Down => self.select(self.tab, (pos + 1) % n),
            Key::Home => self.select(self.tab, 0),
            Key::End => self.select(self.tab, n - 1),
            // Tabs: Tab and Shift-Tab, or PgUp and PgDn.
            Key::Tab | Key::PageDown | Key::PageUp => {
                let t = TABS.len();
                let tab = if key == Key::PageUp || mods.shift { (self.tab + t - 1) % t } else { (self.tab + 1) % t };
                self.select(tab, self.item_on[tab]);
            }
            Key::Escape => return Action::Close,
            Key::Enter if is_folder(self.item) => {
                let (path, _) = self.folder(self.item, settings);
                self.mode = Mode::Prompt(self.item, TextField::new(&path.to_string_lossy(), 1000));
            }
            Key::Delete | Key::Backspace if is_folder(self.item) => {
                settings.folders.set(self.item - I_FOLDER, None);
                return Action::Changed;
            }
            _ if is_folder(self.item) => {}
            Key::Enter if TEXT_ITEMS.contains(&self.item) => {
                let m = &settings.modem;
                let value = match self.item {
                    I_PORT => m.device.clone(),
                    I_INIT => m.init.clone(),
                    _ => m.dial.clone(),
                };
                self.mode = Mode::Prompt(self.item, TextField::new(&value, 200));
            }
            // The serial port also steps through the ports the system lists.
            Key::Left | Key::Right if self.item == I_PORT => {
                let ports = badterm_core::modem::ports();
                if !ports.is_empty() {
                    let i = ports.iter().position(|p| *p == settings.modem.device);
                    let n = ports.len();
                    let next = match (i, back) {
                        (None, _) => 0,
                        (Some(i), true) => (i + n - 1) % n,
                        (Some(i), false) => (i + 1) % n,
                    };
                    settings.modem.device = ports[next].clone();
                    return Action::Changed;
                }
            }
            _ if TEXT_ITEMS.contains(&self.item) => {}
            Key::Enter if self.item == I_COLORS => self.mode = Mode::Parts,
            Key::Enter if self.item == I_MACROS => {
                let first = settings.macros.first().cloned().unwrap_or_default();
                self.mode = Mode::Macros(0, TextField::new(&first, 200));
            }
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
            I_GLOW => {
                let all = Glow::ALL;
                let i = all.iter().position(|&g| g == s.glow).unwrap_or(0);
                s.glow = all[step(i, all.len())];
            }
            I_SCANLINES => {
                let all = Scanlines::ALL;
                let i = all.iter().position(|&m| m == s.scanlines).unwrap_or(0);
                s.scanlines = all[step(i, all.len())];
            }
            I_CURVATURE => {
                let all = Curvature::ALL;
                let i = all.iter().position(|&m| m == s.curvature).unwrap_or(0);
                s.curvature = all[step(i, all.len())];
            }
            I_NINE_DOT => s.nine_dot = !s.nine_dot,
            I_ASPECT => {
                let all = Aspect::ALL;
                let i = all.iter().position(|&a| a == s.aspect).unwrap_or(0);
                s.aspect = all[step(i, all.len())];
            }
            I_SCALING => {
                let all = ScaleMode::ALL;
                let i = all.iter().position(|&m| m == s.scaling).unwrap_or(0);
                s.scaling = all[step(i, all.len())];
            }
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
            I_BELL => {
                let all = BellMode::ALL;
                let i = all.iter().position(|&m| m == s.bell).unwrap_or(0);
                s.bell = all[step(i, all.len())];
            }
            I_SPEED => {
                let all = badterm_core::modem::SPEEDS;
                let i = all.iter().position(|&v| v == s.modem.speed).unwrap_or(all.len() - 2);
                s.modem.speed = all[step(i, all.len())];
            }
            I_FLOW => {
                let all = FlowControl::ALL;
                let i = all.iter().position(|&f| f == s.modem.flow).unwrap_or(0);
                s.modem.flow = all[step(i, all.len())];
            }
            I_REDIAL => s.redial = !s.redial,
            I_SPLASH => s.splash = !s.splash,
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
        let title = format!(" BadTerm {} \u{2502} Settings", env!("CARGO_PKG_VERSION"));
        text(s, 0, 0, COLS, &title, t.title_bar.fg(), t.title_bar.bg());
        let mut cursor = None;
        match &self.mode {
            Mode::Menu => self.draw_menu(s, settings, own_crt, update),
            Mode::Parts | Mode::Grid { .. } => self.draw_colors(s, settings),
            Mode::Prompt(item, field) => {
                self.draw_menu(s, settings, own_crt, update);
                let (title, hint) = match *item {
                    I_IMPORT => ("Import a Theme", "A BadTerm theme file (.toml), from anywhere"),
                    I_PORT => ("Serial Port", "/dev/ttyUSB0, /dev/cu.usbmodem1101, COM3..."),
                    I_INIT => ("Init String", "Sent before dialing, e.g. ATZ or AT&F E1 V1 X4"),
                    I_DIAL => ("Dial Command", "Before the number: ATDT for tone, ATDP for pulse"),
                    i if is_folder(i) => ("Folder", "Empty for the default"),
                    _ => ("Save as a Theme", "A name for these colors, to share or keep"),
                };
                let (x, y, w) = (6, 9, 68);
                frame(s, x, y, w, 6, title, t.window.fg(), t.window.bg());
                label(s, x + 2, y + 1, hint, t.window_text.fg(), t.window_text.bg());
                cursor = Some(field.draw(s, x + 2, y + 3, w - 4, false, t.input.fg(), t.input.bg()));
            }
            Mode::Macros(line, field) => cursor = Some(self.draw_macros(s, settings, *line, field)),
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
            Mode::Menu => "Tab Next tab   \u{2191}\u{2193} Choose   \u{2190}\u{2192} Change   Enter Open   Esc Done",
            Mode::Parts => "\u{2191}\u{2193} Choose a part   Enter Pick its colors   Esc Back",
            Mode::Grid { .. } => "\u{2190}\u{2192} Foreground   \u{2191}\u{2193} Background   Enter Keep   Esc Undo",
            Mode::Prompt(..) => "Enter OK   Esc Cancel",
            Mode::Macros(..) => "\u{2191}\u{2193} Choose   Type to change (saved as you go)   Esc Done",
            Mode::Message(_) => "Press any key",
        };
        text(s, 1, 24, COLS - 2, help, t.help.fg(), t.help.bg());
        cursor
    }

    fn draw_macros(&self, s: &mut Screen, settings: &Settings, line: usize, field: &TextField) -> (usize, usize) {
        let t = theme::current();
        let (x, y, w) = (3, 2, COLS - 6);
        frame(s, x, y, w, 21, "Keyboard Macros", t.window.fg(), t.window.bg());
        let mut cursor = (0, 0);
        for i in 0..crate::macros::COUNT {
            let yy = y + 2 + i;
            let focused = i == line;
            let name_attr = if focused { t.keys } else { t.field_name };
            let k = format!("{HOTKEY}-{}", crate::macros::key_name(i));
            text(s, x + 2, yy, 7, &format!("{k:>6}"), name_attr.fg(), name_attr.bg());
            if focused {
                cursor = field.draw(s, x + 10, yy, w - 12, false, t.input.fg(), t.input.bg());
            } else {
                let v = settings.macros.get(i).map(String::as_str).unwrap_or("");
                text(s, x + 10, yy, w - 12, &truncate(v, w - 12), t.field_value.fg(), t.field_value.bg());
            }
        }
        let help = [
            "Sent to the BBS when you press the key. In a macro:",
            "  ^M  Enter          ^[  Esc        ^A to ^Z  other control keys",
            "  ~   wait 1/2 sec   ^^  a ^        ^~  a ~",
            "  {user}  {password}   the dialing directory entry's",
            "For example:  {user}^M~{password}^M   logs in.",
        ];
        for (i, l) in help.iter().enumerate() {
            label(s, x + 2, y + 14 + i, l, t.window_text.fg(), t.window_text.bg());
        }
        cursor
    }

    fn draw_menu(&self, s: &mut Screen, settings: &Settings, own_crt: Option<&str>, update: Option<&str>) {
        let t = theme::current();
        let (w, h) = (72, 16);
        let (x, y) = ((COLS - w) / 2, 3);
        frame(s, x, y, w, h, "Settings", t.window.fg(), t.window.bg());
        // The tabs, along the top as in NetRunner.
        let mut tx = x + 2;
        for (i, (name, _)) in TABS.iter().enumerate() {
            let a = if i == self.tab { t.selected } else { t.window_text };
            let label_text = format!(" {name} ");
            text(s, tx, y + 1, label_text.len(), &label_text, a.fg(), a.bg());
            tx += label_text.len() + 1;
        }
        divider(s, x, y + 2, w, t.window.fg(), t.window.bg());
        for (pos, &item) in TABS[self.tab].1.iter().enumerate() {
            let yy = y + 4 + pos;
            let focused = item == self.item;
            let name_attr = if focused { t.keys } else { t.field_name };
            text(s, x + 2, yy, 20, &format!("{:>19}", ITEMS[item]), name_attr.fg(), name_attr.bg());
            let a = if focused { t.input } else { t.field_value };
            if is_folder(item) {
                let (path, chosen) = self.folder(item, settings);
                let shown = path.to_string_lossy().into_owned();
                let room = w - 26;
                let shown = if shown.chars().count() > room {
                    format!("\u{2026}{}", shown.chars().skip(shown.chars().count() + 1 - room).collect::<String>())
                } else {
                    shown
                };
                text(s, x + 23, yy, room, &shown, a.fg(), a.bg());
                if !chosen {
                    let d = t.note;
                    label(s, x + w - 11, yy, "(default)", d.fg(), if focused { a.bg() } else { d.bg() });
                }
                continue;
            }
            if ACTIONS.contains(&item) {
                continue;
            }
            let v = format!("\u{25C4} {:<22} \u{25BA}", self.value(item, settings));
            text(s, x + 23, yy, 26, &v, a.fg(), a.bg());
        }
        let note = match (self.item, own_crt, update) {
            (I_UPDATES, _, Some(v)) => format!("BadTerm {v} is out: {}", badterm_core::update::DOWNLOAD_PAGE),
            (I_UPDATES, _, None) => "When on, asks git.badcluster.dev for the newest version".to_string(),
            (I_BELL, ..) => "Ctrl-G from the BBS; an entry in the directory can have its own".to_string(),
            (I_PORT, ..) => "Enter to type it; \u{2190}\u{2192} step through the ports found".to_string(),
            (I_SPEED, ..) => "Between the computer and the modem, not the call's speed".to_string(),
            (I_FLOW, ..) => "RTS/CTS for almost every modem".to_string(),
            (I_INIT | I_DIAL, ..) => "For directory entries whose protocol is Modem".to_string(),
            (I_CRT, Some(own), _) => format!("This BBS has its own phosphor mode: {own}"),
            (I_GLOW, ..) => "How much bright text glows onto the dark around it".to_string(),
            (I_SCANLINES, ..) => "In phosphor mode; they show from twice the size up".to_string(),
            (I_CURVATURE, ..) => "How much phosphor mode's glass bulges".to_string(),
            (I_ASPECT, ..) => "The text area's shape; Alt-A cycles through them".to_string(),
            (I_SCALING, ..) => match settings.scaling {
                ScaleMode::Auto => "Whole numbers in a window, fit when full screen".to_string(),
                ScaleMode::Integer => "Every pixel exact; a border fills the rest".to_string(),
                ScaleMode::Fit => "As large as fits, keeping the shape".to_string(),
                ScaleMode::Stretch => "Fills the window; the shape follows it".to_string(),
            },
            (I_BEZEL, ..) => "Your own: a PNG with a see-through screen, in bezels/".to_string(),
            (I_SCHEME | I_IMPORT | I_EXPORT, ..) => {
                "Themes are kept in the themes folder (badterm --paths)".to_string()
            }
            (i, ..) if is_folder(i) => "Enter to change (created if need be); Del for the default".to_string(),
            _ => String::new(),
        };
        label(s, x + 2, y + h - 2, &note, t.note.fg(), t.note.bg());
    }

    /// An item's current value, as shown.
    fn value(&self, item: usize, s: &Settings) -> String {
        match item {
            I_CRT => s.crt.name().to_string(),
            I_GLOW => s.glow.name().into(),
            I_SCANLINES => s.scanlines.name().into(),
            I_CURVATURE => s.curvature.name().into(),
            I_NINE_DOT => if s.nine_dot { "9 dots, as on a VGA" } else { "8 dots" }.into(),
            I_ASPECT => s.aspect.name().into(),
            I_SCALING => s.scaling.name().into(),
            I_BEZEL if s.bezel.is_empty() => "None".into(),
            I_BEZEL => s.bezel.clone(),
            I_MUSIC => match s.music {
                Music::Off => "Off",
                Music::On => "On",
                Music::All => "On, including ESC [ M",
            }
            .into(),
            I_REDIAL => if s.redial { "On" } else { "Off" }.into(),
            I_BELL => {
                let name = s.bell.name();
                let mut c = name.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
            I_PORT if s.modem.device.is_empty() => "(none chosen)".into(),
            I_PORT => s.modem.device.clone(),
            I_SPEED => format!("{} bps", s.modem.speed),
            I_FLOW => s.modem.flow.name().into(),
            I_INIT => s.modem.init.clone(),
            I_DIAL => s.modem.dial.clone(),
            I_SPLASH => if s.splash { "On, 2 seconds" } else { "Off" }.into(),
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
    fn tabs_and_folders() {
        let mut ui = SettingsUi::default();
        let dir = std::env::temp_dir().join(format!("badterm-folders-{}", std::process::id()));
        ui.open(Vec::new(), Vec::new(), vec![PathBuf::from("/default"); 5]);
        let mut s = Settings::default();
        // Shift-Tab goes back, round to the last tab.
        ui.handle_key(Key::Tab, Mods { shift: true, ..Mods::default() }, &mut s);
        assert_eq!((ui.tab, ui.item), (4, I_PORT));
        // The modem: a speed to step through, a port to type.
        ui.handle_key_test(Key::Down, &mut s);
        assert_eq!(ui.handle_key_test(Key::Right, &mut s), Action::Changed);
        assert_eq!(s.modem.speed, 230_400);
        ui.handle_key_test(Key::Up, &mut s);
        ui.handle_key_test(Key::Enter, &mut s);
        let Mode::Prompt(_, field) = &mut ui.mode else { panic!() };
        *field = TextField::new("/dev/ttyS0", 200);
        assert_eq!(ui.handle_key_test(Key::Enter, &mut s), Action::Changed);
        assert_eq!(s.modem.device, "/dev/ttyS0");
        ui.handle_key_test(Key::PageUp, &mut s);
        assert_eq!((ui.tab, ui.item), (3, I_SCHEME));
        ui.handle_key_test(Key::PageUp, &mut s);
        assert_eq!((ui.tab, ui.item), (2, I_FOLDER));
        // Each tab remembers its place.
        ui.handle_key_test(Key::Down, &mut s);
        ui.handle_key_test(Key::Tab, &mut s);
        ui.handle_key_test(Key::PageUp, &mut s);
        assert_eq!(ui.item, I_FOLDER + 1);
        // Choose a folder (made if missing), then go back to the default.
        ui.handle_key_test(Key::Enter, &mut s);
        let Mode::Prompt(_, field) = &mut ui.mode else { panic!() };
        *field = TextField::new(&dir.to_string_lossy(), 1000);
        assert_eq!(ui.handle_key_test(Key::Enter, &mut s), Action::Changed);
        assert_eq!(s.folders.captures.as_deref(), Some(dir.as_path()));
        assert!(dir.is_dir());
        let mut screen = Screen::new(COLS, ROWS, 0);
        ui.draw(&mut screen, &s, None, None);
        assert_eq!(ui.handle_key_test(Key::Delete, &mut s), Action::Changed);
        assert_eq!(s.folders.captures, None);
        ui.draw(&mut screen, &s, None, None);
        let rows: Vec<String> = (0..ROWS).map(|y| Screen::row_text(screen.row(y))).collect();
        assert!(rows.iter().any(|r| r.contains("Captures") && r.contains("(default)")), "{rows:#?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn macro_editor() {
        let mut ui = SettingsUi { item: I_MACROS, ..SettingsUi::default() };
        let mut s = Settings::default();
        ui.handle_key_test(Key::Enter, &mut s);
        ui.handle_key_test(Key::Down, &mut s);
        for c in "hi^M".chars() {
            assert_eq!(ui.handle_key_test(Key::Char(c), &mut s), Action::Changed);
        }
        assert_eq!(s.macros[1], "hi^M");
        let mut screen = Screen::new(COLS, ROWS, 0);
        assert!(ui.draw(&mut screen, &s, None, None).is_some());
        // Back to the first, then out.
        ui.handle_key_test(Key::Up, &mut s);
        ui.handle_key_test(Key::Escape, &mut s);
        assert_eq!(ui.handle_key_test(Key::Escape, &mut s), Action::Close);
    }

    #[test]
    fn themes_bezels_import_and_export() {
        let mut ui = SettingsUi::default();
        let mut mine = Theme::default();
        mine.window.bg = 2;
        ui.open(vec![("Mine".into(), mine)], vec!["Beige".into(), "Platinum".into()], vec![PathBuf::from("/dl"); 5]);
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
        assert_eq!(s.crt, CrtMode::Green);
        assert_eq!(ui.handle_key_test(Key::Left, &mut s), Action::Changed);
        assert_eq!(s.crt, CrtMode::Color);
        // Tab to the colors.
        for _ in 0..3 {
            ui.handle_key_test(Key::Tab, &mut s);
        }
        assert_eq!(ui.item, I_SCHEME);
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
