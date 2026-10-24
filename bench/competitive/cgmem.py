"""bench/competitive: the memory a cgroup holds, the one measure behind every table's memory and density rows.

memory.current counts page cache, which the kernel fills and reclaims on its own schedule. On fedora, a
shared host, a Kubernetes stack's reading fell from 7 GiB to 1.7 GiB within minutes as other work's
pressure reclaimed its cache, and a density phase's rise across 20 pods drowned in that churn. So the memory
a cgroup holds is memory.current less its file cache (memory.stat `file`), keeping `shmem` (tmpfs and shared
memory, which reclaim can't drop), plus memory.swap.current (fedora swaps to zram, and anonymous memory
swapped out is still held, counted at its full size). On a zswap host memory.current also holds zswap's
compressed copies of pages memory.swap.current already counts, so `zswap` comes off. The fields are the
kernel's: Documentation/admin-guide/cgroup-v2.rst, memory.stat and memory.swap.current. A cgroup without
memory.swap.current (no swap accounting) is an error, not a zero.

Hugepages are held too, but memory.current counts them only when the hierarchy is mounted with
`memory_hugetlb_accounting` (cgroup-v2.rst: "Count HugeTLB memory usage towards the cgroup's overall memory
usage"). Without it, a cgroup's hugepages are its `hugetlb.<size>.current` files, so they're added from there.
E2B backs each sandbox's guest memory with 2 MiB hugepages; no other stack here uses any.
"""
import glob
import os


def _hugetlb_counted():
    with open("/proc/mounts") as f:
        for line in f:
            parts = line.split()
            if len(parts) > 3 and parts[2] == "cgroup2":
                return "memory_hugetlb_accounting" in parts[3].split(",")
    return False


HUGETLB_IN_CURRENT = _hugetlb_counted()


def hugetlb_bytes(path):
    """The hugepage memory the cgroup holds, of every page size; 0 where the hugetlb controller isn't on."""
    total = 0
    for f in glob.glob(os.path.join(path, "hugetlb.*.current")):
        with open(f) as fh:
            total += int(fh.read())
    return total


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
