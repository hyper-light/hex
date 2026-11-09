#!/usr/bin/env python3
"""Goal 17's record, rounds.py: arms interleaved round by round, each round's order rotated and its load and each
burst's window recorded (Ada's rule: a p99 claim stands on at least 299 interleaved rounds, the load recorded each
round), each arm a process speaking its protocol (seam_oci's serve phase, libcrun-driver's fds 3 and 4), a classic
arm's seam_oci asked for the classic root and no way (goal 17's baseline), and the summary each arm's creates over the
timed rounds against every other arm's.
Run: python3 -B -I bench/competitive/probes/rounds_test.py
"""
import io
import json
import os
import stat
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import rounds as rd


class Fake:
    """An arm whose bursts answer `ms` for each lifecycle, or `short` lifecycles fewer than asked."""

    def __init__(self, name, ms, short=0):
        self.name, self.ms, self.short, self.asked = name, ms, short, []

    def burst(self, lifecycles, conc):
        self.asked.append((lifecycles, conc))
        n = lifecycles - self.short
        return [self.ms] * n, [self.ms / 2] * n


def ticking():
    t = [0]

    def clock():
        t[0] += 10
        return t[0]
    return clock


def loads():
    n = [0]

    def load():
        n[0] += 1
        return {"load1": float(n[0]), "procs_running": n[0], "procs_blocked": 0, "cpu_some_avg10": 0.0}
    return load


def stand_in(d, name, body):
    path = os.path.join(d, name)
    with open(path, "w") as f:
        f.write(f"#!{sys.executable}\n{body}")
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)
    return path


SEAM = """import json, os, sys
bursts = 0
for line in sys.stdin:
    word, n, conc = line.split()
    n = int(n)
    if n == 3:
        print(json.dumps({"error": "create v000001-000: refused"}), flush=True)
        break
    print(json.dumps({"create_ms": [1.5] * n, "delete_ms": [0.5] * n}), flush=True)
    bursts += 1
json.dump({"phases": {"serve": {"bursts": bursts, "mount_namespaces": {"tree": bursts}}},
           "cgroup": os.environ.get("HEXBENCH_CGROUP")}, open(sys.argv[1], "w"))
"""

SEAM_ENV = """import json, os, sys
for line in sys.stdin:
    word, n, conc = line.split()
    print(json.dumps({"create_ms": [1.5] * int(n), "delete_ms": [0.5] * int(n)}), flush=True)
json.dump({"phases": {"serve": {}}, "root": os.environ.get("HEXBENCH_ROOT"), "way": os.environ.get("HEX_OCI_MNTNS")},
          open(sys.argv[1], "w"))
"""

DRIVER = """import os
req, rep = os.fdopen(3), os.fdopen(4, "w")
for line in req:
    verb = line.split()[0]
    rep.write({"create": "ok 2000000 0\\n", "start": "ok 100000 0\\n", "delete": "ok 1000000 0\\n"}[verb])
    rep.flush()
"""


