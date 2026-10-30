#!/usr/bin/env python3
"""Draw Phosphor's application icon: a CRT with a glowing green prompt,
in the VGA font Phosphor itself uses.

Writes phosphor-<size>.png for each size, phosphor.ico (Windows),
phosphor.icns (macOS) and phosphor-64.rgba (raw pixels for the window icon,
so the app needs no PNG decoder) next to this script. Needs Pillow:

    python3 make_icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
FONT = (HERE.parent / "fonts" / "vga_8x16.bin").read_bytes()
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]

GREEN = (51, 255, 102)
GLOW = (20, 200, 80)
GLASS_TOP = (6, 34, 16)
GLASS_BOTTOM = (2, 14, 6)
BEZEL = (46, 48, 52)
BEZEL_EDGE = (24, 25, 28)
HIGHLIGHT = (86, 90, 96)


def glyph(ch):
    """Rows of the 8x16 VGA glyph for a CP437 code."""
    return FONT[ch * 16 : ch * 16 + 16]


def text_mask(text, scale):
    """A mask with `text` drawn in the VGA font, each pixel `scale` wide."""
    img = Image.new("L", (8 * len(text) * scale, 16 * scale), 0)
    d = ImageDraw.Draw(img)
    for i, ch in enumerate(text.encode("cp437")):
        for y, bits in enumerate(glyph(ch)):
            for x in range(8):
                if bits & (0x80 >> x):
                    x0, y0 = (i * 8 + x) * scale, y * scale
                    d.rectangle([x0, y0, x0 + scale - 1, y0 + scale - 1], fill=255)
    return img


def vertical_gradient(size, top, bottom):
    w, h = size
    img = Image.new("RGB", size)
    for y in range(h):
        t = y / max(h - 1, 1)
        img.paste(tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)), (0, y, w, y + 1))
    return img


def draw(n):
    """The full icon at n x n pixels (n >= 64)."""
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # The case: a rounded square, as app icons on every platform are,
    # lit from above.
    m = round(n * 0.06)
    r = round(n * 0.2)
    case = Image.new("L", (n, n), 0)
    ImageDraw.Draw(case).rounded_rectangle([m, m, n - 1 - m, n - 1 - m], r, fill=255)
    shade = vertical_gradient((n, n), HIGHLIGHT, BEZEL_EDGE)
    img.paste(shade, (0, 0), case)
    inner = Image.new("L", (n, n), 0)
    e = max(n // 96, 1)
    ImageDraw.Draw(inner).rounded_rectangle([m + e, m + 2 * e, n - 1 - m - e, n - 1 - m - 2 * e], r - e, fill=255)
    img.paste(vertical_gradient((n, n), BEZEL, BEZEL), (0, 0), inner)

    # The glass, with scanlines.
    g0 = round(n * 0.17)
    box = (g0, g0, n - g0, n - round(n * 0.2))
    gw, gh = box[2] - box[0], box[3] - box[1]
    glass = vertical_gradient((gw, gh), GLASS_TOP, GLASS_BOTTOM)
    lines = ImageDraw.Draw(glass)
    step = max(n // 128, 2)
    for y in range(0, gh, step * 2):
        lines.rectangle([0, y, gw, y + step - 1], fill=tuple(max(c - 4, 0) for c in GLASS_BOTTOM))
    shape = Image.new("L", (gw, gh), 0)
    ImageDraw.Draw(shape).rounded_rectangle([0, 0, gw - 1, gh - 1], round(n * 0.07), fill=255)
    img.paste(glass, box[:2], shape)

    # The prompt, glowing.
    scale = max(round(gw / 2.1 / 16), 1)
    mask = text_mask(">_", scale)
    tx = box[0] + (gw - mask.width) // 2
    ty = box[1] + (gh - mask.height) // 2
    full = Image.new("L", (n, n), 0)
    full.paste(mask, (tx, ty))
    for radius, strength in ((n / 22, 150), (n / 60, 220)):
        glow = full.filter(ImageFilter.GaussianBlur(radius)).point(lambda v, s=strength: min(255, v * s // 100))
        img.paste(Image.new("RGBA", (n, n), GLOW + (255,)), (0, 0), Image.composite(glow, Image.new("L", (n, n)), shape_at(shape, box, n)))
    img.paste(Image.new("RGBA", (n, n), GREEN + (255,)), (0, 0), full)

    # A soft reflection across the top of the glass.
    sheen = Image.new("L", (gw, gh), 0)
    ImageDraw.Draw(sheen).ellipse([-gw // 2, -gh, gw + gw // 2, gh // 4], fill=10)
    sheen = Image.composite(sheen, Image.new("L", (gw, gh)), shape)
    white = Image.new("RGBA", (gw, gh), (255, 255, 255, 255))
    img.paste(white, box[:2], sheen)
    return img


def shape_at(shape, box, n):
    """The glass shape as a full-size mask, so the glow stays on the glass."""
    m = Image.new("L", (n, n), 0)
    m.paste(shape, box[:2])
    return m


def draw_small(n):
    """Hand-tuned pixel art for 16-48 px, where the full drawing blurs."""
    s = n / 16
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def px(x0, y0, x1, y1, color):
        d.rectangle([round(x0 * s), round(y0 * s), round((x1 + 1) * s) - 1, round((y1 + 1) * s) - 1], fill=color)

    px(1, 1, 14, 14, BEZEL_EDGE)
    px(1, 1, 14, 13, BEZEL)
    px(2, 1, 13, 1, HIGHLIGHT)
    px(3, 3, 12, 11, GLASS_BOTTOM)
    px(3, 3, 12, 5, GLASS_TOP)
    # ">" and "_".
    for x, y in ((4, 5), (5, 6), (6, 7), (5, 8), (4, 9)):
        px(x, y, x, y, GREEN)
    px(8, 9, 11, 9, GREEN)
    # Rounded corners.
    for x, y in ((1, 1), (14, 1), (1, 14), (14, 14)):
        px(x, y, x, y, (0, 0, 0, 0))
    return img


def main():
    images = {}
    for n in SIZES:
        img = draw_small(n) if n <= 48 else draw(n) if n >= 256 else draw(1024).resize((n, n), Image.LANCZOS)
        img.save(HERE / f"phosphor-{n}.png")
        images[n] = img
    (HERE / "phosphor-64.rgba").write_bytes(images[64].convert("RGBA").tobytes())
    images[256].save(HERE / "phosphor.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)],
                     append_images=[images[n] for n in (16, 24, 32, 48, 64, 128)])
    images[1024].save(HERE / "phosphor.icns", append_images=[images[n] for n in (16, 32, 64, 128, 256, 512)])


if __name__ == "__main__":
    main()
