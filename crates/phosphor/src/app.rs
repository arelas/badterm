//! The windowed application: event loop handling, mode switching between the
//! dialer and a live session, hotkeys, scrollback viewing, file transfers and
//! presentation.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use phosphor_core::dialer::{self, CrtMode, Directory, Entry};
use phosphor_core::export::{self, AnsiOptions};
use phosphor_core::screen::{Cell, Screen};
use phosphor_core::session::{NetEvent, Session, SessionConfig, SessionEvent, State};
use phosphor_core::transfer::{Direction, Outcome};
use phosphor_core::{Encoding, TermEvent, cp437};
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
use crate::settings::{Music, Settings};
use crate::sound::Player;
use crate::ui::art::{self as art_ui, ArtAction, ArtView};
use crate::ui::dialer::{self as dialer_ui, Action, DialerUi};
use crate::ui::files::{FilePicker, PickerAction};
use crate::ui::login::{LoginAction, LoginPrompt};
use crate::ui::save::{SaveFormat, SavePrompt};
use crate::ui::settings::{self as settings_ui, SettingsUi};
use crate::ui::theme::{self, ThemeFile};
use crate::ui::transfer::{self as transfer_ui, DownloadAction, DownloadPrompt, UploadAction, UploadPrompt};
use crate::ui::{self, HOTKEY, format_date};

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

