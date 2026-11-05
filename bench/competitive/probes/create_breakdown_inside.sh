#!/usr/bin/env bash
# bench/competitive/probes: where the Runtime seam's create spends its time, inside the bench container
# (create_breakdown.sh starts it with host.sh's OCI-pass flags, so the node's box cgroups have their controllers).
# After a probe of each, ROUNDS rounds, each running these in a shuffled order, so drift over the run favours none:
#   libcrun  oci_bench.py's lifecycle phase on libcrun in process: the create p50 goal 5's target is a quarter of;
#   seam     seam_oci's lifecycle phase, the oci class through the seam in process: its measured create p50;
#   traced   the same with the breakdown's tracer on around create (seam_oci's HEXBENCH_TRACE and
#            HEXBENCH_TRACE_OPS=create): an ftrace instance of this run's own records every other timed create and
#            what it ran, cut into parts by create_breakdown.py; the creates between are its untraced twins.
#   seam_idle, libcrun_idle  with IDLE_AB set: the seam's and libcrun's lifecycle phases with that many idle threads
#            up in this container (with_idle), so the same rounds time each with and without them (goal 5 plan
#            step 2's AC-c within one run); libcrun's pivot_root walks them, the control that they are walked.
# Then create_breakdown.py writes summary.md and summary.json. A round's item that fails is logged and skipped (the
# summary names what's missing); a probe that fails ends the run before any round. The probes include a detailed
# trace, which adds each open's entry with its path, so create_breakdown.py can hold the rounds' cuts to the files
# they stand for.
# Env: N (1000), WARMUP (100), ROUNDS (3), IDLE_AB (0: no idle kinds).
set -uo pipefail
N=${N:-1000}; WARMUP=${WARMUP:-100}; ROUNDS=${ROUNDS:-3}
OUT=/opt/out; mkdir -p /run/err

# The OCI pass's bundles (oci_inside.sh): the `sleep` container seam_oci's lifecycle creates, and the `true` one
# its command line names.
ALL_NS='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
# With HEXBENCH_OWNER, the bundles are oci_inside.sh's for an owner (goal 5 extended, step 6): Docker's default
# capabilities (moby's oci/caps/defaults.go), no CAP_SYS_ADMIN, and /proc/sys read-only as Docker's readonlyPaths
# make it, so a box's cgroup and network namespace may be handed to the owner's next box.
CAPS='["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FSETID","CAP_FOWNER","CAP_MKNOD","CAP_NET_RAW","CAP_SETGID","CAP_SETUID","CAP_SETFCAP","CAP_SETPCAP","CAP_NET_BIND_SERVICE","CAP_SYS_CHROOT","CAP_KILL","CAP_AUDIT_WRITE"]'
PROC_CAPS=""; [ -n "${HEXBENCH_OWNER:-}" ] && PROC_CAPS=",\"capabilities\":{\"bounding\":$CAPS,\"effective\":$CAPS,\"permitted\":$CAPS}"
RO_PATHS=""; [ -n "${HEXBENCH_OWNER:-}" ] && RO_PATHS=',"readonlyPaths":["/proc/sys"]'
# With HEXBENCH_DOCKER_PATHS=1, the masks and read-only paths of Docker's default spec instead (moby docker-v29.9.2
# daemon/pkg/oci/defaults.go:110-116 and :194-207; its per-CPU thermal_throttle masks lie under /sys, which these
# bundles don't mount): goal 5 extended step 3's measure of the init binding them through its own proc.
if [ "${HEXBENCH_DOCKER_PATHS:-0}" = 1 ]; then
  RO_PATHS=',"maskedPaths":["/proc/acpi","/proc/asound","/proc/interrupts","/proc/kcore","/proc/keys","/proc/latency_stats","/proc/sched_debug","/proc/scsi","/proc/timer_list","/proc/timer_stats","/sys/devices/virtual/powercap","/sys/firmware"],"readonlyPaths":["/proc/bus","/proc/fs","/proc/irq","/proc/sys","/proc/sysrq-trigger"]'
