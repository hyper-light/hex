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
  - class: what runs it, an OCI container, a Firecracker microVM or a Shards microVM, the three Hex
    places. The trace records none, so each owner's sandboxes are one class: a runtime is chosen per
    workload, not per launch. Owners go to classes so each class has a third of the day's launches:
    the owner with the most launches first, each to the class with the fewest so far, ties OCI, then
    Firecracker, then Shards.
  - overhead: what each class's runtime costs its node beside the sandbox's own memory, an
    `overhead` row each: OCI 0.42 MiB (Hex's oci class, bench/competitive/RESULT.md:95), Firecracker
    5 MiB (its specification's bound on the VMM's memory, tested on every pull request, and Agache et
    al., NSDI 2020: "memory overhead of less than 5MB per container";
    docs/research/runtime/substrates-and-sandboxes.md §3.3), Shards 5 MiB as a stand-in for its own
    figure, which no source here states.
  - nodes: Hex's own fleet's hosts, --copies times each, from kiln's runner facts (2026-11-05,
    bench/sched/kiln_workload.md): each host's threads x 1000 millicores, its memory, its GPUs with
    memory of their own, no background load, and the classes its OS runs. Linux runs all three;
    x86_64 Windows runs OCI containers (Hex's oci class over HCS) and Shards microVMs (WHP); arm64
    macOS runs only Shards microVMs (Hypervisor.framework). Shards' hosts are its own: KVM on Linux,
    Hypervisor.framework on arm64 macOS, WHP on x86_64 Windows (hyper-light/shards a0cced87,
    crates/api/src/lib.rs:172-178).

  huawei_trace.py COLD_STARTS_ZIP REQUESTS_ZIP OUT [--day 30] [--keep-alive-s 0] [--copies 5]
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
CLASSES = ("oci", "firecracker", "shards")
OVERHEAD = {"oci": 440_402, "firecracker": 5 * MIB, "shards": 5 * MIB}
RUNS = {"linux": "oci+firecracker+shards", "windows": "oci+shards", "macos": "shards"}
# Hex's fleet: (runner, threads, memory bytes, GPUs with memory of their own, OS), kiln's facts.
FLEET = (
    ("adacachy", 32, 131_986_903_040, 1, "linux"),
    ("kiln-a094a12d.attlocal.net", 32, 67_128_643_584, 0, "linux"),
    ("jupiter", 32, 132_766_306_304, 0, "windows"),
    ("adalundhe-macbook-pro", 18, 68_719_476_736, 0, "macos"),
    ("adas-mac-studio-2.local", 14, 38_654_705_664, 0, "macos"),
    ("adas-mac-studio-3.local", 14, 38_654_705_664, 0, "macos"),
    ("adas-mac-studio.local", 14, 38_654_705_664, 0, "macos"),
    ("adas-mac-studio.local-2", 32, 103_079_215_104, 0, "macos"),
    ("adas-macbook-pro.local", 15, 51_539_607_552, 0, "macos"),
    ("e30189ea29182644fe5a4b2e06c7dac2", 18, 137_438_953_472, 0, "macos"),
)
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


def classes_by_owner(sandboxes, owners):
    launches = collections.Counter(owners[user] for _, _, user, *_ in sandboxes)
    got = dict.fromkeys(CLASSES, 0)
    out = {}
    for owner, n in sorted(launches.items(), key=lambda x: (-x[1], x[0])):
        cls = min(CLASSES, key=lambda c: (got[c], CLASSES.index(c)))
        out[owner] = cls
        got[cls] += n
    return out, got


def convert(cold_zip, req_zip, day, keep_alive_s, copies):
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
    classes, launches = classes_by_owner(sandboxes, owners)
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
        "# classes by owner, a third of the launches each: "
        + ", ".join(f"{c} {launches[c]}" for c in CLASSES),
        "# overheads: oci 0.42 MiB (Hex, bench/competitive/RESULT.md:95), firecracker 5 MiB (its specification "
        "and Agache et al. NSDI 2020, substrates-and-sandboxes.md §3.3), shards 5 MiB (a stand-in: no source "
        "states Shards' own)",
        f"# nodes: Hex's fleet's {len(FLEET)} hosts x {copies} (kiln's runner facts, 2026-11-05): "
        + ", ".join(f"{name} ({os})" for name, *_, os in FLEET),
    ]
    for _ in range(copies):
        out += [f"node,{t * 1000},{mem},{gpus},0,{RUNS[os]}" for _, t, mem, gpus, os in FLEET]
    out += ["owner,1"] * len(owners)
    out += [f"overhead,{c},{OVERHEAD[c]}" for c in CLASSES]
    for start, _pod, user, _func, life, cpu, mem in sandboxes:
        owner = owners[user]
        out.append(f"leg,{int((start - t0) * NS)},{life},{cpu},{mem * MIB},0,plain,{owner},0,-,-,{classes[owner]}")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cold_starts_zip")
    ap.add_argument("requests_zip")
    ap.add_argument("out")
    ap.add_argument("--day", type=int, default=30)
    ap.add_argument("--keep-alive-s", type=int, default=0)
    ap.add_argument("--copies", type=int, default=5)
    a = ap.parse_args()
    try:
        if not 0 <= a.day <= 30 or a.keep_alive_s < 0 or a.copies < 1:
            raise Refused("--day is 0-30, --keep-alive-s at least 0, --copies at least 1")
        text = convert(a.cold_starts_zip, a.requests_zip, a.day, a.keep_alive_s, a.copies)
    except (Refused, KeyError, ValueError, UnicodeDecodeError, zipfile.BadZipFile, OSError) as e:
        print(f"huawei_trace: refused: {type(e).__name__}: {e}", file=sys.stderr)
        return 1
    with open(a.out, "w") as fh:
        fh.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
