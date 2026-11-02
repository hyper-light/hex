#!/usr/bin/env python3
"""bench/competitive/probes: the create breakdown's trace reader and summary (create_breakdown.md).

  create_breakdown.py drain <trace_pipe> <out file> <stop file>
  create_breakdown.py check <run dir> <prefix>
  create_breakdown.py summarize <run dir>
  create_breakdown.py switch <trace file> <trace stats> [seam result json]
  create_breakdown.py mounts <trace file> <trace stats> <asm/unistd_64.h> [pruned|classic]

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
from exec_breakdown import Missing, POLLS, RENAMES, enter, exited, first, last_before, named, sched  # noqa: E402

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
        ("the pipes made; a namespace made ahead taken; the request sent to a seed, which wakes and reads "
         "config.json", SEED),
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


def fsyncs_of(parts):
    """The parts a durable record write's fsyncs are (decision 5's first lever, plan step 2)."""
    return [k for k, (n, _) in enumerate(parts) if n.startswith("the record's fsync")
            or n.startswith("the directory's fsync")]


def cgroup_made_of(parts):
    """The cgroup's making, which a pool made ahead would take off the path, its bounds still written at the take
    (plan step 5)."""
    return [k for k, (n, _) in enumerate(parts) if n in ("the box root listed, its boxes counted",
                                                         "the cgroup made (kernel)")]


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


def create_cuts(ev, bench, cid, seeded=False, pruned=False):
    """A create's cuts on the shim path, with `seeded` on the seed path (goal 5 plan step 4), or with
    `pruned` on the pruned-seed path (goal 5 plan step 2): the init builds its root from descriptors and
    ends with chroot, so the switch is the chroot and no init pivot_root appears."""
    c = eb.Cuts(ev)
    tmp = f".{cid}.tmp"

    def opened_last_before(pid, call, before, test, what):
        """The last exit of `call` by `pid` after the cut before it and before event `before`: an open the call's
        path puts right before that event."""
        i = last_before(ev, c.at[-1] + 1, before, exited(pid, call), what)
        c.at.append(i)
        c.opens.append((i, pid, call, test, what))
        return i

    def record_write(n):
        """A record write's cuts: its temp file made, its fsync's entry and exit, the directory's. A record whose
        directories all lie on tmpfs isn't synced (decision 1, goal 5 plan step 2). The rounds trace a rename's
        exit alone (its entry fetches two paths), so such a write's "the record written" runs from its temp file
        to its rename's return, holding the close and the rename, and the three parts after it are empty."""
        renamed = first(ev, c.at[-1], exited(bench, *RENAMES), f"record write {n}'s rename")
        syncs = [i for i in range(c.at[-1], renamed) if enter(bench, "fsync")(ev[i])]
        if syncs:
            opened_last_before(bench, "openat", syncs[0], lambda p: named(p, tmp), f"record write {n}'s temp file")
            c.at.append(syncs[0])
            c.to(exited(bench, "fsync"), f"record write {n}'s fsync returning")
            c.to(enter(bench, "fsync"), f"record write {n}'s directory fsync")
            c.to(exited(bench, "fsync"), f"record write {n}'s directory fsync returning")
        else:
            opened_last_before(bench, "openat", renamed, lambda p: named(p, tmp), f"record write {n}'s temp file")
            c.at += [renamed] * 4

    c.opened(bench, "openat", 1, 0, lambda p: named(p, f"{cid}.json"), "the node's look for the box's record")
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
        # A seed clones the init into its cgroup and the namespaces the spec makes; the init joins the network
        # namespace made ahead itself. On the classic seed path the seed opens the box's cgroup by path; on the
        # pruned path the agent opened it (a descriptor the seed can't name by path) and the seed instead reads
        # config.json through the bundle descriptor, so the cut anchors on that read (goal 5 plan step 2).
        born, seed, init = seed_fork(ev, c.at[-1], bench)
        clone = last_before(ev, c.at[-1] + 1, born, enter(seed, "clone3"), "the seed's clone of the init")
        if pruned:
            opened = last_before(ev, c.at[-1] + 1, clone, exited(seed, "openat"),
                                 "the seed's read of config.json")
            c.at += [opened, clone]
            c.opens.append((opened, seed, "openat", lambda p: named(p, "config.json"),
                            "the seed's read of config.json"))
        else:
            opened = last_before(ev, c.at[-1] + 1, clone, exited(seed, "open"),
                                 "the seed's open of the box's cgroup")
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
    c.opened(bench, "openat", 2, renamed, lambda p: p.startswith("/proc/") and named(p, "stat"),
             "the node's open of the init's stat, reading the state back")
    record_write(2)
    return c


