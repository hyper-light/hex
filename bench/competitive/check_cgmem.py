#!/usr/bin/env python3
"""bench/competitive: cgmem.py's memory.stat breakdown, off the bench host (AC-T7.1.5's accounting; the density
phase's attribution, runtime-architecture.md decision 2).

  1. An oracle test: parse_stat reads fixtures/memory.stat (a cgroup's memory.stat, kernel 7.2.9) entry for
     entry as the fixture has it, by an independent reading of the same file.
  2. breakdown and rise on a stand-in cgroup directory whose memory.current and memory.stat are written to
     known values: each part, kernel_other and unattributed come out as computed by hand, and held agrees.
  3. Hostile input: a missing entry, a line that isn't one name and one integer, a name given twice, a
     negative, non-ASCII digits, bytes that aren't text, an empty file and a long line each come back as a
     reason (and the entry left out), never an exception; a breakdown over such a reading is None where a
     sum needs what's missing.
  4. quiet_snapshot: a reading waits for memory.current to hold still across two of the kernel's stat flushes,
     at most QUIET_TRIES windows, and one that never does is marked.
  5. tree_bytes: a state root's entries and bytes, a missing root as zero, and the entry budget's stop.

  python3 -I check_cgmem.py
"""
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
sys.path.insert(0, HERE)
import cgmem  # noqa: E402

FIXTURE = os.path.join(HERE, "fixtures", "memory.stat")


def cgroup(d, stat, current, swap=0):
    for name, text in (("memory.stat", stat), ("memory.current", f"{current}\n"),
                       ("memory.swap.current", f"{swap}\n")):
        with open(os.path.join(d, name), "w" if isinstance(text, str) else "wb") as f:
            f.write(text)
    return d


class Oracle(unittest.TestCase):
    def test_each_entry_reads_as_the_fixture_has_it(self):
        """Parse the fixture; expect every one of its lines' entries, at its value, and no reason."""
        with open(FIXTURE) as f:
            text = f.read()
        oracle = {}
        for line in text.split("\n"):
            if line:
                name, _, value = line.partition(" ")
                oracle[name] = int(value)
        stat, reasons = cgmem.parse_stat(text)
        self.assertEqual(reasons, [])
        self.assertEqual(stat, oracle)
        self.assertGreater(len(stat), len(cgmem.STAT_ENTRIES))  # non-vacuity: the fixture holds every entry
        for k in cgmem.STAT_ENTRIES:
            self.assertIn(k, stat)


KNOWN = """anon 1000000
file 500000
kernel 300000
kernel_stack 16384
pagetables 40960
sec_pagetables 0
percpu 8000
sock 4096
vmalloc 8192
shmem 70000
zswap 0
slab_reclaimable 100000
slab_unreclaimable 20000
slab 120000
"""


class Breakdown(unittest.TestCase):
    def test_parts_as_computed_by_hand(self):
        """Break down a cgroup of known readings; expect each part, kernel_other and unattributed as computed."""
        with tempfile.TemporaryDirectory(dir=os.path.expanduser("~/.cache")) as d:
            # current = anon + file + kernel + sock + 1,000 that memory.stat doesn't name
            snap = cgmem.snapshot(cgroup(d, KNOWN, 1000000 + 500000 + 300000 + 4096 + 1000, swap=2048))
        b = cgmem.breakdown(snap)
        self.assertEqual(b["reasons"], [])
        self.assertEqual((b["anon"], b["shmem"], b["kernel"], b["slab"], b["sock"]), (1000000, 70000, 300000, 120000, 4096))
        self.assertEqual(b["kernel_other"], 300000 - (120000 + 8000 + 40960 + 0 + 16384 + 8192))
        self.assertEqual(b["held"], 1805096 - 500000 + 70000 + 2048)
        self.assertEqual(b["held"], cgmem.held(snap))
        self.assertEqual(b["unattributed"], b["held"] - (1000000 + 70000 + 300000 + 4096) - 2048)
        self.assertEqual(b["unattributed"], 1000)  # what memory.current holds that memory.stat doesn't name

    def test_rise_is_peak_less_base(self):
        """Rise from a baseline to a peak breakdown; expect each part's difference and None where one lacks it."""
        with tempfile.TemporaryDirectory(dir=os.path.expanduser("~/.cache")) as d:
            base = cgmem.breakdown(cgmem.snapshot(cgroup(d, KNOWN, 1900000)))
            peak = cgmem.breakdown(cgmem.snapshot(cgroup(d, KNOWN.replace("anon 1000000", "anon 1500000")
                                                             .replace("sock 4096\n", ""), 2400000)))
        r = cgmem.rise(peak, base)
        self.assertEqual(r["anon"], 500000)
        self.assertEqual(r["slab"], 0)
        self.assertIsNone(r["sock"])
        self.assertEqual(r["reasons"], ["peak: no sock"])


