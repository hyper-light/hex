#!/usr/bin/env python3
"""Counts what a container image's layers hold that a staged tarball root must carry (docs/design/shards-guest.md
§4a): hard-link groups, owners, extended attributes, times, sparse and PAX extras, whiteouts and device nodes.

    census.py <image tar from `docker save`>   -> one JSON object per layer on stdout, then a total

Reads each layer with Python's tarfile, which decodes PAX and GNU records (xattrs arrive as PAX keys
SCHILY.xattr.* or LIBARCHIVE.xattr.*). Prints counts and up to five examples of each kind.
"""
import collections, json, sys, tarfile

EXAMPLES = 5


def layers(image):
    with tarfile.open(image) as outer:
        manifest = json.load(outer.extractfile("manifest.json"))
        for path in manifest[0]["Layers"]:
            yield path, outer.extractfile(path)


def census(name, fileobj):
    c = collections.Counter()
    ex = collections.defaultdict(list)
    owners = collections.Counter()
    xattr_ns = collections.Counter()
    pax_other = collections.Counter()
    mtimes = collections.Counter()
    groups = collections.defaultdict(list)

    def note(kind, what):
        c[kind] += 1
        if len(ex[kind]) < EXAMPLES:
            ex[kind].append(what)

    with tarfile.open(fileobj=fileobj, mode="r|*") as t:
        for m in t:
            c["entries"] += 1
            kind = {tarfile.REGTYPE: "file", tarfile.AREGTYPE: "file", tarfile.DIRTYPE: "dir",
                    tarfile.SYMTYPE: "symlink", tarfile.LNKTYPE: "hardlink", tarfile.CHRTYPE: "char",
                    tarfile.BLKTYPE: "block", tarfile.FIFOTYPE: "fifo", tarfile.CONTTYPE: "file",
                    tarfile.GNUTYPE_SPARSE: "sparse"}.get(m.type, "other:%r" % m.type)
            c[kind] += 1
            if kind == "hardlink":
                groups[m.linkname].append(m.name)
            if kind in ("char", "block", "fifo"):
                note("device_or_fifo", f"{m.name} ({kind})")
            if m.issparse():
                note("sparse_entries", m.name)
            base = m.name.rsplit("/", 1)[-1]
            if base.startswith(".wh."):
                note("whiteouts", m.name)
            if m.uid != 0 or m.gid != 0:
                owners[(m.uid, m.gid, m.uname, m.gname)] += 1
                note("non_root_owner", f"{m.name} {m.uid}:{m.gid} {m.uname}:{m.gname}")
            if m.mode & 0o6000:
                note("setuid_or_setgid", f"{m.name} {oct(m.mode)}")
            mtimes[int(m.mtime)] += 1
            for key, value in m.pax_headers.items():
                for prefix in ("SCHILY.xattr.", "LIBARCHIVE.xattr."):
                    if key.startswith(prefix):
                        attr = key[len(prefix):]
                        ns = attr.split(".", 1)[0]
                        xattr_ns[f"{prefix}{ns}"] += 1
                        note("xattrs", f"{m.name} {attr} ({len(value.encode('utf-8', 'surrogateescape'))} bytes)")
                        break
                else:
                    if key.startswith("GNU.sparse."):
                        note("pax_sparse", f"{m.name} {key}")
                    elif key not in ("path", "linkpath", "size", "mtime", "atime", "ctime", "uid", "gid",
                                     "uname", "gname"):
                        pax_other[key] += 1
                    else:
                        pax_other[f"(standard) {key}"] += 1
    for target, names in groups.items():
        c["hardlink_groups"] += 1
        if len(ex["hardlink_groups"]) < EXAMPLES or any("perl" in n for n in [target, *names]):
            ex["hardlink_groups"].append([target, *names])
    return {
        "layer": name,
        "counts": dict(c),
        "owners": [[*k, n] for k, n in owners.most_common(10)],
        "distinct_owners": len(owners),
        "xattr_namespaces": dict(xattr_ns),
        "pax_keys_other": dict(pax_other),
        "mtimes": {"distinct": len(mtimes), "min": min(mtimes, default=0), "max": max(mtimes, default=0),
                   "zero": mtimes.get(0, 0)},
        "examples": dict(ex),
    }


def main():
    total = collections.Counter()
    for name, fileobj in layers(sys.argv[1]):
        row = census(name, fileobj)
        print(json.dumps(row, sort_keys=True))
        total.update(row["counts"])
        total.update({f"xattr {k}": v for k, v in row["xattr_namespaces"].items()})
    print(json.dumps({"total": dict(total)}, sort_keys=True))


if __name__ == "__main__":
    main()
