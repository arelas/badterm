//! The windowed application: event loop handling, mode switching between the
//! dialer and a live session, hotkeys, scrollback viewing, file transfers and
//! presentation.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use badterm_core::dialer::{self, CrtMode, Directory, Entry};
use badterm_core::export::{self, AnsiOptions};
use badterm_core::screen::{Cell, Screen};
use badterm_core::session::{NetEvent, Session, SessionConfig, SessionEvent, State};
use badterm_core::transfer::{Direction, Outcome};
use badterm_core::{Encoding, TermEvent};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Fullscreen, UserAttentionType, Window, WindowId};

use crate::bezel::{self, Bezel};
use crate::input::{self, Key, Mods};
use crate::render::{Display, Font, Frame, Renderer, Scaling};
use crate::secrets::{self, Keychain, Store};
use crate::selection::Selection;
use crate::settings::{BellMode, Folders, Music, ScaleMode, Settings};
use crate::sound::Player;
use crate::ui::art::{self as art_ui, ArtAction, ArtView};
use crate::ui::dialer::{self as dialer_ui, Action, DialerUi};
use crate::ui::files::{FilePicker, PickerAction};
use crate::ui::login::{LoginAction, LoginPrompt};
use crate::ui::save::{SaveAction, SaveFormat, SavePrompt};
use crate::ui::settings::{self as settings_ui, SettingsUi};
use crate::ui::theme::{self, ThemeFile};
use crate::ui::transfer::{self as transfer_ui, DownloadAction, DownloadPrompt, Proto, UploadAction, UploadPrompt};
use crate::ui::{self, HOTKEY, expand_home, format_date};

mod call;
mod draw;
mod keys;
mod mouse;
mod prefs;
mod script;
mod shape;
mod tabs;

use tabs::Tab;

const COLS: usize = 80;
const ROWS: usize = 25;
/// Terminal rows plus the status bar.
const GRID_ROWS: usize = ROWS + 1;
const SCROLLBACK_LINES: usize = 10_000;
/// Cursor blink half-period; text blinks at half this rate.
const TICK: Duration = Duration::from_millis(267);
const NOTICE_TIME: Duration = Duration::from_secs(4);
/// The news of a newer version stays a while longer.
const UPDATE_NOTICE_TIME: Duration = Duration::from_secs(20);
/// Between redials of a busy or unreachable BBS.
const REDIAL_DELAY: Duration = Duration::from_secs(15);
const MAX_REDIALS: u32 = 10;
/// How often the screen updates while baud rate emulation holds text back.
const PACE_FRAME: Duration = Duration::from_millis(16);
/// The bell rings at most this often.
const BELL_GAP: Duration = Duration::from_millis(250);
/// How long the screen flashes for a visual bell.
const FLASH: Duration = Duration::from_millis(120);

pub enum UserEvent {
    /// A network event for the session with this id.
    Net(u64, NetEvent),
    /// A telnet:// (or rlogin://, ...) link opened with BadTerm while it
    /// runs; macOS sends these to the running app.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    OpenUrl(String),
    /// The update check found this newer version.
    UpdateAvailable(String),
    /// The script of the call with this id has something to do.
    Script(u64),
}

/// A busy call waiting to be dialed again.
struct Redial {
    entry: Entry,
    index: Option<usize>,
    /// Which redial this will be: 1 for the first.
    attempt: u32,
    at: Instant,
}

pub struct Paths {
    pub directory: PathBuf,
    /// Display preferences.
    pub settings: PathBuf,
    pub captures: PathBuf,
    /// Pinned SSH host keys.
    pub known_hosts: PathBuf,
    /// Where ZMODEM downloads are saved.
    pub downloads: PathBuf,
    /// Default location offered for import/export.
    pub files: PathBuf,
    /// Where default SSH keys are looked for.
    pub ssh_keys: Option<PathBuf>,
    /// Color themes (`.toml`), shared or saved from the settings.
    pub themes: PathBuf,
    /// The user's own monitor bezels (`.png`).
    pub bezels: PathBuf,
    /// Files BBSes store here (SyncTERM's cache), a folder per BBS.
    pub bbs_cache: PathBuf,
    /// Scripts (`.rhai`).
    pub scripts: PathBuf,
    /// Font files for directory entries.
    pub fonts: PathBuf,
}

