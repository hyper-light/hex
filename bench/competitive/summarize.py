#!/usr/bin/env python3
"""bench/competitive: turns a run directory's JSON into the ratio table (markdown on stdout).

  summarize.py <run dir>

Percentiles are nearest-rank over each round's samples. Best of N: for each runtime and metric, the
round with the best value (lowest p50 for a latency, highest rate, lowest cost, highest density). Hex's
ratio is the best competitor's value over Hex's for lower-is-better metrics and Hex's over the best
competitor's for higher-is-better ones, so above 1 means Hex is ahead and 4 meets the 4-5x bar. Each of
Hex's own columns (`hex`, and `hex-seam` for the process class through the Runtime seam) gets its own
ratio, against competitors of its own layer: Hex's command line against command lines, and its seam against
the runtimes driven in process (libcrun) where a table has them.
"""
import glob
import json
import math
import os
import re
import sys


def pct(samples, q):
    s = sorted(samples)
    return s[max(0, math.ceil(q * len(s)) - 1)] if s else None


def dist(samples):
    return {"n": len(samples), "p50": pct(samples, 0.5), "p99": pct(samples, 0.99),
            "p99.9": pct(samples, 0.999), "max": max(samples) if samples else None}


def rounds(run, prefix):
    out = {}
    for path in sorted(glob.glob(os.path.join(run, prefix + ".r*.json"))):
        m = re.search(r"\.r(\d+)\.json$", path)
        with open(path) as f:
            out[int(m.group(1))] = json.load(f)
    return out


def best_dist(samples_by_round):
    ds = [dist(s) for s in samples_by_round if s]
    return min(ds, key=lambda d: d["p50"]) if ds else None


def fmt(v, unit):
    if v is None:
        return "n/a"
    if unit == "ms":
        return f"{v:.2f}"
    if unit == "/s":
        return f"{v:.0f}"
    if unit == "cpu":
        return f"{v * 1e3:.2f} ms"
    if unit == "bytes":
        return f"{v / 2**20:.2f} MiB"
    if unit == "per_gib":
        return f"{v:.0f}"
    if unit == "cpus":
        return f"{v:.3f} CPUs"
    if unit == "x":
        return f"{v:.2f}x"
    return str(v)


# Runtimes driven in process: competitors of Hex's Runtime seam, not of its command line, which pays a
# process spawn per call that a library call doesn't (docs/design/runtime-architecture.md, "Two layers").
IN_PROCESS = {"libcrun"}


def ratio(values, lower_better, me="hex"):
    """values: {name: value}; returns (text, ratio) of Hex's column `me` against the best competitor of its
    layer, which is never one of Hex's own columns."""
    names = [k for k in values if not k.startswith("hex")]
    in_process = [k for k in names if k in IN_PROCESS]
    rivals = in_process if me == "hex-seam" and in_process else [k for k in names if k not in IN_PROCESS]
    # A value at or below zero (a memory delta the kernel reclaimed away, a cost too small to see) is not a
    # measurement, so it never enters a ratio.
    hexv = values.get(me)
    comp = {k: values[k] for k in rivals if values[k] is not None and values[k] > 0}
    if hexv is None or hexv <= 0 or not comp:
        return "n/a", None
    best = min(comp, key=comp.get) if lower_better else max(comp, key=comp.get)
    r = comp[best] / hexv if lower_better else hexv / comp[best]
    return f"{r:.2f}x vs {best}", r


# Memory is what the cgroups hold, page cache aside (cgmem.py), in every table. Rounds measured before that
# have only memory.current, which the kernel's reclaim moves at will, so they never enter these rows.
MEMORY_AXIS = "memory held per idle {} (page cache aside)"
DENSITY_AXIS = "density (idle {} per GiB held)"


def best(rs, get, lower):
    """Best of N: the lowest (or highest) positive value across rounds. A value at or below zero (a density
    phase stopped early, a rise reclaim hid) is not a measurement, so it never wins."""
    vals = [v for v in (get(r) for r in rs) if v is not None and v > 0]
    return (min(vals) if lower else max(vals)) if vals else None


def held(density, key):
    return ((density or {}).get("held") or {}).get(key)


