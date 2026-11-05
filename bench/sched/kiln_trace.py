#!/usr/bin/env python3
"""Turn an export of kiln CI legs into hex-sim's trace text (crates/hex-sim/src/replay.rs), for
G2 step 3's recorded kiln workload (AC-T7.4.3).

Input, one JSON file:
  {"source": "<where and when it was exported>",
   "runners": [{"name": str, "cpu_milli": int, "mem_bytes": int, "gpus": int}, ...],
   "runs": [<kiln ci.runs.get answers, each with its "legs">, ...]}

Each leg kiln ran (it has started_at and finished_at, in unix seconds) becomes one `leg` line:
  - arrives: queued_at less the earliest queued_at in the export, in ns; runs for finished_at less
    started_at, in ns, at least 1 s, kiln's own resolution.
  - need: the leg's declared requirements (cpu as Kubernetes CPU, memory as a Kubernetes quantity,
    gpu a count). A leg that declared none takes --undeclared CPU_MILLI,MEM_BYTES, which the trace's
    source line records; without the flag such a leg is refused.
  - kind: `benchmark` if its job name holds "bench", `build` if it holds build, test, gate, clippy,
    check or lint, else `plain`.
Each runner becomes one `node` line, with no background load. A leg kiln never ran (no start or no
end) is counted in the trace's header, never silently dropped. Anything else the script can't read is
refused, naming the run and the leg, and nothing is written.
"""

import argparse
import json
import re
import sys

KIND_BENCH = re.compile(r"bench", re.I)
KIND_BUILD = re.compile(r"build|test|gate|clippy|check|lint", re.I)
CPU = re.compile(r"^(\d+)(?:\.(\d{1,3}))?(m?)$")
MEM = re.compile(r"^(\d+)(Ki|Mi|Gi|Ti|k|M|G|T)?$")
MEM_UNIT = {None: 1, "k": 10**3, "M": 10**6, "G": 10**9, "T": 10**12,
            "Ki": 2**10, "Mi": 2**20, "Gi": 2**30, "Ti": 2**40}
NS = 10**9


class Refused(Exception):
    pass


def cpu_milli(text):
    m = CPU.match(str(text).strip())
    if not m:
        raise Refused(f"cpu {text!r} is not a Kubernetes CPU quantity")
    whole, frac, milli = m.groups()
    if milli:
        if frac:
            raise Refused(f"cpu {text!r}: a fraction of a millicore")
        return int(whole)
    return int(whole) * 1000 + int((frac or "").ljust(3, "0"))


def mem_bytes(text):
    m = MEM.match(str(text).strip())
    if not m:
        raise Refused(f"memory {text!r} is not a Kubernetes quantity")
    return int(m.group(1)) * MEM_UNIT[m.group(2)]


def kind(job):
    if KIND_BENCH.search(job):
        return "benchmark"
    if KIND_BUILD.search(job):
        return "build"
    return "plain"


def convert(export, undeclared):
    runners = export.get("runners") or []
    if not runners:
        raise Refused("the export names no runner")
    names = sorted(r["name"] for r in runners)
    if len(set(names)) != len(names):
        raise Refused("a runner named twice")
    by_name = {r["name"]: r for r in runners}
    legs, not_run, defaulted = [], 0, 0
    for run in export.get("runs") or []:
        for leg in run.get("legs") or []:
            where = f"run {run.get('id')!r} leg {leg.get('key')!r}"
            q, s, f = leg.get("queued_at"), leg.get("started_at"), leg.get("finished_at")
            if s is None or f is None:
                not_run += 1
                continue
            if q is None or not (q <= s <= f):
                raise Refused(f"{where}: queued, started and finished out of order")
            if leg.get("runner") not in by_name:
                raise Refused(f"{where}: runner {leg.get('runner')!r} is not in the export's runners")
            req = leg.get("requirements") or {}
            if "cpu" in req and "memory" in req:
                cpu, mem = cpu_milli(req["cpu"]), mem_bytes(req["memory"])
            elif undeclared is None:
                raise Refused(f"{where}: declares no cpu and memory, and --undeclared was not given")
            else:
                cpu = cpu_milli(req["cpu"]) if "cpu" in req else undeclared[0]
                mem = mem_bytes(req["memory"]) if "memory" in req else undeclared[1]
                defaulted += 1
            gpus = int(req.get("gpu", 0))
            legs.append((q, str(run.get("id")), str(leg.get("key")),
                         max(f - s, 1) * NS, cpu, mem, gpus, kind(str(leg.get("job", "")))))
    if not legs:
        raise Refused("the export holds no leg kiln ran")
    legs.sort()
    t0 = legs[0][0]
    out = [f"# source: {export.get('source', 'unstated')}",
           f"# legs: {len(legs)} ran; {not_run} never ran (not in the trace); "
           f"{defaulted} declared no cpu or memory"
           + (f" and took {undeclared[0]} millicores, {undeclared[1]} bytes" if defaulted else ""),
           "# nodes in runner-name order: " + " ".join(names)]
    for n in names:
        r = by_name[n]
        out.append(f"node,{int(r['cpu_milli'])},{int(r['mem_bytes'])},{int(r.get('gpus', 0))},0")
    for q, _run, _key, runs_for, cpu, mem, gpus, k in legs:
        out.append(f"leg,{(q - t0) * NS},{runs_for},{cpu},{mem},{gpus},{k}")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("export")
    ap.add_argument("out")
    ap.add_argument("--undeclared", help="CPU_MILLI,MEM_BYTES for a leg that declared none")
    a = ap.parse_args()
    undeclared = None
    if a.undeclared:
        c, m = a.undeclared.split(",")
        undeclared = (int(c), int(m))
    try:
        with open(a.export) as fh:
            text = convert(json.load(fh), undeclared)
    except (Refused, KeyError, ValueError, TypeError) as e:
        print(f"kiln_trace: refused: {e}", file=sys.stderr)
        return 1
    with open(a.out, "w") as fh:
        fh.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
