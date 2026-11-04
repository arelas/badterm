//! The dialing directory: saved BBS entries, stored as TOML, with import and
//! export in SyncTERM `.lst`, CSV, JSON and TOML formats.

use std::fmt;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::ansi::Encoding;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Telnet,
    /// Telnet over TLS.
    #[serde(rename = "telnets")]
    TelnetS,
    /// Plain TCP with no telnet processing.
    Raw,
    Ssh,
    Rlogin,
    /// A real modem on a serial port, dialing the entry's phone number (its
    /// address) with the modem settings (see [`crate::modem`]).
    Modem,
}

impl Protocol {
    pub const ALL: [Protocol; 6] =
        [Protocol::Telnet, Protocol::TelnetS, Protocol::Raw, Protocol::Ssh, Protocol::Rlogin, Protocol::Modem];

    /// The usual port; 0 for a modem, which has none.
    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Telnet | Protocol::Raw => 23,
            Protocol::TelnetS => 992,
            Protocol::Ssh => 22,
            Protocol::Rlogin => 513,
            Protocol::Modem => 0,
        }
    }

    /// SyncTERM's names for them, too.
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Telnet => "Telnet",
            Protocol::TelnetS => "TelnetS",
            Protocol::Raw => "Raw",
            Protocol::Ssh => "SSH",
            Protocol::Rlogin => "RLogin",
            Protocol::Modem => "Modem",
        }
    }

    /// Whether the connection speaks the telnet protocol.
    pub fn is_telnet(self) -> bool {
        matches!(self, Protocol::Telnet | Protocol::TelnetS)
    }

    fn from_name(s: &str) -> Option<Protocol> {
        match s.trim().to_ascii_lowercase().as_str() {
            "telnet" => Some(Protocol::Telnet),
            "telnets" => Some(Protocol::TelnetS),
            "raw" => Some(Protocol::Raw),
            "ssh" | "sshna" => Some(Protocol::Ssh),
            "rlogin" | "rloginreversed" => Some(Protocol::Rlogin),
            "modem" | "dialup" | "serial" => Some(Protocol::Modem),
            _ => None,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub encoding: Encoding,
    pub username: String,
    /// Stored in plain text, as in other BBS dialers.
    pub password: String,
    /// Answer the BBS's login prompts with `username` and `password`.
    #[serde(skip_serializing_if = "is_true")]
    pub auto_login: bool,
    pub term_type: String,
    /// Screen size: one of [`SCREEN_MODES`].
    #[serde(skip_serializing_if = "is_80")]
    pub cols: u16,
    #[serde(skip_serializing_if = "is_25")]
    pub rows: u16,
    /// The font, as a CTerm font ID: 0 for CP437, or an Amiga font for
    /// Amiga BBSes.
    #[serde(skip_serializing_if = "is_zero")]
    pub font: u8,
    /// iCE colors (the blink bit as a bright background) from the start:
    /// None leaves it to the BBS (`CSI ?33h`), as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ice: Option<bool>,
    /// Baud rate emulation: show what the BBS sends at this speed, as over
    /// a modem (0: as fast as it arrives).
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub baud: u32,
    /// Phosphor mode for this system; None follows the global setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crt: Option<CrtMode>,
    /// What the bell (Ctrl-G) does on this system; None follows the global
    /// setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bell: Option<BellMode>,
    /// Start capturing the session to a file as soon as it connects.
    #[serde(skip_serializing_if = "is_false")]
    pub capture: bool,
    /// The capture file's name, with `{name}`, `{date}` and `{time}` filled
    /// in; relative names are in the captures folder, and a `.txt` name
    /// keeps only the text (see [`crate::session::CaptureKind`]). Empty for
    /// the usual `name-date-time.ans`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub capture_file: String,
    /// SSH private key to try first; otherwise the SSH agent and the usual
    /// `~/.ssh/id_*` files are tried.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key_file: String,
    /// Where this BBS's downloads go, and where uploads to it start
    /// looking; empty for the folders in the settings.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub download_dir: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub upload_dir: String,
    /// No status bar under this BBS's screen.
    #[serde(skip_serializing_if = "is_false")]
    pub hide_status: bool,
    /// The group it is listed in (the directory can show one at a time).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
    pub comment: String,
    /// The password is kept in the OS keychain rather than in `password`.
    #[serde(skip_serializing_if = "is_false")]
    pub keychain: bool,
    /// Pinned entries sort ahead of the rest.
    #[serde(skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// Unix timestamps.
    pub added: Option<u64>,
    pub last_connected: Option<u64>,
    pub calls: u32,
}

impl Default for Entry {
    fn default() -> Self {
        Entry {
            name: String::new(),
            host: String::new(),
            port: 23,
            protocol: Protocol::Telnet,
            encoding: Encoding::Cp437,
            username: String::new(),
            password: String::new(),
            auto_login: true,
            term_type: "ANSI".into(),
            cols: 80,
            rows: 25,
            font: 0,
            ice: None,
            baud: 0,
            crt: None,
            bell: None,
            capture: false,
            capture_file: String::new(),
            key_file: String::new(),
            download_dir: String::new(),
            upload_dir: String::new(),
            hide_status: false,
            group: String::new(),
            comment: String::new(),
            keychain: false,
            pinned: false,
            added: None,
            last_connected: None,
            calls: 0,
        }
    }
}

impl Entry {
    /// The screen size, (columns, rows): the entry's, if it is one BadTerm
    /// has, or 80x25.
    pub fn screen_mode(&self) -> (u16, u16) {
        let mode = (self.cols, self.rows);
        if SCREEN_MODES.contains(&mode) { mode } else { SCREEN_MODES[0] }
    }

    /// Parse `telnet://host:port`, `ssh://user@host`, `host:port` or `host`
    /// into an entry.
    pub fn from_address(addr: &str) -> Option<Entry> {
        let addr = addr.trim();
        let (protocol, rest) = match addr.split_once("://") {
            Some((scheme, rest)) => (Protocol::from_name(scheme)?, rest),
            None => (Protocol::Telnet, addr),
        };
        let rest = rest.trim_end_matches('/');
        let (username, rest) = match rest.rsplit_once('@') {
            Some((user, host)) => (user.to_string(), host),
            None => (String::new(), rest),
        };
        let (host, port) = split_host_port(rest)?;
        let port = port.unwrap_or(protocol.default_port());
        Some(Entry { name: host.clone(), host, port, protocol, username, added: Some(now()), ..Entry::default() })
    }

    pub fn address(&self) -> String {
        if self.port == self.protocol.default_port() || self.protocol == Protocol::Modem {
            self.host.clone()
        } else if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn same_system(&self, other: &Entry) -> bool {
        self.host.eq_ignore_ascii_case(&other.host) && self.port == other.port && self.protocol == other.protocol
    }

    /// Identifies the system (protocol, host and port), e.g. for remembering
    /// which default entries were already offered.
    pub fn system_id(&self) -> String {
        format!("{}://{}:{}", self.protocol.name().to_ascii_lowercase(), self.host.to_ascii_lowercase(), self.port)
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

/// The baud rates an entry can emulate; 0 is no limit.
pub const BAUD_RATES: [u32; 11] = [0, 300, 1200, 2400, 9600, 14400, 19200, 28800, 33600, 57600, 115200];

fn is_80(n: &u16) -> bool {
    *n == 80
}

fn is_25(n: &u16) -> bool {
    *n == 25
}

/// The screen sizes (columns, rows) an entry can ask for. Modes of 43 rows
/// or more use the 8x8 font.
pub const SCREEN_MODES: [(u16, u16); 6] = [(80, 25), (80, 43), (80, 50), (132, 25), (132, 37), (132, 52)];

/// What the bell (Ctrl-G) does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BellMode {
    /// A beep.
    #[default]
    Sound,
    /// The screen flashes.
    Visual,
    Both,
    Off,
}

impl BellMode {
    pub const ALL: [BellMode; 4] = [BellMode::Sound, BellMode::Visual, BellMode::Both, BellMode::Off];

    pub fn name(self) -> &'static str {
        match self {
            BellMode::Sound => "Beep",
            BellMode::Visual => "Flash",
            BellMode::Both => "Beep and flash",
            BellMode::Off => "Off",
        }
    }

    pub fn sound(self) -> bool {
        matches!(self, BellMode::Sound | BellMode::Both)
    }

    pub fn visual(self) -> bool {
        matches!(self, BellMode::Visual | BellMode::Both)
    }
}

/// "Phosphor mode": the look of a CRT, drawn by the front end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrtMode {
    #[default]
    Off,
    /// A color CRT.
    Color,
    /// A green-phosphor monochrome monitor (P1).
    Green,
    /// An amber monochrome monitor (P3).
    Amber,
}

impl CrtMode {
    pub const ALL: [CrtMode; 4] = [CrtMode::Off, CrtMode::Color, CrtMode::Green, CrtMode::Amber];

    pub fn next(self) -> CrtMode {
        let i = CrtMode::ALL.iter().position(|&m| m == self).unwrap_or(0);
        CrtMode::ALL[(i + 1) % CrtMode::ALL.len()]
    }

    pub fn name(self) -> &'static str {
        match self {
            CrtMode::Off => "Off",
            CrtMode::Color => "Color CRT",
            CrtMode::Green => "Green screen",
            CrtMode::Amber => "Amber screen",
        }
    }
}

