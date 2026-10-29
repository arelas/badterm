//! The windowed application: event loop handling, mode switching between the
//! dialer and a live session, hotkeys, scrollback viewing, file transfers and
//! presentation.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use phosphor_core::dialer::{self, Directory, Entry};
use phosphor_core::screen::{Cell, Screen};
use phosphor_core::session::{NetEvent, Session, SessionConfig, SessionEvent, State};
use phosphor_core::transfer::{Direction, Outcome};
use phosphor_core::{Encoding, TermEvent, cp437};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Fullscreen, UserAttentionType, Window, WindowId};

use crate::input::{self, Key, Mods};
use crate::render::{Font, Frame, Renderer};
use crate::ui::dialer::{self as dialer_ui, Action, DialerUi};
use crate::ui::transfer::{self as transfer_ui, UploadPrompt};
use crate::ui::{self, format_date};

const COLS: usize = 80;
const ROWS: usize = 25;
/// Terminal rows plus the status bar.
const GRID_ROWS: usize = ROWS + 1;
const SCROLLBACK_LINES: usize = 10_000;
/// Cursor blink half-period; text blinks at half this rate.
const TICK: Duration = Duration::from_millis(267);
const NOTICE_TIME: Duration = Duration::from_secs(4);
const HOTKEY: &str = if cfg!(target_os = "macos") { "Cmd" } else { "Alt" };

pub enum UserEvent {
    /// A network event for the session with this id.
    Net(u64, NetEvent),
}

pub struct Paths {
    pub directory: PathBuf,
    pub captures: PathBuf,
    /// Pinned SSH host keys.
    pub known_hosts: PathBuf,
    /// Where ZMODEM downloads are saved.
    pub downloads: PathBuf,
    /// Default location offered for import/export.
    pub files: PathBuf,
}

