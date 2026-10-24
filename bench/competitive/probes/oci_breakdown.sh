#!/usr/bin/env bash
# bench/competitive/probes: runs oci_breakdown_inside.sh in the bench container, as host.sh runs the OCI
# pass, on the competitors' binaries host.sh fetched ($HEX_BENCH/bin) and the given Hex binary. On fedora,
# in a quiet moment, under a systemd user unit:
#   systemd-run --user --unit=hexbench-breakdown --collect bash bench/competitive/probes/oci_breakdown.sh <hex>
# Env: N, WARMUP, LIFECYCLES, RUNTIMES, STEPS (oci_breakdown_inside.sh's), CPUSET (4-31), and OLD_DRIVER_REV: a
# commit whose oci_bench.py step 5 also runs, right after this tree's, to compare the two drivers' ceilings.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
HEX=$(realpath "${1:?hex release binary}")
case "$(hostname)" in fedora*) ;; *) echo "refusing: the bench host is fedora, not $(hostname)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
OUT="$BASE/out/breakdown-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT"
[ -x "$BASE/bin/crun" ] || { echo "refusing: no $BASE/bin/crun (host.sh fetches it)" >&2; exit 4; }
# CLAUDE.md §7: load average under 2, waited for up to 15 minutes, as host.sh does.
for i in $(seq 1 90); do
  load=$(cut -d' ' -f1 /proc/loadavg)
  awk -v l="$load" 'BEGIN { exit !(l < 2) }' && break
  [ "$i" = 90 ] && { echo "refusing: 1-minute load average $load is not under 2" >&2; exit 3; }
  sleep 10
done
{ echo "hex: $HEX ($(sha256sum "$HEX" | cut -c1-16))"; (cd "$BASE/bin" && sha256sum crun); } > "$OUT/versions.txt"
if [ -n "${OLD_DRIVER_REV:-}" ]; then
  mkdir -p "$OUT/old-driver"
  for f in oci_bench.py cgmem.py; do git -C "$LAB" show "$OLD_DRIVER_REV:bench/competitive/$f" > "$OUT/old-driver/$f"; done
  echo "old driver: $OLD_DRIVER_REV" >> "$OUT/versions.txt"
fi
NAME=hexbench-breakdown-$$
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
timeout 7200 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
  --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$BASE/bin:/opt/bin:ro" -v "$HEX:/opt/bin/hex:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" \
  -e N -e WARMUP -e LIFECYCLES -e RUNTIMES -e STEPS \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/oci_breakdown_inside.sh 2>&1 | tee "$OUT/probe.log"
echo "results: $OUT"
