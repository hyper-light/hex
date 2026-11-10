"""Synthetic only: no original Harness construction, process creation, CLI, VM or capture reads."""
import importlib.util
import io
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import threading
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("cold_boot_under_test", Path(__file__).with_name("cold_boot_census.py"))
if spec is None or spec.loader is None:
    raise RuntimeError("cold driver module missing")
cold = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cold)


class ColdCohort(unittest.TestCase):
    def test_cp_t7_4_first_completed_is_distinct_from_evented_and_observers(self):
        rows = [b"client.answer.hello 777\n", b"client.guest.answer.hello 11\n", b"client.guest.answer.hello 3\n"]
        actual = cold.classify_hellos(rows, len(rows), max(map(len, rows)))
        self.assertEqual(actual["first_sdk_hello"]["nanos"], 11)
        self.assertEqual([row["nanos"] for row in actual["subsequent_answered_hellos"]], [3])
        self.assertEqual(actual["normal_answered_count"], len(cold.NORMAL_HELLO_PHASES))
        self.assertIn("root verification", actual["attribution"])
        self.assertFalse(actual["per_row_VM_identity_proven"])
        self.assertFalse(actual["warm_borrower_rows_included"])

    def test_cp_t7_4_retry_counts_are_ambiguous_without_losing_raw_outcomes(self):
        rows = [b"client.guest.answer.hello !2\n", b"client.guest.answer.hello >4\n",
                b"client.guest.answer.hello 9\n", b"client.guest.answer.hello 3\n", b"client.guest.answer.hello 5\n"]
        actual = cold.classify_hellos(rows, len(rows), max(map(len, rows)))
        self.assertEqual(actual["first_sdk_hello"]["line"], 3)
        self.assertEqual(len(actual["subsequent_answered_hellos"]), 2)
        self.assertEqual(actual["refused_hellos"][0]["raw"], rows[0].decode())
        self.assertEqual(actual["run_out_hellos"][0]["raw"], rows[1].decode())
        self.assertEqual(actual["attribution"], "ambiguous/excluded from first-VM cohort")

    def test_cp_t7_4_missing_cut_and_malformed_data_are_not_zero_success(self):
        actual = cold.classify_hellos([b"census full at 1\n", b"broken"], 1, len(b"census full at 1\n"))
        self.assertIsNone(actual["first_sdk_hello"])
        self.assertTrue(actual["full_markers"])
        self.assertTrue(actual["malformed"])
        self.assertEqual(actual["attribution"], "ambiguous/excluded from first-VM cohort")

    def test_cp_t7_4_next_guest_requires_fresh_root_and_stopped_proof(self):
        calls, writes = [], []

        def one(number):
            calls.append(number)
            return {"run": f"/synthetic/{number}", "status": "passed", "stopped_receipt_verified": number == 1}

        result = cold.campaign(3, one, lambda row: writes.append(json.loads(json.dumps(row))))
        self.assertEqual(calls, [1, 2])
        self.assertEqual(result["status"], "failed")
        self.assertEqual(len(writes[-1]["iterations"]), 2)
        with self.assertRaises(cold.census.Failed):
            cold.campaign(0, lambda _: self.fail("not called"), lambda _: None)
        calls.clear()

        def repeated(number):
            calls.append(number)
            return {"run": "/synthetic/same", "status": "passed", "stopped_receipt_verified": True}

        self.assertEqual(cold.campaign(3, repeated, lambda _: None)["status"], "failed")
        self.assertEqual(calls, [1, 2])


