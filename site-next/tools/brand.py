"""BadTerm's logo kit, drawn from the same blocks as the app icon
(crates/badterm/assets/icon/make_icon.py) and the splash screen's letters.

  icon:      the mark B▄ on its dark tile (the app icon, as SVG)
  mark:      B▄ alone, on nothing
  wordmark:  BADTERM▄, the splash letters in the ice ramp with the copper cursor
  lockups:   icon + wordmark side by side; stacked, with the tagline
  one-color: mark and wordmark in white or black, for print and embroidery

SVGs are rectangles on a grid of 8 units a block (so a block can carry the
CP437 ▒ shadow pattern, a unit a pixel), with crispEdges, so they stay sharp
at any size. favicons() writes the site's tab and home-screen icons.

    python3 -I site-next/tools/brand.py OUTDIR    # the kit, and a sheet of it
"""

import math
import os
import sys

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "crates", "badterm", "assets", "icon"))
import art  # noqa: E402
import make_icon as mi  # noqa: E402
from ansi import FONT, VGA  # noqa: E402

U = 8  # SVG units a block
HEX = lambda c: "#%02X%02X%02X" % c  # noqa: E731
SHADE = mi.FONT8[0xB1 * 8: 0xB1 * 8 + 8]  # ▒
TILE_TOP, TILE_BOTTOM, RIM = mi.TILE_TOP, mi.TILE_BOTTOM, mi.RIM
COPPER_TOP, COPPER = mi.VGA[14], mi.VGA[6]


ON_LIGHT = [3, 3, 3, 3, 3, 3, 1, 1, 1, 1, 1, 1]   # the ramp's dark half, for light ground


def word_grid(word="BADTERM", cursor=True, light=False):
    """The wordmark's blocks: rows and a color for each lit block. `light`
    keeps to the ramp's dark half, which holds up on white."""
    rows = ["" for _ in range(12)]
    for i, ch in enumerate(word):
        for y, r in enumerate(art.LETTERS[ch]):
            rows[y] += r + ("." if i < len(word) - 1 else "")
    cx = len(rows[0]) + 1
    if cursor:
        rows = [r + "." + ("####" if y >= 10 else "....") for y, r in enumerate(rows)]

    def color(x, y):
        if cursor and x >= cx:
            return COPPER if light else (COPPER_TOP if y == 10 else COPPER)
        return VGA[(ON_LIGHT if light else art.ICE)[y]]
    return rows, color


def mark_grid():
    return mi.mark(compact=False)


def runs(rows, color, ox=0, oy=0):
    """Lit blocks as horizontal runs of one color: (x, y, length, rgb)."""
    out = []
    for y, row in enumerate(rows):
        x = 0
        while x < len(row):
            if row[x] != "#":
                x += 1
                continue
            c = color(x, y)
            x0 = x
            while x < len(row) and row[x] == "#" and color(x, y) == c:
                x += 1
            out.append((ox + x0, oy + y, x - x0, c))
    return out


def shadow_cells(rows):
    reach = mi.outside(rows)
    return [(x, y) for y in range(len(rows) + 1) for x in range(len(rows[0]) + 1)
            if not mi.lit(rows, x, y) and mi.lit(rows, x - 1, y - 1) and (x, y) in reach]


def svg_blocks(rows, color, ox, oy, shadow="pattern", mono=None):
    """The <g> for a block shape at block offset (ox, oy)."""
    parts = []
    if shadow and not mono:
        fill = "url(#shade)" if shadow == "pattern" else HEX(VGA[8])
        for x, y in shadow_cells(rows):
            parts.append(f'<rect x="{(ox + x) * U}" y="{(oy + y) * U}" width="{U}" height="{U}" fill="{fill}"/>')
    for x, y, n, c in runs(rows, (lambda x, y: mono) if mono else color, ox, oy):
        parts.append(f'<rect x="{x * U}" y="{y * U}" width="{n * U}" height="{U}" fill="{HEX(c)}"/>')
    return "".join(parts)


