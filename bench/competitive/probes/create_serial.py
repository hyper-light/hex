#!/usr/bin/env python3
"""bench/competitive/probes: create_serial.sh's reader, each figure per lifecycle of its run.

A MODE=offcpu run: bpftrace's off-CPU map (@off[kstack, comm]: ns), its on-CPU profile (@cpu[kstack]: samples) and
its directory operations (@dirops[func, fs]: count). The off-CPU time is grouped into the locks a create can wait on
(a directory's i_rwsem, cgroup_mutex, the mount namespace's lock, a futex of the runtime's own) and the waits for work
(a socket, a pipe, a poll, a child), then come the top blocking sites, the directory operations and the profile.

A MODE=locks run: each directory's exclusive holds (@hold[vfs_mkdir or vfs_rmdir, fs, ino]: stats) beside the waits
on its i_rwsem (@rw[slow path, fs, ino]: stats), each directory named from the run's dirs.<label>.txt; cgroup_mutex's
holders (@fn[function, 1 if the runtime's own]: stats) and the forks (@fork[1 if into a cgroup]: stats); the lookups
that missed the dcache (@lookups[fs, directory, name]: count), with each name's digits and hex runs written as #; and
the share of the time cgroup_mutex was owned and the runtime's cgroup directory's i_rwsem held (@cgm_owned[bucket,
owned] and @dir_held[bucket, 2 write-held, 1 read-held, 0 free]: samples), over the 100 ms buckets in which the runtime
made or removed at least half the median bucket's cgroups (@busy[bucket]: count).

  create_serial.py <label>=<bt file>:<lifecycles> ...
"""
import os
import re
import sys
from collections import defaultdict

SCHED = re.compile(r'^(finish_task_switch|__schedule|schedule$|schedule_timeout|schedule_hrtimeout|'
                   r'schedule_preempt_disabled|io_schedule|preempt_schedule|__cond_resched|__wait_for_common|'
                   r'wait_for_completion|schedule_idle)')
# A blocking site's class, by its frames: the first rule whose patterns all match wins.
CLASSES = [
    ("a directory's i_rwsem", [r'rwsem_down_', r'(filename_create|do_rmdir|lookup_slow|open_last_lookups|'
                                              r'__start_renaming|do_unlinkat|iterate_dir|lock_rename)']),
    ("cgroup_mutex", [r'__mutex_lock', r'(cgroup_|cgroup_bpf)']),
    ("the mount namespace's lock", [r'rwsem_down_', r'(do_lock_mount|copy_mnt_ns|path_mount|do_umount|do_loopback|'
                                                   r'do_new_mount|__do_sys_pivot_root|lock_mount)']),
    ("an RCU grace period (umount)", [r'synchronize_rcu']),
    ("a futex (the runtime's own locks)", [r'futex_']),
    ("work: a socket", [r'(recvmsg|sock_)']),
    ("work: a pipe or FIFO", [r'(pipe_read|pipe_write|wait_for_partner|fifo_open)']),
    ("work: a poll", [r'(do_poll|do_sys_poll|ep_poll)']),
    ("work: a child", [r'(do_wait|kernel_wait4|__do_wait)']),
    ("a sleep (the runtime's own polling)", [r'(do_nanosleep|hrtimer_nanosleep)']),
]
# The filesystems a directory's inode can be on here; an rwsem decoded as an inode's that names none is another lock's.
FILESYSTEMS = {"cgroup2", "tmpfs", "overlay", "btrfs", "proc", "sysfs", "devtmpfs", "ext4", "xfs", "nsfs", "bpf",
               "debugfs", "tracefs", "mqueue", "devpts", "cgroup"}
# A stats() value as bpftrace 0.24 prints it, `{ .count = N, .average = N, .total = N }`, or as earlier ones did.
STATS = r'\{? ?\.?count:? =? ?(\d+), \.?average:? =? ?(\d+), \.?total:? =? ?(\d+) ?\}?'


