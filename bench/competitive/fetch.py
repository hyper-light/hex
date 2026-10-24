#!/usr/bin/env python3
"""bench/competitive: fetches the competitors' release binaries and prints their versions.

  fetch.py <base dir> [name ...]   binaries land in <base>/bin, the guest kernel in <base>/vm; each
                                   download in its own new <base>/dl/<name>/

By default the OCI runtimes (runc, crun, youki, runsc), each its project's latest release, resolved now
and recorded (tag, URL, SHA-256, and whether an upstream checksum verified it), so a result names exactly
what it measured. Named, the others: `krun` (crun's libkrun handler, from Fedora's signed packages) for
the OCI pass, and each at a pinned release, `codex` for the process-sandbox pass, `firecracker`,
`cloud-hypervisor` and `guest-kernel` for the microVM pass, `containerd`, `nerdctl` and `kata` for the
containerd lab, and `zeroboot-firecracker` and `zeroboot-kernel` (into <base>/zeroboot) for Zeroboot's. Nothing is executed from a download directory; an archive is never extracted wholesale
(only the members named, or for kata only its opt/kata/ tree, are copied).
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
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
    """gVisor's releases ship as gvisor.tar.bz2 (with a .sha512) since 2026; runsc is taken from it."""
    url = "https://storage.googleapis.com/gvisor/releases/release/latest/x86_64/gvisor.tar.bz2"
    d = fresh(base, "runsc")
    path = os.path.join(d, "gvisor.tar.bz2")
    get(url, path)
    want = get(url + ".sha512").decode().split()[0]
    h = hashlib.sha512()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    if h.hexdigest() != want:
        raise SystemExit("runsc: gvisor.tar.bz2 sha512 mismatch")
    # runsc runs its Sentry from gvisor-bin/ beside itself (gvisor_sentry and helpers), so the set is
    # installed together; only regular files named runsc or directly under gvisor-bin/ are taken.
    taken = []
    with tarfile.open(path) as tar:
        for m in tar.getmembers():
            parts = m.name.split("/")
            if not m.isfile() or ".." in parts or m.name.startswith("/"):
                continue
            if parts == ["runsc"] or (len(parts) == 2 and parts[0] == "gvisor-bin"):
                dst = os.path.join(bindir, *parts)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                with tar.extractfile(m) as src, open(dst, "wb") as out_f:
                    shutil.copyfileobj(src, out_f)
                os.chmod(dst, 0o755)
                taken.append(m.name)
    if "runsc" not in taken or "gvisor-bin/gvisor_sentry" not in taken:
        raise SystemExit(f"runsc: gvisor.tar.bz2 lacks runsc or gvisor-bin/gvisor_sentry (found {taken})")
    out = os.path.join(bindir, "runsc")
    return {"tag": "release/latest", "url": url, "sha256": sha256(out), "verified": True,
            "note": "gvisor.tar.bz2 sha512-verified; runsc and gvisor-bin/ installed from it: " + ", ".join(sorted(taken))}


# codex is pinned rather than latest: the process-sandbox pass measures one release, named here.
CODEX_TAG = "rust-v0.164.0"
CODEX_ASSET = "codex-x86_64-unknown-linux-musl.tar.gz"


def codex(base, bindir):
    """codex's static musl build at CODEX_TAG, checked against the sha256 GitHub's release API publishes
    for the asset (a release with no digest is refused, never installed unchecked)."""
    rel = json.loads(get(f"https://api.github.com/repos/openai/codex/releases/tags/{CODEX_TAG}"))
    a = asset(rel, re.escape(CODEX_ASSET))
    digest = (a.get("digest") or "").removeprefix("sha256:")
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise SystemExit(f"codex: {CODEX_ASSET} has no sha256 digest in the release API, so it can't be checked")
    d = fresh(base, "codex")
    tgz = os.path.join(d, CODEX_ASSET)
    get(a["browser_download_url"], tgz)
    if sha256(tgz) != digest:
        raise SystemExit(f"codex: {CODEX_ASSET} doesn't match its published sha256")
    member = CODEX_ASSET.removesuffix(".tar.gz")
    with tarfile.open(tgz) as t:
        m = t.getmember(member)
        if not m.isfile():
            raise SystemExit(f"codex: {member} in the tarball isn't a regular file")
        with t.extractfile(m) as src, open(os.path.join(d, "codex"), "wb") as dst:
            shutil.copyfileobj(src, dst)
    out = install(os.path.join(d, "codex"), bindir, "codex")
    return {"tag": CODEX_TAG, "url": a["browser_download_url"], "sha256": sha256(out), "verified": True,
            "note": f"{CODEX_ASSET} checked against the release API's sha256; {member} installed from it"}


