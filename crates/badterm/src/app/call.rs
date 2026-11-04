//! Calls: dialing, hanging up, redialing, the session's events, and what
//! goes over the line (transfers, macros, paste, capture, saving).

use super::*;

impl App {
    pub(super) fn connect(&mut self, mut entry: Entry, index: Option<usize>) {
        self.redial = None;
        self.redials = 0;
        match secrets::password(&entry, &*self.secrets) {
            Ok(p) => entry.password = p,
            // The login asks instead.
            Err(e) => self.notify(format!("Could not read the password from the keychain: {e}")),
        }
        let downloads = self.entry_folder(&entry, Folders::DOWNLOADS);
        // Uploads start in this BBS's folder, not the last one's.
        self.upload_dir = PathBuf::new();
        let config = SessionConfig {
            scrollback: SCROLLBACK_LINES,
            known_hosts: self.paths.known_hosts.clone(),
            download_dir: downloads.clone(),
            ssh_agent: true,
            ssh_key_dir: self.paths.ssh_keys.clone(),
            modem: self.settings.modem.clone(),
        };
        let (cols, rows) = match entry.encoding.screen_size() {
            Some((c, r)) => (c as u16, r as u16),
            None => entry.screen_mode(),
        };
        let mut session = Session::new(entry, cols as usize, rows as usize, config);
        session.term.music_on_csi_m = self.settings.music == Music::All;
        session.term.music_enabled = self.settings.music != Music::Off;
        session.term.syncterm.cache_dir = Some(self.paths.bbs_cache.join(cache_folder(&session.entry)));
        session.term.download_dir = Some(downloads);
        self.load_entry_font(&mut session);
        self.dialer_open = false;
        self.session_id += 1;
        let id = self.session_id;
        let proxy = self.proxy.clone();
        session.connect(move |ev| {
            // Fails only when the event loop has exited.
            let _ = proxy.send_event(UserEvent::Net(id, ev));
        });
        self.set_title(&format!("BadTerm \u{2014} {}", session.entry.name));
        self.session = Some(session);
        self.session_index = index;
        self.scroll = 0;
    }

    /// Dial an address from the command line or a link, with the directory's
    /// settings for that system if it has an entry for it.
    pub(super) fn dial_address(&mut self, e: Entry) {
        let id = e.system_id();
        let index = self.directory.entries.iter().position(|x| x.system_id() == id);
        let e = index.map_or(e, |i| self.directory.entries[i].clone());
        // Where the hang-up question shows, if one is needed.
        self.dialer_open = true;
        self.dial(e, index);
    }

    /// Dial from the directory: straight away, or after confirming that the
    /// live call should be hung up.
    /// The entry's font file, in place of its built-in font.
    fn load_entry_font(&mut self, session: &mut Session) {
        let name = session.entry.font_file.trim();
        if name.is_empty() {
            return;
        }
        let path = ui::expand_home(name);
        let path = if path.is_absolute() { path } else { self.paths.fonts.join(path) };
        let font = std::fs::read(&path).map_err(anyhow::Error::from).and_then(|d| badterm_core::fontfile::parse(&d));
        match font {
            Ok(data) => {
                use badterm_core::screen::ENTRY_FONT;
                let sc = &mut session.term.screen;
                sc.look.load_font(ENTRY_FONT, &data);
                sc.base_font = ENTRY_FONT;
                sc.look.fonts[0] = ENTRY_FONT;
            }
            Err(e) => self.notify(format!("Font {}: {e:#}", path.display())),
        }
    }

    pub(super) fn dial(&mut self, mut entry: Entry, index: Option<usize>) {
        // An address not in the directory gets the settings' font and
        // screen mode, as a new entry would.
        if index.is_none() {
            self.new_entry_defaults(&mut entry);
        }
        match &self.session {
            Some(s) if s.state != State::Closed => {
                self.dialer.confirm_hang_up(&s.entry.name, &entry.name);
                self.pending_dial = Some((entry, index));
            }
            _ => self.connect(entry, index),
        }
    }

    pub(super) fn hang_up(&mut self) {
        self.stop_script(None);
        self.redial = None;
        self.macro_steps.clear();
        self.session = None;
        self.session_index = None;
        self.scroll = 0;
        self.player.stop();
        self.upload_prompt = None;
        self.save_prompt = None;
        self.selection = None;
        self.login = None;
        self.remember = None;
        self.transfer_direction = None;
        self.set_title("BadTerm");
    }

