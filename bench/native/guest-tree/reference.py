"""HEX-GUEST-21's reference (docs/design/shards-guest.md §4a): each image as containerd unpacks it on Linux.

For each image, through the host's containerd (`ctr`, in a namespace of its own, `hexg21`), as root in a
privileged container that shares the host's containerd socket and state (fedora's root lab, through the
docker group):
- pull it by digest for linux/amd64 (the lab image: import it, written by lab_image.py);
- save its layers' own blobs, byte-exact from the content store (`ctr content get`), in order, as
  `<out>/<name>/layer<i>.tar.gz` (what Hex stages), and the manifest naming them;
- mount its snapshot read-only (`ctr snapshots view` at its chain id, then `ctr snapshots mounts`: an
  overlay of the lower layers alone; ctr 2.3.6's `images mount` gave an empty tmpfs here), walk it with `hex selftest-box tree`, and unmount;
- record ctr's and containerd's versions, the snapshotter, the kernel, and each image's digests.

Usage: python3 -I reference.py [--here] HEX OUT IMAGE...   (--here: containerd runs in this container, as
reference_vm.sh runs it, the hex walker at /hex)   (IMAGE: name@sha256:..., or `lab:tree`, `lab:file`,
`lab:hardlink` for the lab images)
"""

import hashlib
import json
import os
import subprocess
import sys

NS = "hexg21"
# The container the root lab runs ctr in: fedora:44 by digest (research §9's census), whose glibc runs
# the host's ctr and hex.
BOX = "registry.fedoraproject.org/fedora:44@sha256:b3e011d47e686a89781495c9a87bc80349af742965e5d51e79cd51711982e4a2"


# Where containerd runs: beside fedora's own (through `docker run`, by default), or HERE, in the container
# this runs in, which runs its own containerd (reference_vm.sh, in a Mac Studio's Docker VM).
HERE = False


def root(hex_bin, out, script):
    """Runs `script` (sh) as root beside containerd; returns its stdout."""
    if HERE:
        done = subprocess.run(["sh", "-euc", script], capture_output=True)
        if done.returncode != 0:
            raise SystemExit(f"root lab step failed ({done.returncode}): {script}\n{done.stderr.decode()}")
        return done.stdout
    # Each container's id, recorded as it starts (the lead's rule: every container recorded, and removed when
    # its run ends; --rm removes it).
    cid = os.path.join(out, ".cid")
    if os.path.exists(cid):
        os.remove(cid)
    cmd = [
        "docker", "run", "--rm", "--cidfile", cid, "--privileged", "--network", "host",
        # The lead's shared fedora limits for a container (hex-slot, the T7 fedora-unit-limits rule):
        # an operator's limit, not tuning.
        "--memory", "8g", "--oom-score-adj", "1000",
        "-v", "/run/containerd:/run/containerd",
        "-v", "/var/lib/containerd:/var/lib/containerd",
        "-v", "/usr/bin/ctr:/usr/bin/ctr:ro",
        "-v", f"{hex_bin}:/hex:ro",
        "-v", f"{out}:{out}",
        BOX, "sh", "-euc", script,
    ]
    done = subprocess.run(cmd, capture_output=True)
    if os.path.exists(cid):
        with open(cid) as f, open(os.path.join(out, "CONTAINERS.txt"), "a") as log:
            log.write(f.read().strip() + "\n")
        os.remove(cid)
    if done.returncode != 0:
        raise SystemExit(f"root lab step failed ({done.returncode}): {script}\n{done.stderr.decode()}")
    return done.stdout


def ctr(hex_bin, out, *args):
    quoted = " ".join("'" + a.replace("'", "'\\''") + "'" for a in args)
    return root(hex_bin, out, f"ctr -n {NS} {quoted}")


def content(hex_bin, out, digest):
    return ctr(hex_bin, out, "content", "get", digest)


EXT = {
    "application/vnd.oci.image.layer.v1.tar+gzip": ".tar.gz",
    "application/vnd.docker.image.rootfs.diff.tar.gzip": ".tar.gz",
    "application/vnd.oci.image.layer.v1.tar": ".tar",
}


