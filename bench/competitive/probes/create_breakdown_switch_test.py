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


def fixture(dry=0, mount_us=100, clones=True):
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
        "netns": {"made_on_path": dry},
        "dev_mounts": {"taken": 97, "on_path": 3, "failed": 0},
    }}}
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


if __name__ == "__main__":
    unittest.main()
