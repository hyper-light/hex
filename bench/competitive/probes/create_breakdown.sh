#!/usr/bin/env bash
# bench/competitive/probes: runs create_breakdown_inside.sh in the bench container, with host.sh's OCI-pass flags,
# on the libcrun driver and crun host.sh built and fetched ($HEX_BENCH/bin) and the given Hex and seam_oci binaries.
# Its /run is a tmpfs, as a host's is and the OCI pass's is (6f7ba39). Runs before this one's change
# (create-breakdown-20261030T000822Z and earlier) had /run on the image's overlayfs, where node records are fsynced.
# Every binary is copied into the run's directory, so what ran is kept beside what it measured. On fedora:
#   create_breakdown.sh <hex release binary> <seam_oci release binary>
# Env: N, WARMUP, ROUNDS, PROBE_ONLY (create_breakdown_inside.sh's), CPUSET (4-31); BEFORE_HEX and BEFORE_SEAM, a Hex
# and seam_oci from before a change, whose seam rounds then run in every round beside the given ones (`before`), so a
# change's ratio is each run's own and its rounds meet the same host; IDLE_AB, that many idle threads around a seam
# and a libcrun item in every round (`seam_idle`, `libcrun_idle`), the threads' ratio the run's own the same way;
# IDLE_THREADS, EXTRA_MOUNTS and AC_E, goal 5 plan step 2's (below); MOUNTS, MOUNTS_CONC and MOUNTS_PATH, plan step 3's
# per-call trace of the init's mounts (create_breakdown_inside.sh's mounts_trace); IPCUTS_AB=1, every round's seam and
# traced items again with the node's ipc and uts pool drained, the pool's on-against-drained the run's own (goal 5
# extended step 4); and CLASSIC=1, its rounds on the
# classic path (every create through the shim, by a createRuntime hook); LOAD_GATE=0 skips the wait for
# load under 2, as host.sh allows: this probe's comparisons are within one run (the parts against their creates,
# Hex's create against libcrun's), which cancel other people's CPU load. BENCH_HOST_OK names another host, as
# host.sh takes it.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
case "$(hostname)" in fedora*|"${BENCH_HOST_OK:-__none__}") ;; *) echo "refusing: the bench host is fedora, not $(hostname) (BENCH_HOST_OK=$(hostname) runs it here)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
# AC-e (goal 5 plan step 2) compares nothing against libcrun — it reads only the pruned root's switch — and nor does
# the mounts trace (plan step 3), so each needs only hex and seam_oci; every other run needs host.sh's crun and its
# libcrun driver.
ALONE=0; [ "${AC_E:-0}" = 0 ] && [ "${MOUNTS:-0}" = 0 ] || ALONE=1
if [ "$ALONE" = 0 ]; then
  for f in crun libcrun-driver; do
    [ -x "$BASE/bin/$f" ] || { echo "refusing: no $BASE/bin/$f (host.sh fetches crun and builds the libcrun driver)" >&2; exit 4; }
  done
fi
# One breakdown at a time: each run's tracer keeps the events of every process named as the runtimes are, host-wide,
# so two at once would each trace the other's creates (their checks then refuse them), and their stamps can be one.
up=$(docker ps --format '{{.Names}}' | grep -E '^hexbench-(createbd|execbd)-' | paste -sd' ' - || true)
[ -z "$up" ] || { echo "refusing: another breakdown probe is up ($up); its tracer and this one's would see each other's processes" >&2; exit 3; }
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="$BASE/out/create-breakdown-$STAMP"
mkdir -p "$BASE/out"; mkdir "$OUT" || { echo "refusing: $OUT is there already, another run's of this second" >&2; exit 3; }
mkdir "$OUT/bin"
# The run's tracer is named by its stamp and this script's pid, which no other run up on the host shares
# (create_breakdown_inside.sh's TRACE_TAG).
export TRACE_TAG="${STAMP}_$$"
cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"
if [ "$ALONE" = 0 ]; then cp "$BASE/bin/crun" "$BASE/bin/libcrun-driver" "$OUT/bin/"; fi
if [ -n "${BEFORE_HEX:-}" ]; then
  cp "$(realpath "$BEFORE_HEX")" "$OUT/bin/before-hex"; cp "$(realpath "${BEFORE_SEAM:?BEFORE_SEAM, with BEFORE_HEX}")" "$OUT/bin/before-seam_oci"
fi
[ "${LOAD_GATE:-1}" = 0 ] || for i in $(seq 1 90); do
  load=$(cut -d' ' -f1 /proc/loadavg)
  awk -v l="$load" 'BEGIN { exit !(l < 2) }' && break
  [ "$i" = 90 ] && { echo "refusing: 1-minute load average $load is not under 2" >&2; exit 3; }
  sleep 10
