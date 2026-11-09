"""Shards' cost per microVM on a Linux host with KVM (bench/sched/shards_overhead.md): for each guest size the
recorded workload uses, the host memory an idle guest holds and its boot-to-ready time.

    python3 -I shards_overhead.py WORK [--runs 3] [--boots 100] [--guests 5] [--idle-s 3] [--sizes CPU/MIB,...]
        [--memory-runs RUNS]   (0: boot-to-ready alone)

Reads SHARDS_BIN, HEX_GUEST_KERNEL and HEX_GUEST_INITRD and boots as Hex's scripts/guest/boot_check.py does
(`shards run vm`, `console=ttyS0 panic=-1`, no console). With HEX_VMM=firecracker it boots the same guest,
kernel and command line under Firecracker instead (FIRECRACKER_BIN, `--no-api`, a config file naming the
kernel, initrd, vCPUs, memory and a vsock), for Firecracker's own overhead. READY is boot_check's: a CONNECT to the relay's
control port answered OK, then the connection's 8-byte token, tried every millisecond from the moment the
VM process is started. A boot not READY by its bound is a failure, counted: the bound follows Hex's rule
for learned waits (the lead, 2026-11-06), twice the slowest boot to ready on record for this host class,
then twice the slowest of the record's and this run's, only rising. A guest is stopped by SIGKILL and
waited for, which the kernel ends without a bound of ours.

- Boot-to-ready, per size: RUNS runs of BOOTS boots one after another, each stopped and waited for before the
  next; per run its p50 and p99 (nearest rank); the best run by p99.
- Memory, per size: RUNS runs of GUESTS guests booted to READY and left idle IDLE_S seconds. For each guest,
  its process tree's smaps: PSS in all, and the guest RAM's (the anonymous or memfd mappings whose sizes sum
  to the guest's memory), so the VMM's own is the rest. From the cgroup's memory.stat before the guests and
  with them idle, what the kernel holds per guest (`kernel`, of it `sec_pagetables`, KVM's own page tables).
- Overhead, per size: what one more guest costs a node beside its own RAM, the VMM's own private memory
  (the most of any guest) plus the kernel's per guest (the most of any run). The VMM's text, shared by every
  guest on a node, is a cost per node, not per guest, so the figure doesn't depend on how many share it.
"""

import json
import os
import re
import socket
import statistics
import subprocess
import sys
import time

CONTROL_PORT = 0x4845_5800  # hex_core::stream::CONTROL_PORT
TOKEN_BYTES = 8
CMDLINE = "console=ttyS0 panic=-1"
# The recorded workload's resource pools (bench/sched/huawei_workload.md): millicores and MiB.
POOLS = [(300, 128), (400, 256), (600, 512), (800, 768), (1000, 1024), (1200, 1280), (1400, 1536),
         (1600, 1792), (1800, 2048), (2200, 2560), (2600, 3072), (3000, 3584), (3400, 4096), (6600, 8192)]
KIB = 1024
POLL_S = 0.001
MAPPING = re.compile(r"^[0-9a-f]+-[0-9a-f]+ ")
# The relay's longest answer to a CONNECT: "OK", a u32 port and a newline (Firecracker's hybrid-vsock
# handshake, which Shards' relay speaks).
REPLY_MAX = len(b"OK 4294967295\n")
# Hex's rule for learned waits: the bound is this many times the slowest recorded (the lead, 2026-11-06).
BOUND_FACTOR = 2
# The slowest boot to ready on record for KVM on fedora: the 3,584 MiB guests' best run under a load spike,
# p99 6,088.2 ms (bench/sched/shards_overhead.md; that run's maximum wasn't kept).
RECORDED_SLOWEST_S = 6.0882
slowest_s = RECORDED_SLOWEST_S


def handshake(path, left_s):
    """One try, within the `left_s` seconds its boot has left: True once the guest's relay answers, False
    while it doesn't yet."""
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    conn.settimeout(left_s)
    try:
        conn.connect(path)
        conn.sendall(b"CONNECT %d\n" % CONTROL_PORT)
        line = b""
        while not line.endswith(b"\n") and len(line) < REPLY_MAX:
            chunk = conn.recv(1)
            if not chunk:
                return False
            line += chunk
        if not line.startswith(b"OK "):
            return False
        got = b""
        while len(got) < TOKEN_BYTES:
            chunk = conn.recv(TOKEN_BYTES - len(got))
            if not chunk:
                return False
            got += chunk
        return True
    except OSError:
        return False
    finally:
        conn.close()


