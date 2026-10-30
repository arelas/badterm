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
  login, public keys (a key file set in the entry, your SSH agent, or
  `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa`), keyboard-interactive and
  password auth. Anything not stored, such as a password, a key passphrase or
  a one-time code, is asked for in a login dialog. Host keys are trusted on
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
- **Copy with the mouse**: drag to select text and it is copied when you let
  go; hold `Shift` as you let go to copy it with its colors, as ANSI
- **Save the scrollback** (`Alt-S`) as an `.ans` file for ANSI viewers and
  editors, or as plain `.txt`, or save the screen as a `.png`
- **Dialing directory** with add/edit/delete, quick connect, call counts,
  last-called dates, pinned favorites, search (`/`), and sorting by name,
  last called or number of calls (`S`). It starts with
  [The Vortex BBS](telnet://vortexbbs.com), the project's home BBS.
- **Passwords in the OS keychain** (Keychain on macOS, Credential Manager on
  Windows, the Secret Service on Linux) rather than the directory file. Where
  no keychain is available, they stay in the file.
- **Import/export** of SyncTERM `.lst`, CSV, JSON and TOML directories. Use
  the dialer, the command line, or drag a file onto the window.
- **ANSI music**: BBS tunes in the BASIC `PLAY` language (`ESC [ N` or
  `ESC [ |` up to Ctrl-N) play as PC-speaker square waves. `Alt-M` turns
  it off, or also on for `ESC [ M`, which many doors use but which
  otherwise means "delete a line"
- **Phosphor mode** (`Alt-P`): the look of a CRT, with glowing text,
  scanlines and curved glass, in color or as a green or amber monochrome
  monitor, for every BBS or chosen per directory entry
- **VGA-accurate display options**: 9-dot character cells, where the
  line-drawing characters join up as they did on a VGA (`Alt-W`), and the
  4:3 shape of a CRT instead of square pixels (`Alt-A`). An entry can use
  80x50 with the 8x8 font.
- Clipboard paste. A normal window scales by whole numbers so every pixel
  is exact; fullscreen (`Alt-Enter`) and maximized windows fill the screen,
  keeping the shape and keeping the pixels sharp.

## Installing

Downloads are on the [releases page](https://git.badcluster.dev/arelas/phosphor/releases).

- **Windows**: unzip `phosphor-…-windows-x64.zip` and run `phosphor.exe`.
- **macOS**: open `phosphor-…-macos-arm64.dmg` and drag Phosphor to
  Applications. Phosphor isn't signed with an Apple Developer ID yet, so the
  first time, Control-click it and choose Open.
- **Linux**: `phosphor-…-linux-x64.AppImage` runs on most distributions:
  `chmod +x` it and run it. The tarball has the bare program, plus
  `phosphor.desktop` and `phosphor.png` for adding it to your menus by hand
  (copy them to `~/.local/share/applications/` and
  `~/.local/share/icons/hicolor/256x256/apps/`).

## Building

Install Rust (stable, 1.89 or newer) from <https://rustup.rs>, then:

```sh
cargo build --release
./target/release/phosphor
```

No extra system libraries are required on Linux: X11 and Wayland are both
supported, and they and ALSA (for ANSI music) are loaded at run time.

## Usage

```
phosphor                         start, not connected (Alt-D opens the dialing directory)
phosphor bbs.example.org         connect directly (also host:port,
                                 telnet://host:port, raw://host:port,
                                 ssh://user@host:port)
phosphor --import syncterm.lst   add entries from a SyncTERM/CSV/JSON/TOML file
phosphor --export bbses.csv      write the directory (format from extension)
phosphor --paths                 show where data is stored
phosphor --render art.ans out.png
                                 draw an ANSI file to a PNG (add --screen for
                                 just the last 80x25 screen, --crt color|green|
                                 amber, --nine-dot, --aspect)
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
| `P` | Pin or unpin the entry (pinned entries are listed first) |
| `/`, `F3`, `Ctrl-F` | Search by name, address or comment (`Enter` dials, `Esc` clears) |
| `S` | Sort by name, last called, or number of calls |
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
| `Alt-S` | Save the scrollback (`.ans`, `.txt`) or the screen (`.png`) |
| drag with the mouse | Copy the selected text (`Shift` on release: as ANSI) |
| `Alt-U` | Upload files with ZMODEM (or drop files on the window) |
| `Alt-V`, `Shift-Ins` | Paste from the clipboard |
| `Alt-Enter` | Toggle fullscreen (fills the screen, keeping the shape) |
| `Alt-P` | Phosphor mode: off, color CRT, green screen, amber screen |
| `Alt-A` | 4:3 aspect, as on a CRT, or square pixels |
| `Alt-W` | 9-dot or 8-dot character cells |
| `Alt-M` | ANSI music: on, on including `ESC [ M`, off |
| `Alt-X` | Exit |

`Alt-P`, `Alt-A`, `Alt-W` and `Alt-M` work anywhere, including in the dialing
directory, and are remembered in `settings.toml`. An entry's "Screen mode"
can be 80x25 or 80x50, and its "Phosphor mode" can override `Alt-P` for that
BBS. While connected to a BBS with its own phosphor mode, `Alt-P` changes
that BBS's mode rather than the global one.

Downloads are saved to `Downloads/Phosphor`; `phosphor --paths` shows every
location Phosphor uses.

For SSH entries, set the user name in the entry. Many BBSes accept a user
like `new` or `guest` and then run their own login. Phosphor logs in with:

1. "none", which is all most BBSes need;
2. public keys: the entry's "SSH key file" (asking for its passphrase if it
   has one), then your SSH agent (`SSH_AUTH_SOCK`; on Windows the OpenSSH
   agent service or Pageant), then unencrypted `~/.ssh/id_ed25519`,
   `id_ecdsa` and `id_rsa`. Encrypted default keys are skipped rather than
   asking for a passphrase on every BBS that offers key logins: add them to
   your agent, or name one in the entry;
3. keyboard-interactive or password, using the entry's password if there is
   one and asking otherwise (or when it is refused). The login dialog can
   remember the password for next time (`Tab`).

Passwords you enter in the directory, remember from the login dialog, or
import with a SyncTERM list are moved into the OS keychain when the
directory is saved. Exported directories don't include them.

Keys are sent the way ANSI-BBS software expects: arrows as `ESC [ A..D`,
`Home`/`End` as `ESC [ H` / `ESC [ K`, `PgUp`/`PgDn` as `ESC [ V` / `ESC [ U`,
`Ins` as `ESC [ @` and `Del` as DEL.

## Website

`site/` holds the project's static web page; see [site/README.md](site/README.md).

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

The bundled VGA fonts come from DOSBox (GPL-2.0-or-later). See
[crates/phosphor/assets/fonts/README.md](crates/phosphor/assets/fonts/README.md).
