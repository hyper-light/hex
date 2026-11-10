#!/usr/bin/env python3
"""Run only this lane's registered libtest module in a new private runner copy.

Called on the approved runner with a gzip Git/source archive on stdin. The root,
archive hash, source hash and expected selected count are explicit arguments.
"""

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


root = Path(sys.argv[1]).expanduser()
archive_sha, fixture_sha = sys.argv[2:4]
expected_count = int(sys.argv[4])
selector = sys.argv[5]
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
            raise ValueError("source archive contains a nonregular or escaping member")
    bundle.extractall(source)

fixture = source / "crates/hex-node/src/placement/adversarial_bounds_tests.rs"
if digest(fixture.read_bytes()) != fixture_sha:
    raise ValueError("fixture source hash mismatch")
parent = source / "crates/hex-node/src/placement/tests.rs"
registration = '\n#[path = "adversarial_bounds_tests.rs"]\nmod adversarial_bounds_tests;\n'
parent_before = digest(parent.read_bytes())
parent.write_bytes(parent.read_bytes() + registration.encode())
identities = {
    str(path.relative_to(source)): digest(path.read_bytes())
    for path in sorted(source.rglob("*"))
    if path.is_file()
}
(root / "source-hashes.json").write_text(json.dumps(identities, indent=2) + "\n")
for directory in [root / "target", root / "cache/cargo", root / "temp"]:
    directory.mkdir(parents=True)
toolchain = Path.home() / ".cache/hex-lead/rustup-scheduler-copy/toolchains/1.99.1-aarch64-apple-darwin/bin"
env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
env.update({
    "PATH": str(toolchain) + os.pathsep + env["PATH"],
    "RUSTUP_HOME": str(toolchain.parents[1]),
    "RUSTC": str(toolchain / "rustc"),
    "RUSTDOC": str(toolchain / "rustdoc"),
    "CARGO_HOME": str(root / "cache/cargo"),
    "CARGO_TARGET_DIR": str(root / "target"),
    "CARGO_BUILD_JOBS": "1",
    "RUST_TEST_THREADS": "1",
    "TMPDIR": str(root / "temp"),
})
command = [
    "nice", "-n", "10", str(toolchain / "cargo"),
    "test", "-p", "hex-node", "--lib", "--locked",
    selector, "--", "--exact", "--nocapture", "--test-threads=1",
]
manifest = {
    "platform": platform.platform(), "arch": platform.machine(),
    "archive_sha256": archive_sha, "fixture_sha256": fixture_sha,
    "source_hashes_sha256": digest((root / "source-hashes.json").read_bytes()),
    "temporary_registration": {"path": str(parent.relative_to(source)),
        "before_sha256": parent_before, "after_sha256": digest(parent.read_bytes()),
        "appended": registration},
    "command": command, "expected_selected_count": expected_count,
    "root": str(root), "owned_paths": [str(source), str(root / "target"),
        str(root / "cache"), str(root / "temp")],
    "scope": "native macOS libtest execution, private filesystem and SimRuntime only; no native runtime or VM effects",
}
for name in ["rustc", "cargo"]:
    manifest[name] = subprocess.check_output([str(toolchain / name), "--version"], env=env, text=True).strip()
with (root / "test.log").open("xb") as log:
    child = subprocess.Popen(command, cwd=source, env=env, stdout=log, stderr=subprocess.STDOUT)
    manifest["owned_cargo_pid"] = child.pid
    (root / "run.json").write_text(json.dumps(manifest, indent=2) + "\n")
    manifest["exit_code"] = child.wait()
raw = (root / "test.log").read_bytes()
text = raw.decode("utf-8", errors="replace")
selected = [line for line in text.splitlines() if line.startswith("test placement::tests::adversarial_bounds_tests::")]
summaries = [line for line in text.splitlines() if line.startswith("test result:")]
manifest.update({
    "log_sha256": digest(raw), "log_bytes": len(raw),
    "selected_lines": selected, "summaries": summaries,
    "selected_count_matches": len(selected) == expected_count,
    "remaining_temp_entries": len(list((root / "temp").iterdir())),
    "cleanup_witnesses": [line for line in text.splitlines() if "expected=" in line],
})
(root / "result.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(json.dumps(manifest, indent=2), flush=True)
if not manifest["selected_count_matches"]:
    print(text[-12000:], flush=True)
    sys.exit(1)
sys.exit(manifest["exit_code"])
