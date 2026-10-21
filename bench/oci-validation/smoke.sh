#!/usr/bin/env bash
# A smoke test of `hex oci`: build a tiny OCI bundle from a container image's rootfs and run a
# command through create/start/state/delete, as root in a privileged container (the same shape as
# the runtime-tools harness). Usage: smoke.sh <hex binary> <image> <workdir>
set -euo pipefail
HEX=$(realpath "${1:?hex binary}")
IMAGE=${2:-registry.fedoraproject.org/fedora:44}
WORK=$(realpath -m "${3:?workdir}")
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
mkdir -p "$WORK"
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$HEX:/opt/hex:ro" -v "$WORK:$WORK" -v "$HERE:/opt/lab:ro" \
  -e WORK="$WORK" "$IMAGE" bash /opt/lab/smoke_inside.sh
