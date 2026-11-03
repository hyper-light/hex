#!/usr/bin/env bash
# bench/competitive/probes: runs exec_breakdown_inside.sh in the bench container, with host.sh's OCI-pass flags,
# on the libcrun driver and crun host.sh built and fetched ($HEX_BENCH/bin), the given Hex and seam_oci binaries,
# and exec_floor.c, built here. Every binary is copied into the run's directory, so what ran is kept beside what
# it measured. On fedora:
#   exec_breakdown.sh <hex release binary> <seam_oci release binary>
# Env: N, WARMUP, ROUNDS, FLOOR_N, FLOOR_WARMUP, CHECK (exec_breakdown_inside.sh's), CPUSET (4-31); LOAD_GATE=0 skips the
# wait for load under 2, as host.sh allows: this probe's comparisons are within one run (the floor against
# libcrun's exec, the parts against their call), which cancel other people's CPU load. BENCH_HOST_OK names
# another host, as host.sh takes it.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
case "$(hostname)" in fedora*|"${BENCH_HOST_OK:-__none__}") ;; *) echo "refusing: the bench host is fedora, not $(hostname) (BENCH_HOST_OK=$(hostname) runs it here)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
for f in crun libcrun-driver; do
  [ -x "$BASE/bin/$f" ] || { echo "refusing: no $BASE/bin/$f (host.sh fetches crun and builds the libcrun driver)" >&2; exit 4; }
done
# One breakdown at a time: each run's tracer keeps the events of every process named as the runtimes are, host-wide,
# so two at once would each trace the other's lifecycles, and their stamps can be one.
up=$(docker ps --format '{{.Names}}' | grep -E '^hexbench-(createbd|execbd)-' | paste -sd' ' - || true)
[ -z "$up" ] || { echo "refusing: another breakdown probe is up ($up); its tracer and this one's would see each other's processes" >&2; exit 3; }
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="$BASE/out/exec-breakdown-$STAMP"
mkdir -p "$BASE/out"; mkdir "$OUT" || { echo "refusing: $OUT is there already, another run's of this second" >&2; exit 3; }
mkdir "$OUT/bin"
# The run's trace instance is named by its stamp and this script's pid, which no other run up on the host shares
# (exec_breakdown_inside.sh's TRACE_TAG).
export TRACE_TAG="${STAMP}_$$"
gcc -O2 -Wall -Wextra -Werror -o "$OUT/bin/exec_floor" "$HERE/exec_floor.c"
cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"; cp "$BASE/bin/crun" "$BASE/bin/libcrun-driver" "$OUT/bin/"
[ "${LOAD_GATE:-1}" = 0 ] || for i in $(seq 1 90); do
  load=$(cut -d' ' -f1 /proc/loadavg)
  awk -v l="$load" 'BEGIN { exit !(l < 2) }' && break
  [ "$i" = 90 ] && { echo "refusing: 1-minute load average $load is not under 2" >&2; exit 3; }
  sleep 10
done
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-31}"
  [ "${LOAD_GATE:-1}" = 0 ] && echo "load gate: skipped (LOAD_GATE=0): the comparisons are within this run"
  echo "load at start: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at start, $r: $(head -1 "/proc/pressure/$r")"; done
  echo "hex: $("$OUT/bin/hex" --version) (sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16)), $(file -b "$OUT/bin/hex" | cut -d, -f1-4)"
  echo "seam_oci: sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "exec_floor: sha256 $(sha256sum "$OUT/bin/exec_floor" | cut -c1-16), $(gcc --version | head -1)"
  echo "crun: $("$OUT/bin/crun" --version | head -1) (sha256 $(sha256sum "$OUT/bin/crun" | cut -c1-16))"
  echo "libcrun: $("$OUT/bin/libcrun-driver" --version 2>&1 | head -1) (driver sha256 $(sha256sum "$OUT/bin/libcrun-driver" | cut -c1-16))"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
} > "$OUT/env.txt"
NAME=hexbench-execbd-$$
# On any end of this script, a stopped unit's TERM among them: the container gone, then the run's trace instance
# taken down by the inside script's own teardown in a short container of its own, for a container killed before its
# trap ran. It touches this run's tag alone, never another run's.
untag() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run --rm --label hex.bench=competitive --privileged --security-opt label=disable --network=none \
    -v /usr:/usr:ro -v "$LAB:/opt/lab:ro" -e TRACE_TAG -e UNTRACE_ONLY=1 \
    registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/exec_breakdown_inside.sh >/dev/null 2>&1 \
    || echo "the run's trace instance (tag $TRACE_TAG) couldn't be checked gone" >&2
}
trap untag EXIT
trap 'exit 143' TERM INT
timeout 7200 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
  --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" \
  -e N -e WARMUP -e ROUNDS -e FLOOR_N -e FLOOR_WARMUP -e CHECK -e TRACE_TAG \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/exec_breakdown_inside.sh 2>&1 | tee "$OUT/probe.out"
{ echo "load at end: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at end, $r: $(head -1 "/proc/pressure/$r")"; done; } >> "$OUT/env.txt"
echo "results: $OUT"
