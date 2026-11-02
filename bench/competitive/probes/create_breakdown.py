#!/usr/bin/env python3
"""bench/competitive/probes: the create breakdown's trace reader and summary (create_breakdown.md).

  create_breakdown.py drain <trace_pipe> <out file> <stop file>
  create_breakdown.py check <run dir> <prefix>
  create_breakdown.py summarize <run dir>
  create_breakdown.py test   (selftest: the record-write cutter over synthetic traces)

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
    ("the init's clone into its cgroup and new namespaces: pid, mount, ipc, uts (kernel), until it runs", INIT),
    ("the init's first steps, to its join of the namespace made ahead", INIT),
    ("the namespace made ahead joined (setns, kernel)", INIT),
    ("the init's root mounted: the rootfs bound, /usr, /proc and /dev, its device nodes made", INIT),
    ("pivot_root (kernel)", INIT),
    ("the old root unmounted; the hostname set; the process become (capabilities, user); ready sent", INIT),
    ("the agent wakes to the ready signal and reads it; the init's stat opened", HANDSHAKE),
    ("the init's start time read; the container's state written (state.json, renamed into place)", OWN),
] + CREATE_PARTS[28:]
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
                                                         ONCE_PARTS[1][0], "the cgroup made (kernel)")]


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
    to pivot_root. A seed the pool's own thread starts forks a child that execs instead, so it isn't taken."""
    for i in range(start, len(ev)):
        e = ev[i]
        if e.kind == "sched" and e.name == "fork" and e.pid != bench:
            child = int(e.kv["child_pid"])
            if any(enter(child, "pivot_root")(x) for x in ev[i + 1:]):
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


def create_cuts(ev, bench, cid, seeded=False, once=False):
    """A create's cuts on the shim path, or with `seeded` on the seed path (goal 5 plan step 4), and with `once` as
    well where the record is written once, last (goal 5 extended step 5)."""
    c = eb.Cuts(ev)
    c.record_kinds = []  # "rename" (a durable record) or "close" (O_EXCL, goal 5 extended step 5), per write
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
    made = c.to(enter(bench, "mkdir", "mkdirat"), "the cgroup's mkdir")
    held(ev[made].path().startswith("/sys/fs/cgroup/") and named(ev[made].path(), cid),
         f"the first mkdir is the box's cgroup (it made {ev[made].path()!r})")
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
        # A seed opens the box's cgroup and clones the init into it and the namespaces the spec makes; the init
        # joins the network namespace made ahead itself.
        born, seed, init = seed_fork(ev, c.at[-1], bench)
        clone = last_before(ev, c.at[-1] + 1, born, enter(seed, "clone3"), "the seed's clone of the init")
        opened = last_before(ev, c.at[-1] + 1, clone, exited(seed, "open"), "the seed's open of the box's cgroup")
        c.at += [opened, clone]
        c.opens.append((opened, seed, "open", lambda p: named(p, cid), "the seed's open of the box's cgroup"))
        c.to(exited(init, "clone3"), "the init's first run")
        joins = c.to(enter(init, "setns"), "the init's join of the namespace made ahead")
        held(arg(ev[joins], "flags") == CLONE_NEWNET, "the init's setns joins a network namespace")
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
ONCE_OPS = {"create": (ONCE_PARTS, lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True, once=True))}


def ops_for(trace):
    """The op table for a trace: the seed path's when no create forked from the agent itself (a seed cloned its
    init, goal 5 plan step 4), else the shim path's, read from the trace's first traced create; on the seed path,
    the record-once path's when the agent renamed nothing into place before its first mkdir, the box's cgroup (goal
    5 extended step 5)."""
    ev, _ = eb.parse(trace)
    begin = next((i for i, e in enumerate(ev) if e.kind == "mark" and e.text.split()[:3] == ["hexbd", "B", "create"]),
                 None)
    if begin is None:
        return OPS
    bench = ev[begin].pid
    renamed, made = False, False
    for e in ev[begin + 1:]:
        if e.kind == "mark" and e.pid == bench and e.text.split()[:2] == ["hexbd", "E"]:
            return SEED_OPS if renamed else ONCE_OPS
        if e.kind == "sched" and e.name == "fork" and e.pid == bench:
            return OPS
        if e.pid == bench and not made:
            renamed |= e.kind == "exit" and e.name in RENAMES
            made = e.kind == "enter" and e.name in ("mkdir", "mkdirat")
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
            kinds = {}
            for c in got:
                for k in c.get("record_kinds", []):
                    kinds[k] = kinds.get(k, 0) + 1
            print(f"check {prefix}.{kind}: {sum('parts' in c for c in got)} of {len(got)} traced creates cut whole, "
                  f"{unread} trace lines not read as events; record writes by kind {kinds}")
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
            for kind in ("libcrun", "seam", "before") if kind != "before" or eb.rounds(run, kind)}
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
        root = sum(parts[k]["p50"] for k in named_parts(spec, SEED_PARTS[18][0], "pivot_root (kernel)"))
        pivot = sum(parts[k]["p50"] for k in named_parts(spec, "pivot_root (kernel)"))
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
                  f"| | of it, pivot_root | {pivot:.3f} | {share(pivot)} |",
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
    if whole and spec is ONCE_PARTS:
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
        c.record_kinds = []
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

    print("create_breakdown selftest: ok (close-cut, rename-cut, fd match, and refusal)")
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
    if len(a) == 1 and a[0] == "test":
        return selftest()
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
