#!/usr/bin/env python3
"""ROOT-only combined OCI/Firecracker/Shards run; missing capability stays unverified."""

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace

import run as single
from combined_oracle import (BACKEND_CAPABILITIES, COMMON_CAPABILITIES, capabilities,
                             mutation, restoration, scope_records)
from evidence import (BACKENDS, POLICY, RULES, TARGETS, InvalidEvidence, artifact,
                      check_run, finite_positive, hex_digest, load_json, natural, require, sha256)


def write(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def plan_cases(plan):
    require(isinstance(plan, dict) and plan.get("schema") == "hex-completion-combined-plan-2",
            "combined plan schema required")
    require(hex_digest(plan.get("source"), 40) and plan.get("target") in TARGETS
            and plan.get("topology") in ("laptop", "peer"), "exact combined source/target/topology required")
    rows = plan.get("cases")
    require(isinstance(rows, list) and len(rows) == len(BACKENDS), "all three backends must be explicit")
    result = {}
    for row in rows:
        require(isinstance(row, dict) and row.get("backend") in BACKENDS
                and row["backend"] not in result, "unknown/duplicate combined backend")
        require(finite_positive(row.get("timeout_seconds"))
                and finite_positive(row.get("cleanup_timeout_seconds")), "explicit finite case budgets required")
        require(row.get("proof") == "full", "selected-only proof cannot complete a combined backend cell")
        require(row.get("execution_target") in TARGETS, "each cell needs its actual execution target")
        require(plan["topology"] == "peer" or row["execution_target"] == plan["target"],
                "local cell cannot substitute another execution host")
        guards = row.get("required_guards")
        require(isinstance(guards, list) and guards and all(isinstance(guard, str) and guard for guard in guards)
                and len(set(guards)) == len(guards), "explicit nonempty guard census required")
        if plan["topology"] == "peer":
            require(hex_digest(row.get("origin_binary_sha256")), "peer origin executable pin required")
        result[row["backend"]] = row
    require(set(result) == set(BACKENDS), "missing required backend")
    return result


def capability_bundle(root, row, source, backend, target, byte_limit):
    contract = artifact(root, row.get("contract"))
    report = load_json(artifact(root, row.get("capability")), byte_limit)
    checked = capabilities(report, source, backend, target, sha256(contract))
    # Available is a typed observation with a retained artifact, never a label
    # supplied by the plan. A completed workload is still required separately.
    for name in COMMON_CAPABILITIES + BACKEND_CAPABILITIES[backend]:
        claim = report["capabilities"].get(name)
        if claim is not None and claim["state"] == "available":
            witness = load_json(artifact(root, claim["witness"]), byte_limit)
            require(isinstance(witness, dict)
                    and witness.get("schema") == "hex-completion-capability-witness-2"
                    and witness.get("source") == source and witness.get("backend") == backend
                    and witness.get("target") == target and witness.get("capability") == name
                    and witness.get("execution_target") == report["execution_target"]
                    and witness.get("topology") == report["topology"]
                    and witness.get("contract_sha256") == sha256(contract)
                    and witness.get("observed") is True and witness.get("scope") == "root-native",
                    "capability artifact is not an exact observed native claim")
            require(isinstance(witness.get("observation"), dict) and witness["observation"],
                    "capability witness has no observation")
    return checked, report


def retain(input_root, row, output_root):
    source = artifact(input_root, row)
    destination = output_root / row["path"]
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        artifact(output_root, row)
    else:
        shutil.copy2(source, destination)
        artifact(output_root, row)
    return destination


def retain_run(input_root, row, output_root, byte_limit):
    path = retain(input_root, row, output_root)
    original = artifact(input_root, row)
    value = load_json(original, byte_limit)
    for name in ("binary", "fixture", "build", "log", "events", "resources_before", "resources_after", "scope_history"):
        if value.get(name) is not None:
            retain(original.parent, value[name], path.parent)
    if value.get("scope_history") is not None:
        history = load_json(artifact(original.parent, value["scope_history"]), byte_limit)
        require(isinstance(history, list), "typed retained scope history required")
        for item in history:
            require(isinstance(item, dict) and isinstance(item.get("observations"), list), "scope observations required")
            for observation in item["observations"]:
                if isinstance(observation, dict) and observation.get("record") is not None:
                    retain(original.parent, observation["record"], path.parent)
    return path


def retain_mutation(input_root, row, output_root, byte_limit):
    path = retain(input_root, row, output_root)
    original = artifact(input_root, row)
    value = load_json(original, byte_limit)
    for name in ("before_manifest", "mutated_manifest", "restored_manifest",
                 "forbidden_effect", "before_resources", "after_resources"):
        retain(original.parent, value.get(name), path.parent)
    for name in ("baseline", "restored"):
        retain_run(original.parent, value.get(name), path.parent, byte_limit)
    mutant = value.get("mutant")
    require(isinstance(mutant, dict), "mutant execution receipt required")
    for name in ("binary", "log", "build"):
        retain(original.parent, mutant.get(name), path.parent)
    original_build = artifact(original.parent, mutant["build"])
    retained_build = artifact(path.parent, mutant["build"])
    build = load_json(original_build, byte_limit)
    for name in ("source_before", "source_after"):
        retain(original_build.parent, build.get(name), retained_build.parent)
    return path


def retain_origin(input_root, row, output_root, byte_limit):
    path = retain(input_root, row, output_root)
    original = artifact(input_root, row)
    value = load_json(original, byte_limit)
    for name in ("binary", "reply_artifact"):
        retain(original.parent, value.get(name), path.parent)
    return path


def check_origin(root, row, plan, case, run_row, workload, byte_limit):
    path = artifact(root, row)
    value = load_json(path, byte_limit)
    require(value.get("schema") == "hex-completion-origin-1" and value.get("scope") == "root-peer-origin"
            and value.get("source") == plan["source"] and value.get("target") == plan["target"]
            and value.get("outcome") == "verified" and type(value.get("exit")) is int
            and value["exit"] == 0 and value.get("reaped") is True and natural(value.get("pid"), positive=True),
            "actual source-pinned peer origin control required")
    binary = artifact(path.parent, value.get("binary"))
    require(sha256(binary) == case["origin_binary_sha256"], "origin used another Hex executable")
    reply = load_json(artifact(path.parent, value.get("reply_artifact")), byte_limit)
    require(reply == value.get("reply") and isinstance(reply, dict)
            and value.get("physical") == workload["physical"] and reply.get("physical") == workload["physical"]
            and reply.get("key") == workload["key"] and reply.get("tracked") is True
            and reply.get("created") is True and reply.get("refusals") == []
            and natural(reply.get("at")), "origin did not observe the same live placement")
    require(value.get("runner_receipt_sha256") == run_row["sha256"],
            "origin receipt was not finalized against this exact native runner result")
    return {"target": plan["target"], "execution_target": case["execution_target"],
            "routed_to_capable_peer": True, "receipt_sha256": row["sha256"]}


def check_cell(root, inputs, plan, case, result, byte_limit):
    backend = case["backend"]
    capability, _ = capability_bundle(inputs, case, plan["source"], backend, plan["target"], byte_limit)
    require(capability["execution_target"] == case["execution_target"]
            and capability["topology"] == plan["topology"], "planned/native execution route mismatch")
    if capability["outcome"] != "verified":
        require(result.get("run") is None, "unavailable capability must not be presented as a successful run")
        return {**capability, "native_cases": 0}
    run_path = artifact(root, result.get("run"))
    run = load_json(run_path, byte_limit)
    require((run.get("source"), run.get("backend"), run.get("target"), run.get("topology"))
            == (plan["source"], backend, case["execution_target"], plan["topology"]), "combined run belongs to another cell")
    verified = check_run(run_path.parent, run, byte_limit)
    require(verified["proof"] == "full", "combined cell lacks the complete backend workload")
    fixture = load_json(artifact(run_path.parent, run.get("fixture")), byte_limit)
    require(fixture.get("factory_contract_sha256") == case["contract"]["sha256"],
            "executed configuration is not pinned to the initialized factory contract")
    gaps = []
    if run.get("resources_before") is None or run.get("resources_after") is None:
        resources = {"outcome": "unverified", "gaps": ["complete native resource/terminal observations missing"]}
    else:
        before = load_json(artifact(run_path.parent, run.get("resources_before")), byte_limit)
        after = load_json(artifact(run_path.parent, run.get("resources_after")), byte_limit)
        require(after.get("owned_boxes") == [verified["physical"]], "resource observations belong to another workload box")
        if before.get("observed") is not True or after.get("observed") is not True:
            resources = {"outcome": "unverified", "gaps": ["native driver/whole-scope resource producer is unverified"]}
        elif run.get("scope_history") is None:
            resources = {"outcome": "unverified", "gaps": ["independent Creating/Ready/restart record artifacts missing"]}
        else:
            history = load_json(artifact(run_path.parent, run["scope_history"]), byte_limit)
            require(after.get("scope_history") in (None, history), "resource and independent scope histories disagree")
            after = {**after, "scope_history": history}
            phases = scope_records(run_path.parent, history, after.get("owned_boxes"), byte_limit)
            resources = restoration(before, after, plan["source"], after.get("owned_boxes"))
            resources["gaps"].extend(phases["gaps"])
            if resources["gaps"]:
                resources["outcome"] = "unverified"
    route = None
    if plan["topology"] == "peer":
        if result.get("origin") is None:
            gaps.append("actual peer-origin control bundle missing")
        else:
            route = check_origin(root, result["origin"], plan, case, result["run"], verified, byte_limit)
    guards = case.get("required_guards")
    receipts = result.get("mutations")
    require(isinstance(guards, list) and guards and all(isinstance(guard, str) and guard for guard in guards)
            and len(set(guards)) == len(guards), "explicit required guard census is missing")
    require(isinstance(receipts, list), "mutation receipt list required; missing campaigns remain unverified")
    seen = set()
    mutation_results = []
    for row in receipts:
        path = artifact(root, row)
        value = load_json(path, byte_limit)
        guard = value.get("guard")
        require(guard in guards and guard not in seen, "unexpected/duplicate mutation guard")
        seen.add(guard)
        mutation_results.append(mutation(path.parent, value, plan["source"], backend, byte_limit))
    gaps.extend(resources["gaps"])
    gaps.extend("required native mutation not observed: " + name for name in guards if name not in seen)
    return {"backend": backend, "target": plan["target"], "outcome": "unverified" if gaps else "verified",
            "native_cases": 1, "workload": verified, "resources": resources,
            "route": route, "mutations": mutation_results, "gaps": gaps}


def assess(root, receipt, byte_limit):
    require(receipt.get("schema") == "hex-completion-combined-2"
            and receipt.get("scope") == "root-native-combined"
            and receipt.get("policy") == POLICY and receipt.get("rules_sha256") == RULES,
            "current combined native receipt required")
    plan = load_json(artifact(root, receipt.get("plan")), byte_limit)
    cases = plan_cases(plan)
    inputs = root / "inputs"
    require(inputs.is_dir() and not inputs.is_symlink(), "retained combined input directory required")
    rows = receipt.get("cases")
    require(isinstance(rows, list) and len(rows) == len(BACKENDS), "combined result omitted a backend")
    results = {}
    for row in rows:
        require(isinstance(row, dict) and row.get("backend") in cases and row["backend"] not in results,
                "unexpected/duplicate combined outcome")
        backend = row["backend"]
        try:
            results[backend] = check_cell(root, inputs, plan, cases[backend], row, byte_limit)
        except (InvalidEvidence, OSError, UnicodeError) as error:
            results[backend] = {"backend": backend, "target": plan["target"],
                                "outcome": "failed" if row.get("run") is not None else "unverified",
                                "gaps": [str(error)]}
    ordered = [results[backend] for backend in BACKENDS]
    outcome = ("failed" if any(row["outcome"] == "failed" for row in ordered) else
               "unverified" if any(row["outcome"] != "verified" for row in ordered) else "verified")
    return {"outcome": outcome, "source": plan["source"], "target": plan["target"],
            "topology": plan["topology"], "required_backends": list(BACKENDS), "cases": ordered,
            "verified_backends": sum(row["outcome"] == "verified" for row in ordered)}


def execute(args):
    require(args.collect_only or args.root_authorized, "combined native execution is ROOT-only")
    require(natural(args.byte_limit, positive=True), "explicit artifact byte budget required")
    original_plan = args.plan.resolve(strict=True)
    plan = load_json(original_plan, args.byte_limit)
    cases = plan_cases(plan)
    source_root = args.inputs.resolve(strict=True)
    root = args.output.resolve()
    require(not root.exists() and not root.is_relative_to(source_root), "new private combined output required")
    inputs = root / "inputs"
    inputs.mkdir(parents=True)
    shutil.copy2(original_plan, root / "plan.json")
    require(sha256(original_plan) == sha256(root / "plan.json"), "plan changed during retention")
    receipt = {"schema": "hex-completion-combined-2", "scope": "root-native-combined",
               "policy": POLICY, "rules_sha256": RULES, "plan": single.file_receipt(root, root / "plan.json"),
               "cases": []}
    for backend in BACKENDS:
        case = cases[backend]
        result = {"backend": backend, "run": None, "mutations": []}
        receipt["cases"].append(result)
        write(root / "combined.json", receipt)
        try:
            for name in ("contract", "capability"):
                retain(source_root, case.get(name), inputs)
            report = load_json(artifact(inputs, case["capability"]), args.byte_limit)
            for claim in report.get("capabilities", {}).values():
                if isinstance(claim, dict) and claim.get("state") == "available":
                    retain(source_root, claim.get("witness"), inputs)
            capability, _ = capability_bundle(inputs, case, plan["source"], backend, plan["target"], args.byte_limit)
            result["capability"] = capability
            if capability["outcome"] != "verified":
                continue
            if case.get("run") is not None:
                path = retain_run(source_root, case["run"], inputs, args.byte_limit)
                result["run"] = single.file_receipt(root, path)
            elif args.collect_only:
                result["state"] = "awaiting-native-run"
                continue
            else:
                for name in ("binary", "build", "fixture"):
                    retain(source_root, case.get(name), inputs)
                config = load_json(artifact(inputs, case["fixture"]), args.byte_limit)
                require(config.get("factory_contract_sha256") == case["contract"]["sha256"],
                        "configuration lacks exact factory contract pin")
                output = root / backend
                single.run(SimpleNamespace(root_authorized=True, source=plan["source"],
                    binary=artifact(inputs, case["binary"]), binary_sha256=case["binary"]["sha256"],
                    build_receipt=artifact(inputs, case["build"]), config=artifact(inputs, case["fixture"]),
                    backend=backend, topology=plan["topology"], proof="full", target=case["execution_target"],
                    timeout=case["timeout_seconds"], cleanup_timeout=case["cleanup_timeout_seconds"],
                    byte_limit=args.byte_limit, output=output))
                result["run"] = single.file_receipt(root, output / "run.json")
            # Native mutation campaigns run separately in disposable ROOT source.
            # Their complete bundles must be retained before the combined gate can pass.
            if case.get("origin") is not None:
                path = retain_origin(source_root, case["origin"], inputs, args.byte_limit)
                result["origin"] = single.file_receipt(root, path)
            for row in case.get("mutations", []):
                path = retain_mutation(source_root, row, inputs, args.byte_limit)
                result["mutations"].append(single.file_receipt(root, path))
        except (InvalidEvidence, OSError, subprocess.SubprocessError, UnicodeError) as error:
            result["error"] = str(error)
        finally:
            write(root / "combined.json", receipt)
    receipt["assessment"] = assess(root, receipt, args.byte_limit)
    write(root / "combined.json", receipt)
    print(json.dumps(receipt["assessment"]))
    return 0 if receipt["assessment"]["outcome"] == "verified" else 1


def attach(args):
    require(natural(args.byte_limit, positive=True), "explicit artifact byte budget required")
    receipt_path = args.receipt.resolve(strict=True)
    root = receipt_path.parent
    receipt = load_json(receipt_path, args.byte_limit)
    plan = load_json(artifact(root, receipt.get("plan")), args.byte_limit)
    plan_cases(plan)
    result = [row for row in receipt["cases"] if row.get("backend") == args.backend]
    require(len(result) == 1, "exact existing combined backend required")
    original = args.bundle.resolve(strict=True)
    row = single.file_receipt(original.parent, original)
    # A new, namespaced intake cannot overwrite a prior run, plan or observation.
    destination = root / "intake" / args.backend / args.kind / row["sha256"]
    require(not destination.exists(), "bundle was already retained; assess it without importing again")
    destination.mkdir(parents=True)
    importer = {"origin": retain_origin, "mutation": retain_mutation, "run": retain_run}[args.kind]
    path = importer(original.parent, row, destination, args.byte_limit)
    retained = single.file_receipt(root, path)
    if args.kind in ("origin", "run"):
        require(result[0].get(args.kind) is None, args.kind + " receipt already attached")
        result[0][args.kind] = retained
    else:
        result[0]["mutations"].append(retained)
    receipt["assessment"] = assess(root, receipt, args.byte_limit)
    write(receipt_path, receipt)
    print(json.dumps(receipt["assessment"]))
    return 0 if receipt["assessment"]["outcome"] == "verified" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    run = modes.add_parser("execute")
    run.add_argument("--root-authorized", action="store_true")
    run.add_argument("--collect-only", action="store_true", help="retain/assess only; never invoke a native workload")
    run.add_argument("--plan", type=Path, required=True)
    run.add_argument("--inputs", type=Path, required=True)
    run.add_argument("--output", type=Path, required=True)
    run.add_argument("--byte-limit", type=int, required=True)
    check = modes.add_parser("assess")
    check.add_argument("--receipt", type=Path, required=True)
    check.add_argument("--byte-limit", type=int, required=True)
    intake = modes.add_parser("attach")
    intake.add_argument("--receipt", type=Path, required=True)
    intake.add_argument("--byte-limit", type=int, required=True)
    intake.add_argument("--backend", choices=BACKENDS, required=True)
    intake.add_argument("--kind", choices=("origin", "mutation", "run"), required=True)
    intake.add_argument("--bundle", type=Path, required=True)
    args = parser.parse_args()
    if args.mode == "execute":
        return execute(args)
    if args.mode == "attach":
        return attach(args)
    path = args.receipt.resolve(strict=True)
    result = assess(path.parent, load_json(path, args.byte_limit), args.byte_limit)
    print(json.dumps(result))
    return 0 if result["outcome"] == "verified" else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (InvalidEvidence, OSError, subprocess.SubprocessError, UnicodeError) as error:
        print(f"UNVERIFIED combined run: {error}", file=sys.stderr)
        sys.exit(1)
