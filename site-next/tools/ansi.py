"""A small ANSI art toolkit for the website: a screen of CP437 cells, a
half-block pixel canvas over it, an .ans writer with a SAUCE record, and a PNG
renderer in the VGA font BadTerm draws with.

Colors are VGA indices 0-15. Bright backgrounds (8-15) are iCE colors: the
.ans writer sets them with the blink attribute and flags the file as iCE in
its SAUCE, as the scene's viewers expect.
"""

import os
import struct

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
FONT = open(os.path.join(REPO, "crates", "badterm", "assets", "fonts", "vga_8x16.bin"), "rb").read()

# Names in VGA order (index = attribute value), and the ANSI order SGR 30-37 uses.
BLACK, BLUE, GREEN, CYAN, RED, MAGENTA, BROWN, GRAY = 0, 1, 2, 3, 4, 5, 6, 7
DGRAY, LBLUE, LGREEN, LCYAN, LRED, LMAGENTA, YELLOW, WHITE = 8, 9, 10, 11, 12, 13, 14, 15
# The VGA's 16 colors in attribute order, as BadTerm shows them.
VGA = [
    (0x00, 0x00, 0x00), (0x00, 0x00, 0xAA), (0x00, 0xAA, 0x00), (0x00, 0xAA, 0xAA),
    (0xAA, 0x00, 0x00), (0xAA, 0x00, 0xAA), (0xAA, 0x55, 0x00), (0xAA, 0xAA, 0xAA),
    (0x55, 0x55, 0x55), (0x55, 0x55, 0xFF), (0x55, 0xFF, 0x55), (0x55, 0xFF, 0xFF),
    (0xFF, 0x55, 0x55), (0xFF, 0x55, 0xFF), (0xFF, 0xFF, 0x55), (0xFF, 0xFF, 0xFF),
]
# Attribute color -> ANSI SGR color number (30 + n): attribute order is
# black, blue, green, cyan, red, magenta, brown, gray; ANSI's is
# black, red, green, yellow, blue, magenta, cyan, white.
TO_ANSI = [0, 4, 2, 6, 1, 5, 3, 7]

FULL, LOWER, UPPER, LEFT, RIGHT = 0xDB, 0xDC, 0xDF, 0xDD, 0xDE
LIGHT, MEDIUM, DARK = 0xB0, 0xB1, 0xB2


def cp(text):
    """Unicode text -> CP437 codes."""
    return list(text.encode("cp437"))


