#!/usr/bin/env python3
"""ROOT-only source-pinned profile collector; unknown counters never become zeros."""

import argparse
import importlib.util
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts/scheduler-completion"))
from evidence import (InvalidEvidence, finite_positive, hex_digest, load_json,
                      natural, require, sha256)
from mutation import command
from compare import METRICS


def counters(cgroup, byte_limit):
    if cgroup is None:
        return None
    result = {}
    for name in ("cpu.stat", "memory.stat", "memory.current", "memory.peak"):
        path = cgroup / name
        with path.open("rb") as source:
            raw = source.read(byte_limit + 1)
        require(len(raw) <= byte_limit, "cgroup counter exceeded admitted byte bound")
        if name.endswith(".stat"):
            result[name] = {key:int(value) for key, value in (line.split() for line in raw.decode().splitlines())}
        else:
            result[name] = int(raw)
    return result


def deltas(before, after):
    require(before is not None and after is not None, "counter snapshots required")
    cpu, memory = {}, {}
    for target, filename in ((cpu, "cpu.stat"), (memory, "memory.stat")):
        for name, value in after[filename].items():
            if name in before[filename]:
                target[name] = value - before[filename][name]
    for key in ("user_usec", "system_usec"):
        require(cpu.get(key, -1) >= 0, "CPU counter decreased/missing")
    require(memory.get("pgfault", -1) >= 0 and memory.get("pgmajfault", -1) >= 0,
            "fault counter decreased/missing")
    return {"cpu_user_seconds":cpu["user_usec"]/1_000_000,
            "cpu_kernel_seconds":cpu["system_usec"]/1_000_000,
            "minor_faults_total":memory["pgfault"] - memory["pgmajfault"],
            "major_faults_total":memory["pgmajfault"],
            "retained_bytes":after["memory.current"],
            "kernel_bytes":after["memory.stat"].get("kernel"),
            # memory.peak includes anonymous, file and kernel charges; it is deliberately not RSS.
            "cgroup_memory_peak_bytes":after["memory.peak"]}


def collect(args):
    require(args.root_authorized, "ROOT owns all profiles/benchmarks")
    require(finite_positive(args.timeout) and finite_positive(args.cleanup_timeout), "finite profile budgets required")
    spec = load_json(args.manifest, args.byte_limit)
    require(spec.get("build") == "release" and hex_digest(spec.get("source"),40), "source-pinned release required")
    require(natural(spec.get("samples"), positive=True), "positive intended sample count required")
    binary = Path(spec["binary"]).resolve(strict=True)
    require(sha256(binary) == spec["binary_sha256"], "profile binary hash mismatch")
    args.output.mkdir(parents=True, exist_ok=False)
    pinned = args.output / binary.name
    import shutil
    shutil.copy2(binary, pinned)
    require(sha256(pinned) == spec["binary_sha256"], "copied profile binary changed")
    result = {key:spec[key] for key in ("source","hardware","os","architecture","backend","workload_sha256",
              "security_sha256","resource_envelope_sha256","configuration_sha256")}
    result.update(schema="hex-completion-profile-1", build="release", scope="root-profile",
                  intended_samples=spec["samples"], coverage=spec.get("coverage",[]), samples=[],
                  binary_sha256=sha256(pinned), host=platform.platform(), gaps=[])
    cgroup = Path(spec["owned_cgroup"]).resolve(strict=True) if spec.get("owned_cgroup") else None
    if cgroup is not None:
        require(sys.platform.startswith("linux") and str(cgroup) != "/sys/fs/cgroup", "exact owned delegated cgroup required")
        require(cgroup.name == spec["owned_cgroup_name"], "cgroup ownership marker mismatch")
    for index in range(spec["samples"]):
        before = counters(cgroup, args.byte_limit)
        began = time.monotonic_ns()
        cpu_began = time.process_time_ns()
        row = command([str(pinned), *spec["args"]], args.output, f"sample-{index}", args.timeout, args.cleanup_timeout)
        elapsed = (time.monotonic_ns() - began) / 1_000_000_000
        caller = (time.process_time_ns() - cpu_began) / 1_000_000_000
        after = counters(cgroup, args.byte_limit)
        metrics = {name:None for name in METRICS}
        metrics.update(latency_seconds=elapsed, caller_cpu_seconds=caller)
        operations = spec.get("operations_per_sample")
        if natural(operations, positive=True):
            metrics["throughput_per_second"] = operations / elapsed
        if before is not None:
            metrics.update(deltas(before, after))
        # The instrumented executable writes its own explicit counters. There is
        # no guessed copy count, allocator capacity proxy or cumulative-RSS delta.
        instrument = None
        if spec.get("instrumentation"):
            instrument_path = Path(spec["instrumentation"].replace("@SAMPLE@",str(index)))
            instrument = load_json(instrument_path,args.byte_limit)
            require(instrument.get("source") == spec["source"], "instrumentation source mismatch")
            require(instrument.get("accounting_valid") is True, "invalid allocation/copy accounting")
            for name in METRICS:
                if name in instrument.get("metrics",{}):
                    metrics[name] = instrument["metrics"][name]
        verified = row["exit"] == 0 and row["reaped"] and not row["timed_out"]
        # A caller-provided independent oracle in the workload must have passed.
        # Exit alone cannot establish that the workload actually dispatched.
        raw = Path(row["log"]).read_text(errors="replace")
        require(spec["oracle_marker"] in raw, "missing actual workload oracle marker")
        result["samples"].append({"run":f"{args.output.name}/{index}", "verified":verified,
            "command":row,"metrics":metrics,"before":before,"after":after,"instrumentation":instrument})
        (args.output/"profile.json").write_text(json.dumps(result,indent=2,allow_nan=False)+"\n")
        require(verified, "profile sample failed; retained exact failure")
    result["gaps"] = sorted({name for sample in result["samples"]
                             for name,value in sample["metrics"].items() if value is None})
    (args.output/"profile.json").write_text(json.dumps(result,indent=2,allow_nan=False)+"\n")
    print(json.dumps({"profile":str(args.output/"profile.json"), "unmeasured":result["gaps"]}))
    return 0 if not result["gaps"] else 1


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized",action="store_true")
    parser.add_argument("--manifest",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    parser.add_argument("--timeout",type=float,required=True)
    parser.add_argument("--cleanup-timeout",type=float,required=True)
    return collect(parser.parse_args())


if __name__=="__main__":
    try:
        sys.exit(main())
    except (InvalidEvidence,OSError,subprocess.SubprocessError) as error:
        print(f"UNVERIFIED profile: {error}",file=sys.stderr)
        sys.exit(1)
