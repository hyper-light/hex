#!/bin/bash
# daytona_chain.sh: bring the stack up, wait for the default snapshot, then gate and run round 1.
T7=$HOME/.cache/hex-bench/t7
"$T7/daytona_up.sh" || exit $?
python3 -I "$T7/daytona_snapshot_wait.py" || exit $?
exec "$T7/run_daytona_round.sh" 1 "$(date +%s)"
