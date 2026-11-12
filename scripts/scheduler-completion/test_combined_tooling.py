"""New intake/plan/receipt controls only; no backend, Cargo, native process or campaign."""

import ast
import copy
import contextlib
import hashlib
import io
import json
from pathlib import Path, PureWindowsPath
import tempfile
import unittest
from unittest.mock import patch, Mock
from types import SimpleNamespace

from build import source_state
from combined import capability_bundle, check_origin, execute, plan_cases, retain_origin, retain_run
from combined_oracle import forbidden_effect, negative_libtest, restoration, source_restoration, scope_records
from evidence import BACKENDS, POLICY, RULES, InvalidEvidence, check_run, sha256, artifact
from run import file_receipt
from mutation_campaign import output_path
from prepare import case_inputs
from test_combined_controls import COMMITMENT, CONTRACT, LINUX, MAC, SOURCE, resource_control


LIMIT = 65536


def save(root, name, value):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = value if isinstance(value, bytes) else (json.dumps(value) + "\n").encode()
    path.write_bytes(raw)
    return {"path": name, "bytes": len(raw), "sha256": sha256(path)}


def plan():
    return {"schema": "hex-completion-combined-plan-2", "source": SOURCE, "target": MAC,
            "topology": "peer", "cases": [{"backend": backend, "execution_target": LINUX,
                "proof": "full", "timeout_seconds": 7, "cleanup_timeout_seconds": 3,
                "required_guards": ["exact-physical-identity"], "origin_binary_sha256": "f" * 64}
                for backend in BACKENDS]}


def origin_control(root):
    binary = save(root, "hex", b"data-only origin executable shape")
    reply = {"physical": "owned", "key": 7, "at": 2, "tracked": True, "created": True, "refusals": []}
    row = {"schema": "hex-completion-origin-1", "scope": "root-peer-origin", "source": SOURCE,
           "target": MAC, "outcome": "verified", "exit": 0, "reaped": True, "pid": 8,
           "binary": binary, "physical": "owned", "reply": reply,
           "reply_artifact": save(root, "reply.json", reply), "runner_receipt_sha256": "a" * 64}
    case = {"execution_target": LINUX, "origin_binary_sha256": binary["sha256"]}
    workload = {"physical": "owned", "key": 7}
    return row, case, workload


