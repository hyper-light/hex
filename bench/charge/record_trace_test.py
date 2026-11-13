#!/usr/bin/env python3
"""record_trace.py's steps (joint plan step 14, AC-T7.4.1): each window's CPU kept to the millisecond as whole busy
threads and one more for the rest, time and CPU carried from window to window so neither rounds away, memory rounded
up to a MiB.
Run: python3 -B -I bench/charge/record_trace_test.py
"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import record_trace as rt

MIB = 1 << 20


class StepsTest(unittest.TestCase):
    def test_w14_a_window_keeps_its_cpu_to_the_millisecond(self):
        """Do turn a 100 ms window that used 230 ms of CPU and holds a byte past 5 MiB into steps. Expect 30 ms of 3
        threads, then 70 ms of 2, each holding 6 MiB."""
        self.assertEqual(rt.Steps().window(100_000, 230_000, 5 * MIB + 1), [(30, 3, 6), (70, 2, 6)])

    def test_w14_an_idle_window_and_a_whole_one_are_one_step(self):
        """Do turn a window that used no CPU, and one that used exactly two CPUs. Expect one step each."""
        self.assertEqual(rt.Steps().window(100_000, 0, 0), [(100, 0, 0)])
        self.assertEqual(rt.Steps().window(100_000, 200_000, MIB), [(100, 2, 1)])

    def test_w14_time_and_cpu_carry_from_window_to_window(self):
        """Do turn windows ending at 100.4 ms (50.6 ms of CPU), 200.9 ms (101.7 ms) and 200.999 ms (101.9 ms). Expect
        the first 100 ms with 50 of CPU, the second 100 ms with the 51 carried, the third nothing (no whole millisecond
        since), and the steps' time and CPU summing to the whole milliseconds read."""
        steps = rt.Steps()
        first = steps.window(100_400, 50_600, 0)
        second = steps.window(200_900, 101_700, 0)
        third = steps.window(200_999, 101_900, 0)
        self.assertEqual(first, [(50, 1, 0), (50, 0, 0)])
        self.assertEqual(second, [(51, 1, 0), (49, 0, 0)])
        self.assertEqual(third, [])
        every = first + second + third
        self.assertEqual(sum(ms for ms, _, _ in every), 200)
        self.assertEqual(sum(ms * threads for ms, threads, _ in every), 101)


class FakeCgroup:
    """A cgroup v2 tree under a temporary directory: the session's own and its parent, each file as the kernel writes it."""

    def __init__(self, root, cpu_max_on_session=True):
        self.root = root
        self.parent = os.path.join(root, "runner.slice")
        self.session = os.path.join(self.parent, "agent-1.scope")
        os.makedirs(self.session)
        self.write(self.parent, "cpu.max", "max 100000\n")
        if cpu_max_on_session:
            self.write(self.session, "cpu.max", "max 50000\n")
        self.set(0, 0, populated=True)

    @staticmethod
    def write(cgroup, name, text):
        with open(os.path.join(cgroup, name), "w") as f:
            f.write(text)

    def set(self, usage_usec, memory, populated):
        self.write(self.session, "cpu.stat", f"usage_usec {usage_usec}\nuser_usec 0\nsystem_usec 0\n")
        if memory is None:
            if os.path.exists(os.path.join(self.session, "memory.current")):
                os.remove(os.path.join(self.session, "memory.current"))
        else:
            self.write(self.session, "memory.current", f"{memory}\n")
        self.write(self.session, "cgroup.events", f"populated {int(populated)}\nfrozen 0\n")


class FakeProc:
    """A /proc of the session's processes under a temporary directory: each process's smaps_rollup or, where the test
    gives none, its status, as the kernel writes them, its memory in kB."""

    def __init__(self, root):
        self.root = root
        os.makedirs(root)

    def put(self, pid, kb, rollup=True):
        os.makedirs(os.path.join(self.root, str(pid)), exist_ok=True)
        if rollup:
            text = f"55d0c0000000-7ffc00000000 ---p 00000000 00:00 0    [rollup]\nRss:   {kb} kB\nPss:   {kb} kB\n"
            FakeCgroup.write(os.path.join(self.root, str(pid)), "smaps_rollup", text)
        else:
            text = f"Name:\tagent\nVmPeak:\t{2 * kb} kB\nVmRSS:\t{kb} kB\nThreads:\t4\n"
            FakeCgroup.write(os.path.join(self.root, str(pid)), "status", text)

    def end(self, pid):
        import shutil
        shutil.rmtree(os.path.join(self.root, str(pid)))


