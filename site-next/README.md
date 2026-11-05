# badterm.com

The website: the 1990s ANSI scene in a modern page, live at
<https://badterm.com/> since 4 November 2026. The pages' text and pictures
come from [`site/`](../site/README.md) and [`extras/`](../extras/README.md);
this folder draws and builds them.

    python3 -I site-next/build.py            # build into site-next/dist
    python3 -I site-next/build.py --deploy   # build, then rsync to badterm.com

The release job runs the second after each release.

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
- **The art is ANSI, made by `tools/art.py`.** The front page's screen is
  BadTerm's logo, `crates/badterm/assets/logo/badterm.ans`, the same piece the
  app's splash screen shows, so the two can't drift apart. It is BAD over TERM
  in a gold ki aura, drawn by `tools/powerup.py` with the letters from
  `tools/letterpen.py` (numpy, run by hand; the build only reads the file). The
  corner the logo leaves free holds the tagline, the menu whose hotkeys the page
  makes clickable, and the prompt. The HELP and EXTRAS banners are the pages'
  titles in the same letters, stroke and ki (`powerup.py --banner WORD`, into
  `src/art/`; H, L, P, X and S were drawn for them), with a spaced-out line
  and the artist's tag (`bc!`) under them.

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
  topped by the ANSI HELP banner.
- **The extras page is built from `extras/`:** a card per theme
  (`themes/*.toml` plus its preview) and per bezel (`bezels/bezels.toml`), each
  with the real file name and size, BBS-listing style. The "share yours" text
  comes from `site/extras.html`. The page is topped by the ANSI EXTRAS banner.
- **The three pages share** `src/_nav.html` and `src/_footer.html`. The footer
  carries a SAUCE-style record for each page.
- **The logo kit is made by `tools/brand.py`** from the files the app ships:
  the logo (`crates/badterm/assets/logo/`) and the icon
  (`crates/badterm/assets/icon/`, made by its `make_icon.py`: the logo's B on a
  slate tile, without the ki, an ANSI piece of 32x16 cells). The build publishes it at `brand/`.
  The kit has:
  - the logo, as `.ans` and PNG, and as a poster with its own caption
  - the wordmark on one line, `src/img/badterm-line.svg`, made by
    `powerup.py --line`: a unit a half-cell, with crispEdges
  - the icon: macOS's, the square tile, its `.ans`, and as SVG
  - BAD over TERM in one color, white or black, for print
  - a 1200x630 card for shared links

  The site uses the kit for its favicon (SVG, a 64 px PNG and a 180 px
  home-screen icon), its link previews and the header's wordmark (27 px, a
  pixel a half-cell, without the ki, which crowds the letters at that size). `tools/iconlab.py` is the sheet of the directions the
  first icon (B▄, retired) was chosen from.

## Serving

`build.py` adds `?v=<hash>` to every file it links (Cloudflare keeps CSS for
a week and images for a month) and stamps the download buttons from the newest
release, as `site/deploy.py` does. The page has no inline script or style, so
badterm.com's CSP (`script-src 'self'; style-src 'self'`) applies unchanged.

The site is served from `/var/www/badterm` on oracle1 by nginx
(`/etc/nginx/sites-available/badterm.com`). Its pages have no inline script
or style, so the server's CSP applies unchanged. The old site is backed up
there in `~/backups/badterm-site-20261104-223338.tar.gz`.
