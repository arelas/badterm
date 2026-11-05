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
- **The help page keeps one copy of its text:** the article and lede in
  `site/help.html`. `build.py` takes them from there, numbers the sections and
  builds the contents menu from those headings, in their order. The menu is
  sticky, and its lightbar follows the section in view. The layout adds VGA
  keycaps, the key lists as panels, and code as terminal panels. The page is
  topped by an ANSI HELP banner in the logo's ice ramp.
- **The extras page is built from `extras/`:** a card per theme
  (`themes/*.toml` plus its preview) and per bezel (`bezels/bezels.toml`), each
  with the real file name and size, BBS-listing style. The "share yours" text
  comes from `site/extras.html`. The page is topped by a gold EXTRAS banner.
- **The three pages share** `src/_nav.html` and `src/_footer.html`. The footer
  carries a SAUCE-style record for each page.
- **The logo kit is made by `tools/brand.py`** from the app icon's own drawing
  (`crates/badterm/assets/icon/make_icon.py`), and the build publishes it at
  `brand/`. The mark is B▄: the splash B and the logo's copper cursor. The
  wordmark is BADTERM▄. The kit has:
  - SVGs on a grid of 8 units a block, with crispEdges and the CP437 ▒ shadow
    as a pattern
  - an on-light wordmark, using the ramp's darker half
  - one-color white and black versions
  - horizontal and stacked lockups, the stacked one with the tagline in VGA
    pixels
  - a 1200x630 card for shared links

  The site uses the kit for its favicon (SVG, a 64 px PNG and a 180 px
  home-screen icon), its link previews and the header's wordmark (26 px,
  two pixels a block). `tools/iconlab.py` is the sheet of the four directions
  the mark was chosen from.

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
