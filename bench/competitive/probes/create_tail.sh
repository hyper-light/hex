#!/usr/bin/env bash
# bench/competitive/probes: what the seam's slowest creates and starts wait on (create_tail.md). Runs
# create_tail_inside.sh in a container with host.sh's OCI-pass flags, on the given binaries, and beside it a reader
# container on READER_CPUS (16-31) that streams the run's trace instance to $OUT/trace.txt, so the reading takes no
# CPU from the pinned run. Then create_tail.py reads it. On fedora:
#   [HEXBENCH_OWNER=<owner>] [N=10000] [ROUNDS=1] create_tail.sh <hex release binary> <seam_oci release binary>
# Not a timed pass: the tracer costs each switch and wake-up some; what it reads is where the slow calls wait.
# BENCH_MEMORY and BENCH_OOM set the container's --memory (40g) and --oom-score-adj (none) on a shared host.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
case "$(hostname)" in fedora*|"${BENCH_HOST_OK:-__none__}") ;; *) echo "refusing: the bench host is fedora, not $(hostname) (BENCH_HOST_OK=$(hostname) runs it here)" >&2; exit 2 ;; esac
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
up=$(docker ps --format '{{.Names}}' | grep -E '^hexbench-tail-' | paste -sd' ' - || true)
[ -z "$up" ] || { echo "refusing: another tail probe is up ($up)" >&2; exit 3; }
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="$BASE/out/create-tail-$STAMP"
mkdir -p "$BASE/out"; mkdir "$OUT" || { echo "refusing: $OUT is there already" >&2; exit 3; }
mkdir "$OUT/bin"
export TRACE_TAG="${STAMP}_$$"
cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"
containers() { docker ps --format '{{.Names}} ({{.Image}}, {{.Status}})' 2>&1 | paste -sd ';' - | sed 's/;/; /g'; }
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-15}, reader on ${READER_CPUS:-16-31}"
  echo "load at start: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at start, $r: $(head -1 "/proc/pressure/$r")"; done
  echo "containers at start, every one up: $(containers)"
  echo "hex: $("$OUT/bin/hex" --version) (sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16)), $(file -b "$OUT/bin/hex" | cut -d, -f1-4)"
  echo "seam_oci: sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
  echo "trace tag: $TRACE_TAG (its instance hextail-$TRACE_TAG)"
} > "$OUT/env.txt"
NAME=hexbench-tail-$$; READER=hexbench-tail-reader-$$
untag() {
  docker rm -f "$NAME" "$READER" >/dev/null 2>&1 || true
  docker run --rm --label hex.bench=competitive --privileged --security-opt label=disable --network=none \
    -v /usr:/usr:ro -v "$LAB:/opt/lab:ro" -e TRACE_TAG -e UNTRACE_ONLY=1 \
    registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/create_tail_inside.sh >/dev/null 2>&1 \
    || echo "the run's trace instance (tag $TRACE_TAG) couldn't be checked gone" >&2
}
trap untag EXIT
trap 'exit 143' TERM INT
# The reader: waits for the instance, says it's reading, streams it to the run's end marker, says it's done.
docker run -d --name "$READER" --label hex.bench=competitive --cpuset-cpus="${READER_CPUS:-16-31}" --memory=1g \
  ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} --privileged --security-opt label=disable --network=none \
  -v /usr:/usr:ro -v "$OUT:/opt/out" registry.fedoraproject.org/fedora:44 bash -c '
    T=/sys/kernel/tracing; mountpoint -q $T || mount -t tracefs nodev $T
    for i in $(seq 1 6000); do [ -s /opt/out/instance ] && break; sleep 0.1; done
    I=$(cat /opt/out/instance) || exit 5
    exec 3< "$I/trace_pipe"; touch /opt/out/reading
    awk "{print} /hexbd D/{exit}" <&3 > /opt/out/trace.txt; touch /opt/out/read' >/dev/null
timeout 7200 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-15}" --memory="${BENCH_MEMORY:-40g}" ${BENCH_OOM:+--oom-score-adj="$BENCH_OOM"} \
  --privileged --security-opt label=disable --cgroupns=private --network=none --tmpfs /run:rw,exec,nosuid,size=1g \
  -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$OUT:/opt/out" \
  -e N -e WARMUP -e ROUNDS -e TRACE_TAG -e HEXBENCH_OWNER \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/create_tail_inside.sh 2>&1 | tee "$OUT/probe.out"
timeout 600 docker wait "$READER" >/dev/null 2>&1 || true
{ echo "load at end: $(cat /proc/loadavg)"
  for r in cpu memory io; do echo "pressure at end, $r: $(head -1 "/proc/pressure/$r")"; done
  echo "containers at end, every one up: $(containers)"
  echo "trace: $(wc -l < "$OUT/trace.txt") lines, $(du -h "$OUT/trace.txt" | cut -f1)"; } >> "$OUT/env.txt"
python3 -I "$HERE/create_tail.py" read "$OUT" > "$OUT/summary.md" 2> "$OUT/summary.err" || echo "the reader failed: $(tail -c 400 "$OUT/summary.err")"
echo "results: $OUT"
