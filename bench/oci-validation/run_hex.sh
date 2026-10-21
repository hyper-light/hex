#!/usr/bin/env bash
# runtime-tools validation against `hex oci`, through a shim that maps the suite's `<runtime> <verb>`
# calls to `hex oci <verb>`. Same harness as the baseline (bench/oci-validation/run.sh).
#   run_hex.sh <runtime-tools checkout> <hex binary> <out dir> [image]
set -euo pipefail
RT=$(realpath "${1:?runtime-tools}")
HEX=$(realpath "${2:?hex binary}")
OUT=$(realpath -m "${3:?out dir}")
IMAGE=${4:-registry.fedoraproject.org/fedora:44}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
mkdir -p "$OUT"
printf '#!/bin/sh\nexec /opt/hex oci "$@"\n' > "$OUT/hex-oci"; chmod +x "$OUT/hex-oci"
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$RT:$RT:ro" -v "$OUT:$OUT" -v "$HEX:/opt/hex:ro" -v "$HERE:/opt/lab:ro" \
  -e RT="$RT" -e OUT="$OUT" -e RUNTIME="$OUT/hex-oci" -e HEX_OCI_ROOT=/run/hex-oci \
  "$IMAGE" bash /opt/lab/inside.sh
