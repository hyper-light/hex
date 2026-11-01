#!/usr/bin/env bash
# bench/competitive/probes: where the Runtime seam's create spends its time, inside the bench container
# (create_breakdown.sh starts it with host.sh's OCI-pass flags, so the node's box cgroups have their controllers).
# After a probe of each, ROUNDS rounds, each running these in a shuffled order, so drift over the run favours none:
#   libcrun  oci_bench.py's lifecycle phase on libcrun in process: the create p50 goal 5's target is a quarter of;
#   seam     seam_oci's lifecycle phase, the oci class through the seam in process: its measured create p50;
#   traced   the same with the breakdown's tracer on around create (seam_oci's HEXBENCH_TRACE and
#            HEXBENCH_TRACE_OPS=create): an ftrace instance of this run's own records every other timed create and
#            what it ran, cut into parts by create_breakdown.py; the creates between are its untraced twins.
# Then create_breakdown.py writes summary.md and summary.json. A round's item that fails is logged and skipped (the
# summary names what's missing); a probe that fails ends the run before any round. The probes include a detailed
# trace, which adds each open's entry with its path, so create_breakdown.py can hold the rounds' cuts to the files
# they stand for.
# Env: N (1000), WARMUP (100), ROUNDS (3).
set -uo pipefail
N=${N:-1000}; WARMUP=${WARMUP:-100}; ROUNDS=${ROUNDS:-3}
OUT=/opt/out; mkdir -p /run/err

# The OCI pass's bundles (oci_inside.sh): the `sleep` container seam_oci's lifecycle creates, and the `true` one
# its command line names.
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

# EXTRA_MOUNTS (goal 5 plan step 2, AC-d): that many tmpfs in the agent's own mount namespace, away from
# the bundles' mount, so the classic init's clone (which copies the agent's table) would grow with them.
# The pruned seed pruned its namespace once at start, so every init it clones copies its two mounts and
# the init's clone (part 16) stays flat; the agent's open_tree of the rootfs, cloning a mount whose source
# (the bundles' /run) these aren't under, stays small too.
if [ "${EXTRA_MOUNTS:-0}" -gt 0 ]; then
  mkdir -p /mnt/em
  for i in $(seq 1 "${EXTRA_MOUNTS}"); do
    mkdir -p "/mnt/em/$i" && mount -t tmpfs none "/mnt/em/$i" -o size=1k,nr_inodes=4 2>/dev/null || true
  done
  echo "extra agent mounts: $(grep -c ' /mnt/em/' /proc/self/mountinfo) up (asked ${EXTRA_MOUNTS}), agent mounts now $(grep -c '' /proc/self/mountinfo)"
fi

# cgroup v2's no-internal-process rule, as oci_inside.sh: everything to a leaf so the root parents containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done

# The tracer: a trace instance of this run's own, so its buffer, events and filters touch no other tracer's. The
# events are where create_breakdown.py cuts a create's parts, and only those that cost a call little: an open's
# entry fetches its path, about 4 us a time (exec_breakdown.md), so the rounds trace the opens' exits and an open is
# told by its place in the call; the detailed trace adds the entries (DETAIL), for the probe that holds those places
# to their files. mkdir's entry fetches its path too, and a create makes two, so it stays: its path tells the
# cgroup's from the state directory's in every round. Each event is kept only for the names this run's processes
# have: seam_oci (the node agent, and the child it clones for the shim until that execs) and hex (the shim or a
# seed, and the container's init either clones, until the init execs its workload at start); a write only for hex's,
# the shim's pid line and the init's ready signal. open as well as openat: rustix opens with open(2) on x86_64. The
# clock is CLOCK_MONOTONIC's.
T=/sys/kernel/tracing
mountpoint -q "$T" || mount -t tracefs nodev "$T" || { echo "refusing: no tracefs at $T" >&2; exit 4; }
# A per-run instance name. `$$` is 1 in the bench container (bash is its init) and tracefs is the host's,
# shared across containers, so `hexcb-$$` was `hexcb-1` for every run and collided when a prior run left
# its instance behind under load (its untrace's rmdir raced); the epoch-nanosecond suffix makes it unique.
I="$T/instances/hexcb-$$-$(date +%s%N)"
EVENTS="sched/sched_process_fork sched/sched_process_exec sched/sched_process_exit
  syscalls/sys_exit_openat syscalls/sys_exit_open syscalls/sys_enter_fsync syscalls/sys_exit_fsync
  syscalls/sys_exit_rename syscalls/sys_exit_renameat syscalls/sys_exit_renameat2
  syscalls/sys_enter_mkdir syscalls/sys_exit_mkdir syscalls/sys_enter_mkdirat syscalls/sys_exit_mkdirat
  syscalls/sys_enter_mknodat syscalls/sys_enter_mknod syscalls/sys_enter_bpf syscalls/sys_exit_bpf
  syscalls/sys_enter_pipe2
  syscalls/sys_enter_clone3 syscalls/sys_exit_clone3 syscalls/sys_enter_clone syscalls/sys_exit_clone
  syscalls/sys_enter_setns syscalls/sys_exit_setns syscalls/sys_enter_pivot_root syscalls/sys_exit_pivot_root
  syscalls/sys_enter_chroot syscalls/sys_exit_chroot
  syscalls/sys_enter_open_tree syscalls/sys_exit_open_tree
  syscalls/sys_enter_write syscalls/sys_exit_poll syscalls/sys_exit_ppoll syscalls/sys_exit_wait4
  syscalls/sys_exit_waitid"