class Rounds(unittest.TestCase):
    def test_each_round_rotates_the_order_and_records_its_load_and_each_bursts_window(self):
        """Do two warmup and three timed rounds of three arms, expect each round's order rotated by one arm, the
        warmup rounds numbered below 0, one load reading a round, and each burst's window from the clock around it."""
        arms = [Fake("copy", 1.0), Fake("tree", 0.5), Fake("empty", 0.75)]
        out = io.StringIO()
        rd.run_rounds(arms, 3, 2, 8, 8, out, clock=ticking(), load=loads())
        rows = [json.loads(line) for line in out.getvalue().splitlines()]
        self.assertEqual([r["round"] for r in rows], [-2, -1, 0, 1, 2])
        self.assertEqual([r["warmup"] for r in rows], [True, True, False, False, False])
        self.assertEqual(rows[0]["order"], ["copy", "tree", "empty"])
        self.assertEqual(rows[1]["order"], ["tree", "empty", "copy"])
        self.assertEqual(rows[2]["order"], ["empty", "copy", "tree"])
        self.assertEqual([r["load"]["load1"] for r in rows], [1.0, 2.0, 3.0, 4.0, 5.0])
        first = rows[0]["bursts"][0]
        self.assertEqual((first["arm"], first["t0_ns"], first["t1_ns"]), ("copy", 10, 20))
        self.assertEqual(rows[1]["bursts"][0]["t0_ns"], 70)
        self.assertTrue(all(a.asked == [(8, 8)] * 5 for a in arms))

    def test_a_burst_answering_fewer_lifecycles_than_asked_is_refused_and_its_round_kept(self):
        """Do a round whose second arm answers 7 of 8, expect ArmError naming it, the round's line written with the
        burst before it alone."""
        out = io.StringIO()
        with self.assertRaises(rd.ArmError) as e:
            rd.run_rounds([Fake("copy", 1.0), Fake("tree", 1.0, short=1)], 2, 0, 8, 8, out, clock=ticking(),
                          load=loads())
        self.assertIn("tree", str(e.exception))
        rows = [json.loads(line) for line in out.getvalue().splitlines()]
        self.assertEqual(len(rows), 1)
        self.assertEqual([b["arm"] for b in rows[0]["bursts"]], ["copy"])

    def test_the_summary_pools_timed_rounds_and_sets_each_arm_against_every_other(self):
        """Do one warmup round and two timed ones, expect each arm's creates over the timed rounds alone, its round
        medians, the second arm's rounds below the first's and its p50 ratio, and the first's below the second's."""
        out = io.StringIO()
        rd.run_rounds([Fake("copy", 2.0), Fake("tree", 1.0)], 2, 1, 4, 2, out, clock=ticking(), load=loads())
        rows = [json.loads(line) for line in out.getvalue().splitlines()]
        s = rd.summarize(rows, ["copy", "tree"])
        self.assertEqual(s["rounds"], 2)
        self.assertEqual(s["per_arm"]["copy"]["create_ms"]["n"], 8)
        self.assertEqual(s["per_arm"]["tree"]["round_medians"], [1.0, 1.0])
        vs = s["per_arm"]["tree"]["against_copy"]
        self.assertEqual((vs["rounds_below"], vs["rounds"], vs["p50_ratio"], vs["p99_lower"]), (2, 2, 0.5, True))
        back = s["per_arm"]["copy"]["against_tree"]
        self.assertEqual((back["rounds_below"], back["p50_ratio"], back["p99_lower"]), (0, 2.0, False))
        self.assertEqual(s["load1"]["p50"], 2.0)

    def test_percentiles_are_nearest_rank(self):
        """Do 1..100, expect p50 50, p99 99, p99.9 100 (summarize.py's rank)."""
        d = rd.dist(list(range(1, 101)))
        self.assertEqual((d["p50"], d["p99"], d["p99.9"], d["max"]), (50, 99, 100, 100))


