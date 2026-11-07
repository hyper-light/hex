#!/usr/bin/env bash
# HEX-GUEST-39's before/after (bench/native/guest/first-create/README.md): an upgrade's first create, a node that
# boots its guest at the first create (A) against one that boots it when made (B). Each turn is one upgrade:
# a fresh copy of Shards and of its VM process, each re-signed under a new identifier (the VM process with
# Shards' vm.entitlements and Hardened Runtime, as its build.rs signs it), and a fresh SHARDS_CACHE; then one
# process makes the node and creates and deletes one box (crates/hex-cli/tests/guest_first_create.rs). Turns
# go A B, B A, ... (ABBA), under the host's own co-tenant load, written before each turn. No turn is dropped.
#   pair.sh PAIRS IMAGE A_BIN A_INITRD B_BIN B_INITRD SHARDS VM_BINARY ENTITLEMENTS OUT
#   (HEX_GUEST_KERNEL set; macOS: codesign)
set -uo pipefail
pairs=$1; image=$2; a_bin=$3; a_initrd=$4; b_bin=$5; b_initrd=$6; shards=$7; vm=$8; ent=$9; out=${10}
mkdir -p "$out"; out=$(cd "$out" && pwd)
cotenants() {
    local load hex top
    load=$(uptime | sed 's/.*load averages*: //')
    hex=$(launchctl list | awk '$1 != "-" && $3 ~ /^hex\./ {printf "%s ", $3}')
    top=$(ps -Ao pcpu=,comm= | sort -rn | head -3 | awk '{n=split($2, p, "/"); printf "%s %s%%, ", p[n], $1}')
    echo "load $load | hex: ${hex:-none} | top: $top"
}
n=0
turn() { # arm pair
    local arm=$1 r=$2 bin initrd u
    if [ "$arm" = A ]; then bin=$a_bin; initrd=$a_initrd; else bin=$b_bin; initrd=$b_initrd; fi
    n=$((n + 1)); u=$out/upgrade-$n; mkdir -p "$u"
    cp "$shards" "$u/shards" && codesign --force -s - -i "shards-upgrade-$n" "$u/shards" 2> /dev/null
    cp "$vm" "$u/shards-vm" && codesign --force -s - -i "shards-vm-upgrade-$n" --entitlements "$ent" -o runtime "$u/shards-vm" 2> /dev/null
    echo "pair $r $arm upgrade $n $(cotenants)" >> "$out/load.txt"
    line=$(HEX_GUEST_INITRD=$initrd HEX_FIRST_IMAGE=$image SHARDS_BIN=$u/shards SHARDS_VM_BINARY=$u/shards-vm \
        SHARDS_CACHE=$u/cache "$bin" --nocapture --test-threads=1 2>&1 | tee "$u.txt" | grep -m1 '^FIRST ')
    echo "pair $r $arm ${line:-FAILED: $(grep -m1 -E 'panicked|error' "$u.txt")}" >> "$out/turns.txt"
    rm -rf "${u:?}"
}
for ((r = 0; r < pairs; r++)); do
    if ((r % 4 == 0 || r % 4 == 3)); then turn A $r; turn B $r; else turn B $r; turn A $r; fi
done
python3 -I "$(dirname "$0")/summary.py" "$out" | tee "$out/summary.txt"