def oci_section(run):
    names = ["hex", "runc", "crun", "youki", "runsc", "krun", "libcrun"]
    data = {n: rounds(run, n) for n in names}
    data = {n: r for n, r in data.items() if r}
    rows = []  # (axis, unit, lower_better, {name: value})
    for op in ("create", "start", "exec", "delete"):
        per = {}
        for n, rs in data.items():
            per[n] = best_dist([r["phases"].get("lifecycle", {}).get("ms", {}).get(op, []) for r in rs.values()])
        for q in ("p50", "p99", "p99.9", "max"):
            rows.append((f"{op} {q} (ms)", "ms", True, {n: (d or {}).get(q) for n, d in per.items()}))
    for op in ("create", "delete"):
        per = {n: best_dist([r["phases"].get("lifecycle_nonet", {}).get("ms", {}).get(op, []) for r in rs.values()])
               for n, rs in data.items()}
        rows.append((f"{op} p50 without a network namespace (ms)", "ms", True,
                     {n: (d or {}).get("p50") for n, d in per.items()}))
    per = {n: best_dist([r["phases"].get("start_true", {}).get("ms", []) for r in rs.values()]) for n, rs in data.items()}
    for q in ("p50", "p99", "p99.9", "max"):
        rows.append((f"create+start of `true` {q} (ms)", "ms", True, {n: (d or {}).get(q) for n, d in per.items()}))
    for c in ("1", "8", "32"):
        rows.append((f"throughput at concurrency {c} (lifecycles/s)", "/s", False,
                     {n: max((r["phases"].get("throughput", {}).get(c, {}).get("per_s") or 0 for r in rs.values()), default=None) or None
                      for n, rs in data.items()}))
    # The sustained round with the best rate, and how far its last window fell from its first.
    sus = {n: max((r["phases"]["sustained"] for r in rs.values() if "sustained" in r["phases"]),
                  key=lambda s: s["per_s"], default=None) for n, rs in data.items()}
    rows.append(("sustained lifecycles/s at concurrency 32 (HEXBENCH_SUSTAINED of them)", "/s", False,
                 {n: (s or {}).get("per_s") for n, s in sus.items()}))
    # No ratio: a last window faster than the first (warm-up) isn't better than one level with it.
    rows.append(("sustained: the last 1,000 lifecycles' rate against the first's", "x", None,
                 {n: (s or {}).get("last_over_first") for n, s in sus.items()}))
    rows.append(("CPU per lifecycle, concurrency 1 (whole bench cgroup)", "cpu", True,
                 {n: min((r["phases"]["throughput"]["1"]["cpu_s_per_lifecycle"] for r in rs.values()
                          if "throughput" in r["phases"]), default=None) for n, rs in data.items()}))
    rows.append((MEMORY_AXIS.format("container"), "bytes", True,
                 {n: best(rs.values(), lambda r: held(r["phases"].get("density"), "per_unit_bytes"), True)
                  for n, rs in data.items()}))
    rows.append((DENSITY_AXIS.format("containers"), "per_gib", False,
                 {n: best(rs.values(), lambda r: held(r["phases"].get("density"), "per_gib"), False)
                  for n, rs in data.items()}))
    return list(data), rows, data


