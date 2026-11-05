#!/usr/bin/env python3
"""Copy a release to GitHub, github.com/arelas/badterm.

That repository is a push mirror of git.badcluster.dev/arelas/badterm, where
BadTerm is developed, built and released. The mirror carries the code and
tags; this carries each release's files: a GitHub release for the same tag,
marked prerelease as ours are, with the same files.

    GH_RELEASE_TOKEN=... python3 -I packaging/github-release.py TAG SHA DIR
    GH_RELEASE_TOKEN=... python3 -I packaging/github-release.py --from-forgejo TAG

The first is the release job's last step (DIR is its dist/release). The
second copies a release that is already on Forgejo, downloading its files
first, such as one made before the mirror. Run either again and it fills in
the same release, replacing files of the same name.

GH_RELEASE_TOKEN is a fine-grained GitHub token for that one repository,
with Contents read and write. Without it this says so and does nothing, so a
release never waits on GitHub. GH_REPO picks another repository (owner/name).
Python 3, standard library only.
"""

import json
import os
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.github.com"
UPLOADS = "https://uploads.github.com"
FORGEJO = "https://git.badcluster.dev/api/v1/repos/arelas/badterm"
AGENT = "badterm-release-copy"


def call(method, url, token=None, data=None, ctype="application/json"):
    """(status, decoded JSON or None)."""
    headers = {"User-Agent": AGENT, "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if data is not None:
        headers["Content-Type"] = ctype
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=600) as r:
            body = r.read()
            return r.status, json.loads(body) if body else None
    except urllib.error.HTTPError as e:
        body = e.read()
        try:
            return e.code, json.loads(body) if body else None
        except ValueError:
            return e.code, {"message": body[:200].decode("utf-8", "replace")}


def fail(what, status, reply):
    msg = (reply or {}).get("message", "") if isinstance(reply, dict) else ""
    sys.exit(f"github-release: {what}: HTTP {status} {msg}")


def from_forgejo(tag, folder):
    """Download the Forgejo release's files into `folder`; its commit."""
    status, rel = call("GET", f"{FORGEJO}/releases/tags/{urllib.parse.quote(tag)}")
    if status != 200:
        fail(f"no Forgejo release {tag}", status, rel)
    for a in rel.get("assets", []):
        req = urllib.request.Request(a["browser_download_url"], headers={"User-Agent": AGENT})
        with urllib.request.urlopen(req, timeout=600) as r, open(os.path.join(folder, a["name"]), "wb") as f:
            f.write(r.read())
        print(f"downloaded {a['name']}")
    status, t = call("GET", f"{FORGEJO}/tags/{urllib.parse.quote(tag)}")
    if status != 200:
        fail(f"no Forgejo tag {tag}", status, t)
    return t["commit"]["sha"]


def copy(tag, sha, folder, token, repo):
    status, rel = call("GET", f"{API}/repos/{repo}/releases/tags/{urllib.parse.quote(tag)}", token)
    if status == 404:
        version = tag.lstrip("v")
        body = (
            f"BadTerm {version}, built and released at "
            f"https://git.badcluster.dev/arelas/badterm/releases/tag/{tag} (this is a copy).\n\n"
            "Downloads and how to install: https://badterm.com/#download. "
            "SHA256SUMS has every file's checksum."
        )
        new = {"tag_name": tag, "target_commitish": sha, "name": f"BadTerm {version}", "body": body,
               "prerelease": True, "draft": False}
        status, rel = call("POST", f"{API}/repos/{repo}/releases", token, json.dumps(new).encode())
        if status != 201:
            fail(f"creating the release {tag}", status, rel)
        print(f"made the release {tag}")
    elif status != 200:
        fail(f"looking for the release {tag}", status, rel)
    there = {a["name"]: a["id"] for a in rel.get("assets", [])}
    for name in sorted(os.listdir(folder)):
        path = os.path.join(folder, name)
        if not os.path.isfile(path):
            continue
        if name in there:
            status, reply = call("DELETE", f"{API}/repos/{repo}/releases/assets/{there[name]}", token)
            if status != 204:
                fail(f"replacing {name}", status, reply)
        with open(path, "rb") as f:
            data = f.read()
        url = f"{UPLOADS}/repos/{repo}/releases/{rel['id']}/assets?name={urllib.parse.quote(name)}"
        status, reply = call("POST", url, token, data, "application/octet-stream")
        if status != 201:
            fail(f"uploading {name}", status, reply)
        print(f"uploaded {name} ({len(data)} bytes)")
    print(rel["html_url"])


def main():
    args = sys.argv[1:]
    token = os.environ.get("GH_RELEASE_TOKEN", "").strip()
    repo = os.environ.get("GH_REPO", "arelas/badterm")
    if not token:
        print("github-release: GH_RELEASE_TOKEN is not set, so the release is not copied to GitHub.")
        return
    if args[:1] == ["--from-forgejo"] and len(args) == 2:
        with tempfile.TemporaryDirectory() as folder:
            sha = from_forgejo(args[1], folder)
            copy(args[1], sha, folder, token, repo)
    elif len(args) == 3:
        copy(args[0], args[1], args[2], token, repo)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
