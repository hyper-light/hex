#!/usr/bin/env bash
# bench/competitive/probes: goal 5's added levers measured (levers.c): clone3's cost by namespace flag, the parent's
# call and then the child's exit and reap, a child's setns into an ipc and a uts namespace made ahead, and a record-sized write (temp file, write, close, rename) on the bench
# container's tmpfs /run, its overlay root and the host's disk. Run as the bench is, as root in a privileged container
# of the OCI pass's image, on fedora. Not of record: no load gate, the load and thread count printed with the run.
#   levers.sh [n]
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
. "$HERE/../containers.sh"
bench_trap :
N=${1:-2000}
P=$HOME/.cache/hex-t7/levers-probe; mkdir -p "$P/disk"
cp "$HERE/levers.c" "$P/levers.c"
echo "levers.c sha256 $(sha256sum "$P/levers.c" | cut -c1-16); $(date -u +%FT%TZ); load $(cut -d' ' -f1-3 /proc/loadavg); kernel $(uname -r); host threads $(ps -eLf | wc -l)"
up=$(docker ps --format '{{.Names}}' | grep hexbench- || true)
[ -z "$up" ] || { echo "refusing: a bench container is up: $up" >&2; exit 3; }
docker run "${BENCH_LABELS[@]}" --rm --privileged --security-opt label=disable --cgroupns=private --network=none \
  --tmpfs /run:rw,exec,size=256m -v "$P:/p" -e N="$N" hexbench/pass:44-860612303 bash -c '
    set -e; gcc -O2 -o /run/levers /p/levers.c
    mkdir -p /sys/fs/cgroup/levers /root/rec
    /run/levers clone "$N" /sys/fs/cgroup/levers
    /run/levers setns "$N"
    for d in /run /root/rec /p/disk; do echo "dir $d: $(stat -f -c %T "$d")"; /run/levers write "$N" "$d" 400; done'
