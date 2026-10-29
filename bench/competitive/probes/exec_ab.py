"""bench/competitive/probes: exec_ab.sh's reports (exec_ab.md).

  exec_ab.py summarize <run dir>   each round's exec and start p50 per build and libcrun's, and each build's
                                   over libcrun's in the same round, as markdown
  exec_ab.py sched <trace.txt>     one build's traced execs, by where the scheduler ran them: when the program's
                                   process first runs after its fork, how often it moved CPU before it ended,
                                   and the parts of the call around them (p50s, µs)
"""
import glob
import json
import os
import re
import statistics
import sys


def lifecycle_ms(path, op):
    with open(path) as f:
        r = json.load(f)
    return r.get("phases", {}).get("lifecycle", {}).get("ms", {}).get(op, []), r.get("errors") or {}


def summarize(run):
    rows = {}
    for path in sorted(glob.glob(os.path.join(run, "*.r*.json"))):
        m = re.match(r"(.+)\.r(\d+)\.json$", os.path.basename(path))
        rows.setdefault(m.group(1), {})[int(m.group(2))] = path
    with open(os.path.join(run, "env.txt")) as f:
        out = [f"# exec A/B: {os.path.basename(run.rstrip('/'))}", "", "```", f.read().rstrip(), "```", ""]
    for op in ("exec", "start"):
        lc = {}
        for rnd, path in rows.get("libcrun", {}).items():
            ms, _ = lifecycle_ms(path, op)
            if ms:
                lc[rnd] = statistics.median(ms)
        rounds = sorted({r for per in rows.values() for r in per})
        out += [f"## {op} p50 (ms), and over libcrun's in the same round", "",
                "| | " + " | ".join(f"r{r}" for r in rounds) + " | best | best over libcrun's best |",
                "|---|" + "---|" * (len(rounds) + 2)]
        for name in ["libcrun"] + sorted(n for n in rows if n != "libcrun"):
            cells, best = [], None
            for rnd in rounds:
                path = rows[name].get(rnd)
                ms, errors = lifecycle_ms(path, op) if path else ([], {})
                if not ms:
                    cells.append(f"none {errors}" if errors else "n/a")
                    continue
                p50 = statistics.median(ms)
                best = p50 if best is None else min(best, p50)
                ratio = f" ({p50 / lc[rnd]:.3f})" if name != "libcrun" and rnd in lc else ""
                cells.append(f"{p50:.3f}{ratio}")
            over = f"{best / min(lc.values()):.3f}" if name != "libcrun" and best and lc else ""
            out.append(f"| {name} | " + " | ".join(cells) + f" | {best:.3f} | {over} |" if best else
                       f"| {name} | " + " | ".join(cells) + " | n/a | |")
        out.append("")
    print("\n".join(out))


LINE = re.compile(r"^\s*(.+?)-(\d+)\s+\[(\d+)\]\s+\S+\s+(\d+\.\d+):\s+([a-z_0-9]+)[:(]?\s?(.*)$")
FIELD = re.compile(r"(\w+)=(\S+)")


def sched(trace):
    calls, cur = [], None
    with open(trace, errors="replace") as f:
        for line in f:
            m = LINE.match(line)
            if not m:
                continue
            pid, cpu, us, event, rest = int(m.group(2)), int(m.group(3)), float(m.group(4)) * 1e6, m.group(5), m.group(6)
            if event == "tracing_mark_write":
                if "hexbd B exec" in rest:
                    cur = {"agent": pid, "B": us, "events": []}
                elif "hexbd E exec" in rest and cur:
                    cur["E"] = us
                    calls.append(cur)
                    cur = None
                continue
            if cur is not None:
                cur["events"].append((us, cpu, pid, event, rest))
    parts = {k: [] for k in ("before the clone", "fork to the child's first run", "the program's exit to the agent's wake",
                             "the call")}
    moved = 0
    for c in calls:
        child = fork = clone = exited = first = woke = None
        migrated = False
        for us, cpu, pid, event, rest in c["events"]:
            f = dict(FIELD.findall(rest))
            if event == "sys_clone3" and pid == c["agent"] and not rest.startswith("->") and clone is None:
                clone = us
            elif event == "sched_process_fork" and child is None:
                child, fork = f.get("child_pid"), us
            elif event == "sched_switch" and child and first is None and f.get("next_pid") == child:
                first = us
            elif event == "sched_migrate_task" and child and f.get("pid") == child:
                migrated = True
            elif event == "sys_exit_group" and child and str(pid) == child:
                exited = us
            elif event == "sched_wakeup" and exited is not None and woke is None and f.get("pid") == str(c["agent"]):
                woke = us
        if None in (clone, fork, first, exited, woke):
            continue
        moved += migrated
        parts["before the clone"].append(clone - c["B"])
        parts["fork to the child's first run"].append(first - fork)
        parts["the program's exit to the agent's wake"].append(woke - exited)
        parts["the call"].append(c["E"] - c["B"])
    n = len(parts["the call"])
    out = [f"## Scheduler trace: {os.path.basename(trace)}", "", f"{n} traced execs read whole of {len(calls)}; "
           f"the program's process moved CPU before it ended in {moved} of {n}.", "", "| part | p50 (µs) | p90 (µs) |",
           "|---|---|---|"]
    for name, v in parts.items():
        if v:
            v.sort()
            out.append(f"| {name} | {statistics.median(v):.0f} | {v[len(v) * 9 // 10]:.0f} |")
    print("\n".join(out + [""]))


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "summarize":
        summarize(sys.argv[2])
    elif len(sys.argv) == 3 and sys.argv[1] == "sched":
        sched(sys.argv[2])
    else:
        sys.exit(__doc__)
