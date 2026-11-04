//! ANSI-BBS / ANSI X3.64 terminal emulation.
//!
//! [`Terminal`] consumes the byte stream from the remote system, drives a
//! [`Screen`], and collects any replies (cursor position reports, etc.) that
//! must be sent back.

use serde::{Deserialize, Serialize};

use crate::cp437;
use crate::music::{self, Tone};
use crate::palette::{ANSI_TO_VGA, rgb};
use crate::screen::{Cell, Color, DEFAULT_BG, DEFAULT_FG, Look, Screen, flags};

/// How incoming bytes are interpreted: the character set, and the
/// emulations that go with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    #[default]
    Cp437,
    Utf8,
    /// Commodore 64 (see [`crate::petscii`]).
    Petscii,
    /// Commodore 128's 80 columns (see [`crate::petscii`]).
    C128,
    /// Atari 8-bit (see [`crate::atascii`]).
    Atascii,
    /// CP437 with AVT/0+ codes as well as ANSI (see [`crate::avatar`]).
    Avatar,
    /// CP437 ANSI with RIPscrip graphics (see [`crate::rip`]).
    Rip,
    /// Prestel viewdata (see [`crate::viewdata`]).
    Viewdata,
    /// The BBC Micro's teletext screen, Mode 7 (see [`crate::viewdata`]).
    Mode7,
    /// Atari ST: VT-52 with IGS graphics (see [`crate::igs`]).
    Igs,
    /// Amiga: ANSI with SkyPix graphics (see [`crate::skypix`]).
    Skypix,
}

impl Encoding {
    pub const ALL: [Encoding; 11] = [
        Encoding::Cp437,
        Encoding::Utf8,
        Encoding::Petscii,
        Encoding::C128,
        Encoding::Atascii,
        Encoding::Viewdata,
        Encoding::Mode7,
        Encoding::Igs,
        Encoding::Skypix,
        Encoding::Avatar,
        Encoding::Rip,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Encoding::Cp437 => "CP437",
            Encoding::Utf8 => "UTF-8",
            Encoding::Petscii => "PETSCII",
            Encoding::C128 => "C128",
            Encoding::Atascii => "ATASCII",
            Encoding::Avatar => "Avatar",
            Encoding::Rip => "RIPscrip",
            Encoding::Viewdata => "Viewdata",
            Encoding::Mode7 => "Mode 7",
            Encoding::Igs => "IGS",
            Encoding::Skypix => "SkyPix",
        }
    }

    /// A C64 or Atari system, with its own screen size and font.
    pub fn is_retro(self) -> bool {
        matches!(self, Encoding::Petscii | Encoding::C128 | Encoding::Atascii)
    }

    /// The screen such a system has, if it has its own.
    pub fn screen_size(self) -> Option<(usize, usize)> {
        match self {
            Encoding::Petscii => Some((crate::petscii::COLS, crate::petscii::ROWS)),
            Encoding::C128 => Some((crate::petscii::C128_COLS, crate::petscii::ROWS)),
            Encoding::Atascii => Some((crate::atascii::COLS, crate::atascii::ROWS)),
            Encoding::Rip => Some((crate::rip::COLS, crate::rip::ROWS)),
            Encoding::Viewdata => Some((crate::viewdata::COLS, crate::viewdata::ROWS)),
            Encoding::Mode7 => Some((crate::viewdata::COLS, crate::viewdata::BEEB_ROWS)),
            Encoding::Igs => Some((crate::igs::COLS, crate::igs::ROWS)),
            Encoding::Skypix => Some((crate::skypix::COLS, crate::skypix::ROWS)),
            _ => None,
        }
    }

    /// Typed or pasted text as the remote expects it. A CR (Enter) becomes
    /// the system's own end of line.
    pub fn encode_text(self, text: &str) -> Vec<u8> {
        match self {
            Encoding::Cp437 | Encoding::Avatar | Encoding::Rip | Encoding::Igs => cp437::encode(text),
            // The Amiga's character set is ISO-8859-1.
            Encoding::Skypix => text.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect(),
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Petscii | Encoding::C128 => text
                .chars()
                .filter_map(|c| if c == '\r' { Some(0x0D) } else { crate::petscii::encode_char(c) })
                .collect(),
            Encoding::Atascii => text
                .chars()
                .filter_map(|c| if c == '\r' { Some(0x9B) } else { crate::atascii::encode_char(c) })
                .collect(),
            Encoding::Viewdata => text.chars().filter_map(crate::viewdata::encode_char).collect(),
            Encoding::Mode7 => text.chars().filter_map(crate::viewdata::beeb_encode_char).collect(),
        }
    }
}

const MAX_PARAMS: usize = 32;
const MAX_STRING: usize = 4096;
/// The Device Attributes reply: "CTerm" and the revision whose sequences
/// BadTerm follows (1.332: the blob, flip and zoom graphics commands).
pub const CTERM_DA: &[u8] = b"\x1b[=67;84;101;114;109;1;332c";
use crate::screen::FIRST_LOADABLE_FONT;
/// A DCS string (a sixel picture) can be large; longer ones are cut off.
const MAX_DCS: usize = 32 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// ESC followed by an intermediate byte, e.g. `ESC ( B`; waiting for the final byte.
    EscIntermediate,
    Csi,
    /// OSC: collected for the window title.
    Osc,
    /// DCS: collected (sixel graphics).
    Dcs,
    /// APC: collected (SyncTERM's commands).
    Apc,
    /// SOS, PM: ignored until ST.
    IgnoreString,
    /// `CSI = slot ; size {`: this many bytes of a font follow.
    FontBlock(u8, usize),
    /// Saw ESC inside a string; `\` terminates it.
    StringEsc,
    /// An ANSI music string, up to Ctrl-N.
    Music,
}

/// Things the front end may want to react to.
#[derive(Clone, Debug, PartialEq)]
pub enum TermEvent {
    Bell,
    Title(String),
    /// ANSI music to play.
    Music(Vec<Tone>),
    /// Sound from SyncTERM's audio commands.
    Audio(crate::audio::AudioOp),
    /// RIPscrip wants the user to answer this (see [`Terminal::rip_answer`]).
    RipQuestion(crate::rip::Question),
}