/// The entries a new directory starts with, first among them the project's
/// own BBS. More can be added here over time: existing directories pick up
/// new ones once (see [`Directory::offer_defaults`]).
pub fn default_entries() -> Vec<Entry> {
    vec![Entry {
        name: "The Vortex BBS".into(),
        host: "vortexbbs.com".into(),
        port: 23,
        protocol: Protocol::Telnet,
        comment: "Home BBS of the BadTerm project".into(),
        pinned: true,
        ..Entry::default()
    }]
}

fn split_host_port(s: &str) -> Option<(String, Option<u16>)> {
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p.parse().ok()?),
            None => None,
        };
        return Some((host.to_string(), port));
    }
    match s.rsplit_once(':') {
        // A bare IPv6 address has several colons and no port.
        Some((h, p)) if !h.contains(':') => Some((h.to_string(), Some(p.parse().ok()?))),
        _ => Some((s.to_string(), None)),
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Toml,
    Json,
    /// Ours, or a list such as the Telnet BBS Guide's `bbslist.csv`.
    Csv,
    SyncTerm,
    /// MagiTerm's `magiterm.ini`: SyncTERM's layout, but every entry SSH.
    /// Import only.
    MagiTerm,
    /// EtherTerm's `dialdirectory.xml`. Import only.
    EtherTerm,
    /// NetRunner's binary `netrunner.phn`. Import only.
    NetRunner,
    /// Qodem's `fonebook.txt`. Import only.
    Qodem,
}

impl Format {
    pub fn from_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            "csv" => Some(Format::Csv),
            "ini" if path.file_stem().is_some_and(|s| s.eq_ignore_ascii_case("magiterm")) => Some(Format::MagiTerm),
            "lst" | "ini" => Some(Format::SyncTerm),
            "xml" => Some(Format::EtherTerm),
            "phn" => Some(Format::NetRunner),
            "txt" if path.file_stem().is_some_and(|s| s.eq_ignore_ascii_case("fonebook")) => Some(Format::Qodem),
            _ => None,
        }
    }
}

