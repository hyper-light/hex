#!/usr/bin/env python3
"""bench/competitive/probes: the breakdown probe's trace reader and summary (exec_breakdown.md).

  exec_breakdown.py drain <trace_pipe> <out file> <stop file>
  exec_breakdown.py check <run dir> <prefix>
  exec_breakdown.py summarize <run dir>

drain copies a trace instance's trace_pipe to a file until the stop file appears, then takes what's left.

check reads one set of results (<prefix>.libcrun.json, <prefix>.seam.json, <prefix>.traced.* and <prefix>.detail.*,
<prefix>.floor.json) and exits 1 naming what doesn't read whole: an error recorded, a lost trace event, a traced
call not cut into its parts, a cut that the detailed trace shows opening another file than the one it stands for,
a floor mode without its samples.

summarize writes the run's markdown (stdout) and summary.json beside its results: libcrun's exec p50, the floor
under exec against a quarter of it, the seam's measured start and exec p50, and each traced call cut into its
parts. A part runs from one event of the call's critical path to the next (exec_cuts, start_cuts), so a call's
parts cover it whole, from seam_oci's marker before it to the one after.

The measured rounds trace only events that cost a call little: no path is fetched, so an open is told by its
place in the call (the node's first openat, its second...), which the call's fixed path makes the same every
time. The detailed trace adds each open's entry with its path, and check holds every such cut to the file it
stands for. Percentiles are nearest-rank, as summarize.py takes them; the trace's clock is CLOCK_MONOTONIC, to
the microsecond tracefs prints.
"""
import glob
import json
import math
import os
import re
import select
import sys


def drain(pipe, out, stop):
    """Reads without blocking, so an instance whose recording has stopped never ends the read early:
    trace_pipe answers EAGAIN when it's empty (kernel/trace/trace.c, tracing_wait_pipe), and the stop file
    is made only once recording is off, so the pass after it sees takes everything left."""
    fd = os.open(pipe, os.O_RDONLY | os.O_NONBLOCK)
    poller = select.poll()
    poller.register(fd, select.POLLIN)
    with open(out, "wb") as f:
        stopping = False
        while True:
            poller.poll(100)
            while True:
                try:
                    chunk = os.read(fd, 1 << 16)
                except BlockingIOError:
                    break
                if not chunk:
                    break
                f.write(chunk)
            if stopping:
                break
            stopping = os.path.exists(stop)
    os.close(fd)


# One line of trace_pipe: "<comm>-<pid> [<cpu>] <flags> <seconds>.<micros>: <event>", the comm free text.
LINE = re.compile(r"^\s*(?P<comm>.*?)-(?P<pid>\d+)\s+\[(?P<cpu>\d+)\]\s+\S+\s+(?P<s>\d+)\.(?P<us>\d{6}):\s+(?P<body>.*)$")
SYS_ENTER = re.compile(r"^sys_(?P<name>\w+)\((?P<args>.*)\)\s*$")
SYS_EXIT = re.compile(r"^sys_(?P<name>\w+) -> (?P<ret>0x[0-9a-f]+|-?\d+)\s*$")
KV = re.compile(r"(\w+)=(\S+)")
QUOTED = re.compile(r'"((?:[^"\\]|\\.)*)"')


class Event:
    __slots__ = ("ts", "pid", "kind", "name", "text", "kv")

    def __init__(self, ts, pid, kind, name, text, kv=None):
        self.ts, self.pid, self.kind, self.name, self.text, self.kv = ts, pid, kind, name, text, kv or {}

    def path(self):
        """An open's path, in a detailed trace: the first quoted string in its arguments."""
        m = QUOTED.search(self.text)
        return m.group(1) if m else ""


def parse(path):
    """The trace's events in order, and how many lines weren't events (the summary reports them)."""
    events, unread = [], 0
    with open(path, errors="replace") as f:
        for line in f:
            m = LINE.match(line)
            if not m:
                unread += line.strip() != "" and not line.startswith("#")
                continue
            ts, pid, body = int(m["s"]) * 1_000_000 + int(m["us"]), int(m["pid"]), m["body"]
            if body.startswith("tracing_mark_write: "):
                events.append(Event(ts, pid, "mark", "", body[len("tracing_mark_write: "):].strip()))
            elif body.startswith("sched_process_"):
                name, _, rest = body.partition(": ")
                events.append(Event(ts, pid, "sched", name[len("sched_process_"):], rest, dict(KV.findall(rest))))
            elif (e := SYS_EXIT.match(body)):
                events.append(Event(ts, pid, "exit", e["name"], e["ret"]))
            elif (e := SYS_ENTER.match(body)):
                events.append(Event(ts, pid, "enter", e["name"], e["args"]))
            else:
                unread += 1
    return events, unread


