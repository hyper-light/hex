#!/usr/bin/env python3
"""AC-T7.4.1: bounded cold-campaign reconstruction; no VM/API calls or seed decisions."""
import argparse
from collections import Counter
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys


spec = importlib.util.spec_from_file_location("cold_shared_verifier", Path(__file__).with_name("verify_census.py"))
if spec is None or spec.loader is None:
    raise RuntimeError("the existing safe census verifier is unavailable")
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)
Invalid, require, same, integer = shared.Invalid, shared.require, shared.same, shared.integer

KIND = "cold-node-boot-SDK-Hello-census"
HELLO = "client.guest.answer.hello"
# The two existing call sites named by cold_boot_census.py at source8f51140e. Counts do not prove VM identity.
PHASES = ("GuestRuntime.guest initial client", "Recorded.attach evented join")
DURATION_MAX = (1 << 64) * 1_000_000_000 - 1  # Rust Duration's u64 seconds and subsecond nanoseconds.


class Reader:
    """Remember hashes while delegating every file open/read to the shared safe primitives."""
    def __init__(self, root):
        self.root = Path(root).resolve(strict=True)
        self.inputs = {}

    def remember(self, path, digest, size):
        name = "/".join(shared.relative(self.root, path))
        value = {"sha256": digest, "bytes": size}
        if name in self.inputs:
            same(value, self.inputs[name], "reopened authority input")
        self.inputs[name] = value

    def read(self, path, bound):
        require(type(bound) is int and 0 <= bound < sys.maxsize, "invalid input byte bound")
        data = shared.read_bytes(self.root, path, bound)
        self.remember(path, hashlib.sha256(data).hexdigest(), len(data))
        return data

    def json(self, path, bound):
        return shared.decode_json(self.read(path, bound))


