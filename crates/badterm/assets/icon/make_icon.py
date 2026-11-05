#!/usr/bin/env python3
"""Draw BadTerm's application icon: its mark, "B▄", on a dark tile.

The mark is the first letter of the BADTERM▄ logo and its cursor. The B is
the splash screen's own (crates/badterm/src/ui/splash.rs): 9 x 12 blocks in
its ice ramp, as if lit from above (a white highlight, light cyan, cyan, deep
blue). The cursor is a copper bar at its baseline, a block's gap after it, in
VGA yellow over brown. Both cast a drop shadow down and to the right. On the
big sizes the shadow is drawn with the CP437 "▒" dither, as TheDraw-era logos
drew theirs, and the tile has a faint "░" texture, so it reads as a text-mode
screen.

Every size draws whole blocks of whole pixels, so the mark stays crisp:
  - 16 and 24 px use the splash B a pixel a block (without its shadow at 16).
  - 32 and 48 px use a compact B (6 x 8 blocks, the same shape), so the mark
    neither crowds the tile nor shrinks to half of it.
  - 64 px and up use the splash B again, larger.
macOS sizes from 64 up sit on Apple's icon grid (an 824-in-1024 squircle with
room for its shadow). Windows' and Linux's fill the tile, as taskbars and tabs
expect.

Writes badterm-<size>.png for each size (the full tile), badterm.ico
(Windows), badterm.icns (macOS, on Apple's grid) and badterm-64.rgba (raw
pixels for the window icon, so the app needs no PNG decoder) next to this
script. Needs Pillow:

    python3 make_icon.py
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
FONT8 = (HERE.parent / "fonts" / "vga_8x8.bin").read_bytes()
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]

VGA = {15: (255, 255, 255), 11: (85, 255, 255), 3: (0, 170, 170), 1: (0, 0, 170), 8: (85, 85, 85),
       14: (255, 255, 85), 6: (170, 85, 0)}

# The splash's B, 9 x 12 blocks, and its ramp by row.
SPLASH_B = [
    "#######..", "########.", "###...###", "###...###", "###..###.", "#######..",
    "########.", "###...###", "###...###", "###...###", "########.", "#######..",
]
SPLASH_RAMP = [15, 11, 11, 11, 11, 11, 3, 3, 3, 1, 1, 1]
# The same B at 6 x 8, for 32 and 48 px: two-block strokes, the corners cut
# the same way, the ramp in the same proportions.
COMPACT_B = ["#####.", "######", "##..##", "#####.", "######", "##..##", "######", "#####."]
COMPACT_RAMP = [15, 11, 11, 11, 3, 3, 1, 1]


def mark(compact=False):
    """The mark as (rows, color_at): the B, a block's gap, the cursor."""
    letter, ramp = (COMPACT_B, COMPACT_RAMP) if compact else (SPLASH_B, SPLASH_RAMP)
    w, h = len(letter[0]), len(letter)
    cw, ch = (3, 1) if compact else (4, 2)          # the cursor: under a third of the letter's height
    rows = [r + "." + ("#" * cw if y >= h - ch else "." * cw) for y, r in enumerate(letter)]
    cursor_x = w + 1

    def color_at(x, y):
        if x >= cursor_x:
            return VGA[14] if (ch == 2 and y == h - 2) else VGA[6] if ch == 2 else VGA[14]
        return VGA[ramp[y]]
    return rows, color_at


TILE_TOP, TILE_BOTTOM = (24, 28, 38), (9, 11, 16)
TEXTURE = (30, 36, 50)
RIM = (54, 64, 86)
SHADOW = VGA[8]


# ---------------------------------------------------------------- drawing

def lit(rows, x, y):
    return 0 <= y < len(rows) and 0 <= x < len(rows[0]) and rows[y][x] == "#"


