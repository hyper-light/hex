#!/usr/bin/env python3
"""Goal 5 step 3: AC-e retains every pooled/dry create and enforces the mounts criterion.
Run: python3 -B -I bench/competitive/probes/create_breakdown_switch_test.py
Synthetic ftrace and seam inputs are in memory; real parsers and CLI routing run unchanged.
"""
import contextlib
import io
import json
import os
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import create_breakdown as cb


def fixture(dry=0, mount_us=100, clones=True, node_made=None):
    """100 creates at eight in flight: the first `dry` inits born with their network namespace, the rest
    joining one by setns. With `node_made`, that many of those joined were made by the node on the path
    for an owner's box (step 6's reuse), counted among the pool's takes made on the path too."""
    def line(pid, ts, body):
        sec, us = divmod(ts, 1_000_000)
        return f"hex-{pid} [004] d..2 {sec}.{us:06d}: {body}\n"
    trace = []
    for i in range(100):
        pid, t = 1000 + i, 1_000_000 + i * 10_000
        if clones:
            trace.append(line(pid, t, "sys_clone3 -> 0x0"))
        start = t
        if i >= dry:
            trace.append(line(pid, t + 10, "sys_setns(fd: 0x3, flags: 0x40000000)"))
            trace.append(line(pid, t + 20, "sys_setns -> 0x0"))
            start = t + 20
        trace.append(line(pid, start + mount_us, "sys_chroot(filename: 0x1)"))
        trace.append(line(pid, start + mount_us + 1, "sys_chroot -> 0x0"))
    seam = {"errors": {}, "phases": {"throughput": {
        "8": {"lifecycles": 100, "create_ms": [1.0] * 98 + [2.5, 3.0]},
        "netns": {"made_on_path": dry + (node_made or 0)},
        "dev_mounts": {"taken": 97, "on_path": 3, "failed": 0},
    }}}
    if node_made is not None:
        seam["phases"]["throughput"]["netns_reuse"] = {"handed_on": 50, "fresh": 50 - dry,
                                                       "made_on_path": node_made}
    return {"trace": "".join(trace), "stats": "overrun: 0\n", "seam": json.dumps(seam)}


def run(files, scope="mounts", cli=False, seam=True):
    stdout, stderr = io.StringIO(), io.StringIO()
    def opened(path, *args, **kwargs):
        return io.StringIO(files[os.fspath(path)])
    with patch("builtins.open", side_effect=opened), contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        if cli:
            with patch.object(sys, "argv", ["create_breakdown.py", "switch", "trace", "stats", "seam", scope]):
                rc = cb.main()
        else:
            rc = cb.switch("trace", "stats", "seam" if seam else None, scope)
    return rc, json.loads(stdout.getvalue().splitlines()[0]), stderr.getvalue()