def boot(work, cpus, mib):
    """Starts a guest and waits for READY: (process, seconds to READY), or (process, None) if it wasn't READY
    within twice the slowest boot to ready yet seen, the record's or this run's."""
    global slowest_s
    deadline_s = BOUND_FACTOR * slowest_s
    os.makedirs(work, exist_ok=True)
    vsock = os.path.join(work, "vsock.sock")
    if os.path.exists(vsock):
        os.unlink(vsock)
    if os.environ.get("HEX_VMM", "shards") == "firecracker":
        config = os.path.join(work, "vm.json")
        with open(config, "w") as f:
            json.dump({"boot-source": {"kernel_image_path": os.environ["HEX_GUEST_KERNEL"],
                                       "initrd_path": os.environ["HEX_GUEST_INITRD"], "boot_args": CMDLINE},
                       "drives": [], "machine-config": {"vcpu_count": cpus, "mem_size_mib": mib},
                       "vsock": {"guest_cid": 3, "uds_path": vsock}}, f)
        args = [os.environ["FIRECRACKER_BIN"], "--no-api", "--config-file", config]
    else:
        args = [os.environ["SHARDS_BIN"], "run", "vm", "--kernel", os.environ["HEX_GUEST_KERNEL"], "--initrd",
                os.environ["HEX_GUEST_INITRD"], "--vsock", vsock, "--cpus", str(cpus), "--memory", str(mib),
                "--cmdline", CMDLINE, "--no-console"]
    log = open(os.path.join(work, "vm.log"), "wb")
    start = time.monotonic()
    vm = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    while (left_s := deadline_s - (time.monotonic() - start)) > 0:
        if vm.poll() is not None:
            return vm, None
        if handshake(vsock, left_s):
            took = time.monotonic() - start
            slowest_s = max(slowest_s, took)
            return vm, took
        time.sleep(POLL_S)
    print(f"BOOT mib={mib} cpus={cpus} not READY within {deadline_s:.3f} s", flush=True)
    return vm, None


def stop(vm):
    vm.kill()
    vm.wait()


def tree(root):
    """The process and its descendants, walked with a queue."""
    seen, queue = [], [root]
    while queue:
        pid = queue.pop()
        seen.append(pid)
        try:
            for tid in os.listdir(f"/proc/{pid}/task"):
                with open(f"/proc/{pid}/task/{tid}/children") as f:
                    queue.extend(int(c) for c in f.read().split())
        except OSError:
            continue
    return seen


def mappings(pid):
    """Each mapping of pid: (name, size, rss, pss, private) in KiB."""
    out, cur = [], None
    try:
        with open(f"/proc/{pid}/smaps") as f:
            for line in f:
                if MAPPING.match(line):
                    parts = line.split(None, 5)
                    cur = {"name": parts[5].strip() if len(parts) > 5 else "", "Size": 0, "Rss": 0, "Pss": 0,
                           "Private_Clean": 0, "Private_Dirty": 0}
                    out.append(cur)
                elif cur is not None:
                    key, _, rest = line.partition(":")
                    if key in cur and key != "name":
                        cur[key] = int(rest.split()[0])
    except OSError:
        return []
    return [(m["name"], m["Size"], m["Rss"], m["Pss"], m["Private_Clean"] + m["Private_Dirty"]) for m in out]


def guest_ram(maps, mib):
    """The mappings that are the guest's RAM: the anonymous ones, largest first, whose sizes sum to exactly the
    guest's memory (on KVM x86_64 Shards maps it as the main region, split at 3 GiB above that, beside a 24 MiB
    and a 16 MiB one); None where they don't. What is left, thread stacks among it, is the VMM's own."""
    want = mib * KIB
    anon = sorted((m for m in maps if m[0] == "" or m[0].startswith("/memfd:") or m[0].startswith("[anon")),
                  key=lambda m: -m[1])
    picked, total = [], 0
    for m in anon:
        if total >= want:
            break
        picked.append(m)
        total += m[1]
    return picked if total == want else None


def cgroup_stat():
    with open("/proc/self/cgroup") as f:
        path = f.read().strip().split("::", 1)[1]
    base = "/sys/fs/cgroup" + path
    stat = {}
    with open(base + "/memory.stat") as f:
        for line in f:
            k, v = line.split()
            stat[k] = int(v)
    with open(base + "/memory.current") as f:
        stat["current"] = int(f.read())
    return stat


