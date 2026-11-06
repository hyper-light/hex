#!/bin/bash
# The macOS box-user root lab (native plan step 2). Run as root on the Mac Studio; it measures the
# privileged mechanisms step 2's acceptance criteria need and runs the box-user HEX-RT conformance against
# a real helper daemon:
#   - membership: a transient pool user is made (dscl), N members run as it with one that daemonizes
#     (setsid), proc_listpids(PROC_UID_ONLY) must find all, RLIMIT_NPROC must cap the forks, a kill-all
#     empties the user, then it is removed (criterion 1);
#   - the box-user class end to end: a real `hex __box-helper` daemon + the box-user HEX-RT conformance;
#   - the ledger bound: root arming a running task (priv), a fatal cap (cap fatal), the high-watermark
#     EXC_RESOURCE notification (cap soft), and the box total across 1..28 members and a fork burst
#     (box ... steady|burst) (criteria 3-5).
#
# Safety (reviewed by the lead): it never asks for or prints a password; it picks a FREE uid (no account,
# no live process) so the kill-all can't hit a real user; it BUILDS as the invoking user (never cargo as
# root) and runs only the built binaries as root; and on any exit it kills only the helper PID it started,
# removes the user and socket, and gives the record back to the invoking user. A watchdog bounds the run.
#
# Usage:  sudo bash bench/native/macos/boxlab.sh [N]      # N = members for the membership test (default 6)
set -u

if [ "$(id -u)" -ne 0 ]; then
    echo "boxlab: must run as root (sudo bash $0)" >&2
    exit 2
fi
BUILDUSER="${SUDO_USER:-}"
if [ -z "$BUILDUSER" ] || [ "$BUILDUSER" = "root" ]; then
    echo "boxlab: run via sudo as a normal user, so builds run as you and not root" >&2
    exit 2
fi

N="${1:-6}"
HOMEDIR="$(dscl . -read "/Users/$BUILDUSER" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
[ -z "$HOMEDIR" ] && { echo "boxlab: can't find $BUILDUSER's home" >&2; exit 2; }
SRCDIR="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$SRCDIR/../../.." && pwd)"
DIR="$HOMEDIR/.cache/hex-t7/native-s2"
USERTARGET="$DIR/user-target"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
REC="$DIR/boxuser-root-$TS.txt"
LABUSER="_hexboxlab"
SOCK="$DIR/helper.sock"

run_user() { sudo -u "$BUILDUSER" -H "$@"; }

# Is uid $1 taken by an account or any live process?
uid_taken() {
    dscl . -list /Users UniqueID | awk '{print $2}' | grep -qx "$1" && return 0
    ps -axo uid= 2>/dev/null | tr -d ' ' | grep -qx "$1" && return 0
    return 1
}

# Pick the first free uid in 7000-7099.
LABUID=""
for u in $(seq 7000 7099); do
    if ! uid_taken "$u"; then LABUID="$u"; break; fi
done
[ -z "$LABUID" ] && { echo "boxlab: no free uid in 7000-7099" >&2; exit 1; }

HELPER_PID=""
WATCHDOG_PID=""
cleanup() {
    [ -n "$HELPER_PID" ] && kill "$HELPER_PID" 2>/dev/null
    [ -n "$HELPER_PID" ] && wait "$HELPER_PID" 2>/dev/null
    [ -n "$WATCHDOG_PID" ] && kill "$WATCHDOG_PID" 2>/dev/null
    rm -f "$SOCK" 2>/dev/null
    dscl . -delete "/Users/$LABUSER" >/dev/null 2>&1 || true
    chown -R "$BUILDUSER" "$DIR" 2>/dev/null || true
}
trap cleanup EXIT TERM INT

# Watchdog: terminate this script (its EXIT trap cleans up) if the whole run overruns.
MAINPID=$$
WATCHDOG_SECS=900
( sleep "$WATCHDOG_SECS"; echo "boxlab: watchdog fired after ${WATCHDOG_SECS}s" >&2; kill -TERM "$MAINPID" 2>/dev/null ) &
WATCHDOG_PID=$!

make_user() {
    dscl . -delete "/Users/$LABUSER" >/dev/null 2>&1 || true
    dscl . -create "/Users/$LABUSER" || return 1
    dscl . -create "/Users/$LABUSER" UserShell /usr/bin/false
    dscl . -create "/Users/$LABUSER" UniqueID "$LABUID"
    dscl . -create "/Users/$LABUSER" PrimaryGroupID 20
    dscl . -create "/Users/$LABUSER" NFSHomeDirectory /var/empty
    dscl . -create "/Users/$LABUSER" RealName "Hex box lab (transient)"
    # Re-check nothing else holds the uid now (before any kill-all runs against it).
    if ps -axo uid= 2>/dev/null | tr -d ' ' | grep -qx "$LABUID"; then
        echo "boxlab: a process already runs at uid $LABUID; aborting" >&2
        return 1
    fi
}

echo "boxlab PLAN:"
echo "  pool user $LABUSER at uid $LABUID (chosen free in 7000-7099)"
echo "  build as $BUILDUSER (never root): boxuser_probe, hex, the box-user conformance test"
echo "  run as root: membership probe; a real 'hex __box-helper' + box-user HEX-RT conformance; priv/cap/box measurements"
echo "  record -> $REC ; cleans up user/socket/ownership on exit; watchdog ${WATCHDOG_SECS}s"
echo

