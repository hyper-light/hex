"""Pure/light behavior checks only. Never invokes a runtime, GPU, VM, mutation or profiler."""

import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

from evidence import (BACKENDS, STAGES, TARGETS, InvalidEvidence, artifact,
                      check_events, libtest_result, load_json, matrix, sha256)
from mutation import replacement

compare_path=Path(__file__).resolve().parents[2]/"bench/scheduler-completion/compare.py"
spec=importlib.util.spec_from_file_location("completion_compare",compare_path)
comparison=importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


def full_events():
    selected=[{"kind":"gpu","key":256}]
    data=[
        {"runtime_class":"oci","native_process_verified":True,"native_executable_sha256":"a"*64},
        {"selected":selected,"holder_count":1,"durable_before_execution":True},
        {"uuid":"b"*32,"selected":selected,"native_join_verified":True,"words":3,
         "native_inventory":[{"uuid":"b"*32,"pci":[0,1,0,0]}],
         "mismatches":0,"prefill_mismatches":3,"hardware":True,"dispatch_completed":True},
        {"host_accessible":1,"guest_accessible":0},
        {"dispatches":0,"refused":True},
        {"selected":selected,"holder_count":1,"compute_verified":True,"same_physical_box":True},
        {"selected":selected,"holder_count":1,"compute_verified":True,"same_physical_box":True,
         "catalog_reordered":True,"load_order_changed":True},
        {"boxes":0,"holders":0,"owned_native_processes":0,"owned_artifacts":0},
        {"selected_stages":9},
    ]
    return [dict(row,stage=stage,backend="oci",topology="laptop",outcome="verified")
            for row,stage in zip(data,STAGES)]


class EvidenceBehavior(unittest.TestCase):
    def test_accepts_exact_real_case_shape(self):
        self.assertEqual(check_events(full_events(),"oci","laptop")["native_stages"],9)

    def test_rejects_zero_selected(self):
        with self.assertRaises(InvalidEvidence):
            libtest_result("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out;",1)

    def test_rejects_ignored_only(self):
        with self.assertRaises(InvalidEvidence):
            libtest_result("test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;",1)

    def test_rejects_missing_capability(self):
        value=full_events();value[0]["runtime_class"]="process"
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_missing_native_join(self):
        value=full_events();value[2]["native_join_verified"]=False
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_recovery_repick(self):
        value=full_events();value[6]["selected"]=[{"kind":"gpu","key":512}]
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_foreign_access(self):
        value=full_events();value[3]["guest_accessible"]=1
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_vacuous_wrong_id(self):
        value=full_events();value[4]["dispatches"]=1
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_unpoisoned_outputs(self):
        value=full_events();value[2]["prefill_mismatches"]=0
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_native_cleanup_gap(self):
        value=full_events();value[7]["owned_native_processes"]=1
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_preserves_selected_only_gaps(self):
        value=full_events();value=[value[i] for i in (0,1,2,7,8)]
        value[-1]["selected_stages"]=5
        result=check_events(value,"oci","laptop","selected")
        self.assertIn("foreign-device",result["remaining"])
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop","full")

    def test_rejects_duplicate_or_nonfinite_json(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/"receipt.json"
            for raw in ('{"exit":0,"exit":1}','{"value":NaN}'):
                path.write_text(raw)
                with self.assertRaises(InvalidEvidence):load_json(path,100)

    def test_checks_artifact_bytes_and_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/"log";path.write_bytes(b"checked")
            row={"path":"log","sha256":sha256(path),"bytes":7}
            self.assertEqual(artifact(root,row),path)
            path.write_bytes(b"changed")
            with self.assertRaises(InvalidEvidence):artifact(root,row)

    def test_rejects_symlink_or_path_escape(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/"log";path.write_bytes(b"checked")
            link=root/"link";link.symlink_to(path)
            for name in ("link","../log"):
                with self.assertRaises(InvalidEvidence):
                    artifact(root,{"path":name,"sha256":sha256(path),"bytes":7})

    def test_matrix_never_promotes_missing_hardware(self):
        value=matrix([])
        self.assertEqual(value["required_cells"],39)
        self.assertEqual(value["verified_cells"],0)
        self.assertFalse(value["complete"])

    def test_nonlinux_firecracker_requires_peer_route(self):
        row={"target":"aarch64-apple-darwin","backend":"firecracker","topology":"peer",
             "outcome":"verified","native_receipt_verified":True,"execution_target":TARGETS[0]}
        with self.assertRaises(InvalidEvidence):matrix([row])

    def test_guard_replacement_is_exact_and_restorable(self):
        before=b"prefix\nactual_guard();\nsuffix\n"
        after=replacement(before,b"actual_guard();",b"")
        self.assertEqual(after,b"prefix\n\nsuffix\n")
        self.assertEqual(before,b"prefix\nactual_guard();\nsuffix\n")
        with self.assertRaises(InvalidEvidence):replacement(before,b"missing",b"")
        with self.assertRaises(InvalidEvidence):replacement(before+before,b"actual_guard();",b"")

    def test_tail_distribution_preserves_observed_samples(self):
        result=comparison.distribution([1,2,3,100])
        self.assertEqual((result["n"],result["p50"],result["p99"],result["max"]),(4,2,100,100))
        self.assertIsNone(result["median_interval"]["lower"])

    def test_median_confidence_uses_binomial_bounds(self):
        result=comparison.distribution(list(range(20)))
        self.assertGreaterEqual(result["median_interval"]["coverage"],0.95)
        self.assertLessEqual(result["median_interval"]["lower"],result["p50"])
        self.assertGreaterEqual(result["median_interval"]["upper"],result["p50"])

    def test_profiles_reject_nan_and_negative_values(self):
        for values in ([float("nan")],[float("inf")],[-1],[True]):
            with self.assertRaises(ValueError):comparison.distribution(values)


if __name__=="__main__":
    unittest.main()
