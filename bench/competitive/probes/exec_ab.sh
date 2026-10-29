#!/usr/bin/env bash
# bench/competitive/probes: exec and start through the Runtime seam for two or more builds of Hex in one run, beside
# libcrun in process (exec_ab.md). Each round runs every build's seam_oci lifecycle phase and libcrun's in a shuffled
# order, in the bench container with host.sh's OCI-pass flags, so the builds meet the host as each other and as the
# libcrun exec they are set against. TRACE=1 adds, after the rounds, a run of each build with the scheduler's events
# traced (exec_ab.py sched reads it). Every binary is copied into the run's directory. On fedora:
#   exec_ab.sh <name>=<hex release binary>:<seam_oci release binary> ...
# Env: N (600), WARMUP (60), ROUNDS (3), TRACE (0), TRACE_N (300), CPUSET (4-31); BENCH_HOST_OK names another host,
# as host.sh takes it. No load gate: the comparisons are within the run, as exec_breakdown.sh's LOAD_GATE=0.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
[ $# -ge 1 ] || { echo "usage: exec_ab.sh <name>=<hex>:<seam_oci> ..." >&2; exit 2; }
case "$(hostname)" in fedora*|"${BENCH_HOST_OK:-__none__}") ;; *) echo "refusing: the bench host is fedora, not $(hostname) (BENCH_HOST_OK=$(hostname) runs it here)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
for f in crun libcrun-driver; do
  [ -x "$BASE/bin/$f" ] || { echo "refusing: no $BASE/bin/$f (host.sh fetches crun and builds the libcrun driver)" >&2; exit 4; }
done
OUT="$BASE/out/exec-ab-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT/bin"
cp "$BASE/bin/crun" "$BASE/bin/libcrun-driver" "$OUT/bin/"
NAMES=()
for arg in "$@"; do
  name=${arg%%=*}; bins=${arg#*=}; hex=${bins%%:*}; seam=${bins#*:}
  [[ "$name" =~ ^[a-z0-9_]+$ ]] || { echo "refusing: build name $name isn't [a-z0-9_]+" >&2; exit 2; }
  [ "$name" != libcrun ] || { echo "refusing: libcrun names the baseline" >&2; exit 2; }
  cp "$(realpath "$hex")" "$OUT/bin/hex.$name"; cp "$(realpath "$seam")" "$OUT/bin/seam_oci.$name"
  NAMES+=("$name")
done
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-31}"
  echo "load at start: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at start, $r: $(head -1 "/proc/pressure/$r")"; done
  for name in "${NAMES[@]}"; do
    echo "build $name: hex sha256 $(sha256sum "$OUT/bin/hex.$name" | cut -c1-16), seam_oci sha256 $(sha256sum "$OUT/bin/seam_oci.$name" | cut -c1-16)"
  done
  echo "libcrun: $("$OUT/bin/libcrun-driver" --version 2>&1 | head -1) (driver sha256 $(sha256sum "$OUT/bin/libcrun-driver" | cut -c1-16))"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
} > "$OUT/env.txt"
NAME=hexbench-execab-$$
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
timeout 7200 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
  --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" \
  -e N -e WARMUP -e ROUNDS -e TRACE -e TRACE_N -e BUILDS="${NAMES[*]}" \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/exec_ab_inside.sh 2>&1 | tee "$OUT/run.log"
{ echo "load at end: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at end, $r: $(head -1 "/proc/pressure/$r")"; done; } >> "$OUT/env.txt"
python3 -I "$HERE/exec_ab.py" summarize "$OUT" > "$OUT/summary.md"
[ "${TRACE:-0}" = 0 ] || for name in "${NAMES[@]}"; do python3 -I "$HERE/exec_ab.py" sched "$OUT/$name.trace.txt"; done >> "$OUT/summary.md"
cat "$OUT/summary.md"
echo "results: $OUT"
