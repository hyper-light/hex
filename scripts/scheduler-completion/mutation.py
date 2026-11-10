#!/usr/bin/env python3
"""ROOT-only one exact guard mutation, relevant red oracle, byte restoration and green rerun."""

import argparse
import json
import os
import re
import signal
import subprocess
import sys
from pathlib import Path

from evidence import InvalidEvidence, finite_positive, load_json, require, sha256


def replacement(original, old, new):
    require(bool(old) and old != new, "mutation must change one nonempty exact guard")
    require(original.count(old) == 1, "guard does not occur exactly once at pinned source")
    return original.replace(old, new, 1)


def command(argv, root, name, timeout, cleanup_timeout):
    require(isinstance(argv, list) and argv and all(isinstance(part, str) and part for part in argv),
            "exact argv array required, never a shell command")
    path = root / f"{name}.log"
    with path.open("xb") as log:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        row = {"argv": argv, "pid": child.pid, "timed_out": False}
        try:
            child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            row["timed_out"] = True
            child.terminate()
            try:
                child.wait(timeout=cleanup_timeout)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=cleanup_timeout)
        finally:
            row.update(exit=child.returncode, reaped=child.poll() is not None, log=str(path),
                       sha256=sha256(path), native_cleanup="requires fixture's own native observers")
    return row


def execute(args):
    require(args.root_authorized, "only ROOT may run a mutation campaign")
    spec = load_json(args.spec, args.byte_limit)
    require(finite_positive(args.timeout) and finite_positive(args.cleanup_timeout), "finite budgets required")
    scope = args.source_root.resolve(strict=True)
    filename = Path(spec["file"])
    require(not filename.is_absolute() and ".." not in filename.parts, "mutation path escaped private source")
    path = scope / filename
    require(not path.is_symlink() and path.resolve(strict=True).is_relative_to(scope), "nonregular mutation source")
    require(sha256(path) == spec["original_sha256"], "source guard revision/hash mismatch")
    original = path.read_bytes()
    changed = replacement(original, spec["old"].encode(), spec["new"].encode())
    args.output.mkdir(parents=True, exist_ok=False)
    saved = args.output / "original.bytes"
    saved.write_bytes(original)
    receipt = {"schema":"hex-completion-mutation-1","source":spec["source"],"file":str(path),
               "before_sha256":sha256(path),"original_copy":str(saved.resolve()),
               "restored":False,"phases":[]}
    def persist():
        (args.output / "mutation.json").write_text(json.dumps(receipt, indent=2) + "\n")
    def invoke(name, argv):
        row = command(argv, args.output, name, args.timeout, args.cleanup_timeout)
        receipt["phases"].append({"name":name, **row})
        persist()
        require(not row["timed_out"] and row["reaped"], f"{name} timed out/unreaped; not a meaningful guard control")
        return row
    persist()
    try:
        green = invoke("baseline", spec["verify"])
        require(green["exit"] == 0, "baseline is not green; mutation cannot establish a regression")
        path.write_bytes(changed)
        receipt["mutated_sha256"] = sha256(path)
        persist()
        build = invoke("mutant-build", spec["build"])
        require(build["exit"] == 0, "mutant failed to build; no behavioral evidence")
        red = invoke("mutant", spec["verify"])
        raw = Path(red["log"]).read_text(errors="replace")
        require(red["exit"] != 0, "removed guard did not make its oracle fail")
        require(re.search(spec["relevant_failure"], raw) is not None, "red failure does not witness the forbidden effect")
        require(not re.search(r"could not compile|error\[E\d+\]|linker .*failed|UNVERIFIED", raw),
                "compile/unavailable failure is not a meaningful guard mutation")
        receipt["relevant_red"] = True
    finally:
        # Refuse to overwrite any foreign edit, even inside ROOT's disposable checkout.
        current = path.read_bytes()
        require(current in (original, changed), "source changed outside this mutation; preserve before restore")
        path.write_bytes(original)
        receipt["restored"] = sha256(path) == spec["original_sha256"]
        persist()
    require(receipt["restored"], "source restoration hash mismatch")
    restored_build = invoke("restored-build", spec["build"])
    require(restored_build["exit"] == 0, "restored source did not build")
    restored = invoke("restored", spec["verify"])
    require(restored["exit"] == 0, "restored guard did not return green")
    receipt["outcome"] = "verified"
    persist()
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized", action="store_true")
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--byte-limit", type=int, required=True)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--cleanup-timeout", type=float, required=True)
    return execute(parser.parse_args())


if __name__ == "__main__":
    # SIGTERM becomes an ordinary unwind so byte restoration executes; SIGKILL is
    # still uncertain and ROOT can restore original.bytes only after hash review.
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(InterruptedError("ROOT cancellation")))
    try:
        sys.exit(main())
    except (InvalidEvidence, OSError, subprocess.SubprocessError) as error:
        print(f"UNVERIFIED mutation: {error}", file=sys.stderr)
        sys.exit(1)
