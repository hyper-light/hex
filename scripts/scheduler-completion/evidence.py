#!/usr/bin/env python3
"""AC-T7.3.1/4.1: fail-closed acceptance receipts, and the process-group ownership their drivers share;
no native work is performed here."""

import hashlib
import json
import math
import os
import re
import signal
import subprocess
import time
from pathlib import Path, PureWindowsPath


class InvalidEvidence(ValueError):
    pass


BACKENDS = ("oci", "firecracker", "shards")
TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl",
    "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc", "aarch64-apple-darwin",
)
STAGES = (
    "capability", "binding", "compute", "foreign", "wrong-id",
    "scheduler-recovery", "runtime-recovery", "cleanup", "complete",
)
SELECTED_STAGES = ("capability", "binding", "compute", "cleanup", "complete")
POLICY = "0342b85d9c808e3ced041a6c3ef0e9bf70438272"
RULES = "df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3"


def require(condition, why):
    if not condition:
        raise InvalidEvidence(why)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path, byte_limit):
    with Path(path).open("rb") as source:
        raw = source.read(byte_limit + 1)
    require(len(raw) <= byte_limit, f"JSON exceeds admitted byte bound: {path}")
    try:
        return json.loads(raw, object_pairs_hook=unique_object,
                          parse_constant=lambda value: (_ for _ in ()).throw(
                              InvalidEvidence(f"nonfinite JSON: {value}")))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise InvalidEvidence(f"invalid JSON: {path}: {error}") from error


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def hex_digest(value, length=64):
    return isinstance(value, str) and re.fullmatch(f"[0-9a-f]{{{length}}}", value) is not None


def natural(value, positive=False):
    return type(value) is int and value >= int(positive)


def finite_positive(value):
    return type(value) in (int, float) and math.isfinite(value) and value > 0


def start_group(argv, **options):
    """Start argv leading a session of its own: its process group (its PID) holds every descendant that
    does not leave it, and end_group ends that whole group. A signal to the driver's own group no longer
    reaches it, so drivers turn SIGTERM into an unwind that ends it first (unwind_on_sigterm)."""
    # Windows has no process group to signal or probe: the descendants would go unwitnessed.
    require(os.name == "posix", "ending a child's process group needs POSIX")
    return subprocess.Popen(argv, start_new_session=True, **options)


def group_alive(pgid):
    """Signal 0 to the group: only ProcessLookupError says no process is left in it."""
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        pass  # a member this user may not signal is still there
    return True


def group_ended(child, budget):
    """Reap the leader, then probe its group until no process is left, within `budget` seconds."""
    deadline = time.monotonic() + budget
    try:
        child.wait(timeout=budget)
    except subprocess.TimeoutExpired:
        return False
    # The other members are other parents' children: no wait or notification reaches them, so signal 0
    # is probed at the cadence Popen.wait(timeout) polls its own child (CPython Lib/subprocess.py, _wait).
    delay = 0.0005
    while group_alive(child.pid):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        delay = min(delay * 2, remaining, 0.05)
        time.sleep(delay)
    return True


def end_group(child, cleanup_timeout):
    """End what is left of the group `child` leads (start_group), on every path: TERM, then KILL once it
    outlasts `cleanup_timeout`, each phase bounded by it. Observed, never assumed: `reaped` is the leader
    alone; `group_gone` holds only once signal 0 finds no process left in the group, and descendants that
    left the group are outside that witness; `group_signaled` says the driver had to end something."""
    signaled = False
    for signum in (signal.SIGTERM, signal.SIGKILL):
        if child.poll() is not None and not group_alive(child.pid):
            break
        try:
            os.killpg(child.pid, signum)
            signaled = True
        except (ProcessLookupError, PermissionError):
            pass
        if group_ended(child, cleanup_timeout):
            break
    reaped = child.poll() is not None
    return {"reaped": reaped, "group_signaled": signaled, "group_gone": reaped and not group_alive(child.pid)}


def unwind_on_sigterm():
    """SIGTERM (a guard's, or ROOT's cancellation) becomes one InterruptedError, an OSError the drivers
    report as UNVERIFIED, so their finally ends the child's group before they exit. Later TERMs are
    ignored: GNU timeout signals the driver and then its group, and that must not cut the cleanup short."""
    def cancel(*_):
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        raise InterruptedError("ROOT cancellation")
    signal.signal(signal.SIGTERM, cancel)


def artifact(root, row):
    """Only retained regular files within the receipt directory count as evidence."""
    require(isinstance(row, dict), "missing artifact receipt")
    name = row.get("path")
    require(isinstance(name, str) and name, "missing artifact path")
    require("\\" not in name and not PureWindowsPath(name).drive, "portable POSIX relative artifact path required")
    relative = Path(name)
    require(not relative.is_absolute() and ".." not in relative.parts, "artifact escaped run")
    base = Path(root).resolve(strict=True)
    path = base / relative
    # Reject every symlink component, including one that currently resolves back into the run.
    cursor = base
    for part in relative.parts:
        cursor = cursor / part
        require(not cursor.is_symlink(), "symlink is not retained evidence")
    require(path.is_file() and path.resolve(strict=True).is_relative_to(base), "artifact missing")
    require(hex_digest(row.get("sha256")) and sha256(path) == row["sha256"], "artifact hash mismatch")
    require(natural(row.get("bytes")) and path.stat().st_size == row["bytes"], "artifact size mismatch")
    return path


