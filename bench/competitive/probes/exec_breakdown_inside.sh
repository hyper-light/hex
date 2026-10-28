#!/usr/bin/env bash
# bench/competitive/probes: where the Runtime seam's exec and start spend their time, and the floor under exec,
# inside the bench container (exec_breakdown.sh starts it with host.sh's OCI-pass flags). After a probe of each,
# ROUNDS rounds, each running these in a shuffled order, so drift over the run favours none:
#   libcrun  oci_bench.py's lifecycle phase on libcrun in process: the exec p50 goal 3's target is a quarter of;
#   floor    exec_floor.c against a running `sleep` container crun made from the same bundle, right before or
#            after libcrun's, so the two meet the same host;
#   seam     seam_oci's lifecycle phase, the oci class through the seam in process: its measured p50s;
#   traced   the same with the breakdown's tracer on (seam_oci's HEXBENCH_TRACE): an ftrace instance of this
#            run's own records every other timed start and exec between seam_oci's markers, and what they ran,
#            cut into parts by exec_breakdown.py; the calls between are its untraced twins.
# Then exec_breakdown.py writes summary.md and summary.json. A round's item that fails is logged and skipped
# (the summary names what's missing); a probe that fails ends the run before any round. The probes include a
# detailed trace, which adds each open's entry with its path, so exec_breakdown.py can hold the rounds' cuts to
# the files they stand for.
# Env: N (1000), WARMUP (100), ROUNDS (2), FLOOR_N (1000), FLOOR_WARMUP (100).
set -uo pipefail
N=${N:-1000}; WARMUP=${WARMUP:-100}; ROUNDS=${ROUNDS:-2}; FLOOR_N=${FLOOR_N:-1000}; FLOOR_WARMUP=${FLOOR_WARMUP:-100}
OUT=/opt/out; mkdir -p /run/err

# The OCI pass's bundles (oci_inside.sh): a `sleep` container to exec into, and the `true` one seam_oci names.
ALL_NS='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
bundle() { # bundle <dir> <args json>
  local B=$1; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
  ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
  cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":$ALL_NS} }
JSON
}
bundle /run/bundle-sleep '["/usr/bin/sleep","infinity"]'
bundle /run/bundle-true '["/usr/bin/true"]'

# cgroup v2's no-internal-process rule, as oci_inside.sh: everything to a leaf so the root parents containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done

# The tracer: a trace instance of this run's own, so its buffer, events and filters touch no other tracer's.
# The events are where exec_breakdown.py cuts the parts, and only those that cost a call little: an open's or
# an execve's entry fetches its path, about 4 us a time in the trial runs (exec_breakdown.md), so the rounds
# trace the opens' exits, and an open is told by its place in the call; the detailed trace adds the entries
# (DETAIL), for the probe that holds those places to their files. Each event is kept only for the names this
# run's processes have: seam_oci; hex (the exec helper, the child it clones until that execs, and a container's
# init until it execs its workload); true (the program), whose loader's opens aren't cuts. open as well as
# openat: rustix opens with open(2) on x86_64. The clock is CLOCK_MONOTONIC's.
T=/sys/kernel/tracing
mountpoint -q "$T" || mount -t tracefs nodev "$T" || { echo "refusing: no tracefs at $T" >&2; exit 4; }
I="$T/instances/hexbd-$$"
EVENTS="sched/sched_process_fork sched/sched_process_exec sched/sched_process_exit
  syscalls/sys_exit_openat syscalls/sys_exit_open syscalls/sys_enter_setns syscalls/sys_exit_setns
  syscalls/sys_enter_clone3 syscalls/sys_exit_clone3 syscalls/sys_enter_clone syscalls/sys_exit_clone
  syscalls/sys_enter_exit_group syscalls/sys_exit_poll syscalls/sys_exit_ppoll syscalls/sys_exit_wait4
  syscalls/sys_exit_waitid syscalls/sys_exit_rename syscalls/sys_exit_renameat syscalls/sys_exit_renameat2"