class AttachTest(unittest.TestCase):
    def setUp(self):
        import tempfile
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)

    def test_w14_the_window_is_the_period_of_the_nearest_cpu_max(self):
        """Do read the window of a session whose own cpu.max names a 50 ms period, and of one whose cpu.max is absent
        under a parent naming 100 ms. Expect 50 ms, then 100 ms; and a session no cgroup of whose path has a cpu.max
        refused, naming it."""
        own = FakeCgroup(os.path.join(self.dir.name, "a"))
        self.assertEqual(rt.period_us(own.session, own.root), 50_000)
        inherited = FakeCgroup(os.path.join(self.dir.name, "b"), cpu_max_on_session=False)
        self.assertEqual(rt.period_us(inherited.session, inherited.root), 100_000)
        os.remove(os.path.join(inherited.parent, "cpu.max"))
        with self.assertRaises(SystemExit) as refused:
            rt.period_us(inherited.session, inherited.root)
        self.assertIn(inherited.session, str(refused.exception))

    def test_w14_an_attached_session_is_recorded_until_it_empties(self):
        """Do attach to a running session whose window is 100 ms, then let three windows pass in which it uses 50, 150
        and 0 ms of CPU and holds 1, 2 and 2 MiB, after which it empties. Expect the trace's header to name the cgroup
        and the window, and its steps to be those three windows', from the attach on, then its end."""
        fake = FakeCgroup(os.path.join(self.dir.name, "c"), cpu_max_on_session=False)
        fake.set(1_000_000, MIB, populated=True)
        windows = iter([(1_050_000, MIB, True), (1_200_000, 2 * MIB, True), (1_200_000, 2 * MIB, False)])
        clock = [0]

        def now_ns():
            return clock[0]

        def sleep_until(deadline_ns):
            clock[0] = deadline_ns
            usage, memory, populated = next(windows)
            fake.set(usage, memory, populated)

        out = os.path.join(self.dir.name, "trace.txt")
        rt.attach(out, fake.session, fake.root, now_ns=now_ns, sleep_until=sleep_until)
        with open(out) as f:
            text = f.read()
        self.assertIn(f"attached to the cgroup {fake.session}", text)
        self.assertIn("# window: 100000 us", text)
        steps = [tuple(map(int, line.split())) for line in text.splitlines() if not line.startswith("#")]
        self.assertEqual(steps, [(50, 1, 1), (50, 0, 1), (50, 2, 2), (50, 1, 2), (100, 0, 2)])
        self.assertIn("# ended: 300 ms, 200 ms of CPU, the cgroup emptied", text)

    def test_w14_a_cgroup_with_no_memory_controller_is_recorded_from_its_processes_rss(self):
        """ROOT, 2026-11-13: kiln's per-agent cgroups have cpu but no memory controller. Do attach to a running session
        whose cgroup has no memory.current, its two processes holding 1 MiB (its smaps_rollup's Rss) and 2 MiB (its
        status's VmRSS, it having no smaps_rollup); let a window pass, then one in which the second ends, after which
        the session empties. Expect the trace's header to name its memory as its processes' Rss, its steps to hold 3
        MiB, then 1 MiB, the ended process out of the sum, and its CPU cpu.stat's as before."""
        fake = FakeCgroup(os.path.join(self.dir.name, "e"), cpu_max_on_session=False)
        fake.set(1_000_000, None, populated=True)
        FakeCgroup.write(fake.session, "cgroup.procs", "11\n12\n")
        proc = FakeProc(os.path.join(self.dir.name, "proc"))
        proc.put(11, 1024)
        proc.put(12, 2048, rollup=False)
        clock = [0]

        def now_ns():
            return clock[0]

        def first():
            fake.set(1_100_000, None, populated=True)

        def second():
            proc.end(12)
            fake.set(1_200_000, None, populated=False)

        windows = iter([first, second])

        def sleep_until(deadline_ns):
            clock[0] = deadline_ns
            next(windows)()

        out = os.path.join(self.dir.name, "rss.txt")
        rt.attach(out, fake.session, fake.root, now_ns=now_ns, sleep_until=sleep_until, proc=proc.root)
        with open(out) as f:
            text = f.read()
        self.assertIn("# memory: procs-rss", text)
        steps = [tuple(map(int, line.split())) for line in text.splitlines() if not line.startswith("#")]
        self.assertEqual(steps, [(100, 1, 3), (100, 1, 1)])

    def test_w14_a_session_in_its_cgroups_memory_says_so(self):
        """Do attach to a session whose cgroup has memory.current. Expect the trace's header to name its memory as the
        cgroup's."""
        fake = FakeCgroup(os.path.join(self.dir.name, "f"), cpu_max_on_session=False)
        fake.set(0, MIB, populated=True)
        clock = [0]

        def sleep_until(deadline_ns):
            clock[0] = deadline_ns
            fake.set(0, MIB, populated=False)

        out = os.path.join(self.dir.name, "cgroup.txt")
        rt.attach(out, fake.session, fake.root, now_ns=lambda: clock[0], sleep_until=sleep_until)
        with open(out) as f:
            self.assertIn("# memory: cgroup", f.read())

    def test_w14_a_cgroup_is_named_as_proc_names_it_or_by_its_directory(self):
        """Do name a session's cgroup as /proc/<pid>/cgroup and systemd name it (rooted at the cgroup mount, with or
        without its leading slash), and by its directory under the mount. Expect each the directory under the mount."""
        root = "/sys/fs/cgroup"
        scope = "user.slice/user-1000.slice/user@1000.service/app.slice/agent-1.scope"
        for named in (f"/{scope}", scope, f"{root}/{scope}"):
            self.assertEqual(rt.cgroup_dir(named, root), f"{root}/{scope}")

    def test_w14_an_empty_or_missing_cgroup_is_refused(self):
        """Do attach to a session that has emptied, and to a path that is no cgroup. Expect each refused, naming it,
        and no trace written."""
        fake = FakeCgroup(os.path.join(self.dir.name, "d"))
        fake.set(0, 0, populated=False)
        out = os.path.join(self.dir.name, "none.txt")
        for path in (fake.session, os.path.join(fake.parent, "gone.scope")):
            with self.assertRaises(SystemExit) as refused:
                rt.attach(out, path, fake.root)
            self.assertIn(path, str(refused.exception))
            self.assertFalse(os.path.exists(out))


if __name__ == "__main__":
    unittest.main()