HOSTILE = {
    "a missing entry": ("file 1\nshmem 2\n", "no anon"),
    "three fields": ("anon 1 2\n", "line 1"),
    "one field": ("anon\n", "line 1"),
    "not a number": ("anon lots\n", "line 1"),
    "a negative": ("anon -5\n", "line 1"),
    "superscript digits": ("anon ²\n", "line 1"),
    "a float": ("anon 1.5\n", "line 1"),
    "a name given twice": ("anon 1\nanon 2\n", "line 2: anon given twice"),
    "an empty file": ("", "no anon"),
    "a long line": ("anon " + "9" * 100000 + "x\n", "line 1"),
    "NUL bytes": ("anon \x00\x00\n", "line 1"),
}


class Hostile(unittest.TestCase):
    def test_each_malformed_reading_is_a_reason(self):
        """Parse each hostile memory.stat; expect a reason naming it and no exception."""
        for name, (text, reason) in HOSTILE.items():
            with self.subTest(name):
                stat, reasons = cgmem.parse_stat(text)
                self.assertTrue(any(reason in r for r in reasons), reasons)
                self.assertTrue(all(len(r) < 200 for r in reasons))

    def test_a_twice_given_name_keeps_the_first(self):
        """Parse a name given twice; expect its first value."""
        self.assertEqual(cgmem.parse_stat("anon 1\nanon 2\n")[0]["anon"], 1)

    def test_bytes_that_arent_text(self):
        """Snapshot a memory.stat holding bytes that aren't UTF-8; expect a reason, the good lines read, no exception."""
        with tempfile.TemporaryDirectory(dir=os.path.expanduser("~/.cache")) as d:
            snap = cgmem.snapshot(cgroup(d, b"anon \xff\xfe\nfile 10\nshmem 3\n", 100))
        self.assertIn("line 1", snap["reasons"][0])
        self.assertEqual(snap["stat"], {"file": 10, "shmem": 3})
        b = cgmem.breakdown(snap)
        self.assertEqual(b["held"], 93)
        self.assertIsNone(b["anon"])
        self.assertIsNone(b["unattributed"])
        self.assertIsNone(b["kernel_other"])

    def test_held_without_file_is_a_typed_error(self):
        """Ask held of a reading without file; expect a ValueError naming it, and breakdown's held None."""
        with tempfile.TemporaryDirectory(dir=os.path.expanduser("~/.cache")) as d:
            snap = cgmem.snapshot(cgroup(d, "anon 5\n", 100))
        with self.assertRaisesRegex(ValueError, "no file, shmem"):
            cgmem.held(snap)
        self.assertIsNone(cgmem.breakdown(snap)["held"])


