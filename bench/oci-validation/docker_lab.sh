#!/usr/bin/env bash
# seam_root.sh's lab (the node agent's seam as root) on a machine whose own /usr isn't Fedora's and whose
# containers run in a Linux VM (Docker Desktop on macOS, its WSL2 VM on Windows): the test binaries, and the `hex`
# binary they name, are built at <commit> in docker_pass.sh's Fedora 44 image (its recipe, so one image serves
# both), then run as root by seam_root_inside.sh in a privileged container from it, the image's /usr standing in
# for the host's. The tests' HOME is a volume of the lab's own, since the container's root is overlayfs, which
# can't hold a layered box's writable layer. Exits non-zero on any failure.
#   docker_lab.sh <commit> [test]...   (tests: hex-cli's oci_box, node_socket, oci_spec_box and oci_seccomp, and
#                                      hex-node's own, by default)
set -euo pipefail
COMMIT=${1:?commit}; shift
TESTS=${*:-oci_box node_socket oci_spec_box oci_seccomp hex-node}
# A Windows shell (Git Bash) is told to leave the containers' paths alone, and hands git a path git.exe reads.
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'
HERE=$(cd "$(dirname "$0")" && { pwd -W 2>/dev/null || pwd; })
REPO=$(git -C "$HERE" rev-parse --show-toplevel)
SHA=$(git -C "$REPO" rev-parse --short "$COMMIT")
PACKAGES="python3 procps-ng findutils diffutils util-linux coreutils gcc make tar gzip git curl pkgconf-pkg-config \
  glibc-static json-c-devel libcap-devel libseccomp-devel"
IMG=hexbench/pass:44-$(printf '%s' "$PACKAGES" | cksum | cut -d' ' -f1)
SRC=hexlab-src-$SHA
say() { echo "$(date -u +%FT%TZ) $*"; }

say "image $IMG"
if ! docker image inspect "$IMG" > /dev/null 2>&1; then
  NAME=hexlab-image-$SHA; trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
  docker run --name "$NAME" registry.fedoraproject.org/fedora:44 \
    sh -c "dnf -y -q install --setopt=install_weak_deps=False $PACKAGES && dnf clean all" > /dev/null
  docker commit "$NAME" "$IMG" > /dev/null
  docker rm "$NAME" > /dev/null
fi

# Extracted with the time of extraction (tar -m), so cargo never takes an older commit's artifacts in the shared
# target volume for this one's.
say "source $SHA into volume $SRC"
docker volume create "$SRC" > /dev/null
git -C "$REPO" -c core.autocrlf=false -c core.eol=lf archive "$SHA" | docker run -i --rm -v "$SRC:/src" "$IMG" sh -c 'find /src -mindepth 1 -delete && tar -x -m -C /src'

say "build the tests: $TESTS"
BINS=$(docker run --rm -v "$SRC:/src:ro" -v hexpass-cargo:/cargo -v hexpass-rustup:/rustup -v hexlab-target:/target \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/target -e RUSTUP_TOOLCHAIN=1.99.1 \
  -e PATH=/cargo/bin:/usr/bin:/bin -e TESTS="$TESTS" -w /src "$IMG" bash -euo pipefail -c '
    [ -x /cargo/bin/rustup ] || curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none >&2
    for i in 1 2 3; do rustup toolchain install 1.99.1 --profile minimal >&2 && break; sleep 10; done
    args=(); for t in $TESTS; do case $t in hex-node) ;; *) args+=(--test "$t") ;; esac; done
    { [ "${#args[@]}" = 0 ] || nice -n 10 cargo test --locked --no-run --message-format=json -p hex-cli "${args[@]}"
      case " $TESTS " in *" hex-node "*) nice -n 10 cargo test --locked --no-run --message-format=json -p hex-node --lib ;; esac
    } | python3 -I -c "import json, sys
for m in map(json.loads, sys.stdin):
    if m.get(\"reason\") == \"compiler-artifact\" and m.get(\"executable\") and m[\"target\"][\"kind\"] != [\"bin\"]:
        print(m[\"executable\"])"')
say "run as root: $(echo $BINS)"
# The test binaries name /target/debug/hex, so the target volume is mounted where they were built.
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v hexlab-target:/target -v "$SRC:/src:ro" -v hexlab-home:/root -e HOME=/root \
  "$IMG" bash /src/bench/oci-validation/seam_root_inside.sh $BINS