DETAIL="syscalls/sys_enter_openat syscalls/sys_enter_open"
filter() { # filter <event>: the processes it's kept for
  case $1 in
    */sys_*_open|*/sys_*_openat) echo 'comm == "seam_oci" || comm == "hex"' ;;
    *) echo 'comm == "seam_oci" || comm == "hex" || comm == "true"' ;;
  esac
}
untrace() { # the instance's events off and the instance gone, whatever state it's in
  [ -d "$I" ] || return 0
  echo 0 > "$I/tracing_on" 2>/dev/null
  for ev in $EVENTS $DETAIL; do echo 0 > "$I/events/$ev/enable" 2>/dev/null; done
  rmdir "$I" 2>/dev/null || echo "the trace instance $I wouldn't go" >&2
}
trap untrace EXIT

# traced <out name> <n> <warmup> [detail]: seam_oci's lifecycle phase with the tracer on, its trace in
# <out name>.txt and the instance's per-CPU counts in <out name>.stats (exec_breakdown.py refuses a trace that
# lost events); `detail` adds the opens' entries.
traced() {
  local name=$1 n=$2 warmup=$3 events=$EVENTS rc=0
  [ "${4:-}" = detail ] && events="$EVENTS $DETAIL"
  mkdir "$I" && echo mono > "$I/trace_clock" && echo 0 > "$I/tracing_on" && echo 4096 > "$I/buffer_size_kb" \
    || { echo "the trace instance $I couldn't be made" >&2; untrace; return 1; }
  for ev in $events; do
    { filter "$ev" > "$I/events/$ev/filter" && echo 1 > "$I/events/$ev/enable"; } \
      || { echo "event $ev couldn't be filtered and enabled" >&2; untrace; return 1; }
  done
  rm -f /run/hexbd.stop
  python3 -I /opt/lab/probes/exec_breakdown.py drain "$I/trace_pipe" "$OUT/$name.txt" /run/hexbd.stop &
  local drain=$!
  HEXBENCH_TRACE=$I HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/$name.json" "$n" "$warmup" 0 /opt/bin/hex \
    /run/bundle-sleep /run/bundle-true 2>&1 | tee -a "$OUT/traced.log" || rc=1
  echo 0 > "$I/tracing_on"
  touch /run/hexbd.stop
  wait "$drain" || rc=1
  for c in "$I"/per_cpu/cpu*; do echo "== ${c##*/}"; cat "$c/stats"; done > "$OUT/$name.stats"
  untrace
  return $rc
}

# floor <out name> <n> <warmup>: exec_floor against a running `sleep` container crun makes from the same
# bundle, its init's pid from crun's state and its cgroup from the init's.
floor() {
  local name=$1 n=$2 warmup=$3 R=/run/floor-crun id="floor-$$-$1" rc=0
  /opt/bin/crun --root "$R" create --bundle /run/bundle-sleep "$id" < /dev/null > /dev/null 2>> "$OUT/floor.log" \
    && /opt/bin/crun --root "$R" start "$id" 2>> "$OUT/floor.log" || { echo "crun couldn't start $id" >&2; rc=1; }
  if [ $rc = 0 ]; then
    local pid cg
    pid=$(/opt/bin/crun --root "$R" state "$id" | python3 -I -c 'import json, sys; print(json.load(sys.stdin)["pid"])')
    cg=/sys/fs/cgroup$(sed -n 's/^0:://p' "/proc/$pid/cgroup")
    echo "$name: container $id, init $pid, cgroup $cg" >> "$OUT/floor.log"
    /opt/bin/exec_floor "$OUT/$name.json" "$n" "$warmup" "$pid" "$cg" 2>> "$OUT/floor.log" || rc=1
  fi
  /opt/bin/crun --root "$R" delete --force "$id" 2>> "$OUT/floor.log"
  return $rc
}

