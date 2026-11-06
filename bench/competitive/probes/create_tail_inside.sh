#!/usr/bin/env bash
# bench/competitive/probes: what the seam's slowest creates and starts wait on, inside the bench container
# (create_tail.sh starts it with host.sh's OCI-pass flags). One ftrace instance of this run's own records, for the
# runtime's processes (seam_oci, its threads hex-*, the seeds and inits hex, the bundles' programs sleep and true), every switch in and out (with the
# kernel stack of each switch out that blocks), every wake-up and fork; seam_oci's breakdown markers bracket every
# other timed create and start of its lifecycle phase (HEXBENCH_TRACE pointed at a directory whose trace_marker is
# the instance's and whose tracing_on is a plain file, so the instance records throughout). A reader container on
# CPUs this one doesn't use (create_tail.sh) streams the instance's trace_pipe to $OUT/trace.txt, and
# create_tail.py walks each bracketed call's wake-up chain back from its end.
# Env: N (10000), WARMUP (100), ROUNDS (1), TRACE_TAG, HEXBENCH_OWNER (one owner's boxes: step 6's reuse on).
set -uo pipefail
N=${N:-10000}; WARMUP=${WARMUP:-100}; ROUNDS=${ROUNDS:-1}
OUT=/opt/out; TAG=${TRACE_TAG:?TRACE_TAG}
T=/sys/kernel/tracing
mountpoint -q "$T" || mount -t tracefs nodev "$T" || { echo "refusing: no tracefs at $T" >&2; exit 4; }
I="$T/instances/hextail-$TAG"
KG="hextail_$TAG"  # this run's kprobes: rcu_sync_enter's entry (rse) and return (rsx), every task's
CGEV="cgroup/cgroup_attach_task cgroup/cgroup_mkdir cgroup/cgroup_rmdir cgroup/cgroup_release"
# Whose cgroup each teardown work item takes apart: an fprobe on each controller's offline and release callback, every
# task's, with the id of the cgroup its css is of (BTF member access; offline_css itself is inlined, with no BTF entry,
# on fedora's 6.19). create_tail.py joins the id to cgroup_rmdir's path (Linux v6.19 mm/memcontrol.c, kernel/sched/
# core.c, block/blk-cgroup.c: the callbacks css_killed_work_fn and css_release_work_fn run).
CSSF="o_mem:mem_cgroup_css_offline o_cpu:cpu_cgroup_css_offline o_blk:blkcg_css_offline r_mem:mem_cgroup_css_released r_cpu:cpu_cgroup_css_released"

# The OCI pass's bundles, as create_breakdown_inside.sh makes them (an owner's: Docker's default capabilities and
# /proc/sys read-only, so a box's cgroup and network namespace may be handed to the owner's next box).
ALL_NS='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
CAPS='["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FSETID","CAP_FOWNER","CAP_MKNOD","CAP_NET_RAW","CAP_SETGID","CAP_SETUID","CAP_SETFCAP","CAP_SETPCAP","CAP_NET_BIND_SERVICE","CAP_SYS_CHROOT","CAP_KILL","CAP_AUDIT_WRITE"]'
PROC_CAPS=""; [ -n "${HEXBENCH_OWNER:-}${HEXBENCH_DOCKER_CAPS:-}" ] && PROC_CAPS=",\"capabilities\":{\"bounding\":$CAPS,\"effective\":$CAPS,\"permitted\":$CAPS}"
RO_PATHS=""; [ -n "${HEXBENCH_OWNER:-}" ] && RO_PATHS=',"readonlyPaths":["/proc/sys"]'
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

# cgroup v2's no-internal-process rule, as oci_inside.sh: everything to a leaf so the root parents containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done
mkdir -p "$HOME/.cache/hex-t7/ociseam"

