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

## amiga/*.bin

Amiga fonts for Amiga BBSes and art, chosen per directory entry or by the
BBS with CTerm's `CSI n ; m SP D` (IDs as SyncTERM numbers them: 37
P0T-NOoDLE, 38 mO'sOul, 39 MicroKnight Plus, 40 Topaz Plus, 41 MicroKnight,
42 Topaz). Each is 256 glyphs x 16 bytes in ISO-8859-1 order, like the VGA
files.

The `raw/` files of TrueSchool's "Multi Platform Amiga Fonts" v1.02 by dMG
of TrueSchool and Divine Stylers (<https://github.com/rewtnull/amigafonts>,
commit d42987535d94ebbfb51e64fee76f9974c6ae145a), renamed. Its README
releases them under the GPL with the font exception
(<https://www.gnu.org/licenses/gpl-faq.html#FontException>) and credits:

- Topaz (Kickstart 1.x: `topaz_a500`, 2.x: `topaz_a1200`), © Amiga Inc.;
  these versions and Topaz Plus © dMG/t!s^dS!
- P0T-NOoDLE © Leo "Nudel" Davidson
- MicroKnight © Niels Krogh "Nölb/Grafictive" Mortensen; MicroKnight Plus
  © dMG/t!s^dS!
- mO'sOul © Desoto/Mo'Soul

The Kickstart 2.x Topaz is the one used for CTerm's "Topaz" and "Topaz
Plus"; the 1.x versions are kept for a future font menu.

## c64_upper.bin, c64_lower.bin and atari.bin

8x8 fonts for Commodore 64 (PETSCII) and Atari 8-bit (ATASCII) BBSes: the
C64's upper case/graphics and lower/upper case sets in screen-code order,
and the Atari's in ATASCII order, each 128 glyphs followed by the same 128
inverted. They are BadTerm's own, made by `make_retro_fonts.py`, not
copies of those machines' character ROMs: the letters, digits and
punctuation are the VGA 8x8 font's above, and the graphics characters are
drawn by rules in the script (so is the Atari's `|`, a solid bar there
where the VGA's has a gap). Which character goes where follows Rebecca
Bettencourt's mapping tables (https://github.com/kreativekorp/charset).