def one(hex_bin, out, image, record):
    lab = image.startswith("lab")
    variant = image.split(":", 1)[1] if ":" in image and lab else "tree"
    name = f"lab-{variant}" if lab else image.split("@")[0].replace("/", "_").replace(":", "_")
    here = os.path.join(out, name)
    os.makedirs(here, exist_ok=True)
    if lab:
        lab_tar = os.path.join(here, "lab-oci.tar")
        built = subprocess.run(
            [sys.executable, "-I", os.path.join(os.path.dirname(os.path.abspath(__file__)), "lab_image.py"), variant, lab_tar],
            check=True, capture_output=True, text=True,
        ).stdout
        record.append(f"lab image built: {built.strip()}")
        ref = f"docker.io/hexlab/{variant}:1"
        try:
            ctr(hex_bin, out, "images", "import", "--platform", "linux/amd64", lab_tar)
        except SystemExit as e:
            record.append(f"{name}: ctr refused its import and unpack: {str(e).splitlines()[-1]}")
            return
        made = f"imported from {os.path.basename(lab_tar)} (ctr images import --platform linux/amd64)"
    else:
        ref = image
        ctr(hex_bin, out, "images", "pull", "--platform", "linux/amd64", ref)
        made = "pulled by digest (ctr images pull --platform linux/amd64)"
    listed = ctr(hex_bin, out, "images", "ls").decode().splitlines()
    top = next(l.split()[2] for l in listed[1:] if l.split()[0] == ref)
    doc = json.loads(content(hex_bin, out, top))
    manifest_digest = top
    if "manifests" in doc:
        manifest_digest = next(
            m["digest"] for m in doc["manifests"]
            if m.get("platform", {}).get("os") == "linux"
            and m.get("platform", {}).get("architecture") == "amd64"
            and not m.get("platform", {}).get("variant")
        )
        doc = json.loads(content(hex_bin, out, manifest_digest))
    layers = []
    for i, layer in enumerate(doc["layers"]):
        ext = EXT.get(layer["mediaType"])
        if ext is None:
            raise SystemExit(f"{ref}: layer {i} is {layer['mediaType']}, which staging doesn't read")
        path = os.path.join(here, f"layer{i}{ext}")
        with open(path, "wb") as f:
            f.write(content(hex_bin, out, layer["digest"]))
        layers.append(os.path.basename(path))
        record.append(f"{name} layer{i} {layer['digest']} {layer['mediaType']}")
    with open(os.path.join(here, "layers.txt"), "w") as f:
        f.write("\n".join(layers) + "\n")
    # The snapshot's own view, read-only (its lower layers alone): the chain id names the image's top
    # snapshot (image-spec config.md, "Layer ChainID").
    config = json.loads(content(hex_bin, out, doc["config"]["digest"]))
    chain = None
    for diff in config["rootfs"]["diff_ids"]:
        chain = diff if chain is None else "sha256:" + hashlib.sha256(f"{chain} {diff}".encode()).hexdigest()
    tree = os.path.join(here, "reference.tree")
    printed_at = os.path.join(here, "mounts.ctr")
    view = f"hexg21-view-{name}"
    # One shell: a view ctr makes is held by a lease that ends with the ctr process, and containerd's
    # collector removes it once no step holds it. The per-layer form (`per layer` in RECORD) is checked
    # against ctr's printed mount on the lab image, whose options fit a page.
    check = name == "lab-tree"
    root(hex_bin, out, (
        f"ctr -n {NS} snapshots rm {view} >/dev/null 2>&1 || true;"
        f" ctr -n {NS} snapshots view {view} {chain} >/dev/null;"
        f" m=$(ctr -n {NS} snapshots mounts /mnt/r {view}); printf '%s\\n' \"$m\" > '{printed_at}';"
        " case \"$m\" in 'mount -t overlay '*)"
        "   l=$(printf '%s' \"$m\" | sed -E 's/.*lowerdir=([^, ]*).*/\\1/');"
        "   o=$(printf '%s' \"$m\" | sed -E 's/.* -o ([^ ]*).*/\\1/' | tr ',' '\\n' | grep -v '^lowerdir=' | paste -sd,);"
        "   p=\"mount -t overlay overlay /mnt/r -o $(printf '%s' \"$l\" | tr ':' '\\n' | sed 's/^/lowerdir+=/' | paste -sd,),$o\";;"
        "  *) p=\"$m\";; esac;"
        f" printf '%s\\n' \"$p\" >> '{printed_at}';"
        f" mkdir -p /mnt/r && eval \"$p\" && /hex selftest-box tree /mnt/r > '{tree}'; s=$?; umount /mnt/r;"
        + (f" if [ $s = 0 ]; then eval \"$m\" && /hex selftest-box tree /mnt/r > '{tree}.ctr-options'; s=$?;"
           " umount /mnt/r; fi;" if check else "")
        # The collector may have removed the view already, its lease ended with the ctr that made it (fedora,
        # grafana's long walk): the mount held the committed layers below it, which the image holds.
        + f" r=$(ctr -n {NS} snapshots rm {view} 2>&1) || case \"$r\" in *'not found'*) ;;"
        " *) printf '%s\\n' \"$r\" >&2; exit 1;; esac; exit $s"
    ))
    printed, mounted = open(printed_at).read().splitlines()[:2]
    record.append(f"{name} view mounts (ctr's): {printed}")
    if mounted != printed:
        record.append(f"{name} mounted per layer: {mounted}")
    if check:
        equal = open(tree, "rb").read() == open(tree + ".ctr-options", "rb").read()
        record.append(f"{name}: ctr's printed mount and the per-layer mount walk identical: {equal}")
        if not equal:
            raise SystemExit(f"{name}: the per-layer mount's walk differs from ctr's")
    n = sum(1 for _ in open(tree))
    record.append(
        f"{name}: {ref} index/top {top} manifest {manifest_digest}; {made}; snapshot mounted read-only"
        f" (ctr snapshots view at the chain id + ctr snapshots mounts, the view's lowerdirs alone, each its own"
        f" lowerdir+= where ctr printed an overlay), walked as root: {n} entries"
    )


