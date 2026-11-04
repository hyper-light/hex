#!/usr/bin/env bash
# The hooks fix's trace and old-reader lab (docs/design/runtime.md, "createContainer and startContainer hooks
# run in the container"; AC-T7.1.4): in the validation suite's privileged container, as root, strace each
# binary's `hex oci create` of a hookless bundle, and the after binary's of a hooked one, and have the before
# binary read and delete a container the after binary made with a startContainer hook. hooks_trace_inside.sh
# says what it checks; it exits non-zero on any check that fails.
#   hooks_trace.sh <before hex> <after hex> <out dir> [image]
# BENCH_MEMORY and BENCH_OOM, when set, cap the container as seam_root.sh's are capped.
set -euo pipefail
BEFORE=$(realpath "${1:?before hex}"); AFTER=$(realpath "${2:?after hex}"); OUT=$(realpath -m "${3:?out dir}")
IMAGE=${4:-registry.fedoraproject.org/fedora:44}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
mkdir -p "$OUT"
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  ${BENCH_MEMORY:+--memory="$BENCH_MEMORY"} ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  --tmpfs /run:rw,exec,nosuid,size=1g \
  -v /usr:/usr:ro -v "$OUT:$OUT" -v "$BEFORE:/opt/before-hex:ro" -v "$AFTER:/opt/after-hex:ro" -v "$HERE:/opt/lab:ro" \
  -e OUT="$OUT" "$IMAGE" bash /opt/lab/hooks_trace_inside.sh
