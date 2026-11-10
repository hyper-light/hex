#!/usr/bin/env python3
"""R08/R14: compare raw equivalent workload samples; missing counters stay unverified."""

import argparse
import json
import math
import sys
from pathlib import Path

# All values use base units; direction says which side of a ratio is better.
METRICS = {
    "latency_seconds": "lower", "throughput_per_second": "higher",
    "cpu_user_seconds": "lower", "cpu_kernel_seconds": "lower",
    "caller_cpu_seconds": "lower", "allocations_total": "lower",
    "reallocations_total": "lower", "allocated_bytes": "lower",
    "copied_bytes": "lower", "rss_peak_bytes": "lower",
    "retained_bytes": "lower", "kernel_bytes": "lower",
    "minor_faults_total": "lower", "major_faults_total": "lower",
}
EQUIVALENT = ("hardware", "os", "architecture", "backend", "workload_sha256",
              "security_sha256", "resource_envelope_sha256", "configuration_sha256")


def require(condition, why):
    if not condition:
        raise ValueError(why)


def quantile(values, numerator, denominator):
    # Inverse empirical CDF (nearest rank). This never interpolates unobserved tail samples.
    require(bool(values), "empty distribution")
    ordered = sorted(values)
    index = max(0, (len(ordered) * numerator + denominator - 1) // denominator - 1)
    return ordered[index]


def median_interval(values):
    """Exact distribution-free >=95% interval via the binomial sign/order-statistic method."""
    ordered = sorted(values)
    n = len(ordered)
    require(n > 0, "empty median interval")
    # If [min,max] cannot reach 95% coverage, admit that its bounds are unbounded.
    denominator = 1 << n
    tail = 1
    if 40 * tail > denominator:
        return {"lower": None, "upper": None, "coverage": None,
                "reason": "insufficient n for finite distribution-free 95% median interval"}
    k = 1
    # P(Bin(n,1/2)<k) <= 0.025 gives interval [X_k, X_(n-k+1)].
    while k < (n + 1) // 2:
        proposed = tail + math.comb(n, k)
        if 40 * proposed > denominator:
            break
        tail = proposed
        k += 1
    return {"lower": ordered[k - 1], "upper": ordered[n - k],
            "coverage": 1 - 2 * tail / denominator, "order": k,
            "method": "binomial sign/order statistics; independent samples required"}


def distribution(values):
    require(bool(values), "empty sample array")
    require(all(type(value) in (int, float) and math.isfinite(value) and value >= 0 for value in values),
            "nonfinite/negative/non-numeric sample")
    return {"n": len(values), "p50": quantile(values, 50, 100),
            "p90": quantile(values, 90, 100), "p99": quantile(values, 99, 100),
            "max": max(values), "min": min(values), "median_interval": median_interval(values),
            "tail_resolution": f"one observation / {len(values)}",
            "tail_uncertainty": "p99 is descriptive; no tail precision inferred from small n"}


def compare(candidate, baseline, target_ratio):
    require(type(target_ratio) in (int, float) and math.isfinite(target_ratio) and target_ratio >= 4,
            "R08 comparative target must be explicitly supplied and at least 4")
    for field in EQUIVALENT:
        require(field in candidate and candidate[field] == baseline.get(field),
                f"not equivalent: {field}")
    for row in (candidate, baseline):
        require(row.get("schema") == "hex-completion-profile-1", "unknown profile schema")
        require(row.get("build") == "release", "comparisons require release builds")
        require(row.get("scope") == "root-profile", "implementer/light/compile data is not a profile")
        require(row.get("intended_samples") == len(row.get("samples", [])) and row["intended_samples"] > 0,
                "zero/missing selected samples")
        require(len({sample["run"] for sample in row["samples"]}) == row["intended_samples"],
                "duplicate sample run")
        require(all(sample.get("verified") is True for sample in row["samples"]), "failed workload sample")
    report = {"schema": "hex-completion-comparison-1", "candidate": candidate.get("source"),
              "baseline": baseline.get("source"), "target_ratio": target_ratio,
              "equivalence": {field: candidate[field] for field in EQUIVALENT}, "metrics": {}, "gaps": []}
    for metric, direction in METRICS.items():
        sides = []
        for name, row in (("candidate", candidate), ("baseline", baseline)):
            values = [sample.get("metrics", {}).get(metric) for sample in row["samples"]]
            if any(value is None for value in values):
                report["gaps"].append(f"{name}.{metric}: unmeasured samples; never treated as zero")
                sides.append(None)
            else:
                sides.append(distribution(values))
        result = {"direction": direction, "candidate": sides[0], "baseline": sides[1], "ratio": None,
                  "conservative_ratio": None, "achieved": False}
        if all(side is not None for side in sides):
            c, b = sides
            numerator, denominator = (b["p50"], c["p50"]) if direction == "lower" else (c["p50"], b["p50"])
            if denominator > 0:
                result["ratio"] = numerator / denominator
            else:
                report["gaps"].append(f"{metric}: zero denominator; no finite ratio claimed")
            ci, bi = c["median_interval"], b["median_interval"]
            low, high = (bi["lower"], ci["upper"]) if direction == "lower" else (ci["lower"], bi["upper"])
            if low is not None and high is not None and high > 0:
                result["conservative_ratio"] = low / high
                result["achieved"] = result["conservative_ratio"] >= target_ratio
            else:
                report["gaps"].append(f"{metric}: comparative uncertainty is unresolved")
        report["metrics"][metric] = result
    required_coverage = {"caller", "runtime", "guest", "induced-kernel"}
    for name, row in (("candidate", candidate), ("baseline", baseline)):
        if set(row.get("coverage", [])) != required_coverage:
            report["gaps"].append(f"{name}: parent/runtime/guest/kernel cost coverage incomplete")
    report["all_targets_achieved"] = not report["gaps"] and all(row["achieved"] for row in report["metrics"].values())
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--target-ratio", type=float, required=True)
    parser.add_argument("--byte-limit", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    script_dir = Path(__file__).resolve().parents[2] / "scripts/scheduler-completion"
    sys.path.insert(0, str(script_dir))
    from evidence import load_json
    result = compare(load_json(args.candidate, args.byte_limit), load_json(args.baseline, args.byte_limit), args.target_ratio)
    args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"report": str(args.output), "all_targets_achieved": result["all_targets_achieved"],
                      "gaps": result["gaps"]}))
    # A measured miss remains a useful valid comparison; it is not a success against the R08 bar.
    return 0 if result["all_targets_achieved"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError) as error:
        print(f"UNVERIFIED comparison: {error}", file=sys.stderr)
        sys.exit(1)
