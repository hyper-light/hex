#!/usr/bin/env python3
"""Goal 6 lever (4)'s A/B readings from one cell's out dir (goal6-mntns.md §12): the seed-start mount-lock holds (the
seeds' and the node's `pivot_root`, each timed from grant to release), the first 20 creates' part 19, the steady part
19 and whole create, and a co-tenant's mount stall (mount_cotenant.c's pairs) across the node's start and after it.
Run: python3 -I seedstart_extract.py CELL_OUT_DIR COTENANT_FILE"""
import json
import os
import re
import sys

LINE = re.compile(r"^\s*(?P<comm>.+?)-(?P<pid>\d+)\s+\[\d+\]\s+\S+\s+(?P<s>\d+)\.(?P<us>\d+):\s+(?P<body>.*)$")
ENTER = re.compile(r"^sys_enter: NR (\d+) \(([0-9a-f, ]*)\)")
EXIT = re.compile(r"^sys_exit: NR (\d+) = (-?\d+)")
NR_SETNS, NR_CHROOT, NR_PIVOT_ROOT, CLONE_NEWNET = 308, 161, 155, 0x40000000


def pct(values, q):
    v = sorted(values)
    return v[min(len(v) - 1, int(q * len(v)))] if v else None


def main():
    out, cotenant = sys.argv[1], sys.argv[2]
    args, spans, open_at, call, holding, pivots = {}, [], {}, {}, {}, []
    first = None
    with open(os.path.join(out, "mounts.c8.txt"), errors="replace") as f:
        for line in f:
            m = LINE.match(line)
            if not m:
                continue
            ts, pid, body = int(m["s"]) * 1_000_000 + int(m["us"]), int(m["pid"]), m["body"]
            first = first if first is not None else ts
            if (e := ENTER.match(body)):
                nr = int(e[1])
                call[pid] = nr
                a = [int(x, 16) for x in e[2].replace(" ", "").split(",") if x]
                if nr == NR_SETNS:
                    args[pid] = a
                if nr == NR_CHROOT and pid in open_at:
                    spans.append((open_at.pop(pid), ts))
            elif (e := EXIT.match(body)):
                a = args.get(pid, [])
                if int(e[1]) == NR_SETNS and int(e[2]) == 0 and len(a) > 1 and a[1] & CLONE_NEWNET:
                    open_at.setdefault(pid, ts)
            elif body.startswith("nsw:"):
                # The node's own prune runs on its `hex-prune` thread, whose system calls the trace doesn't follow:
                # its holds are counted by that thread's name.
                prune = m["comm"].strip() == "hex-prune"
                holding[pid] = [ts, NR_PIVOT_ROOT if prune else call.get(pid)]
            elif body.startswith("nsww:") and pid in holding:
                holding[pid][0] = None
            elif body.startswith("nswwr:") and pid in holding and holding[pid][0] is None:
                holding[pid][0] = ts
            elif body.startswith("nsu:"):
                h = holding.pop(pid, None)
                if h and h[0] is not None and h[1] == NR_PIVOT_ROOT:
                    pivots.append((h[0], ts - h[0]))
    spans.sort()
    first20 = [end - start for start, end in spans[:20]]
    start_end = spans[19][1] if len(spans) >= 20 else None
    pairs = []
    with open(cotenant) as f:
        for line in f:
            # The co-tenant is ended by a signal, so its last line may be cut short: only whole lines count.
            fields = line.split()
            if len(fields) == 2 and all(x.isdigit() for x in fields):
                pairs.append((int(fields[0]), int(fields[1])))
    during = [d for t, d in pairs if first is not None and start_end is not None and first <= t <= start_end]
    after = [d for t, d in pairs if start_end is not None and t > start_end]
    summary = json.load(open(os.path.join(out, "mounts.c8.summary.json")))
    creates = json.load(open(os.path.join(out, "mounts.c8.json")))["phases"]["throughput"]["8"]["create_ms"]
    print(json.dumps({
        "pivot_root_holds": len(pivots), "pivot_root_hold_us": sorted(d for _, d in pivots),
        "first20_part19_p50_us": pct(first20, 0.5), "first20_part19_max_us": max(first20) if first20 else None,
        "part19_p50_us": summary["span_p50_us"], "part19_p99_us": summary["span_p99_us"],
        "create_p99_ms": round(pct(creates, 0.99), 3),
        "part19_unpreempted_p99_us": summary.get("span_unpreempted_p99_us"),
        "mount_lock_waits_per_create": summary.get("nssem_waits_per_create"),
        "other_rwsem_waits_per_create": summary.get("other_rwsem_waits_per_create"),
        "cotenant_pairs_during_start": len(during), "cotenant_max_us_during_start": max(during) if during else None,
        "cotenant_p99_us_during_start": pct(during, 0.99), "cotenant_max_us_after": max(after) if after else None,
        "cotenant_p99_us_after": pct(after, 0.99),
    }))


if __name__ == "__main__":
    main()
