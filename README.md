# BadTerm

*The badass terminal.*

A cross-platform BBS client (telnet, SSH, RLogin and real modems) written in
Rust, in the tradition of SyncTERM, NetRunner, QModem and Telix. Runs
natively on Windows, macOS and Linux. Website, help and downloads:
<https://badterm.com/>.

BadTerm draws its own text-mode screen using real CP437 bitmap fonts, so ANSI
art renders pixel-for-pixel as it did on a VGA card. It doesn't depend on the
fonts and colors of a host terminal.

> **Status:** 0.3, in daily use. Telnet, telnet over TLS, SSH, RLogin, raw
> TCP and modem connections; ZMODEM, YMODEM, XMODEM and Kermit transfers;
> ANSI-BBS (as SyncTERM's CTerm, with its fonts, sixel, pictures, sound and
> the mouse), Avatar, RIPscrip, PETSCII, ATASCII, Viewdata, BBC Mode 7, IGS
> (Atari ST) and SkyPix (Amiga); and the dialing directory with import and export all
> work. See [ROADMAP.md](ROADMAP.md) for what's planned.

## Features

- **ANSI-BBS emulation**: cursor movement, erase/insert/delete, scroll regions,
  save/restore cursor, cursor position reports (used by BBSes to detect ANSI),
  CP437 "graphic" control characters, form feed clears the screen
