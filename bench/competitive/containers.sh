# Sourced by each bench script that starts containers (CLAUDE.md sections 2 and 3). docker runs them outside the unit
# that starts the script, so a script that dies mid-pass (an OOM kill, a full disk) would leave them running under
# every later timed run. Each carries its run's labels, and goes when the run does, however it ends:
#   hex.bench.run=<run>             HEXBENCH_RUN, else the systemd unit the script runs in, else shell-<pid>: the
#                                   unit's ExecStopPost=reap.sh <unit> removes them after a SIGKILL, when no trap runs
#   hex.bench.script=<run>.<pid>    the script's own: bench_trap removes them on any exit the script sees
if [ -z "${HEXBENCH_RUN:-}" ]; then
  HEXBENCH_RUN=$(sed -n 's#^0::.*/\([^/]*\)\.service\(/.*\)\{0,1\}$#\1#p' /proc/self/cgroup 2>/dev/null || true)
  HEXBENCH_RUN=${HEXBENCH_RUN:-shell-$$}
fi
export HEXBENCH_RUN
BENCH_SCRIPT="$HEXBENCH_RUN.$$"
BENCH_LABELS=(--label hex.bench=competitive --label "hex.bench.run=$HEXBENCH_RUN" --label "hex.bench.script=$BENCH_SCRIPT")

# bench_reap: this script's own containers, removed. Only those labelled with it, never another run's.
bench_reap() {
  local ids
  ids=$(docker ps -aq --filter "label=hex.bench.script=$BENCH_SCRIPT" 2>/dev/null) || return 0
  [ -z "$ids" ] || docker rm -f $ids >/dev/null 2>&1 || true
}

# bench_trap <cleanup>: on exit, the script's own cleanup, then its containers; INT and TERM end the script through it
# (bash runs no EXIT trap for a signal it doesn't catch).
bench_trap() {
  trap "$1; bench_reap" EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
}
