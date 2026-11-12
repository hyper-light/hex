"""Bounded data-oracle controls; no runtime, VM, GPU or mutation is invoked."""

import copy
import unittest

from combined_oracle import (BACKEND_CAPABILITIES, COMMON_CAPABILITIES, capabilities,
                             restoration)
from evidence import InvalidEvidence, check_events
from test_evidence import full_events


SOURCE = "a" * 40
CONTRACT = "b" * 64
LINUX = "x86_64-unknown-linux-gnu"
MAC = "aarch64-apple-darwin"
SCOPE = "c" * 32
COMMITMENT = "d" * 64


def resource_control():
    before = {"schema": "hex-completion-resources-2", "scope": "root-native-resources",
              "source": SOURCE, "observed": True, "authority": "native-node-1", "pool": "pool-1",
              "total": {"cpu_milli": 1000, "mem_bytes": 4000, "disk_bytes": 12000},
              "free": {"cpu_milli": 800, "mem_bytes": 3000, "disk_bytes": 10000},
              "boxes": [{"id": "foreign", "allocation": {"resources": {
                  "cpu_milli": 200, "mem_bytes": 1000, "disk_bytes": 2000}}}],
              "devices": [{"id": {"kind": "gpu", "key": 256}, "holder": None}],
              "driver_bindings": [{"id": {"kind": "gpu", "key": 256},
                                   "observed": True, "binding": "on-host-driver"}]}
    after = copy.deepcopy(before)
    after.update(owned_native_processes=[], owned_artifacts=[], owned_boxes=["owned"],
                 terminal_scopes=[{"id": "owned", "scope": "released", "observed": True,
                                   "record_present": False, "native_scope": SCOPE,
                                   "native_commitment": COMMITMENT}],
                 scope_history=[{"id": "owned", "observations": [
                     {"stage": stage, "observed": True, "source_phase": "Creating" if stage == "creating" else "Ready",
                      **({"init_published": False} if stage == "creating" else {}), "native_scope": SCOPE,
                      "native_commitment": COMMITMENT}
                     for stage in ("binding", "creating", "scheduler-recovery", "runtime-recovery")]}])
    return before, after


def capability_control(origin=MAC, execution=LINUX, topology="peer"):
    return {"schema": "hex-completion-capability-2", "scope": "root-native-capability",
            "source": SOURCE, "backend": "firecracker", "target": origin,
            "execution_target": execution, "topology": topology,
            "contract_sha256": CONTRACT, "capabilities": {
                name: {"state": "available", "observed": True, "witness": {}}
                for name in COMMON_CAPABILITIES + BACKEND_CAPABILITIES["firecracker"]}}


class OracleControls(unittest.TestCase):
    def test_scope_control_is_positive_and_nonvacuous(self):
        before, after = resource_control()
        self.assertEqual(restoration(before, after, SOURCE, ["owned"])["outcome"], "verified")

    def test_reused_box_id_cannot_transplant_terminal_scope(self):
        before, after = resource_control()
        after["terminal_scopes"][0]["native_scope"] = "e" * 32
        with self.assertRaises(InvalidEvidence):
            restoration(before, after, SOURCE, ["owned"])

    def test_same_scope_cannot_transplant_another_original_spec(self):
        before, after = resource_control()
        after["terminal_scopes"][0]["native_commitment"] = "e" * 64
        with self.assertRaises(InvalidEvidence):
            restoration(before, after, SOURCE, ["owned"])

    def test_both_restart_receipts_must_keep_original_scope(self):
        for index in (2, 3):
            with self.subTest(stage=index):
                before, after = resource_control()
                after["scope_history"][0]["observations"][index]["native_scope"] = "e" * 32
                with self.assertRaises(InvalidEvidence):
                    restoration(before, after, SOURCE, ["owned"])

    def test_scope_history_cannot_omit_creating(self):
        before, after = resource_control()
        del after["scope_history"][0]["observations"][1]
        with self.assertRaises(InvalidEvidence):
            restoration(before, after, SOURCE, ["owned"])

    def test_mac_origin_can_route_firecracker_to_linux_peer(self):
        value = capability_control()
        self.assertEqual(capabilities(value, SOURCE, "firecracker", MAC, CONTRACT)["outcome"], "verified")

    def test_linux_origin_does_not_make_nonlinux_execution_capable(self):
        value = capability_control(origin=LINUX, execution=MAC)
        self.assertEqual(capabilities(value, SOURCE, "firecracker", LINUX, CONTRACT)["outcome"], "unverified")

    def test_absent_linux_peer_vfio_stays_unverified(self):
        value = capability_control()
        value["capabilities"]["vfio"] = {"state": "unavailable", "reason": "two GPUs on host drivers"}
        self.assertEqual(capabilities(value, SOURCE, "firecracker", MAC, CONTRACT)["outcome"], "unverified")

    def test_boolean_mismatch_is_not_a_zero_count(self):
        events = full_events()
        events[2]["mismatches"] = False
        with self.assertRaises(InvalidEvidence):
            check_events(events, "oci", "laptop")

    def test_boolean_prefill_is_not_one_word(self):
        events = full_events()
        events[2].update(words=1, prefill_mismatches=True)
        with self.assertRaises(InvalidEvidence):
            check_events(events, "oci", "laptop")

    def test_boolean_foreign_access_is_not_a_zero_count(self):
        events = full_events()
        events[3]["guest_accessible"] = False
        with self.assertRaises(InvalidEvidence):
            check_events(events, "oci", "laptop")

    def test_boolean_wrong_dispatch_is_not_a_zero_count(self):
        events = full_events()
        events[4]["dispatches"] = False
        with self.assertRaises(InvalidEvidence):
            check_events(events, "oci", "laptop")

    def test_boolean_restart_holder_is_not_one_holder(self):
        for index in (5, 6):
            with self.subTest(stage=index):
                events = full_events()
                events[index]["holder_count"] = True
                with self.assertRaises(InvalidEvidence):
                    check_events(events, "oci", "laptop")

    def test_boolean_cleanup_is_not_a_zero_count(self):
        for name in ("boxes", "holders", "owned_native_processes", "owned_artifacts"):
            with self.subTest(field=name):
                events = full_events()
                events[7][name] = False
                with self.assertRaises(InvalidEvidence):
                    check_events(events, "oci", "laptop")


if __name__ == "__main__":
    unittest.main()
