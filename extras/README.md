# Extras: themes, bezels and scripts

Color themes and monitor bezels for BadTerm, offered for download on
<https://badterm.com/extras.html>. Contributions are welcome: add a file,
check it with the tests, and send a pull request.

## Themes (`themes/*.toml`)

A theme file, as BadTerm's "Save as a theme" writes it (see the README's
"Color themes"), with a `description` line for the website:

```toml
name = "Sunset"
author = "Your name"
description = "One line about it."

[colors]
title_bar = { fg = "yellow", bg = "magenta" }
# ... the other parts
palette = ["#1B1026", ...]   # optional: its own 16 colors
```

Its name must not be a built-in scheme's.

## Bezels (`bezels/*.png`)

A PNG with a see-through screen area in the middle (BadTerm draws the
screen there, at the area's own shape), plus an entry in
`bezels/bezels.toml`. Only art you made yourself, or may share: no photos
of other people's work, and no makers' names or logos.

## Previews and checks

`previews/` holds the pictures the website shows. After adding a theme or a
bezel:

```sh
cargo test -p badterm extra            # every file loads
cargo test -p badterm make_extra_previews -- --ignored   # the previews
```

The four bezels here were drawn by `cargo test -p badterm make_extra_bezels
-- --ignored`, from the styles in `crates/badterm/src/bezel.rs`.

## Scripts (`scripts/*.rhai`)

Scripts in [Rhai](https://rhai.rs), as the README's "Scripts" describes.
`login.rhai` logs in to most boards; copy it to your scripts folder
(`badterm --paths`) and change it to suit.
