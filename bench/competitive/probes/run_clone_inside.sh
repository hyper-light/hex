#!/usr/bin/env bash
# bench/competitive/probes: inside run_clone.sh's container. seam_box's runs of `true` at concurrency 1 (its latency
# phase, N runs) and 32 (its throughput phase at 32, LIFECYCLES runs), each under one bpftrace program: the runtime's
# clones from cgroup_can_fork to cgroup_post_fork's return, by whether each clones into a cgroup (CLONE_INTO_CGROUP,
# 0x200000000, holds cgroup_mutex across that span); the runtime's cgroup_mkdir, cgroup_rmdir and cgroup_kill_write
# timed whole (each takes cgroup_mutex: its wait and its hold together); and each wait for cgroup_mutex alone, from
# lock:contention_begin to contention_end, by whether the waiter is the runtime's. The runtime is seam_box's threads
# and its node agent's supervisor (hex-supervisor).
set -uo pipefail
dnf -y -q install bpftrace procps-ng python3 >/run/dnf.log 2>&1 || { echo "dnf failed"; tail -5 /run/dnf.log; exit 3; }
echo "tools: $(bpftrace --version 2>&1 | head -1)"
mountpoint -q /sys/kernel/tracing || mount -t tracefs nodev /sys/kernel/tracing
mountpoint -q /sys/kernel/debug || mount -t debugfs nodev /sys/kernel/debug
CG=/sys/fs/cgroup; mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done
echo "+cpu +io +memory +pids" > "$CG/cgroup.subtree_control"
F='$c == "seam_box" || $c == "hex-supervisor"'
cat > /run/clone.bt <<BT
config = { max_map_keys = 262144 }
kprobe:cgroup_can_fork
{
	\$c = comm;
	if ($F) {
		@fk[(uint64)curtask->pid] = nsecs;
		@fi[(uint64)curtask->pid] = (((struct kernel_clone_args *)arg1)->flags & 0x200000000) != 0 ? 1 : 0;
	}
}
kretprobe:cgroup_post_fork
{
	\$t = @fk[(uint64)curtask->pid];
	if (\$t != 0) {
		@clone[@fi[(uint64)curtask->pid]] = stats(nsecs - \$t);
		delete(@fk[(uint64)curtask->pid]);
		delete(@fi[(uint64)curtask->pid]);
	}
}
kprobe:cgroup_mkdir, kprobe:cgroup_rmdir, kprobe:cgroup_kill_write
{
	\$c = comm;
	if ($F) { @tk[(uint64)curtask->pid] = nsecs; }
}
kretprobe:cgroup_mkdir, kretprobe:cgroup_rmdir, kretprobe:cgroup_kill_write
{
	\$t = @tk[(uint64)curtask->pid];
	if (\$t != 0) {
		@taker[probe] = stats(nsecs - \$t);
		delete(@tk[(uint64)curtask->pid]);
	}
}
tracepoint:lock:contention_begin
{
	if ((uint64)args.lock_addr == kaddr("cgroup_mutex") && @cw[(uint64)curtask->pid] == 0) {
		@cw[(uint64)curtask->pid] = nsecs;
	}
}
tracepoint:lock:contention_end
{
	if ((uint64)args.lock_addr == kaddr("cgroup_mutex")) {
		\$t = @cw[(uint64)curtask->pid];
		if (\$t != 0) {
			\$c = comm;
			if ($F) { @cgm_wait[1] = stats(nsecs - \$t); } else { @cgm_wait[0] = stats(nsecs - \$t); }
			delete(@cw[(uint64)curtask->pid]);
		}
	}
}
END { clear(@fk); clear(@fi); clear(@tk); clear(@cw); }
BT
run(){ # run <label> <env...>: seam_box under the program, in a cgroup of its own it can hand controllers down from
  local label=$1; shift
  bpftrace /run/clone.bt > "/opt/out/bt.$label.txt" 2>"/opt/out/bt.$label.err" &
  local bt=$! i
  # The workload waits for the fact that every probe is attached, as create_serial_inside.sh's does.
  for i in $(seq 1 1200); do grep -q '^Attached [0-9]* probe' "/opt/out/bt.$label.err" 2>/dev/null && break; kill -0 $bt 2>/dev/null || break; sleep 0.1; done
  grep -q '^Attached [0-9]* probe' "/opt/out/bt.$label.err" \
    || { echo "$label: bpftrace never said its probes were attached: $(head -c 300 "/opt/out/bt.$label.err")"; kill -INT $bt 2>/dev/null; return 1; }
  mkdir -p "$CG/$label"
  ( echo "$BASHPID" > "$CG/$label/cgroup.procs" && exec env "$@" /opt/bin/seam_box "/opt/out/out.$label.json" \
      "${N:-2000}" 20 "${LIFECYCLES:-20000}" 0 0 ) > "/opt/out/$label.log" 2>&1 || echo "$label: the workload failed ($label.log)"
  kill -INT $bt; wait $bt 2>/dev/null
  echo "$label: $(wc -l < "/opt/out/bt.$label.txt") lines; $(tail -3 "/opt/out/$label.log" | tr '\n' ' ')"
}
run c1 HEXBENCH_PHASES=latency
run c32 HEXBENCH_PHASES=throughput HEXBENCH_CONCURRENCY=32
echo DONE
