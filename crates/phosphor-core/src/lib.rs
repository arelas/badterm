//! Front-end independent core of the Phosphor BBS client: CP437 handling,
//! ANSI-BBS terminal emulation, the telnet protocol, the dialing directory and
//! connection sessions.

pub mod ansi;
pub mod cp437;
pub mod dialer;
pub mod palette;
pub mod screen;
pub mod session;
pub mod telnet;

pub use ansi::{Encoding, TermEvent, Terminal};
pub use dialer::{Directory, Entry, Protocol};
pub use screen::{Cell, Color, Screen};
pub use session::{NetEvent, Session};
