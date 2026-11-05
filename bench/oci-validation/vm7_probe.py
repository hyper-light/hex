# Whether the running kernel takes goal 6's two calls (docs/research/runtime/goal5-root.md §5), run as root:
# - open_tree's OPEN_TREE_NAMESPACE (Linux 7.0, 1 << 1), by its errors alone: with no directory (-1) and an empty
#   path, a kernel that knows the flag refuses the directory (EBADF), one that doesn't refuses the flag (EINVAL)
#   before it looks at the directory. An unknown bit (0x40) is the control: it must give EINVAL, or the first
#   answer says nothing.
# - clone3's CLONE_EMPTY_MNTNS (Linux 7.1, 1 << 37, implying CLONE_NEWNS): a child cloned with it, which exits at
#   once; a kernel that doesn't know the bit refuses it (EINVAL).
import ctypes
import errno
import os
import signal
import struct

SYS_OPEN_TREE = 428  # x86_64 and aarch64 alike (the unified syscall table, Linux 5.2)
SYS_CLONE3 = 435  # likewise (Linux 5.3)
AT_EMPTY_PATH = 0x1000
OPEN_TREE_NAMESPACE = 1 << 1
CLONE_EMPTY_MNTNS = 1 << 37
libc = ctypes.CDLL(None, use_errno=True)
libc.syscall.restype = ctypes.c_long


def err():
    e = ctypes.get_errno()
    return errno.errorcode.get(e, str(e))


def open_tree_answer(flags):
    r = libc.syscall(ctypes.c_long(SYS_OPEN_TREE), ctypes.c_int(-1), b"", ctypes.c_uint(AT_EMPTY_PATH | flags))
    if r >= 0:
        os.close(r)
        return "ok"
    return err()


def clone3_answer(flags):
    # struct clone_args, version 0: flags, pidfd, child_tid, parent_tid, exit_signal, stack, stack_size, tls.
    args = ctypes.create_string_buffer(struct.pack("8Q", flags, 0, 0, 0, signal.SIGCHLD, 0, 0, 0))
    r = libc.syscall(ctypes.c_long(SYS_CLONE3), args, ctypes.c_size_t(64))
    if r == 0:
        os._exit(0)
    if r < 0:
        return err()
    _, status = os.waitpid(r, 0)
    return "ok" if os.waitstatus_to_exitcode(status) == 0 else f"child status {status}"


ns = open_tree_answer(OPEN_TREE_NAMESPACE)
control = open_tree_answer(0x40)
verdict = "supported" if ns == "EBADF" and control == "EINVAL" else "NOT supported"
print(f"probe: open_tree(-1, \"\", AT_EMPTY_PATH|OPEN_TREE_NAMESPACE) -> {ns}; unknown bit 0x40 -> {control}: "
      f"OPEN_TREE_NAMESPACE {verdict}")
empty = clone3_answer(CLONE_EMPTY_MNTNS)
print(f"probe: clone3(CLONE_EMPTY_MNTNS) -> {empty}: CLONE_EMPTY_MNTNS {'supported' if empty == 'ok' else 'NOT supported'}")
