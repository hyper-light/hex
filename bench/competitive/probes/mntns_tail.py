#!/usr/bin/env python3
"""Goal 17's record, read for where each way's create spends its time (the lead's step 4 (1)): from one cell's traced
rounds (rounds.py's bursts, rounds.c<N>.traced/rounds.jsonl) and its trace (mounts.c<N>.txt: every call, switch,
fork and wakeup of the hex processes, mounts_trace's), each create cut into its parts on the trace's clock:
- the agent before the claim: from the create's start (its bench thread's return from the burst's barrier) to its
  init's wakeup by that thread, the claim of a parked init;
- the init's wait to run: from the claim to the init's next time on a CPU;
- the init before part 19: from then to its join of the network namespace made ahead;
- part 19: from that join to the init's chdir (span root, as create_breakdown.py mounts cuts it);
- the init after part 19: from its chdir to its ready, its last write before it opens the exec FIFO (its first
  openat after part 19, which blocks until the start; the write is the sync pipe's two-byte ready);
- the handoff back: from the init's ready to the create's thread's next time on a CPU;
- the agent after: from then to the create's end, its start plus its time as rounds.py has it.
A create is its bench thread's: the thread that woke the init (sched_waking names its waker). rounds.py lists a burst's
creates in the order seam_oci spawned its threads, which is their tids' order. A thread's time off the CPU in the
agent's parts, and the init's in its own, is split into preempted (charged to the task that took the CPU, by its name
without a kernel thread's per-CPU suffix) and blocked. For each way: every part's mean over all creates and over the
slowest 1% (by create time, nearest rank), and what took the slowest 1%'s CPU, the agent's and the init's.
Run: python3 -I mntns_tail.py <cell results dir> [concurrency]
"""
import bisect
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import create_breakdown as cb  # noqa: E402
import exec_breakdown as eb  # noqa: E402

WAYS = ("copy", "tree", "empty")
NR_WRITE, NR_OPENAT = 1, 257
CLONE_NEWNET = 0x40000000
PARTS = ("agent before the claim", "init's wait to run", "init before part 19", "part 19", "init after part 19",
         "handoff back", "agent after")


def pct(xs, q):
    s = sorted(xs)
    return s[max(0, math.ceil(q * len(s)) - 1)] if s else None


class Proc:
    """One task's timeline as the trace has it: its fork, its wakeups (with their waker), its switches in and out,
    and an init's part 19 and ready."""

    __slots__ = ("fork", "wakes", "ins", "outs", "setns", "p19", "p19_end", "write", "ready")

    def __init__(self):
        self.fork, self.wakes, self.ins, self.outs = None, [], [], []
        self.setns = self.p19 = self.p19_end = self.write = self.ready = None


def read_trace(lines):
    """Every traced task's fork, wakeups by waker, switches in and out (whether it left runnable and who took the
    CPU), and an init's part 19's start (the exit of its join of a network namespace), end (its next chdir) and ready
    (its last write before its first openat after part 19), each on the trace's clock (us). The calls are a hex
    process's whatever name the line shows: the trace kept those alone as it recorded (mounts_trace's filter), and
    ftrace names a pid when the trace is read, so an init that has run its program since shows as that program."""
    procs = {}
    get = lambda pid: procs.setdefault(pid, Proc())
    for line in lines:
        m = eb.LINE.match(line)
        if not m:
            continue
        ts = int(m["s"]) * 1_000_000 + int(m["us"])
        body, pid = m["body"], int(m["pid"])
        if (e := cb.SWITCH.match(body)):
            get(int(e["prev"])).outs.append((ts, e["state"].startswith("R"), e["next_comm"].split("/")[0]))
            get(int(e["next"])).ins.append(ts)
        elif (e := cb.WAKING.match(body)):
            get(int(e["pid"])).wakes.append((ts, pid))
        elif (e := cb.FORK.match(body)):
            get(int(e["child"])).fork = (ts, int(e["parent"]))
        elif (e := cb.RAW_ENTER.match(body)):
            p, nr = get(pid), int(e["nr"])
            args = [int(a, 16) for a in e["args"].replace(" ", "").split(",") if a]
            if nr == cb.NR_SETNS and p.p19 is None:
                p.setns = args[1] if len(args) > 1 else None
            elif nr == cb.NR_CHDIR and p.p19 is not None and p.p19_end is None:
                p.p19_end = ts
            elif nr == NR_OPENAT and p.p19_end is not None and p.ready is None and p.write is not None:
                p.ready = p.write
        elif (e := cb.RAW_EXIT.match(body)):
            p, nr, ret = get(pid), int(e["nr"]), int(e["ret"])
            if nr == cb.NR_SETNS and p.p19 is None and ret == 0 and (p.setns or 0) & CLONE_NEWNET:
                p.p19 = ts
            elif nr == NR_WRITE and ret > 0 and p.p19_end is not None and p.ready is None:
                p.write = ts
    return procs


def next_in(proc, t):
    """The task's first switch in at or after t, or None (its switches in sorted)."""
    i = bisect.bisect_left(proc.ins, t)
    return proc.ins[i] if i < len(proc.ins) else None


