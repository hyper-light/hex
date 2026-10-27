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

A density phase reads its baseline only once no cgroup under what it watches is still being torn down
(settle). The kernel charges a new memory cgroup's own statistics to its parent (mm/memcontrol.c,
mem_cgroup_css_alloc: set_active_memcg(parent)) and frees them only after the cgroup is released, so a baseline
read right after cgroups came and went starts high and the phase's rise comes out low. On adacachy, right after
300 cgroups were made and removed, a baseline read at once stood 15.1-17.1 MiB above a reading 0.5 s later, and
one read once settle returned (41-56 ms) stood 2.0-3.2 MiB above it (four runs): what the kernel frees after a
release (RCU-deferred), under 1% of a density phase's rise over 1,000 units.

A density phase also records what its held rise is made of (breakdown, rise): memory.stat's anon, shmem, kernel
and sock, which with swap, less zswap, and hugepages make up what's held (memory.current charges anonymous,
file and kernel pages and socket buffers; file includes shmem), and kernel's own parts (slab, percpu,
pagetables, sec_pagetables, kernel_stack, vmalloc). So one runtime's held memory over another's is named by
entry: shmem for runtime state on a tmpfs, slab for kernel objects its container setup leaves charged.
"""
import glob
import os
import re
import time

_HUGETLB_USAGE = re.compile(r"hugetlb\.[0-9]+[KMG]B\.current")
_HUGETLB_RSVD = re.compile(r"hugetlb\.[0-9]+[KMG]B\.rsvd\.current")
_COUNT = re.compile(r"[0-9]+")  # ASCII digits only: str.isdigit takes "²", which int() refuses


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


# The memory.stat entries a density phase records (cgroup-v2.rst, memory.stat). What a cgroup holds is the sum of
# HELD_PARTS: anon, shmem (tmpfs and shared memory), kernel (every kernel allocation charged to the cgroup) and
# sock, with swap added and zswap's compressed copies taken off as held_bytes does. KERNEL_PARTS are the named
# parts of kernel; what kernel holds beyond them is kernel_other.
STAT_ENTRIES = ("anon", "file", "shmem", "kernel", "slab", "percpu", "pagetables", "kernel_stack", "sock", "vmalloc")
HELD_PARTS = ("anon", "shmem", "kernel", "sock")
KERNEL_PARTS = ("slab", "percpu", "pagetables", "sec_pagetables", "kernel_stack", "vmalloc")


def parse_stat(text):
    """memory.stat's text as ({entry: bytes}, [reasons]): a line that isn't one name and one non-negative
    integer, or a name given twice, is a reason and is left out, as is each of STAT_ENTRIES that's missing."""
    stat, reasons = {}, []
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        parts = line.split()
        if len(parts) != 2 or not _COUNT.fullmatch(parts[1]):
            reasons.append(f"line {n}: not a name and a non-negative integer: {line[:80]!r}")
        elif parts[0] in stat:
            reasons.append(f"line {n}: {parts[0]} given twice")
        else:
            stat[parts[0]] = int(parts[1])
    reasons += [f"no {k}" for k in STAT_ENTRIES if k not in stat]
    return stat, reasons


def _read_int(path):
    with open(path) as f:
        return int(f.read())


def snapshot(path):
    """One reading of the cgroup: memory.current, memory.swap.current, faulted hugepages not in memory.current,
    and memory.stat with the reasons it couldn't be read whole. held and breakdown take the same reading."""
    with open(os.path.join(path, "memory.stat"), "rb") as f:
        stat, reasons = parse_stat(f.read().decode(errors="replace"))  # a byte that isn't text is a malformed line
    return {"current": _read_int(os.path.join(path, "memory.current")),
            "swap": _read_int(os.path.join(path, "memory.swap.current")),
            "huge": 0 if HUGETLB_IN_CURRENT else hugetlb_bytes(path), "stat": stat, "reasons": reasons}


def held(snap):
    """What the cgroup holds, from a snapshot; a memory.stat without file or shmem is a ValueError."""
    stat = snap["stat"]
    missing = [k for k in ("file", "shmem") if k not in stat]
    if missing:
        raise ValueError(f"memory.stat has no {', '.join(missing)}: {'; '.join(snap['reasons'])}")
    return snap["current"] - stat["file"] + stat["shmem"] - stat.get("zswap", 0) + snap["swap"] + snap["huge"]


