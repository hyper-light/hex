#!/usr/bin/env python3
"""A recorded day for G4's fair-share bar (AC-T7.5.2) from Azure's VM trace 2019 (Azure Public Dataset
V2: Cortez et al., "Resource Central: Understanding and Predicting Workloads for Improved Resource
Management in Large Cloud Platforms", SOSP 2017; https://github.com/Azure/AzurePublicDataset, licensed
CC BY 4.0, https://creativecommons.org/licenses/by/4.0/). The trace is converted, not copied: this
script is the conversion, and the raw file is not committed.

The trace: one region, 30 days. Each row of its `vmtable` is a VM: its `subscription id` (its owner),
`timestamp vm created` and `deleted` (seconds from the trace's start), and its `vm virtual core count
bucket` and `vm memory (gb) bucket`. Each VM is taken as a request of one of Hex's three kinds (an
OCI container, a Firecracker microVM or a Shards microVM) by its shape.

The rules, applied in this order and each fixed before any replay (bench/sched/g4_huawei.md):
  - host: an EC2 m5d.metal (48,000 millicores, 384 GiB), the host G2's main trace uses;
  - size: a bucket's label, its upper bound (cores 2, 4, 8, 24; GiB 2, 4, 8, 32, 64); a VM in an
    open bucket (">24", ">64") has no size and is left out, counted, and its owner can't be one of
    the four;
  - an owner's ask: the larger of its summed CPU and its summed memory, as a share of hosts (its
    dominant share, as DRF counts it);
  - owners: the four subscriptions asking for the most over the 30 days whose every VM is at most a
    quarter of a host (12 cores and 96 GiB: DRF's equal shares assume requests small beside a host),
    each of weight 1;
  - day: the day on which those owners' VMs start and end most often (most churn), first if tied; a
    creation at the trace's first second is a VM that was already running, not a start, and a
    deletion at or past its last second a VM still running, not an end;
  - hosts: the largest whole number of hosts at which each of the four asks, on average over that
    day, for more than a quarter of them;
  - a VM alive at the day's start arrives at its start (time 0); each runs until its recorded
    deletion, at least 1 ms;
  - kind: an owner's VMs are one kind, by G2's rule (`huawei_trace.py`, `classes_by_owner`): the
    owner with the most VMs first, each to the kind with the fewest so far, ties OCI, then
    Firecracker, then Shards; each VM's memory counts its kind's overhead, as G2's traces do.

  azure_vm_trace.py VMTABLE_CSV_GZ OUT
"""

import collections
import csv
import gzip
import hashlib
import sys

SOURCE = "https://github.com/Azure/AzurePublicDataset/releases/download/dataset-v2/trace_data_vmtable_vmtable.csv.gz"
HOST_CPU, HOST_MEM = 48_000, 384 << 30
HOST_CORES, HOST_GIB = 48, 384
DAY_S = 86_400
DAYS = 30
MIB = 1 << 20
CLASSES = ["oci", "firecracker", "shards"]
OVERHEAD = {"oci": 440_402, "firecracker": 5 * MIB, "shards": 5 * MIB}


def size(cores, gib):
    """A bucket's label as the VM's size, or None for an open bucket."""
    if cores.startswith(">") or gib.startswith(">"):
        return None
    return int(cores), int(gib)


