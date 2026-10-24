#!/usr/bin/env bash
# bench/competitive: one round of Zeroboot (zeroboot_inside.sh) in its own privileged lab container, so its
# whole stack is the container's: the bench command run_refereed.sh times; the container is labelled for
# the referee's stack.
#   zeroboot.sh <out dir> <out json name> <n> <warmup> <lifecycles> <density cap>
# Needs $BASE/zeroboot from zeroboot_build.sh. Env: HEXBENCH_PHASES, CPUSET (4-31).
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
OUT=$(realpath "${1:?out dir}"); NAME=${2:?out json name}; shift 2
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
for f in zeroboot firecracker vmlinux init; do
  [ -f "$BASE/zeroboot/$f" ] || { echo "refusing: no $BASE/zeroboot/$f (zeroboot_build.sh)" >&2; exit 4; }
done
C="hexbench-zeroboot-$$"
cleanup() { docker rm -f "$C" > /dev/null 2>&1 || true; }
trap cleanup EXIT
docker run --rm --name "$C" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
  --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v "$BASE/zeroboot:/opt/zeroboot:ro" -v "$HERE:/opt/lab:ro" -v "$OUT:/opt/out" -e HEXBENCH_PHASES \
  hexbench/zeroboot:1 bash /opt/lab/zeroboot_inside.sh "/opt/out/$NAME" "$@"
