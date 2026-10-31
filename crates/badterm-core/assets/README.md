# Assets

## vga_8x8.bin

The IBM VGA 8x8 font, which RIPscrip draws its text in (see `src/rip.rs`): a
copy of `crates/badterm/assets/fonts/vga_8x8.bin`, kept here so the crate
builds on its own. 256 glyphs x 8 bytes in CP437 order, most significant bit
leftmost.

Extracted from the `int10_font_08` table in `src/ints/int10_memory.cpp` of
DOSBox Staging v0.80.0 (<https://github.com/dosbox-staging/dosbox-staging>),
Copyright (C) 2002-2021 The DOSBox Team, distributed under the GNU General
Public License version 2 or (at your option) any later version. It is used here
under the terms of GPL version 3 or later.
