#!/usr/bin/env bash
# Builds the Shards guest Hex's oci class runs in on macOS and Windows (docs/design/shards-guest.md §4):
#   scripts/guest/build.sh x86_64|aarch64 SHARDS OUT [VARIANT]
# SHARDS is a Shards checkout at the revision the guest is built from; VARIANT is a Shards kernel
# variant (`hyperv` for a Windows host's guest: resources/kernel/README.md). Writes to OUT:
# - the kernel, Shards' pinned Linux built by Shards' own scripts/build-kernel.sh in Shards' pinned
#   builder (resources/kernel/builder.env), and its config, checked against kernel-needs.config;
# - `hex`, static glibc (the runtime's decision: no loader or libraries in the guest), built in the
#   Rust image of rust-toolchain.toml's version;
# - initramfs-<arch>.cpio, whose /init is that `hex` (initramfs.py);
# - SHA256SUMS, and toolchain-*.txt naming every tool and image.
# Needs bash, python3 and Docker running Linux containers of ARCH: natively (a Linux host of ARCH, or
# Docker Desktop on an arm64 Mac for aarch64), or emulated (QEMU's binfmt handlers), slower but the
# same inputs. Each container is
# bounded by BENCH_MEMORY (default 8g) and BENCH_OOM (default 1000), and pinned to GUEST_CPUSET
# where it is set.
set -euo pipefail

usage="usage: build.sh x86_64|aarch64 SHARDS OUT [VARIANT]"
arch=${1:?$usage}
shards=${2:?$usage}
out=${3:?$usage}
variant=${4:-}
case $arch in
x86_64) kernel_name=vmlinux platform=linux/amd64 ;;
aarch64) kernel_name=Image platform=linux/arm64 ;;
*)
    echo "$usage" >&2
    exit 2
    ;;
esac

here=$(cd "$(dirname "$0")" && pwd)
hex=$(cd "$here/../.." && pwd)
shards=$(cd "$shards" && pwd)
mkdir -p "$out/kernel" "$out/target" "$out/cargo"
out=$(cd "$out" && pwd)
# The lead's shared fedora limits for a build or bench container (memory 8g, OOM score 1000, cargo -j 4;
# hex-slot and the T7 fedora-unit-limits rule): an operator's limit on a shared host, not tuning. Set them
# to override.
limits=(--platform "$platform" --memory "${BENCH_MEMORY:-8g}" --oom-score-adj "${BENCH_OOM:-1000}")
if [ -n "${GUEST_CPUSET:-}" ]; then
    limits+=(--cpuset-cpus "$GUEST_CPUSET")
fi
user="$(id -u):$(id -g)"

# 1. The kernel: what scripts/build-kernel-in-builder.sh does, bounded as above.
# shellcheck source=/dev/null
. "$shards/resources/kernel/builder.env"
docker run --rm "${limits[@]}" -v "$shards:/src:ro" -v "$out/kernel:/out" \
    -e ARCH_="$arch" -e VARIANT_="$variant" -e SNAPSHOT="$SNAPSHOT" -e PACKAGES="$PACKAGES" \
    "$BUILDER" bash -euo pipefail -c '
        rm -f /etc/apt/sources.list.d/*
        echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/$SNAPSHOT bookworm main" \
            > /etc/apt/sources.list
        apt-get -o Acquire::Check-Valid-Until=false update -q
        # shellcheck disable=SC2086
        apt-get install -y -q --no-install-recommends $PACKAGES
        {
            gcc --version | head -1
            ld --version | head -1
            dpkg-query -W -f="\${Package} \${Version}\n" $PACKAGES gcc-12 binutils libc6-dev
        } > "/out/toolchain-kernel.txt"
        /src/scripts/build-kernel.sh "$ARCH_" /out "$VARIANT_"
        chown -R '"$user"' /out
    '
suffix=${variant:+-${variant//+/-}}
kernel=$(ls "$out/kernel/$kernel_name"-*-"$arch$suffix")
config=$(ls "$out/kernel/config"-*-"$arch$suffix")
missing=0
while IFS= read -r line; do
    case $line in
    CONFIG_*=*) grep -qxF -- "$line" "$config" || { echo "the guest kernel lacks $line" >&2; missing=1; } ;;
    esac
done <"$here/kernel-needs.config"
[ $missing -eq 0 ]

# 2. A static `hex` for the guest.
rust=$(sed -nE 's/^channel = "([0-9.]+)"$/\1/p' "$hex/rust-toolchain.toml")
image=docker.io/library/rust:$rust-bookworm
target=$arch-unknown-linux-gnu
flags_var=CARGO_TARGET_$(echo "$target" | tr 'a-z-' 'A-Z_')_RUSTFLAGS
docker pull -q --platform "$platform" "$image" >/dev/null
docker run --rm "${limits[@]}" --user "$user" -v "$hex:/hex:ro" -v "$out/target:/target" \
    -v "$out/cargo:/cargo" -v "$out:/out" -w /hex \
    -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/target -e RUSTUP_TOOLCHAIN="$rust" \
    -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" -e "$flags_var=-C target-feature=+crt-static" \
    "$image" bash -euo pipefail -c "
        cargo build --release --locked --target $target -p hex-cli --bin hex
        cp /target/$target/release/hex /out/hex
        ldd /out/hex 2>&1 | grep -Eq 'not a dynamic executable|statically linked'
        { rustc -vV; ldd --version | head -1; } > /out/toolchain-hex.txt
    "
docker image inspect --format '{{index .RepoDigests 0}}' "$image" >>"$out/toolchain-hex.txt"
(cd "$hex" && git rev-parse HEAD && git status --porcelain) >"$out/hex-commit.txt"

# 3. The initramfs.
python3 -I "$here/initramfs.py" "$out/hex" "$out/initramfs-$arch.cpio"

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
(cd "$out" && sha256 "kernel/$(basename "$kernel")" "kernel/$(basename "$config")" hex \
    "initramfs-$arch.cpio" >SHA256SUMS && cat SHA256SUMS)
