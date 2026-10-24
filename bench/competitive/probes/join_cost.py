#!/usr/bin/env python3
"""bench/competitive/probes: what one cgroup v2 migration (a write to cgroup.procs) costs, by how long
it comes after the one before. Runs as root inside join_cost.sh's container (the OCI lab's shape).

  join_cost.py <out.json> <n per gap>

The kernel serializes migrations on cgroup_threadgroup_rwsem, a percpu rwsem (kernel/cgroup/cgroup.c,
cgroup_procs_write_start). Its writer enters through rcu_sync, which waits a full RCU grace period
unless another writer passed one within the last grace period (kernel/rcu/sync.c, rcu_sync_enter). So
a migration that comes soon after another is cheap, and one that comes after a quiet spell costs a
grace period. `hex oci create` joins its container to its cgroup by such a write; `hex box run` does
too, in a tight loop in its bench. For each gap, n migrations of a sleeping process between two sibling
cgroups, each timed alone.
"""
import json
import os
import subprocess
import sys
import time

CG = "/sys/fs/cgroup"
GAPS_S = (0.0, 0.001, 0.005, 0.02, 0.05, 0.1, 0.2)


def pct(s, q):
    s = sorted(s)
    return s[min(len(s) - 1, int(q * len(s)))]


def main():
    out, n = sys.argv[1], int(sys.argv[2])
    a, b = f"{CG}/jc-a", f"{CG}/jc-b"
    os.mkdir(a)
    os.mkdir(b)
    p = subprocess.Popen(["/usr/bin/sleep", "infinity"], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
    try:
        with open(f"{a}/cgroup.procs", "w") as f:
            f.write(str(p.pid))
        res = {}
        for gap in GAPS_S:
            ms = []
            for i in range(n):
                time.sleep(gap)
                fd = os.open(f"{b if i % 2 == 0 else a}/cgroup.procs", os.O_WRONLY)
                t0 = time.perf_counter_ns()
                os.write(fd, str(p.pid).encode())
                ms.append((time.perf_counter_ns() - t0) / 1e6)
                os.close(fd)
            res[str(gap)] = {"n": n, "p50_ms": pct(ms, 0.5), "p90_ms": pct(ms, 0.9), "p99_ms": pct(ms, 0.99),
                             "max_ms": max(ms), "min_ms": min(ms)}
            print(f"gap {gap * 1e3:6.1f} ms: p50 {res[str(gap)]['p50_ms']:7.3f} ms, p90 {res[str(gap)]['p90_ms']:7.3f}, "
                  f"max {res[str(gap)]['max_ms']:7.3f}", flush=True)
    finally:
        p.kill()
        p.wait()
        for d in (a, b):
            os.rmdir(d)
    with open(out, "w") as f:
        json.dump(res, f)


if __name__ == "__main__":
    main()
