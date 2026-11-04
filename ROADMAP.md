# Roadmap

The goal is feature parity with the classic BBS clients: SyncTERM, NetRunner,
QModem and Telix. Items are roughly in priority order within each section.

## Punch list

The next things to do, in order.

- [ ] Try the newer features against the real thing: a modem (or tcpser or
      a WiFi modem), a RIPscrip BBS, an Avatar BBS, a Kermit download from a
      live board, Synchronet's graphical doors (SyncDoom: pictures, sound
      and the mouse), an IGS board, a SkyPix board
- [ ] Try each release on a real desktop on all three OSes. Done on
      0.1.4: calling Vortex, Alt-D while connected, Alt-Enter. Still to try:
      Cmd-D on macOS, the SSH login dialog and keychain, a ZMODEM download,
      mouse copy, Alt-S, directory search and sort, the bell's flash, the
      color schemes
- [ ] winget and Homebrew, updated from CI on each release

## Done (0.1)

- [x] Native window with pixel-exact CP437 VGA 8x16 font
- [x] ANSI-BBS emulation: CUP/CUU/CUD/CUF/CUB, CNL/CPL, CHA/HPA/VPA, ED/EL/ECH,
      ICH/DCH/IL/DL, SU/SD, DECSTBM, SCP/RCP, DECSC/DECRC, IND/RI/NEL, RIS,
      DSR/CPR, DECAWM, DECTCEM, OSC title, delayed last-column wrap
- [x] SGR: bold, faint, underline, blink, reverse, conceal; 16/256/24-bit color
- [x] iCE colors (`CSI ?33h` and manual toggle), blinking text, blinking cursor
- [x] Telnet: BINARY, ECHO, SGA, TTYPE, NAWS, RFC 1143-style loop prevention
- [x] Raw TCP connections
- [x] CP437 and UTF-8 encodings
- [x] Scrollback buffer, including screens cleared by the BBS
- [x] Session capture to file
- [x] Dialing directory: add/edit/delete, quick connect, call stats, pinned
      entries, starts with The Vortex BBS
- [x] Directory import/export: SyncTERM `.lst`, CSV, JSON, TOML
- [x] Clipboard paste, fullscreen, drag-and-drop import
- [x] SSH: none/password/keyboard-interactive auth, PTY, TOFU host key pinning
- [x] SSH public keys: entry key file (with passphrase prompt), agent
      (`SSH_AUTH_SOCK`, Windows OpenSSH agent, Pageant), default `~/.ssh` keys
- [x] Login dialog for passwords, passphrases and keyboard-interactive
      questions not stored in the entry, with retry on a wrong answer
- [x] Passwords stored in the OS keychain, falling back to the directory file
- [x] Device Attributes (`CSI c`) reply
- [x] Fullscreen (`Alt-Enter`) fills the screen with sharp fractional
      scaling
- [x] Mouse selection and copy, as text or ANSI
- [x] Directory search and sorting (name, last called, calls)
- [x] Save scrollback to `.ans`/`.txt`; save the screen as PNG
- [x] Application icon (window, Windows .exe, macOS bundle, Linux desktop file)
- [x] Packages: macOS `.app` in a `.dmg`, Linux AppImage
- [x] Phosphor mode: CRT glow, scanlines, curvature and vignette; color,
      green or amber
- [x] 9-dot VGA cells, 4:3 aspect correction, 80x50 per entry (8x8 font)
- [x] Phosphor mode per directory entry
- [x] ANSI music (`CSI N`, `CSI |`, optionally `CSI M`; BASIC PLAY syntax,
      square-wave synthesis; ALSA loaded at run time, cpal on macOS/Windows)
- [x] Website (`site/`): features, phosphor-mode screenshots, downloads,
      The Vortex BBS
- [x] `badterm --render`: ANSI file to PNG, with SAUCE iCE detection
- [x] ZMODEM download (auto-start on ZRQINIT) and upload (Alt-U, drag and
      drop, or auto-prompt on ZRINIT), batches, progress window, cancel
