#!/usr/bin/env bash
# The AppArmor half of goal 6's LSM checks (AC-T7.1.4), in vmaa_build.sh's Ubuntu 24.04 guest under KVM:
#   1. builds the guest once (vmaa_build.sh);
#   2. builds hex (release) and the oci_lsm test in an ubuntu:24.04 container from <tree>, so the binaries
#      link the guest's own glibc, into <out>/target, which vm7_lab.sh shares into the guest at its path;
#   3. boots the guest to run oci_lsm as root (vm7_lab.sh's seam mode), with acme_secure_profile loaded;
#   4. boots it again to run runtime-tools' validation suite against that hex (vm7_validate.sh), leaving
#      <out>/validate/out/results.tsv and each test's log;
#   5. and again against the guest's crun, the reference on the same kernel, into <out>/crun/out.
# Every container and the guest are held to the shared host's caps (CPUs 16-31, BENCH_MEMORY, BENCH_OOM).
# Usage: aa_lab.sh <repo tree> <runtime-tools checkout> <out dir>
set -euo pipefail
TREE=$(realpath "${1:?repo tree}")
RT=$(realpath "${2:?runtime-tools}")
OUT=$(realpath -m "${3:?out dir}")
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
export VM_DIR=${VM_DIR:-$HOME/.cache/hex-t7/lsm/vm-aa}
say() { echo "$(date -u +%FT%TZ) $*"; }
[ ! -e "$OUT" ] || { echo "refusing: $OUT exists; each run gets a new dir" >&2; exit 2; }
mkdir -p "$OUT/target"
TGT=$OUT/target

bash "$HERE/vmaa_build.sh"

say "build hex and oci_lsm for the guest (ubuntu:24.04)"
docker run --rm --cpuset-cpus "${VM_CPUS:-16-31}" --memory "${BENCH_MEMORY:-8g}" --oom-score-adj "${BENCH_OOM:-1000}" \
  -v "$TREE:/src:ro" -v "$TGT:$TGT" -v hexlsm-cargo:/cargo -v hexlsm-rustup:/rustup \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR="$TGT/build" -e CARGO_BUILD_JOBS=4 \
  -e PATH=/cargo/bin:/usr/bin:/bin -e TGT="$TGT" -e OWNER="$(id -u):$(id -g)" -w /src ubuntu:24.04 bash -euo pipefail -c '
    export DEBIAN_FRONTEND=noninteractive
    apt-get -qq update && apt-get -qq install -y --no-install-recommends curl ca-certificates gcc libc6-dev > /dev/null
    [ -x /cargo/bin/rustup ] || curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none
    for i in 1 2 3; do rustup toolchain install 1.99.1 --profile minimal && break; sleep 10; done
    nice -n 10 cargo build --release --locked -p hex-cli
    nice -n 10 cargo test --locked -p hex-cli --test oci_lsm --no-run 2> "$TGT/test-build.log" || { cat "$TGT/test-build.log"; exit 1; }
    cp "$(ls -t "$TGT"/build/debug/deps/oci_lsm-* | grep -v "\.d$" | head -1)" "$TGT/oci_lsm"
    chown -R "$OWNER" "$TGT"'

say "oci_lsm in the AppArmor guest"
set +e
bash "$HERE/vm7_lab.sh" "$TGT" "$TGT/oci_lsm"
seam=$?
say "runtime-tools in the AppArmor guest"
VM_TAR=0 bash "$HERE/vm7_validate.sh" "$RT" "$TGT/build/release/hex" "$OUT/validate"
validate=$?
say "runtime-tools against crun in the AppArmor guest"
# vm7_validate.sh runs `<binary> oci <verb>`; this stands in for the binary and hands the verb to crun.
printf '#!/bin/sh\nshift\nexec crun "$@"\n' > "$OUT/crun-as-hex"; chmod +x "$OUT/crun-as-hex"
VM_TAR=0 bash "$HERE/vm7_validate.sh" "$RT" "$OUT/crun-as-hex" "$OUT/crun"
set -e
for r in validate crun; do
  echo "$r: $(cut -f2 "$OUT/$r/out/results.tsv" 2>/dev/null | sort | uniq -c | tr '\n' ' ')"
  grep -E 'linux_mount_label|linux_process_apparmor_profile' "$OUT/$r/out/results.tsv" 2>/dev/null
done
say "seam exit $seam, validate exit $validate"
[ "$seam" = 0 ] && [ "$validate" = 0 ]
