#!/usr/bin/env python3
"""bench/competitive/probes: seed_path_inside.sh's reader of its sched trace (sched_waking and sched_process_fork for
the processes named hex). For each fork a seed's clone3 fires, the time from that seed's first wakeup after its last
clone (the node's request arriving on its socket) to the init's birth: the seed's clone of an init, request to child.
A seed is the process that forks; an init forks nothing before it execs, so its wakeups end no interval.

  seed_path.py <trace.txt>
"""
import math
import re
import sys

LINE = re.compile(r'^\s*\S+?-(\d+)\s+\[\d+\]\s+\S+\s+([0-9.]+): (\w+): (.*)$')


def percentile(xs, q):
    """The nearest-rank percentile of sorted `xs`."""
    return xs[min(len(xs) - 1, max(0, math.ceil(q / 100 * len(xs)) - 1))]


def main():
    woken = {}     # seed pid -> its first wakeup since its last clone
    intervals, seeds = [], set()
    for line in open(sys.argv[1], errors='replace'):
        m = LINE.match(line)
        if not m:
            continue
        ts, event, rest = float(m.group(2)), m.group(3), m.group(4)
        if event == 'sched_waking':
            w = re.search(r'\bpid=(\d+)', rest)
            if w:
                woken.setdefault(int(w.group(1)), ts)
        elif event == 'sched_process_fork':
            f = re.search(r'\bpid=(\d+) child_comm=\S+ child_pid=(\d+)', rest)
            if f:
                seed = int(f.group(1))
                start = woken.pop(seed, None)
                if start is not None:
                    intervals.append((ts - start) * 1e3)
                    seeds.add(seed)
    if not intervals:
        print("the seed's clone of an init: no intervals in the trace")
        sys.exit(1)
    xs = sorted(intervals)
    print(f"the seed's clone of an init, its request's arrival to the init's birth, ms: n={len(xs)} "
          f"p50={percentile(xs, 50):.3f} p90={percentile(xs, 90):.3f} p99={percentile(xs, 99):.3f} "
          f"max={xs[-1]:.3f}, by {len(seeds)} seeds")


main()
