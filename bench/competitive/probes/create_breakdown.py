#!/usr/bin/env python3
"""bench/competitive/probes: the create breakdown's trace reader and summary (create_breakdown.md).

  create_breakdown.py drain <trace_pipe> <out file> <stop file>
  create_breakdown.py check <run dir> <prefix>
  create_breakdown.py summarize <run dir>
  create_breakdown.py switch <trace file> <trace stats> [seam result json] [switch|mounts]
  create_breakdown.py mounts <trace file> <trace stats> <asm/unistd_64.h> [pruned|classic] [switch|root]
  create_breakdown.py test   (selftest: the record-write and chosen-cgroup cutters over synthetic traces)

drain copies a trace instance's trace_pipe to a file until the stop file appears, then takes what's left
(exec_breakdown.py's).

check reads one set of results (<prefix>.libcrun.json, <prefix>.seam.json, <prefix>.traced.* and <prefix>.detail.*)
and exits 1 naming what doesn't read whole: an error recorded, a lost trace event, a traced create not cut into its
parts, a cut that the detailed trace shows opening another file than the one it stands for.

summarize writes the run's markdown (stdout) and summary.json beside its results: libcrun's create p50, the seam's,
and each traced create cut into its parts, grouped as decision 5 of docs/design/runtime-architecture.md weighs them:
the box's record, the cgroup, the shim's spawn and exec (or, where a seed clones the init, the request to the seed:
goal 5 plan step 4), the init's set-up, the handshake, and the runtime's own work. A part runs from one event of the call's critical path to the next (create_cuts), so a create's parts cover
it whole, from seam_oci's marker before it to the one after.

The trace, its markers and its reading are exec_breakdown.py's (its module is imported from beside this one):
the measured rounds fetch no path but where the kernel fetches one for every call of its kind (mkdir's), so an
open is told by its place in the call, and the detailed trace, with each open's path, holds every such cut to the
file it stands for. Percentiles are nearest-rank; the clock is CLOCK_MONOTONIC's, to the microsecond.
"""
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import exec_breakdown as eb  # noqa: E402  (beside this file; python3 -I leaves the script's directory off the path)
from exec_breakdown import (Missing, POLLS, RENAMES, enter, exited, first, last_before, named, probed,  # noqa: E402
                            sched)

RECORD = "the box's record"
CGROUP = "the cgroup, made and bounded"
SHIM = "the shim's spawn and exec"
SEED = "the seed: the request and its clone's preparing"
INIT = "the init's set-up, to the exec FIFO"
HANDSHAKE = "the handshake"
OWN = "the node's and the runtime's own work"
GROUPS = [RECORD, CGROUP, SHIM, SEED, INIT, HANDSHAKE, OWN]

# The parts of a create through the seam (node.rs create_oci, oci/run.rs create_in_cgroup, oci/init.rs's shim and
# container), each (name, group), in critical-path order: part k runs from cut k to cut k+1 (create_cuts).
CREATE_PARTS = [
    ("the call reaches the node; the spec checked; the box's record looked for", OWN),
    ("its cgroup looked for; a watch slot reserved; the record encoded, its temp file made", RECORD),
    ("the record written", RECORD),
    ("the record's fsync (kernel)", RECORD),
    ("the record closed and renamed into place; the store's directory opened", RECORD),
    ("the directory's fsync (kernel)", RECORD),
    ("the box root listed, its boxes counted", CGROUP),
    ("the cgroup made (kernel)", CGROUP),
    ("its bounds written: memory.max, memory.swap.max, pids.max", CGROUP),
    ("the container's state directory made; config.json read and parsed; its device policy planned", OWN),
    ("the exec FIFO made; the device program's instructions made", OWN),
    ("the device program loaded (kernel: the verifier)", OWN),
    ("the program held for its policy looked up (before goal 5's extension, its identity and the hints read and "
     "written); attached", OWN),
    ("the pipes made; a namespace made ahead taken; the shim's spawn prepared", OWN),
    ("the shim's clone (vfork, kernel), until its child runs", SHIM),
    ("the child's set-up; the hex binary's execve (kernel)", SHIM),
    ("the hex binary starts: relocated, its pages faulted in, libc's start-up", SHIM),
    ("Rust's start-up; the shim reads its environment; config.json opened", SHIM),
    ("config.json read and parsed; the cgroup, its procs and the namespace made ahead opened", INIT),
    ("the namespace made ahead joined (setns, kernel)", INIT),
    ("the container's other namespaces made: pid, mount, ipc, uts (unshare)", INIT),
    ("the init's clone into its cgroup (kernel), until it runs", INIT),
    ("the init's root mounted: the rootfs bound, /usr, /proc and /dev", INIT),
    ("pivot_root (kernel)", INIT),
    ("the old root unmounted; the hostname set; the process become (capabilities, user); ready sent", INIT),
    ("the agent wakes to the ready signal and reads it; the init's stat opened", HANDSHAKE),
    ("the init's start time read; the shim reaped", HANDSHAKE),
    ("the container's state written (state.json, renamed into place)", OWN),
    ("the state read back (state.json); the init's stat opened", OWN),
    ("the init checked; the record encoded, its temp file made", RECORD),
    ("the record written", RECORD),
    ("the record's fsync (kernel)", RECORD),
    ("the record closed and renamed into place; the store's directory opened", RECORD),
    ("the directory's fsync (kernel)", RECORD),
    ("the init watched (its pidfd, its stat checked, the epoll set); the box's info made; the call returns", OWN),
]
# The parts of a create whose init a seed clones (goal 5 plan step 4: node.rs create_oci through createpool.rs, the
# seed's serve_create in oci/seed.rs, init.rs clone_child): the shim path's first 13 and last 6, and between them the
# request to a seed, its clone of the init into the cgroup and the namespaces the spec makes, and the init's set-up.
SEED_PARTS = CREATE_PARTS[:13] + [
    ("the pipes made; a namespace made ahead taken; the request sent to a seed, which wakes and opens the cgroup", SEED),
    ("the seed's clone prepared", SEED),
    ("the init's clone into its cgroup and new namespaces: pid, mount, and ipc and uts unless a pair is made ahead "
     "(kernel), until it runs", INIT),
    ("the init's first steps, with its joins of an ipc and uts pair made ahead, to its join of the network namespace "
     "made ahead", INIT),
    ("the namespace made ahead joined (setns, kernel)", INIT),
    ("the init's root mounted: the rootfs bound, /usr, /proc and /dev, its device nodes made", INIT),
    ("pivot_root (kernel)", INIT),
    ("the old root unmounted; the hostname set; the process become (capabilities, user); ready sent", INIT),
    ("the agent wakes to the ready signal and reads it; the init's stat opened", HANDSHAKE),
    ("the init's start time read; the container's state written (state.json, renamed into place)", OWN),
] + CREATE_PARTS[28:]
# The parts of a create whose init a pruned seed clones (goal 5 plan step 2): the seed path's, but the init
# builds its root from the descriptors the agent cloned and ends with `chroot`, with no `pivot_root`. Its three
# root parts (seed parts 18-20) are renamed; the switch (part 20, 1-indexed) is the `chroot`, measured in place
# of `pivot_root`. The mounts (move_mount/fsmount/mount_setattr) aren't traced, so part 19 runs to the `chroot`,
# the `MS_MOVE` onto / among them where an init made one (step 2's; from step 3 the rootfs is attached on /
# itself); a pruned create traces one `chroot` and no init `pivot_root` (create_cuts).
PRUNED_SEED_PARTS = (
    SEED_PARTS[:13]
    + [
        # The pruned seed is handed the cgroup as a descriptor, so it opens none; it reads config.json
        # through the bundle descriptor instead (goal 5 plan step 2).
        ("the pipes made; a namespace made ahead taken; the request sent to a seed, which wakes and reads its "
         "spec (config.json; since 5bc0a2c, the checked spec's sealed memfd)", SEED),
    ]
    + SEED_PARTS[14:18]
    + [
        ("the init's root attached and its mounts made: the rootfs and binds (move_mount), proc and /dev "
         "(fsmount), its device nodes (mknod)", INIT),
        ("the root chrooted into (chroot)", INIT),
        ("the hostname set; the process become (capabilities, user); ready sent", INIT),
    ]
    + SEED_PARTS[21:]
)
# The parts of a create whose record is written once, last (goal 5 extended step 5: node.rs create_oci writes no record
# until the box is whole, and the runtime hands it the state it wrote): the seed path's without its first record write
# or the state's read back, the box's record looked for in both of its places, and the record's write cut at its temp
# file's open entry (create_breakdown_inside.sh's kprobe), so a write's filesystem work is parts of its own.
RECORD_ENCODED = ("the record encoded", RECORD)
TEMP_MADE = ("its temp file made (kernel)", RECORD)
ONCE_PARTS = [
    ("the call reaches the node; the spec checked; its turn taken; the box's record looked for in both places", OWN),
    ("its cgroup looked for; a watch slot reserved; the box root counted", CGROUP),
] + CREATE_PARTS[7:13] + SEED_PARTS[13:23] + [RECORD_ENCODED, TEMP_MADE] + CREATE_PARTS[30:]
# The same on the pruned-seed path (goal 5 plan step 2), its seed and root parts the pruned path's.
PRUNED_ONCE_PARTS = ONCE_PARTS[:8] + PRUNED_SEED_PARTS[13:23] + ONCE_PARTS[18:]
# The record-once parts where the node's owner chooses the box's cgroup once the runtime has read its spec (goal 5
# extended step 6: node.rs create_oci's Choice, oci/run.rs Cgroup::Chosen): the state directory and the exec FIFO are
# made first, then the cgroup is chosen, an idle one of the owner's read clean and handed on (no mkdir, its bounds
# written only where they differ) or a fresh one made (a slot's mkdirat, `.<n>`, or the box's own). The same number of
# parts as ONCE_PARTS, so each run's parts line up; where a cgroup is handed on, the made and bounds parts are empty.
CHOSEN_PARTS = [
    ONCE_PARTS[0],
    ("a watch slot reserved; the owner's cgroup and network choosers made", OWN),
    ("the container's state directory made; config.json read and parsed; its device policy planned", OWN),
    ("the exec FIFO made; the cgroup chosen: an idle one of its owner's read clean and handed on, or a fresh one named",
     CGROUP),
    ("a fresh cgroup made (kernel); none where one was handed on", CGROUP),
    ("a fresh cgroup's bounds written; the device program's instructions made", CGROUP),
] + ONCE_PARTS[6:]
PRUNED_CHOSEN_PARTS = CHOSEN_PARTS[:8] + PRUNED_SEED_PARTS[13:23] + CHOSEN_PARTS[18:]
# The part lists whose record is written once (goal 5 extended step 5), which summarize's bookkeeping reads.
ONCE_LISTS = (ONCE_PARTS, PRUNED_ONCE_PARTS, CHOSEN_PARTS, PRUNED_CHOSEN_PARTS)
# The node's bookkeeping, goal 5 extended step 5's first measure: the record's writes and the container's state
# written, and read back where it is, as run 4 cut them (its parts 2-6 and 23-29: the record written twice, the state
# written and read back), bookkeeping_of.
STATE_WRITTEN = SEED_PARTS[22][0]
READ_BACK = CREATE_PARTS[28][0]
RECORD_FS = [TEMP_MADE[0]] + [n for n, _ in CREATE_PARTS[30:34]]
# Run 4's bookkeeping (create-breakdown-20261031T061841Z, create_breakdown.md): its parts 2, 3, 23, 24, 25 and 26,
# 0.047 + 0.032 + 0.051 + 0.027 + 0.066 + 0.069 ms, the record's fsyncs empty. Step 5's measures are against it.
RUN4_BOOKKEEPING_MS = 0.292


def fsyncs_of(parts):
    """The parts a durable record write's fsyncs are (decision 5's first lever, plan step 2)."""
    return [k for k, (n, _) in enumerate(parts) if n.startswith("the record's fsync")
            or n.startswith("the directory's fsync")]


def cgroup_made_of(parts):
    """The cgroup's making, which a pool made ahead would take off the path, its bounds still written at the take
    (plan step 5)."""
    return [k for k, (n, _) in enumerate(parts) if n in ("the box root listed, its boxes counted",
                                                         ONCE_PARTS[1][0], "the cgroup made (kernel)",
                                                         CHOSEN_PARTS[4][0])]


def bookkeeping_of(parts):
    """The parts of a part list that are the node's bookkeeping: its record's writes (the record's group) and the
    container's state written and read back."""
    return [k for k, (n, g) in enumerate(parts) if g == RECORD or n in (STATE_WRITTEN, READ_BACK)]


def named_parts(parts, *names):
    """The parts `names` name, in a part list."""
    return [k for k, (n, _) in enumerate(parts) if n in names]

ARG = re.compile(r"\b(\w+): (0x[0-9a-f]+|\d+)")
BPF_PROG_LOAD, BPF_PROG_ATTACH = 5, 8  # include/uapi/linux/bpf.h, enum bpf_cmd
CLONE_NEWNET = 0x40000000  # include/uapi/linux/sched.h
CLONE_NEWIPC = 0x08000000
CLONE_NEWUTS = 0x04000000
CLONE_NEWNS = 0x00020000  # include/uapi/linux/sched.h