def held_bytes(path):
    return held(snapshot(path))


def breakdown(snap):
    """A snapshot's held memory by part: STAT_ENTRIES and sec_pagetables as memory.stat has them (an entry it
    lacks is None), swap, zswap and huge, kernel_other (kernel less KERNEL_PARTS) and unattributed (held less
    HELD_PARTS, swap and huge, zswap taken off); `reasons` names what memory.stat lacked or had malformed, and
    a sum that needs a missing entry is None."""
    stat = snap["stat"]
    out = {k: stat.get(k) for k in STAT_ENTRIES + ("sec_pagetables",)}
    out.update(swap=snap["swap"], zswap=stat.get("zswap", 0), huge=snap["huge"], reasons=list(snap["reasons"]))
    kernel_parts = [stat.get(k, 0 if k == "sec_pagetables" else None) for k in KERNEL_PARTS]
    out["kernel_other"] = (stat["kernel"] - sum(kernel_parts)
                           if "kernel" in stat and None not in kernel_parts else None)
    parts = [stat.get(k) for k in HELD_PARTS]
    try:
        whole = held(snap)
    except ValueError:
        whole = None
    out["held"] = whole
    out["unattributed"] = (whole - sum(parts) - snap["swap"] - snap["huge"] + out["zswap"]
                           if whole is not None and None not in parts else None)
    return out


def rise(peak, base):
    """Each numeric part's rise from a baseline breakdown to a peak one (None where either lacks it), with both
    readings' reasons."""
    out = {k: (v - base[k] if isinstance(v, int) and isinstance(base.get(k), int) else None)
           for k, v in peak.items() if k != "reasons"}
    out["reasons"] = [f"base: {r}" for r in base["reasons"]] + [f"peak: {r}" for r in peak["reasons"]]
    return out


# A state root holds a few files per container; a walk past this many entries stops, its count marked short.
TREE_ENTRY_BUDGET = 1_000_000


def tree_bytes(root):
    """The bytes under a runtime's state root: files and directories counted (symlinks not followed), their
    apparent size, and the blocks they take (st_blocks, what a tmpfs charges as shmem). A walk is flat, with a
    stack, and stops at TREE_ENTRY_BUDGET entries ("short": True); an entry that goes while it's walked is
    skipped, and a root that isn't there reads zero."""
    out = {"root": root, "entries": 0, "apparent_bytes": 0, "allocated_bytes": 0, "short": False}
    stack = [root]
    while stack:
        top = stack.pop()
        try:
            with os.scandir(top) as it:
                for e in it:
                    if out["entries"] >= TREE_ENTRY_BUDGET:
                        out["short"] = True
                        return out
                    try:
                        st = e.stat(follow_symlinks=False)
                    except FileNotFoundError:
                        continue
                    out["entries"] += 1
                    out["apparent_bytes"] += st.st_size
                    out["allocated_bytes"] += st.st_blocks * 512
                    if e.is_dir(follow_symlinks=False):
                        stack.append(e.path)
        except (FileNotFoundError, NotADirectoryError):
            continue
    return out


def dying(path):
    """How many cgroups under `path` are removed but not yet released (cgroup.stat nr_dying_descendants); 0 for a
    stand-in directory off the bench host, which has no cgroup.stat."""
    try:
        with open(os.path.join(path, "cgroup.stat")) as f:
            for line in f:
                k, v = line.split()
                if k == "nr_dying_descendants":
                    return int(v)
    except FileNotFoundError:
        pass
    return 0


def settle(paths, timeout_s):
    """Waits, at most timeout_s, until no cgroup under `paths` is dying; returns how many still were."""
    deadline = time.monotonic() + timeout_s
    while True:
        left = sum(dying(p) for p in paths if os.path.isdir(p))
        if left == 0 or time.monotonic() > deadline:
            return left
        time.sleep(0.005)


def per_unit(held, count):
    """A density phase's figures from its held-memory rise and the units it held."""
    return {"bytes": held, "per_unit_bytes": held / count if count and held > 0 else None,
            "per_gib": count / (held / 2**30) if held > 0 else None}
