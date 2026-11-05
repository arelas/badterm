#!/usr/bin/env python3
"""BADTERM, powering up: the wordmark as a scene logo, 80x25.

    python3 -I site-next/tools/powerup.py OUT.ans [seed]

BAD over TERM, the scene's stacked layout ("negative / land", "earth /
bound"), TERM tucked under BAD's right half and touching it, so the two words
are one welded mass under one keyline. The letters are drawn by hand on a grid
of whole cells across (an edge anywhere else frays into shade glyphs) and
half-cells ("quads") down: 9 rows tall, stems 4 cells, 0.22 of the cap, as the
masters' 0.23.

Layers, back to front:
  aura      gold energy rising off the word, the Super Saiyan. A field: each
            quad's heat falls with its distance from the word, and the reach
            is longest straight up, where it breaks into tongues and spikes.
            White-hot against the keyline, then yellow, yellow worked over
            brown, brown, and a light-shade rim, as real pack flame is drawn.
  keyline   one black outline around both words: every shape has an edge.
  chrome    ice chrome, iCE's cold metal, in bands across each word (a white
            crown, light-cyan sky, the dark blue horizon, cyan below, a
            light-cyan lip at the floor), lit from the upper left: top and left
            faces brighter, right faces darker, white glints at the joints.
  bolts     lightning forking down through the gold, white core, cyan edge.
  ground    the floor cracking under it, glowing, rock lifting off.
  caption   the word said plainly, spaced, and the tag.
"""

import math
import os
import random
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ansi import (BLACK, BLUE, BROWN, CYAN, DGRAY, GRAY, LBLUE, LCYAN, LGREEN, LMAGENTA, MAGENTA, RED, WHITE, YELLOW, Screen)  # noqa: E402

W, H = 80, 25
QH = H * 2
LIGHT, MEDIUM, DARK = 0xB0, 0xB1, 0xB2
FULL, LOWER, UPPER = 0xDB, 0xDC, 0xDF

B_ = ["###########...", "############..", "#############.", "####.....#####", "####......####", "####......####",
      "####.....#####", "############..", "###########...", "#############.", "####......####", "####......####",
      "####......####", "####......####", "####.....#####", "##############", "#############.", "###########..."]
A_ = ["...########...", "..##########..", ".############.", "#####....#####", "####......####", "####......####",
      "####......####", "####......####", "##############", "##############", "####......####", "####......####",
      "####......####", "####......####", "####......####", "####......####", "####......####", "####......####"]
D_ = ["##########....", "############..", "#############.", "####.....#####", "####......####", "####......####",
      "####......####", "####......####", "####......####", "####......####", "####......####", "####......####",
      "####......####", "####......####", "####.....#####", "#############.", "############..", "##########...."]
T_ = ["#############", "#############", "#############", "....#####....", "....#####....", "....#####....",
      "....#####....", "....#####....", "....#####....", "....#####....", "....#####....", "....#####....",
      "....#####....", "....#####....", "....#####....", "....#####....", "....#####....", "....#####...."]
E_ = ["###########", "###########", "###########", "####.......", "####.......", "####.......",
      "####.......", "#########..", "#########..", "####.......", "####.......", "####.......",
      "####.......", "####.......", "####.......", "###########", "###########", "###########"]
R_ = ["###########...", "############..", "#############.", "####.....#####", "####......####", "####......####",
      "####.....#####", "#############.", "############..", "####...#####..", "####....#####.", "####.....#####",
      "####......####", "####......####", "####......####", "####......####", "####......####", "####......####"]
M_ = ["#####.......#####", "######.....######", "#######...#######", "####.###.###.####", "####..#####..####",
      "####...###...####", "####....#....####", "####.........####", "####.........####", "####.........####",
      "####.........####", "####.........####", "####.........####", "####.........####", "####.........####",
      "####.........####", "####.........####", "####.........####"]


def line(letters):
    return ["." .join(l[y] for l in letters) for y in range(18)]


