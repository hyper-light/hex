#!/usr/bin/env python3
"""A recorded day for G4's fair-share bar (AC-T7.5.2) from Azure's Packing Trace 2020: Hadary et al.,
"Protean: VM Allocation Service at Scale", OSDI 2020; https://github.com/Azure/AzurePublicDataset,
licensed CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). The trace is converted, not
copied: this script is the conversion, and the raw file is not committed.

The trace: one zone, 14 days. Each VM request names its `tenantId` (its owner), `priority`,
`starttime` and `endtime` (fractional days; negative for a VM alive at the start; NULL for one alive
past the 90-day cap) and a `vmTypeId`, whose cores and memory are fractions of a machine, per
machine type. Each VM is taken as a request of one of Hex's three kinds (an OCI container, a
Firecracker microVM or a Shards microVM) by its shape.

The rules, applied in this order and each fixed before any replay (bench/sched/g4_huawei.md):
  - host: the machine type that runs the most VM types; a request is that machine's fractions of an
    EC2 m5d.metal (48,000 millicores, 384 GiB), the host G2's main trace uses; a request whose VM
    type that machine doesn't run is left out and counted;
  - an owner's ask: the larger of its summed CPU and its summed memory (its dominant share, as DRF
    counts it);
  - owners: the four tenants asking for the most over the trace whose every request is at most a
    quarter of a host (DRF's equal shares assume requests small beside a host), each of weight 1;
  - day: the day on which those owners' requests start and end most often (most churn), first if tied;
  - hosts: the largest whole number of hosts at which each of the four asks, on average over that
    day, for more than a quarter of them;
  - a request alive at the day's start arrives at its start (time 0); each runs until its recorded end
    (the cap's end, 90 days, for one past it), at least 1 ms;
  - kind: an owner's requests are one kind, by G2's rule (`huawei_trace.py`, `classes_by_owner`): the
    owner with the most requests first, each to the kind with the fewest so far, ties OCI, then
    Firecracker, then Shards; each request's memory counts its kind's overhead, as G2's traces do.

  azure_trace.py PACKING_ZIP OUT
"""

import collections
import hashlib
import sqlite3
import sys
import tempfile
import zipfile

SOURCE = ("https://github.com/Azure/AzurePublicDataset/releases/download/dataset-packing-2020/"
          "azurevmallocation_dataset2020_AzurePackingTraceV1.zip")
SHA256 = "af967499c4f2c38bb37f8fbb202768115cff6c2e522272a498a414762d85fe16"
HOST_CPU, HOST_MEM = 48_000, 384 << 30
DAY_NS = 86_400 * 10**9
MIB = 1 << 20
CLASSES = ["oci", "firecracker", "shards"]
OVERHEAD = {"oci": 440_402, "firecracker": 5 * MIB, "shards": 5 * MIB}
CAP_DAYS = 90.0


