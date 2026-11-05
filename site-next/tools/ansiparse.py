"""Read an ANSI file into cells, as an 80-column terminal draws it: the
SGR colours (bold for bright text, blink for an iCE background), cursor moves,
erases, the last column's delayed wrap, and SAUCE stripped.

    cells = parse(open(path, "rb").read())   # rows of (ch, fg, bg)
"""

FROM_ANSI = [0, 4, 2, 6, 1, 5, 3, 7]


def parse(data, cols=80, ice=True):
    if b"\x1aSAUCE00" in data[-140:]:
        data = data[:data.rindex(b"\x1aSAUCE00")]
    elif b"\x1a" in data:
        data = data[:data.index(b"\x1a")]
    rows = []
    x = y = 0
    fg, bg, bold, blink = 7, 0, False, False
    wrap = False
    saved = (0, 0)

    def row(n):
        while len(rows) <= n:
            rows.append([(0x20, 7, 0)] * cols)
        return rows[n]

    def attr():
        return (fg + (8 if bold else 0), bg + (8 if blink and ice else 0))

    i = 0
    n = len(data)
    while i < n:
        b = data[i]
        if b == 0x1B and i + 1 < n and data[i + 1] == 0x5B:
            j = i + 2
            while j < n and 0x30 <= data[j] <= 0x3F:
                j += 1
            params = data[i + 2:j].decode("latin-1")
            final = chr(data[j]) if j < n else ""
            i = j + 1
            ps = [int(p) if p.isdigit() else None for p in params.lstrip("?").split(";")]
            def p(k, d):
                return ps[k] if k < len(ps) and ps[k] is not None else d
            if final == "m":
                for v in (ps or [0]):
                    v = 0 if v is None else v
                    if v == 0:
                        fg, bg, bold, blink = 7, 0, False, False
                    elif v == 1:
                        bold = True
                    elif v in (2, 22):
                        bold = False
                    elif v in (5, 6):
                        blink = True
                    elif v == 25:
                        blink = False
                    elif 30 <= v <= 37:
                        fg = FROM_ANSI[v - 30]
                    elif v == 39:
                        fg = 7
                    elif 40 <= v <= 47:
                        bg = FROM_ANSI[v - 40]
                    elif v == 49:
                        bg = 0
            elif final in "Hf":
                y, x = max(0, p(0, 1) - 1), max(0, min(cols - 1, p(1, 1) - 1))
                wrap = False
            elif final == "A":
                y = max(0, y - max(1, p(0, 1))); wrap = False
            elif final == "B":
                y += max(1, p(0, 1)); wrap = False
            elif final == "C":
                x = min(cols - 1, x + max(1, p(0, 1))); wrap = False
            elif final == "D":
                x = max(0, x - max(1, p(0, 1))); wrap = False
            elif final == "J" and p(0, 0) == 2:
                rows.clear(); x = y = 0
            elif final == "K":
                r = row(y)
                for k in range(x, cols):
                    r[k] = (0x20, 7, attr()[1])
            elif final == "s":
                saved = (x, y)
            elif final == "u":
                x, y = saved
            continue
        i += 1
        if b == 0x0D:
            x = 0; wrap = False
        elif b == 0x0A:
            y += 1; wrap = False
        elif b == 0x09:
            x = min(cols - 1, (x + 8) & ~7)
        elif b in (0x07, 0x00, 0x1B):
            pass
        else:
            if wrap:
                x = 0; y += 1; wrap = False
            f, g = attr()
            row(y)[x] = (b, f, g)
            if x == cols - 1:
                wrap = True
            else:
                x += 1
    return rows
