#!/usr/bin/env bash
# AC-T7.1.3: oci container start p50, image present, is at most 50 ms. Times create+start of a
# container whose rootfs is already unpacked (a tiny workload: /bin/true), best of a warm-up then N
# timed runs, and prints the p50/p99 in milliseconds. Runs as root in a privileged container with a
# private cgroup namespace, the same shape as the runtime-tools harness.
#   run.sh <hex release binary> <N> [image]
set -euo pipefail
HEX=$(realpath "${1:?hex binary}"); N=${2:-200}; IMAGE=${3:-registry.fedoraproject.org/fedora:44}
OUT=$(realpath -m "${4:-$HOME/.cache/hex-t7/hexstart}"); mkdir -p "$OUT"
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$HEX:/opt/hex:ro" -v "$HERE:/opt/lab:ro" -v "$OUT:/opt/out" -e N="$N" "$IMAGE" bash /opt/lab/inside.sh
cat "$OUT/result.txt" 2>/dev/null
