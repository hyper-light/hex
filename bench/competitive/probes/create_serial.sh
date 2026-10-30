#!/usr/bin/env bash
# bench/competitive/probes: where the seam's creates serialize under concurrency (goal 5 plan step 4). Runs
# create_serial_inside.sh in a privileged container like the OCI pass's, with network for dnf (perf and bpftrace
# aren't on the host), on the given Hex and seam_oci binaries, each copied into the run's directory: off-CPU time
# by kernel stack (bpftrace on finish_task_switch) and an on-CPU profile for the bench's threads (seam_oci, the
# node agent in process; hex, its seeds and the inits before they exec), at concurrency 1 (start_true, sequential)
# and 32 (sustained). On fedora:
#   create_serial.sh <hex release binary> <seam_oci release binary>
# Not a timed pass: the load is recorded, not gated; what it reads is where the threads wait, not how fast.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
OUT="$BASE/out/create-serial-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT/bin"
cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-31}"
  echo "load at start: $(cat /proc/loadavg)"
  echo "containers at start: $(docker ps --format '{{.Names}}' | paste -sd ' ' - | cut -c1-400)"
  echo "hex: $("$OUT/bin/hex" --version) (sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16))"
  echo "seam_oci: sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "harness: bench/competitive at $(git -C "$HERE" rev-parse --short HEAD)$(git -C "$HERE" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
} > "$OUT/env.txt"
NAME=hexbench-serial-$$
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
timeout 1800 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
  --privileged --security-opt label=disable --cgroupns=private --tmpfs /run:rw,exec,nosuid,size=4g \
  -v "$OUT/bin:/opt/bin:ro" -v "$HERE:/opt/probes:ro" -v "$OUT:/opt/out" \
  registry.fedoraproject.org/fedora:44 bash -c 'bash /opt/probes/create_serial_inside.sh; cp /run/bt.*.txt /run/bt.*.err /run/seam.*.log /run/out.*.json /opt/out/ 2>/dev/null' \
  2>&1 | tee "$OUT/probe.out"
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
echo "results: $OUT"