def offcpu(txt):
    off, cpu, dirops = defaultdict(int), defaultdict(int), defaultdict(int)
    for m in re.finditer(r'@(off|cpu)\[\n?(.*?)\n?(?:, ([^\]\n]+))?\]: (\d+)', txt, re.S):
        kind, body, comm, val = m.group(1), m.group(2), m.group(3), int(m.group(4))
        frames = [re.sub(r'\+\d+$', '', f.strip()) for f in body.splitlines() if f.strip()]
        rest = [f for f in frames if not SCHED.match(f)]
        if kind == 'off':
            off[(comm.strip() if comm else '?', ' < '.join(rest[:4]) or '(scheduler only)')] += val
        else:
            cpu[' < '.join(frames[:3]) or '(user)'] += val
    for m in re.finditer(r'@dirops\[(\w+), ([^\]]+)\]: (\d+)', txt):
        dirops[(m.group(1), m.group(2).strip())] += int(m.group(3))
    return off, cpu, dirops


def klass(site):
    for name, pats in CLASSES:
        if all(re.search(p, site) for p in pats):
            return name
    return "other"


def stats(txt, name, nkeys):
    """{key tuple: (count, total ns)} of a stats() map with `nkeys` keys."""
    out = {}
    keys = r', '.join([r'([^,\]]*)'] * nkeys)
    for m in re.finditer(rf'@{name}\[{keys}\]: {STATS}', txt):
        out[tuple(k.strip() for k in m.groups()[:nkeys])] = (int(m.group(nkeys + 1)), int(m.group(nkeys + 3)))
    return out


def dirs(path):
    """{(fs, inode): path} from a run's directory listing: /sys/fs/cgroup's are cgroup2's, /run's the tmpfs's."""
    out = {}
    if os.path.exists(path):
        for line in open(path, errors='replace'):
            ino, _, p = line.strip().partition(' ')
            if ino.isdigit():
                out[("cgroup2" if p.startswith("/sys/fs/cgroup") else "tmpfs", ino)] = p
    return out


def pattern(name):
    return re.sub(r'\d', '#', re.sub(r'[0-9a-f]{8,}', '#', name))


def report_offcpu(runs):
    head = '  '.join(f'{label:>9}' for label, *_ in runs)
    print(f"off-CPU ms per lifecycle, by what a create waits on\n{head}  class")
    for c in [c for c, _ in CLASSES] + ["other"]:
        vals = [sum(v for (_comm, site), v in off.items() if klass(site) == c) / n / 1e6 for _l, n, (off, _c, _d) in runs]
        print('  '.join(f'{v:9.3f}' for v in vals) + f'  {c}')
    for label, n, (off, _cpu, _d) in runs:
        print(f"\n{label}: top blocking sites, off-CPU ms per lifecycle ({n} lifecycles)")
        for (comm, site), v in sorted(off.items(), key=lambda kv: -kv[1])[:14]:
            print(f"  {v / n / 1e6:8.3f}  {comm:14.14} {site[:140]}")
    for label, n, (_off, _cpu, dirops) in runs:
        print(f"\n{label}: directory operations per lifecycle, by the filesystem of the directory they change")
        for (func, fs), v in sorted(dirops.items(), key=lambda kv: -kv[1]):
            print(f"  {v / n:8.2f}  {func:12} {fs}")
    for label, _n, (_off, cpu, _d) in runs:
        tot = sum(cpu.values()) or 1
        print(f"\n{label}: on-CPU samples, top 8 by share")
        for k, v in sorted(cpu.items(), key=lambda kv: -kv[1])[:8]:
            print(f"  {100 * v / tot:6.2f}%  {k[:140]}")


