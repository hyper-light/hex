#!/usr/bin/env bash
# Goal 17's run of record (decision 6's update; docs/research/runtime/goal6-mntns.md): on a host whose kernel has
# both calls (Linux 7.1 or later), the three ways a root node makes a pruned create's mount namespace, through goal 0's
# driver in process (seam_oci): the copy (the classic way), the namespace made from its tree (open_tree's
# OPEN_TREE_NAMESPACE, 7.0) and the empty namespace (clone3's CLONE_EMPTY_MNTNS, 7.1). They run interleaved round by
# round under the under-load bar's arms, best of 2, with runtime-tools' suite beside them, so the fastest becomes the
# default by bench/competitive/probes/mntns_default.txt.
#
#   mntns_record.sh probe   whether this kernel takes the two calls (bench/oci-validation/vm7_probe.py, rootless)
#   mntns_record.sh host    the run of record, in the harness's privileged container (create_breakdown.sh)
#
# host builds release hex and seam_oci from HEAD by git archive (their own CARGO_TARGET_DIR). Then PASSES passes (2:
# best of 2, as summarize.py takes it), each with the host's clock state (host_clock.sh) at its start and end, and one
# cell at each of LOOPS_SET busy loops ("6 12"), at nice 0 on CPUSET, the CPUs the cell's container runs on. A cell is
# create_breakdown.sh's MOUNTS_ARMS run: 8,000 idle threads on the host, owner bundles with reuse on, Docker's masked
# and read-only paths, the /dev pool on, and rounds.py's ARMS ("hex:copy hex:tree hex:empty"; add libcrun to run it
# beside them) for ROUNDS rounds after WARMUP, each round a burst of 8 lifecycles at 8 in flight on each arm, its order
# rotated and the host's load read, once untraced and once traced (part 19, to the init's chdir: MOUNTS_SPAN=root, so
# a namespace joined from its tree, which no chroot ends, is cut alike). Each cell's arms must show every create made
# the way it asked, or the record refuses it. Then runtime-tools' validation suite against this hex and against CRUN,
# each in its privileged container (run_hex.sh, run.sh; RT the built checkout). mntns_record.py reads it all into
# record.txt: the tables, goal 17's verdicts and the record lines for mntns_default.txt.
#
# ROUNDS is 299: the fewest rounds whose largest sample bounds the p99 with 95% confidence, N >= ln(0.05)/ln(0.99) =
# 298.07 (Ada's rule: a p99 claim stands on at least 299 interleaved rounds, the load recorded each round). WARMUP is
# 1: each arm's first burst meets its node cold, and is summarized apart. CPUSET is, unless set, the CPUs of the L3
# domain that doesn't hold CPU 0, where the host's interrupts and housekeeping land, read from sysfs; a host with one
# L3 domain needs CPUSET set. The cell's busy loops run on it, as the bar's are on the same cores. Every other
# variable that create_breakdown.sh reads may be set. Start it detached, its run bounded, e.g. on adacachy2 (RT and
# CRUN from bench/oci-validation/rt_setup.sh; 10,800 s holds the parts' own bounds, 4 cells x 708 s and 2 suites x 58
# tests x 60 s, 9,792 s, and the release builds, which have none of their own):
#   systemd-run --user --unit hex-g6-rec-$(date -u +%Y%m%dT%H%M%SZ) --collect -p MemoryHigh=3500M -p MemoryMax=7G \
#     -p OOMScoreAdjust=1000 -p RuntimeMaxSec=10800 -E RT=$HOME/.cache/hex-g6/rt/runtime-tools \
#     -E CRUN=$HOME/.cache/hex-g6/rt/crun-1.27 -- /usr/bin/bash "$PWD/bench/competitive/probes/mntns_record.sh" host
set -uo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
REPO=$(git -C "$HERE" rev-parse --show-toplevel) || { echo "refusing: not in a git checkout" >&2; exit 2; }
MODE=${1:-}
probe() { unshare --user --map-root-user --mount python3 -I "$REPO/bench/oci-validation/vm7_probe.py"; }
case $MODE in
  probe) probe; exit $? ;;
  host) ;;
  *) echo "usage: mntns_record.sh probe|host" >&2; exit 2 ;;
esac

