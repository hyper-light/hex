#!/usr/bin/env bash
# Goal 17's rootless variant: the arms of its run of record (mntns_record.sh) that apply rootless, run as this user,
# with no container and nothing as root. Hex's node as it ships rootless, beside libcrun rootless with the same user
# namespace mapping, through goal 0's driver in process, interleaved round by round (rounds.py) under the under-load
# bar's arms: 6 and 12 busy loops at nice 0 on CPUSET, 8,000 idle threads, Docker's default capabilities and paths.
# Hex's rootless creates take the classic root in each box's own user namespace (ClassicReason::Rootless): decision 6's
# ways are a root node's, so none of them is an arm here. Part 19 isn't read either: its trace is ftrace's, and tracefs
# is root's. 2 passes, best of 2, as summarize.py takes it; the host's clock state at each pass's start and end. Then
# runtime-tools' suite rootless against this hex and against CRUN (rootless_validate.sh): its subset is the tests crun
# passes rootless; given CRUN_ROOT_RESULTS (the run of record's rt/crun/results.tsv), the tests crun passes as root and
# fails rootless are named as needing root, left out.
#
# It builds release hex and seam_oci from HEAD by git archive, as mntns_record.sh does, libcrun's in-process driver from
# crun's pinned source (fetch.py crun-src, libcrun/build.sh) and the idle threads' program. Each cell runs in a delegated
# systemd --user scope of its own (rootless_cell.sh, Delegate=yes). ROUNDS (299), WARMUP (1), CPUSET and the timed
# lock are mntns_record.sh's, for the same reasons. reads it all with rootless_record.py into record.txt. E.g. on
# adacachy2:
#   systemd-run --user --unit hex-g6-rlrec-$(date -u +%Y%m%dT%H%M%SZ) --collect -p RuntimeMaxSec=10800 \
#     -E RT=$HOME/.cache/hex-g6/rt/runtime-tools -E CRUN=$HOME/.cache/hex-g6/rt/crun-1.27 \
#     -- /usr/bin/bash "$PWD/bench/competitive/probes/rootless_record.sh"
set -uo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
REPO=$(git -C "$HERE" rev-parse --show-toplevel) || { echo "refusing: not in a git checkout" >&2; exit 2; }
[ "$(id -u)" != 0 ] || { echo "refusing: the rootless variant runs as a user, not root" >&2; exit 2; }
[ -S "${XDG_RUNTIME_DIR:-/nonexistent}/systemd/private" ] \
  || { echo "refusing: no systemd --user manager answers for the cells' delegated scopes" >&2; exit 2; }
REV=$(git -C "$REPO" rev-parse --short HEAD)
RUN=rootless-$(date -u +%Y%m%dT%H%M%SZ)
O=${OUT_ROOT:-$HOME/.cache/hex-t7/g6/rec}/$RUN
mkdir -p "$O" || exit 2
PASSES=${PASSES:-2}; LOOPS_SET=${LOOPS_SET:-6 12}; ROUNDS=${ROUNDS:-299}; WARMUP=${WARMUP:-1}
st() { echo "$(date -u +%FT%TZ) $*" | tee -a "$O/status"; }

