//! The dialing directory screen: list, add/edit form, quick connect, and
//! import/export.

use std::path::PathBuf;
use std::rc::Rc;

use badterm_core::Encoding;
use badterm_core::dialer::{self, BellMode, CrtMode, Directory, Entry, Protocol};
use badterm_core::screen::{Cell, Screen};

use super::*;
use crate::input::{Key, Mods};
use crate::render::{CODEPAGE_FONTS, EXTRA_FONTS};
use crate::secrets::{self, Store};

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

const LIST_TOP: usize = 4;
const LIST_ROWS: usize = 17;

pub enum Action {
    None,
    /// Connect to the directory entry at this index.
    Connect(usize),
    /// Connect to an address that is not (yet) in the directory.
    QuickConnect(Box<Entry>),
    /// The directory changed and should be saved.
    Changed,
    /// The user confirmed hanging up the current call to dial the one chosen.
    HangUpAndDial,
    /// Close the dialing directory.
    Close,
    /// Open the settings screen.
    Settings,
}

const FIELDS: [&str; 23] = [
    "Name",
    "Group",
    "Address",
    "Port",
    "Protocol",
    "Emulation",
    "Screen mode",
    "Font",
    "iCE colors",
    "Phosphor mode",
    "Bell",
    "Status line",
    "Baud rate",
    "Terminal type",
    "User name",
    "Password",
    "Auto-login",
    "SSH key file",
    "Capture",
    "Capture file",
    "Downloads",
    "Uploads",
    "Comment",
];
const F_GROUP: usize = 1;
const F_ADDRESS: usize = 2;
const F_PORT: usize = 3;
const F_PROTOCOL: usize = 4;
const F_ENCODING: usize = 5;
const F_ROWS: usize = 6;
const F_FONT: usize = 7;
const F_ICE: usize = 8;
const F_CRT: usize = 9;
const F_BELL: usize = 10;
const F_STATUS: usize = 11;
const F_BAUD: usize = 12;
const F_TERM: usize = 13;
const F_USER: usize = 14;
const F_PASSWORD: usize = 15;
const F_AUTO_LOGIN: usize = 16;
const F_KEY_FILE: usize = 17;
const F_CAPTURE: usize = 18;
const F_CAPTURE_FILE: usize = 19;
const F_DOWNLOADS: usize = 20;
const F_UPLOADS: usize = 21;
const F_COMMENT: usize = 22;
/// The fields changed with Space or the arrow keys rather than typed into.
const OPTION_FIELDS: [usize; 11] =
    [F_PROTOCOL, F_ENCODING, F_ROWS, F_FONT, F_ICE, F_CRT, F_BELL, F_STATUS, F_BAUD, F_AUTO_LOGIN, F_CAPTURE];
/// How many fields the form shows at once; it scrolls through the rest.
const FORM_ROWS: usize = 19;

struct Form {
    /// Index of the entry being edited, or None for a new entry.
    index: Option<usize>,
    base: Entry,
    text: [TextField; 23],
    /// The stored password could not be read from the keychain, so an empty
    /// password field means "unchanged" rather than "none".
    keychain_unread: bool,
    protocol: Protocol,
    encoding: Encoding,
    /// (columns, rows)
    mode: (u16, u16),
    /// CTerm font ID.
    font: u8,
    /// None: up to the BBS.
    ice: Option<bool>,
    /// None: the global phosphor mode (Alt-P).
    crt: Option<CrtMode>,
    /// None: the global setting.
    bell: Option<BellMode>,
    baud: u32,
    auto_login: bool,
    capture: bool,
    hide_status: bool,
    focus: usize,
    /// The first field shown.
    top: usize,
    error: Option<String>,
}

impl Form {
    /// Keep the field with the focus in view.
    fn scroll(&mut self) {
        if self.focus < self.top {
            self.top = self.focus;
        } else if self.focus >= self.top + FORM_ROWS {
            self.top = self.focus + 1 - FORM_ROWS;
        }
    }

    fn new(index: Option<usize>, e: Entry, store: &dyn Store) -> (Form, Option<String>) {
        let t = |s: &str| TextField::new(s, 120);
        let (password, problem) = match secrets::password(&e, store) {
            Ok(p) => (p, None),
            Err(err) => (String::new(), Some(format!("Could not read the password from the keychain: {err}"))),
        };
        let form = Form {
            index,
            text: [
                t(&e.name),
                t(&e.group),
                t(&e.host),
                TextField::new(&e.port.to_string(), 5),
                TextField::default(),
                TextField::default(),
                TextField::default(),
                TextField::default(),
                TextField::default(),
                TextField::default(),
                TextField::default(),
                TextField::default(),
                TextField::default(),
                t(&e.term_type),
                t(&e.username),
                t(&password),
                TextField::default(),
                TextField::new(&e.key_file, 400),
                TextField::default(),
                TextField::new(&e.capture_file, 400),
                TextField::new(&e.download_dir, 400),
                TextField::new(&e.upload_dir, 400),
                t(&e.comment),
            ],
            keychain_unread: problem.is_some(),
            protocol: e.protocol,
            encoding: e.encoding,
            mode: e.screen_mode(),
            font: e.font,
            ice: e.ice,
            crt: e.crt,
            bell: e.bell,
            baud: e.baud,
            auto_login: e.auto_login,
            capture: e.capture,
            hide_status: e.hide_status,
            focus: 0,
            top: 0,
            error: problem.clone(),
            base: e,
        };
        (form, problem)
    }

