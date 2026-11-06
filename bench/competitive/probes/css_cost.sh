#!/usr/bin/env bash
# bench/competitive/probes: what each controller's teardown of a box's cgroup costs (css_cost_inside.sh says how),
# in a container with host.sh's OCI-pass flags, on the given binaries; css_cost.py reads the trace. On fedora:
#   [N=2000] [CPUSET=4-15] css_cost.sh <hex release binary> <seam_oci release binary>
# Not a timed pass: the kprobes cost each callback some. BENCH_MEMORY and BENCH_OOM as create_tail.sh takes them.
# HEXBENCH_DEVFS, _PROCFS and _OWNER reach seam_oci's node as host.sh's pass hands them
# (unset: the node's defaults), so a run can read the walk with a pool's superblocks and without them. CSS_FNS_EXTRA
# names more kernel functions to time so (the release's rstat flushes: css_rstat_flush and its callbacks).
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
. "$HERE/../containers.sh"
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
case "$(hostname)" in fedora*|"${BENCH_HOST_OK:-__none__}") ;; *) echo "refusing: the bench host is fedora, not $(hostname)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
STAMP=$(date -u +%Y%m%dT%H%M%SZ); OUT="$BASE/out/css-cost-$STAMP"
mkdir -p "$BASE/out"; mkdir "$OUT" || { echo "refusing: $OUT is there already" >&2; exit 3; }
mkdir "$OUT/bin"; cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"
export TRACE_TAG="${STAMP}_$$"
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"; echo "pinned to ${CPUSET:-4-15}"
  echo "load at start: $(cat /proc/loadavg)"
  echo "hex sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16), seam_oci sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD 2>/dev/null || echo unknown)"
} > "$OUT/env.txt"
NAME=hexbench-css-$$
untag() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run --rm "${BENCH_LABELS[@]}" --privileged --security-opt label=disable --network=none \
    -v /usr:/usr:ro -v "$LAB:/opt/lab:ro" -e TRACE_TAG -e UNTRACE_ONLY=1 \
    registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/css_cost_inside.sh >/dev/null 2>&1 || true
}
bench_trap untag
timeout 3600 docker run --rm --name "$NAME" "${BENCH_LABELS[@]}" --cpuset-cpus="${CPUSET:-4-15}" --memory="${BENCH_MEMORY:-40g}" ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  --privileged --security-opt label=disable --cgroupns=private --network=none --tmpfs /run:rw,exec,nosuid,size=1g \
  -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" -e N -e TRACE_TAG -e HEXBENCH_DEVFS -e HEXBENCH_PROCFS \
  -e HEXBENCH_OWNER -e CSS_FNS_EXTRA \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/css_cost_inside.sh 2>&1 | tee "$OUT/probe.out"
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
python3 -I "$HERE/css_cost.py" read "$OUT" > "$OUT/summary.md" 2> "$OUT/summary.err" || echo "the reader failed: $(tail -c 300 "$OUT/summary.err")"
echo "results: $OUT"
