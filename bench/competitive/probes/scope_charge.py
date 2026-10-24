#!/usr/bin/env python3
"""bench/competitive/probes: where a systemd scope box's memory is charged.

  scope_charge.py <boxes>     (in a delegated user service, as box_bench.py runs)

box_bench.py's density phase found systemd-scope boxes holding ~1.1 MiB each across what it watches: the
driver's cgroup S, the scopes' slice, and the user manager's and bus's cgroups (fedora, 2026-10-23, rounds
5-6). This starts <boxes> scope boxes exactly as the density phase does (box_bench.spawn of `systemd-run
--user --scope --slice=...`) and prints, per box, how each memory.stat field of each of those cgroups moved,
so the charge can be named by kind (anon, pagetables, kernel_stack, slab, sock, shmem...) and by place
rather than guessed. The first run (2026-10-23) read only S and the slice: 33 KiB and ~253 KiB per box.
"""
import os
import signal
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.realpath(__file__))))
import box_bench as bb  # noqa: E402


def stat(path):
    if not os.path.isdir(path):
        return {}
    with open(os.path.join(path, "memory.stat")) as f:
        return {k: int(v) for k, v in (line.split() for line in f if line.strip())}


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
    kids = [bb.spawn(bb.argv_for("systemd-scope", None, ["/usr/bin/sleep", "infinity"], slice_=bb.DENSITY_SLICE))
            for _ in range(boxes)]
    time.sleep(3)
    after = [stat(path) for _, path in places]
    try:
        for (name, _), a, b in zip(places, before, after):
            moved = {k: (b.get(k, 0) - a.get(k, 0)) / boxes for k in sorted(set(a) | set(b))}
            print(f"{name}, per box, fields that moved by 1 KiB or more:")
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
            os.waitpid(pid, 0)
        subprocess.run(["systemctl", "--user", "stop", bb.DENSITY_SLICE], timeout=bb.CALL_TIMEOUT_S)


if __name__ == "__main__":
    main()
