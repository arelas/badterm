//! File transfer overlays: the ZMODEM progress window and the upload prompt.

use std::path::PathBuf;

use phosphor_core::screen::Screen;
use phosphor_core::transfer::{Direction, Progress, XyProtocol};

use super::files::{FilePicker, PickerAction};
use crate::input::{Key, Mods};

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
    if let Some(at) = p.resumed_at {
        files.push_str(&format!("   resumed at {}", format_bytes(at)));
    }
    if p.files_queued > 0 {
        files.push_str(&format!("   queued: {}", p.files_queued));
    }
    text(s, x + 3, y + 5, inner, &files, t.window_text.fg(), t.window_text.bg());
    let hint = "Esc cancels";
    label(s, x + (W - hint.len()) / 2, y + 7, hint, t.keys.fg(), t.keys.bg());
}

/// Upload protocols in the order offered: ZMODEM, then the others.
pub const UPLOAD_PROTOCOLS: [Option<XyProtocol>; 6] = [
    None,
    Some(XyProtocol::Ymodem),
    Some(XyProtocol::YmodemG),
    Some(XyProtocol::Xmodem1k),
    Some(XyProtocol::XmodemCrc),
    Some(XyProtocol::Xmodem),
];

pub fn protocol_name(p: Option<XyProtocol>) -> &'static str {
    p.map_or("ZMODEM", XyProtocol::name)
}

/// What the upload window decided.
pub enum UploadAction {
    None,
    Cancel,
    /// Send these, with this protocol (None: ZMODEM).
    Send(Option<XyProtocol>, Vec<PathBuf>),
}

/// The upload window: a file picker and the protocol. `requested` is set
/// when the BBS asked for a ZMODEM upload (it is waiting with ZRINIT), which
/// fixes the protocol.
pub struct UploadPrompt {
    pub picker: FilePicker,
    pub protocol: usize,
    pub requested: bool,
}

impl UploadPrompt {
    pub fn new(requested: bool, dir: PathBuf, protocol: usize) -> UploadPrompt {
        let protocol = if requested { 0 } else { protocol.min(UPLOAD_PROTOCOLS.len() - 1) };
        UploadPrompt { picker: FilePicker::new(dir, true, &[]), protocol, requested }
    }

    pub fn protocol(&self) -> Option<XyProtocol> {
        UPLOAD_PROTOCOLS[self.protocol]
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods) -> UploadAction {
        match key {
            Key::Escape => return UploadAction::Cancel,
            Key::Left | Key::Right if !self.requested => {
                let n = UPLOAD_PROTOCOLS.len();
                self.protocol = if key == Key::Left { (self.protocol + n - 1) % n } else { (self.protocol + 1) % n };
            }
            _ => {
                if let PickerAction::Chosen(paths) = self.picker.handle_key(key, mods) {
                    return UploadAction::Send(self.protocol(), paths);
                }
            }
        }
        UploadAction::None
    }

    /// Draw the window; returns the text cursor, if a path is being typed.
    pub fn draw(&mut self, s: &mut Screen) -> Option<(usize, usize)> {
        let (w, h) = (74, 21);
        let (x, y) = ((s.cols() - w) / 2, (s.rows().saturating_sub(h)) / 2);
        let title = if self.requested { "The BBS is ready to receive" } else { "Upload" };
        let t = theme::current();
        frame(s, x, y, w, h, title, t.window.fg(), t.window.bg());
        let name_attr = if self.requested { t.field_name } else { t.keys };
        label(s, x + 2, y + 1, "Protocol:", name_attr.fg(), name_attr.bg());
        let v = format!("\u{25C4} {:<10} \u{25BA}", protocol_name(self.protocol()));
        label(s, x + 12, y + 1, &v, t.field_value.fg(), t.field_value.bg());
        let note = match self.protocol() {
            None if self.requested => "(the BBS started ZMODEM)",
            Some(p) if !p.batch() => "(one file; start the BBS's receive first)",
            Some(_) => "(start the BBS's receive first)",
            None => "(drop files on the window, too)",
        };
        label(s, x + 28, y + 1, note, t.note.fg(), t.note.bg());
        let cursor = self.picker.draw(s, x + 2, y + 3, w - 4, h - 6);
        let marked = self.picker.marked();
        let keys = if marked > 0 {
            format!("Enter send {marked} marked  Space mark  Bksp up  Tab path  Esc cancel")
        } else {
            "Enter send  Space mark  \u{2190}\u{2192} protocol  Bksp up  Tab path  Esc cancel".to_string()
        };
        label(s, x + 2, y + h - 2, &truncate(&keys, w - 4), t.keys.fg(), t.keys.bg());
        cursor
    }
}

