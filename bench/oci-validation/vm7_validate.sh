#!/usr/bin/env bash
# runtime-tools' validation suite against `hex oci` as root in vm7_lab.sh's Linux 7.2 guest (goal 6: the suite
# still passes every test crun passes), the guest's counterpart of run_hex.sh. One dir is shared into the guest:
# a copy of the runtime-tools checkout (rt/), the hex binary, the host's tar (bin/) and out/, where the guest
# writes results.tsv and each test's log, as inside.sh does.
# Usage: vm7_validate.sh <runtime-tools checkout> <hex binary> <out dir>
set -euo pipefail
RT=$(realpath "${1:?runtime-tools}")
HEX=$(realpath "${2:?hex binary}")
DIR=$(realpath -m "${3:?out dir}")
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
[ ! -e "$DIR" ] || { echo "refusing: $DIR exists; each run gets a new dir" >&2; exit 2; }
mkdir -p "$DIR/bin" "$DIR/out"
cp -a "$RT" "$DIR/rt"
cp "$HEX" "$DIR/hex"
# The Fedora guest has no tar, so the host's goes in; a guest with its own (vmaa_build.sh's) sets VM_TAR=0.
[ "${VM_TAR:-1}" = 0 ] || cp "$(command -v tar)" "$DIR/bin/tar"
printf '#!/bin/sh\nexec %s/hex oci "$@"\n' "$DIR" > "$DIR/out/hex-oci"; chmod +x "$DIR/out/hex-oci"
VM7_MODE=validate bash "$HERE/vm7_lab.sh" "$DIR" "$DIR/hex"
