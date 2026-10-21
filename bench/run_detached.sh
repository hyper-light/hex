#!/usr/bin/env bash
# Detached CP-T7.1 bench runner (coordinator's instruction, 2026-10-21). Runs on fedora, fully
# detached from the launching session. It:
#   1. builds the hex release binary (fine under load),
#   2. waits until the 1-minute load stays below 2 for 5 minutes (checked every 30 s), deadline
#      06:00Z — fedora's bench slot opens at the account's usage pause (~22:15Z),
#   3. runs noisy_neighbor (AC-T7.1.2, fedora NVMe) and hex_start (AC-T7.1.3), each best-effort,
#   4. records every result with its conditions under ~/.cache/hex-t7/bench-<ts>/.
# It runs from a private copy of the repo (the launcher rsyncs it to a stable dir) so a repo re-sync
# or the session ending cannot pull the ground out from under it.
set -uo pipefail

TS=$(date -u +%Y%m%dT%H%M%SZ)
OUT="$HOME/.cache/hex-t7/bench-$TS"
mkdir -p "$OUT"
exec >>"$OUT/run.log" 2>&1
echo "START $(date -u) cwd=$(pwd) host=$(hostname)"
export CARGO_TARGET_DIR="$HOME/.cache/hex-t7/target"

echo "=== build release hex ==="
if ! nice -n 10 cargo build --release -p hex-cli -j 12; then
  echo "BUILD FAILED — cannot benchmark"
  exit 1
fi
HEX="$CARGO_TARGET_DIR/release/hex"
echo "hex binary: $HEX"
nice -n 10 cargo build --release --bench noisy_neighbor -j 12 || echo "WARN: noisy_neighbor prebuild failed (will retry at measure time)"

echo "=== wait for load<2 sustained 5 min (deadline 06:00Z) ==="
deadline=$(date -u -d 'tomorrow 06:00' +%s)
today6=$(date -u -d 'today 06:00' +%s)
now=$(date -u +%s)
if [ "$now" -lt "$today6" ]; then deadline=$today6; fi
below=0
quiet=0
while :; do
  now=$(date -u +%s)
  if [ "$now" -ge "$deadline" ]; then
    echo "$(date -u +%H:%MZ) DEADLINE 06:00Z reached without a sustained quiet window; recording anyway"
    break
  fi
  l1=$(awk '{print $1}' /proc/loadavg)
  if awk "BEGIN{exit !($l1 < 2)}"; then below=$((below + 30)); else below=0; fi
  echo "$(date -u +%H:%MZ) load1=$l1 below2_for=${below}s"
  if [ "$below" -ge 300 ]; then
    echo "$(date -u +%H:%MZ) QUIET: load<2 sustained 5 min"
    quiet=1
    break
  fi
  sleep 30
done

{
  echo "when=$(date -u)"
  echo "sustained_quiet_window=$quiet"
  uptime
  echo "nproc=$(nproc)"
  grep -E 'MemTotal|MemAvailable' /proc/meminfo
} | tee "$OUT/conditions.txt"

echo "=== noisy_neighbor (AC-T7.1.2, fedora NVMe) ==="
if nice -n 10 cargo bench --bench noisy_neighbor -j 12 >"$OUT/noisy_neighbor.txt" 2>&1; then
  grep -E 'noisy_neighbor:|AC-T7.1.2' "$OUT/noisy_neighbor.txt" || tail -6 "$OUT/noisy_neighbor.txt"
else
  echo "noisy_neighbor FAILED (see noisy_neighbor.txt)"
  tail -15 "$OUT/noisy_neighbor.txt"
fi

echo "=== hex_start (AC-T7.1.3) ==="
if command -v docker >/dev/null 2>&1; then
  if bash bench/hex_start/run.sh "$HEX" 200 >"$OUT/hex_start.txt" 2>&1; then
    tail -8 "$OUT/hex_start.txt"
  else
    echo "hex_start FAILED (see hex_start.txt)"
    tail -15 "$OUT/hex_start.txt"
  fi
else
  echo "hex_start SKIP: no docker on this host"
fi

echo "DONE $(date -u). Results in $OUT"
