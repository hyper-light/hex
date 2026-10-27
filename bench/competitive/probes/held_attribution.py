#!/usr/bin/env python3
"""bench/competitive/probes: what one runtime's held memory per idle container is, over another's, by memory.stat
entry (runtime-architecture.md decision 2; AC-T7.1.5's accounting).

Reads a run directory's density phases (oci_bench.py: `held` and `stat_rise`, both from one reading at the peak
less one at the baseline), pairs round k of the runtime against round k of the reference (oci_inside.sh runs them
interleaved, in a shuffled order each round), and gives per container, for each pair: the held difference, each
part's difference (cgmem.HELD_PARTS, kernel by cgmem.KERNEL_PARTS, swap, zswap, hugepages), the share of the held
difference the named parts leave unattributed, the largest part, and the state roots' bytes.

  held_attribution.py <run dir> [runtime (hex)] [reference (crun)]

Writes <run dir>/held_attribution.json and prints a Markdown table.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.realpath(__file__))))  # cgmem.py, under -I too
import cgmem  # noqa: E402

PARTS = cgmem.HELD_PARTS + ("swap", "zswap", "huge")
KERNEL = cgmem.KERNEL_PARTS + ("kernel_other",)


def density(path):
    with open(path) as f:
        d = json.load(f)
    p = d["phases"].get("density")
    if not p or "stat_rise" not in p:
        raise SystemExit(f"{path}: no density phase with a memory.stat rise ({d['errors'].get('density', 'not run')})")
    return p


def per(p, key):
    v = p["stat_rise"].get(key)
    return v / p["count"] if isinstance(v, int) and p["count"] else None


def diff(a, b, key):
    x, y = per(a, key), per(b, key)
    return x - y if x is not None and y is not None else None


def pair(a, b):
    held = a["held"]["bytes"] / a["count"] - b["held"]["bytes"] / b["count"]
    parts = {k: diff(a, b, k) for k in PARTS}
    kernel = {k: diff(a, b, k) for k in KERNEL}
    if None in parts.values():
        named, share = None, None
    else:
        named = sum(v for k, v in parts.items() if k != "zswap") - parts["zswap"]
        share = abs(held - named) / abs(held) if held else None
    largest = max(((k, v) for k, v in {**{k: v for k, v in parts.items() if k != "kernel"}, **kernel}.items()
                   if v is not None), key=lambda kv: abs(kv[1]), default=(None, None))
    state = {}
    for side, p in (("runtime", a), ("reference", b)):
        root = p.get("state_root") or {}
        rise = {k: root["peak"][k] - root["base"][k] for k in ("entries", "apparent_bytes", "allocated_bytes")} \
            if root else None
        state[side] = {"root": root.get("peak", {}).get("root"), "short": root.get("peak", {}).get("short"),
                       "per_container": {k: v / p["count"] for k, v in rise.items()} if rise else None}
    return {"count": [a["count"], b["count"]], "held_diff_per_container": held, "parts": parts, "kernel": kernel,
            "named_sum": named, "unattributed_share": share, "largest": {"part": largest[0], "bytes": largest[1]},
            "state_root": state, "reasons": a["stat_rise"]["reasons"] + b["stat_rise"]["reasons"],
            # Each run's own held rise less its named entries' (memory.stat out of step with memory.current),
            # and whether its two readings came once the cgroup held still (cgmem.quiet_snapshot).
            "own_unattributed_per_container": {"runtime": per(a, "unattributed"), "reference": per(b, "unattributed")},
            "readings": {"runtime": a.get("readings"), "reference": b.get("readings")}}


def kib(v):
    return "n/a" if v is None else f"{v / 1024:+.2f}"


def main():
    run = sys.argv[1]
    rt = sys.argv[2] if len(sys.argv) > 2 else "hex"
    ref = sys.argv[3] if len(sys.argv) > 3 else "crun"
    rounds = sorted(int(f[len(rt) + 2:-5]) for f in os.listdir(run)
                    if f.startswith(f"{rt}.r") and f.endswith(".json") and f[len(rt) + 2:-5].isdigit())
    out = {"runtime": rt, "reference": ref, "rounds": {}}
    for k in rounds:
        b_path = os.path.join(run, f"{ref}.r{k}.json")
        if os.path.exists(b_path):
            out["rounds"][str(k)] = pair(density(os.path.join(run, f"{rt}.r{k}.json")), density(b_path))
    with open(os.path.join(run, "held_attribution.json"), "w") as f:
        json.dump(out, f, indent=1)
    ks = list(out["rounds"])
    print(f"KiB per idle container, {rt} less {ref}, by round: " + ", ".join(ks))
    print("| part | " + " | ".join(f"round {k}" for k in ks) + " |")
    print("|---|" + "---:|" * len(ks))
    rows = [("held", lambda r: r["held_diff_per_container"])]
    rows += [(k, lambda r, k=k: r["parts"][k]) for k in PARTS]
    rows += [(f"kernel: {k}", lambda r, k=k: r["kernel"][k]) for k in KERNEL]
    rows += [("named parts' sum", lambda r: r["named_sum"])]
    for side in ("runtime", "reference"):
        rows.append((f"state root ({side}) allocated",
                     lambda r, s=side: (r["state_root"][s]["per_container"] or {}).get("allocated_bytes")))
    for name, get in rows:
        print(f"| {name} | " + " | ".join(kib(get(out["rounds"][k])) for k in ks) + " |")
    for k in ks:
        r = out["rounds"][k]
        share = r["unattributed_share"]
        print(f"round {k}: unattributed {'n/a' if share is None else f'{share:.1%}'}; largest {r['largest']['part']}"
              f" ({kib(r['largest']['bytes'])} KiB); each run's own unattributed (KiB/container): "
              f"{kib(r['own_unattributed_per_container']['runtime'])} {rt}, "
              f"{kib(r['own_unattributed_per_container']['reference'])} {ref}; readings {r['readings']}; "
              f"reasons: {r['reasons'] or 'none'}")


if __name__ == "__main__":
    main()
