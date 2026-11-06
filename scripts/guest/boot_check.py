"""Boots the guest image through Shards and proves it ready (docs/design/shards-guest.md §2): init's
`hex guest ready` on the console, then a CONNECT to the relay's control port answered OK, and the
relay's first bytes on it, the connection's token (hex_core::stream::TOKEN_BYTES). Reads SHARDS_BIN,
HEX_GUEST_KERNEL and HEX_GUEST_INITRD. Every wait is on a fact, never a clock (CLAUDE.md §4): the
console's line or the VM's end, which a guest that can't start brings (init powers off, and a panic
reboots at once, `panic=-1`).

    python3 -I scripts/guest/boot_check.py WORKDIR [--console] [--share DIR[:ro]]...
        [--cmdline-extra WORDS]

The console is kept in WORKDIR/vm.log; --console echoes it too. Each --share is given to Shards and
named on the command line (`hex.shares=`), so READY proves init mounted it: a share init can't mount
ends the guest. --cmdline-extra adds kernel parameters, as a `hex.shares=` naming a share no device
backs, which must end the guest.
"""

import os
import platform
import socket
import subprocess
import sys
import time

# The longest answer to a CONNECT: `OK <port>\n`, the port a u32 in decimal (firecracker
# docs/vsock.md, "Host-Initiated Connections").
OK_BYTES = len(b"OK 4294967295\n")
READY = b"hex guest ready"

# The control port and token width, as docs/design/shards-guest.md §3a assigns them (hex_core::stream).
CONTROL_PORT = 0x4845_5800
TOKEN_BYTES = 8
# hex_runtime::guest's: `panic=-1` reboots at once on a panic (kernel-parameters.txt, `panic=`).
CMDLINE = "console=ttyS0 panic=-1"


def read_exact(conn, n):
    got = b""
    while len(got) < n:
        chunk = conn.recv(n - len(got))
        if not chunk:
            raise ConnectionError(f"closed after {len(got)} of {n} bytes")
        got += chunk
    return got


def handshake(path):
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        conn.connect(path)
        conn.sendall(b"CONNECT %d\n" % CONTROL_PORT)
        line = b""
        while not line.endswith(b"\n") and len(line) < OK_BYTES:
            chunk = conn.recv(1)
            if not chunk:
                raise ConnectionError(f"closed after {line!r}")
            line += chunk
        if not line.startswith(b"OK "):
            raise ConnectionError(f"answered {line!r}")
        return line.strip().decode(), read_exact(conn, TOKEN_BYTES)
    finally:
        conn.close()


# HEX-GUEST-19's guest VM peak resident set, the most of three runs, in MiB (a boot is part of its workload;
# bench/native/guest/peak-rss/RECORD.md), and Shards' least guest by architecture (shards_api::min_memory_mib).
PEAK_MIB = 284


def guest_memory_mib():
    """Shards' least guest and twice the measured peak, as hex-cli's guest_real takes."""
    least = 64 if platform.machine() in ("arm64", "aarch64") else 128
    return least + 2 * PEAK_MIB


def values(argv, flag):
    return [argv[i + 1] for i, word in enumerate(argv) if word == flag]


def main(argv):
    if not argv:
        sys.exit(__doc__)
    work = argv[0]
    console = "--console" in argv
    shares = values(argv, "--share")
    cmdline = " ".join([CMDLINE] + values(argv, "--cmdline-extra"))
    if shares:
        modes = ",".join("ro" if share.endswith(":ro") else "rw" for share in shares)
        cmdline += f" hex.shares={modes}"
    shards, kernel, initrd = (os.environ[k] for k in ("SHARDS_BIN", "HEX_GUEST_KERNEL", "HEX_GUEST_INITRD"))
    os.makedirs(work, exist_ok=True)
    vsock = os.path.join(work, "vsock.sock")
    if os.path.exists(vsock):
        os.unlink(vsock)
    args = [shards, "run", "vm", "--kernel", kernel, "--initrd", initrd, "--vsock", vsock,
            "--cpus", str(os.cpu_count()), "--memory", str(guest_memory_mib()), "--cmdline", cmdline]
    for share in shares:
        args += ["--share", share]
    print("running:", " ".join(args), flush=True)
    log = open(os.path.join(work, "vm.log"), "wb")
    start = time.monotonic()
    vm = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    try:
        ready = False
        for line in vm.stdout:
            log.write(line)
            log.flush()
            if console:
                sys.stdout.buffer.write(line)
                sys.stdout.flush()
            if line.strip() == READY:
                ready = True
                break
        if not ready:
            print(f"FAIL: the VM ended first, exit {vm.wait()}", flush=True)
            return 1
        try:
            ok, token = handshake(vsock)
        except (OSError, ConnectionError) as e:
            print(f"FAIL: init said it is ready, but the relay answered: {e}", flush=True)
            return 1
        print(f"READY after {time.monotonic() - start:.3f} s: {ok}, token {token.hex()}", flush=True)
        return 0
    finally:
        vm.terminate()
        # Shards' own grace for a run's stop (crates/shards/src/daemon.rs:231, `STOP_GRACE`), then
        # SIGKILL; the console is drained meanwhile so the VM never blocks writing it.
        try:
            rest, _ = vm.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            vm.kill()
            rest, _ = vm.communicate()
        log.write(rest or b"")
        log.close()
        print(f"the VM ended: exit {vm.returncode}", flush=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
