#!/usr/bin/env python3
"""Synthetic bounded verifier tests only; no real capture/VM/API/measurement/seed input is opened."""
from collections import Counter
import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

import scheduler_census as census
import verify_census as verifier

SOURCE = Path(__file__).resolve().parents[4]
SCRATCH = Path.home() / ".cache/hex-lead/verifier-synthetic-tests"


class Fixture:
    def __init__(self, run, legacy=False, guest=False):
        self.run, self.legacy = run, legacy
        self.maximum, self.kinds = verifier.protocol(SOURCE)
        self.ledger = []
        (run / "census").mkdir()
        for index, kind in enumerate(self.kinds):
            self.add(kind + ".tsv", "call" if not kind.startswith("hear_") else "forced-tunnel",
                     f"client.answer.schedule.{kind} 1\n", index + 1,
                     "call" if not kind.startswith("hear_") else None, True)
        # PID reuse is deliberate; a failed producer's answered row must remain outside success attribution.
        self.add("failed-probe.tsv", "runtime-probe", "client.answer.capacity 7\nclient.answer.capacity " + ("20" if legacy else "!20") + "\n", 123, "process", False)
        self.add("good-probe.tsv", "runtime-probe", "client.answer.capacity 10\nclient.answer.capacity >30\n", 123, "process", True)
        for phase in ("local", "remote"):
            self.ledger.append({"kind": "verified-physical-lifecycle", "phase": phase,
                                "key": {"owner": 0, "epoch": 1, "seq": 0}, "physical": "synthetic-" + phase})
        (run / "pids.jsonl").write_text("".join(json.dumps(row) + "\n" for row in self.ledger))
        (run / "runtime.json").write_text(json.dumps({"root": str(run), "ledger": str(run / "pids.jsonl"),
                                                     "max_frame": self.maximum, "max_census_lines": 3}))
        harness = object.__new__(census.Harness)
        harness.root = run
        harness.runtime = {"root": str(run), "ledger": str(run / "pids.jsonl"), "max_frame": self.maximum}
        harness.args = SimpleNamespace(n=1, max_census_lines=3)
        harness.wait_kinds = self.kinds
        harness.report = {"kind": "actual-scheduler-wait-census", "status": "passed", "profile": "debug", "mode": "guest" if guest else "native",
                          "n": 1, "max_rows_per_process": 3, "verified_physical_created": 2,
                          "verified_physical_deleted": 2, "backend_baseline_restored": True,
                          "source_revision": "synthetic; not an actual capture"}
        harness.census_report()
        self.report = harness.report
        if legacy:
            self.report["census"].pop("refused_rows_by_name")
            for item in self.report["census"]["per_process"]:
                item.pop("refused")
        if guest:
            (run / "real.node-state").mkdir()
            record = run / "real.node-state/@guest"
            vsock = str(run / "guest-tmp/owned/v")
            record.write_text(f"hex-guest 1\npid 7\nstarted 11\nvsock {vsock}\n")
            (run / "owned-guest.json").write_text(json.dumps({"record": str(record), "pid": 7, "started": 11, "vsock": vsock,
                                                           "declared_config": {"vcpus": 1, "memory_mib": 64}}))
            self.report["owned_guest_retired"] = {"guest_stopped": True, "pid": 7, "started": 11, "end": "Stopped"}
        self.save()

    def add(self, name, kind, text, pid, exit_kind, success):
        path = self.run / "census" / name
        path.write_text(text)
        self.ledger.append({"kind": kind, "pid": pid, "census": str(path)})
        if exit_kind:
            self.ledger.append({"kind": "census-" + exit_kind + "-exit", "pid": pid, "census": str(path),
                                "returncode": 0 if success else 1, "success": success})

    def save(self):
        (self.run / "result.json").write_text(json.dumps(self.report))

    def verify(self, max_result_bytes=None, max_ledger_rows=None):
        return verifier.verify(self.run, self.run / "result.json", SOURCE,
                               (self.run / "result.json").stat().st_size if max_result_bytes is None else max_result_bytes,
                               len(self.ledger) if max_ledger_rows is None else max_ledger_rows)


