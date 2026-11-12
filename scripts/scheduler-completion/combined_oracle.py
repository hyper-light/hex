"""AC-T7.3.1/4.1: combined backend, resource return and guard-effect receipts."""

import re

from evidence import (BACKENDS, TARGETS, artifact, check_run, hex_digest, load_json,
                      natural, require, sha256)


RESOURCE_FIELDS = ("cpu_milli", "mem_bytes", "disk_bytes")
COMMON_CAPABILITIES = ("runtime_class", "initialized_factory", "physical_selection", "native_process")
BACKEND_CAPABILITIES = {
    "oci": ("oci_isolation", "device_enforcement"),
    "firecracker": ("kvm", "iommu", "vfio", "firecracker_vmm", "device_passthrough"),
    "shards": ("shards_vmm", "physical_renderer_selection", "physical_probe_v2"),
}
EFFECTS = ("foreign_dispatch", "wrong_identity_dispatch", "selection_changed",
           "holder_duplicated", "release_before_scope_end", "backend_substitution")


def identity(value):
    require(isinstance(value, dict) and value.get("kind") == "gpu"
            and natural(value.get("key")), "exact native GPU identity required")
    return value["kind"], value["key"]


def capabilities(report, source, backend, target, contract_sha256):
    """Capability gaps remain unverified even when another backend can execute."""
    require(backend in BACKENDS and target in TARGETS, "unknown backend/target")
    require(isinstance(report, dict) and report.get("schema") == "hex-completion-capability-2",
            "initialized factory capability receipt required")
    require(report.get("source") == source and report.get("target") == target
            and report.get("backend") == backend, "capability belongs to another source or backend")
    execution = report.get("execution_target")
    topology = report.get("topology")
    require(execution in TARGETS and topology in ("laptop", "peer"),
            "explicit origin/execution target and topology required")
    require(report.get("scope") == "root-native-capability", "capability is not a native observation")
    require(hex_digest(contract_sha256) and report.get("contract_sha256") == contract_sha256,
            "capability factory contract is not source-pinned")
    observations = report.get("capabilities")
    require(isinstance(observations, dict), "capability observations required")
    required = COMMON_CAPABILITIES + BACKEND_CAPABILITIES[backend]
    gaps = []
    for name in required:
        row = observations.get(name)
        if row is None:
            gaps.append({"capability": name, "reason": "unobserved"})
            continue
        require(isinstance(row, dict) and row.get("state") in ("available", "unavailable", "unobserved"),
                "typed capability state required")
        if row["state"] != "available":
            gaps.append({"capability": name, "reason": row.get("reason", row["state"])})
        else:
            require(row.get("observed") is True and isinstance(row.get("witness"), dict),
                    "available capability lacks its retained native witness")
    if topology == "laptop" and execution != target:
        gaps.append({"capability": "routing", "reason": "local execution target differs from origin"})
    if backend == "firecracker" and execution not in TARGETS[:4]:
        gaps.append({"capability": "kvm", "reason": "Firecracker requires a Linux execution target"})
    return {"backend": backend, "target": target, "execution_target": execution, "topology": topology,
            "outcome": "unverified" if gaps else "verified", "gaps": gaps}


def resource_values(value):
    require(isinstance(value, dict) and set(value) == set(RESOURCE_FIELDS),
            "complete typed resource amounts required")
    require(all(natural(value[name]) for name in RESOURCE_FIELDS), "invalid resource amount")
    return tuple(value[name] for name in RESOURCE_FIELDS)


def rows_by_id(rows, field):
    require(isinstance(rows, list), "resource observation rows required")
    result = {}
    for row in rows:
        require(isinstance(row, dict) and isinstance(row.get(field), str) and row[field],
                "resource observation identity required")
        require(row[field] not in result, "duplicate resource observation")
        result[row[field]] = row
    return result


def scope_identity(row):
    require(isinstance(row, dict) and hex_digest(row.get("native_scope"), 32)
            and hex_digest(row.get("native_commitment")),
            "exact 128-bit native incarnation and immutable scope/spec commitment required")
    return row["native_scope"], row["native_commitment"]


