#!/usr/bin/env bash
# One cell of goal 17's rootless variant (rootless_record.sh), as this user, with no container and nothing as root, in a
# delegated systemd --user scope of its own (Delegate=yes, as hex-cgroup-delegation.md describes). Hex's node as it
# ships rootless, every create the classic root in the box's own user namespace (ClassicReason::Rootless: decision 6's
# ways are a root node's), and libcrun rootless beside it, each box's root the user's own uid and gid in both (one id
# each, all an unprivileged writer of uid_map may map), interleaved by rounds.py under the under-load bar's arms that
# apply rootless: L busy loops at nice 0 on CPUSET, IDLE_THREADS idle threads (8,000), Docker's default capabilities and
# its masked and read-only paths. Part 19 isn't read here: its trace is ftrace's, and tracefs is root's.
# The scope's cgroup is split into leaves (cgroup v2's no-internal-processes rule): driver/ (this script, rounds.py and
# the drivers), load/ (the loops and the idle threads), arms/ (each hex arm's box root, rounds.py's ROUNDS_CGROUP) and
# libcrun/ (each libcrun container's cgroup, ROUNDS_LIBCRUN_CGROUP), the scope's controllers handed down to both.
# Usage: rootless_cell.sh <out dir> <hex> <seam_oci> <libcrun-driver> <idlethreads> <loops>
# Env: CPUSET, ROUNDS, WARMUP, IDLE_THREADS, and the node's HEXBENCH_* settings, which reach its arm as they are.
set -uo pipefail
OUT=${1:?out dir}; HEX=${2:?hex}; SEAM=${3:?seam_oci}; LIBCRUN=${4:?libcrun-driver}; IDLE=${5:?idlethreads}; L=${6:?loops}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
[ "$(id -u)" != 0 ] || { echo "refusing: the rootless cell runs as a user, not root"; exit 2; }
CG=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)
[ -w "$CG/cgroup.subtree_control" ] \
  || { echo "refusing: $CG isn't this user's to split (run the cell in a scope with Delegate=yes)"; exit 2; }
mkdir -p "$OUT" "$CG/driver" "$CG/load" "$CG/arms" "$CG/libcrun" || exit 2
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/driver/cgroup.procs" 2>/dev/null; done
ctl=$(sed 's/\([^ ]*\)/+\1/g' "$CG/cgroup.controllers")
for d in "$CG" "$CG/arms" "$CG/libcrun"; do
  echo "$ctl" > "$d/cgroup.subtree_control" || { echo "refusing: the scope's controllers ($ctl) couldn't be handed to $d"; exit 2; }
done

# The bundles: oci_inside.sh's, with an owner's Docker capabilities and Docker's paths (create_breakdown_inside.sh's), a
# user namespace of each box's own, and the rootfs shared by a link, so libcrun's bundle of its own per container
# (rounds.py) finds the same tree.
B=$OUT/bundles
mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
CAPS='["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FSETID","CAP_FOWNER","CAP_MKNOD","CAP_NET_RAW","CAP_SETGID","CAP_SETUID","CAP_SETFCAP","CAP_SETPCAP","CAP_NET_BIND_SERVICE","CAP_SYS_CHROOT","CAP_KILL","CAP_AUDIT_WRITE"]'
PATHS='"maskedPaths":["/proc/acpi","/proc/asound","/proc/interrupts","/proc/kcore","/proc/keys","/proc/latency_stats","/proc/sched_debug","/proc/scsi","/proc/timer_list","/proc/timer_stats","/sys/devices/virtual/powercap","/sys/firmware"],"readonlyPaths":["/proc/bus","/proc/fs","/proc/irq","/proc/sys","/proc/sysrq-trigger"]'
bundle() { # bundle <dir> <args json>
  mkdir -p "$1" && ln -sfn "$B/rootfs" "$1/rootfs"
  cat > "$1/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0},
             "capabilities":{"bounding":$CAPS,"effective":$CAPS,"permitted":$CAPS}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":[{"type":"user"},{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}],
           "uidMappings":[{"containerID":0,"hostID":$(id -u),"size":1}],"gidMappings":[{"containerID":0,"hostID":$(id -g),"size":1}],
           $PATHS} }
JSON
}
bundle "$B/bundle-sleep" '["/usr/bin/sleep","infinity"]'
bundle "$B/bundle-true" '["/usr/bin/true"]'

{ echo "cell: $(hostname), kernel $(uname -r), uid $(id -u), scope $CG, controllers $ctl, $(date -u +%FT%TZ)"
  echo "load at start: $(cat /proc/loadavg)"; } > "$OUT/env.txt"
PIDS=() LOOPS=()
stop() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done; for p in "${PIDS[@]}"; do wait "$p" 2>/dev/null; done; }
trap stop EXIT
if [ "${IDLE_THREADS:-8000}" -gt 0 ]; then
  ( echo "$BASHPID" > "$CG/load/cgroup.procs"; exec "$IDLE" "${IDLE_THREADS:-8000}" ) > "$OUT/idlethreads.out" 2>&1 &
  PIDS+=("$!")
  # As create_breakdown.sh waits for them: up to 150 looks, 0.2 s apart.
  for i in $(seq 1 150); do
    up=$(ls "/proc/${PIDS[0]}/task" 2>/dev/null | wc -l)
    [ "$up" -ge "${IDLE_THREADS:-8000}" ] && break
    [ "$i" = 150 ] && { echo "refusing: only $up of ${IDLE_THREADS:-8000} idle threads came up"; exit 5; }
    sleep 0.2
  done
  echo "idle threads: $up up (asked ${IDLE_THREADS:-8000})" >> "$OUT/env.txt"
fi
for _ in $(seq 1 "$L"); do
  ( echo "$BASHPID" > "$CG/load/cgroup.procs"; exec taskset -c "${CPUSET:?CPUSET}" bash -c 'while :; do :; done' ) &
  PIDS+=("$!"); LOOPS+=("$!")
done
echo "busy loops: $L on CPUs $CPUSET" >> "$OUT/env.txt"
STATE=${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR}/hex-g6-libcrun-$$
mkdir -p "$STATE"
ROUNDS_SEAM=$SEAM ROUNDS_HEX=$HEX ROUNDS_SLEEP=$B/bundle-sleep ROUNDS_TRUE=$B/bundle-true ROUNDS_CGROUP=$CG/arms \
  ROUNDS_LIBCRUN=$LIBCRUN ROUNDS_LIBCRUN_ROOT=$STATE ROUNDS_LIBCRUN_CGROUP=${CG#/sys/fs/cgroup}/libcrun \
  python3 -I "$HERE/rounds.py" "$OUT/rounds" "${ROUNDS:?ROUNDS}" "${WARMUP:?WARMUP}" 8 8 libcrun hex
rc=$?
# No wait for the loops before the rounds: the nodes come after them. Each loop's CPU time at the end (utime + stime,
# clock ticks) shows it ran throughout.
for p in "${LOOPS[@]}"; do read -ra f < "/proc/$p/stat" && echo "$p $(( f[13] + f[14] ))"; done 2>/dev/null \
  | paste -sd' ' | sed "s/^/busy loops' CPU ticks by pid (CLK_TCK $(getconf CLK_TCK)): /" >> "$OUT/env.txt"
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
rm -rf "${STATE:?}"
echo "results: $OUT"
exit "$rc"
