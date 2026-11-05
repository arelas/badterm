//! Calls: dialing, hanging up, redialing, the session's events, and what
//! goes over the line (transfers, macros, paste, capture, saving).

use super::*;

/// Where a call goes: a tab of its own, or in place of a tab's call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Slot {
    New,
    Replace(usize),
}

impl App {
    /// Dial `entry`, for a tab.
    fn start_call(&mut self, mut entry: Entry, index: Option<usize>) -> Tab {
        match secrets::password(&entry, &*self.secrets) {
            Ok(p) => entry.password = p,
            // The login asks instead.
            Err(e) => self.notify(format!("Could not read the password from the keychain: {e}")),
        }
        // (Uploads start in this BBS's folder: a new tab has no last one.)
        let downloads = self.entry_folder(&entry, Folders::DOWNLOADS);
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
        session.term.fonts_dir = Some(self.paths.fonts.clone());
        self.load_entry_font(&mut session);
        self.last_id += 1;
        let id = self.last_id;
        let proxy = self.proxy.clone();
        session.connect(move |ev| {
            // Fails only when the event loop has exited.
            let _ = proxy.send_event(UserEvent::Net(id, ev));
        });
        Tab::new(session, id, index)
    }

    /// Dial `entry` (from the directory, a link or the command line) and
    /// show the call: in a tab of its own, or in place of a tab's call.
    pub(super) fn open_call(&mut self, entry: Entry, index: Option<usize>, slot: Slot) {
        let replace = match slot {
            Slot::Replace(t) if t < self.tabs.len() => Some(t),
            _ => None,
        };
        if replace.is_none() && self.tabs.len() >= tabs::MAX_TABS {
            return self.notify(format!("{} calls are open, the most there can be: hang one up first", tabs::MAX_TABS));
        }
        self.dialer_open = false;
        self.new_tab_dial = false;
        let tab = self.start_call(entry, index);
        let t = match replace {
            Some(t) => {
                self.stop_script(t, None);
                // The call it had hangs up.
                self.tabs[t] = tab;
                t
            }
            None => {
                self.tabs.push(tab);
                self.tabs.len() - 1
            }
        };
        self.show_tab(t);
    }

    /// Put tab `t` on screen.
    pub(super) fn show_tab(&mut self, t: usize) {
        let Some(tab) = self.tabs.get_mut(t) else { return };
        tab.unseen = false;
        tab.attention = false;
        let title = tab.title.clone();
        self.active = t;
        // Music and a selection belong to the call that was showing.
        self.player.stop();
        self.selection = None;
        self.dragging = false;
        self.mouse_held = None;
        self.mouse_reported = None;
        self.set_title(&title);
    }

    /// Hang up tab `t`'s call, if it is still on, and close the tab.
    pub(super) fn close_tab(&mut self, t: usize) {
        if t >= self.tabs.len() {
            return;
        }
        self.stop_script(t, None);
        let len = self.tabs.len();
        let on_screen = t == self.active;
        self.tabs.remove(t);
        if self.tabs.is_empty() {
            self.active = 0;
            self.player.stop();
            self.selection = None;
            return self.set_title("BadTerm");
        }
        let next = tabs::after_close(self.active, t, len);
        if on_screen {
            self.show_tab(next);
        } else {
            self.active = next;
        }
    }

    /// What a call off screen has to say, in the status bar, with which tab
    /// it is in.
    pub(super) fn notify_from(&mut self, t: usize, msg: impl Into<String>) {
        let msg = msg.into();
        match self.tabs.get(t).filter(|_| t != self.active) {
            Some(tab) => {
                let name = truncate(&tab.session.entry.name, 24);
                self.notify(format!("{name} (tab {}): {msg}", t + 1));
            }
            None => self.notify(msg),
        }
    }

