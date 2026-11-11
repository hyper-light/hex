"""ROOT-only admitted Darwin mechanism control, no VM or product release claim."""
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import subprocess
import sys
import tarfile

root = Path(sys.argv[1]).expanduser()
expected, revision = sys.argv[2:4]
root.mkdir(parents=True, exist_ok=False)
raw = sys.stdin.buffer.read()
if hashlib.sha256(raw).hexdigest() != expected:
    raise ValueError("source archive SHA mismatch")
(root / "source.tar.gz").write_bytes(raw)
source = root / "source"
source.mkdir()
with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as bundle:
    for item in bundle.getmembers():
        path = PurePosixPath(item.name)
        if not item.isfile() or path.is_absolute() or ".." in path.parts:
            raise ValueError("nonregular source member")
    bundle.extractall(source, filter="data")
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
before = {str(path.relative_to(source)): sha(path) for path in source.rglob("*") if path.is_file()}
(root / "source-before.json").write_text(json.dumps(before, indent=2) + "\n")
for path in source.rglob("*"):
    if path.is_file():
        path.chmod(0o444)
(root / "temp").mkdir()
env = {key: value for key, value in os.environ.items() if not key.startswith(("DYLD_", "HEX_DEBUG_", "SHARDS_DEBUG_"))}
env["TMPDIR"] = str(root / "temp")
clang = subprocess.check_output(["/usr/bin/xcrun", "--find", "clang"], env=env, text=True).strip()
sdk = subprocess.check_output(["/usr/bin/xcrun", "--show-sdk-path"], env=env, text=True).strip()
result = {
    "source": revision, "archive_sha256": expected, "platform": platform.platform(),
    "driver_pid": os.getpid(), "clang": clang, "clang_sha256": sha(Path(clang)), "sdk": sdk,
    "compiler_version": subprocess.check_output([clang, "--version"], env=env, text=True),
    "sdk_version": subprocess.check_output(["/usr/bin/xcrun", "--show-sdk-version"], env=env, text=True).strip(),
    "memory_mib": 64,
    "memory_input_basis": "explicit disposable input: existing ARM guest minimum64MiB, within admitted4GiB; no production allowance",
    "phases": [], "VM_GPU_invocations": 0,
}
leaf = source / "macos_setexec_preseal_control.c"
binary = root / "setexec-control"
phases = [
    ("compile", ["nice", "-n", "10", clang, "-std=c17", "-O0", "-Wall", "-Wextra", "-Werror", str(leaf), "-o", str(binary)], 90),
    ("native-four-cuts", ["nice", "-n", "10", str(binary), "--memory-mib", "64"], 60),
]
for name, command, guard in phases:
    phase = {"name": name, "command": command, "external_guard_seconds": guard}
    result["phases"].append(phase)
    with (root / (name + ".log")).open("xb") as log:
        child = subprocess.Popen(command, env=env, cwd=root, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        phase["pid"] = child.pid
        (root / "running.json").write_text(json.dumps(result, indent=2) + "\n")
        try:
            phase["exit"] = child.wait(timeout=guard)
        except subprocess.TimeoutExpired:
            phase["timed_out"] = True
            child.terminate()
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
            phase["exit"] = child.returncode
            phase["nested_actor_cleanup"] = "UNKNOWN; do not signal unverified descendant identities"
        phase["reaped"] = child.returncode is not None
    data = (root / (name + ".log")).read_bytes()
    phase.update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
    if name == "compile" and phase["exit"] == 0:
        result["binary_sha256"] = sha(binary)
    if name == "native-four-cuts":
        phase["actual_case_results"] = [
            {"case": int(case), "error": int(error), "relevant": int(relevant), "main_cleanup": cleanup}
            for case, error, relevant, cleanup in re.findall(r"RESULT case=(\d+) error=(\d+) relevant=(\d+) main_cleanup=(REAPED|UNKNOWN)", data.decode(errors="replace"))
        ]
        phase["expected_fixed_cases"] = 4
        phase["selection_verified"] = (
            [item["case"] for item in phase["actual_case_results"]] == [0, 1, 2, 3]
            and all(item["error"] == 0 and item["relevant"] == 1 and item["main_cleanup"] == "REAPED" for item in phase["actual_case_results"])
        )
    if phase["exit"] != 0:
        break
after = {str(path.relative_to(source)): sha(path) for path in source.rglob("*") if path.is_file()}
(root / "source-after.json").write_text(json.dumps(after, indent=2) + "\n")
result["source_changes"] = [name for name in set(before) | set(after) if before.get(name) != after.get(name)]
result["temp_entries"] = len(list((root / "temp").iterdir()))
result["exit"] = 0 if len(result["phases"]) == 2 and all(item["exit"] == 0 and item.get("selection_verified", True) for item in result["phases"]) and before == after else 1
(root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result, indent=2), flush=True)
for phase in result["phases"]:
    print((root / (phase["name"] + ".log")).read_text(errors="replace"), flush=True)
sys.exit(result["exit"])
