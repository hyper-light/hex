#!/usr/bin/env bash
# The create breakdown's AC-e run (bench/competitive/probes/create_breakdown_inside.sh, 8 creates in flight) in
# vm7_lab.sh's Linux 7.2 guest, for goal 6's under-load bar where the bench host's 6.19 lacks the calls (decision
# 6's update). Not of record: a VM's numbers. The run's directory (OUT_DIR, default under VM_DIR/breakdown) is the
# guest's one share: bin/ holds the given hex and seam_oci and idlethreads (built from probes/idlethreads.c here,
# for the guest's Fedora 44), lab/ a copy of bench/competitive, env the variables below that are set, and out/ the
# breakdown's results, handed back. In the guest vm7_breakdown_inside.sh starts IDLE_THREADS idle threads and
# LOOPS busy loops at nice 0 on every vCPU, in a cgroup beside the bench's, and the breakdown in a cgroup and
# mount namespace of its own, as the bench container on a host. VM_CPUS, VM_SMP and VM_MEM size the guest. PROBE names
# another probe of bench/competitive/probes to run so (css_cost_inside.sh, with N lifecycles; SUPERBLOCKS and
# CSS_FNS_EXTRA as it reads them).
# Usage: vm7_breakdown.sh <hex release binary> <seam_oci release binary>
set -euo pipefail
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); COMP=$(dirname "$HERE")/competitive
VM_DIR=${VM_DIR:-$HOME/.cache/hex-t7/g6/vm}
D=${OUT_DIR:-$VM_DIR/breakdown/$(date -u +%Y%m%dT%H%M%SZ)-$$}
mkdir -p "$D"/bin "$D"/out
[ -z "$(ls -A "$D/out")" ] || { echo "refusing: $D/out isn't empty" >&2; exit 2; }
cp "$HEX" "$D/bin/hex"; cp "$SEAM" "$D/bin/seam_oci"
gcc -O2 -pthread -o "$D/bin/idlethreads" "$COMP/probes/idlethreads.c"
rm -rf "${D:?}/lab"; cp -r "$COMP" "$D/lab"
# Only what's set reaches the guest; an unset one stays unset there.
: > "$D/env"
for v in AC_E AC_E_MOUNTS IDLE_THREADS LOOPS HEX_OCI_MNTNS HEXBENCH_OWNER \
  HEXBENCH_REUSE HEXBENCH_DOCKER_PATHS PROBE N SUPERBLOCKS SUPERBLOCKS_ALONE CSS_FNS_EXTRA; do
  [ -n "${!v+x}" ] && printf 'export %s=%q\n' "$v" "${!v}" >> "$D/env"
done
printf 'export TRACE_TAG=vm7_%s\n' "$(date -u +%Y%m%dT%H%M%SZ)" >> "$D/env"
{ echo "host: $(hostname), kernel $(uname -r), load $(cut -d' ' -f1-3 /proc/loadavg), date $(date -u +%FT%TZ)"
  echo "guest: VM_CPUS=${VM_CPUS:-16-31} VM_SMP=${VM_SMP:-8} VM_MEM=${VM_MEM:-6G}"
  echo "binaries: $(cd "$D/bin" && sha256sum hex seam_oci | cut -c1-16 | paste -sd' ')"
  echo "harness: bench at $(git -C "$HERE" rev-parse --short HEAD)$(git -C "$HERE" diff --quiet HEAD -- .. || echo ' with uncommitted changes')"
  echo "containers: $(docker ps --format '{{.Names}}' | paste -sd, -)"
  sed 's/^export /env: /' "$D/env"
} > "$D/host.txt"
rc=0
VM7_MODE=breakdown bash "$HERE/vm7_lab.sh" "$D" "$D/bin/seam_oci" || rc=$?
echo "host at end: load $(cut -d' ' -f1-3 /proc/loadavg), date $(date -u +%FT%TZ)" >> "$D/host.txt"
echo "results: $D/out"
exit "$rc"
