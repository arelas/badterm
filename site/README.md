# Website

A static page: `index.html`, `style.css` and `img/`. No build step;
serve the folder from any web server.

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
