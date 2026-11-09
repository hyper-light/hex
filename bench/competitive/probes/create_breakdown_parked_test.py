#!/usr/bin/env python3
"""The parked claim's cuts (goal 16; the lead, 2026-11-09): a create given a cgroup with its init parked there
(decision 8: a made-ahead slot's, cloned ahead by the refill), cut whole. The seed path's cutter refused every create of
create-breakdown-20261108T223355Z on adacachy2 ("lacks a seed's fork of the init"): there is no seed and no clone on
the path, and no root to switch, the init's root made when it parked.
Run: python3 -B -I bench/competitive/probes/create_breakdown_parked_test.py
Its fixtures are that run's first traced create, l000001, from its traced and its detailed trace (testdata/parked.*).
"""
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
sys.path.insert(0, HERE)
import create_breakdown as cb  # noqa: E402
import exec_breakdown as eb  # noqa: E402

TRACED = os.path.join(HERE, "testdata", "parked.traced.txt")
DETAIL = os.path.join(HERE, "testdata", "parked.detail.txt")


class ParkedClaim(unittest.TestCase):
    def test_a_trace_of_parked_claims_is_read_as_one(self):
        self.assertIs(cb.ops_for(TRACED), cb.PARKED_OPS)
        self.assertIs(cb.ops_for(DETAIL), cb.PARKED_OPS)

    def test_a_parked_claim_is_cut_whole_into_its_parts(self):
        (call,), _ = eb.calls(TRACED, cb.PARKED_OPS)
        self.assertNotIn("missing", call, call.get("missing"))
        # l000001's parts in microseconds, read off its trace: the record looked for in both places; the choosers; the
        # state directory and the checked spec; the FIFO and the cgroup's claim; the root's trees; the opens for the
        # init; the request to its read; the init to its root's mount namespace; that join; the pair; the network's
        # join; the init to its ready signal; the agent's wake to the init's stat; the state renamed; the record's
        # open; its write to the close (no fsync, no rename: O_EXCL); the init watched and the call's return.
        self.assertEqual(call["parts"], [6, 4, 22, 35, 8, 7, 25, 47, 1, 3, 1, 19, 6, 17, 19, 1, 0, 0, 0, 9])
        self.assertEqual(sum(call["parts"]), call["span"])
        self.assertEqual(len(call["parts"]), len(cb.PARKED_PARTS))
        self.assertEqual(call["cgroups"], ["parked"])
        self.assertEqual(call["record_kinds"], ["close"])

    def test_a_parked_claims_opens_are_the_files_they_stand_for(self):
        # The detailed trace names each open's file: the record's two looks, the init's stat and the record's write.
        (call,), _ = eb.calls(DETAIL, cb.PARKED_OPS)
        self.assertNotIn("missing", call, call.get("missing"))
        self.assertEqual(sum(call["parts"]), call["span"])

    def test_an_init_the_path_forked_is_no_parked_claim(self):
        # The same create with its init made by a fork inside its window, as a shim's or a seed's is: not read as a
        # parked claim, and refused by its cutter, naming why.
        with open(TRACED) as f:
            lines = f.readlines()
        fork = ("        seam_oci-503486  [013] ...1. 79178.538725: sched_process_fork: comm=seam_oci pid=503486 "
                "child_comm=seam_oci child_pid=503504\n")
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "forked.txt")
            with open(path, "w") as f:
                f.writelines(lines[:1] + [fork] + lines[1:])
            self.assertIsNot(cb.ops_for(path), cb.PARKED_OPS)
            (call,), _ = eb.calls(path, cb.PARKED_OPS)
            self.assertIn("forked", call.get("missing", ""))


if __name__ == "__main__":
    unittest.main()
