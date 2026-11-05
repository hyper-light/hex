#!/usr/bin/env python3
"""bench/competitive: fetch.py's crun source for libcrun in process, off the network (host.sh's OCI pass; CP-T7.1,
goal 5 extended step 6's second criterion, whose passes each ask for it).

  1. A tarball already in <base>/src whose sha256 is the pinned one is used as it is: no request is made, so a
     pass doesn't spend GitHub's unauthenticated budget on bytes it holds.
  2. One whose bytes aren't the pinned ones is never used: the release is asked for again.

  python3 -I check_fetch.py
"""
import hashlib
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
sys.path.insert(0, HERE)
import fetch  # noqa: E402

TAG = "9.9.9"
PINNED = b"crun's pinned source"


class CrunSource(unittest.TestCase):
    def setUp(self):
        self.asked = []

        def offline(url, dest=None, limit=0):
            self.asked.append(url)
            raise OSError("the check has no network")

        self.saved = (fetch.get, fetch.CRUN_SRC)
        fetch.get, fetch.CRUN_SRC = offline, (TAG, hashlib.sha256(PINNED).hexdigest())

    def tearDown(self):
        fetch.get, fetch.CRUN_SRC = self.saved

    def tarball(self, base, data):
        os.makedirs(os.path.join(base, "src"))
        path = os.path.join(base, "src", f"crun-{TAG}.tar.gz")
        with open(path, "wb") as f:
            f.write(data)
        return path

    def test_the_pinned_tarball_on_disk_is_used_without_a_request(self):
        # Do: the pinned bytes in <base>/src, every request refused. Expect: its path, verified, nothing asked.
        with tempfile.TemporaryDirectory() as base:
            path = self.tarball(base, PINNED)
            got = fetch.crun_src(base, os.path.join(base, "bin"))
        self.assertEqual(self.asked, [])
        self.assertEqual((got["path"], got["sha256"], got["verified"]),
                         (path, hashlib.sha256(PINNED).hexdigest(), True))

    def test_a_tarball_that_isnt_the_pinned_bytes_is_asked_for_again(self):
        # Do: other bytes under the pinned name, every request refused. Expect: the release asked for, the
        # refusal raised, the bytes on disk never handed on.
        with tempfile.TemporaryDirectory() as base:
            self.tarball(base, b"not crun's pinned source")
            with self.assertRaises(OSError):
                fetch.crun_src(base, os.path.join(base, "bin"))
        self.assertEqual(self.asked, [f"https://api.github.com/repos/containers/crun/releases/tags/{TAG}"])


if __name__ == "__main__":
    unittest.main()
