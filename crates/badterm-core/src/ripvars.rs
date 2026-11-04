//! RIPscrip's text variables (see [`crate::rip`]): `$DATE$`, `$TIME$` and
//! the other built-in ones, and the user's own, which a BBS defines with
//! `RIP_DEFINE` (`1D`) and fills into what buttons send or what it asks
//! with `RIP_QUERY` (`1 ESC`).

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{Datelike, Local, Timelike};

/// A user variable's definition (`1D`): `NAME[,width]:[?question?][default]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Define {
    pub name: String,
    pub width: usize,
    pub question: String,
    pub default: String,
}

impl Define {
    pub fn parse(text: &[u8]) -> Option<Define> {
        let text = String::from_utf8_lossy(text);
        let (head, rest) = text.split_once(':')?;
        let (name, width) = match head.split_once(',') {
            Some((n, w)) => (n, w.trim().parse().unwrap_or(60)),
            None => (head, 60),
        };
        let name = name.trim().to_ascii_uppercase();
        if !is_name(&name) {
            return None;
        }
        let (question, default) = match rest.strip_prefix('?').and_then(|r| r.split_once('?')) {
            Some((q, d)) => (q.to_string(), d.to_string()),
            None => (String::new(), rest.to_string()),
        };
        Some(Define { name, width: width.clamp(1, 60), question, default })
    }

    /// What to ask: the BBS's question, or RIPterm's own.
    pub fn prompt(&self) -> String {
        if self.question.is_empty() { format!("Enter {}", self.name) } else { self.question.clone() }
    }
}

/// A variable's name: 1-12 letters, digits and underscores, not starting
/// with a digit or an underscore.
pub fn is_name(s: &str) -> bool {
    (1..=12).contains(&s.len())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
}

/// The built-in variables that give a value. `mouse`: where the mouse was
/// last clicked on the graphics, if it was.
pub fn builtin(name: &str, mouse: Option<(usize, usize)>) -> Option<String> {
    let now = Local::now();
    let (x, y) = mouse.unwrap_or((0, 0));
    let hour12 = match now.hour() % 12 {
        0 => 12,
        h => h,
    };
    Some(match name {
        "RIPVER" => String::from_utf8_lossy(crate::rip::VERSION_REPLY).into_owned(),
        "DATE" => now.format("%m/%d/%y").to_string(),
        "MONTH" => now.format("%B").to_string(),
        "MONTHNUM" => format!("{:02}", now.month()),
        "DAY" => format!("{:02}", now.day()),
        "DOY" => format!("{:03}", now.ordinal()),
        "YEAR" => format!("{:02}", now.year() % 100),
        "FYEAR" => format!("{:04}", now.year()),
        "TIME" => now.format("%H:%M:%S").to_string(),
        "HOUR" => format!("{hour12:02}"),
        "MHOUR" => format!("{:02}", now.hour()),
        "MIN" => format!("{:02}", now.minute()),
        "SEC" => format!("{:02}", now.second()),
        "AMPM" => if now.hour() < 12 { "AM" } else { "PM" }.into(),
        "DATETIME" => now.format("%a %b %d %H:%M:%S %Y").to_string(),
        "TIMEZONE" => now.format("%Z").to_string(),
        "DOW" => now.format("%A").to_string(),
        "ADOW" => now.format("%a").to_string(),
        "WDAY" => format!("{}", now.weekday().num_days_from_sunday()),
        "WOY" => now.format("%U").to_string(),
        "WOYM" => now.format("%W").to_string(),
        "X" => format!("{x:04}"),
        "Y" => format!("{y:04}"),
        "XY" => format!("{x:04}:{y:04}"),
        "XYM" => format!("{x:04}:{y:04}:000"),
        "M" => "000".into(),
        "MSTAT" => "YES".into(),
        "STATBAR" | "TWIN" | "CURSOR" => "ON".into(),
        _ => return None,
    })
}

/// The variables saved for a BBS: `NAME=value` lines.
pub fn load(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(n, _)| is_name(n))
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect()
}

pub fn save(path: &Path, vars: &BTreeMap<String, String>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text: String = vars.iter().map(|(n, v)| format!("{n}={}\n", v.replace(['\r', '\n'], " "))).collect();
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_and_builtins() {
        let d = Define::parse(b"full_name,30:?What is your full name?John Doe").unwrap();
        assert_eq!(
            d,
            Define {
                name: "FULL_NAME".into(),
                width: 30,
                question: "What is your full name".into(),
                default: "John Doe".into()
            }
        );
        let d = Define::parse(b"AGE:").unwrap();
        assert_eq!((d.width, d.prompt(), d.default.as_str()), (60, "Enter AGE".into(), ""));
        assert!(Define::parse(b"_X:").is_none() && Define::parse(b"no colon").is_none());
        assert_eq!(builtin("RIPVER", None).as_deref(), Some("RIPSCRIP015400"));
        assert_eq!(builtin("XY", Some((297, 321))).as_deref(), Some("0297:0321"));
        assert_eq!(builtin("DATE", None).unwrap().len(), 8);
        assert!(builtin("NOPE", None).is_none());
    }
}