fi
bundle() { # bundle <dir> <args json>
  local B=$1; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
  ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
  cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}$PROC_CAPS},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":$ALL_NS$RO_PATHS} }
JSON
}
bundle /run/bundle-sleep '["/usr/bin/sleep","infinity"]'
bundle /run/bundle-true '["/usr/bin/true"]'
# A createRuntime hook routes a bundle's every create classic, through the shim (a create-time hook runs against host
# paths, decision 5). The node runs it in its own namespace once the init waits at the exec FIFO (run.rs's
# record_created), so it adds to the create's tail and nothing to the init's mounts.
with_hook() { # with_hook <bundle>
  python3 -I -c 'import json, sys
p = sys.argv[1]; c = json.load(open(p)); c["hooks"] = {"createRuntime": [{"path": "/usr/bin/true", "args": ["true"]}]}
json.dump(c, open(p, "w"))' "$1/config.json"
}
# MOUNTS_PATH=classic (goal 5 plan step 3): the mounts trace's `true` creates take the classic path.
if [ "${MOUNTS_PATH:-pruned}" = classic ]; then
  bundle /run/bundle-true-classic '["/usr/bin/true"]'
  with_hook /run/bundle-true-classic
fi
# CLASSIC=1 (goal 5 plan step 3): both bundles take the classic path, so the rounds time the classic init's root
# mounted (the shim path's part 23, its first run to its pivot_root) as they time the pruned path's part 19.
if [ "${CLASSIC:-0}" = 1 ]; then
  with_hook /run/bundle-sleep
  with_hook /run/bundle-true
fi

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
#
# Each open's entry is marked too, by a kprobe on the openat and open system calls' entry points that records
# nothing but where it fired (goal 5 extended step 5): it fetches no path, so it costs a call little, and a record
# write is cut from its file's open on, its filesystem work its own parts: a durable record to the rename that puts
# it in place, a non-durable one — written once straight to its file with O_EXCL, no temp and no rename (goal 5
# extended step 5) — to that fd's close, so close is traced too (sys_enter_close names the fd sys_exit_openat
# returned, sys_exit_close is the cut's end). The kprobes are defined for this run alone (group hexcb_<tag>),
# enabled only in its instance, and removed when it ends. The instance and the group are named by the tag
# create_breakdown.sh gives the run (TRACE_TAG, its UTC stamp and its pid on the host): this shell's own pid is 1 in
# every probe container, so a name of it would be every run's, and a killed run's instance would refuse the next
# (create_breakdown.md, the 100 rounds' second chunk). UNTRACE_ONLY=1 takes the tag's instance and kprobes down and
# ends: create_breakdown.sh runs it once its container is gone, for a run killed before its own trap ran.
TAG=${TRACE_TAG:?create_breakdown.sh gives the run its TRACE_TAG}
case $TAG in *[!A-Za-z0-9_]*|"") echo "refusing: TRACE_TAG $TAG names no trace instance" >&2; exit 2 ;; esac
T=/sys/kernel/tracing
mountpoint -q "$T" || mount -t tracefs nodev "$T" || { echo "refusing: no tracefs at $T" >&2; exit 4; }
I="$T/instances/hexcb-$TAG"
KG="hexcb_$TAG"
PROBES="$KG/openat_in $KG/open_in"
case $(uname -m) in
  x86_64) SYS=__x64_sys ;;
  aarch64) SYS=__arm64_sys ;;
  *) echo "refusing: no syscall entry points known for $(uname -m)" >&2; exit 4 ;;
esac
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
unprobe() { # this run's kprobes gone, once no instance has them on
  untrace
  for ev in $PROBES; do echo "-:$ev" >> "$T/kprobe_events" 2>/dev/null; done
}
trap unprobe EXIT
[ "${UNTRACE_ONLY:-0}" = 1 ] && exit 0
{ echo "p:$KG/openat_in ${SYS}_openat" >> "$T/kprobe_events" && echo "p:$KG/open_in ${SYS}_open" >> "$T/kprobe_events"; } \
  || { echo "refusing: the opens' entry kprobes couldn't be defined" >&2; exit 4; }

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
  local SW="syscalls/sys_exit_clone3 syscalls/sys_enter_setns syscalls/sys_exit_setns syscalls/sys_enter_chroot syscalls/sys_exit_chroot syscalls/sys_enter_pivot_root syscalls/sys_exit_pivot_root syscalls/sys_enter_chdir"
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

