#!/usr/bin/env python3
"""Goal 17's record of the mount-namespace ways (mntns_record.py), from cells of rounds.py's interleaved arms, the
classic root's (goal 17's baseline) beside the three ways:
- the record lines mntns_default.txt reads, each way's create p50 the median of its cells', the classic root never one;
- part 19 per arm, each span claimed by the burst whose window holds its start, warmup rounds' dropped;
- the verdicts: in each pass, the default way's create p50 and p99 against the classic root's, the comparison goal 17's
  text names, with its caveat; best of 2 per load, each new way's create p50 against the copy's and the default way's
  part 19 p99 against the bar; and runtime-tools' tests crun passes;
- a refusal of a cell whose arm took another way, of a classic arm whose creates weren't all the classic root's, of a
  record with no classic arm, of a span in no window and of a window short of spans.
Run: python3 -B -I bench/competitive/probes/mntns_record_test.py
"""
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import mntns_record as mr
import rounds as rd

ARMS = ("classic", "copy", "tree", "empty")
WAYS = ("copy", "tree", "empty")
ROUNDS, WARMUP, BURST = 2, 1, 2
LIFECYCLES = (WARMUP + ROUNDS) * BURST


def rows_for(create, arms=ARMS, base_ns=1_000_000_000):
    """rounds.py's lines for `arms`: WARMUP + ROUNDS rounds, BURST creates a burst at create[arm] ms, each burst 0.5 ms
    long, 1 ms apart, rotated as rounds.py rotates them."""
    rows, k = [], 0
    for r in range(WARMUP + ROUNDS):
        row = {"round": r - WARMUP, "warmup": r < WARMUP, "load": {"load1": 6.0 + r, "procs_running": 7},
               "order": [arms[(r + i) % len(arms)] for i in range(len(arms))], "bursts": []}
        for arm in row["order"]:
            t0 = base_ns + k * 1_000_000
            row["bursts"].append({"arm": arm, "t0_ns": t0, "t1_ns": t0 + 500_000,
                                  "create_ms": [create[arm]] * BURST, "delete_ms": [0.4] * BURST})
            k += 1
        rows.append(row)
    return rows


