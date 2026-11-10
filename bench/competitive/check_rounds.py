#!/usr/bin/env python3
"""bench/competitive: a crashed round is never a measured one (host.sh's OCI pass; CP-T7.1, goal 5 extended step 6,
whose ratio readings take host.sh's exit and table as the run's; CLAUDE.md section 3, no silent drops).

  1. host.sh over a seam_oci that writes its first round and exits 101 in its second: host.sh exits non-zero
     and names each crashed round with its exit, and its table marks them and takes no best of N beside them.
  2. The same pass with no round crashing exits 0 with nothing marked (the check's own non-vacuity).
  3. summarize.py over a run whose crashed.txt holds a line that isn't oci_inside.sh's: the line is shown as it
     is, never dropped, and the rounds it may stand for are taken by no best of N.

The OCI pass runs as host.sh runs it, but for docker: a stand-in runs oci_inside.sh in a user and mount
namespace of its own (unshare -rm), with the run's binds and its /run a tmpfs, so 1 and 2 need Linux.

  python3 -I check_rounds.py
"""
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
sys.path.insert(0, HERE)
import summarize  # noqa: E402

# docker run's stand-in: its -v binds, --tmpfs mounts and -e NAME=value settings, then the command after the
# image, in a user and mount namespace of the caller's. Any other docker command does nothing.
DOCKER = r"""#!/usr/bin/env bash
[ "$1" = run ] || exit 0
shift; binds=(); tmpfs=()
while [ $# -gt 0 ]; do
  case $1 in
    -v) binds+=("$2"); shift 2 ;;
    --tmpfs) tmpfs+=("${2%%:*}"); shift 2 ;;
    -e) case $2 in *=*) export "$2" ;; esac; shift 2 ;;
    --name|--label|--security-opt) shift 2 ;;
    -*) shift ;;
    *) break ;;
  esac
done
shift
setup='set -e; mount -t tmpfs none /opt'
for b in "${binds[@]}"; do
  src=${b%%:*}; rest=${b#*:}; dst=${rest%%:*}
  [ "$src" = "$dst" ] || setup="$setup; mkdir -p '$dst'; mount --bind '$src' '$dst'"
done
for t in "${tmpfs[@]}"; do setup="$setup; mount -t tmpfs none '$t'"; done
exec unshare -rm bash -c "$setup; exec \"\$@\"" docker-run "$@"
"""

# seam_oci's stand-in: a round's JSON, as seam_oci writes one, while CRASH_ROUND isn't its round; in that round
# a panic's message and exit 101, as seam_oci's on a bad CPU list.
SEAM_OCI = r"""#!/usr/bin/env bash
case "$1" in
  *".r${CRASH_ROUND:-0}.json")
    echo "thread 'main' panicked at crates/hex-bench/benches/seam_oci.rs:601:41:" >&2; exit 101 ;;
esac
echo '{"phases": {}, "errors": {}}' > "$1"
"""


def write(path, text):
    with open(path, "w") as f:
        f.write(text)
    os.chmod(path, 0o755)


def can_unshare():
    if sys.platform != "linux" or not shutil.which("unshare"):
        return False
    return subprocess.run(["unshare", "-rm", "true"], capture_output=True).returncode == 0


@unittest.skipUnless(can_unshare(), "host.sh's OCI pass needs Linux and an unprivileged user namespace (unshare -rm)")
class HostPass(unittest.TestCase):
    def run_pass(self, crash_round):
        tmp = tempfile.mkdtemp(prefix="check_rounds.")
        self.addCleanup(shutil.rmtree, tmp)
        stubs = os.path.join(tmp, "stubs")
        os.makedirs(stubs)
        write(os.path.join(stubs, "docker"), DOCKER)
        write(os.path.join(tmp, "seam_oci"), SEAM_OCI)
        write(os.path.join(tmp, "hex"), "#!/bin/sh\nexit 1\n")
        out = os.path.join(tmp, "out")
        env = dict(os.environ, PATH=stubs + os.pathsep + os.environ["PATH"], BENCH_HOST_OK=socket.gethostname(),
                   LOAD_GATE="0", FETCH="0", PHASES="oci", RUNTIMES=" ", STANDIN="0", ROUNDS="2", CPUSET="0",
                   HEX_BENCH=os.path.join(tmp, "base"), OUT_DIR=out, SEAM_OCI_BIN=os.path.join(tmp, "seam_oci"),
                   CRASH_ROUND=str(crash_round))
        done = subprocess.run(["bash", os.path.join(HERE, "host.sh"), os.path.join(tmp, "hex")], env=env,
                              capture_output=True, text=True, timeout=300)
        with open(os.path.join(out, "table.md")) as f:
            table = f.read()
        return done, table

    def test_a_crashed_round_ends_the_pass_non_zero_naming_it(self):
        # Do: seam_oci's second round exits 101, in process and over the socket. Expect: host.sh exits non-zero,
        # naming each crashed round and its exit; the table marks both and has no OCI table from round 1 alone.
        done, table = self.run_pass(2)
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)
        for name in ("ociseam r2 exited 101", "ociseamsock r2 exited 101"):
            self.assertIn(name, done.stderr)
            self.assertIn(name, table)
        self.assertNotIn("OCI class", table)

    def test_a_pass_with_no_crashed_round_exits_zero(self):
        # Do: no round crashes. Expect: host.sh exits 0 and the table marks nothing crashed.
        done, table = self.run_pass(0)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertNotIn("rashed", table)


class Summary(unittest.TestCase):
    def test_a_line_that_isnt_oci_inside_shs_is_shown_and_no_round_is_taken(self):
        # Do: a run with ociseam's round 1 and a crashed.txt line that names no round. Expect: the line in the
        # summary as it is, and no OCI table from ociseam's round 1 (the line may stand for any runtime's).
        with tempfile.TemporaryDirectory() as run:
            with open(os.path.join(run, "ociseam.r1.json"), "w") as f:
                json.dump({"phases": {}, "errors": {}}, f)
            write(os.path.join(run, "crashed.txt"), "ociseam \xff garbled\n")
            got = summarize.summary(run)
        self.assertIn("ociseam \xff garbled", got)
        self.assertNotIn("OCI class", got)


if __name__ == "__main__":
    unittest.main()
