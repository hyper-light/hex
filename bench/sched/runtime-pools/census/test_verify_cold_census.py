"""AC-T7.4.1: synthetic files only; no CLI/VM/API/process/capture or seed measurements."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

import verify_cold_census as verifier

spec = importlib.util.spec_from_file_location("cold_fixture_classifier", Path(__file__).with_name("cold_boot_census.py"))
if spec is None or spec.loader is None:
    raise RuntimeError("cold fixture classifier is unavailable")
cold = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cold)
SOURCE = Path(__file__).resolve().parents[4]
SCRATCH = Path.home() / ".cache/hex-lead/cold-verifier-synthetic"


class Fixture:
    def __init__(self, parent, samples=None, count=1):
        self.parent = parent
        parent.mkdir()
        self.maximum, _ = verifier.shared.protocol(SOURCE)
        self.reports, self.ledgers, self.roots = [], [], []
        for number in range(1, count + 1):
            root = parent / f"fresh-{number}"
            root.mkdir()
            (root / "census").mkdir()
            (root / "cold-node.node-state").mkdir()
            self.roots.append(root)
            rows = samples or [f"{cold.HELLO} {number * 11}\n".encode(), f"{cold.HELLO} 3\n".encode()]
            selected, observer = root / "census/backend.tsv", root / "census/observer.tsv"
            selected.write_bytes(b"".join(rows))
            observer.write_bytes(f"{cold.HELLO} 999\n".encode())
            identity = {"pid": 500 + number, "started": 700 + number, "vsock": str(root / "guest-tmp/exact/v")}
            record = root / "cold-node.node-state/@guest"
            record.write_text(f"hex-guest 1\npid {identity['pid']}\nstarted {identity['started']}\nvsock {identity['vsock']}\n")
            config = {"binary": "/synthetic/shards", "kernel": "/synthetic/kernel", "initrd": "/synthetic/initrd",
                      "vcpus": 1, "memory_mib": 64, "shares": [{"dir": str(root / "guest-shares"), "read_only": True}]}
            (root / "owned-guest.json").write_text(json.dumps({"record": str(record), **identity, "owned_vsock_root": None, "declared_config": config}))
            owner = {"pid": 123, "parent": 456, "recorded_by": 456, "run": str(root)}
            producer = {**owner, "kind": "cold-node", "census": str(selected), "profile": "debug", "source_revision": "synthetic-revision", "hex_sha256": "a" * 64}
            # Deliberately reused PID: filename authority must exclude this observer's Hello999.
            ledger = [{**producer, "kind": "runtime-probe", "census": str(observer)}, producer,
                      {**owner, "kind": "reaped", "returncode": -15}]
            self.ledgers.append(ledger)
            runtime = {"root": str(root), "ledger": str(root / "pids.jsonl"), "max_frame": self.maximum,
                       "max_census_lines": len(rows), "wait_mode": "measuring", "profile": "debug",
                       "source_revision": "synthetic-revision", "hex_sha256": "a" * 64}
            (root / "runtime.json").write_text(json.dumps(runtime))
            stopped = {"guest_stopped": True, "end": "Stopped", "pid": identity["pid"], "started": identity["started"]}
            detail = {"backend_ready": True, "cohort": "FIRST-BOOT backend-process first SDK Hello", "seed_installation": "not performed",
                      "boxes_created": False, "workload_success": "not exercised", "full_boot_latency": "not measured",
                      "ready_class_verified": True, "ready_snapshot": {"classes": ["oci"], "boxes": [], "capacity": {"id": f"guest:{identity['pid']}:{identity['started']}"}},
                      "identity": identity, "receipt": str(root / "owned-guest.json"), "ledger": runtime["ledger"],
                      "producer": {key: producer[key] for key in ("kind", "pid", "census", "profile", "source_revision")},
                      "sdk_hello": cold.classify_hellos(rows, len(rows), self.maximum), "stopped_identity_basis": "ready",
                      "stopped_receipt_verified": True, "backend_reaped": True}
            report = {"kind": verifier.KIND, "status": "passed", "mode": "guest", "wait_mode": "measuring", "n": 1,
                      "profile": "debug", "max_rows_per_process": len(rows), "source_revision": "synthetic-revision",
                      "comparison_or_performance_claim": False, "cold_boot": detail, "owned_guest_retired": stopped,
                      "environment": {"guest_cpus": 1, "guest_memory_mib": 64, "guest_shares_sha256": hashlib.sha256((str(root / "guest-shares") + ":ro").encode()).hexdigest()},
                      "binary_and_artifact_hashes": {name: {"path": path, "sha256": "a" * 64} for name, path in
                                                      (("shards", config["binary"]), ("kernel", config["kernel"]), ("initrd", config["initrd"]), ("hex", "/synthetic/hex"))}}
            self.reports.append(report)
        self.save()

    def save(self):
        iterations = []
        for number, (root, report, ledger) in enumerate(zip(self.roots, self.reports, self.ledgers), 1):
            (root / "result.json").write_text(json.dumps(report))
            (root / "pids.jsonl").write_text("".join(json.dumps(row) + "\n" for row in ledger))
            iterations.append({"ordinal": number, "run": str(root), "result": str(root / "result.json"), "status": report["status"], "error": None,
                               "stopped_receipt_verified": True, "identity": report["cold_boot"]["identity"], "retirement": report["owned_guest_retired"], "cold_boot": report["cold_boot"]})
        self.campaign = {"kind": verifier.KIND, "n": len(iterations), "iterations": iterations, "status": "completed",
                         "seed_installation": "not performed", "workload_success": "not exercised", "full_boot_latency": "not measured",
                         "ambiguous_first_VM_cohorts": sum(row["cold_boot"]["sdk_hello"]["attribution"].startswith("ambiguous") for row in iterations)}
        self.save_campaign()

    def save_campaign(self):
        (self.parent / "campaign.json").write_text(json.dumps(self.campaign))

    def verify(self, **budgets):
        maximum_result = max(path.stat().st_size for path in [self.parent / "campaign.json", *(root / "result.json" for root in self.roots)])
        return verifier.verify(self.parent, self.parent / "campaign.json", SOURCE,
                               budgets.get("max_result_bytes", maximum_result), budgets.get("max_ledger_rows", max(map(len, self.ledgers))),
                               "synthetic-revision")


class ColdVerifierTests(unittest.TestCase):
    def setUp(self):
        SCRATCH.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="unit-", dir=SCRATCH)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def fixture(self, name="campaign", **kwargs):
        return Fixture(self.root / name, **kwargs)

    def test_cp_t7_4_reconstructs_normal_first_cohort_and_hashes_every_opened_authority(self):
        fixture = self.fixture(count=2)
        result = fixture.verify()
        self.assertEqual(result["status"], "consistent")
        self.assertEqual(result["cohorts"]["normal_iterations"], 2)
        self.assertEqual(result["profiles_reported"], ["debug"])
        self.assertEqual(result["distributions_ns"]["normal_first_completed_sdk_hello"], {"n": 2, "p50": 11, "p90": 22, "p99": 22, "p999": 22, "max": 22})
        self.assertEqual(result["distributions_ns"]["subsequent_completed_sdk_hello"]["n"], 2)
        self.assertFalse(result["per_row_VM_identity_proven"])
        self.assertTrue(result["unobserved_pre_hello_boot_retries_possible"])
        self.assertEqual(len(result["authority_inputs"]), 1 + 7 * 2)
        for name, record in result["authority_inputs"].items():
            data = (fixture.parent / name).read_bytes()
            self.assertEqual(record, {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})

    def test_cp_t7_4_ambiguous_retries_preserve_refusal_runout_and_subsequent_cohorts(self):
        rows = [f"{cold.HELLO} {word}\n".encode() for word in ("!7", ">8", "11", "3", "5")]
        result = self.fixture(samples=rows).verify()
        self.assertEqual(result["cohorts"]["ambiguous_iterations"], 1)
        self.assertEqual(result["cohorts"]["wire_refused_rows"], 1)
        self.assertEqual(result["cohorts"]["run_out_rows"], 1)
        self.assertEqual(result["distributions_ns"]["normal_first_completed_sdk_hello"]["n"], 0)
        self.assertEqual(result["distributions_ns"]["all_first_completed_sdk_hello"]["max"], 11)
        self.assertEqual(result["distributions_ns"]["subsequent_completed_sdk_hello"]["n"], 2)

    def test_cp_t7_4_raw_sdk_summary_mutation_cannot_replace_reconstruction(self):
        fixture = self.fixture()
        fixture.reports[0]["cold_boot"]["sdk_hello"]["first_sdk_hello"]["nanos"] = 999
        fixture.save()
        with self.assertRaisesRegex(verifier.Invalid, "independent raw"):
            fixture.verify()

    def test_cp_t7_4_positive_n_fresh_roots_ordinals_and_containment_are_required(self):
        fixture = self.fixture(count=2)
        original = copy.deepcopy(fixture.campaign)
        changes = (lambda c: c.update(n=0), lambda c: c.update(n=3),
                   lambda c: c["iterations"][1].update(run=c["iterations"][0]["run"]),
                   lambda c: c["iterations"][0].update(ordinal=2),
                   lambda c: c["iterations"][0].update(run=str(fixture.parent / "../outside")))
        for change in changes:
            fixture.campaign = copy.deepcopy(original)
            change(fixture.campaign)
            fixture.save_campaign()
            with self.assertRaises(verifier.Invalid):
                fixture.verify()

    def test_cp_t7_4_only_exact_filename_then_recording_owner_and_later_reap_prove_closure(self):
        fixture = self.fixture()
        original = copy.deepcopy(fixture.ledgers[0])
        for ledger in ([original[2], *original[:2]], [*original[:2], {**original[2], "parent": 999}],
                       [*original, copy.deepcopy(original[1])]):
            fixture.ledgers[0] = ledger
            fixture.save()
            with self.assertRaises(verifier.Invalid):
                fixture.verify()
        fixture.ledgers[0] = original
        fixture.reports[0]["cold_boot"]["producer"]["census"] = str(fixture.roots[0] / "census/observer.tsv")
        fixture.save()
        with self.assertRaisesRegex(verifier.Invalid, "filename authority"):
            fixture.verify()

    def test_cp_t7_4_ready_empty_baseline_exact_stopped_and_config_are_required(self):
        mutations = (lambda r: r["cold_boot"].update(boxes_created=True),
                     lambda r: r["cold_boot"]["ready_snapshot"].update(boxes=[{"id": "unexpected"}]),
                     lambda r: r["cold_boot"]["ready_snapshot"]["capacity"].update(id="guest:wrong"),
                     lambda r: r["owned_guest_retired"].update(started=999),
                     lambda r: r["owned_guest_retired"].update(end="Exited"),
                     lambda r: r["environment"].update(guest_memory_mib=65))
        for number, mutate in enumerate(mutations):
            fixture = self.fixture(name=f"case-{number}")
            mutate(fixture.reports[0])
            fixture.save()
            with self.assertRaises(verifier.Invalid):
                fixture.verify()

    def test_cp_t7_4_malformed_truncated_or_missing_first_rows_are_not_zero_success(self):
        for number, samples in enumerate(([b"broken\n"], [b"census full at 2\n"], [b"client.answer.other 1\n"])):
            with self.subTest(number=number):
                with self.assertRaisesRegex(verifier.Invalid, "missing/malformed/truncated"):
                    self.fixture(name=f"raw-{number}", samples=samples).verify()

    def test_cp_t7_4_duplicate_json_bounds_symlink_and_fifo_are_refused(self):
        fixture = self.fixture()
        with self.assertRaisesRegex(verifier.Invalid, "budget"):
            fixture.verify(max_result_bytes=1)
        with self.assertRaisesRegex(verifier.Invalid, "budget"):
            fixture.verify(max_ledger_rows=1)
        campaign = fixture.parent / "campaign.json"
        campaign.write_bytes(b'{"n":1,"n":1}')
        with self.assertRaisesRegex(verifier.Invalid, "invalid JSON input"):
            fixture.verify()
        fixture.save()
        raw = fixture.roots[0] / "census/backend.tsv"
        raw.unlink()
        raw.symlink_to(campaign)
        with self.assertRaises(OSError):
            fixture.verify()
        raw.unlink()
        os.mkfifo(raw)
        with self.assertRaisesRegex(verifier.Invalid, "regular file"):
            fixture.verify()

    def test_cp_t7_4_boolean_values_cannot_impersonate_integer_authority(self):
        fixture = self.fixture()
        fixture.campaign["iterations"][0]["ordinal"] = True
        fixture.save_campaign()
        with self.assertRaisesRegex(verifier.Invalid, "integer"):
            fixture.verify()
        fixture.save()
        path = fixture.roots[0] / "owned-guest.json"
        receipt = json.loads(path.read_bytes())
        receipt["declared_config"]["vcpus"] = True
        path.write_text(json.dumps(receipt))
        with self.assertRaisesRegex(verifier.Invalid, "integer"):
            fixture.verify()

        for number, field in enumerate(("pid", "started")):
            fixture = self.fixture(name=f"typed-retirement-{number}")
            fixture.reports[0]["owned_guest_retired"][field] = True
            fixture.save()
            with self.assertRaisesRegex(verifier.Invalid, "integer"):
                fixture.verify()
        fixture = self.fixture(name="typed-cohort-count")
        fixture.campaign["ambiguous_first_VM_cohorts"] = False
        fixture.save_campaign()
        with self.assertRaisesRegex(verifier.Invalid, "integer"):
            fixture.verify()

    def test_cp_t7_4_different_capture_profiles_cannot_be_pooled(self):
        fixture = self.fixture(count=2)
        fixture.reports[1]["profile"] = "release"
        fixture.reports[1]["cold_boot"]["producer"]["profile"] = "release"
        for row in fixture.ledgers[1]:
            if "profile" in row:
                row["profile"] = "release"
        path = fixture.roots[1] / "runtime.json"
        runtime = json.loads(path.read_bytes())
        runtime["profile"] = "release"
        path.write_text(json.dumps(runtime))
        fixture.save()
        with self.assertRaisesRegex(verifier.Invalid, "mixes profiles"):
            fixture.verify()


if __name__ == "__main__":
    unittest.main()