DETAIL="syscalls/sys_enter_openat syscalls/sys_enter_open"
filter() { # filter <event>: the processes it's kept for
  case $1 in
    */sys_enter_write) echo 'comm == "hex"' ;;
    *) echo 'comm == "seam_oci" || comm == "hex"' ;;
  esac
}
untrace() { # the instance's events off and the instance gone, whatever state it's in
  [ -d "$I" ] || return 0
  echo 0 > "$I/tracing_on" 2>/dev/null
  for ev in $EVENTS $DETAIL; do echo 0 > "$I/events/$ev/enable" 2>/dev/null; done
  rmdir "$I" 2>/dev/null || echo "the trace instance $I wouldn't go" >&2
}
trap untrace EXIT

# traced <out name> <n> <warmup> [detail]: seam_oci's lifecycle phase with the tracer on around create, its trace in
# <out name>.txt and the instance's per-CPU counts in <out name>.stats (create_breakdown.py refuses a trace that lost
# events); `detail` adds the opens' entries.
traced() {
  local name=$1 n=$2 warmup=$3 events=$EVENTS rc=0
  [ "${4:-}" = detail ] && events="$EVENTS $DETAIL"
  mkdir "$I" && echo mono > "$I/trace_clock" && echo 0 > "$I/tracing_on" && echo 4096 > "$I/buffer_size_kb" \
    || { echo "the trace instance $I couldn't be made" >&2; untrace; return 1; }
  for ev in $events; do
    { filter "$ev" > "$I/events/$ev/filter" && echo 1 > "$I/events/$ev/enable"; } \
      || { echo "event $ev couldn't be filtered and enabled" >&2; untrace; return 1; }
  done
  rm -f /run/hexcb.stop
  python3 -I /opt/lab/probes/create_breakdown.py drain "$I/trace_pipe" "$OUT/$name.txt" /run/hexcb.stop &
  local drain=$!
  HEXBENCH_TRACE=$I HEXBENCH_TRACE_OPS=create HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/$name.json" "$n" \
    "$warmup" 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true 2>&1 | tee -a "$OUT/traced.log" || rc=1
  echo 0 > "$I/tracing_on"
  touch /run/hexcb.stop
  wait "$drain" || rc=1
  for c in "$I"/per_cpu/cpu*; do echo "== ${c##*/}"; cat "$c/stats"; done > "$OUT/$name.stats"
  untrace
  return $rc
}

