#!/usr/bin/env python3
"""Draw BadTerm's application icon: the B of its logo.

The icon is the logo's own B (../logo/badterm.ans), drawn with the same pen
(site-next/tools/letterpen.py), larger, and shaded by the same code
(site-next/tools/powerup.py): the negative-land stroke (a cyan ground, light
green and light blue worked over it in vertical streaks), a black keyline and
a grey ghost shadow. There is no ki: at icon sizes it crowds the letter, as it
did the website's header. The B sits on a slate tile, so its black keyline
outlines it. The canvas is 32 x 16 text cells, which in the VGA's 8 x 16 font
is 256 x 256 pixels. It is also written as an ANSI piece, badterm-icon.ans, on
black.

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
- badterm-<size>.png for each size (the full tile), and badterm-mac-1024.png
- badterm.ico (Windows)
- badterm.icns (macOS, on Apple's grid)
- badterm-64.rgba (raw pixels for the window icon, so the app needs no PNG
  decoder)
- badterm.svg (the 32 px picture, a unit a half-cell, for favicons)
- badterm-icon.ans

Needs Pillow and numpy:

    python3 -I crates/badterm/assets/icon/make_icon.py
"""

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
SX, SY = 1.75, 1.05          # the logo's B is 1.2 x 1.0: the icon's fills its square
TILE = ((52, 58, 76), (24, 27, 36))   # slate, lighter at the top: the keyline shows against it
RIM = (70, 80, 104)          # a hairline round the tile, lit a little
LIGHT, MEDIUM, DARK, FULL, UPPER, LOWER = 0xB0, 0xB1, 0xB2, 0xDB, 0xDF, 0xDC


def piece():
    """The icon as a Screen (the B, its keyline, shadow and stroke, on
    black), and the half-cells that are the tile's ground."""
    pu.W, pu.H, pu.QH = COLS, ROWS, ROWS * 2
    cells, _ = letterpen.mask_of("B", letterpen.letters(), sx=SX, sy=SY)
    xs, ys = [x for x, _ in cells], [y for _, y in cells]
    ox = (COLS - (max(xs) - min(xs) + 1)) // 2 - min(xs)
    oy = (pu.QH - (max(ys) - min(ys) + 1)) // 2 - min(ys)
    mask = {(ox + x, oy + y): 0 for x, y in cells if 0 <= ox + x < COLS and 0 <= oy + y < pu.QH}
    keyline = {(x, qy) for qy in range(pu.QH) for x in range(COLS)
               if (x, qy) not in mask and any((x + a, qy + b) in mask for a in (-1, 0, 1) for b in (-1, 0, 1))}
    cv = pu.Canvas()
    pu.shadow(cv, mask)
    pu.shade_letters(cv, mask, {0: oy})
    s = Screen(COLS, ROWS)
    cv.onto(s)
    reached = pu.outside(mask)
    ground = set()
    for qy in range(pu.QH):
        for x in range(COLS):
            if (x, qy) in mask or (x, qy) in keyline or not reached[qy, x]:
                continue
            if halves_of(s, x, qy) == 0:       # black there, and not the shadow
                ground.add((x, qy))
    return s, ground


