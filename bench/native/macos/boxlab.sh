#!/bin/bash
# The macOS box-user root lab (native plan step 2). Run as root on the Mac Studio; it builds boxuser_probe,
# then measures the privileged mechanisms the step's acceptance criteria need:
#   - the box user's membership: a transient pool user is made (dscl), N members run as it with one that
#     daemonizes (setsid), proc_listpids(PROC_UID_ONLY) must find all, RLIMIT_NPROC must cap the forks, and
#     a kill-all must empty the user; then the user is removed (criterion 1);
#   - the ledger bound: root arming a running task (priv), a fatal cap (cap fatal), the high-watermark
#     EXC_RESOURCE notification (cap soft), and the elastic box total across 1..28 members and a fork burst
#     (box ... steady|burst) (criteria 3-5).
# Root on the Mac Studio is Ada's to grant; this script never asks for or prints a password. It cleans up
# the transient user even on failure. Output is teed to a record under ~/.cache/hex-t7/native-s2.
#
# Usage:  sudo bash bench/native/macos/boxlab.sh [N]       # N = members for the membership test (default 6)
set -u

if [ "$(id -u)" -ne 0 ]; then
    echo "boxlab: must run as root (sudo bash $0)" >&2
    exit 2
fi

N="${1:-6}"
DIR="$HOME/.cache/hex-t7/native-s2"
# $HOME under sudo is root's; put the record where the probe is, under the invoking user's cache if known.
if [ -n "${SUDO_USER:-}" ]; then
    DIR="$(dscl . -read "/Users/$SUDO_USER" NFSHomeDirectory | awk '{print $2}')/.cache/hex-t7/native-s2"
fi
PROBE="$DIR/boxuser_probe"
SRC="$(cd "$(dirname "$0")" && pwd)/boxuser_probe.c"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
REC="$DIR/boxuser-root-$TS.txt"

mkdir -p "$DIR"
if ! cc -O2 -Wall -Wextra -o "$PROBE" "$SRC"; then
    echo "boxlab: build failed" >&2
    exit 1
fi

# A transient pool user for the membership test: a high uid unlikely to collide, no login, no home.
LABUSER="_hexboxlab"
LABUID=7000
cleanup() {
    dscl . -delete "/Users/$LABUSER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

make_user() {
    cleanup
    dscl . -create "/Users/$LABUSER" || return 1
    dscl . -create "/Users/$LABUSER" UserShell /usr/bin/false
    dscl . -create "/Users/$LABUSER" UniqueID "$LABUID"
    dscl . -create "/Users/$LABUSER" PrimaryGroupID 20
    dscl . -create "/Users/$LABUSER" NFSHomeDirectory /var/empty
    dscl . -create "/Users/$LABUSER" RealName "Hex box lab (transient)"
}

{
    echo "# boxuser_probe, ROOT, adas-mac-studio-3, macOS $(sw_vers -productVersion) $(sw_vers -buildVersion), $(uname -m), $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "# load1 $(sysctl -n vm.loadavg | awk '{print $2}'), cores $(sysctl -n hw.ncpu)"

    echo "## C1 membership: $N members as a transient pool user (uid $LABUID), one daemonized"
    if make_user; then
        "$PROBE" uid "$LABUID" "$N"

        echo "## box-user class end to end: the real helper daemon + HEX-RT conformance"
        # Build the hex binary (and the conformance test) into a root-owned target dir, so the lab needs
        # nothing of the user's tree; start the daemon as root on a socket with the transient pool user,
        # and run the box-user HEX-RT conformance (the node) against it.
        export CARGO_TARGET_DIR="$DIR/root-target"
        if cargo build -q -p hex-cli --bin hex; then
            SOCK="$DIR/helper.sock"
            "$CARGO_TARGET_DIR/debug/hex" __box-helper --socket "$SOCK" --users "$LABUID" --nproc 1024 &
            HELPER_PID=$!
            # Wait on the fact: the socket appears (a bounded poll, not a fixed sleep).
            for _ in $(seq 1 100); do [ -S "$SOCK" ] && break; sleep 0.05; done
            if [ -S "$SOCK" ]; then
                HEX_BOX_HELPER_SOCK="$SOCK" cargo test -q -p hex-cli --test macos_process_box \
                    -- --ignored box_user_boxes_hold_the_contract \
                    && echo "box-user HEX-RT conformance: PASS" \
                    || echo "box-user HEX-RT conformance: FAIL"
            else
                echo "the helper socket never appeared; skipping box-user conformance"
            fi
            kill "$HELPER_PID" 2>/dev/null || true
            wait "$HELPER_PID" 2>/dev/null || true
        else
            echo "building hex failed; skipping box-user conformance"
        fi

        cleanup
        echo "(transient user $LABUSER removed)"
    else
        echo "make_user failed; skipping membership test"
    fi

    echo "## priv: root arms task_set_phys_footprint_limit on a running task"
    "$PROBE" priv 64

    echo "## C3 fatal cap overshoot (confirms the rootless result holds as root)"
    for mb in 64 256; do "$PROBE" cap fatal "$mb"; done

    echo "## C4/notify: high-watermark EXC_RESOURCE (latency, once-per-limit, re-arm)"
    "$PROBE" cap soft 64

    echo "## C3/C5 elastic box total and fork burst, 1..28 members"
    for n in 1 4 8 14 28; do "$PROBE" box 256 "$n" steady; done
    for n in 14 28; do "$PROBE" box 256 "$n" burst; done
} 2>&1 | tee "$REC"

echo "=== RECORD $REC ==="
