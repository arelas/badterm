# Website

A static page: `index.html`, `style.css` and `img/`, published at
<https://badterm.com/>.

## Publishing

CI publishes the site after each release (the last step of the release
job in `.forgejo/workflows/ci.yml`). By hand:

```sh
site/deploy.py            # or --dry-run to build without uploading
```

It points the download buttons at the newest release's files, leaving out
any that release lacks (along with text marked `data-needs` for them), and
rsyncs the result to `/var/www/badterm` on oracle1, where nginx serves it
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
the renderer's snapshot test:

```sh
BADTERM_SNAPSHOT_DIR=/tmp/shots cargo test -p badterm snapshot -- --ignored
```

CRT screenshots are saved as JPEG (they are soft gradients), text-mode
ones as PNG.