untrace() {
  [ -d "$I" ] || return 0
  echo 0 > "$I/tracing_on" 2>/dev/null
  echo '!stacktrace' > "$I/events/sched/sched_switch/trigger" 2>/dev/null
  for ev in $CGEV; do echo '!stacktrace' > "$I/events/$ev/trigger" 2>/dev/null; done
  for ev in rse rsx; do echo 0 > "$I/events/$KG/$ev/enable" 2>/dev/null; done
  for e in $CSSF; do echo 0 > "$I/events/$KG/${e%%:*}/enable" 2>/dev/null; done
  for ev in sched/sched_switch sched/sched_waking sched/sched_wakeup_new sched/sched_process_fork \
            sched/sched_process_exit irq/softirq_entry irq/softirq_exit irq/irq_handler_entry irq/irq_handler_exit \
            workqueue/workqueue_execute_start workqueue/workqueue_execute_end cgroup/cgroup_attach_task \
            cgroup/cgroup_mkdir cgroup/cgroup_rmdir cgroup/cgroup_release; do
    echo 0 > "$I/events/$ev/enable" 2>/dev/null
  done
  rmdir "$I" 2>/dev/null || echo "the trace instance $I wouldn't go" >&2
  for ev in rse rsx; do echo "-:$KG/$ev" >> "$T/kprobe_events" 2>/dev/null; done
  for e in $CSSF; do echo "-:$KG/${e%%:*}" >> "$T/dynamic_events" 2>/dev/null; done
}
trap untrace EXIT
[ "${UNTRACE_ONLY:-0}" = 1 ] && exit 0

mkdir "$I" || { echo "refusing: no trace instance $I" >&2; exit 4; }
echo 0 > "$I/tracing_on"
# 8,192 KiB a CPU: the reader container streams the instance as it records, and at this size no event was lost in
# create_tail.md's runs 1, 2 and 5 (create-tail-20261105T032219Z, 3,208,594 lines; 20261105T035431Z, 4,784,275;
# 20261105T112335Z, N=10,000), each "none dropped". A run that loses any says how many in env.txt's dropped line; the
# reader's handshake makes a rerun at a larger size a new run, not a retry here (tracebuf.sh's way).
echo 8192 > "$I/buffer_size_kb"
OURS='comm == "seam_oci" || comm == "sleep" || comm == "true" || comm ~ "hex*"'
echo "prev_comm == \"seam_oci\" || prev_comm == \"sleep\" || prev_comm == \"true\" || prev_comm ~ \"hex*\" || next_comm == \"seam_oci\" || next_comm == \"sleep\" || next_comm == \"true\" || next_comm ~ \"hex*\"" \
  > "$I/events/sched/sched_switch/filter"
echo "$OURS" > "$I/events/sched/sched_waking/filter"
echo "$OURS" > "$I/events/sched/sched_wakeup_new/filter"
echo 'comm == "seam_oci" || comm == "sleep" || comm == "true" || comm ~ "hex*" || child_comm == "seam_oci" || child_comm ~ "hex*"' \
  > "$I/events/sched/sched_process_fork/filter" 2>/dev/null \
  || echo "$OURS" > "$I/events/sched/sched_process_fork/filter"
echo "$OURS" > "$I/events/sched/sched_process_exit/filter"
# A blocking switch out's kernel stack: what the task waits in (a preempted one, state R, has none).
echo 'stacktrace if (prev_comm == "seam_oci" || prev_comm == "sleep" || prev_comm == "true" || prev_comm ~ "hex*") && prev_state & 3' \
  > "$I/events/sched/sched_switch/trigger" || { echo "refusing: the stacktrace trigger wasn't taken" >&2; exit 4; }
for ev in sched_switch sched_waking sched_wakeup_new sched_process_fork sched_process_exit; do
  echo 1 > "$I/events/sched/$ev/enable" || { echo "refusing: sched/$ev wasn't enabled" >&2; exit 4; }
done
# Every CPU's interrupt work and kworkers' work items, unfiltered: what a run's time hides, and what preempts it;
# and every task's cgroup operations with their stacks, and its rcu_sync_enter's entry and return, so a waker that
# held a cgroup lock is named by what it did under it and whether it waited out a grace period there
# (percpu_down_write's rcu_sync_enter, kernel/locking/percpu-rwsem.c:235, Linux v6.19).
{ echo "p:$KG/rse rcu_sync_enter" >> "$T/kprobe_events" && echo "r:$KG/rsx rcu_sync_enter" >> "$T/kprobe_events"; } \
  || { echo "refusing: rcu_sync_enter's kprobes weren't taken" >&2; exit 4; }
