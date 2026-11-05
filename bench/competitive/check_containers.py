#!/usr/bin/env python3
"""bench/competitive: a bench run's containers end with it, however it ends (CLAUDE.md sections 2 and 3: a container
a run leaves running holds CPU and memory past its run, under every later run's timed parts; CP-T7.1's readings).

  1. host.sh's OCI pass in a systemd unit whose ExecStopPost is reap.sh, its unit SIGKILLed mid-pass (as an OOM kill
     or a full disk's end does): no container of the run is left, and while it ran its container was labelled with
     the run (hex.bench.run=<unit>).
  2. The same pass in a unit with no ExecStopPost, stopped (SIGTERM): host.sh's own trap removes its container.

Each pass is real (docker, the lab image, oci_inside.sh), with a seam_oci that marks its start and then blocks, so
the unit is ended mid-pass, once the pass's container runs. It needs Linux, docker, a systemd user manager and
registry.fedoraproject.org/fedora:44 already pulled; elsewhere it's skipped, and says why. Only this check's own
containers are removed: its units' and the one its directory's root-owned files are cleared with.

  python3 -I check_containers.py
"""
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
IMAGE = "registry.fedoraproject.org/fedora:44"
DEADLINE_S = 180  # the lab image's start and oci_inside.sh's set-up before seam_oci, on a loaded host

# seam_oci's stand-in: says it started (the fact the check waits on), then holds the pass where it is.
SEAM_OCI = """#!/usr/bin/env bash
touch /opt/out/seam.started
exec sleep 600
"""


def quiet(*cmd):
    return subprocess.run(cmd, capture_output=True, text=True)


def why_not():
    if sys.platform != "linux":
        return "needs Linux"
    for cmd in (["docker", "image", "inspect", IMAGE], ["systemctl", "--user", "is-system-running"]):
        if not shutil.which(cmd[0]):
            return f"needs {cmd[0]}"
        r = quiet(*cmd)
        if cmd[0] == "docker" and r.returncode != 0:
            return f"needs docker and {IMAGE} pulled"
        if cmd[0] == "systemctl" and r.stdout.strip() not in ("running", "degraded"):
            return "needs a systemd user manager"
    return None


def until(fact, what):
    """Waits for `fact()` to hold, at most DEADLINE_S; fails naming `what` if it never does."""
    end = time.monotonic() + DEADLINE_S
    while time.monotonic() < end:
        if fact():
            return
        time.sleep(0.5)
    raise AssertionError(f"{what} didn't happen in {DEADLINE_S} s")


def containers(*filters):
    args = ["docker", "ps", "-aq"]
    for f in filters:
        args += ["--filter", f]
    return quiet(*args).stdout.split()


@unittest.skipIf(why_not(), why_not() or "")
class RunEnd(unittest.TestCase):
    def pass_in_unit(self, how, stop_post):
        base = os.path.join(os.path.expanduser("~/.cache"), "hex-bench-checks")
        os.makedirs(base, exist_ok=True)
        tmp = tempfile.mkdtemp(prefix="containers.", dir=base)
        unit = f"hexcheck-containers-{os.getpid()}-{how}"
        self.addCleanup(self.clear, tmp, unit)
        with open(os.path.join(tmp, "seam_oci"), "w") as f:
            f.write(SEAM_OCI)
        os.chmod(os.path.join(tmp, "seam_oci"), 0o755)
        with open(os.path.join(tmp, "hex"), "w") as f:
            f.write("#!/bin/sh\nexit 1\n")
        os.chmod(os.path.join(tmp, "hex"), 0o755)
        out = os.path.join(tmp, "out")
        env = {"PATH": os.environ["PATH"], "BENCH_HOST_OK": socket.gethostname(), "LOAD_GATE": "0", "FETCH": "0",
               "PHASES": "oci", "RUNTIMES": " ", "STANDIN": "0", "ROUNDS": "1",
               "CPUSET": str(min(os.sched_getaffinity(0))), "HEX_BENCH": os.path.join(tmp, "base"), "OUT_DIR": out,
               "SEAM_OCI_BIN": os.path.join(tmp, "seam_oci"), "BENCH_MEMORY": "1g", "BENCH_OOM": "1000"}
        cmd = ["systemd-run", "--user", "--quiet", f"--unit={unit}", "--collect"]
        if stop_post:
            cmd.append(f"--property=ExecStopPost=/usr/bin/bash {HERE}/reap.sh {unit}")
        cmd += [f"--setenv={k}={v}" for k, v in env.items()]
        cmd += ["bash", os.path.join(HERE, "host.sh"), os.path.join(tmp, "hex")]
        started = subprocess.run(cmd, capture_output=True, text=True)
        self.assertEqual(started.returncode, 0, started.stderr)
        until(lambda: os.path.exists(os.path.join(out, "seam.started")), "the pass's seam_oci starting")
        pid = quiet("systemctl", "--user", "show", "-p", "MainPID", "--value", unit).stdout.strip()
        name = f"hexbench-oci-{pid}"
        running = containers(f"name=^/{name}$", "status=running")
        labelled = containers(f"name=^/{name}$", f"label=hex.bench.run={unit}")
        self.assertEqual(len(running), 1, f"{name} isn't running mid-pass")
        return unit, name, labelled

    def ended(self, unit):
        return quiet("systemctl", "--user", "is-active", unit).stdout.strip() not in ("active", "activating",
                                                                                     "deactivating")

    def clear(self, tmp, unit):
        quiet("systemctl", "--user", "kill", "-s", "SIGKILL", unit)
        quiet("systemctl", "--user", "reset-failed", unit)
        for c in containers(f"label=hex.bench.run={unit}"):
            quiet("docker", "rm", "-f", c)
        # oci_inside.sh writes as root into the run's directory: cleared from a container of this check's own.
        quiet("docker", "run", "--rm", "--label", f"hex.bench.run={unit}", "-v", f"{tmp}:/t", IMAGE,
              "find", "/t", "-mindepth", "1", "-delete")
        shutil.rmtree(tmp, ignore_errors=True)

    def test_a_sigkilled_units_containers_are_removed_by_its_stop_post(self):
        # Do: kill the unit with SIGKILL mid-pass (no trap of host.sh's runs). Expect: its ExecStopPost (reap.sh)
        # leaves no container of the run, which was labelled with the run while it ran.
        unit, name, labelled = self.pass_in_unit("kill", stop_post=True)
        try:
            quiet("systemctl", "--user", "kill", "-s", "SIGKILL", unit)
            until(lambda: self.ended(unit), f"{unit} ending")
            self.assertEqual(containers(f"name=^/{name}$"), [], f"{name} left running after {unit}'s SIGKILL")
        finally:
            quiet("docker", "rm", "-f", name)
        self.assertEqual(len(labelled), 1, f"{name} carried no hex.bench.run={unit} label")

    def test_a_stopped_units_containers_are_removed_by_the_scripts_trap(self):
        # Do: stop the unit mid-pass (SIGTERM), no ExecStopPost. Expect: host.sh's trap leaves no container of it.
        unit, name, _ = self.pass_in_unit("term", stop_post=False)
        try:
            quiet("systemctl", "--user", "stop", unit)
            until(lambda: self.ended(unit), f"{unit} ending")
            self.assertEqual(containers(f"name=^/{name}$"), [], f"{name} left running after {unit}'s stop")
        finally:
            quiet("docker", "rm", "-f", name)


if __name__ == "__main__":
    unittest.main()
