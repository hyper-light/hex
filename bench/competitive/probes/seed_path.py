#!/usr/bin/env python3
"""bench/competitive/probes: seed_path_inside.sh's reader of its sched trace (sched_waking, sched_process_fork and
sched_switch for the processes named hex). For each init a seed clones: from that seed's first wakeup by the node
agent (`seam_oci`) since its last clone, the node's request arriving on its socket, to the init's birth (the fork event
the seed's clone3 fires, after copy_process) and to its first run (the first switch to it). A seed is the process that
forks; an init forks nothing before it execs. A wakeup by any other task (a signal, the seed's own answer blocking)
starts no interval.

  seed_path.py <trace.txt>
"""
import math
import re
import sys

LINE = re.compile(r'^\s*(.*?)-(\d+)\s+\[\d+\]\s+\S+\s+([0-9.]+): (\w+): (.*)$')


def percentile(xs, q):
    """The nearest-rank percentile of sorted `xs`."""
    return xs[min(len(xs) - 1, max(0, math.ceil(q / 100 * len(xs)) - 1))]


def summary(what, xs):
    xs = sorted(xs)
    return (f"{what}, ms: n={len(xs)} p50={percentile(xs, 50):.3f} p90={percentile(xs, 90):.3f} "
            f"p99={percentile(xs, 99):.3f} max={xs[-1]:.3f}")


def main():
    woken = {}     # seed pid -> the agent's first wakeup of it since its last clone
    born = {}      # init pid -> (its request's arrival, its birth)
    to_birth, to_run, seeds = [], [], set()
    for line in open(sys.argv[1], errors='replace'):
        m = LINE.match(line)
        if not m:
            continue
        waker, ts, event, rest = m.group(1).strip(), float(m.group(3)), m.group(4), m.group(5)
        if event == 'sched_waking':
            w = re.search(r'\bpid=(\d+)', rest)
            if w and waker == 'seam_oci':
                woken.setdefault(int(w.group(1)), ts)
        elif event == 'sched_process_fork':
            f = re.search(r'\bpid=(\d+) child_comm=\S+ child_pid=(\d+)', rest)
            if f:
                seed, child = int(f.group(1)), int(f.group(2))
                start = woken.pop(seed, None)
                if start is not None:
                    to_birth.append((ts - start) * 1e3)
                    born[child] = start
                    seeds.add(seed)
        elif event == 'sched_switch':
            s = re.search(r'next_pid=(\d+)', rest)
            if s and int(s.group(1)) in born:
                to_run.append((ts - born.pop(int(s.group(1)))) * 1e3)
    if not to_birth:
        print("the seed's clone of an init: no intervals in the trace")
        sys.exit(1)
    print(summary("the seed's clone of an init, its request's arrival to the init's birth", to_birth)
          + f", by {len(seeds)} seeds")
    if to_run:
        print(summary("the same to the init's first run", to_run))


main()