{ echo "inside: date $(date -u +%FT%TZ), kernel $(uname -r), load $(cat /proc/loadavg)"
  echo "inside: /usr/bin/true is $(file -b /usr/bin/true 2>/dev/null | cut -d, -f1-2 || echo unknown)"
  echo "inside: cpus allowed $(sed -n 's/^Cpus_allowed_list:\t*//p' /proc/self/status)"
  echo "params: N=$N WARMUP=$WARMUP ROUNDS=$ROUNDS FLOOR_N=$FLOOR_N FLOOR_WARMUP=$FLOOR_WARMUP"
} >> "$OUT/env.txt"

# Each once, small, before any round: a broken one ends the run here rather than costing the rounds.
HEXBENCH_PHASES=lifecycle python3 -I /opt/lab/oci_bench.py libcrun "$OUT/probe.libcrun.json" 2 0 0 0 64 > "$OUT/probe.log" 2>&1 \
  || { echo "refusing: libcrun's probe failed ($OUT/probe.log)" >&2; exit 5; }
HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/probe.seam.json" 2 0 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true \
  >> "$OUT/probe.log" 2>&1 || { echo "refusing: seam_oci's probe failed ($OUT/probe.log)" >&2; exit 5; }
traced probe.traced 3 1 >> "$OUT/probe.log" 2>&1 || { echo "refusing: the traced probe failed ($OUT/probe.log)" >&2; exit 5; }
traced probe.detail 20 1 detail >> "$OUT/probe.log" 2>&1 \
  || { echo "refusing: the detailed traced probe failed ($OUT/probe.log)" >&2; exit 5; }
floor probe.floor 5 1 >> "$OUT/probe.log" 2>&1 || { echo "refusing: the floor's probe failed ($OUT/floor.log)" >&2; exit 5; }
python3 -I /opt/lab/probes/exec_breakdown.py check "$OUT" probe >> "$OUT/probe.log" 2>&1 \
  || { echo "refusing: the probes' results don't read whole ($OUT/probe.log)" >&2; exit 5; }
echo "probes ok"

# What the host is doing as each item starts: its load, and the share of the last 10 s in which some task was
# stalled on CPU, memory or I/O (/proc/pressure, the kernel's PSI), so a round that met a different host shows.
host_now() {
  echo "load $(cut -d' ' -f1-3 /proc/loadavg); stalled cpu/memory/io $(for r in cpu memory io; do
    sed -n 's/^some avg10=\([0-9.]*\).*/\1/p' "/proc/pressure/$r"; done | paste -sd/)%"
}
# libcrun's exec and the floor run back to back, in either order, so the floor is set against the libcrun exec
# that met the same host.
for round in $(seq 1 "$ROUNDS"); do
  for item in $(printf '%s\n' pair seam traced | shuf); do
    [ "$item" = pair ] && item=$(printf '%s\n' libcrun floor | shuf | paste -sd' ')
    for one in $item; do
      echo "round $round: $one ($(host_now))"
      case $one in
        libcrun) HEXBENCH_PHASES=lifecycle python3 -I /opt/lab/oci_bench.py libcrun "$OUT/libcrun.r$round.json" "$N" \
                   "$WARMUP" 0 0 64 >> "$OUT/libcrun.log" 2>&1 ;;
        seam) HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/seam.r$round.json" "$N" "$WARMUP" 0 /opt/bin/hex \
                /run/bundle-sleep /run/bundle-true >> "$OUT/seam.log" 2>&1 ;;
        traced) traced "traced.r$round" "$N" "$WARMUP" ;;
        floor) floor "floor.r$round" "$FLOOR_N" "$FLOOR_WARMUP" ;;
      esac || echo "round $round: $one FAILED"
    done
  done
done
echo "at the end (inside): $(host_now)" >> "$OUT/env.txt"
python3 -I /opt/lab/probes/exec_breakdown.py summarize "$OUT" > "$OUT/summary.md" || echo "summarize FAILED"
echo done > "$OUT/inside.done"
