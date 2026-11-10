#!/usr/bin/env python3
"""Goal 17's record of the mount-namespace ways, from mntns_record.sh's cells (its `exits`: time, cell, exit, results
dir, seconds). The record runs in two phases (the lead's rulings of 2026-11-09): phase A, the classic root (goal 17's
baseline, rounds.py's hex:classic) and the three ways interleaved, for the verdicts against the classic root; phase B,
the three ways alone, for part 19's bar and the default, which the classic arm's whole-namespace copies and detached
old roots on the global mount locks would otherwise reach (rec17, mntns-host-20261109T165122Z). A cell is one phase's
pass (p1, p2) at one load (L6, L12 busy loops), named <phase>-p<pass>-L<loops>: create_breakdown.sh's run of the
phase's arms interleaved by rounds.py (MOUNTS_ARMS), once untraced and once traced.

From each cell:
- every hex arm's own counts (seam_oci's serve phase, arms.json) must show each of its creates made the way it asked,
  and the classic arm's all the classic root's, none pruned, untraced and traced; phase A's cells must each hold the
  classic arm, phase B's none, and the record both phases; or the run is refused (non-vacuity, exit 3);
- the arms' creates, from the untraced rounds (rounds.py's summary, each arm against every other);
- part 19 per arm, from the traced rounds: each span (create_breakdown.py mounts) is claimed by the burst whose window
  holds its start. Every hex burst's window must hold one span per lifecycle, and no span may lie outside every
  window, or the run is refused.

Then goal 17's verdicts, each naming its phase:
- B: the default way, by the lead's rule: the way whose create p50 is below each other's at the record's 95%, by a
  one-sided sign test on the rounds below over every timed round of phase B (ties left out), where one is; else the
  way with the lowest part 19 p99 at the heaviest load, then the lowest create p99 there (best of 2, summarize.py's:
  the pass with the lower p50, its figures whole), the ways' own order last; the rest ranked so after it. Each new
  way's create p50 against the copy's in each pass (goal 17's own record of the ways), and the default way's part 19
  p99 against the bar (0.5 ms), best of 2 per load.
- A: in each pass at each load, the default way's create p50 and p99 against the classic root's, the comparison the
  goal's text names, with its caveat.
- runtime-tools' tests crun passes, all passed by hex.
Last, the record lines for mntns_default.txt, in the default's order: each way's create p50 in ms, the median of its
phase B cells', the run and the commit; the classic root is never a default, so has none.
Run: python3 -I mntns_record.py EXITS RUN REV MODE [RUNTIME_TOOLS_DIR]
"""
import bisect
import json
import math
import os
import re
import statistics
import sys

WAYS = ("copy", "tree", "empty")
# Goal 17's baseline arm: the classic root (rounds.py's hex:classic), never a default.
BASELINE = "classic"
# Ada's under-load bar: part 19's p99 at most 0.5 ms (goal5-mounts.md §8 and §11; decision 6's HEX-MNTNS-7).
BAR_US = 500
# The record's confidence: Ada's rule that a p99 claim stand at 95% (mntns_record.sh's 299 rounds,
# ln(0.05)/ln(0.99) = 298.07), which the sign test that lets p50 decide the default holds to as well.
CONFIDENCE = 0.95
CELL = re.compile(r"^(?P<phase>[AB])-(?P<pass>p\d+)-L(?P<loads>\d+)$")


class Refused(Exception):
    pass


def pct(samples, q):
    """Nearest rank, as summarize.py's and rounds.py's."""
    s = sorted(samples)
    return s[max(0, math.ceil(q * len(s)) - 1)] if s else None


def sign_k(n, confidence=CONFIDENCE):
    """The fewest of n rounds (ties left out) in which one arm's median create must be below another's for a one-sided
    sign test to say its p50 is below at `confidence`: the least k with P(Binomial(n, 1/2) >= k) <= 1 - confidence;
    n + 1 where none is."""
    tail = 0.0
    for k in range(n, -1, -1):
        tail += math.comb(n, k) / 2 ** n
        if tail > 1 - confidence:
            return k + 1
    return 0


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
    by_arm = {a: [] for a in WAYS + (BASELINE,)}
    for start, span, preempted in spans:
        i = bisect.bisect_right(starts, start) - 1
        if i < 0 or start > windows[i][1]:
            raise Refused(f"cell {cell}: a part 19 starting at {start} us lies in no burst's window")
        held[i] += 1
        if not windows[i][2]:
            by_arm[windows[i][3]].append((span, preempted))
    for (t0, _, _, arm, n), k in zip(windows, held):
        if k != n:
            raise Refused(f"cell {cell}: {arm}'s burst at {t0} us made {n} creates and holds {k} part 19 spans")
    return by_arm


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
    per = summary["per_arm"]
    return {"create": {a: p["create_ms"] for a, p in per.items()},
            "against": {a: {o: p.get("against_" + o) for o in per if o != a} for a, p in per.items()},
            "rounds": summary["rounds"],
            "part19": {a: part19(s) for a, s in claim(traced_rows, spans, name).items() if s},
            "load1": (min(loads), pct(loads, 0.5), max(loads)) if loads else None}


