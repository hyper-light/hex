#!/usr/bin/env python3
"""CP-T7.4/LB03: lead-run actual scheduler wait census over ordinary native/Guest hex node endpoints.

This file never builds anything. All state, credentials, requests, logs and PID records are private
cache files. No HOME, real SSH configuration, authorized_keys or runner service is changed.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import pwd
import re
import resource
import selectors
import shlex
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import tarfile
import time


class Failed(RuntimeError):
    pass


def require(fact, why):
    if not fact:
        raise Failed(why)


def atomic_json(path, value):
    path = Path(path)
    temporary = path.with_name(path.name + ".next")
    with temporary.open("w", encoding="utf-8") as output:
        json.dump(value, output, sort_keys=True)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    os.replace(temporary, path)


def record(runtime, kind, pid=None, **details):
    row = {"kind": kind, "pid": os.getpid() if pid is None else pid,
           "parent": os.getppid() if pid is None else os.getpid(),
           "recorded_by": os.getpid(), "run": str(runtime["root"]), **details}
    data = (json.dumps(row, sort_keys=True) + "\n").encode()
    fd = os.open(runtime["ledger"], os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        require(os.write(fd, data) == len(data), "short PID-ledger write")
        os.fsync(fd)
    finally:
        os.close(fd)


def runtime_file(path):
    with Path(path).open(encoding="utf-8") as data:
        return json.load(data)


def remaining(deadline):
    left = deadline - time.monotonic()
    require(left > 0, "the derived operation deadline expired")
    return left


def read_exact(stream, count, deadline, eof=False):
    data = bytearray()
    with selectors.DefaultSelector() as poll:
        poll.register(stream, selectors.EVENT_READ)
        while len(data) < count:
            require(poll.select(remaining(deadline)), "no frame bytes before the derived deadline")
            part = os.read(stream.fileno(), count - len(data))
            if not part:
                if eof and not data:
                    return None
                raise Failed("partial frame or unexpected EOF")
            data.extend(part)
    return bytes(data)


def write_all(stream, data, runtime):
    deadline = time.monotonic() + runtime["wait"]
    view = memoryview(data)
    with selectors.DefaultSelector() as poll:
        poll.register(stream, selectors.EVENT_WRITE)
        while view:
            require(poll.select(remaining(deadline)), "no frame write before the derived deadline")
            count = os.write(stream.fileno(), view)
            require(count > 0, "zero-byte frame write")
            view = view[count:]


def frame(stream, runtime):
    deadline = time.monotonic() + runtime["wait"]
    prefix = read_exact(stream, struct.calcsize(">I"), deadline, eof=True)
    if prefix is None:
        return None
    count = struct.unpack(">I", prefix)[0]
    require(count <= runtime["max_frame"], "peer frame exceeds the actual common-wire bound")
    return prefix + read_exact(stream, count, deadline)


def save_first(path, data):
    try:
        with Path(path).open("xb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
    except FileExistsError:
        pass


def forced(runtime_path, sender, receiver_socket):
    runtime = runtime_file(runtime_path)
    require("SSH_ORIGINAL_COMMAND" in os.environ, "forced tunnel requires actual SSH authentication")
    record(runtime, "forced-wrapper", sender=sender)
    error_path = Path(runtime["root"]) / f"forced-{sender}-{os.getpid()}.err"
    with error_path.open("ab", buffering=0) as errors:
        child_environment, census_path = census_environment(runtime, os.environ.copy(), f"forced-{sender}")
        child = subprocess.Popen([runtime["hex"], "schedule", "tunnel", "--socket", receiver_socket,
                                  "--from", str(sender)], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=errors, bufsize=0, env=child_environment)
        record(runtime, "forced-tunnel", child.pid, sender=sender, census=str(census_path), profile=runtime["profile"], source_revision=runtime.get("source_revision"), hex_sha256=runtime.get("hex_sha256"))
        faults = []

        def incoming():
            try:
                first = True
                while True:
                    message = frame(sys.stdin.buffer, runtime)
                    if message is None:
                        break
                    if first:
                        save_first(Path(runtime["root"]) / f"hello-{sender}.frame", message)
                        first = False
                    if b"p1 start " in message:
                        save_first(Path(runtime["root"]) / f"start-{sender}.frame", message)
                    write_all(child.stdin, message, runtime)
            except Exception as why:
                faults.append(type(why).__name__)
            finally:
                child.stdin.close()

        pump = threading.Thread(target=incoming, daemon=True)
        pump.start()
        try:
            while True:
                message = frame(child.stdout, runtime)
                if message is None:
                    break
                write_all(sys.stdout.buffer, message, runtime)
            child.wait(timeout=runtime["wait"])
            require(child.returncode == 0, "forced tunnel process failed; see private log")
            require(not faults, "forced tunnel input failed")
        finally:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=runtime["wait"])
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=runtime["wait"])


def shim():
    runtime = runtime_file(os.environ["HEX_SSH_E2E_RUNTIME"])
    record(runtime, "ssh-shim")
    os.execv(runtime["ssh"], [runtime["ssh"], "-F", runtime["ssh_config"], *sys.argv[2:]])


def integer(expression):
    """Only literal/one-operator unsigned source definitions; never evaluate source code."""
    text = expression.replace("_", "").strip()
    match = re.fullmatch(r"(0[xX][0-9a-fA-F]+|[0-9]+)(?:\s*(<<|\*|\+)\s*([0-9]+))?", text)
    require(match is not None, "unsupported source integer definition")
    left = int(match.group(1), 0) if match.group(1).lower().startswith("0x") else int(match.group(1))
    if match.group(2) is None:
        return left
    right = int(match.group(3))
    if match.group(2) == "<<":
        return left << right
    if match.group(2) == "*":
        return left * right
    return left + right


def source_constant(path, name):
    source = Path(path).read_text()
    match = re.search(r"pub const " + re.escape(name) + r"\s*:\s*\w+\s*=\s*([^;]+);", source)
    require(match is not None, f"missing existing source constant {name}")
    return integer(match.group(1))


def policy(source):
    return {"max_frame": source_constant(source / "crates/hex-core/src/wire.rs", "MAX_MESSAGE"),
            "min_cpu": source_constant(source / "crates/hex-api/src/bounds.rs", "MIN_CPU_MILLI"),
            "min_memory": source_constant(source / "crates/hex-api/src/bounds.rs", "MIN_MEMORY"),
            "policy_sha256": hashlib.sha256((source / "CLAUDE.md").read_bytes()).hexdigest()}


def digest_file(path, chunk):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        while True:
            data = source.read(chunk)
            if not data:
                break
            digest.update(data)
    return digest.hexdigest()


def census_environment(runtime, environment, label):
    fd, path = tempfile.mkstemp(prefix=label + "-", suffix=".tsv", dir=Path(runtime["root"]) / "census")
    os.close(fd)
    environment["HEX_WAITS_CENSUS"] = path
    environment["HEX_WAITS_CENSUS_MAX"] = str(runtime["max_census_lines"])
    return environment, Path(path)


def operation(value):
    if isinstance(value, str):
        return value.lower()
    return re.sub(r"(?<!^)(?=[A-Z])", "_", next(iter(value))).lower()


def quoted(value):
    return '"' + str(value).replace("\\", "\\\\").replace('"', '\\"') + '"'


class Harness:
    def __init__(self, args):
        require(sys.platform in ("linux", "darwin"), "this fixture supports native Linux and macOS")
        if sys.platform == "darwin":
            require(platform.machine().lower() in ("arm64", "aarch64"), "macOS fixture requires arm64")
        self.args, self.mode = args, args.mode
        self.phase = "setup"
        self.source = Path(args.source).resolve()
        require(args.mode != "native" or sys.platform == "linux", "ordinary native OCI hex node census requires Linux")
        require(args.mode != "guest" or sys.platform == "darwin", "ordinary hex node Guest path currently requires macOS arm64")
        parent = Path(args.run_parent).expanduser().resolve()
        parent.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="e-", dir=parent))
        os.chmod(self.root, 0o700)
        self.runtime = {**policy(self.source), "root": str(self.root),
                        "ledger": str(self.root / "pids.jsonl"), "hex": str(Path(args.hex).resolve()),
                        "runtime_client": str(Path(args.runtime_client).resolve()),
                        "profile": args.profile, "max_census_lines": args.max_census_lines,
                        "bootstrap": args.deadline_seconds, "wait": args.deadline_seconds,
                        "wait_ns": math.ceil(args.deadline_seconds * 1_000_000_000),
                        "wait_rule": "operator outer cancellation budget; never a runtime timing seed",
                        "ssh": shutil.which("ssh"), "ssh_config": str(self.root / "ssh_config")}
        self.sshd = args.sshd or shutil.which("sshd") or "/usr/sbin/sshd"
        self.keygen = shutil.which("ssh-keygen")
        require(self.runtime["ssh"] and self.keygen and Path(self.sshd).is_file(), "OpenSSH tools must already be installed")
        require(Path(self.runtime["hex"]).is_file() and Path(self.runtime["runtime_client"]).is_file(), "supply already-built Hex and real runtime-probe binaries")
        self.runtime_path = self.root / "runtime.json"
        self.children = []
        self.children_lock = threading.Lock()
        self.stop_accepting = threading.Event()
        self.listener = None
        self.acceptor = None
        self.accept_fault = None
        descriptor_limit = resource.getrlimit(resource.RLIMIT_NOFILE)[0]
        child_limit = os.sysconf("SC_CHILD_MAX")
        finite = [limit for limit in (descriptor_limit, child_limit) if limit > 0 and limit != resource.RLIM_INFINITY]
        require(finite, "platform must supply a finite child/descriptor bound")
        self.child_bound = min(finite)
        self.keys = []
        self.backend_process = self.backend_socket = self.backend_state = self.guest_receipt = None
        self.scheduler_processes = []
        self.baseline = []
        self.verified_created = self.verified_deleted = 0
        self.success_controls = {}
        (self.root / "census").mkdir()
        self.environment = os.environ.copy()
        for key in ("SSH_AUTH_SOCK", "SSH_AGENT_PID", "HEX_WAITS_CENSUS", "HEX_WAITS_CENSUS_MAX"):
            self.environment.pop(key, None)
        self.environment["HEX_SSH_E2E_RUNTIME"] = str(self.runtime_path)
        self.report = {"kind": "actual-scheduler-wait-census", "mode": args.mode,
                       "source": str(self.source), "profile": args.profile,
                       "profile_authority": "operator-declared; preserve build receipt and binary hash",
                       "n": args.n, "max_rows_per_process": args.max_census_lines,
                       "comparison_or_performance_claim": False}
        self.wait_kinds = [re.sub(r"(?<!^)(?=[A-Z])", "_", word).lower() for word in re.findall(r"^    ([A-Za-z]+),$", (self.source / "crates/hex-core/src/schedule/waits.rs").read_text(), re.MULTILINE)]
        require(self.wait_kinds and len(set(self.wait_kinds)) == len(self.wait_kinds), "cannot derive actual WaitKind set")
        require(args.cpu_milli >= self.runtime["min_cpu"] and args.box_memory_mib * (1 << 20) >= self.runtime["min_memory"], "requested real box is below actual API minima")
        self.backend_env = self.environment.copy()
        guest_names = ("SHARDS_BIN", "HEX_GUEST_KERNEL", "HEX_GUEST_INITRD", "HEX_GUEST_CPUS", "HEX_GUEST_MEMORY_MIB", "HEX_GUEST_SHARES")
        for name in guest_names:
            self.backend_env.pop(name, None)
        if args.mode == "guest":
            supplied = {"SHARDS_BIN": args.shards_bin, "HEX_GUEST_KERNEL": args.kernel, "HEX_GUEST_INITRD": args.initrd,
                        "HEX_GUEST_CPUS": str(args.guest_cpus), "HEX_GUEST_MEMORY_MIB": str(args.guest_memory_mib)}
            require(all(supplied.values()), "all real guest inputs must be explicitly supplied")
            self.backend_env.update(supplied)
            if args.guest_shares is not None:
                self.backend_env["HEX_GUEST_SHARES"] = args.guest_shares
            else:
                # A private empty share satisfies GuestConfig without exposing host user folders.
                share = self.root / "guest-shares"
                share.mkdir()
                self.backend_env["HEX_GUEST_SHARES"] = str(share) + ":ro"
            docker_config = self.root / "docker-config"
            docker_config.mkdir()
            atomic_json(docker_config / "config.json", {})
            self.backend_env["DOCKER_CONFIG"] = str(docker_config)
            guest_temp = self.root / "guest-tmp"
            guest_temp.mkdir()
            self.backend_env["TMPDIR"] = str(guest_temp)
            self.backend_env["SHARDS_HOME"] = str(self.root / "guest-home")
            require(shutil.disk_usage(self.root).free >= 30 * (1 << 30), "R16 requires at least30GiB free before the real guest run")
        require(args.mode != "native" or args.cgroup_parent, "native mode requires an explicitly delegated --cgroup-parent")
        self.rootfs = self.root / ("rootfs.tar" if args.mode == "guest" else "rootfs")
        linux_hex = Path(args.linux_hex).resolve()
        require(linux_hex.is_file(), "supply the already-built static Linux hex for the native architecture")
        if args.mode == "guest":
            with tarfile.open(self.rootfs, "w") as archive:
                info = tarfile.TarInfo("hex")
                info.size, info.mode = linux_hex.stat().st_size, 0o755
                with linux_hex.open("rb") as source:
                    archive.addfile(info, source)
        else:
            self.rootfs.mkdir()
            shutil.copyfile(linux_hex, self.rootfs / "hex")
            (self.rootfs / "hex").chmod(0o755)
        inputs = {"hex": self.runtime["hex"], "runtime_probe": self.runtime["runtime_client"], "linux_hex": str(linux_hex)}
        if args.mode == "guest":
            inputs.update(shards=args.shards_bin, kernel=args.kernel, initrd=args.initrd)
        self.report["binary_and_artifact_hashes"] = {name: {"path": str(Path(path).resolve()), "sha256": digest_file(path, self.runtime["max_frame"])} for name, path in inputs.items()}
        self.report["environment"] = {"os": platform.platform(), "machine": platform.machine(), "cpu_count": os.cpu_count(),
            "box_cpu_milli": args.cpu_milli, "box_memory_mib": args.box_memory_mib,
            "guest_cpus": args.guest_cpus, "guest_memory_mib": args.guest_memory_mib,
            "guest_shares_sha256": None if self.mode != "guest" else hashlib.sha256(self.backend_env["HEX_GUEST_SHARES"].encode()).hexdigest(),
            "path_sha256": hashlib.sha256(os.environ.get("PATH", "").encode()).hexdigest(),
            "free_disk_bytes_before": shutil.disk_usage(self.root).free}
        self.report["receipts"] = {name: {"path": str(Path(path).resolve()), "sha256": digest_file(path, self.runtime["max_frame"])} for name, path in {"policy": args.policy_receipt, "build": args.build_receipt, "deadline": args.deadline_evidence}.items()}
        self.report["preparation_policy"] = {"revision": "6bf5b15da904931b984ecd25662a8b8c571268bf", "rules_sha256": "0d936117dbc6663eb02829c3e921879cecdad3a8f503047588baa21dc82da936", "role": "implementer"}
        with Path(args.policy_receipt).open("rb") as receipt:
            execution_policy = receipt.read(self.runtime["max_frame"] + 1)
        require(len(execution_policy) <= self.runtime["max_frame"], "execution policy receipt exceeds the common-wire bound")
        self.report["execution_policy"] = json.loads(execution_policy)
        require(isinstance(self.report["execution_policy"], dict), "execution policy receipt must be a JSON object")
        atomic_json(self.runtime_path, self.runtime)

    def spawn(self, name, argv, *, ready=False, stdin=None, stdout=None, env=None):
        log = self.root / (name + ".log")
        selected_env = (self.environment if env is None else env).copy()
        census_path = None
        if str(argv[0]) in (self.runtime["hex"], self.runtime["runtime_client"]):
            selected_env, census_path = census_environment(self.runtime, selected_env, self.phase + "-" + name)
        with self.children_lock:
            self.children = [(kind, process) for kind, process in self.children if process.poll() is None]
            require(len(self.children) < self.child_bound, "host-derived child/descriptor budget exhausted")
            with log.open("ab", buffering=0) as output:
                process = subprocess.Popen(argv, stdin=stdin if stdin is not None else subprocess.DEVNULL,
                                           stdout=subprocess.PIPE if ready else (stdout if stdout is not None else output),
                                           stderr=output, env=selected_env, bufsize=0)
            process.hex_census_path = None if census_path is None else str(census_path)
            self.children.append((name, process))
        record(self.runtime, name, process.pid, census=process.hex_census_path, profile=self.runtime["profile"], source_revision=self.runtime.get("source_revision"), hex_sha256=self.runtime.get("hex_sha256"))
        return process

    def command(self, name, argv, timeout=None, env=None):
        process = self.spawn(name, argv, stdout=subprocess.PIPE, env=env)
        try:
            output, _ = process.communicate(timeout=self.runtime["wait"] if timeout is None else timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=self.runtime["wait"])
            raise Failed(f"{name} exceeded the derived deadline; see private log")
        record(self.runtime, "census-process-exit", process.pid, census=process.hex_census_path,
               returncode=process.returncode, success=process.returncode == 0)
        require(process.returncode == 0, f"{name} failed; see {name}.log in the private run directory")
        require(len(output) <= self.runtime["max_frame"], f"{name} output exceeds the common-wire bound")
        return output

    def ready(self, process, expected):
        deadline = time.monotonic() + self.runtime["wait"]
        line = bytearray()
        while not line.endswith(b"\n"):
            require(len(line) <= self.runtime["max_frame"], "readiness output exceeded wire bound")
            part = read_exact(process.stdout, 1, deadline)
            line.extend(part)
        require(line.decode().strip() == "ready " + str(expected), "unexpected readiness line")

    def stop(self, process, crash=False):
        if process.poll() is None:
            process.kill() if crash else process.terminate()
            try:
                process.wait(timeout=self.runtime["wait"])
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=self.runtime["wait"])
        record(self.runtime, "reaped", process.pid, returncode=process.returncode)

    def make_key(self, name):
        path = self.root / name
        self.command("keygen-" + name, [self.keygen, "-q", "-t", "ed25519", "-N", "", "-f", str(path)])
        os.chmod(path, 0o600)
        self.keys.append(path)
        return path

    def setup_ssh(self):
        host = self.make_key("host-key")
        clients = [self.make_key("client-0"), self.make_key("client-1")]
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(socket.SOMAXCONN)
        port = self.listener.getsockname()[1]
        user = pwd.getpwuid(os.geteuid()).pw_name
        script = str(Path(__file__).resolve())
        authorized = []
        for sender, key in enumerate(clients):
            receiver = self.root / ("r.sock" if sender == 0 else "o.sock")
            command = "exec " + shlex.join([sys.executable, script, "_forced", str(self.runtime_path), str(sender), str(receiver)])
            escaped = command.replace("\\", "\\\\").replace('"', '\\"')
            authorized.append('restrict,command="' + escaped + '" ' + key.with_suffix(".pub").read_text().strip())
        (self.root / "authorized_keys").write_text("\n".join(authorized) + "\n")
        host_public = host.with_suffix(".pub").read_text().split()
        (self.root / "known_hosts").write_text("hex-fixture " + " ".join(host_public[:2]) + "\n")
        ssh_config = []
        for target, key in [(1, clients[0]), (0, clients[1])]:
            ssh_config += [f"Host fixture-{target}", " HostName 127.0.0.1", f" Port {port}",
                           " User " + user, " IdentityFile " + quoted(key), " IdentitiesOnly yes",
                           " BatchMode yes", " PasswordAuthentication no", " KbdInteractiveAuthentication no",
                           " StrictHostKeyChecking yes", " HostKeyAlias hex-fixture",
                           " UserKnownHostsFile " + quoted(self.root / "known_hosts"),
                           " GlobalKnownHostsFile /dev/null", " ForwardAgent no", " ClearAllForwardings yes"]
        (self.root / "ssh_config").write_text("\n".join(ssh_config) + "\n")
        server = ["HostKey " + quoted(host), "AuthorizedKeysFile " + quoted(self.root / "authorized_keys"),
                  "PidFile " + quoted(self.root / "sshd.pid"), "UsePAM no", "UseDNS no", "StrictModes yes",
                  "PubkeyAuthentication yes", "AuthenticationMethods publickey", "PasswordAuthentication no",
                  "KbdInteractiveAuthentication no", "PermitRootLogin forced-commands-only", "PermitUserRC no",
                  "PermitUserEnvironment no", "AllowAgentForwarding no", "AllowTcpForwarding no", "PermitTunnel no",
                  "X11Forwarding no", "PrintMotd no", "PrintLastLog no", "LogLevel ERROR", "AllowUsers " + user]
        config = self.root / "sshd_config"
        config.write_text("\n".join(server) + "\n")
        effective = self.command("sshd-effective", [self.sshd, "-T", "-f", str(config)]).decode()
        options = dict(line.split(None, 1) for line in effective.splitlines() if " " in line)
        grace = float(options.get("logingracetime", "0"))
        require(math.isfinite(grace) and grace > 0, "native sshd must supply a finite positive LoginGraceTime")
        self.report["native_ssh_login_grace_seconds"] = grace
        with (self.root / "ssh_config").open("a") as output:
            # Native SSH grace and the existing harness record supply this bound, not a picked timeout.
            output.write(f"Host *\n ConnectTimeout {math.ceil(self.runtime['wait'])}\n")
        atomic_json(self.runtime_path, self.runtime)
        private_bin = self.root / "bin"
        private_bin.mkdir()
        shim_path = private_bin / "ssh"
        shim_path.write_text("#!/bin/sh\nexec " + shlex.join([sys.executable, script, "_shim"]) + ' "$@"\n')
        shim_path.chmod(0o700)
        self.environment["PATH"] = str(private_bin) + os.pathsep + os.environ.get("PATH", os.defpath)
        self.report["transport"] = {"address": "127.0.0.1", "os_assigned_port": port,
                                    "host_key_pinned": True, "forced_senders": [0, 1]}
        self.report["outer_cancellation_policy"] = {key: self.runtime[key] for key in ("bootstrap", "wait", "wait_rule", "policy_sha256")}

        def accept():
            try:
                while not self.stop_accepting.is_set():
                    connection, _ = self.listener.accept()
                    try:
                        self.spawn("sshd-" + str(connection.fileno()), [self.sshd, "-i", "-e", "-f", str(config)], stdin=connection, stdout=connection)
                    finally:
                        connection.close()
            except Exception as why:
                if not self.stop_accepting.is_set():
                    self.accept_fault = type(why).__name__
        self.acceptor = threading.Thread(target=accept, daemon=True)
        self.acceptor.start()

    def backend(self, name):
        socket_path = self.root / (name + ".sock")
        state = self.root / (name + ".node-state")
        require(not state.exists(), "real backend state must be fresh and exclusively owned")
        argv = [self.runtime["hex"], "node", "--socket", str(socket_path), "--state", str(state)]
        if self.mode == "native":
            parent = Path(self.args.cgroup_parent).resolve()
            box_root = parent / (self.root.name + "-" + name)
            require(not box_root.exists(), "refuse to reuse an existing cgroup tenant root")
            argv += ["--box-root", str(box_root)]
        process = self.spawn(name, argv, ready=True, env=self.backend_env)
        self.backend_process, self.backend_socket, self.backend_state = process, socket_path, state
        self.ready(process, socket_path)
        self.baseline = self.snapshot()["boxes"]
        require(not self.baseline, "fresh real endpoint unexpectedly contains a physical box")
        if self.mode == "guest":
            self.guest_receipt = self.root / "owned-guest.json"
            result = self.probe("--remember-guest", str(state / "@guest"), "--owned-root", str(self.root),
                                "--receipt", str(self.guest_receipt))
            require(result.get("remembered"), "real guest ownership was not retained")
            record(self.runtime, "owned-guest", result["pid"], started=result["started"], receipt=str(self.guest_receipt), ownership="fresh private node record; identity-checked retirement only")
        return socket_path


    def scheduler(self, name, node, backends, peers, realm):
        socket_path = self.root / (name + ".sock")
        config = self.root / (name + ".json")
        atomic_json(config, {"cluster": realm, "node": node, "state": str(self.root / (name + ".state")),
                             "socket": str(socket_path), "nodes": [], "models": [], "backends": backends, "peers": peers})
        process = self.spawn(name, [self.runtime["hex"], "schedule", "serve", "--config", str(config)], ready=True)
        self.ready(process, socket_path)
        self.scheduler_processes.append(process)
        return process, socket_path, config

    def restart(self, name, process, socket_path, config):
        self.stop(process, crash=True)
        process = self.spawn(name + "-restart", [self.runtime["hex"], "schedule", "serve", "--config", str(config)], ready=True)
        self.ready(process, socket_path)
        return process

    def call(self, socket_path, value, deadline=None):
        request = self.root / "request.json"
        atomic_json(request, value)
        if deadline is None:
            deadline = time.monotonic() + self.runtime["wait"]
        while True:
            remaining(deadline)
            log = self.root / "call.log"
            before = log.stat().st_size if log.exists() else 0
            process = self.spawn("call", [self.runtime["hex"], "schedule", "call", "--socket", str(socket_path), "--request", str(request)], stdout=subprocess.PIPE)
            try:
                output, _ = process.communicate(timeout=remaining(deadline))
            except subprocess.TimeoutExpired:
                self.stop(process, crash=True)
                raise Failed("scheduler call exceeded the derived deadline")
            require(len(output) <= self.runtime["max_frame"], "call output exceeds the common-wire bound")
            if process.returncode != 0:
                record(self.runtime, "census-call-exit", process.pid, census=process.hex_census_path, returncode=process.returncode, operation=operation(value), success=False)
                with log.open("rb") as errors:
                    errors.seek(before)
                    why = errors.read(self.runtime["max_frame"])
                # This guard refuses before key allocation or any runtime effect. Honor only
                # its typed backpressure; all other failures still terminate the fixture.
                if b"the node holds its most scheduler command publication (1)" in why:
                    continue
                raise Failed("call failed; see call.log in the private run directory")
            reply = json.loads(output)
            record(self.runtime, "census-call-exit", process.pid, census=process.hex_census_path, returncode=process.returncode, operation=operation(value), success=not reply.get("refusals"))
            require(not reply.get("refusals"), "scheduler returned an actual typed runtime refusal")
            kind = operation(value)
            self.success_controls[kind] = self.success_controls.get(kind, 0) + 1
            return reply

    def status(self, socket_path, key, fact):
        deadline = time.monotonic() + self.runtime["wait"]
        while True:
            remaining(deadline)
            reply = self.call(socket_path, {"Status": {"key": key}}, deadline=deadline)
            if fact(reply):
                return reply
            # Each iteration waits on an actual bounded CLI answer; there is no polling sleep.

    def probe(self, *arguments):
        output = self.command("runtime-probe", [self.runtime["runtime_client"], *arguments], env=self.backend_env)
        return json.loads(output)

    def snapshot(self, inspect=None):
        arguments = ["--socket", str(self.backend_socket)]
        if inspect is not None:
            arguments += ["--inspect", inspect]
        return self.probe(*arguments)

    def submit(self, socket_path, label):
        cpu, memory = self.args.cpu_milli, self.args.box_memory_mib * (1 << 20)
        architecture = "arm64" if platform.machine().lower() in ("arm64", "aarch64") else "amd64"
        cls = "shards" if self.mode == "guest" else "oci"
        demand = f"cpus={cpu}m,mem={memory}b,class:{cls},linux/{architecture}"
        spec = {"id": label, "bounds": {"cpu_max_milli": cpu, "memory_max": memory},
                "class": {"oci": {"rootfs": {"dir": str(self.rootfs)},
                                   "init": {"args": ["/hex", "selftest-box", "wait"], "env": [], "cwd": None, "user": None},
                                   "mounts": [], "hostname": None, "network": "private", "hosts": []}}, "owner": None}
        reply = self.call(socket_path, {"Submit": {"demand": demand, "payload": {"spec": spec, "start": True}}})
        require(reply.get("key") is not None, "submit did not return its real durable key")
        return reply["key"]

    def one_box(self, socket_path, label, target, link=False):
        key = self.submit(socket_path, label)
        ready = self.status(socket_path, key, lambda row: row.get("created"))
        require(ready.get("at") == target, "workload reached an unexpected real endpoint")
        physical = ready.get("physical")
        require(isinstance(physical, str) and physical, "ready status omitted its physical box ID")
        live = self.snapshot(inspect=physical)
        require([box["id"] for box in live["boxes"]] == [physical], "real endpoint does not hold exactly the requested physical box")
        require(str(live["inspected"]["state"]).lower() == "running", "actual physical init is not running")
        self.verified_created += 1
        if link:
            self.call(socket_path, {"LinkUp": {"peer": target}})
        self.call(socket_path, "Retry")
        self.status(socket_path, key, lambda row: row.get("created"))
        self.call(socket_path, {"Delete": {"key": key}})
        self.status(socket_path, key, lambda row: row.get("ended") or not row.get("tracked", True))
        require(self.snapshot()["boxes"] == self.baseline, "actual endpoint retains a box after scheduler deletion")
        self.verified_deleted += 1
        record(self.runtime, "verified-physical-lifecycle", key=key, physical=physical, phase=self.phase)

    def exercise(self):
        self.setup_ssh()
        marker = self.source / "SOURCE_REVISION"
        self.report["source_revision"] = marker.read_text().strip() if marker.is_file() else self.command("source-revision", ["git", "-C", str(self.source), "rev-parse", "HEAD"]).decode().strip()
        self.report["hex_version"] = self.command("hex-version", [self.runtime["hex"], "--version"]).decode().strip()
        self.runtime["source_revision"] = self.report["source_revision"]
        self.runtime["hex_sha256"] = self.report["binary_and_artifact_hashes"]["hex"]["sha256"]
        atomic_json(self.runtime_path, self.runtime)
        atomic_json(self.root / "provenance.json", self.report)
        backend = self.backend("real")
        cls = "shards" if self.mode == "guest" else "oci"
        self.phase = "local"
        local, socket_path, _ = self.scheduler("l", 0, [{"class": cls, "socket": str(backend)}], [], self.root.name + "-local")
        for index in range(self.args.n):
            self.one_box(socket_path, f"local-{index}", 0)
        self.stop(local)
        self.phase = "remote"
        realm = self.root.name + "-peer"
        route0 = [{"node": 1, "host": "fixture-1", "command": "forced-tunnel"}]
        route1 = [{"node": 0, "host": "fixture-0", "command": "forced-tunnel"}]
        remote, remote_socket, _ = self.scheduler("r", 1, [{"class": cls, "socket": str(backend)}], route1, realm)
        owner, owner_socket, _ = self.scheduler("o", 0, [], route0, realm)
        for index in range(self.args.n):
            # Both offers come from actual live Describe replies, never synthetic capacity facts.
            actual_remote = self.call(remote_socket, {"Describe": {"from": 0, "cluster": realm}})
            actual_owner = self.call(owner_socket, {"Describe": {"from": 1, "cluster": realm}})
            require(actual_remote.get("offer") and actual_owner.get("offer"), "live Describe omitted peer facts")
            self.call(owner_socket, {"Advertise": {"from": 1, "cluster": realm,
                       "offer": actual_remote["offer"], "observations": actual_remote["observations"]}})
            self.call(remote_socket, {"Advertise": {"from": 0, "cluster": realm,
                       "offer": actual_owner["offer"], "observations": actual_owner["observations"]}})
            self.one_box(owner_socket, f"remote-{index}", 1, link=True)
        self.stop(owner)
        self.stop(remote)
        require(self.snapshot()["boxes"] == self.baseline, "physical cleanup baseline was not restored")
        self.report["verified_physical_created"] = self.verified_created
        self.report["verified_physical_deleted"] = self.verified_deleted
        self.report["backend_baseline_restored"] = True
        self.report["backend_eager_boot"] = self.mode == "guest"
        self.report["success_controls"] = self.success_controls
        require(self.verified_created == self.args.n * 2 and self.verified_deleted == self.args.n * 2,
                "not all local and remote real lifecycles completed")

    def finish_backend(self):
        if self.backend_process is None:
            return
        # Fresh private state and an empty initial list establish this endpoint's exclusive scope.
        # On a failed trial, clean only the boxes actually returned by that owned endpoint.
        for process in self.scheduler_processes:
            self.stop(process)
        snapshot = self.snapshot()
        for box in snapshot["boxes"]:
            self.probe("--socket", str(self.backend_socket), "--delete", box["id"])
        require(self.snapshot()["boxes"] == self.baseline, "actual physical cleanup failed")
        self.stop(self.backend_process)
        self.backend_process = None
        if self.mode == "guest":
            require(self.guest_receipt is not None, "guest identity receipt missing; retirement is owed")
            stopped = self.probe("--stop-owned-guest", str(self.backend_state / "@guest"),
                                 "--owned-root", str(self.root), "--receipt", str(self.guest_receipt),
                                 "--wait-nanos", str(self.runtime["wait_ns"]))
            require(stopped.get("guest_stopped"), "owned guest retirement was not proved")
            self.report["owned_guest_retired"] = stopped

    def census_report(self):
        counts, per_process, cut, malformed = {}, [], [], []
        ledger_path = Path(self.runtime["ledger"])
        ledger = [] if not ledger_path.exists() else [json.loads(line) for line in ledger_path.read_text().splitlines()]
        producers = {Path(row["census"]).name: row for row in ledger if row.get("census") and row["kind"] not in ("census-call-exit", "census-process-exit")}
        outcomes = {Path(row["census"]).name: row for row in ledger if row["kind"] == "census-call-exit" and row.get("census")}
        process_outcomes = {Path(row["census"]).name: row for row in ledger if row["kind"] == "census-process-exit" and row.get("census")}
        successful_cli_counts, successful_process_counts = {}, {}
        for path in sorted((self.root / "census").glob("*.tsv")):
            rows, answers, ran_out = 0, {}, {}
            with path.open(encoding="utf-8") as source:
                while True:
                    line = source.readline(self.runtime["max_frame"] + 1)
                    if not line:
                        break
                    rows += 1
                    if len(line.encode()) > self.runtime["max_frame"] or rows > self.args.max_census_lines + 1:
                        malformed.append(path.name)
                        break
                    if line.startswith("census full at "):
                        cut.append(path.name)
                        continue
                    match = re.fullmatch(r"(\S+) (>?)\s*(\d+)\n?", line)
                    if match is None:
                        malformed.append(path.name)
                        continue
                    name, timed_out, _ = match.groups()
                    target = ran_out if timed_out else answers
                    target[name] = target.get(name, 0) + 1
                    if not timed_out:
                        counts[name] = counts.get(name, 0) + 1
            producer = producers.get(path.name, {})
            outcome = outcomes.get(path.name, {})
            process_outcome = process_outcomes.get(path.name, {})
            if process_outcome.get("success"):
                for name, count in answers.items():
                    successful_process_counts[name] = successful_process_counts.get(name, 0) + count
            if producer.get("kind") == "call" and outcome.get("success"):
                for name, count in answers.items():
                    successful_cli_counts[name] = successful_cli_counts.get(name, 0) + count
            per_process.append({"file": path.name, "rows": rows, "answers": answers, "ran_out": ran_out,
                                "producer": producer, "call_outcome": outcome, "process_outcome": process_outcome,
                                "sha256": digest_file(path, self.runtime["max_frame"])})
        missing = {kind: counts.get("client.answer.schedule." + kind, 0) for kind in self.wait_kinds
                   if counts.get("client.answer.schedule." + kind, 0) < self.args.n}
        single_call_missing = {kind: successful_cli_counts.get("client.answer.schedule." + kind, 0) for kind in self.wait_kinds if not kind.startswith("hear_") and successful_cli_counts.get("client.answer.schedule." + kind, 0) < self.args.n}
        self.report["census"] = {"per_process": per_process, "answer_rows_by_name": counts, "successful_single_call_rows": successful_cli_counts,
                                 "successful_process_rows": successful_process_counts,
                                 "required_raw_answer_rows_per_kind": self.args.n,
                                 "required_verified_successful_trials": self.args.n,
                                 "missing_successful_single_call_rows": single_call_missing,
                                 "missing_row_targets": missing, "truncated_files": cut, "malformed_files": malformed,
                                 "raw_multiplexed_rows_may_include_refusals": True,
                                 "seed_generation": "not performed; coordinator must review success/attempt provenance"}
        require(not cut and not malformed and not missing and not single_call_missing, "census is cut, malformed, or lacks successful controls; see raw artifacts")

    def cleanup(self):
        self.stop_accepting.set()
        if self.listener is not None:
            try:
                self.listener.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            self.listener.close()
        if self.acceptor is not None:
            self.acceptor.join(timeout=self.runtime["wait"])
            require(not self.acceptor.is_alive(), "temporary SSH acceptor did not stop")
        with self.children_lock:
            children = list(self.children)
        for _, process in reversed(children):
            self.stop(process)
        # Only recorded descendant PIDs are considered; a unique private-run path in their exact
        # PID's command additionally fences PID reuse. Never scan processes by a name/pattern.
        ledger = Path(self.runtime["ledger"])
        recorded = [] if not ledger.exists() else [json.loads(line) for line in ledger.read_text().splitlines()]
        for row in reversed(recorded):
            if row["kind"] not in ("ssh-shim", "forced-wrapper", "forced-tunnel"):
                continue
            pid = row["pid"]
            deadline = time.monotonic() + self.runtime["wait"]
            signalled = False
            while True:
                query = self.spawn("pid-check", ["/bin/ps", "-ww", "-p", str(pid), "-o", "command="], stdout=subprocess.PIPE)
                try:
                    command, _ = query.communicate(timeout=remaining(deadline))
                except subprocess.TimeoutExpired:
                    self.stop(query)
                    raise Failed("recorded descendant did not settle before the derived deadline")
                if str(self.root).encode() not in command:
                    break
                try:
                    os.kill(pid, signal.SIGTERM if not signalled else signal.SIGKILL)
                    record(self.runtime, "signalled-owned-descendant", pid)
                except ProcessLookupError:
                    break
                signalled = True
                # The next iteration waits on this exact PID's actual process-table result.
        require(self.accept_fault is None, "temporary SSH acceptor failed")
        for key in self.keys:
            key.unlink(missing_ok=True)


def main():
    os.umask(0o077)
    if len(sys.argv) > 1 and sys.argv[1] == "_shim":
        shim()
        return
    if len(sys.argv) > 1 and sys.argv[1] == "_forced":
        forced(sys.argv[2], int(sys.argv[3]), sys.argv[4])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hex", required=True, help="already-built current Hex CLI")
    parser.add_argument("--runtime-client", required=True, help="already-built actual Runtime probe fixture")
    parser.add_argument("--mode", choices=("native", "guest"), required=True)
    parser.add_argument("--profile", choices=("debug", "release"), required=True)
    parser.add_argument("--n", type=int, required=True, help="operator's successful trials per local and remote phase")
    parser.add_argument("--max-census-lines", type=int, required=True)
    parser.add_argument("--cpu-milli", type=int, required=True)
    parser.add_argument("--box-memory-mib", type=int, required=True)
    parser.add_argument("--deadline-seconds", type=float, required=True, help="operator outer cancellation budget, not a runtime seed")
    parser.add_argument("--deadline-evidence", required=True)
    parser.add_argument("--policy-receipt", required=True)
    parser.add_argument("--build-receipt", required=True)
    parser.add_argument("--linux-hex", required=True, help="already-built static Linux hex of the native architecture")
    parser.add_argument("--cgroup-parent", help="native Linux's explicitly delegated parent, never a shared tenant root")
    parser.add_argument("--shards-bin", default=os.environ.get("SHARDS_BIN"))
    parser.add_argument("--kernel", default=os.environ.get("HEX_GUEST_KERNEL"))
    parser.add_argument("--initrd", default=os.environ.get("HEX_GUEST_INITRD"))
    parser.add_argument("--guest-cpus", type=int, default=None if "HEX_GUEST_CPUS" not in os.environ else int(os.environ["HEX_GUEST_CPUS"]))
    parser.add_argument("--guest-memory-mib", type=int, default=None if "HEX_GUEST_MEMORY_MIB" not in os.environ else int(os.environ["HEX_GUEST_MEMORY_MIB"]))
    parser.add_argument("--guest-shares", default=os.environ.get("HEX_GUEST_SHARES"))
    parser.add_argument("--source", required=True, help="current Hex source, for existing ABI/wait constants and policy hash")
    parser.add_argument("--sshd", help="installed native sshd absolute path")
    parser.add_argument("--run-parent", default=str(Path.home() / ".cache/hex-lead/census-runs"))
    args = parser.parse_args()
    require(args.n > 0 and 0 < args.max_census_lines <= (1 << 64) - 1, "N and per-process row budget must be positive representable integers")
    require(0 < args.cpu_milli <= (1 << 32) - 1 and 0 < args.box_memory_mib * (1 << 20) <= (1 << 64) - 1, "box resources must fit the actual API types")
    require(math.isfinite(args.deadline_seconds) and 0 < args.deadline_seconds * 1_000_000_000 <= (1 << 64) - 1, "outer deadline must be finite, positive and representable")
    if args.mode == "guest":
        require(args.guest_cpus is not None and 0 < args.guest_cpus <= (1 << 32) - 1 and args.guest_memory_mib is not None and args.guest_memory_mib > 0, "real guest CPU/memory must be explicitly supplied")
    harness = Harness(args)
    def interrupted(number, _frame):
        raise Failed(f"harness interrupted by signal {number}")
    for number in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(number, interrupted)
    try:
        harness.exercise()
        harness.report["status"] = "passed"
    except Exception as why:
        harness.report["status"] = "failed"
        harness.report["error"] = str(why)
    finally:
        try:
            harness.finish_backend()
        except Exception as why:
            harness.report["status"] = "failed"
            harness.report["backend_cleanup_error"] = str(why)
        try:
            harness.cleanup()
            harness.census_report()
        except Exception as why:
            harness.report["status"] = "failed"
            harness.report["cleanup_error"] = str(why)
        finally:
            for key in harness.keys:
                key.unlink(missing_ok=True)
        atomic_json(harness.root / "result.json", harness.report)
    print("result " + str(harness.root / "result.json"))
    if harness.report["status"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
