#!/usr/bin/env python3
"""Goal 17's record of the mount-namespace ways, from mntns_record.sh's cells (its `exits`: time, cell, exit, results
dir, seconds). A cell is one pass (p1, p2) at one load (L6, L12 busy loops): create_breakdown.sh's run of the ways and
the classic root (goal 17's baseline, the lead's ruling of 2026-11-09) interleaved by rounds.py (MOUNTS_ARMS), once
untraced and once traced.

From each cell:
- every hex arm's own counts (seam_oci's serve phase, arms.json) must show each of its creates made the way it asked,
  and the classic arm's all the classic root's, none pruned, untraced and traced, or the run is refused
  (non-vacuity, exit 3); so is a record whose cells hold no classic arm;
- the arms' creates, from the untraced rounds (rounds.py's summary);
- part 19 per way, from the traced rounds: each span (create_breakdown.py mounts) is claimed by the burst whose
  window holds its start. Every hex burst's window must hold one span per lifecycle, and no span may lie outside
  every window, or the run is refused.
Then goal 17's verdicts: in each pass, the default way's create p50 and p99 against the classic root's, the comparison
the goal's text names (which measures goal 5's prune with goal 17's ways, said beside it); per load, best of 2 as
summarize.py takes it (the pass with the lower p50, its figures whole), each new way's create p50 against the copy's,
goal 17's own record of the ways, and the default way's part 19 p99 against the bar (0.5 ms); runtime-tools' tests crun
passes, all passed by hex. Last, the record lines for mntns_default.txt: each way's create p50 in ms, the median of its
cells', the run and the commit; the classic root is never a default, so has none.
Run: python3 -I mntns_record.py EXITS RUN REV MODE [RUNTIME_TOOLS_DIR]
"""
import bisect
import json
import math
import os
import statistics
import sys

WAYS = ("copy", "tree", "empty")
# Goal 17's baseline arm: the classic root (rounds.py's hex:classic), never a default.
BASELINE = "classic"
# Ada's under-load bar: part 19's p99 at most 0.5 ms (goal5-mounts.md §8 and §11; decision 6's HEX-MNTNS-7).
BAR_US = 500


class Refused(Exception):
    pass


def pct(samples, q):
    """Nearest rank, as summarize.py's and rounds.py's."""
    s = sorted(samples)
    return s[max(0, math.ceil(q * len(s)) - 1)] if s else None


def read(path):
    with open(path) as f:
        return json.load(f)


def rows_of(path):
    with open(path) as f:
        return [json.loads(line) for line in f if line.strip()]


def check_ways(arms, cell, kind):
    """Each hex arm's creates, all made the way it asked (seam_oci's serve phase's `mount_namespaces`), and the classic
    arm's all the classic root's, none pruned (its `roots`)."""
    for arm in WAYS + (BASELINE,):
        said = arms.get(arm)
        if said is None:
            continue
        serve = said.get("result", {}).get("phases", {}).get("serve")
        if said.get("exit") != 0 or serve is None:
            raise Refused(f"cell {cell} ({kind}): arm {arm} ended {said.get('exit')} without its serve phase: "
                          f"{json.dumps(said.get('result', {}).get('errors'))}")
        made = serve.get("mount_namespaces", {})
        if arm == BASELINE:
            roots = serve.get("roots", {})
            if roots.get("classic") != serve["lifecycles"] or roots.get("pruned") or any(made.values()):
                raise Refused(f"cell {cell} ({kind}): the classic arm served {serve['lifecycles']} creates, which took "
                              f"{roots}")
        elif made.get(arm, 0) != serve["lifecycles"] or any(made.get(w, 0) for w in WAYS if w != arm):
            raise Refused(f"cell {cell} ({kind}): arm {arm} served {serve['lifecycles']} creates, which took {made}")


