#!/usr/bin/env python3
"""Write the sample screens for the site's sixel, SyncTERM-picture and
code-page screenshots (img/sixel.png, img/syncterm.png, img/codepage.png)
into this folder: sixel-menu.bin, syncterm-pic.bin and cp866.ans. Needs
Pillow. Then, from the repository's root:

    BADTERM_SIXEL=site/samples/sixel-menu.bin BADTERM_SNAPSHOT_DIR=/tmp/shots \\
        cargo test -p badterm sixel_snapshot -- --ignored      # -> sixel.png
    (the same with syncterm-pic.bin)
    badterm --render site/samples/cp866.ans codepage.png --screen --font cp866
"""

import base64
import math
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
ICON = HERE.parent.parent / "crates/badterm/assets/icon/badterm-256.png"


def sixel(im):
    q = im.convert("RGB").quantize(colors=128)
    pal = q.getpalette()[: 128 * 3]
    w, h = q.size
    px = q.load()
    s = ['\x1bP0;1;0q"1;1;%d;%d' % (w, h)]
    for i in range(len(pal) // 3):
        r, g, b = pal[i * 3 : i * 3 + 3]
        s.append("#%d;2;%d;%d;%d" % (i, r * 100 // 255, g * 100 // 255, b * 100 // 255))
    for band in range(0, h, 6):
        cols = sorted({px[x, y] for x in range(w) for y in range(band, min(band + 6, h))})
        for ci, c in enumerate(cols):
            s.append("#%d" % c)
            row = []
            for x in range(w):
                bits = 0
                for k in range(6):
                    if band + k < h and px[x, band + k] == c:
                        bits |= 1 << k
                row.append(chr(63 + bits))
            out, i = [], 0
            while i < len(row):
                j = i
                while j < len(row) and row[j] == row[i]:
                    j += 1
                n = j - i
                out.append(("!%d%s" % (n, row[i])) if n > 3 else row[i] * n)
                i = j
            s.append("".join(out))
            s.append("$" if ci < len(cols) - 1 else "-")
    s.append("\x1b\\")
    return "".join(s)


def sixel_menu():
    icon = Image.open(ICON).convert("RGBA")
    bg = Image.new("RGBA", icon.size, (0, 0, 0, 255))
    bg.alpha_composite(icon)
    t = "\x1b[2J\x1b[H\x1b[1;36m Sixel pictures\x1b[0;36m, drawn where the BBS puts them\r\n\r\n"
    t += "\x1b[3;3H" + sixel(bg.resize((176, 176), Image.LANCZOS))
    menu = [("G", "Gallery of sixel art"), ("V", "View a picture"), ("U", "Upload your own"), ("Q", "Back to the main menu")]
    for i, (k, v) in enumerate(menu):
        t += f"\x1b[{5 + i * 2};32H\x1b[1;33m[{k}]\x1b[0;37m {v}"
    t += "\x1b[18;32H\x1b[1;30mUp to 256 colors, kept in the\x1b[19;32Hscrollback, scrolling with the text."
    t += "\x1b[23;1H\x1b[0;36mSelection: \x1b[1;37m"
    (HERE / "sixel-menu.bin").write_text(t)


def syncterm_picture():
    w, h = 320, 160
    img = Image.new("RGB", (w, h))
    p = img.load()
    for y in range(h):
        for x in range(w):
            p[x, y] = (int(20 + y * 0.9), int(10 + y * 0.5), int(60 + y * 0.6))
            if (x - 220) ** 2 + (y - 95) ** 2 < 38**2:
                p[x, y] = (255, int(170 + (y - 57) * 0.8), 60)
            hill = 110 + 18 * math.sin(x / 40) + 10 * math.sin(x / 17)
            if y > hill:
                p[x, y] = (int(30 + (y - hill) * 0.5), int(70 + (y - hill) * 0.6), 60)
    ppm = b"P6 %d %d 255\n" % (w, h) + img.tobytes()
    t = "\x1b[2J\x1b[H\x1b[1;37m SyncTERM pictures\x1b[0;37m: PPM and JPEG XL, anywhere on the screen\r\n"
    t += "\x1b_SyncTERM:C;DrawPPMBlob;DX=160;DY=48;ZX=1;ZY=1;%s\x1b\\" % base64.b64encode(ppm).decode()
    t += "\x1b[15;6H\x1b[0;36mDrawn by the BBS with SyncTERM's commands,"
    t += "\x1b[16;6Hfrom its cache on your computer or sent with the page."
    t += "\x1b[18;6H\x1b[1;33mSynchronet's graphical doors draw their frames this way."
    (HERE / "syncterm-pic.bin").write_text(t)


def code_page():
    t = "\x1b[2J\x1b[H"
    t += '\x1b[1;44;37m  Доска объявлений "Вихрь"' + " " * 45 + "узел 1 \x1b[0m\r\n\r\n"
    t += "\x1b[0;36m ╔══════════════════════════════════╗\r\n"
    for k, v in [("С", "Сообщения"), ("Ф", "Файлы"), ("И", "Игры и двери"), ("Ч", "Чат с сисопом"), ("В", "Выход")]:
        t += f" ║ \x1b[1;33m[{k}]\x1b[0;37m {v:<29}\x1b[0;36m║\r\n"
    t += " ╚══════════════════════════════════╝\r\n\r\n"
    t += "\x1b[0;32m Последний звонок: \x1b[1;37mсегодня, 21:14\r\n"
    t += "\x1b[0;32m Кодовая страница: \x1b[1;37mCP866 (шрифт от BBS)\r\n\r\n"
    t += "\x1b[0;37m Ваш выбор: \x1b[1;37m"
    (HERE / "cp866.ans").write_bytes(t.encode("cp866"))


if __name__ == "__main__":
    sixel_menu()
    syncterm_picture()
    code_page()
