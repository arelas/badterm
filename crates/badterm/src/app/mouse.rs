//! The mouse: selecting and copying text, and the wheel.

use super::*;

impl App {
    /// The terminal cell under the mouse, if the terminal is what is shown.
    pub(super) fn mouse_cell(&self) -> Option<(usize, usize)> {
        let tab = self.tab()?;
        let s = &tab.session;
        let overlay =
            tab.login.is_some() || tab.save_prompt.is_some() || tab.upload_prompt.is_some() || s.transfer().is_some();
        if self.dialer_open || overlay {
            return None;
        }
        let (ox, oy, w, h) = self.view_rect;
        let (mx, my) = (self.mouse.0.max(0.0) as usize, self.mouse.1.max(0.0) as usize);
        let (cols, rows) = (self.renderer.cols(), self.renderer.rows());
        let x = (mx.saturating_sub(ox) * cols / w.max(1)).min(cols - 1);
        let y = (my.saturating_sub(oy) * rows / h.max(1)).min(s.term.screen.rows() - 1);
        Some((x, y))
    }

    /// Where the mouse is, for a mouse report, if the BBS wants them and
    /// may have them now (its screen showing live, Shift not held, which
    /// keeps the mouse for selecting): the cell, and the pixel on the
    /// terminal's own pixel grid.
    fn mouse_report_at(&self) -> Option<((usize, usize), (usize, usize))> {
        if self.scroll() != 0 || self.mods.shift_key() {
            return None;
        }
        let cell = self.mouse_cell()?;
        let s = self.session().filter(|s| s.state == State::Connected && s.term.mouse.tracking())?;
        let (ox, oy, w, h) = self.view_rect;
        let (mx, my) = (self.mouse.0.max(0.0) as usize, self.mouse.1.max(0.0) as usize);
        let nx = mx.saturating_sub(ox) * self.renderer.native_width() / w.max(1);
        let ny = my.saturating_sub(oy) * self.renderer.native_height() / h.max(1);
        let sc = &s.term.screen;
        let (pw, ph) = sc.size_px();
        let text_h = self.renderer.native_height() * sc.rows() / self.renderer.rows().max(1);
        let px = (nx * pw / self.renderer.native_width().max(1)).min(pw - 1);
        let py = (ny * ph / text_h.max(1)).min(ph - 1);
        Some((cell, (px, py)))
    }

    /// Send the BBS a mouse report, if it wants this one.
    fn mouse_report(&mut self, event: badterm_core::mouse::MouseEvent) -> bool {
        let Some((cell, pixel)) = self.mouse_report_at() else { return false };
        let mods =
            badterm_core::mouse::MouseMods { shift: false, alt: self.mods.alt_key(), ctrl: self.mods.control_key() };
        let Some(session) = self.tab_mut().map(|t| &mut t.session) else { return false };
        if let Some(bytes) = session.term.mouse.report(event, cell, pixel, mods) {
            session.send(&bytes);
            self.mouse_reported = Some((cell, pixel));
        }
        true
    }

