"""The tables of bench/sched/proximity.md, from the log of `cargo bench -p hex-bench --bench sched_proximity`.

  python3 -I bench/sched/proximity_tables.py LOG

Prints, for the record and each labelled sensitivity run: per node model and kind, how many seeds
proximity cut the completion time's p99 and p50 against blind, the least and mean cuts, and whether
goodput was ever lower; then, for the record, one row a seed: coordinate rounds and final median error,
and each policy's goodput, p50 and p99 completion and mean RTT.
"""

import re
import statistics
import sys

KINDS = ["all", "Oci", "Firecracker", "Shards"]
POLICIES = ["Blind", "Proximity", "Oracle"]


def parse(path):
    runs = {}
    run = fleet = seed = policy = None
    for line in open(path, encoding="utf-8"):
        m = re.match(r"== (.*?): (\w+), seed (\d+): horizon ([\d.]+) s, coordinates (\d+) rounds, median error ([\d.]+)%", line)
        if m:
            run, fleet, seed = m.group(1), m.group(2), int(m.group(3))
            runs.setdefault(run, {})[(fleet, seed)] = {"rounds": int(m.group(5)), "error": float(m.group(6))}
            continue
        m = re.match(r"   (\w+)\s+goodput\s+([\d.]+)/s.*p50\s+([\d.]+) p99\s+([\d.]+) p99\.9.*rtt\s+([\d.]+) ms", line)
        if m and run:
            policy = m.group(1)
            runs[run][(fleet, seed)][(policy, "all")] = tuple(float(x) for x in m.groups()[1:])
            continue
        m = re.match(r"\s+(Oci|Firecracker|Shards)\s+placed\s+\d+ done\s+\d+ goodput\s+([\d.]+)/s p50\s+([\d.]+) p99\s+([\d.]+) ms", line)
        if m and run and policy:
            runs[run][(fleet, seed)][(policy, m.group(1))] = tuple(float(x) for x in m.groups()[1:]) + (0.0,)
    return runs


def cut(blind, other):
    return 100.0 * (1.0 - other / blind)


def summary(name, studies):
    print(f"### {name}\n")
    print("| node model | kind | p99 cut on | least p99 cut | mean p99 cut | p50 cut on | least p50 cut | goodput lower on |")
    print("|---|---|---|---|---|---|---|---|")
    for fleet in sorted({f for f, _ in studies}):
        seeds = sorted(s for f, s in studies if f == fleet)
        for kind in KINDS:
            rows = [studies[(fleet, s)] for s in seeds]
            p99 = [cut(r[("Blind", kind)][2], r[("Proximity", kind)][2]) for r in rows]
            p50 = [cut(r[("Blind", kind)][1], r[("Proximity", kind)][1]) for r in rows]
            lower = sum(r[("Proximity", kind)][0] < r[("Blind", kind)][0] for r in rows)
            n = len(rows)
            print(f"| {fleet} | {kind} | {sum(c > 0 for c in p99)}/{n} | {min(p99):+.3f}% | {statistics.mean(p99):+.2f}% | "
                  f"{sum(c > 0 for c in p50)}/{n} | {min(p50):+.1f}% | {lower}/{n} |")
    errors = sorted(v["error"] for v in studies.values())
    rounds = max(v["rounds"] for v in studies.values())
    print(f"\nFinal median coordinate error: min {errors[0]:.1f}%, median {statistics.median(errors):.1f}%, "
          f"max {errors[-1]:.1f}%; most rounds {rounds}.\n")


def per_seed(studies):
    print("| node model | seed | rounds | final error | " + " | ".join(f"{p} goodput /s, p50 s, p99 s, RTT ms" for p in POLICIES) + " |")
    print("|---|---|---|---|" + "---|" * len(POLICIES))
    for (fleet, seed), v in sorted(studies.items()):
        cells = []
        for p in POLICIES:
            g, p50, p99, rtt = v[(p, "all")]
            cells.append(f"{g:.4f}, {p50 / 1e3:.2f}, {p99 / 1e3:.1f}, {rtt:.1f}")
        print(f"| {fleet} | {seed} | {v['rounds']} | {v['error']:.1f}% | " + " | ".join(cells) + " |")
    print()


def main():
    runs = parse(sys.argv[1])
    for name, studies in runs.items():
        summary(name, studies)
    record = runs.get("RECORD")
    if record:
        print("### RECORD, every seed (all launches)\n")
        per_seed(record)


if __name__ == "__main__":
    main()
