#!/usr/bin/env python3
"""AC-T7.4.1: bounded census reconstruction. No VM/API calls, seeds or eligibility decisions."""
import argparse
from collections import Counter, defaultdict
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys


class Invalid(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise Invalid(message)


def integer(value, name, minimum=0):
    require(type(value) is int and value >= minimum, name + " must be a bounded nonnegative integer")
    return value


def same(actual, expected, name):
    require(actual == expected, name + " differs from reconstructed input")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def decode_json(data):
    try:
        value = json.loads(data, object_pairs_hook=unique_object)
    except (ValueError, RecursionError, UnicodeError) as error:
        raise Invalid("invalid JSON input") from error
    require(isinstance(value, dict), "JSON input must be an object")
    return value


def relative(run, value):
    require(isinstance(value, (str, Path)), "input path must be explicit")
    path = Path(value)
    require(".." not in path.parts, "input path escapes explicit run")
    path = path if path.is_absolute() else run / path
    try:
        parts = path.relative_to(run).parts
    except ValueError as error:
        raise Invalid("input path escapes explicit run") from error
    require(parts and all(part not in (".", "..") for part in parts), "invalid input path")
    return parts


@contextmanager
def input_file(run, value):
    """Directory-relative no-follow opens never traverse a result-controlled external source."""
    parts = relative(run, value)
    require(hasattr(os, "O_NOFOLLOW") and hasattr(os, "O_DIRECTORY"), "no-follow input reads require Unix")
    directory = os.open(run, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in parts[:-1]:
            next_directory = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
            os.close(directory)
            directory = next_directory
        # Inspect type before reading; a FIFO must not wait for a writer just to be rejected.
        descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
        with os.fdopen(descriptor, "rb") as source:
            before = os.fstat(source.fileno())
            require(stat.S_ISREG(before.st_mode), "input must be a regular file")
            yield source, before.st_size
            after = os.fstat(source.fileno())
            same((after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns),
                 (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns), "input snapshot")
    finally:
        os.close(directory)


def read_bytes(run, value, bound):
    integer(bound, "byte budget")
    with input_file(run, value) as (source, size):
        require(size <= bound, "input exceeds byte budget")
        data = source.read(bound + 1)
        require(len(data) <= bound and len(data) == size, "input exceeds budget or changed during read")
    return data


def protocol(source):
    """The caller explicitly supplies code authority; no report-controlled source/binary is read."""
    source = Path(source).resolve(strict=True)
    wire = (source / "crates/hex-core/src/wire.rs").read_text()
    match = re.search(r"pub const MAX_MESSAGE:\s*usize\s*=\s*([0-9_]+)(?:\s*<<\s*([0-9_]+))?;", wire)
    require(match is not None, "cannot read actual protocol byte bound")
    left, shift = int(match.group(1).replace("_", "")), int((match.group(2) or "0").replace("_", ""))
    require(0 <= shift < 64 and 0 < left <= sys.maxsize, "invalid protocol usize bound")
    maximum = left << shift
    require(0 < maximum <= sys.maxsize, "invalid protocol byte bound")
    waits = (source / "crates/hex-core/src/schedule/waits.rs").read_text()
    declaration = re.search(r"pub enum WaitKind\s*\{([^}]+)\}", waits)
    require(declaration is not None, "cannot read actual scheduler WaitKinds")
    kinds = [re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()
             for name in re.findall(r"^\s+([A-Za-z]+),\s*$", declaration.group(1), re.MULTILINE)]
    require(kinds and len(kinds) == len(set(kinds)), "invalid actual WaitKinds")
    return maximum, kinds


def counts(value):
    require(isinstance(value, dict), "counts must be an object")
    for name, count in value.items():
        require(isinstance(name, str), "count name must be text")
        integer(count, "sample count")
    return value


def statistics(samples):
    """Exact integer nearest ranks; values and output remain nanoseconds, never wait seeds."""
    n = sum(samples.values())
    result = {"n": n, "p50": None, "p90": None, "p99": None, "p999": None, "max": None}
    if not n:
        return result
    ranks = {name: (n * numerator + denominator - 1) // denominator
             for name, numerator, denominator in (("p50", 50, 100), ("p90", 90, 100), ("p99", 99, 100), ("p999", 999, 1000))}
    cumulative = 0
    for value, count in sorted(samples.items()):
        cumulative += count
        for name, rank in ranks.items():
            if result[name] is None and cumulative >= rank:
                result[name] = value
        result["max"] = value
    return result


def ledger(run, path, maximum, row_budget, names):
    integer(row_budget, "ledger row budget", 1)
    producers, calls, processes = {}, {}, {}
    digest = hashlib.sha256()
    row_count = byte_count = 0
    physical = {"local": set(), "remote": set()}
    with input_file(run, path) as (source, size):
        require(size <= row_budget * maximum, "ledger exceeds byte budget")
        while True:
            line = source.readline(maximum + 1)
            if not line:
                break
            row_count += 1
            byte_count += len(line)
            require(row_count <= row_budget and len(line) <= maximum and line.endswith(b"\n"), "ledger exceeds row/line budget or is partial")
            digest.update(line)
            row = decode_json(line)
            if row.get("kind") == "verified-physical-lifecycle":
                phase = row.get("phase")
                require(phase in physical and isinstance(row.get("key"), dict) and isinstance(row.get("physical"), str), "invalid physical lifecycle event")
                identity = json.dumps(row["key"], sort_keys=True)
                require(identity not in physical[phase], "duplicate physical lifecycle event")
                physical[phase].add(identity)
            if not row.get("census"):
                continue
            parts = relative(run, row["census"])
            require(len(parts) == 2 and parts[0] == "census" and parts[1] in names, "ledger references an unreported census source")
            target = calls if row.get("kind") == "census-call-exit" else processes if row.get("kind") == "census-process-exit" else producers
            require(parts[1] not in target, "duplicate filename attribution")
            target[parts[1]] = row
        same(byte_count, size, "ledger bytes")
    return producers, calls, processes, {"sha256": digest.hexdigest(), "rows": row_count, "bytes": byte_count,
                                          "physical_control_events": {phase: len(keys) for phase, keys in physical.items()}}


def retirement(run, report, maximum):
    retired = report.get("owned_guest_retired")
    if retired is None:
        require(report.get("mode") != "guest", "owned guest retirement is absent")
        return None
    require(isinstance(retired, dict) and retired.get("guest_stopped") is True, "owned guest retirement is unproved")
    receipt_data = read_bytes(run, "owned-guest.json", maximum)
    receipt = decode_json(receipt_data)
    record_data = read_bytes(run, receipt.get("record"), maximum)
    try:
        lines = record_data.decode("utf-8").splitlines()
        require(len(lines) == 4 and lines[0] == "hex-guest 1", "invalid owned guest record")
        fields = dict(line.split(" ", 1) for line in lines[1:])
        require(set(fields) == {"pid", "started", "vsock"}, "invalid owned guest identity fields")
        identity = {"pid": int(fields["pid"]), "started": int(fields["started"]), "vsock": fields["vsock"]}
    except (ValueError, UnicodeError) as error:
        raise Invalid("invalid owned guest identity") from error
    integer(identity["pid"], "guest PID", 1)
    integer(identity["started"], "guest start identity", 1)
    for name in ("pid", "started", "vsock"):
        same(receipt.get(name), identity[name], "guest receipt identity")
    for name in ("pid", "started"):
        same(retired.get(name), identity[name], "guest retirement identity")
    require(retired.get("end") == "Stopped" or retired.get("already_gone") is True, "owned guest has no recorded terminal proof")
    require(isinstance(receipt.get("declared_config"), dict), "guest configuration receipt is absent")
    # The socket may already be gone. Its identity string is compared, never opened or rediscovered.
    return {"pid": identity["pid"], "started": identity["started"], "vsock": identity["vsock"],
            "receipt_sha256": hashlib.sha256(receipt_data).hexdigest(), "record_sha256": hashlib.sha256(record_data).hexdigest(),
            "recorded_terminal": retired.get("end", "already_gone"), "live_API_check": "not performed by verifier"}


def verify(run, result_path, source, max_result_bytes, max_ledger_rows):
    run = Path(run).resolve(strict=True)
    maximum, kinds = protocol(source)
    data = read_bytes(run, result_path, integer(max_result_bytes, "result byte budget", 1))
    report = decode_json(data)
    require(report.get("kind") == "actual-scheduler-wait-census" and report.get("status") == "passed", "source result is not a passed census")
    require(not any(name in report for name in ("error", "cleanup_error", "backend_cleanup_error")), "source result contains a failure")
    n = integer(report.get("n"), "N", 1)
    row_limit = integer(report.get("max_rows_per_process"), "per-process row bound", 1)
    same(report.get("verified_physical_created"), 2 * n, "physical creates")
    same(report.get("verified_physical_deleted"), 2 * n, "physical deletes")
    require(report.get("backend_baseline_restored") is True, "physical baseline was not restored")
    runtime = decode_json(read_bytes(run, "runtime.json", maximum))
    same(runtime.get("root"), str(run), "runtime run authority")
    same(runtime.get("max_frame"), maximum, "actual protocol bound")
    same(runtime.get("max_census_lines"), row_limit, "runtime row bound")
    census = report.get("census")
    require(isinstance(census, dict) and isinstance(census.get("per_process"), list), "per-process report is absent")
    records = {}
    for item in census["per_process"]:
        require(isinstance(item, dict), "per-process item must be an object")
        name = item.get("file")
        require(isinstance(name, str) and Path(name).name == name and name.endswith(".tsv"), "invalid raw census filename")
        require(name not in records, "duplicate raw filename")
        records[name] = item
    folder = os.open(run / "census", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        with os.scandir(folder) as entries:
            found = set()
            for entry in entries:
                require(len(found) < len(records) and entry.name in records and entry.is_file(follow_symlinks=False), "unreported or unsafe raw census source")
                found.add(entry.name)
            same(found, set(records), "raw census source set")
    finally:
        os.close(folder)
    producers, calls, processes, ledger_record = ledger(run, runtime.get("ledger"), maximum, max_ledger_rows, records)
    same(set(producers), set(records), "producer filename set")
    same(ledger_record["physical_control_events"], {"local": n, "remote": n}, "physical lifecycle events")
    typed = "refused_rows_by_name" in census
    require(all(("refused" in row) == typed for row in records.values()), "mixed outcome schemas")
    histograms = {kind: defaultdict(Counter) for kind in ("raw_unmarked", "wire_refused", "run_out", "filename_call_exit_success", "filename_process_exit_success")}
    raw_counts, refused_counts, call_counts, process_counts, runout_counts = (Counter() for _ in range(5))
    cut, malformed, reconstructed = set(), set(), []
    for name, item in records.items():
        declared_rows = integer(item.get("rows"), "reported rows")
        require(declared_rows <= row_limit + 1, "reported rows exceed the per-process bound")
        producer, call, process = producers[name], calls.get(name, {}), processes.get(name, {})
        same(item.get("producer"), producer, "reported producer")
        same(item.get("call_outcome", {}), call, "reported call attribution")
        same(item.get("process_outcome", {}), process, "reported process attribution")
        for outcome in (call, process):
            if outcome:
                same(outcome.get("pid"), producer.get("pid"), "filename identity diagnostics")
                require(type(outcome.get("success")) is bool, "exit outcome is not classified")
                require(type(outcome.get("returncode")) is int, "exit return code is not an integer")
                require(not outcome["success"] or outcome["returncode"] == 0, "successful exit has nonzero return code")
        if process:
            same(process["success"], process["returncode"] == 0, "process exit classification")
        require(not call or producer.get("kind") == "call", "call outcome belongs to a different producer kind")
        require(not process or producer.get("kind") != "call", "process outcome cannot replace call attribution")
        digest = hashlib.sha256()
        observed = {"answers": Counter(), "refused": Counter(), "ran_out": Counter()}
        rows = byte_count = 0
        with input_file(run, Path("census") / name) as (raw, size):
            require(size <= declared_rows * maximum, "raw bytes exceed reported row/protocol bound")
            while True:
                line = raw.readline(maximum + 1)
                if not line:
                    break
                rows += 1
                byte_count += len(line)
                require(rows <= declared_rows and len(line) <= maximum, "raw data exceeds reported row/line bound")
                digest.update(line)
                if re.fullmatch(rb"census full at [0-9]+ lines\r?\n", line):
                    cut.add(name)
                    continue
                match = re.fullmatch(rb"([^\s]+) ([>!]?)([0-9]+)\r?\n", line)
                if match is None:
                    malformed.add(name)
                    continue
                wait, marker, nanos = match.groups()
                require(len(nanos) <= len(str((1 << 64) * 1_000_000_000 - 1)), "duration exceeds Rust Duration bound")
                ns = int(nanos)
                require(ns <= (1 << 64) * 1_000_000_000 - 1, "duration exceeds Rust Duration bound")
                wait = wait.decode("utf-8")
                require(typed or marker != b"!", "typed refusal appears in an undeclared legacy schema")
                category, field = ("wire_refused", "refused") if marker == b"!" else ("run_out", "ran_out") if marker == b">" else ("raw_unmarked", "answers")
                observed[field][wait] += 1
                histograms[category][wait][ns] += 1
                if not marker:
                    if producer.get("kind") == "call" and call.get("success"):
                        histograms["filename_call_exit_success"][wait][ns] += 1
                    if process.get("success"):
                        histograms["filename_process_exit_success"][wait][ns] += 1
            same(byte_count, size, "raw bytes")
        same(rows, declared_rows, "raw line count")
        same(digest.hexdigest(), item.get("sha256"), "raw SHA256")
        if "bytes" in item:
            same(integer(item["bytes"], "reported bytes"), byte_count, "reported raw bytes")
        for field in ("answers", "ran_out") + (("refused",) if typed else ()):
            same(counts(item.get(field)), dict(observed[field]), "per-process " + field)
        raw_counts.update(observed["answers"])
        refused_counts.update(observed["refused"])
        runout_counts.update(observed["ran_out"])
        if producer.get("kind") == "call" and call.get("success"):
            call_counts.update(observed["answers"])
        if process.get("success"):
            process_counts.update(observed["answers"])
        reconstructed.append({"file": name, "sha256": digest.hexdigest(), "bytes": byte_count, "rows": rows,
                              "answers": dict(observed["answers"]), "refused": dict(observed["refused"]), "ran_out": dict(observed["ran_out"])})
    for field, actual in (("answer_rows_by_name", raw_counts), ("successful_single_call_rows", call_counts), ("successful_process_rows", process_counts)):
        same(counts(census.get(field)), dict(actual), "global " + field)
    if typed:
        same(counts(census.get("refused_rows_by_name")), dict(refused_counts), "global refused rows")
    if "ran_out_rows_by_name" in census:
        same(counts(census["ran_out_rows_by_name"]), dict(runout_counts), "global run-out rows")
    same(census.get("truncated_files"), sorted(cut), "truncated flags")
    same(census.get("malformed_files"), sorted(malformed), "malformed flags")
    require(not cut and not malformed, "raw capture is truncated or malformed")
    missing = {kind: raw_counts.get("client.answer.schedule." + kind, 0) for kind in kinds if raw_counts.get("client.answer.schedule." + kind, 0) < n}
    missing_calls = {kind: call_counts.get("client.answer.schedule." + kind, 0) for kind in kinds if not kind.startswith("hear_") and call_counts.get("client.answer.schedule." + kind, 0) < n}
    same(census.get("missing_row_targets"), missing, "raw N targets")
    same(census.get("missing_successful_single_call_rows"), missing_calls, "filename-attributed N targets")
    same(census.get("required_raw_answer_rows_per_kind"), n, "declared N target")
    same(census.get("required_verified_successful_trials"), n, "declared physical target")
    require(not missing and not missing_calls, "capture misses its N targets")
    distributions = {category: {name: statistics(values) for name, values in sorted(hist.items())} for category, hist in histograms.items()}
    distributions["non_refusal_wire_answers"] = distributions["raw_unmarked"] if typed else {}
    distributions["legacy_unmarked_unclassified"] = {} if typed else distributions["raw_unmarked"]
    return {"status": "consistent", "scope": "bounded structural reconstruction; wire answer and process exit are not workload success or seed eligibility",
            "run": str(run), "source_revision_reported": report.get("source_revision"), "profile_reported": report.get("profile"),
            "result_sha256": hashlib.sha256(data).hexdigest(), "protocol_max_bytes": maximum, "max_rows_per_process": row_limit,
            "ledger": ledger_record, "n": n, "physical_created": 2 * n, "physical_deleted": 2 * n,
            "owned_guest_retirement": retirement(run, report, maximum), "legacy_wire_outcomes_unclassified": not typed,
            "per_process": reconstructed, "distributions_ns": distributions}


def create_output(path, result):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        json.dump(result, output, sort_keys=True)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", required=True)
    parser.add_argument("--result", required=True)
    parser.add_argument("--source", required=True, help="explicit code authority for actual MAX_MESSAGE and WaitKinds")
    parser.add_argument("--max-result-bytes", type=int, required=True, help="operator bootstrap JSON budget")
    parser.add_argument("--max-ledger-rows", type=int, required=True, help="operator ledger scan budget")
    parser.add_argument("--output", required=True, help="new file only; existing outputs are never replaced")
    args = parser.parse_args()
    try:
        result = verify(args.run, args.result, args.source, args.max_result_bytes, args.max_ledger_rows)
        create_output(args.output, result)
    except (Invalid, OSError, UnicodeError) as error:
        print("census verification refused: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
