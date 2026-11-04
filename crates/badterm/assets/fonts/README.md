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

## codepages_8x16.bin and codepages_8x8.bin

CTerm's built-in code-page fonts, IDs 1 to 31 in order (Cyrillic, Greek,
Hebrew, Turkish, Baltic, Central and West European, Armenian, CP850, CP865,
CP866, KOI8-R and -U and the rest, as SyncTERM numbers and names them in
`CSI n ; m SP D`): 31 fonts of 256 glyphs, 16 (or 8) bytes each. Extracted
from `conio_fontdata` in `src/conio/allfonts.c` of Synchronet
(<https://gitlab.synchro.net/main/sbbs>), part of its conio library,
Copyright Rob Swindell, distributed under the GNU Lesser General Public
License version 2.1 or (at your option) any later version, and used here
under the terms of GPL version 3 or later. Where it has no 8x8 version of a
font (IDs 1, 4, 5, 6, 10, 12, 13, 22, 27, 30 and 31), the 8x8 one here is
made from the 8x16 by joining each pair of rows.

## vga_8x14.bin and codepages_8x14.bin

The VGA's 8x14 font (CP437) and the code-page fonts at 8x14, for the
80x28 screen mode, from the same `allfonts.c`, under the same terms. Where
it has no 8x14 version of a code-page font (IDs 1, 4, 5, 6, 10, 12, 13, 17,
19, 22, 26, 27, 30 and 31), the one here is its 8x16 without the top and
bottom rows.

## amiga/amiga_8x8.bin and amiga/amiga_8x14.bin

The six Amiga fonts (CTerm IDs 37 to 42, in that order) at 8x8 and 8x14,
for the 43- and 50-line modes and 80x28, from the same `allfonts.c`,
under the same terms. Where it has none at a size, the one here is made
from the 8x16 (every other row for 8x8; without the top and bottom rows
for 8x14).
