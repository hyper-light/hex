#!/usr/bin/env python3
"""bench/competitive/probes: create_serial.sh's reader, each figure per lifecycle of its run.

A MODE=offcpu run: bpftrace's off-CPU map (@off[kstack, comm]: ns), its on-CPU profile (@cpu[kstack]: samples) and
its directory operations (@dirops[func, fs]: count). The off-CPU time is grouped into the locks a create can wait on
(a directory's i_rwsem, cgroup_mutex, the mount namespace's lock, a futex of the runtime's own) and the waits for work
(a socket, a pipe, a poll, a child), then come the top blocking sites, the directory operations and the profile.

A MODE=locks run: each directory's exclusive holds (@hold[vfs_mkdir or vfs_rmdir, fs, ino]: stats) beside the waits
on its i_rwsem (@rw[slow path, fs, ino]: stats), each directory named from the run's dirs.<label>.txt; cgroup_mutex's
takers, each timed whole, wait and hold (@fn[function, 1 if the runtime's own]: stats), the forks (@fork[1 if into a
cgroup]: stats) and the waits for the lock alone (@cgm_wait[1 if the runtime's]: stats); the lookups that missed the
dcache (@lookups[fs, directory, name]: count), with each name's digits and hex runs written as #; and the share of the
time cgroup_mutex and the runtime's cgroup directory's i_rwsem were in each state (@cgm_state[bucket, 2 held, 1
handed off, 0 free] and @dir_state[bucket, 3 write-held, 2 read-held, 1 handed off, 0 free]: samples), over the 100
ms buckets in which the runtime made or removed at least half the median bucket's cgroups (@busy[bucket]: count).

A MODE=checker run, each figure per namespace checked (its out.<label>.json's netns_reuse.checked, which stands in
for <lifecycles>): the checker threads' system calls (@sys[number]: stats), their futex waits for the next namespace
left out of the sum, and their on-CPU profile at 4999 Hz (@samples), by the system call each sample fell in or user
mode (@on_sys[number], @on_user: count) and by the kernel work its stack names (@cpu[kstack, ustack]: count).

  create_serial.py <label>=<bt file>:<lifecycles> ...
"""
import json
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


# x86_64's numbers for the calls a checker makes (arch/x86/entry/syscalls/syscall_64.tbl).
SYSCALLS = {0: "read", 1: "write", 2: "open", 3: "close", 9: "mmap", 10: "mprotect", 11: "munmap",
            14: "rt_sigprocmask", 28: "madvise", 41: "socket", 44: "sendto", 45: "recvfrom", 49: "bind",
            54: "setsockopt", 131: "sigaltstack", 186: "gettid", 202: "futex", 204: "sched_getaffinity",
            257: "openat", 272: "unshare", 308: "setns"}
