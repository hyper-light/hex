#!/usr/bin/env python3
"""bench/competitive: one OCI runtime's lifecycle, throughput, CPU, memory and density.

Runs inside the bench container (see oci_inside.sh), as root, with the runtime's binary under /opt/bin
and the bundle at /run/bundle-sleep (workload `sleep infinity`) and /run/bundle-true (`true`). `libcrun` is
libcrun in process: each process that calls it starts its own libcrun-driver (libcrun/driver.c), which
makes and times each call the way crun's command line does, with no process spawned per call.

  oci_bench.py <runtime> <out.json> <n> <warmup> <throughput lifecycles> <density cap> <density budget MiB>

Phases, in order; a phase that fails records its error and the next one still runs:
  lifecycle   n sequential create, start, exec (when the runtime has one), delete --force of a `sleep`
              container; each operation timed alone (wall clock around the CLI call).
  lifecycle_nonet  the same, on a bundle without a network namespace, so create and delete are timed with
              and without one.
  start_true  n sequential create+start of a `true` container (AC-T7.1.3's measure), deleted untimed.
  throughput  create+start+delete --force of `true` containers at concurrency 1, 8 and 32: lifecycles/s,
              and CPU-seconds per lifecycle from the bench cgroup's cpu.stat. Each concurrency slot is a
              worker process of its own: CPython holds its interpreter lock across posix_spawn
              (Modules/posixmodule.c), so threads would spawn one at a time and cap the rate.
  sustained   the throughput lifecycle at concurrency 32 for HEXBENCH_SUSTAINED lifecycles (10,000): the
              rate in each window of 1,000, and the last window's against the first's. A short burst can't
              show decay under churn, which one production system saw fall from 110 to 45 starts/s over an
              hour (docs/research/runtime/oci-runtimes-and-fast-start.md §4.4).
  density     `sleep` containers started one after another until the cap or the memory budget, then
              memory per container from the bench cgroup: the memory it holds (cgmem.py), and its
              memory.current, which the budget bounds; all deleted afterwards. Beside the held figure, its
              memory.stat rise by entry (cgmem.breakdown) and the bytes under the runtime's state root, so a
              difference in held memory between runtimes is attributed to named entries
              (probes/held_attribution.py).
HEXBENCH_PHASES names the phases to run (all six by default).

Every CLI call's stdio goes to /dev/null or a reused error file, never a pipe: a detached `create` leaves
the container's init holding the caller's stdio, so reading a pipe to EOF would deadlock (hex_start's
lesson, commit 4c76c13). Every call has a timeout, so a hung runtime ends the phase with an error.
"""
import fcntl
import json
import os
import select
import signal
import sys
import multiprocessing as mp
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))  # cgmem.py, beside this file even under -I
import cgmem  # noqa: E402

CALL_TIMEOUT_S = 60
# The bench container's paths; the overrides exist so the driver's control flow can be checked against a
# stand-in runtime off the bench host (see the README).
CG = os.environ.get("HEXBENCH_CGROUP", "/sys/fs/cgroup")
BIN = os.environ.get("HEXBENCH_BIN", "/opt/bin")
ERR = os.environ.get("HEXBENCH_ERR", "/run/err")

RUNTIMES = {
    "hex": {"argv": [f"{BIN}/hex", "oci"], "env": {"HEX_OCI_ROOT": "/run/hex-oci"}, "exec": True,
            "state": "/run/hex-oci"},
    "runc": {"argv": [f"{BIN}/runc", "--root", "/run/runc"], "env": {}, "exec": True, "state": "/run/runc"},
    "crun": {"argv": [f"{BIN}/crun", "--root", "/run/crun"], "env": {}, "exec": True, "state": "/run/crun"},
    "youki": {"argv": [f"{BIN}/youki", "--root", "/run/youki"], "env": {}, "exec": True, "state": "/run/youki"},
    "runsc": {"argv": [f"{BIN}/runsc", "--root", "/run/runsc", "--network=none"], "env": {}, "exec": True,
              "state": "/run/runsc"},
    # crun's libkrun handler (Fedora's crun, named krun; fetch.py): each container a microVM, at crun's
    # defaults (vCPUs as the CPUs it may use, up to 16; 1024 MiB). Nothing can exec into its guest.
    "krun": {"argv": [f"{BIN}/krun", "--root", "/run/krun"], "env": {"LD_LIBRARY_PATH": "/opt/krun/lib"},
             "exec": False, "state": "/run/krun"},
    # libcrun in process, built from crun's pinned release (libcrun/build.sh).
    "libcrun": {"driver": f"{BIN}/libcrun-driver", "root": "/run/libcrun", "exec": True, "state": "/run/libcrun"},
}


