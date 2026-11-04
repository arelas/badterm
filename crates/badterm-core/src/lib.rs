//! Front-end independent core of the BadTerm BBS client: CP437 handling,
//! ANSI-BBS terminal emulation, the telnet protocol, the dialing directory and
//! connection sessions
//! (telnet, raw TCP, SSH, RLogin and modems) with file transfers.

pub mod ansi;
pub mod atascii;
pub mod audio;
pub mod autologin;
pub mod avatar;
pub mod codepage;
pub mod cp437;
pub mod dialer;
pub mod export;
pub mod igs;
pub mod kermit;
pub mod modem;
pub mod mouse;
pub mod music;
pub mod palette;
pub mod petscii;
pub mod rip;
pub mod sauce;
pub mod screen;
pub mod session;
pub mod sixel;
pub mod skypix;
pub mod syncterm;
pub mod telnet;
pub mod tls;
pub mod transfer;
pub mod transport;
pub mod update;
pub mod viewdata;
pub mod xmodem;

pub use ansi::{Encoding, TermEvent, Terminal};
pub use dialer::{Directory, Entry, Protocol};
pub use screen::{Cell, Color, Screen};
pub use session::{AuthPrompt, NetEvent, PromptKind, Session, SessionConfig, SessionEvent};