class Screen:
    def __init__(self, cols=80, rows=25):
        self.cols, self.rows = cols, rows
        self.cells = [[(0x20, GRAY, BLACK) for _ in range(cols)] for _ in range(rows)]

    def put(self, x, y, ch, fg=GRAY, bg=BLACK):
        if 0 <= x < self.cols and 0 <= y < self.rows:
            self.cells[y][x] = (ch, fg, bg)

    def text(self, x, y, s, fg=GRAY, bg=BLACK):
        for i, ch in enumerate(cp(s)):
            self.put(x + i, y, ch, fg, bg)
        return x + len(s)

    def parts(self, x, y, parts, bg=BLACK):
        """Text in pieces of different colors: [(text, fg), ...]."""
        for s, fg in parts:
            x = self.text(x, y, s, fg, bg)
        return x

    def centered(self, y, parts, bg=BLACK, width=None):
        n = sum(len(s) for s, _ in parts)
        width = width or self.cols
        return self.parts((width - n) // 2, y, parts, bg)

    def fill(self, x0, y0, w, h, ch=0x20, fg=GRAY, bg=BLACK):
        for y in range(y0, y0 + h):
            for x in range(x0, x0 + w):
                self.put(x, y, ch, fg, bg)

    # ---- output ----

    def ans(self, sauce=None, home=True):
        """The screen as ANSI: SGR colors (bold for bright foregrounds, blink
        for iCE backgrounds), each row ended by CR LF, trailing blanks dropped.
        Rows stay inside 79 columns where they can, so no terminal wraps them."""
        out = bytearray()
        if home:
            out += b"\x1b[0m\x1b[2J\x1b[1;1H"
        cur = None
        for y, row in enumerate(self.cells):
            last = len(row)
            while last > 0 and row[last - 1][0] == 0x20 and row[last - 1][2] == BLACK:
                last -= 1
            for ch, fg, bg in row[:last]:
                if ch == 0x20 and bg == BLACK and cur and cur[1] == BLACK:
                    out.append(0x20)  # a blank on black needs no color change
                    continue
                if ch == FULL and bg == BLACK and cur and cur[0] == fg:
                    out.append(ch)
                    continue
                want = (fg, bg)
                if want != cur:
                    out += sgr(cur, want)
                    cur = want
                out.append(ch)
            if y < len(self.cells) - 1:
                if cur and cur[1] != BLACK:
                    out += b"\x1b[0m"
                    cur = None
                out += b"\r\n"
        out += b"\x1b[0m"
        if sauce:
            out += sauce_record(len(out), self.cols, self.rows, **sauce)
        return bytes(out)

    def png(self, path, scale=1, cell_w=8):
        from PIL import Image
        w, h = self.cols * cell_w, self.rows * 16
        img = Image.new("RGB", (w, h))
        px = img.load()
        for y, row in enumerate(self.cells):
            for x, (ch, fg, bg) in enumerate(row):
                glyph = FONT[ch * 16:(ch + 1) * 16]
                for gy in range(16):
                    bits = glyph[gy]
                    for gx in range(cell_w):
                        if gx < 8:
                            on = bits & (0x80 >> gx)
                        else:  # the VGA's ninth column copies the eighth for line graphics
                            on = (0xC0 <= ch <= 0xDF) and (bits & 1)
                        px[x * cell_w + gx, y * 16 + gy] = VGA[fg] if on else VGA[bg]
        if scale != 1:
            img = img.resize((w * scale, h * scale), Image.NEAREST)
        img.save(path, optimize=True)
        return img


def sgr(cur, want):
    """The SGR sequence that changes the colors from `cur` (None: unknown) to
    `want`: bold makes a foreground bright, blink (iCE) a background."""
    fg, bg = want
    parts = []
    if cur is None or (cur[0] >= 8 and fg < 8) or (cur[1] >= 8 and bg < 8):
        parts.append("0")
        cur = (GRAY, BLACK)
    if fg >= 8 and cur[0] < 8:
        parts.append("1")
    if bg >= 8 and cur[1] < 8:
        parts.append("5")
    if fg & 7 != cur[0] & 7:
        parts.append(str(30 + TO_ANSI[fg & 7]))
    if bg & 7 != cur[1] & 7:
        parts.append(str(40 + TO_ANSI[bg & 7]))
    return ("\x1b[" + ";".join(parts) + "m").encode()


def sauce_record(size, cols, rows, title="", author="", group="", date="", ice=True, font="IBM VGA"):
    """A SAUCE 00 record (preceded by the EOF marker): character data, ANSi,
    its width and height, iCE flag, 8-pixel letter spacing, legacy aspect."""
    def field(s, n):
        return s.encode("cp437")[:n].ljust(n, b" ")
    flags = (0x01 if ice else 0) | (0x01 << 1) | (0x01 << 3)
    rec = b"SAUCE" + b"00" + field(title, 35) + field(author, 20) + field(group, 20) + field(date, 8)
    rec += struct.pack("<IBBHHHHBB", size, 1, 1, cols, rows, 0, 0, 0, flags)
    rec += font.encode("ascii").ljust(22, b"\0")
    assert len(rec) == 128, len(rec)
    return b"\x1a" + rec


def quads_svg(s, title="BadTerm", clip=None):
    """A Screen as an SVG of its half-cells, a unit each: every half-cell the
    color its cell mostly shows (a shade, its main ink), black left out, so
    it sits on any dark ground; crisp at any size. With `clip`, the whole
    screen on a black tile with rounded corners (an icon)."""
    top_of = {0xDB: lambda f, b: (f, f), 0xDF: lambda f, b: (f, b), 0xDC: lambda f, b: (b, f),
              0xB2: lambda f, b: (f, f), 0xB1: lambda f, b: (f, f)}
    grid = [[None] * s.cols for _ in range(s.rows * 2)]
    for y in range(s.rows):
        for x in range(s.cols):
            ch, fg, bg = s.cells[y][x]
            t, b = top_of.get(ch, lambda f, b: (b, b))(fg, bg)
            grid[y * 2][x], grid[y * 2 + 1][x] = t, b
    if clip is None:   # crop to the ink
        rows = [i for i, r in enumerate(grid) if any(c not in (None, BLACK) for c in r)]
        cols = [x for x in range(s.cols) if any(grid[y][x] not in (None, BLACK) for y in rows)]
        y0, y1, x0, x1 = rows[0], rows[-1], cols[0], cols[-1]
    else:
        y0, y1, x0, x1 = 0, s.rows * 2 - 1, 0, s.cols - 1
    out = []
    for y in range(y0, y1 + 1):
        x = x0
        while x <= x1:
            c = grid[y][x]
            n = 1
            while x + n <= x1 and grid[y][x + n] == c:
                n += 1
            if c not in (None, BLACK):
                out.append('<rect x="%d" y="%d" width="%d" height="1" fill="#%02X%02X%02X"/>' % ((x - x0, y - y0, n) + tuple(VGA[c])))
            x += n
    w, h = x1 - x0 + 1, y1 - y0 + 1
    body = "".join(out)
    if clip is not None:   # a rounded tile, its corner radius `clip` units, on black
        body = ('<clipPath id="t"><rect width="%d" height="%d" rx="%g"/></clipPath><g clip-path="url(#t)">'
                '<rect width="%d" height="%d" fill="#000000"/>%s</g>' % (w, h, clip, w, h, body))
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d" shape-rendering="crispEdges">'
            '<title>%s</title>%s</svg>\n' % (w, h, w, h, title, body))


class Pixels:
    """A canvas of half-block pixels: two per text cell, stacked. Drawing it
    onto a Screen picks each cell's character from its two colors."""

    def __init__(self, w, h):
        self.w, self.h = w, h
        self.px = [[None] * w for _ in range(h)]

    def set(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = c

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.px[y][x]
        return None

    def onto(self, s, x0=0, y0=0):
        for ty in range(self.h // 2):
            for x in range(self.w):
                top, bottom = self.px[ty * 2][x], self.px[ty * 2 + 1][x]
                if top is None and bottom is None:
                    continue
                if top == bottom:
                    s.put(x0 + x, y0 + ty, FULL, top, BLACK)
                elif top is None:
                    s.put(x0 + x, y0 + ty, LOWER, bottom, BLACK)
                elif bottom is None:
                    s.put(x0 + x, y0 + ty, UPPER, top, BLACK)
                elif bottom >= 8 and top < 8:
                    # Keep the bright color in front, so no iCE background is needed.
                    s.put(x0 + x, y0 + ty, LOWER, bottom, top)
                else:
                    s.put(x0 + x, y0 + ty, UPPER, top, bottom)