done
ROOT=$(docker info --format '{{.DockerRootDir}}' 2>/dev/null || echo unknown)
# Every container up on the host, so what ran beside the probe is in its record (another bench, or a fleet's checks).
containers() { docker ps --format '{{.Names}} ({{.Image}}, {{.Status}})' 2>&1 | paste -sd ';' - | sed 's/;/; /g'; }
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-31}"
  [ "${LOAD_GATE:-1}" = 0 ] && echo "load gate: skipped (LOAD_GATE=0): the comparisons are within this run"
  echo "load at start: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at start, $r: $(head -1 "/proc/pressure/$r")"; done
  echo "containers at start, every one up: $(containers)"
  echo "docker's root: $ROOT, on $(stat -f -c %T "$ROOT" 2>/dev/null || echo unknown) ($(findmnt -n -o SOURCE,FSTYPE,OPTIONS -T "$ROOT" 2>/dev/null | cut -c1-160))"
  echo "hex: $("$OUT/bin/hex" --version) (sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16)), $(file -b "$OUT/bin/hex" | cut -d, -f1-4)"
  echo "seam_oci: sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  [ -x "$OUT/bin/before-hex" ] && echo "before: hex sha256 $(sha256sum "$OUT/bin/before-hex" | cut -c1-16)," \
    "seam_oci sha256 $(sha256sum "$OUT/bin/before-seam_oci" | cut -c1-16)"
  if [ "$ALONE" = 0 ]; then
    echo "crun: $("$OUT/bin/crun" --version | head -1) (sha256 $(sha256sum "$OUT/bin/crun" | cut -c1-16))"
    echo "libcrun: $("$OUT/bin/libcrun-driver" --version 2>&1 | head -1) (driver sha256 $(sha256sum "$OUT/bin/libcrun-driver" | cut -c1-16))"
  fi
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
  echo "probe: /run a tmpfs (size 1g), where the runtimes keep their state, as host.sh's OCI pass has it"
  echo "trace tag: $TRACE_TAG (its instance hexcb-$TRACE_TAG, its kprobes' group hexcb_$TRACE_TAG)"
} > "$OUT/env.txt"
NAME=hexbench-createbd-$$
HELPER=""
# IDLE_THREADS (goal 5 plan step 2, AC-c): hold that many idle threads on the host (outside the bench
# container), so `pivot_root`'s walk of every host thread has them to visit. The pruned-seed root makes
# no `pivot_root`, so its create p50 is flat in the host's thread count; the classic path grows with it.
if [ "${IDLE_THREADS:-0}" -gt 0 ]; then
  gcc -O2 -pthread -o "$OUT/bin/idlethreads" "$HERE/idlethreads.c"
  "$OUT/bin/idlethreads" "$IDLE_THREADS" > "$OUT/idlethreads.out" 2>> "$OUT/idlethreads.out" &
  HELPER=$!
  for i in $(seq 1 150); do
    up=$(ls "/proc/$HELPER/task" 2>/dev/null | wc -l)
    [ "$up" -ge "$IDLE_THREADS" ] && break
    [ "$i" = 150 ] && { echo "refusing: only $up of $IDLE_THREADS idle threads came up" >&2; kill "$HELPER" 2>/dev/null; exit 5; }
    sleep 0.2
  done
  echo "idle threads: $(ls "/proc/$HELPER/task" 2>/dev/null | wc -l) up (asked $IDLE_THREADS), host threads now $(ls -d /proc/[0-9]*/task/[0-9]* 2>/dev/null | wc -l)" >> "$OUT/env.txt"
fi
# IDLE_AB (goal 5 plan step 2, AC-c within one run): that many idle threads, started inside the bench container
# around every round's seam_idle and libcrun_idle (create_breakdown_inside.sh's with_idle), so each round times Hex's
# create, and libcrun's as the control whose pivot_root walks them, with and without them: the ratio is the run's own,
# its rounds meeting the same host, where IDLE_THREADS's runs compare across runs.
if [ "${IDLE_AB:-0}" -gt 0 ]; then
  gcc -O2 -pthread -o "$OUT/bin/idlethreads" "$HERE/idlethreads.c"
  echo "idle-thread A/B: $IDLE_AB idle threads up inside the container around each round's seam_idle and libcrun_idle; host threads before them $(ls -d /proc/[0-9]*/task/[0-9]* 2>/dev/null | wc -l)" >> "$OUT/env.txt"
fi
# On any end of this script, a stopped unit's TERM among them: the container gone, then the run's tracer state taken
# down by the inside script's own teardown in a short container of its own, for a container killed before its trap
# ran. It touches this run's tag alone, never another run's.
untag() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  [ -n "$HELPER" ] && kill "$HELPER" 2>/dev/null || true
  docker run --rm --label hex.bench=competitive --privileged --security-opt label=disable --network=none \
    -v /usr:/usr:ro -v "$LAB:/opt/lab:ro" -e TRACE_TAG -e UNTRACE_ONLY=1 \
    registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/create_breakdown_inside.sh >/dev/null 2>&1 \
    || echo "the run's tracer state (tag $TRACE_TAG) couldn't be checked gone" >&2
}
trap untag EXIT
trap 'exit 143' TERM INT
timeout 7200 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" --memory="${BENCH_MEMORY:-40g}" ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  --privileged --security-opt label=disable --cgroupns=private --network=none --tmpfs /run:rw,exec,nosuid,size=1g \
  -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" \
  -e N -e WARMUP -e ROUNDS -e PROBE_ONLY -e TRACE_TAG -e EXTRA_MOUNTS -e AC_E -e IDLE_AB -e MOUNTS -e MOUNTS_CONC -e MOUNTS_PATH -e CLASSIC -e HEXBENCH_DEVFS -e AC_E_MOUNTS -e HEXBENCH_OWNER -e HEXBENCH_REUSE -e HEXBENCH_DOCKER_PATHS -e IPCUTS_AB -e HEX_OCI_MNTNS -e HEXBENCH_PROCFS \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/create_breakdown_inside.sh 2>&1 | tee "$OUT/probe.out"
{ echo "load at end: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at end, $r: $(head -1 "/proc/pressure/$r")"; done
  echo "containers at end, every one up: $(containers)"; } >> "$OUT/env.txt"
echo "results: $OUT"