def layout():
    """The two words' quads, as a dict (x, qy) -> which word (0 BAD, 1 TERM)."""
    mask = {}
    bad, term = line([B_, A_, D_]), line([T_, E_, R_, M_])
    bx, by = 5, 6          # BAD's top-left: x in cells, y in quads
    tx, ty = 78 - len(term[0]), by + 17   # TERM under BAD's right half, touching it
    for w, (rows, ox, oy) in enumerate(((bad, bx, by), (term, tx, ty))):
        for y, r in enumerate(rows):
            for x, c in enumerate(r):
                if c == "#":
                    mask[(ox + x, oy + y)] = w
    return mask, (by, by + 17), (ty, ty + 17)


# ---------------------------------------------------------------- canvas

class Canvas:
    def __init__(self):
        self.q = np.full((QH, W), -1, dtype=int)     # quad colours, -1 black
        self.fixed = {}

    def set(self, x, qy, c):
        if 0 <= x < W and 0 <= qy < QH:
            self.q[qy, x] = -1 if c is None else c
            self.fixed.pop((x, qy // 2), None)

    def glyph(self, x, y, ch, fg, bg):
        if 0 <= x < W and 0 <= y < H:
            assert ch not in (LIGHT, MEDIUM, DARK) or fg != bg, "a shade glyph mixes two inks"
            self.fixed[(x, y)] = (ch, fg, bg)
            self.q[y * 2, x] = self.q[y * 2 + 1, x] = -1

    def onto(self, s):
        for y in range(H):
            for x in range(W):
                if (x, y) in self.fixed:
                    s.put(x, y, *self.fixed[(x, y)])
                    continue
                t, b = int(self.q[y * 2, x]), int(self.q[y * 2 + 1, x])
                if t < 0 and b < 0:
                    continue
                if t == b:
                    s.put(x, y, FULL, t, BLACK)
                elif t < 0:
                    s.put(x, y, LOWER, b, BLACK)
                elif b < 0:
                    s.put(x, y, UPPER, t, BLACK)
                elif b >= 8 and t < 8:
                    s.put(x, y, LOWER, b, t)
                else:
                    s.put(x, y, UPPER, t, b)


# ---------------------------------------------------------------- the aura

def distance(mask):
    """Euclidean distance (in quads; a cell is as wide as a quad is tall) from
    every quad to the nearest quad of the word."""
    pts = np.array(list(mask.keys()))
    yy, xx = np.mgrid[0:QH, 0:W]
    d = np.full((QH, W), 1e9)
    dy_best = np.zeros((QH, W))
    for x, y in pts:
        dd = np.hypot(xx - x, yy - y)
        closer = dd < d
        d = np.where(closer, dd, d)
        dy_best = np.where(closer, yy - y, dy_best)
    return d, dy_best


def outside(mask):
    """Quads the outside reaches (not the letters' counters)."""
    seen = np.zeros((QH, W), dtype=bool)
    todo = [(x, y) for x in range(W) for y in (0, QH - 1)] + [(x, y) for y in range(QH) for x in (0, W - 1)]
    while todo:
        x, y = todo.pop()
        if not (0 <= x < W and 0 <= y < QH) or seen[y, x] or (x, y) in mask:
            continue
        seen[y, x] = True
        todo += [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
    return seen


def pocket(mask, x, qy, span=12):
    """A quad with the word close on both its left and its right, and over
    it: the room under a T's bar, the channel between two letters."""
    left = any((x - k, qy) in mask for k in range(1, span))
    right = any((x + k, qy) in mask for k in range(1, span))
    over = any((x, qy - k) in mask for k in range(1, 19)) or any((x, qy + k) in mask for k in range(1, 19))
    narrow = any((x - k, qy) in mask for k in range(1, 3)) and any((x + k, qy) in mask for k in range(1, 3))
    return (left and right and over) or narrow


def aura(cv, mask, rng):
    d, dy = distance(mask)
    out = outside(mask)
    # how far the energy reaches, by column: tongues and the tall spikes of hair
    xs = np.arange(W)
    tongues = 4.0 + 2.2 * np.sin(xs * 0.9 + rng.uniform(0, 6)) + 1.6 * np.sin(xs * 2.3 + rng.uniform(0, 6))
    spikes = np.zeros(W)
    for cx in rng.sample(range(4, 76), 9):
        hgt, wid = rng.uniform(9, 17), rng.uniform(2.5, 4.5)
        spikes = np.maximum(spikes, hgt * np.clip(1 - np.abs(xs - cx) / wid, 0, 1) ** 1.4)
    up = 5.0 + tongues + spikes                       # reach straight up
    # every column's tongue burns a little longer or shorter than its neighbours'
    streak = np.array([0.32 * math.sin(x * 1.9 + 1.3) * math.cos(x * 0.61 + 0.4) + rng.uniform(-0.08, 0.08) for x in range(W)])
    side, down = 4.0, 2.5
    heat_rgb = {}
    for qy in range(QH):
        for x in range(W):
            if (x, qy) in mask or d[qy, x] > 40 or not out[qy, x] or pocket(mask, x, qy):
                continue
            dist = d[qy, x]
            up_frac = max(0.0, -dy[qy, x] / max(dist, 1e-6))          # 1 straight above the word
            down_frac = max(0.0, dy[qy, x] / max(dist, 1e-6))
            reach = side + (up[x] - side) * up_frac ** 1.5 - (side - down) * down_frac
            reach *= 1 + rng.uniform(-0.10, 0.10)                     # ragged edges
            t = (dist - 2.0) / max(1.0, reach)                         # 0 at the keyline
            t += streak[x] * (0.25 + 0.75 * up_frac)                   # tongues: alternate columns burn shorter
            heat_rgb[(x, qy)] = t
    # paint: solids first, then cells that sit across a heat boundary become blends
    # Shaded the way Lord Jazz shades a stroke: one ground under the fill and
    # a bright ink drawn over it, its density stepping down as the heat falls.
    # Two solid tones, yellow inside and brown outside, split where the heat
    # crosses 0.45, the split wobbling; a single shade cell of transition at
    # the split; the rim a light shade of brown on black. Shading only where
    # two colours meet, as the tutorials say, never as the fill.
    STEPS = [(0.40, (FULL, YELLOW, BROWN)), (0.50, (DARK, YELLOW, BROWN)), (0.80, (FULL, BROWN, BROWN)),
             (0.88, (MEDIUM, BROWN, RED)), (1.00, (LIGHT, BROWN, BLACK))]

    def step(t):
        for edge, cell in STEPS:
            if t < edge:
                return cell
        return None
    for y in range(H):
        for x in range(W):
            a, b = heat_rgb.get((x, y * 2)), heat_rgb.get((x, y * 2 + 1))
            sa = step(a) if a is not None and a >= 0 else None
            sb = step(b) if b is not None and b >= 0 else None
            if sa is None and sb is None:
                continue
            if sa and sb:
                hot = sa if a <= b else sb
                ch, fg, bg = hot
                if ch == FULL and fg == BROWN and (x % 3 == 1 or (x * 7 + y * 3) % 11 == 0):
                    ch, fg, bg = DARK, BROWN, RED           # streaks and embers in the brown
                elif ch == FULL and fg == YELLOW and x % 3 == 0 and a > 0.15:
                    ch, fg, bg = DARK, YELLOW, BROWN        # streaks in the yellow
                if ch == FULL:
                    cv.set(x, y * 2, fg); cv.set(x, y * 2 + 1, fg)
                else:
                    cv.glyph(x, y, ch, fg, bg)
            else:
                # a tongue's end: a half block on black, in the step's ink
                one = sa or sb
                c = one[1] if one[1] != BROWN or one[0] in (FULL, DARK) else BROWN
                cv.set(x, y * 2, c if sa else None)
                cv.set(x, y * 2 + 1, c if sb else None)
    for _ in range(7):
        x, qy = rng.randint(2, W - 3), rng.randint(0, QH - 12)
        if out[qy, x] and d[qy, x] > 5 and (x, qy // 2) not in cv.fixed and cv.q[qy, x] >= 0:
            cv.set(x, qy, WHITE)
    return d


def bolts(cv, rng, d):
    """Lightning forking down through the gold toward the word, stopping short of it."""
    for sx in (rng.randint(10, 22), rng.randint(56, 70)):
        x, qy = sx, 0
        branch = None
        while qy < QH and d[min(qy, QH - 1), max(0, min(W - 1, x))] > 2.5:
            step = rng.randint(2, 3)
            for _ in range(step):
                if qy >= QH or d[qy, x] <= 2.5:
                    break
                cv.set(x, qy, WHITE)
                for side in (-1, 1):
                    if 0 <= x + side < W and d[qy, x + side] > 2.5 and rng.random() < 0.5:
                        cv.set(x + side, qy, LCYAN)
                qy += 1
            nx = max(1, min(W - 2, x + rng.choice((-2, -1, 1, 2))))
            for xx in range(min(x, nx), max(x, nx) + 1):
                if qy < QH and d[qy, xx] > 2.5:
                    cv.set(xx, qy, LCYAN)
            if branch is None and qy > 6 and rng.random() < 0.35:
                branch = (x, qy)
            x = nx
        if branch:
            bx, bq = branch
            for i in range(rng.randint(5, 9)):
                bx += rng.choice((-1, 1)) * (1 if i % 2 else 2)
                bq += 1
                if not (0 <= bx < W and bq < QH) or d[bq, bx] <= 2.5:
                    break
                cv.set(bx, bq, LCYAN)


def ground(cv, rng, d, floor_q, x_from=1):
    """The floor under the word cracking open, rock lifting off it."""
    for x in range(x_from, W - 1):
        if d[floor_q, x] > 1.5:
            cv.set(x, floor_q, DGRAY)
    for _ in range(9):                                # cracks, glowing
        x = rng.randint(x_from + 2, W - 5)
        for k in range(rng.randint(2, 5)):
            if d[floor_q, x] > 1.5:
                cv.set(x, floor_q, YELLOW if k % 2 == 0 else BROWN)
            x += rng.choice((-1, 1))
    for _ in range(18):                               # chips rising, lit from below
        x, qy = rng.randint(x_from + 1, W - 4), floor_q - rng.randint(1, 8)
        if qy < 0 or d[qy, x] <= 2.5:
            continue
        cv.set(x, qy, GRAY if rng.random() < 0.4 else DGRAY)
        if rng.random() < 0.5 and d[qy, x + 1] > 2.5:
            cv.set(x + 1, qy, DGRAY)
        if qy + 1 < QH and d[qy + 1, x] > 2.5:
            cv.set(x, qy + 1, YELLOW)


# ---------------------------------------------------------------- chrome

# Each palette: the ground under the whole fill, the accent over it by
# height (its hue slides down the stroke), the highlight, the ground the
# foot of the stroke sits on.
PALETTES = {
    "ice": {"ground": CYAN, "accents": [(0.34, LCYAN), (1.01, LBLUE)], "high": WHITE, "foot": BLUE, "edge_low": LBLUE},
    "frieza": {"ground": MAGENTA, "accents": [(0.30, WHITE), (0.62, LCYAN), (1.01, LMAGENTA)], "high": WHITE, "foot": BLUE,
               "edge_low": LMAGENTA},
}


def _jitter(x, y):
    return ((x * 73856093) ^ (y * 19349663)) % 1000 / 1000.0 - 0.5


def shade_letters(cv, mask, tops, palette):
    """Lord Jazz's stroke, measured off negative land row by row. Over one
    cyan ground: the top edge in half blocks with light-green highlights on
    its lit (left) side; the rows under it mostly light green worked over
    cyan; the middle about half light blue over cyan, half cyan; the lower
    rows light blue over cyan going solid light blue, the shadow (right) side
    darker; the foot light blue, with a little of it over grey, the floor's
    reflection. Texture runs in vertical streaks, never scattered. Edges are
    half blocks on black."""
    G, C, B, BD, F = LGREEN, CYAN, LBLUE, BLUE, GRAY
    cells = {}
    for (x, qy), w in mask.items():
        r = (qy - tops[w]) // 2
        cells.setdefault((w, r, x), [False, False])[(qy - tops[w]) % 2] = True

    def run(w, r, x):
        a0 = x
        while cells.get((w, r, a0 - 1)) == [True, True]:
            a0 -= 1
        a1 = x
        while cells.get((w, r, a1 + 1)) == [True, True]:
            a1 += 1
        return a0, a1

    for (w, r, x), (t, b) in cells.items():
        top_q = tops[w] + r * 2
        y = top_q // 2
        if t and b:
            a0, a1 = run(w, r, x)
            n = a1 - a0 + 1
            i = x - a0
            lit = i < max(1, (n * 3 + 2) // 5)            # the left three fifths of a run face the light
            right_flank = i == n - 1 and n > 1
            streak = (x * 5 + w * 3) % 4                   # which vertical streak this column is in
            above_open = (x, top_q - 1) not in mask
            if r == 0 or above_open:
                if lit and streak in (0, 2):
                    cv.glyph(x, y, MEDIUM if streak else DARK, G, C)
                else:
                    cv.set(x, top_q, C); cv.set(x, top_q + 1, C)
            elif r <= 2:
                if lit and streak != 3:
                    cv.glyph(x, y, MEDIUM if streak else DARK, G, C)
                elif streak == 1:
                    cv.glyph(x, y, LIGHT, G, C)
                else:
                    cv.set(x, top_q, C); cv.set(x, top_q + 1, C)
            elif r <= 4:
                if streak in (0, 2):
                    cv.glyph(x, y, LIGHT if r == 3 else MEDIUM, B, C)
                else:
                    cv.set(x, top_q, C); cv.set(x, top_q + 1, C)
            elif r <= 6:
                if right_flank:
                    cv.glyph(x, y, MEDIUM, B, BD)
                elif streak != 1:
                    cv.glyph(x, y, DARK, B, C)
                else:
                    cv.set(x, top_q, B); cv.set(x, top_q + 1, B)
            else:
                if right_flank:
                    cv.glyph(x, y, DARK, B, BD)
                else:
                    cv.set(x, top_q, B); cv.set(x, top_q + 1, B)
        else:
            # a half-filled cell is the silhouette: a cutter on black
            if r <= 2:
                lit = (x - 1, top_q + (0 if t else 1)) not in mask or (x - 2, top_q) not in mask
                c = G if lit and r == 0 else C
            elif r <= 4:
                c = C
            else:
                c = B
            cv.set(x, top_q, c if t else None)
            cv.set(x, top_q + 1, c if b else None)


def shadow(cv, mask):
    """Grey behind the word, a cell right and a quad down, where it falls on
    the keyline's black: the ghost stroke that stands in for a clean drop
    shadow."""
    for (x, qy) in list(mask):
        p = (x + 1, qy + 1)
        if p not in mask and 0 <= p[0] < W and 0 <= p[1] < QH:
            cv.set(p[0], p[1], DGRAY)


PALETTE = "ice"


def main():
    global PALETTE
    out = sys.argv[1]
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 11
    PALETTE = sys.argv[3] if len(sys.argv) > 3 else "ice"
    rng = random.Random(seed)
    mask, bad_rows, term_rows = layout()
    cv = Canvas()
    d = aura(cv, mask, rng)
    bolts(cv, rng, d)
    ground(cv, rng, d, term_rows[1] + 4, x_from=20)
    # the keyline: one quad of black all round the word
    for qy in range(QH):
        for x in range(W):
            if (x, qy) in mask:
                continue
            near = any((x + dx, qy + dy) in mask for dx in (-1, 0, 1) for dy in (-1, 0, 1))
            if near:
                cv.set(x, qy, None)
                cv.fixed.pop((x, qy // 2), None)
    # the counters: black
    reached = outside(mask)
    for qy in range(QH):
        for x in range(W):
            if not reached[qy, x] and (x, qy) not in mask:
                cv.set(x, qy, None)
    shadow(cv, mask)
    shade_letters(cv, mask, {0: bad_rows[0], 1: term_rows[0]}, PALETTE)
    s = Screen(W, H)
    cv.onto(s)
    # the caption, stacked in the room under BAD, saying the word plainly
    cx = 2
    s.parts(cx, 15, [("t h e", GRAY)])
    s.parts(cx, 17, [("b a d a s s", WHITE)])
    s.parts(cx, 19, [("t e r m i n a l", GRAY)])
    s.parts(cx, 21, [("· ", DGRAY), ("b a d t e r m . c o m", CYAN)])
    s.parts(cx, 23, [("bc", DGRAY), ("!", YELLOW)])
    with open(out, "wb") as f:
        f.write(s.ans(sauce=dict(title="BADTERM", author="Bad Cluster", group="", date="20261105")))
    print("wrote", out)


if __name__ == "__main__":
    main()