- [x] Website at badterm.com (site/deploy.py), published by CI on release
- [x] CI tests the OS keychain (Windows, macOS) and the Windows installer
- [x] Linux .deb and .rpm (nfpm), Windows installer (Inno Setup)
- [x] telnet://, telnets:// and rlogin:// links on Windows, macOS and Linux
- [x] RLogin, telnet over TLS (pinned like SSH host keys)
- [x] Auto-login (prompts or Alt-L), redial on busy
- [x] CTerm font selection and palette (OSC 4/104); Amiga fonts per entry
- [x] Screen modes 80x43, 132x25, 132x37, 132x52
- [x] Baud rate emulation
- [x] Settings screen (Alt-O) with interface color schemes and a color editor; help (Alt-Z)
- [x] Shareable color themes (import, save, a themes folder); monitor bezels, built in or PNG
- [x] XMODEM (checksum, CRC, 1K) and YMODEM (batch, G); a file picker for uploads
- [x] Keyboard macros (Alt-1 to Alt-0); EtherTerm, MagiTerm and Telnet BBS Guide imports
- [x] ANSI viewer with SAUCE (Alt-F, or a file on the command line)
- [x] NetRunner phone book import
- [x] ZMODEM crash recovery: interrupted downloads resume
- [x] PETSCII (Commodore 64) and ATASCII (Atari 8-bit) emulation with their own fonts
- [x] Named BadTerm; published on crates.io
- [x] Color schemes with their own palettes (Bad Cluster), and ones without
      fills (Dark blue, Dark cyan)
- [x] Website: help page, and an extras page of themes and bezels from
      `extras/`
- [x] Phosphor mode keeps colors true: the glow only spreads light onto
      darker neighbours

## Done (0.2 and 0.3)

- [x] Sixel graphics, as pixel tiles on the screen's cells
- [x] SyncTERM's extensions, identifying as CTerm 1.332: the file cache,
      PPM and JPEG XL pictures, pixel copy and paste, the audio channels
      (WAV, AIFF, VOC, FLAC, Ogg Opus and Vorbis), loadable fonts
- [x] xterm mouse reporting for BBSes and doors (9, 1000, 1002, 1003,
      1006, 1016)
- [x] Viewdata (Prestel), IGS (Atari ST) and SkyPix (Amiga)
- [x] The window keeps the picture's shape as it's resized, and opens at
      the size and place it was left
- [x] A new splash screen and application icon, as ANSI art
- [x] CI keeps compiled dependencies between runs, and builds a release
      once
- [x] The website's screenshots retaken (true colors in phosphor mode),
      with ones of Viewdata, IGS, SkyPix, sixel, SyncTERM pictures and
      code pages
## Connections

- [x] Serial port / modem dialing (`serialport` crate), AT command init strings

## Emulation

- [x] CTerm's loadable fonts, and xterm mouse reporting (9, 1000, 1002,
      1003, 1006, 1016)
- [x] CTerm's built-in code-page fonts (IDs 1-31), and bracketed paste
- [x] Typing, pasting and copying in the code page the BBS's font is in (all 31)
- [x] More fonts: CP437 8x14 (80x28), `.fnt`/`.psf` files; Amiga fonts in
      8-line modes
- [x] Device Attributes identify as CTerm (1.332), for SyncTERM's extensions
- [x] More screen modes: 40x25, 80x28, 80x30, 80x60 (not yet: resizable)
- [x] Avatar/0+ emulation
- [x] More retro systems: C128 80-column mode, PETSCII autologin and copy as text
- [x] Viewdata (Prestel) and BBC Micro Mode 7, with Reveal (`Alt-K`)
- [x] Atari ST IGS graphics (not yet: its sound, MIDI)
- [x] Amiga SkyPix graphics (not yet: samples, brushes sent by XMODEM, Amiga disk fonts)
- [x] Sixel graphics
- [x] SyncTERM's pictures: the file cache, PPM and JPEG XL drawing, pixel
      copy and paste, fonts from the cache (`SetFont`)
- [x] SyncTERM's audio APCs (WAV, AIFF, VOC, FLAC, MP3, Ogg Opus and
      Vorbis), and ANSI music in the foreground (`MF`, the default)
- [x] RIPscrip graphics, with icon and scene files (not yet: RIP file
      transfers, variables)
- [x] Visual and audible bell options

## File transfer

- [x] Kermit, with sliding windows
- [x] Per-entry upload/download directories

## Dialer and UI

- [x] Per-entry capture and text log files
- [x] More per-entry settings: hide the status line
- [x] Groups in the directory (`G`)
- [x] Import more formats: qodem
- [x] More in the settings screen: default font and screen mode, hotkeys
- [ ] Multiple simultaneous sessions (tabs)
- [x] Scripting (Rhai); per-entry macros
- [ ] More themes and bezels in `extras/`, from others
- [ ] Optional curses/TUI front end built on `badterm-core`

## Packaging

- [x] macOS Developer ID signing and notarization, and Windows signing
      (Azure Artifact Signing), in CI as ZephyrFTP does
- [ ] winget and Homebrew (see the punch list); Flathub
- [ ] An MSI for managed Windows deployments
