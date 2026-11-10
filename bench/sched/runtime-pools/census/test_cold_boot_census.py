"""Synthetic only: no original Harness construction, process creation, CLI, VM or capture reads."""
import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest


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


if __name__ == "__main__":
    unittest.main()