def libtest_result(raw, expected):
    """Require one exact libtest invocation, with no ignored/skipped or filtered intended case."""
    require(natural(expected, positive=True), "intended selected count must be positive")
    summaries = re.findall(
        r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;",
        raw, flags=re.MULTILINE)
    require(len(summaries) == 1, "missing/ambiguous libtest summary")
    status, passed, failed, ignored, measured, _filtered = summaries[0]
    require((status, int(passed), int(failed), int(ignored), int(measured)) ==
            ("ok", expected, 0, 0, 0), "wrong selected count or nonpassing cases")
    require(not re.search(r"\b(SKIP|UNVERIFIED|Unsupported|simulated|compile-only)\b", raw),
            "native proof contains an unavailable/simulated path")


def check_events(events, backend, topology, proof="full"):
    require(backend in BACKENDS and topology in ("laptop", "peer"), "invalid case identity")
    expected = STAGES if proof == "full" else SELECTED_STAGES
    require(proof in ("full", "selected"), "unknown proof scope")
    require([event.get("stage") for event in events] == list(expected), "missing/repeated/out-of-order native stage")
    for event in events:
        require(event.get("backend") == backend and event.get("topology") == topology,
                "event case identity mismatch")
        require(event.get("outcome") == "verified", f"unverified stage: {event.get('stage')}")
    capability, binding, compute = events[:3]
    cleanup, complete = events[-2:]
    require(capability.get("runtime_class") == backend, "scheduler label is not a native Runtime class")
    require(capability.get("native_process_verified") is True, "no independent native process witness")
    require(hex_digest(capability.get("native_executable_sha256")), "native executable not pinned")
    selected = binding.get("selected")
    require(isinstance(selected, list) and len(selected) == 1, "fixture requires exactly one selected physical GPU")
    require(type(binding.get("holder_count")) is int and binding.get("holder_count") == 1 and binding.get("durable_before_execution") is True,
            "no single durable holder before execution")
    require(hex_digest(compute.get("uuid"), 32), "no actual physical GPU UUID")
    require(compute.get("selected") == selected and compute.get("native_join_verified") is True,
            "GPU UUID is not independently joined to the exact selected native identity")
    require(selected[0].get("kind") == "gpu" and natural(selected[0].get("key")),
            "selected identity is not a native GPU")
    joins=[]
    translation=compute.get("physical_translation")
    if backend=="firecracker":
        require(isinstance(translation,dict) and translation.get("native_key")==selected[0]["key"],
                "no actual VMM host-to-guest translation")
        host=translation.get("host_pci")
        require(isinstance(host,list) and len(host)==4 and all(natural(part) for part in host),
                "missing native host PCI")
        domain,bus,slot,function=host
        require(domain<=0xffffffff and bus<=255 and slot<=31 and function<=7,"invalid native host PCI")
        require((domain<<16)|(bus<<8)|(slot<<3)|function==selected[0]["key"],"wrong VMM host PCI key")
    for device in compute.get("native_inventory",[]):
        luid=device.get("luid")
        if luid is not None:
            mask=device.get("node_mask")
            require(hex_digest(luid,16) and natural(mask,positive=True) and mask&(mask-1)==0,
                    "linked-node LUID does not identify one exact physical node")
            require(sum(row.get("luid")==luid for row in compute["native_inventory"])==1,
                    "LUID is shared by multiple physical Vulkan devices")
        pci=device.get("pci")
        if pci is not None:
            require(isinstance(pci,list) and len(pci)==4 and all(natural(part) for part in pci),
                    "malformed native PCI correlation")
            domain,bus,slot,function=pci
            require(domain<=0xffffffff and bus<=255 and slot<=31 and function<=7,"invalid PCI domain")
            key=(domain<<16)|(bus<<8)|(slot<<3)|function
        else:
            key=device.get("metal_registry_id")
        matches=(key==selected[0]["key"]) if backend!="firecracker" else (pci==translation.get("guest_pci"))
        if matches:
            joins.append(device.get("uuid"))
    require(joins==[compute["uuid"]],"UUID/native physical join missing, ambiguous or inconsistent")
    require(natural(compute.get("words"), positive=True)
            and natural(compute.get("mismatches")) and compute["mismatches"] == 0,
            "vacuous or incorrect workload")
    require(natural(compute.get("prefill_mismatches"), positive=True)
            and compute["prefill_mismatches"] == compute["words"], "output was not poisoned before dispatch")
    require(compute.get("hardware") is True and compute.get("dispatch_completed") is True,
            "no completed hardware dispatch")
    if proof == "full":
        foreign, wrong, scheduler, runtime = events[3:7]
        require(natural(foreign.get("host_accessible"), positive=True)
                and natural(foreign.get("guest_accessible")) and foreign["guest_accessible"] == 0,
                "foreign-device denial lacks a live positive host control")
        require(natural(wrong.get("dispatches")) and wrong["dispatches"] == 0 and wrong.get("refused") is True,
                "wrong identity reached GPU dispatch")
        for recovery in (scheduler, runtime):
            require(recovery.get("selected") == selected and natural(recovery.get("holder_count"), positive=True)
                    and recovery["holder_count"] == 1,
                    "recovery repicked or duplicated the physical holder")
            require(recovery.get("compute_verified") is True and recovery.get("same_physical_box") is True,
                    "recovery did not retain the running workload")
        require(runtime.get("catalog_reordered") is True and runtime.get("load_order_changed") is True,
                "recovery never challenged catalog/load ordering")
    require(all(natural(cleanup.get(name)) and cleanup[name] == 0
                for name in ("boxes", "holders", "owned_native_processes", "owned_artifacts")),
            "native cleanup incomplete")
    require(natural(complete.get("selected_stages"), positive=True)
            and complete["selected_stages"] == len(expected), "vacuous completion")
    return {"backend": backend, "topology": topology, "proof": proof, "native_stages": len(expected),
            "physical":binding.get("physical"),"key":binding.get("key"),
            "remaining": [] if proof == "full" else ["foreign-device", "wrong-id", "scheduler-recovery", "runtime-recovery"]}


