#!/usr/bin/env bash
# bench/competitive/probes: where creates wait under concurrency, inside the probe container (create_serial.sh).
# MODE=offcpu (the default): off-CPU time by kernel stack (bpftrace on finish_task_switch), each futex call's time by
# the user stack that made it (so the runtime's own locks are named), and an on-CPU profile for a runtime's own
# processes, and each one's directory operations (vfs_mkdir, vfs_rmdir, vfs_unlink, vfs_mknod)
# counted by the filesystem of the directory they change: cgroup2 (a cgroup parent), tmpfs (a state root) or overlay
# (the container's own disk). Three runs, back to back on the same host state:
#   hex.c1    seam_oci's start_true (create, start and delete of `true`, one at a time): the node agent in process
#             (seam_oci) and its seeds and inits before they exec (hex);
#   hex.c32   seam_oci's sustained phase, concurrency 32, the same lifecycle;
#   crun.c32  oci_bench.py's sustained phase on libcrun in process, concurrency 32: its drivers and their inits before
#             they exec (libcrun-driver).
# MODE=locks: what holds the locks those waits are on, and which directory each wait is for. For the runtime's own
# processes: each vfs_mkdir and vfs_rmdir timed by the directory it changes (its parent's i_rwsem is held across
# it); each wait in an rw_semaphore's slow path timed by the directory whose i_rwsem it is (an rwsem that isn't an
# inode's reads as no filesystem); each lookup that missed the dcache (lookup_slow) by its directory and name. For
# every process, the kernel's takers of cgroup_mutex timed whole, their wait for it and their hold together
# (cgroup_mkdir, cgroup_rmdir, cgroup_kill_write, cgroup_bpf_prog_attach, and the work a removed cgroup queues:
# css_killed_work_fn, css_release_work_fn, cgroup_bpf_release), and a fork from cgroup_can_fork to cgroup_post_fork's
# return, by whether it clones into a cgroup (CLONE_INTO_CGROUP holds cgroup_mutex across that span); each wait for
# cgroup_mutex timed alone, from the lock's contention_begin tracepoint to its contention_end, by whether the waiter
# is the runtime's; and each time cgroup_mutex's holder leaves its CPU while it holds the lock, until it runs again,
# by whether it was preempted (still runnable: the switch's prev_state 0 or TASK_REPORT_MAX, Linux v6.19
# include/trace/events/sched.h:202-213, include/linux/sched.h:1661-1662) or blocked, and whether it is the runtime's,
# so a convoy behind a preempted holder is told from the lock's own work. Those takers count every kworker's work on
# the host and the node's own start; so each taker,
# the attach and the fork into a cgroup are also counted for the boxes' own cgroups alone (@box: those directly
# under seam_oci's oci boxes' root, /sys/fs/cgroup/hex-oci-seam/.oci, found from the work's or the call's cgroup).
# And how much of the time each lock is taken, sampled on every CPU at 999 Hz in 100 ms buckets:
# cgroup_mutex (held: an owner in its owner word; handed off: only the waiter and hand-off flags, the lock free
# until a woken waiter runs; free), and the i_rwsem of the cgroup directory the runtime last made a cgroup in
# (write-held, read-held, handed off, free); a bucket in which the runtime made or removed a cgroup is a busy one.
# Five runs: hex.c1, crun.c1 (oci_bench.py's start_true on libcrun), hex.c32, crun.c32, and hex.c32nonet, Hex's
# sustained phase on a `true` bundle without a network namespace of its own. Each run lists the directories under
# /run and /sys/fs/cgroup (each its own filesystem) with their inode numbers (dirs.<label>.txt), so create_serial.py
# names the directory each inode is.
# MODE=checker: where the time of the threads that check a given-back network namespace before its owner's next box
# has it goes (hex-netns-check, crates/hex-node/src/netns.rs; goal 5 extended, step 6): each system call they make
# timed, by its number, and an on-CPU profile of them at 4999 Hz by kernel and user stack and by the system call each
# sample fell in. Two runs, hex.c1 over 5000 lifecycles and hex.c32; HEXBENCH_OWNER must be set, for the namespaces
# are kept only for an owner's boxes.
# MODE=devfs: the same profile for the thread that makes each box's /dev ahead of its create (hex-devfs, step 3's pool,
# crates/hex-node/src/devfs.rs): its system calls timed by number, the futex waits among them, and its on-CPU samples;
# two runs, hex.c1 over 5000 lifecycles and hex.c32, read per /dev it made.
# MODE=oncpu: the same profile for the runtime's own processes, the lifecycles in process (seam_oci) and its seeds and
# inits before they exec (hex): their system calls timed by number, and their on-CPU samples by kernel and user stack
# and by the system call each fell in, so concurrency 32's CPU a lifecycle past concurrency 1's is named by the call
# and the kernel work it went to, a lock's spinning among them. Two runs, hex.c1 over 5000 lifecycles and hex.c32.
# MODE=threads: where each of a runtime's threads spends its CPU, by name: Hex's lifecycles (seam_oci), the node's
# threads (hex-netns-check, hex-netns, hex-seeds, hex-supervisor, hex-cgroup-remover, the socket's), its seeds and
# inits before they exec (hex) and the program (true), sampled on every CPU at 999 Hz, and beside them the other
# tasks on the probe's CPUs by name (CPUSET): what concurrency 1 runs beside its lifecycle's own path. Two runs, hex.c1
# over 5000 lifecycles and hex.c32, one owner's boxes where HEXBENCH_OWNER is set.
# Env: MODE (offcpu), SUSTAINED (5000), HEXBENCH_OWNER (one owner for every box seam_oci makes, so a box's cgroup
# is handed to the owner's next box: goal 5 extended, step 6; the bundles' processes then hold Docker's default
# capabilities, libcrun's alike, since a box that keeps all of root's never shares a cgroup).
set -uo pipefail
SUSTAINED=${SUSTAINED:-5000}; MODE=${MODE:-offcpu}
# The profiles' sampling rates: the checker's, devfs's and oncpu's, and threads'. Each program says its rate in its
# output (@profile_hz), which create_serial.py reads, so the rate is stated here alone. Odd rates off the timer tick's
# 1000 Hz, so the samples don't fall in step with the tick's own work: a convention (bpftrace's and perf's examples
# sample at 99, 999 or 4999 Hz for it), not a measure.
PROFILE_HZ=4999; THREADS_HZ=999
# The soft descriptor limit everything here runs with (HEXBENCH_NOFILE): `hard` for the hard limit the container was
# given, as Hex's installer sets a node's (the lead, 2026-11-04), a number for that, unset for the container's own
# (dockerd's 1024), at which a Hex node holds no /proc/net files. Said here and in nofile.txt.
case "${HEXBENCH_NOFILE:-given}" in
  given) ;;
  hard) ulimit -Sn "$(ulimit -Hn)" ;;
  *) ulimit -Sn "$HEXBENCH_NOFILE" ;;
