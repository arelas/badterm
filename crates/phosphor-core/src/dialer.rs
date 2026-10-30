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
}

impl Protocol {
    pub const ALL: [Protocol; 5] =
        [Protocol::Telnet, Protocol::TelnetS, Protocol::Raw, Protocol::Ssh, Protocol::Rlogin];

    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Telnet | Protocol::Raw => 23,
            Protocol::TelnetS => 992,
            Protocol::Ssh => 22,
            Protocol::Rlogin => 513,
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
    /// Screen height: 25 lines, or 50 (with the 8x8 font).
    #[serde(skip_serializing_if = "is_25")]
    pub rows: u16,
    /// Phosphor mode for this system; None follows the global setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crt: Option<CrtMode>,
    /// SSH private key to try first; otherwise the SSH agent and the usual
    /// `~/.ssh/id_*` files are tried.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key_file: String,
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
            rows: 25,
            crt: None,
            key_file: String::new(),
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
        if self.port == self.protocol.default_port() {
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

fn is_25(n: &u16) -> bool {
    *n == 25
}

/// The screen heights an entry can ask for.
pub const SCREEN_ROWS: [u16; 2] = [25, 50];

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
            CrtMode::Off => "off",
            CrtMode::Color => "color CRT",
            CrtMode::Green => "green screen",
            CrtMode::Amber => "amber screen",
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
        comment: "Home BBS of the Phosphor project".into(),
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
    Csv,
    SyncTerm,
}

impl Format {
    pub fn from_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            "csv" => Some(Format::Csv),
            "lst" | "ini" => Some(Format::SyncTerm),
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
            SortOrder::Name => "name",
            SortOrder::LastCalled => "last called",
            SortOrder::Calls => "calls",
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
        let filter = filter.trim().to_lowercase();
        let mut v: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                filter.is_empty()
                    || [&e.name, &e.host, &e.comment].iter().any(|f| f.to_lowercase().contains(&filter))
                    || e.address().to_lowercase().contains(&filter)
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
        None => bail!("unrecognized dialing directory format: {}", path.display()),
    };
    let stamp = now();
    Ok(entries
        .into_iter()
        .filter(|e| !e.host.is_empty())
        .map(|mut e| {
            if e.name.is_empty() {
                e.name = e.host.clone();
            }
            e.added.get_or_insert(stamp);
            e
        })
        .collect())
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
    };
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn sniff(text: &str) -> Option<Format> {
    let t = text.trim_start();
    if t.starts_with('{') || t.starts_with('[') && t[1..].trim_start().starts_with('{') {
        Some(Format::Json)
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
    let c_name = col(&["name", "bbs", "bbs name", "system"]);
    let c_host = col(&["host", "address", "hostname", "telnet"]);
    let c_port = col(&["port"]);
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
        }
        let name = get(c_name);
        if !name.is_empty() {
            e.name = name;
        }
        if get(c_enc).to_ascii_lowercase().replace('-', "") == "utf8" {
            e.encoding = Encoding::Utf8;
        }
        e.username = get(c_user);
        let tt = get(c_tt);
        if !tt.is_empty() {
            e.term_type = tt;
        }
        e.comment = get(c_comment);
        out.push(e);
    }
    Ok(out)
}

/// Parse a SyncTERM dialing directory (`syncterm.lst`): an INI file with one
/// section per BBS.
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
        assert_eq!(d.view("commas"), [0]);
        assert_eq!(d.view(":2323"), [1]);
        assert!(d.view("nothing like it").is_empty());

        let t = toml::to_string_pretty(&d).unwrap();
        assert!(t.contains("sort = \"last_called\""), "{t}");
        assert_eq!(toml::from_str::<Directory>(&t).unwrap().sort, SortOrder::LastCalled);
    }
}