pub struct Terminal {
    pub screen: Screen,
    pub encoding: Encoding,
    /// Treat `ESC [ M` with no parameter as the start of ANSI music, as
    /// many BBS doors expect, instead of "delete one line". `ESC [ N` and
    /// `ESC [ |` always start music.
    pub music_on_csi_m: bool,
    /// Whether music is played at all (it isn't waited for if not).
    pub music_enabled: bool,
    /// `MF` (the default, as in BASIC and CTerm): the BBS's text waits
    /// while a tune plays; `MB`: it plays behind. Kept from tune to tune.
    music_fore: bool,
    /// The tune began with `CSI M`, whose `M` may be the start of `MF`.
    music_via_m: bool,
    /// `CSI ? 80 h` (DECSDM): sixel pictures go at the top left of the
    /// screen and don't scroll it.
    pub sixel_display_mode: bool,
    state: State,
    /// The kind of string being read: OSC, DCS or another.
    string_kind: State,
    dcs: Vec<u8>,
    /// SyncTERM's file cache, pictures and pixel buffers.
    pub syncterm: crate::syncterm::SyncTerm,
    /// The mouse reporting the BBS has asked for.
    pub mouse: crate::mouse::MouseModes,
    /// `CSI ? 2004 h`: pasted text comes between `ESC [ 200 ~` and
    /// `ESC [ 201 ~`, so the BBS can tell it from typing.
    pub bracketed_paste: bool,
    private: u8,
    intermediate: u8,
    params: Vec<u32>,
    param_present: bool,
    osc: Vec<u8>,
    utf8_buf: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,
    replies: Vec<u8>,
    events: Vec<TermEvent>,
    petscii: crate::petscii::Petscii,
    atascii: crate::atascii::Atascii,
    avatar: crate::avatar::Avatar,
    viewdata: crate::viewdata::Viewdata,
    /// The RIPscrip canvas, when the encoding is RIP.
    rip: Option<Box<crate::rip::Rip>>,
    /// Where downloads go: RIP looks there for icons and scenes too.
    pub download_dir: Option<std::path::PathBuf>,
    /// A RIP file transfer just started, and what came after it (the
    /// protocol's first bytes), for the session.
    rip_block: Option<(crate::rip::BlockTransfer, Vec<u8>)>,
    /// The Atari ST screen, when the encoding is IGS.
    igs: Option<Box<crate::igs::Igs>>,
    /// The Amiga screen, when the encoding is SkyPix.
    skypix: Option<Box<crate::skypix::Skypix>>,
    /// Nothing has been printed on this line yet (where a RIP line can
    /// start).
    line_start: bool,
    /// Bytes not read yet because the BBS asked to wait for a sound
    /// (`A;Wait`), until then.
    held: Vec<u8>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        Terminal {
            screen: Screen::new(cols, rows, scrollback),
            encoding: Encoding::Cp437,
            music_on_csi_m: false,
            music_enabled: true,
            music_fore: true,
            music_via_m: false,
            sixel_display_mode: false,
            state: State::Ground,
            string_kind: State::IgnoreString,
            dcs: Vec::new(),
            syncterm: Default::default(),
            mouse: Default::default(),
            bracketed_paste: false,
            private: 0,
            intermediate: 0,
            params: Vec::with_capacity(MAX_PARAMS),
            param_present: false,
            osc: Vec::new(),
            utf8_buf: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
            replies: Vec::new(),
            events: Vec::new(),
            petscii: Default::default(),
            atascii: Default::default(),
            avatar: Default::default(),
            viewdata: Default::default(),
            rip: None,
            download_dir: None,
            rip_block: None,
            igs: None,
            skypix: None,
            line_start: true,
            held: Vec::new(),
        }
    }

    /// Set up the screen for the encoding: a C64 or Atari system gets that
    /// machine's colors and font.
    pub fn start_emulation(&mut self) {
        match self.encoding {
            Encoding::Petscii => self.petscii.start(&mut self.screen),
            Encoding::C128 => {
                self.petscii = crate::petscii::Petscii::c128();
                self.petscii.start(&mut self.screen);
            }
            Encoding::Atascii => self.atascii.start(&mut self.screen),
            Encoding::Rip => self.rip = Some(Box::default()),
            Encoding::Viewdata => self.viewdata.start(&mut self.screen),
            Encoding::Mode7 => {
                self.viewdata = crate::viewdata::Viewdata::new(crate::viewdata::Kind::Beeb);
                self.viewdata.start(&mut self.screen);
            }
            Encoding::Igs => self.igs = Some(Box::default()),
            Encoding::Skypix => self.skypix = Some(Box::default()),
            _ => {}
        }
    }

    /// The RIPscrip canvas, for a RIP system.
    pub fn rip(&self) -> Option<&crate::rip::Rip> {
        self.rip.as_deref()
    }

    pub fn rip_mut(&mut self) -> Option<&mut crate::rip::Rip> {
        self.rip.as_deref_mut()
    }

    /// Show a teletext page's concealed text, or hide it again (Viewdata
    /// and Mode 7); the new state, or None for other systems.
    pub fn toggle_reveal(&mut self) -> Option<bool> {
        matches!(self.encoding, Encoding::Viewdata | Encoding::Mode7)
            .then(|| self.viewdata.toggle_reveal(&mut self.screen))
    }

    /// The Atari ST screen, for an IGS system.
    pub fn igs(&self) -> Option<&crate::igs::Igs> {
        self.igs.as_deref()
    }

    pub fn igs_mut(&mut self) -> Option<&mut crate::igs::Igs> {
        self.igs.as_deref_mut()
    }

    /// The Amiga screen, for a SkyPix system.
    pub fn skypix(&self) -> Option<&crate::skypix::Skypix> {
        self.skypix.as_deref()
    }

    /// After SkyPix has drawn: its replies, bells and cursor.
    fn skypix_done(&mut self) {
        let Some(sky) = self.skypix.as_deref_mut() else { return };
        self.replies.append(&mut sky.replies);
        for _ in 0..std::mem::take(&mut sky.bells) {
            self.events.push(TermEvent::Bell);
        }
        let (x, y) = sky.cursor();
        self.screen.move_to(x, y);
        self.screen.cursor_visible = sky.cursor_on;
    }

    /// After IGS has drawn: its replies, bells and cursor.
    fn igs_done(&mut self) {
        let Some(igs) = self.igs.as_deref_mut() else { return };
        self.replies.append(&mut igs.replies);
        for _ in 0..std::mem::take(&mut igs.bells) {
            self.events.push(TermEvent::Bell);
        }
        for op in igs.audio.drain(..) {
            if self.music_enabled {
                self.events.push(TermEvent::Audio(op));
            }
        }
        let (x, y) = igs.cursor();
        self.screen.move_to(x, y);
        self.screen.cursor_visible = igs.cursor_on;
    }

    /// The code page the BBS's text is in, when it has chosen a code-page
    /// font for its screen (CP866, KOI8-R...): typing, pasting and copying
    /// go through it rather than CP437.
    pub fn codepage(&self) -> Option<&'static crate::codepage::CodePage> {
        matches!(self.encoding, Encoding::Cp437 | Encoding::Avatar | Encoding::Rip)
            .then(|| crate::codepage::for_font(self.screen.look.fonts[0]))
            .flatten()
    }

    /// What the characters on the screen are, for copying them as text:
    /// the code page, or the C64's or Atari's screen codes.
    pub fn text_codepage(&self) -> Option<&'static crate::codepage::CodePage> {
        match self.encoding {
            Encoding::Petscii | Encoding::C128 | Encoding::Atascii => {
                crate::codepage::screen_codes(self.screen.look.fonts[0])
            }
            _ => self.codepage(),
        }
    }

    /// Text typed or pasted, as the BBS expects its bytes.
    pub fn encode_text(&self, text: &str) -> Vec<u8> {
        match self.codepage() {
            Some(cp) => cp.encode_str(&text.replace("\r\n", "\r")),
            None => self.encoding.encode_text(text),
        }
    }

    /// Pasted text, as the BBS should get it: in brackets if it asked for
    /// them (with any escape taken out, so the text can't end them early).
    pub fn paste(&self, text: &str) -> Vec<u8> {
        if !self.bracketed_paste {
            return self.encode_text(text);
        }
        let text: String = text.chars().filter(|&c| c != '\x1b').collect();
        let mut out = b"\x1b[200~".to_vec();
        out.extend(self.encode_text(&text));
        out.extend_from_slice(b"\x1b[201~");
        out
    }

    /// Bytes the terminal wants sent to the remote (e.g. CPR replies).
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub fn take_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn feed(&mut self, data: &[u8]) {
        match self.encoding {
            Encoding::Petscii | Encoding::C128 => {
                for &b in data {
                    self.petscii.byte(&mut self.screen, b, &mut self.events);
                }
            }
            Encoding::Atascii => {
                for &b in data {
                    self.atascii.byte(&mut self.screen, b, &mut self.events);
                }
            }
            Encoding::Viewdata | Encoding::Mode7 => self.viewdata.feed(&mut self.screen, data, &mut self.events),
            Encoding::Igs => {
                if let Some(igs) = self.igs.as_deref_mut() {
                    igs.feed(data);
                }
                self.igs_done();
            }
            Encoding::Skypix => {
                if let Some(sky) = self.skypix.as_deref_mut() {
                    sky.feed(data);
                }
                self.skypix_done();
            }
            _ => {
                if self.syncterm.hold.is_some() {
                    self.held.extend_from_slice(data);
                    return;
                }
                for (i, &b) in data.iter().enumerate() {
                    self.byte(b);
                    if self.syncterm.hold.is_some() {
                        self.held.extend_from_slice(&data[i + 1..]);
                        break;
                    }
                    // A RIP file transfer: the rest is the protocol's.
                    if let Some(block) = self.rip.as_mut().and_then(|r| r.block.take()) {
                        let rest = data[i + 1..].strip_prefix(b"\n").unwrap_or(&data[i + 1..]);
                        self.rip_block = Some((block, rest.to_vec()));
                        break;
                    }
                }
            }
        }
    }

    /// Time passing: sound reports the BBS asked for, and reading again
    /// after a wait for a sound.
    pub fn poll(&mut self, now: std::time::Instant) {
        self.syncterm.audio.poll(now, &mut self.replies);
        if let Some(igs) = self.igs.as_deref_mut().filter(|g| g.hold.is_some()) {
            igs.poll(now);
            self.igs_done();
        }
        if let Some(sky) = self.skypix.as_deref_mut().filter(|s| s.hold.is_some()) {
            sky.poll(now);
            self.skypix_done();
        }
        if self.syncterm.hold.is_some_and(|t| now >= t) {
            self.syncterm.hold = None;
            let held = std::mem::take(&mut self.held);
            self.feed(&held);
        }
    }

    /// A key was pressed: an IGS pause or color cycle stops for it.
    pub fn interrupt(&mut self) {
        if let Some(igs) = self.igs.as_deref_mut() {
            igs.interrupt();
            self.igs_done();
        }
    }

    /// When [`Terminal::poll`] has something to do.
    pub fn next_poll(&self) -> Option<std::time::Instant> {
        let igs = self.igs.as_ref().and_then(|g| g.hold);
        let sky = self.skypix.as_ref().and_then(|s| s.hold);
        [self.syncterm.hold, self.syncterm.audio.next_due(), igs, sky].into_iter().flatten().min()
    }

    fn byte(&mut self, b: u8) {
        if self.state == State::Ground {
            let taken = match self.encoding {
                Encoding::Avatar => self.avatar_byte(b),
                Encoding::Rip => self.rip_byte(b),
                _ => false,
            };
            if taken {
                return;
            }
        }
        // CAN and SUB abort any sequence in progress (ECMA-48).
        if self.state != State::Ground && !matches!(self.state, State::FontBlock(..)) && (b == 0x18 || b == 0x1A) {
            self.state = State::Ground;
            return;
        }
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::EscIntermediate => self.state = State::Ground,
            State::Csi => self.csi(b),
            State::Osc | State::IgnoreString => match b {
                0x07 if self.state == State::Osc => self.finish_string(),
                0x1B => self.state = State::StringEsc,
                _ if self.state == State::Osc && self.osc.len() < MAX_STRING => self.osc.push(b),
                _ => {}
            },
            State::FontBlock(slot, len) => {
                self.dcs.push(b);
                if self.dcs.len() >= len {
                    self.state = State::Ground;
                    let font = std::mem::take(&mut self.dcs);
                    self.screen.look.load_font(slot, &font);
                }
            }
            State::Dcs | State::Apc => match b {
                0x1B => self.state = State::StringEsc,
                _ if self.dcs.len() < MAX_DCS => self.dcs.push(b),
                _ => {}
            },
            State::Music => match b {
                0x0E => {
                    self.state = State::Ground;
                    // After CSI M, a tune starting B, F, L, S (or N without
                    // a number) means M and that: CSI MF is MF, not F.
                    let mut tune = std::mem::take(&mut self.osc);
                    if self.music_via_m {
                        let first = tune.first().map(|b| b.to_ascii_uppercase());
                        let digit_next = tune.get(1).is_some_and(u8::is_ascii_digit);
                        if matches!(first, Some(b'B' | b'F' | b'L' | b'S')) || first == Some(b'N') && !digit_next {
                            tune.insert(0, b'M');
                        }
                    }
                    // The last MF or MB says how this and later tunes play.
                    for w in tune.windows(2) {
                        if w[0].eq_ignore_ascii_case(&b'M') {
                            match w[1].to_ascii_uppercase() {
                                b'F' => self.music_fore = true,
                                b'B' => self.music_fore = false,
                                _ => {}
                            }
                        }
                    }
                    let tones = music::parse(&tune);
                    self.osc = tune;
                    if !tones.is_empty() {
                        let ms: u64 = tones.iter().map(|t| t.ms as u64).sum();
                        let length = std::time::Duration::from_millis(ms);
                        self.syncterm.audio.music(length);
                        self.events.push(TermEvent::Music(tones));
                        // In the foreground, nothing more shows until it ends
                        // (a minute at most).
                        if self.music_fore && self.music_enabled {
                            let wait = length.min(std::time::Duration::from_secs(60));
                            self.syncterm.hold = Some(std::time::Instant::now() + wait);
                        }
                    }
                }
                _ if self.osc.len() < MAX_STRING => self.osc.push(b),
                // Too long to be music: give up on it.
                _ => self.state = State::Ground,
            },
            State::StringEsc => {
                if b == b'\\' {
                    self.finish_string();
                } else {
                    // Not ST: abandon the string and treat this as a new escape.
                    self.state = State::Escape;
                    self.escape(b);
                }
            }
        }
    }

    /// AVATAR codes, in the text stream (see [`crate::avatar`]).
    fn avatar_byte(&mut self, b: u8) -> bool {
        use crate::avatar::Outcome;
        match self.avatar.byte(&mut self.screen, b) {
            Outcome::Pass => false,
            Outcome::Done => true,
            Outcome::Show(bytes) => {
                for b in bytes {
                    self.byte(b);
                }
                true
            }
        }
    }

    fn rip_io(&mut self) -> Option<(&mut crate::rip::Rip, &mut Screen, crate::rip::Io<'_>)> {
        let rip = self.rip.as_deref_mut()?;
        let io = crate::rip::Io {
            cache: self.syncterm.cache_dir.as_deref(),
            downloads: self.download_dir.as_deref(),
            replies: &mut self.replies,
        };
        Some((rip, &mut self.screen, io))
    }

    /// A file transfer RIP started, and the bytes after it.
    pub fn take_rip_block(&mut self) -> Option<(crate::rip::BlockTransfer, Vec<u8>)> {
        self.rip_block.take()
    }

    /// A RIP question or bell, as an event.
    fn rip_events(&mut self) {
        let Some(rip) = self.rip.as_deref_mut() else { return };
        if std::mem::take(&mut rip.bell) {
            self.events.push(TermEvent::Bell);
        }
        if let Some(q) = rip.take_question() {
            self.events.push(TermEvent::RipQuestion(q));
        }
    }

    /// A click on the RIP graphics at (x, y): what to send, if anything
    /// (see [`crate::rip::Rip::press`]).
    pub fn rip_press(&mut self, x: usize, y: usize) -> Option<Vec<u8>> {
        let (rip, screen, mut io) = self.rip_io()?;
        let out = rip.press(screen, x, y, &mut io);
        self.rip_events();
        out
    }

    /// The answer to a [`TermEvent::RipQuestion`] (None: cancelled); what
    /// it lets be sent is in the replies.
    pub fn rip_answer(&mut self, answer: Option<String>) {
        if let Some((rip, screen, mut io)) = self.rip_io() {
            rip.answer(screen, answer, &mut io);
        }
        self.rip_events();
    }

    /// RIP lines, which start a line with `!|` (see [`crate::rip`]).
    fn rip_byte(&mut self, b: u8) -> bool {
        use crate::rip::Feed;
        let Some(rip) = self.rip.as_mut() else { return false };
        let mut io = crate::rip::Io {
            cache: self.syncterm.cache_dir.as_deref(),
            downloads: self.download_dir.as_deref(),
            replies: &mut self.replies,
        };
        match rip.byte(&mut self.screen, b, self.line_start, &mut io) {
            Feed::Pass => false,
            Feed::Done => {
                let ask = rip.bell;
                // A scene file plays here, as if the BBS had sent it.
                if let Some(scene) = rip.take_scene() {
                    rip.scene_depth += 1;
                    // The LF after the line that asked for it is still to come.
                    let eat_lf = std::mem::take(&mut rip.eat_lf);
                    self.line_start = true;
                    for b in scene {
                        self.byte(b);
                    }
                    if let Some(rip) = self.rip.as_mut() {
                        rip.scene_depth = rip.scene_depth.saturating_sub(1);
                        rip.eat_lf = eat_lf;
                    }
                    self.line_start = true;
                }
                if ask || self.rip.as_ref().is_some_and(|r| r.has_question()) {
                    self.rip_events();
                }
                true
            }
            Feed::Text(bytes) => {
                self.line_start = false;
                for b in bytes {
                    self.byte(b);
                }
                true
            }
        }
    }

    fn ground(&mut self, b: u8) {
        if self.encoding == Encoding::Utf8 && self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8_buf[self.utf8_len] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_need {
                    let ch = std::str::from_utf8(&self.utf8_buf[..self.utf8_len]).ok().and_then(|s| s.chars().next());
                    self.utf8_need = 0;
                    self.screen.put(ch.and_then(cp437::from_unicode).unwrap_or(b'?'));
                }
                return;
            }
            // Truncated sequence: show a replacement and handle this byte normally.
            self.utf8_need = 0;
            self.screen.put(b'?');
        }
        match b {
            0x00 | 0x7F => {}
            0x07 => self.events.push(TermEvent::Bell),
            0x08 => self.screen.backspace(),
            0x09 => self.screen.tab(),
            0x0A => {
                self.screen.line_feed();
                self.line_start = true;
            }
            0x0C => self.screen.erase_display(2),
            0x0D => {
                self.screen.carriage_return();
                self.line_start = true;
            }
            0x1B => self.state = State::Escape,
            _ => match self.encoding {
                // Like ANSI.SYS, every other byte is a glyph, including the
                // "graphic" control characters (☺, ♥, ►, ...).
                Encoding::Cp437
                | Encoding::Petscii
                | Encoding::C128
                | Encoding::Atascii
                | Encoding::Avatar
                | Encoding::Rip
                | Encoding::Viewdata
                | Encoding::Mode7
                | Encoding::Igs
                | Encoding::Skypix => {
                    if self.avatar.insert {
                        self.screen.insert_chars(1);
                    }
                    self.line_start = false;
                    self.screen.put(b)
                }
                Encoding::Utf8 => match b {
                    0x01..=0x1F => {}
                    0x20..=0x7E => self.screen.put(b),
                    0xC2..=0xDF => self.utf8_start(b, 2),
                    0xE0..=0xEF => self.utf8_start(b, 3),
                    0xF0..=0xF4 => self.utf8_start(b, 4),
                    _ => self.screen.put(b'?'),
                },
            },
        }
    }

    fn utf8_start(&mut self, b: u8, need: usize) {
        self.utf8_buf[0] = b;
        self.utf8_len = 1;
        self.utf8_need = need;
    }

    fn escape(&mut self, b: u8) {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.state = State::Csi;
                self.private = 0;
                self.intermediate = 0;
                self.params.clear();
                self.param_present = false;
            }
            b']' => {
                self.state = State::Osc;
                self.string_kind = State::Osc;
                self.osc.clear();
            }
            b'P' => {
                self.state = State::Dcs;
                self.string_kind = State::Dcs;
                self.dcs.clear();
            }
            b'_' => {
                self.state = State::Apc;
                self.string_kind = State::Apc;
                self.dcs.clear();
            }
            b'X' | b'^' => {
                self.state = State::IgnoreString;
                self.string_kind = State::IgnoreString;
            }
            0x20..=0x2F => self.state = State::EscIntermediate,
            b'7' => self.screen.save_cursor(),
            b'8' => self.screen.restore_cursor(),
            b'c' => {
                self.screen.reset();
                self.mouse = Default::default();
                self.bracketed_paste = false;
            }
            b'D' => self.screen.line_feed(),
            b'E' => {
                self.screen.carriage_return();
                self.screen.line_feed();
            }
            b'M' => self.screen.reverse_index(),
            0x1B => self.state = State::Escape,
            _ => log::debug!("unhandled ESC {:?}", b as char),
        }
    }

    fn finish_string(&mut self) {
        self.state = State::Ground;
        match self.string_kind {
            State::Osc => {}
            State::Dcs => return self.finish_dcs(),
            State::Apc => {
                let apc = std::mem::take(&mut self.dcs);
                if !self.syncterm.apc(&apc, &mut self.screen, &mut self.replies) {
                    log::debug!("unhandled APC {:?}", String::from_utf8_lossy(&apc[..apc.len().min(32)]));
                }
                self.events.extend(self.syncterm.audio_ops.drain(..).map(TermEvent::Audio));
                if apc.capacity() <= 1 << 20 {
                    self.dcs = apc;
                    self.dcs.clear();
                }
                return;
            }
            _ => return,
        }
        let s = String::from_utf8_lossy(&self.osc).into_owned();
        let (code, rest) = s.split_once(';').unwrap_or((s.as_str(), ""));
        match code {
            // Set the window title.
            "0" | "2" => self.events.push(TermEvent::Title(rest.to_string())),
            "4" => self.set_palette(rest),
            "104" => self.reset_palette(rest),
            _ => log::debug!("unhandled OSC {code}"),
        }
    }

    /// A DCS string: `p1 ; p2 q ...` is a sixel picture.
    fn finish_dcs(&mut self) {
        let dcs = std::mem::take(&mut self.dcs);
        // CTerm's loadable font: `CTerm:Font:slot:base64`.
        if let Some(rest) = dcs.strip_prefix(b"CTerm:Font:") {
            let text = String::from_utf8_lossy(rest);
            if let Some((slot, b64)) = text.split_once(':')
                && let (Ok(slot), Some(data)) = (slot.trim().parse::<u8>(), crate::syncterm::decode_b64(b64))
                && !self.screen.look.load_font(slot, &data)
            {
                log::debug!("not loading a {}-byte font into slot {slot}", data.len());
            }
            return;
        }
        let start = dcs.iter().position(|b| !matches!(b, b'0'..=b'9' | b';')).unwrap_or(dcs.len());
        if dcs.get(start) == Some(&b'q') {
            let params: Vec<u32> =
                String::from_utf8_lossy(&dcs[..start]).split(';').map(|p| p.parse().unwrap_or(0)).collect();
            let background = self.screen.look.rgb(self.screen.pen.bg);
            let img = crate::sixel::decode(&params, &dcs[start + 1..], background);
            self.screen.place_image(&img, !self.sixel_display_mode);
        } else {
            log::debug!("unhandled DCS {:?}", String::from_utf8_lossy(&dcs[..dcs.len().min(32)]));
        }
        // Keep the buffer for the next one unless it was a big picture.
        if dcs.capacity() <= 1 << 20 {
            self.dcs = dcs;
            self.dcs.clear();
        }
    }

    /// `OSC 4 ; index ; color [; index ; color ...]`, where a color is
    /// `rgb:R/G/B` (1-4 hex digits each), `#RRGGBB`, or `?` to ask for it.
    fn set_palette(&mut self, args: &str) {
        let mut parts = args.split(';');
        while let (Some(i), Some(spec)) = (parts.next(), parts.next()) {
            let Ok(i) = i.trim().parse::<u8>() else { continue };
            if spec == "?" {
                let c = self.screen.look.palette[i as usize];
                let reply = format!("\x1b]4;{i};rgb:{:02x}/{:02x}/{:02x}\x1b\\", c >> 16, (c >> 8) & 0xFF, c & 0xFF);
                self.replies.extend_from_slice(reply.as_bytes());
            } else if let Some(c) = parse_color(spec) {
                self.screen.look.palette[i as usize] = c;
            }
        }
    }

    /// `OSC 104` resets the whole palette; `OSC 104 ; index ...` just those.
    fn reset_palette(&mut self, args: &str) {
        let standard = &Look::standard().palette;
        if args.trim().is_empty() {
            self.screen.look.palette = *standard;
            return;
        }
        for i in args.split(';').filter_map(|i| i.trim().parse::<u8>().ok()) {
            self.screen.look.palette[i as usize] = standard[i as usize];
        }
    }

    fn csi(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                if self.params.is_empty() {
                    self.params.push(0);
                }
                let p = self.params.last_mut().unwrap();
                *p = p.saturating_mul(10).saturating_add((b - b'0') as u32);
                self.param_present = true;
            }
            b';' | b':' => {
                if self.params.is_empty() {
                    self.params.push(0);
                }
                if self.params.len() < MAX_PARAMS {
                    self.params.push(0);
                }
                self.param_present = true;
            }
            0x3C..=0x3F => {
                if !self.param_present && self.private == 0 {
                    self.private = b;
                }
            }
            // `ESC [ !` asks whether this is a RIPscrip terminal.
            b'!' if self.encoding == Encoding::Rip && self.params.is_empty() && self.private == 0 => {
                self.state = State::Ground;
                self.replies.extend_from_slice(crate::rip::VERSION_REPLY);
            }
            0x20..=0x2F => self.intermediate = b,
            0x40..=0x7E => {
                self.state = State::Ground;
                self.dispatch_csi(b);
            }
            // C0 controls are executed even in the middle of a sequence.
            0x00..=0x1F => {
                if b == 0x1B {
                    self.state = State::Escape;
                } else {
                    self.ground(b);
                }
            }
            _ => self.state = State::Ground,
        }
    }

    /// Parameter `i`, with 0 or missing replaced by `default`.
    fn p(&self, i: usize, default: u32) -> u32 {
        match self.params.get(i) {
            Some(&v) if v != 0 => v,
            _ => default,
        }
    }

    fn dispatch_csi(&mut self, b: u8) {
        if self.intermediate != 0 {
            // CTerm font selection: `CSI slot ; font SP D`.
            if self.intermediate == b' ' && b == b'D' && self.private == 0 {
                let slot = self.params.first().copied().unwrap_or(0) as usize;
                let font = self.params.get(1).copied().unwrap_or(0);
                if slot < 4 && font <= 255 {
                    self.screen.look.fonts[slot] = font as u8;
                }
                return;
            }
            log::debug!("unhandled CSI with intermediate {:?} {:?}", self.intermediate as char, b as char);
            return;
        }
        match self.private {
            0 => {}
            b'?' if b == b'S' => return self.graphics_attributes(),
            b'?' => return self.dec_mode(b),
            b'=' if b == b'n' => return self.cterm_report(),
            // CTerm's older way to load a font: the font's bytes follow.
            b'=' if b == b'{' => {
                let slot = self.params.first().copied().unwrap_or(255).min(255) as u8;
                let len = match self.params.get(1).copied().unwrap_or(0) {
                    1 => 3584,
                    2 => 2048,
                    _ => 4096,
                };
                self.dcs.clear();
                self.state = State::FontBlock(slot, len);
                return;
            }
            // CTerm Device Attributes: what this CTerm-like terminal can do.
            b'<' if b == b'c' && self.params.first().is_none_or(|&v| v == 0) => {
                return self.replies.extend_from_slice(b"\x1b[<0;1;2;3;4;5;6;7c");
            }
            _ => {
                log::debug!("unhandled private CSI {:?}{:?} {:?}", self.private as char, b as char, self.params);
                return;
            }
        }
        let no_params = self.params.is_empty();
        if b == b'|' || no_params && (b == b'N' || b == b'M' && self.music_on_csi_m) {
            self.state = State::Music;
            self.music_via_m = b == b'M';
            self.osc.clear();
            return;
        }
        let n = self.p(0, 1) as usize;
        let s = &mut self.screen;
        match b {
            b'@' => s.insert_chars(n),
            b'A' => s.move_by(0, -(n as isize)),
            b'B' | b'e' => s.move_by(0, n as isize),
            b'C' | b'a' => s.move_by(n as isize, 0),
            b'D' => s.move_by(-(n as isize), 0),
            b'E' => {
                s.move_by(0, n as isize);
                s.carriage_return();
            }
            b'F' => {
                s.move_by(0, -(n as isize));
                s.carriage_return();
            }
            b'G' | b'`' => {
                let y = s.cursor().1;
                s.move_to(n - 1, y);
            }
            b'd' => {
                let x = s.cursor().0;
                s.move_to(x, n - 1);
            }
            b'H' | b'f' => {
                let (row, col) = (self.p(0, 1) as usize, self.p(1, 1) as usize);
                self.screen.move_to(col - 1, row - 1);
            }
            b'J' => s.erase_display(self.params.first().copied().unwrap_or(0)),
            b'K' => s.erase_line(self.params.first().copied().unwrap_or(0)),
            b'L' => s.insert_lines(n),
            b'M' => s.delete_lines(n),
            b'P' => s.delete_chars(n),
            b'S' => s.scroll_up(n),
            b'T' => s.scroll_down(n),
            b'X' => s.erase_chars(n),
            b'Z' => s.back_tab(n),
            // Device Attributes: CTerm, SyncTERM's emulation, which BBSes
            // look for before sending its pictures and sounds; what it can
            // do is asked next (`CSI < c`), and answered truthfully.
            b'c' if self.params.first().is_none_or(|&v| v == 0) => self.replies.extend_from_slice(CTERM_DA),
            b'm' => self.sgr(),
            b'n' => match self.params.first().copied().unwrap_or(0) {
                5 => self.replies.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let (x, y) = self.screen.cursor();
                    self.replies.extend_from_slice(format!("\x1b[{};{}R", y + 1, x + 1).as_bytes());
                }
                _ => {}
            },
            b'r' => {
                let rows = self.screen.rows() as u32;
                let (top, bottom) = (self.p(0, 1), self.p(1, rows));
                self.screen.set_scroll_region(top as usize - 1, bottom as usize - 1);
            }
            b's' => s.save_cursor(),
            b'u' => s.restore_cursor(),
            // CTerm/PabloDraw 24-bit color: CSI 0;r;g;b t (bg) / CSI 1;r;g;b t (fg).
            b't' if self.params.len() == 4 => {
                let c = Color::Rgb(rgb(self.params[1] as u8, self.params[2] as u8, self.params[3] as u8));
                match self.params[0] {
                    0 => self.screen.pen.bg = c,
                    1 => self.screen.pen.fg = c,
                    _ => {}
                }
            }
            b'h' | b'l' => {}
            _ => log::debug!("unhandled CSI {:?} {:?}", b as char, self.params),
        }
    }

    /// CTerm's state reports (`CSI = Ps n`).
    fn cterm_report(&mut self) {
        let reply = match self.params.first().copied().unwrap_or(1) {
            // Fonts: the first loadable slot, the last selection's result
            // (none yet), and the four slots' fonts.
            1 => {
                let f = self.screen.look.fonts;
                format!("\x1b[=1;{FIRST_LOADABLE_FONT};99;{};{};{};{}n", f[0], f[1], f[2], f[3])
            }
            // The DEC modes that are set.
            2 => {
                let s = &self.screen;
                let l = &s.look;
                let set = [
                    (7, s.autowrap),
                    (25, s.cursor_visible),
                    (31, l.bright_font),
                    (32, l.bright_plain),
                    (33, s.ice_colors),
                    (34, l.blink_font),
                    (35, l.blink_steady),
                    (80, self.sixel_display_mode),
                    (2004, self.bracketed_paste),
                ];
                let mut modes: Vec<String> = set.iter().filter(|m| m.1).map(|m| m.0.to_string()).collect();
                modes.extend(self.mouse.set_modes().iter().map(|m| m.to_string()));
                format!("\x1b[=2;{}n", modes.join(";"))
            }
            // A cell's height and width in pixels.
            3 => {
                let (w, h) = self.screen.cell_px();
                format!("\x1b[=3;{h};{w}n")
            }
            // No last column flag, no OSC 8 links.
            4 => "\x1b[=4;0n".to_string(),
            5 => "\x1b[=5;0n".to_string(),
            6 => "\x1b[=6;0n".to_string(),
            // Audio: the channels playing.
            7 => self.syncterm.audio.report(self.params.get(1).map(|&c| c as usize)),
            _ => return,
        };
        self.replies.extend_from_slice(reply.as_bytes());
    }

    /// XTSMGRAPHICS (`CSI ? Pi ; Pa S`): the number of sixel color
    /// registers (1) and the graphics size (2), read only.
    fn graphics_attributes(&mut self) {
        let reply = match (self.params.first().copied(), self.params.get(1).copied()) {
            (Some(1), Some(1 | 4)) => "\x1b[?1;0;256S".to_string(),
            (Some(2), Some(1 | 4)) => {
                let (w, h) = self.screen.size_px();
                format!("\x1b[?2;0;{w};{h}S")
            }
            (Some(i @ 1..=3), _) => format!("\x1b[?{i};3;0S"),
            (Some(i), _) => format!("\x1b[?{i};1;0S"),
            (None, _) => return,
        };
        self.replies.extend_from_slice(reply.as_bytes());
    }

    fn dec_mode(&mut self, b: u8) {
        let on = match b {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        for &mode in &self.params {
            if self.mouse.set(mode, on) {
                continue;
            }
            match mode {
                7 => self.screen.autowrap = on,
                80 => self.sixel_display_mode = on,
                2004 => self.bracketed_paste = on,
                25 => self.screen.cursor_visible = on,
                // CTerm: blink attribute selects bright background (iCE colors).
                // CTerm: bright and blink select fonts, and need not
                // brighten or blink.
                31 => self.screen.look.bright_font = on,
                32 => self.screen.look.bright_plain = on,
                33 => self.screen.ice_colors = on,
                34 => self.screen.look.blink_font = on,
                35 => self.screen.look.blink_steady = on,
                _ => log::debug!("unhandled DEC mode {mode} {on}"),
            }
        }
    }

    fn sgr(&mut self) {
        if self.params.is_empty() {
            self.params.push(0);
        }
        let pen = &mut self.screen.pen;
        let mut i = 0;
        while i < self.params.len() {
            let p = self.params[i];
            match p {
                0 => *pen = Cell { ch: b' ', ..Cell::default() },
                1 => pen.flags = (pen.flags | flags::BOLD) & !flags::FAINT,
                2 => pen.flags = (pen.flags | flags::FAINT) & !flags::BOLD,
                4 => pen.flags |= flags::UNDERLINE,
                5 | 6 => pen.flags |= flags::BLINK,
                7 => pen.flags |= flags::REVERSE,
                8 => pen.flags |= flags::CONCEAL,
                21 | 22 => pen.flags &= !(flags::BOLD | flags::FAINT),
                24 => pen.flags &= !flags::UNDERLINE,
                25 => pen.flags &= !flags::BLINK,
                27 => pen.flags &= !flags::REVERSE,
                28 => pen.flags &= !flags::CONCEAL,
                30..=37 => pen.fg = Color::Pc(ANSI_TO_VGA[(p - 30) as usize]),
                39 => pen.fg = DEFAULT_FG,
                40..=47 => pen.bg = Color::Pc(ANSI_TO_VGA[(p - 40) as usize]),
                49 => pen.bg = DEFAULT_BG,
                90..=97 => pen.fg = Color::Pc(ANSI_TO_VGA[(p - 90) as usize] + 8),
                100..=107 => pen.bg = Color::Pc(ANSI_TO_VGA[(p - 100) as usize] + 8),
                38 | 48 => {
                    let color = match self.params.get(i + 1) {
                        Some(5) => {
                            let c = self.params.get(i + 2).map(|&v| Color::Xterm(v as u8));
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = (i + 4 < self.params.len()).then(|| {
                                let v = &self.params[i + 2..i + 5];
                                Color::Rgb(rgb(v[0] as u8, v[1] as u8, v[2] as u8))
                            });
                            i += 4;
                            c
                        }
                        _ => None,
                    };
                    if let Some(c) = color {
                        if p == 38 { pen.fg = c } else { pen.bg = c }
                    }
                }
                _ => log::debug!("unhandled SGR {p}"),
            }
            i += 1;
        }
    }
}

