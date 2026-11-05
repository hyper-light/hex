#!/usr/bin/env python3
"""bench/competitive: a bench run's containers end with it, however it ends (CLAUDE.md sections 2 and 3: a container
a run leaves running holds CPU and memory past its run, under every later run's timed parts; CP-T7.1's readings).

  1. host.sh's OCI pass in a systemd unit, every process of it SIGKILLed mid-pass (its cgroup.kill, as systemd's
     SIGKILL and the kernel's OOM kill of a cgroup do): once the unit and its reaper (reap-<unit>) have ended, no
     container of the run is left, and while it ran its container was labelled with the run (hex.bench.run=<unit>).
     Not an ExecStopPost: Linux 6.19 SIGKILLs a process cloned into a cgroup that was cgroup.kill'ed (fixed upstream,
     8e3599202166), and systemd starts ExecStopPost in the unit's cgroup, so after a SIGKILL it never ran.
  2. The same pass stopped (SIGTERM), with no reaper: host.sh's own trap removes its container.
  3. bench/oci-validation's cgroups.sh, run.sh, smoke.sh and docker_lab.sh, each in a unit SIGKILLed (cgroup.kill)
     once its first container runs: no container it started is left, and that one carried hex.bench.run=<unit>.
     Their containers' own commands are a whole lab each, so a docker stand-in on PATH runs each `docker run` as the
     script asks, labels and all, with its entrypoint a holder that sleeps and one label more, the check's own, by
     which a container the script started is found though the script labels none; every other docker command is
     docker's own.

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


def ended(unit):
    return quiet("systemctl", "--user", "is-active", unit).stdout.strip() not in ("active", "activating",
                                                                                 "deactivating")


def containers(*filters):
    args = ["docker", "ps", "-aq"]
    for f in filters:
        args += ["--filter", f]
    return quiet(*args).stdout.split()


@unittest.skipIf(why_not(), why_not() or "")
class RunEnd(unittest.TestCase):
    def pass_in_unit(self, how, reaper):
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
        if not reaper:
            env["HEXBENCH_RUN"] = unit  # a run its caller names starts no reaper of its own: the trap alone
        cmd = ["systemd-run", "--user", "--quiet", f"--unit={unit}", "--collect"]
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

    def clear(self, tmp, unit):
        quiet("systemctl", "--user", "kill", "-s", "SIGKILL", unit)
        quiet("systemctl", "--user", "reset-failed", unit)
        quiet("systemctl", "--user", "stop", f"reap-{unit}")
        for c in containers(f"label=hex.bench.run={unit}"):
            quiet("docker", "rm", "-f", c)
        # oci_inside.sh writes as root into the run's directory: cleared from a container of this check's own.
        quiet("docker", "run", "--rm", "--label", f"hex.bench.run={unit}", "-v", f"{tmp}:/t", IMAGE,
              "find", "/t", "-mindepth", "1", "-delete")
        shutil.rmtree(tmp, ignore_errors=True)

    def test_a_sigkilled_units_containers_are_removed_by_its_reaper(self):
        # Do: SIGKILL every process of the unit at once mid-pass (its cgroup.kill; no trap of host.sh's runs). Expect:
        # once the unit and its reaper have ended, no container of the run is left, and it was labelled with the run.
        unit, name, labelled = self.pass_in_unit("kill", reaper=True)
        try:
            cgroup = quiet("systemctl", "--user", "show", "-p", "ControlGroup", "--value", unit).stdout.strip()
            with open(f"/sys/fs/cgroup{cgroup}/cgroup.kill", "w") as f:
                f.write("1")
            until(lambda: ended(unit) and ended(f"reap-{unit}"), f"{unit} and its reaper ending")
            self.assertEqual(containers(f"name=^/{name}$"), [], f"{name} left running after {unit}'s SIGKILL")
        finally:
            quiet("docker", "rm", "-f", name)
        self.assertEqual(len(labelled), 1, f"{name} carried no hex.bench.run={unit} label")

    def test_a_stopped_units_containers_are_removed_by_the_scripts_trap(self):
        # Do: stop the unit mid-pass (SIGTERM), its run named so no reaper starts. Expect: host.sh's trap leaves no
        # container of it.
        unit, name, _ = self.pass_in_unit("term", reaper=False)
        try:
            quiet("systemctl", "--user", "stop", unit)
            until(lambda: ended(unit), f"{unit} ending")
            self.assertEqual(containers(f"name=^/{name}$"), [], f"{name} left running after {unit}'s stop")
        finally:
            quiet("docker", "rm", "-f", name)


# docker's stand-in for the oci-validation scripts: `docker run` with the check's label and a holder for its entrypoint,
# so the container the script asks for runs until it's ended; anything else is docker's own.
DOCKER = """#!/usr/bin/env bash
if [ "$1" = run ]; then
  shift
  exec "$REAL_DOCKER" run --label "hexcheck.case=$HEXCHECK_CASE" -v "$HEXCHECK_HOLD:/opt/hexcheck-hold:ro" \\
    --entrypoint /opt/hexcheck-hold "$@"
