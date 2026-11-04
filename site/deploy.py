#!/usr/bin/env python3
"""Publish the website to badterm.com.

Points the download buttons at the newest release's files (links whose file
that release lacks are left out), then copies the site to the web server
with rsync. nginx serves badterm.com from /var/www/badterm on oracle1.

    site/deploy.py            # build and upload
    site/deploy.py --dry-run  # build into a temporary folder and stop

The server can be changed with BADTERM_SITE_DEST (an rsync destination).
"""

import hashlib
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.parse
import urllib.request

API = "https://git.badcluster.dev/api/v1/repos/arelas/badterm/releases?limit=1"
DEST = os.environ.get("BADTERM_SITE_DEST", "ubuntu@oracle1.badcluster.dev:/var/www/badterm/")
SITE = os.path.dirname(os.path.abspath(__file__))
FILES = ["index.html", "help.html", "extras.html", "style.css", "img", "fonts"]
# The themes and bezels offered for download, in the repository.
EXTRAS = os.path.join(os.path.dirname(SITE), "extras")


def latest_release():
    # Cloudflare turns away Python's default user agent.
    req = urllib.request.Request(API, headers={"User-Agent": "badterm-site-deploy", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        releases = json.load(r)
    if not releases:
        sys.exit("no releases published yet")
    return releases[0]


def stamp(page, release):
    """Fill in the version, and link each data-asset button to its file."""
    assets = {a["name"]: a["browser_download_url"] for a in release["assets"]}
    version = release["tag_name"].lstrip("v")

    def link(m):
        attrs, suffix, rest, label = m.group(1), m.group(2), m.group(3), m.group(4)
        url = next((u for n, u in assets.items() if n.endswith(suffix)), None)
        if url is None:
            print(f"  {release['tag_name']} has no *{suffix}; leaving that link out")
            return "" if "button" in attrs + rest else label
        rest = re.sub(r'href="[^"]*"', f'href="{html.escape(url)}"', rest)
        return "<a " + " ".join(p for p in (attrs.strip(), rest.strip()) if p) + f">{label}</a>"

    def needs(m):
        """Text about a package, shown only once the release has it."""
        have = any(n.endswith(m.group(1)) for n in assets)
        return m.group(2) if have else m.group(3)

    page = re.sub(r'<a ([^>]*?)data-asset="([^"]+)"([^>]*)>(.*?)</a>', link, page)
    # <span data-needs="suffix">with it<span data-else>without it</span></span>
    page = re.sub(r'<span data-needs="([^"]+)">(.*?)<span data-else>(.*?)</span></span>', needs, page, flags=re.S)
    # Drop the whitespace left where a button was.
    page = re.sub(r"\n[ \t]+(?=\n)", "", page)
    # Whatever version the page was written with.
    return re.sub(r"<span data-version>[^<]*</span>", f"<span data-version>{html.escape(version)}</span>", page)


def fingerprint(text, base):
    """Add `?v=<hash of the file>` to each local file `text` links to
    (style.css, img/..., fonts/...), so a changed file gets a new address:
    Cloudflare and browsers keep stylesheets, images and fonts for weeks."""

    def tag(m):
        attr, path = m.group(1), m.group(2)
        try:
            with open(os.path.join(base, urllib.parse.unquote(path)), "rb") as f:
                digest = hashlib.sha256(f.read()).hexdigest()[:10]
        except OSError:
            return m.group(0)
        return f"{attr}{path}?v={digest}{m.group(3)}"

    return re.sub(r'((?:src|href)="|url\(")((?:img|fonts|extras)/[^"?]+|style\.css)(")', tag, text)


def card(preview, alt, name, author, description, download, size):
    e = html.escape
    url = urllib.parse.quote
    return (
        '<article class="item">'
        f'<img class="pixels" src="{url(preview)}" width="{size[0]}" height="{size[1]}" alt="{e(alt)}" loading="lazy">'
        '<div class="item-body">'
        f"<h3>{e(name)}</h3>"
        f'<p class="by">by {e(author)}</p>'
        f"<p>{e(description)}</p>"
        f'<a class="button" href="{url(download)}" download>Download</a>'
        "</div></article>"
    )


def extras(page, out):
    """Copy the themes and bezels into the site, and list them on the
    extras page (in place of its <!-- themes --> and <!-- bezels -->)."""
    dest = os.path.join(out, "extras")
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
                           t["name"], t.get("author", ""), t.get("description", ""), f"extras/themes/{name}", (640, 400)))
    with open(os.path.join(EXTRAS, "bezels", "bezels.toml"), "rb") as f:
        catalog = tomllib.load(f)
    bezels = []
    for b in catalog.get("bezel", []):
        shutil.copy(os.path.join(EXTRAS, "bezels", b["file"]), os.path.join(dest, "bezels", b["file"]))
        stem = b["file"][: -len(".png")].lower().replace(" ", "-")
        bezels.append(card(f"extras/previews/bezel-{stem}.png", f"BadTerm's splash screen in the {b['name']} bezel",
                           b["name"], b.get("author", ""), b.get("description", ""), f"extras/bezels/{b['file']}", (640, 560)))
    print(f"  extras: {len(themes)} themes, {len(bezels)} bezels")
    return page.replace("<!-- themes -->", "\n".join(themes)).replace("<!-- bezels -->", "\n".join(bezels))


def main():
    dry = "--dry-run" in sys.argv[1:]
    release = latest_release()
    print(f"Latest release: {release['tag_name']}")
    out = tempfile.mkdtemp(prefix="badterm-site-")
    for name in FILES:
        src = os.path.join(SITE, name)
        if os.path.isdir(src):
            shutil.copytree(src, os.path.join(out, name))
        else:
            shutil.copy(src, out)
    # The stylesheet first: its fonts' addresses change its own hash.
    css = os.path.join(out, "style.css")
    with open(css, encoding="utf-8") as f:
        sheet = fingerprint(f.read(), out)
    with open(css, "w", encoding="utf-8") as f:
        f.write(sheet)
    for name in ("index.html", "help.html", "extras.html"):
        path = os.path.join(out, name)
        with open(path, encoding="utf-8") as f:
            page = f.read()
        if name == "index.html":
            page = stamp(page, release)
        if name == "extras.html":
            page = extras(page, out)
        with open(path, "w", encoding="utf-8") as f:
            f.write(fingerprint(page, out))
    if dry:
        print(f"Built in {out}")
        return
    # Readable by nginx whatever the local umask.
    cmd = ["rsync", "-rlt", "--delete", "--chmod=D755,F644", out + "/", DEST]
    print(" ".join(cmd))
    subprocess.run(cmd, check=True)
    shutil.rmtree(out)
    print("Published: https://badterm.com/")


if __name__ == "__main__":
    main()
