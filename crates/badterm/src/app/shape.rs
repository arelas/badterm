//! "Fit, keep shape" keeps the window itself the picture's shape (or the
//! bezel's), so there are no borders to fill: once a resize settles, the
//! window is put back in shape, keeping the side the user dragged.
//!
//! The window systems winit runs on have no common way to hold a window to
//! a shape while it is dragged, so it is put right after; waiting until the
//! drag ends keeps it from fighting the mouse.

use super::*;

/// How long after the last resize to wait before putting it in shape.
const SETTLE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct WindowShape {
    on: bool,
    /// Width over height.
    ratio: f64,
    /// When to put the window in shape.
    at: Option<Instant>,
    /// The size it last settled at, to tell which side a drag changed.
    settled: Option<(u32, u32)>,
    /// The size last asked for, so one the window manager refuses (a
    /// tiling one, or a minimum size) isn't asked for again and again.
    asked: Option<(u32, u32)>,
    /// The window's smallest size.
    pub min: (u32, u32),
}

impl WindowShape {
    /// The window changed size: put it in shape once it stops.
    pub fn resized(&mut self, _size: PhysicalSize<u32>) {
        if self.on {
            self.at = Some(Instant::now() + SETTLE);
        }
    }

    /// The shape it should be, and whether to keep it in it, from each
    /// frame; a new shape (another aspect, bezel or width) applies at once.
    pub fn want(&mut self, ratio: f64, on: bool) {
        if on && (!self.on || (ratio - self.ratio).abs() > 1e-3) {
            self.at = Some(Instant::now());
            self.asked = None;
        }
        if !on {
            self.at = None;
        }
        (self.on, self.ratio) = (on, ratio);
    }

    pub fn due(&self) -> Option<Instant> {
        self.at
    }

    /// The size to ask for, if `size` isn't in shape (within `max`, the
    /// screen's size).
    fn target(&mut self, size: (u32, u32), max: Option<(u32, u32)>) -> Option<(u32, u32)> {
        self.at = None;
        let prev = self.settled.unwrap_or(size);
        self.settled = Some(size);
        let t = fit_shape(size, prev, self.ratio, self.min, max);
        let close = t.0.abs_diff(size.0) <= 1 && t.1.abs_diff(size.1) <= 1;
        if close || self.asked == Some(t) {
            return None;
        }
        self.asked = Some(t);
        Some(t)
    }
}

/// `size` put in the shape `ratio` (width over height): the side that
/// changed most since `prev` stays, the other follows; then made to fit
/// between `min` and `max`.
fn fit_shape(size: (u32, u32), prev: (u32, u32), ratio: f64, min: (u32, u32), max: Option<(u32, u32)>) -> (u32, u32) {
    let (w, h) = (size.0 as f64, size.1 as f64);
    let change = |now: f64, was: u32| (now - was as f64).abs() / was.max(1) as f64;
    let (mut w, mut h) = if change(h, prev.1) > change(w, prev.0) { (h * ratio, h) } else { (w, w / ratio) };
    if let Some((mw, mh)) = max {
        // Room for the title bar and a taskbar.
        let s = (mw as f64 * 0.95 / w).min(mh as f64 * 0.9 / h).min(1.0);
        (w, h) = (w * s, h * s);
    }
    let s = (min.0 as f64 / w).max(min.1 as f64 / h).max(1.0);
    ((w * s).round() as u32, (h * s).round() as u32)
}

impl App {
    pub(super) fn snap_window_shape(&mut self) {
        self.shape.at = None;
        let Some(g) = &self.gfx else { return };
        if g.window.fullscreen().is_some() || g.window.is_maximized() || g.window.is_minimized() == Some(true) {
            return;
        }
        let size = g.window.inner_size();
        let max = g.window.current_monitor().map(|m| (m.size().width, m.size().height));
        if let Some((w, h)) = self.shape.target((size.width, size.height), max) {
            let _ = g.window.request_inner_size(PhysicalSize::new(w, h));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_side_dragged() {
        // Wider: the height follows.
        assert_eq!(fit_shape((1600, 800), (1280, 800), 1.6, (640, 400), None), (1600, 1000));
        // Taller: the width follows.
        assert_eq!(fit_shape((1280, 1000), (1280, 800), 1.6, (640, 400), None), (1600, 1000));
        // A corner: the side that changed most.
        assert_eq!(fit_shape((1400, 1000), (1280, 800), 1.6, (640, 400), None), (1600, 1000));
    }

    #[test]
    fn stays_on_screen_and_above_the_minimum() {
        assert_eq!(fit_shape((1280, 2000), (1280, 800), 1.6, (640, 400), Some((1920, 1080))), (1555, 972));
        assert_eq!(fit_shape((640, 300), (640, 400), 16.0 / 9.0, (640, 400), None), (711, 400));
    }

    #[test]
    fn asks_once() {
        let mut s = WindowShape { on: true, ratio: 1.6, min: (640, 400), ..Default::default() };
        assert_eq!(s.target((1280, 900), None), Some((1280, 800)));
        // Refused: not asked again.
        assert_eq!(s.target((1280, 900), None), None);
        assert_eq!(s.target((1280, 900), None), None);
        // In shape already.
        assert_eq!(s.target((1600, 1000), None), None);
    }
}
