#!/usr/bin/env bash
# HEX-GUEST-22's A/B, interleaved round by round (Ada's rule: records are taken under the host's own
# co-tenant load, never by waiting for quiet; so each round runs both arms, whatever load comes hitting both
# alike): both staged arms' benches (arm.sh) start, each its own guest, and take turns a round at a time in
# ABBA order (A B, B A, A B, ...), so neither always goes first. Before each turn the host's load, its running
# Hex units and its top CPU users are recorded beside the round. No round is dropped: the judgment is on the
# pairs, round 0 (each guest's first create after its boot) aside, as every HEX-GUEST-22 record. An arm whose
# bench ends early (its guest refused to boot, say) stops the run at that round, which it says, with the other
# arm's bench stopped by its recorded PID: no turn waits on a reader that is gone (CLAUDE.md §2).
#   pair.sh ROUNDS IMAGES MEMX ARM_A ARM_B OUT   (with HEX_GUEST_KERNEL and SHARDS_BIN set)
set -uo pipefail
rounds=$1; images=$2; memx=$3; a=$4; b=$5; out=$6
mkdir -p "$out"; out=$(cd "$out" && pwd)
start() { # arm-dir name: its bench, waiting on its turns
    local d=$1 n=$2
    mkdir -p "$out/$n"
    rm -f "${out:?}/$n/go" "${out:?}/$n/done"
    mkfifo "$out/$n/go" "$out/$n/done"
    # Once its bench ends, the arm holds its turn FIFOs open read-write and answers `x` in place of the
    # bench's `.`, so a turn under way completes and the driver reads that the arm ended; it waits there,
    # within an hour, until the driver stops it by its recorded PID.
    ( source "$d/env.sh"; cd "$ARM_TREE" || exit
      HEX_GUEST22_IMAGES=$images HEX_GUEST22_ROUNDS=$rounds HEX_GUEST22_MEMX=$memx \
          HEX_GUEST22_TURN=$out/$n "$ARM_BIN" --nocapture --test-threads=1 > "$out/$n.out" 2>&1 &
      echo $! > "$out/$n.pid"; wait $!; echo $? > "$out/$n.exit"
      exec 3<> "$out/$n/go" 4<> "$out/$n/done"; printf x >&4; read -r -t 3600 -n 1 -u 3 ) &
    echo $! > "$out/$n.shell"
    echo "$n: $(grep ARM_COMMIT "$d/env.sh") $(grep LIST "$d/env.sh" | sed 's/.*LIST=//')" >> "$out/arms.txt"
}
cotenants() {
    local load hex top
    load=$(uptime | sed 's/.*load averages*: //')
    if command -v launchctl > /dev/null; then
        hex=$(launchctl list | awk '$1 != "-" && $3 ~ /^hex\./ {printf "%s ", $3}')
    else
        hex=$(systemctl --user list-units 'hex-*' --state=active --no-legend --plain 2> /dev/null | awk '{printf "%s ", $1}')
    fi
    top=$(ps -Ao pcpu=,comm= | sort -rn | head -3 | awk '{n=split($2, p, "/"); printf "%s %s%%, ", p[n], $1}')
    echo "load $load | hex: ${hex:-none} | top: $top"
}
turn() { # name round; fails when the arm's bench has ended
    echo "round $2 $1 $(cotenants)" >> "$out/load.txt"
    printf . > "$out/$1/go"
    [ "$(head -c 1 "$out/$1/done")" = . ]
}
start "$a" A; start "$b" B
for ((r = 0; r < rounds; r++)); do
    if ((r % 4 == 0 || r % 4 == 3)); then
        turn A $r && turn B $r || break
    else
        turn B $r && turn A $r || break
    fi
done
if ((r < rounds)); then
    echo "STOPPED at round $r of $rounds: an arm's bench ended (A exit $(cat "$out/A.exit" 2> /dev/null), B exit $(cat "$out/B.exit" 2> /dev/null))"
    for n in A B; do [ -e "$out/$n.exit" ] || kill "$(cat "$out/$n.pid")"; done
fi
for n in A B; do
    while [ ! -e "$out/$n.exit" ] && kill -0 "$(cat "$out/$n.pid")" 2> /dev/null; do sleep 1; done
    kill "$(cat "$out/$n.shell")" 2> /dev/null
done
wait
echo "A exit $(cat "$out/A.exit"), B exit $(cat "$out/B.exit")"
python3 -I "$(dirname "$0")/summary.py" "$out" | tee "$out/summary.txt"