def main(hex_bin, out, images):
    out = os.path.abspath(out)
    os.makedirs(out, exist_ok=True)
    record = []
    record.append("ctr: " + ctr(hex_bin, out, "version").decode().replace("\n", " | "))
    record.append("snapshotter: " + root(hex_bin, out, "ctr plugins ls | grep -E 'snapshotter.*overlayfs'").decode().strip())
    record.append("kernel: " + os.uname().release)
    # Every image of this namespace removed first, its snapshots with it (--sync waits for the collector), so
    # each image is unpacked now, after the SINCE the Hex side is given: a snapshot kept from an earlier run,
    # or one a later image shares (alpine's base under caddy), would carry that run's mkparent times.
    root(hex_bin, out, f"ctr -n {NS} images ls -q | xargs -r ctr -n {NS} images rm --sync >/dev/null")
    # ctr 2.3.6's `snapshots ls` has no -q: its table, less its header.
    left = root(hex_bin, out, f"ctr -n {NS} snapshots ls | tail -n +2 | wc -l").decode().strip()
    record.append(f"fresh: every {NS} image removed (images rm --sync); snapshots left: {left}")
    for image in images:
        one(hex_bin, out, image, record)
    with open(os.path.join(out, "RECORD.txt"), "w") as f:
        f.write("\n".join(record) + "\n")
    print("\n".join(record))


if __name__ == "__main__":
    if sys.argv[1:2] == ["--here"]:
        HERE = True
        del sys.argv[1]
    if len(sys.argv) < 4:
        sys.exit("usage: reference.py HEX OUT IMAGE...")
    main(os.path.abspath(sys.argv[1]), sys.argv[2], sys.argv[3:])
