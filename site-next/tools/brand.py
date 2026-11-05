"""BadTerm's logo kit, from the logo and the icon the app ships.

  logo:     the BADTERM wordmark powering up, an 80x25 ANSI piece
            (crates/badterm/assets/logo/badterm.ans), as .ans and PNG
  poster:   the logo with its own caption: the tagline, badterm.com, the tag
  line:     the wordmark on one line, for headers (src/img/badterm-line.svg)
  icon:     the app icon (crates/badterm/assets/icon/): macOS's and the square
            tile, its ANSI piece, and as SVG
  letters:  BAD over TERM in one color, white or black, for print and embroidery
  card:     1200 x 630, for links shared on social sites and chats

The kit is read from files committed with the app. Those are made by
site-next/tools/powerup.py and crates/badterm/assets/icon/make_icon.py, which
need numpy. This module only copies and renders them, so the site builds
without numpy. favicons() writes the site's tab and home-screen icons.

    python3 -I site-next/tools/brand.py OUTDIR    # the kit, and a sheet of it
"""

import os
import re
import shutil
import sys

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
ICON = os.path.join(REPO, "crates", "badterm", "assets", "icon")
LINE = os.path.join(os.path.dirname(HERE), "src", "img", "badterm-line.svg")
sys.path.insert(0, HERE)
import art  # noqa: E402
import letterpen  # noqa: E402
from ansi import FONT, VGA  # noqa: E402


def icon_svg():
    """The icon as SVG: its 32 px picture, a unit a pixel, on its rounded
    slate tile (crates/badterm/assets/icon/badterm.svg, from make_icon.py)."""
    return open(os.path.join(ICON, "badterm.svg"), encoding="utf-8").read()


def letters_svg(fill):
    """BAD over TERM, as the logo sets them (powerup.py's layout), in one
    color: the letters' half-cells, a unit each."""
    L = letterpen.letters()
    bad, _ = letterpen.mask_of("BAD", L, gap=-0.2, sx=1.2)
    term, tw = letterpen.mask_of("TERM", L, gap=0.3, sx=1.2)
    cells = {(4 + x, 6 + y) for x, y in bad} | {(79 - tw - 1 + x, 26 + y) for x, y in term}
    cells = {(x, y) for x, y in cells if 0 <= x < 80 and 0 <= y < 50}
    x0, y0 = min(x for x, _ in cells), min(y for _, y in cells)
    w, h = max(x for x, _ in cells) - x0 + 1, max(y for _, y in cells) - y0 + 1
    rects = []
    for y in range(y0, y0 + h):
        x = x0
        while x < x0 + w:
            if (x, y) in cells:
                n = 1
                while (x + n, y) in cells:
                    n += 1
                rects.append('<rect x="%d" y="%d" width="%d" height="1"/>' % (x - x0, y - y0, n))
                x += n
            else:
                x += 1
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d" shape-rendering="crispEdges">'
            '<title>BadTerm</title><g fill="%s">%s</g></svg>\n' % (w, h, w * 4, h * 4, fill, "".join(rects)))


