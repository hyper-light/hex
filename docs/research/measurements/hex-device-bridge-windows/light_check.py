#!/usr/bin/env python3
"""Private approved-runner HCS unit/core-Clippy check; source archive arrives on stdin."""

import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import subprocess
import sys
import tarfile


def digest(data):
    return hashlib.sha256(data).hexdigest()


root = Path(sys.argv[1])
archive_sha, source_revision = sys.argv[2:4]
root.mkdir(parents=True, exist_ok=False)
source = root / "source"
source.mkdir()
archive = sys.stdin.buffer.read()
if digest(archive) != archive_sha:
    raise ValueError("source archive hash mismatch")
(root / "source.tar.gz").write_bytes(archive)
with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
    for member in bundle.getmembers():
        path = PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts or not (member.isfile() or member.isdir()):
            raise ValueError("archive contains a nonregular or escaping source member")
    bundle.extractall(source)
identities = {str(p.relative_to(source)): digest(p.read_bytes())
              for p in sorted(source.rglob("*")) if p.is_file()}
(root / "source-hashes.json").write_text(json.dumps(identities, indent=2) + "\n")
for directory in [root / "target", root / "cache/cargo", root / "temp"]:
    directory.mkdir(parents=True)
rustup = Path.home() / ".cache/hex-lead/rustup-scheduler-copy"
toolchain = rustup / "toolchains/1.99.1-aarch64-apple-darwin/bin"
env = {k: v for k, v in os.environ.items()
       if not k.startswith(("DYLD_", "HEX_DEBUG_", "SHARDS_DEBUG_"))}
owned_env = {
    "PATH": str(toolchain) + os.pathsep + env["PATH"],
    "RUSTUP_HOME": str(rustup), "RUSTC": str(toolchain / "rustc"),
    "RUSTDOC": str(toolchain / "rustdoc"), "CARGO_HOME": str(root / "cache/cargo"),
    "CARGO_TARGET_DIR": str(root / "target"), "CARGO_BUILD_JOBS": "1",
    "RUST_TEST_THREADS": "1", "TMPDIR": str(root / "temp"),
}
env.update(owned_env)
result = {
    "source_revision": source_revision, "archive_sha256": archive_sha,
    "source_manifest_sha256": digest((root / "source-hashes.json").read_bytes()),
    "platform": platform.platform(), "arch": platform.machine(),
    "root": str(root), "needs": "cpus=1,mem=4g,disk=12g", "phases": [],
    "hcs_sha256": identities["crates/hex-core/src/hcs.rs"],
    "tests_sha256": identities["crates/hex-core/src/hcs/device_instance_tests.rs"],
    "scope": "pure HCS data units and native core Clippy only; no Windows/HCS/hardware/VM effects",
}
for name in ["rustc", "cargo"]:
    result[name] = subprocess.check_output([str(toolchain / name), "--version"], env=env, text=True).strip()


def run_phase(name, args):
    command = ["nice", "-n", "10", str(toolchain / "cargo")] + args
    phase = {"name": name, "command": command}
    with (root / (name + ".log")).open("xb") as log:
        child = subprocess.Popen(command, cwd=source, env=env, stdout=log, stderr=subprocess.STDOUT)
        phase["owned_pid"] = child.pid
        result["phases"].append(phase)
        (root / "run.json").write_text(json.dumps(result, indent=2) + "\n")
        phase["exit_code"] = child.wait()
    raw = (root / (name + ".log")).read_bytes()
    phase.update({"log_sha256": digest(raw), "log_bytes": len(raw)})
    return raw.decode("utf-8", errors="replace"), phase


unit_text, unit = run_phase("hcs-unit", [
    "test", "-p", "hex-core", "--lib", "--locked", "hcs::", "--", "--nocapture", "--test-threads=1",
])
unit["selected_lines"] = [line for line in unit_text.splitlines() if line.startswith("test hcs::")]
unit["new_selected_lines"] = [line for line in unit["selected_lines"]
                              if line.startswith("test hcs::device_instance_tests::")]
unit["summaries"] = [line for line in unit_text.splitlines() if line.startswith("test result:")]
unit["nonzero_selection"] = len(unit["selected_lines"]) > 0
unit["all_nine_new_tests_selected"] = len(unit["new_selected_lines"]) == 9
if unit["exit_code"] == 0 and unit["nonzero_selection"] and unit["all_nine_new_tests_selected"]:
    run_phase("core-clippy", ["clippy", "-p", "hex-core", "--all-targets", "--locked", "--", "-D", "warnings"])
else:
    result["clippy_not_run"] = "unit compilation/execution/selection did not pass"
result["remaining_temp_entries"] = len(list((root / "temp").iterdir()))
result["exit_code"] = 0 if (
    unit["nonzero_selection"] and unit["all_nine_new_tests_selected"]
    and len(result["phases"]) == 2 and all(p["exit_code"] == 0 for p in result["phases"])
) else 1
(root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result, indent=2), flush=True)
if not unit["nonzero_selection"]:
    print(unit_text[-12000:], flush=True)
sys.exit(result["exit_code"])
