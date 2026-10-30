# Roadmap

The goal is feature parity with the classic BBS clients: SyncTERM, NetRunner
and mTelnet. Items are roughly in priority order within each section.

## Punch list

The next things to do, in order.

- [ ] Try each release on a real desktop on all three OSes. Done on
      0.1.4: calling Vortex, Alt-D while connected, Alt-Enter. Still to try:
      Cmd-D on macOS, the SSH login dialog and keychain, a ZMODEM download,
      mouse copy, Alt-S, directory search and sort
- [ ] Give the website its permanent home (it is at
      badcluster.dev/phosphor for now), and redeploy it from CI on release

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
- [x] `phosphor --render`: ANSI file to PNG, with SAUCE iCE detection
- [x] ZMODEM download (auto-start on ZRQINIT) and upload (Alt-U, drag and
      drop, or auto-prompt on ZRINIT), batches, progress window, cancel
- [x] Website at badcluster.dev/phosphor (site/deploy.py)
- [x] Linux .deb and .rpm (nfpm), Windows installer (Inno Setup)
- [x] telnet://, telnets:// and rlogin:// links on Windows, macOS and Linux
- [x] RLogin, telnet over TLS (pinned like SSH host keys)
- [x] Auto-login (prompts or Alt-L), redial on busy
- [x] CTerm font selection and palette (OSC 4/104); Amiga fonts per entry
- [x] Screen modes 80x43, 132x25, 132x37, 132x52
- [x] Baud rate emulation
- [x] Settings screen (Alt-O) with interface color schemes and a color editor; help (Alt-Z)

## Connections

- [ ] Serial port / modem dialing (`serialport` crate), AT command init strings

## Emulation

- [ ] More CTerm: `CSI = ...` modes, and the other font IDs (code pages)
- [ ] Loadable fonts: CP437 8x14, Commodore, Atari, other code pages (850,
      866, ...), custom `.fnt`/`.psf`; Amiga fonts in 8-line modes
- [ ] Configurable Device Attributes identity (e.g. CTerm, once its
      extensions are supported)
- [ ] More screen modes: 40-column, resizable
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

- [ ] More per-entry settings: iCE default, capture/log files, hide status line
- [ ] Multiple directories / groups
- [ ] Import more formats: NetRunner, mTelnet, Telnet BBS Guide lists, qodem
- [ ] SAUCE record reading for captured/viewed files; built-in ANSI viewer
- [ ] More in the settings screen: default font and screen mode, scaling,
      key bindings
- [ ] Multiple simultaneous sessions (tabs)
- [ ] Scripting / macros (e.g. Lua or Rhai) bound to keys
- [ ] Optional curses/TUI front end built on `phosphor-core`

## Packaging

- [ ] macOS Developer ID signing and notarization
- [ ] Flatpak; an MSI for managed Windows deployments
