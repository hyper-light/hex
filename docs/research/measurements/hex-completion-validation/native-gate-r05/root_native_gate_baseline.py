#!/usr/bin/env python3
"""ROOT-only c576 Linux stand-in baseline; source-only until ROOT dispatches it.

One driver, no build, mutation, service/ancestor write or VM/GPU claim. See
ROOT-NATIVE-GATE-BASELINE.md for the exact admission and fresh-scope invocation.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import select
import selectors
import signal
import subprocess
import sys
import time


PIN = "c576d4c2a0a37d7722a81174b9ab1d7ff1434ad5"
COHORT = Path("/home/adalundhe/.cache/hex-lead/cohort-c576d4c2-linux-loader-gate-c576")
SOURCE = COHORT / "source"
BINARY = COHORT / "target/debug/deps/hex_node-d6283f30893b31f3"
BINARY_SHA = "f8b10118b68414c0d9dc3faed95664bc8b248f7b27bd76e285dd0fbd2b8372d9"
BINARY_BYTES = 185675000
SOURCES = {
    "crates/hex-runtime/src/linux/sys.rs": "f90284ac37e877201342f3c170f4ae091a82269f460fbc47815706dd73f055e4",
    "crates/hex-node/src/supervise/completion_native_gate_tests.rs": "da4ba641b6122a402007aa8939cddb6154dbe114ac437ce47073afdae7431241",
    "crates/hex-api/src/spec.rs": "6c209d5880bf1003f33c2aee26da0929e3b48dbc7b2ce299c1578c69e65f67ad",
    "crates/hex-api/src/bounds.rs": "54876217b8f7494f7f23cedc2b4060dd14d884424a1d8192cd30f71dc0f030e8",
}
CGROOT = Path("/sys/fs/cgroup")
CONTROLLERS = {"cpu", "io", "memory", "pids"}
# Explicit user-authorized TEST inputs, never production allowances/defaults.
JOB_MEMORY = 4 * 1024**3
TEST_MEMORY = 64 * 1024**2
PIDS = 256
LIMIT = 1024**2
PHASE_SECONDS = 180
SELECTORS = (
    "completion_publication_refusal_never_executes_marker",
    "completion_durable_publication_releases_and_reaps",
    "completion_controller_parent_death_cancels_gate",
)
RESOURCE_FILES = (
    "cgroup.procs", "cgroup.events", "cgroup.type", "cgroup.controllers",
    "cgroup.subtree_control", "cgroup.stat", "cpu.max", "cpu.stat",
    "memory.max", "memory.swap.max", "memory.current", "memory.peak",
    "memory.events", "pids.max", "pids.current", "pids.events", "io.stat",
)


class Refused(Exception):
    pass


def require(value, why):
    if not value:
        raise Refused(why)


def left(end):
    require(math.isfinite(end), "finite original deadline required")
    remaining = end - time.monotonic()
    require(remaining > 0, "original 180s phase guard expired")
    return remaining


def read(path):
    with Path(path).open("rb") as file:
        data = file.read(LIMIT + 1)
    require(len(data) <= LIMIT, f"read exceeds existing 1MiB bound: {path}")
    return data.decode("utf-8", "strict")


def write(path, value):
    with Path(path).open("w") as file:
        file.write(value)


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as file:
        while block := file.read(LIMIT):
            value.update(block)
    return value.hexdigest()


def birth(pid):
    text = read(f"/proc/{pid}/stat")
    fields = text.rsplit(")", 1)[1].split()
    return {"pid": pid, "ppid": int(fields[1]), "start_ticks": int(fields[19])}


def group_of(pid):
    lines = read(f"/proc/{pid}/cgroup").splitlines()
    unified = [line[3:] for line in lines if line.startswith("0::/")]
    require(len(unified) == 1, "exact unified cgroup membership required")
    path = (CGROOT / unified[0].lstrip("/")).resolve(strict=True)
    require(path.is_relative_to(CGROOT) and path != CGROOT, "non-root cgroup required")
    return path


def procs(path):
    return {int(pid) for pid in read(path / "cgroup.procs").split()}


def directories(path):
    return {p for p in path.iterdir() if p.is_dir()}


def save(path, data):
    with Path(path).open("x") as file:
        json.dump(data, file, indent=2, sort_keys=True)
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


class Evidence:
    def __init__(self, lab):
        self.lab = lab
        self.journal = (lab / "journal.jsonl").open("x")
        self.failures = []

    def record(self, event, **data):
        try:
            json.dump({"event": event, "monotonic": time.monotonic(), **data}, self.journal, sort_keys=True)
            self.journal.write("\n")
            self.journal.flush()
            os.fsync(self.journal.fileno())
        except OSError as error:
            # Evidence I/O failure must not bypass exact owned-child cleanup.
            self.failures.append(f"journal {event}: {error}")

    def snapshot(self, name, root, end):
        pending, rows = [root], {}
        while pending:
            left(end)
            current = pending.pop()
            entry = {}
            for filename in RESOURCE_FILES:
                try:
                    entry[filename] = read(current / filename)
                except FileNotFoundError:
                    entry[filename] = {"absent": True}
            entry["identities"] = []
            for pid in procs(current):
                try:
                    entry["identities"].append(birth(pid))
                except (FileNotFoundError, ProcessLookupError):
                    entry["identities"].append({"pid": pid, "identity": "vanished during observation"})
            rows[str(current)] = entry
            pending.extend(directories(current))
        space = os.statvfs(self.lab)
        save(self.lab / name, {"cgroups": rows, "disk_available_bytes": space.f_bavail * space.f_frsize})


def verify_pins():
    require(BINARY.is_file() and BINARY.stat().st_size == BINARY_BYTES, "c576 binary size/path mismatch")
    require(digest(BINARY) == BINARY_SHA, "c576 binary SHA mismatch")
    for relative, expected in SOURCES.items():
        require(digest(SOURCE / relative) == expected, f"exact c576 source mismatch: {relative}")
    shell = Path("/bin/sh").stat()
    binary = BINARY.stat()
    return {"pin": PIN, "binary": str(BINARY), "bytes": BINARY_BYTES,
            "binary_sha256": BINARY_SHA, "binary_inode": [binary.st_dev, binary.st_ino],
            "sources": SOURCES, "shell_sha256": digest("/bin/sh"), "shell_inode": [shell.st_dev, shell.st_ino],
            "driver_sha256": digest(Path(__file__).resolve())}


def exec_phase():
    # SAME script self-exec preparation, not an installed helper/manager. Move only
    # this actor before Node can fork; no Python pre_exec callback or PID migration.
    unit, actor, selector, parent, started, end = sys.argv[2:]
    end = float(end)
    left(end)
    parent = int(parent)
    require(os.getppid() == parent and birth(parent)["start_ticks"] == int(started), "driver parent changed")
    current = group_of(os.getpid())
    require(current.name == "control" and current.parent.name == unit, "actor outside exact driver scope")
    actor = Path(actor).resolve(strict=True)
    require(actor.parent.parent == current.parent and actor.name == "actor", "foreign actor leaf")
    require(not procs(actor) and read(actor / "cgroup.type").strip() == "domain", "actor leaf not fresh empty domain")
    require(selector in SELECTORS, "unknown native selector")
    require(actor.parent.name == f"native-{SELECTORS.index(selector)}" and group_of(parent) == current, "phase/driver custody changed")
    left(end)
    write(actor / "cgroup.procs", str(os.getpid()))
    require(group_of(os.getpid()) == actor, "actor self-placement failed")
    # Execute the SAME opened inode whose bytes were checked, not a replaceable pathname.
    fd = os.open(BINARY, os.O_RDONLY | os.O_CLOEXEC)
    value = hashlib.sha256()
    with os.fdopen(os.dup(fd), "rb") as file:
        while block := file.read(LIMIT):
            value.update(block)
    require(os.fstat(fd).st_size == BINARY_BYTES and value.hexdigest() == BINARY_SHA, "binary changed before exec")
    require(os.execve in os.supports_fd, "fd-based exact ELF exec required")
    full = "supervise::completion_native_gate_tests::" + selector
    left(end)
    os.execve(fd, [str(BINARY), "--ignored", "--exact", full, "--nocapture", "--test-threads=1"], dict(os.environ))


def reap_ready(process, pidfd, end):
    require(select.select([pidfd], [], [], left(end))[0], "owned pidfd terminal guard expired")
    # Positive native terminal notification, then the SAME direct child's sole reap.
    # No Popen timeout busy loop or periodic process probe.
    return process.wait()


def run_owned(argv, env, out, err, end, evidence):
    process, pidfd, poller = None, None, selectors.DefaultSelector()
    result = {"argv": argv, "both_eof": False, "reaped": False, "error": None}
    outputs = {"stdout": out.open("xb"), "stderr": err.open("xb")}
    counts = {"stdout": 0, "stderr": 0}
    try:
        left(end)
        require(not evidence.failures, "evidence unavailable before actor")
        process = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        result["created_pid"] = process.pid
        pidfd = os.pidfd_open(process.pid)
        evidence.record("owned-child-created", pid=process.pid, argv=argv)
        result["identity"] = birth(process.pid)
        evidence.record("owned-child", **result["identity"], argv=argv)
        require(not evidence.failures, "owned-child evidence unavailable")
        poller.register(pidfd, selectors.EVENT_READ, "terminal")
        for name, stream in [("stdout", process.stdout), ("stderr", process.stderr)]:
            os.set_blocking(stream.fileno(), False)
            poller.register(stream, selectors.EVENT_READ, name)
        while poller.get_map():
            for key, _ in poller.select(left(end)):
                if key.data == "terminal":
                    poller.unregister(key.fileobj)
                    result["returncode"] = reap_ready(process, pidfd, end)
                    result["reaped"] = True
                else:
                    data = os.read(key.fd, LIMIT + 1 - counts[key.data])
                    if not data:
                        poller.unregister(key.fileobj)
                        key.fileobj.close()
                    else:
                        outputs[key.data].write(data)
                        counts[key.data] += len(data)
                        require(counts[key.data] <= LIMIT, f"{key.data} exceeds 1MiB; raw partial retained")
        result["both_eof"] = True
    except Exception as error:
        result["error"] = f"{type(error).__name__}: {error}"
    finally:
        for file in outputs.values():
            try:
                file.flush()
                os.fsync(file.fileno())
            except OSError as error:
                result["error"] = f"raw output preservation: {error}"
            finally:
                try:
                    file.close()
                except OSError as error:
                    result["error"] = f"raw output closure: {error}"
        if process is not None and not result["reaped"]:
            result["pending_pid"] = process.pid
            evidence.record("owned-child-partial-before-cleanup", result=result, captured_bytes=counts)
            try:
                # Exact handle of the direct child created above, never a searched/reused PID.
                require(pidfd is not None, "no exact child pidfd; cleanup remains uncertain")
                signal.pidfd_send_signal(pidfd, signal.SIGKILL)
                result["returncode"] = reap_ready(process, pidfd, end)
                result["reaped"] = True
            except ProcessLookupError:
                try:
                    result["returncode"] = reap_ready(process, pidfd, end)
                    result["reaped"] = True
                except Exception as error:
                    result["cleanup_error"] = f"{type(error).__name__}: {error}"
            except Exception as error:
                result["cleanup_error"] = f"{type(error).__name__}: {error}"
        for stream in (process.stdout, process.stderr) if process is not None else ():
            if stream is not None:
                stream.close()
        poller.close()
        if pidfd is not None:
            os.close(pidfd)
    return process, result


def empty(path):
    return dict(line.split() for line in read(path / "cgroup.events").splitlines())["populated"] == "0"


def clean_phase(path, process, end, evidence):
    facts = {"path": str(path), "kill_needed": False, "empty": False,
             "reaped": process is None or process.returncode is not None, "error": None}
    try:
        # The subtree was created empty by this driver; only its self-placed one
        # actor and that actor's native descendants were admitted to it.
        with (path / "cgroup.events").open("r") as event:
            if not empty(path):
                facts["kill_needed"] = True
                write(path / "cgroup.kill", "1")
                # Register before clearing/reading the actual state to avoid a lost wake.
                notify = select.poll()
                notify.register(event.fileno(), select.POLLPRI | select.POLLERR)
                while True:
                    event.seek(0)
                    state = dict(line.split() for line in event.read().splitlines())
                    if state["populated"] == "0":
                        break
                    notify.poll(math.ceil(left(end) * 1000))
            facts["empty"] = empty(path)
        if process is not None and process.returncode is None:
            # Still the unreaped direct child: reopening an observation pidfd
            # cannot select a reused PID while this sole waiter retains it.
            pidfd = os.pidfd_open(process.pid)
            try:
                reap_ready(process, pidfd, end)
            finally:
                os.close(pidfd)
        if process is not None:
            facts["reaped"] = True
        pending, ordered = [path], []
        while pending:
            left(end)
            current = pending.pop()
            ordered.append(current)
            pending.extend(directories(current))
        for current in reversed(ordered):
            require(not procs(current), "nonempty owned cgroup cannot be removed")
            current.rmdir()
    except Exception as error:
        facts["error"] = f"{type(error).__name__}: {error}"
    evidence.record("phase-cleanup", **facts)
    return facts


def config(lab, phase, shell_sha):
    program = {"args": ["/bin/sh", "-c", "printf '%s' \"$2\" > \"$1\"", "hex-gate", "@MARKER@", "@TOKEN@"],
               "env": [], "cwd": None, "user": None}
    bounds = {"cpu_weight": None, "cpu_max_milli": 1000, "memory_max": TEST_MEMORY,
              "memory_high": None, "io_weight": None, "io_latency_us": None, "io_max": None, "pids_max": PIDS}
    rootfs = lab / "empty-rootfs"
    oci = {"rootfs": {"dir": str(rootfs)}, "init": {"args": ["/bin/sh", "-c", "exit 0"], "env": [], "cwd": None, "user": None},
           "mounts": [], "hostname": None, "network": "private", "hosts": []}
    return {"delegated_parent": str(phase), "lab": str(lab),
            "spec": {"id": "completion-gate-baseline", "bounds": bounds, "class": {"firecracker": oci},
                     "owner": None, "devices": [], "allocation": None, "route": None},
            "controller": program, "controller_sha256": shell_sha, "test_binary_sha256": BINARY_SHA,
            "pidfd_budget": 2, "guard_ms": 60000, "gate_budget_ms": 120000,
            "observation_ms": 1, "gate_wait_syscall": 271, "byte_limit": LIMIT}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--unit", required=True, help="exact freshly delegated systemd user .scope")
    parser.add_argument("--lab", required=True, type=Path, help="new absolute evidence directory below c576 cohort")
    args = parser.parse_args()
    require(sys.platform == "linux" and platform.machine() == "x86_64", "exact Linux x64 fixture required")
    require(os.geteuid() != 0 and os.getuid() == os.geteuid(), "unprivileged same-UID driver required")
    require(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL, "sole retained child ownership required")
    require(re.fullmatch(r"[A-Za-z0-9_-]+\.scope", args.unit), "exact scope unit name required")
    require(args.lab.is_absolute() and args.lab.parent.resolve(strict=True) == COHORT.resolve(strict=True), "fresh lab must be direct child of c576 cohort")
    args.lab.mkdir(mode=0o700)
    evidence = Evidence(args.lab)
    def interrupted(signum, _frame):
        evidence.failures.append(f"driver cancellation signal {signum}")
        raise Refused(f"owned driver received signal {signum}; preserve partial/cleanup uncertainty")
    signal.signal(signal.SIGTERM, interrupted)
    scope, control, moved, enabled, rows, errors = None, None, False, False, [], []
    driver = birth(os.getpid())
    try:
        pins = verify_pins()  # No child or native actor exists before all pin checks.
        save(args.lab / "pins-before.json", pins)
        evidence.record("driver", identity=driver, requested_admission={"cpus": 1, "mem": "4g", "disk": "12g", "receipt": "ROOT external normal admission required"}, vm_gpu_claim=False)
        scope = group_of(os.getpid())
        require(scope.name == args.unit, "driver not in exact supplied scope")
        require(procs(scope) == {os.getpid()} and not directories(scope), "fresh scope must contain ONLY own driver PID and no children")
        require(read(scope / "cgroup.type").strip() == "domain", "normal domain required")
        require(not read(scope / "cgroup.subtree_control").split(), "fresh scope already distributes controllers")
        require(CONTROLLERS <= set(read(scope / "cgroup.controllers").split()), "missing cpu/io/memory/pids delegation")
        limits_before = {name: read(scope / name) for name in ("cpu.max", "memory.max", "memory.swap.max", "pids.max")}
        quota, period = limits_before["cpu.max"].split()
        require(quota != "max" and int(quota) == int(period), "scope CPU quota must equal100%")
        require(int(limits_before["memory.max"]) == JOB_MEMORY and int(limits_before["memory.swap.max"]) == 0 and int(limits_before["pids.max"]) == PIDS, "scope must be4GiB/swap0/pids256")
        evidence.snapshot("scope-before.json", scope, time.monotonic() + PHASE_SECONDS)
        control = scope / "control"
        control.mkdir()
        require(procs(scope) == {os.getpid()}, "foreign process arrived before self migration")
        write(control / "cgroup.procs", str(os.getpid()))
        moved = True
        require(group_of(os.getpid()) == control and not procs(scope) and procs(control) == {os.getpid()}, "onlyself migration/empty parent not established")
        # Only this exact newly delegated scope is written, never an ancestor/service.
        meta_end = time.monotonic() + PHASE_SECONDS
        meta, info = run_owned(["/usr/bin/systemctl", "--user", "show", args.unit, "--property=ControlGroup,Delegate,ActiveState"], dict(os.environ), args.lab / "unit.stdout", args.lab / "unit.stderr", meta_end, evidence)
        evidence.record("unit-query", result=info)
        require(info["error"] is None and info.get("returncode") == 0 and info["reaped"] and info["both_eof"], "unit query uncertain")
        properties = dict(line.split("=", 1) for line in read(args.lab / "unit.stdout").splitlines())
        require(properties.get("Delegate") == "yes" and properties.get("ActiveState") == "active" and properties.get("ControlGroup") == "/" + str(scope.relative_to(CGROOT)), "exact active delegated user scope not confirmed")
        require(not procs(scope) and procs(control) == {os.getpid()} and directories(scope) == {control}, "foreign internal process/child before controller enable")
        require(not evidence.failures, "preflight evidence unavailable")
        write(scope / "cgroup.subtree_control", "+cpu +io +memory +pids")
        enabled = True
        require(CONTROLLERS <= set(read(scope / "cgroup.subtree_control").split()), "controller enable/readback failed")
        (args.lab / "empty-rootfs").mkdir()
        env = dict(os.environ)
        env.pop("HEX_COMPLETION_GATE_ROLE", None)
        env.pop("HEX_COMPLETION_GATE_SCOPE", None)
        for index, selector in enumerate(SELECTORS):
            require(not evidence.failures, "evidence/cancellation failure; remaining selectors UNRUN")
            phase = scope / f"native-{index}"
            phase.mkdir()
            end = time.monotonic() + PHASE_SECONDS
            process, observed, cleanup = None, None, None
            try:
                require(not procs(phase), "phase not empty")
                write(phase / "cgroup.subtree_control", "+cpu +io +memory +pids")
                actor = phase / "actor"
                actor.mkdir()
                cfg = args.lab / f"case-{index}.config.json"
                save(cfg, config(args.lab, phase, pins["shell_sha256"]))
                verify_pins()
                require(digest("/bin/sh") == pins["shell_sha256"], "shell changed before actor")
                evidence.snapshot(f"case-{index}.before.json", phase, end)
                child_env = {**env, "HEX_COMPLETION_ROOT_AUTHORIZED": "1", "HEX_COMPLETION_GATE_CONFIG": str(cfg), "HEX_COMPLETION_GATE_BYTES": str(LIMIT), "RUST_TEST_THREADS": "1"}
                argv = [sys.executable, str(Path(__file__).resolve()), "--exec-phase", args.unit, str(actor), selector, str(driver["pid"]), str(driver["start_ticks"]), repr(end)]
                process, observed = run_owned(argv, child_env, args.lab / f"case-{index}.stdout", args.lab / f"case-{index}.stderr", end, evidence)
                evidence.record("native-observed", selector=selector, result=observed)
                evidence.snapshot(f"case-{index}.after.json", phase, end)
            except Exception as error:
                errors.append(f"{selector}: {type(error).__name__}: {error}")
            finally:
                cleanup = clean_phase(phase, process, end, evidence)
            # Raw/error/cleanup records are durable before any result classification.
            text = read(args.lab / f"case-{index}.stdout") if (args.lab / f"case-{index}.stdout").exists() else ""
            summary = re.findall(r"(?m)^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
            counts = [int(n) for n in summary[0][1:]] if len(summary) == 1 else None
            status = summary[0][0] if len(summary) == 1 else None
            passed = bool(observed and observed["error"] is None and observed.get("returncode") == 0 and observed["reaped"] and observed["both_eof"] and status == "ok" and counts == [1, 0, 0] and cleanup["empty"] and cleanup["reaped"] and not cleanup["kill_needed"] and cleanup["error"] is None)
            rows.append({"selector": selector, "passed": passed, "harness_status": status, "completed_harness_counts": counts, "observation": observed, "cleanup": cleanup})
            save(args.lab / f"case-{index}.result.json", rows[-1])
            if not cleanup["empty"] or not cleanup["reaped"] or cleanup["error"]:
                raise Refused("owned phase cleanup uncertain; remaining selectors UNRUN")
        require(verify_pins() == pins, "source/binary/shell post-state changed")
        evidence.snapshot("scope-after-phases.json", scope, time.monotonic() + PHASE_SECONDS)
    except Exception as error:
        errors.append(f"{type(error).__name__}: {error}")
        evidence.record("refusal", error=errors[-1])
    finally:
        restored = False
        try:
            save(args.lab / "pins-after.json", verify_pins())
        except Exception as error:
            errors.append(f"post-pins: {type(error).__name__}: {error}")
            evidence.record("post-pins-uncertain", error=errors[-1])
        try:
            if moved:
                require(directories(scope) == {control} and procs(control) == {os.getpid()} and not procs(scope), "private descendants/foreign process remain; no controller restoration")
                if enabled:
                    write(scope / "cgroup.subtree_control", "-cpu -io -memory -pids")
                require(not read(scope / "cgroup.subtree_control").split(), "private scope controller restoration failed")
                write(scope / "cgroup.procs", str(os.getpid()))
                require(group_of(os.getpid()) == scope and procs(scope) == {os.getpid()}, "self restoration uncertain")
                control.rmdir()
                restored = not directories(scope)
                limits_after = {name: read(scope / name) for name in limits_before}
                save(args.lab / "scope-restoration.json", {"before": limits_before, "after": limits_after,
                     "same_limits": limits_before == limits_after, "procs": sorted(procs(scope)),
                     "subtree_control": read(scope / "cgroup.subtree_control"), "driver": driver})
                require(limits_before == limits_after, "unit resource limits changed")
            elif control is not None and control.exists() and not procs(control):
                control.rmdir()
            evidence.record("driver-restoration", restored=restored, identity=driver)
        except Exception as error:
            errors.append(f"restoration: {type(error).__name__}: {error}")
            evidence.record("driver-restoration-uncertain", error=errors[-1])
            restored = False
        errors.extend(evidence.failures)
        result = {"schema": "hex-native-gate-baseline-c576-1", "vm_gpu_claim": False, "source": PIN,
                  "attempted": len(rows), "required": 3, "passed": sum(row["passed"] for row in rows),
                  "completed_selected": sum(sum(row["completed_harness_counts"][:2]) for row in rows if row["completed_harness_counts"] is not None),
                  "cases": rows, "errors": errors, "driver_restored": restored,
                  "status": "PASS" if len(rows) == 3 and all(row["passed"] for row in rows) and not errors and restored else "FAIL_OR_UNVERIFIED"}
        save(args.lab / "result.json", result)
        evidence.journal.close()
        members = {}
        for path in args.lab.rglob("*"):
            if path.is_symlink():
                members[str(path.relative_to(args.lab))] = {"type": "symlink", "target": os.readlink(path)}
            elif path.is_file():
                members[str(path.relative_to(args.lab))] = {"bytes": path.stat().st_size, "sha256": digest(path)}
        save(args.lab / "SHA256.json", members)
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    try:
        if len(sys.argv) > 1 and sys.argv[1] == "--exec-phase":
            exec_phase()
            raise Refused("exec unexpectedly returned")
        raise SystemExit(main())
    except Exception as error:
        print(f"REFUSE {type(error).__name__}: {error}", file=sys.stderr)
        raise SystemExit(2)