def arg(e, name):
    """A syscall entry's integer argument, as tracefs prints it (decimal, or hex with its 0x)."""
    m = dict(ARG.findall(e.text))
    if name not in m:
        raise Missing(f"{e.name}'s {name} argument")
    return int(m[name], 0)


def close_fd(e):
    """The fd a close's entry names (its `fd:` argument), or None when its arguments don't carry one."""
    m = dict(ARG.findall(e.text))
    return int(m["fd"], 0) if "fd" in m else None


def held(cond, what):
    if not cond:
        raise Missing(what)


def state_dir(path, cid):
    """Whether a traced path is box `cid`'s container state directory: `<node key>-<box id>` under the node's runtime
    directory (node.rs container_name, goal 5 plan step 2), or `<state>/oci/<box id>` as nodes before it kept it."""
    return (re.fullmatch(r"[0-9a-f]{16}-" + re.escape(cid), path.rpartition("/")[2]) is not None
            or ("/oci/" in path and named(path, cid)))


def seed_fork(ev, start, bench):
    """A seed's fork of the init: the first fork after `start` by a process other than the agent whose child goes on
    to switch its root — `pivot_root` on the classic seed path, `chroot` on the pruned-seed path (goal 5 plan
    step 2). A seed the pool's own thread starts forks a child that execs instead, so it isn't taken."""
    for i in range(start, len(ev)):
        e = ev[i]
        if e.kind == "sched" and e.name == "fork" and e.pid != bench:
            child = int(e.kv["child_pid"])
            switches = enter(child, "pivot_root", "chroot")
            if any(switches(x) for x in ev[i + 1:]):
                return i, e.pid, child
    raise Missing("a seed's fork of the init")


def record_write_cuts(c, ev, bench, cid, n, split=False):
    """A record write's cuts, appended to `c`, its kind recorded on `c.record_kinds`. The write opens the
    record's file; a durable record is written to a temp file and renamed into place, a non-durable one is
    written once straight to its file with O_EXCL — no temp, no rename (goal 5 extended step 5). So "the record
    written" runs from the file's open to the rename's return, or, with no rename, to that fd's close() return —
    the fd sys_exit_openat returned — and the three parts after it are empty; a durable record's two fsyncs are
    cut as their own parts instead. The kind ("rename" or "close") is chosen by what this create's window holds;
    a write with neither a matching close nor a rename is refused. With `split`, the open is cut at its kprobe
    entry too. The rounds trace a rename's and a close's exit alone (a rename's entry fetches two paths)."""
    tmp = f".{cid}.tmp"
    start = c.at[-1] + 1
    opened = first(ev, start, exited(bench, "openat"), f"record write {n}'s open")
    fd = int(ev[opened].text, 0)
    renamed = next((i for i in range(opened + 1, len(ev)) if exited(bench, *RENAMES)(ev[i])), None)
    if renamed is not None:
        terminal, kind, what = renamed, "rename", f"record write {n}'s temp file"
        test = lambda p: named(p, tmp)  # noqa: E731  (a durable record's temp file)
        syncs = [i for i in range(start, renamed) if enter(bench, "fsync")(ev[i])]
    else:
        closed = next((j for j in range(opened + 1, len(ev))
                       if enter(bench, "close")(ev[j]) and close_fd(ev[j]) == fd), None)
        if closed is None:
            raise Missing(f"record write {n}'s close or rename")
        terminal = first(ev, closed, exited(bench, "close"), f"record write {n}'s fd {fd} close returning")
        kind, what, syncs = "close", f"record write {n}'s file", []
        test = lambda p: named(p, f"{cid}.json")  # noqa: E731  (a non-durable record, written in place)
    c.record_kinds.append(kind)
    if split:
        c.at.append(last_before(ev, start, opened, probed(bench, "openat"), f"{what}'s open entry"))
    c.at.append(opened)
    c.opens.append((opened, bench, "openat", test, what))
    if syncs:
        c.at.append(syncs[0])
        c.to(exited(bench, "fsync"), f"record write {n}'s fsync returning")
        c.to(enter(bench, "fsync"), f"record write {n}'s directory fsync")
        c.to(exited(bench, "fsync"), f"record write {n}'s directory fsync returning")
    else:
        c.at += [terminal] * 4


SLOT = re.compile(r"\.(0|[1-9][0-9]*)")


def chosen_cuts(c, ev, bench, cid):
    """A chosen cgroup's cuts (CHOSEN_PARTS[1:8]), appended to `c`: the state directory's mkdir, the exec FIFO's
    mknod, a fresh cgroup's mkdir and its return (a slot's, named `.<n>` under the oci boxes' root as hex_core::reuse
    names it, or the box's own), then the device program's load and attach as on the other paths. A cgroup handed on
    from the owner's earlier box makes no mkdir: its made part is empty, cut where the program's first call or the
    create's first pipe starts, and its choice (the idle cgroup's files read, bounds written where they differ) lies
    in the part from the FIFO. `c.cgroups` records which: "fresh" or "handed on"."""
    state = c.to(enter(bench, "mkdir", "mkdirat"), "the state directory's mkdir")
    held(state_dir(ev[state].path(), cid),
         f"a chosen cgroup's create makes the container's state directory first (it made {ev[state].path()!r})")
    fifo = c.to(enter(bench, "mknodat", "mknod"), "the exec FIFO's mknod")
    pipe = first(ev, fifo, enter(bench, "pipe2"), "the create's first pipe")
    calls = [i for i in range(fifo, pipe) if enter(bench, "bpf")(ev[i])]
    made = [i for i in range(fifo, pipe) if enter(bench, "mkdir", "mkdirat")(ev[i])]
    if made:
        held(len(made) == 1, f"a chosen cgroup is made by one mkdir, not {len(made)}")
        name = ev[made[0]].path()
        held(SLOT.fullmatch(name) is not None or name == cid
             or (name.startswith("/sys/fs/cgroup/") and named(name, cid)),
             f"the chosen cgroup's mkdir makes a slot or the box's own (it made {name!r})")
        c.at.append(made[0])
        c.to(exited(bench, "mkdir", "mkdirat"), "the chosen cgroup's mkdir returning")
        c.cgroups.append("fresh")
    else:
        c.at += [calls[0] if calls else pipe] * 2
        c.cgroups.append("handed on")
    loads = [i for i in calls if arg(ev[i], "cmd") == BPF_PROG_LOAD]
    if loads:
        c.at.append(loads[0])
        c.to(exited(bench, "bpf"), "the load returning")
    else:
        c.at += [calls[0] if calls else pipe] * 2
    if calls:
        held(arg(ev[calls[-1]], "cmd") == BPF_PROG_ATTACH,
             "the agent's last bpf call before its pipes is the device program's attach")
        c.at.append(first(ev, calls[-1], exited(bench, "bpf"), "the attach returning"))
    else:
        c.at.append(pipe)


def create_cuts(ev, bench, cid, seeded=False, pruned=False, once=False, chosen=False):
    """A create's cuts on the shim path, with `seeded` on the seed path (goal 5 plan step 4), or with
    `pruned` on the pruned-seed path (goal 5 plan step 2): the init builds its root from descriptors and
    ends with chroot, so the switch is the chroot and no init pivot_root appears. With `once` as well where
    the record is written once, last (goal 5 extended step 5), and `chosen` where the box's owner chooses its
    cgroup once the runtime has read its spec (goal 5 extended step 6, chosen_cuts)."""
    c = eb.Cuts(ev)
    c.record_kinds = []  # "rename" (a durable record) or "close" (O_EXCL, goal 5 extended step 5), per write
    c.cgroups = []  # "fresh" or "handed on" where the cgroup is chosen (goal 5 extended step 6)
    tmp = f".{cid}.tmp"

    def opened_last_before(pid, call, before, test, what):
        """The last exit of `call` by `pid` after the cut before it and before event `before`: an open the call's
        path puts right before that event."""
        i = last_before(ev, c.at[-1] + 1, before, exited(pid, call), what)
        c.at.append(i)
        c.opens.append((i, pid, call, test, what))
        return i

    def record_write(n, split=False):
        record_write_cuts(c, ev, bench, cid, n, split)

    looked = lambda p: named(p, f"{cid}.json")  # noqa: E731
    if once:
        # Both places: the runtime directory's, then the state directory's.
        c.opened(bench, "openat", 2, 0, looked, "the node's look for the box's record in its second place")
    else:
        c.opened(bench, "openat", 1, 0, looked, "the node's look for the box's record")
        record_write(1)
    if chosen:
        chosen_cuts(c, ev, bench, cid)
    else:
        made = c.to(enter(bench, "mkdir", "mkdirat"), "the cgroup's mkdir")
        # By its path, or, since the node makes a box's cgroup through the box root's directory it holds open
        # (3ea3ffa), by its name alone under that directory: a mkdirat of the box's id.
        cgroup = ev[made].path()
        held((cgroup.startswith("/sys/fs/cgroup/") and named(cgroup, cid))
             or (ev[made].name == "mkdirat" and cgroup == cid),
             f"the first mkdir is the box's cgroup (it made {cgroup!r})")
        c.to(exited(bench, "mkdir", "mkdirat"), "the cgroup's mkdir returning")
        state = c.to(enter(bench, "mkdir", "mkdirat"), "the state directory's mkdir")
        held(state_dir(ev[state].path(), cid),
             f"the second mkdir is the container's state directory (it made {ev[state].path()!r})")
        fifo = c.to(enter(bench, "mknodat", "mknod"), "the exec FIFO's mknod")
        # The device program, between the FIFO and the create's first pipe: its load where the node holds no program
        # for the box's policy, then its attach; a box whose program the oci boxes' root carries makes neither call
        # (goal 5 extended, step 1). A part with no call is empty, cut where the next one starts.
        pipe = first(ev, fifo, enter(bench, "pipe2"), "the create's first pipe")
        calls = [i for i in range(fifo, pipe) if enter(bench, "bpf")(ev[i])]
        loads = [i for i in calls if arg(ev[i], "cmd") == BPF_PROG_LOAD]
        if loads:
            c.at.append(loads[0])
            c.to(exited(bench, "bpf"), "the load returning")
        else:
            c.at += [calls[0] if calls else pipe] * 2
        if calls:
            held(arg(ev[calls[-1]], "cmd") == BPF_PROG_ATTACH,
                 "the agent's last bpf call before its pipes is the device program's attach")
            c.at.append(first(ev, calls[-1], exited(bench, "bpf"), "the attach returning"))
        else:
            c.at.append(pipe)
    if seeded:
        # A seed clones the init into its cgroup and the namespaces the spec makes; the init joins the network
        # namespace made ahead itself. On the classic seed path the seed opens the box's cgroup by path; on the
        # pruned path the agent opened it (a descriptor the seed can't name by path) and the seed instead reads
        # config.json through the bundle descriptor, so the cut anchors on that read (goal 5 plan step 2).
        born, seed, init = seed_fork(ev, c.at[-1], bench)
        clone = last_before(ev, c.at[-1] + 1, born, enter(seed, "clone3"), "the seed's clone of the init")
        if pruned:
            # Since 5bc0a2c the seed takes the spec the runtime checked from a sealed memfd, read with pread
            # (linux::oci::checked), so its last pread before the clone ends the read; an older seed opened
            # config.json through the bundle's descriptor.
            reads = [i for i in range(c.at[-1] + 1, clone) if exited(seed, "pread64")(ev[i])]
            if reads:
                c.at += [reads[-1], clone]
            else:
                opened = last_before(ev, c.at[-1] + 1, clone, exited(seed, "openat"),
                                     "the seed's read of config.json, or of the checked spec")
                c.at += [opened, clone]
                c.opens.append((opened, seed, "openat", lambda p: named(p, "config.json"),
                                "the seed's read of config.json"))
        else:
            opened = last_before(ev, c.at[-1] + 1, clone, exited(seed, "open"),
                                 "the seed's open of the box's cgroup")
            c.at += [opened, clone]
            c.opens.append((opened, seed, "open", lambda p: named(p, cid), "the seed's open of the box's cgroup"))
        c.to(exited(init, "clone3"), "the init's first run")
        # An ipc and uts pair made ahead is joined first (goal 5 extended step 4), inside part 17, so the network
        # namespace's join is the setns named for it, and each join before it names one of the pair.
        is_join = enter(init, "setns")
        since = c.at[-1]
        joins = c.to(lambda e: is_join(e) and arg(e, "flags") == CLONE_NEWNET,
                     "the init's join of the network namespace made ahead")
        before = [arg(e, "flags") for e in ev[since:joins] if is_join(e)]
        held(before in ([], [CLONE_NEWIPC, CLONE_NEWUTS]), "the init joins the pair, or none, before the network namespace")
        c.to(exited(init, "setns"), "the join returning")
    else:
        spawn = first(ev, c.at[-1], enter(bench, "clone3", "clone"), "the shim's spawn")
        c.at.append(spawn)
        fork = first(ev, spawn, sched(bench, "fork"), "the shim's fork")
        shim = int(ev[fork].kv["child_pid"])
        c.to(exited(shim, "clone3", "clone"), "the shim's first run")
        c.to(sched(shim, "exec"), "the shim's exec event")
        c.to(exited(shim, *POLLS), "the shim's first traced call (std's poll of its stdio)")
        joins = first(ev, c.at[-1], enter(shim, "setns"), "the shim's join of the namespace made ahead")
        cgroup = first(ev, c.at[-1], exited(shim, "open"), "the shim's open of the cgroup")
        held(cgroup < joins, "the shim opens the cgroup before it joins a namespace")
        opened_last_before(shim, "openat", cgroup, lambda p: named(p, "config.json"),
                           "the shim's open of config.json")
        held(arg(ev[joins], "flags") == CLONE_NEWNET, "the shim's setns joins a network namespace")
        c.at.append(joins)
        c.to(exited(shim, "setns"), "the join returning")
        c.to(enter(shim, "clone3"), "the shim's clone of the init")
        born = first(ev, c.at[-1], sched(shim, "fork"), "the init's fork")
        init = int(ev[born].kv["child_pid"])
        c.to(exited(init, "clone3"), "the init's first run")
    if pruned:
        # No pivot_root: the init attaches the rootfs and mounts with the new mount API (move_mount,
        # fsmount, mount_setattr, not traced), makes its device nodes (mknod, traced), then chroots into
        # the root (traced), on / already (step 2's init moved it there by MS_MOVE, not traced). Part 19
        # runs to the chroot; part 20 is the chroot itself. No init calls pivot_root on this path (AC-a).
        is_pivot = enter(init, "pivot_root")
        held(not any(is_pivot(e) for e in ev), "no init calls pivot_root on the pruned path (AC-a)")
        c.to(enter(init, "chroot"), "the init's root attached and its mounts made, to the chroot")
        c.to(exited(init, "chroot"), "the root chrooted into, returning")
    else:
        c.to(enter(init, "pivot_root"), "the init's pivot_root")
        c.to(exited(init, "pivot_root"), "pivot_root returning")
    ready = c.to(enter(init, "write"), "the init's ready signal")
    held(arg(ev[ready], "count") == 2, "the init's first write is its ready signal (`R` and a newline)")
    c.opened(bench, "openat", 1, ready, lambda p: p.startswith("/proc/") and named(p, "stat"),
             "the agent's open of the init's stat")
    if not seeded:
        c.to(exited(bench, "wait4"), "the agent's reaping of the shim")
    renamed = c.to(exited(bench, *RENAMES), "the container's state renamed into place")
    if once:
        record_write(2, split=True)
        return c
    c.opened(bench, "openat", 2, renamed, lambda p: p.startswith("/proc/") and named(p, "stat"),
             "the node's open of the init's stat, reading the state back")
    record_write(2)
    return c