/// How the dialing directory is listed. Pinned entries always come first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    #[default]
    Name,
    /// Most recently called first.
    LastCalled,
    /// Most called first.
    Calls,
}

impl SortOrder {
    pub fn name(self) -> &'static str {
        match self {
            SortOrder::Name => "Name",
            SortOrder::LastCalled => "Last called",
            SortOrder::Calls => "Calls",
        }
    }

    pub fn next(self) -> SortOrder {
        match self {
            SortOrder::Name => SortOrder::LastCalled,
            SortOrder::LastCalled => SortOrder::Calls,
            SortOrder::Calls => SortOrder::Name,
        }
    }

    fn is_default(&self) -> bool {
        *self == SortOrder::Name
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    /// How the list is shown.
    #[serde(default, skip_serializing_if = "SortOrder::is_default")]
    pub sort: SortOrder,
    /// Default entries already offered to this directory (by
    /// [`Entry::system_id`]), so one the user deleted does not come back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defaults_offered: Vec<String>,
    #[serde(default, rename = "entry")]
    pub entries: Vec<Entry>,
}

impl Directory {
    /// The groups entries are in, in order, without repeats (ignoring case).
    pub fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = Vec::new();
        for e in &self.entries {
            let g = e.group.trim();
            if !g.is_empty() && !groups.iter().any(|h| h.eq_ignore_ascii_case(g)) {
                groups.push(g.to_string());
            }
        }
        groups.sort_by_key(|g| g.to_lowercase());
        groups
    }

    /// Load the directory, returning an empty one if the file does not exist.
    pub fn load(path: &Path) -> Result<Directory> {
        match fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Directory::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Save atomically (write to a temporary file, then rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
    }

    /// Pinned entries first, then by name.
    pub fn sort(&mut self) {
        self.entries.sort_by_key(|e| (!e.pinned, e.name.to_lowercase()));
    }

    /// Indices of the entries to list, in `self.sort` order, keeping those
    /// whose name, address or comment contains `filter` (ignoring case).
    pub fn view(&self, filter: &str) -> Vec<usize> {
        self.view_group(filter, None)
    }

    /// [`Directory::view`], of one group's entries.
    pub fn view_group(&self, filter: &str, group: Option<&str>) -> Vec<usize> {
        let filter = filter.trim().to_lowercase();
        let mut v: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                group.is_none_or(|g| e.group.trim().eq_ignore_ascii_case(g))
                    && (filter.is_empty()
                        || [&e.name, &e.host, &e.comment, &e.group].iter().any(|f| f.to_lowercase().contains(&filter))
                        || e.address().to_lowercase().contains(&filter))
            })
            .collect();
        let name = |i: &usize| self.entries[*i].name.to_lowercase();
        match self.sort {
            SortOrder::Name => v.sort_by_key(|i| (!self.entries[*i].pinned, name(i))),
            SortOrder::LastCalled => v.sort_by_key(|i| {
                let e = &self.entries[*i];
                (!e.pinned, std::cmp::Reverse(e.last_connected.unwrap_or(0)), name(i))
            }),
            SortOrder::Calls => v.sort_by_key(|i| {
                let e = &self.entries[*i];
                (!e.pinned, std::cmp::Reverse(e.calls), name(i))
            }),
        }
        v
    }

    /// Add any default entries this directory has not been offered before.
    /// Returns true if the directory changed and should be saved.
    pub fn offer_defaults(&mut self) -> bool {
        let mut changed = false;
        for e in default_entries() {
            let id = e.system_id();
            if self.defaults_offered.contains(&id) {
                continue;
            }
            if !self.entries.iter().any(|x| x.same_system(&e)) {
                self.entries.push(Entry { added: Some(now()), ..e });
            }
            self.defaults_offered.push(id);
            changed = true;
        }
        if changed {
            self.sort();
        }
        changed
    }

    /// Add entries that are not already present (same host, port and
    /// protocol). Returns how many were added.
    pub fn merge(&mut self, new: Vec<Entry>) -> usize {
        let mut added = 0;
        for e in new {
            if !self.entries.iter().any(|x| x.same_system(&e)) {
                self.entries.push(e);
                added += 1;
            }
        }
        self.sort();
        added
    }
}

/// Read entries from a file, choosing the format by extension (falling back
/// to sniffing the content).
pub fn import(path: &Path) -> Result<Vec<Entry>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    // NetRunner's phone book is binary.
    if bytes.starts_with(NETRUNNER_MAGIC) || Format::from_path(path) == Some(Format::NetRunner) {
        return Ok(finish_import(from_netrunner(&bytes)?));
    }
    // SyncTERM lists from Windows may be in a legacy code page.
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => crate::cp437::decode(e.as_bytes()),
    };
    let text = text.trim_start_matches('\u{feff}');
    let format = Format::from_path(path).or_else(|| sniff(text));
    let entries = match format {
        Some(Format::Toml) => toml::from_str::<Directory>(text)?.entries,
        Some(Format::Json) => from_json(text)?,
        Some(Format::Csv) => from_csv(text)?,
        Some(Format::SyncTerm) => from_syncterm(text),
        Some(Format::MagiTerm) => from_syncterm(text)
            .into_iter()
            .map(|mut e| {
                if e.protocol == Protocol::Telnet {
                    e.protocol = Protocol::Ssh;
                    if e.port == Protocol::Telnet.default_port() {
                        e.port = Protocol::Ssh.default_port();
                    }
                }
                e
            })
            .collect(),
        Some(Format::EtherTerm) => from_etherterm(text)?,
        Some(Format::Qodem) => from_qodem(text),
        // Read above, as bytes.
        Some(Format::NetRunner) => unreachable!("NetRunner phone books are read before decoding"),
        None => bail!("unrecognized dialing directory format: {}", path.display()),
    };
    Ok(finish_import(entries))
}

