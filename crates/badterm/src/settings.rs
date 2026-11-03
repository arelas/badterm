//! Display preferences kept between runs, in `settings.toml` next to the
//! dialing directory.

use std::path::{Path, PathBuf};

pub use badterm_core::dialer::BellMode;
pub use badterm_core::modem::ModemConfig;
use serde::{Deserialize, Serialize};

use crate::crt::{CrtMode, Curvature, Glow, Scanlines};
use crate::ui::theme::Theme;

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

/// The shape the text area is shown at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Aspect {
    /// Square pixels: 640x400 is 16:10.
    #[default]
    Square,
    /// As on a CRT.
    #[serde(rename = "4:3")]
    FourThree,
    #[serde(rename = "16:10")]
    SixteenTen,
    #[serde(rename = "16:9")]
    SixteenNine,
}

impl Aspect {
    pub const ALL: [Aspect; 4] = [Aspect::Square, Aspect::FourThree, Aspect::SixteenTen, Aspect::SixteenNine];

    pub fn name(self) -> &'static str {
        match self {
            Aspect::Square => "Square pixels",
            Aspect::FourThree => "4:3, as on a CRT",
            Aspect::SixteenTen => "16:10",
            Aspect::SixteenNine => "16:9",
        }
    }

    /// Width over height to force, or None for square pixels.
    pub fn ratio(self) -> Option<f64> {
        match self {
            Aspect::Square => None,
            Aspect::FourThree => Some(4.0 / 3.0),
            Aspect::SixteenTen => Some(16.0 / 10.0),
            Aspect::SixteenNine => Some(16.0 / 9.0),
        }
    }

    pub fn next(self) -> Aspect {
        let i = Aspect::ALL.iter().position(|&a| a == self).unwrap_or(0);
        Aspect::ALL[(i + 1) % Aspect::ALL.len()]
    }
}

/// Settings files from before the choice of shapes said `aspect = true`.
fn aspect_or_bool<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Aspect, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Old(bool),
        New(Aspect),
    }
    Ok(match Either::deserialize(d)? {
        Either::Old(true) => Aspect::FourThree,
        Either::Old(false) => Aspect::Square,
        Either::New(a) => a,
    })
}

/// How the screen is fitted to the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScaleMode {
    /// Whole-number steps in a window, as large as fits when full screen
    /// or maximized.
    Auto,
    /// Always whole-number steps, so every pixel is exact.
    Integer,
    /// As large as fits, keeping the shape; a window is kept in that
    /// shape too, so there are no borders.
    #[default]
    Fit,
    /// The whole window, whatever its shape.
    Stretch,
}

impl ScaleMode {
    pub const ALL: [ScaleMode; 4] = [ScaleMode::Auto, ScaleMode::Integer, ScaleMode::Fit, ScaleMode::Stretch];

    pub fn name(self) -> &'static str {
        match self {
            ScaleMode::Auto => "Auto",
            ScaleMode::Integer => "Whole numbers",
            ScaleMode::Fit => "Fit, keep shape",
            ScaleMode::Stretch => "Stretch to fill",
        }
    }
}

/// The folders the user has chosen, in [`Folders::NAMES`] order; `None`
/// is BadTerm's default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folders {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captures: Option<PathBuf>,
    /// Where Alt-S saves the scrollback or screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saves: Option<PathBuf>,
    /// Where the upload picker starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploads: Option<PathBuf>,
    /// Where the ANSI viewer's picker starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub art: Option<PathBuf>,
}

impl Folders {
    pub const NAMES: [&str; 5] = ["Downloads", "Captures", "Saved screens", "Uploads start in", "ANSI art starts in"];
    pub const DOWNLOADS: usize = 0;
    pub const CAPTURES: usize = 1;
    pub const SAVES: usize = 2;
    pub const UPLOADS: usize = 3;
    pub const ART: usize = 4;

    fn slot(&mut self, i: usize) -> &mut Option<PathBuf> {
        match i {
            0 => &mut self.downloads,
            1 => &mut self.captures,
            2 => &mut self.saves,
            3 => &mut self.uploads,
            _ => &mut self.art,
        }
    }

    pub fn get(&self, i: usize) -> Option<&PathBuf> {
        [&self.downloads, &self.captures, &self.saves, &self.uploads, &self.art][i].as_ref()
    }

