//! Mouse reports for the BBS, as xterm (and CTerm, after it) sends them
//! once the BBS asks: X10's presses (`CSI ? 9 h`), presses and releases
//! (1000), with drags (1002) or every movement (1003), in the old
//! `CSI M b x y` form or, with 1006, `CSI < b ; x ; y M`/`m` (1016: the
//! same, in pixels).

/// What the mouse did. Buttons are 0 (left), 1 (middle) and 2 (right).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEvent {
    Press(u8),
    Release(u8),
    /// It moved, with this button held (or none).
    Move(Option<u8>),
    /// The wheel: up or down.
    Wheel(bool),
}

/// Keys held with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// The reporting the BBS has asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseModes {
    pub x10: bool,
    pub normal: bool,
    pub drag: bool,
    pub any: bool,
    pub sgr: bool,
    pub pixels: bool,
}

impl MouseModes {
    /// Whether the BBS wants the mouse at all.
    pub fn tracking(&self) -> bool {
        self.x10 || self.normal || self.drag || self.any
    }

    /// Set or reset a DEC private mode, if it's one of these.
    pub fn set(&mut self, mode: u32, on: bool) -> bool {
        match mode {
            9 => self.x10 = on,
            1000 => self.normal = on,
            1002 => self.drag = on,
            1003 => self.any = on,
            1006 => self.sgr = on,
            1016 => self.pixels = on,
            _ => return false,
        }
        true
    }

    /// The modes that are set, for CTerm's mode report.
    pub fn set_modes(&self) -> Vec<u32> {
        [(9, self.x10), (1000, self.normal), (1002, self.drag), (1003, self.any), (1006, self.sgr), (1016, self.pixels)]
            .iter()
            .filter(|m| m.1)
            .map(|m| m.0)
            .collect()
    }

    /// The report for an event at a cell (0-based) and pixel (0-based,
    /// on the terminal's own pixel grid), if one is due.
    pub fn report(
        &self,
        event: MouseEvent,
        cell: (usize, usize),
        pixel: (usize, usize),
        mods: MouseMods,
    ) -> Option<Vec<u8>> {
        if !self.tracking() {
            return None;
        }
        let (code, release) = match event {
            MouseEvent::Press(b) => (b.min(2) as u32, false),
            MouseEvent::Release(_) if self.x10 && !self.normal && !self.drag && !self.any => return None,
            MouseEvent::Release(b) => (if self.sgr || self.pixels { b.min(2) as u32 } else { 3 }, true),
            MouseEvent::Move(held) => {
                if !(self.any || self.drag && held.is_some()) {
                    return None;
                }
                (32 + held.map_or(3, |b| b.min(2) as u32), false)
            }
            MouseEvent::Wheel(up) if !self.x10 || self.normal || self.drag || self.any => {
                (if up { 64 } else { 65 }, false)
            }
            MouseEvent::Wheel(_) => return None,
        };
        // X10's reports carry no keys.
        let only_x10 = self.x10 && !self.normal && !self.drag && !self.any;
        let keys =
            if only_x10 { 0 } else { ((mods.shift as u32) * 4) | ((mods.alt as u32) * 8) | ((mods.ctrl as u32) * 16) };
        let b = code | keys;
        if self.sgr || self.pixels {
            let (x, y) = if self.pixels { pixel } else { cell };
            let end = if release { 'm' } else { 'M' };
            return Some(format!("\x1b[<{b};{};{}{end}", x + 1, y + 1).into_bytes());
        }
        // The old form: a byte each, so positions past 222 can't be sent.
        let (x, y) = (cell.0 + 33, cell.1 + 33);
        (x <= 255 && y <= 255).then(|| vec![0x1B, b'[', b'M', (32 + b) as u8, x as u8, y as u8])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(list: &[u32]) -> MouseModes {
        let mut m = MouseModes::default();
        for &n in list {
            m.set(n, true);
        }
        m
    }

    #[test]
    fn reports() {
        let none = MouseMods::default();
        let at = (4, 2);
        let px = (37, 21);
        // Nothing asked for, nothing sent.
        assert_eq!(MouseModes::default().report(MouseEvent::Press(0), at, px, none), None);
        // X10: presses only, the old form.
        let x10 = modes(&[9]);
        assert_eq!(x10.report(MouseEvent::Press(0), at, px, none).unwrap(), b"\x1b[M \x25\x23");
        assert_eq!(x10.report(MouseEvent::Release(0), at, px, none), None);
        // 1000: releases are button 3; keys add 4, 8, 16.
        let normal = modes(&[1000]);
        assert_eq!(normal.report(MouseEvent::Release(0), at, px, none).unwrap(), b"\x1b[M#\x25\x23");
        let ctrl = MouseMods { ctrl: true, ..none };
        assert_eq!(normal.report(MouseEvent::Press(2), at, px, ctrl).unwrap(), b"\x1b[M\x32\x25\x23");
        assert_eq!(normal.report(MouseEvent::Move(Some(0)), at, px, none), None);
        // 1002 reports drags, 1003 any movement; SGR is one-based text.
        let drag = modes(&[1002, 1006]);
        assert_eq!(drag.report(MouseEvent::Move(Some(0)), at, px, none).unwrap(), b"\x1b[<32;5;3M");
        assert_eq!(drag.report(MouseEvent::Move(None), at, px, none), None);
        let any = modes(&[1003, 1006]);
        assert_eq!(any.report(MouseEvent::Move(None), at, px, none).unwrap(), b"\x1b[<35;5;3M");
        assert_eq!(any.report(MouseEvent::Release(2), at, px, none).unwrap(), b"\x1b[<2;5;3m");
        assert_eq!(any.report(MouseEvent::Wheel(false), at, px, none).unwrap(), b"\x1b[<65;5;3M");
        // 1016: pixels.
        let pixels = modes(&[1000, 1016]);
        assert_eq!(pixels.report(MouseEvent::Press(0), at, px, none).unwrap(), b"\x1b[<0;38;22M");
        // The old form can't send columns past 222.
        assert_eq!(normal.report(MouseEvent::Press(0), (230, 0), px, none), None);
    }
}
