#!/usr/bin/env python3
"""What the seam's slowest creates and starts wait on (create_tail.md).

`create_tail.py read <run dir>` reads create_tail_inside.sh's trace (trace.txt: the run's own ftrace instance, the
runtime's switches with each blocking switch's kernel stack, its wake-ups and forks, and seam_oci's markers around
every other timed create and start) and walks each bracketed call's critical path back from its end marker, the
wait-graph walk of Giraldeau and Dagenais (IEEE TPDS 27(8), 2016, "Wait analysis of distributed systems using
kernel tracing", §3): from the caller's thread at the call's end, each interval it ran is charged to it running;
each interval it sat runnable is charged to the run queue (preempted, by whom; or woken and not yet run); each
interval it was blocked is followed into the task that woke it, from the wake-up back, when that task is the
runtime's own and traced, and else charged to the blocking stack and its waker. A fork is followed into its parent.
Every interval of the call is charged once, so each call's charges sum to its span.

It writes, per op (create, start): the calls' spans, then the mean charge by kind for the slowest 0.1% and 1% of
calls and for the middle band (the 45th to 55th percentile), and the kinds whose charge the tail adds, largest
first; and each of the slowest calls' charges in full. `create_tail.py test` is its selftest.
"""
import bisect
import collections
import re
import sys

LINE = re.compile(r"^\s*(?P<comm>.+?)-(?P<pid>\d+)\s+\[(?P<cpu>\d+)\]\s+(?P<flags>\S+)\s+(?P<ts>\d+\.\d+): "
                  r"(?P<ev>[\w]+): (?P<rest>.*)$")
# A stacktrace trigger's entry: "<comm>-<pid> [cpu] flags ts: <stack trace>", its frames on the lines after it.
STACK = re.compile(r"^\s*.+?-\d+\s+\[(?P<cpu>\d+)\]\s+\S+\s+\d+\.\d+: <stack trace>$")
SWITCH = re.compile(r"prev_comm=(?P<pc>.*?) prev_pid=(?P<pp>\d+) prev_prio=-?\d+ prev_state=(?P<st>\S+) ==> "
                    r"next_comm=(?P<nc>.*?) next_pid=(?P<np>\d+)")
WAKE = re.compile(r"comm=(?P<c>.*?) pid=(?P<p>\d+) ")
WORK = re.compile(r"work struct \S+: function (?P<f>\S+)")
FORK = re.compile(r"comm=(?P<c>.*?) pid=(?P<p>\d+) child_comm=(?P<cc>.*?) child_pid=(?P<cp>\d+)")
MARK = re.compile(r"hexbd (?P<be>[BE]) (?P<op>\w+) (?P<id>\S+)(?: (?P<ns>\d+))?")
# Frames that say only that a task gave up its CPU, skipped when a stack is named by the frames that say why.
SCHED = ("do_trace_event", "__schedule", "schedule", "schedule_", "io_schedule", "__traceiter", "trace_event", "ftrace",
         "event_trigger", "stack_trace", "__trace_stack", "preempt_schedule", "__cond_resched", "do_nanosleep",
         "hrtimer_nanosleep", "__mutex_lock", "mutex_lock", "rwsem_down", "down_", "__down", "__wait_for_common",
         "wait_for_completion", "__rt_mutex", "percpu_rwsem_wait", "__percpu_down")
MAX_STEPS = 100_000  # a walk's budget of intervals: a call far past it is reported cut short, not walked further
FRAMES = 4           # frames that name a blocking stack


def ours(comm):
    return comm == "seam_oci" or comm.startswith("hex")


SOFTIRQ = re.compile(r"vec=\d+ \[action=(?P<a>\w+)\]")
HARDIRQ = re.compile(r"irq=\d+ name=(?P<n>\S+)")
PERCPU = re.compile(r"/\d+(:\d+H?)?$")


def who(comm):
    """A task's name without its CPU or pool (kworker/19:1 is a kworker, ksoftirqd/19 a ksoftirqd)."""
    return PERCPU.sub("", comm.strip())


class Task:
    __slots__ = ("comm", "ins", "outs", "wakes", "fork")

    def __init__(self, comm):
        self.comm = comm
        self.ins = []    # (ts, cpu)
        self.outs = []   # (ts, state, next_comm, stack list, cpu)
        self.wakes = []  # (ts, waker pid, waker comm, in interrupt, cpu)
        self.fork = None  # (ts, parent pid)


