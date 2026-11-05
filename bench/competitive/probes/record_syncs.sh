#!/usr/bin/env bash
# bench/competitive/probes: counts the fsync family a seam create makes (goal 5 plan step 2, decision 1). Runs
# record_syncs_inside.sh in the bench container, with host.sh's OCI-pass flags, twice: with /run a tmpfs, as on a
# host, where a root node keeps its containers' state; and with /run on the container's overlay, as host.sh's
# container has it unless it mounts one. Every binary is copied into the run's directory, so what ran is kept
# beside what it counted. On fedora:
#   record_syncs.sh <hex binary> <seam_oci binary> <strace binary>
# Env: N (record_syncs_inside.sh's), CPUSET (4-31). A count, not a timing: no load gate.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
. "$HERE/../containers.sh"
HEX=$(realpath "${1:?hex binary}"); SEAM=$(realpath "${2:?seam_oci binary}"); STRACE=$(realpath "${3:?strace binary}")
case "$(hostname)" in fedora*|"${BENCH_HOST_OK:-__none__}") ;; *) echo "refusing: the bench host is fedora, not $(hostname) (BENCH_HOST_OK=$(hostname) runs it here)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
OUT="$BASE/out/record-syncs-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT/bin"
cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"; cp "$STRACE" "$OUT/bin/strace"
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "load at start: $(cat /proc/loadavg)"
  echo "hex: $("$OUT/bin/hex" --version) (sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16))"
  echo "seam_oci: sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "strace: $("$OUT/bin/strace" -V | head -1) (sha256 $(sha256sum "$OUT/bin/strace" | cut -c1-16))"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
} > "$OUT/env.txt"
NAME=hexbench-recsync-$$
bench_trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true'
for run in tmpfs overlay; do
  RUNMOUNT=(); [ "$run" = tmpfs ] && RUNMOUNT=(--tmpfs /run:rw,exec,nosuid,size=1g)
  timeout 1800 docker run --rm --name "$NAME" "${BENCH_LABELS[@]}" --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
    --privileged --security-opt label=disable --cgroupns=private --network=none "${RUNMOUNT[@]}" \
    -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" -e N -e RUN="$run" \
    registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/record_syncs_inside.sh 2>&1 | tee -a "$OUT/probe.out"
done
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
echo "results: $OUT"
