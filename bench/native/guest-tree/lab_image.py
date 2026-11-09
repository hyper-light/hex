"""Writes HEX-GUEST-21's lab image (docs/design/shards-guest.md §4a) as an OCI image layout tarball, for
`ctr images import`: two layers written by hand with Python's tarfile, so each holds exactly the cases the
comparison needs, which no build tool emits on demand (a missing parent, a write through a link):

layer 1: directories, files with owners, a setuid file, a `user.` xattr (PAX SCHILY), a hard link, links
  absolute and relative (`bin` -> `usr/bin`, `lib` -> `/usr/lib`).
layer 2: a whiteout (`etc/.wh.a`), an opaque directory (`d/.wh..wh..opq`), a file replaced by a directory
  (`f`), a directory replaced by a link (`r`), a directory replaced by a file (`g`), a file under layer 1's
  link (`bin/tool`), a file whose parents no layer made (`usr/local/bin/x`), and files through this layer's
  own links (`a` -> `/etc` then `a/x`, `c` -> `../../..` then `c/x`).

Usage: python3 -I lab_image.py tree|file|hardlink OUT.tar  (prints the builder, each layer's digest and diff id)
"""

import gzip
import hashlib
import io
import json
import sys
import tarfile

T1 = 1_700_000_000
T2 = 1_700_100_000


def entry(kind, name, data=b"", mode=0o644, uid=0, gid=0, mtime=T1, link="", xattrs=None):
    info = tarfile.TarInfo(name)
    info.type = kind
    info.mode = mode
    info.uid, info.gid = uid, gid
    info.mtime = mtime
    info.linkname = link
    info.size = len(data) if kind == tarfile.REGTYPE else 0
    if xattrs:
        info.pax_headers = {f"SCHILY.xattr.{k}": v for k, v in xattrs.items()}
    return info, data


D, F, L, H = tarfile.DIRTYPE, tarfile.REGTYPE, tarfile.SYMTYPE, tarfile.LNKTYPE

LAYERS = [
    [
        entry(D, "etc", mode=0o755),
        entry(F, "etc/a", b"a\n"),
        entry(F, "etc/b", b"b\n", xattrs={"user.lab": "1"}),
        entry(D, "usr", mode=0o755),
        entry(D, "usr/bin", mode=0o755),
        entry(F, "usr/bin/perl5", b"perl\n", mode=0o755),
        entry(H, "usr/bin/perl", link="usr/bin/perl5", mode=0o755),
        entry(F, "usr/bin/su", b"su\n", mode=0o4755),
        entry(D, "usr/lib", mode=0o755),
        entry(L, "bin", link="usr/bin", mode=0o777),
        entry(L, "lib", link="/usr/lib", mode=0o777),
        entry(D, "home", mode=0o700, uid=1000, gid=1000),
        entry(F, "home/.profile", b"profile\n", uid=1000, gid=1000),
        entry(D, "d", mode=0o755),
        entry(F, "d/old", b"old\n"),
        entry(D, "d/sub", mode=0o755),
        entry(F, "d/sub/s", b"s\n"),
        entry(F, "f", b"file\n"),
        entry(D, "g", mode=0o755),
        entry(F, "g/inner", b"inner\n"),
        entry(D, "r", mode=0o755),
        entry(F, "r/x", b"x\n"),
    ],
    [
        entry(D, "etc", mode=0o755, mtime=T2),
        entry(F, "etc/.wh.a", mtime=T2),
        entry(D, "d", mode=0o750, mtime=T2),
        entry(F, "d/.wh..wh..opq", mtime=T2),
        entry(F, "d/new", b"new\n", mtime=T2),
        entry(D, "f", mode=0o755, mtime=T2),
        entry(F, "f/inside", b"inside\n", mtime=T2),
        entry(L, "r", link="etc", mode=0o777, mtime=T2),
        entry(F, "g", b"now a file\n", mtime=T2),
        entry(F, "bin/tool", b"tool\n", mode=0o755, mtime=T2),
        entry(F, "usr/local/bin/x", b"x\n", mode=0o755, mtime=T2),
        entry(L, "a", link="/etc", mode=0o777, mtime=T2),
        entry(F, "a/x", b"through a\n", mtime=T2),
        entry(L, "c", link="../../..", mode=0o777, mtime=T2),
        entry(F, "c/x", b"through c\n", mtime=T2),
    ],
]

# Two more images, each one case alone, since a case containerd fails fails its whole unpack:
# - `file`: layer 2 writes `p/inner` where layer 1 made `p` a regular file;
# - `hardlink`: layer 2 makes `h` a hard link to layer 1's file `q`.
VARIANTS = {
    "tree": LAYERS,
    "file": [
        [entry(F, "p", b"p\n"), entry(D, "etc", mode=0o755)],
        [entry(F, "p/inner", b"inner\n", mtime=T2)],
    ],
    "hardlink": [
        [entry(F, "q", b"q\n")],
        [entry(H, "h", link="q", mtime=T2)],
    ],
}


def blob(blobs, data):
    digest = "sha256:" + hashlib.sha256(data).hexdigest()
    blobs[digest] = data
    return digest


def main(variant, out):
    blobs = {}
    layers, diff_ids = [], []
    for entries in VARIANTS[variant]:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as tar:
            for info, data in entries:
                tar.addfile(info, io.BytesIO(data) if info.type == tarfile.REGTYPE else None)
        raw = raw.getvalue()
        gz = io.BytesIO()
        with gzip.GzipFile(fileobj=gz, mode="wb", mtime=0) as z:
            z.write(raw)
        gz = gz.getvalue()
        diff_ids.append("sha256:" + hashlib.sha256(raw).hexdigest())
        layers.append({
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "digest": blob(blobs, gz),
            "size": len(gz),
        })
    config = json.dumps({
        "architecture": "amd64",
        "os": "linux",
        "rootfs": {"type": "layers", "diff_ids": diff_ids},
        "config": {},
    }, sort_keys=True).encode()
    manifest = json.dumps({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": blob(blobs, config),
            "size": len(config),
        },
        "layers": layers,
    }, sort_keys=True).encode()
    index = json.dumps({
        "schemaVersion": 2,
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": blob(blobs, manifest),
            "size": len(manifest),
            "platform": {"architecture": "amd64", "os": "linux"},
            "annotations": {"io.containerd.image.name": f"docker.io/hexlab/{variant}:1"},
        }],
    }, sort_keys=True).encode()
    with tarfile.open(out, "w") as tar:
        def add(name, data):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mtime = T1
            tar.addfile(info, io.BytesIO(data))
        add("oci-layout", b'{"imageLayoutVersion":"1.0.0"}')
        add("index.json", index)
        for digest, data in sorted(blobs.items()):
            add("blobs/sha256/" + digest.split(":", 1)[1], data)
    print(f"builder: Python {sys.version.split()[0]} tarfile (PAX), gzip mtime 0")
    for layer, diff in zip(layers, diff_ids):
        print(f"layer {layer['digest']} diff_id {diff}")


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] not in VARIANTS:
        sys.exit(f"usage: lab_image.py {{{','.join(VARIANTS)}}} OUT.tar")
    main(sys.argv[1], sys.argv[2])
