#!/usr/bin/env python3
"""Draw BadTerm's application icon: the B of its logo, powering up.

The icon is an ANSI piece, as the logo is (../logo/badterm.ans). It is the
logo's own B, drawn with the same pen (site-next/tools/letterpen.py) a little
wider, and shaded and set in its gold ki by the same code
(site-next/tools/powerup.py):
- the negative-land stroke: a cyan ground, light green and light blue worked
  over it in vertical streaks
- a black keyline and a grey ghost shadow
- the ki, white-hot at the keyline and gold outside, crackling
The canvas is 32 x 16 text cells, which in the VGA's 8 x 16 font is 256 x 256
pixels. It is written here as badterm-icon.ans.

Every size is made from that one picture:
- 256 px is the piece at 1:1, 512 and 1024 px at 2:1 and 4:1, so every pixel
  stays whole
- 128, 64 and 48 px average it down, so its shades mix as they do on a CRT
- 32 px is a pixel per half-cell, each the color its cell mostly shows (a
  shade, its main ink); 24 and 16 px average that down
Windows' and Linux's sizes fill a rounded tile, as taskbars expect. macOS sizes
from 64 up sit on Apple's icon grid: an 824-in-1024 squircle with room for its
shadow.

Writes these next to this script:
- badterm-<size>.png for each size (the full tile)
- badterm.ico (Windows)
- badterm.icns (macOS, on Apple's grid)
- badterm-64.rgba (raw pixels for the window icon, so the app needs no PNG
  decoder)
- badterm-icon.ans

Needs Pillow and numpy:

    python3 -I crates/badterm/assets/icon/make_icon.py
"""

import random
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
TOOLS = HERE.parents[3] / "site-next" / "tools"
sys.path.insert(0, str(TOOLS))
import letterpen  # noqa: E402
import powerup as pu  # noqa: E402
from ansi import VGA, Screen  # noqa: E402

SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
COLS, ROWS = 32, 16          # 256 x 256 pixels in the 8 x 16 font
SEED = 23                    # the build of the ki chosen
SX = 1.35                    # the logo's B is 1.2: the icon's is a little wider, to sit in a square
CAP = 6                      # the B's cap line, in half-cells: its claw reaches the top
RIM = (54, 64, 86)           # a hairline round the tile, so a black tile holds its edge on black
LIGHT, MEDIUM, DARK, FULL, UPPER, LOWER = 0xB0, 0xB1, 0xB2, 0xDB, 0xDF, 0xDC


def piece():
    """The icon as a Screen: the B, its keyline, shadow, stroke and ki."""
    pu.W, pu.H, pu.QH = COLS, ROWS, ROWS * 2
    cells, _ = letterpen.mask_of("B", letterpen.letters(), sx=SX)
    xs = [x for x, _ in cells]
    ox = (COLS - (max(xs) - min(xs) + 1)) // 2 - min(xs)
    mask = {(ox + x, CAP + y): 0 for x, y in cells if 0 <= ox + x < COLS and 0 <= CAP + y < pu.QH}
    rng = random.Random(SEED)
    cv = pu.Canvas()

    def never(x, qy):
        return False
    # A fuller ki than the logo's, so it fills the square round the letter.
    d, heat = pu.ki(cv, mask, rng, never, side=5.5, down=3.0, base=6.0)
    pu.arcs(cv, rng, d, heat, never, n=1)
    for qy in range(pu.QH):
        for x in range(COLS):
            if (x, qy) not in mask and any((x + a, qy + b) in mask for a in (-1, 0, 1) for b in (-1, 0, 1)):
                cv.set(x, qy, None)
                cv.fixed.pop((x, qy // 2), None)
    reached = pu.outside(mask)
    for qy in range(pu.QH):
        for x in range(COLS):
            if not reached[qy, x] and (x, qy) not in mask:
                cv.set(x, qy, None)
    pu.shadow(cv, mask)
    pu.shade_letters(cv, mask, {0: CAP})
    s = Screen(COLS, ROWS)
    cv.onto(s)
    return s


def full(s):
    """The piece in the VGA font, 256 x 256."""
    path = HERE / ".icon-1x.png"
    s.png(str(path), scale=1)
    img = Image.open(path).convert("RGB")
    path.unlink()
    return img


def halves(s):
    """32 x 32: a pixel per half-cell, the color it mostly shows."""
    img = Image.new("RGB", (COLS, ROWS * 2))
    px = img.load()
    for y in range(ROWS):
        for x in range(COLS):
            ch, fg, bg = s.cells[y][x]
            top, bottom = {FULL: (fg, fg), UPPER: (fg, bg), LOWER: (bg, fg), DARK: (fg, fg), MEDIUM: (fg, fg)}.get(ch, (bg, bg))
            px[x, y * 2], px[x, y * 2 + 1] = VGA[top], VGA[bottom]
    return img


def picture(base, small, n):
    """The piece at n pixels square: whole pixels up, averaged down."""
    if n >= 256:
        return base.resize((n, n), Image.NEAREST)
    if n > 32:
        return base.resize((n, n), Image.BOX)
    return small.resize((n, n), Image.BOX) if n < 32 else small


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


def draw(base, small, n, apple=False):
    if apple:
        m = round(n * 100 / 1024)
        box = (m, n - m, round(n * 185 / 1024))
    else:
        box = (0, n, round(n * 0.18))
    a, b, _ = box
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    outline = shape(n, box, apple)
    if apple and n >= 64:   # the soft shadow Apple's grid leaves room for
        sh = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        sh.putalpha(outline.point(lambda v: v * 0.45))
        sh = sh.transform((n, n), Image.AFFINE, (1, 0, 0, 0, 1, -round(n * 0.012)))
        img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(n * 0.014)))
    art = Image.new("RGBA", (n, n), (0, 0, 0, 255))
    art.paste(picture(base, small, b - a), (a, a))
    img.paste(art, (0, 0), outline)
    if n >= 32:   # a hairline rim
        width = max(1, n // 128)
        inner = shape(n, (a + width, b - width, max(0, box[2] - width)), apple)
        ring = ImageChops.subtract(outline, inner)
        img.paste(Image.new("RGBA", (n, n), RIM + (255,)), (0, 0), ring)
    return img


def main():
    s = piece()
    (HERE / "badterm-icon.ans").write_bytes(
        s.ans(sauce=dict(title="BadTerm icon", author="Bad Cluster", group="", date="20261105")))
    base, small = full(s), halves(s)
    images = {n: draw(base, small, n) for n in SIZES}
    for n, img in images.items():
        img.save(HERE / f"badterm-{n}.png")
    (HERE / "badterm-64.rgba").write_bytes(images[64].convert("RGBA").tobytes())
    images[256].save(HERE / "badterm.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)],
                     append_images=[images[n] for n in (16, 24, 32, 48, 64, 128)])
    # macOS: Apple's grid from 64 up; the list-view sizes fill their tile.
    mac = {n: (draw(base, small, n, apple=True) if n >= 64 else images[n]) for n in (16, 32, 64, 128, 256, 512, 1024)}
    mac[1024].save(HERE / "badterm.icns", append_images=[mac[n] for n in (16, 32, 64, 128, 256, 512)])
    mac[1024].save(HERE / "badterm-mac-1024.png")
    print("wrote", ", ".join(f"badterm-{n}.png" for n in SIZES), "badterm.ico badterm.icns badterm-64.rgba badterm-icon.ans")


if __name__ == "__main__":
    main()
