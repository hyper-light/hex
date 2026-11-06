#!/usr/bin/env python3
"""What each controller's teardown of a box's cgroup costs (css_cost_inside.sh's trace).

`css_cost.py read <run dir>`: from the run's trace.txt, every kworker work item by function (count, total and mean
time, the slowest), and each controller's css callback by kprobe entry and return on the same CPU and task (count,
total, mean, the slowest), each per box made and deleted (seam.json's lifecycles). `css_cost.py test` is its
selftest.
"""
import collections
import json
import re
import sys

LINE = re.compile(r"^\s*(?P<comm>.+?)-(?P<pid>\d+)\s+\[(?P<cpu>\d+)\]\s+\S+\s+(?P<ts>\d+\.\d+): (?P<ev>[\w]+): (?P<rest>.*)$")
WORK = re.compile(r"function (?P<f>\S+)")


def costs(lines):
    """({work function: [ms...]}, {css callback: [ms...]})."""
    works, calls = collections.defaultdict(list), collections.defaultdict(list)
    open_work, open_call = {}, {}
    for raw in lines:
        m = LINE.match(raw)
        if not m:
            continue
        ts, ev, key = float(m["ts"]), m["ev"], (m["cpu"], m["pid"])
        if ev == "workqueue_execute_start":
            w = WORK.search(m["rest"])
            open_work[key] = (ts, w["f"] if w else "?")
        elif ev == "workqueue_execute_end":
            o = open_work.pop(key, None)
            if o:
                works[o[1]].append((ts - o[0]) * 1e3)
        elif ev.startswith("e_"):
            open_call[(m["pid"], ev[2:])] = ts
        elif ev.startswith("x_"):
            t0 = open_call.pop((m["pid"], ev[2:]), None)
            if t0 is not None:
                calls[ev[2:]].append((ts - t0) * 1e3)
    return works, calls


def table(w, title, d, boxes):
    w(f"\n{title} (per box: total over {boxes} boxes)\n\n| function | count | per box | total ms | mean ms | max ms | ms per box |\n|---|---|---|---|---|---|---|\n")
    for f, xs in sorted(d.items(), key=lambda kv: -sum(kv[1])):
        w(f"| {f} | {len(xs)} | {len(xs) / boxes:.2f} | {sum(xs):.1f} | {sum(xs) / len(xs):.4f} | {max(xs):.3f} | {sum(xs) / boxes:.4f} |\n")


def selftest():
    lines = [
        "kworker/4:1-99 [004] ..... 1.000000: workqueue_execute_start: work struct 0: function css_killed_work_fn",
        "kworker/4:1-99 [004] ..... 1.000100: e_mem_cgroup_css_offline: (mem_cgroup_css_offline+0x0/0x10)",
        "kworker/4:1-99 [004] ..... 1.000400: x_mem_cgroup_css_offline: (offline_css+0x1/0x2 <- mem_cgroup_css_offline)",
        "kworker/4:1-99 [004] ..... 1.000500: workqueue_execute_end: work struct 0: function css_killed_work_fn",
    ]
    works, calls = costs(lines)
    assert [round(x, 6) for x in works["css_killed_work_fn"]] == [0.5], works
    assert [round(x, 6) for x in calls["mem_cgroup_css_offline"]] == [0.3], calls
    print("selftest: ok")


def main(argv):
    if argv[1:] == ["test"]:
        selftest()
        return 0
    run = argv[2]
    with open(f"{run}/trace.txt", errors="replace") as f:
        works, calls = costs(f)
    boxes = len(json.load(open(f"{run}/seam.json"))["phases"]["lifecycle"]["ms"]["create"])
    w = sys.stdout.write
    w(f"# css_cost: {run}\n")
    table(w, "Work items by function", {k: v for k, v in works.items() if len(v) >= boxes // 10}, boxes)
    table(w, "Each controller's css callbacks", calls, boxes)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
