# The new badterm.com (preview)

A rethink of the website: the 1990s ANSI scene in a modern page. Live as a
preview at <https://badterm.com/newsitedemo/>; the current site in `site/`
is untouched.

    python3 -I site-next/build.py            # build into site-next/dist
    python3 -I site-next/build.py --deploy   # build, then rsync to the preview

Needs Python 3.11+, Pillow, fontTools and brotli
(`apt install python3-pil python3-fonttools python3-brotli`).

## What's in it

- **The hero is a live terminal.** `src/app.js` is a small ANSI-BBS terminal:
  SGR colors (bold for bright, blink for iCE backgrounds), cursor moves, erase,
  the last column's delayed wrap, SAUCE. It draws an 80x25 screen in BadTerm's
  own VGA 8x16 glyphs, plus BadTerm's status bar in the same format as the app's.
  The page dials The Vortex BBS (`ATDT vortexbbs.com`, `CONNECT 33600`), then
  receives the logo at the chosen line speed. Any key or click skips the dial,
  and with reduced motion the screen shows up already drawn. Click the screen
  and the menu's keys work: D, F, S and C jump to their sections, P cycles
  phosphor mode, R redials.
- **The screen is drawn through WebGL.** With phosphor off it uses sharp
  scaling, so pixels stay crisp at any size. Phosphor on is a restrained CRT:
  glow onto darker neighbours, scanlines only at 2x and up, a slight curve,
  in color, green or amber. Without WebGL the screen is a plain canvas.
- **The art is ANSI, made by `tools/art.py`.** The logo is the app's splash
  letters, read from `crates/badterm/src/ui/splash.rs`, so the two can't drift
  apart. It's drawn the scene's way:
  - half-block pixels
  - one short ramp per object: ice (white, light cyan, cyan, blue) for BadTerm,
    gold (white, yellow, brown) for The Vortex BBS
  - a dark-gray drop shadow, and rules that fade out to dots
  - everything kept inside 79 columns
  - the artist's tag in the corner (`bc!`)

  The files are real `.ans`, each with a SAUCE record (iCE, 8-pixel spacing,
  legacy aspect), and `art/hero.png` is the no-script fallback.
- **The VGA font is BadTerm's own.** `tools/vgafont.py` turns
  `crates/badterm/assets/fonts/vga_8x16.bin` (from DOSBox Staging, GPL-2.0+)
  into a web font at 64 units a pixel. It is only ever set at 16, 32, 48 or
  64 px, so every font pixel lands on whole screen pixels. Running text is
  IBM Plex Sans.
- **Features are a BBS file area,** with real hotkeys and a blue lightbar on
  hover and focus. The phosphor section is a drag-to-compare slider.

## Serving

`build.py` adds `?v=<hash>` to every file it links (Cloudflare keeps CSS for
a week and images for a month) and stamps the download buttons from the newest
release, as `site/deploy.py` does. The page has no inline script or style, so
badterm.com's CSP (`script-src 'self'; style-src 'self'`) applies unchanged.

The preview lives outside `/var/www/badterm`, because the release job's
`rsync --delete` owns that folder. On oracle1, in
`/etc/nginx/sites-available/badterm.com` (backup in
`/etc/nginx/badterm.com.bak-20261105-020118`):

    location = /newsitedemo { return 301 /newsitedemo/; }
    location ^~ /newsitedemo/ { alias /var/www/badterm-newsitedemo/; }

`^~` keeps the site's `.css`, `.js` and image rules from catching the preview's
files. The block adds no headers of its own, so it inherits the server's
security headers. The page is marked `noindex`.