# switch <out name> <lifecycles>: goal 5 plan step 2's AC-e. A throughput phase pinned to 8 creates in flight
# (seam_oci's HEXBENCH_THROUGHPUT_CONC) with the root's parts' events on — setns (the net-ns join that opens the
# mounts, part 19), chroot (the switch, part 20), and pivot_root to prove the path makes none — recording for the
# whole phase. It passes no HEXBENCH_TRACE, so seam_oci never toggles tracing_on; this turns it on once and leaves
# it, so every init's events are caught however they interleave. create_breakdown.py switch reads <out name>.txt
# against <out name>.stats (its per-CPU counts).
switch() {
  local name=$1 lc=$2 rc=0
  local SW="syscalls/sys_enter_setns syscalls/sys_exit_setns syscalls/sys_enter_chroot syscalls/sys_exit_chroot syscalls/sys_enter_pivot_root syscalls/sys_exit_pivot_root"
  mkdir "$I" && echo mono > "$I/trace_clock" && echo 0 > "$I/tracing_on" && echo 8192 > "$I/buffer_size_kb" \
    || { echo "the trace instance $I couldn't be made" >&2; untrace; return 1; }
  for ev in $SW; do
    { filter "$ev" > "$I/events/$ev/filter" && echo 1 > "$I/events/$ev/enable"; } \
      || { echo "event $ev couldn't be filtered and enabled" >&2; untrace; return 1; }
  done
  rm -f /run/hexcb.stop
  python3 -I /opt/lab/probes/create_breakdown.py drain "$I/trace_pipe" "$OUT/$name.txt" /run/hexcb.stop &
  local drain=$!
  echo 1 > "$I/tracing_on"
  HEXBENCH_PHASES=throughput HEXBENCH_THROUGHPUT_CONC=8 /opt/bin/seam_oci "$OUT/$name.json" 0 0 "$lc" \
    /opt/bin/hex /run/bundle-sleep /run/bundle-true 2>&1 | tee -a "$OUT/switch.log" || rc=1
  echo 0 > "$I/tracing_on"
  touch /run/hexcb.stop
  wait "$drain" || rc=1
  for c in "$I"/per_cpu/cpu*; do echo "== ${c##*/}"; cat "$c/stats"; done > "$OUT/$name.stats"
  for ev in $SW; do echo 0 > "$I/events/$ev/enable" 2>/dev/null; done
  untrace
  return $rc
}

# Where the node's records and the containers' state go (seam_oci keeps them under $HOME/.cache/hex-t7/ociseam, as
# in the OCI pass): their filesystem decides what an fsync costs.
mkdir -p "$HOME/.cache/hex-t7/ociseam"
{ echo "inside: date $(date -u +%FT%TZ), kernel $(uname -r), load $(cat /proc/loadavg)"
  echo "inside: cpus allowed $(sed -n 's/^Cpus_allowed_list:\t*//p' /proc/self/status)"
  echo "inside: the node's state under $HOME/.cache/hex-t7/ociseam, on $(stat -f -c %T "$HOME/.cache/hex-t7/ociseam")" \
    "($(findmnt -n -o FSTYPE,OPTIONS -T "$HOME/.cache/hex-t7/ociseam" | cut -c1-160))"
  echo "inside: /run on $(stat -f -c %T /run); the cgroup root's subtree_control: $(cat /sys/fs/cgroup/cgroup.subtree_control)"
  echo "params: N=$N WARMUP=$WARMUP ROUNDS=$ROUNDS AC_E=${AC_E:-0}"
} >> "$OUT/env.txt"

# AC_E (goal 5 plan step 2, AC-e): a focused run of its own, not the rounds. The root switch (chroot) p99 at
# 8 creates in flight, while the host holds AC-e's idle threads (create_breakdown.sh's IDLE_THREADS). A small
# seam probe that the binaries work at concurrency 8, then one traced throughput phase pinned to 8, read by
# create_breakdown.py switch (pass when p99 <= 0.5 ms and no init pivot_root). No libcrun, no rounds.
if [ "${AC_E:-0}" -gt 0 ]; then
  echo "AC-e: switch trace, 8 creates in flight, $AC_E lifecycles, load $(cut -d' ' -f1-3 /proc/loadavg)"
  HEXBENCH_PHASES=throughput HEXBENCH_THROUGHPUT_CONC=8 /opt/bin/seam_oci "$OUT/switch.probe.json" 0 0 16 \
    /opt/bin/hex /run/bundle-sleep /run/bundle-true > "$OUT/switch.log" 2>&1 \
    || { echo "refusing: the concurrency-8 seam probe failed ($OUT/switch.log)" >&2; echo done > "$OUT/inside.done"; exit 5; }
  switch switch "$AC_E" \
    || { echo "refusing: the switch trace failed ($OUT/switch.log)" >&2; echo done > "$OUT/inside.done"; exit 6; }
  python3 -I /opt/lab/probes/create_breakdown.py switch "$OUT/switch.txt" "$OUT/switch.stats" "$OUT/switch.json" | tee "$OUT/switch.result"
  rc=${PIPESTATUS[0]}
  echo "AC-e done (inside), load $(cut -d' ' -f1-3 /proc/loadavg)" >> "$OUT/env.txt"
  echo done > "$OUT/inside.done"
  exit "$rc"
