# Website

A static site published at <https://badterm.com/>:

- `index.html`: the home page;
- `help.html`: how to use BadTerm (keep it in step with the app's keys and
  settings, and the README);
- `extras.html`: the themes and bezels to download, listed by `deploy.py`
  from the repository's [`extras/`](../extras/README.md) folder (in place of
  its `<!-- themes -->` and `<!-- bezels -->`);
- `style.css`, `img/`, and `fonts/` (IBM Plex, served from the site itself
  because its Content-Security-Policy allows nothing else; see
  `fonts/OFL.txt`).

Its look follows badcluster.dev: the void, copper and teal.

## Publishing

CI publishes the site after each release (the last step of the release
job in `.forgejo/workflows/ci.yml`). By hand:

```sh
site/deploy.py            # or --dry-run to build without uploading
```

It points the download buttons at the newest release's files, leaving out
any that release lacks (along with text marked `data-needs` for them),
fills in the version, builds the extras page, adds `?v=` and a hash of the
file to every link to a stylesheet, image, font or extra (Cloudflare and
browsers keep those for a week or more, so a changed file needs a new
address), and rsyncs the result to `/var/www/badterm` on oracle1, where nginx serves it
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
its logo.

The other emulations have snapshots of their own, which take a file of what
a BBS sends (or, for Viewdata, draw a sample page):
`viewdata_snapshot` (`BADTERM_VIEWDATA`), `igs_snapshot` (`BADTERM_IGS`),
`skypix_snapshot` (`BADTERM_SKYPIX`) and `sixel_snapshot` (`BADTERM_SIXEL`,
which also takes SyncTERM's picture commands); see their notes in
`crates/badterm/src/render.rs`.

CRT screenshots are saved as JPEG (they are soft gradients), text-mode
ones as PNG.
