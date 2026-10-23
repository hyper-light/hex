#!/usr/bin/env python3
"""bench/competitive: one OCI runtime's lifecycle, throughput, CPU, memory and density.

Runs inside the bench container (see oci_inside.sh), as root, with the runtime's binary under /opt/bin
and the bundle at /run/bundle-sleep (workload `sleep infinity`) and /run/bundle-true (`true`).

  oci_bench.py <runtime> <out.json> <n> <warmup> <throughput lifecycles> <density cap> <density budget MiB>

Phases, in order; a phase that fails records its error and the next one still runs:
  lifecycle   n sequential create, start, exec (when the runtime has one), delete --force of a `sleep`
              container; each operation timed alone (wall clock around the CLI call).
  start_true  n sequential create+start of a `true` container (AC-T7.1.3's measure), deleted untimed.
  throughput  create+start+delete --force of `true` containers at concurrency 1, 8 and 32: lifecycles/s,
              and CPU-seconds per lifecycle from the bench cgroup's cpu.stat.
  density     `sleep` containers started one after another until the cap or the memory budget, then
              memory per container from the bench cgroup's memory.current; all deleted afterwards.

Every CLI call's stdio goes to /dev/null or a reused error file, never a pipe: a detached `create` leaves
the container's init holding the caller's stdio, so reading a pipe to EOF would deadlock (hex_start's
lesson, commit 4c76c13). Every call has a timeout, so a hung runtime ends the phase with an error.
"""
import json
import os
import signal
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

CALL_TIMEOUT_S = 60
CG = "/sys/fs/cgroup"

RUNTIMES = {
    "hex": {"argv": ["/opt/bin/hex", "oci"], "env": {"HEX_OCI_ROOT": "/run/hex-oci"}, "exec": False},
    "runc": {"argv": ["/opt/bin/runc", "--root", "/run/runc"], "env": {}, "exec": True},
    "crun": {"argv": ["/opt/bin/crun", "--root", "/run/crun"], "env": {}, "exec": True},
    "youki": {"argv": ["/opt/bin/youki", "--root", "/run/youki"], "env": {}, "exec": True},
    "runsc": {"argv": ["/opt/bin/runsc", "--root", "/run/runsc", "--network=none"], "env": {}, "exec": True},
}


class OpError(Exception):
    pass


class Runtime:
    def __init__(self, name):
        spec = RUNTIMES[name]
        self.name = name
        self.argv = spec["argv"]
        self.has_exec = spec["exec"]
        self.env = dict(os.environ, **spec["env"])

    def call(self, *args, err_path=None):
        """Runs one CLI call; returns (wall ms, CPU s of the call). Raises OpError on failure.

        posix_spawn (vfork-class) and a blocking wait4 keep the harness's own cost and jitter out of
        the timing; a timer kills a call that outlives CALL_TIMEOUT_S."""
        errfd = os.open(err_path or f"/run/err/{self.name}.err", os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o644)
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
        return self.call("create", "--bundle", bundle, cid, **kw)

    def start(self, cid, **kw):
        return self.call("start", cid, **kw)

    def exec_true(self, cid, **kw):
        return self.call("exec", cid, "/usr/bin/true", **kw)

    def delete(self, cid, **kw):
        return self.call("delete", "--force", cid, **kw)


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


def phase_lifecycle(rt, n, warmup):
    ops = {"create": [], "start": [], "exec": [], "delete": []}
    cpu = {"create": [], "start": [], "exec": [], "delete": []}
    for i in range(warmup + n):
        cid = f"{rt.name}-l{i}"
        timed = [("create", lambda: rt.create("/run/bundle-sleep", cid)), ("start", lambda: rt.start(cid))]
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
        cid = f"{rt.name}-t{i}"
        a, _ = rt.create("/run/bundle-true", cid)
        try:
            b, _ = rt.start(cid)
        finally:
            rt.delete(cid)
        if i >= warmup:
            ms_all.append(a + b)
    return {"ms": ms_all}


def phase_throughput(rt, lifecycles):
    out = {}
    for conc in (1, 8, 32):
        def one(i, conc=conc):
            cid = f"{rt.name}-c{conc}-{i}"
            err = f"/run/err/{rt.name}.c{conc}.{i}.err"
            rt.create("/run/bundle-true", cid, err_path=err)
            try:
                rt.start(cid, err_path=err)
            finally:
                rt.delete(cid, err_path=err)
            os.unlink(err)
        cpu0, t0 = cpu_usec(), time.perf_counter()
        with ThreadPoolExecutor(max_workers=conc) as pool:
            list(pool.map(one, range(lifecycles)))
        wall = time.perf_counter() - t0
        cpu_s = (cpu_usec() - cpu0) / 1e6
        out[str(conc)] = {"lifecycles": lifecycles, "wall_s": wall, "per_s": lifecycles / wall,
                          "cpu_s_per_lifecycle": cpu_s / lifecycles}
    return out


def phase_density(rt, cap, budget_mib):
    base = mem_bytes()
    budget = budget_mib * 1024 * 1024
    started = []
    t0 = time.perf_counter()
    stop_reason = "cap"
    try:
        for i in range(cap):
            cid = f"{rt.name}-d{i}"
            try:
                rt.create("/run/bundle-sleep", cid)
                started.append(cid)
                rt.start(cid)
            except OpError as e:
                stop_reason = f"error at {i}: {e}"
                break
            if mem_bytes() - base > budget:
                stop_reason = "memory budget"
                break
        wall = time.perf_counter() - t0
        time.sleep(2)  # let the started inits settle before reading memory
        used = mem_bytes() - base
    finally:
        for cid in started:
            try:
                rt.delete(cid)
            except OpError:
                pass
    count = len(started)
    return {"count": count, "stop": stop_reason, "start_wall_s": wall, "memory_bytes": used,
            "memory_per_container_bytes": used / count if count else None,
            "containers_per_gib": count / (used / 2**30) if used > 0 else None}


def main():
    name, out_path = sys.argv[1], sys.argv[2]
    n, warmup, lifecycles, cap, budget = (int(x) for x in sys.argv[3:8])
    rt = Runtime(name)
    result = {"runtime": name, "n": n, "warmup": warmup, "phases": {}, "errors": {}}
    for phase, fn in (("lifecycle", lambda: phase_lifecycle(rt, n, warmup)),
                      ("start_true", lambda: phase_start_true(rt, n, warmup)),
                      ("throughput", lambda: phase_throughput(rt, lifecycles)),
                      ("density", lambda: phase_density(rt, cap, budget))):
        t0 = time.perf_counter()
        try:
            result["phases"][phase] = fn()
        except OpError as e:
            result["errors"][phase] = str(e)
        sys.stdout.write(f"{name} {phase}: {time.perf_counter() - t0:.0f}s"
                         f"{' ERROR ' + result['errors'][phase][:200] if phase in result['errors'] else ''}\n")
        sys.stdout.flush()
    with open(out_path, "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