def pct(sorted_values, p):
    rank = max(1, -(-len(sorted_values) * p // 100))
    return sorted_values[rank - 1]


def timing(work, cpus, mib, runs, boots):
    best = None
    for run in range(runs):
        times, failures = [], 0
        for i in range(boots):
            vm, t = boot(os.path.join(work, f"t{mib}-{run}-{i % 2}"), cpus, mib)
            stop(vm)
            if t is None:
                failures += 1
            else:
                times.append(t)
        times.sort()
        if not times:
            print(f"TIMING mib={mib} cpus={cpus} run={run} failures={failures} (no boot reached READY)", flush=True)
            continue
        row = (pct(times, 99), pct(times, 50), times[-1], failures, len(times))
        print(f"TIMING mib={mib} cpus={cpus} run={run} boots={len(times)} failures={failures} "
              f"p50_ms={row[1] * 1e3:.1f} p99_ms={row[0] * 1e3:.1f} max_ms={row[2] * 1e3:.1f}", flush=True)
        if best is None or row < best:
            best = row
    return best


def memory(work, cpus, mib, runs, guests, idle_s, show_maps):
    rows, kernels = [], []
    for run in range(runs):
        before = cgroup_stat()
        vms = []
        for i in range(guests):
            vm, t = boot(os.path.join(work, f"m{mib}-{run}-{i}"), cpus, mib)
            vms.append(vm)
            if t is None:
                print(f"MEMORY mib={mib} run={run} guest {i} never READY", flush=True)
        time.sleep(idle_s)
        after = cgroup_stat()
        run_rows = []
        for i, vm in enumerate(vms):
            maps = [m for pid in tree(vm.pid) for m in mappings(pid)]
            ram = guest_ram(maps, mib)
            pss = sum(m[3] for m in maps)
            private = sum(m[4] for m in maps)
            if ram is None:
                print(f"MEMORY mib={mib} run={run} guest {i}: no mappings sum to its RAM; largest: "
                      + "; ".join(f"{m[0] or 'anon'} {m[1] // KIB} MiB rss {m[2] // KIB}" for m in
                                  sorted(maps, key=lambda m: -m[1])[:6]), flush=True)
                continue
            ram_pss = sum(m[3] for m in ram)
            ram_private = sum(m[4] for m in ram)
            run_rows.append((pss, ram_pss, pss - ram_pss, private - ram_private))
            if show_maps and run == 0 and i == 0:
                print(f"MAPS mib={mib} processes={len(tree(vm.pid))} guest RAM: "
                      + "; ".join(f"{m[0] or 'anon'} {m[1] // KIB} MiB rss {m[2]} KiB" for m in ram), flush=True)
        per = {k: (after.get(k, 0) - before.get(k, 0)) / guests for k in
               ("current", "anon", "file", "shmem", "kernel", "sec_pagetables", "pagetables", "slab", "percpu")}
        print(f"MEMORY mib={mib} cpus={cpus} run={run} guests={guests} per guest: "
              + " ".join(f"{k}_kib={v / KIB:.0f}" for k, v in per.items()), flush=True)
        for vm in vms:
            stop(vm)
        kernels.append(per["kernel"])
        rows.extend(run_rows)
        if run_rows:
            print(f"MEMORY mib={mib} run={run} pss_kib={[r[0] for r in run_rows]} guest_ram_pss_kib="
                  f"{[r[1] for r in run_rows]} vmm_own_pss_kib={[r[2] for r in run_rows]} "
                  f"vmm_own_private_kib={[r[3] for r in run_rows]} kernel_kib={per['kernel'] / KIB:.0f} "
                  f"sec_pagetables_kib={per['sec_pagetables'] / KIB:.0f}", flush=True)
    return rows, kernels


def main(argv):
    if not argv:
        sys.exit(__doc__)
    work = argv[0]
    opt = {k: v for k, v in zip(argv[1::2], argv[2::2])}
    runs = int(opt.get("--runs", 3))
    boots = int(opt.get("--boots", 100))
    guests = int(opt.get("--guests", 5))
    idle_s = float(opt.get("--idle-s", 3))
    memory_runs = int(opt.get("--memory-runs", runs))
    pools = POOLS
    if "--sizes" in opt:
        pools = [tuple(int(x) for x in s.split("/")) for s in opt["--sizes"].split(",")]
    for cpu, mib in pools:
        cpus = max(1, -(-cpu // 1000))
        best = timing(work, cpus, mib, runs, boots)
        rows, kernels = memory(work, cpus, mib, memory_runs, guests, idle_s, True) if memory_runs else ([], [])
        if best:
            print(f"SUMMARY TIMING mib={mib} cpus={cpus} best-of-{runs} by p99: p50_ms={best[1] * 1e3:.1f} "
                  f"p99_ms={best[0] * 1e3:.1f} max_ms={best[2] * 1e3:.1f} failures={best[3]}", flush=True)
        if rows:
            col = lambda j: sorted(r[j] for r in rows)
            print(f"SUMMARY MEMORY mib={mib} guests={len(rows)}: host per idle guest (PSS) median "
                  f"{statistics.median(col(0)) / KIB:.1f} MiB max {col(0)[-1] / KIB:.1f}; guest RAM committed "
                  f"median {statistics.median(col(1)) / KIB:.1f} max {col(1)[-1] / KIB:.1f}; VMM's own PSS "
                  f"median {statistics.median(col(2)) / KIB:.2f} max {col(2)[-1] / KIB:.2f}; VMM's own private "
                  f"median {statistics.median(col(3)) / KIB:.2f} max {col(3)[-1] / KIB:.2f}", flush=True)
            overhead = col(3)[-1] * KIB + max(kernels)
            print(f"SUMMARY OVERHEAD mib={mib} cpus={cpus}: the VMM's own private (most) {col(3)[-1]} KiB + "
                  f"the kernel's per guest (most) {max(kernels) / KIB:.0f} KiB = {overhead:.0f} bytes", flush=True)
        print(f"SUMMARY BOUND mib={mib} slowest boot to ready so far {slowest_s * 1e3:.1f} ms, bound "
              f"{BOUND_FACTOR * slowest_s * 1e3:.1f} ms", flush=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