class Missing(Exception):
    """A traced call whose critical path lacks an event its cuts need: it's counted, never cut short."""


def first(ev, start, test, what):
    for i in range(start, len(ev)):
        if test(ev[i]):
            return i
    raise Missing(what)


def last_before(ev, start, end, test, what):
    for i in range(end - 1, start - 1, -1):
        if test(ev[i]):
            return i
    raise Missing(what)


def enter(pid, *names):
    return lambda e: e.pid == pid and e.kind == "enter" and e.name in names


def exited(pid, *names):
    return lambda e: e.pid == pid and e.kind == "exit" and e.name in names


def sched(pid, name):
    return lambda e: e.pid == pid and e.kind == "sched" and e.name == name


def named(path, name):
    """Whether a traced path names the file `name`. tracefs prints at most 63 bytes of a path and then "...",
    so a last component cut short matches the start of `name`."""
    if path.endswith("..."):
        head, _, last = path[:-3].rpartition("/")
        return bool(head) and bool(last) and name.startswith(last)
    return path.rpartition("/")[2] == name


POLLS = ("poll", "ppoll")
RENAMES = ("rename", "renameat", "renameat2")


class Cuts:
    """A call's cuts, built along its critical path, and the opens among them, each with the file it stands
    for, which a detailed trace checks."""

    def __init__(self, ev):
        self.ev, self.at, self.opens = ev, [0], []

    def to(self, test, what):
        self.at.append(first(self.ev, self.at[-1], test, what))
        return self.at[-1]

    def opened(self, pid, call, n, since, test, what):
        """The n-th exit of `call` (openat or open: rustix opens with open(2) on x86_64) by `pid` from event
        `since` on: the open that the call's fixed path puts there."""
        k = 0
        for i in range(since, len(self.ev)):
            e = self.ev[i]
            if e.pid == pid and e.kind == "exit" and e.name == call:
                k += 1
                if k == n:
                    if i < self.at[-1]:
                        raise Missing(f"{what} before the cut it follows")
                    self.at.append(i)
                    self.opens.append((i, pid, call, test, what))
                    return i
        raise Missing(what)


# The parts of an exec through the seam (node.rs exec_oci, the `hex __oci-exec` helper of oci/exec.rs), each
# (name, group), in critical-path order: part k runs from cut k to cut k+1 (exec_cuts).
EXEC_PARTS = [
    ("the call reaches the node; the box's record opened", "the node's bookkeeping"),
    ("the record read and decoded; the container's state opened", "the node's bookkeeping"),
    ("the state read and parsed; the init's stat opened", "the node's bookkeeping"),
    ("the init checked alive; the helper's request made; its stdio's first /dev/null opened", "the node's bookkeeping"),
    ("the rest of its stdio opened; the spawn prepared", "the node's bookkeeping"),
    ("the helper's clone (vfork), until the child runs", "helper spawn and re-exec"),
    ("the child's stdio set; the helper's execve (kernel)", "helper spawn and re-exec"),
    ("the helper's binary starts: relocated, its pages faulted in, libc's start-up", "helper spawn and re-exec"),
    ("Rust's start-up; the request read and parsed; config.json opened", "helper spawn and re-exec"),
    ("config.json read and parsed; the container's cgroup opened", "request and config"),
    ("the cgroup's procs and the init's namespaces opened, each compared with the helper's", "namespace joins"),
    ("the namespaces joined (setns)", "namespace joins"),
    ("the signals forwarded; the lifeline made", "the clone"),
    ("the clone into the container's cgroup (kernel)", "the clone"),
    ("the child first runs", "the clone"),
    ("the child becomes the container's process (user, capabilities, lifeline)", "the child's set-up"),
    ("the program resolved; its execve (kernel)", "the program's exec"),
    ("the program runs: the dynamic loader, libc, main", "the program's exec"),
    ("the program's exit (kernel, to its exit event)", "the program's exec"),
    ("its teardown, and the helper's poll wakes", "the wait"),
    ("the helper reaps it", "the wait"),
    ("the helper ends itself as the program did", "the wait"),
    ("the helper's exit (kernel, to its exit event)", "the wait"),
    ("its teardown, and the node's poll wakes", "the wait"),
    ("the node reaps the helper", "the wait"),
    ("the node returns the exit", "the wait"),
]


