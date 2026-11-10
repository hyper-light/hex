#!/usr/bin/env python3
"""The census reader's censored samples and the seed rule (decision 9; the lead, 2026-11-09). A wait that runs out is
no sample (Karn's point), but its census line, `name >nanos`, says its slowest is at least the bound it ran out at:
waits-census-20261109T161828Z's suites arm had node_socket's create answers run out at 440 ms in 27 of 79 loops while
its row's max was 41.5 ms, so a seed from the answers alone would have cut that patience to 83 ms.
Run: python3 -B -I bench/competitive/probes/waits_census_test.py
"""
import contextlib
import io
import math
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import waits_census as wc  # noqa: E402

TIMES_SLOWEST = 2  # hex_core::patience::TIMES_SLOWEST: a wait lasts twice its record


class Reading(unittest.TestCase):
    def test_a_census_reads_answers_and_run_outs_apart(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "census-suites.txt"
            path.write_text("a 10\na 30\nb >440000000\na >50\nb 7\ncensus full at 6 lines\n")
            answers, ran_out, refused, cut = wc.read(path)
        self.assertEqual(answers, {"a": [10, 30], "b": [7]})
        self.assertEqual(ran_out, {"b": [440_000_000], "a": [50]})
        self.assertEqual(refused, {})
        self.assertEqual(cut, "census full at 6 lines")


    def test_refusal_census_keeps_refusals_out_of_success_distributions(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "census-suites.txt"
            path.write_text("a 10\na !900\nb !0\na >40\n")
            answers, ran_out, refused, cut = wc.read(path)
        self.assertEqual(answers, {"a": [10]})
        self.assertEqual(ran_out, {"a": [40]})
        self.assertEqual(refused, {"a": [900], "b": [0]})
        self.assertIsNone(cut)
        self.assertIsNone(wc.seed(answers.get("b", []), ran_out.get("b", [])))


class SeedRule(unittest.TestCase):
    def test_a_row_of_enough_answers_seeds_from_its_max(self):
        self.assertEqual(wc.seed(list(range(1, wc.SEEDABLE + 1)), []), wc.SEEDABLE)

    def test_a_row_of_too_few_answers_and_no_run_out_seeds_nothing(self):
        self.assertIsNone(wc.seed([5] * (wc.SEEDABLE - 1), []))

    def test_a_run_out_seeds_at_least_the_bound_it_ran_out_at(self):
        # Answers alone: 41.5 ms; ran out at 440 ms: the record is 440 ms, however few the answers.
        self.assertEqual(wc.seed([41_500_000] * wc.SEEDABLE, [440_000_000, 440_000_000]), 440_000_000)
        self.assertEqual(wc.seed([5], [80]), 80)

    def test_a_row_whose_answers_outrun_its_run_outs_seeds_from_its_max(self):
        self.assertEqual(wc.seed([900] * wc.SEEDABLE, [400]), 900)

    def test_the_seed_rule_converges_on_a_censored_row(self):
        # Do: a wait whose answers reach T, seeded at r0 far below it, through census after census, each answer past
        # the wait's bound (twice its record) a run-out at that bound, each census's seed the next record. Expect: the
        # record doubles each census until the bound holds every answer, within log2(T / r0) + 1 censuses, and then
        # nothing runs out and the record is the answers' max.
        answers = [1_000 + k for k in range(wc.SEEDABLE * 2)] + [3_000_000]  # T = 3 ms, the rest about 1 us
        record, r0 = 10_000, 10_000
        for census in range(1, 64):
            bound = TIMES_SLOWEST * record
            answered = [a for a in answers if a <= bound]
            ran_out = [bound for a in answers if a > bound]
            record = wc.seed(answered, ran_out)
            if not ran_out:
                break
        self.assertEqual(ran_out, [])
        self.assertEqual(record, max(answers))
        self.assertLessEqual(census, math.ceil(math.log2(max(answers) / r0)) + 1)


class Table(unittest.TestCase):
    def test_the_table_says_what_ran_out_and_the_record_it_gives(self):
        with tempfile.TemporaryDirectory() as d:
            out = Path(d)
            (out / "env.txt").write_text("host: test\n")
            lines = [f"client.answer.create {41_500_000}" for _ in range(wc.SEEDABLE)]
            lines += ["client.answer.create >440000000"] * 27
            (out / "census-suites.txt").write_text("\n".join(lines) + "\n")
            text = io.StringIO()
            with contextlib.redirect_stdout(text):
                wc.main(out)
        row = next(line for line in text.getvalue().splitlines() if line.startswith("| client.answer.create |"))
        self.assertIn("| 27 at 440.000 ms |", row)
        self.assertIn("record 440.000 ms", row)


if __name__ == "__main__":
    unittest.main()
