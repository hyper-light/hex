#!/usr/bin/env bash
# bench/competitive/probes: create_tail.sh with the whose-attribution beside it (create_tail_whose.bt, bpftrace in a
# privileged container on the reader's CPUs), then create_tail_attr.py on the run: what the seam's slowest creates
# wait on, and whose teardown preempts them (goal 16; create_tail.md, goal16_ab.md). g5rec's g16trace.sh from fedora,
# its host, paths and CPUs made parameters. CPUSET is one range (the bt's predicates take its first and last CPU).
#   [HEXBENCH_OWNER=] [N=10000] [ROUNDS=2] [CPUSET=4-15] [READER_CPUS=16-31] [TIMED_LOCK=<file>] \
#     [BENCH_MEMORY=8g] [BENCH_OOM=1000] create_tail_whose.sh <hex release binary> <seam_oci release binary> <out dir>
# The bpftrace image (Fedora 44's bpftrace) is made once by a container and a commit, as docker_pass.sh makes its own.
# The timed lock is held from before that until bpftrace has gone: bpftrace's attaching patches kernel text on every
# CPU and its programs run on all of them, so neither may meet another holder's record. Its children don't hold it.
# Its record: <out dir>/status.txt, bt.txt (the maps at bpftrace's end), bt.log, create_tail.log, attr.md, and the
# host's superblocks at its start and its end (superblocks-start.txt, -end.txt). It runs from any working directory: its
# siblings are named by its own.
set -uo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
O=$(realpath -m "${3:?out dir}"); mkdir -p "$O"
CPUSET=${CPUSET:-4-15}; READER_CPUS=${READER_CPUS:-16-31}
FIRST=${CPUSET%%-*}; LAST=${CPUSET##*-}
case "$FIRST$LAST" in *[!0-9]*|'') echo "refusing: CPUSET $CPUSET isn't one range a-b" >&2; exit 2 ;; esac
log() { echo "$(date -u +%FT%TZ) $*" >> "$O/status.txt"; }
LOCK=${TIMED_LOCK:-$HOME/.cache/hex-t7/timed.lock}; mkdir -p "$(dirname "$LOCK")"
exec 9>>"$LOCK"
flock -w 3600 9 || { log "the timed lock $LOCK wasn't free in an hour"; exit 3; }
cp "$HERE/create_tail_whose.bt" "$O/whose.bt"
# The host's superblocks by filesystem type and by whose mount namespaces mount them, which a memcg's offline walks
# (one list_lru each) and a kernfs root's notify tells (goal 16; the lead, 2026-11-09): every mount namespace read once,
# through one of its processes, and a superblock counted once, by its device, under the first owner it is seen with:
# hex where that process is Hex's (hex*, seam_oci) or in a cgroup under a hex-* one, docker where in docker's, else
# the host. Its first line is the total.
read -r -d '' SB <<'EOS'
for p in /proc/[0-9]*; do ns=$(readlink "$p/ns/mnt" 2>/dev/null) || continue; echo "$ns ${p#/proc/}"; done | sort -u -k1,1 > /tmp/ns
while read -r ns p; do
  comm=$(cat "/proc/$p/comm" 2>/dev/null); cg=$(head -1 "/proc/$p/cgroup" 2>/dev/null | cut -d: -f3)
  case "$comm $cg" in hex*|seam_oci*|*/hex*) own=hex ;; *docker*) own=docker ;; *) own=host ;; esac
  awk -v own="$own" '{ for (i = 7; i <= NF; i++) if ($i == "-") { print $3, $(i + 1), own; break } }' "/proc/$p/mountinfo" 2>/dev/null
