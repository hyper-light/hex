#!/usr/bin/env bash
# Goal 6's record of the mount-namespace ways (decision 6's update; docs/research/runtime/goal6-mntns.md, its §16): the
# copy (the classic way), the namespace made from the tree (open_tree's OPEN_TREE_NAMESPACE, Linux 7.0) and the empty
# namespace (clone3's CLONE_EMPTY_MNTNS, Linux 7.1), interleaved in one run under the under-load bar's conditions, so
# the fastest becomes the default by bench/competitive/probes/mntns_default.txt.
#
#   mntns_record.sh probe   whether this kernel takes the two calls (bench/oci-validation/vm7_probe.py, rootless)
#   mntns_record.sh host    of record, on a Linux 7.1+ host: the one command after fedora's move to 7.2
#   mntns_record.sh guest   the same in the 7.2 guest (bench/oci-validation/vm7_breakdown.sh), indicative only
#
# host and guest build release hex and seam_oci from HEAD by git archive (their own CARGO_TARGET_DIR), take the timed
# lock, and run ROUNDS rounds (default 3) at each of LOOPS_SET (default "6 12") busy loops, the three ways in an order
# rotated each round. Each cell is a fresh node: MOUNTS creates (default 2000) at 8 in flight with 8,000 idle threads,
# owner bundles with reuse on, Docker's paths, the /dev pool on. The phase runs once untraced (its create p50, the
# record's) and once traced (part 19). Each cell's mount-namespace counts must show the way it was asked, or the run
# refuses. The record lines go to the run's record.txt for mntns_default.txt. Start it in a hex-slot unit (fedora's
# rule), e.g.:
#   ~/.cache/hex-t7/bin/hex-slot hex-g6-rec-$(date -u +%Y%m%dT%H%M%SZ) -p MemoryHigh=6G -p MemoryMax=8G \
#     -p OOMScoreAdjust=1000 -p AllowedCPUs=16-31 -p Delegate=yes -- /usr/bin/bash "$PWD/bench/competitive/probes/mntns_record.sh" host
set -uo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
REPO=$(git -C "$HERE" rev-parse --show-toplevel) || { echo "refusing: not in a git checkout" >&2; exit 2; }
MODE=${1:-}
probe() { unshare --user --map-root-user --mount python3 -I "$REPO/bench/oci-validation/vm7_probe.py"; }
case $MODE in
  probe) probe; exit $? ;;
  host|guest) ;;
  *) echo "usage: mntns_record.sh probe|host|guest" >&2; exit 2 ;;
esac

REV=$(git -C "$REPO" rev-parse --short HEAD)
RUN=mntns-$MODE-$(date -u +%Y%m%dT%H%M%SZ)
O=${OUT_ROOT:-$HOME/.cache/hex-t7/g6/rec}/$RUN
mkdir -p "$O" || exit 2
ROUNDS=${ROUNDS:-3}; LOOPS_SET=${LOOPS_SET:-6 12}; MOUNTS=${MOUNTS:-2000}
st() { echo "$(date -u +%FT%TZ) $*" | tee -a "$O/status"; }
st "$RUN at $REV ($(uname -r)), rounds $ROUNDS, loops $LOOPS_SET, $MOUNTS creates a cell"

if [ "$MODE" = host ]; then
  P=$(probe 2>&1); echo "$P" > "$O/probe.txt"
  grep -q 'OPEN_TREE_NAMESPACE supported' <<< "$P" && grep -q 'CLONE_EMPTY_MNTNS supported' <<< "$P" \
    || { st "refusing: the record needs a kernel with both calls (Linux 7.1+): $P"; exit 3; }
fi

# Release binaries of HEAD, from a git-archive snapshot with every source touched (cargo-target-dir-per-checkout).
SRC=$O/src
mkdir -p "$SRC" && git -C "$REPO" archive HEAD | tar -x -C "$SRC" && find "$SRC" -type f -exec touch {} + \
  || { st "refusing: no snapshot of $REV"; exit 2; }
export CARGO_TARGET_DIR=$HOME/.cache/hex-t7/g6/target-rec CARGO_BUILD_JOBS=4
( cd "$SRC" && nice -n 10 cargo build -j 4 --release --locked --bin hex && cp "$CARGO_TARGET_DIR/release/hex" "$O/hex" ) \
  > "$O/release.log" 2>&1 || { st "refusing: hex didn't build ($O/release.log)"; exit 4; }
