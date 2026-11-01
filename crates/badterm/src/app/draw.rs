//! Drawing: the status bar, and each frame from whatever is on screen.

use super::*;

impl App {
    /// The font for a screen this many rows high: 8x8 for 50 lines.
    /// The font for a screen this many rows high: 8x8 from 43 lines, as on
    /// an EGA or VGA.
    pub(super) fn font_for(rows: usize) -> Font {
        if rows >= 43 { Font::vga_8x8() } else { Font::vga_8x16() }
    }

    /// The width of what is on screen: the session's (80 or 132 columns),
    /// or 80 for the dialer and the idle screen.
    pub(super) fn view_cols(&self) -> usize {
        if let Some(v) =
            self.art.as_ref().filter(|_| !self.help_open && !self.settings_open && self.art_picker.is_none())
        {
            return v.cols();
        }
        match &self.session {
            Some(s) if !self.dialer_open && !self.settings_open && !self.help_open => s.term.screen.cols(),
            _ => COLS,
        }
    }

    pub(super) fn cell_width(&self) -> usize {
        if self.settings.nine_dot { 9 } else { 8 }
    }

    pub(super) fn draw_status(&mut self) {
        let theme = ui::theme::current();
        let (fg, bg) = (theme.status.fg(), theme.status.bg());
        let cols = self.view_cols();
        if self.status.cols() != cols {
            self.status = Screen::new(cols, 1, 0);
        }
        let s = &mut self.status;
        s.fill_rect(0, 0, cols, 1, Cell::new(b' ', fg, bg));
        if let Some(v) =
            self.art.as_ref().filter(|_| !self.help_open && !self.settings_open && self.art_picker.is_none())
        {
            return ui::text(s, 0, 0, cols, &v.status(), fg, bg);
        }
        if self.art_picker.is_some() && !self.help_open && !self.settings_open {
            return ui::text(s, 0, 0, cols, " View ANSI Art \u{2502} Esc returns", fg, bg);
        }
        if self.help_open || self.settings_open {
            let line = if self.help_open {
                " Help \u{2502} Any key returns"
            } else {
                " Settings \u{2502} Changes are saved as you make them \u{2502} Esc returns"
            };
            return ui::text(s, 0, 0, cols, line, fg, bg);
        }
        let notice = self.notice.as_ref().filter(|(_, until)| Instant::now() < *until).map(|(m, _)| m.clone());
        if self.dialer_open {
            let help = notice.unwrap_or_else(|| self.dialer.status_help().to_string());
            return ui::text(s, 0, 0, cols, &format!(" {help}"), fg, bg);
        }
        let Some(session) = &self.session else {
            let help =
                notice.unwrap_or_else(|| format!("{HOTKEY}-D Dialing directory  {HOTKEY}-Z Help  {HOTKEY}-X Exit"));
            return ui::text(s, 0, 0, cols, &format!(" Not connected \u{2502} {help}"), fg, bg);
        };
        let screen = &session.term.screen;
        if self.scroll > 0 {
            let line = format!(
                " SCROLLBACK {}/{}  \u{2191}\u{2193} PgUp PgDn Home  Any other key returns",
                self.scroll,
                screen.scrollback_len()
            );
            return ui::text(s, 0, 0, cols, &line, theme.status_alt.fg(), theme.status_alt.bg());
        }
        let e = &session.entry;
        if let Some(t) = session.transfer() {
            let line =
                format!(" {} \u{2502} {} transfer in progress \u{2502} Esc cancels", truncate(&e.name, 30), t.name());
            return ui::text(s, 0, 0, cols, &line, fg, bg);
        }
        if self.login.is_some() {
            let line = format!(" Logging in to {} \u{2502} Enter log in  Esc cancel", truncate(&e.name, 40));
            return ui::text(s, 0, 0, cols, &line, fg, bg);
        }
        if let Some(r) = self.redial.as_ref().filter(|_| session.state == State::Closed) {
            let why = if session.error.is_some() { "No connection" } else { "Busy" };
            let secs = r.at.saturating_duration_since(Instant::now()).as_secs() + 1;
            let line = format!(
                " {why} \u{2502} Calling again in {secs}s (redial {} of {MAX_REDIALS}) \u{2502} Esc cancel  {HOTKEY}-R now",
                r.attempt
            );
            return ui::text(s, 0, 0, cols, &line, fg, bg);
        }
        let line = match session.state {
            State::Connecting if self.redials > 0 => {
                format!(" Redialing {} (redial {} of {MAX_REDIALS})...  {HOTKEY}-H cancel", e.address(), self.redials)
            }
            State::Connecting => format!(" Connecting to {}...  {HOTKEY}-H cancel", e.address()),
            State::Closed => match &session.error {
                Some(err) => format!(" Connection failed: {err} \u{2502} {HOTKEY}-R Call again"),
                None => {
                    format!(" Disconnected \u{2502} {HOTKEY}: R Call again  D Directory  H Clear  B Back")
                }
            },
            State::Connected => {
                let t = session.elapsed().as_secs();
                let mut flags = String::new();
                if screen.ice_colors {
                    flags.push_str(" iCE");
                }
                if session.capture_path().is_some() {
                    flags.push_str(" CAP");
                }
                if e.baud > 0 {
                    flags.push_str(&format!(" {}bps", e.baud));
                }
                let right = notice.unwrap_or_else(|| format!("{HOTKEY}: D Dir H Hangup U Upload Z Help X Exit"));
                format!(
                    " {} \u{2502} {:02}:{:02}:{:02}{} \u{2502} {right}",
                    truncate(&e.name, 16),
                    t / 3600,
                    t / 60 % 60,
                    t % 60,
                    flags
                )
            }
        };
        ui::text(s, 0, 0, cols, &line, fg, bg);
    }