for ev in $CGEV; do echo stacktrace > "$I/events/$ev/trigger" || { echo "refusing: $ev's stacktrace trigger" >&2; exit 4; }; done
CSSEV=""
for e in $CSSF; do
  if echo "f:$KG/${e%%:*} ${e#*:} cg=css->cgroup->kn->id:u64" >> "$T/dynamic_events" 2>/dev/null; then CSSEV="$CSSEV $KG/${e%%:*}"
  else echo "no fprobe on ${e#*:} on this kernel: its work items are named by their function alone" >> "$OUT/env.txt"; fi
done
for ev in irq/softirq_entry irq/softirq_exit irq/irq_handler_entry irq/irq_handler_exit \
          workqueue/workqueue_execute_start workqueue/workqueue_execute_end \
          $CGEV $KG/rse $KG/rsx $CSSEV; do
  echo 1 > "$I/events/$ev/enable" || { echo "refusing: $ev wasn't enabled" >&2; exit 4; }
done
# seam_oci's markers into the instance, with tracing_on a plain file so its toggles leave the instance on.
mkdir -p /run/tailmark; : > /run/tailmark/tracing_on; ln -sfn "$I/trace_marker" /run/tailmark/trace_marker

{ echo "inside: date $(date -u +%FT%TZ), kernel $(uname -r), load $(cat /proc/loadavg)"
  echo "inside: cpus allowed $(sed -n 's/^Cpus_allowed_list:\t*//p' /proc/self/status)"
  echo "inside: the cgroup root's subtree_control: $(cat /sys/fs/cgroup/cgroup.subtree_control)"
  echo "params: N=$N WARMUP=$WARMUP ROUNDS=$ROUNDS owner=${HEXBENCH_OWNER:-none} parked_procs=${HEXBENCH_PARKED_PROCS:-unset} docker_caps=${HEXBENCH_DOCKER_CAPS:-unset} box_memory=${HEXBENCH_BOX_MEMORY:-256m}"
} >> "$OUT/env.txt"

# The reader container (create_tail.sh) waits for this file, then streams the instance; this waits for its word.
echo "$I" > "$OUT/instance"
for i in $(seq 1 600); do [ -f "$OUT/reading" ] && break; sleep 0.1; done
[ -f "$OUT/reading" ] || { echo "refusing: the reader never said it was reading (60 s)" >&2; exit 5; }
echo 1 > "$I/tracing_on"
for round in $(seq 1 "$ROUNDS"); do
  echo "round $round: load $(cut -d' ' -f1-3 /proc/loadavg)"
  echo "hexbd R $round" > "$I/trace_marker"
  HEXBENCH_TRACE=/run/tailmark HEXBENCH_TRACE_OPS="create start" HEXBENCH_PHASES=lifecycle \
    /opt/bin/seam_oci "$OUT/seam.r$round.json" "$N" "$WARMUP" 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true \
    >> "$OUT/seam.log" 2>&1 || echo "round $round: seam_oci failed ($(tail -c 300 "$OUT/seam.log"))"
done
# The reader ends at this marker, so it goes in before the instance stops recording.
echo "hexbd D" > "$I/trace_marker" 2>/dev/null
echo 0 > "$I/tracing_on"
echo "dropped (per cpu, from per_cpu/*/stats): $(cat "$I"/per_cpu/cpu*/stats 2>/dev/null | awk '/^overrun:/{s+=$2} /^dropped events:/{s+=$3} END{print s+0}')" >> "$OUT/env.txt"
# The reader ends on "hexbd D"; give it the fact that the instance is done, then wait for it to say so.
for i in $(seq 1 1200); do [ -f "$OUT/read" ] && break; sleep 0.1; done
[ -f "$OUT/read" ] || echo "the reader never said it was done (120 s)" >&2
echo DONE
