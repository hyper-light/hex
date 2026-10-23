#!/usr/bin/env bash
# The `oci` class's per-container cgroups against the kernel (docs/design/runtime.md, "every oci
# container gets its own cgroup"; AC-T7.1.4, AC-T7.1.1): limits in force, the container and only the
# container inside, its cgroup namespace rooted there, device rules enforced, `kill --all` and
# `delete` reaching every process, refusals leaving nothing behind. As root in a privileged container
# with a private cgroup namespace, the shape of the runtime-tools lab. Exits non-zero on any failure.
# Usage: cgroups.sh <hex binary> [image]
set -euo pipefail
HEX=$(realpath "${1:?hex binary}")
IMAGE=${2:-registry.fedoraproject.org/fedora:44}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$HEX:/opt/hex:ro" -v "$HERE:/opt/lab:ro" \
  "$IMAGE" bash /opt/lab/cgroups_inside.sh
