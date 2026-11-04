# Roadmap

The goal is feature parity with the classic BBS clients: SyncTERM, NetRunner,
QModem and Telix. Items are roughly in priority order within each section.

## Punch list

The next things to do, in order.

- [ ] Try the newer features against the real thing: a modem (or tcpser or
      a WiFi modem), a RIPscrip BBS, an Avatar BBS, a Kermit download from a
      live board
- [ ] Try each release on a real desktop on all three OSes. Done on
      0.1.4: calling Vortex, Alt-D while connected, Alt-Enter. Still to try:
      Cmd-D on macOS, the SSH login dialog and keychain, a ZMODEM download,
      mouse copy, Alt-S, directory search and sort, the bell's flash, the
      color schemes
- [ ] winget and Homebrew, updated from CI on each release
- [ ] Retake the website's phosphor-mode screenshots (`vortex-*.jpg`,
      `bezels.jpg`): they predate 0.1.23, whose glow no longer washes out
      the colors (steps in site/README.md)

## Done (0.1)

- [x] Native window with pixel-exact CP437 VGA 8x16 font and integer scaling
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
- [x] Device Attributes (`CSI c`) reply, as a VT100
- [x] Fullscreen (`Alt-Enter`) fills the screen with sharp fractional
      scaling; normal windows keep exact integer scaling
- [x] Mouse selection and copy, as text or ANSI
- [x] Directory search and sorting (name, last called, calls)
- [x] Save scrollback to `.ans`/`.txt`; save the screen as PNG
- [x] Application icon (window, Windows .exe, macOS bundle, Linux desktop file)
- [x] Packages: macOS `.app` in a `.dmg` (signed ad hoc), Linux AppImage
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

## Connections

- [x] Serial port / modem dialing (`serialport` crate), AT command init strings

## Emulation

- [x] CTerm's loadable fonts, and xterm mouse reporting (9, 1000, 1002,
      1003, 1006, 1016)
- [ ] More CTerm: the other built-in font IDs (code pages), bracketed paste
- [ ] Loadable fonts: CP437 8x14, Commodore, Atari, other code pages (850,
      866, ...), custom `.fnt`/`.psf`; Amiga fonts in 8-line modes
- [x] Device Attributes identify as CTerm (1.332), for SyncTERM's extensions
- [ ] More screen modes: 40-column, resizable
- [x] Avatar/0+ emulation
- [ ] More retro systems: C128 80-column mode, PETSCII autologin and copy as text
- [x] Viewdata (Prestel) (not yet: BBC Micro Mode 7, revealing concealed text)
- [x] Atari ST IGS graphics (not yet: its sound, MIDI)
- [x] Amiga SkyPix graphics (not yet: samples, brushes sent by XMODEM, Amiga disk fonts)
- [x] Sixel graphics
- [x] SyncTERM's pictures: the file cache, PPM and JPEG XL drawing, pixel
      copy and paste (not yet: fonts from the cache, `SetFont`)
- [x] SyncTERM's audio APCs (WAV, AIFF, VOC, FLAC, Ogg Opus and Vorbis;
      not yet: MP3, or music played in the foreground, `MF`)
- [x] RIPscrip graphics (not yet: icon and scene files, RIP file transfers)
- [x] Visual and audible bell options

## File transfer

- [x] Kermit (not yet: sliding windows)
- [ ] Per-entry upload/download directories

## Dialer and UI

- [x] Per-entry capture and text log files
- [ ] More per-entry settings: hide status line
- [ ] Multiple directories / groups
- [x] Import more formats: qodem
- [ ] More in the settings screen: default font and screen mode, key bindings
- [ ] Multiple simultaneous sessions (tabs)
- [ ] Scripting (e.g. Lua or Rhai); per-entry macros
- [ ] More themes and bezels in `extras/`, from others
- [ ] Optional curses/TUI front end built on `badterm-core`

## Packaging

- [x] macOS Developer ID signing and notarization, and Windows signing
      (Azure Artifact Signing), in CI as ZephyrFTP does
- [ ] winget and Homebrew (see the punch list); Flathub
- [ ] An MSI for managed Windows deployments
