#!/usr/bin/env bash
# The runtime-tools half of goal 17's record (mntns_record.sh's RT and CRUN): opencontainers/runtime-tools at bd3b0bd,
# the revision goal 6's LSM records ran (goal6-lsm.md), built (its runtimetest and validation/*/*.t), and crun 1.27's
# static release, the reference those records name (crun 1.27, sha256 2a80f801...), checked against its whole sha256
# (first read whole on adacachy2, 2026-11-08, unit hex-g6-rtsetup-20261108T214202Z, matching the record's prefix).
# Rebuilds nothing already there. Needs git, make, Go and curl; nothing runs as root.
#   rt_setup.sh <dir>   then RT=<dir>/runtime-tools CRUN=<dir>/crun-1.27
set -euo pipefail
D=${1:?dir}; mkdir -p "$D"; cd "$D"
[ -d runtime-tools ] || git clone -q https://github.com/opencontainers/runtime-tools
git -C runtime-tools checkout -q bd3b0bd
echo "runtime-tools $(git -C runtime-tools log -1 --format='%h %cs %s' | cut -c1-80); $(go version)"
GOFLAGS=-p=2 make -C runtime-tools runtimetest validation-executables > make.log 2>&1 || { tail -30 make.log; exit 3; }
echo "built $(ls runtime-tools/validation/*/*.t | wc -l) validation tests and runtimetest"
[ -s crun-1.27 ] || curl -fsSL -o crun-1.27 https://github.com/containers/crun/releases/download/1.27/crun-1.27-linux-amd64
chmod +x crun-1.27
sum=$(sha256sum crun-1.27 | cut -d' ' -f1)
case $sum in
  2a80f801a0eeffadb1dca02928e329fbc88dd950fbb6e34e00abeaf9407f7ebf) echo "crun-1.27 sha256 $sum, the record's" ;;
  *) echo "refusing: crun-1.27's sha256 $sum isn't the record's"; exit 4 ;;
esac
./crun-1.27 --version | head -1
