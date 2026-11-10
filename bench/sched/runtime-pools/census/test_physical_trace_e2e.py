"""Synthetic only: no original construction, subprocess, API, VM or capture access."""
import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("physical_trace_under_test", Path(__file__).with_name("physical_trace_e2e.py"))
if spec is None or spec.loader is None:
    raise RuntimeError("physical trace fixture missing")
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class PhysicalTrace(unittest.TestCase):
    def setUp(self):
        parent = Path(os.environ["PHYSICAL_TRACE_SYNTHETIC_ROOT"])
        parent.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="unit-", dir=parent)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.cold = fixture.load_cold(Path(__file__).with_name("cold_boot_census.py"))
        self.base = self.cold.census.Harness
        self.cold.ColdHarness.instances = []
        self.args = SimpleNamespace(validate_seeds=True, mode="guest", deadline_seconds=120,
                                    run_parent=str(self.root), n=3)
        self.adapter = fixture.trace_harness(self.cold, False)

    def initialize(self, harness, args):
        harness.root = self.root
        harness.args, harness.mode = args, args.mode
        harness.environment = os.environ.copy()
        harness.backend_env = harness.environment.copy()
        harness.report = {}
        harness.backend_process = harness.backend_socket = harness.backend_state = harness.guest_receipt = None

    def construct(self, trace=False):
        adapter = fixture.trace_harness(self.cold, trace)
        with patch.object(self.base, "__init__", lambda harness, args: self.initialize(harness, args)):
            return adapter(self.args)

    def ready(self):
        harness = self.construct()
        harness.backend_process = harness.cold_process = SimpleNamespace(poll=lambda: None)
        harness.backend_state = self.root / "cold-node.node-state"
        harness.backend_socket = self.root / "cold-node.sock"
        harness.guest_receipt = self.root / "owned-guest.json"
        harness.report["cold_boot"].update(backend_ready=True, ready_class_verified=True,
                                           receipt=str(harness.guest_receipt))
        return harness

    def test_ac_t7_4_trace_is_explicit_child_only_and_report_is_diagnostic(self):
        with patch.dict(os.environ, {fixture.DEBUG: "inherited", "SYNTHETIC_KEEP": "same"}):
            for trace in (False, True):
                with self.subTest(trace=trace):
                    before = os.environ.copy()
                    harness = self.construct(trace)
                    self.assertEqual(dict(os.environ), before)
                    for environment in (harness.environment, harness.backend_env):
                        if trace:
                            self.assertEqual(environment[fixture.DEBUG], "1")
                        else:
                            self.assertNotIn(fixture.DEBUG, environment)
                        self.assertEqual(environment["SYNTHETIC_KEEP"], "same")
                    self.assertEqual(harness.report["kind"], "diagnostic-physical-armed-trace")
                    self.assertFalse(harness.report["calibration_eligibility"])
                    self.assertIs(harness.report["trace_enabled"], trace)
                    self.assertIn("before ordinary SSH setup", harness.report["startup_order"])
                    for absent in ("boxes_created", "workload_success", "cohort", "sdk_hello"):
                        self.assertNotIn(absent, harness.report["cold_boot"])
                    self.assertEqual(harness.args.n, 3)

    def test_ac_t7_4_requires_armed_guest_and_original_budget_before_construction(self):
        for altered in ({"validate_seeds": False}, {"mode": "native"}, {"deadline_seconds": 119}, {"deadline_seconds": 121}):
            with self.subTest(altered=altered):
                args = SimpleNamespace(**{**vars(self.args), **altered})
                with patch.object(self.base, "__init__", side_effect=AssertionError("base must not construct")):
                    with self.assertRaises(self.cold.census.Failed):
                        self.adapter(args)

    def test_ac_t7_4_cold_readiness_precedes_main_and_real_reuses_one_backend(self):
        harness = self.construct()
        events, sockets = [], []

        def create(instance, name):
            events.append(("create", name))
            instance.backend_process = SimpleNamespace(poll=lambda: None)
            instance.backend_state = self.root / "cold-node.node-state"
            instance.backend_socket = self.root / "cold-node.sock"
            instance.guest_receipt = self.root / "owned-guest.json"
            return instance.backend_socket

        def cold_exercise(instance):
            events.append("cold")
            sockets.append(instance.backend("cold-node"))
            instance.cold_process = instance.backend_process
            instance.report["cold_boot"].update(backend_ready=True, ready_class_verified=True,
                                                receipt=str(instance.guest_receipt))

        def main_exercise(instance):
            events.append("physical")
            sockets.append(instance.backend("real"))
            self.assertEqual(instance.args.n, 3)

        with patch.object(self.base, "backend", create), patch.object(self.base, "exercise", main_exercise), \
                patch.object(self.cold.ColdHarness, "exercise", cold_exercise):
            harness.exercise()
        self.assertEqual(events, ["cold", ("create", "cold-node"), "physical"])
        self.assertEqual(sockets, [self.root / "cold-node.sock"] * 2)
        self.assertIs(harness.backend_process, harness.cold_process)

    def test_ac_t7_4_rebound_dead_or_unverified_backend_cannot_spawn_a_replacement(self):
        for fault in ("not-ready", "wrong-class", "different-process", "dead", "state", "socket", "receipt", "receipt-proof"):
            with self.subTest(fault=fault):
                harness = self.ready()
                if fault == "not-ready":
                    harness.report["cold_boot"]["backend_ready"] = False
                elif fault == "wrong-class":
                    harness.report["cold_boot"]["ready_class_verified"] = False
                elif fault == "different-process":
                    harness.backend_process = SimpleNamespace(poll=lambda: None)
                elif fault == "dead":
                    harness.cold_process.poll = lambda: 1
                elif fault in ("state", "socket"):
                    setattr(harness, "backend_" + fault, self.root / "foreign")
                elif fault == "receipt":
                    harness.guest_receipt = self.root / "foreign"
                else:
                    harness.report["cold_boot"]["receipt"] = str(self.root / "foreign")
                with patch.object(self.base, "backend", side_effect=AssertionError("no replacement backend")):
                    with self.assertRaises(self.cold.census.Failed):
                        harness.backend("real")

    def test_ac_t7_4_unknown_or_repeated_backend_names_refuse_before_creation(self):
        with patch.object(self.base, "backend", side_effect=AssertionError("no additional backend")):
            for harness, name in ((self.construct(), "unexpected"), (self.construct(), "real"), (self.ready(), "cold-node")):
                with self.subTest(name=name):
                    with self.assertRaises(self.cold.census.Failed):
                        harness.backend(name)

    def test_ac_t7_4_main_preserves_physical_count_and_restores_module_bindings(self):
        original_instances = self.cold.ColdHarness.instances
        arguments = ["synthetic-trace", "--cold-driver", str(self.root / "cold.py"), "--trace",
                     "--validate-seeds", "--mode", "guest", "--n", "3"]
        forwarded = []

        def run_main():
            forwarded.extend(fixture.sys.argv)
            self.assertEqual(self.cold.ColdHarness.instances, [])
            instance = self.cold.census.Harness(self.args)
            self.assertEqual(self.cold.ColdHarness.instances, [instance])
            self.assertTrue(instance.report["trace_enabled"])

        with patch.object(fixture, "load_cold", return_value=self.cold), patch.object(fixture.sys, "argv", arguments), \
                patch.object(self.base, "__init__", lambda harness, args: self.initialize(harness, args)), \
                patch.object(self.cold.census, "main", run_main):
            self.assertEqual(fixture.main(), 0)
            self.assertIs(fixture.sys.argv, arguments)
        self.assertEqual(forwarded, ["synthetic-trace", "--validate-seeds", "--mode", "guest", "--n", "3"])
        self.assertIs(self.cold.census.Harness, self.base)
        self.assertIs(self.cold.ColdHarness.instances, original_instances)

    def test_ac_t7_4_main_restores_bindings_after_original_failure(self):
        arguments = ["synthetic-trace", "--cold-driver", str(self.root / "cold.py"), "--validate-seeds"]
        original_instances = self.cold.ColdHarness.instances
        failure = self.cold.census.Failed("synthetic original failure")
        with patch.object(fixture, "load_cold", return_value=self.cold), patch.object(fixture.sys, "argv", arguments), \
                patch.object(self.cold.census, "main", side_effect=failure):
            with self.assertRaises(self.cold.census.Failed) as observed:
                fixture.main()
            self.assertIs(observed.exception, failure)
            self.assertIs(fixture.sys.argv, arguments)
        self.assertIs(self.cold.census.Harness, self.base)
        self.assertIs(self.cold.ColdHarness.instances, original_instances)

    def exercise_fault(self, fault):
        harness = self.construct()
        (self.root / "census").mkdir()
        (self.root / "SOURCE_REVISION").write_text("synthetic-source")
        harness.source, harness.runtime_path = self.root, self.root / "runtime.json"
        harness.runtime = {"wait_mode": "armed", "max_frame": 1 << 20, "wait_ns": 120_000_000_000,
                           "ledger": str(self.root / "pids.jsonl"), "profile": "synthetic"}
        harness.report["binary_and_artifact_hashes"] = {"hex": {"sha256": "synthetic-input"}}
        harness.scheduler_processes, harness.children, harness.baseline = [], [], []
        process = SimpleNamespace(pid=31415, returncode=None, hex_census_path=None)
        process.poll = lambda: process.returncode
        state, owner = self.root / "cold-node.node-state", self.root / "owned-guest.json"
        identity = {"record": str(state / "@guest"), "pid": 27182, "started": 7,
                    "vsock": str(self.root / "v"), "declared_config": {"synthetic": True}}
        events = []

        def create(instance, name):
            events.append("create-" + name)
            self.assertEqual(name, "cold-node")
            state.mkdir()
            (state / "@guest").write_text("synthetic exact private record; no OS identity")
            instance.backend_process, instance.backend_state = process, state
            instance.backend_socket = self.root / "cold-node.sock"
            instance.children.append((name, process))
            instance.ready(process, instance.backend_socket)
            instance.guest_receipt = owner
            owner.write_text(json.dumps(identity))
            return instance.backend_socket

        def snapshot(instance, inspect=None):
            if inspect is not None:
                events.append("physical-inspect-" + inspect)
            return {"classes": ["oci"], "boxes": [], "capacity": {"id": "guest:27182:7"}}

        def physical(instance):
            events.append("physical-phase")
            self.assertEqual(instance.backend("real"), self.root / "cold-node.sock")
            instance.snapshot(inspect="owned-key")

        def stop(child, crash):
            self.assertIs(child, process)
            self.assertTrue(crash)
            events.append("reap-original-backend")
            child.returncode = -9
            harness.children = []

        def probe(*arguments):
            self.assertIsNotNone(process.returncode)
            self.assertEqual(arguments[1], str(state / "@guest"))
            self.assertEqual(arguments[arguments.index("--owned-root") + 1], str(self.root))
            selected = Path(arguments[arguments.index("--receipt") + 1])
            events.append(arguments[0])
            if arguments[0] == "--remember-guest":
                with selected.open("x") as data:
                    json.dump(identity, data)
                return {"remembered": True, "pid": identity["pid"], "started": identity["started"]}
            self.assertEqual(arguments[0], "--stop-owned-guest")
            self.assertEqual(json.loads(selected.read_text()), identity)
            return {"guest_stopped": True, "end": "Stopped", "pid": identity["pid"], "started": identity["started"]}

        harness.stop, harness.probe = stop, probe
        faults = fixture.load_module(Path(__file__).with_name("cold_fault_e2e.py"), "synthetic_faults")
        with patch.object(self.base, "backend", create), patch.object(self.base, "ready", lambda *_: events.append("ready-line")), \
                patch.object(self.base, "snapshot", snapshot), patch.object(self.base, "exercise", physical):
            with faults.inject_fault(self.cold, fault):
                if fault == "pre-ready":
                    with self.assertRaises(self.cold.census.Failed) as observed:
                        harness.exercise()
                    error = str(observed.exception)
                    self.assertEqual(error, "ROOT pre-ready parent receipt-boundary fault")
                    harness.report.update(status="failed", error=error)
                    self.assertIs(harness.cold_process, process)
                    harness.finish_backend()
                    self.assertEqual(harness.report["error"], error)
                    self.assertNotIn("physical-phase", events)
                else:
                    harness.exercise()
                    self.assertIn("physical-inspect-owned-key", events)
                    with self.assertRaises(self.cold.census.Failed) as observed:
                        harness.finish_backend()
                    error = str(observed.exception)
                    self.assertEqual(error, "ROOT post-ready finish snapshot fault")
                    self.assertEqual(harness.report["cold_boot"]["original_finish_failure"], error)
                    harness.report["backend_cleanup_error"] = error
                with self.assertRaises(self.cold.census.Failed):
                    harness.census_report()
        self.assertEqual(events.count("create-cold-node"), 1)
        self.assertEqual(events[-3:], ["reap-original-backend", "--remember-guest", "--stop-owned-guest"])
        self.assertIs(harness.cold_process, process)
        self.assertIsNone(harness.backend_process)
        self.assertEqual(harness.report["status"], "failed")
        self.assertTrue(harness.report["cold_boot"]["backend_reaped"])
        self.assertTrue(harness.report["cold_boot"]["stopped_receipt_verified"])
        self.assertEqual(harness.report["owned_guest_retired"]["pid"], identity["pid"])
        self.assertEqual(harness.guest_receipt, owner if fault == "post-ready" else self.root / "cold-recovery-receipt.json")
        if fault == "post-ready":
            self.assertEqual(json.loads(owner.read_text()), identity)

    def test_ac_t7_4_pre_ready_fault_keeps_original_error_and_exact_recovered_closure(self):
        self.exercise_fault("pre-ready")

    def test_ac_t7_4_post_ready_fault_forwards_inspect_and_preserves_original_finish_error(self):
        self.exercise_fault("post-ready")

    def test_ac_t7_4_recovery_cannot_guess_an_identity_from_another_run(self):
        harness = self.construct()
        process = SimpleNamespace(pid=31415, returncode=None)
        harness.backend_process = harness.cold_process = process
        harness.backend_state = self.root / "other-run.node-state"
        harness.runtime = {"max_frame": 1 << 20}
        harness.children = []
        stopped = []

        def stop(child, crash):
            self.assertIs(child, process)
            self.assertTrue(crash)
            child.returncode = -9
            stopped.append(child)

        harness.stop = stop
        harness.probe = lambda *_: self.fail("no guessed guest probe")
        with self.assertRaisesRegex(self.cold.census.Failed, "unexpected private backend state"):
            harness.finish_backend()
        self.assertEqual(stopped, [process])
        self.assertNotIn("owned_guest_retired", harness.report)
        self.assertIn("cleanup_unproven", harness.report["cold_boot"])

    def test_ac_t7_4_main_fault_scope_aborts_without_next_guest(self):
        arguments = ["synthetic-trace", "--cold-driver", str(self.root / "cold.py"), "--fault", "pre-ready",
                     "--validate-seeds", "--mode", "guest", "--n", "3"]
        original_instances = self.cold.ColdHarness.instances
        calls = []

        def original_main():
            instance = self.cold.census.Harness(self.args)
            calls.append(instance)
            instance.backend_process = SimpleNamespace()
            instance.backend_state = self.root / "cold-node.node-state"
            instance.ready(instance.backend_process, self.root / "cold-node.sock")
            self.fail("fault must abort before physical work or another construction")

        with patch.object(fixture, "load_cold", return_value=self.cold), patch.object(fixture.sys, "argv", arguments), \
                patch.object(self.base, "__init__", lambda harness, args: self.initialize(harness, args)), \
                patch.object(self.base, "ready", lambda *_: None), patch.object(self.cold.census, "main", original_main):
            with self.assertRaisesRegex(self.cold.census.Failed, "ROOT pre-ready parent receipt-boundary fault"):
                fixture.main()
            self.assertIs(fixture.sys.argv, arguments)
        self.assertEqual(len(calls), 1)
        self.assertIs(self.cold.census.Harness, self.base)
        self.assertIs(self.cold.ColdHarness.instances, original_instances)


if __name__ == "__main__":
    unittest.main()