def box_section(run):
    # seam_box writes its rounds (boxseam.r<N>.json) apart from box_bench.py's, in the same JSON; best of N
    # takes each method's rounds wherever they are.
    seam = rounds(run, "boxseam")
    rs = list(rounds(run, "box").values()) + list(seam.values())
    methods = ["hex"] + (["hex-seam"] if seam else []) + ["systemd-scope", "systemd-service"]
    rows = []
    per = {m: best_dist([r["methods"].get(m, {}).get("phases", {}).get("latency", {}).get("ms", []) for r in rs])
           for m in methods}
    for q in ("p50", "p99", "p99.9", "max"):
        rows.append((f"box run of `true` {q} (ms)", "ms", True, {m: (d or {}).get(q) for m, d in per.items()}))
    for c in ("1", "8", "32"):
        rows.append((f"throughput at concurrency {c} (runs/s)", "/s", False,
                     {m: max((r["methods"].get(m, {}).get("phases", {}).get("throughput", {}).get(c, {}).get("per_s") or 0
                              for r in rs), default=None) or None for m in methods}))

    def cpu(r, m):
        lat = r["methods"].get(m, {}).get("phases", {}).get("latency")
        if not lat or not lat["call_cpu_s"]:
            return None
        return sum(lat["call_cpu_s"]) / len(lat["call_cpu_s"]) + lat.get("helper_cpu_s_per_run", 0)
    rows.append(("CPU per run (the call, plus systemd's manager and bus)", "cpu", True,
                 {m: min((v for v in (cpu(r, m) for r in rs) if v is not None), default=None) for m in methods}))
    def density(r, m):
        return r["methods"].get(m, {}).get("phases", {}).get("density")
    rows.append((MEMORY_AXIS.format("box"), "bytes", True,
                 {m: best(rs, lambda r: held(density(r, m), "per_unit_bytes"), True) for m in methods}))
    rows.append((DENSITY_AXIS.format("boxes"), "per_gib", False,
                 {m: best(rs, lambda r: held(density(r, m), "per_gib"), False) for m in methods}))
    return methods, rows, rs


def platform_section(run, oci_data):
    """Sandbox platforms against Hex's oci class: create until the sandbox runs (Hex: create+start)."""
    plats = {}
    for path in sorted(glob.glob(os.path.join(run, "platform.*.r*.json"))):
        with open(path) as f:
            d = json.load(f)
        d["round"] = int(re.search(r"\.r(\d+)\.json$", path).group(1))
        name = d["platform"] + (" (hosted)" if d.get("hosted") else "")
        plats.setdefault(name, []).append(d)
    if not plats:
        return [], [], {}
    names = ["hex"] + sorted(plats)
    hex_rounds = list(oci_data.get("hex", {}).values())

    def hex_create_until_running(r):
        lc = r["phases"].get("lifecycle", {}).get("ms", {})
        return [a + b for a, b in zip(lc.get("create", []), lc.get("start", []))]
    per = {"hex": {"create": best_dist([hex_create_until_running(r) for r in hex_rounds]),
                   "exec": best_dist([r["phases"].get("lifecycle", {}).get("ms", {}).get("exec", []) for r in hex_rounds]),
                   "delete": best_dist([r["phases"].get("lifecycle", {}).get("ms", {}).get("delete", []) for r in hex_rounds])}}
    for name, rs in plats.items():
        per[name] = {op: best_dist([r["phases"].get("lifecycle", {}).get("ms", {}).get(op, []) for r in rs])
                     for op in ("create", "exec", "delete")}
    rows = []
    for op, label in (("create", "create until it runs a command"), ("exec", "exec of `true`"), ("delete", "delete")):
        for q in ("p50", "p99", "p99.9", "max"):
            rows.append((f"{label} {q} (ms)", "ms", True, {n: (per[n][op] or {}).get(q) for n in names}))
    for c in ("1", "8", "32"):
        vals = {"hex": max((r["phases"].get("throughput", {}).get(c, {}).get("per_s") or 0 for r in hex_rounds), default=0) or None}
        for name, rs in plats.items():
            vals[name] = max((r["phases"].get("throughput", {}).get(c, {}).get("per_s") or 0 for r in rs), default=0) or None
        rows.append((f"lifecycles/s at concurrency {c}", "/s", False, vals))

    def phase(r, name):
        return r["phases"].get(name) or {}
    # Hex's figures are its bench cgroup's; a platform's are its stack's cgroups' (platform_bench.py).
    # Older rounds' host-wide figures have other keys, so they never enter these rows.
    cpu = {"hex": best(hex_rounds, lambda r: phase(r, "throughput").get("1", {}).get("cpu_s_per_lifecycle"), True)}
    mem = {"hex": best(hex_rounds, lambda r: held(phase(r, "density"), "per_unit_bytes"), True)}
    dens = {"hex": best(hex_rounds, lambda r: held(phase(r, "density"), "per_gib"), False)}
    idle_cpu, idle_mem = {"hex": None}, {"hex": None}
    for name, rs in plats.items():
        cpu[name] = best(rs, lambda r: phase(r, "throughput").get("1", {}).get("stack_cpu_s_per_lifecycle"), True)
        mem[name] = best(rs, lambda r: held(phase(r, "density"), "per_unit_bytes"), True)
        dens[name] = best(rs, lambda r: held(phase(r, "density"), "per_gib"), False)
        idle_cpu[name] = best(rs, lambda r: (r.get("stack") or {}).get("idle_cpu_s_per_s"), True)
        idle_mem[name] = best(rs, lambda r: (r.get("stack") or {}).get("idle_held_bytes"), True)
    rows.append(("CPU per lifecycle at concurrency 1 (platforms: what a lifecycle adds to their stack's CPU)",
                 "cpu", True, cpu))
    rows.append((MEMORY_AXIS.format("sandbox"), "bytes", True, mem))
    rows.append((DENSITY_AXIS.format("sandboxes"), "per_gib", False, dens))
    rows.append(("the stack's own CPU at idle (Hex `oci` runs no daemon)", "cpus", True, idle_cpu))
    rows.append(("the stack's own memory held at idle (Hex `oci` runs no daemon)", "bytes", True, idle_mem))
    if any(phase(r, "resume") for rs in plats.values() for r in rs):
        for op in ("suspend", "resume"):
            per = {"hex": None}
            for name, rs in plats.items():
                per[name] = best_dist([phase(r, "resume").get("ms", {}).get(op, []) for r in rs])
            for q in ("p50", "p99", "p99.9", "max"):
                rows.append((f"{op} {q} (ms; Hex has none yet)", "ms", True, {n: (per.get(n) or {}).get(q) for n in names}))
    return names, rows, plats


