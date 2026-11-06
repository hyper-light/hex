#!/usr/bin/env python3
"""Turn a day of Huawei Cloud's public serverless trace into hex-sim's trace text
(crates/hex-sim/src/replay.rs), for G2's recorded microVM workload (AC-T7.4.3; G4's owners).

The source: Huawei Cloud's public serverless traces 2025, from A. Joosen, A. Hassan, M. Asenov,
R. Singh, L. Darlow, J. Wang et al., "Serverless Cold Starts and Where to Find Them", EuroSys 2025
(doi:10.1145/3689031.3696073), released at https://github.com/sir-lab/data-release under the
Creative Commons Attribution 4.0 International licence (CC BY 4.0). The raw files are not
committed: they are downloaded from the URLs below, checked against the sha256s below (anything else
is refused), and read in place.

What the sandboxes are: Huawei's serverless pods, which may be containers rather than microVMs. Hex
takes each as a microVM request by its shape: when it launched, how long it lived, its CPU and
memory, and its owner.

  - Each cold start of the day is one sandbox launched (a pod created). One whose pod ID names no
    resource pool, or a second cold start of a pod already launched, is counted and left out.
  - arrives: the cold start's `time` (seconds, read exactly), less the day's first, in ns.
  - runs for: from the cold start to the end of the pod's last request of the day (`time_worker` plus
    `totalCost_worker`, both seconds, from the day's request table), plus --keep-alive-s. That is a
    lower bound without a keep-alive; Huawei's pods wait one minute after each request before they
    are deleted ("In our system, this time is set to one minute by default", Joosen et al. §2), so
    --keep-alive-s 60 gives the pod's whole life. At least 1 ms: the two logs' clocks differ
    slightly, so a few pods' last request ends before their cold start's time. A pod with no request
    is counted and left out.
  - need: the pod's resource pool, named in its pod ID (`pool24-600-512`: 600 millicores, 512 MB),
    as millicores and MiB. No GPU: the trace names no device.
  - kind: plain. The trace has no builds or benchmarks.
  - owner: the `userID`, each a dense index by first launch, each of weight 1 (an `owner,1` row
    each); band 0, no gang, no group.
  - nodes: --nodes hosts of an EC2 m5d.metal, the host the Firecracker paper measured on ("two Intel
    Xeon Platinum 8175M processors (for a total of 48 cores with hyper-threading disabled), 384GB of
    RAM", Agache et al., NSDI 2020 §6): 48,000 millicores, 384 GiB, no GPU, no background load.

  huawei_trace.py COLD_STARTS_ZIP REQUESTS_ZIP OUT [--day 30] [--keep-alive-s 0] [--nodes 10]
"""

import argparse
import collections
import csv
import decimal
import hashlib
import io
import re
import sys
import zipfile

SOURCES = {
    "cold starts": ("https://drive.google.com/file/d/1ah7nj5fyedMpOeaFgNVA7oMx3815D8BZ (R5.zip)",
                    "dadea4f0e5df629f016bb30144baf97d996f93ac487a161f713fb7f5f05e8bf0"),
    "requests": ("https://drive.google.com/file/d/102AFIHkEQHuvG8-hZeWHVFqXeeI-a8xS (R5.zip)",
                 "5825f74443cf6e4f5df264f8c94d439fc9eae576399f9786a50ca4cb3316e60d"),
}
POOL = re.compile(r"pool\d+-(\d+)-(\d+)")
NS = decimal.Decimal(10**9)
MIB = 1 << 20
M5D_METAL = (48_000, 384 << 30)
MIN_LIFE_NS = 1_000_000


class Refused(Exception):
    pass


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def rows(zip_path, member_ends):
    z = zipfile.ZipFile(zip_path)
    names = sorted(n for n in z.namelist() if n.endswith(member_ends))
    if not names:
        raise Refused(f"{zip_path} holds no file ending {member_ends!r}")
    for name in names:
        yield from csv.DictReader(io.TextIOWrapper(z.open(name), encoding="utf-8", errors="strict"))


def seconds(text, what):
    try:
        return decimal.Decimal(text)
    except decimal.InvalidOperation:
        raise Refused(f"{what} {text!r} is not a number") from None


