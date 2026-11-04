# Assets

## vga_8x8.bin and vga_8x16.bin

The IBM VGA 8x8 and 8x16 fonts, which RIPscrip and IGS draw their text in
(see `src/rip.rs` and `src/igs.rs`): copies of
`crates/badterm/assets/fonts/vga_8x8.bin` and `vga_8x16.bin`, kept here so
the crate builds on its own. 256 glyphs x 8 (or 16) bytes in CP437 order,
most significant bit leftmost.

Extracted from the `int10_font_08` and `int10_font_16` tables in `src/ints/int10_memory.cpp` of
DOSBox Staging v0.80.0 (<https://github.com/dosbox-staging/dosbox-staging>),
Copyright (C) 2002-2021 The DOSBox Team, distributed under the GNU General
Public License version 2 or (at your option) any later version. It is used here
under the terms of GPL version 3 or later.

## font6x6.bin

BadTerm's own 6x6 font, for IGS's 8-point text (the size the Atari ST's
smallest system font draws): ASCII only, made by `make_font6.py` (run it to
change the font), 256 glyphs x 6 bytes.