    pub(super) fn on_net(&mut self, id: u64, ev: NetEvent) {
        let Some(session) = self.session.as_mut().filter(|_| id == self.session_id) else {
            // A connection that finished after its session was abandoned.
            if let NetEvent::Connected(writer) = ev {
                writer.shutdown();
            }
            return;
        };
        let connected = matches!(ev, NetEvent::Connected(_));
        let closed = matches!(ev, NetEvent::Closed(_));
        // New text moves what a selection was over.
        if matches!(ev, NetEvent::Data(_)) && !self.dragging && self.scroll == 0 {
            self.selection = None;
        }
        session.handle(ev);
        // The question went away (the connection closed).
        if session.auth_prompt().is_none() && self.login.as_ref().is_some_and(|l| !l.rip) {
            self.login = None;
        }
        if connected && let Some(e) = self.session_entry_mut() {
            e.calls += 1;
            e.last_connected = Some(dialer::now());
            self.save_directory();
        }
        self.feed_script();
        if closed {
            if self.login.as_ref().is_some_and(|l| l.rip) {
                self.login = None;
            }
            self.stop_script(Some("stopped, as the call ended"));
            self.schedule_redial();
        }
        self.session_events();
        self.request_redraw();
    }

    /// Dial a busy or unreachable BBS again after a while, a few times.
    pub(super) fn schedule_redial(&mut self) {
        let Some(s) = self.session.as_ref().filter(|s| s.should_redial()) else { return };
        if !self.settings.redial || self.redials >= MAX_REDIALS {
            return;
        }
        let (entry, index) = (s.entry.clone(), self.session_entry_index());
        self.redial = Some(Redial { entry, index, attempt: self.redials + 1, at: Instant::now() + REDIAL_DELAY });
    }

    /// Make the waiting redial now.
    pub(super) fn redial_now(&mut self) {
        let Some(r) = self.redial.take() else { return };
        self.connect(r.entry, r.index);
        self.redials = r.attempt;
    }

    pub(super) fn session_events(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        for ev in session.take_events() {
            match ev {
                SessionEvent::Term(TermEvent::Bell) => self.bell(),
                SessionEvent::Term(TermEvent::RipQuestion(q)) => {
                    let bbs = self.session.as_ref().map_or_else(String::new, |s| s.entry.name.clone());
                    self.login = Some(LoginPrompt::rip_question(&q, &bbs));
                    self.dialer_open = false;
                    self.scroll = 0;
                }
                SessionEvent::Term(TermEvent::Title(t)) => self.set_title(&format!("BadTerm \u{2014} {t}")),
                SessionEvent::Term(TermEvent::Music(tones)) => {
                    if self.settings.music != Music::Off {
                        self.player.play(tones);
                    }
                }
                // SyncTERM's audio; Alt-M's "off" silences it too.
                SessionEvent::Term(TermEvent::Audio(op)) => {
                    if self.settings.music != Music::Off {
                        self.player.audio(op);
                    }
                }
                SessionEvent::Notice(msg) => self.notify(msg),
                SessionEvent::Connected => {
                    // Got through after redialing: worth a look.
                    if self.redials > 0 {
                        self.attention();
                    }
                    if self.session.as_ref().is_some_and(|s| s.entry.capture && s.capture_path().is_none()) {
                        self.toggle_capture();
                    }
                    // The entry's login script.
                    let script = self.session.as_ref().map(|s| s.entry.script.clone()).unwrap_or_default();
                    if !script.trim().is_empty() {
                        let path = self.script_path(&script);
                        self.run_script(&path);
                    }
                    if let Some(pw) = self.remember.take()
                        && let Some(e) = self.session_entry_mut()
                    {
                        e.password = pw;
                        self.save_directory();
                    }
                }
                SessionEvent::AuthPrompt => {
                    let Some(p) = self.session.as_ref().and_then(|s| s.auth_prompt()) else { continue };
                    self.login = Some(LoginPrompt::new(p, self.session_index.is_some(), self.keychain_ok));
                    self.dialer_open = false;
                    self.scroll = 0;
                    self.attention();
                }
                // Both need the terminal view (the prompt and progress window
                // live there), so they close the dialer if it is open.
                SessionEvent::UploadRequested => {
                    self.dialer_open = false;
                    self.scroll = 0;
                    self.open_upload(true);
                    self.attention();
                }
                SessionEvent::TransferStarted => {
                    self.dialer_open = false;
                    self.scroll = 0;
                    self.upload_prompt = None;
                    self.download_prompt = None;
                    self.transfer_direction =
                        self.session.as_ref().and_then(|s| s.transfer()).map(|t| t.progress().direction);
                }
                SessionEvent::TransferFinished(outcome) => {
                    let msg = self.describe_outcome(&outcome);
                    self.transfer_direction = None;
                    self.notify(msg);
                    self.attention();
                }
            }
        }
    }

