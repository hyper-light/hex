#!/usr/bin/env python3
"""bench/competitive/probes: run_clone.sh's reader. Per run of `true` (each run clones its program into its box's
cgroup once, so the clones into a cgroup count the runs, warm-up included), at concurrency 1 and 32: the clone's span
that holds cgroup_mutex, each other taker of the lock (its wait and hold together), and the waits for the lock, the
runtime's and everyone else's, in microseconds.
  run_clone.py <run directory>
"""
import os
import re
import sys

STAT = re.compile(r"^@(\w+)\[(.+?)\]: count (\d+), average (\d+), total (\d+)$")


def stats(path):
    out = {}
    with open(path) as f:
        for line in f:
            m = STAT.match(line.strip())
            if m:
                name, key, count, _, total = m.groups()
                out[(name, key)] = (int(count), int(total))
    return out


def main():
    run = sys.argv[1]
    for label in ("c1", "c32"):
        path = os.path.join(run, f"bt.{label}.txt")
        if not os.path.exists(path):
            print(f"{label}: no bpftrace output")
            continue
        s = stats(path)
        runs = s.get(("clone", "1"), (0, 0))[0]
        if runs == 0:
            print(f"{label}: no clone into a cgroup seen")
            continue
        print(f"{label}: {runs} runs (clones into a cgroup)")
        for (name, key), (count, total) in sorted(s.items()):
            what = {("clone", "1"): "clone into a cgroup (cgroup_mutex held across)",
                    ("clone", "0"): "clone not into a cgroup",
                    ("cgm_wait", "1"): "cgroup_mutex waits, the runtime's",
                    ("cgm_wait", "0"): "cgroup_mutex waits, everyone else's"}.get((name, key), f"{key} (whole)")
            print(f"  {what}: {count / runs:.2f} a run, {total / runs / 1e3:.3f} us a run, "
                  f"{total / count / 1e3:.3f} us each")


if __name__ == "__main__":
    main()
