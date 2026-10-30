#!/usr/bin/env python3
"""Build Phosphor's 8x8 fonts for Commodore 64 (PETSCII) and Atari 8-bit
(ATASCII) BBSes: c64_upper.bin, c64_lower.bin and atari.bin.

They are Phosphor's own drawings in the style of those machines, not copies
of their character ROMs: the letters, digits and punctuation come from the
VGA 8x8 font beside this script (vga_8x8.bin, from DOSBox, GPL), whose
two-pixel strokes are already those of the C64 and Atari fonts, and the graphics
characters are drawn from rules below, character by character.

Which character is where comes from Rebecca Bettencourt's mapping tables
(https://github.com/kreativekorp/charset, mappings/legacy: C64VPRI, C64VALT
and ATARI8IG), which give each glyph as a Unicode character; the lists are
copied below, as code points.

Each font is 256 glyphs x 8 bytes: the 128 characters, then the same
inverted (the C64 and Atari show reverse video that way).

    python3 make_retro_fonts.py   # writes the .bin files next to it
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

# Screen code (C64) or ATASCII code (Atari) -> Unicode code point.
C64_UPPER = [
    0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F,
    0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0xA3, 0x5D, 0x2191, 0x2190,
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
    0x2500, 0x2660, 0x1FB72, 0x1FB78, 0x1FB77, 0x1FB76, 0x1FB7A, 0x1FB71, 0x1FB74, 0x256E, 0x2570, 0x256F, 0x1FB7C, 0x2572, 0x2571, 0x1FB7D,
    0x1FB7E, 0x25CF, 0x1FB7B, 0x2665, 0x1FB70, 0x256D, 0x2573, 0x25CB, 0x2663, 0x1FB75, 0x2666, 0x253C, 0x1FB8C, 0x2502, 0x3C0, 0x25E5,
    0xA0, 0x258C, 0x2584, 0x2594, 0x2581, 0x258F, 0x2592, 0x2595, 0x1FB8F, 0x25E4, 0x1FB87, 0x251C, 0x2597, 0x2514, 0x2510, 0x2582,
    0x250C, 0x2534, 0x252C, 0x2524, 0x258E, 0x258D, 0x1FB88, 0x1FB82, 0x1FB83, 0x2583, 0x1FB7F, 0x2596, 0x259D, 0x2518, 0x2598, 0x259A,
]
C64_LOWER = (
    [0x40] + list(range(0x61, 0x7B)) + [0x5B, 0xA3, 0x5D, 0x2191, 0x2190]
    + list(range(0x20, 0x40))
    + [0x2500] + list(range(0x41, 0x5B)) + [0x253C, 0x1FB8C, 0x2502, 0x1FB96, 0x1FB98]
    + C64_UPPER[0x60:0x7A] + [0x2713] + C64_UPPER[0x7B:0x80]
)
C64_LOWER[0x5F] = 0x1FB98
C64_LOWER[0x69] = 0x1FB99
ATARI = [
    0x2665, 0x251C, 0x1FB87, 0x2518, 0x2524, 0x2510, 0x2571, 0x2572, 0x25E2, 0x2597, 0x25E3, 0x259D, 0x2598, 0x1FB82, 0x2582, 0x2596,
    0x2663, 0x250C, 0x2500, 0x253C, 0x25CF, 0x2584, 0x258E, 0x252C, 0x2534, 0x258C, 0x2514, 0x241B, 0x2191, 0x2193, 0x2190, 0x2192,
] + list(range(0x20, 0x60)) + [0x2666] + list(range(0x61, 0x7B)) + [0x2660, 0x7C, 0x1F8B0, 0x25C0, 0x25B6]

# The glyphs of CP437 0x00-0x1F (the VGA font draws them).
CP437_LOW = "\0☺☻♥♦♣♠•◘○◙♂♀♪♫☼" \
            "►◄↕‼¶§▬↨↑↓→←∟↔▲▼"


def vga():
    data = open(os.path.join(HERE, "vga_8x8.bin"), "rb").read()
    table = {}
    for b in range(256):
        if b < 32:
            ch = CP437_LOW[b]
        elif b == 127:
            ch = "⌂"
        else:
            ch = bytes([b]).decode("cp437")
        table.setdefault(ord(ch), list(data[b * 8:b * 8 + 8]))
    return table


def grid(f):
    """A glyph from f(x, y) -> bool, x and y 0-7 from the top left."""
    return [sum(0x80 >> x for x in range(8) if f(x, y)) for y in range(8)]


def thick_h(y):
    return lambda x, yy: yy in (y, y + 1)


def thick_v(x):
    return lambda xx, y: xx in (x, x + 1)


def union(*fs):
    return lambda x, y: any(f(x, y) for f in fs)


def checker(x, y):
    return (x + y) % 2 == 0


# Lines are two pixels thick, centered on rows/columns 3-4.
H = thick_h(3)
V = thick_v(3)
UP = lambda x, y: x in (3, 4) and y <= 4
DOWN = lambda x, y: x in (3, 4) and y >= 3
LEFT = lambda x, y: y in (3, 4) and x <= 4
RIGHT = lambda x, y: y in (3, 4) and x >= 3


def arc(cx, cy):
    """A rounded corner: a quarter circle of radius 4 centered on a corner
    of the cell ((0|8, 0|8)), two pixels thick."""
    def f(x, y):
        d = ((x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2) ** 0.5
        return 3.0 <= d <= 5.2
    return f


def circle(filled):
    def f(x, y):
        d = ((x - 3.5) ** 2 + (y - 3.5) ** 2) ** 0.5
        return d <= 3.2 if filled else 2.0 <= d <= 3.4
    return f


def tri(corner):
    """A half-cell triangle filling the named corner."""
    return {
        "ul": lambda x, y: x + y <= 7,
        "ur": lambda x, y: x >= y,
        "ll": lambda x, y: x <= y,
        "lr": lambda x, y: x + y >= 7,
    }[corner]


DRAWN = {
    0x00A0: lambda x, y: False,
    0x2500: H, 0x2502: V,
    0x253C: union(H, V),
    0x251C: union(V, RIGHT), 0x2524: union(V, LEFT),
    0x252C: union(H, DOWN), 0x2534: union(H, UP),
    0x250C: union(RIGHT, DOWN), 0x2510: union(LEFT, DOWN),
    0x2514: union(RIGHT, UP), 0x2518: union(LEFT, UP),
    0x256D: arc(8, 8), 0x256E: arc(0, 8), 0x256F: arc(0, 0), 0x2570: arc(8, 0),
    0x2571: lambda x, y: abs((7 - x) - y) <= 0 or abs((7 - x) - y + 1) <= 0,
    0x2572: lambda x, y: abs(x - y) <= 0 or abs(x - y - 1) <= 0,
    0x2573: lambda x, y: x == y or x == y + 1 or 7 - x == y or 8 - x == y,
    0x25CF: circle(True), 0x25CB: circle(False),
    0x25E4: tri("ul"), 0x25E5: tri("ur"), 0x25E3: tri("ll"), 0x25E2: tri("lr"),
    # Block elements.
    0x2580: lambda x, y: y < 4, 0x2584: lambda x, y: y >= 4,
    0x2588: lambda x, y: True,
    0x258C: lambda x, y: x < 4, 0x2590: lambda x, y: x >= 4,
    0x2581: lambda x, y: y == 7, 0x2582: lambda x, y: y >= 6, 0x2583: lambda x, y: y >= 5,
    0x2594: lambda x, y: y == 0, 0x2595: lambda x, y: x == 7,
    0x258F: lambda x, y: x == 0, 0x258E: lambda x, y: x <= 1, 0x258D: lambda x, y: x <= 2,
    0x1FB82: lambda x, y: y <= 1, 0x1FB83: lambda x, y: y <= 2,
    0x1FB87: lambda x, y: x >= 6, 0x1FB88: lambda x, y: x >= 5,
    0x2592: checker,
    0x1FB8C: lambda x, y: x < 4 and checker(x, y),
    0x1FB8F: lambda x, y: y >= 4 and checker(x, y),
    0x1FB96: lambda x, y: not checker(x, y),
    # Diagonal fills: the C64's "upper left to lower right" half shading.
    0x1FB98: lambda x, y: (x - y) % 4 == 0 or (x - y) % 4 == 1,
    0x1FB99: lambda x, y: (x + y) % 4 == 0 or (x + y) % 4 == 1,
    0x2596: lambda x, y: x < 4 and y >= 4, 0x2597: lambda x, y: x >= 4 and y >= 4,
    0x2598: lambda x, y: x < 4 and y < 4, 0x259D: lambda x, y: x >= 4 and y < 4,
    0x259A: lambda x, y: (x < 4) == (y < 4),
    # One-eighth lines at a position (Symbols for Legacy Computing),
    # drawn two pixels wide.
    **{0x1FB70 + i: thick_v(i + 1) for i in range(6)},
    **{0x1FB76 + i: thick_h(i + 1) for i in range(6)},
    0x1FB7C: lambda x, y: x <= 1 or y >= 6,
    0x1FB7D: lambda x, y: x <= 1 or y <= 1,
    0x1FB7E: lambda x, y: x >= 6 or y <= 1,
    0x1FB7F: lambda x, y: x >= 6 or y >= 6,
    0x2713: lambda x, y: (x, y) in {(6, 1), (7, 1), (5, 2), (6, 2), (4, 3), (5, 3), (0, 3), (1, 4), (3, 4), (4, 4),
                                    (1, 5), (2, 5), (3, 5), (2, 6)},
    # Atari: the escape symbol, and "up, then left" (clear screen).
    0x241B: lambda x, y: (y in (1, 6) and 1 <= x <= 6) or (x in (1, 6) and 1 <= y <= 6) or (y == 3 and 3 <= x <= 4),
    0x1F8B0: lambda x, y: (x == 5 and 2 <= y <= 6) or (y == 2 and 1 <= x <= 5) or (x in (2,) and y in (1, 3))
    or (x == 1 and y == 2),
    0x25C0: lambda x, y: x >= 2 and abs(y - 3.5) <= (x - 2) * 0.7 + 0.5 and x <= 7,
    0x25B6: lambda x, y: x <= 5 and abs(y - 3.5) <= (5 - x) * 0.7 + 0.5,
}


def build(codes):
    table = vga()
    out = bytearray()
    missing = []
    for code in codes:
        if code in DRAWN:
            rows = grid(DRAWN[code])
        elif code in table:
            rows = table[code]
        else:
            missing.append(hex(code))
            rows = [0] * 8
        out.extend(rows)
    if missing:
        sys.exit(f"no glyph for {', '.join(missing)}")
    assert len(out) == 128 * 8
    out.extend(b ^ 0xFF for b in out[:])
    return bytes(out)


def main():
    for name, codes in (("c64_upper", C64_UPPER), ("c64_lower", C64_LOWER), ("atari", ATARI)):
        assert len(codes) == 128, (name, len(codes))
        with open(os.path.join(HERE, f"{name}.bin"), "wb") as f:
            f.write(build(codes))
        print(f"{name}.bin")


if __name__ == "__main__":
    main()
