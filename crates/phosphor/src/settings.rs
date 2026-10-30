//! Display preferences kept between runs, in `settings.toml` next to the
//! dialing directory.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::crt::CrtMode;

/// Which ANSI music to play (Alt-M).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Music {
    Off,
    /// Music started by `ESC [ N` or `ESC [ |`.
    #[default]
    On,
    /// Also `ESC [ M` without a parameter, which otherwise deletes a line.
    All,
}

impl Music {
    pub fn next(self) -> Music {
        match self {
            Music::Off => Music::On,
            Music::On => Music::All,
            Music::All => Music::Off,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Music::Off => "ANSI music off",
            Music::On => "ANSI music on",
            Music::All => "ANSI music on, including ESC[M",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Phosphor mode: the CRT look (Alt-P).
    pub crt: CrtMode,
    /// 9-pixel-wide character cells, as on a VGA (Alt-W).
    pub nine_dot: bool,
    /// Show the screen at the 4:3 shape of a CRT rather than with square
    /// pixels (Alt-A).
    pub aspect: bool,
    pub music: Music,
    /// Call a busy or unreachable BBS again, a few times.
    pub redial: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { crt: CrtMode::Off, nine_dot: false, aspect: false, music: Music::On, redial: true }
    }
}

impl Settings {
    /// Load the settings; a missing or unreadable file gives the defaults.
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                log::warn!("ignoring {}: {e}", path.display());
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let s = Settings { crt: CrtMode::Amber, nine_dot: true, aspect: true, music: Music::All, redial: false };
        let text = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
        assert!(text.contains("crt = \"amber\""), "{text}");
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
    }
}
