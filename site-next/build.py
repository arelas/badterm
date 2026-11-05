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
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(HERE, "tools"))
import ansi  # noqa: E402
import art  # noqa: E402

SRC = os.path.join(HERE, "src")
DIST = os.path.join(HERE, "dist")
OLD = os.path.join(REPO, "site")
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
        full = os.path.join(base, path)
        if not os.path.isfile(full):
            raise SystemExit(f"build: {path} is linked but not in dist/")
        with open(full, "rb") as f:
            digest = hashlib.sha256(f.read()).hexdigest()[:10]
        return f"{attr}{path}?v={digest}{end}"
    return re.sub(r'((?:src|href|data-src|data-art)="|url\(")((?:img|fonts|art)/[^"?#]+|style\.css|app\.js)(")', tag, text)


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

    # The page.
    with open(os.path.join(SRC, "index.html"), encoding="utf-8") as f:
        page = f.read()
    page, assets = stamp(page, release)

    def asset(suffix):
        return next((u for n, u in assets.items() if n.endswith(suffix)), None)
    get = {"windows": asset("windows-x64-setup.exe"), "macos": asset("macos-arm64.dmg"), "linux": "#download"}
    get_attrs = " ".join(f'data-{k}="{html.escape(v)}"' for k, v in get.items() if v)
    page = (page.replace("{{version}}", html.escape(version))
                .replace("{{hotspots}}", html.escape(json.dumps(spots, separators=(",", ":")), quote=False).replace("'", "&#39;"))
                .replace("{{prompt}}", json.dumps(prompt, separators=(",", ":")))
                .replace("{{date}}", datetime.date.today().isoformat())
                .replace("{{get}}", get_attrs))
    leftover = re.findall(r"\{\{\w+\}\}", page)
    if leftover:
        raise SystemExit(f"build: unfilled {leftover}")
    page = fingerprint(page, DIST)
    with open(os.path.join(DIST, "index.html"), "w", encoding="utf-8") as f:
        f.write(page)

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