PROFILE_HZ = 4999
# A checker's kernel stack's work, by the first rule any of whose frames match, innermost first.
CHECKER_WORK = [
    ("reading /proc/net/protocols", r'proto_seq_'),
    ("reading /proc/net/dev", r'dev_seq_|dev_get_stats|loopback_get_stats'),
    ("reading /proc/net/sockstat", r'sockstat_seq_show|sockstat6_seq_show'),
    ("reading a seq_file, the rest", r'seq_read'),
    ("opening a /proc/net file", r'do_sys_openat|path_openat|proc_lookup|proc_tgid_net_lookup|seq_open|do_filp_open'),
    ("dumping routes", r'inet_dump_fib|inet6_dump_fib|fib6_dump|rt6_dump|fib_table_dump'),
    ("dumping addresses", r'inet_dump_ifaddr|inet6_dump_addr|in6_dump|inet_fill_ifaddr|inet6_fill_ifaddr'),
    ("dumping rules", r'fib_nl_dumprule|dump_rules|fib_nl_fill_rule'),
    ("netlink, the rest", r'netlink_|rtnl_dump|rtnetlink_rcv|__sys_sendto|__sys_recvfrom|sock_recvmsg|skb_'),
    ("making a socket", r'__sys_socket|sk_alloc'),
    ("closing (releases)", r'__fput|fput|seq_release|sock_close|task_work_run'),
    ("setns", r'setns|switch_task_namespaces|commit_nsset'),
    ("faulting in or allocating pages", r'clear_page|handle_mm_fault|exc_page_fault|__alloc_pages|get_page_from_freelist'),
    ("a futex (the next namespace's wait)", r'futex'),
    ("mapping memory", r'vm_mmap|do_mmap|__vm_munmap|do_vmi_munmap|madvise'),
    ("entering and leaving the kernel", r'entry_SYSCALL|syscall_exit|do_syscall_64|exit_to_user'),
]


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
    print(f"{label}: cgroup_mutex's takers, each timed whole (its wait for the lock and its hold), per lifecycle")
    for (func, own), (cnt, tot) in sorted(stats(txt, 'fn', 2).items(), key=lambda kv: -kv[1][1]):
        who = "the runtime's" if own == '1' else "other processes'"
        print(f"  {cnt / n:6.2f} x {tot / max(cnt, 1) / 1e3:8.1f} us = {tot / n / 1e6:7.3f} ms  {func} ({who})")
    for (into,), (cnt, tot) in sorted(stats(txt, 'fork', 1).items()):
        what = "into a cgroup (CLONE_INTO_CGROUP: cgroup_mutex held)" if into == '1' else "not into a cgroup"
        print(f"  {cnt / n:6.2f} x {tot / max(cnt, 1) / 1e3:8.1f} us = {tot / n / 1e6:7.3f} ms  fork {what}, "
              f"cgroup_can_fork to cgroup_post_fork's return")
    # The holds taken for the boxes' own cgroups alone, those under the bench's oci boxes' root (@box, since goal 5
    # extended step 1): the takers above count every kworker's on the host, and the node's start's cgroups. Per box
    # the bench made a cgroup for, and per lifecycle: the same where every box makes one, and not where an owner's
    # boxes are handed the cgroups their earlier boxes left (goal 5 extended step 6), whose criterion is per
    # lifecycle.
    box = {m.group(1): int(m.group(2)) for m in re.finditer(r'@box\[([^\]]+)\]: (\d+)', txt)}
    if box:
        made = box.get('cgroup_mkdir', 0)
        each = ', '.join(f"{k} {v / max(made, 1):.3f}" for k, v in sorted(box.items()))
        print(f"{label}: cgroup_mutex's holds for the boxes' own cgroups, per box made ({made} made): {each}; "
              f"in all {sum(box.values()) / max(made, 1):.3f}")
        each = ', '.join(f"{k} {v / n:.3f}" for k, v in sorted(box.items()))
        print(f"{label}: cgroup_mutex's holds for the boxes' own cgroups, per lifecycle ({n} lifecycles): {each}; "
              f"in all {sum(box.values()) / n:.3f}")
    busy = {int(m.group(1)): int(m.group(2)) for m in re.finditer(r'@busy\[(\d+)\]: (\d+)', txt)}
    if busy:
        counts = sorted(busy.values())
        floor = counts[len(counts) // 2] / 2
        window = {b for b, v in busy.items() if v >= floor}
        # Each lock's state as the probe decodes its word: the current program's (@cgm_state, @dir_state: an
        # owner, only the waiter and hand-off flags, or nothing), or the first form's (@cgm_owned, @dir_held),
        # which took any nonzero word for held and so counted a lock handed off, free until its woken waiter
        # runs, as held.
        for name, title, states in (
                ('cgm_state', 'cgroup_mutex', {2: 'held', 1: 'handed off (free, a woken waiter yet to run)'}),
                ('dir_state', "the cgroup directory's i_rwsem",
                 {3: 'write-held', 2: 'read-held', 1: 'handed off (free, waiters queued)'}),
                ('cgm_owned', 'cgroup_mutex (first form: any nonzero owner word)', {1: 'owner word set'}),
                ('dir_held', "the cgroup directory's i_rwsem (first form)",
                 {2: 'write-held', 1: 'any other nonzero count'})):
            samples = defaultdict(int)
            for m in re.finditer(rf'@{name}\[(\d+), (\d+)\]: (\d+)', txt):
                if int(m.group(1)) in window:
                    samples[int(m.group(2))] += int(m.group(3))
            if not samples:
                continue
            total = sum(samples.values())
            shares = ', '.join(f'{what} {100 * samples[k] / total:.1f}%' for k, what in states.items())
            print(f"{label}: {title}, of {total} samples in {len(window)} busy 100 ms buckets (each with at least "
                  f"half the median bucket's cgroups made or removed): {shares}")
    for (own,), (cnt, tot) in sorted(stats(txt, 'cgm_wait', 1).items(), reverse=True):
        who = "the runtime's" if own == '1' else "other processes'"
        print(f"{label}: waits for cgroup_mutex, {who}: {cnt / n:.2f} a lifecycle, {tot / max(cnt, 1) / 1e3:.1f} us "
              f"each on average, {tot / n / 1e6:.3f} ms a lifecycle")
    lookups = defaultdict(int)
    for m in re.finditer(r'@lookups\[([^,\]]*), ([^,\]]*), ([^\]]*)\]: (\d+)', txt):
        lookups[(m.group(1), pattern(m.group(2)), pattern(m.group(3)))] += int(m.group(4))
    print(f"{label}: lookups that missed the dcache (lookup_slow), per lifecycle: "
          f"{sum(lookups.values()) / n:.2f} in all")
    for (fs, d, name), v in sorted(lookups.items(), key=lambda kv: -kv[1])[:16]:
        print(f"  {v / n:6.2f}  {fs:8} {d[:40]:40} {name[:60]}")


def counts(txt, name):
    return {int(m.group(1)): int(m.group(2)) for m in re.finditer(rf'@{name}\[(\d+)\]: (\d+)', txt)}


def report_checker(label, path, txt):
    """A MODE=checker run, per namespace checked."""
    out = json.load(open(os.path.join(os.path.dirname(path), f"out.{label}.json")))
    checked = sum(ph.get("netns_reuse", {}).get("checked", 0) for ph in out["phases"].values())
    if not checked:
        print(f"{label}: no namespace was checked")
        return
    samples = int(re.search(r'@samples: (\d+)', txt).group(1))
    us = 1e6 / PROFILE_HZ / checked
    work_of, user = defaultdict(int), 0
    for m in re.finditer(r'@cpu\[\n(.*?)\n\]: (\d+)', txt, re.S):
        lines = m.group(1).split('\n')
        cut = next((i for i, line in enumerate(lines) if line.strip() == ','), len(lines))
        frames = [re.sub(r'\+\d+$', '', line.strip()) for line in lines[:cut] if line.strip()]
        if not frames:
            user += int(m.group(2))
            continue
        name = next((n for n, pat in CHECKER_WORK if any(re.search(pat, f) for f in frames)),
                    'other: ' + ' < '.join(frames[:3]))
        work_of[name] += int(m.group(2))
    mean = [ph["netns_reuse"]["check_us_mean"] for ph in out["phases"].values() if "netns_reuse" in ph]
    print(f"{label}: {checked} namespaces checked, {', '.join(f'{m:.1f}' for m in mean)} us each by the checker's "
          f"own clock; on-CPU {samples * us:.1f} us a check ({samples} samples), user mode {user * us:.1f} us")
    calls = {int(k[0]): v for k, v in stats(txt, 'sys', 1).items()}
    work = {k: v for k, v in calls.items() if SYSCALLS.get(k) != "futex"}
    print(f"  system calls a check, the futex waits left out: {sum(c for c, _ in work.values()) / checked:.1f} calls, "
          f"{sum(t for _, t in work.values()) / 1e3 / checked:.1f} us")
    for k, (c, t) in sorted(work.items(), key=lambda kv: -kv[1][1])[:10]:
        print(f"  {c / checked:7.2f} calls {t / 1e3 / checked:7.1f} us  {SYSCALLS.get(k, k)}")
    print("  on-CPU a check by the system call each sample fell in:")
    for k, v in sorted(counts(txt, 'on_sys').items(), key=lambda kv: -kv[1])[:10]:
        print(f"  {v * us:7.1f} us  {SYSCALLS.get(k, k)}")
    print("  on-CPU a check in the kernel, by its work:")
    for name, v in sorted(work_of.items(), key=lambda kv: -kv[1])[:16]:
        print(f"  {v * us:7.1f} us  {name[:110]}")


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
    if '@samples:' in txt and '@sys[' in txt:
        report_checker(label, path, txt)
for label, n, path, txt in runs:
    if '@hold[' in txt or '@fn[' in txt:
        report_locks(label, n, txt, dirs(os.path.join(os.path.dirname(path), f"dirs.{label}.txt")))
