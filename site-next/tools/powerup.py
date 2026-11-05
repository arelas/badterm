#!/usr/bin/env python3
"""BADTERM, powering up: the wordmark as a scene logo, 80x25.

    python3 -I site-next/tools/powerup.py OUT.ans [seed]            # the piece, with its caption
    python3 -I site-next/tools/powerup.py OUT.ans [seed] --bare     # the corner left empty
    python3 -I site-next/tools/powerup.py OUT.svg [seed] --line     # on one line, for the site's header

The bare piece is BadTerm's logo: crates/badterm/assets/logo/badterm.ans,
which the app's splash screen and the website's front page both show,
with their own words in the corner. Regenerate it after changing this:

    python3 -I site-next/tools/powerup.py crates/badterm/assets/logo/badterm.ans --bare

BAD over TERM, the scene's stacked layout ("negative / land", "earth /
bound"), TERM tucked under BAD's right half and touching it, so the two words
are one welded mass under one keyline. The letters are drawn by hand on a grid
of whole cells across (an edge anywhere else frays into shade glyphs) and
half-cells ("quads") down: 9 rows tall, stems 4 cells, 0.22 of the cap, as the
masters' 0.23.

Layers, back to front:
  ki        the Super Saiyan aura, a field on the quads: heat falls with the
            distance from the word, its reach longest straight up, where the
            outline breaks into a crown of tongues. White-hot at the keyline,
            white worked over gold, a gold body striated upward, a rim of
            gold dark-shaded over brown (a glow; a dark rim reads as fire).
  arcs      blue-white electricity zigzagging through the gold.
  lift      sparks off the tongues' tips, rocks floating up lit from below,
            the floor cracked and glowing, dust blasting out along it.
  keyline   one black outline around both words: every shape has an edge.
  shadow    a grey ghost one cell right and one quad down.
  letters   Lord Jazz's negative-land stroke: one cyan ground, light green and
            light blue worked over it in vertical streaks, darker to the right.
  caption   the word said plainly, spaced, and the tag.
"""

import os
import random
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ansi import (BLACK, BLUE, BROWN, CYAN, DGRAY, GRAY, LBLUE, LCYAN, LGREEN, LMAGENTA, MAGENTA, WHITE, YELLOW, Screen,
                  quads_svg)  # noqa: E402

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
    """The two words' quads, as a dict (x, qy) -> which word (0 BAD, 1 TERM),
    drawn with the pen (letterpen.py): the letters' claws rise above the cap
    line and their drips fall below the baseline, and the letters press
    together until they touch, so each word is one mass."""
    import letterpen
    L = letterpen.letters()
    sx = 1.2
    bad, bw = letterpen.mask_of("BAD", L, gap=-0.2, sx=sx)
    term, tw = letterpen.mask_of("TERM", L, gap=0.3, sx=sx)
    bx, by = 4, 6                     # BAD's cap line at quad 6: its claws reach up into the aura
    tx, ty = 79 - tw - 1, by + 20     # TERM under BAD's right half; its claws rise into BAD's drips
    mask = {}
    for w, (cells, ox, oy) in enumerate(((bad, bx, by), (term, tx, ty))):
        for x, qy in cells:
            p = (ox + x, oy + qy)
            if 0 <= p[0] < W and 0 <= p[1] < QH:
                mask.setdefault(p, w)
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