    pub(super) fn redraw(&mut self) {
        self.draw_status();
        let dialer_cursor =
            if self.dialer_open { self.dialer.draw(&mut self.dialer_screen, &self.directory) } else { None };
        let mut page_cursor = None;
        let splash = self.splash_until.is_some();
        if splash {
            ui::splash::draw(&mut self.page_screen);
        } else if self.help_open {
            ui::help::draw(&mut self.page_screen);
        } else if self.settings_open {
            let own = self.own_crt_mode().map(|m| m.name());
            page_cursor = self.settings_ui.draw(&mut self.page_screen, &self.settings, own, self.update.as_deref());
        } else if let Some(picker) = self.art_picker.as_mut() {
            page_cursor = art_ui::draw_picker(&mut self.page_screen, picker);
        }
        let crt = self.crt_mode();
        let Some(gfx) = self.gfx.as_mut() else { return };
        let size = gfx.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };

        // Dialogs over the terminal are drawn on a copy of its screen.
        let mut overlay_cursor = None;
        if let Some(session) = &self.session {
            let sc = &session.term.screen;
            if (self.overlay.cols(), self.overlay.rows()) != (sc.cols(), sc.rows()) {
                self.overlay = Screen::new(sc.cols(), sc.rows(), 0);
            }
        }
        let overlay = match &self.session {
            Some(session)
                if self.login.is_some()
                    || self.save_prompt.is_some()
                    || self.upload_prompt.is_some()
                    || self.download_prompt.is_some()
                    || session.transfer().is_some() =>
            {
                let sc = &session.term.screen;
                for y in 0..sc.rows() {
                    for (x, cell) in sc.row(y).iter().enumerate() {
                        if let Some(c) = self.overlay.cell_mut(x, y) {
                            *c = *cell;
                        }
                    }
                }
                if let Some(login) = &self.login {
                    overlay_cursor = Some(login.draw(&mut self.overlay));
                } else if let Some(prompt) = &self.save_prompt {
                    overlay_cursor = Some(prompt.draw(&mut self.overlay));
                } else if let Some(prompt) = &mut self.upload_prompt {
                    overlay_cursor = prompt.draw(&mut self.overlay);
                } else if let Some(prompt) = &self.download_prompt {
                    overlay_cursor = prompt.draw(&mut self.overlay);
                } else if let Some(t) = session.transfer() {
                    transfer_ui::draw_progress(&mut self.overlay, t.name(), t.progress());
                }
                true
            }
            _ => false,
        };

