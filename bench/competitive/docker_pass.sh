#!/usr/bin/env bash
# bench/competitive/docker_pass.sh: host.sh's OCI pass on a machine whose own /usr isn't Fedora's and whose
# containers run in a Linux VM (Docker Desktop on macOS, its WSL2 VM on Windows). Everything the pass needs is
# made inside a Fedora 44 image of the pass's own: hex at <commit> built static-glibc for the VM's architecture,
# seam_oci from the same tree, and the libcrun driver from crun's pinned release tarball (libcrun/build.sh). Then
# oci_inside.sh runs in a container from that image, as host.sh runs it, with the image's /usr standing in for the
# host's, and summarize.py writes its table. Only docker, git, tar and a POSIX shell run on the machine itself.
#   docker_pass.sh <commit> <results dir>
# Env (defaults): RUNTIMES (libcrun), HEXBENCH_PHASES (lifecycle), ROUNDS (2), N (1000), WARMUP (100), LIFECYCLES
# (2000), CAP (1000), BUDGET_MIB (8192), MEMORY (16g), CPUSET (the VM's CPUs past an eighth of them, at least 2:
# fedora's 4-31 of 32), HOST_CPU (the machine's processor, for env.txt). The VM's load is recorded at the pass's
# start and end, as host.sh records the host's.
set -euo pipefail
COMMIT=${1:?commit}; RESULTS=${2:?results dir}
# A Windows shell (Git Bash) is told to leave the containers' paths alone, and hands git a path git.exe reads.
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'
HERE=$(cd "$(dirname "$0")" && { pwd -W 2>/dev/null || pwd; })
REPO=$(git -C "$HERE" rev-parse --show-toplevel)
SHA=$(git -C "$REPO" rev-parse --short "$COMMIT")
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
PACKAGES="python3 procps-ng findutils diffutils util-linux coreutils gcc make tar gzip git curl pkgconf-pkg-config \
  glibc-static json-c-devel libcap-devel libseccomp-devel"
IMG=hexbench/pass:44-$(printf '%s' "$PACKAGES" | cksum | cut -d' ' -f1)
CRUN_VERSION=1.30.1
NCPU=$(docker info --format '{{.NCPU}}')
SKIP=$((NCPU / 8)); [ "$SKIP" -ge 2 ] || SKIP=2
CPUSET=${CPUSET:-$SKIP-$((NCPU - 1))}
SRC=hexpass-src-$SHA; BIN=hexpass-bin-$SHA; OUTV=hexpass-out-$STAMP; NAME=hexpass-$STAMP
mkdir -p "$RESULTS"
trap 'docker rm -f "$NAME" "$NAME-image" >/dev/null 2>&1 || true' EXIT
say() { echo "$(date -u +%FT%TZ) $*"; }

# The image is made by a container and a commit, not a build, so a machine's builder busy with other work
# doesn't hold the pass up; its tag names its packages, so a changed list makes a new one.
say "image $IMG"
if ! docker image inspect "$IMG" > /dev/null 2>&1; then
  docker rm -f "$NAME-image" > /dev/null 2>&1 || true
  docker run --name "$NAME-image" registry.fedoraproject.org/fedora:44 \
    sh -c "dnf -y -q install --setopt=install_weak_deps=False $PACKAGES && dnf clean all" > /dev/null
  docker commit "$NAME-image" "$IMG" > /dev/null
  docker rm "$NAME-image" > /dev/null
fi

say "source $SHA into volume $SRC"
docker volume create "$SRC" > /dev/null
git -C "$REPO" -c core.autocrlf=false -c core.eol=lf archive "$SHA" | docker run -i --rm -v "$SRC:/src" "$IMG" sh -c 'find /src -mindepth 1 -delete && tar -x -C /src'

