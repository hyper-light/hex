"""Boots the guest image through Shards and proves it ready (docs/design/shards-guest.md §2): a
CONNECT to the relay's control port answered OK, and the relay's first bytes on it, the
connection's token (hex_core::stream::TOKEN_BYTES). Reads SHARDS_BIN, HEX_GUEST_KERNEL and
HEX_GUEST_INITRD; retries the handshake with backoff until the deadline, as the guest client does,
and fails when the VM process ends first.

    python3 -I scripts/guest/boot_check.py WORKDIR [--console] [--deadline SECONDS]
"""

import os
import socket
import subprocess
import sys
import time

CONTROL_PORT = 0x4845_5800  # hex_core::stream::CONTROL_PORT
TOKEN_BYTES = 8
CMDLINE = "console=ttyS0 panic=-1 -- guest init"


def read_exact(conn, n):
    got = b""
    while len(got) < n:
        chunk = conn.recv(n - len(got))
        if not chunk:
            raise ConnectionError(f"closed after {len(got)} of {n} bytes")
        got += chunk
    return got


def handshake(path, patience):
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    conn.settimeout(patience)
    try:
        conn.connect(path)
        conn.sendall(b"CONNECT %d\n" % CONTROL_PORT)
        line = b""
        while not line.endswith(b"\n") and len(line) < 64:
            chunk = conn.recv(1)
            if not chunk:
                raise ConnectionError(f"closed after {line!r}")
            line += chunk
        if not line.startswith(b"OK "):
            raise ConnectionError(f"answered {line!r}")
        return line.strip().decode(), read_exact(conn, TOKEN_BYTES)
    finally:
        conn.close()


def main(argv):
    if not argv:
        sys.exit("usage: boot_check.py WORKDIR [--console] [--deadline SECONDS]")
    work = argv[0]
    console = "--console" in argv
    deadline_s = float(argv[argv.index("--deadline") + 1]) if "--deadline" in argv else 60.0
    shards, kernel, initrd = (os.environ[k] for k in ("SHARDS_BIN", "HEX_GUEST_KERNEL", "HEX_GUEST_INITRD"))
    os.makedirs(work, exist_ok=True)
    vsock = os.path.join(work, "vsock.sock")
    if os.path.exists(vsock):
        os.unlink(vsock)
    args = [shards, "run", "vm", "--kernel", kernel, "--initrd", initrd, "--vsock", vsock,
            "--cpus", "2", "--memory", "1024", "--cmdline", CMDLINE]
    if not console:
        args.append("--no-console")
    print("running:", " ".join(args), flush=True)
    log = open(os.path.join(work, "vm.log"), "wb")
    start = time.monotonic()
    vm = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    wait, last = 0.01, None
    try:
        while time.monotonic() - start < deadline_s:
            if vm.poll() is not None:
                print(f"FAIL: the VM ended first, exit {vm.returncode}; last: {last}", flush=True)
                return 1
            try:
                ok, token = handshake(vsock, 5.0)
                print(f"READY after {time.monotonic() - start:.3f} s: {ok}, token {token.hex()}", flush=True)
                return 0
            except (OSError, ConnectionError) as e:
                last = e
            time.sleep(wait)
            wait = min(wait * 2, 0.5)
        print(f"FAIL: no answer within {deadline_s} s; last: {last}", flush=True)
        return 1
    finally:
        vm.terminate()
        try:
            vm.wait(10)
        except subprocess.TimeoutExpired:
            vm.kill()
            vm.wait()
        print(f"the VM ended: exit {vm.returncode}", flush=True)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
