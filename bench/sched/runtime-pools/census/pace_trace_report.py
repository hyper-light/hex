#!/usr/bin/env python3
"""Source-only DEBUG-hex-ack analyzer. Root owns execution/review; this is not a timing seed.

Usage (not run by the author):
  python3 -I pace_trace_report.py --result /owned/run/result.json \
      --owner-log /owned/run/o.log [--owner-log /other/owner.log] \
      [--output /owned/new-report.json]

The output preserves trace lines and ns values. Queue ordinals are inferred ONLY from explicit
queue-enter rows per (log,node,peer,kind); they are not real unique queue/attempt IDs. Exact ordered
comparisons are observations, not proof of causality or of an original-stall fix. In particular,
initial remote Describe is legitimate work included in outer_ns: outer/local alone proves no loop.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import sys


PREFIX = "[DEBUG-hex-ack]"
BUSY = "local-advertise-busy"
TIMES = ("local_ns", "outer_ns", "next_wait_ns")


def read_result(path):
    info = {"path": str(path), "missing": False, "malformed": False}
    try:
        data = path.read_bytes()
    except FileNotFoundError:
        info["missing"] = True
        return info, None
    except OSError as why:
        info["error"] = str(why)
        return info, None
    info.update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
    try:
        value = json.loads(data)
        if not isinstance(value, dict):
            raise ValueError("result must be a JSON object")
    except (UnicodeError, ValueError) as why:
        info.update(malformed=True, error=str(why))
        return info, None
    info["missing_fields"] = [] if "status" in value else ["status"]
    if "status" in value and not isinstance(value["status"], str):
        info.update(malformed=True, error="result.status must be a string")
    info["summary"] = {name: value[name] for name in
                       ("status", "kind", "error", "source_revision", "profile", "verified_physical_created",
                        "verified_physical_deleted", "backend_baseline_restored", "owned_guest_retired") if name in value}
    return info, value


def identity(fields, names, malformed):
    values = [fields[name] for name in names if name in fields]
    if len(set(values)) > 1:
        malformed.append("conflicting " + "/".join(names))
    return values[0] if values else None


def trace_row(raw, path, line, sequence):
    row = {"owner_log": str(path), "line": line, "trace_sequence": sequence,
           "raw_line": None, "raw_line_base64": None, "fields": {}, "missing_fields": [],
           "malformed": [], "actual_queue_id": None, "actual_attempt_id": None,
           "inferred_queue_ordinal": None, "derived_busy_sample_ordinal": None}
    try:
        text = raw.decode("utf-8")
    except UnicodeError:
        row["raw_line_base64"] = base64.b64encode(raw).decode("ascii")
        row["malformed"].append("trace line is not UTF-8")
        return row
    row["raw_line"] = text
    if not raw.endswith(b"\n"):
        row["malformed"].append("trace line lacks its final newline in this captured file prefix")
    before, _, body = text.partition(PREFIX)
    row["preamble"] = before
    fields = row["fields"]
    for token in body.split():
        name, separator, value = token.partition("=")
        if not separator or not name or not value:
            row["malformed"].append("invalid key=value token: " + token)
        elif name in fields:
            row["malformed"].append("duplicate field: " + name)
        else:
            fields[name] = value
    required = ["node", "peer", "kind", "stage"]
    if fields.get("stage") == BUSY:
        required.extend(TIMES)
    row["missing_fields"] = [name for name in required if name not in fields]
    for name in ("node", "peer", *TIMES):
        value = fields.get(name)
        if value is not None:
            if re.fullmatch(r"[0-9]+", value) is None:
                row["malformed"].append("unsigned decimal required for " + name)
            else:
                try:
                    row[name] = int(value)
                except ValueError:
                    row["malformed"].append("decimal could not be represented for " + name)
    row["stage"], row["kind"] = fields.get("stage"), fields.get("kind")
    row["actual_call_ns"] = row.get("local_ns")
    row["requested_wait_ns"] = row.get("next_wait_ns")
    row["actual_queue_id"] = identity(fields, ("queue_id", "queue"), row["malformed"])
    row["actual_attempt_id"] = identity(fields, ("attempt_id", "attempt"), row["malformed"])
    return row


def read_log(path):
    info = {"path": str(path), "missing": False, "rows": [], "non_trace_lines": 0}
    digest = hashlib.sha256()
    try:
        with path.open("rb") as source:
            initial = os.fstat(source.fileno())
            left = initial.st_size
            info["captured_prefix_bytes"] = left
            info["read_bytes"] = 0
            line = 0
            # The opening file size is the bound: never follow an actively growing log forever.
            while left:
                raw = source.readline(left)
                if not raw:
                    info["error"] = "log shortened before its captured prefix was read"
                    break
                left -= len(raw)
                line += 1
                info["read_bytes"] += len(raw)
                digest.update(raw)
                if PREFIX.encode("ascii") in raw:
                    info["rows"].append(trace_row(raw, path, line, len(info["rows"]) + 1))
                else:
                    info["non_trace_lines"] += 1
            final = os.fstat(source.fileno())
            info["file_size_after_read"] = final.st_size
            info["grew_after_snapshot"] = final.st_size > initial.st_size
    except FileNotFoundError:
        info["missing"] = True
    except OSError as why:
        info["error"] = str(why)
    info["sha256_of_captured_prefix"] = digest.hexdigest()
    return info


def ordered_observations(rows):
    values = [{"line": row["line"], "derived_busy_sample_ordinal": row["derived_busy_sample_ordinal"],
               "actual_call_ns": row["local_ns"], "outer_ns": row["outer_ns"],
               "requested_wait_ns": row["next_wait_ns"]} for row in rows]
    pairs = []
    for previous, current in zip(values, values[1:]):
        old_wait, wait = previous["requested_wait_ns"], current["requested_wait_ns"]
        pairs.append({"previous_line": previous["line"], "current_line": current["line"],
                      "previous_requested_wait_ns": old_wait, "current_requested_wait_ns": wait,
                      "requested_wait_delta_ns": wait - old_wait,
                      "previous_actual_call_ns": previous["actual_call_ns"],
                      "current_actual_call_ns": current["actual_call_ns"], "current_outer_ns": current["outer_ns"],
                      "wait_non_decreasing": wait >= old_wait, "wait_strictly_increased": wait > old_wait,
                      "actual_call_non_increasing": current["actual_call_ns"] <= previous["actual_call_ns"],
                      "outer_ge_previous_wait_plus_current_call": current["outer_ns"] >= old_wait + current["actual_call_ns"],
                      "wait_increased_while_actual_call_did_not": wait > old_wait and current["actual_call_ns"] <= previous["actual_call_ns"]})
    sequences = {}
    for name in ("actual_call_ns", "outer_ns", "requested_wait_ns"):
        series = [value[name] for value in values]
        transitions = list(zip(series, series[1:]))
        sequences[name] = {"values": series, "non_decreasing": all(b >= a for a, b in transitions) if transitions else None,
                           "strict_increases": sum(b > a for a, b in transitions),
                           "equal": sum(b == a for a, b in transitions), "decreases": sum(b < a for a, b in transitions)}
    return {"sample_count": len(values), "ordered_samples": values, "sequences": sequences,
            "pair_observations": pairs,
            "requested_wait_equals_outer_samples": sum(row["next_wait_ns"] == row["outer_ns"] for row in rows),
            "outer_less_than_actual_call_samples": sum(row["outer_ns"] < row["local_ns"] for row in rows),
            "wait_growth_with_non_growing_call_pairs": sum(pair["wait_increased_while_actual_call_did_not"] for pair in pairs),
            "interpretation": "exact ordered observations only; neither a causal self-feedback proof nor an original-stall/fix verdict"}


def group_queues(log):
    active, ordinals, queues = {}, {}, []
    for row in log["rows"]:
        if row["missing_fields"] or row["malformed"]:
            # An unreadable trace could hide a queue-enter, so no inferred group crosses that gap.
            active.clear()
            row["grouping_fence"] = "incomplete/malformed trace resets inferred contexts until a new queue-enter"
            continue
        if not all(name in row for name in ("node", "peer")) or row.get("kind") is None:
            continue
        context = (row["node"], row["peer"], row["kind"])
        if row.get("stage") == "queue-enter":
            ordinal = ordinals.get(context, 0) + 1
            ordinals[context] = ordinal
            queue = {"owner_log": log["path"], "node": context[0], "peer": context[1], "kind": context[2],
                     "inferred_queue_ordinal": ordinal, "queue_enter_line": row["line"],
                     "grouping": "inferred from explicit queue-enter in this log/context, not a unique queue ID",
                     "next_queue_enter_line": None, "trace_lines": [], "busy_rows": []}
            if context in active:
                active[context]["next_queue_enter_line"] = row["line"]
            active[context] = queue
            queues.append(queue)
        queue = active.get(context)
        if queue is None:
            continue
        row["inferred_queue_ordinal"] = queue["inferred_queue_ordinal"]
        queue["trace_lines"].append(row["line"])
        if row.get("stage") == BUSY:
            row["derived_busy_sample_ordinal"] = len(queue["busy_rows"]) + 1
            queue["busy_rows"].append(row)
    for queue in queues:
        rows = queue.pop("busy_rows")
        queue["actual_queue_ids_seen"] = sorted({row["actual_queue_id"] for row in rows if row["actual_queue_id"] is not None})
        queue["mixed_explicit_queue_ids"] = len(queue["actual_queue_ids_seen"]) > 1
        queue["observations"] = ordered_observations(rows)
        queue["identity_limit"] = "same kind may recur/interleave; inferred ordinals and positional samples are not actual identities"
    return queues


def analyze(result_path, log_paths):
    result_info, _ = read_result(result_path)
    logs = [read_log(path) for path in log_paths]
    queues = [queue for log in logs for queue in group_queues(log)]
    rows = [row for log in logs for row in log["rows"]]
    busy = [row for row in rows if row.get("stage") == BUSY]
    counts = {"missing_result_files": int(result_info["missing"]), "malformed_result_files": int(result_info["malformed"]),
              "result_read_errors": int("error" in result_info), "missing_result_fields": len(result_info.get("missing_fields", [])),
              "owner_logs": len(logs), "missing_owner_logs": sum(log["missing"] for log in logs),
              "owner_log_read_errors": sum("error" in log for log in logs),
              "owner_logs_without_trace_rows": sum(not log["rows"] for log in logs),
              "owner_logs_without_local_advertise_busy": sum(not any(row.get("stage") == BUSY for row in log["rows"]) for log in logs),
              "trace_rows": len(rows), "local_advertise_busy_rows": len(busy),
              "rows_missing_required_fields": sum(bool(row["missing_fields"]) for row in rows),
              "missing_required_fields": sum(len(row["missing_fields"]) for row in rows),
              "malformed_rows": sum(bool(row["malformed"]) for row in rows),
              "malformed_items": sum(len(row["malformed"]) for row in rows),
              "busy_rows_without_actual_queue_id": sum(row["actual_queue_id"] is None for row in busy),
              "busy_rows_without_actual_attempt_id": sum(row["actual_attempt_id"] is None for row in busy),
              "busy_rows_without_queue_enter_group": sum(row["inferred_queue_ordinal"] is None for row in busy),
              "inferred_queue_groups": len(queues), "inferred_groups_with_busy_samples": sum(queue["observations"]["sample_count"] > 0 for queue in queues)}
    incomplete = (result_info["missing"] or result_info["malformed"] or "error" in result_info
                  or result_info.get("missing_fields") or counts["missing_owner_logs"] or counts["owner_log_read_errors"]
                  or counts["rows_missing_required_fields"] or counts["malformed_rows"])
    report = {"kind": "hex-ack-pacing-trace-observations", "diagnostic_only": True,
              "analysis_status": "incomplete" if incomplete else "parsed", "seed_generation": "not performed; diagnostic only",
              "original_stall_status": "not determined by this analyzer", "original_stall_fixed": None,
              "unit": "nanoseconds", "result": result_info, "counts": counts,
              "trace_evidence": "local Advertise Busy rows present" if busy else "no local Advertise Busy rows supplied",
              "queue_grouping_authority": "explicit queue-enter only, scoped to file/node/peer/kind; ordinal is inferred",
              "actual_identity_rule": "missing queue/attempt IDs remain null; derived ordinals never replace actual identities",
              "interpretation_limits": ["No arbitrary ratio, duration or sample-count threshold is imposed.",
                                        "outer_ns includes legitimate remote Describe/other outer work; outer vs local alone does not establish feedback.",
                                        "Monotonic waits and exact carryover inequalities are observations, not causal/fix claims.",
                                        "Stage logs may interleave; inferred contexts are not globally unique queues.",
                                        "Each log is read only through its size at open, not followed or merged by fabricated timestamps."],
              "inferred_queues": queues, "owner_logs": logs}
    return report, bool(incomplete)


def main():
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--result", required=True, type=Path, help="explicit existing result.json")
    parser.add_argument("--owner-log", action="append", required=True, type=Path, help="explicit owner log; repeat for multiple files")
    parser.add_argument("--output", type=Path, help="new JSON report path, otherwise stdout; existing files are never overwritten")
    args = parser.parse_args()
    result_path = args.result.expanduser().resolve()
    log_paths = [path.expanduser().resolve() for path in args.owner_log]
    if len(set(log_paths)) != len(log_paths):
        parser.error("duplicate owner-log paths would duplicate observations")
    output = None if args.output is None else args.output.expanduser().resolve()
    if output in {result_path, *log_paths}:
        parser.error("output must not replace an input artifact")
    report, incomplete = analyze(result_path, log_paths)
    if output is None:
        json.dump(report, sys.stdout, indent=2, sort_keys=True)
        sys.stdout.write("\n")
    else:
        with output.open("x", encoding="utf-8") as stream:
            json.dump(report, stream, indent=2, sort_keys=True)
            stream.write("\n")
    return 1 if incomplete else 0


if __name__ == "__main__":
    raise SystemExit(main())