REV=$(git -C "$REPO" rev-parse --short HEAD)
RUN=mntns-$MODE-$(date -u +%Y%m%dT%H%M%SZ)
O=${OUT_ROOT:-$HOME/.cache/hex-t7/g6/rec}/$RUN
mkdir -p "$O" || exit 2
PASSES=${PASSES:-2}; LOOPS_SET=${LOOPS_SET:-6 12}; ROUNDS=${ROUNDS:-299}; WARMUP=${WARMUP:-1}
ARMS=${ARMS:-hex:copy hex:tree hex:empty}
st() { echo "$(date -u +%FT%TZ) $*" | tee -a "$O/status"; }
# The run's containers go with it, however it ends (containers.sh): sourced once here, so the run's one reaper starts
# here and every script the run calls (each cell's create_breakdown.sh, run_hex.sh, run.sh) labels its containers with
# this run and starts none of its own, which would fail as the reaper already loaded and warn falsely.
. "$HERE/../containers.sh"

# The CPUs of each L3 domain, and the one this run takes: CPUSET, or the domain without CPU 0.
read -r DOMAINS < <(cat /sys/devices/system/cpu/cpu*/cache/index3/shared_cpu_list 2>/dev/null | sort -u | paste -sd' ')
if [ -z "${CPUSET:-}" ]; then
  CPUSET=$(python3 -I - $DOMAINS <<'PY'
import sys
def cpus(text):
    out = set()
    for part in text.split(","):
        a, _, b = part.partition("-")
        out.update(range(int(a), int(b or a) + 1))
    return out
away = [d for d in sys.argv[1:] if 0 not in cpus(d)]
print(away[0] if len(away) == 1 else "")
PY
)
  [ -n "$CPUSET" ] || { st "refusing: no one L3 domain without CPU 0 among [$DOMAINS]; set CPUSET"; exit 2; }
fi
st "$RUN at $REV ($(uname -r)), $PASSES passes, loops $LOOPS_SET on CPUs $CPUSET (L3 domains $DOMAINS), arms $ARMS, $ROUNDS rounds after $WARMUP"

P=$(probe 2>&1); echo "$P" > "$O/probe.txt"
grep -q 'OPEN_TREE_NAMESPACE supported' <<< "$P" && grep -q 'CLONE_EMPTY_MNTNS supported' <<< "$P" \
  || { st "refusing: the record needs a kernel with both calls (Linux 7.1+): $P"; exit 3; }