fi
exec "$REAL_DOCKER" "$@"
"""


@unittest.skipIf(why_not(), why_not() or "")
class OciValidation(unittest.TestCase):
    def killed(self, script, args):
        base = os.path.join(os.path.expanduser("~/.cache"), "hex-bench-checks")
        os.makedirs(base, exist_ok=True)
        tmp = tempfile.mkdtemp(prefix="ocival.", dir=base)
        case = f"{os.getpid()}-{script.split('.')[0]}"
        unit = f"hexcheck-ocival-{case}"
        self.addCleanup(self.clear, tmp, unit, case)
        stubs = os.path.join(tmp, "stubs")
        os.makedirs(stubs)
        for path, text in ((os.path.join(stubs, "docker"), DOCKER),
                           (os.path.join(tmp, "hold"), "#!/bin/sh\nexec sleep 600\n"),
                           (os.path.join(tmp, "hex"), "#!/bin/sh\nexit 1\n")):
            with open(path, "w") as f:
                f.write(text)
            os.chmod(path, 0o755)
        env = {"PATH": stubs + os.pathsep + os.environ["PATH"], "REAL_DOCKER": shutil.which("docker"),
               "HEXCHECK_CASE": case, "HEXCHECK_HOLD": os.path.join(tmp, "hold")}
        cmd = ["systemd-run", "--user", "--quiet", f"--unit={unit}", "--collect", f"--working-directory={tmp}"]
        cmd += [f"--setenv={k}={v}" for k, v in env.items()]
        cmd += ["bash", os.path.join(os.path.dirname(HERE), "oci-validation", script)]
        cmd += [a.replace("{tmp}", tmp) for a in args]
        started = subprocess.run(cmd, capture_output=True, text=True)
        self.assertEqual(started.returncode, 0, started.stderr)
        until(lambda: containers(f"label=hexcheck.case={case}", "status=running"),
              f"{script}'s first container running")
        labelled = containers(f"label=hexcheck.case={case}", f"label=hex.bench.run={unit}")
        cgroup = quiet("systemctl", "--user", "show", "-p", "ControlGroup", "--value", unit).stdout.strip()
        with open(f"/sys/fs/cgroup{cgroup}/cgroup.kill", "w") as f:
            f.write("1")
        until(lambda: ended(unit) and ended(f"reap-{unit}"), f"{unit} and its reaper ending")
        self.assertEqual(containers(f"label=hexcheck.case={case}"), [],
                         f"{script}'s containers left after its unit's SIGKILL")
        self.assertNotEqual(labelled, [], f"{script}'s container carried no hex.bench.run={unit} label")

    def clear(self, tmp, unit, case):
        quiet("systemctl", "--user", "kill", "-s", "SIGKILL", unit)
        quiet("systemctl", "--user", "reset-failed", unit)
        quiet("systemctl", "--user", "stop", f"reap-{unit}")
        for c in containers(f"label=hexcheck.case={case}"):
            quiet("docker", "rm", "-f", c)
        shutil.rmtree(tmp, ignore_errors=True)

    def test_cgroups_sh(self):
        # Do: cgroups.sh's unit SIGKILLed once its container runs. Expect: none of its containers left; labelled.
        self.killed("cgroups.sh", ["{tmp}/hex", IMAGE])

    def test_run_sh(self):
        # Do: run.sh's unit SIGKILLed once its container runs. Expect: none of its containers left; labelled.
        self.killed("run.sh", ["{tmp}", "{tmp}/hex", "{tmp}/out", IMAGE])

    def test_smoke_sh(self):
        # Do: smoke.sh's unit SIGKILLed once its container runs. Expect: none of its containers left; labelled.
        self.killed("smoke.sh", ["{tmp}/hex", IMAGE, "{tmp}/work"])

    def test_docker_lab_sh(self):
        # Do: docker_lab.sh's unit (its source volume a commit's, kept if it was there before) SIGKILLed once its
        # first container (the lab image's build, or the source's copy) runs. Expect: none of its containers left;
        # labelled.
        sha = quiet("git", "-C", HERE, "rev-parse", "--short", "HEAD").stdout.strip()
        self.assertTrue(sha, "docker_lab.sh needs a git checkout of the repository")
        volume = f"hexlab-src-{sha}"
        had = quiet("docker", "volume", "inspect", volume).returncode == 0
        if not had:
            self.addCleanup(quiet, "docker", "volume", "rm", volume)
        self.killed("docker_lab.sh", [sha, "oci_box"])


if __name__ == "__main__":
    unittest.main()
