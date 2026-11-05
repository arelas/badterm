#!/usr/bin/env python3
"""Icon lab: four directions for BadTerm's icon and logo mark, drawn the way
the app draws everything, in whole VGA-colored blocks, so each size stays
crisp. Writes a contact sheet of every concept at its real sizes, on dark and
light ground, with a wordmark lockup.

    python3 -I site-next/tools/iconlab.py OUTDIR

  cursor:   the splash's B, with a copper block cursor at its baseline.
  phosphor: the B glowing on curved CRT glass, scanlines and a power light.
  field:    the tile is the logo's ice ramp; the B is cut out of it.
  shades:   the CP437 smiley, in sunglasses: the badass terminal.
"""

import os
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import art  # noqa: E402  (the splash letters)
from ansi import VGA, FONT  # noqa: E402

B = art.LETTERS["B"]
ICE_ROWS = art.ICE          # by block row of a 12-row letter
C = lambda i: VGA[i]        # noqa: E731
VOID_TOP, VOID_BOTTOM = (22, 26, 36), (9, 11, 16)
FONT8 = open(os.path.join(art.REPO, "crates", "badterm", "assets", "fonts", "vga_8x8.bin"), "rb").read()


# ---------------------------------------------------------------- drawing helpers

def lit(grid, x, y):
    return 0 <= y < len(grid) and 0 <= x < len(grid[0]) and grid[y][x] == "#"


