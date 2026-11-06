#!/usr/bin/env bash
# Assembles the OCI validation lab's root for a guest's architecture (docs/design/shards-guest.md §6a): Alpine's
# minirootfs with util-linux's unshare and bash unpacked over it (inputs.txt, fetched by fetch.sh), the suite's
# runner (bench/oci-validation/inside.sh) at /opt/lab/inside.sh, the shim it calls as the runtime
# (/opt/lab/hex-oci: `exec /opt/hex oci "$@"`), and empty /opt/runtime-tools and /opt/out, where the lab binds
# the run's two shares. The guest's `hex` goes at /opt/hex when the lab is made (HEX-GUEST-20 copies it in).
# Nothing in it is a device node, a FIFO or a socket, which staging refuses.
#   lab.sh <aarch64|x86_64> <inputs dir> <out dir: the lab's root, made empty>
set -euo pipefail
arch=${1:?usage}; inputs=$(cd "${2:?usage}" && pwd); out=${3:?usage}
here=$(cd "$(dirname "$0")" && pwd)
case $arch in aarch64 | x86_64) ;; *) echo "arch $arch is neither aarch64 nor x86_64" >&2; exit 2 ;; esac
rm -rf "${out:?}"; mkdir -p "$out"
tar -xzf "$inputs/alpine-minirootfs-3.24.4-$arch.tar.gz" -C "$out"
for apk in "$inputs/$arch"/*.apk; do
    # An apk is gzip streams of its signature, its control files and its data: unpack all but the
    # control files (.SIGN.*, .PKGINFO, scripts), which name the package and aren't part of the root.
    # tar's notes on apk's own pax keywords go to /dev/null; a failure still ends the run by its status.
    gzip -dc "$apk" | tar -x -C "$out" --exclude='.SIGN.*' --exclude='.PKGINFO' --exclude='.pre-*' \
        --exclude='.post-*' --exclude='.trigger' 2>/dev/null
done
mkdir -p "$out/opt/lab" "$out/opt/runtime-tools" "$out/opt/out"
cp "$here/../../oci-validation/inside.sh" "$out/opt/lab/inside.sh"
printf '#!/bin/sh\nexec /opt/hex oci "$@"\n' >"$out/opt/lab/hex-oci"
chmod 0755 "$out/opt/lab/inside.sh" "$out/opt/lab/hex-oci"
odd=$(find "$out" \( -type b -o -type c -o -type p -o -type s \) | head -5)
[ -z "$odd" ] || { echo "the lab holds what staging refuses: $odd" >&2; exit 1; }
[ -x "$out/usr/bin/unshare" ] && [ -x "$out/bin/bash" ] || { echo "the lab lacks unshare or bash" >&2; exit 1; }
echo "lab for $arch at $out: $(find "$out" | wc -l) entries"