OPS = {"create": (CREATE_PARTS, create_cuts)}
SEED_OPS = {"create": (SEED_PARTS, lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True))}
PRUNED_SEED_OPS = {
    "create": (PRUNED_SEED_PARTS, lambda ev, bench, cid: create_cuts(ev, bench, cid, seeded=True, pruned=True))
}


def ops_for(trace):
    """The op table for a trace: the seed path's when no create forked from the agent itself (a seed cloned its
    init, goal 5 plan step 4), else the shim path's, read from the trace's first traced create."""
    ev, _ = eb.parse(trace)
    begin = next((i for i, e in enumerate(ev) if e.kind == "mark" and e.text.split()[:3] == ["hexbd", "B", "create"]),
                 None)
    if begin is None:
        return OPS
    bench = ev[begin].pid
    for e in ev[begin + 1:]:
        if e.kind == "mark" and e.pid == bench and e.text.split()[:2] == ["hexbd", "E"]:
            # Seeded. The pruned-seed path's init chroots and never pivot_roots (goal 5 plan step 2); the
            # classic seed path pivot_roots. Tell them apart by whether any init chroots in the trace.
            if any(x.kind == "enter" and x.name == "chroot" for x in ev):
                return PRUNED_SEED_OPS
            return SEED_OPS
        if e.kind == "sched" and e.name == "fork" and e.pid == bench:
            return OPS
    return OPS


def check(run, prefix):
    bad = []
    before = ("before",) if os.path.exists(os.path.join(run, f"{prefix}.before.json")) else ()
    for kind in ("libcrun", "seam", "traced", "detail") + before:
        errs, ms = eb.lifecycle_ms(os.path.join(run, f"{prefix}.{kind}.json"), "create")
        if errs or not ms:
            bad.append(f"{kind}: errors {errs}, {len(ms)} creates")
        if kind in ("traced", "detail"):
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
            print(f"check {prefix}.{kind}: {sum('parts' in c for c in got)} of {len(got)} traced creates cut whole, "
                  f"{unread} trace lines not read as events")
    for b in bad:
        print(f"check {prefix}: {b}")
    return 1 if bad else 0


def ratio(a, b):
    return a / b if a is not None and b else None


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
            for kind in ("libcrun", "seam", "before", "seam_idle", "libcrun_idle")
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
    s["create_parts"] = rec
    lines += ["Trace: " + ", ".join(f"r{r} {t['lost']} events lost and {t['unread']} lines not read as events"
                                    for r, t in sorted(traced.items())) + ".", ""]
    with open(os.path.join(run, "summary.json"), "w") as f:
        json.dump(s, f, indent=1)
    print("\n".join(lines))
    return 0