    pub(super) fn on_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        use badterm_core::mouse::MouseEvent;
        let code = match button {
            MouseButton::Left => Some(0),
            MouseButton::Middle => Some(1),
            MouseButton::Right => Some(2),
            _ => None,
        };
        // A BBS that asked for the mouse gets it (the emulations with
        // mouse menus of their own, RIP, IGS and SkyPix, aside).
        if let Some(b) = code
            && !self.dragging
        {
            let event = match state {
                ElementState::Pressed => MouseEvent::Press(b),
                ElementState::Released => MouseEvent::Release(b),
            };
            if self.mouse_report(event) {
                self.mouse_held = (state == ElementState::Pressed).then_some(b);
                return;
            }
        }
        if button != MouseButton::Left {
            return;
        }
        // A tab's chip in the status bar shows its call.
        if state == ElementState::Pressed
            && let Some(t) = self.chip_under_mouse()
        {
            self.show_tab(t);
            return self.request_redraw();
        }
        match state {
            // A RIPscrip button or mouse region sends its command.
            ElementState::Pressed if self.scroll() == 0 && (self.rip_click() || self.igs_click()) => {}
            ElementState::Pressed => {
                self.selection = self.mouse_cell().map(Selection::at);
                self.dragging = self.selection.is_some();
            }
            ElementState::Released if self.dragging => {
                self.dragging = false;
                if let Some(sel) = self.selection.filter(|s| !s.is_empty()) {
                    // Shift copies with colors, as ANSI.
                    self.copy_selection(sel, self.mods.shift_key());
                }
            }
            ElementState::Released => {}
        }
        self.request_redraw();
    }

    /// The tab whose chip in the status bar is under the mouse, if the
    /// chips are showing (more than one call, the terminal on screen).
    fn chip_under_mouse(&self) -> Option<usize> {
        let covered = self.dialer_open
            || self.settings_open
            || self.help_open
            || self.confirm_exit
            || self.art.is_some()
            || self.art_picker.is_some();
        if self.tabs.len() < 2 || covered || self.tab()?.session.entry.hide_status {
            return None;
        }
        let (ox, oy, w, h) = self.view_rect;
        let (mx, my) = (self.mouse.0.max(0.0) as usize, self.mouse.1.max(0.0) as usize);
        if mx < ox || my < oy || mx >= ox + w || my >= oy + h {
            return None;
        }
        let (cols, rows) = (self.renderer.cols(), self.renderer.rows());
        let (x, y) = ((mx - ox) * cols / w.max(1), (my - oy) * rows / h.max(1));
        let t = x / tabs::CHIP_W;
        (y + 1 == rows && t < self.tabs.len()).then_some(t)
    }

    /// Click a RIP mouse region under the mouse, if there is one.
    fn rip_click(&mut self) -> bool {
        if self.mouse_cell().is_none() {
            return false;
        }
        let t = self.active;
        let Some(session) = self.tabs.get_mut(t).map(|t| &mut t.session).filter(|s| s.state == State::Connected) else {
            return false;
        };
        if session.term.rip().is_none() {
            return false;
        }
        let (ox, oy, w, h) = self.view_rect;
        let (mx, my) = (self.mouse.0.max(0.0) as usize, self.mouse.1.max(0.0) as usize);
        let x = mx.saturating_sub(ox) * self.renderer.native_width() / w.max(1);
        let y = my.saturating_sub(oy) * self.renderer.native_height() / h.max(1);
        let rows = session.term.screen.rows();
        let Some((gx, gy)) =
            self.renderer.graphics_point(x, y, rows, badterm_core::rip::WIDTH, badterm_core::rip::HEIGHT)
        else {
            return false;
        };
        let sent = session.rip_press(gx, gy);
        // A question it raised shows now.
        self.session_events(t);
        sent
    }

    /// Click an IGS mouse zone, when the BBS is waiting for one.
    fn igs_click(&mut self) -> bool {
        if self.mouse_cell().is_none() {
            return false;
        }
        let (ox, oy, w, h) = self.view_rect;
        let (mx, my) = (self.mouse.0.max(0.0) as usize, self.mouse.1.max(0.0) as usize);
        let x = mx.saturating_sub(ox) * self.renderer.native_width() / w.max(1);
        let y = my.saturating_sub(oy) * self.renderer.native_height() / h.max(1);
        let t = self.active;
        let Some(session) = self.tabs.get_mut(t).map(|t| &mut t.session).filter(|s| s.state == State::Connected) else {
            return false;
        };
        let rows = session.term.screen.rows();
        // SkyPix reports every click.
        if let Some(sky) = session.term.skypix() {
            let (sw, sh) = (badterm_core::skypix::WIDTH, badterm_core::skypix::HEIGHT);
            let Some((gx, gy)) = self.renderer.graphics_point(x, y, rows, sw, sh) else { return false };
            let text = sky.click(gx as i32, gy as i32);
            session.send(&text);
            return true;
        }
        let Some(igs) = session.term.igs_mut() else { return false };
        let (iw, ih) = (igs.width(), igs.height());
        let Some((gx, gy)) = self.renderer.graphics_point(x, y, rows, iw, ih) else { return false };
        let Some(text) = igs.click(gx as i32, gy as i32) else { return false };
        session.send(&text);
        true
    }

    pub(super) fn on_mouse_move(&mut self, x: f64, y: f64) {
        self.mouse = (x, y);
        // Movement, for a BBS that wants it, when it's into another cell
        // (or pixel, if it asked for pixels).
        if !self.dragging
            && let Some((cell, pixel)) = self.mouse_report_at()
        {
            let pixels = self.session().is_some_and(|s| s.term.mouse.pixels);
            let moved = self.mouse_reported.is_none_or(|(c, p)| if pixels { p != pixel } else { c != cell });
            if moved {
                self.mouse_report(badterm_core::mouse::MouseEvent::Move(self.mouse_held));
            }
            return;
        }
        if self.dragging
            && let Some(cell) = self.mouse_cell()
            && let Some(sel) = self.selection.as_mut()
            && sel.head != cell
        {
            sel.head = cell;
            self.request_redraw();
        }
    }

    pub(super) fn copy_selection(&mut self, sel: Selection, as_ansi: bool) {
        let Some(tab) = self.tab() else { return };
        let session = &tab.session;
        let rows = terminal_rows(session, tab.scroll);
        let lines = sel.lines(&rows);
        let text = if as_ansi {
            let opts =
                AnsiOptions { ice: session.term.screen.ice_colors, unicode: true, codepage: session.term.codepage() };
            String::from_utf8_lossy(&export::ansi(&lines, opts)).into_owned()
        } else {
            export::text_in(&lines, session.term.text_codepage()).trim_end_matches('\n').to_string()
        };
        let msg = match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
            Ok(()) if as_ansi => "Copied as ANSI".to_string(),
            Ok(()) => "Copied".to_string(),
            Err(e) => format!("Copy failed: {e}"),
        };
        self.notify(msg);
    }

    pub(super) fn on_wheel(&mut self, lines: isize) {
        if let Some(v) = self.art.as_mut().filter(|_| self.art_picker.is_none()) {
            v.scroll(lines);
            return self.request_redraw();
        }
        // The wheel, for a BBS that wants the mouse: a report a turn.
        if lines != 0 && !self.dialer_open && self.mouse_report(badterm_core::mouse::MouseEvent::Wheel(lines > 0)) {
            return;
        }
        if !self.dialer_open
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            let max = tab.session.term.screen.scrollback_len() as isize;
            tab.scroll = (tab.scroll as isize + lines).clamp(0, max) as usize;
            self.selection = None;
            self.request_redraw();
        }
    }
}