def exec_cuts(ev, bench, cid):
    c = Cuts(ev)
    c.opened(bench, "openat", 1, 0, lambda p: named(p, f"{cid}.json"), "the node's open of the box's record")
    c.opened(bench, "openat", 2, 0, lambda p: named(p, "state.json"), "the node's open of the container's state")
    c.opened(bench, "openat", 3, 0, lambda p: p.startswith("/proc/") and named(p, "stat"), "the node's open of the init's stat")
    c.opened(bench, "openat", 4, 0, lambda p: p == "/dev/null", "the node's open of /dev/null")
    c.to(enter(bench, "clone3", "clone"), "the node's clone of the helper")
    helper = int(ev[first(ev, c.at[-1], sched(bench, "fork"), "the helper's fork")].kv["child_pid"])
    c.to(exited(helper, "clone3", "clone"), "the helper's first run")
    execd = c.to(sched(helper, "exec"), "the helper's exec event")
    c.to(exited(helper, *POLLS), "the helper's first traced call (std's poll of its stdio)")
    c.opened(helper, "openat", 2, execd, lambda p: named(p, "config.json"), "the helper's open of config.json")
    c.opened(helper, "open", 1, c.at[-1], lambda p: p.startswith("/sys/fs/cgroup/"), "the helper's open of the cgroup")
    joins = first(ev, c.at[-1], enter(helper, "setns"), "the helper's first setns")
    c.at.append(joins)
    clone = first(ev, joins, enter(helper, "clone3"), "the helper's clone3")
    c.at.append(last_before(ev, joins, clone, exited(helper, "setns"), "the helper's last setns"))
    c.at.append(clone)
    fork = c.to(sched(helper, "fork"), "the program's fork")
    child = int(ev[fork].kv["child_pid"])
    c.to(exited(child, "clone3"), "the child's first run")
    c.to(exited(child, *POLLS), "the child's look at its lifeline")
    c.to(sched(child, "exec"), "the program's exec event")
    c.to(enter(child, "exit_group"), "the program's exit_group")
    c.to(sched(child, "exit"), "the program's exit event")
    c.to(exited(helper, *POLLS), "the helper's poll returning")
    c.to(exited(helper, "wait4"), "the helper's wait4")
    c.to(enter(helper, "exit_group"), "the helper's exit_group")
    c.to(sched(helper, "exit"), "the helper's exit event")
    c.to(exited(bench, *POLLS), "the node's poll returning")
    c.to(exited(bench, "waitid"), "the node's waitid")
    return c


# The parts of a start through the seam (node.rs start, oci/run.rs start), as EXEC_PARTS.
START_PARTS = [
    ("the call reaches the node; the box's record opened", "state reads"),
    ("the record read and decoded; the container's state opened", "state reads"),
    ("the state read and parsed, its init checked (the node's); the state opened again", "state reads"),
    ("the state read and parsed, its init checked again (the oci runtime's start); the FIFO opened", "state reads"),
    ("the init, blocked opening the FIFO, wakes and writes its byte; the poll returns", "the FIFO"),
    ("the byte read; the state encoded as running; its temp file made", "state write"),
    ("the state written; renamed into place", "state write"),
    ("config.json opened for the poststart hooks", "hooks' config read"),
    ("config.json read and parsed; the call returns", "hooks' config read"),
]


def start_cuts(ev, bench, cid):
    c = Cuts(ev)
    c.opened(bench, "openat", 1, 0, lambda p: named(p, f"{cid}.json"), "the node's open of the box's record")
    c.opened(bench, "openat", 2, 0, lambda p: named(p, "state.json"), "the node's open of the container's state")
    c.opened(bench, "openat", 4, 0, lambda p: named(p, "state.json"), "start's open of the container's state")
    c.opened(bench, "open", 1, 0, lambda p: named(p, "exec.fifo"), "start's open of the FIFO")
    c.to(exited(bench, *POLLS), "start's poll returning")
    c.opened(bench, "openat", 6, 0, lambda p: named(p, ".state.json.tmp"), "the state's temp file")
    c.to(exited(bench, *RENAMES), "the state's rename")
    c.opened(bench, "openat", 7, 0, lambda p: named(p, "config.json"), "the hooks' config.json")
    return c


