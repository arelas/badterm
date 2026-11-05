# Website content

badterm.com is built by [`site-next/build.py`](../site-next/README.md), which
draws the pages in their ANSI-scene style. This folder holds what it builds
them from:

- `help.html`: the guide. Only its `<article>` is used. The build takes the
  text from there, numbers the sections and makes the help menu from their
  headings, so the guide's text lives in this one place. Keep it in step with
  the app's keys and settings, and with the README.
- `extras.html`: only its "Add to the collection" text is used. The themes
  and bezels themselves come from the repository's
  [`extras/`](../extras/README.md) folder.
- `img/`: the screenshots (see below) and the old icons. The build replaces
  `favicon.png` and `icon.png` with the current icon.
- `fonts/`: IBM Plex Sans, served from the site itself because its
  Content-Security-Policy allows nothing else; see `fonts/OFL.txt`. The VGA
  web font is made by the build from BadTerm's own glyphs.
- `samples/`: the sample screens some screenshots are drawn from.

## Publishing

CI publishes the site after each release (the last step of the release
job in `.forgejo/workflows/ci.yml`). By hand:

```sh
python3 -I site-next/build.py            # build into site-next/dist
python3 -I site-next/build.py --deploy   # build and publish
```

The build needs Python 3.11+, Pillow, fontTools and brotli. It does these
things:
- points the download buttons at the newest release's files
- fills in the version
- builds the help and extras pages
- adds `?v=` and a hash of the file to every link to a file. Cloudflare and
  browsers keep those for a week or more, so a changed file needs a new
  address.
- rsyncs the result to `/var/www/badterm` on oracle1, where nginx serves it
  as badterm.com (`/etc/nginx/sites-available/badterm.com`, behind
  Cloudflare). The old address, badcluster.dev/phosphor/, redirects there.

CI uses the `SITE_DEPLOY_KEY` secret, a key that `ubuntu`'s
`authorized_keys` on oracle1 limits to `rrsync /var/www/badterm`, with
`BADTERM_SITE_DEST=ubuntu@oracle1.badcluster.dev:` (the root of that
folder).

## Screenshots

The BBS screenshots come from a real session, drawn by BadTerm itself:

1. Call a BBS and capture the screen you want with `Alt-C` (captures are
   saved where `badterm --paths` says).
2. Render it, for example in phosphor mode at 4:3 with 9-dot characters:

   ```sh
   badterm --render capture.ans vortex-color.png --screen --crt color --aspect --nine-dot
   ```

The text-mode screenshots (`dialer.png`, `find.png`, `login.png`) come from
the renderer's snapshot test, `badcluster.png` from its theme snapshot,
`rip.png` from its RIP snapshot, and `c64.png` and `atari.png` from its
retro snapshot, and `viewdata.png`, `igs.png` and `skypix.png` from the
snapshots below, of the sample screens in [`samples/`](samples/README.md):

```sh
BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm snapshot -- --ignored
```

They are saved as palette PNGs (a few kilobytes each).

The Vortex screenshots (`vortex-*.jpg`, `vortex-off.png` and `bezels.jpg`,
four renders with `--bezel` in a 2x2 grid) are from a frame of the Vortex's
own opening animation, captured as the steps above say, once it has drawn
its logo. The home page's "Call in" picture,
`site-next/src/img/vortex-welcome.png`, is its welcome screen, captured and
drawn by `site-next/tools/vortex.py`.

The other emulations have snapshots of their own, which take a file of what
a BBS sends (or, for Viewdata, draw a sample page):
`viewdata_snapshot` (`BADTERM_VIEWDATA`), `igs_snapshot` (`BADTERM_IGS`),
`skypix_snapshot` (`BADTERM_SKYPIX`) and `sixel_snapshot` (`BADTERM_SIXEL`,
which also takes SyncTERM's picture commands); see their notes in
`crates/badterm/src/render.rs`.

CRT screenshots are saved as JPEG (they are soft gradients), text-mode
ones as PNG.
