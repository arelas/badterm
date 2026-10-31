//! The mouse: selecting and copying text, and the wheel.

use super::*;

impl App {
    /// The terminal cell under the mouse, if the terminal is what is shown.
    pub(super) fn mouse_cell(&self) -> Option<(usize, usize)> {
        let s = self.session.as_ref()?;
        let overlay = self.login.is_some()
            || self.save_prompt.is_some()
            || self.upload_prompt.is_some()
            || s.transfer().is_some();
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

    pub(super) fn on_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        if button != MouseButton::Left {
            return;
        }
        match state {
            // A RIPscrip button or mouse region sends its command.
            ElementState::Pressed if self.scroll == 0 && self.rip_click() => {}
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

    /// Click a RIP mouse region under the mouse, if there is one.
    fn rip_click(&mut self) -> bool {
        if self.mouse_cell().is_none() {
            return false;
        }
        let Some(session) = self.session.as_mut().filter(|s| s.state == State::Connected) else { return false };
        let Some(rip) = session.term.rip() else { return false };
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
        let Some(text) = rip.click(gx, gy) else { return false };
        session.send(&text);
        true
    }

    pub(super) fn on_mouse_move(&mut self, x: f64, y: f64) {
        self.mouse = (x, y);
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
        let Some(session) = &self.session else { return };
        let rows = terminal_rows(session, self.scroll);
        let lines = sel.lines(&rows);
        let text = if as_ansi {
            let opts = AnsiOptions { ice: session.term.screen.ice_colors, unicode: true };
            String::from_utf8_lossy(&export::ansi(&lines, opts)).into_owned()
        } else {
            export::text(&lines).trim_end_matches('\n').to_string()
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
        if let Some(s) = self.session.as_ref().filter(|_| !self.dialer_open) {
            self.selection = None;
            let max = s.term.screen.scrollback_len() as isize;
            self.scroll = (self.scroll as isize + lines).clamp(0, max) as usize;
            self.request_redraw();
        }
    }
}
