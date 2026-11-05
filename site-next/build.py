#!/usr/bin/env python3
"""Build the new BadTerm website (and, with --deploy, publish it).

    python3 -I site-next/build.py             # build into site-next/dist
    python3 -I site-next/build.py --deploy    # build, then rsync to the server

The page links its downloads to the newest release on git.badcluster.dev (as
site/deploy.py does), draws its ANSI art (tools/art.py), makes its VGA web font
from BadTerm's own glyphs (tools/vgafont.py), and adds ?v=<hash> to every file
it links, so Cloudflare's week- and month-long caches never serve a stale one.

It deploys to BADTERM_NEXT_DEST, by default the preview at
https://badterm.com/newsitedemo/ (/var/www/badterm-newsitedemo on oracle1:
outside /var/www/badterm, which the release job's rsync --delete owns).
Needs Python 3.11+, Pillow, fontTools and brotli.
"""

import base64
import datetime
import hashlib
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(HERE, "tools"))
import ansi  # noqa: E402
import art  # noqa: E402

SRC = os.path.join(HERE, "src")
DIST = os.path.join(HERE, "dist")
OLD = os.path.join(REPO, "site")
EXTRAS = os.path.join(REPO, "extras")
API = "https://git.badcluster.dev/api/v1/repos/arelas/badterm/releases?limit=1"
RELEASES = "https://git.badcluster.dev/arelas/badterm/releases"
DEST = os.environ.get("BADTERM_NEXT_DEST", "ubuntu@oracle1.badcluster.dev:/var/www/badterm-newsitedemo/")


