#!/usr/bin/env bash
# One arm of HEX-GUEST-22's create-and-delete A/B (docs/design/shards-guest.md §4a), staged ahead of the run:
# a worktree of REPO at COMMIT, the bench file of BENCH over it (the same file in every arm; it needs no
# store API), the guest image built from that tree (scripts/guest/image.sh), and the bench's test binary.
# Writes DIR/env.sh naming the binary, the initramfs and the layer list the arm stages from.
#   arm.sh REPO COMMIT BENCH LIST KERNEL CONFIG ARCH DIR
#   LIST: layers.txt (the images' own layer<i>.tar.gz: whole-root staging's names) or blobs.txt (the same
#   files linked as blobs/sha256/<hex>: an image layout's names, which the store takes digests from)
set -euo pipefail
repo=$1; commit=$2; bench=$3; list=$4; kernel=$5; config=$6; arch=$7; dir=$8
mkdir -p "$dir"; dir=$(cd "$dir" && pwd)
tree=$dir/tree
[ -d "$tree" ] || git -C "$repo" worktree add -q --detach "$tree" "$commit"
git -C "$tree" checkout -q --detach "$commit"
git -C "$tree" checkout -q -- .
git -C "$repo" show "$bench:crates/hex-cli/tests/guest_create_bench.rs" \
    > "$tree/crates/hex-cli/tests/guest_create_bench.rs"
export CARGO_TARGET_DIR=$dir/target-guest
bash "$tree/scripts/guest/image.sh" "$arch" "$kernel" "$config" "$dir/image" > "$dir/image.log" 2>&1
export CARGO_TARGET_DIR=$dir/target
bin=$(cd "$tree" && cargo test --release --locked -p hex-cli --test guest_create_bench --no-run \
    --message-format=json 2> "$dir/build.err" | python3 -I -c 'import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "guest_create_bench":
        print(m["executable"])')
[ -n "$bin" ] || { tail -20 "$dir/build.err"; exit 4; }
cat > "$dir/env.sh" <<ENV
ARM_COMMIT=$commit
ARM_BIN=$bin
ARM_TREE=$tree
export HEX_GUEST_INITRD=$dir/image/initramfs-$arch.cpio HEX_GUEST22_LIST=$list
ENV
echo "arm $commit (bench $bench, $list): $bin"
