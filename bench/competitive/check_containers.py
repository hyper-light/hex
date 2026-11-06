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
import ctypes
import os
import re
import select
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
IMAGE = "registry.fedoraproject.org/fedora:44"

# seam_oci's stand-in: says it started (the fact the check waits on), then holds the pass where it is. The hold's 600 s
# is the stand-in's own end, not a measure: the check ends the unit long before it, and a check that dies leaves the
# pass to end by itself then. A wait on the pass ends when the fact holds or the unit's main process ends.
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


def until(fact, what, watch, ends=None, bound_s=None):
    """Waits for `fact()` to hold, looked at again each time `watch` (a descriptor that reads ready when the fact may
    have changed: docker's events, an inotify watch) says so, never on a clock's guess (Ada's 2026-11-06 rule). It
    fails naming `what` when `ends` (a pidfd: the unit whose work it waits for) reads ready first, or past `bound_s`,
    where the work's bound is systemd's own (a unit's stop timeout)."""
    end = None if bound_s is None else time.monotonic() + bound_s
    poller = select.poll()
    poller.register(watch, select.POLLIN)
    if ends is not None:
        poller.register(ends, select.POLLIN)
    while not fact():
        left = None if end is None else max(0, int((end - time.monotonic()) * 1000))
        ready = dict(poller.poll(left))
        if not ready:
            raise AssertionError(f"{what} didn't happen within its bound, {bound_s} s")
        if watch in ready:
            try:
                os.read(watch, 1 << 16)
            except BlockingIOError:
                pass
        if ends is not None and ends in ready and not fact():
            raise AssertionError(f"{what} didn't happen before its unit ended")


def unit_pidfd(unit):
    """A pidfd of the unit's main process, which reads ready once it has ended (pidfd_open(2))."""
    pid = int(quiet("systemctl", "--user", "show", "-p", "MainPID", "--value", unit).stdout.strip() or 0)
    if pid == 0:
        raise AssertionError(f"{unit} has no main process: it ended as it started")
    return os.pidfd_open(pid)


def stop_bound_s(unit):
    """How long systemd lets the unit's stop take (its TimeoutStopUSec, DefaultTimeoutStopSec unless set:
    systemd-system.conf(5)), read from the unit, in seconds: the bound of its ExecStop's work."""
    raw = quiet("systemctl", "--user", "show", "-p", "TimeoutStopUSec", "--value", unit).stdout.strip()
    total = timespan_s(raw)
    if not total:
        raise AssertionError(f"{unit}'s stop timeout {raw!r} isn't a bound")
    return total


def timespan_s(text):
    """A systemd time span as `systemctl show` writes one ("1min 30s", "90s", "500ms"), in seconds
    (systemd.time(7)); 0 for none ("infinity", empty)."""
    scale = {"h": 3600, "min": 60, "s": 1, "ms": 1e-3, "us": 1e-6}
    return sum(float(n) * scale[u] for n, u in re.findall(r"(\d+(?:\.\d+)?)(min|ms|us|s|h)\b", text))


def inotify_creates(directory):
    """A descriptor that reads ready when a file is made in `directory` (inotify(7), IN_CREATE 0x100 in
    include/uapi/linux/inotify.h)."""
    libc = ctypes.CDLL(None, use_errno=True)
    fd = libc.inotify_init1(os.O_NONBLOCK | os.O_CLOEXEC)
    if fd < 0 or libc.inotify_add_watch(fd, os.fsencode(directory), 0x100) < 0:
        raise OSError(ctypes.get_errno(), f"inotify on {directory}")
    return fd


