#!/usr/bin/env bash
# Every Windows check of the native plan's steps 3, 4 and 6 on jupiter, in the order they run around the
# installer's one admin step (bench/install/RESULT.md). Run by kiln on jupiter (its shell is Git Bash), from
# the synced repository; each phase writes its record to bench/install/records/ and exits non-zero at the
# first failure.
#
#   bash bench/install/jupiter.sh build        build hex.exe and the Windows test binaries
#   bash bench/install/jupiter.sh gates        fmt, clippy -D warnings and the workspace's tests, natively
#   bash bench/install/jupiter.sh before       the installer lab's `before` phase (machine fresh of Hex)
#     -> an administrator at jupiter: windows.ps1 install -Binary <hex.exe> (one UAC prompt), and the reboot
#   bash bench/install/jupiter.sh installed    the lab's `installed` phase: every class working
#     -> an administrator at jupiter: windows.ps1 uninstall (one UAC prompt), and the reboot
#   bash bench/install/jupiter.sh uninstalled  the lab's `uninstalled` phase: nothing of Hex's left
#
# `installed` runs step 4's oci suite on the image layers bench/native/windows/oci_setup.ps1 wrote to
# $HEX_OCI_ROOT/layers.txt (default %USERPROFILE%\.cache\hex-native-s4, oci_setup.ps1's own default).
set -euo pipefail

phase=${1:?usage: jupiter.sh <build|gates|before|installed|uninstalled>}
repo=$(cd "$(dirname "$0")/../.." && pwd)
state=$HOME/.cache/hex-t7-windows
export CARGO_TARGET_DIR=$state/target
mkdir -p "$state" "$repo/bench/install/records"
commit=$(git -C "$repo" rev-parse --short HEAD)
stamp=$(date -u +%Y%m%dT%H%M%SZ)
record=$repo/bench/install/records/windows-$phase-$commit-$stamp.txt
plan=$state/plan.txt
bins=$state/bins.txt

win() { cygpath -w "$1"; }

# The test binaries' paths, as `cargo test --no-run` names them, one "<test> <path>" line each.
build() {
  cargo build --locked -p hex-cli --bin hex
  : > "$bins"
  for t in windows_process_box windows_oci_box; do
    exe=$(cargo test --locked -p hex-cli --test "$t" --no-run --message-format=json \
      | grep -o '"executable":"[^"]*"' | tail -1 | sed 's/^"executable":"//; s/"$//; s/\\\\/\\/g')
    [ -n "$exe" ] || { echo "no test binary for $t"; return 1; }
    echo "$t $exe" >> "$bins"
  done
  cat "$bins"
}

bin_of() { awk -v t="$1" '$1 == t { $1 = ""; sub(/^ /, ""); print }' "$bins"; }

lab() {
  local args=(-NoProfile -ExecutionPolicy Bypass -File "$(win "$repo/bench/install/windows_lab.ps1")" "$1"
    -Plan "$(win "$plan")" -Binary "$(win "$CARGO_TARGET_DIR/debug/hex.exe")"
    -TestBin "$(bin_of windows_process_box)" -OciTestBin "$(bin_of windows_oci_box)")
  if [ "$1" = installed ]; then
    local root=${HEX_OCI_ROOT:-$USERPROFILE\\.cache\\hex-native-s4}
    local layers
    layers=$(tr -d '\r\n' < "$(cygpath -u "$root")/layers.txt") \
      || { echo "no layers.txt under $root: run bench/native/windows/oci_setup.ps1 first"; return 1; }
    args+=(-Layers "$layers")
  fi
  powershell.exe "${args[@]}"
}

set +e
{
  set -e
  echo "jupiter.sh $phase: commit $commit, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host: $(hostname), $(powershell.exe -NoProfile -Command '[Environment]::OSVersion.VersionString' | tr -d '\r')"
  echo "load: $(powershell.exe -NoProfile -Command '(Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average' | tr -d '\r')% CPU"
  git -C "$repo" status --short | head -20
  case $phase in
    build) build ;;
    gates)
      cargo fmt --all --check
      cargo clippy --workspace --all-targets --locked -- -D warnings
      cargo test --workspace --locked
      ;;
    before | installed | uninstalled)
      [ -s "$bins" ] || build
      lab "$phase"
      ;;
    *) echo "unknown phase $phase"; exit 2 ;;
  esac
  echo "PHASE $phase: PASS"
} 2>&1 | tee "$record"
code=${PIPESTATUS[0]}
set -e
echo "record: $record (exit $code)"
exit "$code"
