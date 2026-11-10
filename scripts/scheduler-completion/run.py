#!/usr/bin/env python3
"""ROOT-only bounded invocation of a source-pinned completion test binary."""

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

from evidence import (BACKENDS, POLICY, RULES, InvalidEvidence, check_run,
                      finite_positive, hex_digest, load_json, require, sha256)


def file_receipt(root, path):
    return {"path": str(path.relative_to(root)), "sha256": sha256(path), "bytes": path.stat().st_size}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def run(args):
    require(args.root_authorized, "native invocation is ROOT-only; explicit --root-authorized required")
    require(hex_digest(args.source, 40) and hex_digest(args.binary_sha256), "exact source/binary pins required")
    require(finite_positive(args.timeout) and finite_positive(args.cleanup_timeout), "finite run/cleanup budgets required")
    require(args.byte_limit > 0, "positive admitted artifact byte bound required")
    binary = args.binary.resolve(strict=True)
    require(sha256(binary) == args.binary_sha256, "input binary hash mismatch")
    config = args.config.resolve(strict=True)
    value = load_json(config, args.byte_limit)
    require(value.get("source") == args.source and value.get("backend") == args.backend and
            value.get("topology") == args.topology, "fixture source/case mismatch")
    # No generic command or shell string can accidentally select a stale fixture or cargo build.
    test_name = f"completion_{args.backend}_{args.topology}"
    command = [str(binary), "--ignored", "--exact", test_name, "--nocapture", "--test-threads=1"]
    root = args.output.resolve()
    require(not root.exists(), "unique new output directory required")
    root.mkdir(parents=True)
    pinned = root / ("test-binary.exe" if os.name == "nt" else "test-binary")
    shutil.copy2(binary, pinned)
    shutil.copy2(config, root / "fixture.json")
    events = root / "events.json"
    env = dict(os.environ)
    # No admin credentials are copied into receipts. Only this scope's fixture path is recorded.
    env.update(HEX_COMPLETION_CONFIG=str(config), HEX_COMPLETION_EVENTS=str(events),
               HEX_COMPLETION_ROOT_AUTHORIZED="1", RUST_TEST_THREADS="1",
               CARGO_BUILD_JOBS="1", RAYON_NUM_THREADS="1")
    for name in list(env):
        if name.startswith(("DYLD_", "HEX_DEBUG_", "SHARDS_DEBUG_")):
            del env[name]
    receipt = {"schema": "hex-completion-run-1", "scope": "root-native", "policy": POLICY,
               "rules_sha256": RULES, "source": args.source, "backend": args.backend,
               "topology": args.topology, "target": args.target, "command": command,
               "timeout_seconds": args.timeout, "cleanup_timeout_seconds": args.cleanup_timeout,
               "environment": {"platform": platform.platform(), "machine": platform.machine()},
               "binary": file_receipt(root, pinned), "fixture": file_receipt(root, root / "fixture.json"),
               "timed_out": False, "reaped": False, "exit": None}
    log_path = root / "test.log"
    began = time.monotonic_ns()
    with log_path.open("xb") as log:
        child = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                                 stdout=log, stderr=subprocess.STDOUT, shell=False)
        receipt["pid"] = child.pid
        write_json(root / "run.json", receipt)
        try:
            receipt["exit"] = child.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            receipt["timed_out"] = True
            # This exact Popen handle is ours; no process-name search or foreign kill.
            child.terminate()
            try:
                child.wait(timeout=args.cleanup_timeout)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=args.cleanup_timeout)
            receipt["exit"] = child.returncode
        finally:
            receipt["reaped"] = child.poll() is not None
            receipt["elapsed_ns"] = time.monotonic_ns() - began
            receipt["log"] = file_receipt(root, log_path)
            if events.is_file():
                receipt["events"] = file_receipt(root, events)
            # A timeout never becomes cleanup proof; retain the fixture state/owned PID journal.
            write_json(root / "run.json", receipt)
    try:
        verified = check_run(root, receipt, args.byte_limit)
        receipt["assessment"] = {"outcome": "verified", **verified}
    except (InvalidEvidence, OSError) as error:
        receipt["assessment"] = {"outcome": "unverified" if receipt["timed_out"] else "failed",
                                 "reason": str(error)}
    write_json(root / "run.json", receipt)
    print(json.dumps({"receipt": str(root / "run.json"), "assessment": receipt["assessment"]}))
    return 0 if receipt["assessment"]["outcome"] == "verified" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized", action="store_true")
    parser.add_argument("--source", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--backend", choices=BACKENDS, required=True)
    parser.add_argument("--topology", choices=("laptop", "peer"), required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--cleanup-timeout", type=float, required=True)
    parser.add_argument("--byte-limit", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    try:
        return run(parser.parse_args())
    except (InvalidEvidence, OSError, subprocess.SubprocessError) as error:
        print(f"UNVERIFIED: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
