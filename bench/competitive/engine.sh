#!/usr/bin/env bash
# bench/competitive: one round of containerd with nerdctl (engine_inside.sh) in its own privileged lab
# container, so containerd's whole stack is the container's and nothing of it runs on the host. The bench
# command run_refereed.sh times; the container is labelled for the referee's stack.
#   engine.sh <out dir> <out json name> <n> <warmup> <lifecycles> <density cap>
# Env: ENGINE_RUNTIME (e.g. io.containerd.kata-qemu.v2) and ENGINE_NAME (the platform's name in the
# result: nerdctl by default), HEXBENCH_PHASES, KATA_HYPERVISORS; CPUSET (4-31).
# Needs $BASE/bin's containerd, containerd-shim-runc-v2, nerdctl and runc (fetch.py), $BASE/rootfs-image.tar
# (`docker save hexbench/rootfs:1`, the docker rounds' image), and for Kata $BASE/kata (fetch.py kata).
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
OUT=$(realpath "${1:?out dir}"); NAME=${2:?out json name}; shift 2
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
for f in bin/containerd bin/containerd-shim-runc-v2 bin/nerdctl bin/runc rootfs-image.tar; do
  [ -f "$BASE/$f" ] || { echo "refusing: no $BASE/$f" >&2; exit 4; }
done
KATA=()
if [[ "${ENGINE_RUNTIME:-}" == io.containerd.kata* ]]; then
  [ -d "$BASE/kata/opt/kata/bin" ] || { echo "refusing: no $BASE/kata/opt/kata (fetch.py kata)" >&2; exit 4; }
  KATA=(-v "$BASE/kata/opt/kata:/opt/kata:ro")
fi
# containerd's state in a volume on the host's disk: its overlayfs snapshots can't stack on the container's
# own overlay, and in a tmpfs its database would count as memory the stack holds. Docker owns the volume, so
# it removes what root made in it.
C="hexbench-engine-$$"; STATE="hexbench-engine-$$"
cleanup() { docker rm -f "$C" > /dev/null 2>&1 || true; docker volume rm -f "$STATE" > /dev/null 2>&1 || true; }
trap cleanup EXIT
docker volume create --label hex.bench=competitive "$STATE" > /dev/null
docker run --rm --name "$C" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
  --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$BASE/bin:/opt/bin:ro" "${KATA[@]}" -v "$BASE/rootfs-image.tar:/opt/image/rootfs.tar:ro" \
  -v "$STATE:/var/lib/containerd" -v "$HERE:/opt/lab:ro" -v "$OUT:/opt/out" \
  -e ENGINE_RUNTIME -e ENGINE_NAME -e HEXBENCH_PHASES -e KATA_HYPERVISORS \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/engine_inside.sh "/opt/out/$NAME" "$@"