def parse(lines):
    """The trace's tasks, its marked calls and each CPU's interrupt work:
    ({pid: Task}, [(op, id, begin, end, driver pid, ns)], {cpu: [(begin, end, what)]}). A kworker's work items are
    each CPU's work too, as "work <function>"."""
    tasks = {}
    irqs, irq_open = collections.defaultdict(list), {}
    calls, open_ = [], {}
    last_out = {}  # cpu -> the blocking switch out whose stack the next stack lines are

    def task(pid, comm):
        t = tasks.get(pid)
        if t is None:
            t = tasks[pid] = Task(comm)
        return t

    stack_of = None
    for raw in lines:
        if raw.startswith(" => "):
            if stack_of is not None and len(stack_of) < 64:
                stack_of.append(raw[4:].strip())
            continue
        h = STACK.match(raw)
        if h:
            o = last_out.get(h["cpu"])
            stack_of = o[3] if o is not None else None
            continue
        m = LINE.match(raw)
        if not m:
            continue
        ts = float(m["ts"])
        ev, rest, cpu = m["ev"], m["rest"], m["cpu"]
        stack_of = None
        if ev == "sched_switch":
            s = SWITCH.search(rest)
            if not s:
                continue
            pp, np_ = int(s["pp"]), int(s["np"])
            if ours(s["pc"]):
                o = (ts, s["st"], s["nc"], [], int(cpu))
                task(pp, s["pc"]).outs.append(o)
                last_out[cpu] = o
            if ours(s["nc"]):
                task(np_, s["nc"]).ins.append((ts, int(cpu)))
        elif ev in ("sched_waking", "sched_wakeup_new"):
            w = WAKE.search(rest)
            if w and ours(w["c"]):
                irq = any(f in m["flags"] for f in "hHsS")
                task(int(w["p"]), w["c"]).wakes.append((ts, int(m["pid"]), m["comm"].strip(), irq, int(cpu)))
        elif ev == "sched_process_fork":
            f = FORK.search(rest)
            if f:
                task(int(f["cp"]), f["cc"]).fork = (ts, int(f["p"]))
        elif ev in ("softirq_entry", "irq_handler_entry"):
            g = (SOFTIRQ if ev == "softirq_entry" else HARDIRQ).search(rest)
            irq_open[(int(cpu), ev[:4])] = (ts, ("softirq " + g["a"]) if ev == "softirq_entry" and g
                                            else ("irq " + g["n"]) if g else ev)
        elif ev == "workqueue_execute_start":
            g = WORK.search(rest)
            irq_open[(int(cpu), "work")] = (ts, "work " + (g["f"] if g else "?"))
        elif ev == "workqueue_execute_end":
            o = irq_open.pop((int(cpu), "work"), None)
            if o is not None:
                irqs[int(cpu)].append((o[0], ts, o[1]))
        elif ev in ("softirq_exit", "irq_handler_exit"):
            o = irq_open.pop((int(cpu), ev[:4]), None)
            if o is not None:
                irqs[int(cpu)].append((o[0], ts, o[1]))
        elif ev == "tracing_mark_write":
            k = MARK.search(rest)
            if not k:
                continue
            key = (k["op"], k["id"])
            if k["be"] == "B":
                open_[key] = (ts, int(m["pid"]))
            elif key in open_:
                b, pid = open_.pop(key)
                calls.append((k["op"], k["id"], b, ts, pid, int(k["ns"] or 0)))
    for v in irqs.values():
        v.sort()
    return tasks, calls, irqs


def ran(irqs, cpu, a, t, comm):
    """A run of comm on cpu from a to t, as charges: the interrupt work on that CPU within it apart."""
    out, busy = [], 0.0
    xs = irqs.get(cpu, [])
    i = max(0, bisect.bisect_left(xs, (a,)) - 1)
    while i < len(xs) and xs[i][0] < t:
        lo, hi = max(xs[i][0], a), min(xs[i][1], t)
        if hi > lo and not xs[i][2].startswith("work "):
            out.append((xs[i][2], hi - lo))
            busy += hi - lo
        i += 1
    out.append((f"run: {comm}", (t - a) - busy))
    return out


def doing(irqs, cpu, a, t):
    """What the CPU did most of from a to t, of its interrupt work and work items, or "" for none of it."""
    acc = collections.Counter()
    xs = irqs.get(cpu, [])
    i = max(0, bisect.bisect_left(xs, (a,)) - 1)
    while i < len(xs) and xs[i][0] < t:
        lo, hi = max(xs[i][0], a), min(xs[i][1], t)
        if hi > lo:
            acc[xs[i][2]] += hi - lo
        i += 1
    return f" ({acc.most_common(1)[0][0]})" if acc else ""