def best(cells, metric, arm):
    """summarize.py's best of N: the cell whose p50 is lowest, its figures whole, and the pass it came from."""
    have = [(c[metric][arm], p) for p, c in sorted(cells.items()) if c[metric].get(arm, {}).get("p50") is not None]
    return min(have, key=lambda v: v[0]["p50"]) if have else (None, None)


def rank(phase_b):
    """The ways in the default's order, by the lead's rule, and how the first was placed: repeatedly, the way whose
    rounds below each other remaining way's pass the sign test over every timed round of phase B, where one does;
    else the remaining way with the lowest part 19 p99 at the heaviest load, then the lowest create p99 there (best
    of 2), the ways' own order last."""
    below = {w: {v: 0 for v in WAYS} for w in WAYS}
    for by_pass in phase_b.values():
        for c in by_pass.values():
            for w in WAYS:
                for v in WAYS:
                    vs = c["against"].get(w, {}).get(v) if w != v else None
                    if vs:
                        below[w][v] += vs["rounds_below"]
    heaviest = phase_b[max(phase_b)]
    tails = {}
    for w in WAYS:
        p19, _ = best(heaviest, "part19", w)
        cr, _ = best(heaviest, "create", w)
        tails[w] = (p19["p99"] if p19 else math.inf, cr["p99"] if cr else math.inf)

    def passes(w, v):
        n = below[w][v] + below[v][w]
        return below[w][v] >= sign_k(n)

    left, order, how = list(WAYS), [], []
    while left:
        by_p50 = [w for w in left if len(left) > 1 and all(passes(w, v) for v in left if v != w)]
        w = by_p50[0] if by_p50 else min(left, key=lambda x: (tails[x], WAYS.index(x)))
        how.append("p50" if by_p50 else "tail")
        order.append(w)
        left.remove(w)
    return order, how, below, tails, max(phase_b)


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
    cells = {"A": {}, "B": {}}
    try:
        for line in open(exits):
            fields = line.split()
            if len(fields) < 4:
                raise Refused(f"a cell ended with no results dir: {line.strip()}")
            _, name, rc, out = fields[:4]
            if rc != "0":
                raise Refused(f"cell {name} exited {rc}")
            m = CELL.match(name)
            if not m:
                raise Refused(f"cell {name} names no phase, pass and load (<phase>-p<pass>-L<loops>)")
            c = read_cell(name, out)
            if m["phase"] == "A" and BASELINE not in c["create"]:
                raise Refused(f"phase A's cell {name} ran without the classic root")
            if m["phase"] == "B" and BASELINE in c["create"]:
                raise Refused(f"phase B's cell {name} ran the classic root")
            cells[m["phase"]].setdefault(int(m["loads"]), {})[m["pass"]] = c
        for phase in ("A", "B"):
            if not cells[phase]:
                raise Refused(f"no cell of phase {phase}")
    except Refused as e:
        print(f"refusing: {e}")
        return 3
    fmt = lambda v, f="{:.3f}": "n/a" if v is None else f.format(v)
    print(f"# {run} at {rev} ({mode}{'' if mode == 'host' else ', not of record'})")
    print()
    print("Per cell: each arm's create (untraced) and part 19 (traced), and the load each cell's rounds met. Phase A: "
          "the classic root and the ways interleaved; phase B: the ways alone.")
    print()
    print("| phase | loads | pass | arm | create p50 / p99, ms | below classic (rounds) | below copy (rounds) "
          "| part 19 p50 / p99 / unpreempted p99, us | load1 min / p50 / max |")
    print("|---|---|---|---|---|---|---|---|---|")
    for phase in ("A", "B"):
        for L in sorted(cells[phase]):
            for p in sorted(cells[phase][L]):
                c = cells[phase][L][p]
                for arm in [a for a in c["create"] if a in WAYS] + [a for a in c["create"] if a not in WAYS]:
                    cr, vs, pt = c["create"][arm], c["against"].get(arm, {}), c["part19"].get(arm)
                    below = {k: f"{v['rounds_below']} of {v['rounds']}" if v else "" for k, v in vs.items()}
                    p19 = f"{pt['p50']} / {pt['p99']} / {fmt(pt['unpreempted_p99'], '{}')}" if pt else ""
                    ld = c["load1"]
                    print(f"| {phase} | {L} | {p} | {arm} | {fmt(cr['p50'])} / {fmt(cr['p99'])} | "
                          f"{below.get(BASELINE, '')} | {below.get('copy', '')} | {p19} | "
                          + (f"{ld[0]:.1f} / {ld[1]:.1f} / {ld[2]:.1f}" if ld else "") + " |")
    lines = {}
    for way in WAYS:
        p50s = [c["create"][way]["p50"] for by_pass in cells["B"].values() for c in by_pass.values()
                if way in c["create"]]
        if not p50s:
            print(f"refusing: no phase B cell of {way}")
            return 3
        lines[way] = statistics.median(p50s)
    order, how, below, tails, heaviest = rank(cells["B"])
    default = order[0]
    print()
    print(f"## Verdicts (each phase best of {max(len(v) for ph in cells.values() for v in ph.values())} per load, "
          "summarize.py's)")
    print()
    if how[0] == "p50":
        print(f"- B (the ways alone): the default way is {default}, by p50: its create p50 below "
              + ", ".join(f"{v}'s in {below[default][v]} of {below[default][v] + below[v][default]} rounds "
                          f"({sign_k(below[default][v] + below[v][default])} needed)" for v in WAYS if v != default)
              + ", a sign test at 95% over every timed round of phase B.")
    else:
        pairs = ", ".join(f"{w} below {v} in {below[w][v]} of {below[w][v] + below[v][w]} "
                          f"({sign_k(below[w][v] + below[v][w])} needed)"
                          for i, w in enumerate(WAYS) for v in WAYS[i + 1:])
        print(f"- B (the ways alone): the default way is {default}, by its tail: no way's create p50 is below each "
              f"other's at 95% over every timed round of phase B ({pairs}); part 19 p99 at {heaviest} loops, best of "
              f"{len(cells['B'][heaviest])}: " + ", ".join(f"{w} {fmt(tails[w][0], '{}')}" for w in WAYS) + " us; create p99 there: "
              + ", ".join(f"{w} {fmt(tails[w][1])}" for w in WAYS) + " ms.")
    for L in sorted(cells["B"]):
        by_pass = cells["B"][L]
        for new in ("tree", "empty"):
            held = [p for p, c in by_pass.items() if c["create"][new]["p50"] < c["create"]["copy"]["p50"]]
            print(f"- B, {L} loops: {new}'s create p50 below the copy's in the same run in {len(held)} of "
                  f"{len(by_pass)} passes ({', '.join(sorted(held)) or 'none'}).")
        b19, bp = best(by_pass, "part19", default)
        bar = "meets" if b19 and b19["p99"] <= BAR_US else "misses"
        print(f"- B, {L} loops: the default way ({default}): part 19 p99 {fmt(b19 and b19['p99'], '{}')} us, best of "
              f"{len(by_pass)} ({bp}): {bar} the bar ({BAR_US} us); in each pass "
              + ", ".join(f"{p} {c['part19'].get(default, {}).get('p99')}" for p, c in sorted(by_pass.items()))
              + ".")
    for L in sorted(cells["A"]):
        for p, c in sorted(cells["A"][L].items()):
            d, k, vs = c["create"][default], c["create"][BASELINE], c["against"][default][BASELINE]
            print(f"- A, {L} loops, {p}: create p50: {default} {d['p50']:.3f} ms against classic {k['p50']:.3f} ms, "
                  f"the new path below: {'yes' if d['p50'] < k['p50'] else 'no'} ({vs['rounds_below']} of "
                  f"{vs['rounds']} rounds below).")
            print(f"- A, {L} loops, {p}: create p99: {default} {d['p99']:.3f} ms against classic {k['p99']:.3f} ms, "
                  f"improved: {'yes' if d['p99'] < k['p99'] else 'no'}.")
    print("- A: classic against the default way measures goal 5's prune, with the node's /dev and masked-directory "
          "pools the classic init makes its own of, and goal 17's ways together: the comparison goal 17's text names. "
          "Each new way against the copy, phase B's, is goal 17's own record of the ways.")
    if rt_dir:
        crun, missed, more = runtime_tools(rt_dir)
        print(f"- runtime-tools: hex passes {len(crun) - len(missed)} of the {len(crun)} tests crun passes"
              + (f"; fails {', '.join(missed)}" if missed else "")
              + (f"; passes {len(more)} crun fails ({', '.join(more)})" if more else "") + ".")
    print()
    print("# record lines for mntns_default.txt, in the default's order ("
          + ", ".join(f"{w} by {h}" for w, h in zip(order, how)) + "):")
    for way in order:
        print(f"{way} {lines[way]:.3f} {run} {rev}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