class SwitchEvidence(unittest.TestCase):
    def test_every_pooled_dry_and_mixed_create_is_measured(self):
        for dry in (0, 17, 100):
            with self.subTest(dry=dry):
                rc, result, _ = run(fixture(dry=dry))
                self.assertEqual(rc, 0)
                self.assertEqual(result["mounts_spans"], 100)
                self.assertEqual(result["clone_started_spans"], dry)
                self.assertEqual(result["setns_started_spans"], 100 - dry)
                self.assertTrue(result["fallback_counts_match"])
                self.assertEqual(result["create_p99_ms"], 2.5)
                self.assertEqual(result["dev_mounts"]["taken"], 97)

    def test_a_namespace_the_node_made_on_path_for_an_owner_is_counted_as_joined(self):
        """Goal 5 extended step 3 (T-T7.1.4): with step 6's reuse a create that finds the pool dry is given
        a namespace the node makes on its path, which the init joins by setns. Expect the clone-started
        spans to match the pool's takes made on the path less the node's, and a node count that
        disagrees refused."""
        for dry, node_made in ((0, 3), (17, 3), (17, 0)):
            with self.subTest(dry=dry, node_made=node_made):
                rc, result, _ = run(fixture(dry=dry, node_made=node_made))
                self.assertEqual(rc, 0)
                self.assertEqual(result["clone_started_spans"], dry)
                self.assertEqual(result["network_made_by_node"], node_made)
                self.assertEqual(result["expected_network_on_path"], dry)
                self.assertTrue(result["fallback_counts_match"])
        files = fixture(dry=17, node_made=3)
        files["seam"] = files["seam"].replace('"made_on_path": 3}', '"made_on_path": 2}')
        rc, result, _ = run(files)
        self.assertEqual(rc, 1)
        self.assertFalse(result["fallback_counts_match"])

    def test_mounts_bar_boundary_and_cli_scope(self):
        for us, passed in ((499, True), (500, True), (501, False), (600, False)):
            with self.subTest(us=us):
                rc, result, _ = run(fixture(mount_us=us), cli=True)
                self.assertEqual(rc, 0 if passed else 1)
                self.assertEqual(result["criterion"], "mounts")
                self.assertEqual(result["pass"], passed)
        rc, result, _ = run(fixture(mount_us=600), scope="switch")
        self.assertEqual(rc, 0)
        self.assertFalse(result["mounts_le_bar"])

    def test_missing_or_failed_start_and_wrong_fallback_count_are_refused(self):
        files = fixture(dry=17, clones=False)
        self.assertEqual(run(files)[0], 1)
        files = fixture()
        files["trace"] = files["trace"].replace("sys_setns -> 0x0", "sys_setns -> -1", 1)
        self.assertEqual(run(files)[0], 1)
        files = fixture(dry=17)
        files["seam"] = files["seam"].replace('"made_on_path": 17', '"made_on_path": 16')
        self.assertEqual(run(files)[0], 1)

    def test_whole_create_samples_and_concurrency_are_required(self):
        for bad in ("missing", "short", "wrong-concurrency", "errors", "not-json"):
            with self.subTest(bad=bad):
                files = fixture()
                seam = json.loads(files["seam"])
                tp = seam["phases"]["throughput"]
                if bad == "missing":
                    del tp["8"]["create_ms"]
                elif bad == "short":
                    tp["8"]["create_ms"].pop()
                elif bad == "wrong-concurrency":
                    tp["1"] = tp.pop("8")
                elif bad == "errors":
                    seam["errors"] = {"throughput": "create failed"}
                files["seam"] = "{" if bad == "not-json" else json.dumps(seam)
                self.assertEqual(run(files)[0], 1)
        self.assertEqual(run(fixture(), seam=False)[0], 1)

    def test_lost_unpaired_failed_switch_and_init_pivot_are_refused(self):
        for bad in ("lost", "unpaired", "failed", "pivot", "missing-create"):
            with self.subTest(bad=bad):
                files = fixture()
                if bad == "lost":
                    files["stats"] = "overrun: 1\n"
                elif bad == "unpaired":
                    files["trace"] = files["trace"].replace("sys_chroot -> 0x0", "other", 1)
                elif bad == "failed":
                    files["trace"] = files["trace"].replace("sys_chroot -> 0x0", "sys_chroot -> -1", 1)
                elif bad == "pivot":
                    files["trace"] += "hex-1000 [004] d..2 3.000000: sys_pivot_root(new_root: 0x1)\n"
                else:
                    files["trace"] = "\n".join(line for line in files["trace"].splitlines() if not line.startswith("hex-1000 "))
                self.assertEqual(run(files)[0], 1)

    def test_root_in_place_reads_alike_on_a_copied_and_a_joined_namespace(self):
        """Goal 6 (T-T7.1.4, decision 6's update): an init on a namespace made from its tree joins it by
        setns(CLONE_NEWNS) before its network and makes no chroot. Do: one init each way, a seed that
        pivots. Expect the root's span from part 19's start to the first chdir after it, the namespace's
        join added where it came first, the seed left out, and the copied way's part 19 unchanged."""
        def line(pid, ts, body):
            sec, us = divmod(ts, 1_000_000)
            return f"hex-{pid} [004] d..2 {sec}.{us:06d}: {body}\n"
        trace = "".join([
            # The copied way: network joined at +20, chroot at +120, chdir("/") at +125.
            line(1, 1_000_000, "sys_clone3 -> 0x0"),
            line(1, 1_000_010, "sys_setns(fd: 0x3, flags: 0x40000000)"),
            line(1, 1_000_020, "sys_setns -> 0x0"),
            line(1, 1_000_120, "sys_chroot(filename: 0x1)"),
            line(1, 1_000_121, "sys_chroot -> 0x0"),
            line(1, 1_000_125, "sys_chdir(filename: 0x1)"),
            # The joined way: the mount namespace's join takes 7 us, then the network, then chdir 60 us on.
            line(2, 2_000_000, "sys_clone3 -> 0x0"),
            line(2, 2_000_002, "sys_setns(fd: 0x3, flags: 0x20000)"),
            line(2, 2_000_009, "sys_setns -> 0x0"),
            line(2, 2_000_010, "sys_setns(fd: 0x4, flags: 0x40000000)"),
            line(2, 2_000_020, "sys_setns -> 0x0"),
            line(2, 2_000_080, "sys_chdir(filename: 0x1)"),
            # A seed: cloned, pivots, chdirs; never an init.
            line(3, 3_000_000, "sys_clone3 -> 0x0"),
            line(3, 3_000_005, "sys_pivot_root(new_root: 0x1)"),
            line(3, 3_000_900, "sys_chdir(filename: 0x1)"),
        ])
        files = {"trace": trace, "stats": "overrun: 0\n"}
        _, result, _ = run(files, scope="switch", seam=False)
        self.assertEqual(result["root_spans"], 2)
        self.assertEqual(result["root_mntns_joins"], 1)
        self.assertEqual(result["root_max_ms"], 0.105)
        self.assertEqual(result["root_p50_ms"], 0.067)
        self.assertEqual(result["mounts_p50_ms"], 0.1)
        self.assertEqual(result["root_unended"], 0)


if __name__ == "__main__":
    unittest.main()