/// An X11-style color: `rgb:R/G/B` with 1-4 hex digits per channel, or
/// `#RRGGBB`.
fn parse_color(spec: &str) -> Option<crate::palette::Rgb> {
    let spec = spec.trim();
    if let Some(hex) = spec.strip_prefix('#') {
        return (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok()).flatten();
    }
    let mut channels = spec.strip_prefix("rgb:")?.split('/').map(|c| {
        let v = u32::from_str_radix(c, 16).ok().filter(|_| (1..=4).contains(&c.len()))?;
        // Scale to 8 bits: "f" and "ffff" are both full.
        Some((v * 255 / ((1 << (4 * c.len())) - 1)) as u8)
    });
    let (r, g, b) = (channels.next()??, channels.next()??, channels.next()??);
    channels.next().is_none().then(|| rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term() -> Terminal {
        Terminal::new(80, 25, 100)
    }

    fn line(t: &Terminal, y: usize) -> String {
        Screen::row_text(t.screen.row(y))
    }

    #[test]
    fn cursor_position_and_text() {
        let mut t = term();
        t.feed(b"\x1b[3;5HHi");
        assert_eq!(line(&t, 2), "    Hi");
        assert_eq!(t.screen.cursor(), (6, 2));
    }

    #[test]
    fn sgr_colors() {
        let mut t = term();
        t.feed(b"\x1b[1;31;44mX");
        let c = t.screen.row(0)[0];
        assert_eq!(c.fg, Color::Pc(4));
        assert_eq!(c.bg, Color::Pc(1));
        assert!(c.flags & flags::BOLD != 0);
        t.feed(b"\x1b[0;38;2;10;20;30mY");
        assert_eq!(t.screen.row(0)[1].fg, Color::Rgb(0x0A141E));
        t.feed(b"\x1b[38;5;196mZ");
        assert_eq!(t.screen.row(0)[2].fg, Color::Xterm(196));
    }

    #[test]
    fn cursor_position_report() {
        let mut t = term();
        t.feed(b"\x1b[10;20H\x1b[6n");
        assert_eq!(t.take_replies(), b"\x1b[10;20R");
    }

    #[test]
    fn device_attributes() {
        let mut t = term();
        t.feed(b"\x1b[c\x1b[0c");
        assert_eq!(t.take_replies(), [CTERM_DA, CTERM_DA].concat());
        // Secondary DA (`CSI > c`) and `CSI 1 c` are not answered.
        t.feed(b"\x1b[>c\x1b[1c");
        assert!(t.take_replies().is_empty());
    }

    #[test]
    fn ice_mode_toggle() {
        let mut t = term();
        t.feed(b"\x1b[?33h");
        assert!(t.screen.ice_colors);
        t.feed(b"\x1b[?33l");
        assert!(!t.screen.ice_colors);
    }

    #[test]
    fn cp437_glyphs_and_utf8() {
        let mut t = term();
        t.feed(&[0xDB, 0x03]);
        assert_eq!(t.screen.row(0)[0].ch, 0xDB);
        assert_eq!(t.screen.row(0)[1].ch, 0x03);
        t.encoding = Encoding::Utf8;
        t.feed("█é".as_bytes());
        assert_eq!(t.screen.row(0)[2].ch, 0xDB);
        assert_eq!(t.screen.row(0)[3].ch, 0x82);
    }

    #[test]
    fn osc_title_and_strings_ignored() {
        let mut t = term();
        t.feed(b"\x1b]0;My BBS\x07\x1bPjunk\x1b\\ok");
        assert_eq!(t.take_events(), vec![TermEvent::Title("My BBS".into())]);
        assert_eq!(line(&t, 0), "ok");
    }

    #[test]
    fn ansi_art_line_of_exactly_80_columns() {
        let mut t = term();
        let mut data = vec![b'#'; 80];
        data.extend_from_slice(b"\r\nnext");
        t.feed(&data);
        assert_eq!(line(&t, 1), "next");
    }

    #[test]
    fn save_restore_cursor() {
        let mut t = term();
        t.feed(b"\x1b[5;5H\x1b[s\x1b[1;1H\x1b[uX");
        assert_eq!(t.screen.row(4)[4].ch, b'X');
    }

    #[test]
    fn ansi_music() {
        let mut t = term();
        // Muted, so the text doesn't wait for the tunes (see below).
        t.music_enabled = false;
        t.feed(b"Hi\x1b[NMLO3A\x0e there\x1b[|MLO3A8\x0e");
        assert_eq!(line(&t, 0), "Hi there");
        let a = Tone { hz: 440, ms: 500 };
        assert_eq!(t.take_events(), [TermEvent::Music(vec![a]), TermEvent::Music(vec![Tone { ms: 250, ..a }])]);

        // ESC [ M is delete-line unless music is wanted there.
        t.feed(b"\x1b[2;1Hx\x1b[1;1H\x1b[M");
        assert_eq!(line(&t, 0), "x");
        t.music_on_csi_m = true;
        t.feed(b"\x1b[MMLO3A\x0e\x1b[1M");
        assert_eq!(t.take_events(), [TermEvent::Music(vec![a])]);
        assert_eq!(line(&t, 0), "");
    }

    #[test]
    fn music_in_the_foreground_holds_the_text() {
        let mut t = term();
        // By default a tune plays in the foreground: the text after waits.
        t.feed(b"\x1b[NT120L4C\x0eafter");
        assert_eq!(line(&t, 0), "");
        let at = t.next_poll().expect("held");
        t.poll(at);
        assert_eq!(line(&t, 0), "after");
        // MB plays it behind, and that stays for the next tune.
        t.feed(b"\x1b[NMBC\x0e!\x1b[NC\x0e?");
        assert_eq!(line(&t, 0), "after!?");
        // After CSI M, "F..." is MF: here, back to the foreground.
        t.music_on_csi_m = true;
        t.feed(b"\x1b[MFC\x0e#");
        assert_eq!(line(&t, 0), "after!?");
        assert!(matches!(t.take_events().last(), Some(TermEvent::Music(tones)) if tones.len() == 2));
    }

    #[test]
    fn cterm_palette_and_fonts() {
        let mut t = Terminal::new(80, 25, 0);
        // Red (ANSI 1, VGA 4) becomes orange; bright blue (ANSI 12) too.
        t.feed(b"\x1b]4;1;rgb:ff/80/00;12;#123456\x1b\\");
        assert_eq!(t.screen.look.palette[1], 0xFF8000);
        assert_eq!(t.screen.look.rgb(Color::Pc(4)), 0xFF8000);
        assert_eq!(t.screen.look.rgb(Color::Pc(9)), 0x123456);
        t.feed(b"\x1b[31mX");
        assert_eq!(t.screen.row(0)[0].resolve_with(false, &t.screen.look).0, 0xFF8000);
        // Queried, then reset.
        t.feed(b"\x1b]4;1;?\x07");
        assert_eq!(t.take_replies(), b"\x1b]4;1;rgb:ff/80/00\x1b\\");
        t.feed(b"\x1b]104;1\x07");
        assert_eq!(t.screen.look.palette[1], 0xAA0000);
        assert_eq!(t.screen.look.palette[12], 0x123456);
        t.feed(b"\x1b]104\x07");
        assert_eq!(t.screen.look, *Look::standard());

        // Font slots, and bright text in slot 1 without brightening.
        t.feed(b"\x1b[0;42 D\x1b[1;40 D\x1b[?31h\x1b[?32h\x1b[1;32mB");
        let cell = t.screen.row(0)[1];
        assert_eq!(t.screen.look.fonts, [42, 40, 0, 0]);
        assert_eq!(t.screen.look.font_for(&cell), 40);
        assert_eq!(cell.resolve_with(false, &t.screen.look).0, 0x00AA00);
        t.feed(b"\x1b[?31l\x1b[?32l");
        assert_eq!(t.screen.look.font_for(&cell), 42);
        assert_eq!(cell.resolve_with(false, &t.screen.look).0, 0x55FF55);
        assert_eq!(parse_color("rgb:f/0/8"), Some(0xFF0088));
        assert_eq!(parse_color("rgb:ffff/0000/8080"), Some(0xFF0080));
        assert_eq!(parse_color("rgb:1/2"), None);
    }

    #[test]
    fn sixel_pictures() {
        let mut t = term();
        // 16 x 36 pixels (a pixel per sixel row, six bands), red, at 3;5.
        let mut seq = b"\x1b[3;5HA\x1bP0;1;0q\"1;1#1;2;100;0;0".to_vec();
        for _ in 0..6 {
            seq.extend_from_slice(b"!16~-");
        }
        seq.extend_from_slice(b"\x1b\\B");
        t.feed(&seq);
        let s = &t.screen;
        // The picture covers columns 5-6 (from the cursor, which was after
        // the A) of rows 3-5; the last band ends at pixel 36, so the cursor
        // is on the row holding pixel 36 (row 3 + 36 / 16), in the column it
        // started in, where the B went.
        let tile = s.tiles.get(s.row(2)[5].tile).expect("a tile");
        assert_eq!((tile.width, tile.height), (8, 16));
        assert_eq!(tile.pixels[0], crate::sixel::solid(0xFF0000));
        assert!(s.row(4)[6].tile != 0 && s.row(2)[7].tile == 0);
        assert_eq!(s.row(4)[5].ch, b'B');
        assert_eq!(s.row(4)[5].tile, 0, "text replaces the picture");
        assert_eq!(t.screen.cursor(), (6, 4));

        // The size of the screen in pixels, and the color registers.
        t.feed(b"\x1b[?2;1S\x1b[?1;1S\x1b[<c");
        assert_eq!(t.take_replies(), b"\x1b[?2;0;640;400S\x1b[?1;0;256S\x1b[<0;1;2;3;4;5;6;7c");
        t.feed(b"\x1b[=1n\x1b[=2n\x1b[=3n\x1b[=7n");
        assert_eq!(t.take_replies(), b"\x1b[=1;43;99;0;0;0;0n\x1b[=2;7;25n\x1b[=3;16;8n\x1b[=7n");
    }

    #[test]
    fn sixel_pictures_scroll_the_screen() {
        let mut t = term();
        t.feed(b"\x1b[25;1HX\x1bPq\"1;1;8;32\x1b\\");
        // 32 pixels: two rows; the screen scrolled two lines (the cursor
        // ends on the row holding pixel 32).
        assert_eq!(line(&t, 22), "X");
        assert!(t.screen.row(22)[1].tile != 0 && t.screen.row(23)[1].tile != 0);
        assert_eq!(t.screen.cursor(), (1, 24));
        // In display mode it goes at the top left and the cursor stays.
        t.feed(b"\x1b[?80h\x1b[10;10H\x1bPq\"1;1;8;16\x1b\\");
        assert!(t.screen.row(0)[0].tile != 0);
        assert_eq!(t.screen.cursor(), (9, 9));
    }

    #[test]
    fn waiting_for_a_sound_holds_the_text() {
        let mut t = term();
        // A 100 ms tone, then wait for it before showing "after".
        t.feed(b"\x1b_SyncTERM:A;Synth;S=1;W=SIN;F=440;T=100\x1b\\\x1b_SyncTERM:A;Queue;C=2;S=1\x1b\\");
        assert!(matches!(t.take_events()[..], [TermEvent::Audio(crate::audio::AudioOp::Queue { channel: 2, .. })]));
        t.feed(b"before\x1b_SyncTERM:A;Wait;C=2\x1b\\after");
        t.feed(b" more");
        assert_eq!(line(&t, 0), "before");
        let at = t.next_poll().expect("a time to read again");
        t.poll(at);
        assert_eq!(line(&t, 0), "beforeafter more");
        // Its state is asked for and reported when it ends.
        t.feed(b"\x1b[=7;2n\x1b_SyncTERM:A;Update;C=2\x1b\\");
        t.poll(at + std::time::Duration::from_secs(1));
        let replies = String::from_utf8(t.take_replies()).unwrap();
        assert!(replies.ends_with("\x1b[=7;2;0n"), "{replies:?}");
        t.feed(b"\x1b_SyncTERM:Q;libsndfileFormat;0x20;100\x1b\\");
        assert_eq!(t.take_replies(), b"\x1b[=7;101;32;100;1n");
    }

    #[test]
    fn loadable_fonts() {
        use data_encoding::BASE64;
        let mut t = term();
        // An 8x16 font of solid glyphs, by DCS, into slot 43; then chosen.
        let font = vec![0xFFu8; 4096];
        let dcs = format!("\x1bPCTerm:Font:43:{}\x1b\\\x1b[0;43 D", BASE64.encode(&font));
        t.feed(dcs.as_bytes());
        let f = t.screen.look.loaded_font(43).expect("loaded");
        assert_eq!((f.height, f.data.len()), (16, 4096));
        assert_eq!(t.screen.look.fonts[0], 43);
        // The older way: CSI = slot ; size { and the bytes (8x8 here).
        let mut old = b"\x1b[=44;2{".to_vec();
        old.extend(vec![0x18u8; 2048]);
        old.extend(b"ok");
        t.feed(&old);
        assert_eq!(t.screen.look.loaded_font(44).unwrap().height, 8);
        assert!(line(&t, 0).ends_with("ok"), "the font's bytes aren't text: {:?}", line(&t, 0));
        // Built-in slots can't be replaced; a reset keeps loaded fonts.
        t.feed(format!("\x1bPCTerm:Font:5:{}\x1b\\\x1bc", BASE64.encode(&font)).as_bytes());
        assert!(t.screen.look.loaded_font(5).is_none());
        assert!(t.screen.look.loaded_font(43).is_some());
        // The same font twice is kept once.
        t.feed(dcs.as_bytes());
        t.feed(dcs.replace(":43:", ":45:").as_bytes());
        let (a, b) = (t.screen.look.loaded_font(43).unwrap(), t.screen.look.loaded_font(45).unwrap());
        assert!(std::ptr::eq(a.data, b.data));
    }

    #[test]
    fn typing_in_the_bbs_code_page() {
        let mut t = term();
        assert_eq!(t.encode_text("é"), [0x82]);
        // The BBS picks CP866 (font 25) for its screen.
        t.feed(b"\x1b[0;25 D");
        assert_eq!(t.encode_text("Да"), [0x84, 0xA0]);
        assert_eq!(t.paste("Ж\r"), [0x86, b'\r']);
        // UTF-8 sessions stay UTF-8.
        t.encoding = Encoding::Utf8;
        assert!(t.codepage().is_none());
    }

    #[test]
    fn bracketed_paste() {
        let mut t = term();
        assert_eq!(t.paste("hi\r"), b"hi\r");
        t.feed(b"\x1b[?2004h");
        assert_eq!(t.paste("a\x1b[201~b"), b"\x1b[200~a[201~b\x1b[201~");
        t.feed(b"\x1bc");
        assert!(!t.bracketed_paste);
    }

    #[test]
    fn mouse_modes() {
        let mut t = term();
        t.feed(b"\x1b[?1003h\x1b[?1006h");
        assert!(t.mouse.any && t.mouse.sgr && t.mouse.tracking());
        t.feed(b"\x1b[=2n");
        assert_eq!(t.take_replies(), b"\x1b[=2;7;25;1003;1006n");
        t.feed(b"\x1b[?1003l\x1b[?1006l");
        assert!(!t.mouse.tracking());
    }

    #[test]
    fn reset_returns_to_the_entrys_ice_setting() {
        let mut t = Terminal::new(80, 25, 0);
        t.screen.base_ice = true;
        t.feed(b"\x1b[?33l");
        assert!(!t.screen.ice_colors);
        t.feed(b"\x1bc");
        assert!(t.screen.ice_colors);
    }
}