class SyntheticRecovery(unittest.TestCase):
    def setUp(self):
        parent = Path(os.environ["COLD_BOOT_SYNTHETIC_ROOT"])
        parent.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="unit-", dir=parent)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.state = self.root / "cold-node.node-state"
        self.state.mkdir()
        self.process = SimpleNamespace(pid=31415, returncode=None)
        self.events = []
        self.identity = {"record": str(self.state / "@guest"), "pid": 27182, "started": 7,
                         "vsock": str(self.root / "v"), "owned_vsock_root": None, "declared_config": {"same": True}}
        self.harness = SimpleNamespace(root=self.root, backend_state=self.state, backend_process=self.process,
            children=[("cold-node", self.process)], guest_receipt=None,
            runtime={"max_frame": 1 << 20, "wait_ns": 120_000_000_000}, report={"cold_boot": {}},
            stop=self.stop, probe=self.probe)

    def stop(self, process, crash):
        self.assertIs(process, self.process)
        self.assertTrue(crash)
        self.events.append("stop-backend")
        process.returncode = -9

    def probe(self, *arguments):
        self.assertIsNotNone(self.process.returncode)
        self.events.append(arguments[0])
        self.assertEqual(arguments[1], str(self.state / "@guest"))
        selected = Path(arguments[arguments.index("--receipt") + 1])
        if arguments[0] == "--remember-guest":
            with selected.open("x") as data:
                json.dump(self.identity, data)
            return {"remembered": True, "pid": self.identity["pid"], "started": self.identity["started"]}
        self.assertEqual(arguments[0], "--stop-owned-guest")
        return {"guest_stopped": True, "pid": self.identity["pid"], "started": self.identity["started"], "end": "Stopped"}

    def record(self):
        (self.state / "@guest").write_text("synthetic record; helper fake only; no OS identity query")

    def test_cp_t7_4_missing_record_cannot_dispatch_any_guest_probe(self):
        with self.assertRaises(FileNotFoundError):
            cold.recover_pre_ready(self.harness)
        self.assertEqual(self.events, ["stop-backend"])
        self.assertNotIn("owned_guest_retired", self.harness.report)

    def test_cp_t7_4_invalid_existing_receipt_never_dispatches_stop_or_replaces_it(self):
        self.record()
        self.harness.guest_receipt = self.root / "owned-guest.json"
        original = b"not JSON"
        self.harness.guest_receipt.write_bytes(original)
        with self.assertRaises(ValueError):
            cold.recover_pre_ready(self.harness)
        self.assertEqual(self.events, ["stop-backend"])
        self.assertEqual(self.harness.guest_receipt.read_bytes(), original)

    def test_cp_t7_4_mismatched_valid_receipt_stops_before_guest_stop(self):
        self.record()
        self.harness.guest_receipt = self.root / "owned-guest.json"
        original = {**self.identity, "started": self.identity["started"] + 1}
        self.harness.guest_receipt.write_text(json.dumps(original))
        with self.assertRaises(cold.census.Failed):
            cold.recover_pre_ready(self.harness)
        self.assertEqual(self.events, ["stop-backend", "--remember-guest"])
        self.assertEqual(json.loads(self.harness.guest_receipt.read_text()), original)
        self.assertNotIn("owned_guest_retired", self.harness.report)

    def test_cp_t7_4_duplicate_or_linked_receipt_never_dispatches_guest_stop(self):
        self.record()
        original = self.root / "owned-guest.json"
        original.write_text('{"pid":1,"pid":2}')
        self.harness.guest_receipt = original
        with self.assertRaises(cold.census.Failed):
            cold.recover_pre_ready(self.harness)
        self.assertEqual(self.events, ["stop-backend"])
        self.process.returncode = None
        self.harness.backend_process = self.process
        original.unlink()
        original.symlink_to(self.root / "missing-receipt")
        with self.assertRaises(cold.census.Failed):
            cold.recover_pre_ready(self.harness)
        self.assertEqual(self.events, ["stop-backend", "stop-backend"])

    def test_cp_t7_4_existing_exact_receipt_is_used_only_after_fresh_helper_comparison(self):
        self.record()
        original = self.root / "owned-guest.json"
        original.write_text(json.dumps(self.identity))
        self.harness.guest_receipt = original
        cold.recover_pre_ready(self.harness)
        self.assertEqual(self.events, ["stop-backend", "--remember-guest", "--stop-owned-guest"])
        self.assertEqual(self.harness.guest_receipt, original)
        self.assertEqual(self.harness.report["owned_guest_retired"]["end"], "Stopped")
        self.assertIsNone(self.harness.backend_process)

    def test_cp_t7_4_reaped_backend_keeps_its_saved_popen_after_live_children_are_pruned(self):
        self.record()
        self.harness.cold_process = self.process
        self.process.returncode = -9
        self.harness.children = []
        cold.recover_pre_ready(self.harness)
        self.assertEqual(self.events, ["stop-backend", "--remember-guest", "--stop-owned-guest"])
        self.assertEqual(self.harness.report["owned_guest_retired"]["pid"], self.identity["pid"])

    def post_ready_failure(self, original_reaped):
        self.record()
        harness = object.__new__(cold.ColdHarness)
        harness.__dict__.update(self.harness.__dict__)
        harness.cold_process = self.process
        harness.report["cold_boot"]["backend_ready"] = True
        failure = cold.census.Failed("original synthetic finish failure")

        def original(instance):
            if original_reaped:
                self.process.returncode = -9
                instance.backend_process = None
                instance.children = []
            raise failure

        with patch.object(cold.census.Harness, "finish_backend", original):
            with self.assertRaises(cold.census.Failed) as observed:
                harness.finish_backend()
        self.assertIs(observed.exception, failure)
        self.assertEqual(self.events, ["stop-backend", "--remember-guest", "--stop-owned-guest"])
        self.assertEqual(harness.report["owned_guest_retired"]["end"], "Stopped")
        self.assertEqual(harness.report["status"], "failed")
        self.assertEqual(harness.report["cold_boot"]["original_finish_failure"], str(failure))

    def test_cp_t7_4_post_ready_snapshot_failure_recovers_then_reraises_original(self):
        self.post_ready_failure(original_reaped=False)

    def test_cp_t7_4_post_ready_retire_failure_recovers_after_original_cleared_backend(self):
        self.post_ready_failure(original_reaped=True)

    def test_cp_t7_4_pre_ready_recovery_proves_closure_but_never_sample_success(self):
        self.record()
        self.harness.cold_process = self.process
        self.harness.report["cold_boot"]["backend_ready"] = False
        cold.recover_pre_ready(self.harness)
        directory = self.root / "census"
        directory.mkdir()
        path = directory / "backend-node.tsv"
        path.write_bytes(b"client.guest.answer.hello 9\nclient.guest.answer.hello 1\n")
        self.process.hex_census_path = str(path)
        self.harness.args = SimpleNamespace(max_census_lines=len(cold.NORMAL_HELLO_PHASES))
        self.harness.runtime.update(ledger=str(self.root / "pids.jsonl"), profile="synthetic")
        with self.assertRaises(cold.census.Failed):
            cold.ColdHarness.census_report(self.harness)
        report = self.harness.report["cold_boot"]
        self.assertTrue(report["stopped_receipt_verified"])
        self.assertEqual(report["stopped_identity_basis"], "recovery")
        self.assertTrue(report["backend_reaped"])
        self.assertFalse(report["backend_ready"])
        self.assertFalse(report.get("ready_class_verified", False))


