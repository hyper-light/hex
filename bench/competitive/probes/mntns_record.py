#!/usr/bin/env python3
"""Goal 6's record of the mount-namespace ways, from mntns_record.sh's cells (its `exits`: time, cell, exit, out dir).

Each cell is one way (copy, tree, empty) at one load (busy loops) in one round. From each: the untraced phase's create
p50 (untraced.c8.json, the record's), the traced phase's part 19 (mounts.c8.summary.json) and the mount-namespace counts,
which must show every create made the way the cell asked (non-vacuity) or the run is refused (exit 3). Prints a table
of the cells, each new way against the copy in the same round and load, and the three record lines for
bench/competitive/probes/mntns_default.txt: each way's create p50 in ms, the median of its cells', then the run and
the commit.
Run: python3 -I mntns_record.py EXITS RUN REV MODE
"""
import json
import os
import statistics
import sys

WAYS = ("copy", "tree", "empty")


def pct(values, q):
    v = sorted(values)
    return v[min(len(v) - 1, int(q * len(v)))]


def main(argv):
    exits, run, rev, mode = argv
    cells = {}
    for line in open(exits):
        fields = line.split()
        if len(fields) < 4:
            print(f"refusing: a cell ended with no out dir: {line.strip()}")
            return 3
        _, name, rc, out = fields[:4]
        way, loops, rnd = name.split("-")
        if rc != "0":
            print(f"refusing: cell {name} exited {rc}")
            return 3
        untraced = json.load(open(os.path.join(out, "untraced.c8.json")))["phases"]["throughput"]
        traced = json.load(open(os.path.join(out, "mounts.c8.json")))["phases"]["throughput"]
        summary = json.load(open(os.path.join(out, "mounts.c8.summary.json")))
        for phase in (untraced, traced):
            counts = phase["mount_namespaces"]
            if counts.get(way, 0) == 0 or any(counts.get(w, 0) for w in WAYS if w != way):
                print(f"refusing: cell {name} asked {way}, its creates took {counts}")
                return 3
        created = untraced["8"]["create_ms"]
        cells[(way, int(loops[1:]), int(rnd[1:]))] = {
            "p50": pct(created, 0.5), "p99": pct(created, 0.99),
            "part19": (summary["span_p50_us"], summary["span_p99_us"], summary.get("span_unpreempted_p99_us")),
        }
    print(f"# {run} at {rev} ({mode}{'' if mode == 'host' else ', indicative only'})")
    print("| way | loops | round | create p50, ms | create p99, ms | part 19 p50 / p99 / unpreempted p99, us |")
    print("|---|---|---|---|---|---|")
    for (way, loops, rnd), c in sorted(cells.items(), key=lambda kv: (kv[0][1], kv[0][2], WAYS.index(kv[0][0]))):
        p = c["part19"]
        print(f"| {way} | {loops} | {rnd} | {c['p50']:.3f} | {c['p99']:.3f} | {p[0]} / {p[1]} / {p[2]} |")
    for new in ("tree", "empty"):
        pairs = [(c["p50"], cells[("copy", L, r)]["p50"]) for (w, L, r), c in cells.items()
                 if w == new and ("copy", L, r) in cells]
        below = sum(1 for n, c in pairs if n < c)
        print(f"- {new} below copy in {below} of {len(pairs)} pairs (create p50, same round and load)")
    print("# record lines for mntns_default.txt:")
    for way in WAYS:
        p50s = [c["p50"] for (w, _, _), c in cells.items() if w == way]
        if not p50s:
            print(f"refusing: no cell of {way}")
            return 3
        print(f"{way} {statistics.median(p50s):.3f} {run} {rev}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