- **Fonts**: the IBM VGA fonts; the Amiga fonts (Topaz, Topaz Plus,
  MicroKnight, MicroKnight Plus, P0T-NOoDLE, mO'sOul) for Amiga BBSes; and
  SyncTERM's 31 code-page fonts (Cyrillic in CP866, KOI8-R and CP1251,
  Greek, Hebrew, Turkish, Baltic, Central and West European, Armenian,
  CP850, CP865...) for BBSes in other languages: set per directory entry,
  or by the BBS with CTerm's font selection (`CSI n ; m SP D`, including
  the bright and blink font slots of `CSI ?31h` / `?34h`). With a
  code-page font on the screen, what you type and paste is sent in that
  code page (Cyrillic as CP866 or KOI8-R, say), and text you copy comes
  out as the characters it shows
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
- **A real modem**: entries whose protocol is Modem dial their phone number
  (the entry's address) on a modem on a serial port, a USB modem, a WiFi
  modem or tcpser. Set the port, speed, flow control, init string and dial
  command once in Settings (`Alt-O`, the Modem tab). The modem's replies
  show as it dials, a BUSY line is redialed like a busy BBS, and the call
  ends when the carrier drops: NO CARRIER.
- **SSH** (pure Rust, no OpenSSL): "none" auth for BBSes that run their own
  login, public keys (a key file set in the entry, your SSH agent, or
  `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa`), keyboard-interactive and
  password auth. Anything not stored, such as a password, a key passphrase or
  a one-time code, is asked for in a login dialog. Host keys are trusted on
  first use and pinned in BadTerm's own `known_hosts`; a changed key is
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
- **Kermit**, with long packets, CRC checks, compression and file sizes, as
  C-Kermit and G-Kermit speak it: a download starts by itself when the
  BBS's Kermit sends; to upload, start the BBS's Kermit receiving, then
  `Alt-U` and choose Kermit.
- **A file picker** for uploads: browse folders, mark several files with
  `Space`, or press `Tab` to type a path
- **An ANSI viewer** (`Alt-F`, `badterm file.ans`, or drop a file on the
  window when not in a call): scroll through art in its own width, font and
  colors, with its SAUCE title, artist, group, date and comments (`I`)
- **CP437 and UTF-8** sessions, switchable per entry or live with `Alt-E`
- **RIPscrip** graphics, for RIPterm-era BBSes (set the entry's emulation
  to RIPscrip): the 640x350 EGA scenes of lines, shapes, fills, patterns
  and text, with the 80x43 text screen over them, and clickable buttons and
  mouse areas that send their commands. BadTerm answers the BBS's RIP
  query (`ESC [ !`), so the BBS knows to send them. Not supported: icon
  and scene files, and RIP's own file transfers.
- **Sixel graphics**: pictures sent by the BBS (`ESC P q`) are drawn
  where the text cursor is, in up to 256 colors, as SyncTERM and xterm
  draw them. They scroll with the text, stay in the scrollback, and text
  written over them replaces them. BadTerm says it does sixel when asked
  (CTerm's attributes, `ESC [ < c`, and the graphics size, `ESC [ ? 2 ; 1 S`).
- **SyncTERM's pictures**, for Synchronet's graphical doors (SyncDoom and
  the like): BBSes can store files in a cache on your computer (a folder
  per BBS, in BadTerm's cache folder, at most 256 MB each) and draw PPM
  and JPEG XL pictures, from it or sent inline, anywhere on the screen,
  cropped, flipped, zoomed and masked, and copy and paste pixels. To get
  them, BadTerm identifies itself as CTerm (SyncTERM's emulation, revision
  1.332) when a BBS asks, and says which of its features it has.
- **AVATAR** (AVT/0 and AVT/0+), the compact codes of Opus, Maximus and
  other FidoNet-era BBSes, alongside ANSI: set the entry's emulation to
  Avatar
- **Commodore 64 (PETSCII) and Atari 8-bit (ATASCII)** BBSes, set by the
  entry's emulation: their 40-column screens, colors, character sets (the
  C64's two, switched by the BBS), reverse video and screen-editor codes,
  and their keys (Enter, DEL, the cursor keys, the C64's F1 to F8 and
  RUN/STOP on `Esc`). The fonts are BadTerm's own, in those machines'
  style
- **Viewdata (Prestel)** BBSes, such as Telstar, set by the entry's
  emulation: the 40x24 teletext page in its eight colors, with mosaic
  block graphics (contiguous and separated, held across color changes),
  double height, flashing and concealed text, and the UK character set.
  Attributes are worked out from the page as a real decoder does, so a
  BBS that rewrites part of a row gets the colors right. Enter sends the
  `#` key, and `Alt-K` reveals concealed text (answers to quizzes, say).
  **BBC Micro Mode 7**, the BBC's teletext screen, is an emulation of its
  own: 40x25, attributes as bytes, a scrolling screen, the BBC's own
  characters and keys.
- **IGS**, the graphics of Atari ST BBSes (Instant Graphics and Sound,
  version 2.19): VT-52 text and GEM drawing on the ST's screen (medium
  resolution, or low with sixteen colors): lines in their styles, widths
  and ends, boxes, circles, ellipses, arcs and pie slices, polygons and
  flood fills in the VDI's fill patterns and hatches, text in its sizes
  and effects, markers, blits, palettes, loops and clickable mouse zones.
  The ST's smallest text is in a 6x6 font of BadTerm's own. Color cycling
  (`X 8`) animates as on the ST, and a key cuts it, or a pause, short.
  Not yet: the sound chip's effects and music, and MIDI.
- **SkyPix**, the graphics of Amiga BBSes (SkyLine's, then C-Net's): ANSI
  text in Topaz on the Amiga's 640x200 screen in 16 colors, with SkyPix's
  drawing commands: pixels, lines, filled rectangles, ellipses, flood fills,
  brushes blitted with the Amiga's minterms, palettes, text placed at any
  pixel, delays and mouse clicks. Amiga disk fonts are drawn as Topaz at
  about their size. Not yet: samples, and brushes sent by XMODEM (refused).
- **Scrollback** of 10,000 lines. Screens cleared by the BBS are kept, so you can
  scroll back through menus you've already left.
- **Session capture** to `.ans` files (`Alt-C`), or for an entry from the
  start of each call, to a file name of your choosing (`{name}`, `{date}`
  and `{time}` are filled in). A `.txt` capture keeps only the text, as a
  log.
- **The bell** (Ctrl-G) beeps, flashes the screen, both, or neither, set in
  Settings and per directory entry
- **Copy with the mouse**: drag to select text and it is copied when you let
  go; hold `Shift` as you let go to copy it with its colors, as ANSI
- **The mouse for the BBS**: a BBS or door that asks for the mouse (xterm's
  modes 9, 1000, 1002, 1003, 1006 and 1016, as Synchronet's doors use them)
  gets your clicks, drags, movement and wheel, as text cells or pixels.
  Hold `Shift` to select and copy text instead. Pasting goes in brackets
  (`ESC [ 200 ~` ... `201 ~`) when the BBS asks for that (`CSI ? 2004 h`).
- **Fonts from the BBS**: fonts a BBS sends (CTerm's loadable fonts, 8x16,
  8x14 or 8x8, by `DCS`, the older `CSI = n ; n {` or from SyncTERM's
  cache) are used where the BBS selects them, stretched to the screen's
  cells if their height differs.
- **Save the scrollback** (`Alt-S`) as an `.ans` file for ANSI viewers and
  editors, or as plain `.txt`, or save the screen as a `.png`
- **Dialing directory** with add/edit/delete, quick connect, call counts,
  last-called dates, pinned favorites, search (`/`), and sorting by name,
  last called or number of calls (`S`). It starts with
  [The Vortex BBS](telnet://vortexbbs.com), the project's home BBS.
- **Passwords in the OS keychain** (Keychain on macOS, Credential Manager on
  Windows, the Secret Service on Linux) rather than the directory file. Where
  no keychain is available, they stay in the file.
- **Import/export** of SyncTERM `.lst`, CSV, JSON and TOML directories, and
  import from NetRunner, EtherTerm, MagiTerm and Qodem (`fonebook.txt`).
  Use the dialer, the command line, or drag a file onto the window.
- **ANSI music**: BBS tunes in the BASIC `PLAY` language (`ESC [ N` or
  `ESC [ |` up to Ctrl-N) play as PC-speaker square waves. As in BASIC and
  SyncTERM, a tune plays in the foreground (`MF`) unless the BBS says `MB`:
  what the BBS sends after it waits until it ends. `Alt-M` turns it off,
  or also on for `ESC [ M`, which many doors use but which otherwise means
  "delete a line"
- **SyncTERM's sound**, for Synchronet's doors: sound effects and music the
  BBS sends (WAV, AIFF, VOC, FLAC, MP3, and Ogg Opus or Vorbis, from the
  cache or inline) or has BadTerm synthesize, on 16 channels mixed together with
  fades, loops, crossfades and volume, as CTerm documents them. Long music
  is decoded in the background, so the screen doesn't wait for it.
  `Alt-M`'s "off" silences these too
- **Phosphor mode** (`Alt-P`): the look of a CRT, with glowing text,
  scanlines and curved glass, in color or as a green or amber monochrome
  monitor, for every BBS or chosen per directory entry. How much the text
  glows, how strong the scanlines are and how curved the glass is are each
  set in Settings. It starts off, with crisp pixels; turned on, it is a
  color CRT with a light glow, medium scanlines and flat glass. BadTerm
  starts with 8-dot characters and square pixels, fitted to the window in
  its own shape
- **VGA-accurate display options**: 9-dot character cells, where the
  line-drawing characters join up as they did on a VGA (`Alt-W`), and the
  4:3 shape of a CRT instead of square pixels (`Alt-A`). Entries can use
  80x43, 80x50 and 132-column screen modes.
- **Baud rate emulation**, per entry (300 to 115200 bps): text arrives at
  the speed it was drawn for, so ANSI animations play as intended.
  ZMODEM transfers still run at full speed.
- **The window**: the screen fills it, keeping its shape, and when you
  resize the window it settles into that shape too, so there are no
  borders (Settings, Scaling, also offers whole-number steps for exact
  pixels, or stretching). Fullscreen (`Alt-Enter`) and maximized windows
  fill the screen. BadTerm opens at the size and place you left it.
- Clipboard paste (`Alt-V`, `Shift-Ins`)

## Installing

Downloads are on the [website](https://badterm.com/) and the
[releases page](https://git.badcluster.dev/arelas/badterm/releases).

- **Windows**: run `badterm-…-windows-x64-setup.exe` (it and `badterm.exe`
  are code signed). It installs for you
  alone without administrator rights (or for everyone, if you choose), adds
  BadTerm to the Start menu and opens `telnet://`, `telnets://` and
  `rlogin://` links with it. Or unzip `badterm-…-windows-x64.zip` and run
  `badterm.exe` from anywhere.
- **macOS**: open `badterm-…-macos-arm64.dmg` and drag BadTerm to
  Applications. It is signed with an Apple Developer ID and notarized by
  Apple, so it opens like any other app. It opens `telnet://` links once it
  has been run.
- **Linux**: install `badterm-…-linux-x64.deb` (Debian, Ubuntu) or
  `badterm-…-linux-x64.rpm` (Fedora, RHEL, openSUSE), for example with
  `sudo apt install ./badterm-…-linux-x64.deb`. They, the AppImage and the
  tarball need glibc 2.28 or newer: Debian 10, Ubuntu 20.04, RHEL 8 and
  later. Or use the AppImage, which runs on most distributions: `chmod +x` it and run it. The tarball has the
  bare program, plus `badterm.desktop` and `badterm.png` for adding it to
  your menus by hand (copy them to `~/.local/share/applications/` and
  `~/.local/share/icons/hicolor/256x256/apps/`).

## Building

Install Rust (stable, 1.89 or newer) from <https://rustup.rs>, then:

```sh
cargo build --release
./target/release/badterm
```

No extra system libraries are required on Linux: X11 and Wayland are both
supported, and they and ALSA (for sound) are loaded at run time.

## Usage

BadTerm opens with its splash screen for two seconds (any key skips it;
Settings, General turns it off),
then a start-up banner with the version, as terminal programs did.

```
badterm                         start, not connected (Alt-D opens the dialing directory)
badterm bbs.example.org         connect directly (also host:port,
                                 telnet://host:port, telnets://host:port,
                                 rlogin://user@host, raw://host:port,
                                 ssh://user@host:port)
badterm --import syncterm.lst   add entries from a SyncTERM, NetRunner (.phn),
                                 Qodem (fonebook.txt), MagiTerm or EtherTerm
                                 directory, a CSV list (ours, or the Telnet BBS
                                 Guide's bbslist.csv), JSON or TOML
badterm --export bbses.csv      write the directory (format from extension)
badterm --paths                 show where data is stored (settings,
                                 directory, captures, downloads, themes,
                                 bezels, known hosts, the BBS file cache)
badterm --render art.ans out.png
                                 draw an ANSI file to a PNG (add --screen for
                                 just the last 80x25 screen, --crt color|green|
                                 amber, --glow, --scanlines, --curvature, --nine-dot,
                                 --aspect, --font for Amiga art, --bezel)
```

On Windows, BadTerm is a GUI program, so `cmd` doesn't wait for it and its
command-line output can appear after the next prompt. Use
`start /wait badterm --help` in `cmd`, or run it from PowerShell.

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
| `Alt-K` | Reveal concealed text (Viewdata and Mode 7) |
| `Alt-C` | Start/stop capturing to a file |
| `Alt-S` | Save the scrollback (`.ans`, `.txt`) or the screen (`.png`) |
| drag with the mouse | Copy the selected text (`Shift` on release: as ANSI) |
| `Alt-U` | Upload files: ZMODEM, YMODEM, XMODEM or Kermit (or drop files on the window) |
| `Alt-G` | Download with YMODEM, XMODEM or Kermit (ZMODEM and Kermit downloads start by themselves) |
| `Alt-V`, `Shift-Ins` | Paste from the clipboard |
| `Alt-Enter` | Toggle fullscreen (fills the screen, keeping the shape) |
| `Alt-P` | Phosphor mode: off, color CRT, green screen, amber screen |
| `Alt-A` | Aspect ratio: square pixels, 4:3 as on a CRT, 16:10, 16:9 |
| `Alt-W` | 9-dot or 8-dot character cells |
| `Alt-M` | Sound and music (ANSI music and SyncTERM's sound): on, on including `ESC [ M`, off |
| `Alt-O` | Settings, and the interface colors |
| `Alt-F` | View ANSI art: a file picker, then the art with its SAUCE details |
| `Alt-Z` | Help: every key on one page |
| `Alt-X` | Exit (asks first; `Y` or `Alt-X` again to confirm) |

`Alt-P`, `Alt-A`, `Alt-W` and `Alt-M` work anywhere, including in the dialing
directory, and are remembered in `settings.toml`, as are the window's size,
place (not on Wayland, which places windows itself) and whether it was
maximized, for the next start. An entry's "Screen mode"
can be 80x25, 80x43, 80x50, 132x25, 132x37 or 132x52 (43 lines or more in
the 8x8 font), and its "Phosphor mode" can override `Alt-P` for that
BBS. While connected to a BBS with its own phosphor mode, `Alt-P` changes
that BBS's mode rather than the global one. Its "iCE colors" setting is Auto (the BBS
turns them on with `CSI ?33h`, as before), On or Off; with On or Off, `Alt-I`
changes that BBS's setting too.

`Alt-O` (or `O` in the dialing directory) opens the settings, on five
tabs (`Tab` and `Shift-Tab`, or `PgUp`/`PgDn`, move between them).
**Folders** sets where downloads, captures and saved screens go and where
the upload and ANSI-art pickers start (`Enter` to change, `Del` for the
default). The others: phosphor
mode, its glow (off, light, medium or strong: how much bright text lights
the dark around it), its scanlines (off, light, medium or strong) and the
curve of its glass (off for a flat screen, slight, medium or strong), the aspect ratio,
scaling (fit keeping the shape; auto, which is whole numbers in a window
and fit in full screen; always whole numbers; or stretch to fill),
character cells, sound and music, the bell (a beep, a flash of the screen, both
or neither; an entry can have its own), redialing, the modem (see "Dialing
with a modem" below), and the colors of BadTerm's own screens. Pick one of the color schemes (Midnight, the
default; Classic blue, Gray windows, Green phosphor, Amber, Monochrome,
Ocean, Crimson, Plum, Dark blue and Dark cyan, which have no colored
backgrounds, or Bad Cluster, in the copper and teal of badcluster.dev), or,
under "Edit
colors", give each part of the interface (title bar, selected entry,
windows, input fields, key hints, ...) its own foreground and background
from a grid of the 16 x 8 combinations, as in QModem and Telix. Changes
show and are saved straight away; in the grid, `Esc` undoes the last part.

"Keyboard macros" sets what `Alt-1` to `Alt-0` send, written as in Telix
and QModem: `^M` is Enter, `^[` Esc and `^A` to `^Z` the other control
keys, `~` waits half a second, and `{user}` and `{password}` are the
directory entry's. `{user}^M~{password}^M`, for example, logs in.

Color themes can be shared. "Save as a theme" writes the current colors to
a file in the `themes` folder (`badterm --paths` shows where), and "Import
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

A theme can also bring its own 16 colors, in place of the VGA's, for
BadTerm's screens (the BBS's keep theirs), as the Bad Cluster scheme does:
`palette = ["#0E1015", "#161922", ...]` under `[colors]`, sixteen
`#RRGGBB` colors in the order above.

More themes, and more bezels, are on the website's
[extras page](https://badterm.com/extras.html), from the
[extras](extras/README.md) folder here, where you can add your own.

"Monitor bezel" draws a monitor around the screen: a beige 1980s monitor,
a platinum 1990s one, a charcoal late-1990s one, or a terminal (original
designs, not any maker's). The picture then fills the monitor's 4:3 screen,
as it did on the real thing; it looks best with phosphor mode on. For your
own, put a PNG in the `bezels` folder, with the screen area see-through
(transparent): BadTerm finds the screen from the transparent part in the
middle and draws the terminal there. A photo of your own old monitor works
well.

"Check for updates" is off unless you turn it on. When it is on, BadTerm
asks git.badcluster.dev for the newest release once each time it starts
(one HTTPS request, with nothing in it but BadTerm's version), and if
there is a newer one says so on the status bar and the start screen, with
a link to the website. It never downloads or installs anything itself.

Downloads are saved to `Downloads/BadTerm`; `badterm --paths` shows every
location BadTerm uses.

For SSH entries, set the user name in the entry. Many BBSes accept a user
like `new` or `guest` and then run their own login. BadTerm logs in with:

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
`User name:` or Synchronet's `NN:`, BadTerm sends the user name, and then
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

### Dialing with a modem

BadTerm dials out through a modem on a serial port, a USB modem, a WiFi
modem, or anything that acts like one (tcpser, for example). In Settings,
on the Modem tab, set the serial port (`/dev/ttyUSB0`, `/dev/cu.usbmodem…`,
`COM3`; `Enter` types it, `←` `→` step through the ports found), the speed
between the computer and the modem, the flow control (RTS/CTS for almost
every modem), the init string (`ATZ`, or `AT&F`) and the dial command
(`ATDT` for tone, `ATDP` for pulse). Then give a directory entry the
protocol Modem and the phone number as its address.

Calling it shows the modem's replies as it dials. BUSY, NO ANSWER and the
like are redialed as a busy BBS is; the call ends when the carrier drops
(DCD, or NO CARRIER where the port doesn't report it), and `Alt-H` drops
DTR, then sends `+++` and `ATH0`.

### Emulations

An entry's "Emulation" sets how what the BBS sends is shown: CP437
(ANSI-BBS, the usual), UTF-8, Avatar (AVT/0 and AVT/0+ alongside ANSI),
RIPscrip (graphics: click a button or mouse area to send its command; the
text screen is 80x43 over the 640x350 graphics), PETSCII (Commodore 64:
F1 to F8 are the C64's, `Esc` is RUN/STOP), ATASCII (Atari 8-bit),
Viewdata (Prestel: Enter is the `#` key), Mode 7 (BBC Micro), IGS (Atari
ST) or SkyPix (Amiga).

Passwords you enter in the directory, remember from the login dialog, or
import with a SyncTERM list are moved into the OS keychain when the
directory is saved. Exported directories don't include them.

Keys are sent the way ANSI-BBS software expects: arrows as `ESC [ A..D`,
`Home`/`End` as `ESC [ H` / `ESC [ K`, `PgUp`/`PgDn` as `ESC [ V` / `ESC [ U`,
`Ins` as `ESC [ @` and `Del` as DEL. The other emulations send their own
machine's keys: the C64's and Atari's codes, a Prestel keyboard's (`Enter`
is `#`), the BBC Micro's (`Delete` is DEL, the arrows 0x88 to 0x8B), and
the Atari ST's VT-52 arrows (`ESC A..D`).

## Website

`site/` holds the website: the home page, the help page and the extras
page of themes and bezels (built from `extras/`); see
[site/README.md](site/README.md).

## Project layout

```
crates/badterm-core   front-end independent library: CP437, screen model,
                       ANSI parser and the other emulations (RIPscrip,
                       Avatar, PETSCII, ATASCII, Viewdata, IGS, SkyPix),
                       sixel and SyncTERM's pictures and sound, telnet, SSH
                       and modems, transfers, dialing directory, sessions
crates/badterm        the windowed client (winit + softbuffer): renderer,
                       phosphor mode, the sound mixer, text-mode dialer and
                       settings
extras/               themes and bezels offered for download
site/                 the website
packaging/            installers and packages (Windows, macOS, Linux)
```

`badterm-core` has no GUI dependencies, so it can drive other front ends
(a TUI, a headless bot, a test harness) as well.

## Testing

```sh
cargo test
```

The ZMODEM interoperability tests run against lrzsz's `sz` and `rz` when
they are installed (`apt install lrzsz`, `brew install lrzsz`), and the
Kermit ones against C-Kermit (`apt install ckermit`); they are skipped
otherwise. CI sets `BADTERM_REQUIRE_LRZSZ=1` and `BADTERM_REQUIRE_KERMIT=1`
so they can't be skipped there. On Linux, a fake modem on a
pseudo-terminal tests dialing. In CI on Windows and macOS, saving passwords in the
OS keychain is tested too.

To render sample frames to images for visual inspection:

```sh
BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm snapshot -- --ignored
```

The extras' previews (and the bezels themselves) are drawn the same way;
see [extras/README.md](extras/README.md).

## License

Copyright (C) 2026 Bad Cluster.

BadTerm is free software, licensed under the GNU General Public License,
version 3 or (at your option) any later version. See [LICENSE](LICENSE).

The bundled VGA fonts come from DOSBox (GPL-2.0-or-later), and the Amiga
fonts from TrueSchool's collection (GPL with the font exception), the
code-page fonts from Synchronet's SyncTERM (LGPL-2.1-or-later); the C64,
Atari and 6x6 fonts are BadTerm's own. See
[crates/badterm/assets/fonts/README.md](crates/badterm/assets/fonts/README.md)
and [crates/badterm-core/assets/README.md](crates/badterm-core/assets/README.md).
IGS's fill patterns, line styles and markers are the GEM VDI's, as EmuTOS
(GPL-2.0-or-later) defines them.
The website uses IBM Plex, under the SIL Open Font License
([site/fonts/OFL.txt](site/fonts/OFL.txt)).