OPS = {"exec": (EXEC_PARTS, exec_cuts), "start": (START_PARTS, start_cuts)}


def held_to_their_files(window, cuts):
    """In a detailed trace (one with the opens' entries and their paths), each open cut is the file it stands
    for: the entry before its exit, by the same process and call, names that file."""
    if not any(e.kind == "enter" and e.name in ("openat", "open") for e in window):
        return
    for i, pid, call, test, what in cuts.opens:
        j = last_before(window, 0, i, enter(pid, call), f"{what}: no entry before its exit")
        if not test(window[j].path()):
            raise Missing(f"{what}: it opened {window[j].path()!r}")


def calls(trace):
    """Each traced call: its op and id, its parts in microseconds (or what was missing), its span from marker
    to marker, and the time seam_oci measured for it."""
    ev, unread = parse(trace)
    out, i = [], 0
    while i < len(ev):
        e = ev[i]
        f = e.text.split() if e.kind == "mark" else []
        if len(f) == 4 and f[:2] == ["hexbd", "B"] and f[2] in OPS:
            op, cid, bench = f[2], f[3], e.pid
            end = next((j for j in range(i + 1, len(ev)) if ev[j].kind == "mark" and ev[j].pid == bench
                        and ev[j].text.split()[:2] == ["hexbd", "E"]), None)
            if end is None:
                out.append({"op": op, "id": cid, "missing": "its end marker"})
                break
            g = ev[end].text.split()
            measured = int(g[4]) / 1000 if len(g) == 5 and g[2:4] == [op, cid] else None
            window = ev[i:end + 1]
            spec, cut = OPS[op]
            call = {"op": op, "id": cid, "span": window[-1].ts - window[0].ts, "measured": measured}
            try:
                if measured is None:
                    raise Missing(f"an end marker of its own (got {' '.join(g)!r})")
                cuts = cut(window, bench, cid)
                held_to_their_files(window, cuts)
                at = cuts.at + [len(window) - 1]
                if any(b < a for a, b in zip(at, at[1:])) or len(at) != len(spec) + 1:
                    raise Missing("its cuts in order")
                call["parts"] = [window[b].ts - window[a].ts for a, b in zip(at, at[1:])]
            except Missing as m:
                call["missing"] = str(m)
            out.append(call)
            i = end + 1
            continue
        i += 1
    return out, unread


def lost(stats):
    """Events a trace instance lost, from its per-CPU stats (one `key: value` a line, each CPU's after a
    `== cpuN` line): overruns, commit overruns and dropped events."""
    total = 0
    with open(stats) as f:
        for line in f:
            key, _, value = line.partition(":")
            if key.strip() in ("overrun", "commit overrun", "dropped events"):
                total += int(value.split()[0])
    return total


def pct(samples, q):
    s = sorted(samples)
    return s[max(0, math.ceil(q * len(s)) - 1)] if s else None


def dist(samples):
    return {"n": len(samples), "p50": pct(samples, 0.5), "p99": pct(samples, 0.99),
            "max": max(samples) if samples else None, "mean": sum(samples) / len(samples) if samples else None}


def lifecycle_ms(path, op):
    with open(path) as f:
        r = json.load(f)
    return r.get("errors") or {}, r.get("phases", {}).get("lifecycle", {}).get("ms", {}).get(op, [])


def floor_ms(path):
    with open(path) as f:
        r = json.load(f)
    return r, {m: [x / 1e6 for x in v] for m, v in r["ns"].items()}


