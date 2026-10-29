# Phosphor

A cross-platform ANSI-BBS telnet and SSH client written in Rust, in the tradition of
SyncTERM, NetRunner and mTelnet. Runs natively on Windows, macOS and Linux.

Phosphor draws its own text-mode screen using real CP437 bitmap fonts, so ANSI
art renders pixel-for-pixel as it did on a VGA card. It doesn't depend on the
fonts and colors of a host terminal.

> **Status:** early development (0.1). Telnet, SSH and raw TCP connections,
> ZMODEM file transfers, ANSI-BBS emulation, iCE colors, scrollback, capture and
> the dialing directory with import/export work. See [ROADMAP.md](ROADMAP.md)
> for what's planned.

## Features

- **ANSI-BBS emulation**: cursor movement, erase/insert/delete, scroll regions,
  save/restore cursor, cursor position reports (used by BBSes to detect ANSI),
  CP437 "graphic" control characters, form feed clears the screen
- **Colors**: 16-color VGA palette with bold-as-bright, **iCE colors** (blink
  bit selects a bright background; toggle with `Alt-I`, or by the server with
  `CSI ?33h`), real blinking text when iCE is off, xterm 256-color, and 24-bit
  color (SGR `38;2` and the CTerm/PabloDraw `CSI 1;r;g;b t` form)
- **Telnet** with BINARY, ECHO, SGA, TERMINAL-TYPE and NAWS negotiation;
  **raw TCP** for systems that don't speak telnet
- **SSH** (pure Rust, no OpenSSL): "none" auth for BBSes that run their own
  login, password, and keyboard-interactive auth. Host keys are trusted on
  first use and pinned in Phosphor's own `known_hosts`; a changed key is
  refused with an explanation.
- **ZMODEM** downloads start automatically when the BBS sends a file. Uploads
  start with `Alt-U`, by dropping files on the window, or automatically when
  the BBS starts receiving. Batches, progress, speed and ETA are shown, and
  `Esc` cancels. Downloads never overwrite existing files.
- **CP437 and UTF-8** sessions, switchable per entry or live with `Alt-E`
- **Scrollback** of 10,000 lines. Screens cleared by the BBS are kept, so you can
  scroll back through menus you've already left.
- **Session capture** to `.ans` files (`Alt-C`)
- **Dialing directory** with add/edit/delete, quick connect, call counts and
  last-called dates
- **Import/export** of SyncTERM `.lst`, CSV, JSON and TOML directories. Use
  the dialer, the command line, or drag a file onto the window.
- Clipboard paste, fullscreen, integer pixel scaling with letterboxing

## Building

Install Rust (stable, 1.85 or newer) from <https://rustup.rs>, then:

```sh
cargo build --release
./target/release/phosphor
```

No extra system libraries are required on Linux; X11 and Wayland are both
supported and loaded at runtime.

## Usage

```
phosphor                         start, not connected (Alt-D opens the dialing directory)
phosphor bbs.example.org         connect directly (also host:port,
                                 telnet://host:port, raw://host:port,
                                 ssh://user@host:port)
phosphor --import syncterm.lst   add entries from a SyncTERM/CSV/JSON/TOML file
phosphor --export bbses.csv      write the directory (format from extension)
phosphor --paths                 show where data is stored
```

On Windows, Phosphor is a GUI program, so `cmd` doesn't wait for it and its
command-line output can appear after the next prompt. Use
`start /wait phosphor --help` in `cmd`, or run it from PowerShell.

### Dialing directory keys

Open the dialing directory with `Alt-D` (`Cmd-D` on macOS) at any time. If you
are connected, the call stays up behind it, and dialing another BBS asks before
hanging up.

| Key | Action |
| --- | --- |
| `Enter` | Dial the selected entry |
| `Ins` / `A` | Add an entry |
| `E` / `F2` | Edit the selected entry |
| `Del` / `D` | Remove the selected entry |
| `Q` | Quick connect to an address |
| `I` / `X` | Import / export a directory file |
| letter or digit | Jump to the next entry starting with it |
| `Esc` | Close the dialing directory |

### Hotkeys

Every client command is on `Alt` (on macOS, `Cmd`, since Option types
characters there). Every `Ctrl` key, `Ctrl-D` included, always goes to the BBS.

| Key | Action |
| --- | --- |
| `Alt-D` | Open or close the dialing directory |
| `Alt-H` | Hang up (or clear a finished call from the screen) |
| `Alt-B`, `Shift-PgUp`, mouse wheel | Scrollback (arrows, `PgUp`/`PgDn`, `Home`; any other key returns) |
| `Alt-I` | Toggle iCE colors / blinking |
| `Alt-E` | Toggle CP437 / UTF-8 |
| `Alt-C` | Start/stop capturing to a file |
| `Alt-U` | Upload files with ZMODEM (or drop files on the window) |
| `Alt-V`, `Shift-Ins` | Paste from the clipboard |
| `Alt-Enter` | Toggle fullscreen |
| `Alt-X` | Exit |

Downloads are saved to `Downloads/Phosphor`; `phosphor --paths` shows every
location Phosphor uses.

For SSH entries, set the user name (and password, if the BBS needs one) in
the entry. Many BBSes accept a user like `new` or `guest` and then run their
own login.

Keys are sent the way ANSI-BBS software expects: arrows as `ESC [ A..D`,
`Home`/`End` as `ESC [ H` / `ESC [ K`, `PgUp`/`PgDn` as `ESC [ V` / `ESC [ U`,
`Ins` as `ESC [ @` and `Del` as DEL.

## Project layout

```
crates/phosphor-core   front-end independent library: CP437, screen model,
                       ANSI parser, telnet, dialing directory, sessions
crates/phosphor        the windowed client (winit + softbuffer), renderer,
                       text-mode dialer UI
```

`phosphor-core` has no GUI dependencies, so it can drive other front ends
(a TUI, a headless bot, a test harness) as well.

## Testing

```sh
cargo test
```

The ZMODEM interoperability tests run against lrzsz's `sz` and `rz` when
they are installed (`apt install lrzsz`, `brew install lrzsz`) and are skipped
otherwise; CI sets `PHOSPHOR_REQUIRE_LRZSZ=1` so they can't be skipped there.

To render sample frames to images for visual inspection:

```sh
PHOSPHOR_SNAPSHOT_DIR=/tmp/shots cargo test -p phosphor snapshot -- --ignored
```

## License

Phosphor is free software, licensed under the GNU General Public License,
version 3 or (at your option) any later version. See [LICENSE](LICENSE).

The bundled VGA font comes from DOSBox (GPL-2.0-or-later). See
[crates/phosphor/assets/fonts/README.md](crates/phosphor/assets/fonts/README.md).
