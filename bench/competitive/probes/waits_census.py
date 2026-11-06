"""Reads waits_census.sh's run: each census file's lines `name nanos`, by wait name, as a table of p50, p99, p99.9, max
and n in milliseconds, one table an arm. A census cut at its bound says so. A row seeds its wait (its max) only with
n of at least SEEDABLE: fewer samples than that can't have seen the p99.9, so the tail the seed must cover is unknown."""
import math
import sys
from pathlib import Path

# The fewest samples a row seeds from: with n >= 1,000 the nearest-rank p99.9 is an observed sample, not the max.
SEEDABLE = 1_000


def quantile(sorted_ns, q):
    """The nearest-rank quantile q of sorted_ns."""
    rank = max(1, math.ceil(q * len(sorted_ns)))
    return sorted_ns[rank - 1]


def read(path):
    waits, cut = {}, None
    for line in path.read_text().splitlines():
        if line.startswith("census full at"):
            cut = line
            continue
        name, _, ns = line.partition(" ")
        if ns.isdigit():
            waits.setdefault(name, []).append(int(ns))
    return waits, cut


def main(out):
    out = Path(out)
    print(Path(out / "env.txt").read_text())
    for census in sorted(out.glob("census-*.txt")):
        place = census.stem.removeprefix("census-")
        waits, cut = read(census)
        print(f"## Arm: {place}\n")
        if cut:
            print(f"**Cut:** {cut}; raise HEX_WAITS_CENSUS_MAX.\n")
        print("| wait | p50 (ms) | p99 | p99.9 | max | n | seed |")
        print("| --- | ---: | ---: | ---: | ---: | ---: | --- |")
        for name in sorted(waits):
            ns = sorted(waits[name])
            ms = [quantile(ns, q) / 1e6 for q in (0.5, 0.99, 0.999)] + [ns[-1] / 1e6]
            seed = f"max {ms[-1]:.3f} ms" if len(ns) >= SEEDABLE else f"none: n {len(ns)} < {SEEDABLE:,}"
            print(f"| {name} | " + " | ".join(f"{v:.3f}" for v in ms) + f" | {len(ns)} | {seed} |")
        print()


if __name__ == "__main__":
    main(sys.argv[1])
