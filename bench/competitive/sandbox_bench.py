#!/usr/bin/env python3
"""bench/competitive: process sandboxes, one command each, against `hex box run`.

  sandbox_bench.py <out.json> <n> <warmup> <lifecycles> <density cap> <density budget MiB> [method ...]

Runs as root inside the privileged lab container (sandbox_inside.sh), started alone in a cgroup S of its
own. Every sandbox, of every method, starts in a fresh empty cgroup below S, made before its timed run and
removed after, so CPU and memory are S's for all of them alike. Each method runs
`/usr/bin/true` (latency, throughput) or `sleep infinity` (density) in that tool's sandbox, as the tool
is commonly used; METHODS says what each one enforces, which the table prints beside its numbers.
Every run is spawned the same way (fork, then execve in a single-threaded worker process) and waited
with a blocking wait4 and a timeout, and its stdio goes to /dev/null, never a pipe.

Phases, in order; a phase that fails records its error and the next still runs:
  latency     n sequential runs of `true`, each timed alone, after warmup untimed.
  throughput  runs of `true` at concurrency 1, 8 and 32 (worker processes): runs/s, and CPU-seconds per
              run from the cgroup's cpu.stat.
  density     sandboxes holding `sleep infinity`, started one at a time, each up (its `sleep` runs) before
              the next, until the cap, the memory budget or one that exits; then the memory the cgroup
              holds per sandbox (cgmem.py); all killed afterwards.
"""
import json
import multiprocessing as mp
import os
import shutil
import signal
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))  # cgmem.py, beside this file even under -I
import cgmem  # noqa: E402

CALL_TIMEOUT_S = 60
CG = os.environ.get("HEXBENCH_CGROUP", "/sys/fs/cgroup")
BIN = os.environ.get("HEXBENCH_BIN", "/opt/bin")
ROOTFS = os.environ.get("HEXBENCH_ROOTFS", "/run/rootfs")
CONSOLES = os.environ.get("HEXBENCH_CONSOLES", "/run/consoles")  # the density sandboxes' output, while they run
MEM, PIDS = "268435456", "64"  # the bounds `hex box run` gets, where a tool can hold them

# name: (argv before the program, what the tool enforces). The program is appended: /usr/bin/true or
# /usr/bin/sleep infinity. Namespace sandboxes see the host's /usr read-only and nothing else of it.
BWRAP = ["bwrap", "--unshare-all", "--die-with-parent", "--ro-bind", "/usr", "/usr",
         "--symlink", "usr/bin", "/bin", "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib64", "/lib64",
         "--proc", "/proc", "--dev", "/dev", "--"]
METHODS = {
    "hex": ([f"{BIN}/hex", "box", "run", "--memory-max", MEM, "--pids-max", PIDS, "--"],
            "cgroup v2 bounds (memory, pids); no namespaces"),
    "bwrap": (BWRAP, "user, mount, pid, net, ipc, uts and cgroup namespaces; no resource bounds"),
    "nsjail": (["nsjail", "-Mo", "--quiet", "--chroot", ROOTFS, "-R", "/usr", "--use_cgroupv2",
                "--cgroupv2_mount", CG, "--cgroup_mem_max", MEM, "--cgroup_pids_max", PIDS, "--disable_rlimits",
                "--"],
               "mount, pid, net, ipc, uts, user and cgroup namespaces; seccomp off; cgroup v2 bounds (memory, pids)"),
    "firejail": (["firejail", "--quiet", "--noprofile", "--private", "--net=none", "--rlimit-nproc=" + PIDS,
                  "--rlimit-as=" + MEM, "--"],
                 "mount, pid and net namespaces, a private home, seccomp; rlimits (nproc, address space)"),
    "minijail": (["minijail0", "-p", "-v", "-e", "-l", "-u", "-n", "-r", "--"],
                 "pid, mount, net, ipc and uts namespaces; no_new_privs; no resource bounds"),
    "nspawn": (["systemd-nspawn", "--quiet", "--register=no", "--keep-unit", "--private-network",
                "-D", ROOTFS, "--bind-ro=/usr", "--as-pid2"],
               "a container (mount, pid, net, ipc, uts namespaces, its own /dev); no resource bounds without systemd"),
    "srt": (["srt", "--"], "bwrap underneath, and a network proxy that denies all by default; no resource bounds"),
    "codex": (["codex", "sandbox", "--"],
              "codex's Linux sandbox (Landlock and seccomp, no network); no resource bounds"),
}


def own_cgroup():
    if os.environ.get("HEXBENCH_SELF_CGROUP"):
        return os.environ["HEXBENCH_SELF_CGROUP"]
    with open("/proc/self/cgroup") as f:
        for line in f:
            if line.startswith("0::"):
                return os.path.join(CG, line[3:].strip().strip("/"))
    raise SystemExit("no cgroup v2 membership")


