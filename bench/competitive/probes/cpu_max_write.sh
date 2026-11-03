#!/usr/bin/env bash
# bench/competitive/probes: what a reused cgroup's bound writes cost, and a read of cpu.max (goal 5 extended, step 6).
# A cpu.max write takes the host-wide cfs_constraints_mutex and walks every task group on the host (Linux v6.19
# kernel/sched/core.c:9461-9462, :9624); cpu.weight and pids.max writes take no host-wide lock (research f25, f26), and
# cpu.max's read takes none (cpu_max_show, :10104). Times N (2000) open-write-close cycles of each file and N
# open-read-close cycles of cpu.max, in a cgroup of its own under this process's cgroup, which must be delegated (cpu
# and pids are threaded controllers, so a cgroup holding this script can hand them down), pinned to CPUSET (4-15). A
# microbenchmark, not a pass: the load, the co-tenants and the host's cgroup count are recorded beside it.
#   systemd-run --user --scope -p Delegate=yes bash bench/competitive/probes/cpu_max_write.sh
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
N=${N:-2000}; CPUSET=${CPUSET:-4-15}
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
OUT="$BASE/out/cpu-max-write-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT"
own=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)
echo "+cpu +pids" > "$own/cgroup.subtree_control"
T="$own/cpu-max-write-$$"; mkdir "$T"; trap 'rmdir "${T:?}" 2>/dev/null || true' EXIT
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to $CPUSET"
  echo "load at start: $(cat /proc/loadavg)"
  echo "cgroups on the host: $(find /sys/fs/cgroup -mindepth 1 -type d 2>/dev/null | wc -l)"
  echo "containers at start: $(docker ps --format '{{.Names}}' 2>/dev/null | paste -sd ' ' - | cut -c1-400)"
  echo "harness: bench/competitive at $(git -C "$HERE" rev-parse --short HEAD)"
} > "$OUT/env.txt"
taskset -c "$CPUSET" python3 -I - "$T" "$N" > "$OUT/result.txt" <<'EOF'
import os, sys, time
d, n = sys.argv[1], int(sys.argv[2])
def timed(f, value=None):
    p = os.path.join(d, f); ts = []
    for _ in range(n):
        t0 = time.perf_counter_ns()
        if value is None:
            fd = os.open(p, os.O_RDONLY); os.read(fd, 64); os.close(fd)
        else:
            fd = os.open(p, os.O_WRONLY); os.write(fd, value); os.close(fd)
        ts.append(time.perf_counter_ns() - t0)
    ts.sort()
    return "n %d p50 %.1f us p90 %.1f p99 %.1f max %.1f" % (
        n, ts[n // 2] / 1e3, ts[n * 9 // 10] / 1e3, ts[n * 99 // 100] / 1e3, ts[-1] / 1e3)
print("cpu.max write 'max 100000':", timed("cpu.max", b"max 100000"))
print("cpu.max read              :", timed("cpu.max"))
print("cpu.weight write '100'    :", timed("cpu.weight", b"100"))
print("pids.max write 'max'      :", timed("pids.max", b"max"))
EOF
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
cat "$OUT/result.txt"; echo "results: $OUT"