def latest_release():
    try:
        req = urllib.request.Request(API, headers={"User-Agent": "badterm-site-build", "Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=30) as r:
            releases = json.load(r)
        if releases:
            return releases[0]
    except OSError as e:
        print(f"  can't read the releases ({e}); linking to the releases page", file=sys.stderr)
    with open(os.path.join(REPO, "Cargo.toml"), "rb") as f:
        version = tomllib.load(f)["workspace"]["package"]["version"]
    return {"tag_name": f"v{version}", "assets": []}


def stamp(page, release):
    """Point each data-asset button at its file in the release, or drop it."""
    assets = {a["name"]: a["browser_download_url"] for a in release["assets"]}

    def link(m):
        attrs, suffix, rest, label = m.group(1), m.group(2), m.group(3), m.group(4)
        url = next((u for n, u in assets.items() if n.endswith(suffix)), None)
        if url is None:
            if assets:
                print(f"  {release['tag_name']} has no *{suffix}; leaving that button out")
                return ""
            return m.group(0)
        rest = re.sub(r'href="[^"]*"', f'href="{html.escape(url)}"', rest)
        return f"<a {attrs.strip()} {rest.strip()}>{label}</a>".replace("  ", " ")

    page = re.sub(r'<a ([^>]*?)data-asset="([^"]+)"([^>]*)>(.*?)</a>', link, page)
    return re.sub(r"\n[ \t]+(?=\n)", "", page), assets


def fingerprint(text, base):
    """?v=<hash> on every local file linked from `text`."""
    def tag(m):
        attr, path, end = m.group(1), m.group(2), m.group(3)
        full = os.path.join(base, urllib.parse.unquote(path))
        if not os.path.isfile(full):
            raise SystemExit(f"build: {path} is linked but not in dist/")
        with open(full, "rb") as f:
            digest = hashlib.sha256(f.read()).hexdigest()[:10]
        return f"{attr}{path}?v={digest}{end}"
    return re.sub(r'((?:src|href|data-src|data-art)="|url\(")((?:img|fonts|art|extras)/[^"?#]+|style\.css|app\.js)(")', tag, text)


def inner(text, start_re, end_tag):
    """The HTML between the first match of `start_re` and its `end_tag`."""
    m = re.search(start_re, text)
    if not m:
        raise SystemExit(f"build: {start_re!r} not found in the old page")
    return text[m.end():text.index(end_tag, m.end())]


def help_content():
    """The guide's text from site/help.html (the one copy of it): its lede,
    and its article with each section numbered, plus a contents list made
    from those sections, in their order."""
    with open(os.path.join(OLD, "help.html"), encoding="utf-8") as f:
        old = f.read()
    article = inner(old, r'<article class="prose">', "</article>")
    article = re.sub(r'\s*<h1 class="doc-title">.*?</h1>', "", article, count=1, flags=re.S)
    m = re.search(r'\s*<p class="lede">(.*?)</p>', article, re.S)
    lede = f'      <p class="lede">{m.group(1)}</p>' if m else ""
    if m:
        article = article[:m.start()] + article[m.end():]
    toc = []

    def number(m):
        n = len(toc) + 1
        toc.append(f'        <li><a href="#{m.group(1)}"><span class="n">{n:02}</span> {m.group(2)}</a></li>')
        return f'<h2 id="{m.group(1)}"><span class="n vga" aria-hidden="true">{n:02}</span>{m.group(2)}</h2>'
    article = re.sub(r'<h2 id="([^"]+)">(.*?)</h2>', number, article)
    # Links to the old site's pages, to this one's.
    article = article.replace('href="index.html#', 'href="./#').replace('href="index.html"', 'href="./"')
    return {"lede": lede, "article": article.rstrip(), "toc": "\n".join(toc), "toc_count": str(len(toc))}


def size(n):
    return f"{n} bytes" if n < 1024 else f"{n / 1024:.1f} KB"


def card(preview, alt, name, author, description, download, w, h):
    e, q = html.escape, urllib.parse.quote
    file = os.path.basename(download)
    nbytes = os.path.getsize(os.path.join(DIST, download))
    return (
        '      <article class="card">\n'
        f'        <div class="frame"><img class="px" src="{q(preview)}" width="{w}" height="{h}" loading="lazy" alt="{e(alt)}"></div>\n'
        '        <div class="card-body">\n'
        f'          <header><h3 class="vga">{e(name)}</h3><span class="by vga">by {e(author)}</span></header>\n'
        f'          <p>{e(description)}</p>\n'
        f'          <div class="card-dl"><span class="file vga">{e(file)} <span class="dim">{size(nbytes)}</span></span>'
        f'<a class="btn small" href="{q(download)}" download>Download</a></div>\n'
        '        </div>\n'
        '      </article>'
    )


def extras_content():
    """The themes and bezels in the repository's extras/ folder: copied in, and
    a card for each."""
    dest = os.path.join(DIST, "extras")
    shutil.copytree(os.path.join(EXTRAS, "previews"), os.path.join(dest, "previews"))
    os.makedirs(os.path.join(dest, "themes"))
    os.makedirs(os.path.join(dest, "bezels"))
    themes = []
    for name in sorted(os.listdir(os.path.join(EXTRAS, "themes"))):
        if not name.endswith(".toml"):
            continue
        src = os.path.join(EXTRAS, "themes", name)
        with open(src, "rb") as f:
            t = tomllib.load(f)
        shutil.copy(src, os.path.join(dest, "themes", name))
        stem = name[: -len(".toml")]
        themes.append(card(f"extras/previews/theme-{stem}.png", f"The dialing directory in the {t['name']} theme",
                           t["name"], t.get("author", ""), t.get("description", ""), f"extras/themes/{name}", 640, 400))
    with open(os.path.join(EXTRAS, "bezels", "bezels.toml"), "rb") as f:
        catalog = tomllib.load(f)
    bezels = []
    for b in catalog.get("bezel", []):
        shutil.copy(os.path.join(EXTRAS, "bezels", b["file"]), os.path.join(dest, "bezels", b["file"]))
        stem = b["file"][: -len(".png")].lower().replace(" ", "-")
        bezels.append(card(f"extras/previews/bezel-{stem}.png", f"BadTerm's splash screen in the {b['name']} bezel",
                           b["name"], b.get("author", ""), b.get("description", ""), f"extras/bezels/{b['file']}", 640, 560))
    with open(os.path.join(OLD, "extras.html"), encoding="utf-8") as f:
        old = f.read()
    share = inner(inner(old, r'<section [^>]*id="share"[^>]*>', "</section>"), r'<div class="prose">', "</div>")
    share = share.replace('href="index.html#', 'href="./#')
    return {"themes": "\n".join(themes), "bezels": "\n".join(bezels), "share": share.strip(),
            "theme_count": str(len(themes)), "bezel_count": str(len(bezels))}


def page(name, fields, sauce_title):
    """A page from src/: the shared header and footer, its fields filled in,
    its own link in the header marked, every file it links fingerprinted."""
    def read(n):
        with open(os.path.join(SRC, n), encoding="utf-8") as f:
            return f.read()
    nav = read("_nav.html")
    stem = name[: -len(".html")]
    nav = nav.replace(f'data-page="{stem}"', 'aria-current="page"')
    nav = re.sub(r' data-page="\w+"', "", nav)
    footer = read("_footer.html").replace("{{sauce_title}}", html.escape(sauce_title)).replace(
        "{{sauce_pad}}", " " * max(1, 28 - len(sauce_title)))
    text = read(name).replace("{{nav}}", nav.rstrip()).replace("{{footer}}", footer.rstrip())
    fields = {"date": datetime.date.today().isoformat(), **fields}
    for k, v in fields.items():
        text = text.replace("{{" + k + "}}", v)
    leftover = re.findall(r"\{\{\w+\}\}", text)
    if leftover:
        raise SystemExit(f"build: {name}: unfilled {leftover}")
    text = fingerprint(text, DIST)
    with open(os.path.join(DIST, name), "w", encoding="utf-8") as f:
        f.write(text)


def build():
    if os.path.isdir(DIST):
        shutil.rmtree(DIST)
    for d in ("img", "fonts", "art"):
        os.makedirs(os.path.join(DIST, d))

    release = latest_release()
    version = release["tag_name"].lstrip("v")
    print(f"Release: {release['tag_name']} ({len(release['assets'])} files)")

    # Fonts: BadTerm VGA from the app's glyphs; IBM Plex from the current site.
    subprocess.run([sys.executable, "-I", os.path.join(HERE, "tools", "vgafont.py"),
                    os.path.join(DIST, "fonts", "badterm-vga.woff2")], check=True)
    for name in ("ibm-plex-sans-400.woff2", "ibm-plex-sans-600.woff2", "OFL.txt"):
        shutil.copy(os.path.join(OLD, "fonts", name), os.path.join(DIST, "fonts", name))

    # The art.
    date = datetime.date.today().strftime("%Y%m%d")
    sauce = dict(title="BadTerm", author="Bad Cluster", group="", date=date)
    hero, spots, prompt = art.hero(version)
    with open(os.path.join(DIST, "art", "hero.ans"), "wb") as f:
        f.write(hero.ans(sauce=sauce))
    still = ansi.Screen(80, 26)          # the picture before the page's script runs: the screen and the status bar
    still.cells[:25] = hero.cells
    status = " The Vortex BBS │ 00:00:00 iCE │ Alt: D Dir H Hangup U Upload Z Help X Exit"
    still.text(0, 25, status.ljust(80), ansi.BLACK, ansi.CYAN)
    still.png(os.path.join(DIST, "art", "hero.png"))
    still.png(os.path.join(DIST, "art", "hero-2x.png"), scale=2)
    vortex, _ = art.vortex()
    with open(os.path.join(DIST, "art", "vortex.ans"), "wb") as f:
        f.write(vortex.ans(sauce={**sauce, "title": "The Vortex BBS"}))
    vortex.png(os.path.join(DIST, "art", "vortex.png"))
    for stem, screen, title in (("help", art.help_banner(), "BadTerm help"), ("extras", art.extras_banner(), "BadTerm extras")):
        with open(os.path.join(DIST, "art", f"{stem}.ans"), "wb") as f:
            f.write(screen.ans(sauce={**sauce, "title": title}))
        screen.png(os.path.join(DIST, "art", f"{stem}.png"))

    # Screenshots from the current site.
    for name in os.listdir(os.path.join(OLD, "img")):
        shutil.copy(os.path.join(OLD, "img", name), os.path.join(DIST, "img", name))

    # Script: the VGA glyphs go in with it.
    with open(os.path.join(SRC, "app.js"), encoding="utf-8") as f:
        js = f.read()
    glyphs = base64.b64encode(ansi.FONT).decode()
    assert "__VGA_FONT__" in js
    with open(os.path.join(DIST, "app.js"), "w", encoding="utf-8") as f:
        f.write(js.replace("__VGA_FONT__", glyphs))

    with open(os.path.join(SRC, "style.css"), encoding="utf-8") as f:
        css = fingerprint(f.read(), DIST)
    with open(os.path.join(DIST, "style.css"), "w", encoding="utf-8") as f:
        f.write(css)

    # The pages.
    with open(os.path.join(SRC, "index.html"), encoding="utf-8") as f:
        home = f.read()
    _, assets = stamp(home, release)

    def asset(suffix):
        return next((u for n, u in assets.items() if n.endswith(suffix)), None)
    get = {"windows": asset("windows-x64-setup.exe"), "macos": asset("macos-arm64.dmg"), "linux": "#download"}
    home_fields = {
        "version": html.escape(version),
        "hotspots": html.escape(json.dumps(spots, separators=(",", ":")), quote=False).replace("'", "&#39;"),
        "prompt": json.dumps(prompt, separators=(",", ":")),
        "get": " ".join(f'data-{k}="{html.escape(v)}"' for k, v in get.items() if v),
    }
    page("index.html", home_fields, "BadTerm")
    with open(os.path.join(DIST, "index.html"), encoding="utf-8") as f:
        stamped, _ = stamp(f.read(), release)
    with open(os.path.join(DIST, "index.html"), "w", encoding="utf-8") as f:
        f.write(stamped)
    page("help.html", help_content(), "BadTerm help")
    page("extras.html", extras_content(), "BadTerm extras")

    total = sum(os.path.getsize(os.path.join(d, n)) for d, _, ns in os.walk(DIST) for n in ns)
    print(f"Built {DIST} ({total // 1024} KB)")


def deploy():
    cmd = ["rsync", "-rlt", "--delete", "--chmod=D755,F644", DIST + "/", DEST]
    print(" ".join(cmd))
    subprocess.run(cmd, check=True)
    print("Published: https://badterm.com/newsitedemo/")


if __name__ == "__main__":
    build()
    if "--deploy" in sys.argv[1:]:
        deploy()
