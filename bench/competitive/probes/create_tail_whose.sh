#!/usr/bin/env bash
# bench/competitive/probes: create_tail.sh with the whose-attribution beside it (create_tail_whose.bt, bpftrace in a
# privileged container on the reader's CPUs), then create_tail_attr.py on the run: what the seam's slowest creates
# wait on, and whose teardown preempts them (goal 16; create_tail.md, goal16_ab.md). g5rec's g16trace.sh from fedora,
# its host, paths and CPUs made parameters. CPUSET is one range (the bt's predicates take its first and last CPU).
#   [HEXBENCH_OWNER=] [N=10000] [ROUNDS=2] [CPUSET=4-15] [READER_CPUS=16-31] [TIMED_LOCK=<file>] \
#     [BENCH_MEMORY=8g] [BENCH_OOM=1000] create_tail_whose.sh <hex release binary> <seam_oci release binary> <out dir>
# The bpftrace image (Fedora 44's bpftrace) is made once by a container and a commit, as docker_pass.sh makes its own.
# Its record: <out dir>/status.txt, bt.txt (the maps at bpftrace's end), create_tail.log and attr.md.
set -uo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
O=$(realpath -m "${3:?out dir}"); mkdir -p "$O"
CPUSET=${CPUSET:-4-15}; READER_CPUS=${READER_CPUS:-16-31}
FIRST=${CPUSET%%-*}; LAST=${CPUSET##*-}
case "$FIRST$LAST" in *[!0-9]*|'') echo "refusing: CPUSET $CPUSET isn't one range a-b" >&2; exit 2 ;; esac
log() { echo "$(date -u +%FT%TZ) $*" >> "$O/status.txt"; }
cp "$HERE/create_tail_whose.bt" "$O/whose.bt"
IMG=hexbench/bpftrace:44
if ! docker image inspect "$IMG" >/dev/null 2>&1; then
  docker rm -f hexbench-bt-build >/dev/null 2>&1
  docker run --name hexbench-bt-build --network=host --memory=4g registry.fedoraproject.org/fedora:44 \
    dnf -y -q install bpftrace > "$O/bt-image.log" 2>&1 && docker commit hexbench-bt-build "$IMG" >> "$O/bt-image.log" 2>&1
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
  bpftrace -q -o /out/bt.txt /out/whose.bt "$FIRST" "$LAST" > /dev/null
# Its probes attach before the run starts: bpftrace prints its BEGIN line once every one is.
for i in $(seq 1 60); do grep -q 'whose attached' "$O/bt.txt" 2>/dev/null && break; [ "$i" = 60 ] && { log "bpftrace didn't attach in 120 s"; exit 5; }; sleep 2; done
log "bpftrace up on $READER_CPUS, create CPUs $FIRST-$LAST; load $(cut -d' ' -f1-3 /proc/loadavg)"
cd "$LAB/.." || exit 2
LOCK=${TIMED_LOCK:-$HOME/.cache/hex-t7/fedora-timed.lock}; mkdir -p "$(dirname "$LOCK")"
flock -w 3600 "$LOCK" env N="${N:-10000}" ROUNDS="${ROUNDS:-2}" CPUSET="$CPUSET" READER_CPUS="$READER_CPUS" \
  BENCH_MEMORY="${BENCH_MEMORY:-8g}" BENCH_OOM="${BENCH_OOM:-1000}" BENCH_HOST_OK="${BENCH_HOST_OK:-$(hostname)}" \
  bash bench/competitive/probes/create_tail.sh "$HEX" "$SEAM" > "$O/create_tail.log" 2>&1
rc=$?; R=$(grep -o -E '/[^ ]*create-tail-[0-9TZ]+' "$O/create_tail.log" | tail -1)
log "create_tail exit $rc; $R; load $(cut -d' ' -f1-3 /proc/loadavg)"
stop_bt; trap - EXIT
[ -n "$R" ] && python3 -I bench/competitive/probes/create_tail_attr.py bench/competitive/probes "$R" > "$O/attr.md" 2>&1
log "done; bt.txt $(wc -c < "$O/bt.txt" 2>/dev/null) bytes"
