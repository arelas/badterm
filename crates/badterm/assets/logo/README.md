# BadTerm's logo

`badterm.ans` is the BADTERM wordmark as an 80x25 ANSI piece with iCE colours
and SAUCE. BAD is stacked over TERM, the two words welded under one keyline,
shaded in the negative-land style and powering up in a gold ki aura.

The bottom-left corner (columns 0-19, rows 14-24) is left empty. Whatever shows
the piece fills it with its own words:
- the app's splash screen (`src/ui/splash.rs`) puts the version and copyright there
- the website's front page (`site-next/tools/art.py`) puts its menu there

The piece is generated, not drawn by hand. The generator is
`site-next/tools/powerup.py` (Python 3 with numpy), with the letters from
`site-next/tools/letterpen.py`. To regenerate it:

    python3 -I site-next/tools/powerup.py crates/badterm/assets/logo/badterm.ans --bare
