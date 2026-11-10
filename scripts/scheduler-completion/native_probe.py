#!/usr/bin/env python3
"""ROOT-only read-only OS witness for an exact runtime-owned process; never discovers a PID."""

import argparse
import ctypes
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

from evidence import InvalidEvidence, hex_digest, load_json, require, sha256


def linux_process(pid):
    proc = Path("/proc") / str(pid)
    stat = (proc / "stat").read_text()
    # comm can contain spaces/parentheses; the last ')' ends it, and starttime is field 22.
    fields = stat[stat.rfind(")") + 2:].split()
    require(len(fields) >= 20, "short proc stat")
    return {"pid": pid, "start_token": fields[19], "executable": str((proc / "exe").resolve(strict=True)),
            "exe_handle": proc / "exe", "parent": int(fields[1]),
            "namespaces": {name: os.readlink(proc / "ns" / name) for name in ("mnt", "pid")},
            "self_namespaces": {name: os.readlink(Path("/proc/self/ns") / name) for name in ("mnt", "pid")},
            "fd_targets": [os.readlink(entry) for entry in (proc / "fd").iterdir()]}


def mac_process(pid, timeout, byte_limit):
    # libproc supplies the current exact image path. ps supplies the OS start token;
    # neither command enumerates or signals another process.
    libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    function = libproc.proc_pidpath
    function.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
    function.restype = ctypes.c_int
    buffer = ctypes.create_string_buffer(byte_limit)
    require(function(pid, buffer, byte_limit) > 0, "proc_pidpath failed")
    start = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "lstart="],
                           check=True, capture_output=True, timeout=timeout).stdout
    require(len(start) <= byte_limit and bool(start.strip()), "missing/oversized process start token")
    path = Path(os.fsdecode(buffer.value)).resolve(strict=True)
    return {"pid": pid, "start_token": start.decode().strip(), "executable": str(path), "exe_handle": path}


def windows_process(pid, _timeout, byte_limit):
    from ctypes import wintypes
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    kernel.GetProcessTimes.restype = wintypes.BOOL
    kernel.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD,
                                                 wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
    kernel.QueryFullProcessImageNameW.restype = wintypes.BOOL
    # PROCESS_QUERY_LIMITED_INFORMATION only: no terminate, duplicate or inheritance authority.
    handle = kernel.OpenProcess(0x1000, False, pid)
    require(bool(handle), "OpenProcess query refused")
    try:
        creation, end, system, user = (wintypes.FILETIME() for _ in range(4))
        require(bool(kernel.GetProcessTimes(handle, ctypes.byref(creation), ctypes.byref(end),
                                             ctypes.byref(system), ctypes.byref(user))), "GetProcessTimes refused")
        buffer = ctypes.create_unicode_buffer(byte_limit)
        extent = wintypes.DWORD(byte_limit)
        require(bool(kernel.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(extent))),
                "QueryFullProcessImageNameW refused")
        path = Path(buffer.value).resolve(strict=True)
        return {"pid": pid, "start_token": str((creation.dwHighDateTime << 32) | creation.dwLowDateTime),
                "executable": str(path), "exe_handle": path}
    finally:
        require(bool(kernel.CloseHandle(handle)), "observation handle close failed")


def observe(pid, start, backend, expected_executable, expected_hash, timeout, byte_limit):
    require(type(pid) is int and pid > 0, "positive exact runtime-owned PID required")
    require(hex_digest(expected_hash), "native executable hash required")
    if sys.platform.startswith("linux"):
        row = linux_process(pid)
    elif sys.platform == "darwin":
        row = mac_process(pid, timeout, byte_limit)
    elif os.name == "nt":
        row = windows_process(pid, timeout, byte_limit)
    else:
        raise InvalidEvidence("unverified process observer platform")
    require(row["start_token"] == str(start), "PID was reused or its start token changed")
    require(Path(row["executable"]) == Path(expected_executable).resolve(strict=True), "wrong native executable")
    require(sha256(row.pop("exe_handle")) == expected_hash, "native executable hash mismatch")
    if sys.platform.startswith("linux"):
        if backend == "oci":
            require(all(row["namespaces"][name] != row["self_namespaces"][name]
                        for name in ("mnt", "pid")), "OCI process lacks container namespaces")
        else:
            require(any("kvm-vm" in target for target in row["fd_targets"]), "VMM holds no actual KVM VM")
    if backend == "firecracker":
        require(sys.platform.startswith("linux"), "Firecracker must execute on an actual Linux KVM peer")
    row.update(backend=backend, native_process_verified=True,
               native_executable_sha256=expected_hash, observer_platform=platform.platform())
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized", action="store_true")
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--start-token", required=True)
    parser.add_argument("--backend", choices=("oci", "firecracker", "shards"), required=True)
    parser.add_argument("--executable", required=True)
    parser.add_argument("--executable-sha256", required=True)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--byte-limit", type=int, required=True)
    args = parser.parse_args()
    require(args.root_authorized and args.timeout > 0 and args.byte_limit > 0, "ROOT authorization/budgets required")
    print(json.dumps(observe(args.pid, args.start_token, args.backend, args.executable,
                             args.executable_sha256, args.timeout, args.byte_limit), allow_nan=False))


if __name__ == "__main__":
    try:
        main()
    except (InvalidEvidence, OSError, subprocess.SubprocessError) as error:
        print(f"UNVERIFIED native process: {error}", file=sys.stderr)
        sys.exit(1)