# The microVM pass's monitors, pinned: each one's latest release on 2026-10-23.
FIRECRACKER_TAG = "v1.17.0"
CLOUD_HYPERVISOR_TAG = "v54.0"
# The guest kernel Firecracker's getting-started guide (at FIRECRACKER_TAG) boots: the newest vmlinux in its
# latest CI artifact set, here that set on 2026-10-23. It has CONFIG_PVH, so Cloud Hypervisor boots it too.
GUEST_KERNEL = "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/20261023-dd7c24b0d165-0/x86_64/vmlinux-6.18.54"


def firecracker(base, bindir):
    return fetch_firecracker(base, FIRECRACKER_TAG, bindir)


def fetch_firecracker(base, tag, dest):
    """Firecracker's release tarball at `tag`, checked against the release's own .sha256.txt; only its
    firecracker binary is installed, into `dest`."""
    rel = json.loads(get(f"https://api.github.com/repos/firecracker-microvm/firecracker/releases/tags/{tag}"))
    name = f"firecracker-{tag}-x86_64.tgz"
    a = asset(rel, re.escape(name))
    d = fresh(base, f"firecracker-{tag}")
    tgz = os.path.join(d, name)
    get(a["browser_download_url"], tgz)
    want = get(asset(rel, re.escape(name + ".sha256.txt"))["browser_download_url"]).decode().split()[0]
    if sha256(tgz) != want:
        raise SystemExit(f"firecracker: {name} doesn't match its .sha256.txt")
    binary = f"firecracker-{tag}-x86_64"
    with tarfile.open(tgz) as tar:
        m = next((m for m in tar.getmembers() if m.isfile() and os.path.basename(m.name) == binary
                  and ".." not in m.name.split("/") and not m.name.startswith("/")), None)
        if m is None:
            raise SystemExit(f"firecracker: no {binary} in {name}")
        with tar.extractfile(m) as src, open(os.path.join(d, "firecracker"), "wb") as dst:
            shutil.copyfileobj(src, dst)
    os.makedirs(dest, exist_ok=True)
    out = install(os.path.join(d, "firecracker"), dest, "firecracker")
    return {"tag": tag, "url": a["browser_download_url"], "sha256": sha256(out), "verified": True,
            "note": f"{name} checked against its .sha256.txt; {m.name} installed from it"}


def cloud_hypervisor(base, bindir):
    """Cloud Hypervisor's static release binary at CLOUD_HYPERVISOR_TAG, checked against the sha256 GitHub's
    release API publishes for it."""
    rel = json.loads(get(f"https://api.github.com/repos/cloud-hypervisor/cloud-hypervisor/releases/tags/{CLOUD_HYPERVISOR_TAG}"))
    a = asset(rel, "cloud-hypervisor-static")
    digest = (a.get("digest") or "").removeprefix("sha256:")
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise SystemExit("cloud-hypervisor: no sha256 digest in the release API, so it can't be checked")
    d = fresh(base, "cloud-hypervisor")
    path = os.path.join(d, a["name"])
    get(a["browser_download_url"], path)
    if sha256(path) != digest:
        raise SystemExit("cloud-hypervisor: cloud-hypervisor-static doesn't match its published sha256")
    install(path, bindir, "cloud-hypervisor")
    return {"tag": CLOUD_HYPERVISOR_TAG, "url": a["browser_download_url"], "sha256": digest, "verified": True,
            "note": "the static binary, checked against the release API's sha256"}


