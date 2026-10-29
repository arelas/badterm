//! The dialing directory screen: list, add/edit form, quick connect, and
//! import/export.

use std::path::PathBuf;

use phosphor_core::Encoding;
use phosphor_core::dialer::{self, Directory, Entry, Protocol};
use phosphor_core::screen::{Cell, Screen};

use super::*;
use crate::input::{Key, Mods};

pub const COLS: usize = 80;
pub const ROWS: usize = 25;

const LIST_TOP: usize = 4;
const LIST_ROWS: usize = 17;

pub enum Action {
    None,
    /// Connect to the directory entry at this index.
    Connect(usize),
    /// Connect to an address that is not (yet) in the directory.
    QuickConnect(Entry),
    /// The directory changed and should be saved.
    Changed,
    /// The user confirmed hanging up the current call to dial the one chosen.
    HangUpAndDial,
    /// Close the dialing directory.
    Close,
}

const FIELDS: [&str; 9] =
    ["Name", "Address", "Port", "Protocol", "Encoding", "Terminal type", "User name", "Password", "Comment"];
const F_PORT: usize = 2;
const F_PROTOCOL: usize = 3;
const F_ENCODING: usize = 4;
const F_PASSWORD: usize = 7;

struct Form {
    /// Index of the entry being edited, or None for a new entry.
    index: Option<usize>,
    base: Entry,
    text: [TextField; 9],
    protocol: Protocol,
    encoding: Encoding,
    focus: usize,
    error: Option<String>,
}

impl Form {
    fn new(index: Option<usize>, e: Entry) -> Form {
        let t = |s: &str| TextField::new(s, 120);
        Form {
            index,
            text: [
                t(&e.name),
                t(&e.host),
                TextField::new(&e.port.to_string(), 5),
                TextField::default(),
                TextField::default(),
                t(&e.term_type),
                t(&e.username),
                t(&e.password),
                t(&e.comment),
            ],
            protocol: e.protocol,
            encoding: e.encoding,
            focus: 0,
            error: None,
            base: e,
        }
    }

