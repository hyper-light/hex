#!/usr/bin/env python3
"""bench/competitive: fetches the competing OCI runtimes' release binaries and prints their versions.

  fetch.py <base dir>     binaries land in <base>/bin; each download in its own new <base>/dl/<name>/

Each runtime is its project's latest release, resolved now and recorded (tag, URL, SHA-256, and whether
an upstream checksum verified it), so a result names exactly what it measured. Nothing is executed from
a download directory; an archive is never extracted wholesale (only the one binary member is copied).
"""
import hashlib
import json
import os
import re
import shutil
import sys
import tarfile
import urllib.request

UA = {"User-Agent": "hex-bench-fetch"}


def get(url, dest=None, limit=512 * 2**20):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=120) as r:
        data = r.read(limit + 1)
    if len(data) > limit:
        raise SystemExit(f"{url}: larger than {limit} bytes")
    if dest:
        with open(dest, "wb") as f:
            f.write(data)
    return data


def latest(repo):
    return json.loads(get(f"https://api.github.com/repos/{repo}/releases/latest"))


def asset(rel, pattern):
    for a in rel["assets"]:
        if re.fullmatch(pattern, a["name"]):
            return a
    raise SystemExit(f"{rel['html_url']}: no asset matching {pattern}")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def install(src, bindir, name):
    dst = os.path.join(bindir, name)
    shutil.copyfile(src, dst)
    os.chmod(dst, 0o755)
    return dst


def fresh(base, name):
    d = os.path.join(base, "dl", name)
    if os.path.isdir(d):
        shutil.rmtree(d)
    os.makedirs(d)
    return d


def runc(base, bindir):
    rel = latest("opencontainers/runc")
    d = fresh(base, "runc")
    a = asset(rel, r"runc\.amd64")
    path = os.path.join(d, a["name"])
    get(a["browser_download_url"], path)
    sums = get(asset(rel, r"runc\.sha256sum")["browser_download_url"]).decode()
    want = next((l.split()[0] for l in sums.splitlines() if l.strip().endswith("runc.amd64")), None)
    got = sha256(path)
    if want != got:
        raise SystemExit(f"runc: checksum mismatch ({got} != {want})")
    install(path, bindir, "runc")
    return {"tag": rel["tag_name"], "url": a["browser_download_url"], "sha256": got, "verified": True}


def crun(base, bindir):
    rel = latest("containers/crun")
    d = fresh(base, "crun")
    a = asset(rel, r"crun-[0-9.]+-linux-amd64")
    path = os.path.join(d, a["name"])
    get(a["browser_download_url"], path)
    install(path, bindir, "crun")
    return {"tag": rel["tag_name"], "url": a["browser_download_url"], "sha256": sha256(path), "verified": False,
            "note": "static release binary; the release publishes signatures, not a SHA-256 list"}


def youki(base, bindir):
    rel = latest("youki-dev/youki")
    d = fresh(base, "youki")
    a = asset(rel, r"youki-[0-9.]+-x86_64-(musl|gnu)\.tar\.gz")
    path = os.path.join(d, a["name"])
    get(a["browser_download_url"], path)
    with tarfile.open(path) as tar:
        member = next((m for m in tar.getmembers() if m.isfile() and os.path.basename(m.name) == "youki"), None)
        if member is None:
            raise SystemExit("youki: no youki binary in the archive")
        out = os.path.join(d, "youki.bin")
        with tar.extractfile(member) as src, open(out, "wb") as dst:
            shutil.copyfileobj(src, dst)
    install(out, bindir, "youki")
    return {"tag": rel["tag_name"], "url": a["browser_download_url"], "sha256": sha256(out), "verified": False,
            "note": "binary taken from the release tarball"}


def runsc(base, bindir):
    url = "https://storage.googleapis.com/gvisor/releases/release/latest/x86_64/runsc"
    d = fresh(base, "runsc")
    path = os.path.join(d, "runsc")
    get(url, path)
    want = get(url + ".sha512").decode().split()[0]
    h = hashlib.sha512()
    with open(path, "rb") as f:
        h.update(f.read())
    if h.hexdigest() != want:
        raise SystemExit("runsc: sha512 mismatch")
    install(path, bindir, "runsc")
    return {"tag": "release/latest", "url": url, "sha256": sha256(path), "verified": True, "note": "sha512 verified"}


def main():
    base = os.path.realpath(sys.argv[1])
    bindir = os.path.join(base, "bin")
    os.makedirs(bindir, exist_ok=True)
    out = {}
    for name, fn in (("runc", runc), ("crun", crun), ("youki", youki), ("runsc", runsc)):
        try:
            out[name] = fn(base, bindir)
        except (SystemExit, OSError, ValueError) as e:
            out[name] = {"error": str(e)}
    json.dump(out, sys.stdout, indent=1)
    print()


if __name__ == "__main__":
    main()