    fn to_entry(&self) -> Result<Entry, String> {
        let host = self.text[F_ADDRESS].value.trim();
        if host.is_empty() {
            let what = if self.protocol == Protocol::Modem { "The phone number" } else { "Address" };
            return Err(format!("{what} is required"));
        }
        let modem = self.protocol == Protocol::Modem;
        // Allow "host:port" or a URL in the address field; a phone number is
        // taken as it is.
        let parsed = if modem {
            Entry { host: host.to_string(), port: 0, ..Entry::default() }
        } else {
            Entry::from_address(host).ok_or("Invalid address")?
        };
        let mut port: u16 =
            if modem { 0 } else { self.text[F_PORT].value.trim().parse().map_err(|_| "Port must be 1-65535")? };
        if port == 0 && !modem {
            return Err("Port must be 1-65535".into());
        }
        if !modem && parsed.port != parsed.protocol.default_port() {
            port = parsed.port;
        }
        if self.protocol == Protocol::Ssh && self.text[F_USER].value.trim().is_empty() {
            return Err("SSH needs a user name (BBSes often accept \"new\" or \"guest\")".into());
        }
        let name = self.text[0].value.trim();
        let password = self.text[F_PASSWORD].value.clone();
        // A password still in the keychain stays there; a new one is moved
        // there when the directory is saved.
        let kept = self.keychain_unread && password.is_empty() && self.base.keychain;
        Ok(Entry {
            name: if name.is_empty() { parsed.host.clone() } else { name.to_string() },
            host: parsed.host,
            port,
            protocol: self.protocol,
            encoding: self.encoding,
            term_type: self.text[F_TERM].value.trim().to_string(),
            cols: self.mode.0,
            rows: self.mode.1,
            font: self.font,
            ice: self.ice,
            crt: self.crt,
            bell: self.bell,
            baud: self.baud,
            auto_login: self.auto_login,
            capture: self.capture,
            capture_file: self.text[F_CAPTURE_FILE].value.trim().to_string(),
            download_dir: self.text[F_DOWNLOADS].value.trim().to_string(),
            upload_dir: self.text[F_UPLOADS].value.trim().to_string(),
            hide_status: self.hide_status,
            group: self.text[F_GROUP].value.trim().to_string(),
            username: self.text[F_USER].value.clone(),
            password,
            keychain: kept,
            key_file: self.text[F_KEY_FILE].value.trim().to_string(),
            comment: self.text[F_COMMENT].value.clone(),
            added: self.base.added.or(Some(dialer::now())),
            ..self.base.clone()
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    QuickConnect,
    Import,
    Export,
}

enum Mode {
    List,
    Edit(Box<Form>),
    Prompt(PromptKind, TextField),
    ConfirmDelete,
    /// Dialing while connected: asks before hanging up the current call.
    ConfirmHangUp(String),
    Message(String),
}

pub struct DialerUi {
    /// Position of the selected entry in the listed view (see
    /// [`Directory::view`]), not its index in the directory.
    selected: usize,
    top: usize,
    /// Only entries matching this are listed.
    filter: TextField,
    /// Keys go to the filter (after `/`).
    finding: bool,
    /// Only this group's entries are listed.
    group: Option<String>,
    /// What an added entry starts as.
    pub new_entry: Entry,
    mode: Mode,
    /// Default directory offered for import/export paths.
    pub files_dir: PathBuf,
    secrets: Rc<dyn Store>,
}

impl DialerUi {
    pub fn new(files_dir: PathBuf, secrets: Rc<dyn Store>) -> DialerUi {
        DialerUi {
            selected: 0,
            top: 0,
            filter: TextField::new("", 60),
            finding: false,
            group: None,
            new_entry: Entry::default(),
            mode: Mode::List,
            files_dir,
            secrets,
        }
    }

    pub fn show_message(&mut self, msg: impl Into<String>) {
        self.mode = Mode::Message(msg.into());
    }

    /// Ask whether to hang up `current` to dial `next`; answers with
    /// [`Action::HangUpAndDial`] on yes.
    pub fn confirm_hang_up(&mut self, current: &str, next: &str) {
        let msg = format!("Hang up {} and dial {}?", truncate(current, 24), truncate(next, 24));
        self.mode = Mode::ConfirmHangUp(msg);
    }

    pub fn status_help(&self) -> &'static str {
        match self.mode {
            Mode::List if self.finding => "Type to find  \u{2191}\u{2193} Select  Enter Dial  Esc Clear",
            Mode::List => "Enter Dial  Ins Add  E Edit  Del Remove  P Pin  / Find  Esc Close",
            Mode::Edit(_) => "Enter Save  Esc Cancel  Tab/\u{2191}\u{2193} Field  Space/\u{2190}\u{2192} Change option",
            Mode::Prompt(..) => "Enter OK  Esc Cancel",
            Mode::ConfirmDelete => "Y Remove  N Keep",
            Mode::ConfirmHangUp(_) => "Y Hang up and dial  N Stay connected",
            Mode::Message(_) => "Press any key",
        }
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods, dir: &mut Directory) -> Action {
        let view = dir.view_group(&self.filter.value, self.group.as_deref());
        let n = view.len();
        self.selected = self.selected.min(n.saturating_sub(1));
        let current = view.get(self.selected).copied();
        // The selected entry's index, valid when `any`.
        let (any, i) = (current.is_some(), current.unwrap_or_default());
        if self.finding && matches!(self.mode, Mode::List) {
            return self.find_key(key, mods, dir, current);
        }
        match &mut self.mode {
            Mode::List => match key {
                Key::Up => self.selected = self.selected.saturating_sub(1),
                Key::Down if n > 0 => self.selected = (self.selected + 1).min(n - 1),
                Key::PageUp => self.selected = self.selected.saturating_sub(LIST_ROWS),
                Key::PageDown if n > 0 => self.selected = (self.selected + LIST_ROWS).min(n - 1),
                Key::Home => self.selected = 0,
                Key::End => self.selected = n.saturating_sub(1),
                Key::Enter if any => return Action::Connect(i),
                Key::Insert | Key::Char('a') | Key::Char('A') => {
                    let (form, _) = Form::new(None, self.new_entry.clone(), &*self.secrets);
                    self.mode = Mode::Edit(Box::new(form));
                }
                Key::Char('e') | Key::Char('E') | Key::F(2) if any => {
                    let e = dir.entries[i].clone();
                    let (form, _) = Form::new(Some(i), e, &*self.secrets);
                    self.mode = Mode::Edit(Box::new(form));
                }
                Key::Char('p') | Key::Char('P') if any => {
                    let e = &mut dir.entries[i];
                    e.pinned = !e.pinned;
                    let id = (e.system_id(), e.name.clone());
                    dir.sort();
                    self.select(dir, &id);
                    return Action::Changed;
                }
                Key::Char('s') | Key::Char('S') => {
                    let id = current.map(|i| (dir.entries[i].system_id(), dir.entries[i].name.clone()));
                    dir.sort = dir.sort.next();
                    if let Some(id) = id {
                        self.select(dir, &id);
                    }
                    return Action::Changed;
                }
                Key::Char('o') | Key::Char('O') => return Action::Settings,
                Key::Char('g') | Key::Char('G') => {
                    // All, then each group in turn.
                    let groups = dir.groups();
                    let at = self.group.as_ref().and_then(|g| groups.iter().position(|h| h == g));
                    self.group = match at {
                        None => groups.first().cloned(),
                        Some(i) => groups.get(i + 1).cloned(),
                    };
                    self.selected = 0;
                    if groups.is_empty() {
                        self.show_message("No entries are in a group yet: give one a group in its settings (E).");
                    }
                }
                Key::Char('/') | Key::F(3) => self.finding = true,
                Key::Char('f') if mods.ctrl => self.finding = true,
                Key::Delete | Key::Char('d') | Key::Char('D') if current.is_some() => self.mode = Mode::ConfirmDelete,
                Key::Char('q') | Key::Char('Q') => {
                    self.mode = Mode::Prompt(PromptKind::QuickConnect, TextField::new("", 200));
                }
                Key::Char('i') | Key::Char('I') => {
                    let p = self.files_dir.join("syncterm.lst");
                    self.mode = Mode::Prompt(PromptKind::Import, TextField::new(&p.to_string_lossy(), 400));
                }
                Key::Char('x') | Key::Char('X') => {
                    let p = self.files_dir.join("badterm-export.lst");
                    self.mode = Mode::Prompt(PromptKind::Export, TextField::new(&p.to_string_lossy(), 400));
                }
                // Esc first clears a filter, then closes.
                Key::Escape if !self.filter.value.is_empty() => self.clear_filter(dir, current),
                Key::Escape => return Action::Close,
                Key::Char(c) if c.is_alphanumeric() => {
                    // Jump to the next entry starting with the typed character.
                    let c = c.to_ascii_lowercase();
                    let starts = |pos: usize| dir.entries[view[pos]].name.to_lowercase().starts_with(c);
                    let sel = self.selected;
                    if let Some(pos) = (sel + 1..n).chain(0..sel + 1).find(|&pos| starts(pos)) {
                        self.selected = pos;
                    }
                }
                _ => {}
            },
            Mode::Edit(form) => match key {
                Key::Escape => self.mode = Mode::List,
                Key::Enter => match form.to_entry() {
                    Ok(e) => {
                        let index = form.index;
                        let id = (e.system_id(), e.name.clone());
                        // A password that moved (new user or address) or was
                        // cleared leaves the keychain.
                        if let Some(old) = index.map(|i| &dir.entries[i])
                            && !e.keychain
                            && (e.password.is_empty() || secrets::account(old) != secrets::account(&e))
                        {
                            secrets::forget(old, &*self.secrets);
                        }
                        match index {
                            Some(i) => dir.entries[i] = e,
                            None => dir.entries.push(e),
                        }
                        dir.sort();
                        self.select(dir, &id);
                        self.mode = Mode::List;
                        return Action::Changed;
                    }
                    Err(msg) => form.error = Some(msg),
                },
                Key::Tab if mods.shift => form.focus = (form.focus + FIELDS.len() - 1) % FIELDS.len(),
                Key::Tab | Key::Down => form.focus = (form.focus + 1) % FIELDS.len(),
                Key::Up => form.focus = (form.focus + FIELDS.len() - 1) % FIELDS.len(),
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_PROTOCOL => {
                    let all = Protocol::ALL;
                    let i = all.iter().position(|&p| p == form.protocol).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    // Keep the port in step if it was the old protocol's default.
                    if form.text[F_PORT].value == form.protocol.default_port().to_string() {
                        form.text[F_PORT] = TextField::new(&all[next].default_port().to_string(), 5);
                    }
                    form.protocol = all[next];
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_ROWS => {
                    let all = dialer::SCREEN_MODES;
                    let i = all.iter().position(|&m| m == form.mode).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    form.mode = all[next];
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_FONT => {
                    let all = fonts();
                    let i = all.iter().position(|&f| f == form.font).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    form.font = all[next];
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_ICE => {
                    let all = [None, Some(true), Some(false)];
                    let i = all.iter().position(|&v| v == form.ice).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    form.ice = all[next];
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_BAUD => {
                    let all = dialer::BAUD_RATES;
                    let i = all.iter().position(|&b| b == form.baud).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    form.baud = all[next];
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_AUTO_LOGIN => {
                    form.auto_login = !form.auto_login;
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_CAPTURE => {
                    form.capture = !form.capture;
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_STATUS => {
                    form.hide_status = !form.hide_status;
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_BELL => {
                    let all: Vec<Option<BellMode>> =
                        std::iter::once(None).chain(BellMode::ALL.into_iter().map(Some)).collect();
                    let i = all.iter().position(|&m| m == form.bell).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    form.bell = all[next];
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_CRT => {
                    // The global setting, then each mode.
                    let all: Vec<Option<CrtMode>> =
                        std::iter::once(None).chain(CrtMode::ALL.into_iter().map(Some)).collect();
                    let i = all.iter().position(|&m| m == form.crt).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    form.crt = all[next];
                }
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_ENCODING => {
                    let all = Encoding::ALL;
                    let i = all.iter().position(|&e| e == form.encoding).unwrap_or(0);
                    let next = if key == Key::Left { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() };
                    form.encoding = all[next];
                }
                Key::Char(c) if form.focus == F_PORT && !c.is_ascii_digit() => {}
                _ => {
                    if !OPTION_FIELDS.contains(&form.focus) {
                        form.text[form.focus].handle_key(key, mods);
                        form.error = None;
                    }
                }
            },
            Mode::Prompt(kind, field) => match key {
                Key::Escape => self.mode = Mode::List,
                Key::Enter => {
                    let (kind, value) = (*kind, field.value.trim().to_string());
                    self.mode = Mode::List;
                    if value.is_empty() {
                        return Action::None;
                    }
                    return self.run_prompt(kind, &value, dir);
                }
                _ => {
                    field.handle_key(key, mods);
                }
            },
            Mode::ConfirmDelete => {
                if let Key::Char('y' | 'Y') = key
                    && let Some(i) = current
                {
                    let gone = dir.entries.remove(i);
                    secrets::forget(&gone, &*self.secrets);
                    self.mode = Mode::List;
                    return Action::Changed;
                }
                self.mode = Mode::List;
            }
            Mode::ConfirmHangUp(_) => {
                self.mode = Mode::List;
                if let Key::Char('y' | 'Y') = key {
                    return Action::HangUpAndDial;
                }
            }
            Mode::Message(_) => self.mode = Mode::List,
        }
        if let Mode::Edit(form) = &mut self.mode {
            form.scroll();
        }
        Action::None
    }

    /// Keys while typing a filter.
    fn find_key(&mut self, key: Key, mods: Mods, dir: &Directory, current: Option<usize>) -> Action {
        let n = dir.view_group(&self.filter.value, self.group.as_deref()).len();
        match key {
            Key::Escape => {
                self.finding = false;
                self.clear_filter(dir, current);
            }
            Key::Enter => {
                self.finding = false;
                if let Some(i) = current {
                    return Action::Connect(i);
                }
            }
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Down if n > 0 => self.selected = (self.selected + 1).min(n - 1),
            Key::PageUp => self.selected = self.selected.saturating_sub(LIST_ROWS),
            Key::PageDown if n > 0 => self.selected = (self.selected + LIST_ROWS).min(n - 1),
            _ => {
                let before = self.filter.value.clone();
                self.filter.handle_key(key, mods);
                if self.filter.value != before {
                    self.selected = 0;
                }
            }
        }
        Action::None
    }

    /// Drop the filter, keeping the selected entry selected.
    fn clear_filter(&mut self, dir: &Directory, current: Option<usize>) {
        self.filter = TextField::new("", 60);
        if let Some(i) = current {
            let e = &dir.entries[i];
            self.select(dir, &(e.system_id(), e.name.clone()));
        }
    }

    /// Select the entry with this (system id, name), e.g. after re-sorting.
    fn select(&mut self, dir: &Directory, id: &(String, String)) {
        let view = dir.view_group(&self.filter.value, self.group.as_deref());
        let pos = view.iter().position(|&i| dir.entries[i].system_id() == id.0 && dir.entries[i].name == id.1);
        self.selected = pos.unwrap_or(0);
    }

    fn run_prompt(&mut self, kind: PromptKind, value: &str, dir: &mut Directory) -> Action {
        match kind {
            PromptKind::QuickConnect => match Entry::from_address(value) {
                Some(e) => Action::QuickConnect(Box::new(e)),
                None => {
                    self.show_message(format!("Not a valid address: {value}"));
                    Action::None
                }
            },
            PromptKind::Import => match dialer::import(&expand_home(value)) {
                Ok(entries) => {
                    let total = entries.len();
                    let added = dir.merge(entries);
                    self.show_message(format!("Imported {added} new entries ({} already present).", total - added));
                    Action::Changed
                }
                Err(e) => {
                    self.show_message(format!("Import failed: {e:#}"));
                    Action::None
                }
            },
            PromptKind::Export => {
                match dialer::export(&expand_home(value), &dir.entries) {
                    Ok(()) => self.show_message(format!("Exported {} entries to {value}", dir.entries.len())),
                    Err(e) => self.show_message(format!("Export failed: {e:#}")),
                }
                Action::None
            }
        }
    }

    /// Render into `s` (80x25). Returns the text cursor position, if any.
    pub fn draw(&mut self, s: &mut Screen, dir: &Directory) -> Option<(usize, usize)> {
        // A group no entry is in any more.
        if self.group.as_ref().is_some_and(|g| !dir.groups().contains(g)) {
            self.group = None;
        }
        let t = theme::current();
        s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', t.help.fg(), t.help.bg()));
        let title = format!(" BadTerm {} \u{2502} Dialing Directory", env!("CARGO_PKG_VERSION"));
        text(s, 0, 0, COLS, &title, t.title_bar.fg(), t.title_bar.bg());
        let view = dir.view_group(&self.filter.value, self.group.as_deref());
        let entries = |n: usize| if n == 1 { "entry" } else { "entries" };
        let count = if let Some(g) = &self.group {
            format!("{}: {} of {} {} ", truncate(g, 20), view.len(), dir.entries.len(), entries(dir.entries.len()))
        } else if view.len() == dir.entries.len() {
            format!("{} {} ", dir.entries.len(), entries(dir.entries.len()))
        } else {
            format!("{} of {} {} ", view.len(), dir.entries.len(), entries(dir.entries.len()))
        };
        label(s, COLS - count.len(), 0, &count, t.title_bar.fg(), t.title_bar.bg());

        frame(s, 0, 1, COLS, 21, "", t.border.fg(), t.border.bg());
        s.fill_rect(1, LIST_TOP, COLS - 2, LIST_ROWS, Cell::new(b' ', t.entries.fg(), t.entries.bg()));
        let header = format!(" {:<30} {:<30} {:<7} {:>5}", "Name", "Address", "Type", "Calls");
        text(s, 1, 2, COLS - 2, &header, t.headings.fg(), t.headings.bg());
        divider(s, 0, 3, COLS, t.border.fg(), t.border.bg());
        let (list_fg, list_bg) = (t.entries.fg(), t.entries.bg());

        let n = view.len();
        self.selected = self.selected.min(n.saturating_sub(1));
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + LIST_ROWS {
            self.top = self.selected + 1 - LIST_ROWS;
        }
        if n == 0 && !dir.entries.is_empty() {
            text(s, 2, LIST_TOP + 1, COLS - 4, "Nothing matches. Esc clears the search.", list_fg, list_bg);
        } else if n == 0 {
            text(
                s,
                2,
                LIST_TOP + 1,
                COLS - 4,
                "No entries yet. Press Ins to add a BBS, Q to quick-connect,",
                list_fg,
                list_bg,
            );
            text(
                s,
                2,
                LIST_TOP + 2,
                COLS - 4,
                "or I to import a directory (SyncTERM, NetRunner, EtherTerm, CSV...).",
                list_fg,
                list_bg,
            );
        }
        for (row, (pos, e)) in
            view.iter().map(|&i| &dir.entries[i]).enumerate().skip(self.top).take(LIST_ROWS).enumerate()
        {
            // Pinned entries are marked with a diamond.
            let line = format!(
                "{}{:<30} {:<30} {:<7} {:>5}",
                if e.pinned { '\u{2666}' } else { ' ' },
                truncate(&e.name, 30),
                truncate(&e.address(), 30),
                e.protocol.name(),
                e.calls
            );
            let a = if pos == self.selected { t.selected } else { t.entries };
            let (fg, bg) = (a.fg(), a.bg());
            text(s, 1, LIST_TOP + row, COLS - 2, &line, fg, bg);
        }
        // Scroll indicators.
        if self.top > 0 {
            label(s, COLS - 1, LIST_TOP, "\u{25B2}", t.headings.fg(), t.border.bg());
        }
        if self.top + LIST_ROWS < n {
            label(s, COLS - 1, LIST_TOP + LIST_ROWS - 1, "\u{25BC}", t.headings.fg(), t.border.bg());
        }

        if let Some(e) = view.get(self.selected).map(|&i| &dir.entries[i]) {
            let last = e.last_connected.map(format_date).unwrap_or_else(|| "never".into());
            let mut info = format!(" Last called: {last}   Emulation: {}", e.encoding.name());
            if !e.group.is_empty() && self.group.is_none() {
                info.push_str(&format!("   Group: {}", e.group));
            }
            if !e.comment.is_empty() {
                info.push_str(&format!("   {}", e.comment));
            }
            text(s, 0, 22, COLS, &info, t.info.fg(), t.info.bg());
        }

        // The bottom line: the search, or the keys the status bar has no
        // room for.
        let mut cursor = None;
        if self.finding || !self.filter.value.is_empty() {
            label(s, 1, 24, "Find:", t.help_key.fg(), t.help_key.bg());
            let a = if self.finding { t.input } else { t.info };
            let (fg, bg) = (a.fg(), a.bg());
            let pos = self.filter.draw(s, 7, 24, 40, false, fg, bg);
            if self.finding {
                cursor = Some(pos);
            }
        } else {
            let keys =
                format!("S Sort ({})  G Group  Q Quick connect  I Import  X Export  O Settings", dir.sort.name());
            text(s, 1, 24, COLS - 2, &keys, t.help.fg(), t.help.bg());
        }

        match &self.mode {
            Mode::List => cursor,
            Mode::Edit(form) => Some(draw_form(s, form)),
            Mode::Prompt(kind, field) => {
                let (title, hint) = match kind {
                    PromptKind::QuickConnect => {
                        ("Quick Connect", "host, host:port, telnet://host:port or raw://host:port")
                    }
                    PromptKind::Import => {
                        ("Import Directory", "SyncTERM .lst, NetRunner .phn, Qodem fonebook.txt, .csv, .xml...")
                    }
                    PromptKind::Export => ("Export Directory", "Format follows extension: .lst .csv .json .toml"),
                };
                let (x, y, w) = (6, 9, 68);
                frame(s, x, y, w, 6, title, t.window.fg(), t.window.bg());
                label(s, x + 2, y + 1, hint, t.window_text.fg(), t.window_text.bg());
                Some(field.draw(s, x + 2, y + 3, w - 4, false, t.input.fg(), t.input.bg()))
            }
            Mode::ConfirmDelete => {
                let name = view.get(self.selected).map(|&i| dir.entries[i].name.as_str()).unwrap_or("");
                let msg = format!("Remove \"{}\" from the directory?", truncate(name, 40));
                confirm_box(s, "Remove entry?", &msg, &[("Y", "Remove"), ("N", "Keep")]);
                None
            }
            Mode::ConfirmHangUp(msg) => {
                confirm_box(s, "Still connected", msg, &[("Y", "Hang up and dial"), ("N", "Stay connected")]);
                None
            }
            Mode::Message(msg) => {
                message_box(s, "BadTerm", msg);
                None
            }
        }
    }
}

fn draw_form(s: &mut Screen, form: &Form) -> (usize, usize) {
    // The title, a blank row, the fields, the error line and the border.
    let (w, h) = (64, FORM_ROWS + 4);
    let (x, y) = ((COLS - w) / 2, (ROWS - h) / 2);
    let title = if form.index.is_some() { "Edit Entry" } else { "New Entry" };
    let t = theme::current();
    frame(s, x, y, w, h, title, t.window.fg(), t.window.bg());
    let mut cursor = (0, 0);
    let fx = x + 18;
    let fw = w - 20;
    for (i, name) in FIELDS.iter().enumerate().skip(form.top).take(FORM_ROWS) {
        let yy = y + 2 + i - form.top;
        let focused = i == form.focus;
        let name_attr = if focused { t.keys } else { t.field_name };
        text(s, x + 2, yy, 16, &format!("{name:>15}"), name_attr.fg(), name_attr.bg());
        let a = if focused { t.input } else { t.field_value };
        let (fg, bg) = (a.fg(), a.bg());
        match i {
            F_PROTOCOL => {
                let v = format!("\u{25C4} {:<7} \u{25BA}", form.protocol.name());
                text(s, fx, yy, fw, &v, fg, bg);
            }
            F_ENCODING => {
                text(s, fx, yy, fw, &format!("\u{25C4} {:<7} \u{25BA}", form.encoding.name()), fg, bg);
                let note = match form.encoding {
                    Encoding::Petscii => "(Commodore 64: 40x25, its font)",
                    Encoding::Atascii => "(Atari 8-bit: 40x24, its font)",
                    Encoding::Viewdata => "(Prestel: 40x24, teletext graphics)",
                    Encoding::Mode7 => "(BBC Micro teletext: 40x25)",
                    Encoding::Igs => "(Atari ST: VT-52 with IGS graphics)",
                    Encoding::Skypix => "(Amiga: ANSI with SkyPix graphics)",
                    Encoding::Avatar => "(AVT/0+ codes as well as ANSI)",
                    Encoding::Rip => "(graphics; 80x43 text over them)",
                    _ => "",
                };
                label(s, fx + 12, yy, note, t.note.fg(), bg);
            }
            F_ROWS => {
                let (c, r) = form.mode;
                text(s, fx, yy, fw, &format!("\u{25C4} {:<7} \u{25BA}", format!("{c}x{r}")), fg, bg);
            }
            F_FONT => {
                text(s, fx, yy, fw, &format!("\u{25C4} {:<16} \u{25BA}", font_name(form.font)), fg, bg);
                if let Some(note) = font_note(form.font) {
                    label(s, fx + 21, yy, &format!("({note})"), t.note.fg(), bg);
                }
            }
            F_ICE => {
                let (v, hint) = match form.ice {
                    None => ("Auto", "(the BBS decides)"),
                    Some(true) => ("On", "(bright backgrounds)"),
                    Some(false) => ("Off", "(blinking text)"),
                };
                text(s, fx, yy, fw, &format!("\u{25C4} {v:<4} \u{25BA}"), fg, bg);
                label(s, fx + 9, yy, hint, t.note.fg(), bg);
            }
            F_BAUD => {
                let v = if form.baud == 0 { "Unlimited".to_string() } else { format!("{} bps", form.baud) };
                text(s, fx, yy, fw, &format!("\u{25C4} {v:<10} \u{25BA}"), fg, bg);
                if form.baud != 0 {
                    label(s, fx + 15, yy, "(as over a modem)", t.note.fg(), bg);
                }
            }
            F_AUTO_LOGIN => {
                let v = if form.auto_login { "On " } else { "Off" };
                text(s, fx, yy, fw, &format!("\u{25C4} {v} \u{25BA}"), fg, bg);
                let hint = match form.protocol {
                    Protocol::Ssh | Protocol::Rlogin => "(the protocol logs in)".to_string(),
                    _ => format!("(answers Login/Password; or {})", crate::hotkeys::label("L")),
                };
                label(s, fx + 8, yy, &hint, t.note.fg(), bg);
            }
            F_BELL => {
                let name = form.bell.map_or("Default", BellMode::name);
                text(s, fx, yy, fw, &format!("\u{25C4} {name:<14} \u{25BA}"), fg, bg);
                if form.bell.is_none() {
                    label(s, fx + 19, yy, "(Settings)", t.note.fg(), bg);
                }
            }
            F_STATUS => {
                let v = if form.hide_status { "Off" } else { "On " };
                text(s, fx, yy, fw, &format!("\u{25C4} {v} \u{25BA}"), fg, bg);
                if form.hide_status {
                    label(s, fx + 8, yy, "(the BBS's screen alone)", t.note.fg(), bg);
                }
            }
            F_CAPTURE => {
                let v = if form.capture { "On " } else { "Off" };
                text(s, fx, yy, fw, &format!("\u{25C4} {v} \u{25BA}"), fg, bg);
                label(
                    s,
                    fx + 8,
                    yy,
                    &format!("(records each call; or {})", crate::hotkeys::label("C")),
                    t.note.fg(),
                    bg,
                );
            }
            F_PORT if form.protocol == Protocol::Modem => {
                text(s, fx, yy, fw, "", fg, bg);
                label(s, fx, yy, "(a modem has none)", t.note.fg(), bg);
            }
            F_CRT => {
                let name = form.crt.map_or("Default", CrtMode::name);
                text(s, fx, yy, fw, &format!("\u{25C4} {name:<12} \u{25BA}"), fg, bg);
                if form.crt.is_none() {
                    label(s, fx + 17, yy, &format!("({})", crate::hotkeys::label("P")), t.note.fg(), bg);
                }
            }
            _ => {
                let pos = form.text[i].draw(s, fx, yy, fw, i == F_PASSWORD, fg, bg);
                if focused {
                    cursor = pos;
                }
                // What a field holds, where it isn't obvious.
                let hint = match i {
                    F_ADDRESS if form.protocol == Protocol::Modem => "phone number",
                    F_CAPTURE_FILE => "{name} {date} {time}; .txt: text only",
                    F_DOWNLOADS | F_UPLOADS => "the folder in the settings",
                    F_GROUP => "none; G in the list shows one group",
                    _ => "",
                };
                if !hint.is_empty() && form.text[i].value.is_empty() && !focused {
                    label(s, fx, yy, hint, t.note.fg(), bg);
                }
            }
        }
    }
    // There are more fields above or below.
    if form.top > 0 {
        label(s, x + w - 1, y + 2, "\u{25B2}", t.window.fg(), t.window.bg());
    }
    if form.top + FORM_ROWS < FIELDS.len() {
        label(s, x + w - 1, y + 1 + FORM_ROWS, "\u{25BC}", t.window.fg(), t.window.bg());
    }
    if let Some(err) = &form.error {
        text(s, x + 2, y + h - 2, w - 4, err, t.error.fg(), t.error.bg());
    }
    cursor
}

/// The fonts an entry can have, in the order the form steps through.
pub fn fonts() -> Vec<u8> {
    std::iter::once(0)
        .chain(EXTRA_FONTS.iter().map(|&(id, _)| id))
        .chain(CODEPAGE_FONTS.iter().map(|&(id, ..)| id))
        .collect()
}

pub fn font_name(id: u8) -> &'static str {
    EXTRA_FONTS
        .iter()
        .find(|&&(f, _)| f == id)
        .map(|&(_, name)| name)
        .or_else(|| CODEPAGE_FONTS.iter().find(|&&(f, ..)| f == id).map(|&(_, name, _)| name))
        .unwrap_or("CP437 (IBM PC)")
}

/// What a font is for, beside its name in the entry form.
fn font_note(id: u8) -> Option<&'static str> {
    if EXTRA_FONTS.iter().any(|&(f, _)| f == id) {
        return Some("Amiga");
    }
    CODEPAGE_FONTS.iter().find(|&&(f, ..)| f == id).map(|&(.., lang)| lang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::tests::Memory;

    fn dir() -> Directory {
        let mut d = Directory::default();
        d.merge(vec![
            Entry::from_address("alpha.example.org").unwrap(),
            Entry::from_address("beta.example.org").unwrap(),
        ]);
        d
    }

    fn key(ui: &mut DialerUi, d: &mut Directory, k: Key) -> Action {
        ui.handle_key(k, Mods::default(), d)
    }

    #[test]
    fn escape_closes_instead_of_quitting() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), Rc::new(Memory::default())), dir());
        assert!(matches!(key(&mut ui, &mut d, Key::Escape), Action::Close));
    }

    #[test]
    fn hang_up_confirmation() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), Rc::new(Memory::default())), dir());
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Connect(0)));

        ui.confirm_hang_up("Alpha", "Beta");
        assert_eq!(ui.status_help(), "Y Hang up and dial  N Stay connected");
        assert!(matches!(key(&mut ui, &mut d, Key::Char('y')), Action::HangUpAndDial));
        // Back to the list afterwards.
        assert!(matches!(key(&mut ui, &mut d, Key::Escape), Action::Close));

        // Anything but Y keeps the call and returns to the list.
        ui.confirm_hang_up("Alpha", "Beta");
        assert!(matches!(key(&mut ui, &mut d, Key::Char('n')), Action::None));
        assert!(ui.status_help().starts_with("Enter Dial"));
    }