def last_le(xs, t, key=lambda x: x):
    """The last of sorted xs whose key is at most t, or None (a binary search)."""
    lo, hi = 0, len(xs)
    while lo < hi:
        mid = (lo + hi) // 2
        if key(xs[mid]) <= t:
            lo = mid + 1
        else:
            hi = mid
    return xs[lo - 1] if lo else None


def stack_name(stack):
    keep = [f.split("+")[0] for f in stack if not f.split("+")[0].startswith(SCHED)]
    return " < ".join(keep[:FRAMES]) or "(no stack)"


def walk(tasks, irqs, b, e, pid):
    """The call's charges: [(kind, seconds)], summing to e - b, by its critical path back from e."""
    out, cur, t, steps = [], pid, e, 0
    while t > b:
        steps += 1
        if steps > MAX_STEPS:
            out.append(("cut short (walk budget)", t - b))
            break
        tk = tasks.get(cur)
        if tk is None:
            out.append(("untraced", t - b))
            break
        got = last_le(tk.ins, t, key=lambda x: x[0])
        s_in, cpu = got if got is not None else (None, None)
        if s_in is None or (tk.fork is not None and s_in < tk.fork[0]):
            if tk.fork is not None and tk.fork[0] <= t:
                f_ts, parent = tk.fork
                out.append((f"runq: new task {tk.comm}, before its first run", t - max(f_ts, b)))
                cur, t = parent, f_ts
                continue
            o0 = last_le(tk.outs, t, key=lambda x: x[0])
            if o0 is None or o0[0] >= t:
                # no switch out of it is traced before t, and every one is: it ran from before the call began
                out.append((f"run: {tk.comm}", t - b))
                break
            out.append(("untraced", t - b))
            break
        out.extend(ran(irqs, cpu, max(s_in, b), t, tk.comm))
        if s_in <= b:
            break
        o = last_le(tk.outs, s_in, key=lambda x: x[0])
        if o is None or (tk.fork is not None and o[0] < tk.fork[0]):
            if tk.fork is not None and tk.fork[0] <= s_in:
                f_ts, parent = tk.fork
                out.append((f"runq: new task {tk.comm}, before its first run", s_in - max(f_ts, b)))
                cur, t = parent, f_ts
                continue
            w = last_le(tk.wakes, s_in, key=lambda x: x[0])
            if w is None:
                out.append(("untraced", s_in - b))
                break
            # its switch out is before the trace's start, its wake-up within it
            out.append(("runq: woken, not yet run", s_in - max(w[0], b)))
            if w[0] <= b:
                break
            if not w[3] and ours(w[2]) and w[1] in tasks and w[1] != cur:
                cur, t = w[1], w[0]
                continue
            out.append(("untraced", w[0] - b))
            break
        s_out, state, next_comm, stack, o_cpu = o
        if state.startswith("R"):
            what = doing(irqs, o_cpu, max(s_out, b), s_in)
            out.append((f"runq: preempted by {who(next_comm)}{what}", s_in - max(s_out, b)))
            t = s_out
            continue
        w = last_le(tk.wakes, s_in, key=lambda x: x[0])
        if w is None or w[0] < s_out:
            out.append((f"blocked: {stack_name(stack)} (waker untraced)", s_in - max(s_out, b)))
            t = s_out
            continue
        w_ts, waker, waker_comm, irq, w_cpu = w
        out.append(("runq: woken, not yet run", s_in - max(w_ts, b)))
        if w_ts <= b:
            break
        if not irq and ours(waker_comm) and waker in tasks and waker != cur:
            cur, t = waker, w_ts
            continue
        by = "an interrupt" if irq else who(waker_comm)
        if who(waker_comm) == "kworker":
            by += doing(irqs, w_cpu, w_ts - 1e-6, w_ts + 1e-6)
        out.append((f"blocked: {stack_name(stack)}, woken by {by}", w_ts - max(s_out, b)))
        t = s_out
    return out


def pct(xs, q):
    s = sorted(xs)
    return s[min(len(s) - 1, int(q * len(s)))] if s else 0.0


