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
"""
import os


def held_bytes(path):
    with open(os.path.join(path, "memory.stat")) as f:
        stat = {k: int(v) for k, v in (line.split() for line in f if line.strip())}
    with open(os.path.join(path, "memory.current")) as f:
        current = int(f.read())
    with open(os.path.join(path, "memory.swap.current")) as f:
        swapped = int(f.read())
    return current - stat["file"] + stat["shmem"] - stat.get("zswap", 0) + swapped


def per_unit(held, count):
    """A density phase's figures from its held-memory rise and the units it held."""
    return {"bytes": held, "per_unit_bytes": held / count if count and held > 0 else None,
            "per_gib": count / (held / 2**30) if held > 0 else None}
