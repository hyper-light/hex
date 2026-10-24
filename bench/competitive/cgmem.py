"""bench/competitive: the memory a cgroup holds, the one measure behind every table's memory and density rows.

memory.current counts page cache, which the kernel fills and reclaims on its own schedule. On fedora, a
shared host, a Kubernetes stack's reading fell from 7 GiB to 1.7 GiB within minutes as other work's
pressure reclaimed its cache, and a density phase's rise across 20 pods drowned in that churn. So the memory
a cgroup holds is memory.current less its file cache (memory.stat `file`), keeping `shmem` (tmpfs and shared
memory, which reclaim can't drop), plus memory.swap.current (fedora swaps to zram, and anonymous memory
swapped out is still held, counted at its full size). On a zswap host memory.current also holds zswap's
compressed copies of pages memory.swap.current already counts, so `zswap` comes off. The fields are the
kernel's: Documentation/admin-guide/cgroup-v2.rst, memory.stat and memory.swap.current. A cgroup without
memory.swap.current (no swap accounting) is an error, not a zero. memory.stat is a cached view, flushed on a
read only where enough updates are pending and otherwise every 2 s (mm/memcontrol.c), so a reading right after
a burst can miss its newest `file`, `shmem` and `zswap` updates. probes/scope_charge.py measured what that
costs a density reading: 371.4 KiB per box against 370.8 KiB once flushed, 0.2% (adacachy, 100 boxes).

Hugepages are held too, but memory.current counts them only when the hierarchy is mounted with
`memory_hugetlb_accounting` (cgroup-v2.rst: "Count HugeTLB memory usage towards the cgroup's overall memory
usage"). Without it, a cgroup's faulted hugepages are its `hugetlb.<size>.current`, so they're added from there.
Its `hugetlb.<size>.rsvd.current` counts reservations, and a faulted page stays charged there too
(cgroup-v1/hugetlb.rst), so the reservations are a figure of their own (`hugetlb_reserved_bytes`), never added
to what's held. E2B backs each sandbox's guest memory with 2 MiB hugepages; no other stack here uses any.
"""
import glob
import os
import re

_HUGETLB_USAGE = re.compile(r"hugetlb\.[0-9]+[KMG]B\.current")
_HUGETLB_RSVD = re.compile(r"hugetlb\.[0-9]+[KMG]B\.rsvd\.current")


def _hugetlb_counted():
    with open("/proc/mounts") as f:
        for line in f:
            parts = line.split()
            if len(parts) > 3 and parts[2] == "cgroup2":
                return "memory_hugetlb_accounting" in parts[3].split(",")
    return False


HUGETLB_IN_CURRENT = _hugetlb_counted()


def _hugetlb_sum(path, pattern):
    total = 0
    for f in glob.glob(os.path.join(path, "hugetlb.*")):
        if pattern.fullmatch(os.path.basename(f)):
            with open(f) as fh:
                total += int(fh.read())
    return total


def hugetlb_bytes(path):
    """The faulted hugepage memory the cgroup holds, of every page size; 0 where the hugetlb controller isn't on."""
    return _hugetlb_sum(path, _HUGETLB_USAGE)


def hugetlb_reserved_bytes(path):
    """The hugepage memory reserved for the cgroup (faulted or not), of every page size."""
    return _hugetlb_sum(path, _HUGETLB_RSVD)


def held_bytes(path):
    with open(os.path.join(path, "memory.stat")) as f:
        stat = {k: int(v) for k, v in (line.split() for line in f if line.strip())}
    with open(os.path.join(path, "memory.current")) as f:
        current = int(f.read())
    with open(os.path.join(path, "memory.swap.current")) as f:
        swapped = int(f.read())
    huge = 0 if HUGETLB_IN_CURRENT else hugetlb_bytes(path)
    return current - stat["file"] + stat["shmem"] - stat.get("zswap", 0) + swapped + huge


def per_unit(held, count):
    """A density phase's figures from its held-memory rise and the units it held."""
    return {"bytes": held, "per_unit_bytes": held / count if count and held > 0 else None,
            "per_gib": count / (held / 2**30) if held > 0 else None}
