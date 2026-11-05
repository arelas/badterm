"""A pen for drawing letters on the ANSI grid: whole cells across, half cells
("quads") down, which is a grid of square 8x8 pixels. A letter is a list of
strokes; a stroke is a path of (x, y, width) points, the width varying along
it, so strokes swell, taper and come to points. A grid square is ink when its
centre is within half the local width of the path.

The letters are drawn from a description of how the 1990s lettering masters
built theirs, never traced from them:

  * condensed and vertical: about 0.55 as wide as they are tall;
  * contrast: stems three cells thick, joins and hairlines about half that;
  * pointed terminals: a claw flicking up and to the right off each top,
    reaching into the aura; feet tapering to a drip;
  * curved bowls, joined to the stems with thinner tapers;
  * tall counters, slits more than holes, never narrower than a cell and a half.

    python3 -I site-next/tools/letterpen.py OUT.png    # every letter, 4x, for looking at
"""

import math

H = 18  # the cap height in quads


def seg_dist(px, py, ax, ay, bx, by):
    """Distance from p to the segment ab, and how far along it (0..1) the nearest point is."""
    dx, dy = bx - ax, by - ay
    L = dx * dx + dy * dy
    t = 0.0 if L == 0 else max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / L))
    qx, qy = ax + t * dx, ay + t * dy
    return math.hypot(px - qx, py - qy), t


def smooth(path, steps=6):
    """A Catmull-Rom curve through the points, so a bowl is a curve, not a polygon."""
    if len(path) < 3:
        return path
    pts = [path[0]] + path + [path[-1]]
    out = []
    for i in range(1, len(pts) - 2):
        p0, p1, p2, p3 = pts[i - 1], pts[i], pts[i + 1], pts[i + 2]
        for s in range(steps):
            t = s / steps
            t2, t3 = t * t, t * t * t
            out.append(tuple(0.5 * ((2 * p1[k]) + (-p0[k] + p2[k]) * t + (2 * p0[k] - 5 * p1[k] + 4 * p2[k] - p3[k]) * t2
                                    + (-p0[k] + 3 * p1[k] - 3 * p2[k] + p3[k]) * t3) for k in range(3)))
    out.append(path[-1])
    return out


def raster(strokes, w, h=H, ox=0.0, oy=0.0):
    """The set of (x, qy) grid squares the strokes cover."""
    cells = set()
    paths = [smooth(s) for s in strokes]
    for qy in range(-6, h + 6):
        for x in range(-4, w + 4):
            px, py = x + 0.5, qy + 0.5
            for path in paths:
                hit = False
                for (ax, ay, aw), (bx, by, bw) in zip(path, path[1:]):
                    d, t = seg_dist(px, py, ax, ay, bx, by)
                    if d <= (aw + (bw - aw) * t) / 2:
                        hit = True
                        break
                if hit:
                    cells.add((x, qy))
                    break
    return cells


# ---------------------------------------------------------------- the letters
# Each letter: (width in cells, strokes). y runs 0 (cap top) to 18 (baseline);
# claws may rise above 0 and drips fall below 18.

def claw(x, top=0.6, reach=5.4, rise=5.4, lean=1.0):
    """A blade off the top of a stem: it leaves the stem thick, curls up and to
    the right (lean -1 for the left), and comes to a point, like a tongue of
    the aura caught in the metal."""
    return [(x, top + 3.0, 3.4), (x + 0.2 * lean, top + 0.6, 3.1), (x + 0.9 * lean, top - 1.4, 2.6), (x + 2.0 * lean, top - rise * 0.62, 1.9),
            (x + reach * 0.74 * lean, top - rise * 0.9, 1.1), (x + reach * lean, top - rise, 0.3)]


def stem(x, top=0.6, foot=18.0, drip=True):
    """A stem: thinner at the top, swelling to a heavy foot, a drip under it."""
    pts = [(x, top + 0.4, 2.8), (x, 4.0, 3.6), (x, 12.0, 3.9), (x + 0.1, foot - 0.6, 4.2)]
    if drip:
        pts += [(x - 0.1, foot + 1.0, 1.6), (x - 0.2, foot + 2.6, 0.5)]
    return pts


