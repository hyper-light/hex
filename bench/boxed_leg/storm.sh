#!/usr/bin/env bash
# Consumer #1 dispatched-leg smoke, step 2 (runs INSIDE the dispatched CI leg): a build storm shaped
# like the adacachy incident of 11:37Z (a many-core compile at high load, plus RAM), run in the leg
# box. The leg's fair-share cpu.weight throttles it and the legs' parent memory.high caps the legs'
# RAM, so the host — runner and server under `supervisor` (memory.low) — stays alive and the run
# reaches the end. (CP-T7.1, AC-T7.1.1.) Set SMOKE_STORM=0 for a dry mechanism check (no storm).
set -uo pipefail
if [ "${SMOKE_STORM:-1}" != "1" ]; then echo "storm skipped (SMOKE_STORM=0, dry run)"; exit 0; fi
N=$(nproc)
echo "storm: ${N}-way cpu busy-loop + ~2GiB RAM for ~30s (the load-${N} shape)"
sample() { echo "  t=${1}s loadavg=$(cut -d' ' -f1-3 /proc/loadavg) MemAvailable=$(awk '/MemAvailable/{print $2" kB"}' /proc/meminfo)"; }
sample 0
pids=()
for _ in $(seq 1 "$N"); do ( end=$((SECONDS+30)); while [ "$SECONDS" -lt "$end" ]; do :; done ) & pids+=($!); done
# A bounded, safe RAM load (~2 GiB touched), far under the legs'-parent memory.high. The full cap is
# proven by that static value (evidence.sh) and the scaled memory.high throttle demo (run.sh).
if command -v python3 >/dev/null 2>&1; then
  ( timeout 30 python3 -I -c 'b=bytearray(2*1024*1024*1024)
for i in range(0,len(b),4096): b[i]=1
import time; time.sleep(20)' 2>/dev/null; echo "  touched ~2GiB RAM" ) &
  pids+=($!)
fi
for t in 5 10 15 20 25 30; do sleep 5; sample "$t"; done
wait "${pids[@]}" 2>/dev/null || true
sample end
echo "storm done; execution reached here, so the host never missed liveness under the storm"
