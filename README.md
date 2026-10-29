# Phosphor

A cross-platform ANSI-BBS telnet client written in Rust, in the tradition of
SyncTERM, NetRunner and mTelnet. Runs natively on Windows, macOS and Linux.

Phosphor draws its own text-mode screen using real CP437 bitmap fonts, so ANSI
art renders pixel-for-pixel as it did on a VGA card. It doesn't depend on the
fonts and colors of a host terminal.

> **Status:** early development (0.1). Telnet and raw TCP connections, ANSI-BBS
> emulation, iCE colors, scrollback, capture and the dialing directory with
> import/export work. See [ROADMAP.md](ROADMAP.md) for what's planned.

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
phosphor                         open the dialing directory
phosphor bbs.example.org         connect directly (also host:port,
                                 telnet://host:port, raw://host:port)
phosphor --import syncterm.lst   add entries from a SyncTERM/CSV/JSON/TOML file
phosphor --export bbses.csv      write the directory (format from extension)
phosphor --paths                 show where data is stored
```

### Dialing directory keys

| Key | Action |
| --- | --- |
| `Enter` | Dial the selected entry |
| `Ins` / `A` | Add an entry |
| `E` / `F2` | Edit the selected entry |
| `Del` / `D` | Remove the selected entry |
| `Q` | Quick connect to an address |
| `I` / `X` | Import / export a directory file |
| letter or digit | Jump to the next entry starting with it |
| `Esc` | Quit |

### Online hotkeys

On macOS use `Cmd` in place of `Alt`.

| Key | Action |
| --- | --- |
| `Alt-H` | Hang up and return to the dialer |
| `Alt-B`, `Shift-PgUp`, mouse wheel | Scrollback (arrows, `PgUp`/`PgDn`, `Home`; any other key returns) |
| `Alt-I` | Toggle iCE colors / blinking |
| `Alt-E` | Toggle CP437 / UTF-8 |
| `Alt-C` | Start/stop capturing to a file |
| `Alt-V`, `Shift-Ins` | Paste from the clipboard |
| `Alt-Enter` | Toggle fullscreen |
| `Alt-X` | Exit |

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

To render sample frames to images for visual inspection:

```sh
PHOSPHOR_SNAPSHOT_DIR=/tmp/shots cargo test -p phosphor snapshot -- --ignored
```

## License

Phosphor is free software, licensed under the GNU General Public License,
version 3 or (at your option) any later version. See [LICENSE](LICENSE).

The bundled VGA font comes from DOSBox (GPL-2.0-or-later). See
[crates/phosphor/assets/fonts/README.md](crates/phosphor/assets/fonts/README.md).