def main(path, out):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    vms = []
    open_bucket = collections.Counter()
    with gzip.open(path, "rt", newline="") as fh:
        for row in csv.reader(fh):
            if len(row) < 11 or not row[3].isdigit():
                continue
            s = size(row[9].strip(), row[10].strip())
            if s is None:
                open_bucket[row[1]] += 1
                continue
            vms.append((row[0], row[1], s, int(row[3]), int(row[4])))
    largest = collections.defaultdict(float)
    for _, sub, (c, g), _, _ in vms:
        largest[sub] = max(largest[sub], c / HOST_CORES, g / HOST_GIB)
    small = {s for s, l in largest.items() if l <= 0.25 and s not in open_bucket}
    cpu, mem = collections.Counter(), collections.Counter()
    for _, sub, (c, g), lo, hi in vms:
        if sub in small and hi > lo:
            cpu[sub] += c / HOST_CORES * (hi - lo)
            mem[sub] += g / HOST_GIB * (hi - lo)
    over_trace = collections.Counter({s: max(cpu[s], mem[s]) for s in small})
    four = [s for s, _ in over_trace.most_common(4)]
    last = max(hi for *_, hi in vms)
    churn = collections.Counter()
    for _, sub, _, lo, hi in vms:
        if sub in four:
            for t in (lo, hi):
                if 0 < t < last:
                    churn[t // DAY_S] += 1
    day = max(range(DAYS), key=lambda d: (churn[d], -d))
    start, end = day * DAY_S, (day + 1) * DAY_S
    dcpu, dmem = collections.Counter(), collections.Counter()
    for _, sub, (c, g), lo, hi in vms:
        if sub in four:
            a, b = max(lo, start), min(hi, end)
            if b > a:
                dcpu[sub] += c / HOST_CORES * (b - a) / DAY_S
                dmem[sub] += g / HOST_GIB * (b - a) / DAY_S
    ask = {s: max(dcpu[s], dmem[s]) for s in four}
    fourth = min(ask.values())
    hosts = int(4 * fourth) - (1 if 4 * fourth == int(4 * fourth) else 0)
    if hosts < 1:
        raise SystemExit(f"day {day}: the fourth owner asks {fourth:.3f} of a host: no cluster qualifies")
    owner_of = {s: i for i, s in enumerate(four)}
    legs = []
    for vm, sub, (c, g), lo, hi in vms:
        if sub not in owner_of:
            continue
        a = max(lo, start)
        if not (a < end and hi > a):
            continue
        arrive = (a - start) * 10**9
        runs = max((hi - a) * 10**9, 1_000_000)
        legs.append((arrive, vm, owner_of[sub], c * 1000, g << 30, runs))
    legs.sort()
    launches = collections.Counter(owner for _, _, owner, *_ in legs)
    got = dict.fromkeys(CLASSES, 0)
    kind = {}
    for owner, n in sorted(launches.items(), key=lambda x: (-x[1], x[0])):
        cls = min(CLASSES, key=lambda k: (got[k], CLASSES.index(k)))
        kind[owner] = cls
        got[cls] += n
    lines = [
        "# source: Azure VM trace 2019, Azure Public Dataset V2 (Cortez et al., \"Resource Central\", "
        "SOSP 2017; https://github.com/Azure/AzurePublicDataset; licensed CC BY 4.0, "
        "https://creativecommons.org/licenses/by/4.0/). Converted by bench/sched/azure_vm_trace.py.",
        f"# raw: {SOURCE}, sha256 {h.hexdigest()}",
        "# host: EC2 m5d.metal (48 cores, 384 GiB); sizes are bucket labels (upper bounds); VMs in open "
        f"buckets left out: {sum(open_bucket.values())}",
        f"# owners: subscriptions {four}, the four asking most over the 30 days whose every VM is at most "
        "a quarter of a host, each of weight 1, in that order",
        f"# day: {day} (most starts and ends of the four owners' VMs: {churn[day]}); their asks that day "
        "in hosts: " + ", ".join(f"{ask[s]:.2f}" for s in four),
        f"# hosts: {hosts}, the largest whole number at which each of the four asks for more than a quarter",
        f"# requests: {len(legs)}; kinds by owner (G2's rule): " + ", ".join(f"owner {o} {kind[o]}" for o in sorted(kind)),
        "# overheads: oci 0.42 MiB, firecracker 5 MiB, shards 5 MiB (as huawei_trace.py)",
    ]
    lines += [f"node,{HOST_CPU},{HOST_MEM},0,0,oci+firecracker+shards"] * hosts
    lines += ["owner,1"] * len(four)
    lines += [f"overhead,{c},{OVERHEAD[c]}" for c in CLASSES]
    for arrive, _, owner, c, m, runs in legs:
        cls = kind[owner]
        lines.append(f"leg,{arrive},{runs},{c},{m + OVERHEAD[cls]},0,plain,{owner},0,-,-,{cls}")
    with open(out, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("\n".join(lines[:8]))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2]))