def halves_of(s, x, qy):
    """The VGA color half-cell (x, qy) mostly shows."""
    ch, fg, bg = s.cells[qy // 2][x]
    top, bottom = {FULL: (fg, fg), UPPER: (fg, bg), LOWER: (bg, fg), DARK: (fg, fg), MEDIUM: (fg, fg)}.get(ch, (bg, bg))
    return bottom if qy % 2 else top


def tile_at(qy):
    t = qy / (ROWS * 2 - 1)
    return tuple(round(TILE[0][i] + (TILE[1][i] - TILE[0][i]) * t) for i in range(3))


def full(s, ground):
    """The piece in the VGA font, 256 x 256, on its tile."""
    path = HERE / ".icon-1x.png"
    s.png(str(path), scale=1)
    img = Image.open(path).convert("RGB")
    path.unlink()
    px = img.load()
    for x, qy in ground:
        c = tile_at(qy)
        for y in range(qy * 8, qy * 8 + 8):
            for xx in range(x * 8, x * 8 + 8):
                if px[xx, y] == (0, 0, 0):
                    px[xx, y] = c
    return img


def halves(s, ground):
    """32 x 32: a pixel per half-cell, the color it mostly shows."""
    img = Image.new("RGB", (COLS, ROWS * 2))
    px = img.load()
    for qy in range(ROWS * 2):
        for x in range(COLS):
            px[x, qy] = tile_at(qy) if (x, qy) in ground else tuple(VGA[halves_of(s, x, qy)])
    return img


def classify(s, ground):
    """Each 32 px half-cell, by what it is: 'tile', 'key' (the keyline and the
    B's black counters), 'shadow' (the grey ghost) or 'letter' (the stroke)."""
    kinds = {}
    for qy in range(ROWS * 2):
        for x in range(COLS):
            if (x, qy) in ground:
                kinds[(x, qy)] = "tile"
            else:
                c = tuple(VGA[halves_of(s, x, qy)])
                if c == (0, 0, 0):
                    kinds[(x, qy)] = "key"
                elif max(c) - min(c) < 18:
                    kinds[(x, qy)] = "shadow"
                else:
                    kinds[(x, qy)] = "letter"
    return kinds


def flatten(small, s, kinds):
    """Smooth the stroke for the small sizes. The negative-land streaks (green
    and blue over cyan) read as texture at 256 px and as noise at 16, so at the
    small sizes every letter half-cell becomes the one ink the B is already
    mostly made of (its light blue): the letter then stays clean when it is
    reduced down. Keyline, counters and tile are left as they are. Returns the
    flattened 32 px picture and that ink."""
    counts = {}
    for (x, qy), kind in kinds.items():
        if kind == "letter":
            c = tuple(VGA[halves_of(s, x, qy)])
            counts[c] = counts.get(c, 0) + 1
    ink = max(counts, key=counts.get)
    out = small.copy()
    px = out.load()
    for (x, qy), kind in kinds.items():
        if kind == "letter":
            px[x, qy] = ink
    return out, ink


def reduce_crisp(flat, kinds, ink, n):
    """24 and 16 px: reduce the flattened 32 px master letter-first, so a
    one-cell stroke keeps a whole pixel instead of averaging to a grey smear
    (what made the small icon muddy). A target pixel is the ink where its source
    cells are mostly letter, black for the keyline and counters, the ghost's
    grey, else the tile."""
    px0 = flat.load()
    img = Image.new("RGB", (n, n))
    px = img.load()
    sc = (ROWS * 2) / n
    for ty in range(n):
        for tx in range(n):
            x0, x1 = int(tx * sc), max(int(tx * sc) + 1, int((tx + 1) * sc))
            y0, y1 = int(ty * sc), max(int(ty * sc) + 1, int((ty + 1) * sc))
            tally = {"letter": 0, "key": 0, "shadow": 0, "tile": 0}
            shade = (0, 0, 0)
            for qy in range(y0, min(y1, ROWS * 2)):
                for x in range(x0, min(x1, COLS)):
                    kind = kinds[(x, qy)]
                    tally[kind] += 1
                    if kind == "shadow":
                        shade = px0[x, qy]
            tot = sum(tally.values()) or 1
            if tally["letter"] / tot >= 0.28:
                px[tx, ty] = ink
            elif tally["key"] / tot >= 0.28:
                px[tx, ty] = (0, 0, 0)
            elif tally["shadow"] / tot >= 0.45:
                px[tx, ty] = shade
            else:
                px[tx, ty] = tile_at(min(ROWS * 2 - 1, int((ty + 0.5) * sc)))
    return img


def svg(small):
    """The 32 px picture as SVG, a unit a pixel, on a rounded tile."""
    w, h = small.size
    px = small.load()
    rects = []
    for y in range(h):
        x = 0
        while x < w:
            c, n = px[x, y], 1
            while x + n < w and px[x + n, y] == c:
                n += 1
            rects.append('<rect x="%d" y="%d" width="%d" height="1" fill="#%02X%02X%02X"/>' % ((x, y, n) + c))
            x += n
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d" shape-rendering="crispEdges">'
            '<title>BadTerm</title><clipPath id="t"><rect width="%d" height="%d" rx="%g"/></clipPath>'
            '<g clip-path="url(#t)">%s</g></svg>\n' % (w, h, w, h, w, h, w * 0.18, "".join(rects)))


def picture(base, small, crisp, n):
    """The piece at n pixels square: whole pixels up, the gradient sizes
    averaged down, the small ones reduced letter-first (crisp)."""
    if n >= 256:
        return base.resize((n, n), Image.NEAREST)
    if n > 32:
        return base.resize((n, n), Image.BOX)
    return crisp[n] if n < 32 else small


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


def draw(base, small, crisp, n, apple=False):
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
    art = Image.new("RGBA", (n, n), TILE[1] + (255,))
    art.paste(picture(base, small, crisp, b - a), (a, a))
    img.paste(art, (0, 0), outline)
    if n >= 32:   # a hairline rim
        width = max(1, n // 128)
        inner = shape(n, (a + width, b - width, max(0, box[2] - width)), apple)
        ring = ImageChops.subtract(outline, inner)
        img.paste(Image.new("RGBA", (n, n), RIM + (255,)), (0, 0), ring)
    return img


def main():
    s, ground = piece()
    (HERE / "badterm-icon.ans").write_bytes(
        s.ans(sauce=dict(title="BadTerm icon", author="Bad Cluster", group="", date="20261105")))
    base = full(s, ground)
    kinds = classify(s, ground)
    small, ink = flatten(halves(s, ground), s, kinds)             # the small master, stroke smoothed to one blue
    crisp = {n: reduce_crisp(small, kinds, ink, n) for n in (16, 24)}   # reduced letter-first, not averaged
    (HERE / "badterm.svg").write_text(svg(small), encoding="utf-8")
    images = {n: draw(base, small, crisp, n) for n in SIZES}
    for n, img in images.items():
        img.save(HERE / f"badterm-{n}.png")
    (HERE / "badterm-64.rgba").write_bytes(images[64].convert("RGBA").tobytes())
    (HERE / "badterm-32.rgba").write_bytes(images[32].convert("RGBA").tobytes())   # the crisp small, the app's window icon (taskbar/Task Manager)
    images[256].save(HERE / "badterm.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)],
                     append_images=[images[n] for n in (16, 24, 32, 48, 64, 128)])
    # macOS: Apple's grid from 64 up; the list-view sizes fill their tile.
    mac = {n: (draw(base, small, crisp, n, apple=True) if n >= 64 else images[n]) for n in (16, 32, 64, 128, 256, 512, 1024)}
    mac[1024].save(HERE / "badterm.icns", append_images=[mac[n] for n in (16, 32, 64, 128, 256, 512)])
    mac[1024].save(HERE / "badterm-mac-1024.png")
    print("wrote", ", ".join(f"badterm-{n}.png" for n in SIZES),
          "badterm-mac-1024.png badterm.ico badterm.icns badterm-64.rgba badterm-32.rgba badterm.svg badterm-icon.ans")


if __name__ == "__main__":
    main()