        let frame = match &self.session {
            _ if splash || self.help_open || self.settings_open || self.art_picker.is_some() => {
                let mut rows: Vec<&[Cell]> = (0..settings_ui::ROWS).map(|y| self.page_screen.row(y)).collect();
                rows.push(self.status.row(0));
                Frame::plain(rows, page_cursor, true)
            }
            _ if self.art.is_some() => {
                let v = self.art.as_ref().expect("checked");
                let mut rows = v.rows();
                rows.push(self.status.row(0));
                let sc = &v.art.term.screen;
                Frame { look: &sc.look, look_rows: art_ui::ROWS, ..Frame::plain(rows, None, sc.ice_colors) }
            }
            _ if self.dialer_open => {
                let mut rows: Vec<&[Cell]> = (0..dialer_ui::ROWS).map(|y| self.dialer_screen.row(y)).collect();
                rows.push(self.status.row(0));
                Frame::plain(rows, dialer_cursor, true)
            }
            Some(session) if overlay => {
                let mut rows: Vec<&[Cell]> = (0..self.overlay.rows()).map(|y| self.overlay.row(y)).collect();
                rows.push(self.status.row(0));
                // Dialogs are drawn in the BBS's look too, so the screen
                // behind them keeps its fonts and colors.
                let sc = &session.term.screen;
                let graphics = rip_graphics(session);
                Frame {
                    look: &sc.look,
                    look_rows: sc.rows(),
                    graphics,
                    ..Frame::plain(rows, overlay_cursor, sc.ice_colors)
                }
            }
            Some(session) => {
                let sc = &session.term.screen;
                let mut rows = terminal_rows(session, self.scroll);
                rows.push(self.status.row(0));
                let live = self.scroll == 0 || sc.scrollback_len() == 0;
                let cursor = (live && sc.cursor_visible && session.state == State::Connected).then(|| sc.cursor());
                let selection = self.selection;
                // Graphics belong to the live screen, not the scrollback.
                let graphics = rip_graphics(session).filter(|_| self.scroll == 0);
                Frame {
                    selection,
                    look: &sc.look,
                    look_rows: sc.rows(),
                    graphics,
                    ..Frame::plain(rows, cursor, sc.ice_colors)
                }
            }
            None => {
                let mut rows: Vec<&[Cell]> = (0..ROWS).map(|y| self.idle_screen.row(y)).collect();
                rows.push(self.status.row(0));
                Frame::plain(rows, None, true)
            }
        };
        // "Exit BadTerm?" over whatever is showing.
        let confirm_screen;
        let frame = if self.confirm_exit {
            let n = frame.rows.len() - 1;
            let cols = frame.rows.first().map_or(COLS, |r| r.len());
            let mut sc = Screen::new(cols, n, 0);
            for (y, row) in frame.rows[..n].iter().enumerate() {
                for (x, c) in row.iter().enumerate() {
                    if let Some(cell) = sc.cell_mut(x, y) {
                        *cell = *c;
                    }
                }
            }
            let msg = match &self.session {
                Some(s) if s.state == State::Connected => format!("This hangs up the call to {}.", s.entry.name),
                _ => String::new(),
            };
            ui::confirm_box(&mut sc, "Exit BadTerm?", &msg, &[("Y", "Exit"), ("N", "Stay")]);
            confirm_screen = sc;
            let mut rows: Vec<&[Cell]> = (0..n).map(|y| confirm_screen.row(y)).collect();
            rows.push(frame.rows[n]);
            Frame { rows, cursor: None, ..frame }
        } else {
            frame
        };
        // BadTerm's own screens in the theme's colors.
        let ui_look = ui::theme::ui_look();
        let frame = Frame { ui_look: &ui_look, ..frame };
        // The terminal's own height (and font) unless the dialer or the idle
        // screen is showing; one more row for the status bar.
        let text_rows = frame.rows.len() - 1;
        let cols = frame.rows.first().map_or(COLS, |r| r.len());
        let cell_w = if self.settings.nine_dot { 9 } else { 8 };
        // A C64 or Atari screen is laid out in its 8x8 font.
        let base = if frame.look_rows > 0 && Font::eight_only(frame.look.fonts[0]) {
            Font::vga_8x8()
        } else {
            Self::font_for(text_rows)
        };
        self.renderer.set_layout(base, cell_w, cols, text_rows + 1);
        self.renderer.draw(&frame, (self.tick / 2).is_multiple_of(2), self.tick.is_multiple_of(2));
        if self.flash_until.is_some() {
            self.renderer.flash(text_rows);
        }

