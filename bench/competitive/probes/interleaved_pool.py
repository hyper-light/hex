#!/usr/bin/env python3
"""bench/competitive/probes: the second criterion of goal 5 extended step 6 from OCI passes whose seam rounds ran
seam_oci's `interleaved` phase: concurrency 1 and 32 in turn, so each adjacent pair met the host as it was then.

Each pair gives one ratio, its rate at concurrency 32 over its rate at 1. The estimate pools those ratios across every
round of every pass given, for each driver apart (`hex-seam` in process, `hex-seam-socket` over the node agent's
socket): their mean, its standard error by a bootstrap over the pairs (B resamples, seeded), their median, least and
most, and the ratio of the pooled rates (each concurrency's lifecycles over its time). Each pair is printed with the
host's 1-minute load and CPU pressure (`some avg10`) at its start and end (the lead, 2026-11-03T06:12Z: the ratio per
adjacent pair, pooled with a bootstrap SE, load and CPU pressure beside each round), and, where the phase records it,
each half's CPU per lifecycle and the CPUs it kept busy: concurrency 32 held by the CPUs it has, or by waiting; and,
where the phase pinned its halves (HEXBENCH_INTERLEAVED_CPUS1 and _CPUS32), each half's CPUs as read back from every
task the pin reached, and in how many pairs every task held its half's CPUs alone.

  interleaved_pool.py [--boot B] <pass dir>...
"""
import glob
import json
import os
import random
import statistics as st
import sys

DRIVERS = {"ociseam": "hex-seam", "ociseamsock": "hex-seam-socket"}


def pairs_of(run):
    """Each seam round's interleaved pairs in `run`, by driver, in round order."""
    out = {}
    for stem, name in DRIVERS.items():
        for path in sorted(glob.glob(os.path.join(run, f"{stem}.r*.json"))):
            rnd = os.path.basename(path)[len(stem) + 2:-len(".json")]
            if not rnd.isdigit():
                continue
            with open(path) as f:
                phase = json.load(f).get("phases", {}).get("interleaved")
            if not phase:
                continue
            for i, p in enumerate(phase["pairs"]):
                out.setdefault(name, []).append((os.path.basename(run), int(rnd), i, p, phase))
    return out


def netns_line(phases):
    """The kept namespaces' checks over `phases` (each a round's interleaved phase), and the held files' bound."""
    reuse = [ph["netns_reuse"] for ph in phases if ph.get("netns_reuse")]
    if not reuse:
        return "kept namespaces: none counted (a node that keeps none, or a pass before the counts)"
    total = {k: sum(r.get(k, 0) for r in reuse) for k in
             ("checked", "through_files", "protocols", "files_bounded", "files_failed")}
    bounds = {(r.get("files_bound"), r.get("descriptors_soft"), r.get("descriptors_own")) for r in reuse}
    if bounds == {(None, None, None)}:
        bound = "the held files' bound not reported (a binary before it)"
    else:
        bound = "; ".join(f"held files for at most {b} namespaces (soft limit {s}, the node's own {o})"
                          for b, s, o in sorted(bounds, key=str))
    return (f"kept namespaces: {total['checked']} checks, {total['through_files']} through held files, "
            f"{total['protocols']} through the protocols, {total['files_bounded']} entered for the bound, "
            f"{total['files_failed']} whose files failed; {bound}")


def load(at):
    return f"load {at.get('load1')}, cpu some {at.get('cpu_some_avg10')}%"


def cpu(p):
    """A pair's CPU, where its phase recorded it."""
    if "c1_cpu_ms_per_lifecycle" not in p:
        return ""
    return (f"; CPU a lifecycle {p['c1_cpu_ms_per_lifecycle']:.2f} ms at 1, {p['c32_cpu_ms_per_lifecycle']:.2f} at 32; "
            f"CPUs busy {p['c1_cpus_busy']:.2f} at 1, {p['c32_cpus_busy']:.2f} at 32")


def cpu_set(text):
    """A CPU list's CPUs ("4-15,20")."""
    out = set()
    for part in text.split(","):
        lo, _, hi = part.strip().partition("-")
        out.update(range(int(lo), int(hi or lo) + 1))
    return frozenset(out)


def pinned(p):
    """A pair's halves' CPUs as pinned and read back, where its phase pinned them."""
    out = []
    for half in ("c1", "c32"):
        pin = p.get(f"{half}_pinned")
        if pin:
            lists = ", ".join(f"{cpus} on {n}" for cpus, n in sorted(pin["cpus_allowed"].items()))
            out.append(f"{half} pinned {pin['tasks']} tasks, read back {lists}")
    return "; " + "; ".join(out) if out else ""


