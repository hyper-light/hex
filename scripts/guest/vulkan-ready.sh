#!/usr/bin/env bash
# Builds hex-vulkan-guest-ready, the observer Program a Shards GPU guest runs before its GPU pool goes Live
# (AC-T7.4.1; crates/hex-runtime/src/guest/observer/hex-vulkan-guest-ready.c, whose frame observer/driver.rs
# decodes), and the closure it runs in:
#   scripts/guest/vulkan-ready.sh x86_64|aarch64 OUT
# Its inputs are vulkan-ready.env's pins and Hex's tree alone. Writes to OUT, which must be new or empty:
# - closure/bin/hex-vulkan-guest-ready, compiled against the pinned glibc and Vulkan loader;
# - closure/lib/: glibc's dynamic linker, Vulkan's loader, Mesa's Venus driver and every library they load,
#   each under its SONAME;
# - closure/icd/virtio_icd.json: the Venus driver's ICD manifest, its library_path relative to the manifest
#   (../lib/libvulkan_virtio.so), so the closure is the same wherever a share mounts it;
# - closure.sha256: the closure's SHA256SUMS; toolchain.txt: the image, snapshot and package versions;
# - allocation.txt: the Program's allocation units on this ABI, from its own source (vulkan-ready-sizes.c);
# - calibrator/hex-vulkan-ready-calibrate: the static init of the VMs that measure the Program on each GPU
#   (vulkan-ready-calibrate.c; vulkan-ready-manifest.py's `calibrate` boots them);
# - selftest/frame.txt: what the Program wrote run through the closure's own dynamic linker, libc and Vulkan
#   loader against a fake Venus ICD (fake-venus-icd.c: two queue families), the only stand-in, given exactly
#   the allocation its three storage() calls take; selftest/refused-*.txt: its refusals of a queue-family bound
#   under the device's count (saying the count) and of a byte less allocation; selftest/closure.txt: the
#   dynamic linker's resolution of the real Venus driver within the closure alone, every library found there.
# hex-runtime's observer::driver::tests::built_program_frame_is_work_ready decodes selftest/frame.txt
# (HEX_GUEST_READY_FRAME), and scripts/guest/vulkan-ready-manifest.py calibrates and writes the node's
# observer manifest.
# Needs bash and Docker running Linux containers of ARCH, natively or through QEMU's binfmt handlers. Its one
# container is bounded by VULKAN_READY_MEMORY (default 4g) and VULKAN_READY_CPUS (default 2): an operator's
# limits on a shared host, as scripts/guest/build.sh's are, not tuning.
set -euo pipefail

usage="usage: vulkan-ready.sh x86_64|aarch64 OUT"
arch=${1:?$usage}
out=${2:?$usage}
case $arch in
x86_64) platform=linux/amd64 ;;
aarch64) platform=linux/arm64 ;;
*)
    echo "$usage" >&2
    exit 2
    ;;
esac