def outside(grid):
    """Cells the outside reaches (not a letter's counters), so shadows skip the holes."""
    w, h = len(grid[0]) + 2, len(grid) + 2
    seen, todo = set(), [(-1, -1)]
    while todo:
        x, y = todo.pop()
        if (x, y) in seen or not (-1 <= x <= w and -1 <= y <= h) or lit(grid, x, y):
            continue
        seen.add((x, y))
        todo += [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
    return seen


def dither(d, x0, y0, w, h, ch, color, step=1):
    """Fill with the 8x8 VGA glyph `ch`'s pattern (░ ▒ ▓), each pixel `step` wide."""
    rows = FONT8[ch * 8: ch * 8 + 8]
    for y in range(y0, y0 + h):
        bits = rows[((y - y0) // step) % 8]
        for x in range(x0, x0 + w):
            if bits & (0x80 >> (((x - x0) // step) % 8)):
                d.point((x, y), fill=color)


def body_box(n, apple):
    """The tile's box: Apple's grid (824 of 1024, radius 185) for the big
    sizes, nearly full bleed for the small ones, as taskbars and tabs want."""
    if apple:
        m = round(n * 100 / 1024)
        return m, n - m, round(n * 185 / 1024)
    m = max(0, round(n * 0.03))
    return m, n - m, round(n * 0.2)


def tile(n, apple, top=VOID_TOP, bottom=VOID_BOTTOM, texture=True):
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    a, b, r = body_box(n, apple)
    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).rounded_rectangle([a, a, b - 1, b - 1], r, fill=255)
    grad = Image.new("RGBA", (n, n))
    gd = ImageDraw.Draw(grad)
    for y in range(n):
        t = min(1, max(0, (y - a) / max(1, b - a)))
        gd.line([(0, y), (n, y)], fill=tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,))
    if texture and n >= 128:
        dither(gd, 0, 0, n, n, 0xB0, (28, 34, 48, 255), step=max(1, n // 256))
    if apple and n >= 128:   # the drop shadow Apple's grid leaves room for
        sh = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        ImageDraw.Draw(sh).rounded_rectangle([a, a + n * 0.012, b - 1, b - 1 + n * 0.012], r, fill=(0, 0, 0, 120))
        img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(n * 0.012)))
    img.paste(grad, (0, 0), mask)
    if n >= 32:   # a hairline highlight on the rim
        rim = Image.new("L", (n, n), 0)
        ImageDraw.Draw(rim).rounded_rectangle([a, a, b - 1, b - 1], r, outline=255, width=max(1, n // 128))
        img.paste(Image.new("RGBA", (n, n), (52, 62, 84, 255)), (0, 0), rim)
    return img, (a, b, r, mask)


def blocks(img, grid, x0, y0, k, color_at, shadow=None, shadow_dither=True):
    """Draw `grid` ('#' lit) in k-pixel blocks; color_at(x, y) gives each block's
    color. A shadow a block down and right, only where the outside reaches."""
    d = ImageDraw.Draw(img)
    if shadow:
        reach = outside(grid)
        for y in range(len(grid) + 1):
            for x in range(len(grid[0]) + 1):
                if lit(grid, x, y) or not lit(grid, x - 1, y - 1) or (x, y) not in reach:
                    continue
                bx, by = x0 + x * k, y0 + y * k
                if k >= 6 and shadow_dither:
                    dither(d, bx, by, k, k, 0xB1, shadow, step=max(1, k // 8))
                else:
                    d.rectangle([bx, by, bx + k - 1, by + k - 1], fill=shadow)
    for y, row in enumerate(grid):
        for x, c in enumerate(row):
            if c == "#":
                bx, by = x0 + x * k, y0 + y * k
                d.rectangle([bx, by, bx + k - 1, by + k - 1], fill=color_at(x, y))


def fit(n, apple, w_blocks, h_blocks, fill=0.66):
    """The block size, and the top-left, that center a w x h block shape in the tile."""
    a, b, _ = body_box(n, apple)
    span = b - a
    k = max(1, int(span * fill / max(w_blocks, h_blocks)))
    return k, a + (span - w_blocks * k) // 2, a + (span - h_blocks * k) // 2


# ---------------------------------------------------------------- the concepts

def cursor(n, apple):
    img, _ = tile(n, apple)
    grid = [row + ".....".replace(".", ".") for row in B]
    # The cursor: copper, a block wide gap after the B, on its last two rows.
    for y in (10, 11):
        grid[y] = grid[y][:10] + "####"
    grid = [row.ljust(14, ".") for row in grid]
    k, x0, y0 = fit(n, apple, 15, 13, fill=0.7)

    def color(x, y):
        if x >= 10:
            return C(14) if y == 10 else C(6)
        return C(ICE_ROWS[y])
    blocks(img, grid, x0, y0, k, color, shadow=C(8))
    return img


def phosphor(n, apple):
    img, (a, b, r, mask) = tile(n, apple, top=(44, 48, 58), bottom=(20, 22, 28), texture=False)
    # The glass: a black screen inset in the bezel.
    inset = round((b - a) * (0.09 if n >= 64 else 0.06))
    ga, gb = a + inset, b - inset - (round((b - a) * 0.05) if n >= 64 else 0)
    glass = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    gmask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(gmask).rounded_rectangle([ga, ga, gb - 1, gb - 1], max(1, round(r * 0.55)), fill=255)
    screen = Image.new("RGBA", (n, n), (3, 6, 8, 255))
    k = max(1, int((gb - ga) * 0.68 / 12))
    x0 = ga + ((gb - ga) - 9 * k) // 2
    y0 = ga + ((gb - ga) - 12 * k) // 2
    letter = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    blocks(letter, B, x0, y0, k, lambda x, y: C(ICE_ROWS[y]))
    if n >= 48:  # phosphor glow: light spreading onto the dark glass around the letter
        glow = letter.filter(ImageFilter.GaussianBlur(max(1, k * 1.2)))
        glow = Image.blend(Image.new("RGBA", (n, n), (0, 0, 0, 0)), glow, 0.85)
        screen.alpha_composite(glow)
    screen.alpha_composite(letter)
    if n >= 128:  # scanlines, one per block half
        sl = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        sd = ImageDraw.Draw(sl)
        step = max(2, k // 2)
        for y in range(ga, gb, step):
            sd.rectangle([ga, y + step // 2, gb, y + step - 1], fill=(0, 0, 0, 46))
        screen.alpha_composite(sl)
        # curved glass: a soft highlight top-left, a vignette at the corners
        hl = Image.new("L", (n, n), 0)
        ImageDraw.Draw(hl).ellipse([ga - (gb - ga) * 0.3, ga - (gb - ga) * 0.55, gb * 0.95, ga + (gb - ga) * 0.45], fill=36)
        screen.paste(Image.new("RGBA", (n, n), (255, 255, 255, 255)), (0, 0), hl.filter(ImageFilter.GaussianBlur(n * 0.04)))
    glass.paste(screen, (0, 0), gmask)
    img.alpha_composite(glass)
    if n >= 64:  # the copper power light under the glass, right
        d = ImageDraw.Draw(img)
        s = max(2, round(n * 0.022))
        cx, cy = gb - s * 2, gb + (b - gb) // 2 - s // 2 + round((b - a) * 0.005)
        if n >= 128:
            led = Image.new("RGBA", (n, n), (0, 0, 0, 0))
            ImageDraw.Draw(led).ellipse([cx - s * 2, cy - s * 2, cx + s * 3, cy + s * 3], fill=(217, 142, 62, 120))
            img.alpha_composite(led.filter(ImageFilter.GaussianBlur(s)))
        d.rectangle([cx, cy, cx + s - 1, cy + s - 1], fill=(237, 162, 82))
    return img


def field(n, apple):
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    a, b, r = body_box(n, apple)
    k, x0, y0 = fit(n, apple, 10, 13, fill=0.64)
    # The ramp, in bands a block tall, aligned with the letter's block rows,
    # the same order as the logo's: white, light cyan, cyan, blue.
    bands = Image.new("RGBA", (n, n))
    bd = ImageDraw.Draw(bands)
    # band edges at the letter's own row boundaries, extended to the tile's edges
    edges = [(y0 + 1 * k, 15), (y0 + 6 * k, 11), (y0 + 9 * k, 3), (n, 1)]
    top = 0
    for end, col in edges:
        bd.rectangle([0, top, n, end - 1], fill=C(col))
        top = end
    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).rounded_rectangle([a, a, b - 1, b - 1], r, fill=255)
    if apple and n >= 128:
        sh = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        ImageDraw.Draw(sh).rounded_rectangle([a, a + n * 0.012, b - 1, b - 1 + n * 0.012], r, fill=(0, 0, 0, 120))
        img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(n * 0.012)))
    img.paste(bands, (0, 0), mask)
    # The B cut out in near-black, its shadow in deep blue: a dark letter lit from above-left.
    letter = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    blocks(letter, B, x0, y0, k, lambda x, y: (8, 10, 16), shadow=C(1), shadow_dither=False)
    clipped = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    clipped.paste(letter, (0, 0), ImageChops.multiply(letter.split()[3], mask))
    img.alpha_composite(clipped)
    return img


SMILEY = [
    "......######......",
    "....##########....",
    "...############...",
    "..##############..",
    ".################.",
    ".################.",
    "##################",
    "##################",
    "##################",
    "##################",
    "##################",
    "##################",
    ".################.",
    ".################.",
    "..##############..",
    "...############...",
    "....##########....",
    "......######......",
]
# The shades: lenses with a bridge and the arms to the edges, and a smirk.
GLASSES = {(x, 6) for x in range(0, 18)} | {(x, 7) for x in list(range(2, 8)) + list(range(10, 16))} | \
          {(x, 8) for x in list(range(2, 8)) + list(range(10, 16))} | {(x, 9) for x in list(range(3, 7)) + list(range(11, 15))}
GLINT = {(3, 7), (11, 7)}
MOUTH = {(x, 13) for x in range(6, 12)} | {(12, 12), (13, 12)}


def shades(n, apple):
    img, _ = tile(n, apple)
    k, x0, y0 = fit(n, apple, 19, 19, fill=0.72)
    ramp = {0: 15, 1: 15}
    ramp.update({y: 14 for y in range(2, 10)})
    ramp.update({y: 6 for y in range(10, 18)})

    def color(x, y):
        if (x, y) in GLINT:
            return C(15)
        if (x, y) in GLASSES or (x, y) in MOUTH:
            return (6, 6, 10)
        return C(ramp[y])
    blocks(img, SMILEY, x0, y0, k, color, shadow=C(8))
    return img


CONCEPTS = [
    ("cursor", "Cursor", "The splash's B, with a copper block cursor at its baseline: BadTerm's letter, waiting for you to type.", cursor),
    ("phosphor", "Phosphor", "The B glowing on curved CRT glass, scanlines and a copper power light: the app's signature look.", phosphor),
    ("field", "Ice field", "The tile is the logo's ice ramp and the B is cut out of it: bold, flat, unmissable in a dock.", field),
    ("shades", "Shades", "The CP437 smiley in sunglasses: the badass terminal, as a face. A mascot more than a mark.", shades),
]


# ---------------------------------------------------------------- the sheet

def wordmark(height, cursor=False):
    """BADTERM in the splash letters, ice ramp and shadow, `height` pixels
    tall; with `cursor`, the copper block cursor after it."""
    word = "BADTERM"
    k = max(1, height // 13)
    grid = ["" for _ in range(12)]
    for i, ch in enumerate(word):
        for y, row in enumerate(art.LETTERS[ch]):
            grid[y] += row + ("." if i < len(word) - 1 else "")
    cur = len(grid[0]) + 1
    if cursor:
        grid = [row + "." + ("####" if y >= 10 else "....") for y, row in enumerate(grid)]
    w = (len(grid[0]) + 1) * k
    img = Image.new("RGBA", (w, 13 * k), (0, 0, 0, 0))
    blocks(img, grid, 0, 0, k, lambda x, y: (C(14) if y == 10 else C(6)) if x >= cur else C(ICE_ROWS[y]), shadow=C(8))
    return img


def label(d, xy, text, color, scale=2):
    """Text in the VGA 8x16 font, `scale` pixels a font pixel."""
    x, y = xy
    for ch in text.encode("cp437", "replace"):
        g = FONT[ch * 16:(ch + 1) * 16]
        for r in range(16):
            for c in range(8):
                if g[r] & (0x80 >> c):
                    d.rectangle([x + c * scale, y + r * scale, x + c * scale + scale - 1, y + r * scale + scale - 1], fill=color)
        x += 8 * scale


def sheet(out):
    row_h, W = 430, 1700
    img = Image.new("RGB", (W, 120 + row_h * len(CONCEPTS)), (14, 16, 21))
    d = ImageDraw.Draw(img)
    label(d, (40, 36), "BadTerm icon lab", (255, 255, 255), 3)
    label(d, (40, 86), "Each concept at 256 (Apple grid), 64, 32 and 16 px, the small ones also at 4x; on dark and light ground.", (140, 148, 163), 1)
    for i, (key, name, blurb, fn) in enumerate(CONCEPTS):
        y = 120 + i * row_h
        d.rectangle([24, y, W - 24, y + row_h - 24], fill=(20, 23, 30))
        label(d, (48, y + 22), f"{i + 1}  {name}", (85, 255, 255), 2)
        label(d, (48, y + 60), blurb, (170, 170, 170), 1)
        big = fn(256, True)
        img.paste(big, (40, y + 92), big)
        x = 330
        for ground in ((20, 23, 30), (236, 236, 238)):
            gx = x
            d.rectangle([gx - 10, y + 92, gx + 590, y + 92 + 250], fill=ground)
            for s in (64, 32, 16):
                ic = fn(s, s >= 64)
                img.paste(ic, (gx, y + 100 + (64 - s) // 2), ic)
                mag = ic.resize((s * 4 if s < 64 else s, s * 4 if s < 64 else s), Image.NEAREST)
                img.paste(mag, (gx, y + 180), mag) if s < 64 else None
                gx += 80 if s == 64 else (s * 4 + 24)
            # a taskbar / tab strip mock: the 16 px icon beside a label
            ic16 = fn(16, False)
            d.rectangle([x, y + 312, x + 250, y + 336], fill=(ground[0] - 8 if ground[0] > 100 else 34,) * 3)
            img.paste(ic16, (x + 6, y + 316), ic16)
            label(d, (x + 30, y + 316), "BadTerm", (20, 20, 20) if ground[0] > 100 else (220, 220, 220), 1)
            x += 620
        lock = wordmark(48, cursor=(key == "cursor"))
        mark = fn(64, True)
        lx, ly = 330, y + 352
        img.paste(mark, (lx, ly - 8), mark)
        img.paste(lock, (lx + 76, ly + 0), lock)
        if key == "cursor":
            label(d, (lx + 76 + lock.width + 24, ly + 16), "the icon is the wordmark's first letter and its cursor", (140, 148, 163), 1)
    img.save(os.path.join(out, "icon-lab.png"))
    for key, _, _, fn in CONCEPTS:
        fn(1024, True).save(os.path.join(out, f"concept-{key}-1024.png"))


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    os.makedirs(out, exist_ok=True)
    sheet(out)
    print("wrote", os.path.join(out, "icon-lab.png"))
