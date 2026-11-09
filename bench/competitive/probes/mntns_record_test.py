#!/usr/bin/env python3
"""Goal 17's record of the mount-namespace ways (mntns_record.py), in its two phases (the lead's ruling of 2026-11-09):
phase A, the classic root (goal 17's baseline) and the three ways interleaved; phase B, the three ways alone.
- From A: in each pass at each load, the default way's create p50 and p99 against the classic root's, with its caveat.
- From B: the default way, by create p50 where a sign test on the rounds below shows a difference at the record's 95%,
  else by part 19 p99 at the heaviest load, then create p99 there; each new way's create p50 against the copy's; the
  default way's part 19 p99 against the bar; and the record lines mntns_default.txt reads, in the default's order,
  each way's create p50 the median of its B cells'.
- runtime-tools' tests crun passes.
- A refusal of a cell whose arm took another way, of a classic arm whose creates weren't all the classic root's, of a
  phase A cell without the classic arm and a phase B cell with it, of a record missing a phase, of a span in no
  window and of a window short of spans.
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

ARMS_A = ("classic", "copy", "tree", "empty")
ARMS_B = ("copy", "tree", "empty")
WAYS = ARMS_B
ROUNDS, WARMUP, BURST = 2, 1, 2


def rows_for(create, arms, rounds=ROUNDS, base_ns=1_000_000_000):
    """rounds.py's lines for `arms`: WARMUP + `rounds` rounds, BURST creates a burst at create[arm] ms (or
    create(round, arm) ms where it's a function of the round), each burst 0.5 ms long, 1 ms apart, rotated as
    rounds.py rotates them."""
    rows, k = [], 0
    for r in range(WARMUP + rounds):
        row = {"round": r - WARMUP, "warmup": r < WARMUP, "load": {"load1": 6.0 + r, "procs_running": 7},
               "order": [arms[(r + i) % len(arms)] for i in range(len(arms))], "bursts": []}
        for arm in row["order"]:
            ms = create(r - WARMUP, arm) if callable(create) else create[arm]
            t0 = base_ns + k * 1_000_000
            row["bursts"].append({"arm": arm, "t0_ns": t0, "t1_ns": t0 + 500_000,
                                  "create_ms": [ms] * BURST, "delete_ms": [0.4] * BURST})
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


def arms_for(arms, lifecycles, took=None, classic_took=None):
    """Each arm's serve phase as seam_oci says it: a way's creates all made its way and pruned, the classic arm's all
    the classic root's and none pruned; `took` and `classic_took` say otherwise."""
    out = {}
    for way in [a for a in arms if a in WAYS]:
        made = {w: 0 for w in WAYS}
        made[(took or {}).get(way, way)] = lifecycles
        out[way] = {"exit": 0, "result": {"phases": {"serve": {
            "lifecycles": lifecycles, "mount_namespaces": made, "roots": {"classic": 0, "pruned": lifecycles}}}}}
    if "classic" in arms:
        out["classic"] = {"exit": 0, "result": {"phases": {"serve": {
            "lifecycles": lifecycles, "mount_namespaces": {w: 0 for w in WAYS},
            "roots": classic_took or {"classic": lifecycles, "pruned": 0}}}}}
    return out


def cell(root, name, create, part19, arms, rounds=ROUNDS, took=None, classic_took=None, skip=0, stray=False):
    out = os.path.join(root, name)
    for kind in ("untraced", "traced"):
        d = os.path.join(out, f"rounds.c8.{kind}")
        os.makedirs(d)
        rows = rows_for(create, arms, rounds)
        with open(os.path.join(d, "rounds.jsonl"), "w") as f:
            f.writelines(json.dumps(r) + "\n" for r in rows)
        with open(os.path.join(d, "arms.json"), "w") as f:
            json.dump(arms_for(arms, (WARMUP + rounds) * BURST, took, classic_took), f)
        if kind == "untraced":
            with open(os.path.join(d, "summary.json"), "w") as f:
                json.dump(dict(rd.summarize(rows, list(arms)), failed=None), f)
        else:
            with open(os.path.join(out, "mounts.c8.summary.json"), "w") as f:
                json.dump({"spans": spans_for(rows, part19, skip, stray)}, f)
    return out