here=$(cd "$(dirname "$0")" && pwd)
hex=$(cd "$here/../.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)
if [ -n "$(ls -A "$out")" ]; then
    echo "vulkan-ready.sh: $out is not empty" >&2
    exit 1
fi
# shellcheck source=vulkan-ready.env
. "$here/vulkan-ready.env"
# The self-test's Program deadline: what a Linux node's GPU pool start gives its Program at most, its first
# dial's bound (hex_runtime::guest::recorded_dial_bound over bench/native/guest/records.tsv).
wait_ns=$(python3 -I "$here/vulkan-ready-manifest.py" first-dial-ns)

docker run --rm --platform "$platform" --memory "${VULKAN_READY_MEMORY:-4g}" --cpus "${VULKAN_READY_CPUS:-2}" \
    -v "$hex:/hex:ro" -v "$out:/out" \
    -e SNAPSHOT="$SNAPSHOT" -e PACKAGES="$PACKAGES" -e IMAGE="$IMAGE" -e OWNER="$(id -u):$(id -g)" \
    -e WAIT_NS="$wait_ns" \
    "$IMAGE" bash -euo pipefail -c '
        # Whatever ends the build, the caller can remove OUT.
        trap "chown -R $OWNER /out" EXIT
        rm -f /etc/apt/sources.list.d/*
        echo "deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/$SNAPSHOT trixie main" \
            >/etc/apt/sources.list
        export DEBIAN_FRONTEND=noninteractive
        apt-get -o Acquire::Check-Valid-Until=false update -q >/dev/null
        # shellcheck disable=SC2086
        apt-get install -y -q --no-install-recommends $PACKAGES >/dev/null
        multiarch=$(gcc -print-multiarch)
        mkdir -p /out/closure/bin /out/closure/lib /out/closure/icd /out/selftest /out/calibrator
        program=/out/closure/bin/hex-vulkan-guest-ready
        flags="-std=c11 -O2 -Wall -Wextra -Wpedantic -Wconversion -Werror"
        # shellcheck disable=SC2086
        gcc $flags -o "$program" /hex/crates/hex-runtime/src/guest/observer/hex-vulkan-guest-ready.c -lvulkan
        # shellcheck disable=SC2086
        gcc $flags -I/hex/crates/hex-runtime/src/guest/observer -o /out/selftest/sizes \
            /hex/scripts/guest/vulkan-ready-sizes.c -lvulkan
        /out/selftest/sizes >/out/allocation.txt
        # shellcheck disable=SC2086
        gcc $flags -static -o /out/calibrator/hex-vulkan-ready-calibrate /hex/scripts/guest/vulkan-ready-calibrate.c
        driver=/usr/lib/$multiarch/libvulkan_virtio.so
        interpreter=$(readelf -l "$program" | sed -n "s/.*Requesting program interpreter: \(.*\)]/\1/p")
        [ -f "$driver" ] && [ -n "$interpreter" ]
        cp -L "$interpreter" "/out/closure/lib/$(basename "$interpreter")"
        cp -L "$driver" /out/closure/lib/libvulkan_virtio.so
        # Every library the dynamic linker resolves for the Program and the Venus driver, by SONAME.
        for object in "$program" "$driver"; do ldd "$object"; done |
            sed -n "s/^[[:space:]]*\([^ ]*\) => \(\/[^ ]*\) .*/\1 \2/p" | sort -u |
            while read -r soname path; do cp -L "$path" "/out/closure/lib/$soname"; done
        api=$(sed -n "s/.*\"api_version\": *\"\([0-9.]*\)\".*/\1/p" /usr/share/vulkan/icd.d/virtio_icd.json)
        [ -n "$api" ]
        printf "{\n    \"file_format_version\": \"1.0.0\",\n    \"ICD\": {\n        \"library_path\": \"../lib/libvulkan_virtio.so\",\n        \"api_version\": \"%s\"\n    }\n}\n" "$api" \
            >/out/closure/icd/virtio_icd.json

        # Self-test: the closure resolves the real driver alone, and the built Program completes a frame
        # through the dynamic linker, libc and loader of the closure against the fake Venus ICD.
        linker="/out/closure/lib/$(basename "$interpreter")"
        "$linker" --library-path /out/closure/lib --list /out/closure/lib/libvulkan_virtio.so \
            >/out/selftest/closure.txt
        if grep -q "not found" /out/selftest/closure.txt ||
            sed -n "s/.* => \(\/[^ ]*\) .*/\1/p" /out/selftest/closure.txt | grep -v "^/out/closure/lib/"; then
            echo "vulkan-ready.sh: the Venus driver resolves outside its closure" >&2
            exit 1
        fi
        gcc -std=c11 -O2 -Wall -Wextra -Werror -fPIC -shared -fvisibility=hidden \
            -o /out/selftest/libfake_venus.so /hex/scripts/guest/fake-venus-icd.c
        printf "{\n    \"file_format_version\": \"1.0.0\",\n    \"ICD\": {\n        \"library_path\": \"./libfake_venus.so\",\n        \"api_version\": \"1.3.0\"\n    }\n}\n" \
            >/out/selftest/fake_icd.json
        unit() { sed -n "s/^$1 \([0-9]*\)$/\1/p" /out/allocation.txt; }
        physical=$(unit physical-device) observation=$(unit observation) family=$(unit queue-family)
        size_max=$(unit size-max)
        [ -n "$physical" ] && [ -n "$observation" ] && [ -n "$family" ] && [ -n "$size_max" ]
        # The fake device: QUEUES families, ALLOCATION bytes for one device; the frame bound is the decoder'"'"'s.
        program_run() {
            env -i VK_DRIVER_FILES=/out/selftest/fake_icd.json VK_LOADER_LAYERS_DISABLE="~all~" \
                "$linker" --library-path /out/closure/lib "$program" \
                --expected-devices 1 --max-queue-families "$1" --max-allocation-bytes "$2" \
                --max-output-bytes "$size_max" --icd /out/selftest/fake_icd.json --wait-ns "$WAIT_NS"
        }
        # A family bound under the device'"'"'s two runs out, and the refusal says the count it needs.
        if program_run 1 $((physical + observation + family)) >/dev/null 2>/out/selftest/refused-queues.txt; then
            exit 1
        fi
        [ "$(cat /out/selftest/refused-queues.txt)" = "hex-vulkan-guest-ready: queue-families 2" ]
        # Exactly its three storage() calls complete; a byte less is refused at its allocation.
        need=$((physical + observation + 2 * family))
        program_run 2 "$need" >/out/selftest/frame.txt
        if program_run 2 $((need - 1)) >/dev/null 2>/out/selftest/refused-allocation.txt; then
            exit 1
        fi
        [ "$(cat /out/selftest/refused-allocation.txt)" = "hex-vulkan-guest-ready: allocation -1" ]
        {
            echo "image $IMAGE"
            echo "snapshot $SNAPSHOT"
            gcc --version | sed -n 1p
            dpkg-query -W -f="\${Package}=\${Version}\n" gcc-14 libc6 libc6-dev libvulkan1 libvulkan-dev mesa-vulkan-drivers
        } >/out/toolchain.txt
    '
if command -v sha256sum >/dev/null; then
    sum=(sha256sum)
else
    sum=(shasum -a 256)
fi
(cd "$out/closure" && find . -type f | LC_ALL=C sort | sed 's|^\./||' | while read -r f; do "${sum[@]}" "$f"; done) \
    >"$out/closure.sha256"
cat "$out/closure.sha256"