def letters():
    L = {}
    # B: a clawed stem; the upper bowl leaves the stem as a hairline and swells;
    # the lower bowl is fuller and bellies out past the upper.
    L["B"] = (10, [
        stem(1.8), claw(1.8),
        [(3.0, 1.2, 1.4), (5.8, 0.6, 2.4), (8.2, 2.4, 2.9), (8.0, 5.8, 2.6), (5.4, 8.2, 1.9), (3.0, 8.8, 1.3)],
        [(3.0, 9.0, 1.3), (6.4, 8.8, 2.4), (9.2, 11.4, 3.4), (8.8, 15.6, 3.0), (5.8, 17.8, 2.4), (3.0, 17.8, 1.6)],
    ])
    # A: the left leg leans in to a pointed top that runs on into a claw; the
    # right leg drops heavy; a thin bar.
    L["A"] = (10, [
        [(0.6, 20.4, 0.5), (1.0, 18.6, 2.0), (1.4, 16.4, 3.6), (2.2, 9.0, 3.2), (3.8, 3.0, 2.6), (5.4, 0.8, 2.2),
         (7.0, -1.6, 1.5), (8.6, -3.6, 0.8), (9.6, -4.4, 0.25)],
        [(5.2, 1.0, 2.0), (7.2, 3.6, 2.8), (8.2, 10.0, 3.3), (8.6, 17.2, 3.8), (8.7, 19.4, 1.4), (8.6, 20.8, 0.4)],
        [(2.2, 11.4, 1.8), (5.0, 10.8, 2.3), (7.8, 11.0, 2.0), (9.8, 11.8, 0.4)],
    ])
    # D: a clawed stem and one deep bowl, hairline at its joins, full at its belly.
    L["D"] = (10, [
        stem(1.8), claw(1.8, reach=4.0, rise=4.0),
        [(3.0, 1.0, 1.4), (6.2, 1.4, 2.5), (8.8, 5.0, 3.2), (9.4, 10.0, 3.6), (8.6, 14.6, 3.2), (5.6, 17.8, 2.4), (3.0, 17.8, 1.5)],
    ])
    # T: the bar flares to blades at both ends, the left curling back; the
    # stem tapers to a drip.
    L["T"] = (11, [
        [(0.2, -4.4, 0.3), (0.6, -2.2, 1.4), (1.6, 0.4, 2.6), (5.5, 1.6, 3.2), (8.8, 0.6, 2.4), (9.8, -1.8, 1.3), (10.8, -4.4, 0.3)],
        stem(5.5, top=1.4),
    ])
    # E: a clawed stem, a top arm that sweeps up to a point, a short middle
    # arm, a foot that kicks out and up.
    L["E"] = (9.5, [
        stem(1.8, drip=False), claw(1.8, reach=3.6, rise=4.4),
        [(3.0, 1.4, 3.0), (5.8, 1.2, 3.0), (7.8, 0.6, 2.2), (8.8, -0.6, 1.2), (9.2, -1.8, 0.3)],
        [(3.0, 9.2, 2.6), (5.4, 9.0, 2.4), (7.4, 8.4, 1.4), (8.6, 7.6, 0.3)],
        [(3.0, 17.0, 3.2), (6.0, 17.4, 3.2), (8.4, 16.6, 2.4), (9.6, 14.6, 1.2), (10.0, 12.6, 0.3)],
    ])
    # R: B's stem and upper bowl; the leg leaves the bowl thin, thickens and
    # kicks out to a point past the baseline.
    L["R"] = (10, [
        stem(1.8, top=0.2),
        [(3.0, 1.2, 1.4), (5.8, 0.6, 2.4), (8.4, 2.6, 3.0), (8.2, 6.4, 2.6), (5.4, 9.0, 1.9), (3.0, 9.4, 1.3)],
        [(4.6, 9.6, 1.6), (6.4, 11.6, 2.8), (7.8, 14.6, 3.6), (9.0, 17.6, 3.0), (10.6, 19.4, 1.4), (12.0, 20.0, 0.3)],
    ])
    # M: two clawed stems; the inner strokes hairline at the tops, swelling as
    # they fall to the point of the V.
    L["M"] = (13, [
        stem(1.6), claw(1.6, reach=3.6, rise=4.0, lean=-1.0),
        stem(11.2), claw(11.2, reach=3.8, rise=4.6),
        [(2.8, 1.0, 1.3), (4.6, 4.4, 2.4), (6.4, 9.8, 2.9), (8.2, 4.4, 2.4), (10.0, 1.0, 1.3)],
    ])
    return L


def mask_of(word, letters_, gap=0.0, slant=0.0, sx=1.0, sy=1.0):
    """The quads of a word: letters set side by side, `gap` cells apart (negative
    to interlock); returns (set of (x, qy), width)."""
    cells, x = set(), 0.0
    for ch in word:
        w, strokes = letters_[ch]
        # shift each stroke by x, and slant: lean the top right
        moved = [[((px + slant * (H - py) / H) * sx + x, py * sy, pw * (sx + sy) / 2) for px, py, pw in s] for s in strokes]
        cells |= raster(moved, int(x + w * sx + 8), h=int(H * sy) + 2)
        x += w * sx + gap
    return cells, int(round(x - gap))


if __name__ == "__main__":
    import sys
    from PIL import Image, ImageDraw
    out = sys.argv[1]
    L = letters()
    S = 8
    rows = [("BAD", 0.4), ("TERM", 0.4)]
    imgs = []
    for word, gap in rows:
        m, w = mask_of(word, L, gap=-0.2, sx=1.2, sy=1.0)
        xs = [x for x, _ in m]; ys = [y for _, y in m]
        x0, y0 = min(xs) - 1, min(ys) - 1
        im = Image.new("RGB", ((max(xs) - x0 + 2) * S, (max(ys) - y0 + 2) * S), (0, 0, 0))
        d = ImageDraw.Draw(im)
        for x, y in m:
            d.rectangle([(x - x0) * S, (y - y0) * S, (x - x0) * S + S - 1, (y - y0) * S + S - 1], fill=(85, 255, 255))
        # the cap lines, faintly
        for yy in (0, 18):
            d.line([(0, (yy - y0) * S), (im.width, (yy - y0) * S)], fill=(60, 60, 90))
        imgs.append(im)
    W = max(i.width for i in imgs)
    sheet = Image.new("RGB", (W + 20, sum(i.height for i in imgs) + 30), (30, 30, 36))
    y = 10
    for i in imgs:
        sheet.paste(i, (10, y)); y += i.height + 10
    sheet.save(out)
    print(out, sheet.size)
