#!/usr/bin/env python3
"""ROOT-only macOS filesystem notifications for the private Creating observer."""

import argparse
import os
from pathlib import Path
import select
import sys
import time

from evidence import InvalidEvidence, finite_positive, require


def watch(args):
    require(args.root_authorized and sys.platform == "darwin", "ROOT macOS notification adapter required")
    require(finite_positive(args.timeout) and args.entry_limit > 0 and args.event_limit > 0,
            "explicit finite notification bounds required")
    root = args.root.resolve(strict=True)
    require(root.is_dir() and not args.root.is_symlink(), "private canonical Store root required")
    until = time.monotonic() + args.timeout
    held = {}
    with select.kqueue() as events:
        def add(path):
            if path in held:
                return
            require(len(held) < args.entry_limit, "Creating notification watch budget exceeded")
            fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
            try:
                change = select.kevent(fd, filter=select.KQ_FILTER_VNODE,
                    flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR,
                    fflags=select.KQ_NOTE_WRITE | select.KQ_NOTE_DELETE | select.KQ_NOTE_RENAME | select.KQ_NOTE_REVOKE)
                events.control([change], 0, 0)
            except BaseException:
                os.close(fd)
                raise
            held[path] = fd

        def refresh():
            add(root)
            count = 0
            for path in root.iterdir():
                count += 1
                require(count <= args.entry_limit, "Creating directory-entry budget exceeded")
                if not path.is_symlink() and path.is_dir():
                    add(path)
        try:
            refresh()
            print("hex-completion-creating-ready", flush=True)
            count = 0
            while True:
                left = until - time.monotonic()
                if left <= 0:
                    return 0
                changes = events.control(None, args.event_limit, left)
                for change in changes:
                    count += 1
                    require(count <= args.event_limit, "Creating notification event budget exceeded")
                    require(not change.flags & select.KQ_EV_ERROR, "Creating kqueue observation error")
                    require(not change.fflags & (select.KQ_NOTE_DELETE | select.KQ_NOTE_RENAME | select.KQ_NOTE_REVOKE),
                            "Creating watched directory identity changed")
                if changes:
                    refresh()
                    print("hex-completion-creating-changed", flush=True)
        finally:
            for fd in held.values():
                os.close(fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized", action="store_true")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--entry-limit", type=int, required=True)
    parser.add_argument("--event-limit", type=int, required=True)
    return watch(parser.parse_args())


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (InvalidEvidence, OSError) as error:
        print(f"UNVERIFIED Creating notifications: {error}", file=sys.stderr)
        sys.exit(1)
