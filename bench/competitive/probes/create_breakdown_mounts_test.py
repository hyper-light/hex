#!/usr/bin/env python3
"""Goal 5 extended step 3: the mounts reader counts each hold of the kernel's mount lock in an init's part 19.
Run: python3 -B -I bench/competitive/probes/create_breakdown_mounts_test.py
A synthetic ftrace of the events mounts_trace enables, the namespace_sem kprobes among them, read by the real reader.
"""
import contextlib
import io
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import create_breakdown as cb

HEADER = "#define __NR_chroot 161\n#define __NR_setns 308\n#define __NR_open_tree 428\n#define __NR_move_mount 429\n"


def trace(probed=True):
    """Two inits' part 19, each from its network join to its chroot: a move_mount and an open_tree holding the lock
    for write, a statmount-like read holding it shared between them; a hold by a process outside any span."""
    lines, t = [], 1_000_000
    def at(pid, body):
        nonlocal t
        t += 10
        sec, us = divmod(t, 1_000_000)
        lines.append(f"hex-{pid} [004] d..2 {sec}.{us:06d}: {body}\n")
    for pid in (1000, 1001):
        at(pid, "sys_enter: NR 308 (3, 40000000)")
        at(pid, "sys_exit: NR 308 = 0")
        at(pid, "sys_enter: NR 429 (5, 0, 6, 0, 104)")
        probed and at(pid, "nsw: (down_write+0x4/0x60) sem=0xffffffff92c64b40")
        at(pid, "sys_exit: NR 429 = 0")
        at(pid, "sys_enter: NR 428 (6, 0, 88801)")
        probed and at(pid, "nsw: (down_write+0x4/0x60) sem=0xffffffff92c64b40")
        at(pid, "sys_exit: NR 428 = 7")
        probed and at(pid, "nsr: (down_read+0x4/0x60) sem=0xffffffff92c64b40")
        at(pid, "sys_enter: NR 161 (1)")
    probed and at(2000, "nsw: (down_write+0x4/0x60) sem=0xffffffff92c64b40")
    return "".join(lines)


def run(text):
    with tempfile.TemporaryDirectory() as d:
        paths = {name: os.path.join(d, name) for name in ("mounts.txt", "mounts.stats", "unistd_64.h")}
        for name, body in (("mounts.txt", text), ("mounts.stats", "overrun: 0\n"), ("unistd_64.h", HEADER)):
            with open(paths[name], "w") as f:
                f.write(body)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = cb.mounts(paths["mounts.txt"], paths["mounts.stats"], paths["unistd_64.h"])
        with open(os.path.join(d, "mounts.summary.json")) as f:
            return rc, json.load(f), out.getvalue()


class MountLockHolds(unittest.TestCase):
    def test_each_hold_in_a_span_is_counted_by_the_call_it_was_in(self):
        rc, summary, report = run(trace())
        self.assertEqual(rc, 0)
        self.assertEqual(summary["creates"], 2)
        self.assertEqual(summary["nssem_write_per_create"], 2.0)
        self.assertEqual(summary["nssem_read_per_create"], 1.0)
        self.assertEqual(summary["nssem_write_by_call"], "move_mount 1.0, open_tree 1.0")
        self.assertIn("2.0 held for write (by call: move_mount 1.0, open_tree 1.0), 1.0 shared "
                      "(by call: between calls 1.0)", report)

    def test_a_trace_without_the_probes_says_so(self):
        rc, summary, report = run(trace(probed=False))
        self.assertEqual(rc, 0)
        self.assertIsNone(summary["nssem_write_per_create"])
        self.assertIn("The mount lock's holds weren't traced", report)


if __name__ == "__main__":
    unittest.main()