def check(run, prefix):
    bad = []
    timed = None
    for kind in ("libcrun", "seam", "traced", "detail"):
        errs, ms = lifecycle_ms(os.path.join(run, f"{prefix}.{kind}.json"), "exec")
        if errs or not ms:
            bad.append(f"{kind}: errors {errs}, {len(ms)} execs")
        if kind in ("traced", "detail"):
            timed = len(ms)
            stats = os.path.join(run, f"{prefix}.{kind}.stats")
            if lost(stats):
                bad.append(f"{kind}: the trace lost {lost(stats)} events")
            got, unread = calls(os.path.join(run, f"{prefix}.{kind}.txt"))
            for c in got:
                if "missing" in c:
                    bad.append(f"{kind}: {c['op']} {c['id']} lacks {c['missing']}")
            for op in OPS:
                if sum(c["op"] == op for c in got) != timed:
                    bad.append(f"{kind}: {sum(c['op'] == op for c in got)} {op} calls traced, {timed} timed")
            print(f"check {prefix}.{kind}: {sum('parts' in c for c in got)} of {len(got)} traced calls cut whole, "
                  f"{unread} trace lines not read as events")
    r, modes = floor_ms(os.path.join(run, f"{prefix}.floor.json"))
    for m in ("spawn", "clone", "cgroup", "floor"):
        if len(modes.get(m, [])) != r["n"]:
            bad.append(f"floor: {len(modes.get(m, []))} {m} samples, {r['n']} asked for")
    print(f"check {prefix}.floor: joined {r['joined']}")
    for b in bad:
        print(f"check {prefix}: {b}")
    return 1 if bad else 0


def rounds(run, kind):
    out = {}
    for path in sorted(glob.glob(os.path.join(run, f"{kind}.r*.json"))):
        out[int(re.search(r"\.r(\d+)\.json$", path).group(1))] = path
    return out


def ms3(x):
    return "n/a" if x is None else f"{x:.3f}"


