# Website

A static page: `index.html`, `style.css` and `img/`, published at
<https://badcluster.dev/phosphor/>.

## Publishing

```sh
site/deploy.py            # or --dry-run to build without uploading
```

It points the download buttons at the newest release's files, leaving out
any that release lacks (along with text marked `data-needs` for them), and
rsyncs the result to `/var/www/phosphor` on oracle1. nginx serves that
folder at `/phosphor/` through a `location ^~ /phosphor/` block in the
`badcluster.dev` site; the rest of badcluster.dev is a separate repository.
Rerun it after each release.

## Screenshots

The BBS screenshots come from a real session, drawn by Phosphor itself:

1. Call a BBS and capture the screen you want with `Alt-C` (captures are
   saved where `phosphor --paths` says).
2. Render it, for example in phosphor mode at 4:3 with 9-dot characters:

   ```sh
   phosphor --render capture.ans vortex-color.png --screen --crt color --aspect --nine-dot
   ```

The text-mode screenshots (`dialer.png`, `find.png`, `login.png`) come from
the renderer's snapshot test:

```sh
PHOSPHOR_SNAPSHOT_DIR=/tmp/shots cargo test -p phosphor snapshot -- --ignored
```

CRT screenshots are saved as JPEG (they are soft gradients), text-mode
ones as PNG.
