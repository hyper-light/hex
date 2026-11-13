#!/usr/bin/env python3
"""record_trace.py's steps (joint plan step 14, AC-T7.4.1): each window's CPU kept to the millisecond as whole busy
threads and one more for the rest, time and CPU carried from window to window so neither rounds away, memory rounded
up to a MiB.
Run: python3 -B -I bench/charge/record_trace_test.py
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import record_trace as rt

MIB = 1 << 20


class StepsTest(unittest.TestCase):
    def test_w14_a_window_keeps_its_cpu_to_the_millisecond(self):
        """Do turn a 100 ms window that used 230 ms of CPU and holds a byte past 5 MiB into steps. Expect 30 ms of 3
        threads, then 70 ms of 2, each holding 6 MiB."""
        self.assertEqual(rt.Steps().window(100_000, 230_000, 5 * MIB + 1), [(30, 3, 6), (70, 2, 6)])

    def test_w14_an_idle_window_and_a_whole_one_are_one_step(self):
        """Do turn a window that used no CPU, and one that used exactly two CPUs. Expect one step each."""
        self.assertEqual(rt.Steps().window(100_000, 0, 0), [(100, 0, 0)])
        self.assertEqual(rt.Steps().window(100_000, 200_000, MIB), [(100, 2, 1)])

    def test_w14_time_and_cpu_carry_from_window_to_window(self):
        """Do turn windows ending at 100.4 ms (50.6 ms of CPU), 200.9 ms (101.7 ms) and 200.999 ms (101.9 ms). Expect
        the first 100 ms with 50 of CPU, the second 100 ms with the 51 carried, the third nothing (no whole millisecond
        since), and the steps' time and CPU summing to the whole milliseconds read."""
        steps = rt.Steps()
        first = steps.window(100_400, 50_600, 0)
        second = steps.window(200_900, 101_700, 0)
        third = steps.window(200_999, 101_900, 0)
        self.assertEqual(first, [(50, 1, 0), (50, 0, 0)])
        self.assertEqual(second, [(51, 1, 0), (49, 0, 0)])
        self.assertEqual(third, [])
        every = first + second + third
        self.assertEqual(sum(ms for ms, _, _ in every), 200)
        self.assertEqual(sum(ms * threads for ms, threads, _ in every), 101)


if __name__ == "__main__":
    unittest.main()
