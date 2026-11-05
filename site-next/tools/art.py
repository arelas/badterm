"""The website's ANSI art, drawn the way the BBS scene drew logos: on a canvas
of half-block pixels, letters with a short color ramp down them and a dark
shadow down and to the right, thin rules that fade out to dots, and a small
tag in the corner.

  hero: the BADTERM logo (the app's own splash letters, read from
        crates/badterm/src/ui/splash.rs), the tagline, and a BBS-style menu
        whose hotkeys the page makes clickable. 80x25, iCE.
  vortex: an ad for The Vortex BBS, BadTerm's home board, in gold.

Each is returned as (Screen, hotspots), where a hotspot is a clickable run of
cells: {"row", "c0", "c1", "key", "href"}.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ansi import (BLACK, BLUE, BROWN, CYAN, DGRAY, GRAY, LBLUE, LCYAN, WHITE, YELLOW, Pixels, Screen, cp)

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
SPLASH = os.path.join(REPO, "crates", "badterm", "src", "ui", "splash.rs")

LW, LH = 9, 12   # the splash's letter cell, in pixels
STEP = 10        # letter advance (one pixel of space)

# Letters the splash doesn't have, in its style: 3-pixel strokes, corners
# rounded off by a missing pixel.
EXTRA = {
    "V": ["###...###", "###...###", "###...###", "###...###", "###...###", "###...###",
          "####.####", ".#######.", ".#######.", "..#####..", "...###...", "...###..."],
    "O": ["..#####..", ".#######.", "###...###", "###...###", "###...###", "###...###",
          "###...###", "###...###", "###...###", "###...###", ".#######.", "..#####.."],
    "X": ["###...###", "###...###", ".###.###.", ".#######.", "..#####..", "...###...",
          "...###...", "..#####..", ".#######.", ".###.###.", "###...###", "###...###"],
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


def splash_letters():
    """The splash screen's letters, parsed from its source: {char: [rows]}."""
    src = open(SPLASH, encoding="utf-8").read()
    out = {}
    for m in re.finditer(r"\(\s*'(.)',\s*\[(.*?)\],\s*\)", src, re.S):
        rows = re.findall(r'"([#.]+)"', m.group(2))
        assert len(rows) == LH and all(len(r) == LW for r in rows), m.group(1)
        out[m.group(1)] = rows
    assert set("BADTERM") <= set(out), sorted(out)
    return out


LETTERS = {**EXTRA, **splash_letters()}


def logo(p, word, x0, y0, ramp):
    lit = []
    for i, ch in enumerate(word):
        for y, row in enumerate(LETTERS[ch]):
            for x, b in enumerate(row):
                if b == "#":
                    lit.append((x0 + i * STEP + x, y0 + y, y))
    for x, y, _ in lit:              # the shadow first, then the letters over it
        p.set(x + 1, y + 1, SHADOW)
    for x, y, ry in lit:
        p.set(x, y, ramp[ry])


def word_width(word):
    return len(word) * STEP - 1


def rule(s, y, steps, x0=0, width=80):
    for i in range(width):
        d = abs((i + 0.5) - width / 2) / (width / 2)
        for limit, ch, fg in steps:
            if d < limit:
                if ch == 0xFA and i % 2:
                    break  # the last dots every other column
                s.put(x0 + i, y, ch, fg)
                break


def hero(version):
    s = Screen(80, 25)
    p = Pixels(80, 50)
    x0 = (80 - word_width("BADTERM")) // 2
    logo(p, "BADTERM", x0, 6, ICE)          # pixel row 6 = the top of text row 3
    p.onto(s)

    rule(s, 1, ICE_RULE)
    rule(s, 11, ICE_RULE)
    s.centered(13, [("t h e   ", GRAY), ("b a d a s s", WHITE), ("   t e r m i n a l", GRAY)])

    words = ["telnet", "ssh", "modem", "ansi", "petscii", "ripscrip", "sixel", "viewdata"]
    parts = []
    for i, w in enumerate(words):
        if i:
            parts.append((" ù ", DGRAY))
        parts.append((w, CYAN))
    s.centered(15, [(t.replace("ù", "∙"), c) for t, c in parts])

    # The menu: [K] label, hotkey in yellow, brackets dark, labels gray.
    items = [("D", "download", "#download"), ("F", "features", "#features"), ("S", "systems", "#systems"),
             ("P", "phosphor", "#phosphor"), ("C", "call the vortex", "#call")]
    gap = 3
    total = sum(len(f"[{k}] {label}") for k, label, _ in items) + gap * (len(items) - 1)
    x = (80 - total) // 2
    hotspots = []
    for k, label, href in items:
        start = x
        x = s.parts(x, 18, [("[", DGRAY), (k, YELLOW), ("]", DGRAY), (" " + label, GRAY)])
        hotspots.append({"row": 18, "c0": start, "c1": x - 1, "key": k, "href": href})
        x += gap

    # The prompt; the page puts the cursor after it.
    end = s.parts(x0, 21, [("Main menu ", GRAY), ("[", DGRAY), ("D F S P C", YELLOW), ("]", DGRAY), (": ", GRAY)])
    s.parts(1, 24, [(f"v{version}", DGRAY), (" ∙ ", DGRAY), ("badterm.com", CYAN)])
    s.parts(76, 24, [("bc", DGRAY), ("!", DGRAY)])  # the artist's tag: Bad Cluster
    return s, hotspots, {"row": 21, "col": end}


def vortex():
    s = Screen(80, 14)
    p = Pixels(80, 28)
    x0 = (80 - word_width("VORTEX")) // 2
    logo(p, "VORTEX", x0, 4, GOLD)
    p.onto(s)
    rule(s, 0, GOLD_RULE)
    rule(s, 9, GOLD_RULE)
    s.centered(11, [("t h e   ", GRAY), ("v o r t e x", WHITE), ("   b b s", GRAY), ("   ∙   ", DGRAY),
                    ("badterm's home board", BROWN)])
    addr = "telnet://vortexbbs.com"
    start = (80 - len(addr)) // 2
    s.text(start, 13, addr, YELLOW)
    s.parts(76, 13, [("bc", DGRAY), ("!", DGRAY)])
    return s, [{"row": 13, "c0": start, "c1": start + len(addr) - 1, "key": "", "href": "telnet://vortexbbs.com"}]


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    h, spots, cur = hero("0.4.2")
    h.png(os.path.join(out, "hero-test.png"), scale=1)
    v, _ = vortex()
    v.png(os.path.join(out, "vortex-test.png"), scale=1)
    for row in h.cells:
        print("".join(bytes([c[0]]).decode("cp437") for c in row).rstrip())
    print(spots, cur)
