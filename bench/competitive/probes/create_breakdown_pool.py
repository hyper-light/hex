#!/usr/bin/env python3
"""bench/competitive/probes: one A/B estimate from several create-breakdown runs, each a chunk of rounds.

Each run interleaves Hex's seam (`seam.r<n>.json`) and the binary before a change (`before.r<n>.json`) in every round,
so each round gives one paired difference: the round's create p50 through the seam less its create p50 before. The
estimate pools those differences across every run's whole rounds (a round with both files): their mean, its standard
error by a bootstrap over the rounds (B resamples, seeded), and beside it their median and the rounds within, under
and over the window. Each run's own pooled-p50 difference is printed too. Goal 5 extended step 6's third criterion
asks the difference be within 0.02 ms of 0 with that standard error at most 0.01 ms (the lead, 2026-11-03T02:36Z and
03:14Z: chunks of 20 rounds, so a host reset costs one chunk).

  create_breakdown_pool.py [--window MS] [--boot B] <run dir>...
"""
import json
import os
import random
import statistics as st
import sys


def creates(run, kind, n):
    with open(os.path.join(run, f"{kind}.r{n}.json")) as f:
        return json.load(f)["phases"]["lifecycle"]["ms"]["create"]


def whole_rounds(run):
    """The rounds of `run` with both its seam's and its before's result, in order."""
    names = os.listdir(run)
    rounds = set()
    for name in names:
        if name.startswith("seam.r") and name.endswith(".json"):
            digits = name[len("seam.r"):-len(".json")]
            if digits.isdigit() and f"before.r{digits}.json" in names:
                rounds.add(int(digits))
    return sorted(rounds)


def main(argv):
    window, boot, runs = 0.02, 2000, []
    args = iter(argv)
    for a in args:
        if a == "--window":
            window = float(next(args))
        elif a == "--boot":
            boot = int(next(args))
        else:
            runs.append(a)
    if not runs:
        print(__doc__.strip().splitlines()[-1].strip(), file=sys.stderr)
        return 2
    pairs = []
    for run in runs:
        rounds = whole_rounds(run)
        seam = {n: creates(run, "seam", n) for n in rounds}
        before = {n: creates(run, "before", n) for n in rounds}
        diffs = [st.median(seam[n]) - st.median(before[n]) for n in rounds]
        pooled = (st.median([x for n in rounds for x in seam[n]])
                  - st.median([x for n in rounds for x in before[n]])) if rounds else float("nan")
        print(f"{os.path.basename(run.rstrip('/'))}: {len(rounds)} whole rounds, pooled p50 difference "
              f"{pooled:+.4f} ms, its rounds' mean {st.mean(diffs) if diffs else float('nan'):+.4f}")
        pairs += diffs
    if len(pairs) < 2:
        print("refusing: fewer than two whole rounds", file=sys.stderr)
        return 1
    rng = random.Random(7)
    means = [st.mean(rng.choice(pairs) for _ in pairs) for _ in range(boot)]
    within = sum(abs(d) <= window for d in pairs)
    under = sum(d < -window for d in pairs)
    print(f"pooled over {len(runs)} runs, {len(pairs)} rounds: mean paired difference {st.mean(pairs):+.4f} ms, "
          f"bootstrap SE {st.stdev(means):.4f} ({boot} resamples); median {st.median(pairs):+.4f}, "
          f"sd {st.stdev(pairs):.4f}")
    print(f"rounds within {window} ms: {within}; under: {under}; over: {len(pairs) - within - under}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
