#!/usr/bin/env bash
# AC-T7.1.4: opencontainers/runtime-tools' validation suite against an OCI runtime CLI, every test in
# its validation/ directory, as root in a privileged container with its own cgroup namespace (the
# suite needs root: namespaces, mounts, devices, cgroups). One line per test in $OUT/results.tsv:
# name, pass|fail|skip, the first failing TAP line.
#
#   bench/oci-validation/run.sh <runtime-tools checkout, built> <runtime binary> <out dir> [image]
#
# The host's /usr is mounted read-only over the image's, so the runtimes and the suite run with the
# host's own libraries (fedora runs it: the image is fedora too). SELinux labelling of the container
# itself is off (label=disable): in spc_t, runc can't execute the memfd copy of itself it re-execs.
set -euo pipefail
RT=$(realpath "${1:?runtime-tools checkout}")
RUNTIME=$(realpath "${2:?runtime binary}")
OUT=$(realpath -m "${3:?out dir}")
IMAGE=${4:-registry.fedoraproject.org/fedora:44}
mkdir -p "$OUT"
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$RT:$RT:ro" -v "$OUT:$OUT" -v "$RUNTIME:/opt/runtime:ro" -v "$HERE:/opt/lab:ro" \
  -e RT="$RT" -e OUT="$OUT" "$IMAGE" bash /opt/lab/inside.sh
