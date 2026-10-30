//! File transfer overlays: the ZMODEM progress window and the upload prompt.

use std::path::PathBuf;

use phosphor_core::screen::Screen;
use phosphor_core::transfer::{Direction, Progress};

use super::*;

const W: usize = 64;

/// Draw the progress window for an active transfer.
pub fn draw_progress(s: &mut Screen, p: &Progress) {
    let (x, y, h) = ((s.cols() - W) / 2, 8, 9);
    let title = match p.direction {
        Direction::Download => "ZMODEM Download",
        Direction::Upload => "ZMODEM Upload",
    };
    let t = theme::current();
    frame(s, x, y, W, h, title, t.window.fg(), t.window.bg());
    let inner = W - 6;
    let name = if p.file_name.is_empty() { "waiting for the other side..." } else { p.file_name.as_str() };
    text(s, x + 3, y + 2, inner, &format!("File: {name}"), t.window.fg(), t.window.bg());

    let fraction = match p.file_size {
        Some(size) if size > 0 => (p.file_bytes as f64 / size as f64).min(1.0),
        Some(_) => 1.0,
        None => 0.0,
    };
    let bar_w = inner - 6;
    let filled = (fraction * bar_w as f64).round() as usize;
    let mut bar = vec![0xB2; filled];
    bar.resize(bar_w, 0xB0);
    s.draw_text(x + 3, y + 3, &bar, t.field_value.fg(), t.field_value.bg());
    if p.file_size.is_some() {
        label(s, x + 3 + bar_w + 1, y + 3, &format!("{:>3}%", (fraction * 100.0) as u32), t.keys.fg(), t.keys.bg());
    }

    let rate = p.rate();
    let mut line = match p.file_size {
        Some(size) => format!("{} of {}", format_bytes(p.file_bytes), format_bytes(size)),
        None => format_bytes(p.file_bytes),
    };
    if rate > 0.0 {
        line.push_str(&format!("   {}/s", format_bytes(rate as u64)));
        if let Some(size) = p.file_size {
            let eta = (size.saturating_sub(p.file_bytes) as f64 / rate) as u64;
            line.push_str(&format!("   ETA {}:{:02}", eta / 60, eta % 60));
        }
    }
    text(s, x + 3, y + 4, inner, &line, t.window_text.fg(), t.window_text.bg());

    let mut files = format!("Files done: {}", p.files_done);
    if p.files_queued > 0 {
        files.push_str(&format!("   queued: {}", p.files_queued));
    }
    text(s, x + 3, y + 5, inner, &files, t.window_text.fg(), t.window_text.bg());
    let hint = "Esc cancels";
    label(s, x + (W - hint.len()) / 2, y + 7, hint, t.keys.fg(), t.keys.bg());
}

/// The "files to upload" prompt. `requested` is set when the BBS asked for
/// the upload (it is waiting with ZRINIT).
pub struct UploadPrompt {
    pub field: TextField,
    pub requested: bool,
}

impl UploadPrompt {
    pub fn new(requested: bool) -> UploadPrompt {
        UploadPrompt { field: TextField::new("", 1000), requested }
    }

    /// The paths typed, separated by `;`, with quotes and `~/` handled.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.field
            .value
            .split(';')
            .map(|p| p.trim().trim_matches('"').trim())
            .filter(|p| !p.is_empty())
            .map(expand_home)
            .collect()
    }

    /// Draw the prompt; returns the text cursor position.
    pub fn draw(&self, s: &mut Screen) -> (usize, usize) {
        let (x, y, w) = ((s.cols() - 70) / 2, 9, 70);
        let title = if self.requested { "The BBS is ready to receive" } else { "ZMODEM Upload" };
        let t = theme::current();
        let (dim_fg, dim_bg) = (t.window_text.fg(), t.window_text.bg());
        frame(s, x, y, w, 7, title, t.window.fg(), t.window.bg());
        label(s, x + 2, y + 1, "File(s) to send; separate several with ;", dim_fg, dim_bg);
        label(s, x + 2, y + 2, "Tip: you can also drop files onto the window.", dim_fg, dim_bg);
        let cursor = self.field.draw(s, x + 2, y + 4, w - 4, false, t.input.fg(), t.input.bg());
        label(s, x + 2, y + 5, "Enter send   Esc cancel", t.keys.fg(), t.keys.bg());
        cursor
    }
}

pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn prompt_paths() {
        let mut p = UploadPrompt::new(false);
        p.field.value = r#" "/tmp/a b.zip" ; /tmp/c.txt;; "#.into();
        assert_eq!(p.paths(), [PathBuf::from("/tmp/a b.zip"), PathBuf::from("/tmp/c.txt")]);
    }
}
