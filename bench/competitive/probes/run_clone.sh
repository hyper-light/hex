#!/usr/bin/env bash
# bench/competitive/probes: what a process box's run costs in cgroup_mutex, the host-wide lock (runtime design goal 7,
# decision 7). Runs run_clone_inside.sh in a privileged container like create_serial.sh's (bpftrace isn't on the host),
# on the seam_box binary given, copied into the run's directory, at concurrency 1 (seam_box's latency phase) and
# 32 (its throughput phase at 32 alone): each clone's span from cgroup_can_fork to cgroup_post_fork's return, which
# CLONE_INTO_CGROUP holds cgroup_mutex across (Linux v6.19 kernel/cgroup/cgroup.c, cgroup_css_set_fork to
# cgroup_css_set_put_fork); each of the runtime's other takers of the lock timed whole (cgroup_mkdir, cgroup_rmdir,
# cgroup_kill_write); and each wait for the lock alone (lock:contention_begin to contention_end), the runtime's and
# everyone else's. run_clone.py reads them a lifecycle. On fedora, under the timed lock, on CPUs 4-15:
#   flock -w 3600 ~/.cache/hex-t7/fedora-timed.lock run_clone.sh <seam_box release binary>
# BENCH_MEMORY and BENCH_OOM set the container's --memory and --oom-score-adj on a shared host. N (2000) runs at 1,
# LIFECYCLES (20000) at 32.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
OUT="$BASE/out/run-clone-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT/bin"
cp "$(realpath "${1:?seam_box release binary}")" "$OUT/bin/seam_box"
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-15}"
  echo "load at start: $(cat /proc/loadavg)"
  echo "container: --memory ${BENCH_MEMORY:-8g}, --oom-score-adj ${BENCH_OOM:-none}"
  echo "containers at start: $(docker ps --format '{{.Names}}' | paste -sd ' ' - | cut -c1-400)"
  echo "seam_box: sha256 $(sha256sum "$OUT/bin/seam_box" | cut -c1-16)"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
} > "$OUT/env.txt"
NAME=hexbench-runclone-$$
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
timeout 2400 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-15}" \
  --memory="${BENCH_MEMORY:-8g}" ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  --privileged --security-opt label=disable --cgroupns=private --tmpfs /run:rw,exec,nosuid,size=1g \
  -e N="${N:-2000}" -e LIFECYCLES="${LIFECYCLES:-20000}" \
  -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/run_clone_inside.sh 2>&1 | tee "$OUT/probe.out"
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
python3 -I "$HERE/run_clone.py" "$OUT" | tee "$OUT/summary.txt"
echo "results: $OUT"