class VerifyTests(unittest.TestCase):
    def setUp(self):
        SCRATCH.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="synthetic-", dir=SCRATCH)
        self.addCleanup(self.temporary.cleanup)
        self.run = Path(self.temporary.name)

    def test_reconstructs_hashes_counts_outcomes_and_nearest_ranks(self):
        fixture = Fixture(self.run)
        result = fixture.verify()
        self.assertEqual(result["status"], "consistent")
        distributions = result["distributions_ns"]
        self.assertEqual(distributions["raw_unmarked"]["client.answer.capacity"], {"n": 2, "p50": 7, "p90": 10, "p99": 10, "p999": 10, "max": 10})
        self.assertEqual(distributions["wire_refused"]["client.answer.capacity"]["n"], 1)
        self.assertEqual(distributions["run_out"]["client.answer.capacity"]["max"], 30)
        self.assertEqual(distributions["filename_process_exit_success"]["client.answer.capacity"]["n"], 1)
        self.assertEqual(distributions["filename_process_exit_success"]["client.answer.capacity"]["max"], 10)
        self.assertNotIn("client.answer.capacity", distributions["filename_call_exit_success"])
        self.assertFalse(result["legacy_wire_outcomes_unclassified"])
        self.assertEqual(verifier.statistics(Counter({1: 1, 2: 1, 3: 1, 4: 1}))["p50"], 2)
        for row in result["per_process"]:
            self.assertEqual(row["sha256"], hashlib.sha256((self.run / "census" / row["file"]).read_bytes()).hexdigest())

    def test_legacy_unmarked_rows_remain_unclassified(self):
        result = Fixture(self.run, legacy=True).verify()
        self.assertTrue(result["legacy_wire_outcomes_unclassified"])
        self.assertEqual(result["distributions_ns"]["non_refusal_wire_answers"], {})
        self.assertEqual(result["distributions_ns"]["legacy_unmarked_unclassified"]["client.answer.capacity"]["n"], 3)

    def test_raw_hash_and_summary_mutations_are_rejected(self):
        fixture = Fixture(self.run)
        raw = self.run / "census/good-probe.tsv"
        original = raw.read_bytes()
        raw.write_bytes(original.replace(b"10", b"11"))
        with self.assertRaisesRegex(verifier.Invalid, "SHA256"):
            fixture.verify()
        raw.write_bytes(original)
        fixture.report["census"]["answer_rows_by_name"]["client.answer.capacity"] += 1
        fixture.save()
        with self.assertRaisesRegex(verifier.Invalid, "global"):
            fixture.verify()

    def test_bounds_and_no_pid_fallback(self):
        fixture = Fixture(self.run)
        with self.assertRaisesRegex(verifier.Invalid, "budget"):
            fixture.verify(max_result_bytes=1)
        with self.assertRaisesRegex(verifier.Invalid, "row/line"):
            fixture.verify(max_ledger_rows=1)
        by_name = {row["file"]: row for row in fixture.report["census"]["per_process"]}
        by_name["failed-probe.tsv"]["process_outcome"] = copy.deepcopy(by_name["good-probe.tsv"]["process_outcome"])
        fixture.save()
        with self.assertRaisesRegex(verifier.Invalid, "attribution"):
            fixture.verify()

    def test_path_escape_unreported_raw_and_symlinks_are_rejected(self):
        fixture = Fixture(self.run)
        original = fixture.report["census"]["per_process"][0]["file"]
        fixture.report["census"]["per_process"][0]["file"] = "../outside.tsv"
        fixture.save()
        with self.assertRaisesRegex(verifier.Invalid, "filename"):
            fixture.verify()
        fixture.report["census"]["per_process"][0]["file"] = original
        fixture.save()
        extra = self.run / "census/extra.tsv"
        extra.write_text("synthetic 1\n")
        with self.assertRaisesRegex(verifier.Invalid, "unreported"):
            fixture.verify()
        extra.unlink()
        raw = self.run / "census/describe.tsv"
        raw.unlink()
        raw.symlink_to(self.run / "result.json")
        with self.assertRaisesRegex(verifier.Invalid, "unsafe"):
            fixture.verify()

    def test_pass_physical_flags_and_targets_are_required(self):
        fixture = Fixture(self.run)
        original = copy.deepcopy(fixture.report)
        for mutate in (lambda r: r.update(status="failed"), lambda r: r.update(verified_physical_deleted=1),
                       lambda r: r.update(backend_baseline_restored=False), lambda r: r["census"].update(missing_row_targets={"delete": 0})):
            fixture.report = copy.deepcopy(original)
            mutate(fixture.report)
            fixture.save()
            with self.assertRaises(verifier.Invalid):
                fixture.verify()

    def test_guest_identity_must_match_exact_ownership_receipt(self):
        fixture = Fixture(self.run, guest=True)
        self.assertEqual(fixture.verify()["owned_guest_retirement"]["recorded_terminal"], "Stopped")
        fixture.report["owned_guest_retired"]["started"] = 12
        fixture.save()
        with self.assertRaisesRegex(verifier.Invalid, "identity"):
            fixture.verify()

    def test_output_is_create_new_and_preserves_existing_bytes(self):
        output = self.run / "verification.json"
        verifier.create_output(output, {"synthetic": True})
        original = output.read_bytes()
        with self.assertRaises(FileExistsError):
            verifier.create_output(output, {"overwrite": "forbidden"})
        self.assertEqual(output.read_bytes(), original)

    @unittest.skipUnless(hasattr(os, "mkfifo"), "owned FIFO control requires Unix")
    def test_nonregular_input_is_rejected_without_waiting_for_a_writer(self):
        fifo = self.run / "no-writer.json"
        os.mkfifo(fifo, 0o600)
        with self.assertRaisesRegex(verifier.Invalid, "regular"):
            verifier.read_bytes(self.run, fifo, 1)


if __name__ == "__main__":
    unittest.main()