OPS = {"create": (CREATE_PARTS, create_cuts)}
SEED_OPS = {"create": (SEED_PARTS, lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True))}
PRUNED_SEED_OPS = {
    "create": (PRUNED_SEED_PARTS, lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True, pruned=True))
}
ONCE_OPS = {"create": (ONCE_PARTS, lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True, once=True))}
PRUNED_ONCE_OPS = {
    "create": (PRUNED_ONCE_PARTS,
               lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True, pruned=True, once=True))
}
CHOSEN_OPS = {
    "create": (CHOSEN_PARTS,
               lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True, once=True, chosen=True))
}
PRUNED_CHOSEN_OPS = {
    "create": (PRUNED_CHOSEN_PARTS,
               lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True, pruned=True, once=True, chosen=True))
}


def ops_for(trace):
    """The op table for a trace: the seed path's when no create forked from the agent itself (a seed cloned its
    init, goal 5 plan step 4), else the shim path's, read from the trace's first traced create; on the seed path,
    the record-once path's when the agent renamed nothing into place before its first mkdir, the box's cgroup (goal
    5 extended step 5), and the chosen cgroup's when that first mkdir is the container's state directory (goal 5
    extended step 6: an owner's box, its cgroup chosen once the runtime has read its spec)."""
    ev, _ = eb.parse(trace)
    begin = next((i for i, e in enumerate(ev) if e.kind == "mark" and e.text.split()[:3] == ["hexbd", "B", "create"]),
                 None)
    if begin is None:
        return OPS
    bench = ev[begin].pid
    cid = ev[begin].text.split()[3]
    renamed, made, chosen = False, False, False
    for e in ev[begin + 1:]:
        if e.kind == "mark" and e.pid == bench and e.text.split()[:2] == ["hexbd", "E"]:
            # Seeded. The pruned-seed path's init chroots and never pivot_roots (goal 5 plan step 2); the
            # classic seed path pivot_roots. Tell them apart by whether any init chroots in the trace. A record
            # renamed into place before the cgroup's mkdir is the record written twice; none, written once
            # (goal 5 extended step 5).
            pruned = any(x.kind == "enter" and x.name == "chroot" for x in ev)
            if chosen:
                return PRUNED_CHOSEN_OPS if pruned else CHOSEN_OPS
            if pruned:
                return PRUNED_SEED_OPS if renamed else PRUNED_ONCE_OPS
            return SEED_OPS if renamed else ONCE_OPS
        if e.kind == "sched" and e.name == "fork" and e.pid == bench:
            return OPS
        if e.pid == bench and not made:
            renamed |= e.kind == "exit" and e.name in RENAMES
            made = e.kind == "enter" and e.name in ("mkdir", "mkdirat")
            chosen = made and not renamed and state_dir(e.path(), cid)
    return OPS


def check(run, prefix):
    bad = []
    before = tuple(k for k in ("before", "traced-before") if os.path.exists(os.path.join(run, f"{prefix}.{k}.json")))
    for kind in ("libcrun", "seam", "traced", "detail") + before:
        errs, ms = eb.lifecycle_ms(os.path.join(run, f"{prefix}.{kind}.json"), "create")
        if errs or not ms:
            bad.append(f"{kind}: errors {errs}, {len(ms)} creates")
        if kind in ("traced", "detail", "traced-before"):
            stats = os.path.join(run, f"{prefix}.{kind}.stats")
            if eb.lost(stats):
                bad.append(f"{kind}: the trace lost {eb.lost(stats)} events")
            trace = os.path.join(run, f"{prefix}.{kind}.txt")
            got, unread = eb.calls(trace, ops_for(trace))
            for c in got:
                if "missing" in c:
                    bad.append(f"{kind}: create {c['id']} lacks {c['missing']}")
            # seam_oci traces every other timed lifecycle, the first of them included.
            if len(got) != (len(ms) + 1) // 2:
                bad.append(f"{kind}: {len(got)} creates traced of {len(ms)} timed")
            kinds, cgroups = {}, {}
            for c in got:
                for k in c.get("record_kinds", []):
                    kinds[k] = kinds.get(k, 0) + 1
                for k in c.get("cgroups", []):
                    cgroups[k] = cgroups.get(k, 0) + 1
            print(f"check {prefix}.{kind}: {sum('parts' in c for c in got)} of {len(got)} traced creates cut whole, "
                  f"{unread} trace lines not read as events; record writes by kind {kinds}"
                  + (f"; chosen cgroups {cgroups}" if cgroups else ""))
    for b in bad:
        print(f"check {prefix}: {b}")
    return 1 if bad else 0


def ratio(a, b):
    return a / b if a is not None and b else None


def pooled_parts(run, kind):
    """A traced kind's part list and each part's distribution over its rounds' creates cut whole, pooled; None
    without one."""
    spec, whole = None, []
    for path in eb.rounds(run, kind).values():
        trace = path[:-len(".json")] + ".txt"
        ops = ops_for(trace)
        spec = ops["create"][0]
        whole += [c for c in eb.calls(trace, ops)[0] if "parts" in c]
    if spec is None or not whole:
        return None
    return spec, [eb.dist([c["parts"][k] / 1000 for c in whole]) for k in range(len(spec))], len(whole)


