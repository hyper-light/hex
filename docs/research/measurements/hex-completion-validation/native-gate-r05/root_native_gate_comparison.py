#!/usr/bin/env python3
"""ROOT-only source preparation/sealing and parent-only guard comparison; UNRUN.

No compiler invocation here. ROOT builds each new empty target with the documented
commands, seals real Cargo artifacts, then runs one cell per fresh delegated scope.
The immutable a128 helper owns the existing output/pidfd/cleanup primitives.
"""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import sys
import time


HELPER_SHA = "d1b42b03247b336c47a6724d6640c114c89c9ceca81d521db637220585759531"
LEAF = "crates/hex-node/src/supervise/completion_native_gate_tests.rs"
LEAF_SHA = "20e5512d525aae24cdfc58537875d37c48e4386b311d863fcb774d4a245359fe"
SYS = "crates/hex-runtime/src/linux/sys.rs"
SYS_SHA = "f90284ac37e877201342f3c170f4ae091a82269f460fbc47815706dd73f055e4"
OLD = b"        && held_close_unexpected(&keep).is_ok()\n"
NEW = b"        && {\n            let _retained_guard_body = held_close_unexpected;\n            true\n        }\n"
CELLS = ("baseline", "bypass", "restored")
PARENT = "completion_controller_parent_death_cancels_gate"


def need(value, why):
    if not value:
        raise ValueError(why)


