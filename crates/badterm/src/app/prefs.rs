//! Settings: the settings screen, folders, themes, bezels and the update
//! check, and putting changes into effect.

use super::*;

impl App {
    /// Where BadTerm puts things, or starts looking, when the settings
    /// don't say (`Folders` order).
    pub(super) fn default_folder(&self, i: usize) -> PathBuf {
        match i {
            Folders::DOWNLOADS => self.paths.downloads.clone(),
            Folders::CAPTURES => self.paths.captures.clone(),
            Folders::UPLOADS => std::env::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            // Saved screens and the art picker go with the downloads.
            _ => self.folder(Folders::DOWNLOADS),
        }
    }

    /// A folder for calling this entry: its own downloads or uploads
    /// folder, if it has one, or [`App::folder`].
    pub(super) fn entry_folder(&self, e: &Entry, i: usize) -> PathBuf {
        let own = match i {
            Folders::DOWNLOADS => e.download_dir.trim(),
            Folders::UPLOADS => e.upload_dir.trim(),
            _ => "",
        };
        if own.is_empty() { self.folder(i) } else { ui::expand_home(own) }
    }

    /// The font and screen mode the settings give new entries, on an entry
    /// that has the usual ones.
    pub(super) fn new_entry_defaults(&self, e: &mut Entry) {
        if e.font == 0 {
            e.font = self.settings.entry_font;
        }
        if (e.cols, e.rows) == (80, 25) {
            (e.cols, e.rows) = self.settings.entry_screen;
        }
    }

    /// A folder: the one chosen in the settings, or the default.
    pub(super) fn folder(&self, i: usize) -> PathBuf {
        self.settings.folders.get(i).cloned().unwrap_or_else(|| self.default_folder(i))
    }

    pub(super) fn open_settings(&mut self) {
        let defaults = (0..Folders::NAMES.len()).map(|i| self.default_folder(i)).collect();
        self.settings_ui.open(theme::user_themes(&self.paths.themes), bezel::names(&self.paths.bezels), defaults);
        self.settings_open = true;
        self.help_open = false;
    }

    /// Copy a theme file into the themes folder and use it.
    pub(super) fn import_theme(&mut self, path: &std::path::Path) -> Result<(String, PathBuf)> {
        let path = ui::expand_home(&path.to_string_lossy());
        let file = ThemeFile::load(&path)?;
        if theme::SCHEMES.iter().any(|(n, _)| n.eq_ignore_ascii_case(&file.name)) {
            anyhow::bail!("\"{}\" is the name of a built-in scheme", file.name);
        }
        let dest = self.paths.themes.join(theme::file_name(&file.name));
        file.save(&dest)?;
        self.settings.colors = file.colors;
        self.settings_ui.set_themes(theme::user_themes(&self.paths.themes));
        self.apply_settings();
        Ok((file.name, dest))
    }

    /// Load the bezel the settings name, if it changed.
    pub(super) fn load_bezel(&mut self) {
        if self.settings.bezel == self.bezel_name {
            return;
        }
        self.bezel_name = self.settings.bezel.clone();
        self.bezel_frame = None;
        self.bezel = None;
        if self.bezel_name.is_empty() {
            return;
        }
        match Bezel::find(&self.bezel_name, &self.paths.bezels) {
            Ok(b) => self.bezel = Some(b),
            Err(e) => self.notify(format!("Bezel: {e:#}")),
        }
    }

    /// Ask in the background for a newer version, once per run, if the
    /// user has turned the check on. Failures are only logged.
    pub(super) fn check_for_updates(&mut self) {
        if !self.settings.check_updates || self.update_checked {
            return;
        }
        self.update_checked = true;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let current = env!("CARGO_PKG_VERSION");
            match badterm_core::update::latest(current) {
                Ok(r) if badterm_core::update::is_newer(&r.version, current) => {
                    let _ = proxy.send_event(UserEvent::UpdateAvailable(r.version));
                }
                Ok(_) => log::info!("BadTerm is up to date"),
                Err(e) => log::warn!("checking for updates: {e:#}"),
            }
        });
    }

    /// Put changed settings into effect, and save them.
    pub(super) fn apply_settings(&mut self) {
        // The pickers start in the chosen folders again.
        for t in &mut self.tabs {
            t.upload_dir = PathBuf::new();
        }
        self.art_dir = PathBuf::new();
        self.check_for_updates();
        self.load_bezel();
        crate::hotkeys::set(&self.settings.hotkeys);
        ui::theme::set_current(self.settings.colors);
        self.idle_screen = idle_screen(self.update.as_deref());
        if self.settings.music == Music::Off {
            self.player.stop();
        }
        for t in &mut self.tabs {
            t.session.term.music_on_csi_m = self.settings.music == Music::All;
            t.session.term.music_enabled = self.settings.music != Music::Off;
        }
        if let Err(e) = self.settings.save(&self.paths.settings) {
            log::warn!("saving settings: {e:#}");
        }
    }

    pub(super) fn settings_changed(&mut self, msg: impl Into<String>) {
        if let Err(e) = self.settings.save(&self.paths.settings) {
            log::warn!("saving settings: {e:#}");
        }
        self.notify(msg);
    }

    pub(super) fn open_art_picker(&mut self) {
        if self.art_dir.as_os_str().is_empty() {
            let art = self.folder(Folders::ART);
            self.art_dir = if art.is_dir() { art } else { std::env::home_dir().unwrap_or_else(|| PathBuf::from(".")) };
        }
        self.art_picker = Some(FilePicker::new(self.art_dir.clone(), false, art_ui::EXTENSIONS));
        self.picking_script = false;
        self.help_open = false;
        self.settings_open = false;
    }

    /// Show a file in the ANSI viewer.
    pub(super) fn view_art(&mut self, path: &std::path::Path) {
        match ArtView::open(path, 0) {
            Ok(v) => {
                if let Some(dir) = path.parent() {
                    self.art_dir = dir.to_path_buf();
                }
                self.art = Some(v);
            }
            Err(e) => self.notify(format!("{e:#}")),
        }
    }
}
