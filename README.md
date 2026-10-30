# Phosphor

A cross-platform ANSI-BBS client (telnet, SSH and RLogin) written in Rust, in the tradition of
SyncTERM, NetRunner and mTelnet. Runs natively on Windows, macOS and Linux.

Phosphor draws its own text-mode screen using real CP437 bitmap fonts, so ANSI
art renders pixel-for-pixel as it did on a VGA card. It doesn't depend on the
fonts and colors of a host terminal.

> **Status:** early development (0.1). Telnet, telnet over TLS, SSH, RLogin
> and raw TCP connections,
> ZMODEM file transfers, ANSI-BBS emulation, iCE colors, scrollback, capture and
> the dialing directory with import/export work. See [ROADMAP.md](ROADMAP.md)
> for what's planned.

## Features

- **ANSI-BBS emulation**: cursor movement, erase/insert/delete, scroll regions,
  save/restore cursor, cursor position reports (used by BBSes to detect ANSI),
  CP437 "graphic" control characters, form feed clears the screen
- **Fonts**: the IBM VGA fonts, and the Amiga fonts (Topaz, Topaz Plus,
  MicroKnight, MicroKnight Plus, P0T-NOoDLE, mO'sOul) for Amiga BBSes: set
  per directory entry, or by the BBS with CTerm's font selection
  (`CSI n ; m SP D`, including the bright and blink font slots of
  `CSI ?31h` / `?34h`)
- **Palette changes** from the BBS (`OSC 4`, reset with `OSC 104`), as
  SyncTERM does
- **Colors**: 16-color VGA palette with bold-as-bright, **iCE colors** (blink
  bit selects a bright background; toggle with `Alt-I`, or by the server with
  `CSI ?33h`), real blinking text when iCE is off, xterm 256-color, and 24-bit
  color (SGR `38;2` and the CTerm/PabloDraw `CSI 1;r;g;b t` form)
- **Telnet** with BINARY, ECHO, SGA, TERMINAL-TYPE and NAWS negotiation;
  **telnet over TLS** ("TelnetS", port 992); **RLogin**, which logs straight
  in with the entry's user name and password; **raw TCP** for systems that
  don't speak telnet
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
  `Esc` cancels. Downloads never overwrite existing files. An interrupted
  download is kept as `name.part` and resumes where it stopped when the BBS
  sends the file again (ZMODEM crash recovery).
- **YMODEM, YMODEM-G and XMODEM** (CRC, 1K or the original checksum), for
  BBSes and doors that offer them: start the BBS's side, then `Alt-G` to
  download or `Alt-U` to upload. YMODEM keeps file names and sizes and sends
  several files; XMODEM asks what to save a download as.
- **A file picker** for uploads: browse folders, mark several files with
  `Space`, or press `Tab` to type a path
- **An ANSI viewer** (`Alt-F`, `phosphor file.ans`, or drop a file on the
  window when not in a call): scroll through art in its own width, font and
  colors, with its SAUCE title, artist, group, date and comments (`I`)
- **CP437 and UTF-8** sessions, switchable per entry or live with `Alt-E`
- **Commodore 64 (PETSCII) and Atari 8-bit (ATASCII)** BBSes, set by the
  entry's encoding: their 40-column screens, colors, character sets (the
  C64's two, switched by the BBS), reverse video and screen-editor codes,
  and their keys (Enter, DEL, the cursor keys, the C64's F1 to F8 and
  RUN/STOP on `Esc`). The fonts are Phosphor's own, in those machines'
  style
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
  monitor, for every BBS or chosen per directory entry. Phosphor starts in
  it: a color CRT with medium scanlines and flat glass, 8-dot characters
  and square pixels, fitted to the window in its own shape
- **VGA-accurate display options**: 9-dot character cells, where the
  line-drawing characters join up as they did on a VGA (`Alt-W`), and the
  4:3 shape of a CRT instead of square pixels (`Alt-A`). Entries can use
  80x43, 80x50 and 132-column screen modes.
- **Baud rate emulation**, per entry (300 to 115200 bps): text arrives at
  the speed it was drawn for, so ANSI animations play as intended.
  ZMODEM transfers still run at full speed.
- Clipboard paste. A normal window scales by whole numbers so every pixel
  is exact; fullscreen (`Alt-Enter`) and maximized windows fill the screen,
  keeping the shape and keeping the pixels sharp.

## Installing

Downloads are on the [website](https://badcluster.dev/phosphor/) and the
[releases page](https://git.badcluster.dev/arelas/phosphor/releases).

- **Windows**: run `phosphor-…-windows-x64-setup.exe`. It installs for you
  alone without administrator rights (or for everyone, if you choose), adds
  Phosphor to the Start menu and opens `telnet://`, `telnets://` and
  `rlogin://` links with it. Or unzip `phosphor-…-windows-x64.zip` and run
  `phosphor.exe` from anywhere.
- **macOS**: open `phosphor-…-macos-arm64.dmg` and drag Phosphor to
  Applications. Phosphor isn't signed with an Apple Developer ID yet, so the
  first time, Control-click it and choose Open. It opens `telnet://` links
  once it has been run.
- **Linux**: install `phosphor-…-linux-x64.deb` (Debian, Ubuntu) or
  `phosphor-…-linux-x64.rpm` (Fedora, RHEL, openSUSE), for example with
  `sudo apt install ./phosphor-…-linux-x64.deb`. They, the AppImage and the
  tarball need glibc 2.28 or newer: Debian 10, Ubuntu 20.04, RHEL 8 and
  later. Or use the AppImage, which runs on most distributions: `chmod +x` it and run it. The tarball has the
  bare program, plus `phosphor.desktop` and `phosphor.png` for adding it to
  your menus by hand (copy them to `~/.local/share/applications/` and
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

Phosphor opens with its splash screen for two seconds (any key skips it;
Settings, General turns it off),
then a start-up banner with the version, as terminal programs did.

```
phosphor                         start, not connected (Alt-D opens the dialing directory)
phosphor bbs.example.org         connect directly (also host:port,
                                 telnet://host:port, telnets://host:port,
                                 rlogin://user@host, raw://host:port,
                                 ssh://user@host:port)
phosphor --import syncterm.lst   add entries from a SyncTERM, NetRunner (.phn),
                                 MagiTerm or EtherTerm directory, a CSV list (ours, or the Telnet BBS
                                 Guide's bbslist.csv), JSON or TOML
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
| `Alt-R` | Call again (after a hang-up, or now while a redial is waiting) |
| `Alt-L` | Send the entry's user name; again for the password |
| `Alt-1` ... `Alt-0` | Keyboard macros (set them in Settings, `Alt-O`) |
| `Alt-B`, `Shift-PgUp`, mouse wheel | Scrollback (arrows, `PgUp`/`PgDn`, `Home`; any other key returns) |
| `Alt-I` | Toggle iCE colors / blinking |
| `Alt-E` | Toggle CP437 / UTF-8 |
| `Alt-C` | Start/stop capturing to a file |
| `Alt-S` | Save the scrollback (`.ans`, `.txt`) or the screen (`.png`) |
| drag with the mouse | Copy the selected text (`Shift` on release: as ANSI) |
| `Alt-U` | Upload files: ZMODEM, YMODEM or XMODEM (or drop files on the window) |
| `Alt-G` | Download with YMODEM or XMODEM (ZMODEM downloads start by themselves) |
| `Alt-V`, `Shift-Ins` | Paste from the clipboard |
| `Alt-Enter` | Toggle fullscreen (fills the screen, keeping the shape) |
| `Alt-P` | Phosphor mode: off, color CRT, green screen, amber screen |
| `Alt-A` | Aspect ratio: square pixels, 4:3 as on a CRT, 16:10, 16:9 |
| `Alt-W` | 9-dot or 8-dot character cells |
| `Alt-M` | ANSI music: on, on including `ESC [ M`, off |
| `Alt-O` | Settings, and the interface colors |
| `Alt-F` | View ANSI art: a file picker, then the art with its SAUCE details |
| `Alt-Z` | Help: every key on one page |
| `Alt-X` | Exit (asks first; `Y` or `Alt-X` again to confirm) |

`Alt-P`, `Alt-A`, `Alt-W` and `Alt-M` work anywhere, including in the dialing
directory, and are remembered in `settings.toml`. An entry's "Screen mode"
can be 80x25, 80x43, 80x50, 132x25, 132x37 or 132x52 (43 lines or more in
the 8x8 font), and its "Phosphor mode" can override `Alt-P` for that
BBS. While connected to a BBS with its own phosphor mode, `Alt-P` changes
that BBS's mode rather than the global one. Its "iCE colors" setting is Auto (the BBS
turns them on with `CSI ?33h`, as before), On or Off; with On or Off, `Alt-I`
changes that BBS's setting too.

`Alt-O` (or `O` in the dialing directory) opens the settings, on four
tabs (`Tab` and `Shift-Tab`, or `PgUp`/`PgDn`, move between them).
**Folders** sets where downloads, captures and saved screens go and where
the upload and ANSI-art pickers start (`Enter` to change, `Del` for the
default). The others: phosphor
mode, its scanlines (off, light, medium or strong) and the curve of its
glass (off for a flat screen, slight, medium or strong), the aspect ratio,
scaling (fit keeping the shape; auto, which is whole numbers in a window
and fit in full screen; always whole numbers; or stretch to fill),
character cells, ANSI music, redialing, and the colors
of Phosphor's own screens. Pick one of the color schemes (Midnight, the
default; Classic blue, Gray windows, Green phosphor, Amber, Monochrome), or, under "Edit
colors", give each part of the interface (title bar, selected entry,
windows, input fields, key hints, ...) its own foreground and background
from a grid of the 16 x 8 combinations, as in QModem and Telix. Changes
show and are saved straight away; in the grid, `Esc` undoes the last part.

"Keyboard macros" sets what `Alt-1` to `Alt-0` send, written as in Telix
and QModem: `^M` is Enter, `^[` Esc and `^A` to `^Z` the other control
keys, `~` waits half a second, and `{user}` and `{password}` are the
directory entry's. `{user}^M~{password}^M`, for example, logs in.

Color themes can be shared. "Save as a theme" writes the current colors to
a file in the `themes` folder (`phosphor --paths` shows where), and "Import
a theme" copies someone else's file there and switches to it. Every theme
in the folder is in the color scheme list. A theme is a small TOML file,
easy to edit by hand; parts it leaves out keep the default colors:

```toml
name = "Sunrise"
author = "sysop"

[colors]
title_bar = { fg = "yellow", bg = "red" }
selected = { fg = "black", bg = "brown" }
window = { fg = "white", bg = "magenta" }
```

The colors are black, blue, green, cyan, red, magenta, brown, light gray,
dark gray, light blue, light green, light cyan, light red, light magenta,
yellow and white. The parts are `title_bar`, `border`, `headings`,
`entries`, `selected`, `info`, `help`, `help_key`, `status`, `status_alt`,
`window`, `window_title`, `window_text`, `field_name`, `field_value`,
`input`, `keys`, `note` and `error`.

"Monitor bezel" draws a monitor around the screen: a beige 1980s monitor,
a platinum 1990s one, a charcoal late-1990s one, or a terminal (original
designs, not any maker's). The picture then fills the monitor's 4:3 screen,
as it did on the real thing; it looks best with phosphor mode on. For your
own, put a PNG in the `bezels` folder, with the screen area see-through
(transparent): Phosphor finds the screen from the transparent part in the
middle and draws the terminal there. A photo of your own old monitor works
well.

"Check for updates" is off unless you turn it on. When it is on, Phosphor
asks git.badcluster.dev for the newest release once each time it starts
(one HTTPS request, with nothing in it but Phosphor's version), and if
there is a newer one says so on the status bar and the start screen, with
a link to the website. It never downloads or installs anything itself.

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

Entries with a user name log in by themselves over telnet (the entry's
"Auto-login" setting): once the BBS pauses at a prompt such as `Login:`,
`User name:` or Synchronet's `NN:`, Phosphor sends the user name, and then
the password at `Password:`. It answers each once, only in the first two
minutes of a call, and never sends the password before the user name.
`Alt-L` sends them by hand instead, one press each.

A BBS that can't be reached, or that answers and hangs up straight away
saying it is busy, is called again every 15 seconds, up to 10 times; `Esc`
stops it and `Alt-R` calls at once. Set `redial = false` in
`settings.toml` to turn this off.

Telnet over TLS checks the BBS's certificate against the usual certificate
authorities. Many BBSes use self-signed certificates, so one that fails
that check is trusted on first use and pinned in `tls_known_hosts` (beside
`known_hosts`), and a later, different certificate is refused; so is a
self-signed certificate from a BBS whose certificate an authority signed
before.

RLogin sends the entry's password as the "client user name" and its user
name as the "server user name", as SyncTERM does, which is how Synchronet
and other BBSes log RLogin callers in.

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

Copyright (C) 2026 Bad Cluster.

Phosphor is free software, licensed under the GNU General Public License,
version 3 or (at your option) any later version. See [LICENSE](LICENSE).

The bundled VGA fonts come from DOSBox (GPL-2.0-or-later). See
[crates/phosphor/assets/fonts/README.md](crates/phosphor/assets/fonts/README.md).
