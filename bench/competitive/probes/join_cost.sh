#!/usr/bin/env bash
# bench/competitive/probes: is joining a container to its cgroup what made `hex oci create` slower?
# In the OCI lab's shape (a privileged container with its own cgroup namespace, host /usr, the bench's
# binaries), it times cgroup migrations by the gap before each (join_cost.py), then traces one
# `hex oci create` and one `crun create`, each after a quiet half second, with each system call's time
# (strace -T, when the host has strace).
#   join_cost.sh <out dir> <hex release binary>
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
OUT=$(realpath "${1:?out dir}"); HEX=$(realpath "${2:?hex release binary}")
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
mkdir -p "$OUT"
C="hexbench-joincost-$$"
cleanup() { docker rm -f "$C" > /dev/null 2>&1 || true; }
trap cleanup EXIT
docker run --rm --name "$C" --label hex.bench=competitive --cpuset-cpus="${CPUSET:-4-31}" \
  --privileged --security-opt label=disable --cgroupns=private --network=none \
  -v /usr:/usr:ro -v "$BASE/bin:/opt/bin:ro" -v "$HEX:/opt/hex:ro" -v "$HERE:/opt/probe:ro" -v "$OUT:/opt/out" \
  registry.fedoraproject.org/fedora:44 bash -euo pipefail -c '
    mkdir -p /sys/fs/cgroup/lab
    for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
    echo "+cpu +io +memory +pids" > /sys/fs/cgroup/cgroup.subtree_control
    python3 -I /opt/probe/join_cost.py /opt/out/join_cost.json 200
    command -v strace > /dev/null || { echo "no strace on the host: the traces are skipped"; exit 0; }
    B=/run/bundle; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
    ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
    cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":["/usr/bin/sleep","infinity"],"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]} }
JSON
    # strace follows the whole tree, so it ends only once the container is deleted: create in the
    # background, wait (bounded) for the state to say created, then delete.
    trace() { # trace <name> <runtime argv...>
      local name=$1; shift
      sleep 0.5
      strace -f -T -tt -e trace=write,clone,clone3,openat,mkdir,bpf -o "/opt/out/strace.$name.txt" \
        "$@" create --bundle "$B" "jc-$name" < /dev/null > /dev/null 2>&1 &
      local tracer=$!
      for i in $(seq 1 200); do
        "$@" state "jc-$name" 2> /dev/null | grep -Eq "\"status\": *\"created\"" && break
        sleep 0.05
      done
      "$@" delete --force "jc-$name" > /dev/null 2>&1 || true
      wait "$tracer" || true
    }
    HEX_OCI_ROOT=/run/hex-oci trace hex /opt/hex oci
    trace crun /opt/bin/crun --root /run/crun
    for f in /opt/out/strace.*.txt; do
      echo "== $f: the slowest calls"; sed -n "s/.*<\([0-9.]*\)>\$/\1 &/p" "$f" | sort -g -r | head -8
    done
  ' 2>&1 | tee "$OUT/join_cost.log"