/// Download protocols that need starting (ZMODEM starts by itself).
pub const DOWNLOAD_PROTOCOLS: [XyProtocol; 5] = XyProtocol::ALL;

pub enum DownloadAction {
    None,
    Cancel,
    /// Receive with this protocol; the name is for XMODEM.
    Receive(XyProtocol, Option<String>),
}

/// The window that starts an XMODEM or YMODEM download.
pub struct DownloadPrompt {
    pub protocol: usize,
    /// What an XMODEM file is saved as.
    pub name: TextField,
}

impl DownloadPrompt {
    pub fn new(protocol: usize) -> DownloadPrompt {
        DownloadPrompt { protocol: protocol.min(DOWNLOAD_PROTOCOLS.len() - 1), name: TextField::new("", 200) }
    }

    fn proto(&self) -> XyProtocol {
        DOWNLOAD_PROTOCOLS[self.protocol]
    }

    pub fn handle_key(&mut self, key: Key, mods: Mods) -> DownloadAction {
        let n = DOWNLOAD_PROTOCOLS.len();
        match key {
            Key::Escape => return DownloadAction::Cancel,
            Key::Up => self.protocol = (self.protocol + n - 1) % n,
            Key::Down | Key::Tab => self.protocol = (self.protocol + 1) % n,
            Key::Enter => {
                let name = self.name.value.trim().to_string();
                if !self.proto().batch() && name.is_empty() {
                    return DownloadAction::None;
                }
                let name = (!self.proto().batch()).then_some(name);
                return DownloadAction::Receive(self.proto(), name);
            }
            _ if !self.proto().batch() => {
                self.name.handle_key(key, mods);
            }
            _ => {}
        }
        DownloadAction::None
    }

    pub fn draw(&self, s: &mut Screen) -> Option<(usize, usize)> {
        let (w, h) = (60, 13);
        let (x, y) = ((s.cols() - w) / 2, (s.rows().saturating_sub(h)) / 2);
        let t = theme::current();
        frame(s, x, y, w, h, "Download", t.window.fg(), t.window.bg());
        label(s, x + 2, y + 1, "Start the BBS's send first, then choose:", t.window_text.fg(), t.window_text.bg());
        for (i, p) in DOWNLOAD_PROTOCOLS.iter().enumerate() {
            let a = if i == self.protocol { t.selected } else { t.field_value };
            let what = match p {
                XyProtocol::Ymodem => "names and sizes, several files",
                XyProtocol::YmodemG => "YMODEM without waiting: fastest",
                XyProtocol::Xmodem1k => "1K blocks, one file",
                XyProtocol::XmodemCrc => "128-byte blocks, one file",
                XyProtocol::Xmodem => "the original, one file",
            };
            text(s, x + 3, y + 3 + i, w - 6, &format!(" {:<11} {what}", p.name()), a.fg(), a.bg());
        }
        let mut cursor = None;
        if self.proto().batch() {
            label(s, x + 2, y + 9, "Files keep the names the BBS sends.", t.note.fg(), t.note.bg());
        } else {
            label(s, x + 2, y + 9, "Save as:", t.field_name.fg(), t.field_name.bg());
            cursor = Some(self.name.draw(s, x + 11, y + 9, w - 13, false, t.input.fg(), t.input.bg()));
        }
        label(s, x + 2, y + 11, "\u{2191}\u{2193} protocol   Enter start   Esc cancel", t.keys.fg(), t.keys.bg());
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
}