def svg(w_blocks, h_blocks, body, title, defs=""):
    shade = "".join(f'<rect x="{c}" y="{r}" width="1" height="1"/>'
                    for r in range(8) for c in range(8) if SHADE[r] & (0x80 >> c))
    pattern = (f'<pattern id="shade" width="8" height="8" patternUnits="userSpaceOnUse">'
               f'<g fill="{HEX(VGA[8])}">{shade}</g></pattern>')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w_blocks * U} {h_blocks * U}" '
            f'width="{w_blocks * U}" height="{h_blocks * U}" shape-rendering="crispEdges" role="img">'
            f'<title>{title}</title><defs>{pattern}{defs}</defs>{body}</svg>\n')


def tile_svg(size_blocks, apple=True):
    """The dark tile: Apple's squircle (as a superellipse path) or a rounded square."""
    s = size_blocks * U
    grad = (f'<linearGradient id="void" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{HEX(TILE_TOP)}"/><stop offset="1" stop-color="{HEX(TILE_BOTTOM)}"/></linearGradient>')
    if apple:
        pts = []
        for i in range(160):
            t = 2 * math.pi * i / 160
            ct, st = math.cos(t), math.sin(t)
            x = s / 2 + s / 2 * (abs(ct) ** (2 / 5)) * (1 if ct >= 0 else -1)
            y = s / 2 + s / 2 * (abs(st) ** (2 / 5)) * (1 if st >= 0 else -1)
            pts.append(f"{x:.1f},{y:.1f}")
        shape = f'<path d="M{" L".join(pts)} Z" fill="url(#void)" stroke="{HEX(RIM)}" stroke-width="{s / 200:.2f}" shape-rendering="geometricPrecision"/>'
    else:
        shape = (f'<rect x="0.5" y="0.5" width="{s - 1}" height="{s - 1}" rx="{s * 0.2:.1f}" fill="url(#void)" '
                 f'stroke="{HEX(RIM)}" stroke-width="1" shape-rendering="geometricPrecision"/>')
    return grad, shape


def icon_svg(apple=True, shadow="pattern"):
    rows, color = mark_grid()
    size = 24                           # blocks across the tile
    w, h = len(rows[0]) + 1, len(rows) + 1
    ox, oy = (size - w) // 2, (size - h) // 2
    grad, shape = tile_svg(size, apple)
    return svg(size, size, shape + svg_blocks(rows, color, ox, oy, shadow), "BadTerm", grad)


def mark_svg(mono=None):
    rows, color = mark_grid()
    pad = 0 if mono else 1
    return svg(len(rows[0]) + pad, len(rows) + pad, svg_blocks(rows, color, 0, 0, None if mono else "pattern", mono), "BadTerm")


def wordmark_svg(mono=None, shadow="pattern", light=False):
    rows, color = word_grid(light=light)
    pad = 0 if mono else 1
    return svg(len(rows[0]) + pad, len(rows) + pad, svg_blocks(rows, color, 0, 0, None if mono else shadow, mono), "BadTerm")


def tagline_rects(text, ox_units, oy_units, color, scale=1):
    """Text in the VGA 8x16 font as unit rectangles (a font pixel a unit x scale)."""
    parts = []
    x = ox_units
    for ch in text.encode("cp437"):
        g = FONT[ch * 16:(ch + 1) * 16]
        for r in range(16):
            c = 0
            while c < 8:
                if g[r] & (0x80 >> c):
                    c0 = c
                    while c < 8 and g[r] & (0x80 >> c):
                        c += 1
                    parts.append(f'<rect x="{x + c0 * scale}" y="{oy_units + r * scale}" width="{(c - c0) * scale}" height="{scale}"/>')
                else:
                    c += 1
        x += 8 * scale
    return f'<g fill="{HEX(color)}">{"".join(parts)}</g>'


