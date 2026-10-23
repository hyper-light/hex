#!/usr/bin/env bash
# bench/competitive/host.sh: Hex's competitive baseline (goal 3). Runs on the bench host (fedora) only,
# in its bench-calendar slot, under a systemd user unit so it outlives the shell that starts it:
#   systemd-run --user --unit=hexbench --collect bash bench/competitive/host.sh <hex release binary>
# Env (defaults): CPUSET 4-31, ROUNDS 3, N 10000, WARMUP 100, N_OVERRIDE "runsc:3000", LIFECYCLES 2000,
# CAP 1000, BUDGET_MIB 8192 (OCI density), BOX_BUDGET_MIB 4096, PHASES "oci box".
# A later pass in the same run directory: OUT_DIR names it, ROUND_FIRST (1) numbers its rounds after the
# ones there, HEXBENCH_PHASES picks the drivers' phases (all), and FETCH=0 keeps the competitors' binaries
# already in $BASE/bin (their sha256 is recorded) rather than resolving each project's latest release again.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
HEX=$(realpath "${1:?hex release binary}")
case "$(hostname)" in fedora*) ;; *) echo "refusing: the bench host is fedora, not $(hostname)" >&2; exit 2 ;; esac
export CPUSET=${CPUSET:-4-31} ROUNDS=${ROUNDS:-3} N=${N:-10000} WARMUP=${WARMUP:-100}
export N_OVERRIDE=${N_OVERRIDE:-runsc:3000} LIFECYCLES=${LIFECYCLES:-2000} CAP=${CAP:-1000} BUDGET_MIB=${BUDGET_MIB:-8192}
BOX_BUDGET_MIB=${BOX_BUDGET_MIB:-4096}; PHASES=${PHASES:-"oci box"}
export ROUND_FIRST=${ROUND_FIRST:-1} HEXBENCH_PHASES=${HEXBENCH_PHASES:-}
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
# OUT_DIR adds a pass (e.g. RUNTIMES=runsc PHASES=oci) to an existing run directory.
OUT=${OUT_DIR:-"$BASE/out/$(date -u +%Y%m%dT%H%M%SZ)"}
mkdir -p "$OUT" "$BASE/bin"

# CLAUDE.md §7: load average under 2. Wait up to 15 minutes for it; refuse rather than measure noise.
for i in $(seq 1 90); do
  load=$(cut -d' ' -f1 /proc/loadavg)
  awk -v l="$load" 'BEGIN { exit !(l < 2) }' && break
  [ "$i" = 90 ] && { echo "refusing: 1-minute load average $load is not under 2" >&2; exit 3; }
  sleep 10
done

{ echo "--- pass started $(date -u +%FT%TZ): PHASES=$PHASES RUNTIMES=${RUNTIMES:-all}"; echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to $CPUSET"
  echo "memory: $(free -g | awk '/^Mem/ {print $2}') GiB"; echo "load at start: $(cat /proc/loadavg)"
  echo "hex: $HEX ($(sha256sum "$HEX" | cut -c1-16))"; echo "params: ROUNDS=$ROUNDS ROUND_FIRST=$ROUND_FIRST N=$N WARMUP=$WARMUP N_OVERRIDE=$N_OVERRIDE LIFECYCLES=$LIFECYCLES CAP=$CAP BUDGET_MIB=$BUDGET_MIB BOX_BUDGET_MIB=$BOX_BUDGET_MIB HEXBENCH_PHASES=${HEXBENCH_PHASES:-all}"
} >> "$OUT/env.txt"

if [ "${FETCH:-1}" = 0 ]; then
  # Every file, runsc's gvisor-bin/ Sentry set included: a bare glob would hand sha256sum a directory.
  (cd "$BASE/bin" && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$OUT/versions.$(date -u +%H%M%S).kept.sha256"
else
  python3 -I "$HERE/fetch.py" "$BASE" > "$OUT/versions.$(date -u +%H%M%S).json"
fi
cp "$HEX" "$BASE/bin/hex"

if [[ " $PHASES " == *" oci "* ]]; then
  NAME="hexbench-oci-$$"
  cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
  trap cleanup EXIT
  # One privileged container, private cgroup namespace, no network; host /usr read-only provides the
  # workloads and python3. --memory bounds the density phase on top of its own budget.
  timeout 36000 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="$CPUSET" --memory=40g \
    --privileged --security-opt label=disable --cgroupns=private --network=none \
    -v /usr:/usr:ro -v "$BASE/bin:/opt/bin:ro" -v "$HERE:/opt/lab:ro" -v "$OUT:/opt/out" \
    -e RUNTIMES -e ROUNDS -e ROUND_FIRST -e N -e WARMUP -e N_OVERRIDE -e LIFECYCLES -e CAP -e BUDGET_MIB -e HEXBENCH_PHASES \
    registry.fedoraproject.org/fedora:44 bash /opt/lab/oci_inside.sh 2>&1 | tee "$OUT/oci.log"
fi

if [[ " $PHASES " == *" box "* ]]; then
  for round in $(seq "$ROUND_FIRST" $((ROUND_FIRST + ROUNDS - 1))); do
    # A delegated user service: box_bench.py makes its worker cgroups under it (Delegate=yes).
    timeout 36000 systemd-run --user --unit="hexbench-box-$$-$round" --property=Delegate=yes --wait --collect --pipe \
      --setenv=HEXBENCH_PHASES="$HEXBENCH_PHASES" \
      taskset -c "$CPUSET" python3 -I "$HERE/box_bench.py" "$OUT/box.r$round.json" "$N" "$WARMUP" "$LIFECYCLES" \
      "$CAP" "$BOX_BUDGET_MIB" "$BASE/bin/hex" 2>&1 | tee -a "$OUT/box.log"
  done
fi

echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
python3 -I "$HERE/summarize.py" "$OUT" > "$OUT/table.md"
echo "results: $OUT"
