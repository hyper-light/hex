#!/usr/bin/env python3
"""Turn kiln_export.py's export of kiln CI legs into hex-sim's trace text
(crates/hex-sim/src/replay.rs), for G2 step 3's recorded kiln workload (AC-T7.4.3).

Every rule below is counted in the trace's header, so nothing is changed or left out unsaid.

Nodes: each runner kiln knows the CPU threads and memory of that ran at least one leg of the export,
in name order: threads x 1000 millicores, its memory, its GPUs that have memory of their own (an
Apple GPU shares the machine's and reports none), and no background load. Kubernetes pods have no
facts, so the legs they ran are work here but the pods are not nodes. hex-sim's nodes carry no OS, so
a leg may land on a runner of another OS.

Legs: each job kiln ran (it has started_at and finished_at, unix seconds), in arrival order.
  - arrives: the earlier of queued_at and started_at (a job queued after it started was queued again
    for an attempt), less the earliest arrival in the export, in ns.
  - runs for: finished_at less started_at, at least 1 s, kiln's resolution.
  - need, measured: kiln's CPU seconds over the time it ran (mean cores) and its peak RSS, each
    capped at what the runner it ran on has (for a pod or no runner, the node with the most
    threads, then memory, so it fits a real one): a leg can't use more than its machine, so a
    measurement past it is counted as capped.
  - need, unmeasured: the median of the measured legs of the same repository, pipeline and job; else
    of the same repository and job; else the leg is left out and counted.
  - GPUs: 1 if a label names a GPU, else 0.
  - kind: `benchmark` if a label, the pipeline or the job names "bench"; else `build`: kiln's CI legs
    build and test code, changing the host as a build does.
A job kiln never ran is counted, not traced. An export it can't read is refused, naming what, and
nothing is written.

Options (G4, AC-T7.5.2), each off by default, so the trace without them is as before:
  --day YYYY-MM-DD   only legs arriving that UTC day; the rest are counted, not traced.
  --repos a,b,...    only these repositories' legs, in this order; the rest are counted, not traced.
  --owners           an owner per repository, each of weight 1 (in --repos order, else by name),
                     and each leg's tenancy: its repository's owner, band 0, no gang, no group.
"""

import argparse
import collections
import datetime
import gzip
import json
import statistics
import sys

NS = 10**9


class Refused(Exception):
    pass


def node_of(r):
    gpus = sum(1 for g in r.get("gpus") or [] if (g.get("memory") or 0) > 0)
    return int(r["threads"]) * 1000, int(r["mem_total"]), gpus


def kind(run, job):
    names = [run.get("pipeline") or "", job.get("key") or "", job.get("display") or ""]
    names += job.get("labels") or []
    return "benchmark" if any("bench" in n.lower() for n in names) else "build"


