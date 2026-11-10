#!/usr/bin/env python3
"""Root-only fresh-guest Hello sampling, followed by unchanged physical boundary controls."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys


def load(path):
    spec = importlib.util.spec_from_file_location("owned_capacity_fixture", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("the explicit capacity fixture could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


extension_parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
extension_parser.add_argument("--capacity-fixture", required=True)
extension_parser.add_argument("--hello-helper", required=True)
extension_parser.add_argument("--helper-revision", required=True)
extension_parser.add_argument("--hellos", type=int, required=True)
extension_parser.add_argument("--hello-dial-nanos", type=int, required=True)
extension, rest = extension_parser.parse_known_args()
boundary = load(Path(extension.capacity_fixture).resolve())
census = boundary.census
census.require(extension.hellos > 0 and extension.hello_dial_nanos > 0, "explicit positive Hello count/dial budget required")


class HelloBoundary(boundary.Boundary):
    def backend(self, name):
        socket_path = super().backend(name)
        census.require(self.mode == "guest" and self.guest_receipt is not None, "Hello fixture needs its own fresh guest")
        receipt = json.loads(self.guest_receipt.read_text())
        before_record = Path(receipt["record"]).read_bytes()
        before = self.snapshot()
        expected_id = f"guest:{receipt['pid']}:{receipt['started']}"
        census.require(before["capacity"]["id"] == expected_id, "pre-borrow live capacity does not name this owned guest")
        path = self.root / "census/owned-guest-hello.tsv"
        environment = self.environment.copy()
        environment["HEX_WAITS_CENSUS"] = str(path)
        environment["HEX_WAITS_CENSUS_MAX"] = str(self.runtime["max_census_lines"])
        argv = [extension.hello_helper, "--owned-root", str(self.root), "--record", receipt["record"],
                "--receipt", str(self.guest_receipt), "--vsock", receipt["vsock"],
                "--iterations", str(extension.hellos), "--dial-nanos", str(extension.hello_dial_nanos),
                "--expected-class", "oci", "--source-revision", extension.helper_revision,
                "--profile", self.runtime["profile"]]
        if receipt.get("owned_vsock_root") is not None:
            argv += ["--owned-vsock-root", receipt["owned_vsock_root"]]
        process = self.spawn("guest-hello", argv, stdout=subprocess.PIPE, env=environment)
        process.hex_census_path = str(path)
        census.record(self.runtime, "guest-hello", process.pid, census=str(path),
                      profile=self.runtime["profile"], source_revision=extension.helper_revision,
                      binary=extension.hello_helper)
        try:
            try:
                output, _ = process.communicate(timeout=self.runtime["wait"])
            except subprocess.TimeoutExpired as why:
                process.kill()
                process.communicate(timeout=self.runtime["wait"])
                raise census.Failed("the tracked Hello helper exceeded the existing outer cancellation budget") from why
        finally:
            if process.poll() is None:
                self.stop(process, crash=True)
            census.record(self.runtime, "census-process-exit", process.pid, census=str(path),
                          returncode=process.returncode, success=process.returncode == 0)
        census.require(process.returncode == 0 and len(output) <= self.runtime["max_frame"], "Hello helper failed or exceeded summary bound")
        summary = json.loads(output)
        census.require(summary.get("status") == "completed" and summary.get("guest_lifecycle_owned") is False,
                       "Hello helper did not complete in borrowed-only mode")
        for key in ("iterations", "hello_wire_successes", "class_validated_hellos"):
            census.require(summary.get(key) == extension.hellos, "Hello count/class proof differs from explicit count")
        census.require(summary.get("identity") == {key: receipt[key] for key in ("pid", "started", "vsock")},
                       "Hello summary names another VM")
        after = self.snapshot()
        census.require(Path(receipt["record"]).read_bytes() == before_record
                       and after["capacity"]["id"] == expected_id and after["boxes"] == before["boxes"],
                       "borrower drops changed guest identity or physical baseline")
        summary["root_post_drop_same_identity_live_capacity"] = True
        summary["root_physical_baseline_unchanged"] = True
        summary["resource_counts"] = "FD/task/allocator return is not proved by this identity/physical control"
        self.report["guest_hello"] = summary
        return socket_path


if __name__ == "__main__":
    original = census.Harness
    try:
        sys.argv = [sys.argv[0], *rest]
        census.Harness = HelloBoundary
        census.main()
    finally:
        census.Harness = original