def report(tasks, calls, irqs, w=sys.stdout.write):
    by_op = collections.defaultdict(list)
    for c in calls:
        by_op[c[0]].append(c)
    for op in sorted(by_op):
        cs = sorted(by_op[op], key=lambda c: c[3] - c[2])
        spans = [(c[3] - c[2]) * 1e3 for c in cs]
        n = len(cs)
        w(f"\n## {op}: {n} traced calls, span p50 {pct(spans, .5):.3f} p90 {pct(spans, .9):.3f} "
          f"p99 {pct(spans, .99):.3f} p99.9 {pct(spans, .999):.3f} max {spans[-1]:.3f} ms\n\n")
        charges = [walk(tasks, irqs, c[2], c[3], c[4]) for c in cs]
        sums = [sum(d for _, d in ch) * 1e3 for ch in charges]
        off = max((abs(a - s) for a, s in zip(sums, spans)), default=0.0)
        w(f"Every call's charges sum to its span within {off:.6f} ms.\n\n")
        bands = {"mid (p45-p55)": range(int(.45 * n), max(int(.55 * n), int(.45 * n) + 1)),
                 "top 1%": range(max(0, n - max(1, n // 100)), n),
                 "top 0.1%": range(max(0, n - max(1, n // 1000)), n)}
        means = {}
        for name, idx in bands.items():
            acc = collections.Counter()
            for i in idx:
                for k, d in charges[i]:
                    acc[k] += d * 1e3
            means[name] = {k: v / len(idx) for k, v in acc.items()}
        kinds = sorted(set().union(*[set(m) for m in means.values()]),
                       key=lambda k: -(means["top 0.1%"].get(k, 0) - means["mid (p45-p55)"].get(k, 0)))
        w("| kind | mid (p45-p55) mean ms | top 1% mean ms | top 0.1% mean ms | top 0.1% over mid |\n|---|---|---|---|---|\n")
        for k in kinds[:25]:
            a, b1, c = (means[x].get(k, 0.0) for x in ("mid (p45-p55)", "top 1%", "top 0.1%"))
            if max(a, b1, c) < 0.005:
                continue
            w(f"| {k} | {a:.3f} | {b1:.3f} | {c:.3f} | {c - a:+.3f} |\n")
        w(f"\nThe slowest {min(10, n)} calls, each charge of at least 0.05 ms:\n\n")
        for i in range(n - 1, max(-1, n - 11), -1):
            agg = collections.Counter()
            for k, d in charges[i]:
                agg[k] += d * 1e3
            parts = "; ".join(f"{k} {v:.3f}" for k, v in agg.most_common() if v >= 0.05)
            w(f"- {cs[i][1]} {spans[i]:.3f} ms: {parts}\n")


def selftest():
    # A create on the driver (pid 10): runs 1 ms, blocks on a pipe read, woken by its init (pid 20, forked by the
    # seed 30 the driver woke), preempted once by a kworker; the init blocked on a mutex whose waker is a kworker.
    T = "seam_oci-10 [004] ..... {}: tracing_mark_write: hexbd {} create l1{}"
    sw = "{c}-{p} [00{cpu}] d..2. {ts}: sched_switch: prev_comm={c} prev_pid={p} prev_prio=120 prev_state={st} ==> next_comm={n} next_pid={np} next_prio=120"
    wk = "{c}-{p} [00{cpu}] d..2. {ts}: sched_waking: comm={wc} pid={wp} prio=120 target_cpu=004"
    lines = [
        T.format("1.000000", "B", ""),
        "seam_oci-10 [004] d..2. 1.000500: sched_switch: prev_comm=seam_oci prev_pid=10 prev_prio=120 prev_state=R+ ==> next_comm=kworker/4:1 next_pid=99 next_prio=120",
        "kworker/4:1-99 [004] ..... 1.000510: workqueue_execute_start: work struct 0000: function css_killed_work_fn",
        "kworker/4:1-99 [004] ..... 1.000690: workqueue_execute_end: work struct 0000: function css_killed_work_fn",
        "kworker/4:1-99 [004] d..2. 1.000700: sched_switch: prev_comm=kworker/4:1 prev_pid=99 prev_prio=120 prev_state=I ==> next_comm=seam_oci next_pid=10 next_prio=120",
        wk.format(c="seam_oci", p=10, cpu=4, ts="1.000900", wc="hex", wp=30),
        sw.format(c="seam_oci", p=10, cpu=4, ts="1.001000", st="S", n="swapper/4", np=0),
        "seam_oci-10 [004] d..2. 1.001000: <stack trace>",
        " => __schedule+0x1",
        " => schedule+0x2",
        " => pipe_read+0x3",
        " => vfs_read+0x4",
        "hex-30 [005] d..2. 1.001100: sched_switch: prev_comm=swapper/5 prev_pid=0 prev_prio=120 prev_state=R ==> next_comm=hex next_pid=30 next_prio=120",
        "hex-30 [005] ..... 1.001200: sched_process_fork: comm=hex pid=30 child_comm=hex child_pid=20",
        "hex-20 [006] d..2. 1.001400: sched_switch: prev_comm=swapper/6 prev_pid=0 prev_prio=120 prev_state=R ==> next_comm=hex next_pid=20 next_prio=120",
        sw.format(c="hex", p=20, cpu=6, ts="1.001500", st="D", n="swapper/6", np=0),
        "hex-20 [006] d..2. 1.001500: <stack trace>",
        " => __schedule+0x1",
        " => schedule+0x2",
        " => __mutex_lock+0x9",
        " => cgroup_mkdir+0x5",
        " => kernfs_iop_mkdir+0x6",
        "kworker/6:0-98 [006] ..... 1.002000: workqueue_execute_start: work struct 0001: function css_free_rwork_fn",
        wk.format(c="kworker/6:0", p=98, cpu=6, ts="1.003000", wc="hex", wp=20),
        "kworker/6:0-98 [006] ..... 1.003050: workqueue_execute_end: work struct 0001: function css_free_rwork_fn",
        "kworker/6:0-98 [006] d..2. 1.003100: sched_switch: prev_comm=kworker/6:0 prev_pid=98 prev_prio=120 prev_state=I ==> next_comm=hex next_pid=20 next_prio=120",
        wk.format(c="hex", p=20, cpu=6, ts="1.003500", wc="seam_oci", wp=10),
        "swapper/4-0 [004] d..2. 1.003600: sched_switch: prev_comm=swapper/4 prev_pid=0 prev_prio=120 prev_state=R ==> next_comm=seam_oci next_pid=10 next_prio=120",
        "seam_oci-10 [004] d.s1. 1.003700: softirq_entry: vec=9 [action=RCU]",
        "seam_oci-10 [004] d.s1. 1.003750: softirq_exit: vec=9 [action=RCU]",
        T.format("1.004000", "E", " 3000000"),
    ]
    tasks, calls, irqs = parse(lines)
    assert len(calls) == 1, calls
    op, _, b, e, pid, _ = calls[0]
    ch = walk(tasks, irqs, b, e, pid)
    total = sum(d for _, d in ch)
    assert abs(total - (e - b)) < 1e-9, (total, ch)
    got = collections.Counter()
    for k, d in ch:
        got[k] += round(d * 1e6)
    # In microseconds after 1.000000: the driver ran 3600-4000, woken by the init at 3500; the init ran 3100-3500,
    # woken by a kworker at 3000 from its block at 1500 in cgroup_mkdir; it ran 1400-1500, forked by the seed at
    # 1200; the seed ran 1100-1200, woken by the driver at 900; the driver ran 700-900, preempted 500-700 by a
    # kworker, and ran 0-500. Its own run 900-1000 and block from 1000 are off the path: the seed's wake-up is on it.
    # An RCU softirq ran on the driver's CPU 3700-3750, within its last run.
    want = {"run: seam_oci": 400 - 50 + 200 + 500, "softirq RCU": 50,
            "runq: woken, not yet run": 100 + 100 + 200,
            "run: hex": 400 + 100 + 100,
            "blocked: cgroup_mkdir < kernfs_iop_mkdir, woken by kworker (work css_free_rwork_fn)": 1500,
            "runq: new task hex, before its first run": 200,
            "runq: preempted by kworker (work css_killed_work_fn)": 200}
    assert dict(got) == want, (dict(got), want)
    print("selftest: ok")


def main(argv):
    if len(argv) >= 2 and argv[1] == "test":
        selftest()
        return 0
    if len(argv) != 3 or argv[1] != "read":
        print("usage: create_tail.py read <run dir> | test", file=sys.stderr)
        return 2
    with open(f"{argv[2]}/trace.txt", errors="replace") as f:
        tasks, calls, irqs = parse(f)
    w = sys.stdout.write
    w(f"# create_tail: {argv[2]}\n\n{len(calls)} marked calls, {len(tasks)} tasks traced.\n")
    report(tasks, calls, irqs, w)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
