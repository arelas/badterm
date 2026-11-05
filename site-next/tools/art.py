"""The website's ANSI art.

  hero: the front page's screen, BadTerm's logo (the app's own splash piece,
        crates/badterm/assets/logo/badterm.ans), with the tagline, a BBS-style
        menu whose hotkeys the page makes clickable, and the prompt in the
        corner the logo leaves for them. 80x25, iCE.
  banner: a page's title drawn the way the BBS scene drew logos, on a canvas
        of half-block pixels: block letters with a short color ramp down them
        and a dark shadow down and to the right, between thin rules that fade
        out to dots (HELP in ice, EXTRAS in gold), and a small tag.

hero returns (Screen, hotspots, prompt), where a hotspot is a clickable run of
cells: {"row", "c0", "c1", "key", "href"}.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ansiparse  # noqa: E402
from ansi import (BLACK, BLUE, BROWN, CYAN, DGRAY, GRAY, LBLUE, LCYAN, WHITE, YELLOW, Pixels, Screen, cp)

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
LOGO = os.path.join(REPO, "crates", "badterm", "assets", "logo", "badterm.ans")

LW, LH = 9, 12   # the splash's letter cell, in pixels
STEP = 10        # letter advance (one pixel of space)

# More letters in that style: 3-pixel strokes, corners
# rounded off by a missing pixel.
EXTRA = {
    "V": ["###...###", "###...###", "###...###", "###...###", "###...###", "###...###",
          "####.####", ".#######.", ".#######.", "..#####..", "...###...", "...###..."],
    "O": ["..#####..", ".#######.", "###...###", "###...###", "###...###", "###...###",
          "###...###", "###...###", "###...###", "###...###", ".#######.", "..#####.."],
    "X": ["###...###", "###...###", ".###.###.", ".#######.", "..#####..", "...###...",
          "...###...", "..#####..", ".#######.", ".###.###.", "###...###", "###...###"],
    "H": ["###...###", "###...###", "###...###", "###...###", "###...###", "#########",
          "#########", "###...###", "###...###", "###...###", "###...###", "###...###"],
    "L": ["###......", "###......", "###......", "###......", "###......", "###......",
          "###......", "###......", "###......", "###......", "#########", "#########"],
    "P": ["########.", "#########", "###...###", "###...###", "###...###", "#########",
          "########.", "###......", "###......", "###......", "###......", "###......"],
    "S": [".########", "#########", "###......", "###......", "########.", ".########",
          "......###", "......###", "......###", "......###", "#########", "########."],
}

# Color ramps by pixel row, top to bottom (VGA indices). Ice is the app's
# splash ramp: a white highlight, light cyan, cyan, then deep blue.
ICE = [WHITE, LCYAN, LCYAN, LCYAN, LCYAN, LCYAN, CYAN, CYAN, CYAN, BLUE, BLUE, BLUE]
GOLD = [WHITE, YELLOW, YELLOW, YELLOW, YELLOW, YELLOW, BROWN, BROWN, BROWN, BROWN, BROWN, BROWN]
SHADOW = DGRAY

# Rules: (share of the half-width from the middle, character, color).
ICE_RULE = [(0.18, 0xC4, LCYAN), (0.42, 0xC4, CYAN), (0.66, 0xC4, BLUE), (0.80, 0xC4, DGRAY),
            (0.88, 0xF9, DGRAY), (0.95, 0xFA, DGRAY)]
GOLD_RULE = [(0.18, 0xC4, YELLOW), (0.42, 0xC4, BROWN), (0.70, 0xC4, DGRAY), (0.84, 0xF9, DGRAY),
             (0.95, 0xFA, DGRAY)]


# The block letters of BadTerm's first splash screen, 9 x 12 pixels, for the
# pages' banners (HELP, EXTRAS).
BLOCK = {
    "B": ["#######..", "########.", "###...###", "###...###", "###..###.", "#######..",
          "########.", "###...###", "###...###", "###...###", "########.", "#######.."],
    "A": ["..#####..", ".#######.", "###...###", "###...###", "###...###", "#########",
          "#########", "###...###", "###...###", "###...###", "###...###", "###...###"],
    "D": ["#######..", "########.", "###...###", "###...###", "###...###", "###...###",
          "###...###", "###...###", "###...###", "###...###", "########.", "#######.."],
    "T": ["#########", "#########", "...###...", "...###...", "...###...", "...###...",
          "...###...", "...###...", "...###...", "...###...", "...###...", "...###..."],
    "E": ["#########", "#########", "###......", "###......", "###......", "#######..",
          "#######..", "###......", "###......", "###......", "#########", "#########"],
    "R": ["#######..", "########.", "###...###", "###...###", "###...###", "########.",
          "#######..", "###.###..", "###..###.", "###...###", "###...###", "###...###"],
    "M": ["##.....##", "###...###", "####.####", "###.#.###", "###...###", "###...###",
          "###...###", "###...###", "###...###", "###...###", "###...###", "###...###"],
}


LETTERS = {**EXTRA, **BLOCK}


# The logo's cursor: a copper bar, 4 pixels by 2, at the baseline a pixel
# after the last letter (VGA yellow over brown). The app icon is B and this.
CURSOR = [YELLOW, BROWN]


def logo(p, word, x0, y0, ramp, cursor=False):
    lit = []
    for i, ch in enumerate(word):
        for y, row in enumerate(LETTERS[ch]):
            for x, b in enumerate(row):
                if b == "#":
                    lit.append((x0 + i * STEP + x, y0 + y, ramp[y]))
    if cursor:
        cx = x0 + len(word) * STEP
        lit += [(cx + x, y0 + LH - 2 + y, CURSOR[y]) for y in range(2) for x in range(4)]
    for x, y, _ in lit:              # the shadow first, then the letters over it
        p.set(x + 1, y + 1, SHADOW)
    for x, y, c in lit:
        p.set(x, y, c)


def word_width(word, cursor=False):
    return len(word) * STEP - 1 + (5 if cursor else 0)


def rule(s, y, steps, x0=0, width=80):
    for i in range(width):
        d = abs((i + 0.5) - width / 2) / (width / 2)
        for limit, ch, fg in steps:
            if d < limit:
                if ch == 0xFA and i % 2:
                    break  # the last dots every other column
                s.put(x0 + i, y, ch, fg)
                break


def logo_screen():
    """BadTerm's logo (crates/badterm/assets/logo/badterm.ans, drawn by
    powerup.py) as a Screen, its corner (columns 0-19, rows 14-24) empty."""
    s = Screen(80, 25)
    rows = ansiparse.parse(open(LOGO, "rb").read())
    for y, row in enumerate(rows[:25]):
        for x, cell in enumerate(row[:80]):
            s.put(x, y, *cell)
    return s


def poster():
    """The logo with its own caption stacked in the corner: what it is,
    badterm.com, and the tag (as site-next/tools/powerup.py captions it)."""
    s = logo_screen()
    s.parts(2, 15, [("t h e", GRAY)])
    s.parts(2, 17, [("b a d a s s", WHITE)])
    s.parts(2, 19, [("t e r m i n a l", GRAY)])
    s.parts(2, 21, [("\u00b7 ", DGRAY), ("b a d t e r m . c o m", CYAN)])
    s.parts(2, 23, [("bc", DGRAY), ("!", YELLOW)])
    return s


def hero(version):
    """The front page's screen: the logo, and in its corner the tagline, a
    BBS-style menu whose hotkeys the page makes clickable, and the prompt."""
    s = logo_screen()
    x0 = 1
    s.parts(x0, 15, [("the ", GRAY), ("badass", WHITE), (" terminal", GRAY)])
    items = [("D", "download", "#download"), ("F", "features", "#features"), ("S", "systems", "#systems"),
             ("P", "phosphor", "#phosphor"), ("C", "call the vortex", "#call")]
    hotspots = []
    for i, (k, label, href) in enumerate(items):
        y = 17 + i
        end = s.parts(x0, y, [("[", DGRAY), (k, YELLOW), ("]", DGRAY), (" " + label, CYAN)])
        hotspots.append({"row": y, "c0": x0, "c1": end - 1, "key": k, "href": href})
    # The prompt; the page puts the cursor after it.
    end = s.parts(x0, 23, [("Menu ", GRAY), ("[", DGRAY), ("DFSPC", YELLOW), ("]", DGRAY), (": ", GRAY)])
    s.parts(x0, 24, [(f"v{version}", DGRAY)])
    s.parts(17, 24, [("bc", DGRAY), ("!", YELLOW)])  # the artist's tag: Bad Cluster
    assert end < 20 and all(h["c1"] < 20 for h in hotspots), "the corner is 20 columns"
    return s, hotspots, {"row": 23, "col": end}


def banner(word, ramp, rule_steps, tagline):
    """A page's title as a logo: the word in block letters with its ramp and
    shadow between two fading rules, a spaced-out line under it, and the tag.
    80x12."""
    s = Screen(80, 12)
    p = Pixels(80, 24)
    logo(p, word, (80 - word_width(word)) // 2, 4, ramp)
    p.onto(s)
    rule(s, 0, rule_steps)
    rule(s, 9, rule_steps)
    s.centered(11, tagline)
    s.parts(76, 11, [("bc", DGRAY), ("!", DGRAY)])
    return s


def help_banner():
    return banner("HELP", ICE, ICE_RULE, [("e v e r y t h i n g   ", GRAY), ("b a d t e r m", WHITE), ("   d o e s", GRAY)])


def extras_banner():
    return banner("EXTRAS", GOLD, GOLD_RULE, [("t h e m e s", WHITE), ("   \u2219   ", DGRAY), ("b e z e l s", WHITE),
                                                ("   \u2219   ", DGRAY), ("f r o m   y o u", BROWN)])


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    h, spots, cur = hero("0.4.2")
    h.png(os.path.join(out, "hero-test.png"), scale=1)
    help_banner().png(os.path.join(out, "help-test.png"), scale=2)
    extras_banner().png(os.path.join(out, "extras-test.png"), scale=2)
    for row in h.cells:
        print("".join(bytes([c[0]]).decode("cp437") for c in row).rstrip())
    print(spots, cur)