    #[test]
    fn pinning_moves_an_entry_to_the_top() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), Rc::new(Memory::default())), dir());
        key(&mut ui, &mut d, Key::Down);
        assert!(matches!(key(&mut ui, &mut d, Key::Char('p')), Action::Changed));
        assert!(d.entries[0].pinned && d.entries[0].host == "beta.example.org");
        // The selection follows the entry.
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Connect(0)));
    }

    #[test]
    fn phosphor_mode_per_entry() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), Rc::new(Memory::default())), dir());
        key(&mut ui, &mut d, Key::Char('e'));
        let Mode::Edit(form) = &mut ui.mode else { panic!() };
        assert_eq!(form.crt, None);
        form.focus = F_CRT;
        // Default, off, color, green.
        for _ in 0..3 {
            key(&mut ui, &mut d, Key::Right);
        }
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Changed));
        assert_eq!(d.entries[0].crt, Some(CrtMode::Green));

        // Left goes back: from green to color.
        key(&mut ui, &mut d, Key::Char('e'));
        let Mode::Edit(form) = &mut ui.mode else { panic!() };
        form.focus = F_CRT;
        key(&mut ui, &mut d, Key::Left);
        let Mode::Edit(form) = &ui.mode else { panic!() };
        assert_eq!(form.crt, Some(CrtMode::Color));
        let mut s = Screen::new(COLS, ROWS, 0);
        ui.draw(&mut s, &d);
        let rows: Vec<String> = (0..ROWS).map(|y| s.row(y).iter().map(|c| c.ch as char).collect()).collect();
        assert!(rows.iter().any(|r| r.contains("Phosphor mode") && r.contains("Color CRT")), "{rows:#?}");
    }

    #[test]
    fn ice_colors_per_entry() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), Rc::new(Memory::default())), dir());
        key(&mut ui, &mut d, Key::Char('e'));
        let Mode::Edit(form) = &mut ui.mode else { panic!() };
        assert_eq!(form.ice, None);
        form.focus = F_ICE;
        key(&mut ui, &mut d, Key::Right);
        let mut s = Screen::new(COLS, ROWS, 0);
        ui.draw(&mut s, &d);
        let rows: Vec<String> = (0..ROWS).map(|y| Screen::row_text(s.row(y))).collect();
        assert!(rows.iter().any(|r| r.contains("iCE colors") && r.contains("On")), "{rows:#?}");
        key(&mut ui, &mut d, Key::Right);
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Changed));
        assert_eq!(d.entries[0].ice, Some(false));
    }

    #[test]
    fn edited_passwords_and_the_keychain() {
        let store = Rc::new(Memory::default());
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), store.clone()), dir());
        let account = secrets::account(&d.entries[0]);
        store.map.borrow_mut().insert(account.clone(), "pw".into());
        d.entries[0].keychain = true;

        // The editor shows the stored password; saving unchanged keeps it.
        key(&mut ui, &mut d, Key::Char('e'));
        let Mode::Edit(form) = &ui.mode else { panic!() };
        assert_eq!(form.text[F_PASSWORD].value, "pw");
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Changed));
        assert_eq!(d.entries[0].password, "pw");
        assert!(store.map.borrow().contains_key(&account));
        crate::secrets::migrate(&mut d, &*store).unwrap();
        assert!(d.entries[0].keychain && d.entries[0].password.is_empty());

        // Clearing the password removes it from the keychain.
        key(&mut ui, &mut d, Key::Char('e'));
        let Mode::Edit(form) = &mut ui.mode else { panic!() };
        form.focus = F_PASSWORD;
        key(&mut ui, &mut d, Key::Char('u'));
        let ctrl = Mods { ctrl: true, ..Mods::default() };
        ui.handle_key(Key::Char('u'), ctrl, &mut d);
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Changed));
        assert!(!d.entries[0].keychain && d.entries[0].password.is_empty());
        assert!(store.map.borrow().is_empty());

        // Deleting an entry forgets its password.
        store.map.borrow_mut().insert(account, "pw".into());
        d.entries[0].keychain = true;
        key(&mut ui, &mut d, Key::Delete);
        key(&mut ui, &mut d, Key::Char('y'));
        assert!(store.map.borrow().is_empty());
    }

    fn typed(ui: &mut DialerUi, d: &mut Directory, s: &str) {
        for c in s.chars() {
            key(ui, d, Key::Char(c));
        }
    }

    #[test]
    fn find_filters_the_list() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), Rc::new(Memory::default())), dir());
        key(&mut ui, &mut d, Key::Char('/'));
        assert!(ui.status_help().starts_with("Type to find"));
        typed(&mut ui, &mut d, "BETA");
        // Enter dials the (only) match: beta is entry 1.
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Connect(1)));
        // The filter stays, and list keys work on the filtered list.
        assert!(matches!(key(&mut ui, &mut d, Key::Char('e')), Action::None));
        let Mode::Edit(form) = &ui.mode else { panic!() };
        assert_eq!(form.index, Some(1));
        key(&mut ui, &mut d, Key::Escape);
        // Esc clears the filter, keeping beta selected; a second Esc closes.
        assert!(matches!(key(&mut ui, &mut d, Key::Escape), Action::None));
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Connect(1)));
        assert!(matches!(key(&mut ui, &mut d, Key::Escape), Action::Close));
    }

    #[test]
    fn sort_order_cycles_and_keeps_the_selection() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new(), Rc::new(Memory::default())), dir());
        d.entries[1].calls = 9;
        assert!(matches!(key(&mut ui, &mut d, Key::Char('s')), Action::Changed));
        assert_eq!(d.sort, dialer::SortOrder::LastCalled);
        assert!(matches!(key(&mut ui, &mut d, Key::Char('s')), Action::Changed));
        assert_eq!(d.sort, dialer::SortOrder::Calls);
        // Alpha stays selected though beta (more calls) now lists first.
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Connect(0)));
        key(&mut ui, &mut d, Key::Home);
        assert!(matches!(key(&mut ui, &mut d, Key::Enter), Action::Connect(1)));
    }
}
