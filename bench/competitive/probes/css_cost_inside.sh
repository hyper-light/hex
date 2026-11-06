#!/usr/bin/env bash
# bench/competitive/probes: what each controller's teardown of a box's cgroup costs, inside the bench container
# (css_cost.sh starts it with host.sh's OCI-pass flags). One ftrace instance of this run's own records every
# kworker's work items (workqueue_execute_start/end, by function) and, by kprobe and kretprobe, each controller's
# css callbacks a removed cgroup runs (Linux v6.19: memory's mem_cgroup_css_offline/_released/_free,
# mm/memcontrol.c:4685-4687; io's blkcg_css_offline/_free, block/blk-cgroup.c:1544-1545; cpu's
# cpu_cgroup_css_offline/_released/_free, kernel/sched/core.c:10184-10186; pids' pids_css_free,
# kernel/cgroup/pids.c:451), while seam_oci's lifecycle phase makes and deletes N boxes of no owner.
# css_cost.py reads the trace. Env: N (2000), TRACE_TAG.
set -uo pipefail
N=${N:-2000}; OUT=/opt/out; TAG=${TRACE_TAG:?TRACE_TAG}
T=/sys/kernel/tracing
mountpoint -q "$T" || mount -t tracefs nodev "$T" || { echo "refusing: no tracefs at $T" >&2; exit 4; }
I="$T/instances/hexcss-$TAG"; KG="hexcss_$TAG"
FNS="mem_cgroup_css_offline mem_cgroup_css_released mem_cgroup_css_free blkcg_css_offline blkcg_css_free
  cpu_cgroup_css_offline cpu_cgroup_css_released cpu_cgroup_css_free pids_css_free"
untrace() {
  [ -d "$I" ] && { echo 0 > "$I/tracing_on"; for ev in workqueue/workqueue_execute_start workqueue/workqueue_execute_end; do
    echo 0 > "$I/events/$ev/enable" 2>/dev/null; done
    for f in $FNS; do echo 0 > "$I/events/$KG/e_$f/enable" 2>/dev/null; echo 0 > "$I/events/$KG/x_$f/enable" 2>/dev/null; done
    rmdir "$I" 2>/dev/null; }
  for f in $FNS; do echo "-:$KG/e_$f" >> "$T/kprobe_events" 2>/dev/null; echo "-:$KG/x_$f" >> "$T/kprobe_events" 2>/dev/null; done
}
trap untrace EXIT
[ "${UNTRACE_ONLY:-0}" = 1 ] && exit 0
ALL_NS='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
B=/run/bundle-sleep; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":["/usr/bin/sleep","infinity"],"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":$ALL_NS} }
JSON
cp -r /run/bundle-sleep /run/bundle-true; sed -i 's|"/usr/bin/sleep","infinity"|"/usr/bin/true"|' /run/bundle-true/config.json
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done
mkdir -p "$HOME/.cache/hex-t7/ociseam"
mkdir "$I" || { echo "refusing: no trace instance $I" >&2; exit 4; }
echo 0 > "$I/tracing_on"; echo 16384 > "$I/buffer_size_kb"
for f in $FNS; do
  { echo "p:$KG/e_$f $f" >> "$T/kprobe_events" && echo "r:$KG/x_$f $f" >> "$T/kprobe_events"; } \
    || { echo "refusing: no kprobe on $f" >&2; exit 4; }
  echo 1 > "$I/events/$KG/e_$f/enable"; echo 1 > "$I/events/$KG/x_$f/enable"
done
for ev in workqueue/workqueue_execute_start workqueue/workqueue_execute_end; do echo 1 > "$I/events/$ev/enable"; done
echo "inside: kernel $(uname -r), load $(cat /proc/loadavg), controllers $(cat /sys/fs/cgroup/cgroup.subtree_control)" >> "$OUT/env.txt"
echo 1 > "$I/tracing_on"
HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/seam.json" "$N" 20 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true > "$OUT/seam.log" 2>&1 \
  || echo "seam_oci failed: $(tail -c 300 "$OUT/seam.log")"
echo 0 > "$I/tracing_on"
cat "$I/trace" > "$OUT/trace.txt"
echo "dropped: $(cat "$I"/per_cpu/cpu*/stats 2>/dev/null | awk '/^overrun:/{s+=$2} END{print s+0}')" >> "$OUT/env.txt"
echo DONE
