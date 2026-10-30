#!/usr/bin/env bash
# Where do the seam's creates serialize at concurrency 32? Off-CPU and on-CPU time by kernel stack for the
# bench's threads (seam_oci: the node agent in process; hex: the seeds and the inits before they exec),
# at concurrency 1 (start_true, sequential) and 32 (sustained), in a privileged container like the pass's.
set -uo pipefail
dnf -y -q install perf bpftrace procps-ng util-linux >/run/dnf.log 2>&1 || { echo "dnf failed"; tail -5 /run/dnf.log; exit 3; }
echo "tools: $(perf --version 2>&1 | head -1); $(bpftrace --version 2>&1 | head -1)"
mountpoint -q /sys/kernel/tracing || mount -t tracefs nodev /sys/kernel/tracing
mountpoint -q /sys/kernel/debug || mount -t debugfs nodev /sys/kernel/debug
CG=/sys/fs/cgroup; mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done
echo "+cpu +io +memory +pids" > "$CG/cgroup.subtree_control" 2>/dev/null || true
mkdir -p "$CG/w"; echo "$BASHPID" > "$CG/w/cgroup.procs"
B=/run/b; mkdir -p "$B/rootfs"; ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin && mkdir -p usr proc dev tmp )
mk(){ mkdir -p "$1/rootfs"; cp -r "$B/rootfs/." "$1/rootfs/"; cat > "$1/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs"},"process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin"],"user":{"uid":0,"gid":0}},"mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},{"destination":"/proc","type":"proc","source":"proc"},{"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],"linux":{"namespaces":[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]} }
JSON
}
mk /run/sleep '["/usr/bin/sleep","infinity"]'; mk /run/true '["/usr/bin/true"]'
cat > /run/offcpu.bt <<'BT'
kprobe:finish_task_switch*
{
	$prev = (struct task_struct *)arg0;
	$pc = $prev->comm;
	if ($pc == "seam_oci" || $pc == "hex") { @start[(uint64)$prev->pid] = nsecs; }
	$last = @start[(uint64)curtask->pid];
	if ($last != 0) {
		@off[kstack(12), comm] = sum(nsecs - $last);
		delete(@start[(uint64)curtask->pid]);
	}
}
profile:hz:999 /comm == "seam_oci" || comm == "hex"/ { @cpu[kstack(10)] = count(); }
END { clear(@start); print(@off, 40); print(@cpu, 25); }
BT
run(){ # run <label> <phases> [env]
  local label=$1 phases=$2; shift 2
  bpftrace /run/offcpu.bt > /run/bt.$label.txt 2>/run/bt.$label.err &
  local bt=$!; sleep 4
  env HEXBENCH_PHASES="$phases" "$@" /opt/bin/seam_oci /run/out.$label.json 500 20 600 /opt/bin/hex /run/sleep /run/true > /run/seam.$label.log 2>&1
  kill -INT $bt; wait $bt 2>/dev/null
  python3 -I -c "
import json; d=json.load(open('/run/out.$label.json')); ph=d['phases']
for k,v in ph.items():
  if k=='sustained': print('$label', k, '%.0f/s'%v['per_s'], 'n=%d'%v['lifecycles'])
  elif k=='start_true': print('$label', k, 'n=%d'%len(v['ms']))
  elif k=='throughput': print('$label', k, {c:round(x['per_s']) for c,x in v.items()})
print('errors', d['errors'])"
}
run c1 start_true
run c32 sustained HEXBENCH_SUSTAINED=5000
echo DONE