        if let Err(e) = gfx.surface.resize(w, h) {
            return log::error!("resizing surface: {e}");
        }
        match gfx.surface.buffer_mut() {
            Ok(mut buf) => {
                // Auto: fullscreen and maximized windows fill the screen; a
                // normal window keeps whole-number scaling.
                let fill = gfx.window.fullscreen().is_some() || gfx.window.is_maximized();
                let scaling = match self.settings.scaling {
                    ScaleMode::Auto if fill => Scaling::Fill,
                    ScaleMode::Auto | ScaleMode::Integer => Scaling::Integer,
                    ScaleMode::Fit => Scaling::Fill,
                    ScaleMode::Stretch => Scaling::Stretch,
                };
                // A forced shape for the text area (4:3: 640x400, or
                // 720x400, as on a CRT); the status bar comes along.
                let pixel_aspect = match self.settings.aspect.ratio() {
                    Some(ratio) => {
                        let text_h = (self.renderer.native_height() * text_rows / (text_rows + 1)) as f64;
                        self.renderer.native_width() as f64 / text_h / ratio
                    }
                    None => 1.0,
                };
                let (ww, wh) = (w.get() as usize, h.get() as usize);
                if let Some(b) = &self.bezel
                    && self.bezel_frame.as_ref().is_none_or(|(size, _)| *size != (ww, wh))
                {
                    self.bezel_frame = Some(((ww, wh), b.render(ww, wh)));
                }
                match self.bezel_frame.as_ref().filter(|_| self.bezel.is_some()) {
                    Some((_, frame)) => {
                        // The picture fills the monitor's screen, as on a
                        // real one, whatever the pixel shape.
                        let (sw, sh) = (frame.screen.2.max(1) as f64, frame.screen.3.max(1) as f64);
                        let (nw, nh) = (self.renderer.native_width() as f64, self.renderer.native_height() as f64);
                        let (scanlines, curvature) = (self.settings.scanlines, self.settings.curvature);
                        let display = Display {
                            glow: self.settings.glow,
                            scaling: Scaling::Fill,
                            pixel_aspect: sh / sw * nw / nh,
                            crt,
                            scanlines,
                            curvature,
                        };
                        self.view_rect = self.renderer.blit_in(&mut buf, ww, wh, frame.screen, &display);
                        bezel::composite(&mut buf, &frame.pixels);
                    }
                    None => {
                        let (scanlines, curvature) = (self.settings.scanlines, self.settings.curvature);
                        let glow = self.settings.glow;
                        let display = Display { scaling, pixel_aspect, crt, glow, scanlines, curvature };
                        self.view_rect = self.renderer.blit(&mut buf, ww, wh, &display);
                    }
                }
                if let Err(e) = buf.present() {
                    log::error!("presenting frame: {e}");
                }
            }
            Err(e) => log::error!("getting frame buffer: {e}"),
        }
    }
}

/// A RIP system's graphics, to draw under its screen.
fn rip_graphics(session: &Session) -> Option<crate::render::Graphics<'_>> {
    let rip = session.term.rip()?;
    Some(crate::render::Graphics {
        pixels: rip.pixels(),
        width: badterm_core::rip::WIDTH,
        height: badterm_core::rip::HEIGHT,
        palette: rip.palette(),
    })
}