def scope_history(after, owned_boxes):
    rows = rows_by_id(after.get("scope_history"), "id")
    require(set(rows) == set(owned_boxes), "every owned box needs its original binding/creating history")
    original = {}
    gaps = []
    for identifier, row in rows.items():
        observations = row.get("observations")
        require(isinstance(observations, list)
                and all(isinstance(item, dict) for item in observations)
                and [item.get("stage") for item in observations]
                == ["binding", "creating", "scheduler-recovery", "runtime-recovery"],
                "binding/creating and BOTH restart scope observations required in order")
        identities = []
        for item in observations:
            if item.get("observed") is False:
                require(item["stage"] == "creating" and item.get("source_phase") is None
                        and isinstance(item.get("reason"), str) and item["reason"]
                        and not any(name in item for name in ("native_scope", "native_commitment", "record")),
                        "only explicit live Creating observation uncertainty is a phase gap")
                gaps.append({"id": identifier, "stage": "creating", "reason": item["reason"]})
                continue
            require(item.get("observed") is True, "typed scope observation required")
            if item["stage"] == "creating":
                require(item.get("source_phase") == "Creating" and item.get("init_published") is False,
                        "Ready origin or a published native init is not live Creating evidence")
            else:
                require(item.get("source_phase") == "Ready", "immutable Ready scope must name its actual source phase")
            identities.append(scope_identity(item))
        require(all(identity == identities[0] for identity in identities),
                "a restart transplanted another incarnation or original scope/spec")
        original[identifier] = identities[0]
    require(len({scope for scope, _ in original.values()}) == len(original),
            "native incarnation was reused by another owned box")
    return original, gaps


def scope_records(root, history, owned_boxes, byte_limit):
    """Join independent raw Store observations; notification/phase labels alone do not count."""
    _, gaps = scope_history({"scope_history": history}, owned_boxes)
    for row in history:
        original_spec = None
        for item in row["observations"]:
            if item["observed"] is False:
                continue
            record = load_json(artifact(root, item.get("record")), byte_limit)
            require(isinstance(record, dict) and type(record.get("version")) is int and record["version"] == 1
                    and isinstance(record.get("spec"), dict)
                    and record["spec"].get("id") == row["id"], "raw scope record belongs to another box")
            native = record.get("native_scope")
            require(isinstance(native, list) and len(native) == 16
                    and all(natural(value) and value <= 255 for value in native),
                    "raw record lacks the actual 128-bit native scope")
            require(bytes(native).hex() == item["native_scope"], "raw and declared native scope differ")
            if original_spec is None:
                original_spec = record["spec"]
            require(record["spec"] == original_spec, "raw scope/spec changed across publication or restart")
            if item["stage"] == "creating":
                require(record.get("phase") == "creating" and record.get("init") is None,
                        "Creating bytes were copied from Ready or after native publication")
            else:
                require(record.get("phase") == "ready", "Ready observation has another raw phase")
    return {"outcome": "unverified" if gaps else "verified", "gaps": gaps}