def lockup_svg(stacked=False):
    rows, color = word_grid()
    mrows, mcolor = mark_grid()
    ww, wh = len(rows[0]) + 1, len(rows) + 1
    tile = 24
    grad, shape = tile_svg(tile, True)
    if not stacked:
        # The tile at the wordmark's height plus a margin, the wordmark beside it.
        scale = (wh + 4) / tile
        gap = 4
        w, h = round(tile * scale) + gap + ww, wh + 4
        t = f'<g transform="scale({scale:.4f})">{shape}{svg_blocks(mrows, mcolor, (tile - 15) // 2, (tile - 13) // 2)}</g>'
        body = t + svg_blocks(rows, color, round(tile * scale) + gap, 2)
        return svg(w, h, body, "BadTerm", grad)
    tag = "t h e   b a d a s s   t e r m i n a l"
    tag_w_units = len(tag) * 16
    w = max(ww, tile) + 2
    tile_x = (w - tile) // 2
    word_y = tile + 4
    tag_y_units = (word_y + wh + 3) * U
    h = word_y + wh + 3 + 4 + 2
    body = (f'<g transform="translate({tile_x * U} 0)">{shape}{svg_blocks(mrows, mcolor, (tile - 15) // 2, (tile - 13) // 2)}</g>'
            + svg_blocks(rows, color, (w - ww) // 2, word_y)
            + tagline_rects(tag, (w * U - tag_w_units) // 2, tag_y_units, VGA[7], scale=2))
    return svg(w, h, body, "BadTerm, the badass terminal", grad)


def png_blocks(rows, color, k, shadow=True):
    w, h = (len(rows[0]) + 1) * k, (len(rows) + 1) * k
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if shadow:
        for x, y in shadow_cells(rows):
            if k >= 6:
                mi.dither(d, x * k, y * k, k, k, 0xB1, VGA[8], step=max(1, k // 8))
            else:
                d.rectangle([x * k, y * k, x * k + k - 1, y * k + k - 1], fill=VGA[8])
    for x, y, n, c in runs(rows, color):
        d.rectangle([x * k, y * k, (x + n) * k - 1, y * k + k - 1], fill=c)
    return img


def kit(out):
    os.makedirs(out, exist_ok=True)
    files = {
        "badterm-icon.svg": icon_svg(True),
        "badterm-icon-square.svg": icon_svg(False),
        "badterm-mark.svg": mark_svg(),
        "badterm-mark-white.svg": mark_svg(mono=(255, 255, 255)),
        "badterm-mark-black.svg": mark_svg(mono=(0, 0, 0)),
        "badterm-wordmark.svg": wordmark_svg(),
        "badterm-wordmark-solid.svg": wordmark_svg(shadow="solid"),
        "badterm-wordmark-on-light.svg": wordmark_svg(light=True),
        "badterm-wordmark-white.svg": wordmark_svg(mono=(255, 255, 255)),
        "badterm-wordmark-black.svg": wordmark_svg(mono=(0, 0, 0)),
        "badterm-lockup.svg": lockup_svg(False),
        "badterm-lockup-stacked.svg": lockup_svg(True),
    }
    for name, text in files.items():
        with open(os.path.join(out, name), "w", encoding="utf-8") as f:
            f.write(text)
    rows, color = word_grid()
    for k in (4, 8):
        png_blocks(rows, color, k).save(os.path.join(out, f"badterm-wordmark@{k}.png"))
    mrows, mcolor = mark_grid()
    png_blocks(mrows, mcolor, 16).save(os.path.join(out, "badterm-mark@16.png"))
    mi.draw(1024, apple=True).save(os.path.join(out, "badterm-icon-1024.png"))
    mi.draw(512).save(os.path.join(out, "badterm-icon-square-512.png"))
    og_card(os.path.join(out, "badterm-card.png"))
    return sorted(os.listdir(out))


def og_card(path):
    """1200 x 630: the icon and the wordmark on the dark ground, the tagline,
    what it is. For links shared on social sites and chats."""
    W, H = 1200, 630
    img = Image.new("RGB", (W, H), (9, 11, 16))
    d = ImageDraw.Draw(img)
    for y in range(H):   # the tile's gradient, faint
        t = y / H
        d.line([(0, y), (W, y)], fill=tuple(round(14 + (6 - 14) * t + c * 0) for c in range(3)))
    mi.dither(d, 0, 0, W, H, 0xB0, (20, 24, 34), step=2)
    ic = mi.draw(256, apple=True)
    img.paste(ic, (90, (H - 256) // 2 - 20), ic)
    rows, color = word_grid()
    wm = png_blocks(rows, color, 9)
    x = 90 + 256 + 40
    img.paste(wm, (x, 200), wm)

    def text(xy, s, c, scale):
        tx, ty = xy
        for ch in s.encode("cp437"):
            g = FONT[ch * 16:(ch + 1) * 16]
            for r in range(16):
                for cc in range(8):
                    if g[r] & (0x80 >> cc):
                        d.rectangle([tx + cc * scale, ty + r * scale, tx + cc * scale + scale - 1, ty + r * scale + scale - 1], fill=c)
            tx += 8 * scale
    text((x + 4, 200 + wm.height + 28), "t h e   b a d a s s   t e r m i n a l", VGA[7], 2)
    text((x + 4, 200 + wm.height + 88), "BBS terminal for Windows, macOS and Linux", (70, 179, 166), 2)
    img.save(path, optimize=True)


def favicons(img_dir):
    """The site's tab icon (SVG, with a 64 px PNG for the rest) and the
    home-screen icon (180 px, opaque: phones round it themselves)."""
    with open(os.path.join(img_dir, "favicon.svg"), "w", encoding="utf-8") as f:
        f.write(icon_svg(False, shadow="solid"))
    mi.draw(64).save(os.path.join(img_dir, "favicon.png"))
    mi.draw(256).save(os.path.join(img_dir, "icon.png"))
    touch = Image.new("RGBA", (180, 180), TILE_BOTTOM + (255,))
    big = mi.draw(180)
    touch.alpha_composite(big)
    touch.convert("RGB").save(os.path.join(img_dir, "apple-touch-icon.png"))


def sheet(out):
    """One picture of the kit, for review."""
    W, H = 1600, 1100
    img = Image.new("RGB", (W, H), (14, 16, 21))
    d = ImageDraw.Draw(img)

    def text(xy, s, color, scale=1):
        x, y = xy
        for ch in s.encode("cp437", "replace"):
            g = FONT[ch * 16:(ch + 1) * 16]
            for r in range(16):
                for c in range(8):
                    if g[r] & (0x80 >> c):
                        d.rectangle([x + c * scale, y + r * scale, x + c * scale + scale - 1, y + r * scale + scale - 1], fill=color)
            x += 8 * scale
    text((40, 32), "BadTerm logo kit", (255, 255, 255), 3)
    text((40, 84), "The icon is the wordmark's first letter and its cursor.", (140, 148, 163))
    ic = mi.draw(512, apple=True).resize((360, 360), Image.LANCZOS)
    img.paste(ic, (30, 120), ic)
    rows, color = word_grid()
    wm = png_blocks(rows, color, 14)
    img.paste(wm, (420, 170), wm)
    tg = "t h e   b a d a s s   t e r m i n a l"
    text((420 + (wm.width - len(tg) * 16) // 2, 170 + wm.height + 30), tg, VGA[7], 2)
    # one-color on light and dark
    for i, (bg, fg) in enumerate((((236, 236, 238), (0, 0, 0)), ((0, 0, 0), (255, 255, 255)), ((217, 142, 62), (20, 12, 3)))):
        x, y = 420 + i * 390, 420
        d.rectangle([x, y, x + 370, y + 150], fill=bg)
        m = png_blocks(mark_grid()[0], lambda a, b: fg, 6, shadow=False)
        img.paste(m, (x + 20, y + 40), m)
        w2 = png_blocks(rows, lambda a, b: fg, 3, shadow=False)
        img.paste(w2, (x + 130, y + 56), w2)
    # sizes
    x = 40
    y = 640
    text((40, 610), "App icon at 16 24 32 48 64 128 256", (140, 148, 163))
    for n in (16, 24, 32, 48, 64, 128, 256):
        im = mi.draw(n)
        img.paste(im, (x, y + (256 - n)), im)
        x += n + 30
    # the palette
    text((40, 940), "Colors", (140, 148, 163))
    sw = [("white", VGA[15]), ("light cyan", VGA[11]), ("cyan", VGA[3]), ("blue", VGA[1]), ("shadow", VGA[8]),
          ("cursor", COPPER_TOP), ("copper", COPPER), ("tile", TILE_TOP), ("tile", TILE_BOTTOM)]
    for i, (name, c) in enumerate(sw):
        x = 40 + i * 168
        d.rectangle([x, 970, x + 150, 1030], fill=c, outline=(40, 46, 60))
        text((x, 1040), name, (170, 170, 170))
        text((x, 1060), HEX(c), (110, 116, 130))
    img.save(os.path.join(out, "logo-kit.png"))


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "brand"
    names = kit(out)
    sheet(out)
    print("\n".join(names))
