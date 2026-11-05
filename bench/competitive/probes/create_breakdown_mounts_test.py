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

HEADER = ("#define __NR_chdir 80\n#define __NR_chroot 161\n#define __NR_setns 308\n#define __NR_open_tree 428\n"
          "#define __NR_move_mount 429\n")


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


def holders_trace():
    """trace()'s two inits, and holds outside any span: three for write by the pools' maker, one by the node."""
    lines = [trace()]
    for i, (comm, pid) in enumerate((("hex-devfs", 3000),) * 3 + (("seam_oci", 3001),)):
        lines.append(f"{comm}-{pid} [005] d..2 1.{900000 + i:06d}: nsw: (down_write+0x4/0x60) "
                     "sem=0xffffffff92c64b40\n")
    return "".join(lines)


def run(text, span="switch"):
    with tempfile.TemporaryDirectory() as d:
        paths = {name: os.path.join(d, name) for name in ("mounts.txt", "mounts.stats", "unistd_64.h")}
        for name, body in (("mounts.txt", text), ("mounts.stats", "overrun: 0\n"), ("unistd_64.h", HEADER)):
            with open(paths[name], "w") as f:
                f.write(body)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = cb.mounts(paths["mounts.txt"], paths["mounts.stats"], paths["unistd_64.h"], "pruned", span)
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

    def test_holds_outside_the_spans_are_counted_by_their_holder(self):
        """Goal 6 (AC-T7.1.4's under-load bar, goal6-mntns.md §10): the pools' maker and the node hold the lock
        outside any init's span. Expect them per create by command, the stray init's hold among them as `hex`."""
        rc, summary, report = run(holders_trace())
        self.assertEqual(rc, 0)
        self.assertEqual(summary["nssem_write_outside_per_create"], "hex-devfs 1.5, hex 0.5, seam_oci 0.5")
        self.assertIn("The mount lock held for write outside the spans, per create, by holder: hex-devfs 1.5, hex 0.5, "
                      "seam_oci 0.5.", report)

    def test_a_trace_without_the_probes_says_so(self):
        rc, summary, report = run(trace(probed=False))
        self.assertEqual(rc, 0)
        self.assertIsNone(summary["nssem_write_per_create"])
        self.assertIn("The mount lock's holds weren't traced", report)


def lines_of(events):
    """An ftrace text of (us, pid, comm, body) events."""
    out = []
    for t, pid, comm, body in events:
        sec, us = divmod(t, 1_000_000)
        out.append(f"{comm}-{pid} [004] d..2 {sec}.{us:06d}: {body}\n")
    return "".join(out)


def root_trace():
    """Goal 6 (decision 6's update): the root in place. A copied-namespace init (1000) chroots then chdirs; a
    joined-namespace init (1001) joins its mount namespace made from its tree by setns(CLONE_NEWNS), 30 us, before
    its network's join, and chdirs with no chroot. Init 1001's move_mount blocks 40 us waiting for the mount lock
    (the rwsem's write slowpath), woken by another hex process; its open_tree is preempted 25 us by a busy loop."""
    t = 1_000_000
    return lines_of([
        (t, 1000, "hex", "sys_enter: NR 308 (3, 40000000)"),
        (t + 10, 1000, "hex", "sys_exit: NR 308 = 0"),
        (t + 20, 1000, "hex", "sys_enter: NR 429 (5, 0, 6, 0, 104)"),
        (t + 30, 1000, "hex", "sys_exit: NR 429 = 0"),
        (t + 40, 1000, "hex", "sys_enter: NR 161 (1)"),
        (t + 50, 1000, "hex", "sys_exit: NR 161 = 0"),
        (t + 60, 1000, "hex", "sys_enter: NR 80 (1)"),
        (t + 1000, 1001, "hex", "sys_enter: NR 308 (4, 20000)"),
        (t + 1030, 1001, "hex", "sys_exit: NR 308 = 0"),
        (t + 1040, 1001, "hex", "sys_enter: NR 308 (3, 40000000)"),
        (t + 1050, 1001, "hex", "sys_exit: NR 308 = 0"),
        (t + 1060, 1001, "hex", "sys_enter: NR 429 (5, 0, 6, 0, 104)"),
        (t + 1061, 1001, "hex", "nsww: (rwsem_down_write_slowpath+0x0/0x600) sem=0xffffffff92c64b40"),
        (t + 1062, 1001, "hex", "sched_switch: prev_comm=hex prev_pid=1001 prev_prio=120 prev_state=D ==> "
                                "next_comm=swapper/4 next_pid=0 next_prio=120"),
        (t + 1100, 1002, "hex", "sched_waking: comm=hex pid=1001 prio=120 target_cpu=004"),
        (t + 1102, 0, "<idle>", "sched_switch: prev_comm=swapper/4 prev_pid=0 prev_prio=120 prev_state=R ==> "
                                "next_comm=hex next_pid=1001 next_prio=120"),
        (t + 1103, 1001, "hex", "nsw: (down_write+0x4/0x60) sem=0xffffffff92c64b40"),
        (t + 1110, 1001, "hex", "sys_exit: NR 429 = 0"),
        (t + 1120, 1001, "hex", "sys_enter: NR 428 (6, 0, 88801)"),
        (t + 1121, 1001, "hex", "sched_switch: prev_comm=hex prev_pid=1001 prev_prio=120 prev_state=R+ ==> "
                                "next_comm=bash next_pid=77 next_prio=120"),
        (t + 1146, 77, "bash", "sched_switch: prev_comm=bash prev_pid=77 prev_prio=120 prev_state=R ==> "
                               "next_comm=hex next_pid=1001 next_prio=120"),
        (t + 1150, 1001, "hex", "sys_exit: NR 428 = 7"),
        (t + 1160, 1001, "hex", "sys_enter: NR 80 (1)"),
    ])


class RootInPlace(unittest.TestCase):
    def test_root_span_ends_at_the_chdir_and_counts_the_mount_namespace_join(self):
        """T-T7.1 goal 6 (indicative): do the root-in-place cut, expect each init's span from its network join to
        its chdir, the chroot a call within it and the join of a namespace made from its tree counted before it."""
        rc, summary, report = run(root_trace(), span="root")
        self.assertEqual(rc, 0)
        self.assertEqual(summary["creates"], 2)
        # 1000: 50 us, net join exit to chdir. 1001: 110 us plus its 30 us mount namespace join.
        self.assertEqual(summary["span_max_us"], 140)
        self.assertIn("| chroot | 0.5 |", report)
        self.assertIn("| setns (mnt) | 0.5 | 15.0 |", report)
        self.assertIn("to its chdir", report)

    def test_switch_span_is_unchanged_by_default(self):
        rc, summary, _ = run(root_trace())
        self.assertEqual(rc, 0)
        self.assertEqual(summary["creates"], 1)  # the joined init makes no chroot, so no switch span ends

    def test_a_blocked_wait_names_its_call_its_waker_and_the_mount_lock(self):
        """Do a span whose move_mount sleeps in the rwsem's write slowpath, expect the blocked time charged to
        move_mount, woken by hex, as a wait for the mount lock; the preemption charged to the busy loop."""
        rc, summary, report = run(root_trace(), span="root")
        self.assertEqual(rc, 0)
        self.assertIn("move_mount, woken by hex, waiting for the mount lock (write) 20.0", report)
        self.assertIn("the CPU went first to (us per create): bash 12.5", report)
        self.assertEqual(summary["nssem_waits_per_create"], 0.5)


if __name__ == "__main__":
    unittest.main()