say "build hex, seam_oci and the libcrun driver into volume $BIN"
docker volume create "$BIN" > /dev/null
docker run --rm -v "$SRC:/src:ro" -v "$BIN:/out" -v hexpass-cargo:/cargo -v hexpass-rustup:/rustup -v hexpass-target:/target \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/target -e RUSTUP_TOOLCHAIN=1.99.1 \
  -e PATH=/cargo/bin:/usr/bin:/bin -e CRUN_VERSION="$CRUN_VERSION" -w /src "$IMG" bash -euo pipefail -c '
    [ -x /cargo/bin/rustup ] || curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none
    for i in 1 2 3; do rustup toolchain install 1.99.1 --profile minimal && break; sleep 10; done
    triple=$(uname -m)-unknown-linux-gnu; flags=CARGO_TARGET_$(echo "$triple" | tr a-z- A-Z_)_RUSTFLAGS
    env "$flags=-C target-feature=+crt-static" nice -n 10 cargo build --release --locked -p hex-cli --target "$triple"
    cp "/target/$triple/release/hex" /out/hex
    nice -n 10 cargo bench --locked --no-run -p hex-bench --bench seam_oci 2> /tmp/bench.log || { cat /tmp/bench.log; exit 1; }
    cp "$(ls -t /target/release/deps/seam_oci-* | grep -v "\.d$" | head -1)" /out/seam_oci
    curl -fsSL -o /tmp/crun.tar.gz "https://github.com/containers/crun/releases/download/$CRUN_VERSION/crun-$CRUN_VERSION.tar.gz"
    sha256sum /tmp/crun.tar.gz | cut -c1-16 > /out/crun-src.sha256
    BUILD_DIR=/tmp bash /src/bench/competitive/libcrun/build.sh /tmp/crun.tar.gz /out/libcrun-driver
    /out/hex --version > /out/hex.version'

say "pass: RUNTIMES=${RUNTIMES:-libcrun} HEXBENCH_PHASES=${HEXBENCH_PHASES:-lifecycle} ROUNDS=${ROUNDS:-2} N=${N:-1000} on CPUs $CPUSET"
docker volume create "$OUTV" > /dev/null
docker run --rm -v "$BIN:/opt/bin:ro" -v "$OUTV:/opt/out" -e HOST="$(hostname)" -e HOST_CPU="${HOST_CPU:-}" -e SHA="$SHA" \
  -e CPUSET="$CPUSET" -e NCPU="$NCPU" "$IMG" bash -c '
  { echo "--- pass started $(date -u +%FT%TZ): PHASES=oci RUNTIMES=${RUNTIMES:-libcrun} (docker_pass.sh)"
    echo "host: $HOST, its containers in a Linux VM (kernel $(uname -r), $(uname -m), $NCPU cpus)"
    echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
    echo "load gate: none (the VM load is recorded)"
    echo "cpu: ${HOST_CPU:-unknown} ($NCPU cpus in the VM), pinned to $CPUSET"
    echo "memory: $(free -g | awk "/^Mem/ {print \$2}") GiB in the VM"; echo "load at start: $(cat /proc/loadavg)"
    echo "hex: $SHA $(cat /opt/bin/hex.version) ($(sha256sum /opt/bin/hex | cut -c1-16)), static-glibc, $(uname -m)"
    echo "seam_oci: $SHA ($(sha256sum /opt/bin/seam_oci | cut -c1-16))"
    echo "libcrun: crun-'"$CRUN_VERSION"'.tar.gz ($(cat /opt/bin/crun-src.sha256)), driver $(sha256sum /opt/bin/libcrun-driver | cut -c1-16)"
    echo "params: ROUNDS=${ROUNDS:-2} N=${N:-1000} WARMUP=${WARMUP:-100} HEXBENCH_PHASES=${HEXBENCH_PHASES:-lifecycle}"
  } > /opt/out/env.txt'
docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="$CPUSET" --memory="${MEMORY:-16g}" \
  --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v "$BIN:/opt/bin:ro" -v "$SRC:/src:ro" -v "$OUTV:/opt/out" \
  -e RUNTIMES="${RUNTIMES:-libcrun}" -e ROUNDS="${ROUNDS:-2}" -e ROUND_FIRST=1 -e N="${N:-1000}" -e WARMUP="${WARMUP:-100}" \
  -e N_OVERRIDE="${N_OVERRIDE:-runsc:3000}" -e LIFECYCLES="${LIFECYCLES:-2000}" -e CAP="${CAP:-1000}" \
  -e BUDGET_MIB="${BUDGET_MIB:-8192}" -e HEXBENCH_PHASES="${HEXBENCH_PHASES:-lifecycle}" -e SEAM_OCI=/opt/bin/seam_oci \
  "$IMG" bash -c 'ln -s /src/bench/competitive /opt/lab && bash /opt/lab/oci_inside.sh' 2>&1 | tee "$RESULTS/oci.log"
docker run --rm -v "$SRC:/src:ro" -v "$OUTV:/opt/out" "$IMG" bash -c \
  'echo "load at end: $(cat /proc/loadavg)" >> /opt/out/env.txt && python3 -I /src/bench/competitive/summarize.py /opt/out > /opt/out/table.md'
docker run --rm -v "$OUTV:/opt/out:ro" "$IMG" tar -C /opt/out -cz . | tar -xz -C "$RESULTS"
docker volume rm "$OUTV" > /dev/null
say "results: $RESULTS"
