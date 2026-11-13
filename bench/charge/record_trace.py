#!/usr/bin/env python3
"""Records a real harness session's load as a charge trace, the input hex-cli's charge_fleet replays (joint plan step
14, AC-T7.4.1; docs/design/learned-charge.md, "what ROOT measures"): the session runs in a cgroup of its own, and each
window its CPU (`cpu.stat`'s usage_usec) and its memory (`memory.current`) become `millis threads mib` steps.

The window is derived, not picked: the session cgroup's CFS period, the second field of its `cpu.max`, the period Hex
writes into every `cpu.max` and so its Linux charge learner's window (hex-node's WINDOW is
hex_core::cgroup::CPU_PERIOD_US): the trace holds what the learner reads of a VM, no finer and no coarser. Each window
becomes at most two steps that keep its CPU to the millisecond: so many whole busy threads, and one more for the rest
(a window that used 230 ms of CPU in 100 ms is `30 3 m` then `70 2 m`). Time and CPU are carried from window to
window, so neither rounds away over a trace; memory is the window's reading, rounded up to a MiB.

Run on Linux (cgroup v2), inside a cgroup delegated to it, as the driver runs:
  systemd-run --user --scope -p Delegate=yes -- python3 -B -I bench/charge/record_trace.py OUT -- COMMAND...
systemd-run expands `$NAME` in its arguments, so a COMMAND with a `$` in it goes in a script of its own (or has `$$`
for each `$`).
It moves itself into a leaf of its cgroup, `recorder`, starts COMMAND (the harness session: `claude -p ...`,
`codex exec ...`, with its own task) in a sibling leaf, `session`, and records until COMMAND has ended and the
session's cgroup is empty: the session's whole tree, the harness, its tools and the builds and tests it runs. OUT, a
new file, starts with a header naming the host, the command, when it began and the window; an interrupt (SIGINT,
SIGTERM) ends the record there, its steps kept. The trace must outlast the riser's warm-up (SEEDABLE windows) for the
driver to take it: with 100 ms windows, 100 s.

Test: python3 -B -I bench/charge/record_trace_test.py
"""
import datetime
import os
import platform
import shlex
import signal
import subprocess
import sys
import time

MIB = 1 << 20


class Steps:
    """Turns a session's readings since its start into steps, carrying whole milliseconds of time and of CPU from
    each window to the next, so neither rounds away."""

    def __init__(self):
        self.ms = 0
        self.busy = 0

    def window(self, elapsed_us, cpu_us, mem_bytes):
        """The steps of the time since the last window: `elapsed_us` and `cpu_us` are the session's since its start,
        `mem_bytes` what it holds now. None where no whole millisecond has passed: it is carried to the next."""
        ms = elapsed_us // 1000 - self.ms
        if ms <= 0:
            return []
        busy = max(cpu_us // 1000 - self.busy, 0)
        self.ms += ms
        self.busy += busy
        whole, rest = divmod(busy, ms)
        mib = -(-mem_bytes // MIB)
        out = []
        if rest:
            out.append((rest, whole + 1, mib))
        if ms - rest:
            out.append((ms - rest, whole, mib))
        return out


def own_cgroup():
    """This process's cgroup directory (cgroup v2's one line in /proc/self/cgroup)."""
    with open("/proc/self/cgroup") as f:
        line = f.read().strip()
    if not line.startswith("0::/"):
        raise SystemExit(f"not on cgroup v2: /proc/self/cgroup says {line!r}")
    return os.path.join("/sys/fs/cgroup", line[len("0::/"):])


def read(path):
    with open(path) as f:
        return f.read()


def usage_us(cgroup):
    for line in read(os.path.join(cgroup, "cpu.stat")).splitlines():
        name, value = line.split()
        if name == "usage_usec":
            return int(value)
    raise SystemExit(f"{cgroup}/cpu.stat has no usage_usec")


def populated(cgroup):
    for line in read(os.path.join(cgroup, "cgroup.events")).splitlines():
        name, value = line.split()
        if name == "populated":
            return value == "1"
    raise SystemExit(f"{cgroup}/cgroup.events has no populated")


def main(argv):
    if len(argv) < 3 or argv[1] != "--":
        raise SystemExit("usage: record_trace.py OUT -- COMMAND...")
    out, command = argv[0], argv[2:]
    if os.path.exists(out):
        raise SystemExit(f"{out} exists: a record starts a new file")
    own = own_cgroup()
    recorder, session = os.path.join(own, "recorder"), os.path.join(own, "session")
    for leaf in (recorder, session):
        os.makedirs(leaf, exist_ok=False)
    with open(os.path.join(recorder, "cgroup.procs"), "w") as f:
        f.write(str(os.getpid()))
    with open(os.path.join(own, "cgroup.subtree_control"), "w") as f:
        f.write("+cpu +memory")
    period_us = int(read(os.path.join(session, "cpu.max")).split()[1])

    def into_session():
        with open(os.path.join(session, "cgroup.procs"), "w") as f:
            f.write(str(os.getpid()))

    stop = []
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda *_: stop.append(True))
    steps = Steps()
    with open(out, "x") as trace:
        began = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
        trace.write(
            f"# source: a harness session on {platform.node()} ({platform.system()} {platform.release()}), "
            f"recorded by bench/charge/record_trace.py from {began}\n"
            f"# command: {shlex.join(command)}\n"
            f"# window: {period_us} us, its cgroup's cpu.max period (Hex's Linux learner's window)\n"
            f"# lines: millis threads mib\n"
        )
        start = time.monotonic_ns()
        child = subprocess.Popen(command, preexec_fn=into_session)
        tick = start
        while not stop:
            tick += period_us * 1000
            time.sleep(max(tick - time.monotonic_ns(), 0) / 1e9)
            now = time.monotonic_ns()
            cpu, mem = usage_us(session), int(read(os.path.join(session, "memory.current")))
            for ms, threads, mib in steps.window((now - start) // 1000, cpu, mem):
                trace.write(f"{ms} {threads} {mib}\n")
            trace.flush()
            if child.poll() is not None and not populated(session):
                break
        trace.write(f"# ended: {steps.ms} ms, {steps.busy} ms of CPU, command exit {child.poll()}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