def main(argv):
    boot, runs = 2000, []
    args = iter(argv)
    for a in args:
        if a == "--boot":
            boot = int(next(args))
        else:
            runs.append(a)
    if not runs:
        sys.exit(__doc__)
    every = {}
    for run in runs:
        for name, rows in pairs_of(run).items():
            every.setdefault(name, []).extend(rows)
    if not every:
        sys.exit("no interleaved phase in " + ", ".join(runs))
    rng = random.Random(20261103)
    for name, rows in sorted(every.items()):
        print(f"{name}:")
        for run, rnd, i, p, _ in rows:
            print(f"  {run} round {rnd} pair {i + 1}: concurrency 1 {p['c1_per_s']:.0f}/s, 32 {p['c32_per_s']:.0f}/s, "
                  f"{p['ratio']:.2f}x; at its start {load(p['at_start'])}; at its end {load(p['at_end'])}{cpu(p)}"
                  f"{pinned(p)}")
        ratios = [p["ratio"] for _, _, _, p, _ in rows]
        means = sorted(st.fmean(rng.choices(ratios, k=len(ratios))) for _ in range(boot))
        se = st.pstdev(means)
        c1_n = sum(ph["c1_lifecycles"] for *_, ph in rows)
        c32_n = sum(ph["c32_lifecycles"] for *_, ph in rows)
        c1_s = sum(ph["c1_lifecycles"] / p["c1_per_s"] for *_, p, ph in rows)
        c32_s = sum(ph["c32_lifecycles"] / p["c32_per_s"] for *_, p, ph in rows)
        lo, hi = means[int(0.025 * boot)], means[int(0.975 * boot) - 1]
        print(f"  pooled over {len(ratios)} pairs: mean ratio {st.fmean(ratios):.2f}x, bootstrap SE {se:.2f} ({boot} "
              f"resamples; 95% of resampled means {lo:.2f}x to {hi:.2f}x); median {st.median(ratios):.2f}x, least "
              f"{min(ratios):.2f}x, most {max(ratios):.2f}x; pooled rates {c32_n / c32_s:.0f}/s over "
              f"{c1_n / c1_s:.0f}/s, {(c32_n / c32_s) / (c1_n / c1_s):.2f}x")
        with_cpu = [p for _, _, _, p, _ in rows if "c1_cpu_ms_per_lifecycle" in p]
        if with_cpu:
            m = {k: st.median(p[k] for p in with_cpu) for k in
                 ("c1_cpu_ms_per_lifecycle", "c32_cpu_ms_per_lifecycle", "c1_cpus_busy", "c32_cpus_busy")}
            print(f"  CPU, the pairs' medians: {m['c1_cpu_ms_per_lifecycle']:.2f} ms a lifecycle at 1 and "
                  f"{m['c32_cpu_ms_per_lifecycle']:.2f} at 32; CPUs busy {m['c1_cpus_busy']:.2f} at 1 and "
                  f"{m['c32_cpus_busy']:.2f} at 32")
        with_took = [p for _, _, _, p, _ in rows if p.get("c1_p50_ms") is not None]
        if with_took:
            t = {k: st.median(p[k] for p in with_took) for k in ("c1_p50_ms", "c1_p99_ms", "c32_p50_ms", "c32_p99_ms")}
            worst = max(p["c32_p99_ms"] for p in with_took)
            print(f"  a lifecycle's wall, the pairs' medians: p50 {t['c1_p50_ms']:.3f} ms and p99 {t['c1_p99_ms']:.3f} at "
                  f"1, p50 {t['c32_p50_ms']:.3f} and p99 {t['c32_p99_ms']:.3f} at 32 (the most of any pair's p99 at 32: "
                  f"{worst:.3f})")
        # Each half's pin as read back: every task the pin reached held the half's CPUs alone, in how many pairs.
        for half in ("c1", "c32"):
            asked = {ph.get(f"{half}_cpus") for *_, ph in rows} - {None}
            if not asked:
                continue
            held = sum(1 for _, _, _, p, ph in rows
                       if p.get(f"{half}_pinned")
                       and {cpu_set(c) for c in p[f"{half}_pinned"]["cpus_allowed"]} == {cpu_set(ph[f"{half}_cpus"])})
            print(f"  {half} halves pinned to {', '.join(sorted(asked))}: every task on those CPUs alone in {held} "
                  f"of {len(rows)} pairs")
        # The kept namespaces' checks and the held files' bound the node ran with (the lead, 2026-11-04T09:15Z): a
        # run whose bound is 0, as at systemd's default soft limit, held no files and tests none. Each round's phase
        # once: its pairs' rows all carry it.
        print(f"  {netns_line(list({id(ph): ph for *_, ph in rows}.values()))}")


if __name__ == "__main__":
    main(sys.argv[1:])