def check_run(root, receipt, byte_limit):
    require(receipt.get("schema") == "hex-completion-run-1", "unknown run schema")
    require(hex_digest(receipt.get("source"), 40), "source revision missing")
    require(receipt.get("policy") == POLICY and receipt.get("rules_sha256") == RULES, "stale policy")
    proof = receipt.get("proof")
    require(proof in ("full", "selected"), "proof scope missing")
    require(receipt.get("scope") == ("root-native" if proof == "full" else "root-native-selected"),
            "light/model/compile receipt is not native proof")
    require(receipt.get("exit") == 0 and receipt.get("timed_out") is False and receipt.get("reaped") is True and
            receipt.get("group_gone") is True, "failed/unreaped native invocation, or its process group not gone")
    # A child that exits leaving processes in its group leaked them, though the driver then ended them.
    require(receipt.get("group_signaled") is False, "the native invocation left processes running past its exit")
    binary = artifact(root, receipt.get("binary"))
    require(binary.stat().st_size > 0, "empty binary")
    build_path=artifact(root,receipt.get("build"))
    build=load_json(build_path,byte_limit)
    require(build.get("schema")=="hex-completion-build-1" and build.get("source")==receipt["source"] and
            build.get("target")==receipt["target"] and build.get("test")=="completion_backends" and
            build.get("exit")==0 and build.get("timed_out") is False and build.get("reaped") is True and
            build.get("group_gone") is True and build.get("source_unchanged") is True and
            build.get("binary",{}).get("sha256")==receipt["binary"]["sha256"],
            "run is not tied to its actual successful pinned-source build")
    require(build.get("group_signaled") is False, "the fixture build left processes running past its exit")
    log = artifact(root, receipt.get("log"))
    require(log.stat().st_size <= byte_limit, "native log exceeds admitted byte bound")
    libtest_result(log.read_text(encoding="utf-8"), 1)
    event_file = artifact(root, receipt.get("events"))
    events = load_json(event_file, byte_limit)
    require(isinstance(events, list), "event array required")
    return check_events(events, receipt.get("backend"), receipt.get("topology"), proof)


def matrix(rows):
    """Compilation, native workloads and peer-routing are separate obligations."""
    required = {(target, backend, topology) for target in TARGETS
                for backend in BACKENDS for topology in ("laptop", "peer")
                if not (backend == "firecracker" and topology == "laptop" and target not in TARGETS[:4])}
    seen = {}
    for row in rows:
        key = row.get("target"), row.get("backend"), row.get("topology")
        require(key in required and key not in seen, "unexpected/duplicate matrix cell")
        require(row.get("outcome") in ("verified", "failed", "unverified"), "invalid matrix outcome")
        # Firecracker is a Linux KVM VMM. Non-Linux origin cells need actual peer execution on Linux.
        if row.get("outcome") == "verified":
            require(row.get("native_receipt_verified") is True, "matrix claims native from compile/metadata")
            if key[1] == "firecracker":
                require(row.get("execution_target") in TARGETS[:4], "Firecracker did not execute on Linux")
                if key[0] not in TARGETS[:4]:
                    require(row.get("routed_to_capable_peer") is True, "foreign origin not routed to initialized KVM peer")
        seen[key] = row
    missing = sorted(required - seen.keys())
    gaps = [{"target": key[0], "backend": key[1], "topology": key[2], "outcome": "unverified",
             "reason": "missing native receipt"} for key in missing]
    gaps.extend(row for row in rows if row["outcome"] != "verified")
    return {"required_cells": len(required), "verified_cells": len(required) - len(gaps),
            "complete": not gaps, "gaps": gaps}