( cd "$SRC" && nice -n 10 cargo bench -j 4 --no-run --locked -p hex-bench --bench seam_oci --message-format=json ) \
  > "$O/bench.json" 2> "$O/bench.log" || { st "refusing: seam_oci didn't build ($O/bench.log)"; exit 4; }
python3 -I - "$O/bench.json" "$O/seam_oci" <<'PY' || { st "refusing: no unique seam_oci"; exit 4; }
import json, shutil, sys
p = [m["executable"] for l in open(sys.argv[1]) if l.startswith("{") and (m := json.loads(l)).get("reason") == "compiler-artifact"
     and m.get("executable") and m.get("target", {}).get("name") == "seam_oci"]
if len(p) != 1:
    raise SystemExit(1)
shutil.copyfile(p[0], sys.argv[2]); shutil.copymode(p[0], sys.argv[2])
PY
st "built $(sha256sum "$O/hex" "$O/seam_oci" | cut -c1-16 | paste -sd' ')"

cell() { # cell <way> <loops> <round>
  local way=$1 L=$2 r=$3 name=$1-L$2-r$3 pids=() p rc res
  if [ "$MODE" = host ]; then
    for _ in $(seq 1 "$L"); do taskset -c 4-15 bash -c 'while :; do :; done' & pids+=("$!"); done
    sleep 2
    { echo "== $name $(date -u +%FT%TZ) uptime: $(uptime)"; echo "  docker ps: $(docker ps -q | wc -l) containers"; } >> "$O/context.txt"
    ( cd "$SRC" && HEX_OCI_MNTNS=$way MOUNTS=$MOUNTS MOUNTS_CONC=8 MOUNTS_PATH=pruned MOUNTS_UNTRACED=1 IDLE_THREADS=8000 \
        CPUSET=4-15 LOAD_GATE=0 BENCH_HOST_OK="$(hostname)" BENCH_MEMORY=8g BENCH_OOM=1000 HEXBENCH_DEVFS=1 \
        HEXBENCH_OWNER=bench-owner HEXBENCH_REUSE=1 HEXBENCH_DOCKER_PATHS=1 \
        timeout 900 bash bench/competitive/probes/create_breakdown.sh "$O/hex" "$O/seam_oci" ) > "$O/$name.log" 2>&1
    rc=$?
    for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
    for p in "${pids[@]}"; do wait "$p" 2>/dev/null; done
    res=$(grep -o 'results: .*' "$O/$name.log" | tail -1 | cut -d' ' -f2)
  else
    ( cd "$SRC" && HEX_OCI_MNTNS=$way MOUNTS=$MOUNTS MOUNTS_CONC=8 MOUNTS_SPAN=root MOUNTS_UNTRACED=1 IDLE_THREADS=8000 \
        LOOPS=$L HEXBENCH_DEVFS=1 HEXBENCH_OWNER=bench-owner HEXBENCH_REUSE=1 HEXBENCH_DOCKER_PATHS=1 \
        VM_CPUS=4-15 VM_SMP=12 VM_MEM=6G VM_TIMEOUT=1500 BENCH_MEMORY=8g BENCH_OOM=1000 OUT_DIR="$O/runs/$name" \
        timeout 1800 bash bench/oci-validation/vm7_breakdown.sh "$O/hex" "$O/seam_oci" ) > "$O/$name.log" 2>&1
    rc=$?
    res=$O/runs/$name/out
  fi
  echo "$(date -u +%FT%TZ) $name $rc $res" >> "$O/exits"
}

timed() {
  st "TIMED start (lock held) load $(cut -d' ' -f1-3 /proc/loadavg)"
  local ways=(copy tree empty) r L i
  for r in $(seq 1 "$ROUNDS"); do
    for L in $LOOPS_SET; do
      for i in 0 1 2; do cell "${ways[$(( (i + r) % 3 ))]}" "$L" "$r"; done
    done
  done
  st "TIMED end load $(cut -d' ' -f1-3 /proc/loadavg)"
}
export -f st cell timed
export O MODE SRC MOUNTS ROUNDS LOOPS_SET
flock -w 3600 /home/adalundhe/.cache/hex-t7/fedora-timed.lock bash -c timed || { st "refusing: the timed lock or the run failed"; exit 5; }

python3 -I "$HERE/mntns_record.py" "$O/exits" "$RUN" "$REV" "$MODE" | tee "$O/record.txt"
exit "${PIPESTATUS[0]}"
