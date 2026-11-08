#!/usr/bin/env python3
"""Goal 17's rootless variant, from rootless_record.sh's cells (its `exits`: time, cell, exit, results dir, seconds). A
cell is one pass (p1, p2) at one load (L6, L12): rounds.py's arms libcrun and hex, both rootless, interleaved round by
round (rootless_cell.sh).

From each cell: each arm's creates over the timed rounds (rounds.py's summary), and hex against libcrun, the rounds
whose median create is below libcrun's and the pooled p50s' ratio; and the load the rounds met. Then, per load, best of
2 as summarize.py takes it (the pass with the lower p50, its figures whole). Then runtime-tools' rootless subset: the
tests crun passes rootless, how many of them hex passes; and, given crun's results as root, the tests crun passes as
root and fails rootless, which need root and are left out of the subset. Part 19 isn't read rootless: its trace is
ftrace's, and tracefs is root's. Exits 3 when a cell failed.
Run: python3 -I rootless_record.py EXITS RUN REV [RT_ROOTLESS_DIR [CRUN_AS_ROOT_RESULTS_TSV]]
"""
import json
import math
import os
import sys


class Refused(Exception):
    pass


def pct(samples, q):
    s = sorted(samples)
    return s[max(0, math.ceil(q * len(s)) - 1)] if s else None


def read_cell(name, out):
    with open(os.path.join(out, "rounds", "summary.json")) as f:
        summary = json.load(f)
    if summary.get("failed"):
        raise Refused(f"cell {name}: its rounds failed: {summary['failed']}")
    with open(os.path.join(out, "rounds", "rounds.jsonl")) as f:
        loads = [r["load"]["load1"] for r in map(json.loads, f) if not r["warmup"]]
    return {"create": {a: p["create_ms"] for a, p in summary["per_arm"].items()},
            "against": {a: p.get("against_libcrun") for a, p in summary["per_arm"].items()},
            "load1": (min(loads), pct(loads, 0.5), max(loads)) if loads else None}


def passes(path):
    with open(path) as f:
        return {line.split("\t")[0] for line in f if line.split("\t")[1:2] == ["pass"]}


def main(argv):
    exits, run, rev = argv[:3]
    rt_dir = argv[3] if len(argv) > 3 else None
    crun_root = argv[4] if len(argv) > 4 else None
    cells = {}
    try:
        for line in open(exits):
            fields = line.split()
            if len(fields) < 4:
                raise Refused(f"a cell ended with no results dir: {line.strip()}")
            _, name, rc, out = fields[:4]
            if rc != "0":
                raise Refused(f"cell {name} exited {rc}")
            p, loads = name.split("-")
            cells.setdefault(int(loads[1:]), {})[p] = read_cell(name, out)
        if not cells:
            raise Refused("no cell ran")
    except Refused as e:
        print(f"refusing: {e}")
        return 3
    fmt = lambda v: "n/a" if v is None else f"{v:.3f}"
    print(f"# {run} at {rev} (rootless: hex and libcrun, each box in a user namespace of its own)")
    print()
    print("| loads | pass | arm | create p50 / p99, ms | below libcrun (rounds) | p50 over libcrun's | load1 min / p50 / max |")
    print("|---|---|---|---|---|---|---|")
    for L in sorted(cells):
        for p in sorted(cells[L]):
            c = cells[L][p]
            for arm in c["create"]:
                cr, vs, ld = c["create"][arm], c["against"].get(arm), c["load1"]
                print(f"| {L} | {p} | {arm} | {fmt(cr['p50'])} / {fmt(cr['p99'])} | "
                      + (f"{vs['rounds_below']} of {vs['rounds']} | {fmt(vs['p50_ratio'])}" if vs else " | ")
                      + " | " + (f"{ld[0]:.1f} / {ld[1]:.1f} / {ld[2]:.1f}" if ld else "") + " |")
    print()
    print(f"## Hex rootless against libcrun rootless (best of {max(len(v) for v in cells.values())} per load, "
          "summarize.py's)")
    print()
    for L in sorted(cells):
        by_pass = cells[L]
        best = {a: min(((c["create"][a], p) for p, c in by_pass.items() if a in c["create"]),
                       key=lambda v: v[0]["p50"]) for a in ("hex", "libcrun")}
        (h, hp), (lc, lp) = best["hex"], best["libcrun"]
        print(f"- {L} loops, best of {len(by_pass)}: hex create p50 {fmt(h['p50'])} ms ({hp}), libcrun "
              f"{fmt(lc['p50'])} ({lp}): {h['p50'] / lc['p50']:.3f}x libcrun's; p99 {fmt(h['p99'])} against "
              f"{fmt(lc['p99'])}.")
        below = [p for p, c in by_pass.items() if c["create"]["hex"]["p50"] < c["create"]["libcrun"]["p50"]]
        print(f"- {L} loops: hex's create p50 below libcrun's in the same run in {len(below)} of {len(by_pass)} "
              f"passes ({', '.join(sorted(below)) or 'none'}).")
    if rt_dir:
        crun, hexes = passes(os.path.join(rt_dir, "crun", "results.tsv")), passes(os.path.join(rt_dir, "hex", "results.tsv"))
        missed, more = sorted(crun - hexes), sorted(hexes - crun)
        print(f"- runtime-tools rootless: hex passes {len(crun) - len(missed)} of the {len(crun)} tests crun passes "
              "rootless" + (f"; fails {', '.join(missed)}" if missed else "")
              + (f"; passes {len(more)} crun fails rootless ({', '.join(more)})" if more else "") + ".")
        if crun_root:
            need = sorted(passes(crun_root) - crun)
            print(f"- left out as needing root: {len(need)} tests crun passes as root and fails rootless"
                  + (f": {', '.join(need)}" if need else "") + ".")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