/// Write entries to a file in the format implied by its extension.
pub fn export(path: &Path, entries: &[Entry]) -> Result<()> {
    let Some(format) = Format::from_path(path) else {
        bail!("use a .lst, .csv, .json or .toml file name to choose the export format");
    };
    let text = match format {
        Format::Toml => toml::to_string_pretty(&Directory { entries: entries.to_vec(), ..Directory::default() })?,
        Format::Json => serde_json::to_string_pretty(entries)?,
        Format::Csv => to_csv(entries)?,
        Format::SyncTerm => to_syncterm(entries),
        Format::MagiTerm | Format::EtherTerm | Format::NetRunner | Format::Qodem => {
            bail!("that format can be imported, not exported")
        }
    };
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn sniff(text: &str) -> Option<Format> {
    let t = text.trim_start();
    if is_qodem(t) {
        Some(Format::Qodem)
    } else if t.starts_with('{') || t.starts_with('[') && t[1..].trim_start().starts_with('{') {
        Some(Format::Json)
    } else if t.starts_with("<?xml") || t.starts_with("<EtherTerm") {
        Some(Format::EtherTerm)
    } else if t.starts_with("[[entry]]") {
        Some(Format::Toml)
    } else if t.starts_with('[') {
        Some(Format::SyncTerm)
    } else if t.lines().next().is_some_and(|l| l.contains(',')) {
        Some(Format::Csv)
    } else {
        None
    }
}

fn from_json(text: &str) -> Result<Vec<Entry>> {
    // Accept either a bare array or our directory object.
    match serde_json::from_str::<Vec<Entry>>(text) {
        Ok(v) => Ok(v),
        Err(_) => Ok(serde_json::from_str::<Directory>(text)?.entries),
    }
}

const CSV_HEADER: [&str; 8] = ["name", "host", "port", "protocol", "encoding", "username", "term_type", "comment"];

fn to_csv(entries: &[Entry]) -> Result<String> {
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(CSV_HEADER)?;
    for e in entries {
        w.write_record([
            e.name.as_str(),
            &e.host,
            &e.port.to_string(),
            e.protocol.name(),
            e.encoding.name(),
            &e.username,
            &e.term_type,
            &e.comment,
        ])?;
    }
    Ok(String::from_utf8(w.into_inner()?)?)
}

/// Parse CSV with a header row. Columns are matched by name, so files from
/// spreadsheets or BBS lists with extra or reordered columns still import.
fn from_csv(text: &str) -> Result<Vec<Entry>> {
    let mut r = csv::ReaderBuilder::new().flexible(true).trim(csv::Trim::All).from_reader(text.as_bytes());
    let headers: Vec<String> = r.headers()?.iter().map(|h| h.to_ascii_lowercase()).collect();
    let col = |names: &[&str]| headers.iter().position(|h| names.contains(&h.as_str()));
    let c_name = col(&["name", "bbs", "bbs name", "system", "bbsname"]);
    let c_host = col(&["host", "address", "hostname", "telnet", "telnetaddress"]);
    let c_port = col(&["port", "bbsport"]);
    // The Telnet BBS Guide's list: an SSH port, the new-user login, and
    // details worth keeping as the comment.
    let c_ssh_port = col(&["sshport"]);
    let c_new_login = col(&["newlogin"]);
    let c_details: Vec<usize> = ["software", "location", "bbssysop"].iter().filter_map(|n| col(&[n])).collect();
    let c_proto = col(&["protocol", "type", "connectiontype"]);
    let c_enc = col(&["encoding", "charset"]);
    let c_user = col(&["username", "user"]);
    let c_tt = col(&["term_type", "terminal"]);
    let c_comment = col(&["comment", "comments", "description", "notes"]);
    let Some(c_host) = c_host else { bail!("CSV needs a 'host' or 'address' column") };

    let mut out = Vec::new();
    for rec in r.records() {
        let rec = rec?;
        let get = |c: Option<usize>| c.and_then(|c| rec.get(c)).unwrap_or("").to_string();
        let mut e = match Entry::from_address(&get(Some(c_host))) {
            Some(e) => e,
            None => continue,
        };
        if let Some(p) = c_proto.and_then(|c| rec.get(c)).and_then(Protocol::from_name) {
            if e.port == e.protocol.default_port() {
                e.port = p.default_port();
            }
            e.protocol = p;
        }
        if let Ok(port) = get(c_port).parse() {
            e.port = port;
        } else if let Ok(port) = get(c_ssh_port).parse() {
            // Only SSH: log in as the BBS's new-user name, as it says.
            e.protocol = Protocol::Ssh;
            e.port = port;
            e.username = get(c_new_login);
        }
        let name = get(c_name);
        if !name.is_empty() {
            e.name = name;
        }
        if get(c_enc).to_ascii_lowercase().replace('-', "") == "utf8" {
            e.encoding = Encoding::Utf8;
        }
        if c_user.is_some() {
            e.username = get(c_user);
        }
        let tt = get(c_tt);
        if !tt.is_empty() {
            e.term_type = tt;
        }
        e.comment = get(c_comment);
        if e.comment.is_empty() {
            let details: Vec<String> = c_details.iter().map(|&c| get(Some(c))).filter(|d| !d.is_empty()).collect();
            e.comment = details.join(", ");
        }
        out.push(e);
    }
    Ok(out)
}

/// Name imported entries that have none, and date them.
fn finish_import(entries: Vec<Entry>) -> Vec<Entry> {
    let stamp = now();
    entries
        .into_iter()
        .filter(|e| !e.host.is_empty())
        .map(|mut e| {
            if e.name.is_empty() {
                e.name = e.host.clone();
            }
            e.added.get_or_insert(stamp);
            e
        })
        .collect()
}

/// The start of a NetRunner phone book: "NetRunner Phone Book" as a Pascal
/// string.
const NETRUNNER_MAGIC: &[u8] = b"\x14NetRunner Phone Book";

/// Parse NetRunner's `netrunner.phn`: a header, then 200 fixed records of
/// 362 bytes, made of Pascal strings (a length byte, then up to 40
/// characters): the name, the address (`host` or `host:port`), user name
/// and password, then the sysop and BBS software, kept as the comment.
/// Unused records have no name or address.
fn from_netrunner(data: &[u8]) -> Result<Vec<Entry>> {
    const START: usize = 48;
    const RECORD: usize = 362;
    if !data.starts_with(NETRUNNER_MAGIC) {
        bail!("not a NetRunner phone book");
    }
    let text = |r: &[u8], at: usize| -> String {
        let len = (r[at] as usize).min(40);
        crate::cp437::decode(&r[at + 1..at + 1 + len]).trim().to_string()
    };
    let mut out = Vec::new();
    for r in data.get(START..).unwrap_or(&[]).as_chunks::<RECORD>().0 {
        let (name, address) = (text(r, 0), text(r, 41));
        let Some(mut e) = Entry::from_address(&address) else { continue };
        if !name.is_empty() {
            e.name = name;
        }
        e.username = text(r, 82);
        e.password = text(r, 123);
        let details: Vec<String> = [text(r, 320), text(r, 279)].into_iter().filter(|d| !d.is_empty()).collect();
        e.comment = details.join(", ");
        out.push(e);
    }
    Ok(out)
}

/// Parse EtherTerm's `dialdirectory.xml`: `<BBS name=".." ip=".."
/// port=".." protocol="TELNET|ssh" login=".." password=".." font=".." />`.
fn from_etherterm(text: &str) -> Result<Vec<Entry>> {
    if !text.contains("<Phonebook") && !text.contains("<BBS") {
        bail!("not an EtherTerm dialing directory");
    }
    let mut out = Vec::new();
    for tag in text.split("<BBS").skip(1) {
        let Some(end) = tag.find('>') else { continue };
        let attrs = xml_attributes(&tag[..end]);
        let get =
            |k: &str| attrs.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str()).unwrap_or("");
        let Some(mut e) = Entry::from_address(get("ip")) else { continue };
        if let Some(p) = Protocol::from_name(get("protocol")) {
            e.protocol = p;
            e.port = p.default_port();
        }
        if let Ok(port) = get("port").parse() {
            e.port = port;
        }
        e.name = get("name").trim().to_string();
        e.username = get("login").to_string();
        e.password = get("password").to_string();
        let font = get("font").to_ascii_lowercase();
        e.font = match () {
            _ if font.contains("topaz") && font.contains("plus") => 40,
            _ if font.contains("topaz") => 42,
            _ if font.contains("microknight") && font.contains("plus") => 39,
            _ if font.contains("microknight") => 41,
            _ if font.contains("noodle") => 37,
            _ if font.contains("sooul") || font.contains("mosoul") => 38,
            _ => 0,
        };
        out.push(e);
    }
    Ok(out)
}