def restoration(before, after, source, owned_boxes):
    """Compare the existing owner's observations without inventing a holder ledger."""
    require(isinstance(owned_boxes, list) and owned_boxes
            and all(isinstance(item, str) and item for item in owned_boxes)
            and len(set(owned_boxes)) == len(owned_boxes), "exact owned boxes required")
    for snapshot in (before, after):
        require(isinstance(snapshot, dict) and snapshot.get("schema") == "hex-completion-resources-2"
                and snapshot.get("source") == source and snapshot.get("scope") == "root-native-resources",
                "resource observations are not pinned native facts")
        require(snapshot.get("observed") is True, "resource observation uncertainty remains unverified")
        require(isinstance(snapshot.get("authority"), str) and snapshot["authority"],
                "native authority identity required")
        require(isinstance(snapshot.get("pool"), str) and snapshot["pool"], "resource pool identity required")
        total = resource_values(snapshot.get("total"))
        free = resource_values(snapshot.get("free"))
        require(all(room <= limit for room, limit in zip(free, total)), "free resources exceed capacity")
    require((before["authority"], before["pool"]) == (after["authority"], after["pool"]),
            "cleanup observation reopened another authority or pool")
    prior = rows_by_id(before.get("boxes"), "id")
    remaining = rows_by_id(after.get("boxes"), "id")
    require(not set(owned_boxes).intersection(prior), "owned box existed before this fixture")
    require(not set(owned_boxes).intersection(remaining), "owned allocation remains after cleanup")
    require(prior == remaining, "foreign allocations changed during the owned fixture")
    for row in prior.values():
        require(isinstance(row.get("allocation"), dict), "foreign admitted charge is unobserved")
        resource_values(row["allocation"].get("resources"))
    devices_before = {}
    devices_after = {}
    for snapshot, rows in ((before, devices_before), (after, devices_after)):
        require(isinstance(snapshot.get("devices"), list), "complete physical holder snapshot required")
        for device in snapshot["devices"]:
            require(isinstance(device, dict), "typed physical device observation required")
            key = identity(device.get("id"))
            require(key not in rows, "duplicate physical holder identity")
            holder = device.get("holder")
            require(holder is None or isinstance(holder, str), "typed physical holder required")
            require(holder not in owned_boxes, "owned physical claim remains after cleanup")
            rows[key] = holder
    require(devices_before == devices_after, "physical inventory or foreign holder changed")
    require(after.get("owned_native_processes") == [] and after.get("owned_artifacts") == [],
            "positive native/artifact absence observations required")
    ends = rows_by_id(after.get("terminal_scopes"), "id")
    original, missing = scope_history(after, owned_boxes)
    require(set(ends) == set(owned_boxes), "every owned native scope needs a terminal receipt")
    require(all(row.get("scope") == "released" and row.get("observed") is True
                and row.get("record_present") is False for row in ends.values()),
            "controller exit or missing record alone cannot prove whole-scope release")
    require(all(scope_identity(row) == original[identifier] for identifier, row in ends.items()),
            "terminal receipt belongs to another native incarnation or original scope/spec")
    drivers = []
    for snapshot in (before, after):
        require(isinstance(snapshot.get("driver_bindings"), list), "native driver observations required")
        observed = {}
        for row in snapshot["driver_bindings"]:
            require(isinstance(row, dict) and row.get("observed") is True
                    and isinstance(row.get("binding"), str) and row["binding"], "driver binding is unobserved")
            key = identity(row.get("id"))
            require(key not in observed, "duplicate driver binding")
            observed[key] = row["binding"]
        require(set(observed) == set(devices_before), "not every physical GPU has a driver observation")
        drivers.append(observed)
    require(drivers[0] == drivers[1], "host driver bindings were not restored")
    gaps = list(missing)
    if before["total"] != after["total"]:
        gaps.append("physical pool capacity changed")
    if before["free"] != after["free"]:
        # Current room is a native fact and can include ambient activity. No
        # invented byte allowance converts that uncertainty into restoration.
        gaps.append("native free resources differ from the controlled baseline")
    return {"outcome": "unverified" if gaps else "verified", "gaps": gaps,
            "authority": before["authority"], "pool": before["pool"], "owned_boxes": owned_boxes}


def source_restoration(before, mutated, restored, path, original_hash, mutant_hash):
    require(all(isinstance(row, dict) and row for row in (before, mutated, restored)),
            "complete source manifests required")
    require(before == restored, "restored source differs from the original manifest")
    require(all(isinstance(name, str) and name and not name.startswith(("/", "\\"))
                and ".." not in name.replace("\\", "/").split("/") and hex_digest(value)
                for name, value in before.items()), "invalid source manifest path/hash")
    require(all(hex_digest(value) for value in mutated.values()), "invalid mutant source hash")
    require(set(before) == set(mutated), "mutation added or removed an unrecorded source file")
    changed = [name for name in before if before[name] != mutated[name]]
    require(changed == [path] and before.get(path) == original_hash and mutated.get(path) == mutant_hash,
            "mutation changed another source path or missed the guard")
    require(hex_digest(original_hash) and hex_digest(mutant_hash) and original_hash != mutant_hash,
            "exact differing guard hashes required")
    return True


def forbidden_effect(value, source, backend, guard, mutant_binary):
    require(isinstance(value, dict) and value.get("schema") == "hex-completion-forbidden-effect-2"
            and value.get("scope") == "root-native" and value.get("source") == source
            and value.get("backend") == backend and value.get("guard") == guard,
            "mutation effect lacks exact native source/backend/guard attribution")
    require(value.get("binary_sha256") == mutant_binary and hex_digest(mutant_binary),
            "forbidden effect belongs to another mutant executable")
    effect = value.get("effect")
    require(effect in EFFECTS and value.get("observed") is True, "no actual forbidden-effect observation")
    if effect in ("foreign_dispatch", "wrong_identity_dispatch"):
        require(hex_digest(value.get("uuid"), 32) and hex_digest(value.get("expected_uuid"), 32)
                and value["uuid"] != value["expected_uuid"] and natural(value.get("dispatches"), positive=True)
                and natural(value.get("words"), positive=True) and type(value.get("mismatches")) is int
                and value["mismatches"] == 0 and value.get("dispatch_completed") is True,
                "GPU guard mutation lacks completed checked forbidden dispatch")
    elif effect == "selection_changed":
        require(identity(value.get("before")) != identity(value.get("after")), "mutation never repicked")
    elif effect == "holder_duplicated":
        require(natural(value.get("holders"), positive=True) and value["holders"] > 1,
                "mutation never duplicated the holder")
    elif effect == "release_before_scope_end":
        require(value.get("released") is True and natural(value.get("owned_native_processes"), positive=True),
                "mutation never released a still-live native scope")
    else:
        require(value.get("expected_class") == backend and value.get("actual_class") in BACKENDS
                and value["actual_class"] != backend, "mutation never substituted a backend")
    return effect