fi

# Each once, small, before any round: a broken one ends the run here rather than costing the rounds.
HEXBENCH_PHASES=lifecycle python3 -I /opt/lab/oci_bench.py libcrun "$OUT/probe.libcrun.json" 2 0 0 0 64 > "$OUT/probe.log" 2>&1 \
  || { echo "refusing: libcrun's probe failed ($OUT/probe.log)" >&2; exit 5; }
HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/probe.seam.json" 2 0 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true \
  >> "$OUT/probe.log" 2>&1 || { echo "refusing: seam_oci's probe failed ($OUT/probe.log)" >&2; exit 5; }
KINDS="libcrun seam traced"
if [ -x /opt/bin/before-hex ]; then
  KINDS="$KINDS before"
  HEXBENCH_PHASES=lifecycle /opt/bin/before-seam_oci "$OUT/probe.before.json" 2 0 0 /opt/bin/before-hex /run/bundle-sleep \
    /run/bundle-true >> "$OUT/probe.log" 2>&1 || { echo "refusing: the before binary's probe failed ($OUT/probe.log)" >&2; exit 5; }
fi
traced probe.traced 3 1 >> "$OUT/probe.log" 2>&1 || { echo "refusing: the traced probe failed ($OUT/probe.log)" >&2; exit 5; }
traced probe.detail 20 1 detail >> "$OUT/probe.log" 2>&1 \
  || { echo "refusing: the detailed traced probe failed ($OUT/probe.log)" >&2; exit 5; }
python3 -I /opt/lab/probes/create_breakdown.py check "$OUT" probe >> "$OUT/probe.log" 2>&1 \
  || { echo "refusing: the probes' results don't read whole ($OUT/probe.log)" >&2; exit 5; }
echo "probes ok"
[ "${PROBE_ONLY:-0}" = 1 ] && { echo "PROBE_ONLY=1: no rounds"; echo done > "$OUT/inside.done"; exit 0; }

# What the host is doing as each item starts: its load, and the share of the last 10 s in which some task was
# stalled on CPU, memory or I/O (/proc/pressure, the kernel's PSI), so a round that met a different host shows.
host_now() {
  echo "load $(cut -d' ' -f1-3 /proc/loadavg); stalled cpu/memory/io $(for r in cpu memory io; do
    sed -n 's/^some avg10=\([0-9.]*\).*/\1/p' "/proc/pressure/$r"; done | paste -sd/)%"
}
for round in $(seq 1 "$ROUNDS"); do
  for one in $(printf '%s\n' $KINDS | shuf); do
    echo "round $round: $one ($(host_now))"
    case $one in
      libcrun) HEXBENCH_PHASES=lifecycle python3 -I /opt/lab/oci_bench.py libcrun "$OUT/libcrun.r$round.json" "$N" \
                 "$WARMUP" 0 0 64 >> "$OUT/libcrun.log" 2>&1 ;;
      seam) HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/seam.r$round.json" "$N" "$WARMUP" 0 /opt/bin/hex \
              /run/bundle-sleep /run/bundle-true >> "$OUT/seam.log" 2>&1 ;;
      traced) traced "traced.r$round" "$N" "$WARMUP" ;;
      before) HEXBENCH_PHASES=lifecycle /opt/bin/before-seam_oci "$OUT/before.r$round.json" "$N" "$WARMUP" 0 \
                /opt/bin/before-hex /run/bundle-sleep /run/bundle-true >> "$OUT/before.log" 2>&1 ;;
    esac || echo "round $round: $one FAILED"
  done
done
echo "at the end (inside): $(host_now)" >> "$OUT/env.txt"
python3 -I /opt/lab/probes/create_breakdown.py summarize "$OUT" > "$OUT/summary.md" || echo "summarize FAILED"
echo done > "$OUT/inside.done"
