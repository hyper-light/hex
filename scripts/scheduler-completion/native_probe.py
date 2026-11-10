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

from evidence import InvalidEvidence, finite_positive, hex_digest, load_json, require, sha256


def linux_process(pid, byte_limit):
    proc = Path("/proc") / str(pid)
    with (proc / "stat").open("rb") as source:
        raw = source.read(byte_limit + 1)
    require(len(raw) <= byte_limit, "proc stat exceeded caller budget")
    stat = raw.decode()
    # comm can contain spaces/parentheses; the last ')' ends it, and starttime is field 22.
    fields = stat[stat.rfind(")") + 2:].split()
    require(len(fields) >= 20, "short proc stat")
    descriptors = list((proc / "fd").iterdir())
    require(len(descriptors) <= byte_limit, "native descriptor inventory exceeded caller budget")
    return {"pid": pid, "start_token": fields[19], "executable": str((proc / "exe").resolve(strict=True)),
            "exe_handle": proc / "exe", "parent": int(fields[1]),
            "namespaces": {name: os.readlink(proc / "ns" / name) for name in ("mnt", "pid")},
            "self_namespaces": {name: os.readlink(Path("/proc/self/ns") / name) for name in ("mnt", "pid")},
            "fd_targets": [os.readlink(entry) for entry in descriptors]}


def mac_process(pid, timeout, byte_limit):
    # Match Shards' native process identity: rusage_info_v0.ri_proc_start_abstime,
    # Mach absolute time, not ps's second-resolution wall-clock start.
    libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    function = libproc.proc_pidpath
    function.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
    function.restype = ctypes.c_int
    buffer = ctypes.create_string_buffer(byte_limit)
    if function(pid, buffer, byte_limit) <= 0:
        error=ctypes.get_errno()
        if error==3:
            raise ProcessLookupError(error,"native process is absent")
        raise OSError(error,"proc_pidpath failed; absence is unverified")
    class Usage(ctypes.Structure):
        _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in
            ("user", "system", "idle_wakeups", "interrupt_wakeups", "pageins", "wired", "resident",
             "footprint", "started", "ended")]
    usage = Usage()
    rusage = libproc.proc_pid_rusage
    rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    rusage.restype = ctypes.c_int
    if rusage(pid, 0, ctypes.byref(usage)) != 0:
        error=ctypes.get_errno()
        if error==3:
            raise ProcessLookupError(error,"native process is absent")
        raise OSError(error,"proc_pid_rusage failed; absence is unverified")
    path = Path(os.fsdecode(buffer.value)).resolve(strict=True)
    return {"pid": pid, "start_token": str(usage.started), "executable": str(path), "exe_handle": path}


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
    if not handle:
        error=ctypes.get_last_error()
        if error==87: # ERROR_INVALID_PARAMETER: a valid positive PID names no process.
            raise ProcessLookupError(error,"no process with this PID")
        raise OSError(error,"OpenProcess query refused; absence is unverified")
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
        row = linux_process(pid, byte_limit)
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
    parser.add_argument("--pid", type=int)
    parser.add_argument("--start-token")
    parser.add_argument("--identity",type=Path)
    parser.add_argument("--format",choices=("json","hex-guest","hex-selected-guest"),default="json")
    parser.add_argument("--pointer",default="/init")
    parser.add_argument("--backend", choices=("oci", "firecracker", "shards"), required=True)
    parser.add_argument("--executable", required=True)
    parser.add_argument("--executable-sha256", required=True)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--byte-limit", type=int, required=True)
    args = parser.parse_args()
    require(args.root_authorized and finite_positive(args.timeout) and 0 < args.byte_limit <= 0xffffffff,
            "ROOT finite envelope and native u32 buffer bound required")
    pid,start=args.pid,args.start_token
    if args.identity:
        require(pid is None and start is None,"identity source is exclusive")
        if args.format in ("hex-guest","hex-selected-guest"):
            with args.identity.open("rb") as source:
                raw=source.read(args.byte_limit+1)
            require(len(raw)<=args.byte_limit,"identity record exceeded budget")
            lines=raw.decode().splitlines()
            if args.format=="hex-guest":
                require(len(lines)==4 and lines[0]=="hex-guest 1","unexpected guest record format")
            else:
                require(lines[0]=="hex-selected-guest 1" and "running" in lines,
                        "selected guest has no actual running process receipt")
            pids=[line.removeprefix("pid ") for line in lines if line.startswith("pid ")]
            starts=[line.removeprefix("started ") for line in lines if line.startswith("started ")]
            require(len(pids)==1 and len(starts)==1,"missing/ambiguous actual process identity")
            pid=int(pids[0]);start=starts[0]
        else:
            value=load_json(args.identity,args.byte_limit)
            for part in args.pointer.strip("/").split("/"):
                value=value[part]
            pid=value["pid"]
            start=value.get("start_token",value.get("start_ticks",value.get("started")))
    require(pid is not None and start is not None,"runtime-owned identity required")
    print(json.dumps(observe(pid, start, args.backend, args.executable,
                             args.executable_sha256, args.timeout, args.byte_limit), allow_nan=False))


if __name__ == "__main__":
    try:
        main()
    except (InvalidEvidence, OSError, subprocess.SubprocessError) as error:
        print(f"UNVERIFIED native process: {error}", file=sys.stderr)
        sys.exit(1)
