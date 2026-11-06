#!/usr/bin/env python3
"""Which day of Huawei Cloud's public serverless traces 2025 (Joosen et al., EuroSys 2025; CC BY 4.0;
https://github.com/sir-lab/data-release) can carry G4's fair-share bar (AC-T7.5.2): four equal-weight
owners each asking for more than a quarter of a cluster of whole EC2 m5d.metal hosts (48,000
millicores, 384 GiB: the Firecracker paper's host, as `huawei_trace.py` takes it).

Only day 30 has request logs, which are what give a sandbox its lifetime, in every region. For a
region's day 30 it reads the cold starts (each one a sandbox launch: its pod's pool names its CPU and
memory, its `userID` its owner) and the request logs, and takes a sandbox's lifetime as
`huawei_trace.py` does: its cold start to the end of its pod's last request of the day. Each owner's
ask is the mean over the day (86,400 s) of the CPU and memory its sandboxes hold. It prints the busiest
owners, the largest sandbox as a share of a host, and the largest whole number of hosts at which four
owners each ask for more than a quarter (0: none).

The request logs are large (region 2's are 10 zips, 22 GB), so they are streamed: each request file is
downloaded, read and deleted before the next, and only the latest end per launched pod is kept. Each
downloaded file's sha256 is printed. What each request file gave is saved beside the downloads
(`REGION-<drive id>.json`, the latest end per launched pod), so a scan that Google Drive stops ("Quota
exceeded") resumes where it stopped: a saved file is not downloaded again. A refused download stops
the scan with nothing printed as a verdict.

  huawei_owners.py REGION COLD_STARTS_DRIVE_ID REQUEST_DRIVE_ID... [--dir DIR] [--day 30]
"""

import argparse
import collections
import csv
import decimal
import hashlib
import io
import json
import os
import re
import sys
import urllib.request
import zipfile

POOL = re.compile(r"pool\d+-(\d+)-(\d+)")
HOST_CPU, HOST_MEM_MB = 48_000, 384 * 1024
DAY_S = decimal.Decimal(86_400)


def download(drive_id, path):
    url = f"https://drive.usercontent.google.com/download?id={drive_id}&export=download&confirm=t"
    h = hashlib.sha256()
    with urllib.request.urlopen(url, timeout=600) as r, open(path, "wb") as out:
        for block in iter(lambda: r.read(1 << 20), b""):
            h.update(block)
            out.write(block)
    if not zipfile.is_zipfile(path):
        raise SystemExit(f"{drive_id}: not a zip (Drive refused the download?)")
    print(f"# {drive_id}: {os.path.getsize(path)} bytes, sha256 {h.hexdigest()}", flush=True)


def rows(path, ends):
    z = zipfile.ZipFile(path)
    for name in sorted(n for n in z.namelist() if n.endswith(ends)):
        yield from csv.DictReader(io.TextIOWrapper(z.open(name), encoding="utf-8"))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("region")
    ap.add_argument("cold")
    ap.add_argument("requests", nargs="+")
    ap.add_argument("--dir", default=".")
    ap.add_argument("--day", type=int, default=30)
    a = ap.parse_args()
    path = os.path.join(a.dir, f"{a.region}-part.zip")
    # The cold starts (tens of MiB) are kept for a rerun, so a resumed scan doesn't fetch them again.
    cold = os.path.join(a.dir, f"{a.region}-coldstarts.zip")
    if not os.path.exists(cold):
        download(a.cold, cold + ".tmp")
        os.replace(cold + ".tmp", cold)
    launched = {}
    for r in rows(cold, f"day_{a.day:02d}.csv"):
        pool = POOL.search(r["podID"])
        if pool and r["podID"] not in launched:
            launched[r["podID"]] = (decimal.Decimal(r["time"]), r["userID"], int(pool.group(1)), int(pool.group(2)))
    print(f"# {a.region} day {a.day}: {len(launched)} pods launched by cold starts", flush=True)
    last_end = {}
    for drive_id in a.requests:
        saved = os.path.join(a.dir, f"{a.region}-{drive_id}.json")
        if os.path.exists(saved):
            with open(saved) as fh:
                part = {pod: decimal.Decimal(end) for pod, end in json.load(fh).items()}
            print(f"# {drive_id}: read before, {len(part)} pods", flush=True)
        else:
            download(drive_id, path)
            part = {}
            rows_read = 0
            for r in rows(path, ".csv"):
                rows_read += 1
                pod = r["podID"]
                if pod in launched:
                    end = decimal.Decimal(r["time_worker"]) + decimal.Decimal(r["totalCost_worker"] or "0")
                    if end > part.get(pod, end - 1):
                        part[pod] = end
            os.remove(path)
            with open(saved + ".tmp", "w") as fh:
                json.dump({pod: str(end) for pod, end in part.items()}, fh)
            os.replace(saved + ".tmp", saved)
            print(f"# {drive_id}: {rows_read} request rows, {len(part)} pods", flush=True)
        for pod, end in part.items():
            if end > last_end.get(pod, end - 1):
                last_end[pod] = end
    cpu = collections.Counter()
    mem = collections.Counter()
    launches = collections.Counter()
    biggest = collections.Counter()
    traced = 0
    for pod, (start, user, c, m) in launched.items():
        if pod not in last_end:
            continue
        life = max(last_end[pod] - start, decimal.Decimal("0.001"))
        cpu[user] += c * life
        mem[user] += m * life
        launches[user] += 1
        biggest[user] = max(biggest[user], c)
        traced += 1
    # Each owner's mean ask as a share of one host: the larger of CPU and memory.
    share = {u: max(cpu[u] / DAY_S / HOST_CPU, mem[u] / DAY_S / HOST_MEM_MB) for u in cpu}
    top = sorted(share, key=lambda u: -share[u])
    print(f"# {traced} sandboxes traced (a request after their cold start); {len(top)} owners")
    print(f"# largest sandbox: {max(biggest.values(), default=0)} millicores, "
          f"{100 * max(biggest.values(), default=0) / HOST_CPU:.1f}% of a host's CPU")
    for u in top[:10]:
        print(f"owner {u}: {launches[u]} sandboxes, mean {float(cpu[u] / DAY_S) / 1000:.2f} cores, "
              f"{float(mem[u] / DAY_S) / 1024:.2f} GiB, {float(share[u]) * 100:.2f}% of a host")
    fourth = share[top[3]] if len(top) >= 4 else 0
    # Four owners each ask for more than a quarter of N hosts while the fourth's share of one host is
    # more than N / 4: the largest whole N is the largest integer below 4 * fourth.
    n = int(4 * fourth) if 4 * fourth != int(4 * fourth) else int(4 * fourth) - 1
    print(f"VERDICT {a.region} day {a.day}: fourth busiest owner asks {float(fourth) * 100:.3f}% of one host; "
          f"largest cluster of whole m5d.metal hosts where four owners each ask for more than a quarter: "
          f"{max(n, 0)} hosts")
    return 0


if __name__ == "__main__":
    sys.exit(main())