/// The `name="value"` pairs of an XML tag, with entities decoded.
fn xml_attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().rsplit(char::is_whitespace).next().unwrap_or("").to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let Some(close) = after[1..].find(quote) else { break };
        out.push((name, xml_unescape(&after[1..1 + close])));
        rest = &after[close + 2..];
    }
    out
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let Some(semi) = rest[amp..].find(';') else { break };
        let entity = &rest[amp + 1..amp + semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[amp..amp + semi + 1]),
        }
        rest = &rest[amp + semi + 1..];
    }
    out.push_str(rest);
    out
}

/// Parse a SyncTERM dialing directory (`syncterm.lst`): an INI file with one
/// section per BBS.
/// Qodem's phone book: `[entry]` sections of `key=value` lines, after a
/// "# Qodem Phonebook" comment.
fn is_qodem(text: &str) -> bool {
    text.starts_with("# Qodem") || text.lines().take(20).any(|l| l.trim() == "[entry]") && text.contains("\nmethod=")
}

/// Parse Qodem's `fonebook.txt`. Its methods are TELNET, SSH, RLOGIN,
/// SOCKET (raw TCP) and MODEM (the address is a phone number); LOCAL and
/// CMDLINE entries run programs and are left out. Notes span lines, from
/// `notes=<<<END` to `END`, and become the comment.
fn from_qodem(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut cur: Option<Entry> = None;
    let mut skip = false;
    let mut notes: Option<Vec<String>> = None;
    let finish = |e: Option<Entry>, skip: bool, out: &mut Vec<Entry>| {
        if let Some(mut e) = e.filter(|_| !skip) {
            if e.port == 0 {
                e.port = e.protocol.default_port();
            }
            out.push(e);
        }
    };
    for line in text.lines() {
        if let Some(n) = notes.as_mut() {
            if line == "END" {
                let text = notes.take().unwrap_or_default().join(" ");
                if let Some(e) = cur.as_mut() {
                    e.comment = text.trim().to_string();
                }
            } else {
                n.push(line.trim().to_string());
            }
            continue;
        }
        let line = line.trim();
        if line == "[entry]" {
            finish(cur.take(), skip, &mut out);
            cur = Some(Entry { port: 0, ..Entry::default() });
            skip = false;
            continue;
        }
        let (Some(e), Some((key, value))) = (cur.as_mut(), line.split_once('=')) else { continue };
        let value = value.trim();
        match key {
            "name" => e.name = value.to_string(),
            "address" => e.host = value.to_string(),
            "port" => e.port = value.parse().unwrap_or(0),
            "username" => e.username = value.to_string(),
            "password" => e.password = value.to_string(),
            "method" => match value {
                "TELNET" => e.protocol = Protocol::Telnet,
                "SSH" => e.protocol = Protocol::Ssh,
                "RLOGIN" => e.protocol = Protocol::Rlogin,
                "SOCKET" => e.protocol = Protocol::Raw,
                "MODEM" => e.protocol = Protocol::Modem,
                _ => skip = true,
            },
            "emulation" => match value {
                "AVATAR" => e.encoding = Encoding::Avatar,
                "PETSCII" => e.encoding = Encoding::Petscii,
                "ATASCII" => e.encoding = Encoding::Atascii,
                "PRESTEL" => e.encoding = Encoding::Viewdata,
                "BEEB" => e.encoding = Encoding::Mode7,
                "L_UTF8" | "X_UTF8" => e.encoding = Encoding::Utf8,
                _ => {}
            },
            "codepage" if value.eq_ignore_ascii_case("utf-8") => e.encoding = Encoding::Utf8,
            "times_on" => e.calls = value.parse().unwrap_or(0),
            "last_call" => e.last_connected = value.parse().ok().filter(|&t| t != 0),
            "capture_filename" if !value.is_empty() => e.capture_file = value.to_string(),
            "notes" if value == "<<<END" => notes = Some(Vec::new()),
            _ => {}
        }
    }
    finish(cur.take(), skip, &mut out);
    out
}

