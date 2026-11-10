"""Reads waits_census.sh's run: each census file's lines `name nanos`, by wait name, as a table of p50, p99, p99.9, max
and n in milliseconds, one table an arm. A census cut at its bound says so. A row seeds its wait (its max) only with
n of at least SEEDABLE: fewer samples than that can't have seen the p99.9, so the tail the seed must cover is unknown.
A completed wire typed refusal, `name !nanos`, is retained separately, excluded from answer statistics.
An answered wire row alone does not prove workload success; physical/per-call provenance remains required.
Legacy unmarked rows keep their original interpretation; historical refusals cannot be inferred.
A wait that ran out is no sample (Karn's point), but its line, `name >nanos`, says its slowest is at least the bound it
ran out at, which a row's record is never under, however few its answers ([`seed`]): a censored row's patience doubles
each census until nothing runs out (decision 9; the lead, 2026-11-09)."""
import json
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
    """A census file's answers, run-outs and typed refusals, each by wait name in nanoseconds, and the line saying it was cut, if any."""
    waits, ran_out, refused, cut = {}, {}, {}, None
    for line in path.read_text().splitlines():
        if line.startswith("census full at"):
            cut = line
            continue
        name, _, ns = line.partition(" ")
        if ns.isdigit():
            waits.setdefault(name, []).append(int(ns))
        elif ns.startswith(">") and ns[1:].isdigit():
            ran_out.setdefault(name, []).append(int(ns[1:]))
        elif ns.startswith("!") and ns[1:].isdigit():
            refused.setdefault(name, []).append(int(ns[1:]))
    return waits, ran_out, refused, cut


def seed(answers, ran_out):
    """A row's record in nanoseconds, or None: the largest bound it ran out at, and its answers' max where there are
    SEEDABLE of them, whichever is larger. A run-out counts however few the answers: the bound held less than the wait."""
    candidates = list(ran_out)
    if len(answers) >= SEEDABLE:
        candidates.append(max(answers))
    return max(candidates) if candidates else None


def seam_rows(out, place):
    """The rows seam_oci's own timings give: its lifecycle phase's exec of `true`, to the program's end
    (`lifecycle.exec`, the conformance suites' seed), in nanoseconds as the census keeps them."""
    seam = out / f"seam-{place}.json"
    if not seam.exists():
        return {}
    phases = json.loads(seam.read_text()).get("phases", {})
    execs = phases.get("lifecycle", {}).get("ms", {}).get("exec", [])
    return {"lifecycle.exec": [round(ms * 1e6) for ms in execs]} if execs else {}


def main(out):
    out = Path(out)
    print(Path(out / "env.txt").read_text())
    for census in sorted(out.glob("census-*.txt")):
        place = census.stem.removeprefix("census-")
        waits, ran_out, refused, cut = read(census)
        waits.update(seam_rows(out, place))
        print(f"## Arm: {place}\n")
        if cut:
            print(f"**Cut:** {cut}; raise HEX_WAITS_CENSUS_MAX.\n")
        print("| wait | p50 (ms) | p99 | p99.9 | max | n | ran out | refused | seed |")
        print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |")
        for name in sorted(set(waits) | set(ran_out) | set(refused)):
            ns, out_at = sorted(waits.get(name, [])), ran_out.get(name, [])
            ms = [quantile(ns, q) / 1e6 for q in (0.5, 0.99, 0.999)] + [ns[-1] / 1e6] if ns else []
            row = " | ".join(f"{v:.3f}" for v in ms) if ms else "n/a | n/a | n/a | n/a"
            outs = f"{len(out_at)} at {max(out_at) / 1e6:.3f} ms" if out_at else "0"
            refusals = refused.get(name, [])
            refused_at = f"{len(refusals)} at {max(refusals) / 1e6:.3f} ms" if refusals else "0"
            record = seed(ns, out_at)
            if record is None:
                why = f"none: n {len(ns)} < {SEEDABLE:,}"
            else:
                why = f"record {record / 1e6:.3f} ms" + (" (ran out)" if out_at and record == max(out_at) else " (max)")
            print(f"| {name} | {row} | {len(ns)} | {outs} | {refused_at} | {why} |")
        print()


if __name__ == "__main__":
    main(sys.argv[1])
