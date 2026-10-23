#!/usr/bin/env python3
"""bench/competitive/probes: where a systemd scope box's memory is charged.

  scope_charge.py <boxes>     (in a delegated user service, as box_bench.py runs)

box_bench.py's density phase found systemd-scope boxes adding ~1.1 MiB each to the driver's cgroup and
~0.01 MiB to their own scopes (fedora, 2026-10-23, rounds 5-6). This starts <boxes> scope boxes exactly as
the density phase does (box_bench.spawn of `systemd-run --user --scope --slice=...`) and prints, per box,
how each memory.stat field of the driver's cgroup S and of the scopes' slice moved, so the charge can be
named by kind (anon, pagetables, kernel_stack, slab, sock, shmem...) rather than guessed.
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
    s0, l0 = stat(S), stat(sl)
    kids = [bb.spawn(bb.argv_for("systemd-scope", None, ["/usr/bin/sleep", "infinity"], slice_=bb.DENSITY_SLICE))
            for _ in range(boxes)]
    time.sleep(3)
    s1, l1 = stat(S), stat(sl)
    try:
        for name, a, b in (("S (the driver's service)", s0, s1), ("the scopes' slice", l0, l1)):
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
