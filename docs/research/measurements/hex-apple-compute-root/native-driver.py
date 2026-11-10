"""ROOT-only AC-T7.3.1: selected Metal compute, wrong identity and missing-write control.

Run on the admitted native Mac with an immutable Git archive on stdin. This is
functional evidence; native driver allocation/leak freedom and VM attachment are
separate work. Required registry ID is supplied from ROOT's measured inventory.
"""
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import subprocess
import sys
import tarfile

root, expected_archive, revision, registry_id = sys.argv[1:5]
root = Path(root)
root.mkdir(parents=True, exist_ok=False)
archive_bytes = sys.stdin.buffer.read()
assert hashlib.sha256(archive_bytes).hexdigest() == expected_archive
(root / "source.tar.gz").write_bytes(archive_bytes)
source = root / "source"
source.mkdir()
with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz") as archive:
    for member in archive.getmembers():
        path = PurePosixPath(member.name)
        assert not path.is_absolute() and ".." not in path.parts
        assert member.isfile() or member.isdir()
    archive.extractall(source)


def hashes(folder):
    return {
        str(path.relative_to(folder)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(folder.rglob("*"))
        if path.is_file()
    }


manifest = hashes(source)
(root / "source-hashes.json").write_text(json.dumps(manifest, indent=2) + "\n")
for directory in ["cache/cargo", "target", "temp"]:
    (root / directory).mkdir(parents=True)
rustup = Path.home() / ".cache/hex-lead/rustup-scheduler-copy"
toolchain = rustup / "toolchains/1.99.1-aarch64-apple-darwin/bin"
env = {key: value for key, value in os.environ.items() if not key.startswith(("DYLD_", "HEX_METAL_", "HEX_DEBUG_", "SHARDS_DEBUG_"))}
env.update(PATH=str(toolchain) + os.pathsep + env["PATH"], RUSTUP_HOME=str(rustup), RUSTC=str(toolchain / "rustc"), RUSTDOC=str(toolchain / "rustdoc"), CARGO_HOME=str(root / "cache/cargo"), CARGO_TARGET_DIR=str(root / "target"), CARGO_BUILD_JOBS="1", RUST_TEST_THREADS="1", RAYON_NUM_THREADS="1", TMPDIR=str(root / "temp"))
# A single functional round over one native page of u32 is a test vector, not a
# production tuning value. Buffers+oracle take three pages, plus native overhead.
page_bytes = os.sysconf("SC_PAGE_SIZE")
assert page_bytes > 0 and page_bytes % 4 == 0
env.update(HEX_METAL_REGISTRY_ID=str(int(registry_id)), HEX_METAL_ELEMENTS=str(page_bytes // 4), HEX_METAL_ROUNDS="1")
# The existing recorded validation-run guard bounds the entire native invocation,
# including shader compilation and the native wait. Timeout is uncertainty/failure.
records = (source / "bench/native/guest/records.tsv").read_text().splitlines()
samples = [int(line.split("\t")[2]) for line in records if line.startswith("validation_run\t")]
assert samples and min(samples) > 0
native_guard_seconds = 2 * max(samples) / 1_000_000
result = {"source": revision, "archive_sha256": expected_archive, "platform": platform.platform(), "registry_id": int(registry_id), "page_bytes": page_bytes, "elements": page_bytes // 4, "rounds": 1, "native_guard_seconds": native_guard_seconds, "guard_source": "twice the slowest existing validation_run record", "phases": []}


def run(name, command, run_env, cwd, timeout, expected_exit, required):
    phase = {"name": name, "command": command, "expected_exit": expected_exit, "guard_seconds": timeout}
    result["phases"].append(phase)
    with (root / (name + ".log")).open("xb") as log:
        child = subprocess.Popen(command, cwd=cwd, env=run_env, stdout=log, stderr=subprocess.STDOUT)
        phase["pid"] = child.pid
        (root / "run.json").write_text(json.dumps(result, indent=2) + "\n")
        try:
            phase["exit"] = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            phase["timed_out"] = True
            child.kill()
            child.wait(timeout=native_guard_seconds)
            phase["exit"] = child.returncode
    phase["reaped"] = child.returncode is not None
    raw = (root / (name + ".log")).read_bytes()
    phase.update(sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw), required=all(item in raw for item in required))
    (root / "run.json").write_text(json.dumps(result, indent=2) + "\n")
    if phase["exit"] != expected_exit or not phase["required"] or phase.get("timed_out"):
        raise RuntimeError("phase failed: " + name)
    return raw


def build(name, folder, build_env):
    raw = run(name, ["nice", "-n", "10", str(toolchain / "cargo"), "test", "-p", "hex-gpu", "--test", "apple_compute", "--no-run", "--locked", "--message-format=json"], build_env, folder, native_guard_seconds, 0, [])
    artifacts = []
    for line in raw.splitlines():
        try:
            item = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        if item.get("reason") == "compiler-artifact" and item.get("target", {}).get("name") == "apple_compute" and item.get("executable"):
            artifacts.append(item["executable"])
    assert len(artifacts) == 1
    binary = Path(artifacts[0])
    result[name + "_binary"] = {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    return binary


try:
    binary = build("compile-original", source, env)
    arguments = ["--ignored", "--exact", "root_selected_metal_device_executes_checked_kernel", "--nocapture", "--test-threads=1"]
    run("selected-compute", ["nice", "-n", "10", str(binary), *arguments], env, source, native_guard_seconds, 0, [b"1 passed; 0 failed; 0 ignored;", b"status=4 error_code=None", b"mismatches=0", b"live=0"])
    wrong = env.copy()
    wrong["HEX_METAL_REGISTRY_ID"] = str(int(registry_id) ^ (1 << 63))
    result["wrong_registry_id"] = int(wrong["HEX_METAL_REGISTRY_ID"])
    raw = run("wrong-identity", ["nice", "-n", "10", str(binary), *arguments], wrong, source, native_guard_seconds, 101, [b"0 passed; 1 failed; 0 ignored;", b"missing/duplicated in Gpus inventory", b"live=0"])
    assert b"metal_command" not in raw and b"metal_dispatch" not in raw
    mutation = root / "mutation-source"
    shutil.copytree(source, mutation)
    leaf = mutation / "crates/hex-gpu/tests/apple_compute.rs"
    before = leaf.read_text()
    needle = "    output[i] = value ^ 0xffffffffu;"
    assert before.count(needle) == 1
    leaf.write_text(before.replace(needle, "    output[i] = output[i]; // ROOT mutation: leave the initialized wrong result unchanged."))
    changed = {key for key, value in hashes(mutation).items() if manifest.get(key) != value}
    assert changed == {"crates/hex-gpu/tests/apple_compute.rs"}
    result["mutation"] = {"changed": sorted(changed), "before_sha256": hashlib.sha256(before.encode()).hexdigest(), "after_sha256": hashlib.sha256(leaf.read_bytes()).hexdigest()}
    mutation_env = env.copy()
    mutation_env["CARGO_TARGET_DIR"] = str(root / "mutation-target")
    mutated_binary = build("compile-mutation", mutation, mutation_env)
    run("missing-gpu-write", ["nice", "-n", "10", str(mutated_binary), *arguments], mutation_env, mutation, native_guard_seconds, 101, [b"0 passed; 1 failed; 0 ignored;", b"status=4 error_code=None", b"GPU output disagrees", b"live=0"])
    result["exit"] = 0
except Exception as error:
    result["exit"] = 1
    result["error"] = str(error)
finally:
    result["source_unchanged"] = hashes(source) == manifest
    result["temp_entries"] = len(list((root / "temp").iterdir()))
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2), flush=True)
sys.exit(result["exit"])