fn from_syncterm(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut cur: Option<Entry> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.extend(cur.take());
            cur = Some(Entry { name: name.to_string(), port: 0, ..Entry::default() });
            continue;
        }
        let (Some(e), Some((key, value))) = (cur.as_mut(), line.split_once('=')) else { continue };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "address" => e.host = value.to_string(),
            "port" => e.port = value.parse().unwrap_or(0),
            "connectiontype" => match Protocol::from_name(value) {
                Some(p) => e.protocol = p,
                None => log::warn!("{}: unsupported connection type {value}", e.name),
            },
            "username" => e.username = value.to_string(),
            "password" => e.password = value.to_string(),
            "comment" => e.comment = value.to_string(),
            "totalcalls" => e.calls = value.parse().unwrap_or(0),
            "added" => e.added = value.parse().ok(),
            "lastconnected" => e.last_connected = value.parse().ok().filter(|&t| t != 0),
            _ => {}
        }
    }
    out.extend(cur);
    for e in &mut out {
        // SyncTERM addresses may carry the port (host:port) or a URL scheme.
        if let Some(parsed) = Entry::from_address(&e.host) {
            if e.port == 0 && parsed.port != parsed.protocol.default_port() {
                e.port = parsed.port;
            }
            e.host = parsed.host;
        }
        if e.port == 0 {
            e.port = e.protocol.default_port();
        }
    }
    out
}