def negative_libtest(raw):
    rows = re.findall(r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured;", raw, re.M)
    require(rows == [("FAILED", "0", "1", "0", "0")], "mutation needs exactly one actual failing case")
    require(not re.search(r"could not compile|error\[E\d+\]|UNVERIFIED|SKIP|compile-only", raw),
            "compile/unavailable failure cannot kill a behavioral mutation")


def mutation(root, receipt, source, backend, byte_limit):
    require(receipt.get("schema") == "hex-completion-mutation-2" and receipt.get("source") == source
            and receipt.get("backend") == backend, "wrong mutation cohort")
    require(isinstance(receipt.get("guard"), str) and receipt["guard"], "named guard required")
    manifests = [load_json(artifact(root, receipt.get(name)), byte_limit)
                 for name in ("before_manifest", "mutated_manifest", "restored_manifest")]
    source_restoration(*manifests, receipt.get("file"), receipt.get("original_sha256"), receipt.get("mutated_sha256"))
    for name in ("baseline", "restored"):
        path = artifact(root, receipt.get(name))
        run = load_json(path, byte_limit)
        require(run.get("source") == source and run.get("backend") == backend,
                "mutation control uses another source/backend")
        verified = check_run(path.parent, run, byte_limit)
        require(verified["proof"] == "full", "selected-only control does not validate a full mutation")
        history = load_json(artifact(path.parent, run.get("scope_history")), byte_limit)
        phases = scope_records(path.parent, history, [verified["physical"]], byte_limit)
        require(phases["outcome"] == "verified", "mutation control lacks independently observed Creating scope")
    mutant = receipt.get("mutant")
    require(isinstance(mutant, dict) and type(mutant.get("exit")) is int and mutant["exit"] != 0
            and mutant.get("timed_out") is False and mutant.get("reaped") is True,
            "mutant has no bounded actual red execution")
    log = artifact(root, mutant.get("log"))
    require(log.stat().st_size <= byte_limit, "mutation log exceeds caller byte budget")
    negative_libtest(log.read_text())
    binary = artifact(root, mutant.get("binary"))
    build_path = artifact(root, mutant.get("build"))
    build = load_json(build_path, byte_limit)
    require(build.get("source") == source and build.get("exit") == 0 and build.get("reaped") is True
            and build.get("timed_out") is False and build.get("source_unchanged") is True,
            "mutant has no successful checked-source build")
    require(build.get("binary", {}).get("sha256") == sha256(binary)
            and build.get("overlay", {}).get("file") == receipt.get("file")
            and build["overlay"].get("sha256") == receipt.get("mutated_sha256")
            and build["overlay"].get("original_sha256") == receipt.get("original_sha256"),
            "mutant binary was not built with the exact guard overlay")
    for name in ("source_before", "source_after"):
        require(load_json(artifact(build_path.parent, build.get(name)), byte_limit) == manifests[1],
                "mutant build input did not exactly match the retained complete mutant source")
    effect = load_json(artifact(root, receipt.get("forbidden_effect")), byte_limit)
    kind = forbidden_effect(effect, source, backend, receipt["guard"], sha256(binary))
    before_resources = load_json(artifact(root, receipt.get("before_resources")), byte_limit)
    after_resources = load_json(artifact(root, receipt.get("after_resources")), byte_limit)
    restored = restoration(before_resources, after_resources, source, receipt.get("owned_boxes"))
    require(restored["outcome"] == "verified", "guard restoration alone does not establish resource restoration")
    return {"outcome": "verified", "backend": backend, "guard": receipt["guard"], "effect": kind}
