#!/usr/bin/env python3
"""Records a real harness session's load as a charge trace, the input hex-cli's charge_fleet replays (joint plan step
14, AC-T7.4.1; docs/design/learned-charge.md, "what ROOT measures"): the session runs in a cgroup of its own, and each
window its CPU (`cpu.stat`'s usage_usec) and its memory (`memory.current`) become `millis threads mib` steps.

The window is derived, not picked: the CFS period of the session cgroup's `cpu.max`, its second field (its nearest
ancestor's where the cpu controller isn't enabled for it), the period Hex writes into every `cpu.max` and so its Linux
charge learner's window (hex-node's WINDOW is
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

Or attach to a session already running in a cgroup of its own, a harness under kiln's runner among them, passively:
  python3 -B -I bench/charge/record_trace.py OUT --attach CGROUP
CGROUP is its cgroup as `/proc/<pid>/cgroup` (or `systemctl show -p ControlGroup`) names it, rooted at the cgroup
mount, or its directory under /sys/fs/cgroup. It moves no
process and writes no cgroup file: it reads the cgroup's `cpu.stat` and `cgroup.events` each window, from the attach
on, until the cgroup empties or is removed, and its memory: `memory.current` where its parent gives it the memory
controller, else its processes' own: the sum over the `cgroup.procs` of the cgroup and every cgroup below it (its CPU
and its life are its subtree's too) of each one's proportional set (`/proc/<pid>/smaps_rollup`'s Pss: a page shared by
n processes counted 1/n in each, so the sum counts it once; or its `status`'s VmRSS where that can't be read), a process
that ends within a window out of the sum (kiln's per-agent cgroups have cpu but no memory controller; ROOT, 2026-11-13;
the review's E-9). OUT's header names which (`memory: cgroup` or `memory: procs-pss`), so its replay knows what it holds:
a cgroup's memory counts its page cache and kernel memory too, its processes' proportional sets their own pages. A cgroup that has emptied, or a path that is no cgroup, is refused,
naming it. OUT's header names the cgroup, never a command line: a session's arguments are its own.

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


def cgroup_memory(cgroup):
    """What the cgroup holds, its memory controller's count (`memory.current`), in bytes."""
    return int(read(os.path.join(cgroup, "memory.current")))


def kib(text, name):
    """The `name:` line's size in `text`, as /proc writes it in kB, in bytes; none where no line names it."""
    for line in text.splitlines():
        if line.startswith(name + ":"):
            return int(line.split()[1]) * 1024
    return None


def process_pss(pid, proc="/proc"):
    """What process `pid` holds resident, its share of each page it shares, in bytes: its smaps_rollup's Pss, or its
    status's VmRSS where that can't be read (no smaps_rollup, or not this reader's to read: its shared pages then counted
    whole); none once it has ended."""
    try:
        pss = kib(read(os.path.join(proc, str(pid), "smaps_rollup")), "Pss")
        if pss is not None:
            return pss
    except (FileNotFoundError, PermissionError, ProcessLookupError):
        pass
    try:
        return kib(read(os.path.join(proc, str(pid), "status")), "VmRSS") or 0
    except (FileNotFoundError, ProcessLookupError):
        return None


def subtree_procs(cgroup):
    """The processes of `cgroup` and of every cgroup below it, as their `cgroup.procs` list them: a process is in one
    cgroup, so none is listed twice; a cgroup removed while it is walked lists none."""
    pids = []
    for here, _, files in os.walk(cgroup):
        if "cgroup.procs" not in files:
            continue
        try:
            pids.extend(read(os.path.join(here, "cgroup.procs")).split())
        except FileNotFoundError:
            continue
    return pids


def procs_memory(cgroup, proc="/proc"):
    """What the cgroup's subtree's processes hold, in bytes: the sum over them of each one's proportional set, a process
    that ended since its listing out of it (the review's E-9: the subtree, as its CPU is, and Pss, so a shared page is
    counted once)."""
    return sum(pss for pss in (process_pss(pid, proc) for pid in subtree_procs(cgroup)) if pss is not None)


def populated(cgroup):
    for line in read(os.path.join(cgroup, "cgroup.events")).splitlines():
        name, value = line.split()
        if name == "populated":
            return value == "1"
    raise SystemExit(f"{cgroup}/cgroup.events has no populated")


def period_us(cgroup, root="/sys/fs/cgroup"):
    """The CFS period at `cgroup`, in microseconds: the second field of the nearest `cpu.max` on its path up to `root`,
    its own where the cpu controller is enabled for it, else its nearest ancestor's. Refused, naming the cgroup, where
    no cgroup on its path has one."""
    at, top = os.path.realpath(cgroup), os.path.realpath(root)
    while True:
        path = os.path.join(at, "cpu.max")
        if os.path.exists(path):
            return int(read(path).split()[1])
        if at == top or os.path.dirname(at) == at:
            raise SystemExit(f"no cgroup on {cgroup}'s path has a cpu.max to take the window from")
        at = os.path.dirname(at)


