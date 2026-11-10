#!/usr/bin/env python3
"""Goal 17's rootless variant (rootless_record.py), from cells of rounds.py's arms libcrun and hex, rootless:
- each cell's creates per arm and hex against libcrun (its rounds below, its pooled p50 ratio, its p99 lower or not);
- per load, best of 2, as summarize.py takes it;
- runtime-tools' rootless subset: the tests crun passes rootless and how many hex passes, and the tests crun passes as
  root but not rootless, named as needing root;
- a refusal of a cell that failed.
Run: python3 -B -I bench/competitive/probes/rootless_record_test.py
"""
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import rootless_record as rr
import rounds as rd


def cell(root, name, ms, failed=None):
    """A cell: one warmup and two timed rounds of libcrun then hex, 8 creates a burst at ms[arm]."""
    out = os.path.join(root, name)
    d = os.path.join(out, "rounds")
    os.makedirs(d)
    rows = []
    for r in range(3):
        order = ["libcrun", "hex"] if r % 2 == 0 else ["hex", "libcrun"]
        rows.append({"round": r - 1, "warmup": r < 1, "load": {"load1": 12.0 + r, "procs_running": 13},
                     "order": order, "bursts": [{"arm": a, "t0_ns": 0, "t1_ns": 0, "create_ms": [ms[a]] * 8,
                                                 "delete_ms": [0.5] * 8} for a in order]})
    with open(os.path.join(d, "rounds.jsonl"), "w") as f:
        f.writelines(json.dumps(r) + "\n" for r in rows)
    with open(os.path.join(d, "summary.json"), "w") as f:
        json.dump(dict(rd.summarize(rows, ["libcrun", "hex"]), failed=failed), f)
    return out


def results(root, name, passing, tests="abcde"):
    d = os.path.join(root, name)
    os.makedirs(d)
    with open(os.path.join(d, "results.tsv"), "w") as f:
        f.writelines(f"{t}\t{'pass' if t in passing else 'fail'}\t\t0\n" for t in tests)
    return os.path.join(d, "results.tsv")


def record(root, cells, rt=None, crun_root=None, rc=0):
    exits = os.path.join(root, "exits")
    with open(exits, "w") as f:
        f.writelines(f"2026-11-08T22:00:00Z {name} {rc} {d} 30\n" for name, d in cells)
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = rr.main([exits, "rootless-T", "abc1234"] + ([rt] if rt else []) + ([crun_root] if crun_root else []))
    return code, out.getvalue()


class Rootless(unittest.TestCase):
    def test_hex_against_libcrun_each_cell_and_best_of_2(self):
        """Do 2 passes at 6 and 12 loops, hex at 1.0 ms and libcrun at 1.5 then 2.0, expect each cell's row with
        hex below libcrun in 2 of 2 rounds, and best of 2 taking pass 1 for libcrun."""
        with tempfile.TemporaryDirectory() as d:
            cells = [(f"p{p}-L{L}", cell(d, f"p{p}-L{L}", {"hex": 1.0, "libcrun": 1.0 + 0.5 * p}))
                     for L in (6, 12) for p in (1, 2)]
            code, out = record(d, cells)
        self.assertEqual(code, 0, out)
        self.assertIn("| 6 | p1 | hex | 1.000 / 1.000 | 2 of 2 | 0.667 | 13.0 / 13.0 / 14.0 |", out)
        self.assertIn("| 12 | p2 | libcrun | 2.000 / 2.000 |  |  | 13.0 / 13.0 / 14.0 |", out)
        self.assertIn("- 6 loops, best of 2: hex create p50 1.000 ms (p1), libcrun 1.500 (p1): 0.667x libcrun's", out)
        self.assertIn("- 12 loops: hex's create p50 below libcrun's in the same run in 2 of 2 passes (p1, p2)", out)

    def test_the_rootless_subset_and_the_tests_that_need_root(self):
        """Do crun rootless passing a, b, crun as root passing a, b, c, d, and hex rootless passing a, e, expect hex
        to pass 1 of crun's 2 rootless, b named, and c and d named as needing root."""
        with tempfile.TemporaryDirectory() as d:
            rt = os.path.join(d, "rt")
            results(rt, "crun", "ab")
            results(rt, "hex", "ae")
            root = results(d, "root-crun", "abcd")
            code, out = record(d, [("p1-L6", cell(d, "p1-L6", {"hex": 1.0, "libcrun": 1.5}))], rt=rt, crun_root=root)
        self.assertEqual(code, 0, out)
        self.assertIn("- runtime-tools rootless: hex passes 1 of the 2 tests crun passes rootless; fails b; passes 1 "
                      "crun fails rootless (e).", out)
        self.assertIn("- left out as needing root: 2 tests crun passes as root and fails rootless: c, d.", out)

    def test_a_cell_that_failed_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            code, out = record(d, [("p1-L6", cell(d, "p1-L6", {"hex": 1.0, "libcrun": 1.5}))], rc=3)
        self.assertEqual(code, 3)
        self.assertIn("refusing: cell p1-L6 exited 3", out)


if __name__ == "__main__":
    unittest.main()
