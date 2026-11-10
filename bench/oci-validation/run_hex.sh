#!/usr/bin/env bash
# runtime-tools validation against `hex oci`, through a shim that maps the suite's `<runtime> <verb>`
# calls to `hex oci <verb>`. Same harness as the baseline (bench/oci-validation/run.sh).
#   run_hex.sh <runtime-tools checkout> <hex binary> <out dir> [image]
# HEX_OCI_LSM, where set, is hex's LSM policy in the container (lsm::ENV_POLICY).
# BENCH_MEMORY and BENCH_OOM, when set, bound the container (--memory, --oom-score-adj), as seam_root.sh's do: the
# lead's shared fedora limits (8g, OOM score 1000; hex-slot and the T7 fedora-unit-limits rule), an operator's limit,
# not tuning.
set -euo pipefail
RT=$(realpath "${1:?runtime-tools}")
HEX=$(realpath "${2:?hex binary}")
OUT=$(realpath -m "${3:?out dir}")
IMAGE=${4:-registry.fedoraproject.org/fedora:44}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
# Its container carries the run's labels and goes with the run, however it ends (containers.sh).
. "$(dirname "$(realpath "${BASH_SOURCE[0]}")")/../competitive/containers.sh"
bench_trap :
mkdir -p "$OUT"
printf '#!/bin/sh\nexec /opt/hex oci "$@"\n' > "$OUT/hex-oci"; chmod +x "$OUT/hex-oci"
docker run --rm "${BENCH_LABELS[@]}" ${HEX_OCI_LSM:+-e HEX_OCI_LSM="$HEX_OCI_LSM"} --privileged --security-opt label=disable --cgroupns=private --network=none \
  ${BENCH_MEMORY:+--memory="$BENCH_MEMORY"} ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  -v /usr:/usr:ro -v "$RT:$RT:ro" -v "$OUT:$OUT" -v "$HEX:/opt/hex:ro" -v "$HERE:/opt/lab:ro" \
  -e RT="$RT" -e OUT="$OUT" -e RUNTIME="$OUT/hex-oci" -e HEX_OCI_ROOT=/run/hex-oci \
  "$IMAGE" bash /opt/lab/inside.sh
