#!/usr/bin/env bash
# bench/competitive/probes: where creates wait under concurrency, inside the probe container (create_serial.sh).
# MODE=offcpu (the default): off-CPU time by kernel stack (bpftrace on finish_task_switch) and an on-CPU profile for
# a runtime's own processes, and each one's directory operations (vfs_mkdir, vfs_rmdir, vfs_unlink, vfs_mknod)
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
# is the runtime's. Those takers count every kworker's work on the host and the node's own start; so each taker,
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
# Env: MODE (offcpu), SUSTAINED (5000), HEXBENCH_OWNER (one owner for every box seam_oci makes, so a box's cgroup
# is handed to the owner's next box: goal 5 extended, step 6; the bundles' processes then hold Docker's default
# capabilities, libcrun's alike, since a box that keeps all of root's never shares a cgroup).
set -uo pipefail
SUSTAINED=${SUSTAINED:-5000}; MODE=${MODE:-offcpu}
dnf -y -q install perf bpftrace procps-ng util-linux python3 libseccomp libcap json-c >/run/dnf.log 2>&1 \
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
END { clear(@start); }
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
profile:hz:999
{
	\$o = (uint64)((struct mutex *)kaddr("cgroup_mutex"))->owner.counter;
	@cgm_state[nsecs / 100000000, (\$o & 0xfffffffffffffff8) != 0 ? 2 : (\$o != 0 ? 1 : 0)] = count();
	if (@cgparent != 0) {
		\$n = (uint64)((struct inode *)@cgparent)->i_rwsem.count.counter;
		@dir_state[nsecs / 100000000, (\$n & 1) != 0 ? 3 : (((\$n >> 8) & 0x7fffffffffffff) != 0 ? 2 : (\$n != 0 ? 1 : 0))] = count();
	}
}
END { clear(@dt); clear(@dd); clear(@rt); clear(@rs); clear(@ft); clear(@fk); clear(@fi); clear(@cgparent); clear(@cw); clear(@ina); }
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
  "$@" > "/run/$label.log" 2>&1 || echo "$label: the workload failed (/run/$label.log)"
  wait $dirs
  kill -INT $bt; wait $bt 2>/dev/null
  echo "$label: $(wc -l < /run/bt.$label.txt) lines"
}
HEX='$c == "seam_oci" || $c == "hex"'; CRUN='$c == "libcrun-driver"'
hex_c1(){ env HEXBENCH_PHASES=start_true /opt/bin/seam_oci "/run/out.$MODE.hex.c1.json" 500 20 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true; }
hex_c32(){ env HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED="$SUSTAINED" /opt/bin/seam_oci "/run/out.$MODE.hex.c32.json" 20 0 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true; }
hex_c32nonet(){ env HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED="$SUSTAINED" /opt/bin/seam_oci "/run/out.$MODE.hex.c32nonet.json" 20 0 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true-nonet; }
crun_c1(){ env HEXBENCH_PHASES=start_true HEXBENCH_BIN=/opt/bin python3 -I /opt/lab/oci_bench.py libcrun "/run/out.$MODE.crun.c1.json" 500 20 0 0 64; }
crun_c32(){ env HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED="$SUSTAINED" HEXBENCH_BIN=/opt/bin python3 -I /opt/lab/oci_bench.py libcrun "/run/out.$MODE.crun.c32.json" 20 0 0 0 64; }
case $MODE in
  offcpu) run offcpu.hex.c1 "$HEX" hex_c1; run offcpu.hex.c32 "$HEX" hex_c32; run offcpu.crun.c32 "$CRUN" crun_c32 ;;
  locks) run locks.hex.c1 "$HEX" hex_c1; run locks.crun.c1 "$CRUN" crun_c1; run locks.hex.c32 "$HEX" hex_c32; run locks.crun.c32 "$CRUN" crun_c32
    run locks.hex.c32nonet "$HEX" hex_c32nonet ;;
  *) echo "refusing: MODE is offcpu or locks, not $MODE"; exit 2 ;;
esac
for f in /run/out.*.json; do python3 -I -c "
import json,sys; d=json.load(open('$f')); ph=d.get('phases',{})
for k,v in ph.items():
  if k=='sustained': print('$f', k, '%.0f/s'%v['per_s'], 'n=%d'%v['lifecycles'])
  elif k=='start_true': print('$f', k, 'n=%d'%len(v['ms']) if isinstance(v.get('ms'),list) else v)
print('$f errors', d.get('errors'))"; done
echo DONE