def summarize(run):
    s = {"run": run}
    lines = [f"# exec breakdown: {os.path.basename(run.rstrip('/'))}", ""]
    with open(os.path.join(run, "env.txt")) as f:
        lines += ["```", f.read().rstrip(), "```", ""]
    # libcrun's exec, and the seam's measured start and exec, per round and best of N (lowest p50).
    lat = {}
    for kind in ("libcrun", "seam", "traced"):
        for op in ("start", "exec"):
            per = {}
            for rnd, path in rounds(run, kind).items():
                errs, ms = lifecycle_ms(path, op)
                per[rnd] = {"errors": errs, **dist(ms)}
            ok = {r: d for r, d in per.items() if not d["errors"] and d["n"]}
            best = min(ok, key=lambda r: ok[r]["p50"]) if ok else None
            lat[(kind, op)] = {"rounds": per, "best_round": best, "best": ok.get(best)}
    s["latency_ms"] = {f"{k}.{op}": v for (k, op), v in lat.items()}
    nums = sorted({r for v in lat.values() for r in v["rounds"]})
    lines += ["## Measured p50 (ms), per round", "", "| | " + " | ".join(f"r{r}" for r in nums) + " | best |",
              "|---|" + "---|" * (len(nums) + 1)]
    for (kind, op), v in lat.items():
        row = [ms3(v["rounds"][r]["p50"]) if r in v["rounds"] else "n/a" for r in nums]
        lines.append(f"| {kind} {op} | " + " | ".join(row) + f" | {ms3((v['best'] or {}).get('p50'))} |")
    lines.append("")

    # The floor under exec, against a quarter of libcrun's exec p50 in this run.
    floors = {}
    for rnd, path in rounds(run, "floor").items():
        r, modes = floor_ms(path)
        floors[rnd] = {"joined": r["joined"], "load_start": r["load_start"], "load_end": r["load_end"],
                       **{m: dist(v) for m, v in modes.items()}}
    s["floor_ms"] = floors
    lc = lat[("libcrun", "exec")]["best"]
    target = lc["p50"] / 4 if lc else None
    best_floor = min((f["floor"]["p50"] for f in floors.values()), default=None)
    s["libcrun_exec_p50_ms"] = lc and lc["p50"]
    s["target_ms"] = target
    s["floor_best_p50_ms"] = best_floor
    s["floor_above_target"] = None if target is None or best_floor is None else best_floor > target
    lines += ["## The floor under exec (ms; exec_floor.c, interleaved cycle by cycle)", "",
              "| round | mode | n | p50 | p99 | max | load at start, end |", "|---|---|---|---|---|---|---|"]
    for rnd, f in sorted(floors.items()):
        for m in ("spawn", "clone", "cgroup", "floor"):
            d = f[m]
            lines.append(f"| r{rnd} | {m} | {d['n']} | {ms3(d['p50'])} | {ms3(d['p99'])} | {ms3(d['max'])} | "
                         f"{f['load_start'].split(' ')[0]}, {f['load_end'].split(' ')[0]} |")
    n_lc = sum(1 for d in lat[("libcrun", "exec")]["rounds"].values() if d["n"])
    lines += ["", f"libcrun's exec p50, best of {n_lc} rounds: {ms3(lc and lc['p50'])} ms, so goal 3's exec target "
              f"(a quarter of it) is {ms3(target)} ms. The floor's best p50: {ms3(best_floor)} ms, "
              f"{'above' if s['floor_above_target'] else 'under'} the target"
              + (f" ({best_floor / target:.2f}x it)." if target and best_floor else "."), ""]

    # The traced calls, cut into their parts, every traced round pooled.
    traces = sorted(glob.glob(os.path.join(run, "traced.r*.txt")))
    got, unread, n_lost = [], 0, 0
    for t in traces:
        c, u = calls(t)
        got += c
        unread += u
        n_lost += lost(t[:-len(".txt")] + ".stats")
    s["trace"] = {"files": [os.path.basename(t) for t in traces], "lost_events": n_lost, "unread_lines": unread}
    for op, (spec, _) in OPS.items():
        whole = [c for c in got if c["op"] == op and "parts" in c]
        missing = {}
        for c in got:
            if c["op"] == op and "missing" in c:
                missing[c["missing"]] = missing.get(c["missing"], 0) + 1
        parts = [dist([c["parts"][k] / 1000 for c in whole]) for k in range(len(spec))]
        span = dist([c["span"] / 1000 for c in whole])
        measured = dist([c["measured"] / 1000 for c in whole])
        untraced = dist([x for path in rounds(run, "seam").values() for x in lifecycle_ms(path, op)[1]])
        sum_p50 = sum(p["p50"] for p in parts) if whole else None
        groups = {}
        for (_, group), p in zip(spec, parts):
            g = groups.setdefault(group, {"p50": 0.0, "mean": 0.0})
            g["p50"] += p["p50"] or 0
            g["mean"] += p["mean"] or 0
        rec = {"calls": len(whole), "missing": missing, "parts": [{"part": n, "group": g, **p} for (n, g), p in
                                                                   zip(spec, parts)],
               "groups": groups, "span_ms": span, "measured_traced_ms": measured, "measured_untraced_ms": untraced,
               "sum_of_part_p50s_ms": sum_p50}
        lines += [f"## {op} through the seam, in parts ({len(whole)} traced calls"
                  + (f"; {sum(missing.values())} not cut: {missing}" if missing else "") + ")", "",
                  "| part | group | p50 | p99 | mean |", "|---|---|---|---|---|"]
        for (name, group), p in zip(spec, parts):
            lines.append(f"| {name} | {group} | {ms3(p['p50'])} | {ms3(p['p99'])} | {ms3(p['mean'])} |")
        lines += ["", "| group | its parts' p50s, summed | their means, summed |", "|---|---|---|"]
        for group, g in groups.items():
            lines.append(f"| {group} | {g['p50']:.3f} | {g['mean']:.3f} |")
        if whole and measured["p50"] and untraced["p50"]:
            rec["sum_over_measured_traced"] = sum_p50 / measured["p50"]
            rec["sum_over_measured_untraced"] = sum_p50 / untraced["p50"]
            rec["traced_over_untraced"] = measured["p50"] / untraced["p50"]
            lines += ["", f"The parts' p50s sum to {sum_p50:.3f} ms, and their means to "
                      f"{sum(p['mean'] for p in parts):.3f} ms, the calls' mean span from marker to marker "
                      f"({span['mean']:.3f} ms). seam_oci measured these calls at p50 {measured['p50']:.3f} ms: the "
                      f"sum is {rec['sum_over_measured_traced']:.3f} of it. The untraced rounds' {op}, pooled, is p50 "
                      f"{untraced['p50']:.3f} ms: the sum is {rec['sum_over_measured_untraced']:.3f} of it, and the "
                      f"tracer cost the traced calls {rec['traced_over_untraced']:.3f}x at p50.", ""]
        s[f"{op}_parts"] = rec
    lines += [f"Trace: {len(traces)} files, {n_lost} events lost, {unread} lines not read as events.", ""]
    with open(os.path.join(run, "summary.json"), "w") as f:
        json.dump(s, f, indent=1)
    print("\n".join(lines))
    return 0


def main():
    a = sys.argv[1:]
    if len(a) == 4 and a[0] == "drain":
        drain(*a[1:])
        return 0
    if len(a) == 3 and a[0] == "check":
        return check(a[1], a[2])
    if len(a) == 2 and a[0] == "summarize":
        return summarize(a[1])
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