    /// A call off screen wants the user: its chip says so, and the status
    /// bar says what for and how to get there.
    fn call_for(&mut self, t: usize, what: &str) {
        let Some(tab) = self.tabs.get_mut(t).filter(|_| t != self.active) else { return };
        tab.attention = true;
        let name = truncate(&tab.session.entry.name, 24);
        self.notify(format!("{name} {what}: tab {} ({} switches)", t + 1, crate::hotkeys::label("T")));
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

    /// Dial from the directory (or a link): straight away if no call is on
    /// screen, or one has ended (its tab is used again); in a new tab when
    /// the dialer was opened for one (Alt-N); otherwise after asking
    /// whether to call in a new tab or hang up the call on screen.
    pub(super) fn dial(&mut self, mut entry: Entry, index: Option<usize>) {
        // An address not in the directory gets the settings' font and
        // screen mode, as a new entry would.
        if index.is_none() {
            self.new_entry_defaults(&mut entry);
        }
        if self.new_tab_dial {
            return self.open_call(entry, index, Slot::New);
        }
        match self.session().filter(|s| s.state != State::Closed) {
            Some(s) => {
                let current = s.entry.name.clone();
                let room = self.tabs.len() < tabs::MAX_TABS;
                self.dialer.confirm_hang_up(&current, &entry.name, room);
                self.pending_dial = Some((entry, index));
            }
            None => self.open_call(entry, index, Slot::Replace(self.active)),
        }
    }

    /// Hang up the call on screen, and close its tab.
    pub(super) fn hang_up(&mut self) {
        self.close_tab(self.active);
    }

    pub(super) fn on_net(&mut self, id: u64, ev: NetEvent) {
        let Some(t) = self.tab_of(id) else {
            // A connection that finished after its tab was closed.
            if let NetEvent::Connected(writer) = ev {
                writer.shutdown();
            }
            return;
        };
        let on_screen = t == self.active;
        let connected = matches!(ev, NetEvent::Connected(_));
        let closed = matches!(ev, NetEvent::Closed(_));
        let data = matches!(ev, NetEvent::Data(_));
        // New text moves what a selection was over.
        if data && on_screen && !self.dragging && self.tabs[t].scroll == 0 {
            self.selection = None;
        }
        let tab = &mut self.tabs[t];
        tab.session.handle(ev);
        if data && !on_screen {
            tab.unseen = true;
        }
        // The question went away (the connection closed).
        if tab.session.auth_prompt().is_none() && tab.login.as_ref().is_some_and(|l| !l.rip) {
            tab.login = None;
        }
        if connected && let Some(e) = self.entry_mut(t) {
            e.calls += 1;
            e.last_connected = Some(dialer::now());
            self.save_directory();
        }
        self.feed_script(t);
        if closed {
            let tab = &mut self.tabs[t];
            if tab.login.as_ref().is_some_and(|l| l.rip) {
                tab.login = None;
            }
            let was_on = tab.session.was_connected();
            self.stop_script(t, Some("stopped, as the call ended"));
            self.schedule_redial(t);
            // Off screen, the end of a call is news (a busy one that will
            // be redialed is not yet).
            if self.tabs[t].redial.is_none() {
                self.call_for(t, if was_on { "disconnected" } else { "could not connect" });
            }
        }
        self.session_events(t);
        self.request_redraw();
    }

    /// Dial a busy or unreachable BBS again after a while, a few times.
    pub(super) fn schedule_redial(&mut self, t: usize) {
        let Some(tab) = self.tabs.get(t).filter(|tab| tab.session.should_redial()) else { return };
        if !self.settings.redial || tab.redials >= MAX_REDIALS {
            return;
        }
        let (entry, attempt) = (tab.session.entry.clone(), tab.redials + 1);
        let index = self.entry_index(t);
        self.tabs[t].redial = Some(Redial { entry, index, attempt, at: Instant::now() + REDIAL_DELAY });
    }

    /// Make tab `t`'s waiting redial now, in the same tab.
    pub(super) fn redial_now(&mut self, t: usize) {
        let Some(r) = self.tabs.get_mut(t).and_then(|tab| tab.redial.take()) else { return };
        let mut tab = self.start_call(r.entry, r.index);
        tab.redials = r.attempt;
        // Off screen, it stays there (with its mark) until it is wanted.
        tab.unseen = self.tabs[t].unseen;
        tab.attention = self.tabs[t].attention;
        self.tabs[t] = tab;
        if t == self.active {
            self.show_tab(t);
        }
    }

    /// What tab `t`'s call has to say. On screen it may take the screen
    /// (a login, an upload); off screen it marks the tab and says so.
    pub(super) fn session_events(&mut self, t: usize) {
        let Some(tab) = self.tabs.get_mut(t) else { return };
        let on_screen = t == self.active;
        for ev in tab.session.take_events() {
            match ev {
                SessionEvent::Term(TermEvent::Bell) => self.bell(t),
                SessionEvent::Term(TermEvent::RipQuestion(q)) => {
                    let tab = &mut self.tabs[t];
                    tab.login = Some(LoginPrompt::rip_question(&q, &tab.session.entry.name));
                    tab.scroll = 0;
                    if on_screen {
                        self.dialer_open = false;
                    }
                    self.call_for(t, "asks a question");
                }
                SessionEvent::Term(TermEvent::Title(title)) => {
                    let title = format!("BadTerm \u{2014} {title}");
                    if on_screen {
                        self.set_title(&title);
                    }
                    self.tabs[t].title = title;
                }
                // Only the call on screen plays music.
                SessionEvent::Term(TermEvent::Music(tones)) => {
                    if on_screen && self.settings.music != Music::Off {
                        self.player.play(tones);
                    }
                }
                // SyncTERM's audio; Alt-M's "off" silences it too.
                SessionEvent::Term(TermEvent::Audio(op)) => {
                    if on_screen && self.settings.music != Music::Off {
                        self.player.audio(op);
                    }
                }
                SessionEvent::Notice(msg) => self.notify_from(t, msg),
                SessionEvent::Connected => {
                    // Got through after redialing: worth a look.
                    if self.tabs[t].redials > 0 {
                        self.attention();
                        self.call_for(t, "answered");
                    }
                    let s = &self.tabs[t].session;
                    if s.entry.capture && s.capture_path().is_none() {
                        self.toggle_capture(t);
                    }
                    // The entry's login script.
                    let script = self.tabs[t].session.entry.script.clone();
                    if !script.trim().is_empty() {
                        let path = self.script_path(&script);
                        self.run_script(t, &path);
                    }
                    if let Some(pw) = self.tabs[t].remember.take()
                        && let Some(e) = self.entry_mut(t)
                    {
                        e.password = pw;
                        self.save_directory();
                    }
                }
                SessionEvent::AuthPrompt => {
                    let tab = &mut self.tabs[t];
                    let Some(p) = tab.session.auth_prompt() else { continue };
                    tab.login = Some(LoginPrompt::new(p, tab.index.is_some(), self.keychain_ok));
                    tab.scroll = 0;
                    if on_screen {
                        self.dialer_open = false;
                    }
                    self.call_for(t, "asks you to log in");
                    self.attention();
                }
                // Both need the terminal view (the prompt and progress window
                // live there), so they close the dialer if it is open.
                SessionEvent::UploadRequested => {
                    self.tabs[t].scroll = 0;
                    self.open_upload(t, true);
                    if on_screen {
                        self.dialer_open = false;
                    }
                    self.call_for(t, "is waiting for an upload");
                    self.attention();
                }
                SessionEvent::TransferStarted => {
                    if on_screen {
                        self.dialer_open = false;
                    }
                    let tab = &mut self.tabs[t];
                    tab.scroll = 0;
                    tab.upload_prompt = None;
                    tab.download_prompt = None;
                    tab.transfer_direction = tab.session.transfer().map(|x| x.progress().direction);
                }
                SessionEvent::TransferFinished(outcome) => {
                    let msg = self.describe_outcome(t, &outcome);
                    self.tabs[t].transfer_direction = None;
                    if on_screen {
                        self.notify(msg);
                    } else {
                        self.tabs[t].attention = true;
                        self.notify_from(t, msg);
                    }
                    self.attention();
                }
            }
        }
    }

    pub(super) fn describe_outcome(&self, t: usize, outcome: &Outcome) -> String {
        let tab = self.tabs.get(t);
        match (outcome, tab.and_then(|tab| tab.transfer_direction)) {
            (Outcome::Completed(files), Some(Direction::Download)) => match files.as_slice() {
                [] => "Download finished; no files were sent".into(),
                [one] => format!("Downloaded {}", one.display()),
                many => {
                    let dir = tab.map_or_else(
                        || self.folder(Folders::DOWNLOADS),
                        |tab| tab.session.download_dir().to_path_buf(),
                    );
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

    /// Upload, in the call on screen.
    pub(super) fn start_xy_upload(&mut self, protocol: badterm_core::transfer::XyProtocol, paths: Vec<PathBuf>) {
        let Some(session) = self.tab_mut().map(|t| &mut t.session) else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_xy_upload(protocol, paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events(self.active);
    }

    /// Upload, in the call on screen.
    pub(super) fn start_kermit_upload(&mut self, paths: Vec<PathBuf>) {
        let Some(session) = self.tab_mut().map(|t| &mut t.session) else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_kermit_upload(paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events(self.active);
    }

    /// Upload, in the call on screen.
    pub(super) fn start_upload(&mut self, paths: Vec<PathBuf>) {
        let Some(session) = self.tab_mut().map(|t| &mut t.session) else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_upload(paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events(self.active);
    }

    /// Where the entry tab `t`'s call was dialed from is in the directory
    /// now: editing the directory can move it.
    pub(super) fn entry_index(&self, t: usize) -> Option<usize> {
        let tab = self.tabs.get(t)?;
        let id = tab.session.entry.system_id();
        let entries = &self.directory.entries;
        let i = tab.index?;
        if entries.get(i).is_some_and(|e| e.system_id() == id) {
            Some(i)
        } else {
            entries.iter().position(|e| e.system_id() == id)
        }
    }

    pub(super) fn entry_mut(&mut self, t: usize) -> Option<&mut Entry> {
        let i = self.entry_index(t)?;
        self.tabs[t].index = Some(i);
        self.directory.entries.get_mut(i)
    }

    /// The phosphor mode the system on screen asks for, as it is in the
    /// directory (so edits made while connected show at once).
    pub(super) fn own_crt_mode(&self) -> Option<CrtMode> {
        match self.entry_index(self.active) {
            Some(i) => self.directory.entries[i].crt,
            None => self.session().and_then(|s| s.entry.crt),
        }
    }

    pub(super) fn crt_mode(&self) -> CrtMode {
        self.own_crt_mode().unwrap_or(self.settings.crt)
    }

    /// What the bell does on tab `t`'s system: its own setting, or the
    /// global one.
    pub(super) fn bell_mode(&self, t: usize) -> BellMode {
        let own = match self.entry_index(t) {
            Some(i) => self.directory.entries[i].bell,
            None => self.tabs.get(t).and_then(|tab| tab.session.entry.bell),
        };
        own.unwrap_or(self.settings.bell)
    }

    /// Ring tab `t`'s bell: a beep, a flash, or both (at most every so
    /// often, as some BBSes ring it in bursts), and a nudge on the taskbar
    /// when the window is in the background. Off screen, it beeps and marks
    /// the tab, but doesn't flash the call on screen.
    pub(super) fn bell(&mut self, t: usize) {
        if let Some(g) = self.gfx.as_ref().filter(|g| !g.window.has_focus()) {
            g.window.request_user_attention(Some(UserAttentionType::Informational));
        }
        let on_screen = t == self.active;
        if let Some(tab) = self.tabs.get_mut(t).filter(|_| !on_screen) {
            tab.attention = true;
        }
        let now = Instant::now();
        if self.last_bell.is_some_and(|t| now.duration_since(t) < BELL_GAP) {
            return;
        }
        self.last_bell = Some(now);
        let mode = self.bell_mode(t);
        if mode.sound() {
            self.player.play(vec![badterm_core::music::Tone { hz: 880, ms: 120 }]);
        }
        if mode.visual() && on_screen {
            self.flash_until = Some(now + FLASH);
            self.request_redraw();
        }
    }

    /// Send keyboard macro `i` (Alt-1 is 0) to the call on screen, pausing
    /// where it says.
    pub(super) fn start_macro(&mut self, i: usize) {
        let Some(session) = self.session().filter(|s| s.state == State::Connected) else {
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
        let steps = crate::macros::expand(&text, &e.username, &e.password).into();
        let t = self.active;
        self.tabs[t].macro_steps = steps;
        self.tabs[t].macro_at = Instant::now();
        self.run_macro(t);
    }

    /// Send what tab `t`'s macro has ready, up to its next pause.
    pub(super) fn run_macro(&mut self, t: usize) {
        let Some(tab) = self.tabs.get_mut(t) else { return };
        if tab.macro_steps.is_empty() || Instant::now() < tab.macro_at {
            return;
        }
        if tab.session.state != State::Connected {
            tab.macro_steps.clear();
            return;
        }
        while let Some(step) = tab.macro_steps.pop_front() {
            match step {
                crate::macros::Step::Send(text) => {
                    let bytes = tab.session.term.encoding.encode_text(&text);
                    tab.session.send(&bytes);
                }
                crate::macros::Step::Pause => {
                    tab.macro_at = Instant::now() + crate::macros::PAUSE;
                    return;
                }
            }
        }
    }

    /// Start or stop capturing tab `t`'s call to a file.
    pub(super) fn toggle_capture(&mut self, t: usize) {
        let Some(session) = self.tabs.get_mut(t).map(|tab| &mut tab.session) else { return };
        if session.capture_path().is_some() {
            session.stop_capture();
            return self.notify_from(t, "Capture stopped");
        }
        let entry = session.entry.clone();
        let path = capture_file(&entry, &self.folder(Folders::CAPTURES), dialer::now());
        let msg = match self.tabs[t].session.start_capture(path.clone()) {
            Ok(()) => format!("Capturing to {}", path.display()),
            Err(e) => format!("Capture failed: {e:#}"),
        };
        self.notify_from(t, msg);
    }

    pub(super) fn paste(&mut self) {
        let Some(session) = self.tab_mut().map(|t| &mut t.session) else { return };
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
        let Some(tab) = self.tabs.get(self.active) else { anyhow::bail!("not connected") };
        let session = &tab.session;
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
                let rows = terminal_rows(session, tab.scroll);
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