def off_cpu(proc, a, b, into):
    """The task's time off the CPU in [a, b) (us), added to `into`: preempted (by who took the CPU) and blocked."""
    for out_ts, preempted, who in proc.outs:
        if not a <= out_ts < b:
            continue
        back = next_in(proc, out_ts + 1)
        d = min(back if back is not None else b, b) - out_ts
        if preempted:
            into["preempted"] = into.get("preempted", 0) + d
            into["by"][who] = into["by"].get(who, 0) + d
        else:
            into["blocked"] = into.get("blocked", 0) + d


def release(thread, t0, claim):
    """The create's start: its bench thread's return from the burst's barrier, the switch in after the thread's
    first blocked switch out in the burst, or, if it never blocked before its claim (the last to the barrier), its
    first switch in."""
    first = next_in(thread, t0)
    blocked = next((o for o, preempted, _ in thread.outs if t0 <= o < claim and not preempted), None)
    back = next_in(thread, blocked + 1) if blocked is not None else None
    return back if back is not None and back <= claim else (first if first is not None else t0)


def cut(rows, procs):
    """Each way's creates, each (its time, its parts, the agent's and the init's time off the CPU), and how many
    creates the trace couldn't place: a burst whose inits' claims didn't come from one thread a create."""
    for p in procs.values():
        p.ins.sort()
    inits = sorted((p for p in procs.values() if p.p19 is not None and p.p19_end is not None and p.ready is not None),
                   key=lambda p: p.p19)
    starts = [p.p19 for p in inits]
    per, unplaced = {w: [] for w in WAYS}, 0
    for r in rows:
        if r["warmup"]:
            continue
        for b in r["bursts"]:
            if b["arm"] not in WAYS:
                continue
            t0, t1 = b["t0_ns"] // 1000, -(-b["t1_ns"] // 1000)
            mine = inits[bisect.bisect_left(starts, t0):bisect.bisect_right(starts, t1)]
            # Each init's claim: its first wakeup in the burst by a thread other than its own parent, the create's.
            claimed = [(p, next(((t, w) for t, w in p.wakes if t0 <= t <= p.p19 and w != (p.fork or (0, 0))[1]),
                                None)) for p in mine]
            threads = sorted({wake[1] for _, wake in claimed if wake})
            if len(mine) != len(b["create_ms"]) or len(threads) != len(b["create_ms"]):
                unplaced += len(b["create_ms"])
                continue
            for p, (claim, tid) in claimed:
                th = procs[tid]
                start = release(th, t0, claim)
                ran = next_in(p, claim) or claim
                back = next_in(th, p.ready) or p.ready
                end = start + b["create_ms"][threads.index(tid)] * 1000
                parts = [claim - start, ran - claim, p.p19 - ran, p.p19_end - p.p19, p.ready - p.p19_end,
                         back - p.ready, end - back]
                agent, init = {"by": {}}, {"by": {}}
                off_cpu(th, start, claim, agent)
                off_cpu(th, back, end, agent)
                off_cpu(p, claim, p.ready, init)
                per[b["arm"]].append((end - start, parts, agent, init))
    return per, unplaced


def report(name, per, unplaced):
    out = [f"# {name}: each way's create in its parts (traced rounds; us, means)", "",
           f"Creates the trace couldn't place (a burst whose inits' claims didn't come from one thread a create): "
           f"{unplaced}.", "",
           "| way | creates | create p50 / p99 | which | " + " | ".join(PARTS)
           + " | agent preempted / blocked | init preempted / blocked |",
           "|---|---|---|---|" + "---|" * len(PARTS) + "---|---|"]
    for w in WAYS:
        xs = per[w]
        if not xs:
            continue
        total = [x[0] for x in xs]
        p99 = pct(total, 0.99)
        tail = [x for x in xs if x[0] >= p99]
        for which, sel in (("all", xs), ("slowest 1%", tail)):
            n = len(sel)
            means = [sum(x[1][i] for x in sel) / n for i in range(len(PARTS))]
            mean = lambda k, side, sel=sel, n=n: sum(x[side].get(k, 0) for x in sel) / n
            out.append(f"| {w} | {n} | {pct(total, 0.5):.0f} / {p99:.0f} | {which} | "
                       + " | ".join(f"{v:.0f}" for v in means)
                       + f" | {mean('preempted', 2):.0f} / {mean('blocked', 2):.0f}"
                       + f" | {mean('preempted', 3):.0f} / {mean('blocked', 3):.0f} |")
        for side, label in ((2, "agent"), (3, "init")):
            who = {}
            for x in tail:
                for k, v in x[side]["by"].items():
                    who[k] = who.get(k, 0) + v
            top = sorted(who.items(), key=lambda kv: -kv[1])[:5]
            out.append(f"- {w}: the slowest 1%'s {label} preempted by, us per create: "
                       + (", ".join(f"{k} {v / len(tail):.0f}" for k, v in top) or "nothing"))
    return "\n".join(out) + "\n"


def main(argv):
    out = argv[0]
    conc = argv[1] if len(argv) > 1 else "8"
    with open(os.path.join(out, f"rounds.c{conc}.traced", "rounds.jsonl")) as f:
        rows = [json.loads(line) for line in f]
    with open(os.path.join(out, f"mounts.c{conc}.txt"), errors="replace") as f:
        procs = read_trace(f)
    per, unplaced = cut(rows, procs)
    print(report(os.path.basename(out), per, unplaced), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
