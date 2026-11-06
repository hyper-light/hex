#!/usr/bin/env bash
# The bring-up's guest (docs/design/shards-guest.md §4): Shards' released kernel and a static musl
# `hex`, with no Docker and no C toolchain for the guest's architecture:
#   scripts/guest/image.sh x86_64|aarch64 KERNEL CONFIG OUT
# KERNEL and CONFIG are Shards' release of its pinned kernel and that kernel's config (their URLs and
# SHA-256 pinned in Shards' crates/shards/src/kernel.rs); CONFIG is checked against
# kernel-needs.config. `hex` is built for <arch>-unknown-linux-musl, linked by the rust-lld Rust
# ships, as Shards builds its own Linux guests (its .cargo/config.toml). Needs cargo with that
# target's rust-std, python3, and Hex's crates in cargo's cache (it builds offline). Writes to OUT:
# the kernel, its config, `hex`, initramfs-<arch>.cpio and SHA256SUMS.
set -euo pipefail

usage="usage: image.sh x86_64|aarch64 KERNEL CONFIG OUT"
arch=${1:?$usage}
kernel=${2:?$usage}
config=${3:?$usage}
out=${4:?$usage}
case $arch in
x86_64 | aarch64) ;;
*)
    echo "$usage" >&2
    exit 2
    ;;
esac
here=$(cd "$(dirname "$0")" && pwd)
hex=$(cd "$here/../.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)

missing=0
while IFS= read -r line; do
    case $line in
    CONFIG_*=*) grep -qxF -- "$line" "$config" || { echo "the guest kernel lacks $line" >&2; missing=1; } ;;
    esac
done <"$here/kernel-needs.config"
[ $missing -eq 0 ]
cp "$kernel" "$out/$(basename "$kernel")"
cp "$config" "$out/$(basename "$config")"

target=$arch-unknown-linux-musl
upper=$(echo "$target" | tr 'a-z-' 'A-Z_')
export "CARGO_TARGET_${upper}_LINKER=rust-lld"
target_dir=${CARGO_TARGET_DIR:-$out/target}
(cd "$hex" && CARGO_TARGET_DIR=$target_dir cargo build --offline --release --locked --target "$target" -p hex-cli --bin hex)
cp "$target_dir/$target/release/hex" "$out/hex"
python3 -I "$here/initramfs.py" "$out/hex" "$out/initramfs-$arch.cpio"
{
    (cd "$hex" && rustc -vV)
    echo "linker: rust-lld"
    (cd "$hex" && git rev-parse HEAD && git status --porcelain)
} >"$out/toolchain-hex.txt"

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
(cd "$out" && sha256 "$(basename "$kernel")" "$(basename "$config")" hex "initramfs-$arch.cpio" >SHA256SUMS && cat SHA256SUMS)