def claim(rows, spans, cell):
    """Each hex burst's part 19 spans (us) and their time preempted, by the window that holds each span's start;
    warmup rounds' bursts claim theirs and drop them."""
    windows = sorted((b["t0_ns"] // 1000, -(-b["t1_ns"] // 1000), r["warmup"], b["arm"], len(b["create_ms"]))
                     for r in rows for b in r["bursts"] if b["arm"] in WAYS + (BASELINE,))
    starts = [w[0] for w in windows]
    held = [0] * len(windows)
    by_way = {w: [] for w in WAYS + (BASELINE,)}
    for start, span, preempted in spans:
        i = bisect.bisect_right(starts, start) - 1
        if i < 0 or start > windows[i][1]:
            raise Refused(f"cell {cell}: a part 19 starting at {start} us lies in no burst's window")
        held[i] += 1
        if not windows[i][2]:
            by_way[windows[i][3]].append((span, preempted))
    for (t0, _, _, way, n), k in zip(windows, held):
        if k != n:
            raise Refused(f"cell {cell}: {way}'s burst at {t0} us made {n} creates and holds {k} part 19 spans")
    return by_way


def part19(samples):
    spans = [s for s, _ in samples]
    calm = [s for s, p in samples if p == 0]
    return {"n": len(spans), "p50": pct(spans, 0.5), "p99": pct(spans, 0.99),
            "unpreempted_p99": pct(calm, 0.99), "max": max(spans) if spans else None}


def read_cell(name, out):
    """A cell's figures. Its concurrency is its rounds' own: the one rounds.c<N>.untraced its run left."""
    concs = [d[len("rounds.c"):-len(".untraced")] for d in os.listdir(out)
             if d.startswith("rounds.c") and d.endswith(".untraced")]
    if len(concs) != 1:
        raise Refused(f"cell {name}: {len(concs)} concurrencies of rounds in {out}, not one")
    conc = concs[0]
    untraced, traced = (os.path.join(out, f"rounds.c{conc}.{k}") for k in ("untraced", "traced"))
    for kind, d in (("untraced", untraced), ("traced", traced)):
        check_ways(read(os.path.join(d, "arms.json")), name, kind)
    summary = read(os.path.join(untraced, "summary.json"))
    if summary.get("failed"):
        raise Refused(f"cell {name}: its untraced rounds failed: {summary['failed']}")
    traced_rows = rows_of(os.path.join(traced, "rounds.jsonl"))
    spans = read(os.path.join(out, f"mounts.c{conc}.summary.json"))["spans"]
    loads = [r["load"]["load1"] for r in rows_of(os.path.join(untraced, "rounds.jsonl")) + traced_rows
             if not r["warmup"]]
    return {"create": {a: p["create_ms"] for a, p in summary["per_arm"].items()},
            "against": {a: {"copy": p.get("against_copy"), BASELINE: p.get("against_" + BASELINE)}
                        for a, p in summary["per_arm"].items()},
            "rounds": summary["rounds"],
            "part19": {w: part19(s) for w, s in claim(traced_rows, spans, name).items() if s},
            "load1": (min(loads), pct(loads, 0.5), max(loads)) if loads else None}


def best(cells, metric, arm):
    """summarize.py's best of N: the cell whose p50 is lowest, its figures whole, and the pass it came from."""
    have = [(c[metric][arm], p) for p, c in cells.items() if c[metric].get(arm, {}).get("p50") is not None]
    return min(have, key=lambda v: v[0]["p50"]) if have else (None, None)


def runtime_tools(rt_dir):
    """The tests crun passes and how many of them hex passes (results.tsv: name, pass|fail|skip, ...)."""
    passed = {}
    for rt in ("hex", "crun"):
        with open(os.path.join(rt_dir, rt, "results.tsv")) as f:
            passed[rt] = {line.split("\t")[0] for line in f if line.split("\t")[1:2] == ["pass"]}
    return sorted(passed["crun"]), sorted(passed["crun"] - passed["hex"]), sorted(passed["hex"] - passed["crun"])


def main(argv):
    exits, run, rev, mode = argv[:4]
    rt_dir = argv[4] if len(argv) > 4 else None
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
        bare = [f"{p}-L{L}" for L, by_pass in cells.items() for p, c in by_pass.items()
                if BASELINE not in c["create"]]
        if bare:
            raise Refused(f"no cell of the classic root: {', '.join(sorted(bare))} ran without it")
    except Refused as e:
        print(f"refusing: {e}")
        return 3
    fmt = lambda v, f="{:.3f}": "n/a" if v is None else f.format(v)
    print(f"# {run} at {rev} ({mode}{'' if mode == 'host' else ', not of record'})")
    print()
    print("Per cell: each way's create (untraced) and part 19 (traced), and the load each cell's rounds met.")
    print()
    print("| loads | pass | arm | create p50 / p99, ms | below classic (rounds) | below copy (rounds) "
          "| part 19 p50 / p99 / unpreempted p99, us | load1 min / p50 / max |")
    print("|---|---|---|---|---|---|---|---|")
    for L in sorted(cells):
        for p in sorted(cells[L]):
            c = cells[L][p]
            for way in [a for a in c["create"] if a in WAYS] + [a for a in c["create"] if a not in WAYS]:
                cr, vs, pt = c["create"][way], c["against"].get(way, {}), c["part19"].get(way)
                below = {k: f"{v['rounds_below']} of {v['rounds']}" if v else "" for k, v in vs.items()}
                p19 = f"{pt['p50']} / {pt['p99']} / {fmt(pt['unpreempted_p99'], '{}')}" if pt else ""
                ld = c["load1"]
                print(f"| {L} | {p} | {way} | {fmt(cr['p50'])} / {fmt(cr['p99'])} | {below.get(BASELINE, '')} | "
                      f"{below.get('copy', '')} | {p19} | "
                      + (f"{ld[0]:.1f} / {ld[1]:.1f} / {ld[2]:.1f}" if ld else "") + " |")
    lines = {}
    for way in WAYS:
        p50s = [c["create"][way]["p50"] for by_pass in cells.values() for c in by_pass.values() if way in c["create"]]
        if not p50s:
            print(f"refusing: no cell of {way}")
            return 3
        lines[way] = statistics.median(p50s)
    default = min(lines, key=lines.get)
    print()
    print(f"## Verdicts (best of {max(len(v) for v in cells.values())} per load, summarize.py's)")
    print()
    if default == "copy":
        print("- the default way is the copy: neither new way's create p50 is below it (the medians of its cells: "
              + ", ".join(f"{w} {lines[w]:.3f}" for w in WAYS) + " ms).")
    for L in sorted(cells):
        by_pass = cells[L]
        for new in ("tree", "empty"):
            held = [p for p, c in by_pass.items() if c["create"][new]["p50"] < c["create"]["copy"]["p50"]]
            print(f"- {L} loops: {new}'s create p50 below the copy's in the same run in {len(held)} of "
                  f"{len(by_pass)} passes ({', '.join(sorted(held)) or 'none'}).")
        b19, bp = best(by_pass, "part19", default)
        bar = "meets" if b19 and b19["p99"] <= BAR_US else "misses"
        print(f"- {L} loops: the default way ({default}): part 19 p99 {fmt(b19 and b19['p99'], '{}')} us, best of "
              f"{len(by_pass)} ({bp}): {bar} the bar ({BAR_US} us); in each pass "
              + ", ".join(f"{p} {c['part19'].get(default, {}).get('p99')}" for p, c in sorted(by_pass.items()))
              + ".")
        for p, c in sorted(by_pass.items()):
            d, k, vs = c["create"][default], c["create"][BASELINE], c["against"][default][BASELINE]
            print(f"- {L} loops, {p}: create p50: {default} {d['p50']:.3f} ms against classic {k['p50']:.3f} ms, the "
                  f"new path below: {'yes' if d['p50'] < k['p50'] else 'no'} ({vs['rounds_below']} of {vs['rounds']} "
                  f"rounds below).")
            print(f"- {L} loops, {p}: create p99: {default} {d['p99']:.3f} ms against classic {k['p99']:.3f} ms, "
                  f"improved: {'yes' if d['p99'] < k['p99'] else 'no'}.")
    print("- classic against the default way measures goal 5's prune, with the node's /dev and masked-directory pools "
          "the classic init makes its own of, and goal 17's ways together: the comparison goal 17's text names. Each "
          "new way against the copy is goal 17's own record of the ways.")
    if rt_dir:
        crun, missed, more = runtime_tools(rt_dir)
        print(f"- runtime-tools: hex passes {len(crun) - len(missed)} of the {len(crun)} tests crun passes"
              + (f"; fails {', '.join(missed)}" if missed else "")
              + (f"; passes {len(more)} crun fails ({', '.join(more)})" if more else "") + ".")
    print()
    print("# record lines for mntns_default.txt:")
    for way in WAYS:
        print(f"{way} {lines[way]:.3f} {run} {rev}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