def cgroup_dir(named, root="/sys/fs/cgroup"):
    """The directory of the cgroup `named`: as `/proc/<pid>/cgroup` names it, rooted at the mount `root` (its leading
    slash or none), or its directory under `root` itself."""
    if named == root or named.startswith(root.rstrip("/") + "/"):
        return named
    return os.path.join(root, named.lstrip("/"))


def sleep_until_ns(deadline_ns):
    time.sleep(max(deadline_ns - time.monotonic_ns(), 0) / 1e9)


def record(trace, cgroup, period, start, begun, now_ns, sleep_until, stop, ended, memory=cgroup_memory):
    """Each window of `period` microseconds from `start` (ns), the cgroup's CPU past `begun` (its `usage_us` then) and
    its memory (`memory(cgroup)`, in bytes) as steps into `trace`, until `ended()` says why the record ends, the cgroup
    is gone, or `stop` holds an interrupt."""
    steps = Steps()
    tick, why = start, "interrupted"
    while not stop:
        tick += period * 1000
        sleep_until(tick)
        now = now_ns()
        try:
            cpu, mem = usage_us(cgroup), memory(cgroup)
        except FileNotFoundError:
            why = "the cgroup is gone"
            break
        for ms, threads, mib in steps.window((now - start) // 1000, cpu - begun, mem):
            trace.write(f"{ms} {threads} {mib}\n")
        trace.flush()
        try:
            why = ended()
        except FileNotFoundError:
            why = "the cgroup is gone"
        if why:
            break
    trace.write(f"# ended: {steps.ms} ms, {steps.busy} ms of CPU, {why}\n")


def interrupts():
    """A list an interrupt (SIGINT, SIGTERM) is put in: the record ends there, its steps kept."""
    stop = []
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda *_: stop.append(True))
    return stop


def attach(out, cgroup, root="/sys/fs/cgroup", now_ns=time.monotonic_ns, sleep_until=sleep_until_ns, proc="/proc"):
    """Records the session running in `cgroup` into `out` (module docs), passively, until the cgroup empties: its memory
    the cgroup's where its parent gives it the memory controller, else its processes' resident sets (read under
    `proc`)."""
    if not os.path.exists(os.path.join(cgroup, "cgroup.events")):
        raise SystemExit(f"{cgroup} is no cgroup: it has no cgroup.events")
    if not populated(cgroup):
        raise SystemExit(f"{cgroup} has emptied: nothing runs there to record")

    def procs(at):
        return procs_memory(at, proc)

    if os.path.exists(os.path.join(cgroup, "memory.current")):
        memory, source = cgroup_memory, "cgroup, its memory.current"
    else:
        memory = procs
        source = (
            "procs-pss, the sum over its subtree's cgroup.procs of each one's smaps_rollup Pss (else status VmRSS): "
            "its parent gives it no memory controller"
        )
    period = period_us(cgroup, root)
    if os.path.exists(out):
        raise SystemExit(f"{out} exists: a record starts a new file")
    stop = interrupts()
    with open(out, "x") as trace:
        began = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
        trace.write(
            f"# source: a harness session on {platform.node()} ({platform.system()} {platform.release()}), attached "
            f"to the cgroup {cgroup} by bench/charge/record_trace.py from {began}\n"
            f"# window: {period} us, the CFS period of the nearest cpu.max on its path (Hex's Linux learner's window)\n"
            f"# memory: {source}\n"
            f"# lines: millis threads mib\n"
        )
        start, begun = now_ns(), usage_us(cgroup)
        record(trace, cgroup, period, start, begun, now_ns, sleep_until, stop,
               lambda: None if populated(cgroup) else "the cgroup emptied", memory)


def main(argv):
    if len(argv) == 3 and argv[1] == "--attach":
        attach(argv[0], cgroup_dir(argv[2]))
        return 0
    if len(argv) < 3 or argv[1] != "--":
        raise SystemExit("usage: record_trace.py OUT -- COMMAND...  |  record_trace.py OUT --attach CGROUP")
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
    period = period_us(session, own)

    def into_session():
        with open(os.path.join(session, "cgroup.procs"), "w") as f:
            f.write(str(os.getpid()))

    stop = interrupts()
    with open(out, "x") as trace:
        began = datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")
        trace.write(
            f"# source: a harness session on {platform.node()} ({platform.system()} {platform.release()}), "
            f"recorded by bench/charge/record_trace.py from {began}\n"
            f"# command: {shlex.join(command)}\n"
            f"# window: {period} us, its cgroup's cpu.max period (Hex's Linux learner's window)\n"
            f"# memory: cgroup, its memory.current\n"
            f"# lines: millis threads mib\n"
        )
        start, begun = time.monotonic_ns(), usage_us(session)
        child = subprocess.Popen(command, preexec_fn=into_session)

        def ended():
            if child.poll() is not None and not populated(session):
                return f"command exit {child.poll()}"
            return None

        record(trace, session, period, start, begun, time.monotonic_ns, sleep_until_ns, stop, ended)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
