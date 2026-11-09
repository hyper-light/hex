#!/usr/bin/env python3
"""The parked claim's cuts (goal 16; the lead, 2026-11-09): a create given a cgroup with its init parked there
(decision 8: a made-ahead slot's, cloned ahead by the refill), cut whole. The seed path's cutter refused every create of
create-breakdown-20261108T223355Z on adacachy2 ("lacks a seed's fork of the init"): there is no seed and no clone on
the path, and no root to switch, the init's root made when it parked.
Run: python3 -B -I bench/competitive/probes/create_breakdown_parked_test.py
Its fixtures are that run's first traced create, l000001, from its traced and its detailed trace (testdata/parked.*).
"""
import contextlib
import io
import json
import os
import shutil
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

    def test_a_parked_runs_summary_says_where_its_create_goes(self):
        # create-breakdown-20261109T164604Z (the lead's run at ee5f4d77) cut every create but its summary failed:
        # KeyError "the shim's spawn and exec", the levers' table taking a run with no seed for the shim path. Do:
        # summarize a run of one round, its traced create the fixture's, and one untraced between. Expect: its parts and
        # a parked claim's levers, against a quarter of libcrun's create p50.
        def lifecycle(warmup, ms):
            return {"errors": {}, "warmup": warmup, "phases": {"lifecycle": {"ms": {"create": ms}}}}

        with tempfile.TemporaryDirectory() as run:
            with open(os.path.join(run, "env.txt"), "w") as f:
                f.write("host: test\n")
            for kind, body in (("libcrun", lifecycle(1, [0.66, 0.67])), ("seam", lifecycle(1, [0.17, 0.18])),
                               ("traced", lifecycle(1, [0.228, 0.170]))):
                with open(os.path.join(run, f"{kind}.r1.json"), "w") as f:
                    json.dump(body, f)
            shutil.copy(TRACED, os.path.join(run, "traced.r1.txt"))
            open(os.path.join(run, "traced.r1.stats"), "w").close()
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                cb.summarize(run)
            text = out.getvalue()
            self.assertIn("## Create through the seam, in parts (1 traced creates)", text)
            self.assertIn("## Where a parked claim's create goes", text)
            self.assertIn(f"| {cb.REQUEST} |", text)
            with open(os.path.join(run, "summary.json")) as f:
                levers = json.load(f)["levers"]
            self.assertAlmostEqual(levers["target_ms"], eb.dist([0.66, 0.67])["p50"] / 4)


if __name__ == "__main__":
    unittest.main()