def phases(root, create, part19, b_create=None, b_part19=None, b_rounds=ROUNDS, offset=0.1):
    """Both phases' cells, 2 passes at 6 and 12 loops: A's arms the classic and the ways at `create` and `part19`; B's
    the ways alone at `b_create` and `b_part19` (else A's), each pass `offset` ms slower than the one before."""
    cells = []
    for L in (6, 12):
        for p in (1, 2):
            for phase, arms, cr, p19, rounds in (("A", ARMS_A, create, part19, ROUNDS),
                                                 ("B", ARMS_B, b_create or create, b_part19 or part19, b_rounds)):
                at = cr if callable(cr) else {a: v + offset * p for a, v in cr.items()}
                name = f"{phase}-p{p}-L{L}"
                cells.append((name, cell(root, name, at, p19, arms, rounds)))
    return cells


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


def rotating(r, arm):
    """A coin flip at p50: each round a different way fastest, each the fastest, the middle and the slowest as often."""
    if arm == "classic":
        return 2.0
    return (1.0, 1.1, 1.2)[(WAYS.index(arm) - r) % 3]


class Record(unittest.TestCase):
    def test_two_phases_give_the_classic_verdicts_from_a_and_the_bar_and_the_default_from_b(self):
        """Do both phases, 2 passes at 6 and 12 loops, tree fastest in every round and the classic root slowest, expect
        tree the default by p50 (below the copy and empty in 8 of 8 of phase B's rounds, past the 7 a sign test needs),
        its part 19 p99 from B's traced spans meeting the bar, tree below copy in 2 of 2 passes in B, in each pass of A
        its create p50 below the classic root's in every round and its create p99 improved with the caveat, and the
        record lines from B in the default's order, the classic root never one."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, phases(d, FAST, P19))
        self.assertEqual(rc, 0, out)
        self.assertIn("- B (the ways alone): the default way is tree, by p50: its create p50 below copy's in 8 of 8 "
                      "rounds (7 needed), empty's in 8 of 8 rounds (7 needed), a sign test at 95% over every timed "
                      "round of phase B.", out)
        self.assertIn("- B, 6 loops: tree's create p50 below the copy's in the same run in 2 of 2 passes (p1, p2).", out)
        self.assertIn("- B, 12 loops: the default way (tree): part 19 p99 90 us, best of 2 (p1): meets the bar "
                      "(500 us)", out)
        self.assertIn("- A, 6 loops, p1: create p50: tree 0.900 ms against classic 2.100 ms, the new path below: yes "
                      "(2 of 2 rounds below).", out)
        self.assertIn("- A, 12 loops, p2: create p99: tree 1.000 ms against classic 2.200 ms, improved: yes.", out)
        self.assertIn("- A: classic against the default way measures goal 5's prune, with the node's /dev and "
                      "masked-directory pools the classic init makes its own of, and goal 17's ways together: the "
                      "comparison goal 17's text names. Each new way against the copy, phase B's, is goal 17's own "
                      "record of the ways.", out)
        self.assertIn("tree 0.950 mntns-host-T abc1234\nempty 1.050 mntns-host-T abc1234\n"
                      "copy 1.350 mntns-host-T abc1234\n", out)
        self.assertNotIn("classic 2.150 mntns-host-T", out)
        self.assertIn("| B | 6 | p1 | tree | 0.900 / 0.900 |  | 2 of 2 | 90 / 90 / 90 |", out)
        self.assertIn("| A | 6 | p1 | classic | 2.100 / 2.100 |  | 0 of 2 | 900 / 900 / 900 |", out)

    def test_a_coin_flip_at_p50_leaves_the_default_to_the_tail(self):
        """Do phase B's ways each fastest in a third of its 12 rounds, so none is below another's in more than 8 of 12
        (10 needed), and empty's part 19 p99 the lowest, then tree's, expect empty the default by its tail and the
        record lines empty, tree, copy."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, phases(d, FAST, P19, b_create=rotating, b_part19={"copy": 600, "tree": 300,
                                                                                    "empty": 120}, b_rounds=3))
        self.assertEqual(rc, 0, out)
        self.assertIn("- B (the ways alone): the default way is empty, by its tail: no way's create p50 is below each "
                      "other's at 95% over every timed round of phase B", out)
        self.assertIn("part 19 p99 at 12 loops, best of 2: copy 600, tree 300, empty 120 us", out)
        self.assertIn("empty 1.100 mntns-host-T abc1234\ntree 1.100 mntns-host-T abc1234\n"
                      "copy 1.100 mntns-host-T abc1234\n", out)

    def test_a_tie_in_part_19_goes_to_create_p99(self):
        """Do phase B's ways a coin flip at p50 and alike at part 19, tree's create p99 the lowest, expect tree the
        default by its tail, and copy before empty where their tails tie too (the ways' own order)."""
        def alternating(r, arm):
            return {"classic": 2.0}.get(arm) or ({"copy": 1.0, "tree": 1.1, "empty": 1.2} if r % 2 == 0
                                                 else {"copy": 1.2, "tree": 1.0, "empty": 1.1})[arm]
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, phases(d, FAST, P19, b_create=alternating, b_part19={w: 120 for w in WAYS}))
        self.assertEqual(rc, 0, out)
        self.assertIn("- B (the ways alone): the default way is tree, by its tail", out)
        self.assertIn("create p99 there: copy 1.200, tree 1.100, empty 1.200 ms", out)
        self.assertIn("tree 1.000 mntns-host-T abc1234\ncopy 1.000 mntns-host-T abc1234\n"
                      "empty 1.100 mntns-host-T abc1234\n", out)

    def test_a_copy_fastest_by_p50_is_the_default(self):
        """Do the copy fastest of the ways in every round of both phases, expect the copy the default by p50, set
        against the classic root in phase A."""
        copy_first = {"classic": 2.0, "copy": 0.7, "tree": 0.9, "empty": 1.0}
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, phases(d, copy_first, P19))
        self.assertEqual(rc, 0, out)
        self.assertIn("- B (the ways alone): the default way is copy, by p50", out)
        self.assertIn("- A, 6 loops, p1: create p50: copy 0.800 ms against classic 2.100 ms", out)

    def test_a_default_way_whose_part_19_is_slow_misses_the_bar(self):
        """Do part 19 at 700 us for every arm, expect the default's verdict to miss the bar at each load."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, phases(d, FAST, {a: 700 for a in ARMS_A}))
        self.assertEqual(rc, 0, out)
        self.assertIn("part 19 p99 700 us, best of 2 (p1): misses the bar (500 us)", out)

    def test_a_default_way_slower_than_the_classic_root_says_no(self):
        """Do every way slower than the classic root in phase A, expect the default way's create p50 not below the
        classic root's and its create p99 not improved, in each pass."""
        slow = {"classic": 0.5, "copy": 1.2, "tree": 0.8, "empty": 0.9}
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, phases(d, slow, P19, offset=0))
        self.assertEqual(rc, 0, out)
        self.assertIn("- A, 6 loops, p2: create p50: tree 0.800 ms against classic 0.500 ms, the new path below: no "
                      "(0 of 2 rounds below).", out)
        self.assertIn("- A, 12 loops, p1: create p99: tree 0.800 ms against classic 0.500 ms, improved: no.", out)

    def test_each_phase_holds_its_own_arms(self):
        """Do a phase A cell without the classic arm, a phase B cell with it, and records missing either phase, expect
        each refused, naming the cell or the phase."""
        cases = (
            ([("A-p1-L6", ARMS_B), ("B-p1-L6", ARMS_B)], "refusing: phase A's cell A-p1-L6 ran without the classic"),
            ([("A-p1-L6", ARMS_A), ("B-p1-L6", ARMS_A)], "refusing: phase B's cell B-p1-L6 ran the classic root"),
            ([("A-p1-L6", ARMS_A)], "refusing: no cell of phase B"),
            ([("B-p1-L6", ARMS_B)], "refusing: no cell of phase A"),
        )
        for cells, said in cases:
            with tempfile.TemporaryDirectory() as d:
                rc, out = record(d, [(n, cell(d, n, FAST, P19, arms)) for n, arms in cells])
            self.assertEqual(rc, 3, out)
            self.assertIn(said, out)

    def test_a_cell_whose_arm_took_another_way_is_refused(self):
        """Do a tree arm whose creates were all copies, expect the run refused, naming the cell and the arm."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("A-p1-L6", cell(d, "A-p1-L6", FAST, P19, ARMS_A)),
                                 ("B-p1-L6", cell(d, "B-p1-L6", FAST, P19, ARMS_B, took={"tree": "copy"}))])
        self.assertEqual(rc, 3)
        self.assertIn("cell B-p1-L6 (untraced): arm tree served 6 creates, which took", out)

    def test_a_classic_arm_whose_creates_were_not_all_the_classic_roots_is_refused(self):
        """Do a classic arm one of whose six creates was pruned, expect the run refused, naming the cell and what its
        creates took."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("A-p1-L6", cell(d, "A-p1-L6", FAST, P19, ARMS_A,
                                                  classic_took={"classic": 5, "pruned": 1})),
                                 ("B-p1-L6", cell(d, "B-p1-L6", FAST, P19, ARMS_B))])
        self.assertEqual(rc, 3)
        self.assertIn("cell A-p1-L6 (untraced): the classic arm served 6 creates, which took "
                      "{'classic': 5, 'pruned': 1}", out)

    def test_a_span_in_no_window_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("A-p1-L6", cell(d, "A-p1-L6", FAST, P19, ARMS_A)),
                                 ("B-p1-L6", cell(d, "B-p1-L6", FAST, P19, ARMS_B, stray=True))])
        self.assertEqual(rc, 3)
        self.assertIn("lies in no burst's window", out)

    def test_a_window_short_of_spans_is_refused(self):
        """Do a traced burst whose window holds one span for its two creates, expect the run refused."""
        with tempfile.TemporaryDirectory() as d:
            rc, out = record(d, [("A-p1-L6", cell(d, "A-p1-L6", FAST, P19, ARMS_A, skip=1)),
                                 ("B-p1-L6", cell(d, "B-p1-L6", FAST, P19, ARMS_B))])
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
            rc, out = record(d, [("A-p1-L6", cell(d, "A-p1-L6", FAST, P19, ARMS_A)),
                                 ("B-p1-L6", cell(d, "B-p1-L6", FAST, P19, ARMS_B))], rt=rt)
        self.assertEqual(rc, 0, out)
        self.assertIn("- runtime-tools: hex passes 2 of the 3 tests crun passes; fails b; passes 1 crun fails (d).",
                      out)

    def test_the_sign_test_needs_its_count(self):
        """Do the record's 299 rounds, phase B's four cells pooled, and the tests' handfuls, expect the fewest rounds
        below that a one-sided sign test at 95% needs: 165 of 299, 627 of 1,196, 7 of 8, 10 of 12, and none of 4."""
        self.assertEqual([mr.sign_k(n) for n in (299, 1196, 8, 12, 4)], [165, 627, 7, 10, 5])


if __name__ == "__main__":
    unittest.main()