fn to_syncterm(entries: &[Entry]) -> String {
    let mut s = String::new();
    for e in entries {
        // Section names cannot contain brackets.
        let name: String = e.name.chars().map(|c| if c == '[' || c == ']' { '_' } else { c }).collect();
        s.push_str(&format!("[{name}]\n"));
        s.push_str(&format!("\tAddress={}\n", e.host));
        s.push_str(&format!("\tPort={}\n", e.port));
        s.push_str(&format!("\tConnectionType={}\n", e.protocol.name()));
        if let Some(t) = e.added {
            s.push_str(&format!("\tAdded={t}\n"));
        }
        if let Some(t) = e.last_connected {
            s.push_str(&format!("\tLastConnected={t}\n"));
        }
        s.push_str(&format!("\tTotalCalls={}\n", e.calls));
        if !e.username.is_empty() {
            s.push_str(&format!("\tUserName={}\n", e.username));
        }
        if !e.password.is_empty() {
            s.push_str(&format!("\tPassword={}\n", e.password));
        }
        if !e.comment.is_empty() {
            s.push_str(&format!("\tComment={}\n", e.comment));
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Entry> {
        vec![
            Entry {
                name: "Alpha BBS".into(),
                host: "alpha.example.org".into(),
                comment: "Has, commas".into(),
                ..Entry::default()
            },
            Entry {
                name: "Beta".into(),
                host: "beta.example.org".into(),
                port: 2323,
                crt: Some(CrtMode::Amber),
                ..Entry::default()
            },
        ]
    }

    #[test]
    fn parse_addresses() {
        let e = Entry::from_address("telnet://bbs.example.com:2323").unwrap();
        assert_eq!((e.host.as_str(), e.port), ("bbs.example.com", 2323));
        let e = Entry::from_address("bbs.example.com").unwrap();
        assert_eq!(e.port, 23);
        let e = Entry::from_address("[::1]:23").unwrap();
        assert_eq!(e.host, "::1");
        let e = Entry::from_address("ssh://bbs.example.com").unwrap();
        assert_eq!((e.protocol, e.port), (Protocol::Ssh, 22));
        let e = Entry::from_address("ssh://guest@bbs.example.com:2222").unwrap();
        assert_eq!((e.username.as_str(), e.host.as_str(), e.port), ("guest", "bbs.example.com", 2222));
        assert!(Entry::from_address("host:notaport").is_none());
    }

    #[test]
    fn syncterm_round_trip() {
        let text = to_syncterm(&sample());
        let back = from_syncterm(&text);
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].port, 2323);
        assert_eq!(back[0].comment, "Has, commas");
    }

    #[test]
    fn syncterm_real_world() {
        let text = "[Some BBS]\n\tAddress=bbs.example.net:2300\n\tConnectionType=Telnet\n\tScreenMode=80x25\n\
                    [Secure]\nAddress=s.example.net\nConnectionType=SSH\nPort=2222\n";
        let v = from_syncterm(text);
        assert_eq!((v[0].host.as_str(), v[0].port), ("bbs.example.net", 2300));
        assert_eq!((v[1].protocol, v[1].port), (Protocol::Ssh, 2222));
    }

    #[test]
    fn csv_round_trip() {
        let text = to_csv(&sample()).unwrap();
        let back = from_csv(&text).unwrap();
        assert_eq!(back[0].name, "Alpha BBS");
        assert_eq!(back[0].comment, "Has, commas");
        assert_eq!(back[1].port, 2323);
    }

    #[test]
    fn csv_with_foreign_columns() {
        let text = "BBS Name,Address,Sysop\nFoo,foo.example.com:2023,Bob\n";
        let v = from_csv(text).unwrap();
        assert_eq!((v[0].name.as_str(), v[0].port), ("Foo", 2023));
    }

    #[test]
    fn toml_and_json_round_trip() {
        let d = Directory { entries: sample(), defaults_offered: vec!["telnet://x:23".into()], ..Directory::default() };
        let t = toml::to_string_pretty(&d).unwrap();
        assert_eq!(toml::from_str::<Directory>(&t).unwrap(), d);
        let j = serde_json::to_string(&d.entries).unwrap();
        assert_eq!(from_json(&j).unwrap(), d.entries);
    }

    #[test]
    fn merge_skips_duplicates() {
        let mut d = Directory { entries: sample(), ..Directory::default() };
        let added = d.merge(sample());
        assert_eq!(added, 0);
        assert_eq!(d.entries.len(), 2);
    }

    #[test]
    fn defaults_are_offered_once() {
        let mut d = Directory { entries: sample(), ..Directory::default() };
        assert!(d.offer_defaults());
        // Pinned, so first despite the name.
        assert_eq!(d.entries[0].host, "vortexbbs.com");
        assert_eq!(d.entries.len(), 3);
        assert!(!d.offer_defaults());

        // Deleting it sticks, including across a save and load.
        d.entries.remove(0);
        let d: Directory = toml::from_str(&toml::to_string_pretty(&d).unwrap()).unwrap();
        let mut d = d;
        assert!(!d.offer_defaults());
        assert_eq!(d.entries.len(), 2);
    }

    #[test]
    fn new_fields_are_optional_in_toml() {
        let d: Directory = toml::from_str("[[entry]]\nname = \"A\"\nhost = \"a.example\"\n").unwrap();
        assert!(!d.entries[0].pinned && !d.entries[0].keychain && d.entries[0].key_file.is_empty());
        let t = toml::to_string_pretty(&d).unwrap();
        assert!(!t.contains("pinned") && !t.contains("keychain") && !t.contains("key_file"), "{t}");
        assert!(d.entries[0].crt.is_none() && !t.contains("crt"), "{t}");
    }

    #[test]
    fn crt_mode_in_toml() {
        let d: Directory = toml::from_str("[[entry]]\nhost = \"a.example\"\ncrt = \"green\"\n").unwrap();
        assert_eq!(d.entries[0].crt, Some(CrtMode::Green));
        assert_eq!(CrtMode::Amber.next(), CrtMode::Off);
    }

    #[test]
    fn view_sorts_and_filters() {
        let mut d = Directory { entries: sample(), ..Directory::default() };
        d.entries[0].calls = 1;
        d.entries[1].calls = 5;
        d.entries[0].last_connected = Some(200);
        d.entries[1].last_connected = Some(100);
        assert_eq!(d.view(""), [0, 1]);
        d.sort = SortOrder::Calls;
        assert_eq!(d.view(""), [1, 0]);
        d.sort = SortOrder::LastCalled;
        assert_eq!(d.view(""), [0, 1]);
        // Pinned entries stay on top whatever the order.
        d.entries[1].pinned = true;
        assert_eq!(d.view(""), [1, 0]);

        assert_eq!(d.view("ALPHA"), [0]);
        // Groups.
        d.entries[0].group = "Retro".into();
        d.entries[1].group = "retro ".into();
        assert_eq!(d.groups(), ["Retro"]);
        d.entries[1].group = "Art".into();
        assert_eq!(d.groups(), ["Art", "Retro"]);
        assert_eq!(d.view_group("", Some("retro")), [0]);
        assert_eq!(d.view_group("", Some("Art")), [1]);
        assert_eq!(d.view("art"), [1]);
        assert_eq!(d.view("commas"), [0]);
        assert_eq!(d.view(":2323"), [1]);
        assert!(d.view("nothing like it").is_empty());

        let t = toml::to_string_pretty(&d).unwrap();
        assert!(t.contains("sort = \"last_called\""), "{t}");
        assert_eq!(toml::from_str::<Directory>(&t).unwrap().sort, SortOrder::LastCalled);
    }

    #[test]
    fn telnet_bbs_guide_csv() {
        let text = "bbsName, bbsSysop, newLogin, TelnetAddress, bbsPort, sshPort, WebAddress, location, Modem, software\n\
                    13th Floor BBS,Hoy Brothers,new,13th.hoyvision.com,6400,,,\"Oak Ridge, TN, USA\",,\"Image BBS\"\n\
                    3D Realms,,NEW,172.0.79.226,,,,\"Cotati, CA, USA\",,\"Synchronet\"\n\
                    4 Wheel Ham BBS,w7jpj,NEW,bbs.4wheelham.com,,2222,,\"Littleton, CO, USA\",,\"Synchronet\"\n";
        let v = from_csv(text).unwrap();
        assert_eq!((v[0].name.as_str(), v[0].host.as_str(), v[0].port), ("13th Floor BBS", "13th.hoyvision.com", 6400));
        assert_eq!(v[0].comment, "Image BBS, Oak Ridge, TN, USA, Hoy Brothers");
        assert_eq!((v[1].protocol, v[1].port), (Protocol::Telnet, 23));
        assert_eq!((v[2].protocol, v[2].port, v[2].username.as_str()), (Protocol::Ssh, 2222, "NEW"));
    }

    #[test]
    fn etherterm_and_magiterm() {
        let xml = r#"<?xml version="1.0" ?>
<EtherTerm>
    <Phonebook version="1.0">
        <BBS name="Tom &amp; Jerry&#39;s" ip="bbs.example.org" port="6523" protocol="TELNET" login="" password="" font="vga8x16.bmp" keyMap="ANSI" />
        <BBS name="Amiga Land (ssh)" ip="amiga.example.org" port="2222" protocol="ssh" login="guest" password="pw" font="topaz-a1200.bmp" keyMap="ANSI" />
    </Phonebook>
</EtherTerm>"#;
        assert_eq!(sniff(xml), Some(Format::EtherTerm));
        let v = from_etherterm(xml).unwrap();
        assert_eq!(v[0].name, "Tom & Jerry's");
        assert_eq!((v[0].protocol, v[0].port), (Protocol::Telnet, 6523));
        assert_eq!((v[1].protocol, v[1].port, v[1].username.as_str(), v[1].font), (Protocol::Ssh, 2222, "guest", 42));

        let dir = std::env::temp_dir().join(format!("badterm-magiterm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("magiterm.ini");
        std::fs::write(
            &path,
            "[A-Net Online]\naddress = mystic-anet.online \nport = 22 \n\n[After Hours]\naddress = ah-bbs.com\n",
        )
        .unwrap();
        let v = import(&path).unwrap();
        assert_eq!((v[0].protocol, v[0].port), (Protocol::Ssh, 22));
        assert_eq!((v[1].protocol, v[1].port), (Protocol::Ssh, 22));
        assert!(export(&dir.join("out.xml"), &v).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn netrunner_phone_book() {
        let book = include_bytes!("../tests/data/netrunner.phn");
        let v = from_netrunner(book).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].name.as_str(), v[0].host.as_str(), v[0].port), ("The Vortex BBS", "vortexbbs.com", 23));
        assert_eq!((v[0].username.as_str(), v[0].password.as_str()), ("User Here", "Password Here"));
        assert_eq!(v[0].comment, "Enigma 1/2, Sysop Name Here");
        assert_eq!((v[1].name.as_str(), v[1].host.as_str()), ("Another System", "another.com"));
        assert!(from_netrunner(b"not a phone book").is_err());

        // host:port in the address, as NetRunner takes it.
        let mut edited = book.to_vec();
        let at = 48 + 41;
        let addr = b"bbs.example.org:2323";
        edited[at] = addr.len() as u8;
        edited[at + 1..at + 1 + addr.len()].copy_from_slice(addr);
        let v = from_netrunner(&edited).unwrap();
        assert_eq!((v[0].host.as_str(), v[0].port), ("bbs.example.org", 2323));
    }

    #[test]
    fn imports_qodem() {
        let text = "# Qodem Phonebook\n#\n\n[entry]\nname=The Vortex\naddress=vortexbbs.com\nport=\n\
            username=guest\npassword=pw\nmethod=TELNET\nemulation=AVATAR\ncodepage=CP437\ntimes_on=7\n\
            last_call=1700000000\nnotes=<<<END\nBest BBS\nin town\nEND\ncapture_filename=vortex.txt\n\n\
            [entry]\nname=Local shell\naddress=\nmethod=LOCAL\n\n\
            [entry]\nname=Dial-up\naddress=555-1234\nport=\nmethod=MODEM\nemulation=ANSI\n\n\
            [entry]\nname=Secure\naddress=ssh.example.org\nport=2222\nmethod=SSH\nemulation=X_UTF8\n";
        let dir = std::env::temp_dir().join(format!("badterm-qodem-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // Found by its name, or by its content.
        for name in ["fonebook.txt", "phonebook"] {
            fs::write(dir.join(name), text).unwrap();
            let v = import(&dir.join(name)).unwrap();
            assert_eq!(v.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["The Vortex", "Dial-up", "Secure"]);
            let e = &v[0];
            assert_eq!(
                (e.host.as_str(), e.port, e.protocol, e.encoding),
                ("vortexbbs.com", 23, Protocol::Telnet, Encoding::Avatar)
            );
            assert_eq!((e.username.as_str(), e.password.as_str(), e.calls), ("guest", "pw", 7));
            assert_eq!((e.comment.as_str(), e.capture_file.as_str()), ("Best BBS in town", "vortex.txt"));
            assert_eq!(
                (v[1].protocol, v[1].host.as_str(), v[1].address()),
                (Protocol::Modem, "555-1234", "555-1234".into())
            );
            assert_eq!((v[2].protocol, v[2].port, v[2].encoding), (Protocol::Ssh, 2222, Encoding::Utf8));
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