esac
echo "descriptor limit: soft $(ulimit -Sn), hard $(ulimit -Hn) (HEXBENCH_NOFILE=${HEXBENCH_NOFILE:-given})" | tee "/opt/out/nofile.txt"
dnf -y -q install perf bpftrace procps-ng util-linux python3 libseccomp libcap json-c binutils >/run/dnf.log 2>&1 \
  || { echo "dnf failed"; tail -5 /run/dnf.log; exit 3; }
echo "tools: $(perf --version 2>&1 | head -1); $(bpftrace --version 2>&1 | head -1)"
mountpoint -q /sys/kernel/tracing || mount -t tracefs nodev /sys/kernel/tracing
mountpoint -q /sys/kernel/debug || mount -t debugfs nodev /sys/kernel/debug
CG=/sys/fs/cgroup; mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done
echo "+cpu +io +memory +pids" > "$CG/cgroup.subtree_control" 2>/dev/null || true
mkdir -p "$CG/w"; echo "$BASHPID" > "$CG/w/cgroup.procs"; mkdir -p /run/err
# The OCI pass's bundles (oci_inside.sh's shape): a `sleep` init and a `true` one, with every namespace of their own.
CAPS='["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FSETID","CAP_FOWNER","CAP_MKNOD","CAP_NET_RAW","CAP_SETGID","CAP_SETUID","CAP_SETFCAP","CAP_SETPCAP","CAP_NET_BIND_SERVICE","CAP_SYS_CHROOT","CAP_KILL","CAP_AUDIT_WRITE"]'
PROC_CAPS=""; [ -n "${HEXBENCH_OWNER:-}" ] && PROC_CAPS=",\"capabilities\":{\"bounding\":$CAPS,\"effective\":$CAPS,\"permitted\":$CAPS}"
RO_PATHS=""; [ -n "${HEXBENCH_OWNER:-}" ] && RO_PATHS=',"readonlyPaths":["/proc/sys"]'
B=/run/b; mkdir -p "$B/rootfs"; ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin && mkdir -p usr proc dev tmp )
mk(){ mkdir -p "$1/rootfs"; cp -r "$B/rootfs/." "$1/rootfs/"; cat > "$1/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs"},"hostname":"b","process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}$PROC_CAPS},"mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},{"destination":"/proc","type":"proc","source":"proc"},{"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],"linux":{"namespaces":[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]$RO_PATHS} }
JSON
}
mk /run/bundle-sleep '["/usr/bin/sleep","infinity"]'; mk /run/bundle-true '["/usr/bin/true"]'
mk /run/bundle-true-nonet '["/usr/bin/true"]'
sed -i 's/,{"type":"network"}//' /run/bundle-true-nonet/config.json
offcpu(){ # offcpu <filter on a comm value $c>: the off-CPU program for one runtime's processes
  local f=$1
  cat <<BT
config = { max_map_keys = 262144 }
kprobe:finish_task_switch*
{
	\$prev = (struct task_struct *)arg0;
	\$c = \$prev->comm;
	if ($f) { @start[(uint64)\$prev->pid] = nsecs; }
	\$last = @start[(uint64)curtask->pid];
	if (\$last != 0) {
		@off[kstack(12), comm] = sum(nsecs - \$last);
		delete(@start[(uint64)curtask->pid]);
	}
}
kprobe:vfs_mkdir, kprobe:vfs_rmdir, kprobe:vfs_unlink, kprobe:vfs_mknod
{
	\$c = comm;
	if ($f) { @dirops[func, str(((struct inode *)arg1)->i_sb->s_type->name)] = count(); }
}
profile:hz:999 { \$c = comm; if ($f) { @cpu[kstack(10)] = count(); } }
tracepoint:syscalls:sys_enter_futex
{
	\$c = comm;
	if ($f) { @fx[(uint64)curtask->pid] = nsecs; }
}
tracepoint:syscalls:sys_exit_futex
{
	\$t = @fx[(uint64)curtask->pid];
	if (\$t != 0) {
		@futex[ustack(12)] = sum(nsecs - \$t);
		delete(@fx[(uint64)curtask->pid]);
	}
}
END { clear(@start); clear(@fx); }
BT
}
checker(){ # checker <filter on a comm value $c>: the checker program for the threads the filter names
  local f=$1
  # Keyed by the task's own pid, not bpftrace's tid: that is the pid namespace's (get_ns_current_pid_tgid), which
  # fails for every task outside the probe container's, as the system call probes see them all.
  cat <<BT
config = { max_map_keys = 262144 }
tracepoint:raw_syscalls:sys_enter
{
	\$c = comm;
	if ($f) { @s[(uint64)curtask->pid] = nsecs; @n[(uint64)curtask->pid] = args.id; }
}
tracepoint:raw_syscalls:sys_exit
{
	\$t = @s[(uint64)curtask->pid];
	if (\$t != 0) {
		@sys[@n[(uint64)curtask->pid]] = stats(nsecs - \$t);
		@sys_ns = sum(nsecs - \$t);
		delete(@s[(uint64)curtask->pid]);
		delete(@n[(uint64)curtask->pid]);
	}
}
BEGIN { @profile_hz = $PROFILE_HZ; }
profile:hz:$PROFILE_HZ
{
	\$c = comm;
	if ($f) {
		if (@s[(uint64)curtask->pid] != 0) { @on_sys[@n[(uint64)curtask->pid]] = count(); } else { @on_user = count(); }
		@cpu[kstack(16), ustack(10)] = count();
		@samples = count();
	}
}
END { clear(@s); clear(@n); }
BT
}
devfs(){ checker "$1"; } # devfs <filter on a comm value $c>: the checker's program, for the /dev pool's thread
oncpu(){ checker "$1"; } # oncpu <filter on a comm value $c>: the checker's program, for the runtime's own processes
threads(){ # threads <filter on a comm value $c>: each of the filter's threads' samples by name; the rest on the probe's CPUs
  # The idle task told by the task's own pid, not bpftrace's pid: that is the pid namespace's, and fails for every task
  # outside the probe container's (as checker()'s keys say).
  local f=$1 lo=${CPUSET%%-*} hi=${CPUSET##*-}
  cat <<BT
BEGIN { @profile_hz = $THREADS_HZ; }
profile:hz:$THREADS_HZ
{
	\$c = comm;
	if ($f) { @thread[comm] = count(); @ours = count(); }
	else if (cpu >= $lo && cpu <= $hi && curtask->pid != 0) { @beside[comm] = count(); }
	@samples = count();
}
BT
}
locks(){ # locks <filter on a comm value $c>: the locks program for one runtime's processes
  local f=$1
  cat <<BT
config = { max_map_keys = 262144 }
kprobe:vfs_mkdir, kprobe:vfs_rmdir
{
	\$c = comm;
	if ($f) { @dt[(uint64)curtask->pid] = nsecs; @dd[(uint64)curtask->pid] = (uint64)arg1; }
}
kretprobe:vfs_mkdir, kretprobe:vfs_rmdir
{
	\$t = @dt[(uint64)curtask->pid];
	if (\$t != 0) {
		\$i = (struct inode *)@dd[(uint64)curtask->pid];
		if (str(\$i->i_sb->s_type->name) == "cgroup2") { @cgparent = (uint64)\$i; @busy[nsecs / 100000000] = count(); }
		@hold[func, str(\$i->i_sb->s_type->name), \$i->i_ino] = stats(nsecs - \$t);
		delete(@dt[(uint64)curtask->pid]);
		delete(@dd[(uint64)curtask->pid]);
	}
}
kprobe:rwsem_down_write_slowpath, kprobe:rwsem_down_read_slowpath
{
	\$c = comm;
	if ($f) { @rt[(uint64)curtask->pid] = nsecs; @rs[(uint64)curtask->pid] = (uint64)arg0; }
}
kretprobe:rwsem_down_write_slowpath, kretprobe:rwsem_down_read_slowpath
{
	\$t = @rt[(uint64)curtask->pid];
	if (\$t != 0) {
		\$i = (struct inode *)(@rs[(uint64)curtask->pid] - offsetof(struct inode, i_rwsem));
		@rw[func, str(\$i->i_sb->s_type->name), \$i->i_ino] = stats(nsecs - \$t);
		delete(@rt[(uint64)curtask->pid]);
		delete(@rs[(uint64)curtask->pid]);
	}
}
kprobe:lookup_slow
{
	\$c = comm;
	if ($f) {
		\$q = (struct qstr *)arg0;
		\$d = (struct dentry *)arg1;
		@lookups[str(\$d->d_sb->s_type->name), str(\$d->d_name.name), str(\$q->name)] = count();
	}
}
kprobe:cgroup_mkdir, kprobe:cgroup_rmdir, kprobe:cgroup_kill_write, kprobe:cgroup_bpf_prog_attach,
kprobe:css_killed_work_fn, kprobe:css_release_work_fn, kprobe:cgroup_bpf_release
{
	@ft[(uint64)curtask->pid] = nsecs;
}
kretprobe:cgroup_mkdir, kretprobe:cgroup_rmdir, kretprobe:cgroup_kill_write, kretprobe:cgroup_bpf_prog_attach,
kretprobe:css_killed_work_fn, kretprobe:css_release_work_fn, kretprobe:cgroup_bpf_release
{
	\$t = @ft[(uint64)curtask->pid];
	if (\$t != 0) {
		\$c = comm;
		if ($f) { @fn[func, 1] = stats(nsecs - \$t); } else { @fn[func, 0] = stats(nsecs - \$t); }
		delete(@ft[(uint64)curtask->pid]);
	}
}
kprobe:cgroup_mkdir
{
	\$p = (struct kernfs_node *)arg0;
	if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") { @box["cgroup_mkdir"] = count(); }
}
kprobe:cgroup_rmdir
{
	\$p = ((struct kernfs_node *)arg0)->__parent;
	if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") { @box["cgroup_rmdir"] = count(); }
}
kprobe:cgroup_kill_write
{
	\$p = ((struct kernfs_open_file *)arg0)->kn->__parent->__parent;
	if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") { @box["cgroup_kill_write"] = count(); }
}
kprobe:css_killed_work_fn
{
	\$p = ((struct cgroup_subsys_state *)(arg0 - offsetof(struct cgroup_subsys_state, destroy_work)))->cgroup->kn->__parent;
	if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") { @box["css_killed_work_fn"] = count(); }
}
kprobe:css_release_work_fn
{
	\$p = ((struct cgroup_subsys_state *)(arg0 - offsetof(struct cgroup_subsys_state, destroy_work)))->cgroup->kn->__parent;
	if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") { @box["css_release_work_fn"] = count(); }
}
kprobe:cgroup_bpf_release
{
	\$p = ((struct cgroup *)(arg0 - offsetof(struct cgroup, bpf) - offsetof(struct cgroup_bpf, release_work)))->kn->__parent;
	if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") { @box["cgroup_bpf_release"] = count(); }
}
kprobe:cgroup_bpf_prog_attach { @ina[(uint64)curtask->pid] = 1; }
kretprobe:cgroup_get_from_fd
{
	if (@ina[(uint64)curtask->pid] == 1) {
		delete(@ina[(uint64)curtask->pid]);
		if (retval != 0 && (uint64)retval < 0xfffffffffffff000) {
			\$p = ((struct cgroup *)retval)->kn->__parent;
			if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") {
				@box["cgroup_bpf_prog_attach"] = count();
			}
		}
	}
}
kprobe:cgroup_post_fork
{
	\$a = (struct kernel_clone_args *)arg1;
	if ((\$a->flags & 0x200000000) != 0) {
		\$p = \$a->cgrp->kn->__parent;
		if (str(\$p->name) == ".oci" && str(\$p->__parent->name) == "hex-oci-seam") { @box["fork into a cgroup"] = count(); }
	}
}
kprobe:cgroup_can_fork
{
	\$c = comm;
	if ($f) {
		@fk[(uint64)curtask->pid] = nsecs;
		@fi[(uint64)curtask->pid] = (((struct kernel_clone_args *)arg1)->flags & 0x200000000) != 0 ? 1 : 0;
	}
}
kretprobe:cgroup_post_fork
{
	\$t = @fk[(uint64)curtask->pid];
	if (\$t != 0) {
		@fork[@fi[(uint64)curtask->pid]] = stats(nsecs - \$t);
		delete(@fk[(uint64)curtask->pid]);
		delete(@fi[(uint64)curtask->pid]);
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
			if ($f) { @cgm_wait[1] = stats(nsecs - \$t); } else { @cgm_wait[0] = stats(nsecs - \$t); }
			delete(@cw[(uint64)curtask->pid]);
		}
	}
}
tracepoint:sched:sched_switch
{
	\$o = (uint64)((struct mutex *)kaddr("cgroup_mutex"))->owner.counter & 0xfffffffffffffff8;
	if (\$o != 0 && \$o == (uint64)curtask) {
		\$c = comm;
		\$s = args.prev_state;
		@ho[(uint64)args.prev_pid] = nsecs;
		@hk[(uint64)args.prev_pid] = (($f) ? 2 : 0) + ((\$s == 0 || (\$s & 0x100) != 0) ? 1 : 0);
	}
	\$t = @ho[(uint64)args.next_pid];
	if (\$t != 0) {
		@holder_off[@hk[(uint64)args.next_pid]] = stats(nsecs - \$t);
		delete(@ho[(uint64)args.next_pid]);
		delete(@hk[(uint64)args.next_pid]);
	}
}
profile:hz:999
{
	\$o = (uint64)((struct mutex *)kaddr("cgroup_mutex"))->owner.counter;
	@cgm_state[nsecs / 100000000, (\$o & 0xfffffffffffffff8) != 0 ? 2 : (\$o != 0 ? 1 : 0)] = count();
	if (@cgparent != 0) {
		\$n = (uint64)((struct inode *)@cgparent)->i_rwsem.count.counter;
		@dir_state[nsecs / 100000000, (\$n & 1) != 0 ? 3 : (((\$n >> 8) & 0x7fffffffffffff) != 0 ? 2 : (\$n != 0 ? 1 : 0))] = count();
	}
}
END { clear(@dt); clear(@dd); clear(@rt); clear(@rs); clear(@ft); clear(@fk); clear(@fi); clear(@cgparent); clear(@cw); clear(@ina); clear(@ho); clear(@hk); }
BT
}
run(){ # run <label> <filter> <command...>: the mode's program over one workload, and the directories it made
  local label=$1 filter=$2; shift 2
  "$MODE" "$filter" > "/run/$label.bt"
  # The workload waits for the fact that every probe is attached: bpftrace says "Attached N probes" on stderr once
  # both its attach passes are done (bpftrace v0.24.2 src/bpftrace.cpp:569-602, :637; src/output/text.cpp:580-586),
  # not a time (a fixed 5 s let the first ~45% of a run's lifecycles go unprobed under fedora's load, run 7).
  bpftrace "/run/$label.bt" > "/run/bt.$label.txt" 2>"/run/bt.$label.err" &
  local bt=$! i
  for i in $(seq 1 1200); do grep -q '^Attached [0-9]* probe' "/run/bt.$label.err" 2>/dev/null && break; kill -0 $bt 2>/dev/null || break; sleep 0.1; done
  grep -q '^Attached [0-9]* probe' "/run/bt.$label.err" 2>/dev/null \
    || { echo "$label: bpftrace never said its probes were attached (2 min): $(head -c 300 /run/bt.$label.err)"; kill -INT $bt 2>/dev/null; return 1; }
  kill -0 $bt 2>/dev/null || { echo "$label: bpftrace ended early: $(head -c 300 /run/bt.$label.err)"; return 1; }
  ( for i in 1 2 3 4 5 6; do sleep 0.5; find /run /sys/fs/cgroup -xdev -maxdepth 4 -type d -printf '%i %p\n' 2>/dev/null; done \
      | sort -u > "/run/dirs.$label.txt" ) &
  local dirs=$!
  # The bench process's mappings once it's up: bpftrace prints a user stack only after the process is gone, as
  # addresses, which create_serial.py names by these and syms.*.txt.
  ( for i in $(seq 1 600); do p=$(pgrep -x seam_oci | head -1); [ -n "$p" ] && { sleep 1; cat "/proc/$p/maps" > "/run/maps.$label.txt" 2>/dev/null; break; }
      sleep 0.1; done ) &
  local maps=$!
  "$@" > "/run/$label.log" 2>&1 || echo "$label: the workload failed (/run/$label.log)"
  wait $dirs $maps
  kill -INT $bt; wait $bt 2>/dev/null
  echo "$label: $(wc -l < /run/bt.$label.txt) lines"
}
HEX='$c == "seam_oci" || $c == "hex"'; CRUN='$c == "libcrun-driver"'; CHECK='$c == "hex-netns-check"'; DEVFS='$c == "hex-devfs"'; THREADS='$c == "seam_oci" || $c == "hex" || $c == "true" || strncmp($c, "hex-", 4) == 0'
hex_c1(){ env HEXBENCH_PHASES=start_true /opt/bin/seam_oci "/run/out.$MODE.hex.c1.json" 500 20 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true; }
hex_c1k(){ env HEXBENCH_PHASES=start_true /opt/bin/seam_oci "/run/out.$MODE.hex.c1.json" 5000 20 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true; }
hex_c32(){ env HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED="$SUSTAINED" /opt/bin/seam_oci "/run/out.$MODE.hex.c32.json" 20 0 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true; }
hex_c32nonet(){ env HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED="$SUSTAINED" /opt/bin/seam_oci "/run/out.$MODE.hex.c32nonet.json" 20 0 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true-nonet; }
crun_c1(){ env HEXBENCH_PHASES=start_true HEXBENCH_BIN=/opt/bin python3 -I /opt/lab/oci_bench.py libcrun "/run/out.$MODE.crun.c1.json" 500 20 0 0 64; }
crun_c32(){ env HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED="$SUSTAINED" HEXBENCH_BIN=/opt/bin python3 -I /opt/lab/oci_bench.py libcrun "/run/out.$MODE.crun.c32.json" 20 0 0 0 64; }
case $MODE in
  offcpu) run offcpu.hex.c1 "$HEX" hex_c1; run offcpu.hex.c32 "$HEX" hex_c32; run offcpu.crun.c32 "$CRUN" crun_c32 ;;
  locks) run locks.hex.c1 "$HEX" hex_c1; run locks.crun.c1 "$CRUN" crun_c1; run locks.hex.c32 "$HEX" hex_c32; run locks.crun.c32 "$CRUN" crun_c32
    run locks.hex.c32nonet "$HEX" hex_c32nonet ;;
  checker) [ -n "${HEXBENCH_OWNER:-}" ] || { echo "refusing: MODE=checker needs HEXBENCH_OWNER"; exit 2; }
    run checker.hex.c1 "$CHECK" hex_c1k; run checker.hex.c32 "$CHECK" hex_c32 ;;
  devfs) run devfs.hex.c1 "$DEVFS" hex_c1k; run devfs.hex.c32 "$DEVFS" hex_c32 ;;
  oncpu) run oncpu.hex.c1 "$HEX" hex_c1k; run oncpu.hex.c32 "$HEX" hex_c32 ;;
  threads) case ${CPUSET:-} in [0-9]*-[0-9]*) ;; *) echo "refusing: MODE=threads needs CPUSET as from-to, not ${CPUSET:-none}"; exit 2 ;; esac
    run threads.hex.c1 "$THREADS" hex_c1k; run threads.hex.c32 "$THREADS" hex_c32 ;;
  *) echo "refusing: MODE is offcpu, locks, checker, devfs, oncpu or threads, not $MODE"; exit 2 ;;
esac
nm -C -n --defined-only /opt/bin/seam_oci > /run/syms.seam_oci.txt 2>/dev/null
nm -D -C -n --defined-only "$(readlink -f /usr/lib64/libc.so.6)" > /run/syms.libc.txt 2>/dev/null
for f in /run/out.*.json; do python3 -I -c "
import json,sys; d=json.load(open('$f')); ph=d.get('phases',{})
for k,v in ph.items():
  if k=='sustained': print('$f', k, '%.0f/s'%v['per_s'], 'n=%d'%v['lifecycles'])
  elif k=='start_true': print('$f', k, 'n=%d'%len(v['ms']) if isinstance(v.get('ms'),list) else v)
print('$f errors', d.get('errors'))"; done
echo DONE