# ---------------------------------------------------------------- chrome
def shade_letters(cv, mask, tops):
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

    def band(r):
        return max(0, min(8, r))

    def run(w, r, x):
        a0 = x
        while cells.get((w, r, a0 - 1)) == [True, True]:
            a0 -= 1
        a1 = x
        while cells.get((w, r, a1 + 1)) == [True, True]:
            a1 += 1
        return a0, a1

    for (w, r_raw, x), (t, b) in cells.items():
        top_q = tops[w] + r_raw * 2
        y = top_q // 2
        r = band(r_raw)
        if t and b:
            a0, a1 = run(w, r_raw, x)
            n = a1 - a0 + 1
            i = x - a0
            lit = i < max(1, (n + 1) // 2)                 # the left half of a stroke faces the light
            right_flank = i == n - 1 and n > 1
            streak = (x * 5 + w * 3) % 4 if n >= 2 else 3  # a one-cell stroke stays plain; streaks need room
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
                elif not lit and streak == 2:
                    cv.glyph(x, y, LIGHT, B, C)              # the shadow side: a breath of the blue below
                else:
                    cv.set(x, top_q, C); cv.set(x, top_q + 1, C)
            elif r <= 4:
                if streak in (0, 2):
                    cv.glyph(x, y, LIGHT if r == 3 else MEDIUM, B, C)
                elif streak == 1:
                    cv.glyph(x, y, LIGHT, B, C)              # the blue coming in, a column between
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
                elif streak == 0:
                    cv.glyph(x, y, DARK, B, C)               # the cyan ground still showing through the foot
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
    the keyline's black outside the word: the ghost stroke that stands in for
    a clean drop shadow. Counters stay black, so the letters keep reading."""
    reached = outside(mask)
    for (x, qy) in list(mask):
        p = (x + 1, qy + 1)
        if p not in mask and 0 <= p[0] < W and 0 <= p[1] < QH and reached[p[1], p[0]] \
                and (p[0], p[1] + 1) not in mask and (p[0] + 1, p[1]) not in mask and not pocket(mask, p[0], p[1]):
            cv.set(p[0], p[1], DGRAY)




def ki(cv, mask, rng, keep_clear, side=3.6, down=2.0, base=5.0, sky=1.0):
    """The Super Saiyan aura: ki, not fire. White-hot against the word, white
    worked over gold, a gold body, and a rim of gold dark-shaded over brown,
    a glow and not a burn (a dark rim reads as fire). The body is solid
    only where the ki rises; on the sides it is shaded, a glow and not a slab. Its outline is a crown: flame tongues set at a steady
    beat along the word, tall and short in turn, tallest over the open sky
    above TERM, each with hollow sides that draw it to a point. The bands
    follow the outline, so every tongue is the same nest of colours, smaller
    as it rises. White over gold needs a bright (iCE) background, as the piece
    is flagged."""
    d, dy = distance(mask)
    out = outside(mask)
    xs = np.arange(W)
    spikes = np.zeros(W)
    cx, k = rng.uniform(0.5, 3.0), 0
    beat = []
    while cx < W + 2:
        tall = k % 2 == 0
        open_sky = 1.2 if cx > 44 else 1.0
        hgt = (rng.uniform(9, 12) if tall else rng.uniform(4, 6)) * open_sky * sky
        half = rng.uniform(2.4, 3.0) if tall else rng.uniform(1.8, 2.3)
        spikes = np.maximum(spikes, hgt * np.clip(1 - np.abs(xs - cx) / half, 0, 1) ** 1.5)
        beat.append(cx)
        cx += rng.uniform(4.4, 5.6)
        k += 1
    up = base + spikes                                          # one size all round: BAD has no sky to spare
    # where the two words come close (the crotch under A, by the D) the ki is
    # pinched between them: thinner there, or it piles up into a slab
    d_bad, _ = distance({p: w for p, w in mask.items() if w == 0})
    d_term, _ = distance({p: w for p, w in mask.items() if w == 1})
    pinch = np.clip((np.maximum(d_bad, d_term) - 2.0) / 9.0, 0.4, 1.0)
    heat, upness = {}, {}
    for qy in range(QH):
        for x in range(W):
            if (x, qy) in mask or not out[qy, x] or keep_clear(x, qy) or pocket(mask, x, qy) or d[qy, x] > 40:
                continue
            dist = d[qy, x]
            up_frac = max(0.0, -dy[qy, x] / max(dist, 1e-6))
            down_frac = max(0.0, dy[qy, x] / max(dist, 1e-6))
            reach = (side + (up[x] - side) * up_frac ** 2.2 - (side - down) * down_frac) * pinch[qy, x]
            t = max(0.0, (dist - 1.5) / max(1.0, reach))
            if t < 1:
                heat[(x, qy)] = t
                upness[(x, qy)] = up_frac
    # tidy the field: no lone specks, no pinholes (a quad shut out by the
    # pocket test with the gold all round it)
    N4 = ((1, 0), (-1, 0), (0, 1), (0, -1))
    for _ in range(2):
        for (x, qy) in [p for p in heat if sum((p[0] + a, p[1] + b) in heat for a, b in N4) <= 1]:
            heat.pop((x, qy), None)
        for qy in range(QH):
            for x in range(W):
                if (x, qy) in heat or (x, qy) in mask or not out[qy, x] or keep_clear(x, qy) or d[qy, x] < 1.5:
                    continue
                nb = [heat[(x + a, qy + b)] for a, b in N4 if (x + a, qy + b) in heat]
                if len(nb) >= 3:
                    heat[(x, qy)] = sum(nb) / len(nb)
                    upness[(x, qy)] = 0.0
    STEPS = [(0.12, (FULL, WHITE, WHITE)), (0.22, (DARK, WHITE, YELLOW)), (0.32, (MEDIUM, WHITE, YELLOW)),
             (0.80, (FULL, YELLOW, YELLOW)), (1.01, (DARK, YELLOW, BROWN))]

    def step(t):
        for edge, cell in STEPS:
            if t < edge:
                return cell
    phase = rng.randint(0, 3)

    def sideways_ok(x, y):
        """Solid gold only where the ki is rising."""
        return max(upness.get((x, y * 2), 0), upness.get((x, y * 2 + 1), 0)) > 0.30
    for y in range(H):
        for x in range(W):
            a, b = heat.get((x, y * 2)), heat.get((x, y * 2 + 1))
            if a is None and b is None:
                continue
            if a is None or b is None:
                # an edge or a tongue's tip: a half block on black
                t = a if a is not None else b
                ch, c, _ = step(t)
                c = c if ch == FULL else (WHITE if t < 0.32 else YELLOW)
                cv.set(x, y * 2, c if a is not None else None)
                cv.set(x, y * 2 + 1, c if b is not None else None)
                continue
            lo = step(min(a, b))
            rising = min(upness.get((x, y * 2), 0), upness.get((x, y * 2 + 1), 0)) > 0.30
            if lo == (FULL, YELLOW, YELLOW) and rising and (x + phase) % 4 == 0 and min(a, b) < 0.74:
                cv.glyph(x, y, MEDIUM, WHITE, YELLOW)            # white streaming up out of the core
            elif lo == (FULL, YELLOW, YELLOW) and rising and (x + phase) % 4 == 2 and min(a, b) > 0.38:
                cv.glyph(x, y, DARK, YELLOW, BROWN)              # and the rim's deeper gold hanging down
            elif lo == (FULL, YELLOW, YELLOW) and not sideways_ok(x, y):
                # on the sides the ki isn't rising: a glow, shaded, not a slab
                cv.glyph(x, y, *((MEDIUM, WHITE, YELLOW) if min(a, b) < 0.56 else (DARK, YELLOW, BROWN)))
            elif lo[0] == FULL:
                sa, sb = step(a), step(b)
                cv.set(x, y * 2, sa[1] if sa[0] == FULL else lo[1])
                cv.set(x, y * 2 + 1, sb[1] if sb[0] == FULL else lo[1])
            else:
                cv.glyph(x, y, *lo)
    return d, heat


def put(cv, x, qy, c):
    """Set one quad of a cell the aura may have drawn as a shade glyph, keeping
    the cell's other half the colour the glyph mostly showed, not black."""
    cell = cv.fixed.get((x, qy // 2))
    if cell:
        ch, fg, bg = cell
        keep = bg if ch == LIGHT else fg
        cv.set(x, (qy // 2) * 2, keep)
        cv.set(x, (qy // 2) * 2 + 1, keep)
    cv.set(x, qy, c)


def arcs(cv, rng, d, heat, keep_clear, n=3):
    """Electricity crackling through the gold (the second form): a few
    zigzags of light cyan, white at the bends, drawn over the aura's body,
    never on the black and never against the keyline."""
    def ok(x, qy):
        t = heat.get((x, qy))
        return t is not None and 0.30 < t < 0.86 and d[qy, x] > 2.5 and not keep_clear(x, qy)
    tries = drawn = 0
    while drawn < n and tries < 2000:
        tries += 1
        x, qy = rng.randint(4, W - 5), rng.randint(2, QH - 10)
        if not ok(x, qy):
            continue
        path, bends = [], []
        dx = rng.choice((-1, 1))
        for leg in range(rng.randint(3, 5)):
            run = [(x, qy + i) for i in range(1, rng.randint(2, 4))]
            if not all(ok(*p) for p in run):
                break
            path += run
            x, qy = run[-1]
            bends.append((x, qy))
            jump = [(x + dx * i, qy) for i in range(1, rng.randint(2, 4))]
            if not all(ok(*p) for p in jump):
                break
            path += jump
            x, qy = jump[-1]
            dx = -dx if rng.random() < 0.6 else dx
        if len(path) < 7:
            continue
        for p in path:
            put(cv, p[0], p[1], WHITE if p in bends else LCYAN)
        drawn += 1


def lift(cv, rng, d, heat, floor_q, x_from, keep_clear):
    """The world lifting off the ground: sparks rising off the tongues' tips,
    rocks floating up lit gold from below, the floor cracked and glowing,
    dust blasting out sideways along it."""
    placed = 0
    for _ in range(600):
        if placed >= 9:
            break
        x, qy = rng.randint(1, W - 2), rng.randint(0, QH - 12)
        below = [heat.get((x, qy + k)) for k in range(2, 5)]
        if any((x + a, qy + b) in heat for a in (-1, 0, 1) for b in (-1, 0, 1)) or not any(t is not None and t > 0.8 for t in below) \
                or keep_clear(x, qy) or d[qy, x] < 4:
            continue
        put(cv, x, qy, WHITE if rng.random() < 0.5 else YELLOW)
        placed += 1
    for _ in range(7):
        x, qy = rng.randint(x_from + 2, W - 4), floor_q - rng.randint(2, 9)
        if d[qy, x] < 3 or d[qy + 1, x] < 3 or keep_clear(x, qy):
            continue
        for k in range(rng.choice((1, 2))):
            put(cv, x + k, qy, GRAY if k == 0 else DGRAY)
            put(cv, x + k, qy + 1, YELLOW)
    for x in range(x_from, W - 1):
        if d[floor_q, x] > 1.5:
            put(cv, x, floor_q, DGRAY)
    for _ in range(6):
        x = rng.randint(x_from + 2, W - 4)
        for k in range(rng.randint(2, 4)):
            if 0 <= x < W - 1 and d[floor_q, x] > 1.5:
                put(cv, x, floor_q, WHITE if k == 0 else YELLOW)
            x += rng.choice((-1, 1))
    y = floor_q // 2
    for x in list(range(x_from - 6, x_from + 4)) + list(range(W - 7, W)):
        if 0 <= x < W and rng.random() < 0.75 and not keep_clear(x, floor_q):
            cv.glyph(x, y, LIGHT if rng.random() < 0.7 else MEDIUM, GRAY, BLACK)


SEED = 11   # the build of the aura chosen in round 12
CORNER_X, CORNER_ROW = 20, 14   # the caption's corner, kept clear of the aura: columns 0-19, rows 14-24


def piece(seed=SEED):
    """The wordmark, its aura and the floor, with the caption's corner left
    empty for whoever shows it to fill (the study's caption, the website's
    menu, the app's version line), as a Screen."""
    rng = random.Random(seed)
    mask, bad_rows, term_rows = layout()
    cv = Canvas()

    def keep_clear(x, qy):
        return x < CORNER_X and qy >= CORNER_ROW * 2
    floor_q = term_rows[1] + 4
    d, heat = ki(cv, mask, rng, keep_clear)
    arcs(cv, rng, d, heat, keep_clear)
    lift(cv, rng, d, heat, floor_q, CORNER_X, keep_clear)
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
    shade_letters(cv, mask, {0: bad_rows[0], 1: term_rows[0]})
    s = Screen(W, H)
    cv.onto(s)
    return s


def caption(s):
    """The piece's own caption, stacked in the room under BAD, saying the
    word plainly, and the tag."""
    cx = 2
    s.parts(cx, 15, [("t h e", GRAY)])
    s.parts(cx, 17, [("b a d a s s", WHITE)])
    s.parts(cx, 19, [("t e r m i n a l", GRAY)])
    s.parts(cx, 21, [("\u00b7 ", DGRAY), ("b a d t e r m . c o m", CYAN)])
    s.parts(cx, 23, [("bc", DGRAY), ("!", YELLOW)])
    return s


def line(seed=SEED):
    """The wordmark on one line, for the website's header: BAD and TERM side
    by side in the same letters, keyline, shadow and stroke, with a thin ki
    and a low crown. A Screen sized to fit."""
    import letterpen
    L = letterpen.letters()
    bad, bw = letterpen.mask_of("BAD", L, gap=-0.2, sx=1.2)
    term, tw = letterpen.mask_of("TERM", L, gap=0.3, sx=1.2)
    margin, gap, cap = 3, 2, 8
    cols = margin + bw + gap + tw + margin
    rows = (cap + 18 + 6 + 1) // 2
    saved = W, H, QH
    globals().update(W=cols, H=rows, QH=rows * 2)
    try:
        mask = {}
        for w, (cells, ox) in enumerate(((bad, margin), (term, margin + bw + gap))):
            for x, qy in cells:
                p = (ox + x, cap + qy)
                if 0 <= p[0] < W and 0 <= p[1] < QH:
                    mask.setdefault(p, w)
        rng = random.Random(seed)
        cv = Canvas()

        def never(x, qy):
            return False
        ki(cv, mask, rng, never, side=2.2, down=1.6, base=2.6, sky=0.4)
        for qy in range(QH):
            for x in range(W):
                if (x, qy) not in mask and any((x + dx, qy + dy) in mask for dx in (-1, 0, 1) for dy in (-1, 0, 1)):
                    cv.set(x, qy, None)
                    cv.fixed.pop((x, qy // 2), None)
        reached = outside(mask)
        for qy in range(QH):
            for x in range(W):
                if not reached[qy, x] and (x, qy) not in mask:
                    cv.set(x, qy, None)
        shadow(cv, mask)
        shade_letters(cv, mask, {0: cap, 1: cap})
        s = Screen(W, H)
        cv.onto(s)
    finally:
        globals().update(W=saved[0], H=saved[1], QH=saved[2])
    return s


SAUCE = dict(title="BADTERM", author="Bad Cluster", group="", date="20261105")


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    out = args[0]
    seed = int(args[1]) if len(args) > 1 else SEED
    if "--line" in sys.argv:
        with open(out, "w", encoding="utf-8") as f:
            f.write(quads_svg(line(seed)))
        print("wrote", out)
        return
    s = piece(seed)
    if "--bare" not in sys.argv:
        caption(s)
    with open(out, "wb") as f:
        f.write(s.ans(sauce=SAUCE))
    print("wrote", out)


if __name__ == "__main__":
    main()