: "${RT:?RT: the built runtime-tools checkout, its validation/*/*.t}" "${CRUN:?CRUN: the reference crun binary}"
[ -x "$CRUN" ] && ls "$RT"/validation/*/*.t > /dev/null 2>&1 \
  || { st "refusing: RT ($RT) holds no built validation tests, or CRUN ($CRUN) isn't executable"; exit 3; }

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

# A cell is called hung past twice the slowest recorded cell (the false-alarm margin of a failure bound: it never
# changes what's measured, only when a hung cell is called hung; the lead's call, 2026-11-06). No interleaved cell is
# recorded yet: until one is, the bound is the earlier host cell's, two phases of 16 single-phase cells' whole 177 s
# window (hex-g6-givenab-20261106T093407Z, §14), 2 x 2 x 177 = 708 s. Each cell's own seconds go into `exits`, so the
# record's slowest cell sets it next.
CELL_BOUND=708

cell() { # cell <pass> <loops>
  local name=p$1-L$2 pids=() p rc res t0=$SECONDS
  for _ in $(seq 1 "$2"); do taskset -c "$CPUSET" bash -c 'while :; do :; done' & pids+=("$!"); done
  { echo "== $name $(date -u +%FT%TZ) uptime: $(uptime)"; echo "  docker ps: $(docker ps -q | wc -l) containers"; } >> "$O/context.txt"
  ( cd "$SRC" && MOUNTS=8 MOUNTS_CONC=8 MOUNTS_ARMS="$ARMS" MOUNTS_ROUNDS=$ROUNDS MOUNTS_WARMUP=$WARMUP \
      MOUNTS_PATH=pruned MOUNTS_SPAN=root IDLE_THREADS=8000 CPUSET=$CPUSET LOAD_GATE=0 BENCH_HOST_OK="$(hostname)" \
      BENCH_MEMORY=${BENCH_MEMORY:-8g} BENCH_OOM=${BENCH_OOM:-1000} HEXBENCH_DEVFS=1 HEXBENCH_OWNER=bench-owner \
      HEXBENCH_REUSE=1 HEXBENCH_DOCKER_PATHS=1 \
      timeout "$CELL_BOUND" bash bench/competitive/probes/create_breakdown.sh "$O/hex" "$O/seam_oci" ) > "$O/$name.log" 2>&1
  rc=$?
  # No wait for the loops before the cell: its container, nodes and 8,000 threads come first. Each loop's CPU time at
  # the end (utime + stime, clock ticks) shows it ran throughout.
  for p in "${pids[@]}"; do read -ra f < "/proc/$p/stat" && echo "$p $(( f[13] + f[14] ))"; done 2>/dev/null \
    | paste -sd' ' | sed "s/^/  loops' CPU ticks by pid (CLK_TCK $(getconf CLK_TCK)): /" >> "$O/context.txt"
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
  for p in "${pids[@]}"; do wait "$p" 2>/dev/null; done
  res=$(grep -o 'results: .*' "$O/$name.log" | tail -1 | cut -d' ' -f2)
  [ "$rc" = 124 ] && st "cell $name called hung past $CELL_BOUND s"
  echo "$(date -u +%FT%TZ) $name $rc $res $(( SECONDS - t0 ))" >> "$O/exits"
}

# host_clock.sh's two sets: the run's CPUs, and the other L3 domain's but the one holding the package awake.
OTHER=$(tr ' ' '\n' <<< "$DOMAINS" | grep -vx "$CPUSET" | head -1)
clock() { # clock <when>
  { echo "== $1 $(date -u +%FT%TZ)"; P=$CPUSET E=$(python3 -I -c 'import sys
s = set()
for part in sys.argv[1].split(","):
    a, _, b = part.partition("-"); s.update(range(int(a), int(b or a) + 1))
s.discard(0)
print(",".join(map(str, sorted(s))))' "${OTHER:-0}") HOLD=0 bash "$HERE/host_clock.sh" 2>&1; } >> "$O/host_clock.txt"
}

timed() {
  local p L
  st "TIMED start (lock held) load $(cut -d' ' -f1-3 /proc/loadavg)"
  for p in $(seq 1 "$PASSES"); do
    clock "pass $p start"
    for L in $LOOPS_SET; do cell "$p" "$L"; done
    clock "pass $p end"
  done
  st "TIMED end load $(cut -d' ' -f1-3 /proc/loadavg)"
}
export -f st cell clock timed
export O SRC ROUNDS WARMUP ARMS CPUSET DOMAINS OTHER PASSES LOOPS_SET CELL_BOUND HERE
# The host's timed parts take one lock, waited on itself: it frees when its holder exits, and this run's unit bounds
# the wait.
LOCK=${TIMED_LOCK:-$HOME/.cache/hex-t7/timed.lock}
mkdir -p "$(dirname "$LOCK")"
flock "$LOCK" bash -c timed || { st "refusing: the timed lock or the run failed"; exit 5; }

st "runtime-tools: hex and crun ($("$CRUN" --version | head -1)), runtime-tools $(git -C "$RT" rev-parse --short HEAD 2>/dev/null)"
BENCH_MEMORY=${BENCH_MEMORY:-8g} BENCH_OOM=${BENCH_OOM:-1000} bash "$REPO/bench/oci-validation/run_hex.sh" "$RT" "$O/hex" "$O/rt/hex" \
  > "$O/rt-hex.log" 2>&1 || st "runtime-tools against hex exited $? ($O/rt-hex.log)"
BENCH_MEMORY=${BENCH_MEMORY:-8g} BENCH_OOM=${BENCH_OOM:-1000} bash "$REPO/bench/oci-validation/run.sh" "$RT" "$CRUN" "$O/rt/crun" \
  > "$O/rt-crun.log" 2>&1 || st "runtime-tools against crun exited $? ($O/rt-crun.log)"

python3 -I "$HERE/mntns_record.py" "$O/exits" "$RUN" "$REV" "$MODE" "$O/rt" | tee "$O/record.txt"
rc=${PIPESTATUS[0]}
{ echo; echo "## The host's clock state (host_clock.sh, at each pass's start and end)"; echo
  sed 's/^/    /' "$O/host_clock.txt"; } | tee -a "$O/record.txt"
exit "$rc"
