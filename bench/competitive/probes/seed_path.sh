#!/usr/bin/env bash
# bench/competitive/probes: the seam's create through its seeds (goal 5 plan step 4). Runs seed_path_inside.sh in the
# bench container, with the create breakdown's flags (create_breakdown.sh), on the given Hex and seam_oci binaries,
# each copied into the run's directory: a seed's clone of an init, timed by sched events; every exec and every bpf call
# over a run of creates, by strace -f; a seed's memory; and the node's seeds' counts over a sustained phase.
# seed_path.py reads the trace. On fedora:
#   seed_path.sh <hex release binary> <seam_oci release binary>
# Env: N, WARMUP, EXECS, MEMORY_N, SUSTAINED (seed_path_inside.sh's), CPUSET (4-31), STRACE (the directory whose
# usr/bin/strace runs in the container: ~/.cache/hex-t7/strace, Fedora's strace unpacked; the bench host has none).
# Not a timed pass: the load is recorded, not gated. The clone's interval is the seed's own, so the CPU others use
# shows in it only as their share of the cores it runs on.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")"); LAB=$(dirname "$HERE")
. "$HERE/../containers.sh"
HEX=$(realpath "${1:?hex release binary}"); SEAM=$(realpath "${2:?seam_oci release binary}")
STRACE=${STRACE:-$HOME/.cache/hex-t7/strace}
[ -x "$STRACE/usr/bin/strace" ] || { echo "refusing: no $STRACE/usr/bin/strace" >&2; exit 4; }
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
OUT="$BASE/out/seed-path-$(date -u +%Y%m%dT%H%M%SZ)"; mkdir -p "$OUT/bin"
cp "$HEX" "$OUT/bin/hex"; cp "$SEAM" "$OUT/bin/seam_oci"
containers() { docker ps --format '{{.Names}} ({{.Image}}, {{.Status}})' 2>&1 | paste -sd ';' - | sed 's/;/; /g'; }
{ echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to ${CPUSET:-4-31}"
  echo "load at start: $(cat /proc/loadavg)"
  echo "containers at start, every one up: $(containers)"
  echo "hex: $("$OUT/bin/hex" --version) (sha256 $(sha256sum "$OUT/bin/hex" | cut -c1-16)), $(file -b "$OUT/bin/hex" | cut -d, -f1-4)"
  echo "seam_oci: sha256 $(sha256sum "$OUT/bin/seam_oci" | cut -c1-16)"
  echo "strace: $("$STRACE/usr/bin/strace" -V 2>&1 | head -1)"
  echo "harness: bench/competitive at $(git -C "$LAB" rev-parse --short HEAD)$(git -C "$LAB" diff --quiet HEAD -- . || echo ' with uncommitted changes')"
} > "$OUT/env.txt"
NAME=hexbench-seedpath-$$
bench_trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true'
timeout 3600 docker run --rm --name "$NAME" "${BENCH_LABELS[@]}" --cpuset-cpus="${CPUSET:-4-31}" --memory=40g \
  --privileged --security-opt label=disable --cgroupns=private --network=none --tmpfs /run:rw,exec,nosuid,size=1g \
  -v /usr:/usr:ro -v "$OUT/bin:/opt/bin:ro" -v "$LAB:/opt/lab:ro" -v "$STRACE:/opt/strace:ro" -v "$OUT:/opt/out" \
  -e N -e WARMUP -e EXECS -e MEMORY_N -e SUSTAINED \
  registry.fedoraproject.org/fedora:44 bash /opt/lab/probes/seed_path_inside.sh 2>&1 | tee "$OUT/probe.out"
{ echo "load at end: $(cat /proc/loadavg)"; echo "containers at end, every one up: $(containers)"; } >> "$OUT/env.txt"
echo "results: $OUT"