def guest_kernel(base, _bindir):
    return fetch_kernel(base, GUEST_KERNEL, os.path.join(base, "vm"))


def fetch_kernel(base, url, dest):
    """A vmlinux from Firecracker's CI artifacts into <dest>/vmlinux. Firecracker's CI publishes no checksum
    for it, so its sha256 is recorded rather than checked."""
    d = fresh(base, "kernel-" + re.sub(r"[^A-Za-z0-9.-]", "_", url.split("firecracker-ci/", 1)[-1]))
    path = os.path.join(d, "vmlinux")
    get(url, path)
    with open(path, "rb") as f:
        if f.read(4) != b"\x7fELF":
            raise SystemExit(f"{url}: not an ELF vmlinux")
    os.makedirs(dest, exist_ok=True)
    shutil.copyfile(path, os.path.join(dest, "vmlinux"))
    return {"tag": os.path.basename(url), "url": url, "sha256": sha256(path), "verified": False,
            "note": "Firecracker's CI artifact; no published checksum, so its sha256 is recorded"}


# Zeroboot's snapshot reader holds Firecracker v1.12.0's vmstate offsets (src/vmm/vmstate.rs), so its
# templates are made with that release, and the kernel that release's CI booted (the getting-started
# guide's choice at v1.12: the newest vmlinux under firecracker-ci/v1.12).
ZEROBOOT_FIRECRACKER_TAG = "v1.12.0"
ZEROBOOT_KERNEL = "https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/v1.12/x86_64/vmlinux-6.1.128"


def zeroboot_firecracker(base, _bindir):
    return fetch_firecracker(base, ZEROBOOT_FIRECRACKER_TAG, os.path.join(base, "zeroboot"))


def zeroboot_kernel(base, _bindir):
    return fetch_kernel(base, ZEROBOOT_KERNEL, os.path.join(base, "zeroboot"))


# crun's static release has no libkrun, so krun is Fedora's crun, which has the handler built in, with the
# libraries it loads (libkrun, and libkrunfw, the guest kernel it carries).
KRUN_PACKAGES = ("crun", "libkrun", "libkrunfw")
SONAME = re.compile(r"[^/]+\.so(\.[0-9]+)*")


def krun(base, bindir):
    """Each of KRUN_PACKAGES at its latest build in the host's Fedora repositories, with whatever packages of
    theirs the host lacks (`dnf download --resolve`), each checked by `rpm -K` against the host's Fedora keys.
    crun goes to <bindir>/krun, the name crun picks its libkrun handler by; the packages' shared libraries
    (files and their soname links) to <base>/krun/lib, for LD_LIBRARY_PATH."""
    d = fresh(base, "krun")
    dl = subprocess.run(["dnf", "download", "--resolve", "--destdir", d, *KRUN_PACKAGES],
                        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, timeout=600)
    if dl.returncode != 0:
        raise SystemExit(f"krun: dnf download exited {dl.returncode}: {dl.stderr.strip()[-500:]}")
    lib = os.path.join(base, "krun", "lib")
    if os.path.isdir(lib):
        shutil.rmtree(lib)
    os.makedirs(lib)
    packages, taken, crun_out = [], [], None
    for name in sorted(n for n in os.listdir(d) if n.endswith(".rpm")):
        path = os.path.join(d, name)
        k = subprocess.run(["rpm", "-K", path], capture_output=True, text=True, timeout=60)
        if k.returncode != 0 or not k.stdout.rstrip().endswith("signatures OK"):
            raise SystemExit(f"krun: {name} fails rpm -K: {k.stdout.strip()} {k.stderr.strip()}")
        packages.append(subprocess.run(["rpm", "-qp", "--qf", "%{NEVRA}", path], capture_output=True, text=True,
                                       check=True, timeout=60).stdout)
        tar = path + ".tar"
        with open(tar, "wb") as out:  # rpm2archive writes the archive to a stdout that isn't a terminal
            subprocess.run(["rpm2archive", "-n", path], stdout=out, check=True, timeout=120)
        with tarfile.open(tar) as t:
            for m in t.getmembers():
                member = os.path.normpath(m.name)
                head, leaf = os.path.split(member)
                if member == "usr/bin/crun" and m.isfile() and name.startswith("crun-"):
                    with t.extractfile(m) as src, open(os.path.join(d, "krun"), "wb") as dst:
                        shutil.copyfileobj(src, dst)
                    crun_out = install(os.path.join(d, "krun"), bindir, "krun")
                elif head == "usr/lib64" and SONAME.fullmatch(leaf):
                    if m.isfile():
                        with t.extractfile(m) as src, open(os.path.join(lib, leaf), "wb") as dst:
                            shutil.copyfileobj(src, dst)
                    elif m.issym() and "/" not in m.linkname:
                        os.symlink(m.linkname, os.path.join(lib, leaf))
                    else:
                        continue
                    taken.append(member)
    if crun_out is None or not any(x.startswith("usr/lib64/libkrun.so") for x in taken):
        raise SystemExit(f"krun: the packages lack crun or libkrun (took {taken})")
    return {"tag": " ".join(packages), "url": "dnf download --resolve " + " ".join(KRUN_PACKAGES),
            "sha256": sha256(crun_out), "verified": True,
            "note": "Fedora packages checked by rpm -K; crun installed as krun, and into krun/lib: " + ", ".join(taken)}