    fn to_entry(&self) -> Result<Entry, String> {
        let host = self.text[1].value.trim();
        if host.is_empty() {
            return Err("Address is required".into());
        }
        // Allow "host:port" or a URL in the address field.
        let parsed = Entry::from_address(host).ok_or("Invalid address")?;
        let mut port: u16 = self.text[F_PORT].value.trim().parse().map_err(|_| "Port must be 1-65535")?;
        if port == 0 {
            return Err("Port must be 1-65535".into());
        }
        if parsed.port != parsed.protocol.default_port() {
            port = parsed.port;
        }
        if self.protocol == Protocol::Ssh && self.text[6].value.trim().is_empty() {
            return Err("SSH needs a user name (BBSes often accept \"new\" or \"guest\")".into());
        }
        let name = self.text[0].value.trim();
        Ok(Entry {
            name: if name.is_empty() { parsed.host.clone() } else { name.to_string() },
            host: parsed.host,
            port,
            protocol: self.protocol,
            encoding: self.encoding,
            term_type: self.text[5].value.trim().to_string(),
            username: self.text[6].value.clone(),
            password: self.text[7].value.clone(),
            comment: self.text[8].value.clone(),
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
    selected: usize,
    top: usize,
    mode: Mode,
    /// Default directory offered for import/export paths.
    pub files_dir: PathBuf,
}

impl DialerUi {
    pub fn new(files_dir: PathBuf) -> DialerUi {
        DialerUi { selected: 0, top: 0, mode: Mode::List, files_dir }
    }

    pub fn show_message(&mut self, msg: impl Into<String>) {
        self.mode = Mode::Message(msg.into());
    }

    /// Ask whether to hang up `current` to dial `next`; answers with
    /// [`Action::HangUpAndDial`] on yes.
    pub fn confirm_hang_up(&mut self, current: &str, next: &str) {
        let msg = format!("Hang up {} and dial {}? (Y/N)", truncate(current, 24), truncate(next, 24));
        self.mode = Mode::ConfirmHangUp(msg);
    }

    pub fn status_help(&self) -> &'static str {
        match self.mode {
            Mode::List => "Enter Dial  Ins Add  E Edit  Del Remove  Q Quick  I Import  X Export  Esc Close",
            Mode::Edit(_) => "Enter Save  Esc Cancel  Tab/\u{2191}\u{2193} Field  Space/\u{2190}\u{2192} Change option",
            Mode::Prompt(..) => "Enter OK  Esc Cancel",
            Mode::ConfirmDelete => "Y Delete  N Keep",
            Mode::ConfirmHangUp(_) => "Y Hang up and dial  N Stay connected",
            Mode::Message(_) => "Press any key",
        }
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods, dir: &mut Directory) -> Action {
        let n = dir.entries.len();
        self.selected = self.selected.min(n.saturating_sub(1));
        match &mut self.mode {
            Mode::List => match key {
                Key::Up => self.selected = self.selected.saturating_sub(1),
                Key::Down if n > 0 => self.selected = (self.selected + 1).min(n - 1),
                Key::PageUp => self.selected = self.selected.saturating_sub(LIST_ROWS),
                Key::PageDown if n > 0 => self.selected = (self.selected + LIST_ROWS).min(n - 1),
                Key::Home => self.selected = 0,
                Key::End => self.selected = n.saturating_sub(1),
                Key::Enter if n > 0 => return Action::Connect(self.selected),
                Key::Insert | Key::Char('a') | Key::Char('A') => {
                    self.mode = Mode::Edit(Box::new(Form::new(None, Entry::default())));
                }
                Key::Char('e') | Key::Char('E') | Key::F(2) if n > 0 => {
                    let e = dir.entries[self.selected].clone();
                    self.mode = Mode::Edit(Box::new(Form::new(Some(self.selected), e)));
                }
                Key::Delete | Key::Char('d') | Key::Char('D') if n > 0 => self.mode = Mode::ConfirmDelete,
                Key::Char('q') | Key::Char('Q') => {
                    self.mode = Mode::Prompt(PromptKind::QuickConnect, TextField::new("", 200));
                }
                Key::Char('i') | Key::Char('I') => {
                    let p = self.files_dir.join("syncterm.lst");
                    self.mode = Mode::Prompt(PromptKind::Import, TextField::new(&p.to_string_lossy(), 400));
                }
                Key::Char('x') | Key::Char('X') => {
                    let p = self.files_dir.join("phosphor-export.lst");
                    self.mode = Mode::Prompt(PromptKind::Export, TextField::new(&p.to_string_lossy(), 400));
                }
                Key::Escape => return Action::Close,
                Key::Char(c) if c.is_alphanumeric() => {
                    // Jump to the next entry starting with the typed character.
                    let c = c.to_ascii_lowercase();
                    let starts = |i: usize| dir.entries[i].name.to_lowercase().starts_with(c);
                    if let Some(i) = (self.selected + 1..n).chain(0..self.selected + 1).find(|&i| starts(i)) {
                        self.selected = i;
                    }
                }
                _ => {}
            },
            Mode::Edit(form) => match key {
                Key::Escape => self.mode = Mode::List,
                Key::Enter => match form.to_entry() {
                    Ok(e) => {
                        let index = form.index;
                        let name = e.name.clone();
                        match index {
                            Some(i) => dir.entries[i] = e,
                            None => dir.entries.push(e),
                        }
                        dir.sort();
                        self.selected = dir.entries.iter().position(|x| x.name == name).unwrap_or(0);
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
                Key::Char(' ') | Key::Left | Key::Right if form.focus == F_ENCODING => {
                    form.encoding = match form.encoding {
                        Encoding::Cp437 => Encoding::Utf8,
                        Encoding::Utf8 => Encoding::Cp437,
                    };
                }
                Key::Char(c) if form.focus == F_PORT && !c.is_ascii_digit() => {}
                _ => {
                    if form.focus != F_PROTOCOL && form.focus != F_ENCODING {
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
                if let Key::Char('y' | 'Y') = key {
                    dir.entries.remove(self.selected);
                    self.selected = self.selected.min(dir.entries.len().saturating_sub(1));
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
        Action::None
    }

    fn run_prompt(&mut self, kind: PromptKind, value: &str, dir: &mut Directory) -> Action {
        match kind {
            PromptKind::QuickConnect => match Entry::from_address(value) {
                Some(e) => Action::QuickConnect(e),
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
        s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', GRAY, BLACK));
        let title = format!(" Phosphor {} \u{2502} Dialing Directory", env!("CARGO_PKG_VERSION"));
        text(s, 0, 0, COLS, &title, WHITE, BLUE);
        let count = format!("{} entries ", dir.entries.len());
        label(s, COLS - count.len(), 0, &count, LIGHT_CYAN, BLUE);

        frame(s, 0, 1, COLS, 21, "", LIGHT_CYAN, BLUE);
        let header = format!(" {:<30} {:<30} {:<7} {:>5}", "Name", "Address", "Type", "Calls");
        text(s, 1, 2, COLS - 2, &header, YELLOW, BLUE);
        divider(s, 0, 3, COLS, LIGHT_CYAN, BLUE);

        let n = dir.entries.len();
        self.selected = self.selected.min(n.saturating_sub(1));
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + LIST_ROWS {
            self.top = self.selected + 1 - LIST_ROWS;
        }
        if n == 0 {
            text(
                s,
                2,
                LIST_TOP + 1,
                COLS - 4,
                "No entries yet. Press Ins to add a BBS, Q to quick-connect,",
                GRAY,
                BLUE,
            );
            text(
                s,
                2,
                LIST_TOP + 2,
                COLS - 4,
                "or I to import a SyncTERM .lst, CSV, JSON or TOML directory.",
                GRAY,
                BLUE,
            );
        }
        for (row, (i, e)) in dir.entries.iter().enumerate().skip(self.top).take(LIST_ROWS).enumerate() {
            let line = format!(
                " {:<30} {:<30} {:<7} {:>5}",
                truncate(&e.name, 30),
                truncate(&e.address(), 30),
                e.protocol.name(),
                e.calls
            );
            let (fg, bg) = if i == self.selected {
                (BLACK, CYAN)
            } else if e.protocol.supported() {
                (WHITE, BLUE)
            } else {
                (GRAY, BLUE)
            };
            text(s, 1, LIST_TOP + row, COLS - 2, &line, fg, bg);
        }
        // Scroll indicators.
        if self.top > 0 {
            label(s, COLS - 1, LIST_TOP, "\u{25B2}", YELLOW, BLUE);
        }
        if self.top + LIST_ROWS < n {
            label(s, COLS - 1, LIST_TOP + LIST_ROWS - 1, "\u{25BC}", YELLOW, BLUE);
        }

        if let Some(e) = dir.entries.get(self.selected) {
            let last = e.last_connected.map(format_date).unwrap_or_else(|| "never".into());
            let mut info = format!(" Last called: {last}   Encoding: {}", e.encoding.name());
            if !e.comment.is_empty() {
                info.push_str(&format!("   {}", e.comment));
            }
            text(s, 0, 22, COLS, &info, LIGHT_CYAN, BLACK);
            if !e.protocol.supported() {
                text(
                    s,
                    0,
                    23,
                    COLS,
                    &format!(" {} is not supported yet; this entry is kept for when it is.", e.protocol),
                    DARK_GRAY,
                    BLACK,
                );
            }
        }

        match &self.mode {
            Mode::List => None,
            Mode::Edit(form) => Some(draw_form(s, form)),
            Mode::Prompt(kind, field) => {
                let (title, hint) = match kind {
                    PromptKind::QuickConnect => {
                        ("Quick Connect", "host, host:port, telnet://host:port or raw://host:port")
                    }
                    PromptKind::Import => ("Import Directory", "SyncTERM .lst, .csv, .json or .toml file"),
                    PromptKind::Export => ("Export Directory", "Format follows extension: .lst .csv .json .toml"),
                };
                let (x, y, w) = (6, 9, 68);
                frame(s, x, y, w, 6, title, WHITE, BLUE);
                label(s, x + 2, y + 1, hint, GRAY, BLUE);
                Some(field.draw(s, x + 2, y + 3, w - 4, false, WHITE, BLACK))
            }
            Mode::ConfirmDelete => {
                let name = dir.entries.get(self.selected).map(|e| e.name.as_str()).unwrap_or("");
                let msg = format!("Remove \"{}\" from the directory? (Y/N)", truncate(name, 40));
                message_box(s, "Confirm", &msg);
                None
            }
            Mode::ConfirmHangUp(msg) => {
                message_box(s, "Still connected", msg);
                None
            }
            Mode::Message(msg) => {
                message_box(s, "Phosphor", msg);
                None
            }
        }
    }
}

fn draw_form(s: &mut Screen, form: &Form) -> (usize, usize) {
    let (x, y, w, h) = (8, 4, 64, FIELDS.len() + 6);
    let title = if form.index.is_some() { "Edit Entry" } else { "New Entry" };
    frame(s, x, y, w, h, title, WHITE, BLUE);
    let mut cursor = (0, 0);
    let fx = x + 18;
    let fw = w - 20;
    for (i, name) in FIELDS.iter().enumerate() {
        let yy = y + 2 + i;
        let focused = i == form.focus;
        text(s, x + 2, yy, 16, &format!("{name:>15}"), if focused { YELLOW } else { GRAY }, BLUE);
        let (fg, bg) = if focused { (WHITE, BLACK) } else { (LIGHT_CYAN, BLUE) };
        match i {
            F_PROTOCOL => {
                let v = format!("\u{25C4} {:<7} \u{25BA}", form.protocol.name());
                text(s, fx, yy, fw, &v, fg, bg);
                if !form.protocol.supported() {
                    label(s, fx + 13, yy, "(not yet supported)", DARK_GRAY, bg);
                }
            }
            F_ENCODING => text(s, fx, yy, fw, &format!("\u{25C4} {:<7} \u{25BA}", form.encoding.name()), fg, bg),
            _ => {
                let pos = form.text[i].draw(s, fx, yy, fw, i == F_PASSWORD, fg, bg);
                if focused {
                    cursor = pos;
                }
            }
        }
    }
    if let Some(err) = &form.error {
        text(s, x + 2, y + h - 2, w - 4, err, LIGHT_RED, BLUE);
    }
    cursor
}

fn message_box(s: &mut Screen, title: &str, msg: &str) {
    let lines = wrap(msg, 60);
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(title.len() + 4) + 6;
    let h = lines.len() + 4;
    let x = (COLS - w) / 2;
    let y = (ROWS - h) / 2;
    frame(s, x, y, w, h, title, WHITE, BLUE);
    for (i, l) in lines.iter().enumerate() {
        label(s, x + 3, y + 2 + i, l, WHITE, BLUE);
    }
}

fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&truncate(word, width));
    }
    lines.push(cur);
    lines
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('\u{00BB}');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new()), dir());
        assert!(matches!(key(&mut ui, &mut d, Key::Escape), Action::Close));
    }

    #[test]
    fn hang_up_confirmation() {
        let (mut ui, mut d) = (DialerUi::new(PathBuf::new()), dir());
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
}
