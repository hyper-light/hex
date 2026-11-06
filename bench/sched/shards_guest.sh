#!/usr/bin/env bash
# Shards and Hex's guest on an x86_64 Linux host with KVM, everything fetched or built pinned by SHA-256 as
# native step 5's job does on macOS (its m2.sh): zig 0.16.0 by its index's hash; Shards at p/internal-local
# b8270370 plus hex/shards-api a0cced87 (from step 5's bundle), a debug build as step 5 ran; Shards' released
# x86_64 kernel and config, pinned by Shards' crates/shards/src/kernel.rs; and Hex's guest image from
# cp/t7-native-s5 by its scripts/guest/image.sh. For bench/sched/shards_overhead.py
# (bench/sched/shards_overhead.md).
#   shards_guest.sh OUT SHARDS_BUNDLE HEX_REPO
set -euo pipefail
out=${1:?usage: shards_guest.sh OUT SHARDS_BUNDLE HEX_REPO}
bundle=${2:?usage}
hex_repo=${3:?usage}
mkdir -p "$out"
out=$(cd "$out" && pwd)
sha() { sha256sum "$1" | cut -d' ' -f1; }
need() { [ "$(sha "$1")" = "$2" ] || { echo "PIN_MISMATCH $1 $(sha "$1") != $2" >&2; exit 6; }; }

echo "== zig"
z=$out/zig
if ! [ -x "$z/zig" ]; then
    curl -fsSL -o "$out/zig-index.json" https://ziglang.org/download/index.json
    read -r zurl zsha < <(python3 -I -c 'import json,sys; e=json.load(open(sys.argv[1]))["0.16.0"]["x86_64-linux"]; print(e["tarball"], e["shasum"])' "$out/zig-index.json")
    curl -fsSL -o "$out/zig.tar.xz" "$zurl"
    need "$out/zig.tar.xz" "$zsha"
    rm -rf "${z:?}" && mkdir -p "$z" && tar -xJf "$out/zig.tar.xz" -C "$z" --strip-components 1
    echo "zig from $zurl sha256 $zsha"
fi
export PATH=$z:$PATH
zig version

echo "== shards"
s=$out/shards
for _ in 1 2 3; do
    git -C "$s" rev-parse -q --verify HEAD >/dev/null 2>&1 && break
    rm -rf "${s:?}"
    git clone -q --filter=blob:none https://github.com/hyper-light/shards.git "$s" && break
    sleep 10
done
git -C "$s" fetch -q origin p/internal-local
git -C "$s" fetch -q "$bundle" +hex/shards-api:refs/remotes/xfer/shards-api
git -C "$s" checkout -q -B hex/shards-api xfer/shards-api
[ "$(git -C "$s" rev-parse HEAD)" = a0cced87a47138e182df7574c063aa55bb3e1d3a ] || { echo "SHARDS_NOT_a0cced87" >&2; exit 7; }
git -C "$s" merge-base --is-ancestor b8270370a62fc55e8ddc3247f89fdc889cec32a8 HEAD || { echo "SHARDS_NOT_ON_b8270370" >&2; exit 7; }
git -C "$s" log --oneline -1
(cd "$s" && CARGO_TARGET_DIR=$out/target-shards cargo build -q -p shards -j "${JOBS:-4}")
echo "shards $(sha "$out/target-shards/debug/shards")"

echo "== kernel"
r=https://github.com/hyper-light/shards/releases/download/kernel-6.18.48-64a9427a8997
k=$out/kernel
mkdir -p "$k"
for f in vmlinux-6.18.48-x86_64 config-6.18.48-x86_64 SHA256SUMS; do
    [ -f "$k/$f" ] || curl -fsSL -o "$k/$f" "$r/$f"
done
need "$k/vmlinux-6.18.48-x86_64" e23c31e89580f2fb8524cb0f560cbd356c22f2de6ae82081352d809151434261
(cd "$k" && grep -E ' (vmlinux|config)-6\.18\.48-x86_64$' SHA256SUMS | sha256sum -c -)

echo "== hex's guest image"
h=$out/hex
[ -d "$h/.git" ] || git clone -q --no-checkout "$hex_repo" "$h"
git -C "$h" fetch -q "$hex_repo" +cp/t7-native-s5:refs/remotes/src/s5
git -C "$h" checkout -q -B s5 src/s5
git -C "$h" log --oneline -1
(cd "$h" && cargo fetch --locked -q)
CARGO_TARGET_DIR=$out/target-musl bash "$h/scripts/guest/image.sh" x86_64 "$k/vmlinux-6.18.48-x86_64" "$k/config-6.18.48-x86_64" "$out/guest-x86_64"