# Container ids are fixed width, so none is a prefix of another: runsc (like docker) falls back to a
# prefix lookup when an exact id misses, and refuses an ambiguous one (fedora run 2, concurrency 32).


class OpError(Exception):
    pass


class Driver:
    """An in-process runtime's driver (libcrun/driver.c), started by the first call in the process that makes
    it: requests a line on its fd 3, replies a line on its fd 4, each call timed inside it. Its stdio is the
    CLI calls': /dev/null, and stderr to an error file."""

    def __init__(self, name, path, root):
        self.name, self.path, self.root, self.pid = name, path, root, None

    def start(self):
        req_r, self.req_w = os.pipe()
        self.rep_r, rep_w = os.pipe()
        # Its ends above 4, so neither dup2 onto 3 or 4 overwrites the other.
        ends = [fcntl.fcntl(fd, fcntl.F_DUPFD_CLOEXEC, 10) for fd in (req_r, rep_w)]
        os.close(req_r)
        os.close(rep_w)
        errfd = os.open(f"{ERR}/{self.name}.driver.{os.getpid()}.err", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
        try:
            actions = [(os.POSIX_SPAWN_OPEN, 0, os.devnull, os.O_RDONLY, 0),
                       (os.POSIX_SPAWN_OPEN, 1, os.devnull, os.O_WRONLY, 0),
                       (os.POSIX_SPAWN_DUP2, errfd, 2), (os.POSIX_SPAWN_DUP2, ends[0], 3),
                       (os.POSIX_SPAWN_DUP2, ends[1], 4)]
            self.pid = os.posix_spawn(self.path, [self.path, self.root], dict(os.environ), file_actions=actions)
        finally:
            for fd in ends + [errfd]:
                os.close(fd)

    def ask(self, *words):
        """One call: (wall ms, CPU s) as the driver timed it. A failed call, a driver that exits and one
        silent past CALL_TIMEOUT_S (then killed) each raise OpError."""
        if self.pid is None:
            self.start()
        try:
            os.write(self.req_w, (" ".join(words) + "\n").encode())
        except BrokenPipeError:
            raise OpError(f"{words[0]}: the driver exited {self.end(0)}") from None
        deadline, reply = time.monotonic() + CALL_TIMEOUT_S, b""
        while not reply.endswith(b"\n"):
            left = deadline - time.monotonic()
            if left <= 0 or not select.select([self.rep_r], [], [], left)[0]:
                raise OpError(f"{words[0]}: no reply within {CALL_TIMEOUT_S}s; the driver exited {self.end(0)}")
            chunk = os.read(self.rep_r, 4096)
            if not chunk:
                raise OpError(f"{words[0]}: the driver exited {self.end(CALL_TIMEOUT_S)}")
            reply += chunk
        kind, _, rest = reply.decode(errors="replace").strip().partition(" ")
        if kind != "ok":
            raise OpError(f"{words[0]}: {rest}")
        wall_ns, cpu_ns = rest.split()
        return int(wall_ns) / 1e6, int(cpu_ns) / 1e9

    def end(self, patience):
        """Ends the driver: EOF on its requests, then at most `patience` s before it's killed; its exit."""
        if self.pid is None:
            return None
        os.close(self.req_w)
        os.close(self.rep_r)
        pidfd = os.pidfd_open(self.pid)
        try:
            if not select.select([pidfd], [], [], patience)[0]:
                os.kill(self.pid, signal.SIGKILL)
        finally:
            os.close(pidfd)
        _, status = os.waitpid(self.pid, 0)
        self.pid = None
        return os.waitstatus_to_exitcode(status)


class Runtime:
    def __init__(self, name):
        spec = RUNTIMES[name]
        self.name = name
        self.argv = spec.get("argv")
        self.has_exec = spec["exec"]
        self.state = spec["state"]
        self.env = dict(os.environ, **spec.get("env", {}))
        self.driver = Driver(name, spec["driver"], spec["root"]) if "driver" in spec else None

    def call(self, *args, err_path=None):
        """Runs one CLI call; returns (wall ms, CPU s of the call). Raises OpError on failure.

        posix_spawn (vfork-class) and a blocking wait4 keep the harness's own cost and jitter out of
        the timing; a timer kills a call that outlives CALL_TIMEOUT_S."""
        errfd = os.open(err_path or f"{ERR}/{self.name}.err", os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o644)
        try:
            actions = [(os.POSIX_SPAWN_OPEN, 0, os.devnull, os.O_RDONLY, 0),
                       (os.POSIX_SPAWN_OPEN, 1, os.devnull, os.O_WRONLY, 0),
                       (os.POSIX_SPAWN_DUP2, errfd, 2)]
            t0 = time.perf_counter_ns()
            pid = os.posix_spawn(self.argv[0], self.argv + list(args), self.env, file_actions=actions)
            timer = threading.Timer(CALL_TIMEOUT_S, os.kill, (pid, signal.SIGKILL))
            timer.start()
            try:
                _, status, ru = os.wait4(pid, 0)
            finally:
                timer.cancel()
            ms = (time.perf_counter_ns() - t0) / 1e6
            code = os.waitstatus_to_exitcode(status)
            if code:
                os.lseek(errfd, 0, os.SEEK_SET)
                msg = os.read(errfd, 400).decode(errors="replace")
                why = f" (killed at the {CALL_TIMEOUT_S}s timeout)" if code == -signal.SIGKILL else ""
                raise OpError(f"{args[0]} exit {code}{why}: {msg}")
            return ms, ru.ru_utime + ru.ru_stime
        finally:
            os.close(errfd)

    def create(self, bundle, cid, **kw):
        if self.driver:
            return self.driver.ask("create", bundle, cid)
        return self.call("create", "--bundle", bundle, cid, **kw)

    def start(self, cid, **kw):
        if self.driver:
            return self.driver.ask("start", cid)
        return self.call("start", cid, **kw)

    def exec_true(self, cid, **kw):
        if self.driver:
            return self.driver.ask("exec", cid)
        return self.call("exec", cid, "/usr/bin/true", **kw)

    def delete(self, cid, **kw):
        if self.driver:
            return self.driver.ask("delete", cid)
        return self.call("delete", "--force", cid, **kw)

    def close(self):
        """Ends an in-process runtime's driver, which has nothing left to do once the process's calls are
        done; a CLI runtime has none."""
        if self.driver:
            self.driver.end(CALL_TIMEOUT_S)


def cg_read(name):
    with open(os.path.join(CG, name)) as f:
        return f.read()


def cpu_usec():
    for line in cg_read("cpu.stat").splitlines():
        k, v = line.split()
        if k == "usage_usec":
            return int(v)
    raise OpError("cpu.stat has no usage_usec")


def mem_bytes():
    return int(cg_read("memory.current"))


def phase_lifecycle(rt, n, warmup, bundle="/run/bundle-sleep", tag="l"):
    ops = {"create": [], "start": [], "exec": [], "delete": []}
    cpu = {"create": [], "start": [], "exec": [], "delete": []}
    for i in range(warmup + n):
        cid = f"{rt.name}-{tag}{i:06d}"
        timed = [("create", lambda: rt.create(bundle, cid)), ("start", lambda: rt.start(cid))]
        if rt.has_exec:
            timed.append(("exec", lambda: rt.exec_true(cid)))
        created = False
        try:
            for op, fn in timed:
                ms, c = fn()
                created = True
                if i >= warmup:
                    ops[op].append(ms)
                    cpu[op].append(c)
        finally:
            if created:
                ms, c = rt.delete(cid)
        if i >= warmup:
            ops["delete"].append(ms)
            cpu["delete"].append(c)
    if not rt.has_exec:
        ops.pop("exec")
        cpu.pop("exec")
    return {"ms": ops, "cli_cpu_s": cpu}


def phase_start_true(rt, n, warmup):
    ms_all = []
    for i in range(warmup + n):
        cid = f"{rt.name}-t{i:06d}"
        a, _ = rt.create("/run/bundle-true", cid)
        try:
            b, _ = rt.start(cid)
        finally:
            rt.delete(cid)
        if i >= warmup:
            ms_all.append(a + b)
    return {"ms": ms_all}


def throughput_lifecycle(rt, conc, i):
    cid = f"{rt.name}-c{conc:02d}-{i:06d}"
    err = f"{ERR}/{rt.name}.c{conc}.{i}.err"
    rt.create("/run/bundle-true", cid, err_path=err)
    try:
        rt.start(cid, err_path=err)
    finally:
        rt.delete(cid, err_path=err)
    if not rt.driver:  # a CLI call's error file; a driver's errors come back in its replies
        os.unlink(err)


def throughput_worker(name, conc, lifecycles, claimed, start, failed, out):
    """One concurrency slot: after the start barrier, claims lifecycle numbers until none are left or
    another slot has failed, then sends {"done": [each lifecycle's end, perf_counter]} or {"error": why}.
    Only the claim takes the shared lock."""
    rt = Runtime(name)
    done = []
    try:
        if rt.driver:  # up before the clock, as the interpreter is
            rt.driver.start()
        start.wait(timeout=CALL_TIMEOUT_S)
        while not failed.is_set():
            with claimed.get_lock():
                i = claimed.value
                if i >= lifecycles:
                    break
                claimed.value = i + 1
            throughput_lifecycle(rt, conc, i)
            done.append(time.perf_counter())
        out.send({"done": done})
    except Exception as e:  # recorded in the parent; the level stops
        failed.set()
        out.send({"error": f"{type(e).__name__}: {e}", "done": done})
    finally:
        out.close()
        rt.close()


def run_level(rt, conc, lifecycles):
    """`lifecycles` throughput lifecycles from `conc` worker processes: (wall s from the start barrier,
    the bench cgroup's CPU s over it, the sorted end times relative to the start)."""
    ctx = mp.get_context("spawn")
    claimed, start, failed = ctx.Value("q", 0), ctx.Barrier(conc + 1), ctx.Event()
    workers, results = [], []
    try:
        for _ in range(conc):
            a, b = ctx.Pipe(duplex=False)
            w = ctx.Process(target=throughput_worker, args=(rt.name, conc, lifecycles, claimed, start, failed, b))
            w.start()
            b.close()  # only the worker holds the sending end, so one that dies ends recv() with EOFError
            workers.append((w, a))
        # Every slot is up before the clock starts: a worker's interpreter start-up isn't a lifecycle.
        try:
            start.wait(timeout=CALL_TIMEOUT_S)
        except threading.BrokenBarrierError:
            raise OpError(f"concurrency {conc}: a worker wasn't up within {CALL_TIMEOUT_S}s") from None
        cpu0, t0 = cpu_usec(), time.perf_counter()
        for w, a in workers:
            try:
                results.append(a.recv())
            except EOFError:
                results.append({"error": f"a worker exited {w.exitcode} without a result"})
        wall = time.perf_counter() - t0
        cpu_s = (cpu_usec() - cpu0) / 1e6
    finally:
        # No worker outlives its level: each call has its own timeout, so one past this is stuck.
        for w, _ in workers:
            w.join(timeout=CALL_TIMEOUT_S)
            if w.is_alive():
                w.kill()
                w.join()
    errs = [r["error"] for r in results if "error" in r]
    if errs:
        count = sum(len(r.get("done", [])) for r in results)
        raise OpError(f"concurrency {conc}: {errs[0]} (after {count} of {lifecycles} lifecycles)")
    return wall, cpu_s, sorted(t - t0 for r in results for t in r["done"])


def phase_throughput(rt, lifecycles):
    out = {}
    for conc in (1, 8, 32):
        wall, cpu_s, _ = run_level(rt, conc, lifecycles)
        out[str(conc)] = {"lifecycles": lifecycles, "wall_s": wall, "per_s": lifecycles / wall,
                          "cpu_s_per_lifecycle": cpu_s / lifecycles}
    return out


SUSTAINED_WINDOW = 1000


def phase_sustained(rt, lifecycles):
    wall, cpu_s, ends = run_level(rt, 32, lifecycles)
    # Each window's rate from the end of the window before it (the start, for the first) to its own last end.
    windows, prev = [], 0.0
    for k in range(SUSTAINED_WINDOW - 1, len(ends), SUSTAINED_WINDOW):
        windows.append(SUSTAINED_WINDOW / (ends[k] - prev))
        prev = ends[k]
    return {"concurrency": 32, "lifecycles": lifecycles, "wall_s": wall, "per_s": lifecycles / wall,
            "cpu_s_per_lifecycle": cpu_s / lifecycles, "window": SUSTAINED_WINDOW, "window_per_s": windows,
            "last_over_first": windows[-1] / windows[0] if len(windows) > 1 else None}


def phase_density(rt, cap, budget_mib):
    dying = cgmem.settle([CG], CALL_TIMEOUT_S)
    base, snap0 = mem_bytes(), cgmem.snapshot(CG)
    held0, state0 = cgmem.held(snap0), cgmem.tree_bytes(rt.state)
    budget = budget_mib * 1024 * 1024
    created, running = [], 0
    t0 = time.perf_counter()
    stop_reason = "cap"
    try:
        for i in range(cap):
            cid = f"{rt.name}-d{i:06d}"
            try:
                rt.create("/run/bundle-sleep", cid)
                created.append(cid)
                rt.start(cid)
                running += 1
            except OpError as e:
                stop_reason = f"error at {i}: {e}"
                break
            if mem_bytes() - base > budget:
                stop_reason = "memory budget"
                break
        wall = time.perf_counter() - t0
        time.sleep(2)  # let the started inits settle before reading memory
        used = mem_bytes() - base
        snap = cgmem.snapshot(CG)
        held = cgmem.held(snap) - held0
        state = cgmem.tree_bytes(rt.state)
    finally:
        for cid in created:
            try:
                rt.delete(cid)
            except OpError:
                pass
    count = running
    return {"count": count, "stop": stop_reason, "start_wall_s": wall, "memory_bytes": used, "dying_at_start": dying,
            "memory_per_container_bytes": used / count if count else None,
            "containers_per_gib": count / (used / 2**30) if used > 0 else None,
            "held": cgmem.per_unit(held, count),
            # What the held rise is made of (memory.stat at the peak less at the baseline, one reading each with
            # the held figures above), and the runtime's state root at both.
            "stat_rise": cgmem.rise(cgmem.breakdown(snap), cgmem.breakdown(snap0)),
            "state_root": {"base": state0, "peak": state}}


def main():
    name, out_path = sys.argv[1], sys.argv[2]
    n, warmup, lifecycles, cap, budget = (int(x) for x in sys.argv[3:8])
    rt = Runtime(name)
    result = {"runtime": name, "n": n, "warmup": warmup, "phases": {}, "errors": {}}
    wanted = (os.environ.get("HEXBENCH_PHASES")
              or "lifecycle lifecycle_nonet start_true throughput sustained density").split()
    sustained = int(os.environ.get("HEXBENCH_SUSTAINED", "10000"))
    for phase, fn in (("lifecycle", lambda: phase_lifecycle(rt, n, warmup)),
                      ("lifecycle_nonet", lambda: phase_lifecycle(rt, n, warmup, "/run/bundle-sleep-nonet", "n")),
                      ("start_true", lambda: phase_start_true(rt, n, warmup)),
                      ("throughput", lambda: phase_throughput(rt, lifecycles)),
                      ("sustained", lambda: phase_sustained(rt, sustained)),
                      ("density", lambda: phase_density(rt, cap, budget))):
        if phase not in wanted:
            continue
        t0 = time.perf_counter()
        try:
            result["phases"][phase] = fn()
        except OpError as e:
            result["errors"][phase] = str(e)
        sys.stdout.write(f"{name} {phase}: {time.perf_counter() - t0:.0f}s"
                         f"{' ERROR ' + result['errors'][phase][:200] if phase in result['errors'] else ''}\n")
        sys.stdout.flush()
    rt.close()
    with open(out_path, "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
