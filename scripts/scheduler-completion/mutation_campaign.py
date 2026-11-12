#!/usr/bin/env python3
"""ROOT-only guard campaign with complete source restoration and typed native effects."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from build import source_state
from combined_oracle import mutation
from evidence import (BACKENDS, InvalidEvidence, artifact, finite_positive, hex_digest, load_json, require, sha256,
                      unwind_on_sigterm)
from mutation import command, replacement
from run import file_receipt, write_json


STEPS = ("observe_before", "baseline", "mutant_build", "mutant", "restored_build", "restored", "observe_after")


def output_path(root, value):
    require(isinstance(value, str) and value, "exact relative campaign output path required")
    relative = Path(value)
    require(not relative.is_absolute() and ".." not in relative.parts, "campaign output escaped private scope")
    cursor = root
    for part in relative.parts:
        cursor /= part
        require(not cursor.is_symlink(), "campaign output cannot traverse a symlink")
    return cursor


def execute(args):
    require(args.root_authorized, "only ROOT may execute native guard mutations")
    require(finite_positive(args.timeout) and finite_positive(args.cleanup_timeout) and args.byte_limit > 0,
            "explicit finite campaign envelope required")
    spec = load_json(args.spec, args.byte_limit)
    require(spec.get("schema") == "hex-completion-mutation-plan-2" and hex_digest(spec.get("source"), 40)
            and spec.get("backend") in BACKENDS and isinstance(spec.get("guard"), str) and spec["guard"],
            "fixed source/backend/guard plan required")
    steps = spec.get("steps")
    outputs = spec.get("outputs")
    require(isinstance(steps, dict) and set(steps) == set(STEPS) and isinstance(outputs, dict),
            "all bounded baseline/mutant/restored/resource steps required")
    scope = args.source_root.resolve(strict=True)
    root = args.output.resolve()
    require(not root.exists() and not root.is_relative_to(scope), "new campaign output outside private source required")
    root.mkdir(parents=True)
    # Full source observations are made by the same build input checker, without
    # running Cargo. The only admitted dirty state is the one declared overlay.
    before = source_state(scope, spec["source"])
    path = output_path(scope, spec.get("file"))
    require(path.is_file() and hex_digest(spec.get("original_sha256"))
            and sha256(path) == spec["original_sha256"], "original named guard hash mismatch")
    original = path.read_bytes()
    require(isinstance(spec.get("old"), str) and isinstance(spec.get("new"), str), "literal byte replacement required")
    changed = replacement(original, spec["old"].encode(), spec["new"].encode())
    changed_hash = hashlib.sha256(changed).hexdigest()
    overlay = {"file": spec["file"], "original_sha256": spec["original_sha256"], "sha256": changed_hash}
    write_json(root / "source-before.json", before)
    (root / "original.bytes").write_bytes(original)
    receipt = {"schema": "hex-completion-mutation-2", "source": spec["source"], "backend": spec["backend"],
               "guard": spec["guard"], "file": spec["file"], "original_sha256": spec["original_sha256"],
               "mutated_sha256": changed_hash, "owned_boxes": spec.get("owned_boxes"),
               "before_manifest": file_receipt(root, root / "source-before.json"), "phases": [],
               "source_restored": False, "outcome": "unverified", "native_cleanup": "requires exact native scope receipts"}
    write_json(root / "plan.json", spec)

    def journal():
        write_json(root / "mutation.json", receipt)

    def invoke(name, expected_green=True):
        step = steps[name]
        require(isinstance(step, dict) and isinstance(step.get("argv"), list)
                and hex_digest(step.get("executable_sha256")), "source-pinned exact step argv required")
        values = {"@OUTPUT@": str(root), "@SOURCE_ROOT@": str(scope), "@SOURCE@": spec["source"],
                  "@GUARD_FILE@": spec["file"], "@ORIGINAL_SHA256@": spec["original_sha256"],
                  "@MUTATED_SHA256@": changed_hash}
        argv = []
        for part in step["argv"]:
            require(isinstance(part, str) and part, "step argv must contain nonempty strings")
            for token, value in values.items():
                part = part.replace(token, value)
            argv.append(part)
        require(argv and Path(argv[0]).is_absolute() and sha256(Path(argv[0])) == step["executable_sha256"],
                "step executable changed")
        # run/build scripts inside these argv retain their own copied binaries,
        # fixed source, native-process and descendant uncertainty receipts.
        def started(row):
            row["name"] = name
            receipt["phases"].append(row)
            journal()
        try:
            row = command(["nice", "-n", "10", *argv], root, name, args.timeout, args.cleanup_timeout, started)
        finally:
            journal()
        require(sha256(Path(argv[0])) == step["executable_sha256"], "step executable changed during invocation")
        require(row["reaped"] and row["group_gone"] and not row["timed_out"] and type(row["exit"]) is int,
                "campaign phase timed out/unreaped/left its process group; native cleanup remains uncertain")
        require((row["exit"] == 0) if expected_green else (row["exit"] != 0),
                "campaign phase did not produce its required actual exit")

    def named_output(name):
        path = output_path(root, outputs.get(name))
        require(path.is_file(), "campaign step did not retain its declared output: " + name)
        return path

    journal()
    failure = None
    try:
        invoke("observe_before")
        invoke("baseline")
        baseline = named_output("baseline")
        receipt["baseline"] = file_receipt(root, baseline)
        path.write_bytes(changed)
        mutated = source_state(scope, spec["source"], overlay)
        write_json(root / "source-mutated.json", mutated)
        receipt["mutated_manifest"] = file_receipt(root, root / "source-mutated.json")
        journal()
        invoke("mutant_build")
        invoke("mutant", expected_green=False)
    except (InvalidEvidence, OSError, subprocess.SubprocessError, InterruptedError) as error:
        failure = str(error)
        receipt["failure"] = failure
    finally:
        # Foreign edits remain untouched, even in this disposable source checkout.
        current = path.read_bytes()
        require(current in (original, changed), "foreign guard edit observed; preserve original.bytes for ROOT")
        path.write_bytes(original)
        restored = source_state(scope, spec["source"])
        write_json(root / "source-restored.json", restored)
        receipt["restored_manifest"] = file_receipt(root, root / "source-restored.json")
        receipt["source_restored"] = restored == before
        journal()
    require(receipt["source_restored"], "source manifest did not restore exactly")
    # A red step never licenses calling the campaign passed. Resource observation
    # is still attempted after byte restoration, even on an earlier error.
    try:
        if failure is None:
            invoke("restored_build")
            invoke("restored")
        invoke("observe_after")
    except (InvalidEvidence, OSError, subprocess.SubprocessError, InterruptedError) as error:
        receipt["cleanup_or_restored_failure"] = str(error)
        failure = failure or str(error)
        journal()
    if failure is not None:
        return 1
    mutant_path = named_output("mutant")
    mutant = load_json(mutant_path, args.byte_limit)
    mutant_build = named_output("mutant_build")
    receipt["mutant"] = {name: mutant.get(name) for name in ("exit", "reaped", "timed_out", "group_gone")}
    for name in ("binary", "log"):
        receipt["mutant"][name] = file_receipt(root, artifact(mutant_path.parent, mutant.get(name)))
    receipt["mutant"]["build"] = file_receipt(root, mutant_build)
    for name in ("restored", "forbidden_effect", "before_resources", "after_resources"):
        receipt[name] = file_receipt(root, named_output(name))
    try:
        receipt["assessment"] = mutation(root, receipt, spec["source"], spec["backend"], args.byte_limit)
        receipt["outcome"] = receipt["assessment"]["outcome"]
    except (InvalidEvidence, OSError, UnicodeError) as error:
        receipt["assessment"] = {"outcome": "unverified", "reason": str(error)}
    journal()
    print(json.dumps({"receipt": str(root / "mutation.json"), "outcome": receipt["outcome"]}))
    return 0 if receipt["outcome"] == "verified" else 1


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
    # SIGTERM becomes one unwind, so the restoration in execute's finally runs and each phase's group ends.
    unwind_on_sigterm()
    for name in ("CARGO_BUILD_JOBS", "RUST_TEST_THREADS", "RAYON_NUM_THREADS"):
        os.environ[name] = "1"
    try:
        sys.exit(main())
    except (InvalidEvidence, OSError, subprocess.SubprocessError, InterruptedError) as error:
        print(f"UNVERIFIED guard campaign: {error}", file=sys.stderr)
        sys.exit(1)