def convert(cold_zip, req_zip, day, keep_alive_s, nodes):
    for (what, (_, want)), path in zip(SOURCES.items(), (cold_zip, req_zip)):
        if sha256(path) != want:
            raise Refused(f"the {what} file {path} is not the one recorded (sha256 {want})")
    count = collections.Counter()
    launched = {}
    for r in rows(cold_zip, f"day_{day:02d}.csv"):
        pool = POOL.search(r["podID"])
        if not pool:
            count["cold starts whose pod ID names no pool, left out"] += 1
            continue
        if r["podID"] in launched:
            count["second cold starts of a pod already launched, left out"] += 1
            continue
        launched[r["podID"]] = (seconds(r["time"], "cold start time"), r["userID"], r["funcName"],
                                int(pool.group(1)), int(pool.group(2)))
    if not launched:
        raise Refused(f"day {day} holds no cold start")
    last_end = {}
    for r in rows(req_zip, ".csv"):
        pod = r["podID"]
        if pod not in launched:
            continue
        end = seconds(r["time_worker"], "request time") + seconds(r["totalCost_worker"] or "0", "request cost")
        if pod not in last_end or end > last_end[pod]:
            last_end[pod] = end
    keep = decimal.Decimal(keep_alive_s)
    sandboxes = []
    for pod, (start, user, func, cpu, mem) in launched.items():
        if pod not in last_end:
            count["sandboxes with no request in the day's table, left out"] += 1
            continue
        life = int((last_end[pod] - start + keep) * NS)
        if life < MIN_LIFE_NS:
            count["sandboxes whose last request ended before their cold start's time, run for 1 ms"] += 1
            life = MIN_LIFE_NS
        sandboxes.append((start, pod, user, func, life, cpu, mem))
    sandboxes.sort()
    owners = {}
    for _, _, user, *_ in sandboxes:
        owners.setdefault(user, len(owners))
    t0 = sandboxes[0][0]
    shapes = collections.Counter((cpu, mem) for *_, cpu, mem in sandboxes)
    out = [
        "# source: Huawei Cloud public serverless traces 2025, Region 5, day "
        f"{day}: A. Joosen et al., \"Serverless Cold Starts and Where to Find Them\", EuroSys 2025, "
        "doi:10.1145/3689031.3696073; https://github.com/sir-lab/data-release; licensed CC BY 4.0 "
        "(https://creativecommons.org/licenses/by/4.0/). Converted by bench/sched/huawei_trace.py.",
        *(f"# raw {what}: {url}, sha256 {digest}" for what, (url, digest) in SOURCES.items()),
        "# sandboxes: Huawei's serverless pods (possibly containers), taken as microVM requests by "
        "shape: launch, lifetime, CPU, memory, owner",
        f"# {len(launched)} pods launched by cold starts on day {day}; {len(sandboxes)} sandboxes traced; "
        f"{len(owners)} owners (userID), each of weight 1; "
        f"{len({f for *_, f, _, _, _ in sandboxes})} functions",
        f"# lifetime: cold start to the end of the pod's last request of the day, plus {keep_alive_s} s "
        "keep-alive (Huawei's pods keep alive 60 s after each request, Joosen et al. §2)",
        *(f"# {n}: {count[n]}" for n in sorted(count)),
        "# shapes (millicores, MB): " + ", ".join(f"{c}/{m}: {n}" for (c, m), n in shapes.most_common()),
        f"# nodes: {nodes} x EC2 m5d.metal (48 cores, 384 GiB; Agache et al., NSDI 2020 §6)",
    ]
    out += [f"node,{M5D_METAL[0]},{M5D_METAL[1]},0,0"] * nodes
    out += ["owner,1"] * len(owners)
    for start, _pod, user, _func, life, cpu, mem in sandboxes:
        out.append(f"leg,{int((start - t0) * NS)},{life},{cpu},{mem * MIB},0,plain,{owners[user]},0,-,-")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cold_starts_zip")
    ap.add_argument("requests_zip")
    ap.add_argument("out")
    ap.add_argument("--day", type=int, default=30)
    ap.add_argument("--keep-alive-s", type=int, default=0)
    ap.add_argument("--nodes", type=int, default=10)
    a = ap.parse_args()
    try:
        if not 0 <= a.day <= 30 or a.keep_alive_s < 0 or a.nodes < 1:
            raise Refused("--day is 0-30, --keep-alive-s at least 0, --nodes at least 1")
        text = convert(a.cold_starts_zip, a.requests_zip, a.day, a.keep_alive_s, a.nodes)
    except (Refused, KeyError, ValueError, UnicodeDecodeError, zipfile.BadZipFile, OSError) as e:
        print(f"huawei_trace: refused: {type(e).__name__}: {e}", file=sys.stderr)
        return 1
    with open(a.out, "w") as fh:
        fh.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