done < /tmp/ns | awk '!seen[$1]++ { n[$2 " " $3]++; t++ } END { print t, "superblocks"; for (k in n) print n[k], k }' | sort -rn
EOS
superblocks() {
  # The host's cgroup namespace, so each process's cgroup reads whole: in its own, docker's shows its peers' as "/".
  docker run --rm --privileged --pid=host --cgroupns=host --cpuset-cpus="$READER_CPUS" --memory=256m \
    registry.fedoraproject.org/fedora:44 sh -c "$SB" > "$O/superblocks-$1.txt" 2>&1 9>&-
}
superblocks start
IMG=hexbench/bpftrace:44
if ! docker image inspect "$IMG" >/dev/null 2>&1; then
  docker rm -f hexbench-bt-build >/dev/null 2>&1
  docker run --name hexbench-bt-build --network=host --memory=4g registry.fedoraproject.org/fedora:44 \
    dnf -y -q install bpftrace > "$O/bt-image.log" 2>&1 9>&- && docker commit hexbench-bt-build "$IMG" >> "$O/bt-image.log" 2>&1 9>&-
  docker rm -f hexbench-bt-build >/dev/null 2>&1
  docker image inspect "$IMG" >/dev/null 2>&1 || { log "the bpftrace image didn't build ($O/bt-image.log)"; exit 4; }
fi
BT=hexbench-bt-$$
stop_bt() {
  docker kill -s INT "$BT" >/dev/null 2>&1
  for _ in $(seq 1 30); do [ "$(docker inspect -f '{{.State.Running}}' "$BT" 2>/dev/null)" = true ] || break; sleep 2; done
  docker logs "$BT" > "$O/bt.log" 2>&1; docker rm -f "$BT" >/dev/null 2>&1
}
trap stop_bt EXIT
docker run -d --name "$BT" --privileged --pid=host --cpuset-cpus="$READER_CPUS" --memory=2g \
  -v /sys/kernel/debug:/sys/kernel/debug -v /sys/kernel/tracing:/sys/kernel/tracing -v "$O:/out" "$IMG" \
  sh -c 'exec bpftrace -B line -q /out/whose.bt "$0" "$1" > /out/bt.txt' "$FIRST" "$LAST" > /dev/null 9>&-
# Its probes attach before the run starts: bpftrace prints its BEGIN line once every one is. Its output goes to bt.txt
# by its stdout, line by line: an -o file is buffered whole (bpftrace's -B is stdout's), so the line would wait for its
# end. A bpftrace that refused its script has ended: that is said at once, its errors in bt.log.
for i in $(seq 1 60); do
  grep -q 'whose attached' "$O/bt.txt" 2>/dev/null && break
  [ "$(docker inspect -f '{{.State.Running}}' "$BT" 2>/dev/null)" = true ] || { log "bpftrace ended before attaching (bt.log)"; exit 5; }
  [ "$i" = 60 ] && { log "bpftrace didn't attach in 120 s"; exit 5; }
  sleep 2
done
log "bpftrace up on $READER_CPUS, create CPUs $FIRST-$LAST; load $(cut -d' ' -f1-3 /proc/loadavg)"
env N="${N:-10000}" ROUNDS="${ROUNDS:-2}" CPUSET="$CPUSET" READER_CPUS="$READER_CPUS" \
  BENCH_MEMORY="${BENCH_MEMORY:-8g}" BENCH_OOM="${BENCH_OOM:-1000}" BENCH_HOST_OK="${BENCH_HOST_OK:-$(hostname)}" \
  bash "$HERE/create_tail.sh" "$HEX" "$SEAM" > "$O/create_tail.log" 2>&1 9>&-
rc=$?; R=$(grep -o -E '/[^ ]*create-tail-[0-9TZ]+' "$O/create_tail.log" | tail -1)
superblocks end
log "create_tail exit $rc; $R; load $(cut -d' ' -f1-3 /proc/loadavg)"
stop_bt; trap - EXIT
exec 9>&-
[ -n "$R" ] && python3 -I "$HERE/create_tail_attr.py" "$HERE" "$R" > "$O/attr.md" 2>&1
log "done; bt.txt $(wc -c < "$O/bt.txt" 2>/dev/null) bytes"
