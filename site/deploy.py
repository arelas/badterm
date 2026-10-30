#!/usr/bin/env python3
"""Publish the website to badterm.com.

Points the download buttons at the newest release's files (links whose file
that release lacks are left out), then copies the site to the web server
with rsync. nginx serves badterm.com from /var/www/badterm on oracle1.

    site/deploy.py            # build and upload
    site/deploy.py --dry-run  # build into a temporary folder and stop

The server can be changed with BADTERM_SITE_DEST (an rsync destination).
"""

import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request

API = "https://git.badcluster.dev/api/v1/repos/arelas/badterm/releases?limit=1"
DEST = os.environ.get("BADTERM_SITE_DEST", "ubuntu@oracle1.badcluster.dev:/var/www/badterm/")
SITE = os.path.dirname(os.path.abspath(__file__))
FILES = ["index.html", "style.css", "img"]


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
    return page.replace("<span data-version>0.1</span>", f"<span data-version>{html.escape(version)}</span>")


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
    index = os.path.join(out, "index.html")
    with open(index, encoding="utf-8") as f:
        page = stamp(f.read(), release)
    with open(index, "w", encoding="utf-8") as f:
        f.write(page)
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