def convert(export, day=None, repos=None, owners=False):
    runs = export.get("runs")
    if not runs:
        raise Refused("the export holds no run")
    ran = [(r, j) for r in runs for j in r.get("jobs") or [] if j.get("started_at") and j.get("finished_at")]
    jobs = sum(len(r.get("jobs") or []) for r in runs)
    if not ran:
        raise Refused("the export holds no job kiln ran")
    used = {j.get("agent") for _, j in ran}
    nodes = {r["name"]: node_of(r) for r in export.get("runners") or []
             if r.get("name") in used and r.get("threads") and r.get("mem_total")}
    if not nodes:
        raise Refused("no runner that ran a leg has CPU and memory facts")
    largest = max(nodes.values())[:2]
    count = collections.Counter()
    capped = set()

    def measured(j):
        if j.get("cpu_seconds") is None or j.get("peak_rss") is None:
            return None
        dur = max(j["finished_at"] - j["started_at"], 1)
        cpu_cap, mem_cap = nodes[j["agent"]][:2] if j.get("agent") in nodes else largest
        cpu = round(j["cpu_seconds"] / dur * 1000)
        mem = int(j["peak_rss"])
        if cpu > cpu_cap or mem > mem_cap:
            capped.add(j["id"])
        return min(cpu, cpu_cap), min(mem, mem_cap)

    needs, by_job, by_key = {}, collections.defaultdict(list), collections.defaultdict(list)
    for r, j in ran:
        m = measured(j)
        needs[j["id"]] = m
        if m is not None:
            by_job[(r.get("repo_name"), r.get("pipeline"), j.get("key"))].append(m)
            by_key[(r.get("repo_name"), j.get("key"))].append(m)

    def median(ms):
        return (round(statistics.median(c for c, _ in ms)), round(statistics.median(m for _, m in ms)))

    since = until = None
    if day is not None:
        start = datetime.datetime.strptime(day, "%Y-%m-%d").replace(tzinfo=datetime.timezone.utc)
        since = int(start.timestamp())
        until = since + 86400
    legs = []
    for r, j in ran:
        q0 = min(j.get("queued_at") or j["started_at"], j["started_at"])
        if since is not None and not since <= q0 < until:
            count[f"arriving outside {day}, left out"] += 1
            continue
        if repos is not None and r.get("repo_name") not in repos:
            count[f"repo {r.get('repo_name')} not chosen, left out"] += 1
            continue
        need = needs[j["id"]]
        if need is None:
            same_job = by_job.get((r.get("repo_name"), r.get("pipeline"), j.get("key")))
            same_key = by_key.get((r.get("repo_name"), j.get("key")))
            if same_job:
                need, why = median(same_job), "unmeasured, the median of the same job"
            elif same_key:
                need, why = median(same_key), "unmeasured, the median of the same repository and job"
            else:
                count["unmeasured with nothing to take from, left out"] += 1
                continue
            count[why] += 1
        else:
            count["measured"] += 1
            if j["id"] in capped:
                count["capped at their runner"] += 1
        q, s, f = j.get("queued_at"), j["started_at"], j["finished_at"]
        if f < s:
            raise Refused(f"run {r.get('id')!r} job {j.get('id')!r}: finished before it started")
        if q is None or q > s:
            count["queued after they started or never, arriving at their start"] += 1
            q = s
        if f == s:
            count["ran 0 s, traced as 1 s"] += 1
        gpus = 1 if any("gpu" in l.lower() for l in j.get("labels") or []) else 0
        count[f"repo {r.get('repo_name')}"] += 1
        legs.append((q, r.get("repo_name") or "", r.get("number") or 0, j.get("key") or "", j["id"],
                     max(f - s, 1) * NS, need[0], need[1], gpus, kind(r, j)))
    if not legs:
        raise Refused("no leg could be traced")
    legs.sort()
    t0 = legs[0][0]
    names = sorted(nodes)
    order = list(repos) if repos is not None else sorted({leg[1] for leg in legs})
    out = [f"# source: {export.get('source', 'unstated')}",
           f"# made by bench/sched/kiln_trace.py from {len(runs)} runs and {jobs} jobs: {len(ran)} ran, "
           f"{jobs - len(ran)} never ran (not traced), {len(legs)} legs traced",
           *(f"# {n}: {count[n]}" for n in sorted(count)),
           "# nodes in runner-name order: " + " ".join(names)]
    if day is not None:
        out.insert(1, f"# day: {day} UTC, arrivals from its first leg's")
    if owners:
        out.append("# owners in order, weight 1 each: " + " ".join(order))
    for n in names:
        cpu, mem, gpus = nodes[n]
        out.append(f"node,{cpu},{mem},{gpus},0")
    if owners:
        out.extend("owner,1" for _ in order)
    for q, repo, _num, _key, _id, runs_for, cpu, mem, gpus, k in legs:
        tenancy = f",{order.index(repo)},0,-,-" if owners else ""
        out.append(f"leg,{(q - t0) * NS},{runs_for},{cpu},{mem},{gpus},{k}{tenancy}")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("export")
    ap.add_argument("out")
    ap.add_argument("--day")
    ap.add_argument("--repos", type=lambda v: v.split(","))
    ap.add_argument("--owners", action="store_true")
    a = ap.parse_args()
    try:
        opener = gzip.open if a.export.endswith(".gz") else open
        with opener(a.export, "rt") as fh:
            text = convert(json.load(fh), a.day, a.repos, a.owners)
    except (Refused, KeyError, ValueError, TypeError) as e:
        print(f"kiln_trace: refused: {type(e).__name__}: {e}", file=sys.stderr)
        return 1
    with open(a.out, "w") as fh:
        fh.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