def main(zip_path, out):
    h = hashlib.sha256()
    with open(zip_path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    if h.hexdigest() != SHA256:
        raise SystemExit(f"{zip_path} is not the file recorded (sha256 {SHA256})")
    with tempfile.TemporaryDirectory(dir=".") as tmp:
        z = zipfile.ZipFile(zip_path)
        db = next(n for n in z.namelist() if n.endswith((".sqlite", ".db", ".sqlite3")))
        z.extract(db, tmp)
        con = sqlite3.connect(f"{tmp}/{db}")
        per_machine = collections.Counter(m for (m,) in con.execute("select machineId from vmType"))
        host = per_machine.most_common(1)[0][0]
        size = {v: (c, m) for v, c, m in con.execute(
            "select vmTypeId, core, memory from vmType where machineId = ?", (host,))}
        vms = []
        left_out = 0
        for vm, tenant, vmtype, start, end in con.execute(
                "select vmId, tenantId, vmTypeId, starttime, endtime from vm"):
            if vmtype not in size:
                left_out += 1
                continue
            vms.append((vm, tenant, size[vmtype], start, end))
    largest = collections.defaultdict(float)
    for _, tenant, (c, m), _, _ in vms:
        largest[tenant] = max(largest[tenant], c, m)

    def ask_on(day, tenants):
        cpu, mem = collections.Counter(), collections.Counter()
        for _, tenant, (c, m), start, end in vms:
            if tenant in tenants:
                lo = max(max(start, 0.0), day)
                hi = min(CAP_DAYS if end is None else end, day + 1)
                if hi > lo:
                    cpu[tenant] += c * (hi - lo)
                    mem[tenant] += m * (hi - lo)
        # An owner's ask is the larger of its summed CPU and summed memory: its dominant share of the
        # cluster, as DRF counts it (a sum of each request's larger dimension overstates it).
        return {t: max(cpu[t], mem[t]) for t in tenants}

    small = {t for t, s in largest.items() if s <= 0.25}
    cpu14, mem14 = collections.Counter(), collections.Counter()
    for _, tenant, (c, m), start, end in vms:
        if tenant in small:
            lo, hi = max(start, 0.0), min(14.0 if end is None else end, 14.0)
            if hi > lo:
                cpu14[tenant] += c * (hi - lo)
                mem14[tenant] += m * (hi - lo)
    over_trace = collections.Counter({t: max(cpu14[t], mem14[t]) for t in small})
    four = [t for t, _ in over_trace.most_common(4)]
    churn = collections.Counter()
    for _, tenant, _, start, end in vms:
        if tenant in four:
            for t in (start, end):
                if t is not None and 0 <= t < 14:
                    churn[int(t)] += 1
    day = max(range(14), key=lambda d: (churn[d], -d))
    ask = ask_on(day, set(four))
    fourth = min(ask[t] for t in four)
    hosts = int(4 * fourth) - (1 if 4 * fourth == int(4 * fourth) else 0)
    if hosts < 1:
        raise SystemExit(f"day {day}: the fourth owner asks {fourth:.3f} of a host: no cluster qualifies")
    owner_of = {t: i for i, t in enumerate(four)}
    legs = []
    for vm, tenant, (c, m), start, end in vms:
        if tenant not in owner_of:
            continue
        lo = max(max(start, 0.0), day)
        hi = CAP_DAYS if end is None else end
        if not (lo < day + 1 and hi > lo):
            continue
        arrive = round((lo - day) * DAY_NS)
        runs = max(round((hi - lo) * DAY_NS), 1_000_000)
        legs.append((arrive, vm, owner_of[tenant], round(c * HOST_CPU), round(m * HOST_MEM), runs))
    legs.sort()
    launches = collections.Counter(owner for _, _, owner, *_ in legs)
    got = dict.fromkeys(CLASSES, 0)
    kind = {}
    for owner, n in sorted(launches.items(), key=lambda x: (-x[1], x[0])):
        cls = min(CLASSES, key=lambda k: (got[k], CLASSES.index(k)))
        kind[owner] = cls
        got[cls] += n
    lines = [
        "# source: Azure Packing Trace 2020 (Hadary et al., \"Protean: VM Allocation Service at Scale\", "
        "OSDI 2020; https://github.com/Azure/AzurePublicDataset; licensed CC BY 4.0, "
        "https://creativecommons.org/licenses/by/4.0/). Converted by bench/sched/azure_trace.py.",
        f"# raw: {SOURCE}, sha256 {SHA256}",
        f"# host: machine type {host} (it runs {len(size)} VM types, the most), its fractions of an EC2 "
        "m5d.metal (48 cores, 384 GiB); requests of other VM types left out: " + str(left_out),
        f"# owners: tenants {four}, the four asking most over the 14 days whose every request is at most "
        "a quarter of a host, each of weight 1, in that order",
        f"# day: {day} (most starts and ends of the four owners' requests: {churn[day]}); their asks that "
        "day in hosts: " + ", ".join(f"{ask[t]:.2f}" for t in four),
        f"# hosts: {hosts}, the largest whole number at which each of the four asks for more than a quarter",
        f"# requests: {len(legs)}; kinds by owner (G2's rule): " + ", ".join(f"owner {o} {kind[o]}" for o in sorted(kind)),
        "# overheads: oci 0.42 MiB, firecracker 5 MiB, shards 5 MiB (as huawei_trace.py)",
    ]
    lines += [f"node,{HOST_CPU},{HOST_MEM},0,0,oci+firecracker+shards"] * hosts
    lines += ["owner,1"] * len(four)
    lines += [f"overhead,{c},{OVERHEAD[c]}" for c in CLASSES]
    for arrive, _, owner, cpu, mem, runs in legs:
        cls = kind[owner]
        lines.append(f"leg,{arrive},{runs},{cpu},{mem + OVERHEAD[cls]},0,plain,{owner},0,-,-,{cls}")
    with open(out, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    print("\n".join(lines[:8]))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2]))
