#!/usr/bin/env python3
"""The census of the inputs Hex bounds as hostile (Ada's 2026-11-06 rule; input_census.md).

Reads, read-only, the largest of each input a bound in Hex's code holds:

- `cgroup`: every cgroup under /sys/fs/cgroup, each file `crates/hex-runtime/src/linux/cgroup.rs` reads whole
  through `MAX_CGROUP_FILE` (and memory.stat beside them), the largest of each and where it was;
- `net`: the calling thread's network namespace, each `/proc/net` file `netcheck.rs` reads through `MAX_PROC_FILE`,
  and each of its five dumps (`netcheck::DUMPS`) answered whole: its bytes, its reads and its largest read, read
  in reads of `READ_BYTES` (8 KiB) as the check reads them.

Run it in the host's namespace for a busy one, and under `unshare -rn` (after `ip link set lo up`) for a fresh one.
"""

import json
import os
import socket
import struct
import sys

CGROUP_FILES = [
    "cgroup.events", "cgroup.stat", "cgroup.subtree_control", "cgroup.type", "cgroup.freeze",
    "cgroup.max.descendants", "cgroup.procs", "memory.oom.group", "memory.current", "memory.peak",
    "memory.events", "memory.stat", "pids.current", "pids.events", "cpu.stat", "cpu.max", "io.stat",
]
PROC_FILES = ["sockstat", "dev", "protocols", "ip_tables_names", "ip6_tables_names"]
# kind, family, family header bytes: netcheck::DUMPS.
DUMPS = [("addr4", 22, 2, 8), ("addr6", 22, 10, 8), ("route4", 26, 2, 12), ("route6", 26, 10, 12),
         ("rule", 34, 0, 12)]
READ_BYTES = 8 << 10


def size(path):
    try:
        with open(path, "rb") as f:
            return len(f.read())
    except OSError:
        return None


def cgroups():
    out = {name: {"bytes": 0, "where": None} for name in CGROUP_FILES}
    count = 0
    for dirpath, _dirs, files in os.walk("/sys/fs/cgroup"):
        count += 1
        for name in CGROUP_FILES:
            if name in files:
                n = size(os.path.join(dirpath, name))
                if n is not None and n > out[name]["bytes"]:
                    out[name] = {"bytes": n, "where": dirpath}
    return {"cgroups": count, "files": out}


def dump(kind, family, header, seq):
    s = socket.socket(socket.AF_NETLINK, socket.SOCK_RAW, 0)
    s.settimeout(5)
    msg = struct.pack("=IHHII", 16 + header, kind, 0x301, seq, 0) + bytes([family]) + bytes(header - 1)
    s.send(msg)
    total, reads, largest = 0, 0, 0
    while True:
        chunk = s.recv(READ_BYTES)
        reads += 1
        total += len(chunk)
        largest = max(largest, len(chunk))
        at, done = 0, False
        while at + 16 <= len(chunk):
            length, mtype = struct.unpack_from("=IH", chunk, at)
            if mtype in (2, 3):
                done = True
            at += (length + 3) & ~3
        if done:
            break
    s.close()
    return {"bytes": total, "reads": reads, "largest_read": largest}


def net():
    files = {name: size(os.path.join("/proc/thread-self/net", name)) for name in PROC_FILES}
    dumps = {name: dump(kind, fam, hdr, seq) for seq, (name, kind, fam, hdr) in enumerate(DUMPS, 1)}
    return {"proc_net": files, "dumps": dumps}


def main():
    what = sys.argv[1]
    print(json.dumps(cgroups() if what == "cgroup" else net(), indent=1, sort_keys=True))


main()
