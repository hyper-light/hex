"""HEX-GUEST-39's before/after: per arm, the node's making and its first create, p50 / p99 / max by nearest rank,
and the median of the pairs' differences (B - A) in what a user waits for after an upgrade: the first create.
   summary.py OUT  (OUT/turns.txt from pair.sh)"""
import re
import statistics
import sys

turns = {}
failed = 0
for line in open(f"{sys.argv[1]}/turns.txt"):
    m = re.match(r"pair (\d+) ([AB]) FIRST make_ms=([\d.]+) create_ms=([\d.]+) delete_ms=([\d.]+)", line)
    if not m:
        failed += 1
        continue
    turns.setdefault(int(m[1]), {})[m[2]] = (float(m[3]), float(m[4]))


def at(xs, p):
    xs = sorted(xs)
    return xs[max(0, -(-p * len(xs) // 100) - 1)]


pairs = [t for t in turns.values() if "A" in t and "B" in t]
print(f"pairs {len(pairs)}, turns failed {failed}")
for arm in "AB":
    for k, what in ((0, "make"), (1, "first create")):
        xs = [t[arm][k] for t in pairs]
        print(f"{arm} {what}: p50 {at(xs, 50):.1f} p99 {at(xs, 99):.1f} max {max(xs):.1f} ms")
d = [t["B"][1] - t["A"][1] for t in pairs]
print(f"first create B - A: median {statistics.median(d):+.1f} ms")