def sha(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as file:
        while data := file.read(1024**2):
            value.update(data)
    return value.hexdigest()


def read_json(path, expected):
    need(re.fullmatch("[0-9a-f]{64}", expected), "exact reviewed digest required")
    need(sha(path) == expected, "reviewed JSON changed")
    return json.loads(Path(path).read_text())


def put(path, data):
    with Path(path).open("x") as file:
        json.dump(data, file, indent=2, sort_keys=True)
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


def source_map(root):
    result = {}
    for path in root.rglob("*"):
        need(not path.is_symlink(), "archive source cannot contain unreviewed symlink traversal")
        if path.is_file():
            result[path.relative_to(root).as_posix()] = sha(path)
    return result


def base_helper():
    path = Path(__file__).with_name("root_native_gate_baseline.py").resolve(strict=True)
    need(sha(path) == HELPER_SHA, "a128 helper changed; no silent successor")
    spec = importlib.util.spec_from_file_location("a128_gate_primitives", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)  # __main__ is false: no actor or main invocation.
    return module


def prepare(args):
    source = args.source.resolve(strict=True)
    expected = read_json(args.manifest, args.manifest_sha256)
    need(re.fullmatch("[0-9a-f]{40}", args.source_pin), "fixed signed ROOT cohort required")
    need(isinstance(expected, dict) and source_map(source) == expected, "full reviewed source manifest mismatch")
    need(expected.get(LEAF) == LEAF_SHA and expected.get(SYS) == SYS_SHA, "exact new leaf/original sys required")
    need(args.output.is_absolute(), "absolute private output required")
    args.output = args.output.resolve()
    need(not args.output.is_relative_to(source) and not source.is_relative_to(args.output), "new private output outside input source required")
    args.output.mkdir(mode=0o700)
    cells = {}
    original = (source / SYS).read_bytes()
    need(original.count(OLD) == 1, "unique held_child isolation anchor missing")
    mutant = original.replace(OLD, NEW, 1)
    for name in CELLS:
        cell = args.output / name
        cell.mkdir()
        dest = cell / "source"
        dest.mkdir()
        # New regular files and new mtimes, never copied target/dep-info/cache.
        for relative in expected:
            rel = Path(relative)
            need(not rel.is_absolute() and ".." not in rel.parts, "source manifest path escaped")
            target = dest / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as file:
                file.write((source / rel).read_bytes())
            target.chmod(0o600 | ((source / rel).stat().st_mode & 0o111))
        if name == "bypass":
            (dest / SYS).write_bytes(mutant)
        expected_cell = dict(expected)
        expected_cell[SYS] = hashlib.sha256(mutant if name == "bypass" else original).hexdigest()
        need(source_map(dest) == expected_cell, "private copy differs beyond declared one-call overlay")
        for directory in ("target", "cargo", "temp"):
            (cell / directory).mkdir()
        need(not any((cell / "target").iterdir()), "target was not created empty")
        put(cell / "source-before.json", expected_cell)
        cells[name] = {"root": str(cell), "source": str(dest), "source_manifest": expected_cell,
                       "source_before_sha256": sha(cell / "source-before.json"),
                       "target_created_empty": True, "target_reused": False}
    need(cells["baseline"]["source_manifest"] == cells["restored"]["source_manifest"], "restoration differs from original")
    put(args.output / "prepared.json", {"schema": "hex-gate-comparison-prepared-1", "root_source_pin": args.source_pin,
        "input_manifest_sha256": args.manifest_sha256, "leaf_sha256": LEAF_SHA,
        "helper_sha256": HELPER_SHA, "driver_sha256": sha(__file__), "cells": cells,
        "mutation": {"file": SYS, "original_sha256": SYS_SHA, "mutant_sha256": hashlib.sha256(mutant).hexdigest(),
                     "attribution": "whole inherited-FD isolation guard; own and foreign references may remain"}})


def seal(args):
    plan = read_json(args.plan, args.plan_sha256)
    need(plan["schema"] == "hex-gate-comparison-prepared-1" and plan["leaf_sha256"] == LEAF_SHA,
         "wrong preparation schema/leaf")
    need(plan["driver_sha256"] == sha(__file__) and plan["helper_sha256"] == HELPER_SHA, "driver/helper changed")
    need(args.output.is_absolute() and args.output.parent.resolve(strict=True) == args.plan.parent.resolve(strict=True),
         "sealed receipt must remain beside prepared receipt, outside source/targets")
    for name in CELLS:
        row = plan["cells"][name]
        cell, source = Path(row["root"]), Path(row["source"])
        need(source_map(source) == row["source_manifest"], "source changed during compilation")
        need((cell / "cargo.exit").read_text().strip() == "0", "Cargo did not complete0")
        artifacts, finished = [], []
        with (cell / "cargo.jsonl").open() as file:
            for line in file:
                event = json.loads(line)
                if event.get("reason") == "build-finished":
                    finished.append(event.get("success"))
                if event.get("reason") == "compiler-artifact" and event.get("executable") and event.get("target", {}).get("name") == "hex_node":
                    artifacts.append(event)
        need(finished == [True] and len(artifacts) == 1, "exact completed Node libtest artifact required")
        artifact = artifacts[0]
        descriptor = artifact["target"]
        need(artifact.get("fresh") is False and descriptor.get("kind") == ["lib"] and descriptor.get("crate_types") == ["lib"] and artifact.get("profile", {}).get("test") is True,
             "cached/non-libtest Node artifact refused")
        need(Path(artifact["manifest_path"]).resolve(strict=True) == source / "crates/hex-node/Cargo.toml"
             and Path(descriptor["src_path"]).resolve(strict=True) == source / "crates/hex-node/src/lib.rs", "foreign artifact source")
        binary = Path(artifact["executable"]).resolve(strict=True)
        need(binary.is_relative_to((cell / "target").resolve(strict=True)) and artifact["executable"] in artifact["filenames"], "foreign generated executable")
        put(cell / "source-after.json", source_map(source))
        row["build"] = {"binary": str(binary), "bytes": binary.stat().st_size, "sha256": sha(binary),
                        "fresh": False, "cargo_jsonl_sha256": sha(cell / "cargo.jsonl"),
                        "cargo_stderr_sha256": sha(cell / "cargo.stderr"),
                        "source_after_sha256": sha(cell / "source-after.json")}
    plan["schema"] = "hex-gate-comparison-sealed-1"
    put(args.output, plan)


def checked_plan(path, expected):
    plan = read_json(path, expected)
    need(plan["schema"] == "hex-gate-comparison-sealed-1" and plan["leaf_sha256"] == LEAF_SHA
         and plan["driver_sha256"] == sha(__file__), "wrong sealed comparison source")
    for name in CELLS:
        row = plan["cells"][name]
        need(source_map(Path(row["source"])) == row["source_manifest"], "compiled source changed")
        build = row["build"]
        need(build["fresh"] is False and Path(build["binary"]).stat().st_size == build["bytes"]
             and sha(build["binary"]) == build["sha256"], "sealed binary changed")
    return plan


def configure(base, row):
    base.SELECTORS = (PARENT,)
    base.BINARY = Path(row["build"]["binary"])
    base.BINARY_SHA = row["build"]["sha256"]
    base.BINARY_BYTES = row["build"]["bytes"]


def raw_cause(lab, name, expected_owner):
    traces = list(lab.glob("gate-*/parent-death.evidence.json"))
    need(len(traces) == 1, "one actual parent-death trace required")
    raw = json.loads(traces[0].read_text())
    events = raw["events"]
    def one(event):
        rows = [row for row in events if row["event"] == event]
        need(len(rows) == 1, "missing/duplicate phase: " + event)
        return rows[0]
    def ns(row):
        value = row["ns_since_t0_decimal"]
        need(type(value) is str and re.fullmatch("[0-9]+", value), "missing exact event time")
        return int(value)
    origin = one("origin")
    D, G = ns(origin["D"]), ns(origin["G"])
    need(0 < D < G and type(origin["owner_started"]) is int
         and origin["owner_pid"] == expected_owner["pid"] and origin["owner_started"] == expected_owner["start_ticks"], "invalid original timing/owner birth")
    owned = one("parent_owned")
    need(type(owned["pid"]) is int and type(owned["started"]) is int, "missing actual parent identity")
    ready_event = one("stopped_phase_received")
    frame = ready_event["frame"]
    need(len(frame["identities"]) == 2 and len(frame["isolation_acks"]) == 2, "missing native identities/ACKs")
    stop = frame["kernel_stop"]
    native_stop = stop["waitid"]["Ok"]
    need(stop["signal_sent"] is True and native_stop["stopped"] is True and native_stop["stopping_signal"] == int(signal.SIGSTOP)
         and stop["pid"] == frame["identities"][1]["pid"] and stop["started"] == frame["identities"][1]["started"], "same sibling kernel STOP not witnessed")
    for identity, ack in zip(frame["identities"], frame["isolation_acks"]):
        need(type(identity["pid"]) is int and type(identity["started"]) is int
             and type(ack["pid"]) is int and type(ack["started"]) is int
             and ack["pid"] == identity["pid"] and ack["started"] == identity["started"]
             and ack["actual_byte_2_observed"] is True and ack["publication_held"] is True and ack["within_guard"] is True, "not same actual held native launch")
    parent = one("parent_terminal_reap_returned")
    d_check = one("parent_D_check")
    killed = one("parent_kill_returned")
    need(ns(ready_event["at"]) <= ns(killed["at"]) <= ns(parent["at"]) < D
         and ns(d_check["at"]) < D and d_check["before_D"] is True, "P<D missed")
    need(killed["result"] == {"Ok": None} and parent["status"]["Ok"]["signal"] == int(signal.SIGKILL), "parent terminal status not actual KILL")
    for event in ("canary_before_parent_death", "canary_after_cleanup"):
        markers = [row for row in events if row["event"] == event]
        need({row["suffix"] for row in markers} == {"first", "sibling"}
             and len(markers) == 2 and all(set(row["exists"]) == {"Ok"} and row["exists"]["Ok"] is False for row in markers), "canary fact absent/forbidden")
    need(raw["cleanup_uncertainty"] == [] and one("cleanup_complete")["uncertainty"] == [], "native cleanup uncertain")
    receipts = one("existing_receipts_written")
    need(receipts["parent_stdout"] == {"Ok": None} and receipts["gate_result"] == {"Ok": None}, "original receipt I/O failure is not a guard result")
    required_files = ["parent.stdout", "parent.stderr", "parent.stop.json", "gate-result.json"]
    required_files += [f"{stem}.{kind}.json" for stem in ("first", "sibling")
                      for kind in ("isolation-ack", "isolation-tail", "creating.record")]
    preserved = {}
    for relative in required_files:
        path = traces[0].parent / relative
        need(path.is_file() and not path.is_symlink(), "required actual raw receipt missing: " + relative)
        preserved[relative] = {"sha256": sha(path), "bytes": path.stat().st_size}
    if name == "bypass":
        failure = one("first_terminal_notification")
        need(failure["result"] == {"Err": "exact pidfd exit guard expired"}
             and raw["observation"] == failure["result"] and ns(failure["at"]) < G,
             "not the phase-witnessed general isolation failure beforeG")
        need(not any(row["event"] == "first_waitid_returned" for row in events), "unrelated mutation outcome")
        reaps = [row for row in events if row["event"] == "cleanup_native_waitid"]
        need({row["index"] for row in reaps} == {0, 1} and len(reaps) == 2
             and all(row["status"]["Ok"]["terminating_signal"] == int(signal.SIGKILL) for row in reaps), "mutant native cleanup reaps not witnessed")
    else:
        g_check = one("first_G_check")
        need(raw["observation"] == {"Ok": None} and g_check["before_G"] is True and ns(g_check["at"]) < G, "positive observation failed")
        for stem in ("first", "sibling"):
            need(ns(one(stem + "_terminal_notification")["at"]) < G
                 and one(stem + "_waitid_returned")["status"]["Ok"]["exit_status"] == 127, "native127 beforeG missing")
        stat = one("sibling_kernel_stat_after_first")["raw_stat"]
        need(stat.rsplit(")", 1)[1].split()[0] == "T", "sibling not kernel-stopped through first cancellation")
    return {"trace": str(traces[0]), "sha256": sha(traces[0]), "D_ns": str(D), "G_ns": str(G),
            "preserved_raw": preserved,
            "classification": "whole-guard semantic rejection" if name == "bypass" else "positive causal control",
            "foreign_only_claim": False, "EOF_phase_prerequisite": False}


def run_cell(args):
    base = base_helper()
    plan = checked_plan(args.plan, args.plan_sha256)
    row = plan["cells"][args.cell]
    configure(base, row)
    base.require(sys.platform == "linux" and base.platform.machine() == "x86_64" and os.geteuid() != 0 and os.getuid() == os.geteuid(), "unprivileged Linux x64 scope required")
    need(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL, "sole child ownership required")
    need(re.fullmatch(r"[A-Za-z0-9_-]+\.scope", args.unit), "exact fresh scope required")
    need(args.plan.is_absolute() and args.lab.is_absolute()
         and args.lab.parent.resolve(strict=True) == Path(row["root"]).resolve(strict=True),
         "fresh absolute lab beside this cell's source/target required")
    args.lab.mkdir(mode=0o700)
    evidence = base.Evidence(args.lab)
    def interrupted(signum, _frame):
        evidence.failures.append(f"driver cancellation signal {signum}")
        raise base.Refused(f"comparison cancelled by signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    scope = base.group_of(os.getpid())
    control, phase, process, moved, enabled = scope / "control", scope / "native-0", None, False, False
    control_created = False
    errors, observed, cleanup, cause, restored = [], None, None, None, False
    owner = base.birth(os.getpid())
    try:
        need(scope.name == args.unit and base.procs(scope) == {os.getpid()} and not base.directories(scope), "scope not fresh/sole-self")
        need(base.read(scope / "cgroup.type").strip() == "domain" and not base.read(scope / "cgroup.subtree_control").split(), "fresh domain required")
        need(base.CONTROLLERS <= set(base.read(scope / "cgroup.controllers").split()), "four controllers missing")
        limits = {key: base.read(scope / key) for key in ("cpu.max", "memory.max", "memory.swap.max", "pids.max")}
        q, p = limits["cpu.max"].split()
        need(q != "max" and int(q) == int(p) and int(limits["memory.max"]) == base.JOB_MEMORY
             and int(limits["memory.swap.max"]) == 0 and int(limits["pids.max"]) == base.PIDS, "original100%/4GiB/swap0/pids256 scope required")
        base.save(args.lab / "inputs.json", {"plan": str(args.plan), "sha256": args.plan_sha256, "cell": args.cell, "owner": owner, "limits": limits})
        control.mkdir()
        control_created = True
        base.write(control / "cgroup.procs", str(os.getpid()))
        moved = True
        need(base.procs(control) == {os.getpid()} and not base.procs(scope), "self-only move failed")
        _, unit = base.run_owned(["/usr/bin/systemctl", "--user", "show", args.unit, "--property=Delegate,ControlGroup,ActiveState"], dict(os.environ), args.lab / "unit.stdout", args.lab / "unit.stderr", time.monotonic() + base.PHASE_SECONDS, evidence)
        properties = dict(line.split("=", 1) for line in base.read(args.lab / "unit.stdout").splitlines())
        need(unit["error"] is None and unit.get("returncode") == 0 and unit["reaped"] and unit["both_eof"]
             and properties.get("Delegate") == "yes" and properties.get("ActiveState") == "active"
             and properties.get("ControlGroup") == "/" + str(scope.relative_to(base.CGROOT)), "exact delegated user scope unproved")
        need(not base.procs(scope) and base.procs(control) == {os.getpid()} and base.directories(scope) == {control}, "foreign process/child before enable")
        base.write(scope / "cgroup.subtree_control", "+cpu +io +memory +pids")
        enabled = True
        need(base.CONTROLLERS <= set(base.read(scope / "cgroup.subtree_control").split()), "four-controller readback failed")
        phase.mkdir()
        end = time.monotonic() + base.PHASE_SECONDS
        try:
            base.write(phase / "cgroup.subtree_control", "+cpu +io +memory +pids")
            actor = phase / "actor"
            actor.mkdir()
            (args.lab / "empty-rootfs").mkdir()
            cfg = args.lab / "config.json"
            shell_sha = base.digest("/bin/sh")
            base.save(cfg, base.config(args.lab, phase, shell_sha))
            env = dict(os.environ)
            env.pop("HEX_COMPLETION_GATE_ROLE", None)
            env.pop("HEX_COMPLETION_GATE_SCOPE", None)
            env.update(HEX_COMPLETION_ROOT_AUTHORIZED="1", HEX_COMPLETION_GATE_CONFIG=str(cfg), HEX_COMPLETION_GATE_BYTES=str(base.LIMIT), RUST_TEST_THREADS="1")
            argv = [sys.executable, str(Path(__file__).resolve()), "--exec-phase", str(args.plan), args.plan_sha256, args.cell,
                    args.unit, str(actor), PARENT, str(owner["pid"]), str(owner["start_ticks"]), repr(end)]
            evidence.snapshot("resources-before.json", phase, end)
            process, observed = base.run_owned(argv, env, args.lab / "case.stdout", args.lab / "case.stderr", end, evidence)
            evidence.record("actual-parent-only-result", observation=observed)
            evidence.snapshot("resources-after.json", phase, end)
        finally:
            cleanup = base.clean_phase(phase, process, end, evidence)
        # Classification follows existing owned cleanup and retained raw, never masks it.
        need(observed and observed["error"] is None and observed["reaped"] and observed["both_eof"], "incomplete native process/output")
        text = base.read(args.lab / "case.stdout")
        expected = ("FAILED", "0", "1", "0") if args.cell == "bypass" else ("ok", "1", "0", "0")
        actual = re.findall(r"(?m)^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
        need(actual == [expected] and ((observed["returncode"] != 0) == (args.cell == "bypass")), "wrong one-test outcome")
        need(cleanup["empty"] and cleanup["reaped"] and not cleanup["kill_needed"] and cleanup["error"] is None, "outer fallback/uncertainty cannot count")
        cause = raw_cause(args.lab, args.cell, observed["identity"])
        need(base.digest("/bin/sh") == shell_sha, "controller changed during native cell")
        checked_plan(args.plan, args.plan_sha256)
    except Exception as error:
        errors.append(f"{type(error).__name__}: {error}")
        evidence.record("comparison-uncertain", error=errors[-1])
    finally:
        try:
            if moved:
                need(base.directories(scope) == {control} and base.procs(control) == {os.getpid()} and not base.procs(scope), "owned phase/foreign process remains")
                if enabled:
                    base.write(scope / "cgroup.subtree_control", "-cpu -io -memory -pids")
                need(not base.read(scope / "cgroup.subtree_control").split(), "controller restoration incomplete")
                base.write(scope / "cgroup.procs", str(os.getpid()))
                control.rmdir()
                after = {key: base.read(scope / key) for key in limits}
                base.save(args.lab / "scope-restoration.json", {"limits_before": limits, "limits_after": after,
                    "procs": sorted(base.procs(scope)), "subtree_control": base.read(scope / "cgroup.subtree_control"), "owner": owner})
                need(not base.directories(scope) and base.procs(scope) == {os.getpid()} and after == limits, "original limits/self not restored")
                restored = True
            elif control_created and not base.procs(control):
                control.rmdir()
        except Exception as error:
            errors.append(f"restore: {error}")
        evidence.record("comparison-cleanup-before-oracle", cleanup=cleanup, restored=restored, errors=errors)
        errors.extend(evidence.failures)
        base.save(args.lab / "comparison-result.json", {"cell": args.cell, "observation": observed, "cause": cause,
            "cleanup": cleanup, "restored": restored, "errors": errors,
            "expected_outcome_witnessed": not errors and cause is not None and restored,
            "VM_GPU_claim": False, "foreign_only_claim": False})
        evidence.journal.close()
        members = {}
        for path in args.lab.rglob("*"):
            if path.is_symlink():
                members[str(path.relative_to(args.lab))] = {"type": "symlink", "target": os.readlink(path)}
            elif path.is_file():
                members[str(path.relative_to(args.lab))] = {"bytes": path.stat().st_size, "sha256": sha(path)}
        base.save(args.lab / "SHA256.json", members)
    return 0 if not errors and cause is not None and restored else 1


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "--exec-phase":
        path, expected, name = Path(sys.argv[2]), sys.argv[3], sys.argv[4]
        plan = read_json(path, expected)
        need(plan["schema"] == "hex-gate-comparison-sealed-1" and plan["driver_sha256"] == sha(__file__), "child plan changed")
        base = base_helper()
        configure(base, plan["cells"][name])
        sys.argv = [sys.argv[0], "--exec-phase", *sys.argv[5:]]
        base.exec_phase()
        raise ValueError("exec returned")
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    prep = modes.add_parser("prepare")
    prep.add_argument("--source", type=Path, required=True)
    prep.add_argument("--source-pin", required=True)
    prep.add_argument("--manifest", type=Path, required=True)
    prep.add_argument("--manifest-sha256", required=True)
    prep.add_argument("--output", type=Path, required=True)
    for name in ("seal", "cell"):
        command = modes.add_parser(name)
        command.add_argument("--plan", type=Path, required=True)
        command.add_argument("--plan-sha256", required=True)
        if name == "seal":
            command.add_argument("--output", type=Path, required=True)
        else:
            command.add_argument("--cell", choices=CELLS, required=True)
            command.add_argument("--unit", required=True)
            command.add_argument("--lab", type=Path, required=True)
    args = parser.parse_args()
    if args.mode == "prepare":
        prepare(args)
        return 0
    if args.mode == "seal":
        seal(args)
        return 0
    return run_cell(args)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"REFUSE {type(error).__name__}: {error}", file=sys.stderr)
        raise SystemExit(2)