def docker_events(since, *filters):
    """docker's events since `since` (its own clock's seconds) that match `filters`, as a stream: its stdout reads
    ready as one comes. The caller stops it."""
    args = ["docker", "events", "--since", str(since), "--format", "{{.Status}} {{.ID}}"]
    for f in filters:
        args += ["--filter", f]
    proc = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    os.set_blocking(proc.stdout.fileno(), False)
    return proc


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
        os.makedirs(out)
        made = inotify_creates(out)
        self.addCleanup(os.close, made)
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
        main = unit_pidfd(unit)
        self.addCleanup(os.close, main)
        until(lambda: os.path.exists(os.path.join(out, "seam.started")), "the pass's seam_oci starting", made, main)
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
        # its reaper removes the run's container (docker's destroy event, within the reaper's stop timeout), and it was
        # labelled with the run.
        unit, name, labelled = self.pass_in_unit("kill", reaper=True)
        try:
            cgroup = quiet("systemctl", "--user", "show", "-p", "ControlGroup", "--value", unit).stdout.strip()
            gone = docker_events(int(time.time()), f"container={name}", "event=destroy")
            try:
                with open(f"/sys/fs/cgroup{cgroup}/cgroup.kill", "w") as f:
                    f.write("1")
                # The reaper removes it as systemd stops the reaper, within the reaper's stop timeout.
                until(lambda: containers(f"name=^/{name}$") == [], f"{name} removed after {unit}'s SIGKILL",
                      gone.stdout.fileno(), bound_s=stop_bound_s(f"reap-{unit}"))
            finally:
                gone.kill()
                gone.wait()
        finally:
            quiet("docker", "rm", "-f", name)
        self.assertEqual(len(labelled), 1, f"{name} carried no hex.bench.run={unit} label")

    def test_a_stopped_units_containers_are_removed_by_the_scripts_trap(self):
        # Do: stop the unit mid-pass (SIGTERM), its run named so no reaper starts. Expect: host.sh's trap leaves no
        # container of it.
        unit, name, _ = self.pass_in_unit("term", reaper=False)
        try:
            # systemctl stop returns once the stop's job is done, the trap's removal within it.
            quiet("systemctl", "--user", "stop", unit)
            self.assertTrue(ended(unit), f"{unit} still runs after its stop")
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
        events = docker_events(int(time.time()), f"label=hexcheck.case={case}")
        self.addCleanup(events.wait)
        self.addCleanup(events.kill)
        started = subprocess.run(cmd, capture_output=True, text=True)
        self.assertEqual(started.returncode, 0, started.stderr)
        main = unit_pidfd(unit)
        self.addCleanup(os.close, main)
        until(lambda: containers(f"label=hexcheck.case={case}", "status=running"),
              f"{script}'s first container running", events.stdout.fileno(), main)
        labelled = containers(f"label=hexcheck.case={case}", f"label=hex.bench.run={unit}")
        cgroup = quiet("systemctl", "--user", "show", "-p", "ControlGroup", "--value", unit).stdout.strip()
        with open(f"/sys/fs/cgroup{cgroup}/cgroup.kill", "w") as f:
            f.write("1")
        # The reaper removes them as systemd stops the reaper, within the reaper's stop timeout.
        until(lambda: containers(f"label=hexcheck.case={case}") == [],
              f"{script}'s containers removed after its unit's SIGKILL", events.stdout.fileno(),
              bound_s=stop_bound_s(f"reap-{unit}"))
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


@unittest.skipIf(sys.platform != "linux", "needs Linux (pidfds)")
class Waits(unittest.TestCase):
    """The waits themselves, without docker or systemd: each ends on its fact, never on a clock's guess."""

    def test_a_wait_looks_again_when_its_watch_reads_ready(self):
        # Do: a fact that holds once its watch has been written to. Expect: the wait returns, the write read.
        r, w = os.pipe()
        self.addCleanup(os.close, r)
        self.addCleanup(os.close, w)
        os.set_blocking(r, False)
        seen = []
        os.write(w, b"x")
        until(lambda: seen.append(1) or len(seen) > 1, "the second look", r)
        self.assertEqual(len(seen), 2)

    def test_a_wait_fails_when_its_unit_ends_first(self):
        # Do: a fact that never holds, its unit's process ended. Expect: the wait fails, naming what didn't happen.
        r, w = os.pipe()
        self.addCleanup(os.close, r)
        self.addCleanup(os.close, w)
        child = subprocess.Popen(["true"])
        ends = os.pidfd_open(child.pid)
        self.addCleanup(os.close, ends)
        child.wait()
        with self.assertRaisesRegex(AssertionError, "never-fact didn't happen before its unit ended"):
            until(lambda: False, "never-fact", r, ends)

    def test_a_wait_bounded_by_a_stop_timeout_fails_past_it(self):
        # Do: a fact that never holds, with a bound and no unit. Expect: it fails past the bound, saying the bound.
        r, w = os.pipe()
        self.addCleanup(os.close, r)
        self.addCleanup(os.close, w)
        with self.assertRaisesRegex(AssertionError, "within its bound, 0.01 s"):
            until(lambda: False, "never-fact", r, bound_s=0.01)

    def test_systemds_time_spans_read_in_seconds(self):
        # Do: read systemctl show's spans. Expect: their seconds; none for infinity.
        self.assertEqual(timespan_s("1min 30s"), 90)
        self.assertEqual(timespan_s("90s"), 90)
        self.assertAlmostEqual(timespan_s("500ms"), 0.5)
        self.assertEqual(timespan_s("2h 1min"), 7260)
        self.assertEqual(timespan_s("infinity"), 0)


if __name__ == "__main__":
    unittest.main()
