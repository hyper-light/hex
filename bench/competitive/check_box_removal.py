#!/usr/bin/env python3
"""bench/competitive: box_bench.py's removal of a `hex box run` call's worker cgroup, on the real kernel.

`hex box run` moves itself into a `.hexbox-run` leaf of the cgroup it starts in (box_run.rs, box_root). Its
caller, box_bench.py's throughput phase, removes that cgroup tree once the call is reaped. On Linux v6.19 a
reaped task can still be in its cgroup: exit_notify makes it reapable (kernel/exit.c:983), but it leaves its
css_set only at its final switch, in cgroup_task_dead from finish_task_switch (kernel/sched/core.c:5156,
kernel/cgroup/cgroup.c:6989-7008). Until then the cgroup reads populated, and rmdir refuses it with EBUSY
(cgroup.c:6160-6161). box_bench.py's own run of record hit that at concurrency 32 in every round (RESULT.md,
"Goal 7").

This does what that phase does: `workers` processes, each `runs` calls of `hex box run -- /usr/bin/true` in a
fresh worker cgroup, removed by box_bench.remove_tree once the call is reaped. Right after each reap it reads
the wrapper's leaf: whether it still reads populated, and what cgroup.procs and cgroup.threads list then. That
tells a reaped wrapper not yet unlinked (populated with nothing listed) from a process or thread still alive
in it (the delete flake's leaderless thread group, or a stray). It fails on any removal refused, and on any
leaf found with a live process or thread in it after its call was reaped. CLAUDE.md §4; AC-T7.1.1 (`hex box
run` is the primitive kiln wraps a host leg in).

  check_box_removal.py <hex binary> [runs per worker (200)] [workers (32)]

Run on a cgroup v2 host in a delegated cgroup, as host.sh runs box_bench.py:
  systemd-run --user --scope -p Delegate=yes python3 -I check_box_removal.py <hex>
"""
import multiprocessing as mp
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))  # box_bench.py, beside this file even under -I
import box_bench  # noqa: E402


def read(path):
    try:
        with open(path) as f:
            return f.read()
    except FileNotFoundError:
        return ""


def worker(hex_bin, base, runs, conn):
    """`runs` calls, each in a fresh worker cgroup removed once the call is reaped; what each leaf held."""
    argv = box_bench.argv_for("hex", hex_bin, ["/usr/bin/true"])
    seen = {"runs": 0, "populated_after_reap": 0, "alive_after_reap": [], "refused": []}
    try:
        for i in range(runs):
            cg = f"{base}-{i}"
            os.mkdir(cg)
            box_bench.run_once(argv, cg)
            leaf = cg + "/.hexbox-run"
            if "populated 1" in read(leaf + "/cgroup.events"):
                seen["populated_after_reap"] += 1
                procs, threads = read(leaf + "/cgroup.procs").split(), read(leaf + "/cgroup.threads").split()
                if procs or threads:
                    seen["alive_after_reap"].append({"leaf": leaf, "procs": procs, "threads": threads})
            try:
                box_bench.remove_tree(cg)
            except OSError as e:
                # Left for the end, when the kernel is long done with it.
                seen["refused"].append({"tree": cg, "why": f"{e}"})
            seen["runs"] += 1
    except Exception as e:  # reported, never dropped
        seen["error"] = f"{type(e).__name__}: {e}"
    conn.send(seen)


def main():
    hex_bin = os.path.realpath(sys.argv[1])
    runs = int(sys.argv[2]) if len(sys.argv) > 2 else 200
    workers = int(sys.argv[3]) if len(sys.argv) > 3 else 32
    S = box_bench.own_cgroup()
    os.makedirs(S + "/driver", exist_ok=True)
    with open(S + "/driver/cgroup.procs", "w") as f:
        f.write(str(os.getpid()))
    with open(S + "/cgroup.subtree_control", "w") as f:
        f.write("+cpu +io +memory +pids")
    ctx = mp.get_context("spawn")
    procs = []
    for k in range(workers):
        a, b = ctx.Pipe(duplex=False)
        p = ctx.Process(target=worker, args=(hex_bin, f"{S}/c{k}", runs, b))
        p.start()
        b.close()
        procs.append((p, a))
    total = {"runs": 0, "populated_after_reap": 0, "alive_after_reap": [], "refused": [], "errors": []}
    for p, a in procs:
        try:
            s = a.recv()
        except EOFError:
            s = {"error": f"worker exited {p.exitcode} without a result"}
        p.join()
        for k in ("runs", "populated_after_reap"):
            total[k] += s.get(k, 0)
        total["alive_after_reap"] += s.get("alive_after_reap", [])
        total["refused"] += s.get("refused", [])
        if "error" in s:
            total["errors"].append(s["error"])
    for r in total["refused"]:
        box_bench.remove_tree(r["tree"])
    print(f"{total['runs']} calls by {workers} workers; leaf still populated right after the reap: "
          f"{total['populated_after_reap']}; with a live process or thread in it: {len(total['alive_after_reap'])}; "
          f"removals refused: {len(total['refused'])}; errors: {len(total['errors'])}")
    for what in ("alive_after_reap", "refused", "errors"):
        for x in total[what][:3]:
            print(f"  {what}: {x}")
    ok = not total["refused"] and not total["alive_after_reap"] and not total["errors"] \
        and total["runs"] == runs * workers
    print("PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
