# Roadmap

The goal is feature parity with the classic BBS clients: SyncTERM, NetRunner
and mTelnet. Items are roughly in priority order within each section.

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
- [x] Dialing directory: add/edit/delete, quick connect, call stats
- [x] Directory import/export: SyncTERM `.lst`, CSV, JSON, TOML
- [x] Clipboard paste, fullscreen, drag-and-drop import
- [x] SSH: none/password/keyboard-interactive auth, PTY, TOFU host key pinning
- [x] ZMODEM download (auto-start on ZRQINIT) and upload (Alt-U, drag and
      drop, or auto-prompt on ZRINIT), batches, progress window, cancel

## Connections

- [ ] SSH public-key authentication (agent and key files)
- [ ] Prompt for a missing password at connect time instead of failing
- [ ] RLogin
- [ ] Telnet over TLS (TelnetS)
- [ ] Auto-login: send stored user name/password on prompt
- [ ] Serial port / modem dialing (`serialport` crate), AT command init strings
- [ ] Reconnect / redial with retry on busy
- [ ] `telnet://` URL handler registration on each OS
- [ ] Baud rate emulation (300 to 115200) for authentic pacing of ANSI animation

## Emulation

- [ ] Device Attributes (`CSI c`) response, configurable identity
- [ ] ANSI music (`CSI M` / `CSI N` music strings, PC speaker synthesis)
- [ ] CTerm extensions: font selection (`CSI n;m SP D`), palette changes,
      `CSI = ...` modes, bright-background/bright-font modes
- [ ] Loadable fonts: CP437 8x8/8x14, Amiga Topaz/MicroKnight/P0T-NOoDLE,
      Commodore, Atari, other code pages (850, 866, ...), custom `.fnt`/`.psf`
- [ ] 9-pixel-wide VGA cells and 4:3 aspect correction
- [ ] Additional screen modes: 80x43, 80x50, 132x25, 132x37, 40-column, resizable
- [ ] Avatar/0+ emulation
- [ ] PETSCII (C64/C128) and ATASCII (Atari) emulation with their fonts
- [ ] Sixel graphics
- [ ] RIPscrip graphics
- [ ] Visual and audible bell options

## File transfer

- [ ] ZMODEM crash recovery (resume partial downloads)
- [ ] File picker dialog for uploads
- [ ] XMODEM / XMODEM-1K / XMODEM-CRC
- [ ] YMODEM / YMODEM-G
- [ ] Kermit
- [ ] Per-entry upload/download directories

## Dialer and UI

- [ ] Per-entry settings: screen mode, font, iCE default, baud emulation,
      capture/log files, hide status line
- [ ] Search/filter in the directory; sort by name, last called, calls
- [ ] Multiple directories / groups, favorites
- [ ] Import more formats: NetRunner, mTelnet, Telnet BBS Guide lists, qodem
- [ ] Mouse selection and copy (as text or ANSI)
- [ ] Save scrollback to `.ans`/`.txt`; save screen as PNG
- [ ] SAUCE record reading for captured/viewed files; built-in ANSI viewer
- [ ] Configuration screen (default font, scaling, palette, keys)
- [ ] Optional password storage in the OS keychain (`keyring` crate)
- [ ] Multiple simultaneous sessions (tabs)
- [ ] Scripting / macros (e.g. Lua or Rhai) bound to keys
- [ ] Optional curses/TUI front end built on `phosphor-core`

## Packaging

- [ ] Release builds for Windows (MSI/zip), macOS (signed `.app`/`.dmg`),
      Linux (AppImage, `.deb`, Flatpak)
- [ ] Application icon and desktop integration
