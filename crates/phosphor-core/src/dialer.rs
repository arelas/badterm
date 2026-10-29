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
    /// Plain TCP with no telnet processing.
    Raw,
    Ssh,
    Rlogin,
}

impl Protocol {
    pub const ALL: [Protocol; 4] = [Protocol::Telnet, Protocol::Raw, Protocol::Ssh, Protocol::Rlogin];

    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Telnet | Protocol::Raw => 23,
            Protocol::Ssh => 22,
            Protocol::Rlogin => 513,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Protocol::Telnet => "Telnet",
            Protocol::Raw => "Raw",
            Protocol::Ssh => "SSH",
            Protocol::Rlogin => "RLogin",
        }
    }

    /// Whether this build can actually connect with the protocol.
    pub fn supported(self) -> bool {
        matches!(self, Protocol::Telnet | Protocol::Raw | Protocol::Ssh)
    }

    fn from_name(s: &str) -> Option<Protocol> {
        match s.trim().to_ascii_lowercase().as_str() {
            "telnet" | "telnets" => Some(Protocol::Telnet),
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
    pub term_type: String,
    pub comment: String,
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
            term_type: "ANSI".into(),
            comment: String::new(),
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
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

    pub fn sort(&mut self) {
        self.entries.sort_by_key(|e| e.name.to_lowercase());
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
        Format::Toml => toml::to_string_pretty(&Directory { entries: entries.to_vec() })?,
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
            Entry { name: "Beta".into(), host: "beta.example.org".into(), port: 2323, ..Entry::default() },
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
        let d = Directory { entries: sample() };
        let t = toml::to_string_pretty(&d).unwrap();
        assert_eq!(toml::from_str::<Directory>(&t).unwrap(), d);
        let j = serde_json::to_string(&d.entries).unwrap();
        assert_eq!(from_json(&j).unwrap(), d.entries);
    }

    #[test]
    fn merge_skips_duplicates() {
        let mut d = Directory { entries: sample() };
        let added = d.merge(sample());
        assert_eq!(added, 0);
        assert_eq!(d.entries.len(), 2);
    }
}
