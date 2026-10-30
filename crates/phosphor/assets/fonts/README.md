# Fonts

## vga_8x16.bin and vga_8x8.bin

The IBM VGA 8x16 and 8x8 text-mode fonts (the 8x8 one is for 50-line
modes), in CP437 order: 256 glyphs x 16 (or 8) bytes, one byte per scan
line with the most significant bit leftmost.

Extracted from the `int10_font_16` and `int10_font_08` tables in `src/ints/int10_memory.cpp` of
DOSBox Staging v0.80.0 (<https://github.com/dosbox-staging/dosbox-staging>),
Copyright (C) 2002-2021 The DOSBox Team, distributed under the GNU General
Public License version 2 or (at your option) any later version. It is used here
under the terms of GPL version 3 or later.