# The containerd lab, pinned: each project's latest release on 2026-10-23.
CONTAINERD_TAG = "v2.4.2"
NERDCTL_TAG = "v2.4.1"
KATA_TAG = "4.3.1"


def take(tar, members, d, bindir, what):
    """Copies each named regular-file member of tar into bindir under its basename."""
    out = {}
    for name in members:
        m = next((m for m in tar.getmembers() if os.path.normpath(m.name) == name), None)
        if m is None or not m.isfile():
            raise SystemExit(f"{what}: no regular file {name} in the archive")
        with tar.extractfile(m) as src, open(os.path.join(d, os.path.basename(name)), "wb") as dst:
            shutil.copyfileobj(src, dst)
        out[name] = sha256(install(os.path.join(d, os.path.basename(name)), bindir, os.path.basename(name)))
    return out


def containerd(base, bindir):
    """containerd's static release at CONTAINERD_TAG, checked against its .sha256sum; containerd and its runc
    shim installed (the shim runs the runc in <bindir>, the OCI table's)."""
    rel = json.loads(get(f"https://api.github.com/repos/containerd/containerd/releases/tags/{CONTAINERD_TAG}"))
    name = f"containerd-static-{CONTAINERD_TAG.removeprefix('v')}-linux-amd64.tar.gz"
    a = asset(rel, re.escape(name))
    d = fresh(base, "containerd")
    tgz = os.path.join(d, name)
    get(a["browser_download_url"], tgz)
    want = get(asset(rel, re.escape(name + ".sha256sum"))["browser_download_url"]).decode().split()[0]
    if sha256(tgz) != want:
        raise SystemExit(f"containerd: {name} doesn't match its .sha256sum")
    with tarfile.open(tgz) as tar:
        sums = take(tar, ["bin/containerd", "bin/containerd-shim-runc-v2"], d, bindir, "containerd")
    return {"tag": CONTAINERD_TAG, "url": a["browser_download_url"], "sha256": sums["bin/containerd"], "verified": True,
            "note": f"{name} checked against its .sha256sum; installed: " + json.dumps(sums)}


