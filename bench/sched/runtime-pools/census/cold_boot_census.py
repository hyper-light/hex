#!/usr/bin/env python3
"""ROOT ONLY: fresh ordinary node/guest; measuring Hello cohort or armed validation.

No boxes, workload, full-boot timing, benchmark result or seed installation is produced. Root owns
all real execution, data review, cancellation and retirement. Synthetic tests inject no live guests.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import sys


def load_census():
    spec = importlib.util.spec_from_file_location("cold_boot_existing_census", Path(__file__).with_name("scheduler_census.py"))
    if spec is None or spec.loader is None:
        raise RuntimeError("the existing scheduler census could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


census = load_census()
HELLO = "client.guest.answer.hello"
# Source075fd11a: WithGuest::routing->GuestRuntime::boot->guest Client::connect, followed by
# Recorded::attach->join_loop->join Client::connect_evented. One Hello at each call site normally.
# The phase count is a source-derived consistency check, not proof of a particular VM identity.
NORMAL_HELLO_PHASES = ("GuestRuntime.guest initial client", "Recorded.attach evented join")


def report_kind(validate_seeds):
    return "cold-node-seeded-validation" if validate_seeds else "cold-node-boot-SDK-Hello-census"


def classify_hellos(lines, row_bound, line_bound):
    answered, refused, run_out = [], [], []
    malformed, full, rows = [], [], 0
    other = {}
    for number, raw in enumerate(lines, 1):
        rows = number
        if number > row_bound + 1:
            malformed.append({"line": number, "why": "row bound exceeded"})
            break
        if len(raw) > line_bound or not raw.endswith(b"\n"):
            malformed.append({"line": number, "why": "line bound or final newline"})
            continue
        try:
            text = raw.decode("utf-8")
        except UnicodeError:
            malformed.append({"line": number, "why": "not UTF-8"})
            continue
        if text.startswith("census full at "):
            full.append({"line": number, "raw": text})
            continue
        match = re.fullmatch(r"(\S+) ([!>]?)([0-9]+)\n", text)
        if match is None:
            malformed.append({"line": number, "why": "unknown census syntax", "raw": text})
            continue
        name, marker, nanos = match.groups()
        if name != HELLO:
            other[name] = other.get(name, 0) + 1
            continue
        try:
            nanos = int(nanos)
        except ValueError:
            malformed.append({"line": number, "why": "unrepresentable integer", "raw": text})
            continue
        sample = {"line": number, "name": name, "marker": marker, "nanos": nanos, "raw": text}
        {"": answered, "!": refused, ">": run_out}[marker].append(sample)
    ordinary = len(answered) == len(NORMAL_HELLO_PHASES) and not refused and not run_out and not malformed and not full
    return {"kind": "backend-process-first-completed-SDK-Hello", "rows": rows,
            "first_sdk_hello": answered[0] if answered else None, "subsequent_answered_hellos": answered[1:],
            "refused_hellos": refused, "run_out_hellos": run_out, "other_wait_rows": other,
            "malformed": malformed, "full_markers": full, "normal_hello_phases": list(NORMAL_HELLO_PHASES),
            "normal_answered_count": len(NORMAL_HELLO_PHASES), "observed_answered_count": len(answered),
            "attribution": "normal-source-order-consistent; root verification required" if ordinary else "ambiguous/excluded from first-VM cohort",
            "per_row_VM_identity_proven": False, "unobserved_pre_hello_boot_retries_possible": True,
            "warm_borrower_rows_included": False, "seed_decision": "not made"}


def owned_file(root, path, bound):
    root, path = Path(root).resolve(), Path(path)
    census.require(path.is_absolute() and path != root and root in path.parents and ".." not in path.parts,
                   "owned input escapes this fresh private run")
    census.require(path.parent.resolve().is_relative_to(root), "owned parent escapes through a link")
    meta = path.lstat()
    census.require(stat.S_ISREG(meta.st_mode) and meta.st_size <= bound, "owned input must be a bounded regular non-link file")
    with path.open("rb") as data:
        value = data.read(bound + 1)
    census.require(len(value) <= bound, "owned input grew past its existing bound")
    return value


def receipt(harness, path):
    def unique(pairs):
        value = {}
        for key, item in pairs:
            census.require(key not in value, "owned receipt has duplicate JSON fields")
            value[key] = item
        return value
    value = json.loads(owned_file(harness.root, path, harness.runtime["max_frame"]), object_pairs_hook=unique)
    census.require(isinstance(value, dict) and type(value.get("pid")) is int and value["pid"] > 0
                   and type(value.get("started")) is int and value["started"] > 0
                   and isinstance(value.get("vsock"), str) and isinstance(value.get("declared_config"), dict),
                   "owned receipt identity/configuration is invalid")
    census.require(Path(value.get("record", "")) == harness.backend_state / "@guest",
                   "receipt does not name the exact fresh backend record")
    return value


def recover_pre_ready(harness):
    """No PID discovery: stop exact child, then existing helpers verify an exact private record.

    An existing receipt is never replaced. A create-new remember receipt must match its complete
    record/configuration before any stop request. Failure leaves closure unproved and halts campaign.
    """
    process = harness.backend_process if harness.backend_process is not None else getattr(harness, "cold_process", None)
    # Harness.spawn prunes exited children. The saved original Popen remains exact ownership proof
    # after that pruning; a PID/name lookup must never substitute for it.
    recorded = process is not None and (process is getattr(harness, "cold_process", None)
                                       or any(child is process for _, child in harness.children))
    census.require(recorded,
                   "pre-ready recovery has no exact owned backend child")
    harness.stop(process, crash=True)
    census.require(process.returncode is not None, "owned backend was not reaped before recovery")
    harness.backend_process = None
    census.require(harness.backend_state == harness.root / "cold-node.node-state", "unexpected private backend state")
    record = harness.backend_state / "@guest"
    owned_file(harness.root, record, harness.runtime["max_frame"])
    original = harness.guest_receipt
    original_value = None
    if original is not None and (original.exists() or original.is_symlink()):
        original_value = receipt(harness, original)
    recovered = harness.root / "cold-recovery-receipt.json"
    census.require(not recovered.exists() and not recovered.is_symlink(), "recovery receipt already exists; preserve it")
    remembered = harness.probe("--remember-guest", str(record), "--owned-root", str(harness.root), "--receipt", str(recovered))
    census.require(remembered.get("remembered") is True, "existing helper did not retain exact guest identity")
    current = receipt(harness, recovered)
    census.require((remembered.get("pid"), remembered.get("started")) == (current["pid"], current["started"]),
                   "remember reply differs from its generated receipt")
    if original_value is not None:
        census.require(original_value == current, "existing receipt differs from current exact record/config; no stop dispatched")
        selected = original
    else:
        selected = recovered
    harness.guest_receipt = selected
    stopped = harness.probe("--stop-owned-guest", str(record), "--owned-root", str(harness.root),
                            "--receipt", str(selected), "--wait-nanos", str(harness.runtime["wait_ns"]))
    census.require(stopped.get("guest_stopped") is True and
                   (stopped.get("pid"), stopped.get("started")) == (current["pid"], current["started"]),
                   "owned guest retirement did not preserve exact identity")
    harness.report["owned_guest_retired"] = stopped
    harness.report["cold_boot"]["recovery_receipt"] = str(selected)
    harness.report["cold_boot"]["exact_record_recovery"] = True
    harness.report["cold_boot"]["pre_ready_recovery"] = not harness.report["cold_boot"].get("backend_ready", False)
    harness.report["cold_boot"]["recovery_identity"] = {key: current[key] for key in ("pid", "started", "vsock")}


class ColdHarness(census.Harness):
    instances = None

    def __init__(self, args):
        census.require(args.mode == "guest" and args.deadline_seconds == 120,
                       "cold census requires guest mode and original explicit120s budget")
        parent = Path(args.run_parent).expanduser().resolve()
        census.require(not any((parent / name).exists() for name in ("runtime.json", "pids.jsonl", "result.json")),
                       "run parent is an existing capture directory; use its owned cache parent")
        super().__init__(args)
        self.cold_process = None
        self.report["kind"] = report_kind(args.validate_seeds)
        self.report["cold_boot"] = {"backend_ready": False, "cohort": "FIRST-BOOT backend-process first SDK Hello",
                                    "seed_installation": "not performed", "boxes_created": False,
                                    "workload_success": "not exercised", "full_boot_latency": "not measured"}
        if args.validate_seeds:
            self.report["cold_boot"].pop("cohort")
        self.instances.append(self)

    def exercise(self):
        self.phase = "first-boot"
        marker = self.source / "SOURCE_REVISION"
        revision = marker.read_text().strip() if marker.is_file() else self.command("source-revision", ["git", "-C", str(self.source), "rev-parse", "HEAD"]).decode().strip()
        self.report["source_revision"] = self.runtime["source_revision"] = revision
        self.runtime["hex_sha256"] = self.report["binary_and_artifact_hashes"]["hex"]["sha256"]
        census.atomic_json(self.runtime_path, self.runtime)
        try:
            self.backend("cold-node")
        finally:
            self.cold_process = self.backend_process
        self.report["cold_boot"]["backend_ready"] = True
        owner = receipt(self, self.guest_receipt)
        actual = self.snapshot()
        census.require("oci" in actual.get("classes", []) and actual.get("boxes") == [] and self.baseline == [],
                       "ready backend lacks expected API class or empty physical baseline")
        capacity = actual.get("capacity")
        census.require(isinstance(capacity, dict) and capacity.get("id") == f"guest:{owner['pid']}:{owner['started']}",
                       "live capacity does not name the exact owned guest")
        self.report["cold_boot"].update(ready_snapshot=actual, identity={key: owner[key] for key in ("pid", "started", "vsock")},
                                        receipt=str(self.guest_receipt), ready_class_verified=True)

    def finish_backend(self):
        original_failure = None
        if self.report["cold_boot"]["backend_ready"]:
            try:
                return super().finish_backend()
            except Exception as why:
                original_failure = why
                self.report["status"] = "failed"
                self.report["cold_boot"]["original_finish_failure"] = str(why)
        try:
            recover_pre_ready(self)
        except Exception as why:
            self.report["cold_boot"]["cleanup_unproven"] = str(why)
            if original_failure is not None:
                raise original_failure from why
            raise
        if original_failure is not None:
            raise original_failure

    def census_report(self):
        process = self.cold_process
        cold = self.report["cold_boot"]
        cold["ledger"] = str(self.runtime["ledger"])
        armed = self.runtime.get("wait_mode", "measuring") == "armed"
        if process is not None:
            cold["producer"] = {"kind": "cold-node", "pid": process.pid, "census": process.hex_census_path,
                                "profile": self.runtime["profile"], "source_revision": self.runtime.get("source_revision")}
        if armed:
            # The existing armed reporter only verifies an empty directory; it never reads samples.
            super().census_report()
            census.require(process is None or process.hex_census_path is None, "armed backend unexpectedly has a census filename")
        elif process is not None:
            census.require(process.hex_census_path is not None, "backend node has no unique census filename")
            path = Path(process.hex_census_path)
            census.require(path.parent == self.root / "census", "backend census is outside its exact private census directory")
            # Use only the captured backend-node filename, never other observers/borrowers or PID fallback.
            with path.open("rb") as source:
                lines = iter(lambda: source.readline(self.runtime["max_frame"] + 1), b"")
                cold["sdk_hello"] = classify_hellos(lines, self.args.max_census_lines, self.runtime["max_frame"])
        stopped = self.report.get("owned_guest_retired", {})
        identity = cold.get("identity") or cold.get("recovery_identity") or {}
        cold["stopped_identity_basis"] = "ready" if cold.get("identity") else "recovery" if cold.get("recovery_identity") else None
        cold["stopped_receipt_verified"] = (bool(identity) and stopped.get("guest_stopped") is True and stopped.get("end") == "Stopped"
            and (stopped.get("pid"), stopped.get("started")) == (identity.get("pid"), identity.get("started")))
        cold["backend_reaped"] = process is not None and process.returncode is not None
        census.require(cold["backend_ready"] and cold.get("ready_class_verified") and cold["backend_reaped"]
                       and cold["stopped_receipt_verified"] and "backend_cleanup_error" not in self.report,
                       "iteration lacks ready/class/empty-baseline and exact Stopped closure proof")
        if armed:
            census.require("cleanup_error" not in self.report and "cleanup_unproven" not in cold,
                           "armed iteration has an unproved cleanup")
            return
        hello = cold.get("sdk_hello", {})
        census.require(hello.get("first_sdk_hello") is not None and not hello.get("malformed") and not hello.get("full_markers"),
                       "backend first SDK Hello census is missing, malformed or truncated")


def campaign(count, run_one, save, *, validate_seeds=False):
    census.require(type(count) is int and 0 < count <= (1 << 64) - 1, "explicit positive representable N required")
    report = {"kind": report_kind(validate_seeds), "n": count, "iterations": [], "status": "running",
              "seed_installation": "not performed", "workload_success": "not exercised", "full_boot_latency": "not measured"}
    roots = set()
    for ordinal in range(1, count + 1):
        row = run_one(ordinal)
        report["iterations"].append(row)
        root = row.get("run")
        fresh = root is not None and root not in roots
        if root is not None:
            roots.add(root)
        matching_mode = not validate_seeds or row.get("kind") == report["kind"]
        if not fresh or not matching_mode or row.get("status") != "passed" or not row.get("stopped_receipt_verified"):
            report["status"] = "failed"
            report["error"] = "iteration failed/fresh-root or Stopped closure unproved; no next guest"
            save(report)
            return report
        save(report)
    report["status"] = "completed"
    if not validate_seeds:
        report["ambiguous_first_VM_cohorts"] = sum(
            row.get("cold_boot", {}).get("sdk_hello", {}).get("attribution") == "ambiguous/excluded from first-VM cohort"
            for row in report["iterations"])
    save(report)
    return report


def main():
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--n", type=int, required=True, help="positive fresh node/guest iterations")
    parser.add_argument("--output", type=Path, required=True, help="new aggregate JSON path")
    parser.add_argument("--validate-seeds", action="store_true", help="validate installed waits armed; disable census measurement")
    options, rest = parser.parse_known_args()
    if options.validate_seeds:
        rest.append("--validate-seeds")  # The existing Harness owns wait_mode and both child census variables.
    census.require(options.n > 0, "N must be positive")
    os.umask(0o077)
    output = options.output.expanduser().resolve()
    with output.open("x", encoding="utf-8") as initial:
        json.dump({"status": "starting", "kind": report_kind(options.validate_seeds), "n": options.n}, initial)
        initial.write("\n")
    original_arguments, original_harness = sys.argv, census.Harness

    def one(ordinal):
        instances = []
        ColdHarness.instances = instances
        sys.argv = [original_arguments[0], *rest, "--n", "1"]  # One backend per fresh Harness; no physical workload is run.
        fault = None
        try:
            census.main()
        except SystemExit as why:
            if why.code not in (None, 0):
                fault = f"iteration exited {why.code}"
        except Exception as why:
            fault = f"{type(why).__name__}: {why}"
        if len(instances) != 1:
            return {"ordinal": ordinal, "status": "failed", "error": fault or "Harness construction incomplete",
                    "stopped_receipt_verified": False, "cleanup": "no backend is started before Harness construction completes"}
        harness = instances[0]
        cold = harness.report["cold_boot"]
        row = {"ordinal": ordinal, "run": str(harness.root), "result": str(harness.root / "result.json"),
                "status": "failed" if fault else harness.report.get("status"), "error": fault or harness.report.get("error"),
                "stopped_receipt_verified": cold.get("stopped_receipt_verified", False),
                "identity": cold.get("identity"), "retirement": harness.report.get("owned_guest_retired"),
                "cold_boot": cold}
        if options.validate_seeds:
            row["kind"] = harness.report.get("kind")
        return row

    try:
        census.Harness = ColdHarness
        if options.validate_seeds:
            result = campaign(options.n, one, lambda report: census.atomic_json(output, report), validate_seeds=True)
        else:
            result = campaign(options.n, one, lambda report: census.atomic_json(output, report))
    finally:
        sys.argv, census.Harness = original_arguments, original_harness
    print("cold-boot-result " + str(output))
    return 0 if result["status"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
