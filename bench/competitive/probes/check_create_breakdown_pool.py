#!/usr/bin/env python3
"""bench/competitive/probes: create_breakdown_pool.py's verdict on goal 5 extended step 6's third criterion, off the
bench host (CP-T7.1; the lead, 2026-11-03T02:36Z: the paired create p50 difference within 0.02 ms of 0, its
bootstrap standard error at most 0.01 ms).

  1. Rounds whose differences are small and steady: met, exit 0.
  2. Rounds within the window on the mean but too spread for the standard error bar: not met by the SE, exit 1.
  3. Rounds steady but off by more than the window: not met by the window, exit 1.

  python3 -I check_create_breakdown_pool.py
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
import create_breakdown_pool  # noqa: E402


def run_dir(d, diffs):
    """A run whose round n's seam creates sit `diffs[n]` ms above its before's (each a constant 0.3 ms)."""
    for n, diff in enumerate(diffs, 1):
        for kind, ms in (("seam", 0.3 + diff), ("before", 0.3)):
            with open(os.path.join(d, f"{kind}.r{n}.json"), "w") as f:
                json.dump({"phases": {"lifecycle": {"ms": {"create": [ms] * 5}}}}, f)


def pooled(diffs):
    with tempfile.TemporaryDirectory() as d:
        run_dir(d, diffs)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = create_breakdown_pool.main([d])
    return rc, out.getvalue()


class ThirdCriterion(unittest.TestCase):
    def test_small_steady_differences_meet_it(self):
        # Do: 20 rounds each 0.001 to 0.004 ms over. Expect: met (mean within 0.02, SE under 0.01), exit 0.
        rc, out = pooled([0.001, 0.002, 0.003, 0.004] * 5)
        self.assertIn("third criterion: met", out)
        self.assertEqual(rc, 0)

    def test_a_spread_too_wide_for_the_se_bar_does_not(self):
        # Do: 4 rounds at +0.1, -0.1, +0.1, -0.1 ms (mean 0, SE about 0.05). Expect: not met, by its standard error.
        rc, out = pooled([0.1, -0.1, 0.1, -0.1])
        self.assertIn("third criterion: not met: bootstrap SE", out)
        self.assertEqual(rc, 1)

    def test_a_steady_difference_past_the_window_does_not(self):
        # Do: 20 rounds each 0.05 ms over. Expect: not met, by the window.
        rc, out = pooled([0.05] * 20)
        self.assertIn("third criterion: not met: mean +0.0500 ms is outside", out)
        self.assertEqual(rc, 1)


if __name__ == "__main__":
    unittest.main()