# mounts_trace <out name> <lifecycles> <concurrency>: goal 5 plan step 3's per-call trace of the init's mounts. A
# throughput phase at <concurrency> creates in flight with every system call the hex processes make traced by
# raw_syscalls, which records a call's number and arguments and fetches no string (the syscalls subsystem's events
# fetch each path, about 4 us a time, which would charge the very calls this times), with their switches off and on
# the CPU, their page faults in user mode and their forks. It records for the whole phase, as switch does. create_breakdown.py
# mounts cuts part 19 of each create out of it, from the init's join of the network namespace made ahead to its
# chroot (or pivot_root), call by call and mount by mount; the calls' numbers are named from the host's
# asm/unistd_64.h, which the container sees under /usr. The softirqs ksoftirqd runs and the work items kworkers run
# are traced too, so an init preempted while either ran on its CPU has its wait charged to the softirq or the work
# function (the fedora traces of goal 5 step 3 found ksoftirqd and the kworkers first to the CPU in the tail), and so
# are the softirqs run on a hex process as an interrupt returns, which take its CPU without a switch.
mounts_trace() {
  local name=$1 lc=$2 conc=$3 rc=0 ev f nssem
  local MT="raw_syscalls/sys_enter raw_syscalls/sys_exit sched/sched_switch sched/sched_process_fork sched/sched_waking
    irq/softirq_entry irq/softirq_exit workqueue/workqueue_execute_start workqueue/workqueue_execute_end"
  [ -d "$T/events/exceptions/page_fault_user" ] && MT="$MT exceptions/page_fault_user"
  # The kernel's one mount lock, each hold of it counted (goal 5 extended step 3): a kprobe on down_write and one on
  # down_read, filtered to namespace_sem's address and to hex's processes, so each init's span counts its own holds
  # and the call it took each in. Every write hold is namespace_lock's down_write, every shared one down_read
  # (v6.19 fs/namespace.c:1723-1726, the namespace_shared guard at :97, the mount table's iterator at :1541).
  nssem=$(awk '$3 == "namespace_sem" {print "0x" $1; exit}' /proc/kallsyms)
  case $nssem in
    ""|0x0000000000000000) echo "namespace_sem's address isn't readable here: its holds go uncounted" >&2 ;;
    *) for ev in nsw nsr; do echo "-:hexcb/$ev" >> "$T/kprobe_events" 2>/dev/null; done
       { echo "p:hexcb/nsw down_write sem=%di:x64" >> "$T/kprobe_events" \
           && echo "p:hexcb/nsr down_read sem=%di:x64" >> "$T/kprobe_events"; } \
         || { echo "the namespace_sem probes couldn't be made" >&2; return 1; }
       MT="$MT hexcb/nsw hexcb/nsr"
       # A wait for the lock (goal 6): the rwsem's slowpaths, taken only when the lock can't be had at once. Where
       # the kernel inlined them away they can't be probed, and the report's lock waits read 0: said here.
       for ev in nsww nsrw; do echo "-:hexcb/$ev" >> "$T/kprobe_events" 2>/dev/null; done
       if { echo "p:hexcb/nsww rwsem_down_write_slowpath sem=%di:x64" >> "$T/kprobe_events" \
            && echo "p:hexcb/nsrw rwsem_down_read_slowpath sem=%di:x64" >> "$T/kprobe_events"; } 2>/dev/null; then
         MT="$MT hexcb/nsww hexcb/nsrw"
       else
         echo "the mount lock's slowpaths can't be probed here: its waits go uncounted" >&2
       fi ;;
  esac
  mkdir "$I" && echo mono > "$I/trace_clock" && echo 0 > "$I/tracing_on" && echo 16384 > "$I/buffer_size_kb" \
    || { echo "the trace instance $I couldn't be made" >&2; untrace; return 1; }
  for ev in $MT; do
    case $ev in
      hexcb/*) f="sem == $nssem && comm == \"hex\"" ;;
      sched/sched_switch) f='prev_comm == "hex" || next_comm == "hex"' ;;
      sched/sched_process_fork) f='parent_comm == "hex"' ;;
      sched/sched_waking) f='comm == "hex"' ;;
      irq/*) f='comm ~ "ksoftirqd*" || comm == "hex"' ;;
      workqueue/*) f='comm ~ "kworker*"' ;;
      *) f='comm == "hex"' ;;
    esac
    { echo "$f" > "$I/events/$ev/filter" && echo 1 > "$I/events/$ev/enable"; } \
      || { echo "event $ev couldn't be filtered and enabled" >&2; untrace; return 1; }
  done
  rm -f /run/hexcb.stop
  python3 -I /opt/lab/probes/create_breakdown.py drain "$I/trace_pipe" "$OUT/$name.txt" /run/hexcb.stop &
  local drain=$!
  echo 1 > "$I/tracing_on"
  local true_bundle=/run/bundle-true
  [ "${MOUNTS_PATH:-pruned}" = classic ] && true_bundle=/run/bundle-true-classic
  HEXBENCH_PHASES=throughput HEXBENCH_THROUGHPUT_CONC=$conc /opt/bin/seam_oci "$OUT/$name.json" 0 0 "$lc" \
    /opt/bin/hex /run/bundle-sleep "$true_bundle" 2>&1 | tee -a "$OUT/mounts.log" || rc=1
  echo 0 > "$I/tracing_on"
  touch /run/hexcb.stop
  wait "$drain" || rc=1
  for c in "$I"/per_cpu/cpu*; do echo "== ${c##*/}"; cat "$c/stats"; done > "$OUT/$name.stats"
  for ev in $MT; do echo 0 > "$I/events/$ev/enable" 2>/dev/null; done
  untrace
  for ev in nsw nsr nsww nsrw; do echo "-:hexcb/$ev" >> "$T/kprobe_events" 2>/dev/null; done
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
  echo "params: N=$N WARMUP=$WARMUP ROUNDS=$ROUNDS AC_E=${AC_E:-0} IDLE_AB=${IDLE_AB:-0} MOUNTS=${MOUNTS:-0}" \
    "MOUNTS_CONC=${MOUNTS_CONC:-1 8} MOUNTS_PATH=${MOUNTS_PATH:-pruned} MOUNTS_SPAN=${MOUNTS_SPAN:-switch} CLASSIC=${CLASSIC:-0} HEXBENCH_DEVFS=${HEXBENCH_DEVFS:-1} AC_E_MOUNTS=${AC_E_MOUNTS:-0}"
} >> "$OUT/env.txt"

