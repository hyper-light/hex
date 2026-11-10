#!/usr/bin/env python3
"""Private hex-gpu pure units + native compilation only; never execute the hardware test."""
import hashlib, json, os, pathlib, platform, re, subprocess, sys, time

base = pathlib.Path.home() / ".cache/hex-lead/lanes/macos-gpu-01a1c640-0e625a45-light-v1"
source = base / "source"
proof = base / "proof"
assert (proof / "transfer.json").is_file()
assert not (proof / "result.json").exists()
assert platform.system() == "Darwin" and platform.machine() == "arm64"
env = dict(os.environ)
for key in list(env):
    if key.startswith(("DYLD_", "HEX_DEBUG_", "HEX_SKIPS")) or key == "HEX_WAITS_CENSUS":
        env.pop(key)
env.update(CARGO_HOME=str(base / "cargo-home"), CARGO_TARGET_DIR=str(base / "target"),
           TMPDIR=str(base / "tmp"), CARGO_BUILD_JOBS="1", RUST_TEST_THREADS="1",
           RUSTUP_HOME=str(pathlib.Path.home() / ".cache/hex-lead/rustup-scheduler-copy"))
env["PATH"] = str(pathlib.Path.home() / ".cargo/bin") + os.pathsep + env.get("PATH", "")

def manifest():
    return {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(source.rglob("*")) if p.is_file()}

before = manifest()
(proof / "source-before.json").write_text(json.dumps(before, sort_keys=True, indent=2) + "\n")
environment = {"platform": platform.platform(), "machine": platform.machine(), "source": str(source),
               "target": env["CARGO_TARGET_DIR"], "cargo_home": env["CARGO_HOME"],
               "tmpdir": env["TMPDIR"], "jobs": 1, "test_threads": 1,
               "rustc": subprocess.check_output(["rustc", "-Vv"], env=env, text=True),
               "cargo": subprocess.check_output(["cargo", "-V"], env=env, text=True),
               "disk": subprocess.check_output(["df", "-Pk", str(base)], text=True)}
(proof / "environment.json").write_text(json.dumps(environment, sort_keys=True, indent=2) + "\n")
commands = [
    ("pure-lib", ["cargo", "test", "-p", "hex-gpu", "--lib", "--locked", "--", "--test-threads=1"]),
    ("native-clippy", ["cargo", "clippy", "-p", "hex-gpu", "--all-targets", "--locked", "--", "-D", "warnings"]),
]
results = []
for label, argv in commands:
    log = proof / (label + ".log")
    start = time.time()
    with log.open("wb") as output:
        process = subprocess.Popen(argv, cwd=source, env=env, stdout=output, stderr=subprocess.STDOUT)
        identity = subprocess.check_output(["ps", "-p", str(process.pid), "-o", "pid=,ppid=,lstart=,nice=,command="], text=True)
        (proof / (label + "-owner.json")).write_text(json.dumps(
            {"pid": process.pid, "ps": identity, "argv": argv, "started": start}, indent=2) + "\n")
        print(json.dumps({"started": label, "pid": process.pid}), flush=True)
        exit_code = process.wait()
    raw = log.read_bytes()
    result = {"label": label, "argv": argv, "pid": process.pid, "exit": exit_code, "reaped": process.poll() is not None,
              "elapsed_s": time.time() - start, "log_bytes": len(raw), "log_sha256": hashlib.sha256(raw).hexdigest()}
    text = raw.decode("utf-8", errors="replace")
    result["test_results"] = re.findall(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*", text)
    results.append(result)
    print(json.dumps(result), flush=True)
    if exit_code != 0:
        print(text[-10000:], flush=True)
        break

after = manifest()
(proof / "source-after.json").write_text(json.dumps(after, sort_keys=True, indent=2) + "\n")
assert before == after
binaries = {}
for path in (base / "target/debug/deps").glob("hex_gpu-*"):
    if path.is_file() and path.suffix == "" and os.access(path, os.X_OK):
        binaries[str(path.relative_to(base))] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "bytes": path.stat().st_size}
outcome = {"source_commit": "0e625a452827f4e09ca77eec7e16fb491582b45b", "source_files": len(before),
           "source_equal": before == after, "checks": results, "binaries": binaries,
           "temp_entries": [str(p.relative_to(base / "tmp")) for p in sorted((base / "tmp").rglob("*"))],
           "native_gpu_executed": False}
(proof / "result.json").write_text(json.dumps(outcome, sort_keys=True, indent=2) + "\n")
print(json.dumps({"result": str(proof / "result.json"), "source_equal": True, "checks": results,
                  "temp_entries": len(outcome["temp_entries"]), "native_gpu_executed": False}), flush=True)
sys.exit(0 if len(results) == len(commands) and all(r["exit"] == 0 for r in results) else 1)
