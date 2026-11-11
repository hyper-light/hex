#!/usr/bin/env python3
"""Writes the node's GPU observer manifest (HEX_GUEST_GPU_OBSERVER_FILE, AC-T7.4.1) for a closure
scripts/guest/vulkan-ready.sh built, as crates/hex-runtime/src/guest/observer/config/load.rs reads it:

  vulkan-ready-manifest.py OUT --guest-root DIR --cpu-max-milli N --memory-max BYTES --pids-max N
      --max-queue-families N --max-allocation-bytes N --stderr-bytes N

OUT is vulkan-ready.sh's output; DIR is where the guest sees OUT/closure (the node's HEX_GUEST_SHARES
mounts its i-th root, read-only, at /run/hex/shares/i). Every bound is the operator's, none defaulted:
the observer Process's CPU, memory and process caps (paid from the GPU pool VM's room, which the node
checks), the queue families and allocation the Program accepts before it allocates, and the stderr it
keeps. Each artifact's role follows the closure's layout; its size and SHA-256 are the closure's own
(closure.sha256), whose SHA-256 is the manifest's evidence. Writes OUT/observer.json and prints the two
variables the node reads.
"""

import argparse
import hashlib
import json
import pathlib
import sys

LOADER = "lib/libvulkan.so.1"
DRIVER = "lib/libvulkan_virtio.so"
ICD = "icd/virtio_icd.json"
PROGRAM = "bin/hex-vulkan-guest-ready"


def role(relative: str) -> str:
    if relative == PROGRAM:
        return "program"
    if relative == LOADER:
        return "loader"
    if relative == DRIVER:
        return "driver"
    if relative == ICD:
        return "icd"
    if relative.startswith("lib/ld-linux"):
        return "interpreter"
    return "dependency"


def positive(text: str) -> int:
    value = int(text, 10)
    if value <= 0:
        raise argparse.ArgumentTypeError(f"{text} is not positive")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("out", type=pathlib.Path)
    parser.add_argument("--guest-root", required=True)
    for name in ("cpu-max-milli", "memory-max", "pids-max", "max-queue-families", "max-allocation-bytes", "stderr-bytes"):
        parser.add_argument(f"--{name}", required=True, type=positive)
    args = parser.parse_args()
    out = args.out.resolve()
    root = args.guest_root.rstrip("/")
    if not root.startswith("/"):
        parser.error("--guest-root must be an absolute guest path")
    sums = (out / "closure.sha256").read_bytes()
    artifacts = []
    interpreter = None
    for line in sums.decode().splitlines():
        digest, relative = line.split(maxsplit=1)
        host = out / "closure" / relative
        data = host.read_bytes()
        if hashlib.sha256(data).hexdigest() != digest:
            print(f"vulkan-ready-manifest.py: {host} is not the file closure.sha256 names", file=sys.stderr)
            return 1
        kind = role(relative)
        if kind == "interpreter":
            interpreter = f"{root}/{relative}"
        artifacts.append({"role": kind, "host": str(host), "guest": f"{root}/{relative}", "bytes": len(data), "sha256": digest})
    if interpreter is None:
        print("vulkan-ready-manifest.py: the closure has no dynamic linker", file=sys.stderr)
        return 1
    manifest = {
        "evidence": hashlib.sha256(sums).hexdigest(),
        "artifacts": artifacts,
        "launch": [interpreter, "--library-path", f"{root}/lib", f"{root}/{PROGRAM}"],
        "max_queue_families": args.max_queue_families,
        "max_allocation_bytes": args.max_allocation_bytes,
        "stderr_bytes": args.stderr_bytes,
        "bounds": {
            "cpu_weight": None,
            "cpu_max_milli": args.cpu_max_milli,
            "memory_max": args.memory_max,
            "memory_high": None,
            "io_weight": None,
            "io_latency_us": None,
            "io_max": None,
            "pids_max": args.pids_max,
        },
    }
    text = json.dumps(manifest, indent=2) + "\n"
    path = out / "observer.json"
    path.write_text(text)
    print(f"HEX_GUEST_GPU_OBSERVER_FILE={path}")
    print(f"HEX_GUEST_GPU_OBSERVER_SHA256={hashlib.sha256(text.encode()).hexdigest()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
