#!/usr/bin/env bash
# The node agent's seam as root (goal 3 plan step 2's privileged lab): the conformance suite and the oci
# box tests (crates/hex-cli/tests: oci_box, node_socket, oci_spec_box, oci_seccomp) run as root, where
# an exec is cloned straight from the agent's thread into the container's cgroup and pid namespace
# (crates/hex-runtime/src/linux/oci/seed.rs, `exec_direct`), not through a seed as rootless runs take
# it. In a privileged container with a private cgroup namespace, each test in a cgroup of its own under
# the namespace's root, so it can hand controllers down to its boxes. The test binaries and the `hex`
# binary they name are mounted at the paths they were built at; the tests' HOME is a host directory
# (LAB_HOME, ~/.cache/hex-t7/seam-root-home by default), since the container's own root is overlayfs, on
# which the node refuses a layered box's writable layer by name. Exits non-zero on any failure.
# Usage: seam_root.sh <target dir the tests were built in> <test binary>...
set -euo pipefail
TGT=$(realpath "${1:?target dir}"); shift
[ "$#" -gt 0 ] || { echo "usage: seam_root.sh <target dir> <test binary>..." >&2; exit 2; }
IMAGE=${IMAGE:-registry.fedoraproject.org/fedora:44}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
LAB_HOME=${LAB_HOME:-$HOME/.cache/hex-t7/seam-root-home}
mkdir -p "$LAB_HOME"
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$TGT:$TGT" -v "$HERE:/opt/lab:ro" -v "$LAB_HOME:/root" -e HOME=/root \
  "$IMAGE" bash /opt/lab/seam_root_inside.sh "$@"