# The CPUs of each L3 domain, and the one this run takes: CPUSET, or the domain without CPU 0 (mntns_record.sh's).
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
st "$RUN at $REV ($(uname -r)), rootless as uid $(id -u), $PASSES passes, loops $LOOPS_SET on CPUs $CPUSET (L3 domains $DOMAINS), arms libcrun and hex, $ROUNDS rounds after $WARMUP"
: "${RT:?RT: the built runtime-tools checkout, its validation/*/*.t}" "${CRUN:?CRUN: the reference crun binary}"
[ -x "$CRUN" ] && ls "$RT"/validation/*/*.t > /dev/null 2>&1 \
  || { st "refusing: RT ($RT) holds no built validation tests, or CRUN ($CRUN) isn't executable"; exit 3; }

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
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
python3 -I "$SRC/bench/competitive/fetch.py" "$BASE" crun-src > "$O/crun-src.json" 2> "$O/crun-src.log" \
  || { st "refusing: crun's pinned source didn't fetch ($O/crun-src.log)"; exit 4; }
BUILD_DIR=$HOME/.cache JOBS=4 nice -n 10 bash "$SRC/bench/competitive/libcrun/build.sh" "$BASE/src/crun-1.30.1.tar.gz" \
  "$O/libcrun-driver" > "$O/libcrun.log" 2>&1 || { st "refusing: libcrun's driver didn't build ($O/libcrun.log)"; exit 4; }
gcc -O2 -pthread -o "$O/idlethreads" "$SRC/bench/competitive/probes/idlethreads.c" \
  || { st "refusing: the idle threads' program didn't build"; exit 4; }
st "built $(sha256sum "$O/hex" "$O/seam_oci" "$O/libcrun-driver" | cut -c1-16 | paste -sd' '), $("$O/libcrun-driver" --version 2>&1 | head -1)"

# mntns_record.sh's cell bound, until a rootless cell is recorded: each cell's own seconds go into `exits`.
CELL_BOUND=708

cell() { # cell <pass> <loops>
  local name=p$1-L$2 rc t0=$SECONDS
  { echo "== $name $(date -u +%FT%TZ) uptime: $(uptime)"; } >> "$O/context.txt"
  CPUSET=$CPUSET ROUNDS=$ROUNDS WARMUP=$WARMUP IDLE_THREADS=${IDLE_THREADS:-8000} \
    HEXBENCH_OWNER=bench-owner HEXBENCH_REUSE=1 timeout "$CELL_BOUND" systemd-run --user --scope --quiet \
    -p Delegate=yes --unit "hex-g6-rlcell-${RUN#rootless-}-$name" -- bash "$SRC/bench/competitive/probes/rootless_cell.sh" \
    "$O/$name" "$O/hex" "$O/seam_oci" "$O/libcrun-driver" "$O/idlethreads" "$2" > "$O/$name.log" 2>&1
  rc=$?
  [ "$rc" = 124 ] && st "cell $name called hung past $CELL_BOUND s"
  echo "$(date -u +%FT%TZ) $name $rc $O/$name $(( SECONDS - t0 ))" >> "$O/exits"
}

# host_clock.sh's two sets: the run's CPUs, and the other L3 domain's but the one holding the package awake.
OTHER=$(tr ' ' '\n' <<< "$DOMAINS" | grep -vx "$CPUSET" | head -1)
but_cpu0() { python3 -I - "$1" <<'PY'
import sys
s = set()
for part in sys.argv[1].split(","):
    a, _, b = part.partition("-")
    s.update(range(int(a), int(b or a) + 1))
s.discard(0)
print(",".join(map(str, sorted(s))))
PY
}
clock() { # clock <when>
  { echo "== $1 $(date -u +%FT%TZ)"; P=$CPUSET E=$(but_cpu0 "${OTHER:-0}") HOLD=0 bash "$HERE/host_clock.sh" 2>&1; } \
    >> "$O/host_clock.txt"
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
export -f st cell clock but_cpu0 timed
export O SRC RUN ROUNDS WARMUP CPUSET DOMAINS OTHER PASSES LOOPS_SET CELL_BOUND HERE
LOCK=${TIMED_LOCK:-$HOME/.cache/hex-t7/timed.lock}
mkdir -p "$(dirname "$LOCK")"
flock "$LOCK" bash -c timed || { st "refusing: the timed lock or the run failed"; exit 5; }

st "runtime-tools rootless: hex and crun ($("$CRUN" --version | head -1)), runtime-tools $(git -C "$RT" rev-parse --short HEAD 2>/dev/null)"
bash "$SRC/bench/oci-validation/rootless_validate.sh" "$RT" "$O/hex" "$O/rt/hex" hex > "$O/rt-hex.log" 2>&1 \
  || st "runtime-tools rootless against hex exited $? ($O/rt-hex.log)"
bash "$SRC/bench/oci-validation/rootless_validate.sh" "$RT" "$CRUN" "$O/rt/crun" > "$O/rt-crun.log" 2>&1 \
  || st "runtime-tools rootless against crun exited $? ($O/rt-crun.log)"

python3 -I "$HERE/rootless_record.py" "$O/exits" "$RUN" "$REV" "$O/rt" ${CRUN_ROOT_RESULTS:+"$CRUN_ROOT_RESULTS"} \
  | tee "$O/record.txt"
rc=${PIPESTATUS[0]}
{ echo; echo "## The host's clock state (host_clock.sh, at each pass's start and end)"; echo
  sed 's/^/    /' "$O/host_clock.txt"; } | tee -a "$O/record.txt"
exit "$rc"