class ArmedColdBoot(unittest.TestCase):
    def setUp(self):
        parent = Path(os.environ["COLD_BOOT_SYNTHETIC_ROOT"])
        parent.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="armed-unit-", dir=parent)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        (self.root / "census").mkdir()

    def closed(self):
        harness = object.__new__(cold.ColdHarness)
        harness.root = self.root
        harness.runtime = {"wait_mode": "armed", "ledger": str(self.root / "pids.jsonl"), "profile": "synthetic",
                           "source_revision": "synthetic-source", "max_frame": 1 << 20}
        harness.cold_process = SimpleNamespace(pid=31415, returncode=0, hex_census_path=None)
        harness.report = {"kind": "cold-node-seeded-validation", "cold_boot": {"backend_ready": True,
            "ready_class_verified": True, "identity": {"pid": 27182, "started": 7, "vsock": "synthetic-owned"}},
            "owned_guest_retired": {"guest_stopped": True, "end": "Stopped", "pid": 27182, "started": 7}}
        return harness

    def test_cp_t7_4_armed_constructor_preserves_measuring_default_report(self):
        def original(harness, args):
            harness.root = self.root
            harness.runtime = {"wait_mode": "armed" if args.validate_seeds else "measuring"}
            harness.report = {}

        args = SimpleNamespace(mode="guest", deadline_seconds=120, validate_seeds=False, run_parent=str(self.root))
        with patch.object(cold.census.Harness, "__init__", original), patch.object(cold.ColdHarness, "instances", []):
            measuring = cold.ColdHarness(args)
            self.assertEqual(measuring.report, {"kind": "cold-node-boot-SDK-Hello-census", "cold_boot": {
                "backend_ready": False, "cohort": "FIRST-BOOT backend-process first SDK Hello",
                "seed_installation": "not performed", "boxes_created": False,
                "workload_success": "not exercised", "full_boot_latency": "not measured"}})
            args.validate_seeds = True
            armed = cold.ColdHarness(args)
        self.assertEqual(armed.runtime["wait_mode"], "armed")
        self.assertEqual(armed.report["kind"], "cold-node-seeded-validation")
        self.assertNotIn("cohort", armed.report["cold_boot"])
        self.assertNotIn("sdk_hello", armed.report["cold_boot"])

    def test_cp_t7_4_armed_backend_and_probe_spawns_remove_both_census_variables(self):
        runtime = {"root": str(self.root), "wait_mode": "armed", "hex": "/synthetic/hex",
                   "runtime_client": "/synthetic/probe", "ledger": str(self.root / "pids.jsonl"), "profile": "synthetic"}
        environment = {"HEX_WAITS_CENSUS": "/unrelated/inherited.tsv", "HEX_WAITS_CENSUS_MAX": "123", "KEEP": "same"}
        harness = SimpleNamespace(root=self.root, runtime=runtime, phase="synthetic", environment=environment,
                                  children=[], children_lock=threading.Lock(), child_bound=2)
        opened = []

        def popen(argv, **options):
            opened.append((argv, options["env"]))
            return SimpleNamespace(pid=len(opened), poll=lambda: None)

        with patch.object(cold.census.subprocess, "Popen", popen):
            for name, binary in (("cold-node", runtime["hex"]), ("probe", runtime["runtime_client"])):
                child = cold.census.Harness.spawn(harness, name, [binary])
                self.assertIsNone(child.hex_census_path)
        self.assertEqual([env for _, env in opened], [{"KEEP": "same"}, {"KEEP": "same"}])
        self.assertEqual(environment["HEX_WAITS_CENSUS_MAX"], "123")
        self.assertEqual(list((self.root / "census").iterdir()), [])
        ledger = [json.loads(line) for line in Path(runtime["ledger"]).read_text().splitlines()]
        self.assertEqual([row["census"] for row in ledger], [None, None])

    def test_cp_t7_4_armed_report_requires_no_census_and_never_parses_samples(self):
        harness = self.closed()
        with patch.object(cold, "classify_hellos", side_effect=AssertionError("armed must not classify")):
            harness.census_report()
        self.assertEqual(harness.report["census"]["files"], 0)
        self.assertFalse(harness.report["census"]["enabled"])
        self.assertTrue(harness.report["cold_boot"]["stopped_receipt_verified"])
        self.assertTrue(harness.report["cold_boot"]["backend_reaped"])
        self.assertNotIn("sdk_hello", harness.report["cold_boot"])
        self.assertNotIn("cohort", harness.report["cold_boot"])

    def test_cp_t7_4_armed_rejects_any_census_file_without_reading_it(self):
        (self.root / "census" / "unexpected.tsv").write_bytes(b"synthetic; must not parse\n")
        with patch.object(cold, "classify_hellos", side_effect=AssertionError("armed must not classify")):
            with self.assertRaisesRegex(cold.census.Failed, "unexpectedly produced census files"):
                self.closed().census_report()

    def test_cp_t7_4_armed_closure_and_ready_guards_remain_required(self):
        for fault in ("not-ready", "wrong-class", "not-reaped", "wrong-identity", "not-stopped",
                      "backend_cleanup_error", "cleanup_error", "cleanup_unproven"):
            with self.subTest(fault=fault):
                harness = self.closed()
                if fault == "not-ready":
                    harness.report["cold_boot"]["backend_ready"] = False
                elif fault == "wrong-class":
                    harness.report["cold_boot"]["ready_class_verified"] = False
                elif fault == "not-reaped":
                    harness.cold_process.returncode = None
                elif fault == "wrong-identity":
                    harness.report["owned_guest_retired"]["started"] += 1
                elif fault == "not-stopped":
                    harness.report["owned_guest_retired"]["end"] = "unknown"
                elif fault == "cleanup_unproven":
                    harness.report["cold_boot"][fault] = "synthetic failure"
                else:
                    harness.report[fault] = "synthetic failure"
                with self.assertRaises(cold.census.Failed):
                    harness.census_report()

    def test_cp_t7_4_armed_campaign_keeps_mode_and_aborts_before_another_guest(self):
        calls = []

        def one(number):
            calls.append(number)
            return {"run": f"/synthetic/{number}", "kind": "cold-node-seeded-validation", "status": "passed",
                    "stopped_receipt_verified": number == 1}

        report = cold.campaign(3, one, lambda _: None, validate_seeds=True)
        self.assertEqual(calls, [1, 2])
        self.assertEqual(report["kind"], "cold-node-seeded-validation")
        self.assertEqual(report["status"], "failed")
        self.assertNotIn("ambiguous_first_VM_cohorts", report)
        report = cold.campaign(1, one, lambda _: None, validate_seeds=True)
        self.assertEqual(report["status"], "completed")
        self.assertNotIn("ambiguous_first_VM_cohorts", report)
        wrong_mode = lambda _: {**one(1), "kind": "cold-node-boot-SDK-Hello-census"}
        self.assertEqual(cold.campaign(1, wrong_mode, lambda _: None, validate_seeds=True)["status"], "failed")

    def test_cp_t7_4_main_forwards_armed_flag_to_existing_harness(self):
        output = self.root / "campaign.json"
        forwarded = []

        def existing_main():
            forwarded.append(list(cold.sys.argv))
            armed = "--validate-seeds" in cold.sys.argv
            kind = "cold-node-seeded-validation" if armed else "cold-node-boot-SDK-Hello-census"
            harness = SimpleNamespace(root=self.root / str(len(forwarded)), report={"kind": kind, "status": "passed",
                "cold_boot": {"stopped_receipt_verified": True}})
            cold.ColdHarness.instances.append(harness)

        arguments = ["synthetic-driver", "--n", "2", "--output", str(output), "--validate-seeds"]
        with patch.object(cold.sys, "argv", arguments), patch.object(cold.census, "main", existing_main), redirect_stdout(io.StringIO()):
            self.assertEqual(cold.main(), 0)
        self.assertEqual(len(forwarded), 2)
        for arguments in forwarded:
            self.assertEqual(arguments.count("--validate-seeds"), 1)
            self.assertEqual(arguments[-2:], ["--n", "1"])
        result = json.loads(output.read_text())
        self.assertEqual(result["kind"], "cold-node-seeded-validation")
        self.assertEqual(result["status"], "completed")
        self.assertNotIn("ambiguous_first_VM_cohorts", result)
        self.assertTrue(all(row["kind"] == result["kind"] for row in result["iterations"]))

    def test_cp_t7_4_measuring_main_preserves_legacy_three_argument_campaign_hook(self):
        output = self.root / "legacy-campaign.json"
        forwarded, campaigns = [], []
        original_campaign = cold.campaign

        def legacy_campaign(count, one, save):
            campaigns.append(count)
            return original_campaign(count, one, save)

        def existing_main():
            forwarded.append(list(cold.sys.argv))
            harness = SimpleNamespace(root=self.root / str(len(forwarded)), report={
                "kind": "cold-node-boot-SDK-Hello-census", "status": "passed",
                "cold_boot": {"stopped_receipt_verified": True}})
            cold.ColdHarness.instances.append(harness)

        arguments = ["synthetic-driver", "--n", "2", "--output", str(output)]
        with patch.object(cold.sys, "argv", arguments), patch.object(cold.census, "main", existing_main), \
                patch.object(cold, "campaign", legacy_campaign), redirect_stdout(io.StringIO()):
            self.assertEqual(cold.main(), 0)
        self.assertEqual(campaigns, [2])
        self.assertEqual(len(forwarded), 2)
        self.assertTrue(all("--validate-seeds" not in arguments for arguments in forwarded))
        result = json.loads(output.read_text())
        self.assertEqual(result["kind"], "cold-node-boot-SDK-Hello-census")
        self.assertEqual(result["status"], "completed")
        self.assertEqual(result["ambiguous_first_VM_cohorts"], 0)


if __name__ == "__main__":
    unittest.main()