    pub fn set(&mut self, i: usize, path: Option<PathBuf>) {
        *self.slot(i) = path;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Phosphor mode: the CRT look (Alt-P).
    pub crt: CrtMode,
    /// How much phosphor mode's bright text glows.
    pub glow: Glow,
    /// How strong phosphor mode's scanlines are.
    pub scanlines: Scanlines,
    /// How curved phosphor mode's glass is.
    pub curvature: Curvature,
    /// 9-pixel-wide character cells, as on a VGA (Alt-W).
    pub nine_dot: bool,
    /// The shape the screen is shown at (Alt-A).
    #[serde(deserialize_with = "aspect_or_bool")]
    pub aspect: Aspect,
    /// How the screen is scaled to the window.
    pub scaling: ScaleMode,
    pub music: Music,
    /// What the bell (Ctrl-G) does; a directory entry can choose its own.
    pub bell: BellMode,
    /// Call a busy or unreachable BBS again, a few times.
    pub redial: bool,
    /// The colors of BadTerm's own screens.
    pub colors: Theme,
    /// The monitor bezel drawn around the screen, by name; empty for none.
    pub bezel: String,
    /// Keyboard macros, Alt-1 to Alt-0 (see [`crate::macros`]).
    pub macros: Vec<String>,
    /// Where files go and where pickers start; unset ones are the defaults.
    pub folders: Folders,
    /// Show the splash screen at start.
    pub splash: bool,
    /// Ask the project's server for a newer version at each start. Off
    /// unless turned on, as it contacts the server.
    pub check_updates: bool,
    /// The modem, for directory entries that dial out with one.
    pub modem: ModemConfig,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            // Crisp pixels; phosphor mode (Alt-P) is there to turn on, as a
            // color CRT with a light glow, medium scanlines and flat glass.
            crt: CrtMode::Off,
            glow: Glow::Light,
            scanlines: Scanlines::Medium,
            curvature: Curvature::Off,
            nine_dot: false,
            aspect: Aspect::Square,
            scaling: ScaleMode::Fit,
            music: Music::On,
            bell: BellMode::Sound,
            redial: true,
            colors: Theme::default(),
            bezel: String::new(),
            macros: vec![String::new(); crate::macros::COUNT],
            folders: Folders::default(),
            splash: true,
            check_updates: false,
            modem: ModemConfig::default(),
        }
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
        let mut colors = Theme::default();
        colors.selected.fg = 14;
        let s = Settings {
            crt: CrtMode::Amber,
            glow: Glow::Strong,
            scanlines: Scanlines::Strong,
            curvature: Curvature::Off,
            nine_dot: true,
            aspect: Aspect::SixteenNine,
            scaling: ScaleMode::Stretch,
            music: Music::All,
            bell: BellMode::Visual,
            redial: false,
            colors,
            bezel: "Beige".into(),
            macros: vec!["{user}^M".into(); 10],
            folders: Folders { downloads: Some("/tmp/dl".into()), ..Folders::default() },
            splash: false,
            check_updates: true,
            modem: ModemConfig { device: "/dev/ttyUSB0".into(), init: "AT&F".into(), ..ModemConfig::default() },
        };
        let text = toml::to_string_pretty(&s).unwrap();
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), s);
        assert!(text.contains("crt = \"amber\""), "{text}");
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
    }

    #[test]
    fn old_aspect_setting_still_reads() {
        let s: Settings = toml::from_str("aspect = true").unwrap();
        assert_eq!(s.aspect, Aspect::FourThree);
        let s: Settings = toml::from_str("aspect = false\nscaling = \"stretch\"").unwrap();
        assert_eq!((s.aspect, s.scaling), (Aspect::Square, ScaleMode::Stretch));
        let s: Settings = toml::from_str("aspect = \"16:9\"").unwrap();
        assert_eq!(s.aspect, Aspect::SixteenNine);
    }

    #[test]
    fn defaults() {
        let s = Settings::default();
        assert_eq!((s.crt, s.scanlines, s.curvature), (CrtMode::Off, Scanlines::Medium, Curvature::Off));
        assert_eq!((s.nine_dot, s.aspect, s.scaling, s.bezel.as_str()), (false, Aspect::Square, ScaleMode::Fit, ""));
        // A settings file without them gets the same.
        assert_eq!(toml::from_str::<Settings>("music = \"on\"").unwrap(), s);
    }
}
