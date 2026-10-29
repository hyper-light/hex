#!/usr/bin/env bash
# bench/competitive/probes: exec_ab.sh's run inside the bench container. ROUNDS rounds, each running these in a
# shuffled order, so drift over the run favours none:
#   libcrun  oci_bench.py's lifecycle phase on libcrun in process: the exec and start each build is set against;
#   <build>  that build's seam_oci lifecycle phase, the oci class through the seam in process.
# With TRACE=1, then each build's lifecycle once more with an ftrace instance of the run's own recording, between
# seam_oci's markers (HEXBENCH_TRACE), the scheduler's events beside exec_breakdown_inside.sh's: forks, wakeups,
# switches and migrations, for exec_ab.py sched. A round's item that fails is logged and skipped.
# Env: BUILDS (the build names, from exec_ab.sh), N (600), WARMUP (60), ROUNDS (3), TRACE (0), TRACE_N (300).
set -uo pipefail
N=${N:-600}; WARMUP=${WARMUP:-60}; ROUNDS=${ROUNDS:-3}; TRACE_N=${TRACE_N:-300}; OUT=/opt/out
mkdir -p /run/err

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

{ echo "inside: date $(date -u +%FT%TZ), kernel $(uname -r), load $(cat /proc/loadavg)"
  echo "inside: cpus allowed $(sed -n 's/^Cpus_allowed_list:\t*//p' /proc/self/status)"
  echo "params: BUILDS=$BUILDS N=$N WARMUP=$WARMUP ROUNDS=$ROUNDS TRACE=${TRACE:-0} TRACE_N=$TRACE_N"
} >> "$OUT/env.txt"

seam() { # seam <build> <out json> <n> <warmup>
  HEXBENCH_PHASES=lifecycle "/opt/bin/seam_oci.$1" "$2" "$3" "$4" 0 "/opt/bin/hex.$1" /run/bundle-sleep /run/bundle-true \
    >> "$OUT/$1.log" 2>&1
}
for round in $(seq 1 "$ROUNDS"); do
  for item in $(printf '%s\n' libcrun $BUILDS | shuf); do
    echo "round $round: $item (load $(cut -d' ' -f1-3 /proc/loadavg))"
    case $item in
      libcrun) HEXBENCH_PHASES=lifecycle python3 -I /opt/lab/oci_bench.py libcrun "$OUT/libcrun.r$round.json" "$N" \
                 "$WARMUP" 0 0 64 >> "$OUT/libcrun.log" 2>&1 ;;
      *) seam "$item" "$OUT/$item.r$round.json" "$N" "$WARMUP" ;;
    esac || echo "round $round: $item FAILED"
  done
done

[ "${TRACE:-0}" = 0 ] && exit 0
T=/sys/kernel/tracing
mountpoint -q "$T" || mount -t tracefs nodev "$T" || { echo "refusing: no tracefs at $T" >&2; exit 4; }
I="$T/instances/hexab-$$"
EVENTS="sched/sched_process_fork sched/sched_process_exec sched/sched_process_exit sched/sched_switch
  sched/sched_wakeup sched/sched_wakeup_new sched/sched_migrate_task syscalls/sys_enter_clone3 syscalls/sys_exit_clone3
  syscalls/sys_enter_setns syscalls/sys_exit_setns syscalls/sys_enter_exit_group syscalls/sys_exit_ppoll syscalls/sys_exit_waitid"
filter() { # filter <event>: the processes it's kept for, seam_oci and the program it execs
  case $1 in
    sched/sched_switch) echo 'prev_comm == "seam_oci" || next_comm == "seam_oci" || prev_comm == "true" || next_comm == "true"' ;;
    *) echo 'comm == "seam_oci" || comm == "true"' ;;
  esac
}
untrace() {
  [ -d "$I" ] || return 0
  echo 0 > "$I/tracing_on" 2>/dev/null
  for ev in $EVENTS; do echo 0 > "$I/events/$ev/enable" 2>/dev/null; done
  rmdir "$I" 2>/dev/null || echo "the trace instance $I wouldn't go" >&2
}
trap untrace EXIT
for build in $BUILDS; do
  echo "traced: $build (load $(cut -d' ' -f1-3 /proc/loadavg))"
  mkdir "$I" && echo mono > "$I/trace_clock" && echo 0 > "$I/tracing_on" && echo 16384 > "$I/buffer_size_kb" \
    || { echo "the trace instance $I couldn't be made" >&2; untrace; continue; }
  for ev in $EVENTS; do filter "$ev" > "$I/events/$ev/filter" && echo 1 > "$I/events/$ev/enable"; done
  rm -f /run/hexab.stop
  python3 -I /opt/lab/probes/exec_breakdown.py drain "$I/trace_pipe" "$OUT/$build.trace.txt" /run/hexab.stop &
  drain=$!
  HEXBENCH_TRACE=$I seam "$build" "$OUT/$build.traced.json" "$TRACE_N" "$WARMUP" || echo "traced: $build FAILED"
  echo 0 > "$I/tracing_on"; touch /run/hexab.stop; wait "$drain"
  for c in "$I"/per_cpu/cpu*; do echo "== ${c##*/}"; cat "$c/stats"; done > "$OUT/$build.trace.stats"
  untrace
done