class ToolingControls(unittest.TestCase):
    def test_requires_all_backend_cells_without_duplicates(self):
        value = plan()
        self.assertEqual(set(plan_cases(value)), set(BACKENDS))
        for changed in (value["cases"][:-1], [value["cases"][0]] * 3):
            with self.subTest(cases=changed), self.assertRaises(InvalidEvidence):
                plan_cases(dict(value, cases=changed))

    def test_explicit_execution_and_guard_census_required(self):
        for field, changed in (("execution_target", None), ("required_guards", []),
                               ("origin_binary_sha256", None), ("proof", "selected")):
            value = plan()
            value["cases"][0][field] = changed
            with self.subTest(field=field), self.assertRaises(InvalidEvidence):
                plan_cases(value)

    def test_operation_budgets_are_finite_positive_numbers(self):
        for changed in (0, -1, True, float("inf"), float("nan")):
            value = plan()
            value["cases"][0]["timeout_seconds"] = changed
            with self.subTest(value=changed), self.assertRaises(InvalidEvidence):
                plan_cases(value)

    def test_local_plan_cannot_hide_remote_execution(self):
        value = plan()
        value["topology"] = "laptop"
        with self.assertRaises(InvalidEvidence):
            plan_cases(value)

    def test_available_capability_requires_retained_route_matched_witness(self):
        from combined_oracle import BACKEND_CAPABILITIES, COMMON_CAPABILITIES
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            contract = save(root, "contract.json", b"source contract bytes")
            report = {"schema": "hex-completion-capability-2", "scope": "root-native-capability",
                "source": SOURCE, "backend": "firecracker", "target": MAC, "execution_target": LINUX,
                "topology": "peer", "contract_sha256": contract["sha256"], "capabilities": {}}
            for name in COMMON_CAPABILITIES + BACKEND_CAPABILITIES["firecracker"]:
                witness = {"schema": "hex-completion-capability-witness-2", "scope": "root-native",
                    "source": SOURCE, "backend": "firecracker", "target": MAC, "execution_target": LINUX,
                    "topology": "peer", "capability": name, "contract_sha256": contract["sha256"],
                    "observed": True, "observation": {"fixture": "data-only shape"}}
                report["capabilities"][name] = {"state": "available", "observed": True,
                    "witness": save(root, name + ".json", witness)}
            row = {"contract": contract, "capability": save(root, "capability.json", report)}
            self.assertEqual(capability_bundle(root, row, SOURCE, "firecracker", MAC, LIMIT)[0]["outcome"], "verified")
            witness["execution_target"] = MAC
            report["capabilities"][name]["witness"] = save(root, name + ".json", witness)
            row["capability"] = save(root, "capability.json", report)
            with self.assertRaises(InvalidEvidence):
                capability_bundle(root, row, SOURCE, "firecracker", MAC, LIMIT)

    def test_peer_origin_joins_exact_source_box_and_runner(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            origin, case, workload = origin_control(root)
            row = save(root, "origin.json", origin)
            checked = check_origin(root, row, plan(), case, {"sha256": "a" * 64}, workload, LIMIT)
            self.assertTrue(checked["routed_to_capable_peer"])
            for field, changed in (("source", "b" * 40), ("physical", "foreign"),
                                   ("runner_receipt_sha256", "b" * 64), ("exit", False)):
                candidate = dict(origin, **{field: changed})
                with self.subTest(field=field), self.assertRaises(InvalidEvidence):
                    check_origin(root, save(root, "origin.json", candidate), plan(), case,
                                 {"sha256": "a" * 64}, workload, LIMIT)

    def test_origin_intake_copies_only_verified_declared_closure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, destination = root / "source", root / "intake"
            source.mkdir(); destination.mkdir()
            origin, _, _ = origin_control(source)
            row = save(source, "origin.json", origin)
            save(source, "unrelated.txt", b"unrelated owned data")
            retain_origin(source, row, destination, LIMIT)
            self.assertEqual({path.name for path in destination.iterdir()}, {"origin.json", "hex", "reply.json"})
            (source / "hex").write_bytes(b"replaced executable")
            with self.assertRaises(InvalidEvidence):
                retain_origin(source, row, destination, LIMIT)

    def test_guard_restoration_checks_full_source_not_only_guard(self):
        before = {"guard.rs": "a" * 64, "other.rs": "b" * 64}
        changed = dict(before, **{"guard.rs": "c" * 64})
        self.assertTrue(source_restoration(before, changed, dict(before), "guard.rs", "a" * 64, "c" * 64))
        with self.assertRaises(InvalidEvidence):
            source_restoration(before, changed, dict(before, **{"other.rs": "d" * 64}), "guard.rs", "a" * 64, "c" * 64)
        with self.assertRaises(InvalidEvidence):
            source_restoration(before, dict(changed, **{"other.rs": "d" * 64}), before, "guard.rs", "a" * 64, "c" * 64)

    def test_forbidden_dispatch_is_completed_and_distinct_from_selected(self):
        value = {"schema": "hex-completion-forbidden-effect-2", "scope": "root-native", "source": SOURCE,
            "backend": "oci", "guard": "identity", "binary_sha256": CONTRACT, "effect": "foreign_dispatch",
            "observed": True, "expected_uuid": "a" * 32, "uuid": "b" * 32,
            "dispatches": 1, "words": 4, "mismatches": 0, "dispatch_completed": True}
        self.assertEqual(forbidden_effect(value, SOURCE, "oci", "identity", CONTRACT), "foreign_dispatch")
        for field, changed in (("uuid", value["expected_uuid"]), ("mismatches", False),
                               ("dispatches", True), ("words", False), ("dispatch_completed", False)):
            with self.subTest(field=field), self.assertRaises(InvalidEvidence):
                forbidden_effect(dict(value, **{field: changed}), SOURCE, "oci", "identity", CONTRACT)

    def test_compile_or_skip_red_does_not_kill_a_guard(self):
        red = "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out;\n"
        negative_libtest(red)
        for changed in (red + red, "UNVERIFIED\n" + red, "error[E0001]: fixture\n" + red,
                        red.replace("1 failed", "0 failed")):
            with self.subTest(value=changed), self.assertRaises(InvalidEvidence):
                negative_libtest(changed)

    def test_resource_observations_preserve_foreign_charges_and_ambient_uncertainty(self):
        before, after = resource_control()
        after["free"]["mem_bytes"] -= 1
        self.assertEqual(restoration(before, after, SOURCE, ["owned"])["outcome"], "unverified")
        before, after = resource_control()
        after["boxes"][0]["allocation"]["resources"]["cpu_milli"] += 1
        with self.assertRaises(InvalidEvidence):
            restoration(before, after, SOURCE, ["owned"])
        before, after = resource_control()
        after["terminal_scopes"][0]["scope"] = "process"
        with self.assertRaises(InvalidEvidence):
            restoration(before, after, SOURCE, ["owned"])

    def test_campaign_output_cannot_escape_or_follow_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.assertEqual(output_path(root, "owned/output.json"), root / "owned/output.json")
            (root / "link").symlink_to(root, target_is_directory=True)
            for name in ("../foreign.json", "/foreign.json", "link/output.json"):
                with self.subTest(name=name), self.assertRaises(InvalidEvidence):
                    output_path(root, name)

    def test_build_overlay_refuses_any_second_dirty_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original, changed = b"guard enabled", b"guard removed"
            (root / "guard.rs").write_bytes(changed)
            overlay = {"file": "guard.rs", "original_sha256": hashlib.sha256(original).hexdigest(),
                       "sha256": hashlib.sha256(changed).hexdigest()}
            def git_output(argv):
                if "status" in argv: return b" M guard.rs\0"
                if "show" in argv: return original
                if "ls-files" in argv: return b"guard.rs\0"
                raise ValueError("unexpected Git observation")
            with patch("build.git", return_value=SOURCE), patch("build.subprocess.check_output", side_effect=git_output):
                self.assertEqual(source_state(root, SOURCE, overlay), {"guard.rs": overlay["sha256"]})
            with patch("build.git", return_value=SOURCE), patch("build.subprocess.check_output", return_value=b" M guard.rs\0 M other.rs\0"):
                with self.assertRaises(InvalidEvidence):
                    source_state(root, SOURCE, overlay)

    def test_new_tooling_syntax_parses_without_executing_entrypoints(self):
        root = Path(__file__).parent
        for name in ("combined.py", "combined_oracle.py", "build.py", "run.py", "mutation.py", "mutation_campaign.py"):
            with self.subTest(file=name):
                ast.parse((root / name).read_bytes(), filename=name)


class CaseInputControls(unittest.TestCase):
    def inputs(self, root):
        contract = save(root, "contract", b"reviewed fixed factory source contract")
        contract["path"] = str(root / contract["path"])
        return {"backend": "firecracker", "topology": "peer", "proof": "full",
            "origin_target": MAC, "execution_target": LINUX, "require_remote_origin": True,
            "wait_ms": 7, "byte_limit": LIMIT, "max_items": 2, "max_shader_bytes": 16,
            "words": 4, "rounds": 1, "gpu_wait_ns": 3, "challenge": 0,
            "creating_entry_limit": 8, "creating_event_limit": 16, "creating_notification_bytes": LIMIT,
            "observer_timeout_seconds": 2, "factory_contract": contract}

    def test_peer_config_keeps_origin_execution_and_contract_distinct(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            self.assertEqual(case_inputs(self.inputs(root), SOURCE), root / "contract")

    def test_local_config_cannot_relabel_peer_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            value = self.inputs(Path(temporary))
            value["topology"] = "laptop"
            with self.assertRaises(InvalidEvidence): case_inputs(value, SOURCE)

    def test_peer_config_requires_actual_origin_barrier(self):
        with tempfile.TemporaryDirectory() as temporary:
            value = self.inputs(Path(temporary))
            value["require_remote_origin"] = False
            with self.assertRaises(InvalidEvidence): case_inputs(value, SOURCE)

    def test_missing_contract_has_no_fallback(self):
        with tempfile.TemporaryDirectory() as temporary:
            value = self.inputs(Path(temporary))
            del value["factory_contract"]
            with self.assertRaises(InvalidEvidence): case_inputs(value, SOURCE)

    def test_contract_bytes_must_match_reviewed_pin(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            value = self.inputs(root)
            (root / "contract").write_bytes(b"replacement factory")
            with self.assertRaises(InvalidEvidence): case_inputs(value, SOURCE)

    def test_count_inputs_are_explicit_positive_nonboolean_integers(self):
        with tempfile.TemporaryDirectory() as temporary:
            original = self.inputs(Path(temporary))
            for name in ("wait_ms", "byte_limit", "max_items", "max_shader_bytes", "words", "rounds", "gpu_wait_ns"):
                for changed in (None, True, 0, -1, 1.5):
                    with self.subTest(field=name, value=changed), self.assertRaises(InvalidEvidence):
                        case_inputs(dict(original, **{name: changed}), SOURCE)

    def test_observer_budget_has_no_zero_infinite_or_boolean_default(self):
        with tempfile.TemporaryDirectory() as temporary:
            original = self.inputs(Path(temporary))
            for changed in (None, 0, True, float("inf"), float("nan")):
                with self.subTest(value=changed), self.assertRaises(InvalidEvidence):
                    case_inputs(dict(original, observer_timeout_seconds=changed), SOURCE)

    def test_challenge_is_exact_integer_even_when_zero_is_allowed(self):
        with tempfile.TemporaryDirectory() as temporary:
            value = self.inputs(Path(temporary))
            value["challenge"] = False
            with self.assertRaises(InvalidEvidence): case_inputs(value, SOURCE)


class NativeIntakeControls(unittest.TestCase):
    def test_collect_only_never_dispatches_even_when_capability_rows_are_available(self):
        from combined_oracle import BACKEND_CAPABILITIES, COMMON_CAPABILITIES
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            inputs = root / "inputs"
            inputs.mkdir()
            value = plan()
            for case in value["cases"]:
                backend = case["backend"]
                contract = save(inputs, backend + "/contract", b"retained data-only source contract")
                report = {"schema": "hex-completion-capability-2", "scope": "root-native-capability",
                    "source": SOURCE, "backend": backend, "target": MAC, "execution_target": LINUX,
                    "topology": "peer", "contract_sha256": contract["sha256"], "capabilities": {}}
                for name in COMMON_CAPABILITIES + BACKEND_CAPABILITIES[backend]:
                    witness = {"schema": "hex-completion-capability-witness-2", "scope": "root-native",
                        "source": SOURCE, "backend": backend, "target": MAC, "execution_target": LINUX,
                        "topology": "peer", "capability": name, "contract_sha256": contract["sha256"],
                        "observed": True, "observation": {"kind": "data-only control"}}
                    report["capabilities"][name] = {"state": "available", "observed": True,
                        "witness": save(inputs, backend + "/" + name + ".json", witness)}
                case.update(contract=contract, capability=save(inputs, backend + "/capability.json", report))
            plan_row = save(root, "plan.json", value)
            args = SimpleNamespace(collect_only=True, root_authorized=False, byte_limit=LIMIT,
                plan=root / plan_row["path"], inputs=inputs, output=root / "collected")
            with patch("combined.single.run", side_effect=AssertionError("collector must not dispatch")) as native, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(execute(args), 1)
                native.assert_not_called()
            result = json.loads((args.output / "combined.json").read_text())
            self.assertEqual(result["assessment"]["verified_backends"], 0)
            self.assertEqual(result["assessment"]["outcome"], "unverified")
            self.assertEqual([row["backend"] for row in result["cases"]], list(BACKENDS))

    def test_precollected_run_retains_binary_config_log_and_resource_closure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            inputs, output = root / "inputs", root / "retained"
            inputs.mkdir(); output.mkdir()
            fields = {name: save(inputs, name + ".data", b"data-only retained artifact") for name in (
                "binary", "fixture", "build", "log", "events", "resources_before", "resources_after")}
            row = save(inputs, "run.json", {"schema": "data-only-intake-control", **fields})
            save(inputs, "unrelated", b"not an input")
            retained = retain_run(inputs, row, output, LIMIT)
            self.assertEqual(sha256(retained), row["sha256"])
            self.assertEqual({path.name for path in output.iterdir()}, {"run.json", *(name + ".data" for name in fields)})


class CreatingEvidenceControls(unittest.TestCase):
    def records(self, root):
        before, after = resource_control()
        for item in after["scope_history"][0]["observations"]:
            value = {"version": 1, "spec": {"id": "owned", "original_workload": "fixed"},
                     "native_scope": [204] * 16,
                     "phase": "creating" if item["stage"] == "creating" else "ready",
                     "init": None if item["stage"] == "creating" else {"pid": 7, "start_ticks": 11}}
            item["record"] = save(root, item["stage"] + ".json", value)
        return before, after

    def test_independent_actual_creating_bytes_and_ready_records_join(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, after = self.records(root)
            self.assertEqual(scope_records(root, after["scope_history"], ["owned"], LIMIT)["outcome"], "verified")

    def test_ready_bytes_cannot_be_relabelled_as_creating(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, after = self.records(root)
            row = after["scope_history"][0]["observations"][1]
            value = json.loads((root / row["record"]["path"]).read_text())
            value["phase"] = "ready"
            row["record"] = save(root, "creating.json", value)
            with self.assertRaises(InvalidEvidence): scope_records(root, after["scope_history"], ["owned"], LIMIT)

    def test_creating_after_native_publication_is_not_the_prepublication_witness(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, after = self.records(root)
            row = after["scope_history"][0]["observations"][1]
            value = json.loads((root / row["record"]["path"]).read_text())
            value["init"] = {"pid": 7, "start_ticks": 11}
            row["record"] = save(root, "creating.json", value)
            with self.assertRaises(InvalidEvidence): scope_records(root, after["scope_history"], ["owned"], LIMIT)

    def test_explicit_window_miss_remains_unverified_with_no_copied_ready_pair(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            before, after = self.records(root)
            observations = after["scope_history"][0]["observations"]
            observations[1] = {"stage": "creating", "observed": False, "source_phase": None,
                               "reason": "actual initNone window missed"}
            self.assertEqual(scope_records(root, after["scope_history"], ["owned"], LIMIT)["outcome"], "unverified")
            self.assertEqual(restoration(before, after, SOURCE, ["owned"])["outcome"], "unverified")
            observations[1]["native_scope"] = observations[0]["native_scope"]
            with self.assertRaises(InvalidEvidence): scope_records(root, after["scope_history"], ["owned"], LIMIT)

    def test_raw_restart_spec_cannot_change_behind_the_same_commitment_label(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, after = self.records(root)
            row = after["scope_history"][0]["observations"][3]
            value = json.loads((root / row["record"]["path"]).read_text())
            value["spec"]["original_workload"] = "transplanted"
            row["record"] = save(root, "runtime-recovery.json", value)
            with self.assertRaises(InvalidEvidence): scope_records(root, after["scope_history"], ["owned"], LIMIT)

    def test_scope_record_hash_must_match_actual_retained_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _, after = self.records(root)
            (root / "creating.json").write_bytes(b"replacement record")
            with self.assertRaises(InvalidEvidence): scope_records(root, after["scope_history"], ["owned"], LIMIT)

    def test_notification_source_parses_without_running_native_watchers(self):
        root = Path(__file__).parent
        for name in ("creating_events.py", "prepare.py", "combined_oracle.py", "combined.py", "run.py"):
            with self.subTest(file=name): ast.parse((root / name).read_bytes(), filename=name)


class ArtifactExportControls(unittest.TestCase):
    def test_windows_relative_export_uses_portable_posix_separators(self):
        path = Mock()
        path.relative_to.return_value = PureWindowsPath("scope\\test-binary.exe")
        path.stat.return_value.st_size = 4
        with patch("run.sha256", return_value="a" * 64):
            self.assertEqual(file_receipt(Path("unused"), path)["path"], "scope/test-binary.exe")

    def test_drive_relative_or_backslash_receipts_are_never_portable_artifacts(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in ("C:artifact", "scope\\record.json", "..\\escape"):
                row = {"path": name, "sha256": "a" * 64, "bytes": 0}
                with self.subTest(path=name), self.assertRaises(InvalidEvidence): artifact(root, row)


if __name__ == "__main__":
    unittest.main()