class Processes(unittest.TestCase):
    def test_a_hex_arm_and_a_libcrun_arm_answer_bursts_through_their_protocols(self):
        """Do two timed rounds against a stand-in seam_oci and a stand-in libcrun-driver, expect each burst's times
        as their replies give them, the hex arm's result read back from its seam.json, and exit 0."""
        with tempfile.TemporaryDirectory() as d:
            env = dict(os.environ, ROUNDS_SEAM=stand_in(d, "seam", SEAM), ROUNDS_HEX="/hex", ROUNDS_SLEEP="/s",
                       ROUNDS_TRUE="/t", ROUNDS_LIBCRUN=stand_in(d, "driver", DRIVER), ROUNDS_LIBCRUN_ROOT=d)
            out = os.path.join(d, "out")
            saved = dict(os.environ)
            os.environ.update(env)
            try:
                rc = rd.main([out, "2", "0", "2", "2", "hex:tree", "libcrun"])
            finally:
                os.environ.clear()
                os.environ.update(saved)
            self.assertEqual(rc, 0)
            with open(os.path.join(out, "summary.json")) as f:
                s = json.load(f)
            self.assertEqual(s["per_arm"]["tree"]["create_ms"]["p50"], 1.5)
            self.assertEqual(s["per_arm"]["libcrun"]["create_ms"]["p50"], 2.0)
            self.assertEqual(s["per_arm"]["libcrun"]["delete_ms"]["p50"], 1.0)
            with open(os.path.join(out, "arms.json")) as f:
                arms = json.load(f)
            self.assertEqual(arms["tree"]["result"]["phases"]["serve"]["bursts"], 2)
            self.assertEqual(arms["libcrun"]["exits"], [0, 0])

    def test_a_classic_arm_asks_its_seam_oci_for_the_classic_root_and_no_way(self):
        """Goal 17's baseline arm (the lead's ruling of 2026-11-09). Do a hex:classic arm beside a hex:tree arm, under an
        environment asking for the copy and the classic root both, expect the classic arm named classic, its seam_oci
        asked for the classic root (HEXBENCH_ROOT) and for no way, and the tree arm for its way and no classic root."""
        with tempfile.TemporaryDirectory() as d:
            saved = dict(os.environ)
            os.environ.update(ROUNDS_SEAM=stand_in(d, "seam", SEAM_ENV), ROUNDS_HEX="/hex", ROUNDS_SLEEP="/s",
                              ROUNDS_TRUE="/t", HEX_OCI_MNTNS="copy", HEXBENCH_ROOT="classic")
            try:
                rc = rd.main([os.path.join(d, "out"), "1", "0", "2", "2", "hex:classic", "hex:tree"])
            finally:
                os.environ.clear()
                os.environ.update(saved)
            self.assertEqual(rc, 0)
            with open(os.path.join(d, "out", "arms.json")) as f:
                arms = json.load(f)
            self.assertEqual((arms["classic"]["result"]["root"], arms["classic"]["result"]["way"]), ("classic", None))
            self.assertEqual((arms["tree"]["result"]["root"], arms["tree"]["result"]["way"]), (None, "tree"))

    def test_each_hex_arm_gets_a_cgroup_of_its_own_with_the_parents_controllers(self):
        """Do two hex arms under a ROUNDS_CGROUP whose subtree_control holds cpu and memory, expect each arm's node
        handed its own cgroup (HEXBENCH_CGROUP) with both controllers handed down."""
        with tempfile.TemporaryDirectory() as d:
            parent = os.path.join(d, "cg")
            os.mkdir(parent)
            with open(os.path.join(parent, "cgroup.subtree_control"), "w") as f:
                f.write("cpu memory\n")
            saved = dict(os.environ)
            os.environ.update(ROUNDS_SEAM=stand_in(d, "seam", SEAM), ROUNDS_HEX="/hex", ROUNDS_SLEEP="/s",
                              ROUNDS_TRUE="/t", ROUNDS_CGROUP=parent)
            try:
                rc = rd.main([os.path.join(d, "out"), "1", "0", "2", "2", "hex:copy", "hex:tree"])
            finally:
                os.environ.clear()
                os.environ.update(saved)
            self.assertEqual(rc, 0)
            with open(os.path.join(d, "out", "arms.json")) as f:
                arms = json.load(f)
            for way in ("copy", "tree"):
                own = os.path.join(parent, f"arm-{way}")
                self.assertEqual(arms[way]["result"]["cgroup"], own)
                with open(os.path.join(own, "cgroup.subtree_control")) as f:
                    self.assertEqual(f.read(), "+cpu +memory")

    def test_a_libcrun_arm_given_a_cgroup_gives_each_container_its_own_beneath_it(self):
        """Do a burst of two on a libcrun arm with ROUNDS_LIBCRUN_CGROUP set, expect each create handed a bundle of
        its own whose cgroupsPath is that cgroup and its id, its root the base bundle's rootfs, and each bundle gone
        after its delete."""
        with tempfile.TemporaryDirectory() as d:
            base = os.path.join(d, "bundle-true")
            os.makedirs(os.path.join(base, "rootfs"))
            with open(os.path.join(base, "config.json"), "w") as f:
                json.dump({"root": {"path": "rootfs"}, "linux": {"namespaces": [{"type": "user"}]}}, f)
            seen = os.path.join(d, "seen")
            driver = stand_in(d, "driver", DRIVER.replace(
                'verb = line.split()[0]',
                f'verb = line.split()[0]\n    if verb == "create": open({seen!r}, "a").write(open(line.split()[1] + '
                f'"/config.json").read() + "\\n")'))
            env = dict(os.environ, ROUNDS_LIBCRUN=driver, ROUNDS_LIBCRUN_ROOT=d, ROUNDS_TRUE=base,
                       ROUNDS_LIBCRUN_CGROUP="/user.slice/s.scope/libcrun")
            arm = rd.LibcrunArm(2, os.path.join(d, "out"), env)
            try:
                creates, deletes = arm.burst(2, 2)
            finally:
                arm.close()
            self.assertEqual((creates, deletes), ([2.0, 2.0], [1.0, 1.0]))
            with open(seen) as f:
                configs = [json.loads(line) for line in f]
            self.assertEqual(sorted(c["linux"]["cgroupsPath"] for c in configs),
                             ["/user.slice/s.scope/libcrun/lc000000-000", "/user.slice/s.scope/libcrun/lc000000-001"])
            self.assertTrue(all(c["root"]["path"] == os.path.join(base, "rootfs") for c in configs))
            self.assertEqual(os.listdir(os.path.join(d, "out", "arm-libcrun", "bundles")), [])

    def test_an_arm_that_refuses_a_burst_fails_the_run_naming_it(self):
        """Do a burst of 3 against the stand-in seam_oci, which refuses it, expect exit 3 and the refusal in the
        summary."""
        with tempfile.TemporaryDirectory() as d:
            saved = dict(os.environ)
            os.environ.update(ROUNDS_SEAM=stand_in(d, "seam", SEAM), ROUNDS_HEX="/hex", ROUNDS_SLEEP="/s",
                              ROUNDS_TRUE="/t")
            try:
                rc = rd.main([os.path.join(d, "out"), "1", "0", "3", "1", "hex:copy"])
            finally:
                os.environ.clear()
                os.environ.update(saved)
            self.assertEqual(rc, 3)
            with open(os.path.join(d, "out", "summary.json")) as f:
                self.assertIn("refused", json.load(f)["failed"])


if __name__ == "__main__":
    unittest.main()