# with_idle <command...>: the command run while IDLE_AB idle threads are up in this container (idlethreads.c, built
# by create_breakdown.sh). They are threads of the host's like any other, so pivot_root's walk of every thread
# (chroot_fs_refs's for_each_process_thread) visits them though they sit in this container's pid namespace. The
# helper is ended, its threads with it, before the command's status is returned; threads that don't all come up in
# 30 s refuse the item. Each item's threads go in idle_threads.txt, counted as it starts and as it ends: the helper's
# own and the host's, /proc/loadavg's count of the kernel's threads (nr_threads, which no pid namespace narrows).
with_idle() {
  local pid up end host i rc=0
  /opt/bin/idlethreads "$IDLE_AB" > /run/idlethreads.out 2>&1 &
  pid=$!
  for i in $(seq 1 150); do
    up=$(ls "/proc/$pid/task" 2>/dev/null | wc -l)
    [ "$up" -ge "$IDLE_AB" ] && break
    if [ "$i" = 150 ]; then
      echo "refusing: only $up of $IDLE_AB idle threads came up" >&2; kill "$pid"; wait "$pid" 2>/dev/null; return 1
    fi
    sleep 0.2
  done
  host=$(cut -d' ' -f4 /proc/loadavg | cut -d/ -f2)
  "$@" || rc=1
  end=$(ls "/proc/$pid/task" 2>/dev/null | wc -l)
  echo "${one:-probe} r${round:-0}: $up idle threads up at its start, $end at its end (asked $IDLE_AB); the host's" \
    "threads $host at its start, $(cut -d' ' -f4 /proc/loadavg | cut -d/ -f2) at its end" >> "$OUT/idle_threads.txt"
  kill "$pid"; wait "$pid" 2>/dev/null
  [ "$end" -ge "$IDLE_AB" ] || { echo "refusing: only $end of $IDLE_AB idle threads were up at the item's end" >&2; rc=1; }
  return $rc
}

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
  scope=switch
  [ "${AC_E_MOUNTS:-0}" = 1 ] && scope=mounts
  python3 -I /opt/lab/probes/create_breakdown.py switch "$OUT/switch.txt" "$OUT/switch.stats" "$OUT/switch.json" "$scope" | tee "$OUT/switch.result"
  rc=${PIPESTATUS[0]}
  echo "AC-e done (inside), load $(cut -d' ' -f1-3 /proc/loadavg)" >> "$OUT/env.txt"
  echo done > "$OUT/inside.done"
  exit "$rc"
