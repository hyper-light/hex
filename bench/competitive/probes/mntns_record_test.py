#!/usr/bin/env python3
"""Goal 6's record of the mount-namespace ways (mntns_record.py): the record lines mntns_default.txt reads, each way's
create p50 the median of its cells' untraced phases, the same-run comparison against the copy, and a refusal of any
cell whose mount-namespace counts don't show the way it was asked (non-vacuity).
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


def cell(root, name, way, create_ms, part19=(50, 400, 300), took=None):
    """One cell's out dir: its untraced phase's creates, its traced phase's part 19 and the ways its creates took."""
    d = os.path.join(root, name)
    os.makedirs(d)
    counts = {"copy": 0, "empty": 0, "tree": 0}
    counts[took or way] = len(create_ms)
    for f, ms in (("untraced.c8.json", create_ms), ("mounts.c8.json", [x * 2 for x in create_ms])):
        with open(os.path.join(d, f), "w") as out:
            json.dump({"phases": {"throughput": {"8": {"create_ms": ms}, "mount_namespaces": counts}}}, out)
    with open(os.path.join(d, "mounts.c8.summary.json"), "w") as out:
        json.dump({"span_p50_us": part19[0], "span_p99_us": part19[1], "span_unpreempted_p99_us": part19[2]}, out)
    return d


def run(rows, took=None):
    with tempfile.TemporaryDirectory() as root:
        lines = []
        for way, L, r, ms in rows:
            name = f"{way}-L{L}-r{r}"
            d = cell(root, name, way, ms, took=(took or {}).get(name))
            lines.append(f"2026-11-06T02:00:00Z {name} 0 {d}\n")
        exits = os.path.join(root, "exits")
        with open(exits, "w") as f:
            f.writelines(lines)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = mr.main([exits, "mntns-host-T", "abc1234", "host"])
        return rc, out.getvalue()


class Record(unittest.TestCase):
    def test_each_way_gets_its_median_untraced_p50_and_the_new_ways_are_compared_with_the_copy(self):
        """Do two rounds of three ways at one load, expect each way's record line at the median of its cells'
        untraced create p50s, and the count of pairs whose new way is below the copy's in the same round."""
        rows = [("copy", 6, 1, [1.0, 1.0, 1.2]), ("tree", 6, 1, [0.8, 0.8, 0.9]), ("empty", 6, 1, [0.9, 0.9, 1.0]),
                ("copy", 6, 2, [1.1, 1.1, 1.3]), ("tree", 6, 2, [0.7, 0.7, 0.8]), ("empty", 6, 2, [1.2, 1.2, 1.3])]
        rc, out = run(rows)
        self.assertEqual(rc, 0)
        self.assertIn("copy 1.050 mntns-host-T abc1234\n", out)
        self.assertIn("tree 0.750 mntns-host-T abc1234\n", out)
        self.assertIn("empty 1.050 mntns-host-T abc1234\n", out)
        self.assertIn("tree below copy in 2 of 2 pairs", out)
        self.assertIn("empty below copy in 1 of 2 pairs", out)
        self.assertIn("| tree | 6 | 1 | 0.800 |", out)

    def test_a_cell_whose_creates_took_another_way_is_refused(self):
        """Do a tree cell whose creates were all copies, expect the run refused, naming the cell."""
        rows = [("copy", 6, 1, [1.0]), ("tree", 6, 1, [0.8]), ("empty", 6, 1, [0.9])]
        rc, out = run(rows, took={"tree-L6-r1": "copy"})
        self.assertEqual(rc, 3)
        self.assertIn("tree-L6-r1", out)


if __name__ == "__main__":
    unittest.main()