impl Paths {
    pub fn new() -> Paths {
        let proj = directories::ProjectDirs::from("dev", "Bad Cluster", "BadTerm");
        let (config, data, cache) = match &proj {
            Some(p) => (p.config_dir().to_path_buf(), p.data_dir().to_path_buf(), p.cache_dir().to_path_buf()),
            None => (PathBuf::from("."), PathBuf::from("."), PathBuf::from(".")),
        };
        let files = directories::UserDirs::new()
            .map(|u| u.download_dir().unwrap_or(u.home_dir()).to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        Paths {
            directory: config.join("dialer.toml"),
            settings: config.join("settings.toml"),
            themes: config.join("themes"),
            bezels: config.join("bezels"),
            scripts: config.join("scripts"),
            fonts: config.join("fonts"),
            bbs_cache: cache.join("bbs"),
            captures: data.join("captures"),
            known_hosts: config.join("known_hosts"),
            downloads: files.join("BadTerm"),
            files,
            ssh_keys: directories::BaseDirs::new().map(|b| b.home_dir().join(".ssh")),
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
    /// Whether the dialing directory is showing (Alt-D). It opens over a live
    /// session without disturbing it.
    dialer_open: bool,
    /// An entry chosen while connected, waiting for the hang-up confirmation.
    pending_dial: Option<(Entry, Option<usize>)>,
    /// What shows when there is no session and the dialer is closed.
    idle_screen: Screen,
    /// The settings screen (Alt-O), over everything else while open.
    settings_ui: SettingsUi,
    settings_open: bool,
    /// The help screen (Alt-Z), over everything else while open.
    help_open: bool,
    /// "Exit BadTerm?" is showing (Alt-X).
    confirm_exit: bool,
    /// The splash screen shows until then (or a key).
    splash_until: Option<Instant>,
    /// A newer version, if the update check found one.
    update: Option<String>,
    /// The ANSI viewer (Alt-F): choosing a file, then the art.
    art_picker: Option<FilePicker>,
    /// The picker is choosing a script to run, not art to view.
    picking_script: bool,
    art: Option<ArtView>,
    art_dir: PathBuf,
    /// A file to view at start (given on the command line).
    view_at_start: Option<PathBuf>,
    /// The monitor bezel around the screen, and its drawing for the
    /// window's size.
    bezel: Option<Bezel>,
    bezel_name: String,
    bezel_frame: Option<((usize, usize), bezel::Rendered)>,
    /// Whether this run has checked for updates.
    update_checked: bool,
    /// Where the settings and help screens are drawn.
    page_screen: Screen,
    status: Screen,
    /// The terminal with a dialog drawn over it.
    overlay: Screen,
    /// The protocols last chosen for an upload and a download, for the next.
    upload_protocol: usize,
    download_protocol: usize,
    /// Terminal text selected with the mouse.
    selection: Option<Selection>,
    /// The left button is down, extending `selection`.
    dragging: bool,
    /// Mouse position in window pixels.
    mouse: (f64, f64),
    /// For the BBS's mouse reports: the button held, and where the mouse
    /// was last reported (cell and pixel), so moves within it aren't sent.
    mouse_held: Option<u8>,
    mouse_reported: Option<((usize, usize), (usize, usize))>,
    settings: Settings,
    /// Plays ANSI music.
    player: Player,
    /// Where the last frame was drawn in the window: (x, y, w, h).
    view_rect: (usize, usize, usize, usize),
    /// "Fit, keep shape": the window follows the picture's shape.
    shape: shape::WindowShape,
    secrets: Rc<dyn Store>,
    /// False once the keychain has failed; passwords then stay in the file.
    keychain_ok: bool,
    /// The calls, each in a tab, and which of them is on screen.
    tabs: Vec<Tab>,
    active: usize,
    /// The id the last call was given (each call's network events and
    /// script come with its own).
    last_id: u64,
    /// The dialer was opened with Alt-N: what is dialed goes in a new tab.
    new_tab_dial: bool,
    mods: ModifiersState,
    tick: u64,
    next_tick: Instant,
    notice: Option<(String, Instant)>,
    startup: Option<Entry>,
    /// When the bell last rang, and until when the screen flashes for it.
    last_bell: Option<Instant>,
    flash_until: Option<Instant>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>, paths: Paths, startup: Option<Entry>, view: Option<PathBuf>) -> App {
        let (mut directory, mut load_error) = load_directory(&paths.directory);
        directory.sort();
        // New default entries (the first time, the whole starter directory).
        // Not over a file that could not be read: that was moved aside.
        if load_error.is_none()
            && directory.offer_defaults()
            && let Err(e) = directory.save(&paths.directory)
        {
            load_error = Some(format!("Could not save the dialing directory: {e:#}"));
        }
        let settings = Settings::load(&paths.settings);
        crate::hotkeys::set(&settings.hotkeys);
        ui::theme::set_current(settings.colors);
        let secrets: Rc<dyn Store> = Rc::new(Keychain);
        let mut dialer = DialerUi::new(paths.files.clone(), secrets.clone());
        dialer.scripts_dir = paths.scripts.clone();
        // The app starts on the idle screen; the dialer opens only to show a
        // problem with the directory file.
        let dialer_open = load_error.is_some();
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
            dialer_open,
            pending_dial: None,
            idle_screen: idle_screen(None),
            settings_ui: SettingsUi::default(),
            settings_open: false,
            help_open: false,
            confirm_exit: false,
            splash_until: None,
            update: None,
            art_picker: None,
            picking_script: false,
            art: None,
            art_dir: PathBuf::new(),
            view_at_start: view,
            bezel: None,
            bezel_name: String::new(),
            bezel_frame: None,
            update_checked: false,
            page_screen: Screen::new(settings_ui::COLS, settings_ui::ROWS, 0),
            status: Screen::new(COLS, 1, 0),
            overlay: Screen::new(COLS, ROWS, 0),
            upload_protocol: 0,
            download_protocol: 0,
            selection: None,
            dragging: false,
            mouse: (0.0, 0.0),
            mouse_held: None,
            mouse_reported: None,
            view_rect: (0, 0, 1, 1),
            shape: shape::WindowShape::default(),
            settings,
            player: Player::default(),
            secrets,
            keychain_ok: true,
            tabs: Vec::new(),
            active: 0,
            last_id: 0,
            new_tab_dial: false,
            mods: ModifiersState::empty(),
            tick: 0,
            next_tick: Instant::now() + TICK,
            notice: None,
            startup,
            last_bell: None,
            flash_until: None,
        }
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let (nw, nh) = (self.renderer.native_width() as u32, self.renderer.native_height() as u32);
        let attrs = Window::default_attributes()
            .with_title("BadTerm")
            .with_inner_size(PhysicalSize::new(nw * 2, nh * 2))
            .with_maximized(self.settings.window.maximized)
            .with_min_inner_size(PhysicalSize::new(nw, nh))
            .with_window_icon(window_icon());
        // The Wayland app ID and X11 class, which desktops match to
        // badterm.desktop for the launcher icon.
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            use winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};
            let attrs = WindowAttributesExtWayland::with_name(attrs, "badterm", "badterm");
            WindowAttributesExtX11::with_name(attrs, "badterm", "badterm")
        };
        // The size it was last left at (which it can't be smaller than
        // its minimum).
        let attrs = match (self.settings.window.width, self.settings.window.height) {
            (Some(w), Some(h)) if w >= 100 && h >= 100 => {
                attrs.with_inner_size(winit::dpi::LogicalSize::new(w.min(16384), h.min(16384)))
            }
            _ => attrs,
        };
        // Where it was, if that's still on a screen (one may be unplugged).
        let attrs = match (self.settings.window.x, self.settings.window.y) {
            (Some(x), Some(y))
                if event_loop.available_monitors().any(|m| {
                    let (p, s) = (m.position(), m.size());
                    x >= p.x && y >= p.y && x < p.x + s.width as i32 - 40 && y < p.y + s.height as i32 - 40
                }) =>
            {
                attrs.with_position(winit::dpi::PhysicalPosition::new(x, y))
            }
            _ => attrs,
        };
        let window = Rc::new(event_loop.create_window(attrs)?);
        self.shape.min = (nw, nh);
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