def report_locks(label, n, txt, named):
    hold, rw = stats(txt, 'hold', 3), stats(txt, 'rw', 3)
    per = defaultdict(lambda: defaultdict(float))
    for (func, fs, ino), (cnt, tot) in hold.items():
        d = per[(fs, ino)]
        d[func + ' n'] += cnt / n
        d[func + ' ms'] += tot / n / 1e6
    other_rw = defaultdict(float)
    for (func, fs, ino), (cnt, tot) in rw.items():
        if fs in FILESYSTEMS:
            per[(fs, ino)][func] += tot / n / 1e6
        else:
            other_rw[func] += tot / n / 1e6
    print(f"\n{label} ({n} lifecycles): directories, ms per lifecycle: the parent's i_rwsem held exclusively across "
          f"each vfs_mkdir and vfs_rmdir, and waited for (write, read)")
    print(f"  {'mkdir/lc':>8} {'mkdir ms':>8} {'rmdir/lc':>8} {'rmdir ms':>8} {'wait w':>8} {'wait r':>8}  directory")
    rows = sorted(per.items(), key=lambda kv: -(kv[1]['vfs_mkdir ms'] + kv[1]['vfs_rmdir ms'] +
                                                kv[1]['rwsem_down_write_slowpath'] + kv[1]['rwsem_down_read_slowpath']))
    for (fs, ino), d in rows[:10]:
        print(f"  {d['vfs_mkdir n']:8.2f} {d['vfs_mkdir ms']:8.3f} {d['vfs_rmdir n']:8.2f} {d['vfs_rmdir ms']:8.3f} "
              f"{d['rwsem_down_write_slowpath']:8.3f} {d['rwsem_down_read_slowpath']:8.3f}  "
              f"{named.get((fs, ino), f'{fs} inode {ino}')}")
    for func, v in sorted(other_rw.items()):
        print(f"  waits on an rwsem that isn't a directory's (namespace_sem, a process's mmap_lock, ...), {func}: "
              f"{v:.3f} ms")
    print(f"{label}: cgroup_mutex's holders (timed whole; at concurrency 1 a holder's time is its hold), per lifecycle")
    for (func, own), (cnt, tot) in sorted(stats(txt, 'fn', 2).items(), key=lambda kv: -kv[1][1]):
        who = "the runtime's" if own == '1' else "other processes'"
        print(f"  {cnt / n:6.2f} x {tot / max(cnt, 1) / 1e3:8.1f} us = {tot / n / 1e6:7.3f} ms  {func} ({who})")
    for (into,), (cnt, tot) in sorted(stats(txt, 'fork', 1).items()):
        what = "into a cgroup (CLONE_INTO_CGROUP: cgroup_mutex held)" if into == '1' else "not into a cgroup"
        print(f"  {cnt / n:6.2f} x {tot / max(cnt, 1) / 1e3:8.1f} us = {tot / n / 1e6:7.3f} ms  fork {what}, "
              f"cgroup_can_fork to cgroup_post_fork's return")
    busy = {int(m.group(1)): int(m.group(2)) for m in re.finditer(r'@busy\[(\d+)\]: (\d+)', txt)}
    if busy:
        counts = sorted(busy.values())
        floor = counts[len(counts) // 2] / 2
        window = {b for b, v in busy.items() if v >= floor}
        for name, title, states in (('cgm_owned', 'cgroup_mutex', {1: 'owned'}),
                                    ('dir_held', "the cgroup directory's i_rwsem", {2: 'write-held', 1: 'read-held'})):
            samples = defaultdict(int)
            for m in re.finditer(rf'@{name}\[(\d+), (\d+)\]: (\d+)', txt):
                if int(m.group(1)) in window:
                    samples[int(m.group(2))] += int(m.group(3))
            total = sum(samples.values()) or 1
            shares = ', '.join(f'{what} {100 * samples[k] / total:.1f}%' for k, what in states.items())
            print(f"{label}: {title}, of {total} samples in {len(window)} busy 100 ms buckets: {shares}")
    lookups = defaultdict(int)
    for m in re.finditer(r'@lookups\[([^,\]]*), ([^,\]]*), ([^\]]*)\]: (\d+)', txt):
        lookups[(m.group(1), pattern(m.group(2)), pattern(m.group(3)))] += int(m.group(4))
    print(f"{label}: lookups that missed the dcache (lookup_slow), per lifecycle: "
          f"{sum(lookups.values()) / n:.2f} in all")
    for (fs, d, name), v in sorted(lookups.items(), key=lambda kv: -kv[1])[:16]:
        print(f"  {v / n:6.2f}  {fs:8} {d[:40]:40} {name[:60]}")


runs = []
for arg in sys.argv[1:]:
    label, rest = arg.split('=', 1)
    path, n = rest.rsplit(':', 1)
    runs.append((label, int(n), path, open(path, errors='replace').read()))
if not runs:
    print(__doc__, file=sys.stderr)
    sys.exit(2)
off_runs = [(label, n, offcpu(txt)) for label, n, _p, txt in runs if '@off[' in txt]
if off_runs:
    report_offcpu(off_runs)
for label, n, path, txt in runs:
    if '@hold[' in txt or '@fn[' in txt:
        report_locks(label, n, txt, dirs(os.path.join(os.path.dirname(path), f"dirs.{label}.txt")))
