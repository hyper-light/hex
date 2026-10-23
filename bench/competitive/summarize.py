#!/usr/bin/env python3
"""bench/competitive: turns a run directory's JSON into the ratio table (markdown on stdout).

  summarize.py <run dir>

Percentiles are nearest-rank over each round's samples. Best of N: for each runtime and metric, the
round with the best value (lowest p50 for a latency, highest rate, lowest cost, highest density). Hex's
ratio is the best competitor's value over Hex's for lower-is-better metrics and Hex's over the best
competitor's for higher-is-better ones, so above 1 means Hex is ahead and 4 meets the 4-5x bar.
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
    return str(v)


def ratio(values, lower_better):
    """values: {name: value}; returns (text, ratio) of Hex against the best competitor."""
    hexv = values.get("hex")
    comp = {k: v for k, v in values.items() if k != "hex" and v is not None}
    if hexv is None or not comp:
        return "n/a", None
    best = min(comp, key=comp.get) if lower_better else max(comp, key=comp.get)
    r = comp[best] / hexv if lower_better else hexv / comp[best]
    return f"{r:.2f}x vs {best}", r


def oci_section(run):
    names = ["hex", "runc", "crun", "youki", "runsc"]
    data = {n: rounds(run, n) for n in names}
    data = {n: r for n, r in data.items() if r}
    rows = []  # (axis, unit, lower_better, {name: value})
    for op in ("create", "start", "exec", "delete"):
        per = {}
        for n, rs in data.items():
            per[n] = best_dist([r["phases"].get("lifecycle", {}).get("ms", {}).get(op, []) for r in rs.values()])
        for q in ("p50", "p99", "p99.9", "max"):
            rows.append((f"{op} {q} (ms)", "ms", True, {n: (d or {}).get(q) for n, d in per.items()}))
    per = {n: best_dist([r["phases"].get("start_true", {}).get("ms", []) for r in rs.values()]) for n, rs in data.items()}
    for q in ("p50", "p99", "p99.9", "max"):
        rows.append((f"create+start of `true` {q} (ms)", "ms", True, {n: (d or {}).get(q) for n, d in per.items()}))
    for c in ("1", "8", "32"):
        rows.append((f"throughput at concurrency {c} (lifecycles/s)", "/s", False,
                     {n: max((r["phases"].get("throughput", {}).get(c, {}).get("per_s") or 0 for r in rs.values()), default=None) or None
                      for n, rs in data.items()}))
    rows.append(("CPU per lifecycle, concurrency 1 (whole bench cgroup)", "cpu", True,
                 {n: min((r["phases"]["throughput"]["1"]["cpu_s_per_lifecycle"] for r in rs.values()
                          if "throughput" in r["phases"]), default=None) for n, rs in data.items()}))
    rows.append(("memory per idle container", "bytes", True,
                 {n: min((r["phases"]["density"]["memory_per_container_bytes"] for r in rs.values()
                          if r["phases"].get("density", {}).get("memory_per_container_bytes")), default=None)
                  for n, rs in data.items()}))
    rows.append(("density (idle containers per GiB)", "per_gib", False,
                 {n: max((r["phases"]["density"]["containers_per_gib"] for r in rs.values()
                          if r["phases"].get("density", {}).get("containers_per_gib")), default=None)
                  for n, rs in data.items()}))
    return list(data), rows, data


def box_section(run):
    rs = rounds(run, "box")
    methods = ["hex", "systemd-scope", "systemd-service"]
    rows = []
    per = {m: best_dist([r["methods"].get(m, {}).get("phases", {}).get("latency", {}).get("ms", []) for r in rs.values()])
           for m in methods}
    for q in ("p50", "p99", "p99.9", "max"):
        rows.append((f"box run of `true` {q} (ms)", "ms", True, {m: (d or {}).get(q) for m, d in per.items()}))
    for c in ("1", "8", "32"):
        rows.append((f"throughput at concurrency {c} (runs/s)", "/s", False,
                     {m: max((r["methods"].get(m, {}).get("phases", {}).get("throughput", {}).get(c, {}).get("per_s") or 0
                              for r in rs.values()), default=None) or None for m in methods}))

    def cpu(r, m):
        lat = r["methods"].get(m, {}).get("phases", {}).get("latency")
        if not lat or not lat["call_cpu_s"]:
            return None
        return sum(lat["call_cpu_s"]) / len(lat["call_cpu_s"]) + lat.get("helper_cpu_s_per_run", 0)
    rows.append(("CPU per run (the call, plus systemd's manager and bus)", "cpu", True,
                 {m: min((v for v in (cpu(r, m) for r in rs.values()) if v is not None), default=None) for m in methods}))
    rows.append(("memory per idle box", "bytes", True,
                 {m: min((r["methods"][m]["phases"]["density"]["memory_per_box_bytes"] for r in rs.values()
                          if r["methods"].get(m, {}).get("phases", {}).get("density", {}).get("memory_per_box_bytes")),
                         default=None) for m in methods}))
    rows.append(("density (idle boxes per GiB)", "per_gib", False,
                 {m: max((r["methods"][m]["phases"]["density"]["boxes_per_gib"] for r in rs.values()
                          if r["methods"].get(m, {}).get("phases", {}).get("density", {}).get("boxes_per_gib")),
                         default=None) for m in methods}))
    return methods, rows, rs


def table(title, names, rows):
    out = [f"### {title}", "", "| Axis | " + " | ".join(names) + " | Hex vs best competitor |",
           "|---|" + "---|" * len(names) + "---|"]
    for axis, unit, lower, vals in rows:
        txt, _ = ratio(vals, lower)
        out.append(f"| {axis} | " + " | ".join(fmt(vals.get(n), unit) for n in names) + f" | {txt} |")
    return "\n".join(out) + "\n"


def main():
    run = sys.argv[1]
    parts = []
    names, rows, data = oci_section(run)
    if data:
        parts.append(table("OCI class (Hex `oci` against runc, crun, youki, gVisor runsc)", names, rows))
        errs = {n: {k: v for r in rs.values() for k, v in r.get("errors", {}).items()} for n, rs in data.items()}
        errs = {n: e for n, e in errs.items() if e}
        if errs:
            parts.append("Phase errors: " + json.dumps(errs) + "\n")
    methods, rows, rs = box_section(run)
    if rs:
        parts.append(table("Process class (`hex box run` against systemd transient units)", methods, rows))
    print("\n".join(parts))


if __name__ == "__main__":
    main()