    pub(super) fn describe_outcome(&self, outcome: &Outcome) -> String {
        match (outcome, self.transfer_direction) {
            (Outcome::Completed(files), Some(Direction::Download)) => match files.as_slice() {
                [] => "Download finished; no files were sent".into(),
                [one] => format!("Downloaded {}", one.display()),
                many => {
                    let dir = self
                        .session
                        .as_ref()
                        .map_or_else(|| self.folder(Folders::DOWNLOADS), |s| s.download_dir().to_path_buf());
                    format!("Downloaded {} files to {}", many.len(), dir.display())
                }
            },
            (Outcome::Completed(files), _) => match files.as_slice() {
                [one] => format!("Uploaded {}", one.file_name().unwrap_or_default().to_string_lossy()),
                many => format!("Uploaded {} files", many.len()),
            },
            (Outcome::Cancelled, _) => "Transfer cancelled".into(),
            (Outcome::Failed(why), _) => format!("Transfer failed: {why}"),
        }
    }

    pub(super) fn start_xy_upload(&mut self, protocol: badterm_core::transfer::XyProtocol, paths: Vec<PathBuf>) {
        let Some(session) = self.session.as_mut() else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_xy_upload(protocol, paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events();
    }

    pub(super) fn start_kermit_upload(&mut self, paths: Vec<PathBuf>) {
        let Some(session) = self.session.as_mut() else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_kermit_upload(paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events();
    }

    pub(super) fn start_upload(&mut self, paths: Vec<PathBuf>) {
        let Some(session) = self.session.as_mut() else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_upload(paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events();
    }

    /// Where the entry the session was dialed from is in the directory now:
    /// editing the directory can move it.
    pub(super) fn session_entry_index(&self) -> Option<usize> {
        let id = self.session.as_ref()?.entry.system_id();
        let entries = &self.directory.entries;
        let i = self.session_index?;
        if entries.get(i).is_some_and(|e| e.system_id() == id) {
            Some(i)
        } else {
            entries.iter().position(|e| e.system_id() == id)
        }
    }

    pub(super) fn session_entry_mut(&mut self) -> Option<&mut Entry> {
        let i = self.session_entry_index()?;
        self.session_index = Some(i);
        self.directory.entries.get_mut(i)
    }

    /// The phosphor mode the connected system asks for, as it is in the
    /// directory (so edits made while connected show at once).
    pub(super) fn own_crt_mode(&self) -> Option<CrtMode> {
        match self.session_entry_index() {
            Some(i) => self.directory.entries[i].crt,
            None => self.session.as_ref().and_then(|s| s.entry.crt),
        }
    }

    pub(super) fn crt_mode(&self) -> CrtMode {
        self.own_crt_mode().unwrap_or(self.settings.crt)
    }

    /// What the bell does on the connected system: its own setting, or the
    /// global one.
    pub(super) fn bell_mode(&self) -> BellMode {
        let own = match self.session_entry_index() {
            Some(i) => self.directory.entries[i].bell,
            None => self.session.as_ref().and_then(|s| s.entry.bell),
        };
        own.unwrap_or(self.settings.bell)
    }

    /// Ring the bell: a beep, a flash, or both (at most every so often, as
    /// some BBSes ring it in bursts), and a nudge on the taskbar when the
    /// window is in the background.
    pub(super) fn bell(&mut self) {
        if let Some(g) = self.gfx.as_ref().filter(|g| !g.window.has_focus()) {
            g.window.request_user_attention(Some(UserAttentionType::Informational));
        }
        let now = Instant::now();
        if self.last_bell.is_some_and(|t| now.duration_since(t) < BELL_GAP) {
            return;
        }
        self.last_bell = Some(now);
        let mode = self.bell_mode();
        if mode.sound() {
            self.player.play(vec![badterm_core::music::Tone { hz: 880, ms: 120 }]);
        }
        if mode.visual() {
            self.flash_until = Some(now + FLASH);
            self.request_redraw();
        }
    }

    /// Send keyboard macro `i` (Alt-1 is 0), pausing where it says.
    pub(super) fn start_macro(&mut self, i: usize) {
        let Some(session) = self.session.as_ref().filter(|s| s.state == State::Connected) else {
            return self.notify("Not connected");
        };
        // The entry's own, or the settings'.
        let own = session.entry.macros.get(i).filter(|m| !m.is_empty());
        let text = own.or(self.settings.macros.get(i)).cloned().unwrap_or_default();
        if text.is_empty() {
            let key = crate::macros::key_name(i);
            return self.notify(format!(
                "{HOTKEY}-{key} has no macro yet: set one in Settings ({})",
                crate::hotkeys::label("O")
            ));
        }
        let e = &session.entry;
        self.macro_steps = crate::macros::expand(&text, &e.username, &e.password).into();
        self.macro_at = Instant::now();
        self.run_macro();
    }

    /// Send what a macro has ready, up to its next pause.
    pub(super) fn run_macro(&mut self) {
        if self.macro_steps.is_empty() || Instant::now() < self.macro_at {
            return;
        }
        let Some(session) = self.session.as_mut().filter(|s| s.state == State::Connected) else {
            self.macro_steps.clear();
            return;
        };
        while let Some(step) = self.macro_steps.pop_front() {
            match step {
                crate::macros::Step::Send(text) => {
                    let bytes = session.term.encoding.encode_text(&text);
                    session.send(&bytes);
                }
                crate::macros::Step::Pause => {
                    self.macro_at = Instant::now() + crate::macros::PAUSE;
                    return;
                }
            }
        }
    }

    pub(super) fn toggle_capture(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        if session.capture_path().is_some() {
            session.stop_capture();
            return self.notify("Capture stopped");
        }
        let entry = session.entry.clone();
        let path = capture_file(&entry, &self.folder(Folders::CAPTURES), dialer::now());
        let Some(session) = self.session.as_mut() else { return };
        let msg = match session.start_capture(path.clone()) {
            Ok(()) => format!("Capturing to {}", path.display()),
            Err(e) => format!("Capture failed: {e:#}"),
        };
        self.notify(msg);
    }

    pub(super) fn paste(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        let text = match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(t) => t,
            Err(e) => return self.notify(format!("Paste failed: {e}")),
        };
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let bytes = session.term.paste(&text);
        session.send(&bytes);
    }

    /// Write the scrollback (or, for a PNG, the screen) to `path`; returns
    /// what was saved, for the notice.
    pub(super) fn save(&mut self, path: &std::path::Path) -> Result<String> {
        let Some(session) = &self.session else { anyhow::bail!("not connected") };
        let Some(format) = SaveFormat::from_path(path) else {
            anyhow::bail!("use a file name ending in .ans, .txt or .png");
        };
        let sc = &session.term.screen;
        let all: Vec<&[Cell]> =
            (0..sc.scrollback_len()).map(|i| sc.scrollback_line(i)).chain((0..sc.rows()).map(|y| sc.row(y))).collect();
        let (data, what) = match format {
            SaveFormat::Ansi => {
                let opts = AnsiOptions { ice: sc.ice_colors, unicode: false, codepage: None };
                (export::ansi(&all, opts), format!("{} lines", all.len()))
            }
            SaveFormat::Text => {
                (export::text_in(&all, session.term.text_codepage()).into_bytes(), format!("{} lines", all.len()))
            }
            SaveFormat::Png => {
                let mut r = Renderer::new(Font::vga_8x16(), sc.cols(), sc.rows());
                let font = if Font::eight_only(sc.look.fonts[0]) { Font::vga_8x8() } else { Self::font_for(sc.rows()) };
                r.set_layout(font, self.cell_width(), sc.cols(), sc.rows());
                let rows = terminal_rows(session, self.scroll);
                let look_rows = rows.len();
                let frame = Frame {
                    look: &sc.look,
                    look_rows,
                    tiles: Some(&sc.tiles),
                    ..Frame::plain(rows, None, sc.ice_colors)
                };
                r.draw(&frame, true, false);
                (crate::png::encode(r.pixels(), r.native_width(), r.native_height()), "the screen".into())
            }
        };
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))?;
        Ok(what)
    }
}

/// Where an entry's capture goes: its own file name (with `{name}`,
/// `{date}` and `{time}` filled in, relative names in the captures folder),
/// or `name-date-seconds.ans` there.
pub(super) fn capture_file(e: &Entry, captures: &std::path::Path, now: u64) -> PathBuf {
    let pattern = e.capture_file.trim();
    if pattern.is_empty() {
        return captures.join(format!("{}-{}.ans", file_stem(&e.name), stamp(now)));
    }
    let secs = now % 86_400;
    let time = format!("{:02}{:02}{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    let name =
        pattern.replace("{name}", &file_stem(&e.name)).replace("{date}", &format_date(now)).replace("{time}", &time);
    let path = expand_home(&name);
    if path.is_absolute() { path } else { captures.join(path) }
}

/// A BBS's cache folder: its address, made safe as a file name.
fn cache_folder(entry: &Entry) -> String {
    let name: String = format!("{}_{}", entry.host, entry.port)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    name.trim_start_matches('.').to_string()
}
