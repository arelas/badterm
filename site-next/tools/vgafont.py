#!/usr/bin/env python3
"""Make "BadTerm VGA", a web font of the IBM VGA 8x16 glyphs BadTerm draws with.

    python3 -I site-next/tools/vgafont.py OUT.woff2

Reads crates/badterm/assets/fonts/vga_8x16.bin (256 glyphs x 16 bytes, CP437
order, most significant bit leftmost) and draws each lit pixel as a square, at
64 units a pixel: an em of 1024 is the 16-pixel cell, so at font-size 16px (or
32px, 48px...) every font pixel lands on whole screen pixels. The baseline is
four pixels above the cell's bottom. Each CP437 code is mapped to its Unicode
character, the control-code pictures (☺ ♥ ♪ ► ...) included.

The glyphs come from DOSBox Staging (GPL-2.0-or-later); see
crates/badterm-core/assets/README.md. Needs fontTools and brotli
(apt install python3-fonttools python3-brotli).
"""

import os
import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
BIN = os.path.join(REPO, "crates", "badterm", "assets", "fonts", "vga_8x16.bin")

PX = 64            # font units per pixel
W, H = 8, 16       # the cell
ASCENT = 12        # pixels above the baseline
FAMILY = "BadTerm VGA"

# CP437's codes 0x00-0x1F and 0x7F as the pictures the VGA draws for them.
LOW = "\u0000☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼"


def cp437(code):
    if code < 0x20:
        return LOW[code]
    if code == 0x7F:
        return "⌂"
    if code == 0xFF:
        return " "
    return bytes([code]).decode("cp437")


def rects(rows):
    """The lit pixels as rectangles: runs along each row, stacked where the
    rows below repeat the same run (so no two rectangles overlap)."""
    runs = []
    for y, bits in enumerate(rows):
        x = 0
        while x < W:
            if bits & (0x80 >> x):
                x0 = x
                while x < W and bits & (0x80 >> x):
                    x += 1
                runs.append((y, x0, x))
            else:
                x += 1
    out = []
    open_ = {}  # (x0, x1) -> [y0, y1]
    for y in range(H + 1):
        here = {(a, b) for (ry, a, b) in runs if ry == y}
        for key in list(open_):
            if key in here and open_[key][1] == y:
                open_[key][1] = y + 1
                here.discard(key)
            else:
                y0, y1 = open_.pop(key)
                out.append((key[0], y0, key[1], y1))
        for key in here:
            open_[key] = [y, y + 1]
    return out


def glyph(rows):
    pen = TTGlyphPen(None)
    for x0, y0, x1, y1 in rects(rows):
        # Pixel rows count down from the cell's top; font units count up from the baseline.
        top, bottom = (ASCENT - y0) * PX, (ASCENT - y1) * PX
        left, right = x0 * PX, x1 * PX
        pen.moveTo((left, bottom))
        pen.lineTo((left, top))
        pen.lineTo((right, top))
        pen.lineTo((right, bottom))
        pen.closePath()
    return pen.glyph()


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "badterm-vga.woff2"
    data = open(BIN, "rb").read()
    assert len(data) == 256 * H, len(data)
    names = [".notdef"] + [f"cp{c:02X}" for c in range(256)]
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    cmap = {}
    for c in range(256):
        glyphs[f"cp{c:02X}"] = glyph(data[c * H:(c + 1) * H])
        u = ord(cp437(c))
        if u:
            cmap.setdefault(u, f"cp{c:02X}")
    cmap.setdefault(0x20, "cp20")

    fb = FontBuilder(unitsPerEm=H * PX, isTTF=True)
    fb.setupGlyphOrder(names)
    fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyphs)
    # Every cell is 8 pixels wide; each glyph's left side bearing must be where
    # its first pixel really is. Renderers place the outline by it, so a 0 for
    # all would push narrow glyphs (i, l, !) to the cell's left edge.
    glyf = fb.font["glyf"]
    metrics = {}
    for n in names:
        g = glyf[n]
        g.recalcBounds(glyf)
        metrics[n] = (W * PX, g.xMin if g.numberOfContours else 0)
    fb.setupHorizontalMetrics(metrics)
    asc, desc = ASCENT * PX, (H - ASCENT) * PX
    fb.setupHorizontalHeader(ascent=asc, descent=-desc, lineGap=0)
    fb.setupOS2(sTypoAscender=asc, sTypoDescender=-desc, sTypoLineGap=0, usWinAscent=asc, usWinDescent=desc,
                fsSelection=0x40 | 0x80, achVendID="BCLU", version=4)
    fb.setupNameTable({
        "familyName": FAMILY,
        "styleName": "Regular",
        "uniqueFontIdentifier": "BadTerm VGA 8x16",
        "fullName": f"{FAMILY} Regular",
        "psName": "BadTermVGA-Regular",
        "version": "Version 1.000",
        "copyright": "IBM VGA 8x16 glyphs from DOSBox Staging, (C) 2002-2021 The DOSBox Team",
        "licenseDescription": "GNU General Public License, version 2 or (at your option) any later version",
    })
    fb.setupPost(isFixedPitch=1)
    fb.setupMaxp()
    fb.font.flavor = "woff2"
    fb.save(out)
    print(f"{out}: {len(cmap)} characters, {os.path.getsize(out)} bytes")


if __name__ == "__main__":
    main()