def platform_notes(plats):
    """What a platform's lifecycle is, where it differs; then per round: where density stopped, the referee's
    count, processes the stack's cgroups missed, errors."""
    out = []
    for name, rs in sorted(plats.items()):
        about = next((r["about"] for r in rs if r.get("about")), None)
        if about:
            out.append(f"- {name}: {about}")
        for r in sorted(rs, key=lambda r: r["round"]):
            d = r["phases"].get("density") or {}
            bits = [f"density stopped at {d.get('count', 0)} ({d.get('stop', 'no density phase')})"]
            if r.get("referee"):
                bits.append(f"foreign load in {r['referee']['foreign']} of {r['referee']['samples']} samples")
            if r.get("network"):
                nw = r["network"]
                bits.append(f"TCP connect to {nw['endpoint']}: min {nw['connect_ms_min']:.1f} ms, "
                            f"p50 {nw['connect_ms_p50']:.1f} ms")
            if d.get("new_processes_outside_stack"):
                bits.append("new processes outside the stack's cgroups: " + json.dumps(d["new_processes_outside_stack"]))
            if r.get("errors"):
                bits.append("errors: " + json.dumps(r["errors"]))
            out.append(f"- {name}, round {r['round']}: " + "; ".join(bits))
    return "\n".join(out) + "\n"


def lab_section(run, prefix, run_label, unit, units):
    """A lab pass (sandbox_bench.py, vm_bench.py): each method as commonly used, against `hex box run`
    where the pass has it."""
    rs = rounds(run, prefix)
    seen = {m for r in rs.values() for m in r["methods"]}
    if not seen:
        return [], [], rs
    methods = (["hex"] if "hex" in seen else []) + sorted(seen - {"hex"})

    def phase(r, m, name):
        return r["methods"].get(m, {}).get("phases", {}).get(name) or {}
    rows = []
    per = {m: best_dist([phase(r, m, "latency").get("ms", []) for r in rs.values()]) for m in methods}
    for q in ("p50", "p99", "p99.9", "max"):
        rows.append((f"{run_label} {q} (ms)", "ms", True, {m: (d or {}).get(q) for m, d in per.items()}))
    for c in ("1", "8", "32"):
        rows.append((f"throughput at concurrency {c} (runs/s)", "/s", False,
                     {m: best(rs.values(), lambda r: phase(r, m, "throughput").get(c, {}).get("per_s"), False)
                      for m in methods}))
    rows.append(("CPU per run at concurrency 1 (the lab cgroup's)", "cpu", True,
                 {m: best(rs.values(), lambda r: phase(r, m, "throughput").get("1", {}).get("cpu_s_per_run"), True)
                  for m in methods}))
    rows.append((MEMORY_AXIS.format(unit), "bytes", True,
                 {m: best(rs.values(), lambda r: held(phase(r, m, "density"), "per_unit_bytes"), True)
                  for m in methods}))
    rows.append((DENSITY_AXIS.format(units), "per_gib", False,
                 {m: best(rs.values(), lambda r: held(phase(r, m, "density"), "per_gib"), False) for m in methods}))
    return methods, rows, rs


