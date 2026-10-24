#!/usr/bin/env python3
"""bench/competitive/probes: where a systemd scope box's memory is charged.

  scope_charge.py <boxes>     (in a delegated user service, as box_bench.py runs)

box_bench.py's density phase found systemd-scope boxes holding ~1.1 MiB each across what it watches: the
driver's cgroup S, the scopes' slice, and the user manager's and bus's cgroups (fedora, 2026-10-23, rounds
5-6). This starts <boxes> scope boxes exactly as the density phase does (box_bench.spawn of `systemd-run
--user --scope --slice=...`, one at a time, each running before the next) and prints, per box, how each
memory.stat field of each of those cgroups moved, so the charge can be named by kind (anon, pagetables,
kernel_stack, slab, sock, shmem...) and by place rather than guessed. Each place's held memory (cgmem.py, as
the density phase computes it) is printed too, read both the moment the last box is up and once memory.stat
has been flushed. The first two runs read after a 3 s settle instead. The first (2026-10-23) read only S and
the slice: 33 KiB and ~253 KiB per box; the second (2026-10-24, 100 boxes) found the user manager's
init.scope holding ~147 KiB per box and the buses nothing, 0.42 MiB in all, against the density rounds' 1.11
MiB at 1,000 boxes, which were 1,000 queued systemd-run clients, not scopes. The third (adacachy, 2026-10-24,
100 boxes) read 371.4 KiB per box the moment the last was up and 370.8 KiB once flushed.
"""
import os
import signal
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.realpath(__file__))))
import box_bench as bb  # noqa: E402
import cgmem  # noqa: E402


def stat(path):
    if not os.path.isdir(path):
        return {}
    with open(os.path.join(path, "memory.stat")) as f:
        return {k: int(v) for k, v in (line.split() for line in f if line.strip())}


def held(places):
    return [cgmem.held_bytes(path) if os.path.isdir(path) else 0 for _, path in places]


def cgroup_of(pid):
    with open(f"/proc/{pid}/cgroup") as f:
        return bb.CG + f.read().split("::", 1)[1].strip()


def main():
    boxes = int(sys.argv[1])
    S = bb.own_cgroup()
    os.makedirs(S + "/driver", exist_ok=True)
    with open(S + "/driver/cgroup.procs", "w") as f:
        f.write(str(os.getpid()))
    user = S
    while not os.path.basename(user).startswith("user@"):
        user = os.path.dirname(user)
    sl = f"{user}/{bb.DENSITY_SLICE}"
    places = [("S (the driver's service)", S), ("the scopes' slice", sl)] + [
        (f"helper {h[len(bb.CG):]}", h) for h in bb.helper_cgroups()]
    before = [stat(path) for _, path in places]
    held0 = held(places)

    def exited(pid):
        done, status = os.waitpid(pid, os.WNOHANG)
        return f"its launcher exited {os.waitstatus_to_exitcode(status)}" if done else None
    kids = []
    try:
        # One box at a time, each up before the next, as the density phase starts them: systemd-run execs the
        # program once the manager has made its scope and moved it in, so what's read is running scopes.
        for i in range(boxes):
            pid = bb.spawn(bb.argv_for("systemd-scope", None, ["/usr/bin/sleep", "infinity"], slice_=bb.DENSITY_SLICE))
            kids.append(pid)
            why = bb.wait_up(lambda: bb.comm_is_sleep(pid), lambda: exited(pid))
            if why:
                sys.exit(f"box {i}: {why}")
        held_up = held(places)
        # memory.stat is the kernel's cached view: a read flushes it only where enough updates are pending, and
        # the whole hierarchy is flushed every 2 s (mm/memcontrol.c: mem_cgroup_flush_stats, which reads of
        # memory.stat call, and flush_memcg_stats_dwork). So a reading the moment the last box is up can miss
        # the newest updates. The last box's scope is new and small, so a read of its own memory.stat never
        # flushes it: its program's anon shows there once a flush from above has run, after every box was up.
        last = cgroup_of(kids[-1])
        why = bb.wait_up(lambda: stat(last).get("anon", 0) > 0, lambda: None)
        if why:
            sys.exit(f"the last box's memory.stat never showed its program: {why}")
        after = [stat(path) for _, path in places]
        held1 = held(places)
        print(f"held per box over {boxes} boxes: {(sum(held_up) - sum(held0)) / boxes / 1024:.1f} KiB the moment "
              f"the last was up, as the density phase reads it; {(sum(held1) - sum(held0)) / boxes / 1024:.1f} KiB "
              f"once memory.stat was flushed")
        for (name, _), a, b, h0, h1 in zip(places, before, after, held0, held1):
            moved = {k: (b.get(k, 0) - a.get(k, 0)) / boxes for k in sorted(set(a) | set(b))}
            print(f"{name}: held {(h1 - h0) / boxes / 1024:.1f} KiB per box; fields that moved by 1 KiB or more:")
            for k, v in sorted(moved.items(), key=lambda kv: -abs(kv[1])):
                if abs(v) >= 1024:
                    print(f"  {k:28s} {v / 1024:10.1f} KiB")
    finally:
        for pid in kids:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        for pid in kids:
            try:
                os.waitpid(pid, 0)
            except ChildProcessError:  # reaped already: its launcher exited before its box was up
                pass
        subprocess.run(["systemctl", "--user", "stop", bb.DENSITY_SLICE], timeout=bb.CALL_TIMEOUT_S)


if __name__ == "__main__":
    main()