def outside(rows):
    """The blocks the outside reaches (not the letter's counters): only these
    take the shadow, so the holes stay clean."""
    w, h = len(rows[0]) + 2, len(rows) + 2
    seen, todo = set(), [(-1, -1)]
    while todo:
        x, y = todo.pop()
        if (x, y) in seen or not (-1 <= x <= w and -1 <= y <= h) or lit(rows, x, y):
            continue
        seen.add((x, y))
        todo += [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
    return seen


def dither(d, x0, y0, w, h, ch, color, step=1):
    """Fill a rectangle with the 8x8 VGA glyph `ch`'s pattern, tiled from its
    top left, each of its pixels `step` wide."""
    rows = FONT8[ch * 8: ch * 8 + 8]
    for y in range(y0, y0 + h):
        bits = rows[((y - y0) // step) % 8]
        for x in range(x0, x0 + w):
            if bits & (0x80 >> (((x - x0) // step) % 8)):
                d.point((x, y), fill=color)


def shape(n, box, apple, ss=4):
    """The tile's outline, antialiased: Apple's squircle (a superellipse) or a
    rounded square."""
    a, b, r = box
    big = Image.new("L", (n * ss, n * ss), 0)
    d = ImageDraw.Draw(big)
    if apple:
        cx = cy = (a + b) / 2 * ss
        half = (b - a) / 2 * ss
        for py in range(round(a * ss), round(b * ss)):
            v = abs((py + 0.5 - cy) / half)
            if v >= 1:
                continue
            u = (1 - v ** 5) ** (1 / 5) * half
            d.line([(cx - u, py), (cx + u - 1, py)], fill=255)
    else:
        d.rounded_rectangle([a * ss, a * ss, b * ss - 1, b * ss - 1], r * ss, fill=255)
    return big.resize((n, n), Image.LANCZOS)


def layout(n, apple):
    """The tile's box, and which B and block size the mark is drawn at."""
    if apple:
        m = round(n * 100 / 1024)
        box = (m, n - m, round(n * 185 / 1024))
        k = max(1, round((n - 2 * m) * 0.6 / 15))
        return box, False, k
    m = round(n * 0.03)
    box = (m, n - m, round(n * 0.2))
    compact, k = {16: (False, 1), 24: (False, 1), 32: (True, 2), 48: (True, 3)}.get(n, (False, None))
    if k is None:
        k = max(1, round((n - 2 * m) * 0.66 / 15))
    return box, compact, k


def draw(n, apple=False):
    box, compact, k = layout(n, apple)
    a, b, _ = box
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    outline = shape(n, box, apple)
    if apple and n >= 64:   # the soft shadow Apple's grid leaves room for
        sh = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        sh.putalpha(outline.point(lambda v: v * 0.45))
        sh = sh.transform((n, n), Image.AFFINE, (1, 0, 0, 0, 1, -round(n * 0.012)))
        img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(n * 0.014)))
    surface = Image.new("RGBA", (n, n))
    sd = ImageDraw.Draw(surface)
    for y in range(n):
        t = min(1, max(0, (y - a) / max(1, b - a)))
        sd.line([(0, y), (n, y)], fill=tuple(round(TILE_TOP[i] + (TILE_BOTTOM[i] - TILE_TOP[i]) * t) for i in range(3)))
    if n >= 128:
        dither(sd, 0, 0, n, n, 0xB0, TEXTURE, step=max(1, n // 256))
    img.paste(surface, (0, 0), outline)
    if n >= 32:   # a hairline rim, lit from above
        width = max(1, n // 128)
        inner = shape(n, (a + width, b - width, max(0, box[2] - width)), apple)
        ring = ImageChops.subtract(outline, inner)
        img.paste(Image.new("RGBA", (n, n), RIM + (255,)), (0, 0), ring)

    rows, color_at = mark(compact)
    shadowed = n > 16                                 # at 16 px the shadow is only noise, and crowds the corners
    w, h = len(rows[0]) + shadowed, len(rows) + shadowed
    span = b - a
    x0 = a + (span - w * k) // 2
    y0 = a + (span - h * k) // 2
    d = ImageDraw.Draw(img)
    reach = outside(rows)
    for y in range(len(rows) + 1):
        for x in range(len(rows[0]) + 1):
            if not shadowed or lit(rows, x, y) or not lit(rows, x - 1, y - 1) or (x, y) not in reach:
                continue
            bx, by = x0 + x * k, y0 + y * k
            if k >= 6:
                dither(d, bx, by, k, k, 0xB1, SHADOW, step=max(1, k // 8))
            else:
                d.rectangle([bx, by, bx + k - 1, by + k - 1], fill=SHADOW)
    for y, row in enumerate(rows):
        for x, c in enumerate(row):
            if c == "#":
                bx, by = x0 + x * k, y0 + y * k
                d.rectangle([bx, by, bx + k - 1, by + k - 1], fill=color_at(x, y))
    return img


def main():
    images = {n: draw(n) for n in SIZES}
    for n, img in images.items():
        img.save(HERE / f"badterm-{n}.png")
    (HERE / "badterm-64.rgba").write_bytes(images[64].convert("RGBA").tobytes())
    images[256].save(HERE / "badterm.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)],
                     append_images=[images[n] for n in (16, 24, 32, 48, 64, 128)])
    # macOS: Apple's grid from 64 up; the list-view sizes fill their tile.
    mac = {n: (draw(n, apple=True) if n >= 64 else images[n]) for n in (16, 32, 64, 128, 256, 512, 1024)}
    mac[1024].save(HERE / "badterm.icns", append_images=[mac[n] for n in (16, 32, 64, 128, 256, 512)])


if __name__ == "__main__":
    main()