impl Paths {
    pub fn new() -> Paths {
        let proj = directories::ProjectDirs::from("org", "Phosphor", "Phosphor");
        let (config, data) = match &proj {
            Some(p) => (p.config_dir().to_path_buf(), p.data_dir().to_path_buf()),
            None => (PathBuf::from("."), PathBuf::from(".")),
        };
        let files = directories::UserDirs::new()
            .map(|u| u.download_dir().unwrap_or(u.home_dir()).to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Paths {
            directory: config.join("dialer.toml"),
            captures: data.join("captures"),
            known_hosts: config.join("known_hosts"),
            downloads: files.join("Phosphor"),
            files,
        }
    }
}

/// Load the dialing directory. A file that cannot be parsed is moved aside
/// rather than overwritten, and the reason returned for display.
pub fn load_directory(path: &std::path::Path) -> (Directory, Option<String>) {
    match Directory::load(path) {
        Ok(d) => (d, None),
        Err(e) => {
            let backup = path.with_extension(format!("toml.bad-{}", dialer::now()));
            let moved = std::fs::rename(path, &backup).is_ok();
            let mut msg = format!("Could not read the dialing directory: {e:#}.");
            if moved {
                msg.push_str(&format!(" It was moved to {}.", backup.display()));
            }
            (Directory::default(), Some(msg))
        }
    }
}

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    _context: softbuffer::Context<Rc<Window>>,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    gfx: Option<Gfx>,
    renderer: Renderer,
    paths: Paths,
    directory: Directory,
    dialer: DialerUi,
    dialer_screen: Screen,
    status: Screen,
    /// The terminal with a dialog drawn over it.
    overlay: Screen,
    upload_prompt: Option<UploadPrompt>,
    /// Direction of the transfer in progress, for reporting its outcome.
    transfer_direction: Option<Direction>,
    session: Option<Session>,
    session_id: u64,
    /// Directory index of the entry the session was dialed from.
    session_index: Option<usize>,
    /// Lines scrolled back from the live screen; 0 when live.
    scroll: usize,
    mods: ModifiersState,
    tick: u64,
    next_tick: Instant,
    notice: Option<(String, Instant)>,
    startup: Option<Entry>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>, paths: Paths, startup: Option<Entry>) -> App {
        let (mut directory, load_error) = load_directory(&paths.directory);
        directory.sort();
        let mut dialer = DialerUi::new(paths.files.clone());
        if let Some(msg) = load_error {
            dialer.show_message(msg);
        }
        App {
            proxy,
            gfx: None,
            renderer: Renderer::new(Font::vga_8x16(), COLS, GRID_ROWS),
            paths,
            directory,
            dialer,
            dialer_screen: Screen::new(dialer_ui::COLS, dialer_ui::ROWS, 0),
            status: Screen::new(COLS, 1, 0),
            overlay: Screen::new(COLS, ROWS, 0),
            upload_prompt: None,
            transfer_direction: None,
            session: None,
            session_id: 0,
            session_index: None,
            scroll: 0,
            mods: ModifiersState::empty(),
            tick: 0,
            next_tick: Instant::now() + TICK,
            notice: None,
            startup,
        }
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let (nw, nh) = (self.renderer.native_width() as u32, self.renderer.native_height() as u32);
        let attrs = Window::default_attributes()
            .with_title("Phosphor")
            .with_inner_size(PhysicalSize::new(nw * 2, nh * 2))
            .with_min_inner_size(PhysicalSize::new(nw, nh));
        let window = Rc::new(event_loop.create_window(attrs)?);
        let context = softbuffer::Context::new(window.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;
        self.gfx = Some(Gfx { window, surface, _context: context });
        Ok(())
    }

    fn request_redraw(&self) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn set_title(&self, title: &str) {
        if let Some(g) = &self.gfx {
            g.window.set_title(title);
        }
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), Instant::now() + NOTICE_TIME));
    }

    fn save_directory(&mut self) {
        if let Err(e) = self.directory.save(&self.paths.directory) {
            self.dialer.show_message(format!("Could not save the dialing directory: {e:#}"));
        }
    }

    fn connect(&mut self, entry: Entry, index: Option<usize>) {
        let config = SessionConfig {
            scrollback: SCROLLBACK_LINES,
            known_hosts: self.paths.known_hosts.clone(),
            download_dir: self.paths.downloads.clone(),
        };
        let session = match Session::new(entry, COLS, ROWS, config) {
            Ok(s) => s,
            Err(e) => return self.dialer.show_message(format!("{e:#}")),
        };
        self.session_id += 1;
        let id = self.session_id;
        let proxy = self.proxy.clone();
        session.connect(move |ev| {
            // Fails only when the event loop has exited.
            let _ = proxy.send_event(UserEvent::Net(id, ev));
        });
        self.set_title(&format!("Phosphor \u{2014} {}", session.entry.name));
        self.session = Some(session);
        self.session_index = index;
        self.scroll = 0;
    }

    fn hang_up(&mut self) {
        self.session = None;
        self.session_index = None;
        self.scroll = 0;
        self.upload_prompt = None;
        self.transfer_direction = None;
        self.set_title("Phosphor");
    }

    fn on_net(&mut self, id: u64, ev: NetEvent) {
        let Some(session) = self.session.as_mut().filter(|_| id == self.session_id) else {
            // A connection that finished after its session was abandoned.
            if let NetEvent::Connected(writer) = ev {
                writer.shutdown();
            }
            return;
        };
        let connected = matches!(ev, NetEvent::Connected(_));
        session.handle(ev);
        if connected && let Some(e) = self.session_index.and_then(|i| self.directory.entries.get_mut(i)) {
            e.calls += 1;
            e.last_connected = Some(dialer::now());
            self.save_directory();
        }
        self.session_events();
        self.request_redraw();
    }

    fn session_events(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        for ev in session.take_events() {
            match ev {
                SessionEvent::Term(TermEvent::Bell) => {
                    if let Some(g) = self.gfx.as_ref().filter(|g| !g.window.has_focus()) {
                        g.window.request_user_attention(Some(UserAttentionType::Informational));
                    }
                }
                SessionEvent::Term(TermEvent::Title(t)) => self.set_title(&format!("Phosphor \u{2014} {t}")),
                SessionEvent::Notice(msg) => self.notify(msg),
                SessionEvent::UploadRequested => {
                    self.scroll = 0;
                    self.upload_prompt = Some(UploadPrompt::new(true));
                    self.attention();
                }
                SessionEvent::TransferStarted => {
                    self.scroll = 0;
                    self.upload_prompt = None;
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

    /// Ask for the user's attention if the window is in the background.
    fn attention(&self) {
        if let Some(g) = self.gfx.as_ref().filter(|g| !g.window.has_focus()) {
            g.window.request_user_attention(Some(UserAttentionType::Informational));
        }
    }

    fn describe_outcome(&self, outcome: &Outcome) -> String {
        match (outcome, self.transfer_direction) {
            (Outcome::Completed(files), Some(Direction::Download)) => match files.as_slice() {
                [] => "Download finished; no files were sent".into(),
                [one] => format!("Downloaded {}", one.display()),
                many => format!("Downloaded {} files to {}", many.len(), self.paths.downloads.display()),
            },
            (Outcome::Completed(files), _) => match files.as_slice() {
                [one] => format!("Uploaded {}", one.file_name().unwrap_or_default().to_string_lossy()),
                many => format!("Uploaded {} files", many.len()),
            },
            (Outcome::Cancelled, _) => "Transfer cancelled".into(),
            (Outcome::Failed(why), _) => format!("Transfer failed: {why}"),
        }
    }

    fn start_upload(&mut self, paths: Vec<PathBuf>) {
        let Some(session) = self.session.as_mut() else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_upload(paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events();
    }

    fn on_key(&mut self, event_loop: &ActiveEventLoop, event: &KeyEvent) {
        let mods = Mods::from(self.mods);
        if mods.hotkey()
            && let PhysicalKey::Code(code) = event.physical_key
            && self.hotkey(event_loop, code)
        {
            return self.request_redraw();
        }
        let Some(key) = input::translate(event) else { return };
        if self.upload_prompt.is_some() {
            self.prompt_key(key, mods);
        } else if self.session.as_ref().is_some_and(|s| s.transfer().is_some()) {
            if key == Key::Escape
                && let Some(s) = self.session.as_mut()
            {
                s.cancel_transfer();
                self.session_events();
            }
        } else if self.session.is_some() {
            self.terminal_key(key, mods);
        } else {
            match self.dialer.handle_key(key, mods, &mut self.directory) {
                Action::None => {}
                Action::Connect(i) => {
                    let e = self.directory.entries[i].clone();
                    self.connect(e, Some(i));
                }
                Action::QuickConnect(e) => self.connect(e, None),
                Action::Changed => self.save_directory(),
                Action::Quit => event_loop.exit(),
            }
        }
        self.request_redraw();
    }

    /// Client hotkeys (Alt, or Cmd on macOS). Returns true if handled.
    fn hotkey(&mut self, event_loop: &ActiveEventLoop, code: KeyCode) -> bool {
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some(g) = &self.gfx {
                    let fs = g.window.fullscreen().is_none().then_some(Fullscreen::Borderless(None));
                    g.window.set_fullscreen(fs);
                }
                return true;
            }
            KeyCode::KeyX => {
                event_loop.exit();
                return true;
            }
            _ => {}
        }
        let Some(session) = self.session.as_mut() else { return false };
        match code {
            KeyCode::KeyH => self.hang_up(),
            KeyCode::KeyB => self.scroll = self.scroll.max(1).max(ROWS.min(session.term.screen.scrollback_len())),
            KeyCode::KeyI => {
                let s = &mut session.term.screen;
                s.ice_colors = !s.ice_colors;
                let msg = if s.ice_colors { "iCE colors on" } else { "iCE colors off (blinking)" };
                self.notify(msg);
            }
            KeyCode::KeyE => {
                let t = &mut session.term;
                t.encoding = match t.encoding {
                    Encoding::Cp437 => Encoding::Utf8,
                    Encoding::Utf8 => Encoding::Cp437,
                };
                let msg = format!("Encoding: {}", t.encoding.name());
                self.notify(msg);
            }
            KeyCode::KeyC => self.toggle_capture(),
            KeyCode::KeyV => self.paste(),
            // During an upload, the prompt adds files to the batch.
            KeyCode::KeyU
                if session.state == State::Connected
                    && session.transfer().is_none_or(|t| t.progress().direction == Direction::Upload) =>
            {
                let requested = session.upload_requested();
                self.upload_prompt = Some(UploadPrompt::new(requested));
            }
            _ => return false,
        }
        true
    }

    fn prompt_key(&mut self, key: Key, mods: Mods) {
        let Some(prompt) = self.upload_prompt.as_mut() else { return };
        match key {
            Key::Escape => {
                self.upload_prompt = None;
                // A BBS waiting for the upload is told to give up.
                if let Some(s) = self.session.as_mut() {
                    s.decline_upload();
                }
            }
            Key::Enter => {
                let paths = prompt.paths();
                if paths.is_empty() {
                    return;
                }
                self.upload_prompt = None;
                self.start_upload(paths);
            }
            _ => {
                prompt.field.handle_key(key, mods);
            }
        }
    }

    fn toggle_capture(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        if session.capture_path().is_some() {
            session.stop_capture();
            return self.notify("Capture stopped");
        }
        let now = dialer::now();
        let name: String = session.entry.name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
        let path = self.paths.captures.join(format!("{name}-{}-{}.ans", format_date(now), now % 86_400));
        let msg = match session.start_capture(path.clone()) {
            Ok(()) => format!("Capturing to {}", path.display()),
            Err(e) => format!("Capture failed: {e:#}"),
        };
        self.notify(msg);
    }

    fn paste(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        let text = match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(t) => t,
            Err(e) => return self.notify(format!("Paste failed: {e}")),
        };
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let bytes = match session.term.encoding {
            Encoding::Cp437 => cp437::encode(&text),
            Encoding::Utf8 => text.into_bytes(),
        };
        session.send(&bytes);
    }

    fn terminal_key(&mut self, key: Key, mods: Mods) {
        let Some(session) = self.session.as_mut() else { return };
        if session.state == State::Closed {
            return self.hang_up();
        }
        let max = session.term.screen.scrollback_len();
        if self.scroll > 0 {
            self.scroll = match key {
                Key::Up => self.scroll + 1,
                Key::Down => self.scroll - 1,
                Key::PageUp => self.scroll + ROWS,
                Key::PageDown => self.scroll.saturating_sub(ROWS),
                Key::Home => max,
                _ => 0,
            }
            .min(max);
            return;
        }
        match key {
            Key::PageUp if mods.shift => self.scroll = ROWS.min(max),
            Key::Insert if mods.shift => self.paste(),
            _ => {
                let bytes = input::encode(key, mods, session.term.encoding);
                if !bytes.is_empty() {
                    session.send(&bytes);
                }
            }
        }
    }

    fn on_wheel(&mut self, lines: isize) {
        if let Some(s) = &self.session {
            let max = s.term.screen.scrollback_len() as isize;
            self.scroll = (self.scroll as isize + lines).clamp(0, max) as usize;
            self.request_redraw();
        }
    }

    fn draw_status(&mut self) {
        let (fg, bg) = (ui::WHITE, ui::BLUE);
        let s = &mut self.status;
        s.fill_rect(0, 0, COLS, 1, Cell::new(b' ', fg, bg));
        let notice = self.notice.as_ref().filter(|(_, until)| Instant::now() < *until).map(|(m, _)| m.clone());
        let Some(session) = &self.session else {
            let help = notice.unwrap_or_else(|| self.dialer.status_help().to_string());
            return ui::text(s, 0, 0, COLS, &format!(" {help}"), fg, bg);
        };
        let screen = &session.term.screen;
        if self.scroll > 0 {
            let line = format!(
                " SCROLLBACK {}/{}  \u{2191}\u{2193} PgUp PgDn Home  Any other key returns",
                self.scroll,
                screen.scrollback_len()
            );
            return ui::text(s, 0, 0, COLS, &line, ui::BLACK, ui::CYAN);
        }
        let e = &session.entry;
        if session.transfer().is_some() {
            let line = format!(" {} \u{2502} ZMODEM transfer in progress \u{2502} Esc cancels", truncate(&e.name, 30));
            return ui::text(s, 0, 0, COLS, &line, fg, bg);
        }
        let line = match session.state {
            State::Connecting => format!(" Connecting to {}...  {HOTKEY}-H cancel", e.address()),
            State::Closed => match &session.error {
                Some(err) => format!(" Connection failed: {err} \u{2502} Press any key"),
                None => " Disconnected \u{2502} Press any key to return to the dialer".to_string(),
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
                let right = notice.unwrap_or_else(|| format!("{HOTKEY}: H Hangup B Back U Upload I iCE C Cap X Exit"));
                format!(
                    " {} \u{2502} {} {} \u{2502} {:02}:{:02}:{:02}{} \u{2502} {right}",
                    truncate(&e.name, 20),
                    e.protocol.name(),
                    session.term.encoding.name(),
                    t / 3600,
                    t / 60 % 60,
                    t % 60,
                    flags
                )
            }
        };
        ui::text(s, 0, 0, COLS, &line, fg, bg);
    }

    fn redraw(&mut self) {
        self.draw_status();
        let dialer_cursor = match &self.session {
            None => self.dialer.draw(&mut self.dialer_screen, &self.directory),
            Some(_) => None,
        };
        let Some(gfx) = self.gfx.as_mut() else { return };
        let size = gfx.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };

        // Dialogs over the terminal are drawn on a copy of its screen.
        let mut overlay_cursor = None;
        let overlay = match &self.session {
            Some(session) if self.upload_prompt.is_some() || session.transfer().is_some() => {
                let sc = &session.term.screen;
                for y in 0..ROWS.min(sc.rows()) {
                    for (x, cell) in sc.row(y).iter().enumerate() {
                        if let Some(c) = self.overlay.cell_mut(x, y) {
                            *c = *cell;
                        }
                    }
                }
                if let Some(prompt) = &self.upload_prompt {
                    overlay_cursor = Some(prompt.draw(&mut self.overlay));
                } else if let Some(t) = session.transfer() {
                    transfer_ui::draw_progress(&mut self.overlay, t.progress());
                }
                true
            }
            _ => false,
        };

        let frame = match &self.session {
            Some(session) if overlay => {
                let mut rows: Vec<&[Cell]> = (0..ROWS).map(|y| self.overlay.row(y)).collect();
                rows.push(self.status.row(0));
                Frame { rows, cursor: overlay_cursor, ice: session.term.screen.ice_colors }
            }
            Some(session) => {
                let sc = &session.term.screen;
                let sb = sc.scrollback_len();
                let off = self.scroll.min(sb);
                let top = sb - off;
                let mut rows: Vec<&[Cell]> =
                    (top..top + ROWS).map(|i| if i < sb { sc.scrollback_line(i) } else { sc.row(i - sb) }).collect();
                rows.push(self.status.row(0));
                let cursor = (off == 0 && sc.cursor_visible && session.state == State::Connected).then(|| sc.cursor());
                Frame { rows, cursor, ice: sc.ice_colors }
            }
            None => {
                let mut rows: Vec<&[Cell]> = (0..dialer_ui::ROWS).map(|y| self.dialer_screen.row(y)).collect();
                rows.push(self.status.row(0));
                Frame { rows, cursor: dialer_cursor, ice: true }
            }
        };
        self.renderer.draw(&frame, (self.tick / 2).is_multiple_of(2), self.tick.is_multiple_of(2));

        if let Err(e) = gfx.surface.resize(w, h) {
            return log::error!("resizing surface: {e}");
        }
        match gfx.surface.buffer_mut() {
            Ok(mut buf) => {
                self.renderer.blit(&mut buf, w.get() as usize, h.get() as usize);
                if let Err(e) = buf.present() {
                    log::error!("presenting frame: {e}");
                }
            }
            Err(e) => log::error!("getting frame buffer: {e}"),
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        if let Err(e) = self.create_window(event_loop) {
            log::error!("creating window: {e:#}");
            return event_loop.exit();
        }
        if let Some(e) = self.startup.take() {
            let index =
                self.directory.entries.iter().position(|x| x.host.eq_ignore_ascii_case(&e.host) && x.port == e.port);
            match index {
                Some(i) => {
                    let e = self.directory.entries[i].clone();
                    self.connect(e, Some(i));
                }
                None => self.connect(e, None),
            }
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Net(id, ev) => self.on_net(id, ev),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => self.request_redraw(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.on_key(event_loop, &event)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as isize,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 16.0) as isize,
                };
                self.on_wheel(lines);
            }
            WindowEvent::DroppedFile(path) if self.session.is_none() => {
                // Dropping a directory file onto the dialer imports it.
                match dialer::import(&path) {
                    Ok(entries) => {
                        let added = self.directory.merge(entries);
                        self.save_directory();
                        self.dialer.show_message(format!("Imported {added} new entries from {}", path.display()));
                    }
                    Err(e) => self.dialer.show_message(format!("Import failed: {e:#}")),
                }
                self.request_redraw();
            }
            // Dropping files onto a connected session uploads them.
            WindowEvent::DroppedFile(path) => {
                self.upload_prompt = None;
                self.start_upload(vec![path]);
                self.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_tick {
            self.tick += 1;
            self.next_tick = now + TICK;
            if let Some(s) = self.session.as_mut() {
                s.tick();
                self.session_events();
            }
            self.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Headless import used by the command line.
pub fn import_into_directory(paths: &Paths, file: &std::path::Path) -> Result<(usize, usize)> {
    let mut dir = Directory::load(&paths.directory)?;
    let entries = dialer::import(file)?;
    let total = entries.len();
    let added = dir.merge(entries);
    dir.save(&paths.directory).context("saving dialing directory")?;
    Ok((added, total))
}
