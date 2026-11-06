#!/usr/bin/env python3
"""bench/competitive/probes: create_serial.py reads each profile's sampling rate from the run's own output (its
program's @profile_hz), not a copy of the rate create_serial_inside.sh's program was written with (Ada's 2026-11-06
rule; CP-T7.1's readings).

  1. A MODE=threads run sampled at 1000 Hz: 1000 samples of one lifecycle are 1,000,000 us.
  2. A MODE=oncpu run sampled at 2000 Hz: 2000 samples of one lifecycle are 1,000,000 us.
  3. A run whose output names no rate is the inside script's before it said one: the rate its programs ran at then.

  python3 -I check_create_serial.py
"""
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))


def read(label, text, n=1):
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, f"{label}.bt.txt")
        with open(path, "w") as f:
            f.write(text)
        r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "create_serial.py"), f"{label}={path}:{n}"],
                           capture_output=True, text=True)
    if r.returncode != 0:
        raise AssertionError(r.stderr)
    return r.stdout


class Rates(unittest.TestCase):
    def test_a_threads_run_is_read_at_its_own_rate(self):
        # Do: read 1000 samples of thread hex at the 1000 Hz the run says. Expect: 1,000,000 us a lifecycle.
        out = read("threads.c1", "@profile_hz: 1000\n@thread[hex]: 1000\n@ours: 1000\n@samples: 1000\n")
        self.assertIn("1000000.0 us of CPU a lifecycle", out)

    def test_an_oncpu_run_is_read_at_its_own_rate(self):
        # Do: read 2000 on-CPU samples at the 2000 Hz the run says. Expect: 1,000,000 us a lifecycle.
        out = read("oncpu.c1", "@profile_hz: 2000\n@samples: 2000\n@sys[0]: count 1, average 1000, total 1000\n")
        self.assertIn("on-CPU 1000000.0 us a lifecycle", out)

    def test_a_run_that_names_no_rate_is_read_at_the_rate_it_ran_at(self):
        # Do: read a threads run from before the rate was said, 999 samples. Expect: its 999 Hz, 1,000,000 us.
        out = read("threads.c1", "@thread[hex]: 999\n@ours: 999\n@samples: 999\n")
        self.assertIn("1000000.0 us of CPU a lifecycle", out)


if __name__ == "__main__":
    unittest.main()
