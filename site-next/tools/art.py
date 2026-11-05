"""The website's ANSI art.

  hero: the front page's screen, BadTerm's logo (the app's own splash piece,
        crates/badterm/assets/logo/badterm.ans), with the tagline, a BBS-style
        menu whose hotkeys the page makes clickable, and the prompt in the
        corner the logo leaves for them. 80x25, iCE.
  banner: a page's title (HELP, EXTRAS) in the logo's letters and ki, drawn
        by powerup.py --banner into src/art/, with a spaced-out line under it
        and the tag. 80x16.

hero returns (Screen, hotspots, prompt), where a hotspot is a clickable run of
cells: {"row", "c0", "c1", "key", "href"}.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ansiparse  # noqa: E402
from ansi import BLUE, CYAN, DGRAY, GRAY, LCYAN, WHITE, YELLOW, Screen

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
LOGO = os.path.join(REPO, "crates", "badterm", "assets", "logo", "badterm.ans")
ART = os.path.join(os.path.dirname(HERE), "src", "art")   # the banners

# The retired block letters, kept for tools/iconlab.py (the first icon's
# lab): 9 x 12 pixels, 3-pixel strokes, corners rounded off by a missing pixel. 3-pixel strokes, corners
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

# Their ramp by pixel row: a white highlight, light cyan, cyan, deep blue.
ICE = [WHITE, LCYAN, LCYAN, LCYAN, LCYAN, LCYAN, CYAN, CYAN, CYAN, BLUE, BLUE, BLUE]
# And those of BadTerm's first splash screen.
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


def load(path, rows=25):
    """An 80-column .ans file as a Screen."""
    s = Screen(80, rows)
    for y, row in enumerate(ansiparse.parse(open(path, "rb").read())[:rows]):
        for x, cell in enumerate(row[:80]):
            s.put(x, y, *cell)
    return s


def logo_screen():
    """BadTerm's logo (crates/badterm/assets/logo/badterm.ans, drawn by
    powerup.py) as a Screen, its corner (columns 0-19, rows 14-24) empty."""
    return load(LOGO)


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


def banner(stem, tagline):
    """A page's banner: its title in the logo's letters, in its ki (src/art/
    STEM.ans, drawn by powerup.py --banner, 80x15), a spaced-out line under
    it, and the tag. 80x16."""
    s = Screen(80, 16)
    art = load(os.path.join(ART, f"{stem}.ans"), rows=15)
    s.cells[:15] = art.cells
    s.centered(15, tagline)
    s.parts(76, 15, [("bc", DGRAY), ("!", YELLOW)])
    return s


def help_banner():
    return banner("help", [("e v e r y t h i n g   ", GRAY), ("b a d t e r m", WHITE), ("   d o e s", GRAY)])


def extras_banner():
    return banner("extras", [("t h e m e s", WHITE), ("   \u2219   ", DGRAY), ("b e z e l s", WHITE),
                             ("   \u2219   ", DGRAY), ("f r o m   y o u", YELLOW)])


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    h, spots, cur = hero("0.4.2")
    h.png(os.path.join(out, "hero-test.png"), scale=1)
    help_banner().png(os.path.join(out, "help-test.png"), scale=2)
    extras_banner().png(os.path.join(out, "extras-test.png"), scale=2)
    for row in h.cells:
        print("".join(bytes([c[0]]).decode("cp437") for c in row).rstrip())
    print(spots, cur)
