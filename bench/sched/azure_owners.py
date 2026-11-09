#!/usr/bin/env python3
"""Which day of Azure's public VM traces (https://github.com/Azure/AzurePublicDataset, CC BY 4.0) can
carry G4's fair-share bar (AC-T7.5.2): four equal-weight owners each asking, on average over the day,
for more than a quarter of a cluster of whole hosts, each request small beside a host.

  azure_owners.py packing PACKING_ZIP        # Azure Packing Trace 2020 (Hadary et al., OSDI 2020)
  azure_owners.py vms VMTABLE_CSV_GZ          # Azure Public Dataset V2, 2019 (Cortez et al., SOSP 2017)

Packing 2020: one zone, 14 days. Each VM request names its `tenantId` (the owner), `priority`,
`starttime` and `endtime` (fractional days; negative for VMs alive at the start, NULL past the 90-day
cap) and a `vmTypeId`, whose cores and memory are fractions of a machine, per machine type. The host is
the machine type that runs the most VM types; a VM whose type that machine doesn't run is counted and
left out. An owner's ask over a day is the mean, over the day, of the core and memory fractions its
live VMs hold, the larger of the two: in hosts.

VM trace 2019: one region, 30 days, the `vmtable`: each VM's `subscription id` (the owner),
`timestamp vm created` and `deleted` (seconds), and its `vm virtual core count bucket` and `vm memory
(gb) bucket`. A bucket's lower bound is taken as the VM's size (understating each ask, so a day that
qualifies still does at the true sizes), and the host is an EC2 m5d.metal (48 cores, 384 GiB).

For each day it prints the four busiest owners' asks as a share of one host, and the largest whole
number of hosts at which four owners each ask for more than a quarter (0: none); and the largest
request as a share of a host.
"""

import collections
import csv
import gzip
import io
import sqlite3
import sys
import tempfile
import zipfile

DAY_S = 86_400


def verdict(label, day, cpu, mem, biggest):
    # An owner's ask is the larger of its summed CPU and summed memory: its dominant share, as DRF
    # counts it (a sum of each request's larger dimension overstates it).
    owners_share = {o: max(cpu[o], mem[o]) for o in set(cpu) | set(mem)}
    top = sorted(owners_share.items(), key=lambda kv: -kv[1])
    four = [s for _, s in top[:4]]
    fourth = four[3] if len(four) == 4 else 0.0
    # Four owners each ask for more than a quarter of N hosts while the fourth's share of one host is
    # more than N / 4: the largest whole N is the largest integer below 4 * fourth.
    n = int(4 * fourth)
    if n == 4 * fourth:
        n -= 1
    shares = ", ".join(f"{100 * s:.1f}%" for s in four)
    print(f"{label} day {day}: owners {len(top)}; busiest four {shares} of a host; "
          f"largest request {100 * biggest:.1f}% of a host; qualifying cluster {max(n, 0)} hosts", flush=True)
    return max(n, 0)


def packing(path):
    with tempfile.TemporaryDirectory(dir=".") as tmp:
        z = zipfile.ZipFile(path)
        db = [n for n in z.namelist() if n.endswith((".sqlite", ".db", ".sqlite3"))]
        if not db:
            raise SystemExit(f"{path}: no sqlite file in {z.namelist()}")
        z.extract(db[0], tmp)
        con = sqlite3.connect(f"{tmp}/{db[0]}")
        tables = [r[0] for r in con.execute("select name from sqlite_master where type='table'")]
        print(f"# tables: {tables}", flush=True)
        cols = {t: [r[1] for r in con.execute(f"pragma table_info('{t}')")] for t in tables}
        print(f"# columns: {cols}", flush=True)
        req = next(t for t in tables if "tenantId" in cols[t])
        typ = next(t for t in tables if "machineId" in cols[t])
        per_machine = collections.Counter(m for (m,) in con.execute(f"select machineId from {typ}"))
        host = per_machine.most_common(1)[0][0]
        size = {v: (c, m) for v, c, m in con.execute(f"select vmTypeId, core, memory from {typ} where machineId = ?", (host,))}
        print(f"# host: machine type {host}, running {len(size)} VM types of {len(set(per_machine))} machine types' rows", flush=True)
        cpu = collections.defaultdict(lambda: collections.defaultdict(float))
        mem = collections.defaultdict(lambda: collections.defaultdict(float))
        biggest = 0.0
        left_out = 0
        for tenant, vmtype, start, end in con.execute(f"select tenantId, vmTypeId, starttime, endtime from {req}"):
            if vmtype not in size:
                left_out += 1
                continue
            c, m = size[vmtype]
            share = max(c, m)
            biggest = max(biggest, share)
            start = max(start, 0.0)
            end = 14.0 if end is None else min(end, 14.0)
            d = int(start)
            while d < end and d < 14:
                lo, hi = max(start, d), min(end, d + 1)
                if hi > lo:
                    cpu[d][tenant] += c * (hi - lo)
                    mem[d][tenant] += m * (hi - lo)
                d += 1
        print(f"# requests left out (type not run by the host): {left_out}", flush=True)
        best = 0
        for d in range(14):
            best = max(best, verdict("packing2020", d, cpu[d], mem[d], biggest))
        print(f"VERDICT packing2020: largest qualifying cluster on any day: {best} hosts", flush=True)


def lower_bound(bucket):
    text = bucket.strip().lstrip(">").split("-")[0]
    return float(text)


def vms(path):
    host_cores, host_gb = 48.0, 384.0
    cpu = collections.defaultdict(lambda: collections.defaultdict(float))
    mem = collections.defaultdict(lambda: collections.defaultdict(float))
    biggest = 0.0
    buckets = collections.Counter()
    with gzip.open(path, "rt", newline="") as fh:
        for row in csv.reader(fh):
            if len(row) < 11 or not row[3].isdigit():
                continue
            sub, created, deleted = row[1], int(row[3]), int(row[4])
            cores, gb = lower_bound(row[9]), lower_bound(row[10])
            buckets[(row[9], row[10])] += 1
            share = max(cores / host_cores, gb / host_gb)
            biggest = max(biggest, share)
            d = created // DAY_S
            while d * DAY_S < deleted and d < 30:
                lo, hi = max(created, d * DAY_S), min(deleted, (d + 1) * DAY_S)
                if hi > lo:
                    cpu[d][sub] += cores / host_cores * (hi - lo) / DAY_S
                    mem[d][sub] += gb / host_gb * (hi - lo) / DAY_S
                d += 1
    print(f"# buckets (cores, GB): {dict(buckets.most_common(12))}", flush=True)
    best = 0
    for d in range(30):
        best = max(best, verdict("vm2019", d, cpu[d], mem[d], biggest))
    print(f"VERDICT vm2019: largest qualifying cluster on any day: {best} hosts", flush=True)


if __name__ == "__main__":
    {"packing": packing, "vms": vms}[sys.argv[1]](sys.argv[2])
