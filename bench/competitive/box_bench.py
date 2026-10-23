#!/usr/bin/env python3
"""bench/competitive: process boxes, `hex box run` against systemd's transient scopes and services.

Runs on the bench host inside a systemd user service with Delegate=yes (host.sh starts it), so this
process's cgroup S is delegated: `hex box run` takes over the cgroup it starts in, so every Hex call
starts in an empty worker cgroup under S, and the driver itself lives in the S/driver leaf.

  box_bench.py <out.json> <n> <warmup> <lifecycles> <density cap> <density budget MiB> <hex binary>

Methods (each runs /usr/bin/true, or sleep for density, under the same limits: memory 256 MiB, 64 pids):
  hex              hex box run --memory-max 268435456 --pids-max 64 -- PROGRAM
  systemd-scope    systemd-run --user --scope --quiet -p MemoryMax=256M -p TasksMax=64 PROGRAM
  systemd-service  systemd-run --user --wait --quiet --collect -p MemoryMax=256M -p TasksMax=64 PROGRAM

Every call is spawned the same way (fork, then execve in a single-threaded worker process); the Hex
child first writes itself into its worker cgroup, a few microseconds the systemd children skip.
Phases: latency (n sequential runs), throughput (1, 8, 32 worker processes), CPU (the call's own
rusage, plus the user manager's and the user bus's CPU for systemd), density (boxes holding `sleep
infinity` until the cap or the memory budget).
"""
import json
import multiprocessing as mp
import os
import signal
import subprocess
import sys
import time

CG = "/sys/fs/cgroup"
CALL_TIMEOUT_S = 60
LIMITS = {"hex": ["--memory-max", "268435456", "--pids-max", "64", "--"],
          "systemd": ["-p", "MemoryMax=256M", "-p", "TasksMax=64"]}


def own_cgroup():
    # The override exists so the driver's control flow can be checked against stand-ins off the bench host.
    if os.environ.get("HEXBENCH_SELF_CGROUP"):
        return os.environ["HEXBENCH_SELF_CGROUP"]
    with open("/proc/self/cgroup") as f:
        for line in f:
            if line.startswith("0::"):
                return CG + line[3:].strip()
    raise SystemExit("no cgroup v2 membership")


def argv_for(method, hex_bin, program, unit=None):
    if method == "hex":
        return [hex_bin, "box", "run"] + LIMITS["hex"] + program
    if method == "systemd-scope":
        return ["systemd-run", "--user", "--scope", "--quiet"] + LIMITS["systemd"] + program
    if method == "systemd-service":
        extra = ["--wait", "--collect"] if unit is None else ["--unit", unit, "--collect"]
        return ["systemd-run", "--user", "--quiet"] + extra + LIMITS["systemd"] + program
    raise ValueError(method)


def spawn(argv, cgroup=None):
    """fork + (join cgroup) + execve with stdio to /dev/null; returns the child's pid."""
    pid = os.fork()
    if pid == 0:
        try:
            if cgroup:
                with open(cgroup + "/cgroup.procs", "w") as f:
                    f.write(str(os.getpid()))
            null = os.open(os.devnull, os.O_RDWR)
            for fd in (0, 1, 2):
                os.dup2(null, fd)
            os.execvp(argv[0], argv)
        finally:
            os._exit(127)
    return pid


def run_once(argv, cgroup):
    """One timed run: a blocking wait4, with an interval timer that kills a call past the timeout."""
    t0 = time.perf_counter_ns()
    pid = spawn(argv, cgroup)
    signal.signal(signal.SIGALRM, lambda *_: os.kill(pid, signal.SIGKILL))
    signal.setitimer(signal.ITIMER_REAL, CALL_TIMEOUT_S)
    try:
        _, status, ru = os.wait4(pid, 0)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
    ms = (time.perf_counter_ns() - t0) / 1e6
    code = os.waitstatus_to_exitcode(status)
    if code:
        why = f" (killed at the {CALL_TIMEOUT_S}s timeout)" if code == -signal.SIGKILL else ""
        raise RuntimeError(f"{argv[0]} exit {code}{why}")
    return ms, ru.ru_utime + ru.ru_stime


def worker(method, hex_bin, cgroup, count, warmup, conn):
    """A single-threaded worker process: count timed runs of the method; sends samples back."""
    try:
        argv = argv_for(method, hex_bin, ["/usr/bin/true"])
        cg = cgroup if method == "hex" else None
        ms, cpu = [], []
        for i in range(warmup + count):
            a, b = run_once(argv, cg)
            if i >= warmup:
                ms.append(a)
                cpu.append(b)
        conn.send({"ms": ms, "cpu_s": cpu})
    except Exception as e:  # reported, never dropped
        conn.send({"error": f"{type(e).__name__}: {e}"})


def ticks(pid):
    with open(f"/proc/{pid}/stat") as f:
        fields = f.read().rsplit(")", 1)[1].split()
    return int(fields[11]) + int(fields[12])  # utime + stime, in clock ticks


def pids_of(name):
    r = subprocess.run(["pgrep", "-u", str(os.getuid()), "-x", name], capture_output=True, text=True)
    return [int(p) for p in r.stdout.split()]


