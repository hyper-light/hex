#!/usr/bin/env python3
"""Verify this lane's frozen compiler receipts; never invoke Cargo or native code."""
import hashlib
import io
import json
import pathlib
import subprocess
import sys
import tarfile


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_json(path):
    return json.loads(path.read_text())


def main():
    root = pathlib.Path(sys.argv[1]).resolve()
    worktree = pathlib.Path(sys.argv[2]).resolve()
    bundle = root / "proof-bundle.tar.gz"
    meta = read_json(root / "remote-bundle.json")
    assert bundle.stat().st_size == meta["bytes"]
    assert digest(bundle.read_bytes()) == meta["sha256"]
    raw = root / "raw"
    members = read_json(raw / "bundle-members.json")
    observed = {
        str(path.relative_to(raw)) for path in raw.rglob("*") if path.is_file()
    }
    assert observed == set(members) | {"bundle-members.json"}
    for name, item in members.items():
        data = (raw / name).read_bytes()
        assert len(data) == item["bytes"] and digest(data) == item["sha256"]

    chunks = [read_json(path) for path in sorted((root / "chunks").glob("*.json"))]
    position = 0
    with bundle.open("rb") as stream:
        for chunk in chunks:
            assert chunk["offset"] == position
            data = stream.read(chunk["bytes"])
            assert len(data) == chunk["bytes"] and digest(data) == chunk["sha256"]
            position += len(data)
        assert not stream.read(1) and position == meta["bytes"]

    expected = {
        "r96": ("f07bc72bb152106322c844ef299e29e56eebba54", 101, 1794, 53536),
        "r97": ("888764365adf0d54788a51a7699aed396d962e73", 0, 1795, 55078),
    }
    results = {}
    for label, (revision, code, files, pid) in expected.items():
        run = raw / label
        result = read_json(run / "result.json")
        transfer = read_json(run / "transfer.json")
        before = read_json(run / "source-before.json")
        after = read_json(run / "source-after.json")
        assert result["source_commit"] == transfer["source_commit"] == revision
        assert before == after == {
            name: item["sha256"] for name, item in transfer["files"].items()
        }
        archive = subprocess.check_output(
            ["git", "archive", "--format=tar", revision], cwd=worktree
        )
        git_files = {}
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tree:
            for item in tree.getmembers():
                if item.isfile():
                    data = tree.extractfile(item).read()
                    git_files[item.name] = {"bytes": len(data), "sha256": digest(data)}
                else:
                    assert item.isdir(), "unexpected non-file Git archive entry"
        assert git_files == transfer["files"]
        assert len(before) == result["source_files"] == files
        assert result["source_equal"]
        assert result["tests_executed"] == 0 and not result["native_gpu_executed"]
        assert result["binaries"] == {} and result["temp_entries"] == []
        assert len(result["checks"]) == 1
        check = result["checks"][0]
        assert check["exit"] == code and check["pid"] == pid and check["reaped"]
        assert check["test_results"] == []
        log = (run / "windows-test-clippy.log").read_bytes()
        assert len(log) == check["log_bytes"] and digest(log) == check["log_sha256"]
        owner = read_json(run / "windows-test-clippy-owner.json")
        assert owner["pid"] == pid and owner["argv"] == check["argv"]
        environment = read_json(run / "environment.json")
        assert environment["jobs"] == environment["test_threads"] == 1
        assert environment["machine"] == "arm64"
        if code:
            assert b"E0277" in log and b"HcsFailure" in log and b"Display" in log
        else:
            assert b"Finished" in log
        results[label] = {
            "source": revision,
            "source_files_equal_to_signed_git": files,
            "archive_bytes": transfer["archive_bytes"],
            "archive_sha256": transfer["archive_sha256"],
            "compiler": check,
            "temp_entries": 0,
            "tests_executed": 0,
            "native_executed": False,
        }
    report = {
        "status": "VERIFIED_OWN_COMPILER_RECEIPTS_ONLY",
        "bundle": meta,
        "members_verified": len(members),
        "chunk_count": len(chunks),
        "chunks": chunks,
        "runs": results,
        "active_check": None,
        "collector_failures_preserved": [
            {
                "stage": "initial chunk-reader",
                "error": "SyntaxError: Unexpected end of JSON input",
                "effect": "No chunk appended; offset remained zero",
                "correction": "Accumulate initial and subsequent Kiln output fragments",
            },
            {
                "stage": "post-extraction diagnostic",
                "error": "NameError: name 'PY' is not defined",
                "effect": "Bundle hash and safe extraction completed before stray terminator",
                "correction": "Use a named file for final receipt verification",
            },
            {
                "stage": "schema-only diagnostic",
                "error": "NameError: name 'PY' is not defined",
                "effect": "Schema printed before stray terminator; no source/build effect",
                "correction": "Use a named file for final receipt verification",
            },
        ],
    }
    destination = root / "collection.json"
    with destination.open("x") as output:
        json.dump(report, output, sort_keys=True, indent=2)
        output.write("\n")
    print(json.dumps({
        "receipt": str(destination),
        "members_verified": len(members),
        "source_files": {label: item["source_files_equal_to_signed_git"] for label, item in results.items()},
        "compiler_exits": {label: item["compiler"]["exit"] for label, item in results.items()},
        "tests_executed": 0,
        "native_executed": False,
    }))


if __name__ == "__main__":
    main()
