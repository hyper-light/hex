"""HEX-GUEST-22's interleaved A/B, judged on its pairs (pair.sh): for each image, each arm's create, delete and
life (the two together) over the same rounds, round 0 aside, p50 and p99 by nearest rank, and the median of
B - A over the rounds both arms ran.

    python3 -I summary.py OUT
"""

import re
import sys

ROUND = re.compile(r"^round (\d+) (\S+): create ([\d.]+) ms delete ([\d.]+) ms life ([\d.]+) ms$")


def rounds(path):
    out = {}
    for line in open(path):
        m = ROUND.match(line.strip())
        if m and int(m.group(1)) > 0:
            out[(m.group(2), int(m.group(1)))] = tuple(float(m.group(i)) for i in (3, 4, 5))
    return out


def at(sorted_, percent):
    rank = max(1, min(len(sorted_), -(-len(sorted_) * percent // 100)))
    return sorted_[rank - 1]


def main(out):
    a, b = rounds(f"{out}/A.out"), rounds(f"{out}/B.out")
    print(open(f"{out}/arms.txt").read().strip())
    for image in sorted({k[0] for k in a} | {k[0] for k in b}):
        both = sorted(r for (i, r) in a if i == image and (image, r) in b)
        for k, what in enumerate(("create", "delete", "life")):
            av = sorted(a[(image, r)][k] for r in both)
            bv = sorted(b[(image, r)][k] for r in both)
            diff = sorted(b[(image, r)][k] - a[(image, r)][k] for r in both)
            if not both:
                continue
            print(f"{image} {what}: N={len(both)} A p50 {at(av, 50):.1f} p99 {at(av, 99):.1f} | "
                  f"B p50 {at(bv, 50):.1f} p99 {at(bv, 99):.1f} | B-A median {at(diff, 50):+.1f} ms")


if __name__ == "__main__":
    main(sys.argv[1])
