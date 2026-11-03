#!/usr/bin/env python3
"""Draw BadTerm's application icon: the "B" of the splash screen's logo as
ANSI art, on a dark tile.

The letter is the splash's own (crates/badterm/src/ui/splash.rs), 9 x 12
blocks, in the same ice ramp down it (a white highlight, light cyan, cyan,
deep blue) as if lit from above, casting a drop shadow down and to the
right that is drawn with the CP437 "▒" dither, as TheDraw-era logos drew
theirs. Behind it, on the larger sizes, the tile has a faint "░" texture,
so it reads as a text-mode screen. Each size draws the blocks at a whole
number of pixels, so the letter stays crisp down to 16 x 16.

Writes badterm-<size>.png for each size, badterm.ico (Windows),
badterm.icns (macOS) and badterm-64.rgba (raw pixels for the window icon,
so the app needs no PNG decoder) next to this script. Needs Pillow:

    python3 make_icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
FONT8 = (HERE.parent / "fonts" / "vga_8x8.bin").read_bytes()
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]

# The splash's B, and its ramp (VGA colors by row of blocks).
LETTER = [
    "#######..", "########.", "###...###", "###...###", "###..###.", "#######..",
    "########.", "###...###", "###...###", "###...###", "########.", "#######..",
]
VGA = {15: (255, 255, 255), 11: (85, 255, 255), 3: (0, 170, 170), 1: (0, 0, 170), 8: (85, 85, 85)}
RAMP = [15, 11, 11, 11, 11, 11, 3, 3, 3, 1, 1, 1]

TILE = (10, 12, 18)
TEXTURE = (20, 28, 44)
EDGE = (30, 44, 66)
SHADE_LIGHT = 0xB0  # ░
SHADE_MEDIUM = 0xB1  # ▒


def lit(x, y):
    return 0 <= y < len(LETTER) and 0 <= x < len(LETTER[0]) and LETTER[y][x] == "#"


def outside():
    """The blocks around the letter that the outside reaches (not its
    counters): only these take the shadow, so the holes stay clean."""
    w, h = len(LETTER[0]) + 2, len(LETTER) + 2
    seen, todo = set(), [(-1, -1)]
    while todo:
        x, y = todo.pop()
        if (x, y) in seen or not (-1 <= x <= w and -1 <= y <= h) or lit(x, y):
            continue
        seen.add((x, y))
        todo += [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
    return seen


def dither(d, x0, y0, w, h, ch, color, step=1):
    """Fill a rectangle with the 8x8 VGA glyph `ch`'s pattern, tiled, each
    of its pixels `step` wide."""
    rows = FONT8[ch * 8 : ch * 8 + 8]
    for y in range(y0, y0 + h):
        bits = rows[(y // step) % 8]
        for x in range(x0, x0 + w):
            if bits & (0x80 >> ((x // step) % 8)):
                d.point((x, y), fill=color)


def draw(n):
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    # A rounded tile, as app icons on every platform are.
    m, r = round(n * 0.04), round(n * 0.18)
    shape = Image.new("L", (n, n), 0)
    ImageDraw.Draw(shape).rounded_rectangle([m, m, n - 1 - m, n - 1 - m], r, fill=255)
    tile = Image.new("RGBA", (n, n), TILE + (255,))
    if n >= 128:
        dither(ImageDraw.Draw(tile), 0, 0, n, n, SHADE_LIGHT, TEXTURE, step=max(1, n // 256))
    img.paste(tile, (0, 0), shape)
    if n >= 32:
        edge = Image.new("L", (n, n), 0)
        ImageDraw.Draw(edge).rounded_rectangle([m, m, n - 1 - m, n - 1 - m], r, outline=255, width=max(1, n // 64))
        img.paste(Image.new("RGBA", (n, n), EDGE + (255,)), (0, 0), edge)

    # The letter: blocks of k pixels, the shadow a block down and right.
    d = ImageDraw.Draw(img)
    k = max(1, round(n * 0.68 / 13))
    w, h = (len(LETTER[0]) + 1) * k, (len(LETTER) + 1) * k
    x0, y0 = (n - w) // 2, (n - h) // 2
    reach = outside()
    for y in range(len(LETTER) + 1):
        for x in range(len(LETTER[0]) + 1):
            if lit(x, y) or not lit(x - 1, y - 1) or (x, y) not in reach:
                continue
            bx, by = x0 + x * k, y0 + y * k
            if k >= 6:
                dither(d, bx, by, k, k, SHADE_MEDIUM, VGA[8], step=max(1, k // 8))
            else:
                d.rectangle([bx, by, bx + k - 1, by + k - 1], fill=VGA[8])
    for y, row in enumerate(LETTER):
        for x, c in enumerate(row):
            if c == "#":
                bx, by = x0 + x * k, y0 + y * k
                d.rectangle([bx, by, bx + k - 1, by + k - 1], fill=VGA[RAMP[y]])
    return img


def main():
    images = {n: draw(n) for n in SIZES}
    for n, img in images.items():
        img.save(HERE / f"badterm-{n}.png")
    (HERE / "badterm-64.rgba").write_bytes(images[64].convert("RGBA").tobytes())
    images[256].save(HERE / "badterm.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)],
                     append_images=[images[n] for n in (16, 24, 32, 48, 64, 128)])
    images[1024].save(HERE / "badterm.icns", append_images=[images[n] for n in (16, 32, 64, 128, 256, 512)])


if __name__ == "__main__":
    main()
