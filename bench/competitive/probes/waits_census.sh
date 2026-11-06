#!/usr/bin/env bash
# bench/competitive/probes: the waits census (decision 9; waits_census.md says what each row seeds). Records the answer
# distribution of every named wait (hex_core::patience) from the waits' own timing points: the node and its client
# append each answer to the file HEX_WAITS_CENSUS names (hex_node::census). Runs seam_oci over the node's socket, in a
# container with host.sh's OCI-pass flags, under the bar's no-owner conditions, twice: the node's state on the
# container's tmpfs /run, and on the host's own filesystem (fedora's btrfs). On fedora, under hex-slot:
#   [N=10000] [CPUSET=4-15] waits_census.sh <hex release binary> <seam_oci release binary>
# The timed part takes fedora-timed.lock, as the bench's timed passes do. waits_census.py reads the census.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
. "$HERE/../containers.sh"
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
case "$(hostname)" in fedora*|"${BENCH_HOST_OK:-__none__}") ;; *) echo "refusing: the bench host is fedora, not $(hostname)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
STAMP=$(date -u +%Y%m%dT%H%M%SZ); OUT="$BASE/out/waits-census-$STAMP"
mkdir -p "$BASE/out"; mkdir "$OUT" || { echo "refusing: $OUT is there already" >&2; exit 3; }
mkdir "$OUT/bin" "$OUT/disk"; cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"; echo "pinned to ${CPUSET:-4-15}"
  echo "load at start: $(cat /proc/loadavg)"
  echo "state on disk: $(stat -f -c %T "$OUT/disk")"
  echo "hex sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16), seam_oci sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD 2>/dev/null || echo unknown)"
} > "$OUT/env.txt"
NAME=hexbench-waits-$$
bench_trap true
LOCK=${TIMED_LOCK:-$HOME/.cache/hex-t7/fedora-timed.lock}
flock "$LOCK" timeout 7200 docker run --rm --name "$NAME" "${BENCH_LABELS[@]}" --cpuset-cpus="${CPUSET:-4-15}" \
  --memory="${BENCH_MEMORY:-8g}" ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  --privileged --security-opt label=disable --cgroupns=private --network=none --tmpfs /run:rw,exec,nosuid,size=1g \
  -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" -v "$OUT/disk:/opt/disk" -e N \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/waits_census_inside.sh 2>&1 | tee "$OUT/probe.out"
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
python3 -I "$HERE/waits_census.py" "$OUT" > "$OUT/summary.md" 2> "$OUT/summary.err" || echo "the reader failed: $(tail -c 300 "$OUT/summary.err")"
echo "results: $OUT"
