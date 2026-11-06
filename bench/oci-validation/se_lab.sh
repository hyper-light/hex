#!/usr/bin/env bash
# The SELinux half of goal 6's LSM checks (AC-T7.1.4), on an SELinux-enforcing host (fedora): runtime-tools'
# validation suite against <hex> (run_hex.sh) and against the host's crun (run.sh), each in the privileged
# container those scripts make. The kernel there runs SELinux, though the container mounts no selinuxfs, so
# Hex applies linux_mount_label's label, and skips linux_process_apparmor_profile's profile with a warning
# (hex/), or refuses it by name under HEX_OCI_LSM=strict (hex-strict/)
# (docs/design/runtime.md, "SELinux mount labels and AppArmor profiles"). Prints the host's state first, then
# each runtime's counts and the two LSM tests' rows. BENCH_MEMORY and BENCH_OOM bound the containers.
# Usage: se_lab.sh <runtime-tools checkout> <hex binary> <out dir>
set -euo pipefail
RT=$(realpath "${1:?runtime-tools}")
HEX=$(realpath "${2:?hex binary}")
OUT=$(realpath -m "${3:?out dir}")
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
[ ! -e "$OUT" ] || { echo "refusing: $OUT exists; each run gets a new dir" >&2; exit 2; }
mkdir -p "$OUT"
{ date -u; hostname; uname -r; getenforce; uptime; crun --version | head -1; docker ps --format '{{.Names}}'; } | tee "$OUT/host.txt"
bash "$HERE/run_hex.sh" "$RT" "$HEX" "$OUT/hex"
HEX_OCI_LSM=strict bash "$HERE/run_hex.sh" "$RT" "$HEX" "$OUT/hex-strict"
bash "$HERE/run.sh" "$RT" "$(command -v crun)" "$OUT/crun"
for r in hex hex-strict crun; do
  echo "$r: $(cut -f2 "$OUT/$r/results.tsv" | sort | uniq -c | tr '\n' ' ')"
  grep -E 'linux_mount_label|linux_process_apparmor_profile' "$OUT/$r/results.tsv"
done
