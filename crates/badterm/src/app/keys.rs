//! The keyboard: BadTerm's hotkeys, and keys for whichever screen or
//! dialog is in front.

use super::*;

impl App {
    pub(super) fn on_key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        // Any key closes the splash screen (and does nothing else).
        if self.splash_until.take().is_some() {
            return self.request_redraw();
        }
        let mods = Mods::from(self.mods);
        if mods.hotkey()
            && let PhysicalKey::Code(code) = event.physical_key
            && self.hotkey(event_loop, code)
        {
            return self.request_redraw();
        }
        let Some(key) = input::translate(event) else { return };
        self.selection = None;
        if self.confirm_exit {
            match key {
                Key::Char('y') | Key::Char('Y') | Key::Enter => event_loop.exit(),
                Key::Char('n') | Key::Char('N') | Key::Escape => self.confirm_exit = false,
                _ => {}
            }
            return self.request_redraw();
        }
        if self.help_open {
            self.help_open = false;
        } else if self.settings_open {
            self.settings_key(key, mods);
        } else if self.art_picker.is_some() {
            self.art_picker_key(key, mods);
        } else if let Some(v) = self.art.as_mut() {
            match v.handle_key(key) {
                ArtAction::None => {}
                ArtAction::Close => self.art = None,
                ArtAction::Open => self.open_art_picker(),
            }
        } else if self.dialer_open {
            self.dialer_key(key, mods);
        } else if self.login.is_some() {
            self.login_key(key, mods);
        } else if self.save_prompt.is_some() {
            self.save_key(key, mods);
        } else if self.upload_prompt.is_some() {
            self.prompt_key(key, mods);
        } else if self.download_prompt.is_some() {
            self.download_key(key, mods);
        } else if self.session.as_ref().is_some_and(|s| s.transfer().is_some()) {
            if key == Key::Escape
                && let Some(s) = self.session.as_mut()
            {
                s.cancel_transfer();
                self.session_events();
            }
        } else if self.session.is_some() {
            self.terminal_key(key, mods);
        }
        // With no session and the dialer closed (the idle screen), only the
        // hotkeys do anything.
        self.request_redraw();
    }

    pub(super) fn dialer_key(&mut self, key: Key, mods: Mods) {
        match self.dialer.handle_key(key, mods, &mut self.directory) {
            Action::None => {}
            Action::Connect(i) => {
                let e = self.directory.entries[i].clone();
                self.dial(e, Some(i));
            }
            Action::QuickConnect(e) => self.dial(*e, None),
            Action::Changed => self.save_directory(),
            Action::HangUpAndDial => {
                if let Some((e, i)) = self.pending_dial.take() {
                    self.hang_up();
                    self.connect(e, i);
                }
            }
            Action::Settings => self.open_settings(),
            Action::Close => {
                self.dialer_open = false;
                self.pending_dial = None;
            }
        }
    }

    /// Client hotkeys (Alt, or Cmd on macOS). Returns true if handled.
    pub(super) fn hotkey(&mut self, event_loop: &ActiveEventLoop, code: KeyCode) -> bool {
        // While "Exit BadTerm?" shows, only Alt-X (yes) does anything.
        if self.confirm_exit && code != KeyCode::KeyX {
            return true;
        }
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some(g) = &self.gfx {
                    let fs = g.window.fullscreen().is_none().then_some(Fullscreen::Borderless(None));
                    g.window.set_fullscreen(fs);
                }
                return true;
            }
            // Asks first; Alt-X again (or Y) is yes.
            KeyCode::KeyX => {
                if self.confirm_exit {
                    event_loop.exit();
                }
                self.confirm_exit = true;
                return true;
            }
            // The stored login: the user name, then the password.
            KeyCode::KeyL => {
                let msg = match self.session.as_mut().filter(|s| s.state == State::Connected) {
                    None => Some("Not connected"),
                    Some(s) if !s.has_login() => Some("This entry has no user name or password"),
                    // The session reports what it sent.
                    Some(s) => (!s.send_login()).then_some("The user name and password were sent"),
                };
                if let Some(m) = msg {
                    self.notify(m);
                }
                self.session_events();
                return true;
            }
            // Display settings, from anywhere.
            KeyCode::KeyP => {
                // A system with its own phosphor mode keeps the change.
                if let Some(mode) = self.own_crt_mode() {
                    let next = mode.next();
                    if let Some(s) = self.session.as_mut() {
                        s.entry.crt = Some(next);
                    }
                    if let Some(e) = self.session_entry_mut() {
                        e.crt = Some(next);
                        self.save_directory();
                    }
                    let name = self.session.as_ref().map_or("", |s| s.entry.name.as_str());
                    self.notify(format!("Phosphor mode for {name}: {}", next.name()));
                } else {
                    self.settings.crt = self.settings.crt.next();
                    let msg = format!("Phosphor mode: {}", self.settings.crt.name());
                    self.settings_changed(msg);
                }
                return true;
            }
            KeyCode::KeyA => {
                self.settings.aspect = self.settings.aspect.next();
                let msg = format!("Shape: {}", self.settings.aspect.name());
                self.settings_changed(msg);
                return true;
            }
            KeyCode::KeyM => {
                self.settings.music = self.settings.music.next();
                if self.settings.music == Music::Off {
                    self.player.stop();
                }
                if let Some(s) = self.session.as_mut() {
                    s.term.music_on_csi_m = self.settings.music == Music::All;
                    s.term.music_enabled = self.settings.music != Music::Off;
                }
                let msg = self.settings.music.name();
                self.settings_changed(msg);
                return true;
            }
            KeyCode::KeyW => {
                self.settings.nine_dot = !self.settings.nine_dot;
                let msg = if self.settings.nine_dot { "9-dot characters, as on a VGA" } else { "8-dot characters" };
                self.settings_changed(msg);
                return true;
            }
            // The dialing directory, from anywhere; the session (if any)
            // stays connected behind it.
            KeyCode::KeyD => {
                self.dialer_open = !self.dialer_open;
                self.pending_dial = None;
                self.settings_open = false;
                self.help_open = false;
                return true;
            }
            // The ANSI viewer, from anywhere.
            KeyCode::KeyF => {
                if self.art.is_some() || self.art_picker.is_some() {
                    self.art = None;
                    self.art_picker = None;
                } else {
                    self.open_art_picker();
                }
                return true;
            }
            KeyCode::KeyO => {
                if self.settings_open {
                    self.settings_open = false;
                } else {
                    self.open_settings();
                }
                return true;
            }
            // Help, as in Telix.
            KeyCode::KeyZ => {
                self.help_open = !self.help_open;
                return true;
            }
            _ => {}
        }
        // Session hotkeys act on what is on screen, so they are swallowed
        // while the dialer (or settings, or help) covers the session.
        if self.dialer_open || self.settings_open || self.help_open || self.art.is_some() || self.art_picker.is_some() {
            return true;
        }
        if let Some(i) = macro_index(code) {
            self.start_macro(i);
            return true;
        }
        let Some(session) = self.session.as_mut() else { return false };
        match code {
            KeyCode::KeyH => self.hang_up(),
            // Call again: now, if a redial is waiting, or after a hang-up.
            KeyCode::KeyR if session.state == State::Closed => {
                if self.redial.is_none() {
                    let (entry, index) = (session.entry.clone(), self.session_entry_index());
                    self.redial = Some(Redial { entry, index, attempt: 0, at: Instant::now() });
                }
                self.redial_now();
            }
            KeyCode::KeyB => {
                let page = session.term.screen.rows();
                self.scroll = self.scroll.max(1).max(page.min(session.term.screen.scrollback_len()));
            }
            // A teletext page's concealed text (Viewdata, Mode 7).
            KeyCode::KeyK => {
                match session.term.toggle_reveal() {
                    Some(true) => self.notify("Concealed text shown"),
                    Some(false) => self.notify("Concealed text hidden"),
                    None => self.notify("Alt-K reveals concealed text on Viewdata and Mode 7 pages"),
                }
                self.request_redraw();
            }
            KeyCode::KeyI => {
                let s = &mut session.term.screen;
                s.ice_colors = !s.ice_colors;
                let on = s.ice_colors;
                let mut msg = if on { "iCE colors on" } else { "iCE colors off (blinking)" }.to_string();
                // A BBS with its own setting keeps the change.
                if session.entry.ice.is_some() {
                    session.entry.ice = Some(on);
                    session.term.screen.base_ice = on;
                    if let Some(e) = self.session_entry_mut() {
                        e.ice = Some(on);
                        msg = format!("{msg} for {}", e.name);
                        self.save_directory();
                    }
                }
                self.notify(msg);
            }
            KeyCode::KeyE => {
                let t = &mut session.term;
                // Between the PC's two; a C64 or Atari system stays as it is.
                t.encoding = match t.encoding {
                    Encoding::Cp437 => Encoding::Utf8,
                    Encoding::Utf8 => Encoding::Cp437,
                    e => e,
                };
                let msg = format!("Encoding: {}", t.encoding.name());
                self.notify(msg);
            }
            KeyCode::KeyC => self.toggle_capture(),
            KeyCode::KeyS => {
                let now = dialer::now();
                let name = format!("{}-{}.ans", file_stem(&session.entry.name), stamp(now));
                let path = self.folder(Folders::SAVES).join(name);
                self.upload_prompt = None;
                self.save_prompt = Some(SavePrompt::new(&path));
            }
            KeyCode::KeyV => self.paste(),
            // During an upload, the prompt adds files to the batch.
            KeyCode::KeyU
                if session.state == State::Connected
                    && session.transfer().is_none_or(|t| t.progress().direction == Direction::Upload) =>
            {
                let requested = session.upload_requested();
                self.open_upload(requested);
            }
            KeyCode::KeyG if session.state == State::Connected && session.transfer().is_none() => {
                self.upload_prompt = None;
                self.download_prompt = Some(DownloadPrompt::new(self.download_protocol));
            }
            _ => return false,
        }
        true
    }

    pub(super) fn login_key(&mut self, key: Key, mods: Mods) {
        let Some(login) = self.login.as_mut() else { return };
        let answer = match login.handle_key(key, mods) {
            LoginAction::None => return,
            LoginAction::Answer(a) => {
                // A later question (say, a one-time code) leaves the
                // password's choice alone.
                if login.can_remember {
                    self.remember = login.remember.then(|| a.clone());
                }
                Some(a)
            }
            LoginAction::Cancel => None,
        };
        self.login = None;
        if let Some(s) = self.session.as_mut() {
            s.answer_prompt(answer);
        }
    }

    pub(super) fn terminal_key(&mut self, key: Key, mods: Mods) {
        let Some(session) = self.session.as_mut() else { return };
        // A finished call stays on screen to be read (and scrolled back)
        // until Alt-H clears it or another call is dialed.
        let max = session.term.screen.scrollback_len();
        if self.scroll > 0 {
            self.scroll = match key {
                Key::Up => self.scroll + 1,
                Key::Down => self.scroll - 1,
                Key::PageUp => self.scroll + session.term.screen.rows(),
                Key::PageDown => self.scroll.saturating_sub(session.term.screen.rows()),
                Key::Home => max,
                _ => 0,
            }
            .min(max);
            return;
        }
        match key {
            Key::PageUp if mods.shift => self.scroll = session.term.screen.rows().min(max),
            Key::Escape if self.redial.is_some() => {
                self.redial = None;
                self.notify("Redial cancelled");
            }
            _ if session.state == State::Closed => {}
            Key::Insert if mods.shift => self.paste(),
            _ => {
                let bytes = input::encode(key, mods, session.term.encoding, session.term.codepage());
                if !bytes.is_empty() {
                    session.term.interrupt();
                    session.send(&bytes);
                }
            }
        }
    }

    pub(super) fn art_picker_key(&mut self, key: Key, mods: Mods) {
        let Some(picker) = self.art_picker.as_mut() else { return };
        if key == Key::Escape {
            self.art_picker = None;
            return;
        }
        if let PickerAction::Chosen(paths) = picker.handle_key(key, mods) {
            self.art_dir = picker.dir().to_path_buf();
            self.art_picker = None;
            if let Some(path) = paths.first() {
                self.view_art(path);
            }
        }
    }

    pub(super) fn save_key(&mut self, key: Key, mods: Mods) {
        let Some(prompt) = self.save_prompt.as_mut() else { return };
        match key {
            Key::Escape => self.save_prompt = None,
            Key::Enter => {
                let path = prompt.path();
                self.save_prompt = None;
                let msg = match self.save(&path) {
                    Ok(what) => format!("Saved {what} to {}", path.display()),
                    Err(e) => format!("Save failed: {e:#}"),
                };
                self.notify(msg);
            }
            _ => {
                prompt.field.handle_key(key, mods);
            }
        }
    }

    pub(super) fn prompt_key(&mut self, key: Key, mods: Mods) {
        let Some(prompt) = self.upload_prompt.as_mut() else { return };
        match prompt.handle_key(key, mods) {
            UploadAction::None => {}
            UploadAction::Cancel => {
                self.upload_prompt = None;
                // A BBS waiting for the upload is told to give up.
                if let Some(s) = self.session.as_mut() {
                    s.decline_upload();
                }
            }
            UploadAction::Send(protocol, paths) => {
                self.upload_dir = prompt.picker.dir().to_path_buf();
                if !prompt.requested {
                    self.upload_protocol = prompt.protocol;
                }
                self.upload_prompt = None;
                match protocol {
                    Proto::Zmodem => self.start_upload(paths),
                    Proto::Xy(p) => self.start_xy_upload(p, paths),
                    Proto::Kermit => self.start_kermit_upload(paths),
                }
            }
        }
    }

    pub(super) fn download_key(&mut self, key: Key, mods: Mods) {
        let Some(prompt) = self.download_prompt.as_mut() else { return };
        match prompt.handle_key(key, mods) {
            DownloadAction::None => {}
            DownloadAction::Cancel => self.download_prompt = None,
            DownloadAction::Receive(protocol, name) => {
                self.download_protocol = prompt.protocol;
                self.download_prompt = None;
                let Some(session) = self.session.as_mut() else { return };
                let started = match protocol {
                    Proto::Xy(p) => session.start_xy_download(p, name),
                    Proto::Kermit | Proto::Zmodem => session.start_kermit_download(),
                };
                if let Err(e) = started {
                    self.notify(format!("Download failed: {e:#}"));
                }
                self.session_events();
            }
        }
    }

    pub(super) fn settings_key(&mut self, key: Key, mods: Mods) {
        match self.settings_ui.handle_key(key, mods, &mut self.settings) {
            settings_ui::Action::None => {}
            settings_ui::Action::Changed => self.apply_settings(),
            settings_ui::Action::Close => self.settings_open = false,
            settings_ui::Action::Import(path) => {
                let msg = match self.import_theme(&path) {
                    Ok((name, dest)) => format!(
                        "Imported \"{name}\" and switched to it. It is kept as {}, and is in the color scheme list.",
                        dest.display()
                    ),
                    Err(e) => format!("Could not import the theme: {e:#}"),
                };
                self.settings_ui.show_message(msg);
            }
            settings_ui::Action::Export(name) => {
                let file = ThemeFile { name: name.clone(), author: String::new(), colors: self.settings.colors };
                let dest = self.paths.themes.join(theme::file_name(&name));
                let msg = match file.save(&dest) {
                    Ok(()) => format!(
                        "Saved \"{name}\" as {}. Share that file: anyone can bring it in with Import a theme.",
                        dest.display()
                    ),
                    Err(e) => format!("Could not save the theme: {e:#}"),
                };
                self.settings_ui.set_themes(theme::user_themes(&self.paths.themes));
                self.settings_ui.show_message(msg);
            }
        }
    }

    pub(super) fn open_upload(&mut self, requested: bool) {
        self.download_prompt = None;
        if self.upload_dir.as_os_str().is_empty() {
            self.upload_dir = match &self.session {
                Some(s) => self.entry_folder(&s.entry, Folders::UPLOADS),
                None => self.folder(Folders::UPLOADS),
            };
        }
        self.upload_prompt = Some(UploadPrompt::new(requested, self.upload_dir.clone(), self.upload_protocol));
    }
}
