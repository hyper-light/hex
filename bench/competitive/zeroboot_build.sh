#!/usr/bin/env bash
# bench/competitive: builds what Zeroboot's rounds need, into $BASE/zeroboot, the one step that uses the
# network. Zeroboot publishes no releases, so it is built from source at ZEROBOOT_COMMIT with its own
# Cargo.lock; its guest init (guest/init.c) is compiled statically against musl with its Makefile's flags,
# in the lab image (zeroboot/Dockerfile); Firecracker v1.12.0 and that release's CI kernel come from fetch.py.
#   zeroboot_build.sh
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
ZEROBOOT_COMMIT=d97720e32e320d37b04e268fdfb53bbaf8a8366f  # main on 2026-10-23
Z=$BASE/zeroboot; SRC=$BASE/src/zeroboot
mkdir -p "$Z" "$BASE/src"
python3 -I "$HERE/fetch.py" "$BASE" zeroboot-firecracker zeroboot-kernel > "$Z/versions.json"
[ -d "$SRC/.git" ] || git clone --quiet https://github.com/zerobootdev/zeroboot "$SRC"
git -C "$SRC" fetch --quiet origin "$ZEROBOOT_COMMIT"
git -C "$SRC" checkout --quiet --detach "$ZEROBOOT_COMMIT"
CARGO_TARGET_DIR="$BASE/target-zeroboot" nice -n 10 cargo build --quiet --release --locked --manifest-path "$SRC/Cargo.toml"
install -m 755 "$BASE/target-zeroboot/release/zeroboot" "$Z/zeroboot"
docker build -q -t hexbench/zeroboot:1 "$HERE/zeroboot" > "$Z/image.id"
docker run --rm --network=none -v "$SRC/guest:/src:ro" -v "$Z:/out" hexbench/zeroboot:1 \
  musl-gcc -static -Os -march=x86-64 -mno-avx -mno-avx2 -o /out/init /src/init.c
{ echo "zeroboot $ZEROBOOT_COMMIT $(sha256sum "$Z/zeroboot" | cut -c1-64)"; echo "init $(sha256sum "$Z/init" | cut -c1-64)"
  echo "image $(cat "$Z/image.id")"; } > "$Z/build.txt"
cat "$Z/build.txt"
