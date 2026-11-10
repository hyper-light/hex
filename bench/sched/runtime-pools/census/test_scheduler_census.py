#!/usr/bin/env python3
"""Cache-only report regressions. All rows are SYNTHETIC fixtures, never runtime measurements/seeds."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

import scheduler_census as census


class CensusIdentityTests(unittest.TestCase):
    def test_reused_pid_keeps_each_census_file_call_outcome(self):
        parent = Path.home() / ".cache/hex-lead/census-report-tests"
        parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="synthetic-", dir=parent) as temporary:
            root = Path(temporary)
            (root / "census").mkdir()
            refused = root / "census/refused.tsv"
            succeeded = root / "census/succeeded.tsv"
            row = "client.answer.schedule.delete 1\n"
            for path in (refused, succeeded):
                path.write_text(row)
            reused_pid = 123
            ledger = [
                {"kind": "call", "pid": reused_pid, "census": str(refused)},
                {"kind": "census-call-exit", "pid": reused_pid, "census": str(refused), "success": False},
                {"kind": "call", "pid": reused_pid, "census": str(succeeded)},
                {"kind": "census-call-exit", "pid": reused_pid, "census": str(succeeded), "success": True},
            ]
            ledger_path = root / "pids.jsonl"
            ledger_path.write_text("".join(json.dumps(entry) + "\n" for entry in ledger))
            harness = object.__new__(census.Harness)
            harness.root = root
            harness.runtime = {"root": str(root), "ledger": str(ledger_path), "max_frame": len(row.encode())}
            harness.args = SimpleNamespace(n=1, max_census_lines=1)
            harness.wait_kinds = ["delete"]
            harness.report = {}
            harness.census_report()
            result = harness.report["census"]
            per_file = {entry["file"]: entry for entry in result["per_process"]}
            self.assertFalse(per_file[refused.name]["call_outcome"]["success"])
            self.assertTrue(per_file[succeeded.name]["call_outcome"]["success"])
            self.assertEqual(per_file[refused.name]["producer"]["kind"], "call")
            self.assertEqual(per_file[succeeded.name]["producer"]["kind"], "call")
            self.assertEqual(result["successful_single_call_rows"], {"client.answer.schedule.delete": 1})
            self.assertEqual(result["answer_rows_by_name"], {"client.answer.schedule.delete": 2})


    def test_reused_pid_keeps_each_census_file_process_outcome_separate_from_calls(self):
        parent = Path.home() / ".cache/hex-lead/census-report-tests"
        parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="synthetic-process-", dir=parent) as temporary:
            root = Path(temporary)
            (root / "census").mkdir()
            failed = root / "census/probe-failed.tsv"
            succeeded = root / "census/probe-succeeded.tsv"
            called = root / "census/schedule-call.tsv"
            capacity_row = "client.answer.capacity 1\n"
            call_row = "client.answer.schedule.delete 1\n"
            failed.write_text(capacity_row)
            succeeded.write_text(capacity_row)
            called.write_text(call_row)
            reused_pid = 123
            ledger = [
                {"kind": "runtime-probe", "pid": reused_pid, "census": str(failed)},
                {"kind": "census-process-exit", "pid": reused_pid, "census": str(failed), "returncode": 1, "success": False},
                {"kind": "runtime-probe", "pid": reused_pid, "census": str(succeeded)},
                {"kind": "census-process-exit", "pid": reused_pid, "census": str(succeeded), "returncode": 0, "success": True},
                {"kind": "call", "pid": 124, "census": str(called)},
                {"kind": "census-call-exit", "pid": 124, "census": str(called), "returncode": 0, "success": True},
            ]
            ledger_path = root / "pids.jsonl"
            ledger_path.write_text("".join(json.dumps(entry) + "\n" for entry in ledger))
            harness = object.__new__(census.Harness)
            harness.root = root
            harness.runtime = {"root": str(root), "ledger": str(ledger_path), "max_frame": max(len(capacity_row.encode()), len(call_row.encode()))}
            harness.args = SimpleNamespace(n=1, max_census_lines=1)
            harness.wait_kinds = ["delete"]
            harness.report = {}
            harness.census_report()
            result = harness.report["census"]
            per_file = {entry["file"]: entry for entry in result["per_process"]}
            self.assertFalse(per_file[failed.name]["process_outcome"]["success"])
            self.assertEqual(per_file[failed.name]["process_outcome"]["returncode"], 1)
            self.assertTrue(per_file[succeeded.name]["process_outcome"]["success"])
            self.assertEqual(per_file[succeeded.name]["process_outcome"]["returncode"], 0)
            for path in (failed, succeeded):
                self.assertEqual(per_file[path.name]["producer"]["kind"], "runtime-probe")
                self.assertEqual(per_file[path.name]["call_outcome"], {})
            self.assertEqual(per_file[called.name]["process_outcome"], {})
            self.assertEqual(result["successful_process_rows"], {"client.answer.capacity": 1})
            self.assertEqual(result["successful_single_call_rows"], {"client.answer.schedule.delete": 1})
            self.assertEqual(result["answer_rows_by_name"], {"client.answer.capacity": 2, "client.answer.schedule.delete": 1})


if __name__ == "__main__":
    unittest.main()