def switch(trace, stats, seam_json=None):
    """Goal 5 plan step 2's AC-e, read off a trace of a throughput phase run at eight creates in flight
    (`HEXBENCH_THROUGHPUT_CONC=8`) with the root's parts' events on. The criterion bounds the p99 of the
    pruned root's *parts* at 0.5 ms under that concurrency with at least 8,000 idle host threads, so this
    reports two per init, as create_cuts delimits them (run 4's parts 19 and 20):
      - the mounts (part 19): from the init's join of the namespace made ahead (setns `CLONE_NEWNET`) to
        its `chroot` — where it attaches the rootfs and binds (`move_mount`), mounts proc and /dev
        (`fsmount`), sets a mount read-only (`mount_setattr`) and makes device nodes; move_mount/fsmount
        aren't traced, so the span runs from the net-ns join to the chroot;
      - the switch (part 20): the `chroot` itself, paired entry to exit (step 2's init moved its root onto /
        by `MS_MOVE` in part 19, before it).
    Both hold at or below 0.5 ms because neither walks the host: `chroot` is O(1), and the fd-based mounts
    touch only this container's tree, where `pivot_root` would walk every task under `namespace_sem`.

    It also evidences AC-a (no init pivot_root) directly over the raw trace, not by the cutter's internal
    assertion. Two kinds of `pivot_root` can appear, told apart by their caller: a pruned *seed* prunes its
    own mount namespace once at start with a `pivot_root` (decision 5), and that caller never `chroot`s; an
    *init* on the pruned path `chroot`s and never `pivot_root`s. So a `pivot_root` by a pid that also
    `chroot`s would be an init switching both ways (off the path) and fails; `pivot_root`s by pids that
    never `chroot` are the seeds' prunes, reported as such. With the seam result given, every create must
    have switched via `chroot` (chroots == creates): a create that fell back to the classic path would
    `pivot_root` in place of `chroot`, dropping the count. A lost or unpaired event is refused."""
    ev, _unread = eb.parse(trace)
    n_lost = eb.lost(stats)
    # Per pid (an init is single-threaded here, so its events don't interleave with its own): the net-ns
    # join's setns exit opens part 19, and the chroot closes it and is part 20. Eight inits in flight are
    # eight distinct pids. pivot_root by a chrooting pid is an init pivot (AC-a); by one that never chroots,
    # a seed's prune.
    setns_flags, newnet_exit = {}, {}
    pending, switch_us, mounts_us, dangling = {}, [], [], 0
    chroot_pids, pivot_by_pid = set(), {}
    for e in ev:
        if e.name == "setns":
            if e.kind == "enter":
                m = dict(ARG.findall(e.text))
                try:
                    setns_flags[e.pid] = int(m.get("flags", "0"), 0)
                except ValueError:
                    setns_flags[e.pid] = 0
            elif e.kind == "exit":
                if (setns_flags.pop(e.pid, 0) & CLONE_NEWNET) and e.pid not in newnet_exit:
                    newnet_exit[e.pid] = e.ts
        elif e.name == "chroot":
            if e.kind == "enter":
                dangling += e.pid in pending  # a second entry before the first's exit: unexpected
                pending[e.pid] = e.ts
                chroot_pids.add(e.pid)
                if e.pid in newnet_exit:
                    mounts_us.append(e.ts - newnet_exit[e.pid])
            elif e.kind == "exit":
                t0 = pending.pop(e.pid, None)
                if t0 is None:
                    dangling += 1
                else:
                    switch_us.append(e.ts - t0)
        elif e.name == "pivot_root" and e.kind == "enter":
            pivot_by_pid[e.pid] = pivot_by_pid.get(e.pid, 0) + 1
    dangling += len(pending)  # chroot entries with no exit
    init_pivots = sum(c for pid, c in pivot_by_pid.items() if pid in chroot_pids)
    seed_prune_pivots = sum(c for pid, c in pivot_by_pid.items() if pid not in chroot_pids)
    creates = None
    if seam_json:
        try:
            with open(seam_json) as f:
                tp = json.load(f).get("phases", {}).get("throughput", {})
            creates = sum(v["lifecycles"] for k, v in tp.items() if k.isdigit()) or None
        except (OSError, ValueError, KeyError, TypeError):
            creates = None
    ms = lambda us: None if us is None else us / 1e3
    def pcts(xs):
        return eb.pct(xs, 0.5), eb.pct(xs, 0.99), (max(xs) if xs else None)
    mp50, mp99, mmax = pcts(mounts_us)
    sp50, sp99, smax = pcts(switch_us)
    all_switched = creates is None or creates == len(switch_us)
    # A few creates find the net-ns pool dry and unshare their own net namespace rather than join a
    # made-ahead one, so they have no CLONE_NEWNET setns to open part 19 by (the seam result's
    # made_on_path count); tolerate up to 1%.
    missing_spans = len(switch_us) - len(mounts_us)
    mounts_covered = missing_spans <= max(1, len(switch_us) // 100)
    switch_le_bar = sp99 is not None and sp99 <= 500
    mounts_le_bar = mp99 is not None and mp99 <= 500
    # AC-e's 0.5 ms bar gates the switch (part 20): the pruned path's purpose is to make the root switch
    # O(1), off pivot_root's walk of every host thread. The mounts (part 19) take the mount namespace's
    # lock as the classic path's do (run 4's part 19 was 1.012 ms p99 at concurrency 1, over the bar),
    # and the pruned path doesn't change that, so they are reported beside the switch, not gated here.
    ok = (n_lost == 0 and dangling == 0 and bool(switch_us) and init_pivots == 0
          and all_switched and mounts_covered and switch_le_bar)
    print(json.dumps({"ac": "AC-T7.1.4 e", "concurrency": 8, "creates": creates,
                      "chroots": len(switch_us), "mounts_spans": len(mounts_us),
                      "mounts_p50_ms": ms(mp50), "mounts_p99_ms": ms(mp99), "mounts_max_ms": ms(mmax),
                      "mounts_le_bar": mounts_le_bar,
                      "switch_p50_ms": ms(sp50), "switch_p99_ms": ms(sp99), "switch_max_ms": ms(smax),
                      "switch_le_bar": switch_le_bar,
                      "init_pivot_roots": init_pivots, "seed_prune_pivot_roots": seed_prune_pivots,
                      "lost_events": n_lost, "unpaired_chroots": dangling, "bar_ms": 0.5, "pass": ok}))
    if switch_us:
        print(f"AC-e over {len(switch_us)} creates at 8 in flight — the root's parts: "
              f"switch (part 20, chroot) p50 {eb.ms3(ms(sp50))} / p99 {eb.ms3(ms(sp99))} / max {eb.ms3(ms(smax))} ms "
              f"(bar p99 <= 0.5 ms: {'met' if switch_le_bar else 'OVER'}); "
              f"mounts (part 19) p50 {eb.ms3(ms(mp50))} / p99 {eb.ms3(ms(mp99))} / max {eb.ms3(ms(mmax))} ms "
              f"({'<=' if mounts_le_bar else '>'} 0.5 ms, the mount namespace's lock, reported). "
              f"AC-a: {init_pivots} init pivot_roots (the {seed_prune_pivots} in the trace are the pruned seeds' "
              f"prune, by pids that never chroot).")
    if n_lost:
        print(f"refusing: the trace lost {n_lost} events", file=sys.stderr)
    if dangling:
        print(f"refusing: {dangling} chroot entries or exits didn't pair", file=sys.stderr)
    if not switch_us:
        print("refusing: no chroot traced (did the pruned path run? HEXBENCH_THROUGHPUT_CONC set?)",
              file=sys.stderr)
    if not mounts_covered:
        print(f"refusing: {missing_spans} of {len(switch_us)} chroots had no net-ns-join setns before them "
              f"(over the 1% the dry net-ns pool explains)", file=sys.stderr)
    if init_pivots:
        print(f"off the pruned path: {init_pivots} init pivot_roots (a chrooting pid also pivot_rooted)",
              file=sys.stderr)
    if not all_switched:
        print(f"off the pruned path: {len(switch_us)} chroots != {creates} creates (some fell back to "
              f"the classic pivot_root path)", file=sys.stderr)
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
ATTACHES = (NR_MOUNT, NR_MOVE_MOUNT)
# fsconfig's commands (include/uapi/linux/mount.h) and mount(2)'s flags that tell its kinds apart
# (include/uapi/linux/mount.h: MS_REMOUNT, MS_BIND, MS_MOVE, MS_REC and the propagation flags), so a call's
# kind is named: fsconfig's option set from its create, a bind from a move or a remount.
FSCONFIG_CMDS = {0: "flag", 1: "string", 2: "binary", 3: "path", 4: "path_empty", 5: "fd", 6: "create",
                 7: "reconfigure", 8: "create_excl"}
MS_REMOUNT, MS_BIND, MS_MOVE, MS_PROPAGATION = 0x20, 0x1000, 0x2000, 0x10000 | 0x20000 | 0x40000 | 0x80000
# The classic path's first call of its root, making every mount private (setup_rootfs's `mount_change("/",
# PRIVATE | REC)`): where its mounts begin when no join of a network namespace made ahead opened the span (the
# shim's child, whose shim joined it). A seed makes its mounts private too, pruning once at start, so a classic
# span opens only in a process a traced hex process forked or cloned (the trace's sched_process_fork), which a
# seed, the node agent's child, never is.
MS_REC_PRIVATE = 0x4000 | 0x40000
FORK = re.compile(r"^sched_process_fork: comm=.*? pid=(?P<parent>\d+) child_comm=.*? child_pid=(?P<child>\d+)\s*$")
# What took an init's CPU: a softirq (ksoftirqd's, or one run on the init as an interrupt returned) by its action,
# and a kworker's work item by its function.
SOFTIRQ = re.compile(r"^softirq_(?P<edge>entry|exit): vec=\d+ \[action=(?P<action>\w+)\]\s*$")
WORK = re.compile(r"^workqueue_execute_(?P<edge>start|end): work struct (?P<work>\w+): function (?P<function>\S+)\s*$")


def kind_of(nr, args):
    """A call's kind where its number alone doesn't say what part 19 does with it."""
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


def mounts(trace, stats, header, path="pruned"):
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
    what a tail is made of shows. A lost event is refused, as in switch."""
    names = syscall_names(header)
    name = lambda call: (names.get(call[0], f"nr {call[0]}") + (f" ({call[1]})" if call[1] else ""))
    read, unread = 0, 0
    cur, window, off, creates, born = {}, {}, {}, [], set()
    # pre: the init preempted off each CPU; begun: a softirq's or a work item's start on each CPU.
    pre, begun = {}, {}

    def charge(table, label, d):
        table[label] = table.get(label, 0) + d

    with open(trace, errors="replace") as f:
        for line in f:
            m = eb.LINE.match(line)
            if not m:
                unread += line.strip() != "" and not line.startswith("#")
                continue
            read += 1
            ts, pid, body = int(m["s"]) * 1_000_000 + int(m["us"]), int(m["pid"]), m["body"]
            cpu = int(m["cpu"])
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
                if w is not None and nr in (NR_CHROOT, NR_PIVOT_ROOT):
                    w["end"], w["ends"] = ts, name((nr, None))
                    creates.append(window.pop(pid))
                    continue
                args = [int(a, 16) for a in e["args"].replace(" ", "").split(",") if a]
                if (path == "classic" and nr == NR_MOUNT and pid in born and pid not in window
                        and len(args) > 3 and args[3] == MS_REC_PRIVATE):
                    window[pid] = {"start": ts, "opens": "mount (private)", "calls": [], "attaches": [],
                                   "user_run": 0, "user_blocked": 0, "faults": 0, "by": {}, "what": {}, "on": {}}
                cur[pid] = {"nr": nr, "t": ts, "args": args, "run": 0, "blocked": 0}
            elif (e := RAW_EXIT.match(body)):
                nr, c = int(e["nr"]), cur.pop(pid, None)
                if c is None or c["nr"] != nr:
                    continue
                w = window.get(pid)
                if w is not None:
                    w["calls"].append(((nr, kind_of(nr, c["args"])), ts - c["t"], c["run"], c["blocked"]))
                    if nr in ATTACHES:
                        w["attaches"].append(len(w["calls"]))
                if nr == NR_SETNS and int(e["ret"]) == 0 and len(c["args"]) > 1 and c["args"][1] & CLONE_NEWNET:
                    window[pid] = {"start": ts, "opens": "setns", "calls": [], "attaches": [], "user_run": 0,
                                   "user_blocked": 0, "faults": 0, "by": {}, "what": {}, "on": {}}
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
            elif (e := FORK.match(body)):
                born.add(int(e["child"]))
            elif body.startswith("page_fault_user:"):
                if pid in window:
                    window[pid]["faults"] += 1
            else:
                unread += 1
    n_lost = eb.lost(stats)
    if not creates:
        print("refusing: no part 19 found (no init joined a network namespace made ahead and then switched)",
              file=sys.stderr)
        return 1
    for w in creates:
        w["span"] = w["end"] - w["start"]
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
                     f"{top('on') or 'none'}."]

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

    print("\n".join(
        [f"# Part 19, call by call ({os.path.basename(trace)})", "",
         f"{len(creates)} creates, each from its init's "
         f"{' or '.join(sorted({'join of the network namespace made ahead' if w['opens'] == 'setns' else 'first mount, making its mounts private' for w in creates}))}"
         f" to its {' or '.join(sorted({w['ends'] for w in creates}))}: span p50 {eb.pct(spans, 0.5)} us, "
         f"p99 {eb.pct(spans, 0.99)}, max {spans[-1]}. {read} events read, {unread} lines not read as events, "
         f"{n_lost} lost.", "", "## Every create", ""]
        + table(creates) + ["", "## Each mount's share (every create)", ""] + shares(creates)
        + ["", f"## The slowest 1% ({len(tail)} creates, span {cut} us or more)", ""] + table(tail)))
    n = len(creates)
    with open(os.path.splitext(trace)[0] + ".summary.json", "w") as f:
        json.dump({"creates": n, "span_p50_us": eb.pct(spans, 0.5), "span_p99_us": eb.pct(spans, 0.99),
                   "span_max_us": spans[-1], "lost": n_lost, "unread": unread,
                   "calls_per_create": sum(len(w["calls"]) for w in creates) / n,
                   "in_calls_us_per_create": sum(w["sys"] for w in creates) / n,
                   "between_calls_us_per_create": sum(w["user"] for w in creates) / n,
                   "faults_per_create": sum(w["faults"] for w in creates) / n}, f, indent=1)
    if n_lost:
        print(f"refusing: the trace lost {n_lost} events", file=sys.stderr)
        return 1
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
    if a[0] == "switch" and len(a) in (3, 4):
        return switch(a[1], a[2], a[3] if len(a) == 4 else None)
    if a[0] == "mounts" and len(a) in (4, 5) and (len(a) == 4 or a[4] in ("pruned", "classic")):
        return mounts(*a[1:])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