def lab_notes(methods, rs):
    """What each method enforces, then per round: where density stopped and why, how long its sandboxes
    took to start, and errors."""
    out = []
    for m in methods:
        enforces = next((r["methods"][m]["enforces"] for r in rs.values() if m in r["methods"]), "")
        out.append(f"- {m}: {enforces}")
        for n, r in sorted(rs.items()):
            d = r["methods"].get(m, {})
            den = d.get("phases", {}).get("density") or {}
            bits = []
            if den:
                bits.append(f"density stopped at {den.get('count', 0)} ({den.get('stop')}), started one at a "
                            f"time in {den.get('up_wall_s', 0):.1f} s")
                if den.get("ended_after_up"):
                    bits.append(f"{den['ended_after_up']} ended after they came up")
                if den.get("exited_why"):
                    bits.append("the one that exited said: " + json.dumps(den["exited_why"][:200]))
            if d.get("errors"):
                bits.append("errors: " + json.dumps(d["errors"]))
            if bits:
                out.append(f"  - round {n}: " + "; ".join(bits))
    return "\n".join(out) + "\n"


def table(title, names, rows):
    """A section's table; a ratio column for each of Hex's own columns in it, and n/a on a row whose
    lower_better is None."""
    hexes = [n for n in names if n.startswith("hex")]
    heads = "".join(" Hex vs best competitor |" if h == "hex" else f" {h} vs best competitor |" for h in hexes)
    out = [f"### {title}", "", "| Axis | " + " | ".join(names) + " |" + heads,
           "|---|" + "---|" * (len(names) + len(hexes))]
    for axis, unit, lower, vals in rows:
        cells = f"| {axis} | " + " | ".join(fmt(vals.get(n), unit) for n in names) + " |"
        out.append(cells + "".join(f" {ratio(vals, lower, h)[0] if lower is not None else 'n/a'} |" for h in hexes))
    return "\n".join(out) + "\n"


def main():
    run = sys.argv[1]
    parts = []
    names, rows, data = oci_section(run)
    if data:
        parts.append(table("OCI class (Hex `oci` against runc, crun, youki, gVisor runsc, crun+libkrun)" if "libcrun" not in names
                           else "OCI class (Hex `oci` against runc, crun, youki, gVisor runsc, crun+libkrun; libcrun in process, "
                                "the competitor of Hex's seam, outside the command line's ratio)", names, rows))
        errs = {n: {k: v for r in rs.values() for k, v in r.get("errors", {}).items()} for n, rs in data.items()}
        errs = {n: e for n, e in errs.items() if e}
        if errs:
            parts.append("Phase errors: " + json.dumps(errs) + "\n")
    pnames, prows, plats = platform_section(run, data)
    if plats:
        parts.append(table("Sandbox platforms (end to end through each SDK) against Hex `oci`", pnames, prows))
        parts.append(platform_notes(plats))
    methods, rows, rs = box_section(run)
    if rs:
        parts.append(table("Process class (`hex box run`, and the Runtime seam in process, against systemd transient units)"
                           if "hex-seam" in methods else "Process class (`hex box run` against systemd transient units)",
                           methods, rows))
    for prefix, title, run_label, unit, units in (
            ("sandbox", "Process sandboxes (each as commonly used) against `hex box run`", "run of `true`", "sandbox",
             "sandboxes"),
            ("vm", "MicroVM monitors run directly: the baseline for CP-T7.2's microVM classes (Hex has none yet)",
             "boot of a VM whose init ends it at once", "VM", "VMs")):
        lmethods, lrows, lrs = lab_section(run, prefix, run_label, unit, units)
        if lmethods:
            parts.append(table(title, lmethods, lrows))
            parts.append(lab_notes(lmethods, lrs))
    print("\n".join(parts))


if __name__ == "__main__":
    main()