def summarize(run):
    s = {"run": run}
    lines = [f"# create breakdown: {os.path.basename(run.rstrip('/'))}", ""]
    with open(os.path.join(run, "env.txt")) as f:
        lines += ["```", f.read().rstrip(), "```", ""]

    traced, spec = {}, CREATE_PARTS
    for rnd, path in eb.rounds(run, "traced").items():
        ops = ops_for(path[:-len(".json")] + ".txt")
        spec = ops["create"][0]
        got, unread = eb.calls(path[:-len(".json")] + ".txt", ops)
        traced[rnd] = {"calls": got, "unread": unread, "lost": eb.lost(path[:-len(".json")] + ".stats"),
                       "ids": {c["id"] for c in got}}
    rnds = sorted(traced)

    # Each round's create p50: libcrun's and the seam's untraced runs, and the traced run's creates with the
    # tracer recording (every other one) and between them.
    rows = {kind: {r: eb.lifecycle_ms(p, "create") for r, p in eb.rounds(run, kind).items()}
            for kind in ("libcrun", "seam", "before", "seam_idle", "libcrun_idle", "seam_drained")
            if kind in ("libcrun", "seam") or eb.rounds(run, kind)}
    rows["traced: recording"], rows["traced: between"] = {}, {}
    for rnd, path in eb.rounds(run, "traced").items():
        errs = eb.lifecycle_ms(path, "create")[0]
        on, off = eb.split_by_trace(path, "create", traced[rnd]["ids"])
        rows["traced: recording"][rnd], rows["traced: between"][rnd] = (errs, on), (errs, off)
    lat = {}
    for kind, per in rows.items():
        d = {r: {"errors": e, **eb.dist(ms)} for r, (e, ms) in per.items()}
        ok = {r: x for r, x in d.items() if not x["errors"] and x["n"]}
        best = min(ok, key=lambda r: ok[r]["p50"]) if ok else None
        pooled = eb.dist([x for r, (e, ms) in per.items() if not e for x in ms])
        lat[kind] = {"rounds": d, "best_round": best, "best": ok.get(best), "pooled": pooled}
    s["create_ms"] = lat
    nums = sorted({r for v in lat.values() for r in v["rounds"]})
    lines += ["## Create p50 (ms), per round", "",
              "`traced: recording` is the traced run's creates the tracer recorded, every other one; `traced: "
              "between` the creates between them, in the same process with the tracer's events armed.", "",
              "| | " + " | ".join(f"r{r}" for r in nums) + " | best | pooled p50 | pooled p99 |",
              "|---|" + "---|" * (len(nums) + 3)]
    for kind, v in lat.items():
        row = [eb.ms3(v["rounds"][r]["p50"]) if r in v["rounds"] else "n/a" for r in nums]
        lines.append(f"| {kind} | " + " | ".join(row) + f" | {eb.ms3((v['best'] or {}).get('p50'))} | "
                     f"{eb.ms3(v['pooled']['p50'])} | {eb.ms3(v['pooled']['p99'])} |")
    lines.append("")
    # With a binary from before a change beside it (`before`, its rounds shuffled in with the others), the change's
    # ratio is this run's own: the seam's create p50 over the before binary's, by round and pooled.
    if "before" in lat:
        seam_r, before_r = lat["seam"]["rounds"], lat["before"]["rounds"]
        per = {r: ratio(seam_r[r]["p50"], before_r[r]["p50"]) for r in nums if r in seam_r and r in before_r}
        pooled = ratio(lat["seam"]["pooled"]["p50"], lat["before"]["pooled"]["p50"])
        s["seam_over_before"] = {"rounds": per, "pooled": pooled}
        lines += ["The seam's create p50 over the before binary's, in the same rounds: "
                  + ", ".join(f"r{r} {eb.ms3(x)}" for r, x in per.items()) + f"; pooled {eb.ms3(pooled)}.", ""]
    # With idle threads up around some items (IDLE_AB: `seam_idle`, `libcrun_idle`, shuffled in with the others; goal
    # 5 plan step 2's AC-c), each runtime's create p50 with them over its own without them is this run's own, by round
    # and pooled. libcrun's is the control: its pivot_root walks every thread on the host, so it grows with them.
    idle = {}
    for kind, base in (("seam_idle", "seam"), ("libcrun_idle", "libcrun")):
        if kind in lat:
            with_r, without_r = lat[kind]["rounds"], lat[base]["rounds"]
            per = {r: ratio(with_r[r]["p50"], without_r[r]["p50"]) for r in nums if r in with_r and r in without_r}
            idle[base] = {"rounds": per, "pooled": ratio(lat[kind]["pooled"]["p50"], lat[base]["pooled"]["p50"])}
            lines += [f"{base}'s create p50 with the idle threads over its own without them, in the same rounds: "
                      + ", ".join(f"r{r} {eb.ms3(x)}" for r, x in per.items())
                      + f"; pooled {eb.ms3(idle[base]['pooled'])}.", ""]
    if idle:
        s["idle_over_without"] = idle
        # Each idle item's threads as create_breakdown_inside.sh's with_idle counted them, at its start and its end.
        counts = os.path.join(run, "idle_threads.txt")
        if os.path.exists(counts):
            with open(counts) as f:
                items = [line.rstrip("\n") for line in f if line.strip()]
            ups = [int(n) for line in items for n in re.findall(r"(\d+) idle threads up at its start, (\d+)", line)[0]]
            s["idle_threads"] = {"items": items, "fewest_up": min(ups) if ups else None}
            lines += [f"Each idle item's threads, at its start and its end (the fewest up at either: "
                      f"{min(ups) if ups else 'n/a'}):", ""] + [f"- {line}" for line in items] + [""]

    # The parts, by traced round and pooled, each round's sum set against its own calls.
    by_round = {r: [c for c in traced[r]["calls"] if "parts" in c] for r in rnds}
    whole = [c for r in rnds for c in by_round[r]]
    missing = {}
    for r in rnds:
        for c in traced[r]["calls"]:
            if "missing" in c:
                missing[c["missing"]] = missing.get(c["missing"], 0) + 1
    n_parts = len(spec)
    groups_here = [g for g in GROUPS if any(pg == g for _, pg in spec)]
    parts = [eb.dist([c["parts"][k] / 1000 for c in whole]) for k in range(n_parts)]
    parts_r = {r: [eb.dist([c["parts"][k] / 1000 for c in by_round[r]]) for k in range(n_parts)] for r in rnds}
    groups = {g: {"rounds": {r: 0.0 for r in rnds}, "p50": 0.0, "mean": 0.0} for g in groups_here}
    for k, (_, g) in enumerate(spec):
        for r in rnds:
            groups[g]["rounds"][r] += parts_r[r][k]["p50"] or 0
        groups[g]["p50"] += parts[k]["p50"] or 0
        groups[g]["mean"] += parts[k]["mean"] or 0
    # The ipc and uts pool on against drained (IPCUTS_AB, goal 5 extended step 4), every figure from the same shuffled
    # rounds: the whole create, untraced, for what a create gains; and, traced, the init's clone (part 16) with what
    # follows it up to the network join (part 17, which holds the pair's joins), so what moved shows beside what fell.
    # Each kind's counts say which it was: a pool on takes, a drained one makes on the path.
    drained = {}
    for rnd, path in eb.rounds(run, "traced_drained").items():
        got, _ = eb.calls(path[:-len(".json")] + ".txt", ops_for(path[:-len(".json")] + ".txt"))
        drained[rnd] = [c for c in got if "parts" in c]
    if drained and "seam_drained" in lat:
        clone = next(k for k, (name, _) in enumerate(spec) if name.startswith("the init's clone into"))
        steps = next(k for k, (name, _) in enumerate(spec) if name.startswith("the init's first steps"))

        def of(calls, *ks):
            return eb.dist([sum(c["parts"][k] for k in ks) / 1000 for c in calls])

        def counts(kind):
            total = {"taken": 0, "made_on_path": 0}
            for path in eb.rounds(run, kind).values():
                with open(path) as f:
                    got = json.load(f)["phases"]["lifecycle"]["ipc_uts"]
                for key in total:
                    total[key] += got[key]
            return total

        common = sorted(r for r in rnds if r in drained)
        seam_r, dry_r = lat["seam"]["rounds"], lat["seam_drained"]["rounds"]
        paired = [r for r in sorted(seam_r) if r in dry_r]
        ab = {"counts": {k: counts(k) for k in ("seam", "seam_drained", "traced", "traced_drained")},
              "create": {"rounds": {r: ratio(seam_r[r]["p50"], dry_r[r]["p50"]) for r in paired},
                         "on": lat["seam"]["pooled"], "drained": lat["seam_drained"]["pooled"],
                         "p50_ratio": ratio(lat["seam"]["pooled"]["p50"], lat["seam_drained"]["pooled"]["p50"]),
                         "p99_ratio": ratio(lat["seam"]["pooled"]["p99"], lat["seam_drained"]["pooled"]["p99"])}}
        for name, ks in (("part 16", (clone,)), ("part 17", (steps,)), ("parts 16+17", (clone, steps))):
            on = of([c for r in common for c in by_round[r]], *ks)
            off = of([c for r in common for c in drained[r]], *ks)
            ab[name] = {"rounds": {r: ratio(of(by_round[r], *ks)["p50"], of(drained[r], *ks)["p50"]) for r in common},
                        "on": on, "drained": off, "p50_ratio": ratio(on["p50"], off["p50"]),
                        "p99_ratio": ratio(on["p99"], off["p99"])}
        # The interleaved lifecycles (seam_oci's ipc_uts_pairs): each round's on and drained p50, the median of its
        # pairs' differences and how many pairs the pool won, for each op, so work moved to another shows there.
        pairs = {}
        for rnd, path in eb.rounds(run, "pairs").items():
            with open(path) as f:
                pairs[rnd] = json.load(f)["phases"]["ipc_uts_pairs"]
        ab["pairs"] = pairs
        s["ipcuts_ab"] = ab
        lines += ["## The ipc and uts pool on against drained, in the same rounds", "",
                  f"Counts (taken, made on the path): {ab['counts']}", "",
                  "| | on p50 (ms) | drained p50 | on over drained, p50 | on p99 | drained p99 | on over drained, p99 | "
                  "p50 ratio by round |", "|---|---|---|---|---|---|---|---|"]
        for name in ("create", "part 16", "part 17", "parts 16+17"):
            v = ab[name]
            lines.append(f"| {name} | {eb.ms3(v['on']['p50'])} | {eb.ms3(v['drained']['p50'])} | {v['p50_ratio']:.3f} | "
                         f"{eb.ms3(v['on']['p99'])} | {eb.ms3(v['drained']['p99'])} | {v['p99_ratio']:.3f} | "
                         + " ".join(f"{x:.3f}" for x in v["rounds"].values()) + " |")
        lines.append("")
        if pairs:
            lines += ["Interleaved lifecycle by lifecycle, by round: median difference in ms, on minus drained (the share of "
                      "pairs the pool won); then its op's on and drained p50 (ms):", "",
                      "| round | create | start | exec | delete |", "|---|---|---|---|---|"]
            def cell(p):
                return (f"{p['median_difference']:+.4f} ({p['on_faster']:.2f}); {p['on_p50']:.3f} v {p['drained_p50']:.3f}")
            for rnd, p in sorted(pairs.items()):
                lines.append(f"| r{rnd} | " + " | ".join(cell(p[op]) for op in ("create", "start", "exec", "delete")) + " |")
            lines.append("")
    sum_p50 = sum(p["p50"] for p in parts) if whole else None
    per_round = {}
    for r in rnds:
        sp = sum(p["p50"] for p in parts_r[r]) if all(p["n"] for p in parts_r[r]) else None
        btw = eb.dist(rows["traced: between"][r][1])["p50"]
        rcd = eb.dist(rows["traced: recording"][r][1])["p50"]
        per_round[r] = {"sum_of_part_p50s": sp, "between_p50": btw, "recording_p50": rcd,
                        "sum_over_between": ratio(sp, btw), "sum_over_recording": ratio(sp, rcd),
                        "recording_over_between": ratio(rcd, btw)}
    between, recording = lat["traced: between"]["pooled"], lat["traced: recording"]["pooled"]
    seam, libcrun = lat["seam"]["pooled"], lat["libcrun"]["pooled"]
    span = eb.dist([c["span"] / 1000 for c in whole])
    cols = " | ".join(f"r{r} p50" for r in rnds)
    lines += [f"## Create through the seam, in parts ({len(whole)} traced creates"
              + (f"; {sum(missing.values())} not cut: {missing}" if missing else "") + ")", "",
              f"| # | part | group | {cols} | pooled p50 | pooled p99 | pooled mean |",
              "|---|---|---|" + "---|" * (len(rnds) + 3)]
    for k, (name, group) in enumerate(spec):
        row = " | ".join(eb.ms3(parts_r[r][k]["p50"]) for r in rnds)
        lines.append(f"| {k + 1} | {name} | {group} | {row} | {eb.ms3(parts[k]['p50'])} | "
                     f"{eb.ms3(parts[k]['p99'])} | {eb.ms3(parts[k]['mean'])} |")
    lines += ["", f"| group, its parts' p50s summed | {' | '.join(f'r{r}' for r in rnds)} | pooled | its share of "
              "the pooled sum | pooled means |", "|---|" + "---|" * (len(rnds) + 3)]
    for g in groups_here:
        v = groups[g]
        share = ratio(v["p50"], sum_p50)
        lines.append(f"| {g} | " + " | ".join(f"{v['rounds'][r]:.3f}" for r in rnds)
                     + f" | {v['p50']:.3f} | {'n/a' if share is None else f'{100 * share:.1f}%'} | {v['mean']:.3f} |")
    lines += ["", "| round | the parts' p50s, summed | the creates between, p50 | the sum over it | the traced creates "
              "as timed, p50 | the sum over it | recording's cost |", "|---|---|---|---|---|---|---|"]
    for r in rnds:
        p = per_round[r]
        lines.append(f"| r{r} | {eb.ms3(p['sum_of_part_p50s'])} | {eb.ms3(p['between_p50'])} | "
                     f"{eb.ms3(p['sum_over_between'])} | {eb.ms3(p['recording_p50'])} | "
                     f"{eb.ms3(p['sum_over_recording'])} | {eb.ms3(p['recording_over_between'])}x |")
    rec = {"calls": len(whole), "missing": missing, "by_round": per_round,
           "parts": [{"part": n, "group": g, "rounds_p50": {r: parts_r[r][k]["p50"] for r in rnds}, **parts[k]}
                     for k, (n, g) in enumerate(spec)],
           "groups": groups, "span_ms": span, "sum_of_part_p50s_ms": sum_p50,
           "sum_over_between": ratio(sum_p50, between["p50"]),
           "sum_over_recording": ratio(sum_p50, recording["p50"]),
           "sum_over_seam_rounds": ratio(sum_p50, seam["p50"]),
           "recording_over_between": ratio(recording["p50"], between["p50"])}
    if whole and between["p50"] and recording["p50"] and seam["p50"]:
        lines += ["", f"Pooled, the parts' p50s sum to {sum_p50:.3f} ms, and their means to "
                  f"{sum(p['mean'] for p in parts):.3f} ms, the creates' mean span from marker to marker "
                  f"({span['mean']:.3f} ms). Against the creates between the traced ones, p50 {between['p50']:.3f} ms, "
                  f"the sum is {rec['sum_over_between']:.3f}; against the traced creates as seam_oci timed them, p50 "
                  f"{recording['p50']:.3f} ms, {rec['sum_over_recording']:.3f}; against the untraced seam rounds, p50 "
                  f"{seam['p50']:.3f} ms pooled, {rec['sum_over_seam_rounds']:.3f}. Recording cost the traced creates "
                  f"{rec['recording_over_between']:.3f}x at p50.", ""]
        # Goal 5 plan step 1's criterion: the parts, summed, within 10% of create's p50 in the same run (the creates
        # between the traced ones, in the same process at the same moments).
        rec["within_10_percent"] = abs(rec["sum_over_between"] - 1) <= 0.10
        lines += [f"Goal 5 plan step 1's criterion, the parts summed within 10% of create's p50 in the same run: "
                  f"{100 * rec['sum_over_between']:.1f}% of the creates between, "
                  f"{'met' if rec['within_10_percent'] else 'not met'}.", ""]

    # What decision 5's levers take, by the pooled parts: the record's fsyncs (plan step 2), the shim's spawn and
    # exec (the seed, plan steps 3 and 4), and the cgroup's making (a pool made ahead, plan step 5), each against
    # the creates between the traced ones; what's left once the first two go; and goal 5's target, a quarter of
    # libcrun's create p50 in the same run.
    if whole and between["p50"] and libcrun["p50"] and SEED in groups:
        # The seed path (goal 5 plan step 4): what's left for goal 5's added steps, each against the create.
        create = between["p50"]
        fsyncs = sum(parts[k]["p50"] for k in fsyncs_of(spec))
        made = sum(parts[k]["p50"] for k in cgroup_made_of(spec))
        cg, seed = groups[CGROUP]["p50"], groups[SEED]["p50"]
        device = sum(parts[k]["p50"] for k in named_parts(spec, CREATE_PARTS[11][0], CREATE_PARTS[12][0]))
        # On the pruned-seed path (goal 5 plan step 2) the switch is the chroot, not pivot_root, and the
        # root-mounted part is renamed; `pivot` then holds the chroot switch's p50, which AC-a bounds.
        pruned_path = any(n == PRUNED_SEED_PARTS[19][0] for n, _ in spec)
        switch = PRUNED_SEED_PARTS[19][0] if pruned_path else "pivot_root (kernel)"
        mounted = PRUNED_SEED_PARTS[18][0] if pruned_path else SEED_PARTS[18][0]
        root = sum(parts[k]["p50"] for k in named_parts(spec, mounted, switch))
        pivot = sum(parts[k]["p50"] for k in named_parts(spec, switch))
        left = sum_p50 - device - root - made
        target = libcrun["p50"] / 4
        share = lambda x: f"{100 * x / create:.1f}%"  # noqa: E731
        s["levers"] = {"create_p50_ms": create, "libcrun_create_p50_ms": libcrun["p50"], "target_ms": target,
                       "record_fsyncs_ms": fsyncs, "record_ms": groups[RECORD]["p50"], "seed_ms": seed,
                       "device_program_ms": device, "root_ms": root, "pivot_root_ms": pivot, "cgroup_ms": cg,
                       "cgroup_made_ms": made, "sum_over_target": ratio(sum_p50, target),
                       "left_after_device_root_cgroup_made_ms": left, "left_over_target": ratio(left, target)}
        lines += ["## What each lever takes (pooled part p50s, against the creates between, p50 "
                  f"{create:.3f} ms)", "",
                  "| lever | its parts | ms | share of create |", "|---|---|---|---|",
                  f"| the record off create's path (plan step 2) | the record's four fsyncs | {fsyncs:.3f} | "
                  f"{share(fsyncs)} |",
                  f"| | the box's record, whole | {groups[RECORD]['p50']:.3f} | {share(groups[RECORD]['p50'])} |",
                  f"| the seed (plan step 4) | the request to a seed and its clone's preparing | {seed:.3f} | "
                  f"{share(seed)} |",
                  f"| the device program once per policy (goal 5) | the program loaded and attached | {device:.3f} | "
                  f"{share(device)} |",
                  f"| the container's root without pivot_root's walk (goal 5) | the root mounted, and pivot_root | "
                  f"{root:.3f} | {share(root)} |",
                  f"| | of it, {'the switch (chroot)' if pruned_path else 'pivot_root'} | "
                  f"{pivot:.3f} | {share(pivot)} |",
                  f"| fewer `cgroup_mutex` holds (goal 5) | the cgroup, made and bounded | {cg:.3f} | {share(cg)} |",
                  f"| | of it, the making: the box root counted and the mkdir | {made:.3f} | {share(made)} |", "",
                  f"The parts sum to {sum_p50:.3f} ms. Goal 5's target, a quarter of libcrun's create p50 "
                  f"({libcrun['p50']:.3f} ms pooled), is {target:.3f} ms: the create is "
                  f"{eb.ms3(ratio(sum_p50, target))}x it. Take away the device program's load and attach, the root's "
                  f"mounts and pivot_root, and the cgroup's making, each whole, and {left:.3f} ms is left, "
                  f"{eb.ms3(ratio(left, target))}x the target.", ""]
    elif whole and between["p50"] and libcrun["p50"]:
        create = between["p50"]
        fsyncs = sum(parts[k]["p50"] for k in fsyncs_of(spec))
        shim = groups[SHIM]["p50"]
        made = sum(parts[k]["p50"] for k in cgroup_made_of(spec))
        cg = groups[CGROUP]["p50"]
        left = sum_p50 - fsyncs - shim
        target = libcrun["p50"] / 4
        levers = {"create_p50_ms": create, "libcrun_create_p50_ms": libcrun["p50"], "target_ms": target,
                  "record_fsyncs_ms": fsyncs, "record_fsyncs_share": fsyncs / create,
                  "record_ms": groups[RECORD]["p50"], "record_share": groups[RECORD]["p50"] / create,
                  "shim_ms": shim, "shim_share": shim / create,
                  "cgroup_ms": cg, "cgroup_share": cg / create, "cgroup_made_ms": made,
                  "left_after_record_and_shim_ms": left,
                  "cgroup_share_of_left": ratio(cg, left), "cgroup_made_share_of_left": ratio(made, left),
                  "left_over_target": ratio(left, target)}
        s["levers"] = levers
        lines += ["## What each lever takes (pooled part p50s, against the creates between, p50 "
                  f"{create:.3f} ms)", "",
                  "| lever | its parts | ms | share of create |", "|---|---|---|---|",
                  f"| the record off create's path (plan step 2) | the record's four fsyncs | {fsyncs:.3f} | "
                  f"{100 * fsyncs / create:.1f}% |",
                  f"| | the box's record, whole | {groups[RECORD]['p50']:.3f} | "
                  f"{100 * groups[RECORD]['p50'] / create:.1f}% |",
                  f"| the seed (plan steps 3 and 4) | the shim's spawn and exec | {shim:.3f} | "
                  f"{100 * shim / create:.1f}% |",
                  f"| a pool of cgroups made ahead (plan step 5) | the cgroup, made and bounded | {cg:.3f} | "
                  f"{100 * cg / create:.1f}% |",
                  f"| | of it, the making: the box root listed and the mkdir | {made:.3f} | "
                  f"{100 * made / create:.1f}% |", "",
                  f"Once the record's fsyncs and the shim's spawn and exec are gone, the parts left sum to {left:.3f} ms "
                  f"(before whatever a seed's clone costs in the shim's place); the cgroup, made and bounded, is "
                  f"{eb.ms3(ratio(cg, left))} of that, its making alone {eb.ms3(ratio(made, left))}. Goal 5's target, a "
                  f"quarter of libcrun's create p50 ({libcrun['p50']:.3f} ms pooled), is {target:.3f} ms: what's left "
                  f"is {eb.ms3(ratio(left, target))}x it.", ""]
    # Goal 5 extended step 5: the node's bookkeeping (the container's state written, the record's one write) against
    # run 4's, and a record write's filesystem work, each create's own (its temp file made, written, closed, renamed
    # and, where it must outlive the host, synced).
    if whole and any(spec is p for p in ONCE_LISTS):
        kept = bookkeeping_of(spec)
        fs_parts = named_parts(spec, *RECORD_FS)
        bookkeeping = sum(parts[k]["p50"] for k in kept)
        fs = eb.dist([sum(c["parts"][k] for k in fs_parts) / 1000 for c in whole])
        fs_r = {r: eb.dist([sum(c["parts"][k] for k in fs_parts) / 1000 for c in by_round[r]]) for r in rnds}
        fall = 1 - bookkeeping / RUN4_BOOKKEEPING_MS
        s["bookkeeping"] = {"parts": [spec[k][0] for k in kept], "sum_of_part_p50s_ms": bookkeeping,
                            "run4_ms": RUN4_BOOKKEEPING_MS, "fall": fall, "record_fs_ms": fs,
                            "record_fs_rounds_ms": fs_r}
        # The binary from before the change, traced in the same rounds: its bookkeeping as run 4's was cut.
        traced_before = pooled_parts(run, "traced-before")
        if traced_before:
            bspec, bparts, bn = traced_before
            bkept = bookkeeping_of(bspec)
            before_ms = sum(bparts[k]["p50"] for k in bkept)
            s["bookkeeping"]["before"] = {"creates": bn, "parts": [bspec[k][0] for k in bkept],
                                          "parts_p50_ms": [bparts[k]["p50"] for k in bkept],
                                          "sum_of_part_p50s_ms": before_ms, "fall": 1 - ratio(bookkeeping, before_ms)}
        lines += ["## The node's bookkeeping (goal 5 extended step 5)", "",
                  "| | " + " | ".join(f"r{r}" for r in rnds) + " | pooled p50 | pooled p99 |",
                  "|---|" + "---|" * (len(rnds) + 2),
                  "| the record's write, its filesystem work, each create's (" + ", ".join(RECORD_FS) + ") | "
                  + " | ".join(eb.ms3(fs_r[r]["p50"]) for r in rnds) + f" | {eb.ms3(fs['p50'])} | {eb.ms3(fs['p99'])} |",
                  "", f"The container's state written and the record written once ({len(kept)} parts: "
                  + "; ".join(spec[k][0] for k in kept) + f"), their pooled p50s summed: {bookkeeping:.3f} ms, against "
                  f"run 4's {RUN4_BOOKKEEPING_MS:.3f} (its record written twice, the state written and read back): "
                  f"{100 * fall:.1f}% under.", ""]
        if traced_before:
            b = s["bookkeeping"]["before"]
            lines += [f"The binary from before the change, traced in the same rounds ({b['creates']} creates cut "
                      f"whole), its {len(b['parts'])} bookkeeping parts' pooled p50s ("
                      + ", ".join(eb.ms3(x) for x in b["parts_p50_ms"]) + f") summed: {b['sum_of_part_p50s_ms']:.3f} "
                      f"ms; the change's is {100 * b['fall']:.1f}% under it.", ""]
    s["create_parts"] = rec
    lines += ["Trace: " + ", ".join(f"r{r} {t['lost']} events lost and {t['unread']} lines not read as events"
                                    for r, t in sorted(traced.items())) + ".", ""]
    with open(os.path.join(run, "summary.json"), "w") as f:
        json.dump(s, f, indent=1)
    print("\n".join(lines))
    return 0