def spawn(argv, cgroup=None, out=os.devnull):
    """fork, (join `cgroup`), execve with stdin from /dev/null and stdout and stderr to `out`; the
    child's pid."""
    pid = os.fork()
    if pid == 0:
        try:
            if cgroup:
                with open(cgroup + "/cgroup.procs", "w") as f:
                    f.write(str(os.getpid()))
            null = os.open(os.devnull, os.O_RDWR)
            o = null if out == os.devnull else os.open(out, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
            for fd, src in ((0, null), (1, o), (2, o)):
                os.dup2(src, fd)
            os.execvp(argv[0], argv)
        finally:
            os._exit(127)
    return pid


def run_once(argv, cgroup):
    """One timed run: a blocking wait4, with an interval timer that kills a run past the timeout."""
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


def fresh(base, tag):
    """A fresh, empty cgroup for one sandbox, made before its timed run, for every method alike: some
    tools take over the cgroup they start in (`hex box run`) or make children under it (nsjail's cgroup
    v2 mode), which cgroup v2 admits only in a cgroup no other process shares."""
    d = f"{base}/{tag}"
    os.mkdir(d)
    return d


def remove_tree(cg):
    """Kills whatever is left in a cgroup and below it, waits for it to empty, and removes it, deepest
    first (bounded by the tree's size and a deadline)."""
    with open(cg + "/cgroup.kill", "w") as f:
        f.write("1")
    deadline = time.monotonic() + CALL_TIMEOUT_S
    while True:
        with open(cg + "/cgroup.events") as f:
            if "populated 0" in f.read():
                break
        if time.monotonic() > deadline:
            raise RuntimeError(f"{cg} still held processes {CALL_TIMEOUT_S}s after cgroup.kill")
        time.sleep(0.005)
    for d in reversed([d for d, _, _ in os.walk(cg)]):
        os.rmdir(d)


def true_argv(method):
    """A sandbox's command for one run of `true`."""
    return METHODS[method][0] + ["/usr/bin/true"]


def sleep_argv(method):
    """A sandbox's command for one idle sandbox, held for the density phase."""
    return METHODS[method][0] + ["/usr/bin/sleep", "infinity"]


def sleep_runs(cg, _console):
    """A process sandbox is up once its payload, `sleep`, runs somewhere in its cgroup tree."""
    for d, _, _ in os.walk(cg):
        try:
            with open(d + "/cgroup.procs") as f:
                pids = f.read().split()
        except FileNotFoundError:  # a cgroup the tool removed as this walked
            continue
        for pid in pids:
            try:
                with open(f"/proc/{pid}/comm") as f:
                    if f.read().strip() == "sleep":
                        return True
            except (FileNotFoundError, ProcessLookupError):  # a process that ended as this read
                pass
    return False


def worker(argv_of, key, base, k, count, warmup, conn):
    """count runs of `argv_of(key)`, each in a fresh cgroup, after warmup untimed. argv_of is a
    module-level function, as multiprocessing's spawn needs to send it by reference."""
    try:
        ms, cpu = [], []
        for i in range(warmup + count):
            cg = fresh(base, f"w{k}-{i}")
            a, b = run_once(argv_of(key), cg)
            remove_tree(cg)
            if i >= warmup:
                ms.append(a)
                cpu.append(b)
        conn.send({"ms": ms, "cpu_s": cpu})
    except Exception as e:  # reported, never dropped
        conn.send({"error": f"{type(e).__name__}: {e}"})


def run_workers(argv_of, key, workers, per_worker, warmup, base):
    ctx = mp.get_context("spawn")
    procs = []
    for k in range(workers):
        a, b = ctx.Pipe(duplex=False)
        p = ctx.Process(target=worker, args=(argv_of, key, base, k, per_worker, warmup, b))
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


def cpu_usec(cg):
    with open(os.path.join(cg, "cpu.stat")) as f:
        for line in f:
            k, v = line.split()
            if k == "usage_usec":
                return int(v)
    raise RuntimeError("cpu.stat has no usage_usec")


def phase_latency(argv_of, key, n, warmup, base):
    out = run_workers(argv_of, key, 1, n, warmup, base)[0]
    return {"ms": out["ms"], "call_cpu_s": out["cpu_s"]}


def phase_throughput(argv_of, key, lifecycles, base, S):
    res = {}
    for conc in (1, 8, 32):
        per = max(1, lifecycles // conc)
        c0, t0 = cpu_usec(S), time.perf_counter()
        run_workers(argv_of, key, conc, per, 0, base)
        wall = time.perf_counter() - t0
        runs = per * conc
        res[str(conc)] = {"runs": runs, "wall_s": wall, "per_s": runs / wall,
                          "cpu_s_per_run": (cpu_usec(S) - c0) / 1e6 / runs}
    return res


def phase_density(argv, up, cap, budget_mib, base, S, consoles):
    """Sandboxes of `argv`, one at a time, each up (`up(cgroup, console file)`) before the next starts, so
    the budget check sees each one's steady memory; until the cap, the memory budget or one that exits.
    Then the memory S holds for them. Each one's output goes to a file under consoles; the start of an
    exited one's says why."""
    held0 = cgmem.held_bytes(S)
    os.makedirs(consoles)
    cgs, pids, stop, why = [], [], "cap", None
    t0 = time.perf_counter()
    try:
        for i in range(cap):
            cg = fresh(base, f"d{i}")
            cgs.append(cg)
            console = f"{consoles}/d{i}"
            open(console, "wb").close()  # there before its sandbox starts, so `up` can always read it
            pids.append(spawn(argv, cg, console))
            deadline = time.monotonic() + CALL_TIMEOUT_S
            while not up(cg, console):
                if os.waitpid(pids[-1], os.WNOHANG)[0] != 0:
                    pids.pop()
                    with open(console, "rb") as f:
                        why = f.read(500).decode(errors="replace")
                    break
                if time.monotonic() > deadline:
                    raise RuntimeError(f"sandbox {i} neither up nor gone {CALL_TIMEOUT_S}s after it started")
                time.sleep(0.005)
            if why is not None:
                stop = "a sandbox exited"
                break
            if cgmem.held_bytes(S) - held0 > budget_mib * 2**20:
                stop = "memory budget"
                break
        wall = time.perf_counter() - t0
        held = cgmem.held_bytes(S) - held0
        # One that ended after it came up held nothing at the reading, so it isn't counted.
        ended = sum(1 for pid in pids if os.waitpid(pid, os.WNOHANG)[0] != 0)
    finally:
        for cg in cgs:
            remove_tree(cg)
        for pid in pids:
            try:
                os.waitpid(pid, 0)
            except ChildProcessError:
                pass
        shutil.rmtree(consoles)
    alive = len(pids) - ended
    return {"count": alive, "ended_after_up": ended, "stop": stop, "exited_why": why, "up_wall_s": wall,
            "held": cgmem.per_unit(held, alive)}


def run(argv, methods, true_of, sleep_of, up, enforces, extra=None):
    """The driver both sandbox_bench.py and vm_bench.py run: argv is `<out.json> <n> <warmup> <lifecycles>
    <density cap> <density budget MiB> [method ...]`; true_of and sleep_of give a method's commands, up
    says when a density sandbox is up, and enforces what a method enforces."""
    out_path = argv[0]
    n, warmup, lifecycles, cap, budget = (int(x) for x in argv[1:6])
    keys = argv[6:] or list(methods)
    # S is a cgroup this driver starts in alone (the inside script puts it there): it moves to a leaf,
    # S hands the controllers down, and each method's cgroup hands them on to its sandboxes' cgroups.
    S = own_cgroup()
    os.makedirs(S + "/driver", exist_ok=True)
    with open(S + "/driver/cgroup.procs", "w") as f:
        f.write(str(os.getpid()))
    controllers = "+cpu +io +memory +pids"
    with open(S + "/cgroup.subtree_control", "w") as f:
        f.write(controllers)
    wanted = (os.environ.get("HEXBENCH_PHASES") or "latency throughput density").split()
    result = {"n": n, "warmup": warmup, **(extra or {}), "methods": {}}
    for key in keys:
        m = result["methods"][key] = {"enforces": enforces(key), "phases": {}, "errors": {}}
        base = f"{S}/{key}"
        os.makedirs(base, exist_ok=True)
        with open(base + "/cgroup.subtree_control", "w") as f:
            f.write(controllers)
        for phase, fn in (("latency", lambda: phase_latency(true_of, key, n, warmup, base)),
                          ("throughput", lambda: phase_throughput(true_of, key, lifecycles, base, S)),
                          ("density", lambda: phase_density(sleep_of(key), up, cap, budget, base, S,
                                                            f"{CONSOLES}/{key}"))):
            if phase not in wanted:
                continue
            t0 = time.perf_counter()
            try:
                m["phases"][phase] = fn()
            except Exception as e:
                m["errors"][phase] = f"{type(e).__name__}: {e}"
            print(f"{key} {phase}: {time.perf_counter() - t0:.0f}s"
                  f"{' ERROR ' + m['errors'][phase][:200] if phase in m['errors'] else ''}", flush=True)
    with open(out_path, "w") as f:
        json.dump(result, f)


def main():
    run(sys.argv[1:], METHODS, true_argv, sleep_argv, sleep_runs, lambda m: METHODS[m][1])


if __name__ == "__main__":
    main()