fi

# MOUNTS (goal 5 plan step 3): a focused run of its own, not the rounds. For each concurrency in MOUNTS_CONC (1 and
# 8), one throughput phase of MOUNTS lifecycles with every call of the hex processes traced (mounts_trace), cut by
# create_breakdown.py mounts into part 19's calls, the time between them, the time off the CPU and the page faults,
# by mount; with MOUNTS_PATH=classic its creates take the classic path, through the shim. With MOUNTS_SPAN=root
# (goal 6) each span is the root in place, to the init's chdir, its mount namespace's join counted (create_breakdown.py
# mounts). No libcrun, no rounds.
if [ "${MOUNTS:-0}" -gt 0 ]; then
  rc=0
  for conc in ${MOUNTS_CONC:-1 8}; do
    echo "mounts: every call traced, $conc in flight, $MOUNTS lifecycles, load $(cut -d' ' -f1-3 /proc/loadavg)"
    mounts_trace "mounts.c$conc" "$MOUNTS" "$conc" \
      || { echo "refusing: the mounts trace at $conc failed ($OUT/mounts.log)" >&2; rc=6; continue; }
    python3 -I /opt/lab/probes/create_breakdown.py mounts "$OUT/mounts.c$conc.txt" "$OUT/mounts.c$conc.stats" \
      /usr/include/asm/unistd_64.h "${MOUNTS_PATH:-pruned}" "${MOUNTS_SPAN:-switch}" > "$OUT/mounts.c$conc.md" || rc=7
    cat "$OUT/mounts.c$conc.md"
  done
  echo "mounts done (inside), load $(cut -d' ' -f1-3 /proc/loadavg)" >> "$OUT/env.txt"
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
if [ "${IDLE_AB:-0}" -gt 0 ]; then
  KINDS="$KINDS seam_idle libcrun_idle"
  with_idle env HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/probe.seam_idle.json" 2 0 0 /opt/bin/hex /run/bundle-sleep \
    /run/bundle-true >> "$OUT/probe.log" 2>&1 || { echo "refusing: the probe with idle threads failed ($OUT/probe.log)" >&2; exit 5; }
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
      seam_idle) with_idle env HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/seam_idle.r$round.json" "$N" "$WARMUP" 0 \
                   /opt/bin/hex /run/bundle-sleep /run/bundle-true >> "$OUT/seam_idle.log" 2>&1 ;;
      libcrun_idle) with_idle env HEXBENCH_PHASES=lifecycle python3 -I /opt/lab/oci_bench.py libcrun \
                      "$OUT/libcrun_idle.r$round.json" "$N" "$WARMUP" 0 0 64 >> "$OUT/libcrun_idle.log" 2>&1 ;;
    esac || echo "round $round: $one FAILED"
  done
done
echo "at the end (inside): $(host_now)" >> "$OUT/env.txt"
python3 -I /opt/lab/probes/create_breakdown.py summarize "$OUT" > "$OUT/summary.md" || echo "summarize FAILED"
echo done > "$OUT/inside.done"