def og_card(path):
    """1200 x 630: the poster, the logo with its caption, on black."""
    tmp = path + ".tmp.png"
    art.poster().png(tmp, scale=2)
    big = Image.open(tmp).convert("RGB")
    os.remove(tmp)
    card = Image.new("RGB", (1200, 630), (0, 0, 0))
    fit = big.resize((960, 600), Image.LANCZOS)
    card.paste(fit, ((1200 - 960) // 2, 15))
    card.save(path, optimize=True)


def kit(out):
    os.makedirs(out, exist_ok=True)
    logo, poster = art.logo_screen(), art.poster()
    sauce = dict(title="BADTERM", author="Bad Cluster", group="", date="20261105")
    with open(os.path.join(out, "badterm-logo.ans"), "wb") as f:
        f.write(logo.ans(sauce=sauce))
    with open(os.path.join(out, "badterm-poster.ans"), "wb") as f:
        f.write(poster.ans(sauce=sauce))
    for scale, suffix in ((1, ""), (2, "@2x")):
        logo.png(os.path.join(out, f"badterm-logo{suffix}.png"), scale=scale)
        poster.png(os.path.join(out, f"badterm-poster{suffix}.png"), scale=scale)
    shutil.copy(LINE, os.path.join(out, "badterm-line.svg"))
    shutil.copy(os.path.join(ICON, "badterm-icon.ans"), os.path.join(out, "badterm-icon.ans"))
    shutil.copy(os.path.join(ICON, "badterm-mac-1024.png"), os.path.join(out, "badterm-icon-1024.png"))
    shutil.copy(os.path.join(ICON, "badterm-512.png"), os.path.join(out, "badterm-icon-square-512.png"))
    files = {
        "badterm-icon.svg": icon_svg(),
        "badterm-letters-white.svg": letters_svg("#FFFFFF"),
        "badterm-letters-black.svg": letters_svg("#000000"),
    }
    for name, text in files.items():
        with open(os.path.join(out, name), "w", encoding="utf-8") as f:
            f.write(text)
    og_card(os.path.join(out, "badterm-card.png"))
    return sorted(os.listdir(out))


def favicons(img_dir):
    """The site's tab icon (SVG, with a 64 px PNG for the rest) and the
    home-screen icon (180 px, opaque: phones round it themselves)."""
    with open(os.path.join(img_dir, "favicon.svg"), "w", encoding="utf-8") as f:
        f.write(icon_svg())
    shutil.copy(os.path.join(ICON, "badterm-64.png"), os.path.join(img_dir, "favicon.png"))
    shutil.copy(os.path.join(ICON, "badterm-256.png"), os.path.join(img_dir, "icon.png"))
    tile = Image.open(os.path.join(ICON, "badterm-256.png")).convert("RGBA")
    square = Image.new("RGBA", tile.size, (0, 0, 0, 255))
    square.alpha_composite(tile)
    square.convert("RGB").resize((180, 180), Image.BOX).save(os.path.join(img_dir, "apple-touch-icon.png"))


def sheet(out):
    """One picture of the kit, for review."""
    W, H = 1500, 1000
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
    text((40, 28), "BadTerm logo kit", (255, 255, 255), 3)
    poster = Image.open(os.path.join(out, "badterm-poster.png")).convert("RGB")
    img.paste(poster, (40, 90))
    text((40, 500), "The logo, an 80x25 ANSI piece; the icon, its B on slate.", (140, 148, 163))
    mac = Image.open(os.path.join(out, "badterm-icon-1024.png")).convert("RGBA").resize((380, 380), Image.LANCZOS)
    img.paste(mac, (720, 80), mac)
    x, y = 720, 480
    for n in (16, 24, 32, 48, 64, 128):
        im = Image.open(os.path.join(ICON, f"badterm-{n}.png")).convert("RGBA")
        img.paste(im, (x, y + 128 - n), im)
        x += n + 24
    text((40, 560), "On one line, for headers", (140, 148, 163))
    line = open(LINE, encoding="utf-8").read()
    k = 4
    lx, ly = 40, 590
    for x0, y0, n, c in re.findall(r'<rect x="(\d+)" y="(\d+)" width="(\d+)" height="1" fill="#(\w+)"', line):
        x0, y0, n = int(x0), int(y0), int(n)
        d.rectangle([lx + x0 * k, ly + y0 * k, lx + (x0 + n) * k - 1, ly + y0 * k + k - 1], fill="#" + c)
    text((40, 740), "Colors", (140, 148, 163))
    sw = [("white", 15), ("yellow", 14), ("brown", 6), ("light green", 10), ("cyan", 3), ("light blue", 9),
          ("blue", 1), ("dark gray", 8)]
    for i, (name, c) in enumerate(sw):
        x = 40 + i * 175
        d.rectangle([x, 770, x + 150, 850], fill=VGA[c])
        text((x, 860), name, (140, 148, 163))
        text((x, 880), "#%02X%02X%02X" % VGA[c], (90, 98, 113))
    img.save(os.path.join(out, "sheet.png"))


if __name__ == "__main__":
    out = sys.argv[1]
    for name in kit(out):
        print(name)
    sheet(out)