    /// The call on screen, if there is one.
    fn tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    fn tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    fn session(&self) -> Option<&Session> {
        self.tab().map(|t| &t.session)
    }

    /// The tab of the call with this id.
    fn tab_of(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// How far back the call on screen is scrolled.
    fn scroll(&self) -> usize {
        self.tab().map_or(0, |t| t.scroll)
    }

    /// Whether any call is connected.
    fn in_call(&self) -> bool {
        self.tabs.iter().any(|t| t.session.state == State::Connected)
    }

    fn save_directory(&mut self) {
        if self.keychain_ok
            && let Err(e) = secrets::migrate(&mut self.directory, &*self.secrets)
        {
            self.keychain_ok = false;
            log::warn!("keychain unavailable: {e}");
            self.notify("No keychain available; passwords are kept in the directory file");
        }
        if let Err(e) = self.directory.save(&self.paths.directory) {
            self.dialer.show_message(format!("Could not save the dialing directory: {e:#}"));
        }
    }

    /// Ask for the user's attention if the window is in the background.
    fn attention(&self) {
        if let Some(g) = self.gfx.as_ref().filter(|g| !g.window.has_focus()) {
            g.window.request_user_attention(Some(UserAttentionType::Informational));
        }
    }
}

impl App {
    /// Remember the window's size (unless it's maximized or full screen,
    /// when the size to come back to is the one before) and whether it's
    /// maximized, to save at exit.
    fn note_window_size(&mut self) {
        let Some(g) = &self.gfx else { return };
        let w = &mut self.settings.window;
        if g.window.fullscreen().is_some() {
            return;
        }
        w.maximized = g.window.is_maximized();
        if !w.maximized && g.window.is_minimized() != Some(true) {
            let size = g.window.inner_size().to_logical::<f64>(g.window.scale_factor());
            if size.width >= 100.0 && size.height >= 100.0 {
                (w.width, w.height) = (Some(size.width.round() as u32), Some(size.height.round() as u32));
            }
            // Wayland doesn't say where a window is.
            if let Ok(p) = g.window.outer_position() {
                (w.x, w.y) = (Some(p.x), Some(p.y));
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    /// Save the window's size for next time.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.note_window_size();
        let saved = Settings::load(&self.paths.settings);
        if saved.window != self.settings.window {
            let mut s = saved;
            s.window = self.settings.window;
            if let Err(e) = s.save(&self.paths.settings) {
                log::warn!("saving the window's size: {e:#}");
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        if let Err(e) = self.create_window(event_loop) {
            log::error!("creating window: {e:#}");
            return event_loop.exit();
        }
        self.load_bezel();
        // Not when started to call a BBS or show a file: get on with it.
        if self.settings.splash && self.startup.is_none() && self.view_at_start.is_none() {
            self.splash_until = Some(Instant::now() + ui::splash::TIME);
        }
        if let Some(path) = self.view_at_start.take() {
            self.view_art(&path);
        }
        if let Some(e) = self.startup.take() {
            self.dial_address(e);
        }
        self.check_for_updates();
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Net(id, ev) => self.on_net(id, ev),
            UserEvent::Script(id) => self.poll_script(id),
            UserEvent::UpdateAvailable(version) => {
                let msg = format!("BadTerm {version} is out: {}", badterm_core::update::DOWNLOAD_PAGE);
                self.notice = Some((msg, Instant::now() + UPDATE_NOTICE_TIME));
                self.update = Some(version);
                self.idle_screen = idle_screen(self.update.as_deref());
                self.request_redraw();
            }
            UserEvent::OpenUrl(url) => match Entry::from_address(&url) {
                // Before the window exists: the link that launched the app.
                Some(e) if self.gfx.is_none() => self.startup = Some(e),
                Some(e) => {
                    // A link that launched the app can come just after the
                    // window: the call matters more than the splash.
                    self.splash_until = None;
                    if let Some(g) = &self.gfx {
                        g.window.focus_window();
                    }
                    self.dial_address(e);
                    self.request_redraw();
                }
                None => self.notify(format!("Not a BBS address: {url}")),
            },
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            // Closing the window in a call asks first, as Alt-X does.
            WindowEvent::CloseRequested => {
                if self.in_call() && !self.confirm_exit {
                    self.confirm_exit = true;
                    self.request_redraw();
                } else {
                    event_loop.exit();
                }
            }
            WindowEvent::Moved(_) => self.note_window_size(),
            WindowEvent::Resized(size) => {
                self.shape.resized(size);
                self.note_window_size();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.on_key(event_loop, &event)
            }
            WindowEvent::CursorMoved { position, .. } => self.on_mouse_move(position.x, position.y),
            WindowEvent::MouseInput { state, button, .. } => self.on_mouse_button(button, state),
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as isize,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 16.0) as isize,
                };
                self.on_wheel(lines);
            }
            // Art dropped while not in a call opens the viewer.
            WindowEvent::DroppedFile(path)
                if self.session().is_none_or(|s| s.state != State::Connected) && !self.dialer_open && is_art(&path) =>
            {
                self.view_art(&path);
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) if self.tabs.is_empty() || self.dialer_open => {
                // Dropping a directory file onto the dialer (or the idle
                // screen) imports it, and shows the dialer with the result.
                self.dialer_open = true;
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
                if let Some(t) = self.tab_mut() {
                    t.upload_prompt = None;
                }
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
            // Every call, on screen or not.
            for t in 0..self.tabs.len() {
                self.tabs[t].session.tick();
                self.session_events(t);
                if self.tabs[t].redial.as_ref().is_some_and(|r| now >= r.at) {
                    self.redial_now(t);
                }
            }
            self.request_redraw();
        }
        // Baud rate emulation shows text a few characters at a time.
        let mut wake = self.next_tick;
        if let Some(until) = self.flash_until {
            if now >= until {
                self.flash_until = None;
                self.request_redraw();
            } else {
                wake = wake.min(until);
            }
        }
        if let Some(until) = self.splash_until {
            if now >= until {
                self.splash_until = None;
                self.request_redraw();
            } else {
                wake = wake.min(until);
            }
        }
        if let Some(at) = self.shape.due() {
            if now >= at {
                self.snap_window_shape();
            } else {
                wake = wake.min(at);
            }
        }
        for t in 0..self.tabs.len() {
            if !self.tabs[t].macro_steps.is_empty() {
                self.run_macro(t);
                let tab = &self.tabs[t];
                if !tab.macro_steps.is_empty() {
                    wake = wake.min(tab.macro_at);
                }
            }
            // Sound reports and waits for sounds (SyncTERM's audio).
            if let Some(at) = self.tabs[t].session.next_tick() {
                if now >= at {
                    self.tabs[t].session.tick();
                    self.session_events(t);
                    self.request_redraw();
                } else {
                    wake = wake.min(at);
                }
            }
            if self.tabs[t].session.pacing() {
                self.tabs[t].session.pump();
                self.session_events(t);
                if t == self.active {
                    self.request_redraw();
                }
                wake = wake.min(now + PACE_FRAME);
            }
            // What a script waits for may have come in any of the above.
            self.feed_script(t);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

/// The icon for the title bar and taskbar (Windows and X11; macOS takes the
/// app bundle's icon, Wayland the desktop file's).
fn window_icon() -> Option<winit::window::Icon> {
    let rgba = include_bytes!("../../assets/icon/badterm-64.rgba");
    winit::window::Icon::from_rgba(rgba.to_vec(), 64, 64).map_err(|e| log::warn!("window icon: {e}")).ok()
}

/// The terminal rows on view, `scroll` lines back from the live screen.
fn terminal_rows(session: &Session, scroll: usize) -> Vec<&[Cell]> {
    let sc = &session.term.screen;
    let sb = sc.scrollback_len();
    let top = sb - scroll.min(sb);
    (top..top + sc.rows()).map(|i| if i < sb { sc.scrollback_line(i) } else { sc.row(i - sb) }).collect()
}

/// A BBS name made safe for a file name.
fn file_stem(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect()
}

/// A date and second-of-day stamp for file names.
fn stamp(now: u64) -> String {
    format!("{}-{}", format_date(now), now % 86_400)
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn is_art(path: &std::path::Path) -> bool {
    let name = path.to_string_lossy().to_ascii_lowercase();
    art_ui::EXTENSIONS.iter().any(|x| name.ends_with(x))
}

/// Which keyboard macro a hotkey sends: Alt-1 is the first, Alt-0 the tenth.
fn macro_index(code: KeyCode) -> Option<usize> {
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    DIGITS.iter().position(|&d| d == code)
}

/// The screen shown with no session and the dialer closed: blank, with a
/// hint in the middle (the status bar repeats it).
pub(crate) fn idle_screen(update: Option<&str>) -> Screen {
    let t = ui::theme::current();
    let mut s = Screen::new(COLS, ROWS, 0);
    s.fill_rect(0, 0, COLS, ROWS, Cell::new(b' ', t.help.fg(), t.help.bg()));
    // Like a terminal program's start-up banner, at the top left.
    let version = format!(
        "BadTerm {} ({} {}), the ANSI-BBS client",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let lines = [
        (version, t.info),
        ("Copyright (C) 2026 Bad Cluster".to_string(), t.help),
        ("Free software under the GNU GPL, version 3 or later; no warranty.".to_string(), t.help),
        (badterm_core::update::DOWNLOAD_PAGE.to_string(), t.help),
        (String::new(), t.help),
        (
            format!(
                "{} Dialing directory   {} Settings   {} Help   {} Exit",
                crate::hotkeys::label("D"),
                crate::hotkeys::label("O"),
                crate::hotkeys::label("Z"),
                crate::hotkeys::label("X")
            ),
            t.help_key,
        ),
    ];
    for (i, (text, a)) in lines.iter().enumerate() {
        ui::label(&mut s, 0, i, text, a.fg(), a.bg());
    }
    if let Some(v) = update {
        let text = format!("BadTerm {v} is out: {}", badterm_core::update::DOWNLOAD_PAGE);
        ui::label(&mut s, 0, lines.len() + 1, &text, t.help_key.fg(), t.help_key.bg());
    }
    s
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
