"""bench/competitive/probes: what an idle pooled network namespace holds, and how fast one thread makes them
(netns_pool.md, goal 4 plan step 1). Runs inside the bench container as root, with netns_pool built at
/run/netns_pool and the memory controller enabled for /sys/fs/cgroup's children (netns_pool_inside.sh).

  netns_pool.py <out.json> <rounds> <n>

Each round starts netns_pool in a cgroup of its own, reads what the cgroup holds once it is still (cgmem.py's
quiet_snapshot and held: memory.current less file cache, so what is charged and stays), has the probe make n
namespaces and hold them, and reads it again; what the cgroup holds per namespace is the rise over n, with its
kernel parts (memory.stat's slab, percpu and the rest). Beside it, the host's /proc/meminfo before and after:
what the kernel spends system-wide, uncharged parts included, which other work on a shared host moves too. And
each make's time, from which one thread's rate follows. Prints one summary line a round and writes every number
to <out.json>."""
import json
import os
import statistics
import subprocess
import sys
import time

sys.path.insert(0, "/opt/lab")
import cgmem  # noqa: E402

MEMINFO = ("MemAvailable", "Slab", "SReclaimable", "SUnreclaim", "Percpu", "KernelStack", "VmallocUsed", "PageTables")


def meminfo():
    out = {}
    with open("/proc/meminfo") as f:
        for line in f:
            name, _, rest = line.partition(":")
            if name in MEMINFO:
                out[name] = int(rest.split()[0]) * 1024
    return out


def pct(sorted_values, q):
    return sorted_values[min(len(sorted_values) - 1, int(q * len(sorted_values)))]


def round_once(k, n):
    cg = f"/sys/fs/cgroup/netnspool-{os.getpid()}-{k}"
    os.mkdir(cg)
    probe = subprocess.Popen(["/run/netns_pool"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    try:
        with open(os.path.join(cg, "cgroup.procs"), "w") as f:
            f.write(str(probe.pid))
        base_snap, base_quiet = cgmem.quiet_snapshot(cg)
        base_info = meminfo()
        t0 = time.monotonic()
        probe.stdin.write(f"make {n}\n")
        probe.stdin.flush()
        answer = json.loads(probe.stdout.readline())
        wall = time.monotonic() - t0
        peak_snap, peak_quiet = cgmem.quiet_snapshot(cg)
        peak_info = meminfo()
        probe.stdin.write("exit\n")
        probe.stdin.flush()
        probe.wait(timeout=60)
    finally:
        if probe.poll() is None:
            probe.kill()
            probe.wait()
    rise = cgmem.rise(cgmem.breakdown(peak_snap), cgmem.breakdown(base_snap))
    ns = sorted(answer["ns"])
    out = {
        "n": n,
        "made": answer["made"],
        "make_ms": {"p50": statistics.median(ns) / 1e6, "p90": pct(ns, 0.9) / 1e6, "p99": pct(ns, 0.99) / 1e6,
                    "max": ns[-1] / 1e6, "mean": statistics.fmean(ns) / 1e6},
        "rate_per_s": n / wall,
        "cgroup_per_ns_bytes": {k: v / n for k, v in rise.items() if isinstance(v, int)},
        "meminfo_per_ns_bytes": {k: (peak_info[k] - base_info[k]) / n for k in MEMINFO if k in base_info},
        "quiet": {"base": base_quiet, "peak": peak_quiet},
        "reasons": rise["reasons"],
    }
    # The cgroup goes once its probe has: its memory moves to the parent, and the namespaces it held go with the
    # kernel's deferred cleanup.
    cgmem.settle([cg], 10)
    try:
        os.rmdir(cg)
    except OSError as e:
        out["rmdir"] = str(e)
    return out


def main():
    out_path, rounds, n = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    results = []
    for k in range(1, rounds + 1):
        r = round_once(k, n)
        results.append(r)
        c, m, t = r["cgroup_per_ns_bytes"], r["meminfo_per_ns_bytes"], r["make_ms"]
        print(f"round {k}: {r['made']} namespaces held; make p50 {t['p50']:.3f} ms, p99 {t['p99']:.3f} ms, "
              f"one thread {r['rate_per_s']:.0f}/s; the cgroup holds {c.get('held', 0) / 1024:.1f} KiB each "
              f"(kernel {c.get('kernel', 0) / 1024:.1f}: slab {c.get('slab', 0) / 1024:.1f}, "
              f"percpu {c.get('percpu', 0) / 1024:.1f}); the host's Slab {m.get('Slab', 0) / 1024:+.1f} KiB, "
              f"Percpu {m.get('Percpu', 0) / 1024:+.1f} KiB, MemAvailable {m.get('MemAvailable', 0) / 1024:+.1f} KiB each")
    with open(out_path, "w") as f:
        json.dump({"rounds": results}, f, indent=1)


if __name__ == "__main__":
    main()
