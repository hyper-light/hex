#!/usr/bin/env python3
"""bench/competitive/probes: interleaved_pool.py's reading of a pass, off the bench host (goal 5 extended, step 6's
second criterion; CP-T7.1).

  1. The kept namespaces' counts: a pass whose rounds each ran several pairs states each round's counts once, as its
     phase counted them, not once a pair.

  python3 -I check_interleaved_pool.py
"""
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
sys.path.insert(0, HERE)
import interleaved_pool  # noqa: E402


def pair(ratio):
    at = {"load1": 1.0, "cpu_some_avg10": 0.0}
    return {"ratio": ratio, "c1_per_s": 100.0, "c32_per_s": 100.0 * ratio, "at_start": at, "at_end": at}


def round_file(d, rnd, ratios, reuse):
    phase = {"c1_lifecycles": 10, "c32_lifecycles": 10, "pairs": [pair(r) for r in ratios], "netns_reuse": reuse}
    with open(os.path.join(d, f"ociseam.r{rnd}.json"), "w") as f:
        json.dump({"phases": {"interleaved": phase}}, f)


class KeptNamespaces(unittest.TestCase):
    def test_each_rounds_counts_are_stated_once_however_many_pairs_it_ran(self):
        # Two rounds of three pairs each: the first's phase counted 100 checks (90 through held files, 4 through the
        # protocols, 6 entered for the bound), the second's 50 (45, 2, 3). Expect 150, 135, 6 and 9.
        bound = {"files_failed": 0, "files_bound": 5, "descriptors_soft": 1024, "descriptors_own": 1004}
        with tempfile.TemporaryDirectory() as d:
            round_file(d, 1, [4.0, 5.0, 6.0],
                       {"checked": 100, "through_files": 90, "protocols": 4, "files_bounded": 6, **bound})
            round_file(d, 2, [4.5, 5.5, 6.5],
                       {"checked": 50, "through_files": 45, "protocols": 2, "files_bounded": 3, **bound})
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                interleaved_pool.main(["--boot", "10", d])
        line = next(text for text in out.getvalue().splitlines() if "kept namespaces" in text)
        self.assertEqual(line.strip(), "kept namespaces: 150 checks, 135 through held files, 6 through the protocols, "
                         "9 entered for the bound, 0 whose files failed; held files for at most 5 namespaces (soft "
                         "limit 1024, the node's own 1004)")


if __name__ == "__main__":
    unittest.main()