class Quiet(unittest.TestCase):
    def readings(self, currents):
        """quiet_snapshot over a stand-in cgroup whose memory.current is each of `currents` in turn, one per
        window (the stand-in sleep writes the next)."""
        with tempfile.TemporaryDirectory(dir=os.path.expanduser("~/.cache")) as d:
            cgroup(d, KNOWN, currents[0])
            left = list(currents[1:])
            slept = []

            def sleep(s):
                slept.append(s)
                with open(os.path.join(d, "memory.current"), "w") as f:
                    f.write(f"{left.pop(0)}\n")
            snap, how = cgmem.quiet_snapshot(d, sleep=sleep)
        return snap, how, slept

    def test_a_still_cgroup_is_read_after_one_window(self):
        """Read a cgroup whose memory.current holds still; expect it read after one window of two flushes."""
        snap, how, slept = self.readings([5000000, 5000000 + cgmem.QUIET_DRIFT])
        self.assertEqual(how, {"quiet": True, "windows": 1, "drift_bytes": cgmem.QUIET_DRIFT})
        self.assertEqual(slept, [cgmem.QUIET_WINDOW_S])
        self.assertGreater(cgmem.QUIET_WINDOW_S, 2 * cgmem.FLUSH_S)
        self.assertEqual(snap["current"], 5000000 + cgmem.QUIET_DRIFT)

    def test_a_moving_cgroup_waits_until_it_holds_still(self):
        """Read a cgroup that grows for two windows, then holds; expect the reading after the third."""
        snap, how, _ = self.readings([1000000, 3000000, 6000000, 6000000])
        self.assertEqual(how, {"quiet": True, "windows": 3, "drift_bytes": 0})
        self.assertEqual(snap["current"], 6000000)

    def test_a_cgroup_that_never_holds_still_is_marked(self):
        """Read a cgroup that grows every window; expect the last reading after QUIET_TRIES, marked not quiet."""
        currents = [1000000 * (i + 1) for i in range(cgmem.QUIET_TRIES + 1)]
        snap, how, slept = self.readings(currents)
        self.assertEqual(how, {"quiet": False, "windows": cgmem.QUIET_TRIES, "drift_bytes": 1000000})
        self.assertEqual(len(slept), cgmem.QUIET_TRIES)
        self.assertEqual(snap["current"], currents[-1])


class Tree(unittest.TestCase):
    def test_entries_and_bytes(self):
        """Walk a state root of two containers' files; expect their count and sizes, symlinks not followed."""
        with tempfile.TemporaryDirectory(dir=os.path.expanduser("~/.cache")) as d:
            for c in ("a", "b"):
                os.mkdir(os.path.join(d, c))
                with open(os.path.join(d, c, "state.json"), "w") as f:
                    f.write("x" * 100)
                os.symlink("/", os.path.join(d, c, "root"))
            t = cgmem.tree_bytes(d)
        self.assertEqual(t["entries"], 6)
        self.assertFalse(t["short"])
        self.assertGreaterEqual(t["apparent_bytes"], 200)
        self.assertGreater(t["allocated_bytes"], 0)

    def test_a_missing_root_reads_zero(self):
        """Walk a root that isn't there; expect zero entries and bytes."""
        t = cgmem.tree_bytes(os.path.expanduser("~/.cache/hexbench-no-such-root"))
        self.assertEqual((t["entries"], t["apparent_bytes"], t["short"]), (0, 0, False))

    def test_the_budget_stops_the_walk(self):
        """Walk a root of more entries than the budget; expect the walk short at the budget."""
        old = cgmem.TREE_ENTRY_BUDGET
        cgmem.TREE_ENTRY_BUDGET = 3
        try:
            with tempfile.TemporaryDirectory(dir=os.path.expanduser("~/.cache")) as d:
                for i in range(5):
                    open(os.path.join(d, str(i)), "w").close()
                t = cgmem.tree_bytes(d)
        finally:
            cgmem.TREE_ENTRY_BUDGET = old
        self.assertEqual((t["entries"], t["short"]), (3, True))


if __name__ == "__main__":
    unittest.main()