def switch(trace, stats, seam_json=None, scope="switch"):
    """Read the root switch and mounts at concurrency eight. A pooled-network init's part 19
    starts on successful CLONE_NEWNET setns exit. A dry-network init makes that namespace in clone3,
    so its span starts on its own clone3 return zero and includes its initial/loopback setup. No
    create is dropped for being dry. Only chrooting pids are inits; other pivots are seed prunes.

    The mounts criterion requires every create's span and whole-create sample, concurrency eight,
    and exact agreement between clone-started spans and the seam's made_on_path network counter, less
    the namespaces the node made on the path for owners' boxes (netns_reuse made_on_path), which their
    inits join by setns as they join pooled ones.
    The historical switch-only criterion can read old traces without clone-exit instrumentation.
    Lost/unpaired events, failed switches and an init pivot are always refused.

    Beside part 19, the root in place ("root_*"), which reads alike on every way a pruned create's mount
    namespace is made (goal 6, decision 6's update): an init made from its tree joins the namespace by
    setns(CLONE_NEWNS) before its network and makes no chroot, so part 19 has no end there. The root's span
    runs from part 19's start to the init's first chdir after it (the chdir("/") that follows the chroot, or
    on a joined namespace stands for it), plus the mount namespace's join where it came before that start.
    So it is part 19 and the chroot (part 20) on the copied ways, and the same work on the joined one. It needs
    the trace's chdir entries (traces before it have none: it reads no spans there).
    """
    if scope not in ("switch", "mounts"):
        print("refusing: switch criterion is switch or mounts", file=sys.stderr)
        return 1
    ev, unread = eb.parse(trace)
    n_lost = eb.lost(stats)
    setns_flags, newnet_exit, clone_exit = {}, {}, {}
    newns_enter, newns_join, chdir_enters = {}, {}, {}
    pending, switch_us, mounts_us, dangling = {}, [], [], 0
    chroot_pids, pivot_by_pid = set(), {}
    clone_starts, setns_starts, failed_switches = 0, 0, 0
    for e in ev:
        if e.name == "clone3" and e.kind == "exit" and int(e.text, 0) == 0:
            clone_exit.setdefault(e.pid, e.ts)
        elif e.name == "setns":
            if e.kind == "enter":
                m = dict(ARG.findall(e.text))
                try:
                    setns_flags[e.pid] = int(m.get("flags", "0"), 0)
                except ValueError:
                    setns_flags[e.pid] = 0
                if setns_flags[e.pid] & CLONE_NEWNS:
                    newns_enter[e.pid] = e.ts
            elif e.kind == "exit":
                flags = setns_flags.pop(e.pid, 0)
                if flags & CLONE_NEWNET and int(e.text, 0) == 0:
                    newnet_exit.setdefault(e.pid, e.ts)
                t0 = newns_enter.pop(e.pid, None)
                if flags & CLONE_NEWNS and int(e.text, 0) == 0 and t0 is not None and t0 <= e.ts:
                    newns_join.setdefault(e.pid, (t0, e.ts))
        elif e.name == "chdir" and e.kind == "enter":
            chdir_enters.setdefault(e.pid, []).append(e.ts)
        elif e.name == "chroot":
            if e.kind == "enter":
                dangling += e.pid in pending
                pending[e.pid] = e.ts
                chroot_pids.add(e.pid)
                start = newnet_exit.get(e.pid)
                boundary = "setns"
                if start is None:
                    start, boundary = clone_exit.get(e.pid), "clone3"
                if start is not None and start <= e.ts:
                    mounts_us.append(e.ts - start)
                    setns_starts += boundary == "setns"
                    clone_starts += boundary == "clone3"
            elif e.kind == "exit":
                t0 = pending.pop(e.pid, None)
                if t0 is None or t0 > e.ts:
                    dangling += 1
                else:
                    switch_us.append(e.ts - t0)
                    failed_switches += int(e.text, 0) != 0
        elif e.name == "pivot_root" and e.kind == "enter":
            pivot_by_pid[e.pid] = pivot_by_pid.get(e.pid, 0) + 1
    dangling += len(pending)
    root_us, root_joins, root_unended = [], 0, 0
    for pid, cloned in clone_exit.items():
        if pid in pivot_by_pid:
            continue  # a seed, which prunes its own namespace
        start = newnet_exit.get(pid, cloned)
        end = next((t for t in chdir_enters.get(pid, []) if t >= start), None)
        if end is None:
            root_unended += pid in chroot_pids or pid in newns_join
            continue
        span = end - start
        join = newns_join.get(pid)
        if join is not None and join[1] <= start:
            span += join[1] - join[0]
        if join is not None:
            root_joins += 1
        root_us.append(span)
    init_pivots = sum(c for pid, c in pivot_by_pid.items() if pid in chroot_pids)
    seed_prune_pivots = sum(c for pid, c in pivot_by_pid.items() if pid not in chroot_pids)
    creates, create_ms, dev_mounts, expected_dry, levels = None, [], None, None, []
    node_made = None
    seam_error = None
    if seam_json:
        try:
            with open(seam_json) as f:
                result = json.load(f)
            if result.get("errors"):
                raise ValueError("the seam reported errors")
            tp = result["phases"]["throughput"]
            levels = [k for k in tp if k.isdigit()]
            creates = sum(tp[k]["lifecycles"] for k in levels) or None
            create_ms = sorted(x for k in levels for x in tp[k].get("create_ms", []))
            dev_mounts = tp.get("dev_mounts")
            # A namespace the node made on the path for an owner's box (step 6's reuse) is joined by setns,
            # as a pooled one is; the pool counts it among its takes made on the path all the same.
            expected_dry = tp.get("netns", {}).get("made_on_path")
            node_made = tp.get("netns_reuse", {}).get("made_on_path", 0)
            if expected_dry is not None:
                expected_dry -= node_made
        except (OSError, ValueError, KeyError, TypeError, AttributeError) as e:
            seam_error = str(e)
    ms = lambda us: None if us is None else us / 1e3
    def pcts(xs):
        return eb.pct(xs, 0.5), eb.pct(xs, 0.99), (max(xs) if xs else None)
    mp50, mp99, mmax = pcts(mounts_us)
    sp50, sp99, smax = pcts(switch_us)
    all_switched = (not seam_json or (creates is not None and creates == len(switch_us)))
    missing_spans = len(switch_us) - len(mounts_us)
    # New measurements have both boundaries. Old switch-only records tolerate their original 1%
    # missing mount spans; mounts records must cover every create, including network-pool misses.
    mounts_covered = (missing_spans == 0 if scope == "mounts"
                      else 0 <= missing_spans <= max(1, len(switch_us) // 100))
    whole_covered = creates is not None and len(create_ms) == creates
    fallback_counts_match = expected_dry is not None and expected_dry == clone_starts
    mount_evidence = (levels == ["8"] and whole_covered and fallback_counts_match)
    switch_le_bar = sp99 is not None and sp99 <= 500
    mounts_le_bar = mp99 is not None and mp99 <= 500
    ok = (n_lost == 0 and dangling == 0 and failed_switches == 0 and seam_error is None
          and bool(switch_us) and init_pivots == 0 and all_switched and mounts_covered
          and switch_le_bar and (scope != "mounts" or (mounts_le_bar and mount_evidence)))
    print(json.dumps({"ac": "AC-T7.1.4 e", "concurrency": 8, "creates": creates,
                      "chroots": len(switch_us), "mounts_spans": len(mounts_us),
                      "mounts_p50_ms": ms(mp50), "mounts_p99_ms": ms(mp99), "mounts_max_ms": ms(mmax),
                      "mounts_le_bar": mounts_le_bar, "setns_started_spans": setns_starts,
                      "clone_started_spans": clone_starts, "expected_network_on_path": expected_dry,
                      "network_made_by_node": node_made,
                      "fallback_counts_match": fallback_counts_match,
                      "switch_p50_ms": ms(sp50), "switch_p99_ms": ms(sp99), "switch_max_ms": ms(smax),
                      "switch_le_bar": switch_le_bar, "failed_switches": failed_switches,
                      "init_pivot_roots": init_pivots, "seed_prune_pivot_roots": seed_prune_pivots,
                      "lost_events": n_lost, "unread_lines": unread, "unpaired_chroots": dangling,
                      "bar_ms": 0.5, "criterion": scope, "create_samples": len(create_ms),
                      "create_p50_ms": eb.pct(create_ms, 0.5), "create_p99_ms": eb.pct(create_ms, 0.99),
                      "dev_mounts": dev_mounts,
                      "root_spans": len(root_us), "root_p50_ms": ms(eb.pct(root_us, 0.5)),
                      "root_p99_ms": ms(eb.pct(root_us, 0.99)), "root_max_ms": ms(max(root_us) if root_us else None),
                      "root_mntns_joins": root_joins, "root_unended": root_unended, "pass": ok}))
    if switch_us:
        print(f"AC-e over {len(switch_us)} creates at 8 in flight: switch (part 20) "
              f"p50 {eb.ms3(ms(sp50))} / p99 {eb.ms3(ms(sp99))} / max {eb.ms3(ms(smax))} ms; "
              f"mounts (part 19) p50 {eb.ms3(ms(mp50))} / p99 {eb.ms3(ms(mp99))} / "
              f"max {eb.ms3(ms(mmax))} ms (bar p99 <= 0.5 ms: "
              f"{'met' if mounts_le_bar else 'OVER'}). "
              f"Boundaries: {setns_starts} successful network joins, {clone_starts} child clone returns "
              f"(network made on path by the clone: {expected_dry}, by the node for an owner's box: "
              f"{node_made}); {missing_spans} missing.")
    if root_us:
        print(f"Root in place (part 19 to the init's chdir, its mount namespace's join counted) over "
              f"{len(root_us)} inits, {root_joins} joining a namespace made from its tree: "
              f"p50 {eb.ms3(ms(eb.pct(root_us, 0.5)))} / p99 {eb.ms3(ms(eb.pct(root_us, 0.99)))} ms.")
    if create_ms:
        print(f"Whole create at the same concurrency: p50 {eb.pct(create_ms, 0.5):.3f} / "
              f"p99 {eb.pct(create_ms, 0.99):.3f} ms; prepared /dev counts: {dev_mounts}.")
    if n_lost:
        print(f"refusing: the trace lost {n_lost} events", file=sys.stderr)
    if dangling or failed_switches:
        print(f"refusing: {dangling} unpaired chroot events, {failed_switches} failed chroots", file=sys.stderr)
    if not switch_us:
        print("refusing: no chroot traced", file=sys.stderr)
    if not mounts_covered:
        print(f"refusing: {missing_spans} of {len(switch_us)} chroots had no valid mount-start boundary",
              file=sys.stderr)
    if init_pivots:
        print(f"off the pruned path: {init_pivots} init pivot_roots", file=sys.stderr)
    if not all_switched:
        print(f"off the pruned path: {len(switch_us)} chroots != {creates} creates", file=sys.stderr)
    if seam_error:
        print(f"refusing: the seam result is incomplete: {seam_error}", file=sys.stderr)
    if scope == "mounts" and not mount_evidence:
        print(f"refusing: mounts need concurrency 8, every whole-create sample, and matching network "
              f"fallback counts (levels {levels}, samples {len(create_ms)}/{creates}, "
              f"clone spans {clone_starts}/{expected_dry})", file=sys.stderr)
    return 0 if ok else 1


# Goal 5 plan step 3's per-call trace (create_breakdown_inside.sh's mounts_trace): raw_syscalls' entries and exits,
# which carry a call's number and its arguments in hex, and each hex process's switches off and on the CPU and its
# page faults in user mode.
RAW_ENTER = re.compile(r"^sys_enter: NR (?P<nr>-?\d+) \((?P<args>[0-9a-f, ]*)\)\s*$")
RAW_EXIT = re.compile(r"^sys_exit: NR (?P<nr>-?\d+) = (?P<ret>-?\d+)\s*$")
SWITCH = re.compile(r"^sched_switch: prev_comm=.*? prev_pid=(?P<prev>\d+) prev_prio=-?\d+ prev_state=(?P<state>\S+)"
                    r" ==> next_comm=(?P<next_comm>.*?) next_pid=(?P<next>\d+) next_prio=-?\d+\s*$")
# The calls part 19 opens and ends on, and the two it attaches a mount with (x86_64's numbers,
# arch/x86/entry/syscalls/syscall_64.tbl), so the cut needs no header; the header names the rest.
NR_SETNS, NR_CHROOT, NR_PIVOT_ROOT, NR_MOUNT, NR_MOVE_MOUNT, NR_FSCONFIG = 308, 161, 155, 165, 429, 431
NR_CLONE, NR_CLONE3, NR_CHDIR = 56, 435, 80
ATTACHES = (NR_MOUNT, NR_MOVE_MOUNT)
# fsconfig's commands (include/uapi/linux/mount.h) and mount(2)'s flags that tell its kinds apart
# (include/uapi/linux/mount.h: MS_REMOUNT, MS_BIND, MS_MOVE, MS_REC and the propagation flags), so a call's
# kind is named: fsconfig's option set from its create, a bind from a move or a remount.
FSCONFIG_CMDS = {0: "flag", 1: "string", 2: "binary", 3: "path", 4: "path_empty", 5: "fd", 6: "create",
                 7: "reconfigure", 8: "create_excl"}
MS_REMOUNT, MS_BIND, MS_MOVE, MS_PROPAGATION = 0x20, 0x1000, 0x2000, 0x10000 | 0x20000 | 0x40000 | 0x80000
# The classic path's span opens where the create breakdown's part 23 does (the shim path's, its part 19): at the
# init's first run, its return from the shim's clone, so the calls before its mounts (the exec FIFO's directory
# opened, `setsid`) are among its calls as they are in that part. Failing that return, at the first call of its
# root, making every mount private (setup_rootfs's `mount_change("/", PRIVATE | REC)`). A seed makes its mounts
# private too, pruning once at start, so a classic span opens only in a process a traced hex process forked or
# cloned (the trace's sched_process_fork), which a seed, the node agent's child, never is.
MS_REC_PRIVATE = 0x4000 | 0x40000
FORK = re.compile(r"^sched_process_fork: comm=.*? pid=(?P<parent>\d+) child_comm=.*? child_pid=(?P<child>\d+)\s*$")
# What took an init's CPU: a softirq (ksoftirqd's, or one run on the init as an interrupt returned) by its action,
# and a kworker's work item by its function.
SOFTIRQ = re.compile(r"^softirq_(?P<edge>entry|exit): vec=\d+ \[action=(?P<action>\w+)\]\s*$")
WORK = re.compile(r"^workqueue_execute_(?P<edge>start|end): work struct (?P<work>\w+): function (?P<function>\S+)\s*$")
# A hold of the kernel's one mount lock (namespace_sem), for write (nsw, down_write) or shared (nsr, down_read): the
# kprobes create_breakdown_inside.sh's mounts_trace sets on the lock's address (goal 5 extended step 3).
NSSEM = re.compile(r"^(?P<kind>nsw|nsr): \(")
# A wait for it (goal 6): kprobes on the rwsem's slowpaths, which a down_write or down_read takes only when it can't
# have the lock at once (kernel/locking/rwsem.c, rwsem_down_write_slowpath and rwsem_down_read_slowpath), on the same
# address. And who woke a hex process (sched_waking, recorded on the waker), so a blocked wait names what ended it.
NSWAIT = re.compile(r"^(?P<kind>nsww|nsrw): \(")
# A write hold's end and a contended one's grant (goal 6, the lead's 10:47Z ask): up_write on namespace_sem, and the
# write slowpath's return, after which the waiter holds the lock (v6.19 kernel/locking/rwsem.c,
# rwsem_down_write_slowpath). Each hold is timed from its grant to its release, by holder.
NSU = re.compile(r"^nsu: \(")
# A wait in the write slowpath of any other rwsem (goal 6, lever (4)'s steady p99): in a mount call, the mountpoint
# inode's lock, which lock_mount takes before the mount lock (v6.19 fs/namespace.c:2723-2766).
RWW = re.compile(r"^rww: \(")
NSWWR = re.compile(r"^nswwr: \(")
# The holders whose off-CPU time the trace sees (mounts_trace filters sched_switch to them); a co-tenant's isn't.
HEX_COMMS = ("hex", "hex-devfs", "seam_oci")
WAKING = re.compile(r"^sched_waking: comm=.*? pid=(?P<pid>\d+) prio=")


# How each span opened, for the report's first line.
OPENS = {"setns": "join of the network namespace made ahead", "first run": "first run, its return from the clone",
         "mount (private)": "first mount, making its mounts private"}


def kind_of(nr, args):
    """A call's kind where its number alone doesn't say what part 19 does with it."""
    if nr == NR_SETNS and len(args) > 1:
        return "mnt" if args[1] & CLONE_NEWNS else "net" if args[1] & CLONE_NEWNET else None
    if nr == NR_FSCONFIG and len(args) > 1:
        return FSCONFIG_CMDS.get(args[1], str(args[1]))
    if nr == NR_MOUNT and len(args) > 3:
        f = args[3]
        return ("move" if f & MS_MOVE else "remount" if f & MS_REMOUNT else "bind" if f & MS_BIND
                else "propagation" if f & MS_PROPAGATION else "new")
    return None


def syscall_names(header):
    """Each x86_64 call's name by its number, from asm/unistd_64.h's `#define __NR_<name> <nr>` lines."""
    names = {}
    try:
        with open(header) as f:
            for line in f:
                m = re.match(r"#define __NR_(\w+)\s+(\d+)\s*$", line)
                if m:
                    names[int(m[2])] = m[1]
    except OSError:
        pass
    return names


def mounts(trace, stats, header, path="pruned", span="switch"):
    """Goal 5 plan step 3: part 19 of each create, call by call. The span is the switch reader's, from the
    exit of the init's join of the network namespace made ahead (setns with CLONE_NEWNET) to the entry of its
    chroot (the pruned path) or pivot_root (the classic one), per init pid; a classic init whose shim joined the
    namespace (`MOUNTS_PATH=classic`, `path` classic) has its span from the entry of its first mount, making its
    mounts private. Within it each call's time runs from
    its entry to its exit, its time off the CPU included; the rest is the init's own code between calls (its
    page faults among it, which are no calls) and the time it spent off the CPU there. Each mount's share is
    the calls from the one after the previous attach (mount or move_mount) to its own attach; what follows the
    last attach (on the pruned path the devices, the links and the switch's chdir) is a share of its own. Every
    figure is a mean per create but the span's, whose p50, p99 and max are its own, so the trace's microsecond
    timestamps' rounding averages out over the creates; the slowest 1% are shown the same way beside them, so
    what a tail is made of shows. A lost event is refused, as in switch.

    With `span` root (goal 6, decision 6's update), the span is the switch reader's root in place: it ends at the
    init's first chdir instead, the chroot (where there is one) a call within it, and an init that joined its mount
    namespace (setns with CLONE_NEWNS, a namespace made from its tree) before its network has that join counted as
    a call of its span, as the switch reader counts it. Each time off the CPU blocked is charged to the call it slept
    in, the process whose wakeup ended it, and whether it slept waiting for the mount lock (the rwsem slowpath
    kprobes), so a tail's blocked time says what it waited on."""
    names = syscall_names(header)
    name = lambda call: (names.get(call[0], f"nr {call[0]}") + (f" ({call[1]})" if call[1] else ""))
    read, unread = 0, 0
    cur, window, off, creates, born = {}, {}, {}, [], set()
    # pre: the init preempted off each CPU; begun: a softirq's or a work item's start on each CPU.
    pre, begun = {}, {}
    probed = False
    # joins: a mount namespace's join before the span opened, by pid; woke: who woke each blocked init; waits: a
    # mount lock slowpath taken since the init last slept, by pid.
    joins, woke, waits = {}, {}, {}
    # outside: the mount lock's write holds by a traced process outside any span, by its command.
    outside = {}
    # holding: a write hold under way by pid ([granted at, or None while it waits], its command, preempted us, blocked
    # us); held_off: a holder off its CPU mid-hold (since, how); hold_times: each ended (pid, command, us, preempted,
    # blocked); a hold taken in an init's span is "an init"'s, any other by its command.
    holding, held_off, hold_times = {}, {}, []
    ends = (NR_CHDIR,) if span == "root" else (NR_CHROOT, NR_PIVOT_ROOT)

    def charge(table, label, d):
        table[label] = table.get(label, 0) + d

    def gap(w, call, ts):
        """The init's own code since its last call (or the span's start), and the page faults in it, charged to the
        call it leads to: where in the init's code the time between calls goes."""
        g = w["gaps"].setdefault(call, [0, 0])
        g[0] += ts - w["last"]
        g[1] += w["pending"]
        w["pending"] = 0

    with open(trace, errors="replace") as f:
        for line in f:
            m = eb.LINE.match(line)
            if not m:
                unread += line.strip() != "" and not line.startswith("#")
                continue
            read += 1
            ts, pid, body = int(m["s"]) * 1_000_000 + int(m["us"]), int(m["pid"]), m["body"]
            cpu = int(m["cpu"])
            if (e := SWITCH.match(body)):
                prev, nxt = int(e["prev"]), int(e["next"])
                if (h := holding.get(prev)) is not None and h[0] is not None:
                    held_off[prev] = (ts, 2 if e["state"].startswith("R") else 3)
                if (o := held_off.pop(nxt, None)) is not None and nxt in holding:
                    holding[nxt][o[1]] += ts - o[0]
            elif NSU.match(body):
                if (h := holding.pop(pid, None)) is not None and h[0] is not None:
                    hold_times.append((pid, h[1], ts - h[0], h[2], h[3], h[4]))
                held_off.pop(pid, None)
                continue
            elif NSWWR.match(body):
                if (h := holding.get(pid)) is not None and h[0] is None:
                    h[0] = ts
                continue
            if (e := SOFTIRQ.match(body)) or (e := WORK.match(body)):
                kind = "softirq" if "action" in e.re.groupindex else "work"
                label = f"softirq {e['action']}" if kind == "softirq" else f"work {e['function']}"
                if e["edge"] in ("entry", "start"):
                    begun[cpu, kind] = (ts, label)
                elif (b := begun.pop((cpu, kind), None)) is not None and b[1] == label:
                    if pid in window:
                        charge(window[pid]["on"], label, ts - b[0])
                    elif (p := pre.get(cpu)) in window:
                        charge(window[p]["what"], label, ts - b[0])
                continue
            if (e := RAW_ENTER.match(body)):
                nr = int(e["nr"])
                w = window.get(pid)
                if w is not None and nr in ends:
                    gap(w, (nr, None), ts)
                    w["end"], w["ends"] = ts, name((nr, None))
                    creates.append(window.pop(pid))
                    continue
                args = [int(a, 16) for a in e["args"].replace(" ", "").split(",") if a]
                if (path == "classic" and nr == NR_MOUNT and pid in born and pid not in window
                        and len(args) > 3 and args[3] == MS_REC_PRIVATE):
                    window[pid] = {"start": ts, "opens": "mount (private)", "calls": [], "attaches": [],
                                   "user_run": 0, "user_blocked": 0, "faults": 0, "by": {}, "what": {}, "on": {},
                                   "last": ts, "pending": 0, "gaps": {}}
                if (w := window.get(pid)) is not None:
                    gap(w, (nr, kind_of(nr, args)), ts)
                cur[pid] = {"nr": nr, "t": ts, "args": args, "run": 0, "blocked": 0}
            elif (e := RAW_EXIT.match(body)):
                nr, c = int(e["nr"]), cur.pop(pid, None)
                if (path == "classic" and c is None and nr in (NR_CLONE, NR_CLONE3) and int(e["ret"]) == 0
                        and pid in born and pid not in window):
                    window[pid] = {"start": ts, "opens": "first run", "calls": [], "attaches": [], "user_run": 0,
                                   "user_blocked": 0, "faults": 0, "by": {}, "what": {}, "on": {},
                                   "last": ts, "pending": 0, "gaps": {}}
                    continue
                if c is None or c["nr"] != nr:
                    continue
                w = window.get(pid)
                if w is not None:
                    w["last"] = ts
                    w["calls"].append(((nr, kind_of(nr, c["args"])), ts - c["t"], c["run"], c["blocked"]))
                    if nr in ATTACHES:
                        w["attaches"].append(len(w["calls"]))
                if (span == "root" and w is None and nr == NR_SETNS and int(e["ret"]) == 0 and len(c["args"]) > 1
                        and c["args"][1] & CLONE_NEWNS):
                    joins[pid] = ((nr, "mnt"), ts - c["t"], c["run"], c["blocked"])
                if nr == NR_SETNS and int(e["ret"]) == 0 and len(c["args"]) > 1 and c["args"][1] & CLONE_NEWNET:
                    window[pid] = {"start": ts, "opens": "setns", "calls": [], "attaches": [], "user_run": 0,
                                   "user_blocked": 0, "faults": 0, "by": {}, "what": {}, "on": {},
                                   "last": ts, "pending": 0, "gaps": {}, "join": 0}
                    if (j := joins.pop(pid, None)) is not None:
                        window[pid]["calls"].append(j)
                        window[pid]["join"] = j[1]
            elif (e := SWITCH.match(body)):
                # Off the CPU runnable (prev_state R, preempted) or not (blocked: waiting on a lock or I/O). A
                # preemption is charged to the process the CPU went to first, by its name without a kernel
                # thread's per-CPU suffix (kworker/u64:3 is a kworker), so a tail shows whose work it waited on.
                prev, nxt = int(e["prev"]), int(e["next"])
                if prev in window:
                    off[prev] = (ts, "run" if e["state"].startswith("R") else "blocked", e["next_comm"].split("/")[0])
                    if e["state"].startswith("R"):
                        pre[cpu] = prev
                if nxt in off:
                    for c in [c for c, p in pre.items() if p == nxt]:
                        del pre[c]
                    t0, how, who = off.pop(nxt)
                    if nxt in cur:
                        cur[nxt][how] += ts - t0
                    elif nxt in window:
                        window[nxt]["user_" + how] += ts - t0
                    if how == "run" and nxt in window:
                        charge(window[nxt]["by"], who, ts - t0)
                    if how == "blocked" and nxt in window:
                        c = cur.get(nxt)
                        at = name((c["nr"], kind_of(c["nr"], c["args"]))) if c else "between calls"
                        lock = waits.pop(nxt, None)
                        label = (f"{at}, woken by {woke.pop(nxt, 'nothing traced')}"
                                 + (", waiting for another rwsem (write)" if lock == "another rwsem"
                                    else f", waiting for the mount lock ({lock})" if lock else ""))
                        charge(window[nxt].setdefault("blk", {}), label, ts - t0)
            elif (e := WAKING.match(body)):
                woken = int(e["pid"])
                if woken in off and off[woken][1] == "blocked":
                    woke[woken] = "another init" if pid in window else m["comm"].split("/")[0]
            elif RWW.match(body):
                if pid in window:
                    waits[pid] = "another rwsem"
                    window[pid]["rwsem_waits"] = window[pid].get("rwsem_waits", 0) + 1
            elif (e := NSWAIT.match(body)):
                if e["kind"] == "nsww" and (h := holding.get(pid)) is not None:
                    h[0] = None  # it waits: its hold starts at the slowpath's return
                if pid in window:
                    waits[pid] = "write" if e["kind"] == "nsww" else "shared"
                    window[pid]["lock_waits"] = window[pid].get("lock_waits", 0) + 1
            elif (e := FORK.match(body)):
                born.add(int(e["child"]))
            elif body.startswith("page_fault_user:"):
                if pid in window:
                    window[pid]["faults"] += 1
                    window[pid]["pending"] += 1
            elif (e := NSSEM.match(body)):
                probed = True
                if e["kind"] == "nsw":
                    c = cur.get(pid)
                    holding[pid] = [ts, "an init" if pid in window else m["comm"].split("/")[0], 0, 0,
                                    name((c["nr"], kind_of(c["nr"], c["args"]))) if c else "?"]
                if (w := window.get(pid)) is not None:
                    c = cur.get(pid)
                    at = name((c["nr"], kind_of(c["nr"], c["args"]))) if c else "between calls"
                    held = w.setdefault("holds", {}).setdefault(e["kind"], {})
                    held[at] = held.get(at, 0) + 1
                elif e["kind"] == "nsw":
                    # Outside every span: the pools' maker, the node, an init before or after its part 19 (goal 6,
                    # goal6-mntns.md §10). A span's wait is behind these as much as behind another span's holds.
                    charge(outside, m["comm"].split("/")[0], 1)
            else:
                unread += 1
    n_lost = eb.lost(stats)
    if not creates:
        print("refusing: no part 19 found (no init joined a network namespace made ahead and then switched)",
              file=sys.stderr)
        return 1
    for w in creates:
        w["span"] = w["end"] - w["start"] + w.get("join", 0)
        w["sys"] = sum(c[1] for c in w["calls"])
        w["sys_run"] = sum(c[2] for c in w["calls"])
        w["sys_blocked"] = sum(c[3] for c in w["calls"])
        w["user"] = w["span"] - w["sys"]
        w["shares"], last = [], 0
        for b in w["attaches"] + [len(w["calls"])]:
            seg = w["calls"][last:b]
            w["shares"].append(([c[0] for c in seg], sum(c[1] for c in seg)))
            last = b
    spans = sorted(w["span"] for w in creates)
    cut = spans[max(0, len(spans) - max(1, len(spans) // 100))]
    tail = [w for w in creates if w["span"] >= cut]

    def holds(ws, kind):
        """The mount lock's holds per create of `kind` (nsw for write, nsr shared), and by the call each was in."""
        tot = {}
        for w in ws:
            for at, k in w.get("holds", {}).get(kind, {}).items():
                tot[at] = tot.get(at, 0) + k
        by = ", ".join(f"{at} {k / len(ws):.1f}" for at, k in sorted(tot.items(), key=lambda kv: -kv[1]))
        return sum(tot.values()) / len(ws), by

    def table(ws):
        n, per = len(ws), {}
        for w in ws:
            for call, d, run, blocked in w["calls"]:
                p = per.setdefault(call, [0, 0, 0, 0])
                p[0] += 1
                p[1] += d
                p[2] += run
                p[3] += blocked
        mean = lambda key: sum(w[key] for w in ws) / n
        def top(key, k=8):
            tot = {}
            for w in ws:
                for label, d in w[key].items():
                    tot[label] = tot.get(label, 0) + d
            return ", ".join(f"{label} {d / n:.1f}" for label, d in sorted(tot.items(), key=lambda kv: -kv[1])[:k])
        for w in ws:
            w.setdefault("blk", {})
        out = ["| call | calls per create | us per create | us per call | off the CPU in it, us per create: "
               "preempted / blocked |", "|---|---|---|---|---|"]
        for call, (k, d, run, blocked) in sorted(per.items(), key=lambda kv: -kv[1][1]):
            out.append(f"| {name(call)} | {k / n:.1f} | {d / n:.1f} | {d / k:.2f} | {run / n:.1f} / "
                       f"{blocked / n:.1f} |")
        return out + ["", f"- Mean span {mean('span'):.1f} us: in calls {mean('sys'):.1f} (off the CPU in them "
                          f"{mean('sys_run'):.1f} preempted, {mean('sys_blocked'):.1f} blocked), the init's own "
                          f"code between them {mean('user'):.1f} (off the CPU there {mean('user_run'):.1f} "
                          f"preempted, {mean('user_blocked'):.1f} blocked); "
                          f"{sum(len(w['calls']) for w in ws) / n:.1f} calls and {mean('faults'):.1f} page "
                          f"faults in user mode per create.",
                     f"- Preempted, the CPU went first to (us per create): {top('by') or 'none'}; while it waited, "
                     f"its CPU ran (us per create): {top('what') or 'nothing traced'}.",
                     f"- Softirqs run on the init itself as an interrupt returned (us per create): "
                     f"{top('on') or 'none'}.",
                     f"- Blocked, by the call it slept in, what woke it and whether it waited for the mount lock "
                     f"(us per create): {top('blk') or 'none'}; mount lock waits (its rwsem's slowpath) per create "
                     f"{sum(w.get('lock_waits', 0) for w in ws) / n:.1f}.",
                     (f"- The mount lock (namespace_sem) per create: {holds(ws, 'nsw')[0]:.1f} held for write "
                      f"(by call: {holds(ws, 'nsw')[1] or 'none'}), {holds(ws, 'nsr')[0]:.1f} shared "
                      f"(by call: {holds(ws, 'nsr')[1] or 'none'})." if probed
                      else "- The mount lock's holds weren't traced (no namespace_sem probe).")]

    def gaps(ws):
        n, per = len(ws), {}
        for w in ws:
            for call, (d, faults) in w["gaps"].items():
                p = per.setdefault(call, [0, 0])
                p[0] += d
                p[1] += faults
        out = ["| the init's code before | us per create | page faults per create |", "|---|---|---|"]
        for call, (d, faults) in sorted(per.items(), key=lambda kv: -kv[1][0])[:16]:
            out.append(f"| {name(call)} | {d / n:.1f} | {faults / n:.1f} |")
        return out

    def shares(ws):
        most = max(len(w["shares"]) for w in ws)
        out = ["| share | creates | us in its calls per create | its calls, in order (the first create's) |",
               "|---|---|---|---|"]
        for k in range(most):
            have = [w for w in ws if len(w["shares"]) > k]
            label = f"to attach {k + 1}" if k < most - 1 else "after the last attach"
            calls = have[0]["shares"][k][0]
            listed = ", ".join(name(c) for c in calls[:48]) + (" ..." if len(calls) > 48 else "")
            out.append(f"| {label} | {len(have)} | {sum(w['shares'][k][1] for w in have) / len(have):.1f} "
                       f"| {listed} |")
        return out

    def each(ws):
        """Each create of `ws`, slowest first: its span, its calls' time and time off the CPU, its longest calls."""
        out = ["| span, us | in calls | preempted / blocked | its longest calls, us (preempted / blocked in them) |",
               "|---|---|---|---|"]
        for w in sorted(ws, key=lambda w: -w["span"]):
            longest = sorted(w["calls"], key=lambda c: -c[1])[:4]
            out.append(f"| {w['span']} | {w['sys']} | {w['sys_run'] + w['user_run']} / "
                       f"{w['sys_blocked'] + w['user_blocked']} | "
                       + ", ".join(f"{name(c[0])} {c[1]} ({c[2]} / {c[3]})" for c in longest) + " |")
        return out

    print("\n".join(
        [f"# Part 19, call by call ({os.path.basename(trace)})", "",
         f"{len(creates)} creates, each from its init's "
         f"{' or '.join(sorted({OPENS[w['opens']] for w in creates}))}"
         f" to its {' or '.join(sorted({w['ends'] for w in creates}))}: span p50 {eb.pct(spans, 0.5)} us, "
         f"p99 {eb.pct(spans, 0.99)}, max {spans[-1]}. {read} events read, {unread} lines not read as events, "
         f"{n_lost} lost.", "", "## Every create", ""]
        + table(creates) + ["", "## The init's own code between calls, by the call it leads to (every create)", ""]
        + gaps(creates) + ["", "## Each mount's share (every create)", ""] + shares(creates)
        + ["", f"## The slowest 1% ({len(tail)} creates, span {cut} us or more)", ""] + table(tail)
        + ["", "### Each of them", ""] + each(tail)))
    n = len(creates)
    with open(os.path.splitext(trace)[0] + ".summary.json", "w") as f:
        timed = {}
        for _, who, us, p_us, b_us, _ in hold_times:
            timed.setdefault(who, []).append((us, p_us, b_us))
        holders = {}
        for who, hs in timed.items():
            seen = who in HEX_COMMS or who == "an init"
            holders[who] = {"holds": len(hs), "mean_us": round(sum(h[0] for h in hs) / len(hs), 1),
                            "max_us": max(h[0] for h in hs),
                            "preempted_us_per_hold": round(sum(h[1] for h in hs) / len(hs), 1) if seen else None,
                            "blocked_us_per_hold": round(sum(h[2] for h in hs) / len(hs), 1) if seen else None}
        if hold_times:
            print("\n## The mount lock's write holds, by holder\n")
            print("Each timed from its grant (down_write, or the write slowpath's return) to its up_write; off the CPU "
                  "inside it only for Hex's processes, whose switches are traced.\n")
            print("| holder | holds | mean us | max us | preempted in it, us per hold | blocked in it, us per hold |")
            print("|---|---|---|---|---|---|")
            for who, v in sorted(holders.items(), key=lambda kv: -kv[1]["holds"] * kv[1]["mean_us"]):
                fmt = lambda x: "untraced" if x is None else f"{x:.1f}"
                print(f"| {who} | {v['holds']} | {v['mean_us']:.1f} | {v['max_us']} | "
                      f"{fmt(v['preempted_us_per_hold'])} | {fmt(v['blocked_us_per_hold'])} |")
        by_holder = ", ".join(f"{c} {k / n:.1f}" for c, k in sorted(outside.items(), key=lambda kv: (-kv[1], kv[0])))
        if probed:
            print("\n## Outside the spans\n")
            print(f"- The mount lock held for write outside the spans, per create, by holder: {by_holder or 'none'}.")
        # Part 19's tail without the CPU taken from it (goal6-mntns.md §11): the creates preempted nowhere in it.
        unpreempted = sorted(w["span"] for w in creates if w["sys_run"] + w["user_run"] == 0)
        longest = [[us, who, call] for _, who, us, _, _, call in
                   sorted(hold_times, key=lambda h: -h[2])[:5]]
        json.dump({"creates": n, "span_p50_us": eb.pct(spans, 0.5), "span_p99_us": eb.pct(spans, 0.99),
                   "creates_unpreempted": len(unpreempted),
                   "span_unpreempted_p99_us": eb.pct(unpreempted, 0.99) if unpreempted else None,
                   "nssem_longest_holds": longest if hold_times else None,
                   "span_max_us": spans[-1], "lost": n_lost, "unread": unread,
                   "calls_per_create": sum(len(w["calls"]) for w in creates) / n,
                   "in_calls_us_per_create": sum(w["sys"] for w in creates) / n,
                   "between_calls_us_per_create": sum(w["user"] for w in creates) / n,
                   "faults_per_create": sum(w["faults"] for w in creates) / n,
                   "nssem_write_per_create": holds(creates, "nsw")[0] if probed else None,
                   "nssem_write_by_call": holds(creates, "nsw")[1] if probed else None,
                   "nssem_read_per_create": holds(creates, "nsr")[0] if probed else None,
                   "nssem_waits_per_create": sum(w.get("lock_waits", 0) for w in creates) / n,
                   "other_rwsem_waits_per_create": sum(w.get("rwsem_waits", 0) for w in creates) / n,
                   "nssem_write_outside_per_create": by_holder if probed else None,
                   "nssem_holds_by_holder": holders if hold_times else None,
                   # Each create's part 19: its start on the trace's clock (us), its span and its time preempted,
                   # so a run that interleaves arms in one trace cuts part 19 by each burst's window (rounds.py).
                   "spans": [[w["start"], w["span"], w["sys_run"] + w["user_run"]] for w in creates],
                   "span": span}, f, indent=1)
    if n_lost:
        print(f"refusing: the trace lost {n_lost} events", file=sys.stderr)
        return 1
    return 0


def selftest():
    """Synthetic in-memory traces for record_write_cuts (goal 5 extended step 5): a non-durable record's write
    (its file opened with O_EXCL, written and closed, no rename) that the old rename-only cutter refused, a
    durable record's write (temp file opened, written, closed, renamed), a write with neither — refused — and
    the close matched by its fd. `python3 create_breakdown.py test`."""
    bench, cid = 7, "b"

    def e(kind, name, text):
        return eb.Event(0, bench, kind, name, text)

    def cuts(events):
        c = eb.Cuts(events)
        c.record_kinds, c.cgroups = [], []
        return c

    # A non-durable record (O_EXCL): its open's kprobe entry and exit (fd 5), a write, the fd's close. No rename.
    after = [e("mark", "", "the cut the write follows"),
             e("probe", "openat", "openat_in: (args)"),
             e("exit", "openat", "0x5"),
             e("enter", "write", "fd: 0x5, count: 0x12"),
             e("enter", "close", "fd: 0x5"),
             e("exit", "close", "0x0")]
    # The old cutter cut a write only to a rename; this trace has none, so it refused the create.
    assert next((i for i in range(len(after)) if exited(bench, *RENAMES)(after[i])), None) is None
    c = cuts(after)
    record_write_cuts(c, after, bench, cid, 2, split=True)
    assert c.record_kinds == ["close"], c.record_kinds
    assert c.at == [0, 1, 2, 5, 5, 5, 5], c.at  # file made (1->2), written to the close (2->5), then empty

    # A durable record: open (fd 5), write, close, then the rename that puts it in place.
    before = after + [e("exit", "rename", "0x0")]
    c = cuts(before)
    record_write_cuts(c, before, bench, cid, 2, split=True)
    assert c.record_kinds == ["rename"], c.record_kinds
    assert c.at == [0, 1, 2, 6, 6, 6, 6], c.at  # written to the rename (2->6), then empty

    # Neither a matching close nor a rename: refused.
    neither = after[:3]
    c = cuts(neither)
    try:
        record_write_cuts(c, neither, bench, cid, 2, split=True)
    except Missing:
        pass
    else:
        raise AssertionError("a write with neither a close nor a rename must be refused")

    # The close is matched by its fd: a close of another fd before the record fd's is not the cut's end.
    other = [after[0], after[1], after[2], after[3],
             e("enter", "close", "fd: 0x9"), e("exit", "close", "0x0"),
             e("enter", "close", "fd: 0x5"), e("exit", "close", "0x0")]
    c = cuts(other)
    record_write_cuts(c, other, bench, cid, 2, split=True)
    assert c.at == [0, 1, 2, 7, 7, 7, 7], c.at  # the fd-5 close exit is at 7, not the fd-9 close at 5

    # A chosen cgroup (goal 5 extended step 6): the state directory and the FIFO first, then the owner's choice. Handed
    # on: no mkdir, the idle cgroup's files opened and closed, the made parts empty at the create's first pipe.
    state = e("enter", "mkdir", 'pathname: 0x1 "/run/hex-node/0123456789abcdef-b", mode: 0x1ff')
    fifo = e("enter", "mknodat", 'dfd: 0x1, filename: 0x2 "/run/hex-node/0123456789abcdef-b/exec.fifo", mode: 0x1192')
    reads = [e("exit", "openat", "0x15"), e("enter", "close", "fd: 0x15"), e("exit", "close", "0x0")]
    pipe = e("enter", "pipe2", "fildes: 0x3, flags: 0x80000")
    handed = [after[0], state, e("exit", "mkdir", "0x0"), fifo] + reads + [pipe]
    c = cuts(handed)
    chosen_cuts(c, handed, bench, cid)
    assert c.cgroups == ["handed on"], c.cgroups
    assert c.at == [0, 1, 3, 7, 7, 7, 7, 7], c.at
    # Fresh: a slot's mkdirat under the oci boxes' root, cut as the cgroup made.
    slot = [e("enter", "mkdirat", 'dfd: 0x10, pathname: 0x4 ".4", mode: 0x1ff'), e("exit", "mkdirat", "0x0")]
    fresh = [after[0], state, e("exit", "mkdir", "0x0"), fifo] + slot + [pipe]
    c = cuts(fresh)
    chosen_cuts(c, fresh, bench, cid)
    assert c.cgroups == ["fresh"], c.cgroups
    assert c.at == [0, 1, 3, 4, 5, 6, 6, 6], c.at
    # A mkdir that makes neither a slot nor the box's own cgroup is refused, as is a create whose first mkdir isn't
    # its state directory.
    for bad in ([after[0], state, e("exit", "mkdir", "0x0"), fifo,
                 e("enter", "mkdirat", 'dfd: 0x10, pathname: 0x4 ".04", mode: 0x1ff'), e("exit", "mkdirat", "0x0"), pipe],
                [after[0], slot[0], slot[1], state, e("exit", "mkdir", "0x0"), fifo, pipe]):
        try:
            chosen_cuts(cuts(bad), bad, bench, cid)
        except Missing:
            pass
        else:
            raise AssertionError("a chosen cgroup's create with a stray mkdir must be refused")

    print("create_breakdown selftest: ok (close-cut, rename-cut, fd match, refusal, and a chosen cgroup handed on, "
          "fresh, and refused)")
    return 0


def main():
    a = sys.argv[1:]
    if len(a) == 4 and a[0] == "drain":
        eb.drain(*a[1:])
        return 0
    if len(a) == 3 and a[0] == "check":
        return check(a[1], a[2])
    if len(a) == 2 and a[0] == "summarize":
        return summarize(a[1])
    if a and a[0] == "switch" and len(a) in (3, 4, 5):
        return switch(a[1], a[2], a[3] if len(a) >= 4 else None, a[4] if len(a) == 5 else "switch")
    if (a and a[0] == "mounts" and len(a) in (4, 5, 6) and (len(a) < 5 or a[4] in ("pruned", "classic"))
            and (len(a) < 6 or a[5] in ("switch", "root"))):
        return mounts(*a[1:])
    if len(a) == 1 and a[0] == "test":
        return selftest()
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