def spans_for(rows, part19, skip=0, stray=False):
    """Each burst's spans, BURST of them, 10 and 20 us into its window, at part19[arm] us, the second preempted;
    `skip` drops that many from the last burst, `stray` adds one past every window."""
    spans = []
    for r in rows:
        for b in r["bursts"]:
            t = b["t0_ns"] // 1000
            spans += [[t + 10 * (i + 1), part19[b["arm"]], 5 * i] for i in range(BURST)]
    spans = spans[:len(spans) - skip]
    if stray:
        spans.append([rows[-1]["bursts"][-1]["t1_ns"] // 1000 + 100, 50, 0])
    return spans


def arms_for(took=None, classic_took=None):
    """Each arm's serve phase as seam_oci says it: a way's creates all made its way and pruned, the classic arm's all
    the classic root's and none pruned; `took` and `classic_took` say otherwise."""
    out = {}
    for way in WAYS:
        made = {w: 0 for w in WAYS}
        made[(took or {}).get(way, way)] = LIFECYCLES
        out[way] = {"exit": 0, "result": {"phases": {"serve": {
            "lifecycles": LIFECYCLES, "mount_namespaces": made, "roots": {"classic": 0, "pruned": LIFECYCLES}}}}}
    out["classic"] = {"exit": 0, "result": {"phases": {"serve": {
        "lifecycles": LIFECYCLES, "mount_namespaces": {w: 0 for w in WAYS},
        "roots": classic_took or {"classic": LIFECYCLES, "pruned": 0}}}}}
    return out


def cell(root, name, create, part19, arms=ARMS, took=None, classic_took=None, skip=0, stray=False):
    out = os.path.join(root, name)
    for kind in ("untraced", "traced"):
        d = os.path.join(out, f"rounds.c8.{kind}")
        os.makedirs(d)
        rows = rows_for(create, arms)
        with open(os.path.join(d, "rounds.jsonl"), "w") as f:
            f.writelines(json.dumps(r) + "\n" for r in rows)
        with open(os.path.join(d, "arms.json"), "w") as f:
            json.dump(arms_for(took, classic_took), f)
        if kind == "untraced":
            with open(os.path.join(d, "summary.json"), "w") as f:
                json.dump(dict(rd.summarize(rows, list(arms)), failed=None), f)
        else:
            with open(os.path.join(out, "mounts.c8.summary.json"), "w") as f:
                json.dump({"spans": spans_for(rows, part19, skip, stray)}, f)
    return out


def record(root, cells, rt=None):
    exits = os.path.join(root, "exits")
    with open(exits, "w") as f:
        for name, d in cells:
            f.write(f"2026-11-08T22:00:00Z {name} 0 {d} 40\n")
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        rc = mr.main([exits, "mntns-host-T", "abc1234", "host"] + ([rt] if rt else []))
    return rc, out.getvalue()


FAST = {"classic": 2.0, "copy": 1.2, "tree": 0.8, "empty": 0.9}
P19 = {"classic": 900, "copy": 600, "tree": 90, "empty": 120}


class Record(unittest.TestCase):
    def test_two_passes_at_two_loads_give_the_record_lines_and_the_verdicts(self):
        """Do 2 passes at 6 and 12 loops, tree fastest and the classic root slowest, expect each way's record line the
        median of its four cells' create p50s and none for the classic root, tree below copy in 2 of 2 passes at each
        load, tree the default, its part 19 p99 from the traced spans its windows claim meeting the bar, and in each
        pass its create p50 below the classic root's in every round and its create p99 improved, with the caveat."""
        with tempfile.TemporaryDirectory() as d:
            cells = [(f"p{p}-L{L}", cell(d, f"p{p}-L{L}", {a: v + 0.1 * p for a, v in FAST.items()}, P19))
                     for L in (6, 12) for p in (1, 2)]
            rc, out = record(d, cells)
        self.assertEqual(rc, 0, out)
        self.assertIn("copy 1.350 mntns-host-T abc1234\n", out)
        self.assertIn("tree 0.950 mntns-host-T abc1234\n", out)
        self.assertIn("empty 1.050 mntns-host-T abc1234\n", out)
        self.assertNotIn("classic 2.150 mntns-host-T", out)
        self.assertIn("- 6 loops: tree's create p50 below the copy's in the same run in 2 of 2 passes (p1, p2).", out)
        self.assertIn("- 12 loops: the default way (tree): part 19 p99 90 us, best of 2 (p1): meets the bar (500 us)",
                      out)
        self.assertIn("- 6 loops, p1: create p50: tree 0.900 ms against classic 2.100 ms, the new path below: yes "
                      "(2 of 2 rounds below).", out)
        self.assertIn("- 12 loops, p2: create p99: tree 1.000 ms against classic 2.200 ms, improved: yes.", out)
        self.assertIn("- classic against the default way measures goal 5's prune, with the node's /dev and "
                      "masked-directory pools the classic init makes its own of, and goal 17's ways together: the "
                      "comparison goal 17's text names. Each new way against the copy is goal 17's own record of the "
                      "ways.", out)
        self.assertIn("| 6 | p1 | tree | 0.900 / 0.900 | 2 of 2 | 2 of 2 | 90 / 90 / 90 |", out)
        self.assertIn("| 6 | p1 | classic | 2.100 / 2.100 |  | 0 of 2 | 900 / 900 / 900 |", out)

    def test_a_default_way_whose_part_19_is_slow_misses_the_bar(self):
        """Do part 19 at 700 us for every arm, expect the default's verdict to miss the bar at each load."""
        with tempfile.TemporaryDirectory() as d:
            cells = [(f"p{p}-L{L}", cell(d, f"p{p}-L{L}", FAST, {a: 700 for a in ARMS})) for L in (6, 12)
                     for p in (1, 2)]
            rc, out = record(d, cells)
        self.assertEqual(rc, 0, out)
        self.assertIn("part 19 p99 700 us, best of 2 (p1): misses the bar (500 us)", out)

    def test_a_default_way_slower_than_the_classic_root_says_no(self):
        """Do every way slower than the classic root, expect the default way's create p50 not below the classic root's
        and its create p99 not improved, in each pass."""
        slow = {"classic": 0.5, "copy": 1.2, "tree": 0.8, "empty": 0.9}
        with tempfile.TemporaryDirectory() as d:
            cells = [(f"p{p}-L{L}", cell(d, f"p{p}-L{L}", slow, P19)) for L in (6, 12) for p in (1, 2)]
            rc, out = record(d, cells)
        self.assertEqual(rc, 0, out)
        self.assertIn("- 6 loops, p2: create p50: tree 0.800 ms against classic 0.500 ms, the new path below: no "
                      "(0 of 2 rounds below).", out)
        self.assertIn("- 12 loops, p1: create p99: tree 0.800 ms against classic 0.500 ms, improved: no.", out)

    def test_a_record_whose_copy_is_fastest_says_so(self):
        """Do the copy fastest of the ways in every cell, expect the record to name the copy the default and say no new
        way's create p50 is below it."""
        slow_new = {"classic": 2.0, "copy": 0.7, "tree": 0.9, "empty": 1.0}
        with tempfile.TemporaryDirectory() as d:
            cells = [(f"p{p}-L{L}", cell(d, f"p{p}-L{L}", slow_new, P19)) for L in (6, 12) for p in (1, 2)]
            rc, out = record(d, cells)
        self.assertEqual(rc, 0, out)
        self.assertIn("- the default way is the copy: neither new way's create p50 is below it (the medians of its "
                      "cells: copy 0.700, tree 0.900, empty 1.000 ms).", out)
        self.assertIn("- 6 loops, p1: create p50: copy 0.700 ms against classic 2.000 ms", out)

    def test_a_cell_whose_arm_took_another_way_is_refused(self):
        """Do a tree arm whose creates were all copies, expect the run refused, naming the cell and the arm."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("p1-L6", cell(d, "p1-L6", FAST, P19, took={"tree": "copy"}))])
        self.assertEqual(rc, 3)
        self.assertIn("cell p1-L6 (untraced): arm tree served 6 creates, which took", out)

    def test_a_classic_arm_whose_creates_were_not_all_the_classic_roots_is_refused(self):
        """Do a classic arm one of whose six creates was pruned, expect the run refused, naming the cell and what its
        creates took."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("p1-L6", cell(d, "p1-L6", FAST, P19, classic_took={"classic": 5, "pruned": 1}))])
        self.assertEqual(rc, 3)
        self.assertIn("cell p1-L6 (untraced): the classic arm served 6 creates, which took "
                      "{'classic': 5, 'pruned': 1}", out)

    def test_a_record_with_no_classic_arm_is_refused(self):
        """Do cells of the three ways alone, expect the run refused: goal 17's verdicts are set against the classic
        root."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("p1-L6", cell(d, "p1-L6", FAST, P19, arms=WAYS))])
        self.assertEqual(rc, 3)
        self.assertIn("refusing: no cell of the classic root", out)

    def test_a_span_in_no_window_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("p1-L6", cell(d, "p1-L6", FAST, P19, stray=True))])
        self.assertEqual(rc, 3)
        self.assertIn("lies in no burst's window", out)

    def test_a_window_short_of_spans_is_refused(self):
        """Do a traced burst whose window holds one span for its two creates, expect the run refused."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("p1-L6", cell(d, "p1-L6", FAST, P19, skip=1))])
        self.assertEqual(rc, 3)
        self.assertIn("made 2 creates and holds 1 part 19 spans", out)

    def test_runtime_tools_counts_the_tests_crun_passes_that_hex_passes(self):
        """Do crun passing a, b and c and hex passing a and c, expect 2 of the 3 and b named."""
        with tempfile.TemporaryDirectory() as d:
            rt = os.path.join(d, "rt")
            for name, passing in (("crun", "abc"), ("hex", "acd")):
                os.makedirs(os.path.join(rt, name))
                with open(os.path.join(rt, name, "results.tsv"), "w") as f:
                    f.writelines(f"{t}\t{'pass' if t in passing else 'fail'}\t\t0\n" for t in "abcd")
            rc, out = record(d, [("p1-L6", cell(d, "p1-L6", FAST, P19))], rt=rt)
        self.assertEqual(rc, 0, out)
        self.assertIn("- runtime-tools: hex passes 2 of the 3 tests crun passes; fails b; passes 1 crun fails (d).",
                      out)


if __name__ == "__main__":
    unittest.main()