pub enum UserEvent {
    /// A network event for the session with this id.
    Net(u64, NetEvent),
    /// A telnet:// (or rlogin://, ...) link opened with Phosphor while it
    /// runs; macOS sends these to the running app.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    OpenUrl(String),
    /// The update check found this newer version.
    UpdateAvailable(String),
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
            settings: config.join("settings.toml"),
            themes: config.join("themes"),
            bezels: config.join("bezels"),
            captures: data.join("captures"),
            known_hosts: config.join("known_hosts"),
            downloads: files.join("Phosphor"),
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
    /// A newer version, if the update check found one.
    update: Option<String>,
    /// The ANSI viewer (Alt-F): choosing a file, then the art.
    art_picker: Option<FilePicker>,
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
    upload_prompt: Option<UploadPrompt>,
    /// Alt-G: starting an XMODEM or YMODEM download.
    download_prompt: Option<DownloadPrompt>,
    /// Where the last upload came from, and its protocol, for the next.
    upload_dir: PathBuf,
    upload_protocol: usize,
    download_protocol: usize,
    /// Alt-S: where to save the scrollback.
    save_prompt: Option<SavePrompt>,
    /// Terminal text selected with the mouse.
    selection: Option<Selection>,
    /// The left button is down, extending `selection`.
    dragging: bool,
    /// Mouse position in window pixels.
    mouse: (f64, f64),
    settings: Settings,
    /// Plays ANSI music.
    player: Player,
    /// Where the last frame was drawn in the window: (x, y, w, h).
    view_rect: (usize, usize, usize, usize),
    /// The SSH login's question, while one is waiting.
    login: Option<LoginPrompt>,
    /// A typed password to remember once the login succeeds.
    remember: Option<String>,
    secrets: Rc<dyn Store>,
    /// False once the keychain has failed; passwords then stay in the file.
    keychain_ok: bool,
    /// Direction of the transfer in progress, for reporting its outcome.
    transfer_direction: Option<Direction>,
    session: Option<Session>,
    session_id: u64,
    /// Directory index of the entry the session was dialed from.
    session_index: Option<usize>,
    /// A keyboard macro being sent: what is left, and when to go on after
    /// a pause.
    macro_steps: std::collections::VecDeque<crate::macros::Step>,
    macro_at: Instant,
    /// How many times the current call has been redialed.
    redials: u32,
    redial: Option<Redial>,
    /// Lines scrolled back from the live screen; 0 when live.
    scroll: usize,
    mods: ModifiersState,
    tick: u64,
    next_tick: Instant,
    notice: Option<(String, Instant)>,
    startup: Option<Entry>,
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
        ui::theme::set_current(settings.colors);
        let secrets: Rc<dyn Store> = Rc::new(Keychain);
        let mut dialer = DialerUi::new(paths.files.clone(), secrets.clone());
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
            update: None,
            art_picker: None,
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
            upload_prompt: None,
            download_prompt: None,
            upload_dir: std::env::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            upload_protocol: 0,
            download_protocol: 0,
            save_prompt: None,
            selection: None,
            dragging: false,
            mouse: (0.0, 0.0),
            view_rect: (0, 0, 1, 1),
            settings,
            player: Player::default(),
            login: None,
            remember: None,
            secrets,
            keychain_ok: true,
            transfer_direction: None,
            session: None,
            session_id: 0,
            session_index: None,
            redials: 0,
            macro_steps: std::collections::VecDeque::new(),
            macro_at: Instant::now(),
            redial: None,
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
            .with_min_inner_size(PhysicalSize::new(nw, nh))
            .with_window_icon(window_icon());
        // The Wayland app ID and X11 class, which desktops match to
        // phosphor.desktop for the launcher icon.
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            use winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};
            let attrs = WindowAttributesExtWayland::with_name(attrs, "phosphor", "phosphor");
            WindowAttributesExtX11::with_name(attrs, "phosphor", "phosphor")
        };
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

    fn connect(&mut self, mut entry: Entry, index: Option<usize>) {
        self.redial = None;
        self.redials = 0;
        match secrets::password(&entry, &*self.secrets) {
            Ok(p) => entry.password = p,
            // The login asks instead.
            Err(e) => self.notify(format!("Could not read the password from the keychain: {e}")),
        }
        let config = SessionConfig {
            scrollback: SCROLLBACK_LINES,
            known_hosts: self.paths.known_hosts.clone(),
            download_dir: self.paths.downloads.clone(),
            ssh_agent: true,
            ssh_key_dir: self.paths.ssh_keys.clone(),
        };
        let (cols, rows) = entry.screen_mode();
        let mut session = Session::new(entry, cols as usize, rows as usize, config);
        session.term.music_on_csi_m = self.settings.music == Music::All;
        self.dialer_open = false;
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

    /// Dial an address from the command line or a link, with the directory's
    /// settings for that system if it has an entry for it.
    fn dial_address(&mut self, e: Entry) {
        let id = e.system_id();
        let index = self.directory.entries.iter().position(|x| x.system_id() == id);
        let e = index.map_or(e, |i| self.directory.entries[i].clone());
        // Where the hang-up question shows, if one is needed.
        self.dialer_open = true;
        self.dial(e, index);
    }

    /// Dial from the directory: straight away, or after confirming that the
    /// live call should be hung up.
    fn dial(&mut self, entry: Entry, index: Option<usize>) {
        match &self.session {
            Some(s) if s.state != State::Closed => {
                self.dialer.confirm_hang_up(&s.entry.name, &entry.name);
                self.pending_dial = Some((entry, index));
            }
            _ => self.connect(entry, index),
        }
    }

    fn hang_up(&mut self) {
        self.redial = None;
        self.macro_steps.clear();
        self.session = None;
        self.session_index = None;
        self.scroll = 0;
        self.player.stop();
        self.upload_prompt = None;
        self.save_prompt = None;
        self.selection = None;
        self.login = None;
        self.remember = None;
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
        let closed = matches!(ev, NetEvent::Closed(_));
        // New text moves what a selection was over.
        if matches!(ev, NetEvent::Data(_)) && !self.dragging && self.scroll == 0 {
            self.selection = None;
        }
        session.handle(ev);
        // The question went away (the connection closed).
        if session.auth_prompt().is_none() {
            self.login = None;
        }
        if connected && let Some(e) = self.session_entry_mut() {
            e.calls += 1;
            e.last_connected = Some(dialer::now());
            self.save_directory();
        }
        if closed {
            self.schedule_redial();
        }
        self.session_events();
        self.request_redraw();
    }

    /// Dial a busy or unreachable BBS again after a while, a few times.
    fn schedule_redial(&mut self) {
        let Some(s) = self.session.as_ref().filter(|s| s.should_redial()) else { return };
        if !self.settings.redial || self.redials >= MAX_REDIALS {
            return;
        }
        let (entry, index) = (s.entry.clone(), self.session_entry_index());
        self.redial = Some(Redial { entry, index, attempt: self.redials + 1, at: Instant::now() + REDIAL_DELAY });
    }

    /// Make the waiting redial now.
    fn redial_now(&mut self) {
        let Some(r) = self.redial.take() else { return };
        self.connect(r.entry, r.index);
        self.redials = r.attempt;
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
                SessionEvent::Term(TermEvent::Music(tones)) => {
                    if self.settings.music != Music::Off {
                        self.player.play(tones);
                    }
                }
                SessionEvent::Notice(msg) => self.notify(msg),
                SessionEvent::Connected => {
                    // Got through after redialing: worth a look.
                    if self.redials > 0 {
                        self.attention();
                    }
                    if let Some(pw) = self.remember.take()
                        && let Some(e) = self.session_entry_mut()
                    {
                        e.password = pw;
                        self.save_directory();
                    }
                }
                SessionEvent::AuthPrompt => {
                    let Some(p) = self.session.as_ref().and_then(|s| s.auth_prompt()) else { continue };
                    self.login = Some(LoginPrompt::new(p, self.session_index.is_some(), self.keychain_ok));
                    self.dialer_open = false;
                    self.scroll = 0;
                    self.attention();
                }
                // Both need the terminal view (the prompt and progress window
                // live there), so they close the dialer if it is open.
                SessionEvent::UploadRequested => {
                    self.dialer_open = false;
                    self.scroll = 0;
                    self.open_upload(true);
                    self.attention();
                }
                SessionEvent::TransferStarted => {
                    self.dialer_open = false;
                    self.scroll = 0;
                    self.upload_prompt = None;
                    self.download_prompt = None;
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

    fn start_xy_upload(&mut self, protocol: phosphor_core::transfer::XyProtocol, paths: Vec<PathBuf>) {
        let Some(session) = self.session.as_mut() else { return };
        if session.state != State::Connected {
            return self.notify("Not connected");
        }
        if let Err(e) = session.start_xy_upload(protocol, paths) {
            self.notify(format!("Upload failed: {e:#}"));
        }
        self.session_events();
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
        self.selection = None;
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

    fn dialer_key(&mut self, key: Key, mods: Mods) {
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
                self.settings.aspect = !self.settings.aspect;
                let msg = if self.settings.aspect { "4:3 aspect, as on a CRT" } else { "Square pixels" };
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
                t.encoding = match t.encoding {
                    Encoding::Cp437 => Encoding::Utf8,
                    Encoding::Utf8 => Encoding::Cp437,
                };
                let msg = format!("Encoding: {}", t.encoding.name());
                self.notify(msg);
            }
            KeyCode::KeyC => self.toggle_capture(),
            KeyCode::KeyS => {
                let now = dialer::now();
                let path = self.paths.downloads.join(format!("{}-{}.ans", file_stem(&session.entry.name), stamp(now)));
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

    fn login_key(&mut self, key: Key, mods: Mods) {
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

    /// Where the entry the session was dialed from is in the directory now:
    /// editing the directory can move it.
    fn session_entry_index(&self) -> Option<usize> {
        let id = self.session.as_ref()?.entry.system_id();
        let entries = &self.directory.entries;
        let i = self.session_index?;
        if entries.get(i).is_some_and(|e| e.system_id() == id) {
            Some(i)
        } else {
            entries.iter().position(|e| e.system_id() == id)
        }
    }

    fn session_entry_mut(&mut self) -> Option<&mut Entry> {
        let i = self.session_entry_index()?;
        self.session_index = Some(i);
        self.directory.entries.get_mut(i)
    }

    /// The phosphor mode the connected system asks for, as it is in the
    /// directory (so edits made while connected show at once).
    fn own_crt_mode(&self) -> Option<CrtMode> {
        match self.session_entry_index() {
            Some(i) => self.directory.entries[i].crt,
            None => self.session.as_ref().and_then(|s| s.entry.crt),
        }
    }

    fn crt_mode(&self) -> CrtMode {
        self.own_crt_mode().unwrap_or(self.settings.crt)
    }

    fn open_art_picker(&mut self) {
        if self.art_dir.as_os_str().is_empty() {
            self.art_dir = if self.paths.downloads.is_dir() {
                self.paths.downloads.clone()
            } else {
                std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
            };
        }
        self.art_picker = Some(FilePicker::new(self.art_dir.clone(), false, art_ui::EXTENSIONS));
        self.help_open = false;
        self.settings_open = false;
    }

    fn art_picker_key(&mut self, key: Key, mods: Mods) {
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

    /// Show a file in the ANSI viewer.
    fn view_art(&mut self, path: &std::path::Path) {
        match ArtView::open(path, 0) {
            Ok(v) => {
                if let Some(dir) = path.parent() {
                    self.art_dir = dir.to_path_buf();
                }
                self.art = Some(v);
            }
            Err(e) => self.notify(format!("{e:#}")),
        }
    }

    fn open_settings(&mut self) {
        self.settings_ui.open(theme::user_themes(&self.paths.themes), bezel::names(&self.paths.bezels));
        self.settings_open = true;
        self.help_open = false;
    }

    fn settings_key(&mut self, key: Key, mods: Mods) {
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

    /// Copy a theme file into the themes folder and use it.
    fn import_theme(&mut self, path: &std::path::Path) -> Result<(String, PathBuf)> {
        let path = ui::expand_home(&path.to_string_lossy());
        let file = ThemeFile::load(&path)?;
        if theme::SCHEMES.iter().any(|(n, _)| n.eq_ignore_ascii_case(&file.name)) {
            anyhow::bail!("\"{}\" is the name of a built-in scheme", file.name);
        }
        let dest = self.paths.themes.join(theme::file_name(&file.name));
        file.save(&dest)?;
        self.settings.colors = file.colors;
        self.settings_ui.set_themes(theme::user_themes(&self.paths.themes));
        self.apply_settings();
        Ok((file.name, dest))
    }

    /// Load the bezel the settings name, if it changed.
    fn load_bezel(&mut self) {
        if self.settings.bezel == self.bezel_name {
            return;
        }
        self.bezel_name = self.settings.bezel.clone();
        self.bezel_frame = None;
        self.bezel = None;
        if self.bezel_name.is_empty() {
            return;
        }
        match Bezel::find(&self.bezel_name, &self.paths.bezels) {
            Ok(b) => self.bezel = Some(b),
            Err(e) => self.notify(format!("Bezel: {e:#}")),
        }
    }

    /// Ask in the background for a newer version, once per run, if the
    /// user has turned the check on. Failures are only logged.
    fn check_for_updates(&mut self) {
        if !self.settings.check_updates || self.update_checked {
            return;
        }
        self.update_checked = true;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let current = env!("CARGO_PKG_VERSION");
            match phosphor_core::update::latest(current) {
                Ok(r) if phosphor_core::update::is_newer(&r.version, current) => {
                    let _ = proxy.send_event(UserEvent::UpdateAvailable(r.version));
                }
                Ok(_) => log::info!("Phosphor is up to date"),
                Err(e) => log::warn!("checking for updates: {e:#}"),
            }
        });
    }

    /// Put changed settings into effect, and save them.
    fn apply_settings(&mut self) {
        self.check_for_updates();
        self.load_bezel();
        ui::theme::set_current(self.settings.colors);
        self.idle_screen = idle_screen(self.update.as_deref());
        if self.settings.music == Music::Off {
            self.player.stop();
        }
        if let Some(s) = self.session.as_mut() {
            s.term.music_on_csi_m = self.settings.music == Music::All;
        }
        if let Err(e) = self.settings.save(&self.paths.settings) {
            log::warn!("saving settings: {e:#}");
        }
    }

    fn settings_changed(&mut self, msg: impl Into<String>) {
        if let Err(e) = self.settings.save(&self.paths.settings) {
            log::warn!("saving settings: {e:#}");
        }
        self.notify(msg);
    }

    /// The font for a screen this many rows high: 8x8 for 50 lines.
    /// The font for a screen this many rows high: 8x8 from 43 lines, as on
    /// an EGA or VGA.
    fn font_for(rows: usize) -> Font {
        if rows >= 43 { Font::vga_8x8() } else { Font::vga_8x16() }
    }

    /// The width of what is on screen: the session's (80 or 132 columns),
    /// or 80 for the dialer and the idle screen.
    fn view_cols(&self) -> usize {
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

    fn cell_width(&self) -> usize {
        if self.settings.nine_dot { 9 } else { 8 }
    }

    fn save_key(&mut self, key: Key, mods: Mods) {
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

    /// Write the scrollback (or, for a PNG, the screen) to `path`; returns
    /// what was saved, for the notice.
    fn save(&mut self, path: &std::path::Path) -> Result<String> {
        let Some(session) = &self.session else { anyhow::bail!("not connected") };
        let Some(format) = SaveFormat::from_path(path) else {
            anyhow::bail!("use a file name ending in .ans, .txt or .png");
        };
        let sc = &session.term.screen;
        let all: Vec<&[Cell]> =
            (0..sc.scrollback_len()).map(|i| sc.scrollback_line(i)).chain((0..sc.rows()).map(|y| sc.row(y))).collect();
        let (data, what) = match format {
            SaveFormat::Ansi => {
                let opts = AnsiOptions { ice: sc.ice_colors, unicode: false };
                (export::ansi(&all, opts), format!("{} lines", all.len()))
            }
            SaveFormat::Text => (export::text(&all).into_bytes(), format!("{} lines", all.len())),
            SaveFormat::Png => {
                let mut r = Renderer::new(Font::vga_8x16(), sc.cols(), sc.rows());
                r.set_layout(Self::font_for(sc.rows()), self.cell_width(), sc.cols(), sc.rows());
                let rows = terminal_rows(session, self.scroll);
                let look_rows = rows.len();
                let frame = Frame { look: &sc.look, look_rows, ..Frame::plain(rows, None, sc.ice_colors) };
                r.draw(&frame, true, false);
                (crate::png::encode(r.pixels(), r.native_width(), r.native_height()), "the screen".into())
            }
        };
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))?;
        Ok(what)
    }

    /// The terminal cell under the mouse, if the terminal is what is shown.
    fn mouse_cell(&self) -> Option<(usize, usize)> {
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

    fn on_mouse_button(&mut self, button: MouseButton, state: ElementState) {
        if button != MouseButton::Left {
            return;
        }
        match state {
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

    fn on_mouse_move(&mut self, x: f64, y: f64) {
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

    fn copy_selection(&mut self, sel: Selection, as_ansi: bool) {
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

    /// Send keyboard macro `i` (Alt-1 is 0), pausing where it says.
    fn start_macro(&mut self, i: usize) {
        let Some(session) = self.session.as_ref().filter(|s| s.state == State::Connected) else {
            return self.notify("Not connected");
        };
        let text = self.settings.macros.get(i).cloned().unwrap_or_default();
        if text.is_empty() {
            let key = crate::macros::key_name(i);
            return self.notify(format!("{HOTKEY}-{key} has no macro yet: set one in Settings ({HOTKEY}-O)"));
        }
        let e = &session.entry;
        self.macro_steps = crate::macros::expand(&text, &e.username, &e.password).into();
        self.macro_at = Instant::now();
        self.run_macro();
    }

    /// Send what a macro has ready, up to its next pause.
    fn run_macro(&mut self) {
        if self.macro_steps.is_empty() || Instant::now() < self.macro_at {
            return;
        }
        let Some(session) = self.session.as_mut().filter(|s| s.state == State::Connected) else {
            self.macro_steps.clear();
            return;
        };
        while let Some(step) = self.macro_steps.pop_front() {
            match step {
                crate::macros::Step::Send(text) => {
                    let bytes = match session.term.encoding {
                        Encoding::Utf8 => text.into_bytes(),
                        Encoding::Cp437 => cp437::encode(&text),
                    };
                    session.send(&bytes);
                }
                crate::macros::Step::Pause => {
                    self.macro_at = Instant::now() + crate::macros::PAUSE;
                    return;
                }
            }
        }
    }

    fn open_upload(&mut self, requested: bool) {
        self.download_prompt = None;
        self.upload_prompt = Some(UploadPrompt::new(requested, self.upload_dir.clone(), self.upload_protocol));
    }

    fn prompt_key(&mut self, key: Key, mods: Mods) {
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
                    None => self.start_upload(paths),
                    Some(p) => self.start_xy_upload(p, paths),
                }
            }
        }
    }

    fn download_key(&mut self, key: Key, mods: Mods) {
        let Some(prompt) = self.download_prompt.as_mut() else { return };
        match prompt.handle_key(key, mods) {
            DownloadAction::None => {}
            DownloadAction::Cancel => self.download_prompt = None,
            DownloadAction::Receive(protocol, name) => {
                self.download_protocol = prompt.protocol;
                self.download_prompt = None;
                let Some(session) = self.session.as_mut() else { return };
                if let Err(e) = session.start_xy_download(protocol, name) {
                    self.notify(format!("Download failed: {e:#}"));
                }
                self.session_events();
            }
        }
    }

    fn toggle_capture(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        if session.capture_path().is_some() {
            session.stop_capture();
            return self.notify("Capture stopped");
        }
        let path = self.paths.captures.join(format!("{}-{}.ans", file_stem(&session.entry.name), stamp(dialer::now())));
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
                let bytes = input::encode(key, mods, session.term.encoding);
                if !bytes.is_empty() {
                    session.send(&bytes);
                }
            }
        }
    }

    fn on_wheel(&mut self, lines: isize) {
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

    fn draw_status(&mut self) {
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
        if session.transfer().is_some() {
            let line = format!(" {} \u{2502} ZMODEM transfer in progress \u{2502} Esc cancels", truncate(&e.name, 30));
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

    fn redraw(&mut self) {
        self.draw_status();
        let dialer_cursor =
            if self.dialer_open { self.dialer.draw(&mut self.dialer_screen, &self.directory) } else { None };
        let mut page_cursor = None;
        if self.help_open {
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
                    transfer_ui::draw_progress(&mut self.overlay, t.progress());
                }
                true
            }
            _ => false,
        };

        let frame = match &self.session {
            _ if self.help_open || self.settings_open || self.art_picker.is_some() => {
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
                Frame { look: &sc.look, look_rows: sc.rows(), ..Frame::plain(rows, overlay_cursor, sc.ice_colors) }
            }
            Some(session) => {
                let sc = &session.term.screen;
                let mut rows = terminal_rows(session, self.scroll);
                rows.push(self.status.row(0));
                let live = self.scroll == 0 || sc.scrollback_len() == 0;
                let cursor = (live && sc.cursor_visible && session.state == State::Connected).then(|| sc.cursor());
                let selection = self.selection;
                Frame { selection, look: &sc.look, look_rows: sc.rows(), ..Frame::plain(rows, cursor, sc.ice_colors) }
            }
            None => {
                let mut rows: Vec<&[Cell]> = (0..ROWS).map(|y| self.idle_screen.row(y)).collect();
                rows.push(self.status.row(0));
                Frame::plain(rows, None, true)
            }
        };
        // The terminal's own height (and font) unless the dialer or the idle
        // screen is showing; one more row for the status bar.
        let text_rows = frame.rows.len() - 1;
        let cols = frame.rows.first().map_or(COLS, |r| r.len());
        let cell_w = if self.settings.nine_dot { 9 } else { 8 };
        self.renderer.set_layout(Self::font_for(text_rows), cell_w, cols, text_rows + 1);
        self.renderer.draw(&frame, (self.tick / 2).is_multiple_of(2), self.tick.is_multiple_of(2));

        if let Err(e) = gfx.surface.resize(w, h) {
            return log::error!("resizing surface: {e}");
        }
        match gfx.surface.buffer_mut() {
            Ok(mut buf) => {
                // Fullscreen and maximized windows fill the screen; a normal
                // window keeps whole-number scaling.
                let fill = gfx.window.fullscreen().is_some() || gfx.window.is_maximized();
                let scaling = if fill { Scaling::Fill } else { Scaling::Integer };
                // 4:3 for the text area: 640x400 (or 720x400) on a CRT.
                let pixel_aspect = if self.settings.aspect {
                    let text_h = (self.renderer.native_height() * text_rows / (text_rows + 1)) as f64;
                    self.renderer.native_width() as f64 / text_h * 0.75
                } else {
                    1.0
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
                        let display = Display { scaling: Scaling::Fill, pixel_aspect: sh / sw * nw / nh, crt };
                        self.view_rect = self.renderer.blit_in(&mut buf, ww, wh, frame.screen, &display);
                        bezel::composite(&mut buf, &frame.pixels);
                    }
                    None => {
                        let display = Display { scaling, pixel_aspect, crt };
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

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        if let Err(e) = self.create_window(event_loop) {
            log::error!("creating window: {e:#}");
            return event_loop.exit();
        }
        self.load_bezel();
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
            UserEvent::UpdateAvailable(version) => {
                let msg = format!("Phosphor {version} is out: {}", phosphor_core::update::DOWNLOAD_PAGE);
                self.notice = Some((msg, Instant::now() + UPDATE_NOTICE_TIME));
                self.update = Some(version);
                self.idle_screen = idle_screen(self.update.as_deref());
                self.request_redraw();
            }
            UserEvent::OpenUrl(url) => match Entry::from_address(&url) {
                // Before the window exists: the link that launched the app.
                Some(e) if self.gfx.is_none() => self.startup = Some(e),
                Some(e) => {
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => self.request_redraw(),
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
                if self.session.as_ref().is_none_or(|s| s.state != State::Connected)
                    && !self.dialer_open
                    && is_art(&path) =>
            {
                self.view_art(&path);
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) if self.session.is_none() || self.dialer_open => {
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
            if self.redial.as_ref().is_some_and(|r| now >= r.at) {
                self.redial_now();
            }
            self.request_redraw();
        }
        // Baud rate emulation shows text a few characters at a time.
        let mut wake = self.next_tick;
        if !self.macro_steps.is_empty() {
            self.run_macro();
            if !self.macro_steps.is_empty() {
                wake = wake.min(self.macro_at);
            }
        }
        if let Some(s) = self.session.as_mut().filter(|s| s.pacing()) {
            s.pump();
            self.session_events();
            self.request_redraw();
            wake = wake.min(now + PACE_FRAME);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

/// The icon for the title bar and taskbar (Windows and X11; macOS takes the
/// app bundle's icon, Wayland the desktop file's).
fn window_icon() -> Option<winit::window::Icon> {
    let rgba = include_bytes!("../assets/icon/phosphor-64.rgba");
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
    let hint = format!("{HOTKEY}-D opens the dialing directory   {HOTKEY}-O settings");
    let lines = [("Phosphor".to_string(), t.info), (hint, t.help)];
    for (i, (text, a)) in lines.iter().enumerate() {
        let x = (COLS - text.chars().count()) / 2;
        ui::label(&mut s, x, ROWS / 2 - 1 + i * 2, text, a.fg(), a.bg());
    }
    if let Some(v) = update {
        let text = format!("Phosphor {v} is out: {}", phosphor_core::update::DOWNLOAD_PAGE);
        ui::label(&mut s, (COLS - text.chars().count()) / 2, ROWS / 2 + 4, &text, t.help_key.fg(), t.help_key.bg());
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