def bounded_lines(reader, path, row_bound, line_bound):
    """Shared no-follow/nonblocking regular-file open; count and hash even empty producer files."""
    require(type(row_bound) is int and 0 <= row_bound < sys.maxsize // line_bound, "invalid derived row bound")
    digest, total, rows = hashlib.sha256(), 0, 0
    with shared.input_file(reader.root, path) as (source, size):
        require(size <= row_bound * line_bound, "input exceeds derived row/byte budget")
        while True:
            line = source.readline(line_bound + 1)
            if not line:
                break
            rows += 1
            total += len(line)
            require(rows <= row_bound and len(line) <= line_bound, "input exceeds row/line budget")
            digest.update(line)
            yield rows, line
        same(total, size, "streamed input bytes")
    reader.remember(path, digest.hexdigest(), total)


def census_file(reader, path, row_bound, line_bound):
    answered, refused, run_out, malformed, full = [], [], [], [], []
    others, all_counts, rows = Counter(), {name: Counter() for name in ("answered", "refused", "run_out")}, 0
    for number, raw in bounded_lines(reader, path, row_bound + 1, line_bound):
        rows = number
        if not raw.endswith(b"\n"):
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
        name, marker, digits = match.groups()
        require(len(digits) <= len(str(DURATION_MAX)), "sample exceeds Rust Duration representation")
        nanos = int(digits)
        require(nanos <= DURATION_MAX, "sample exceeds Rust Duration representation")
        all_counts[{"": "answered", "!": "refused", ">": "run_out"}[marker]][name] += 1
        if name != HELLO:
            others[name] += 1
            continue
        sample = {"line": number, "name": name, "marker": marker, "nanos": nanos, "raw": text}
        {"": answered, "!": refused, ">": run_out}[marker].append(sample)
    ordinary = len(answered) == len(PHASES) and not refused and not run_out and not malformed and not full
    classification = {"kind": "backend-process-first-completed-SDK-Hello", "rows": rows,
        "first_sdk_hello": answered[0] if answered else None, "subsequent_answered_hellos": answered[1:],
        "refused_hellos": refused, "run_out_hellos": run_out, "other_wait_rows": dict(others),
        "malformed": malformed, "full_markers": full, "normal_hello_phases": list(PHASES),
        "normal_answered_count": len(PHASES), "observed_answered_count": len(answered),
        "attribution": "normal-source-order-consistent; root verification required" if ordinary else "ambiguous/excluded from first-VM cohort",
        "per_row_VM_identity_proven": False, "unobserved_pre_hello_boot_retries_possible": True,
        "warm_borrower_rows_included": False, "seed_decision": "not made"}
    return classification, {name: dict(values) for name, values in all_counts.items()}


def ledger(reader, path, root, row_bound, maximum):
    producers, references, reaped = {}, set(), []
    for ordinal, line in bounded_lines(reader, path, row_bound, maximum):
        require(line.endswith(b"\n"), "ledger has a partial final row")
        row = shared.decode_json(line)
        same(row.get("run"), str(root), "ledger run authority")
        require(row.get("kind") != "verified-physical-lifecycle", "cold campaign contains a physical box lifecycle")
        if row.get("kind") == "reaped":
            integer(row.get("pid"), "reaped PID", 1)
            require(type(row.get("returncode")) is int, "backend reap has no terminal returncode")
            reaped.append((ordinal, row))
        if row.get("census") is None:
            continue
        parts = shared.relative(root, row["census"])
        require(len(parts) == 2 and parts[0] == "census" and parts[1].endswith(".tsv"), "ledger census path is outside the exact private census directory")
        name = parts[1]
        references.add(name)
        if row.get("kind") not in ("census-call-exit", "census-process-exit"):
            require(name not in producers, "duplicate filename producer authority")
            producers[name] = (ordinal, row)
    same(references, set(producers), "ledger filename producer coverage")
    return producers, reaped


def configuration(reader, root, report, cold, maximum):
    receipt_path = root / "owned-guest.json"
    same(cold.get("receipt"), str(receipt_path), "exact ready receipt")
    receipt = reader.json(receipt_path, maximum)
    record_path = root / "cold-node.node-state/@guest"
    same(receipt.get("record"), str(record_path), "exact backend guest record")
    record = reader.read(record_path, maximum)
    try:
        fields = record.decode("utf-8").splitlines()
        require(len(fields) == 4 and fields[0] == "hex-guest 1", "invalid owned guest record")
        pairs = [line.split(" ", 1) for line in fields[1:]]
        require(all(len(pair) == 2 for pair in pairs), "invalid guest record field")
        values = shared.unique_object(pairs)
        same(set(values), {"pid", "started", "vsock"}, "guest identity fields")
        require(values["pid"].isascii() and values["pid"].isdigit() and values["started"].isascii() and values["started"].isdigit(), "invalid guest identity integer")
        identity = {"pid": int(values["pid"]), "started": int(values["started"]), "vsock": values["vsock"]}
    except (ValueError, UnicodeError) as error:
        raise Invalid("invalid owned guest record") from error
    integer(identity["pid"], "guest PID", 1)
    integer(identity["started"], "guest start identity", 1)
    integer(receipt.get("pid"), "receipt PID", 1)
    integer(receipt.get("started"), "receipt start identity", 1)
    require(identity["pid"] <= (1 << 32) - 1 and identity["started"] <= (1 << 64) - 1, "guest identity exceeds API representation")
    same({name: receipt.get(name) for name in identity}, identity, "receipt identity")
    same(cold.get("identity"), identity, "ready guest identity")
    shared.relative(root, identity["vsock"])
    require(receipt.get("owned_vsock_root") is None, "external socket authority is outside this campaign")
    env, artifacts, config = report.get("environment"), report.get("binary_and_artifact_hashes"), receipt.get("declared_config")
    require(isinstance(env, dict) and isinstance(artifacts, dict) and isinstance(config, dict), "guest configuration authority is absent")
    integer(config.get("vcpus"), "declared vCPUs", 1)
    integer(config.get("memory_mib"), "declared memory", 1)
    require(isinstance(config.get("shares"), list) and all(isinstance(share, dict) and share.get("read_only") is True for share in config["shares"]), "private shares must be typed read-only declarations")
    expected = {}
    for field, artifact in (("binary", "shards"), ("kernel", "kernel"), ("initrd", "initrd")):
        require(isinstance(artifacts.get(artifact), dict) and isinstance(artifacts[artifact].get("path"), str), "artifact path authority is absent")
        expected[field] = artifacts[artifact]["path"]
    expected["vcpus"] = integer(env.get("guest_cpus"), "guest vCPUs", 1)
    expected["memory_mib"] = integer(env.get("guest_memory_mib"), "guest memory", 1)
    expected["shares"] = [{"dir": str(root / "guest-shares"), "read_only": True}]
    same(config, expected, "declared guest configuration")
    same(env.get("guest_shares_sha256"), hashlib.sha256((str(root / "guest-shares") + ":ro").encode()).hexdigest(), "default private share authority")
    retired = report.get("owned_guest_retired")
    require(isinstance(retired, dict) and retired.get("guest_stopped") is True and retired.get("end") == "Stopped" and retired.get("already_gone") is not True, "exact Stopped closure is absent")
    integer(retired.get("pid"), "retired PID", 1)
    integer(retired.get("started"), "retired start identity", 1)
    same({name: retired.get(name) for name in ("pid", "started")}, {name: identity[name] for name in ("pid", "started")}, "Stopped guest identity")
    return identity, config


def verify(run, result_path, source, max_result_bytes, max_ledger_rows, expected_revision=None):
    reader = Reader(run)
    maximum, _ = shared.protocol(source)
    integer(max_result_bytes, "result byte budget", 1)
    integer(max_ledger_rows, "ledger row budget", 1)
    require(max_ledger_rows < sys.maxsize // maximum, "ledger byte budget exceeds representation")
    campaign = reader.json(result_path, max_result_bytes)
    same(campaign.get("kind"), KIND, "campaign kind")
    same(campaign.get("status"), "completed", "campaign status")
    n = integer(campaign.get("n"), "N", 1)
    require(n <= (1 << 64) - 1, "N exceeds campaign representation")
    iterations = campaign.get("iterations")
    require(isinstance(iterations, list) and len(iterations) == n, "campaign iteration count differs from positive N")
    for field, value in (("seed_installation", "not performed"), ("workload_success", "not exercised"), ("full_boot_latency", "not measured")):
        same(campaign.get(field), value, "campaign claim scope")
    histograms = {name: Counter() for name in ("normal_first_completed_sdk_hello", "all_first_completed_sdk_hello", "subsequent_completed_sdk_hello", "wire_refused_sdk_hello", "run_out_sdk_hello")}
    cohorts = Counter({name: 0 for name in ("normal_iterations", "ambiguous_iterations", "first_completed_rows", "subsequent_completed_rows", "wire_refused_rows", "run_out_rows", "malformed_rows", "truncated_markers")})
    roots, reconstructed, revisions, profiles = set(), [], set(), set()
    for ordinal, item in enumerate(iterations, 1):
        require(isinstance(item, dict), "iteration is not an object")
        same(integer(item.get("ordinal"), "iteration ordinal", 1), ordinal, "iteration ordinal")
        parts = shared.relative(reader.root, item.get("run"))
        require(len(parts) == 1 and parts[0] not in roots, "iteration requires a unique fresh direct-child run root")
        roots.add(parts[0])
        root = reader.root / parts[0]
        same(item.get("run"), str(root), "iteration run authority")
        same(item.get("result"), str(root / "result.json"), "iteration result authority")
        require(item.get("status") == "passed" and item.get("error") is None and item.get("stopped_receipt_verified") is True, "iteration lacks passing closure")
        report = reader.json(root / "result.json", max_result_bytes)
        require(report.get("kind") == KIND and report.get("status") == "passed" and report.get("mode") == "guest" and report.get("wait_mode") == "measuring", "raw result has wrong kind/status/mode")
        require(not any(key in report for key in ("error", "cleanup_error", "backend_cleanup_error")), "raw result contains a failure")
        same(integer(report.get("n"), "inner N", 1), 1, "one fresh backend per iteration")
        require(report.get("comparison_or_performance_claim") is False, "unexpected performance claim")
        require(report.get("profile") in ("debug", "release"), "capture profile is absent or unknown")
        profiles.add(report["profile"])
        cold = report.get("cold_boot")
        require(isinstance(cold, dict), "cold backend evidence is absent")
        same(item.get("cold_boot"), cold, "aggregate cold evidence")
        same(item.get("identity"), cold.get("identity"), "aggregate guest identity")
        same(item.get("retirement"), report.get("owned_guest_retired"), "aggregate retirement")
        for key in ("backend_ready", "ready_class_verified", "backend_reaped", "stopped_receipt_verified"):
            require(cold.get(key) is True, "ready/reaped/Stopped evidence is absent")
        same(cold.get("stopped_identity_basis"), "ready", "Stopped identity basis")
        require(cold.get("boxes_created") is False and not any(key in cold for key in ("cleanup_unproven", "original_finish_failure")), "cold zero-box/closure evidence is inconsistent")
        for key, value in (("seed_installation", "not performed"), ("workload_success", "not exercised"), ("full_boot_latency", "not measured")):
            same(cold.get(key), value, "iteration claim scope")
        identity, config = configuration(reader, root, report, cold, maximum)
        ready = cold.get("ready_snapshot")
        require(isinstance(ready, dict) and isinstance(ready.get("classes"), list) and "oci" in ready["classes"] and ready.get("boxes") == [], "ready class/empty baseline is absent")
        require(isinstance(ready.get("capacity"), dict), "live guest capacity identity is absent")
        same(ready["capacity"].get("id"), f"guest:{identity['pid']}:{identity['started']}", "ready capacity guest identity")
        runtime = reader.json(root / "runtime.json", maximum)
        for key, value in (("root", str(root)), ("ledger", str(root / "pids.jsonl")), ("max_frame", maximum), ("wait_mode", "measuring")):
            same(runtime.get(key), value, "runtime authority " + key)
        row_bound = integer(report.get("max_rows_per_process"), "per-process row bound", 1)
        require(row_bound + 1 < sys.maxsize // maximum, "raw byte budget exceeds representation")
        same(runtime.get("max_census_lines"), row_bound, "runtime row bound")
        integer(runtime.get("max_census_lines"), "runtime row bound", 1)
        same(cold.get("ledger"), runtime["ledger"], "cold ledger authority")
        same(runtime.get("profile"), report.get("profile"), "runtime profile")
        revision = report.get("source_revision")
        require(isinstance(revision, str) and revision, "source revision is absent")
        revisions.add(revision)
        same(runtime.get("source_revision"), revision, "runtime revision")
        if expected_revision is not None:
            same(revision, expected_revision, "caller source revision")
        producers, reaped = ledger(reader, runtime["ledger"], root, max_ledger_rows, maximum)
        selected = cold.get("producer")
        require(isinstance(selected, dict), "exact backend census producer is absent")
        selected_parts = shared.relative(root, selected.get("census"))
        require(len(selected_parts) == 2 and selected_parts[0] == "census" and selected_parts[1] in producers, "backend census has no filename producer")
        selected_name = selected_parts[1]
        order, producer = producers[selected_name]
        integer(selected.get("pid"), "reported backend PID", 1)
        same({key: producer.get(key) for key in ("kind", "pid", "census", "profile", "source_revision")}, selected, "exact backend filename authority")
        same(producer.get("kind"), "cold-node", "backend producer kind")
        integer(producer.get("pid"), "backend PID", 1)
        integer(producer.get("parent"), "backend parent", 1)
        integer(producer.get("recorded_by"), "backend recording owner", 1)
        same(producer.get("parent"), producer["recorded_by"], "backend parent/recording owner")
        same(producer.get("profile"), runtime["profile"], "backend producer profile")
        same(producer.get("source_revision"), revision, "backend producer revision")
        require(isinstance(report.get("binary_and_artifact_hashes", {}).get("hex"), dict), "Hex binary receipt is absent")
        require(isinstance(producer.get("hex_sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", producer["hex_sha256"]) is not None, "Hex recorded SHA256 is invalid")
        same(producer.get("hex_sha256"), report["binary_and_artifact_hashes"]["hex"].get("sha256"), "backend recorded binary identity")
        same(runtime.get("hex_sha256"), producer["hex_sha256"], "runtime recorded binary identity")
        closure = [row for index, row in reaped if index > order and row.get("pid") == producer["pid"]
                   and row.get("parent") == producer["parent"] and row.get("recorded_by") == producer["recorded_by"]]
        require(closure, "no later matching recorded backend-Popen reap")
        raw_files = []
        for name in producers:
            classification, counts = census_file(reader, root / "census" / name, row_bound, maximum)
            raw_files.append({"file": name, "rows": classification["rows"], "outcomes": counts,
                              "malformed": classification["malformed"], "full_markers": classification["full_markers"]})
            if name == selected_name:
                hello = classification
        same(cold.get("sdk_hello"), hello, "independent raw SDK Hello reconstruction")
        require(cold["sdk_hello"].get("per_row_VM_identity_proven") is False and cold["sdk_hello"].get("unobserved_pre_hello_boot_retries_possible") is True, "SDK identity/retry uncertainty was strengthened")
        require(hello["first_sdk_hello"] is not None and not hello["malformed"] and not hello["full_markers"], "passed cold capture is missing/malformed/truncated")
        normal = hello["attribution"] == "normal-source-order-consistent; root verification required"
        cohorts["normal_iterations" if normal else "ambiguous_iterations"] += 1
        first = hello["first_sdk_hello"]["nanos"]
        histograms["all_first_completed_sdk_hello"][first] += 1
        if normal:
            histograms["normal_first_completed_sdk_hello"][first] += 1
        cohorts["first_completed_rows"] += 1
        for field, histogram, count in (("subsequent_answered_hellos", "subsequent_completed_sdk_hello", "subsequent_completed_rows"),
                                        ("refused_hellos", "wire_refused_sdk_hello", "wire_refused_rows"),
                                        ("run_out_hellos", "run_out_sdk_hello", "run_out_rows")):
            cohorts[count] += len(hello[field])
            histograms[histogram].update(row["nanos"] for row in hello[field])
        cohorts["malformed_rows"] += len(hello["malformed"])
        cohorts["truncated_markers"] += len(hello["full_markers"])
        reconstructed.append({"ordinal": ordinal, "run": str(root), "selected_census": selected_name,
                              "identity": identity, "declared_config": config, "sdk_hello": hello,
                              "backend_reap_returncodes": [row["returncode"] for row in closure], "ledger_attributed_files": raw_files})
    require(len(revisions) == 1, "campaign mixes source revisions")
    require(len(profiles) == 1, "campaign mixes profiles")
    same(integer(campaign.get("ambiguous_first_VM_cohorts"), "aggregate ambiguous cohort count"), cohorts["ambiguous_iterations"], "aggregate ambiguous cohort count")
    return {"status": "consistent", "kind": "cold-census-raw-reconstruction", "n": n,
            "source_revisions_reported": sorted(revisions), "profiles_reported": sorted(profiles), "authority_inputs": reader.inputs,
            "cohorts": dict(cohorts), "distributions_ns": {name: shared.statistics(values) for name, values in histograms.items()},
            "iterations": reconstructed, "per_row_VM_identity_proven": False,
            "unobserved_pre_hello_boot_retries_possible": True, "seed_installation": "not performed",
            "workload_success": "not exercised", "full_boot_latency": "not measured", "performance_claim": False,
            "limits": ["file evidence is not a live API/VM identity or cleanup check",
                       "only ledger-attributed census files reconstructed; directory extras not enumerated",
                       "artifact/config paths are compared as recorded; external artifact bytes are root-owned checks",
                       "zero boxes is the recorded no-workload/empty-baseline assertion, not an independent create-effect counter",
                       "recorded Popen closure joins filename first, then parent/recorder/PID/order; no PID selects census data"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", required=True, help="explicit common campaign parent")
    parser.add_argument("--result", required=True, help="completed campaign JSON within that parent")
    parser.add_argument("--source", required=True, help="explicit code authority for the actual protocol bound")
    parser.add_argument("--max-result-bytes", required=True, type=int)
    parser.add_argument("--max-ledger-rows", required=True, type=int)
    parser.add_argument("--expected-source-revision")
    parser.add_argument("--output", required=True, help="new file only")
    args = parser.parse_args()
    try:
        result = verify(args.run, args.result, args.source, args.max_result_bytes, args.max_ledger_rows, args.expected_source_revision)
        shared.create_output(args.output, result)
    except (Invalid, OSError, UnicodeError) as error:
        print("cold census verification refused: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