def run_workers(method, hex_bin, workers, per_worker, warmup, S):
    ctx = mp.get_context("spawn")
    procs = []
    for k in range(workers):
        a, b = ctx.Pipe(duplex=False)
        p = ctx.Process(target=worker, args=(method, hex_bin, f"{S}/w{k}", per_worker, warmup, b))
        p.start()
        b.close()  # only the worker holds the sending end, so a worker that dies ends recv() with EOFError
        procs.append((p, a))
    out = []
    for p, a in procs:
        try:
            out.append(a.recv())
        except EOFError:
            out.append({"error": f"worker exited {p.exitcode} without a result"})
    for p, _ in procs:
        p.join(timeout=CALL_TIMEOUT_S)
    errs = [o["error"] for o in out if "error" in o]
    if errs:
        raise RuntimeError(errs[0])
    return out


def phase_latency_cpu(method, hex_bin, n, warmup, S):
    helpers = pids_of("systemd") + pids_of("dbus-broker") if method != "hex" else []
    before = sum(ticks(p) for p in helpers)
    out = run_workers(method, hex_bin, 1, n, warmup, S)[0]
    helper_s = (sum(ticks(p) for p in helpers) - before) / os.sysconf("SC_CLK_TCK")
    return {"ms": out["ms"], "call_cpu_s": out["cpu_s"], "helper_cpu_s_per_run": helper_s / n}


def phase_throughput(method, hex_bin, lifecycles, S):
    res = {}
    for conc in (1, 8, 32):
        per = max(1, lifecycles // conc)
        t0 = time.perf_counter()
        run_workers(method, hex_bin, conc, per, 0, S)
        wall = time.perf_counter() - t0
        res[str(conc)] = {"lifecycles": per * conc, "wall_s": wall, "per_s": per * conc / wall}
    return res


def mem(path):
    with open(path + "/memory.current") as f:
        return int(f.read())


def phase_density(method, hex_bin, cap, budget_mib, S, user_service_cg):
    watch = S if method == "hex" else user_service_cg
    base = mem(watch)
    kids, units, stop = [], [], "cap"
    t0 = time.perf_counter()
    for i in range(cap):
        if method == "hex":
            cg = f"{S}/d{i}"
            os.makedirs(cg, exist_ok=True)
            kids.append((spawn(argv_for("hex", hex_bin, ["/usr/bin/sleep", "infinity"]), cg), cg))
        elif method == "systemd-scope":
            kids.append((spawn(argv_for(method, hex_bin, ["/usr/bin/sleep", "infinity"])), None))
        else:
            unit = f"hexbench-d{os.getpid()}-{i}"
            r = subprocess.run(argv_for(method, hex_bin, ["/usr/bin/sleep", "infinity"], unit=unit),
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=CALL_TIMEOUT_S)
            if r.returncode:
                stop = f"systemd-run exit {r.returncode} at {i}"
                break
            units.append(unit)
        if mem(watch) - base > budget_mib * 2**20:
            stop = "memory budget"
            break
    wall = time.perf_counter() - t0
    time.sleep(3)
    used = mem(watch) - base
    count = len(kids) + len(units)
    # teardown: end each box's program; the hex wrappers then clean their boxes and exit
    for pid, cg in kids:
        if cg:
            for box in os.listdir(cg + "/.hexbox") if os.path.isdir(cg + "/.hexbox") else []:
                try:
                    with open(f"{cg}/.hexbox/{box}/cgroup.kill", "w") as f:
                        f.write("1")
                except OSError:
                    pass
        else:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    for pid, _ in kids:
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
    for unit in units:
        subprocess.run(["systemctl", "--user", "kill", "--signal=SIGKILL", unit + ".service"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=CALL_TIMEOUT_S)
    return {"count": count, "stop": stop, "start_wall_s": wall, "memory_bytes": used,
            "memory_per_box_bytes": used / count if count else None,
            "boxes_per_gib": count / (used / 2**30) if used > 0 else None}


def main():
    out_path = sys.argv[1]
    n, warmup, lifecycles, cap, budget = (int(x) for x in sys.argv[2:7])
    hex_bin = os.path.realpath(sys.argv[7])
    S = own_cgroup()
    os.makedirs(S + "/driver", exist_ok=True)
    with open(S + "/driver/cgroup.procs", "w") as f:
        f.write(str(os.getpid()))
    with open(S + "/cgroup.subtree_control", "w") as f:
        f.write("+cpu +io +memory +pids")
    for k in range(32):
        os.makedirs(f"{S}/w{k}", exist_ok=True)
    user_service_cg = os.environ.get("HEXBENCH_USER_CGROUP") or S
    while not os.path.basename(user_service_cg).startswith("user@"):
        user_service_cg = os.path.dirname(user_service_cg)
        if user_service_cg == CG:
            raise SystemExit("not under a user@ service")
    result = {"n": n, "warmup": warmup, "methods": {}}
    for method in ("hex", "systemd-scope", "systemd-service"):
        m = result["methods"][method] = {"phases": {}, "errors": {}}
        for phase, fn in (("latency", lambda: phase_latency_cpu(method, hex_bin, n, warmup, S)),
                          ("throughput", lambda: phase_throughput(method, hex_bin, lifecycles, S)),
                          ("density", lambda: phase_density(method, hex_bin, cap, budget, S, user_service_cg))):
            t0 = time.perf_counter()
            try:
                m["phases"][phase] = fn()
            except Exception as e:
                m["errors"][phase] = f"{type(e).__name__}: {e}"
            print(f"{method} {phase}: {time.perf_counter() - t0:.0f}s"
                  f"{' ERROR ' + m['errors'][phase][:200] if phase in m['errors'] else ''}", flush=True)
    with open(out_path, "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
