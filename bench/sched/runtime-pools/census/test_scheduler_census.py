#!/usr/bin/env python3
"""Cache-only report regressions. All rows are SYNTHETIC fixtures, never runtime measurements/seeds."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import scheduler_census as census


class SeededValidationTests(unittest.TestCase):
    """AC-T7.4.1: synthetic process seams only; no Hex, SSH, Runtime or VM is executed."""
    def setUp(self):
        parent = Path.home() / ".cache/hex-lead/census-report-tests"
        parent.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="synthetic-armed-", dir=parent)
        self.addCleanup(self.temporary.cleanup)
        self.parent = Path(self.temporary.name)

    def harness(self, validate_seeds):
        source = Path(__file__).resolve().parents[4]
        binary = self.parent / "synthetic-not-executable"
        binary.write_bytes(b"synthetic fixture; never executed\n")
        receipt = self.parent / "synthetic-receipt.json"
        receipt.write_text("{}\n")
        limits = census.policy(source)
        args = SimpleNamespace(source=str(source), mode="native", run_parent=str(self.parent),
                               hex=str(binary), runtime_client=str(binary), linux_hex=str(binary),
                               profile="debug", n=1, max_census_lines=1, validate_seeds=validate_seeds,
                               deadline_seconds=120, sshd=str(binary), cpu_milli=limits["min_cpu"],
                               box_memory_mib=(limits["min_memory"] + (1 << 20) - 1) // (1 << 20),
                               cgroup_parent=str(self.parent / "synthetic-delegated"), guest_cpus=None,
                               guest_memory_mib=None, policy_receipt=str(receipt), build_receipt=str(receipt),
                               deadline_evidence=str(receipt))
        with mock.patch.object(census.sys, "platform", "linux"), mock.patch.object(census.shutil, "which", return_value=str(binary)):
            return census.Harness(args)

    def test_default_mode_still_installs_unique_measuring_census(self):
        harness = self.harness(False)
        self.assertEqual(harness.runtime["wait_mode"], "measuring")
        self.assertEqual(harness.report["kind"], "actual-scheduler-wait-census")
        # Older private runtime fixtures without the new mode retain measuring behavior.
        runtime = dict(harness.runtime)
        runtime.pop("wait_mode")
        environment, path = census.census_environment(runtime, {"HEX_WAITS_CENSUS": "inherited", "HEX_WAITS_CENSUS_MAX": "inherited"}, "synthetic")
        self.assertTrue(path.is_file())
        self.assertEqual(environment["HEX_WAITS_CENSUS"], str(path))
        self.assertEqual(environment["HEX_WAITS_CENSUS_MAX"], str(harness.args.max_census_lines))

    def test_armed_report_has_no_census_coverage_and_refuses_any_census_file(self):
        harness = self.harness(True)
        self.assertEqual(census.runtime_file(harness.runtime_path)["wait_mode"], "armed")
        self.assertEqual(harness.report["kind"], "actual-scheduler-seeded-validation")
        self.assertNotIn("max_rows_per_process", harness.report)
        environment, path = census.census_environment(harness.runtime, {"HEX_WAITS_CENSUS": "inherited", "HEX_WAITS_CENSUS_MAX": "inherited", "retained": "value"}, "synthetic")
        self.assertIsNone(path)
        self.assertEqual(environment, {"retained": "value"})
        harness.census_report()
        self.assertEqual(harness.report["census"], {"enabled": False, "files": 0, "scope": "census disabled; no timing distributions, coverage or seed evidence"})
        (harness.root / "census/stray-not-a-tsv").write_text("synthetic\n")
        with self.assertRaisesRegex(census.Failed, "unexpectedly produced census files"):
            harness.census_report()

    def test_armed_local_hex_and_probe_spawns_remove_inherited_census(self):
        harness = self.harness(True)
        # Distinct paths exercise both selected-binary branches without launching either.
        harness.runtime["runtime_client"] += "-probe"
        for binary in (harness.runtime["hex"], harness.runtime["runtime_client"]):
            with self.subTest(binary=binary):
                child = mock.Mock(pid=123)
                child.poll.return_value = 0
                with mock.patch.object(census.subprocess, "Popen", return_value=child) as launch:
                    process = harness.spawn("synthetic", [binary], env={"HEX_WAITS_CENSUS": "inherited", "HEX_WAITS_CENSUS_MAX": "inherited"})
                self.assertEqual(launch.call_args.kwargs["env"], {})
                self.assertIsNone(process.hex_census_path)
        rows = [json.loads(line) for line in Path(harness.runtime["ledger"]).read_text().splitlines()]
        self.assertTrue(all(row["census"] is None for row in rows))
        self.assertEqual(list((harness.root / "census").iterdir()), [])

    def test_armed_forced_tunnel_uses_persisted_mode_and_records_null_path(self):
        harness = self.harness(True)
        child = mock.Mock(pid=123, returncode=0)
        child.poll.return_value = 0
        inherited = {"SSH_ORIGINAL_COMMAND": "synthetic-forced-command", "HEX_WAITS_CENSUS": "inherited", "HEX_WAITS_CENSUS_MAX": "inherited"}
        with mock.patch.dict(census.os.environ, inherited), \
             mock.patch.object(census.subprocess, "Popen", return_value=child) as launch, \
             mock.patch.object(census.threading, "Thread"), mock.patch.object(census, "frame", return_value=None):
            census.forced(harness.runtime_path, 0, "synthetic-scheduler-socket")
        environment = launch.call_args.kwargs["env"]
        self.assertNotIn("HEX_WAITS_CENSUS", environment)
        self.assertNotIn("HEX_WAITS_CENSUS_MAX", environment)
        rows = [json.loads(line) for line in Path(harness.runtime["ledger"]).read_text().splitlines()]
        tunnel = next(row for row in rows if row["kind"] == "forced-tunnel")
        self.assertIsNone(tunnel["census"])
        self.assertEqual(list((harness.root / "census").iterdir()), [])


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


    def test_refusal_census_retains_marked_refusals_outside_success_rows(self):
        parent = Path.home() / ".cache/hex-lead/census-report-tests"
        parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="synthetic-refusals-", dir=parent) as temporary:
            root = Path(temporary)
            (root / "census").mkdir()
            path = root / "census/multiplexed.tsv"
            rows = "client.answer.schedule.hear_control 1\nclient.answer.schedule.hear_control !900\nclient.answer.schedule.hear_control >3\n"
            path.write_text(rows)
            ledger_path = root / "pids.jsonl"
            ledger_path.write_text(json.dumps({"kind": "forced-tunnel", "pid": 123, "census": str(path)}) + "\n")
            harness = object.__new__(census.Harness)
            harness.root = root
            harness.runtime = {"root": str(root), "ledger": str(ledger_path), "max_frame": len(rows.encode())}
            harness.args = SimpleNamespace(n=1, max_census_lines=3)
            harness.wait_kinds = ["hear_control"]
            harness.report = {}
            harness.census_report()
            result = harness.report["census"]
            name = "client.answer.schedule.hear_control"
            self.assertEqual(result["answer_rows_by_name"], {name: 1})
            self.assertEqual(result["refused_rows_by_name"], {name: 1})
            self.assertEqual(result["per_process"][0]["answers"], {name: 1})
            self.assertEqual(result["per_process"][0]["refused"], {name: 1})
            self.assertEqual(result["per_process"][0]["ran_out"], {name: 1})
            self.assertEqual(result["successful_single_call_rows"], {})
            self.assertEqual(result["successful_process_rows"], {})
            self.assertEqual(result["malformed_files"], [])


if __name__ == "__main__":
    unittest.main()
