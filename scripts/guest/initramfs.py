"""Writes the guest's initramfs: a newc cpio (the kernel's initramfs format,
Documentation/driver-api/early-userspace/buffer-format.rst) holding `/init`, the static `hex`,
`/dev/console`, which the kernel opens as init's stdin, stdout and stderr before anything mounts
/dev (init/main.c, console_on_rootfs), and `/dev/null`, which Rust's runtime opens for each of those it
finds closed before `main` (std's `sanitize_standard_fds`): with no console device the kernel opens none,
and without `/dev/null` init aborts before its first line, as PID 1 (g121, 2026-11-15). Nothing else:
`hex guest init` makes every other directory it mounts on. The same inputs give the same bytes: every
entry's time, owner and inode are fixed.

    python3 -I scripts/guest/initramfs.py HEX OUT
"""

import sys


def entry(ino, mode, name, data=b"", rdev=(0, 0)):
    name = name.encode() + b"\0"
    fields = [ino, mode, 0, 0, 1, 0, len(data), 0, 0, rdev[0], rdev[1], len(name), 0]
    head = b"070701" + b"".join(b"%08x" % f for f in fields) + name
    return head + b"\0" * (-len(head) % 4) + data + b"\0" * (-len(data) % 4)


def main(hex_path, out):
    with open(hex_path, "rb") as f:
        hex_bytes = f.read()
    parts = [
        # Modes as the kernel's own devtmpfs gives them (drivers/base/devtmpfs.c): its root "mode=0755",
        # and a device node no driver names a mode for 0600, root's alone, as the console's is.
        entry(1, 0o040755, "dev"),
        # A character device, 5:1, as devices.txt names /dev/console.
        entry(2, 0o020600, "dev/console", rdev=(5, 1)),
        # A character device, 1:3, as devices.txt names /dev/null, read and written by every user, as the
        # kernel's own memory devices give it (drivers/char/mem.c, `devlist`'s 0666).
        entry(3, 0o020666, "dev/null", rdev=(1, 3)),
        # Search and execute for every user (execve(2): EACCES without execute), written by root alone.
        entry(4, 0o100755, "init", hex_bytes),
        entry(0, 0, "TRAILER!!!"),
    ]
    with open(out, "wb") as f:
        f.write(b"".join(parts))


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: initramfs.py HEX OUT")
    main(sys.argv[1], sys.argv[2])
