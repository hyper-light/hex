#!/usr/bin/env bash
# bench/competitive/probes: where creates serialize under concurrency (goal 5 plan step 4). Runs
# create_serial_inside.sh in a privileged container like the OCI pass's, with network for dnf (perf and bpftrace
# aren't on the host), on the given Hex and seam_oci binaries and the libcrun driver host.sh built ($HEX_BENCH/bin),
# each copied into the run's directory. MODE=offcpu (the default): off-CPU time by kernel stack, an on-CPU profile and
# directory operations by filesystem, for Hex's seam at concurrency 1 and 32 and libcrun in process at 32. MODE=locks:
# what holds those locks and which directory each wait is for, for both at concurrency 1 and 32
# (create_serial_inside.sh says which processes are each one's). create_serial.py reads them. MODE=checker: where the
# time of the threads that check a given-back network namespace goes, by system call and on-CPU stack (one owner's
# boxes: HEXBENCH_OWNER). On fedora:
#   [MODE=locks|checker] create_serial.sh <hex release binary> <seam_oci release binary>
# Not a timed pass: the load is recorded, not gated; what it reads is where the threads wait, not how fast.
# BENCH_MEMORY and BENCH_OOM set the container's --memory (40g) and --oom-score-adj (none) on a shared host.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
[ -x "$BASE/bin/libcrun-driver" ] || { echo "refusing: no $BASE/bin/libcrun-driver (host.sh builds it)" >&2; exit 4; }
OUT="$BASE/out/create-serial-${MODE:-offcpu}-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT/bin"
cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"; cp "$BASE/bin/libcrun-driver" "$OUT/bin/"
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-31}"
  echo "load at start: $(cat /proc/loadavg)"
  echo "owner: ${HEXBENCH_OWNER:-none} (one owner's boxes, their bundles holding Docker's default capabilities, when set)"
  echo "container: --memory ${BENCH_MEMORY:-40g}, --oom-score-adj ${BENCH_OOM:-none}"
  echo "containers at start: $(docker ps --format '{{.Names}}' | paste -sd ' ' - | cut -c1-400)"
  echo "hex: $("$OUT/bin/hex" --version) (sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16))"
  echo "seam_oci: sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "libcrun-driver: sha256 $(sha256sum "$OUT/bin/libcrun-driver" | cut -c1-16)"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
} > "$OUT/env.txt"
NAME=hexbench-serial-$$
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
timeout 2400 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory="${BENCH_MEMORY:-40g}" ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  --privileged --security-opt label=disable --cgroupns=private --tmpfs /run:rw,exec,nosuid,size=4g \
  -e SUSTAINED -e MODE -e HEXBENCH_OWNER -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" \
  registry.fedoraproject.org/fedora:44 \
  bash -c 'bash /opt/lab/probes/create_serial_inside.sh; cp /run/bt.*.txt /run/bt.*.err /run/dirs.*.txt /run/*.log /run/out.*.json /opt/out/ 2>/dev/null' \
  2>&1 | tee "$OUT/probe.out"
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
echo "results: $OUT"