def nerdctl(base, bindir):
    """nerdctl's release at NERDCTL_TAG, checked against the release's SHA256SUMS; only nerdctl installed."""
    rel = json.loads(get(f"https://api.github.com/repos/containerd/nerdctl/releases/tags/{NERDCTL_TAG}"))
    name = f"nerdctl-{NERDCTL_TAG.removeprefix('v')}-linux-amd64.tar.gz"
    a = asset(rel, re.escape(name))
    d = fresh(base, "nerdctl")
    tgz = os.path.join(d, name)
    get(a["browser_download_url"], tgz)
    sums = get(asset(rel, "SHA256SUMS")["browser_download_url"]).decode()
    want = next((l.split()[0] for l in sums.splitlines() if l.split()[-1:] == [name]), None)
    if sha256(tgz) != want:
        raise SystemExit(f"nerdctl: {name} doesn't match SHA256SUMS")
    with tarfile.open(tgz) as tar:
        got = take(tar, ["nerdctl"], d, bindir, "nerdctl")
    return {"tag": NERDCTL_TAG, "url": a["browser_download_url"], "sha256": got["nerdctl"], "verified": True,
            "note": f"{name} checked against the release's SHA256SUMS; nerdctl installed from it"}


def kata(base, _bindir):
    """Kata's static release at KATA_TAG (its shims, hypervisors, guest kernels and images), checked against
    the sha256 GitHub's release API publishes for it; its opt/kata/ tree into <base>/kata/opt/kata, which the
    lab mounts at /opt/kata, the prefix its configurations name. Needs a Python whose tarfile reads zstd (3.14)."""
    rel = json.loads(get(f"https://api.github.com/repos/kata-containers/kata-containers/releases/tags/{KATA_TAG}"))
    name = f"kata-static-{KATA_TAG}-amd64.tar.zst"
    a = asset(rel, re.escape(name))
    digest = (a.get("digest") or "").removeprefix("sha256:")
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise SystemExit(f"kata: {name} has no sha256 digest in the release API, so it can't be checked")
    d = fresh(base, "kata")
    path = os.path.join(d, name)
    get(a["browser_download_url"], path, limit=2 * 2**30)
    if sha256(path) != digest:
        raise SystemExit(f"kata: {name} doesn't match its published sha256")
    root = os.path.join(base, "kata")
    if os.path.isdir(root):
        shutil.rmtree(root)
    count = 0
    # One pass over the stream: a seek back in a zstd stream decompresses it again from the start.
    with tarfile.open(path, "r|zst") as tar:
        for m in tar:
            member = os.path.normpath(m.name)
            if not member.startswith("opt/kata/") or m.name.startswith("/") or ".." in m.name.split("/"):
                continue
            if m.isdev() or m.isfifo():
                continue
            if m.issym() and m.linkname.startswith("/") and not m.linkname.startswith("/opt/kata/"):
                continue
            # The data filter keeps every file and link inside root; a symlink into /opt/kata/ resolves in the lab,
            # where the tree is mounted there, so it passes the tar filter, and a file written through one is still
            # refused by the data filter if it would land outside root.
            tar.extract(m, root, filter="tar" if m.issym() else "data")
            count += 1
    return {"tag": KATA_TAG, "url": a["browser_download_url"], "sha256": digest, "verified": True,
            "note": f"{name} checked against the release API's sha256; {count} members of opt/kata/ extracted"}


def main():
    base = os.path.realpath(sys.argv[1])
    bindir = os.path.join(base, "bin")
    os.makedirs(bindir, exist_ok=True)
    out = {}
    fetchers = {"runc": runc, "crun": crun, "youki": youki, "runsc": runsc, "krun": krun, "codex": codex,
                "firecracker": firecracker, "cloud-hypervisor": cloud_hypervisor, "guest-kernel": guest_kernel,
                "containerd": containerd, "nerdctl": nerdctl, "kata": kata,
                "zeroboot-firecracker": zeroboot_firecracker, "zeroboot-kernel": zeroboot_kernel}
    # The OCI runtimes by default; name others (`fetch.py <base> codex`) to fetch only those.
    for name in sys.argv[2:] or ["runc", "crun", "youki", "runsc"]:
        try:
            out[name] = fetchers[name](base, bindir)
        except (SystemExit, OSError, ValueError, KeyError, subprocess.SubprocessError) as e:
            out[name] = {"error": str(e)}
    json.dump(out, sys.stdout, indent=1)
    print()


if __name__ == "__main__":
    main()