run_user mkdir -p "$DIR" || { echo "boxlab: can't make $DIR" >&2; exit 1; }
run_user cc -O2 -Wall -Wextra -o "$DIR/boxuser_probe" "$SRCDIR/boxuser_probe.c" \
    || { echo "boxlab: probe build failed" >&2; exit 1; }
PROBE="$DIR/boxuser_probe"
run_user env CARGO_TARGET_DIR="$USERTARGET" cargo build -q --manifest-path "$REPO/Cargo.toml" -p hex-cli --bin hex \
    || { echo "boxlab: hex build failed" >&2; exit 1; }
HEXBIN="$USERTARGET/debug/hex"
run_user env CARGO_TARGET_DIR="$USERTARGET" cargo test -q --manifest-path "$REPO/Cargo.toml" -p hex-cli --test macos_process_box --no-run \
    || { echo "boxlab: conformance-test build failed" >&2; exit 1; }
# The newest of the test's executables: the target keeps every earlier build's beside the one just made.
TESTBIN="$(find "$USERTARGET/debug/deps" -maxdepth 1 -name 'macos_process_box-*' -type f -perm +111 -print0 2>/dev/null \
    | xargs -0 ls -t 2>/dev/null | head -1)"

{
    echo "# boxlab, ROOT, adas-mac-studio-3, macOS $(sw_vers -productVersion) $(sw_vers -buildVersion), $(uname -m), $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "# load1 $(sysctl -n vm.loadavg | awk '{print $2}'), cores $(sysctl -n hw.ncpu), pool uid $LABUID"

    echo "## C1 membership: $N members as the pool user (uid $LABUID), one daemonized"
    if make_user; then
        "$PROBE" uid "$LABUID" "$N"

        # What derives the helper's reserve and its drain bound, recorded in this one run (each the max of
        # K=19 trials: a 20th draw exceeds the max of 19 with probability 1/20, so the max covers 95%).
        echo "## derive RESERVE_BYTES: the fork window, member fork to child armed (x2 the max growth)"
        "$PROBE" armlat "$LABUID" 19
        echo "## derive DRAIN_PATIENCE: kill-to-empty for the box sizes the box probe runs (x2 the max)"
        for n in 1 4 8 14 28; do "$PROBE" drain "$LABUID" "$n" 19; done

        echo "## box-user class end to end: a real 'hex __box-helper' + box-user HEX-RT conformance + helper CPU"
        "$HEXBIN" __box-helper --socket "$SOCK" --users "$LABUID" &  # its process cap: kern.maxprocperuid
        HELPER_PID=$!
        for _ in $(seq 1 100); do [ -S "$SOCK" ] && break; sleep 0.05; done
        if [ -S "$SOCK" ] && [ -n "$TESTBIN" ]; then
            # Helper CPU at rest, no box: the event-driven daemon blocked on mach_msg plus its
            # defense-in-depth pump poll (criterion: helper CPU <= 0.1% of a core).
            if r0="$("$PROBE" cputime "$HELPER_PID")" && sleep 2 && r1="$("$PROBE" cputime "$HELPER_PID")"; then
                echo "$r0 $r1" | awk '{dw=$4-$2; if(dw>0) printf "helper daemon CPU at rest (no box, 2 s): %.3f%% of a core\n",($3-$1)/dw*100}'
            fi
            # Helper CPU across the conformance run (acquire/arm/start/inspect/signal/delete).
            c0="$("$PROBE" cputime "$HELPER_PID")" || c0=""
            if HEX_BOX_HELPER_SOCK="$SOCK" "$TESTBIN" --exact --ignored box_user_boxes_hold_the_contract --nocapture; then
                echo "box-user HEX-RT conformance: PASS"
            else
                echo "box-user HEX-RT conformance: FAIL"
            fi
            c1="$("$PROBE" cputime "$HELPER_PID")" || c1=""
            if [ -n "$c0" ] && [ -n "$c1" ]; then
                echo "$c0 $c1" | awk '{dw=$4-$2; if(dw>0) printf "helper daemon CPU across the conformance run: %.3f%% of a core (%.1f ms CPU / %.1f ms wall)\n",($3-$1)/dw*100,($3-$1)/1e6,dw/1e6}'
            fi
        else
            echo "the helper socket never appeared or the test binary is missing; skipping conformance"
        fi
        kill "$HELPER_PID" 2>/dev/null || true
        wait "$HELPER_PID" 2>/dev/null || true
        HELPER_PID=""
        rm -f "$SOCK"
        dscl . -delete "/Users/$LABUSER" >/dev/null 2>&1 || true
    else
        echo "make_user failed; skipping membership and box-user tests"
    fi

    echo "## priv: root arms task_set_phys_footprint_limit on a running task"
    "$PROBE" priv 64

    echo "## C3 fatal cap overshoot (confirms the rootless result holds as root)"
    for mb in 64 256; do "$PROBE" cap fatal "$mb"; done

    echo "## C4/notify: high-watermark EXC_RESOURCE (latency, once-per-limit, re-arm)"
    "$PROBE" cap soft 64

    echo "## C3/C5 box total and fork burst, 1..28 members"
    for n in 1 4 8 14 28; do "$PROBE" box 256 "$n" steady; done
    for n in 14 28; do "$PROBE" box 256 "$n" burst; done
} 2>&1 | tee "$REC"

echo "=== RECORD $REC ==="
