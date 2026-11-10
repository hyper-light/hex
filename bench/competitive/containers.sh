# Sourced by each bench script that starts containers (CLAUDE.md sections 2 and 3). docker runs them outside the unit
# that starts the script, so a script that dies mid-pass (an OOM kill, a full disk) would leave them running under
# every later timed run. Each carries its run's labels, and goes when the run does, however it ends:
#   hex.bench.run=<run>             HEXBENCH_RUN, else the systemd unit the script runs in, else shell-<pid>: a run
#                                   that is its unit gets a reaper, which removes them after a SIGKILL, when no trap runs
#   hex.bench.script=<run>.<pid>    the script's own: bench_trap removes them on any exit the script sees
if [ -z "${HEXBENCH_RUN:-}" ]; then
  HEXBENCH_RUN=$(sed -n 's#^0::.*/\([^/]*\)\.service\(/.*\)\{0,1\}$#\1#p' /proc/self/cgroup 2>/dev/null || true)
  # The run's reaper, reap-<unit>: a unit of its own bound to the run's (it stops when that one ends, however it
  # ends), whose ExecStop is reap.sh. Not the run's ExecStopPost: systemd starts that in the run's cgroup, and Linux
  # 6.19 SIGKILLs a process cloned into a cgroup that was cgroup.kill'ed (fixed upstream, 8e3599202166), as systemd's
  # SIGKILL is, so after one it never ran. Named reap-*, so it isn't counted among the hex-* units. A run its caller
  # names (a script the run's own calls, which inherits HEXBENCH_RUN) starts none: its caller's covers it.
  if [ -n "$HEXBENCH_RUN" ] && [ -n "${INVOCATION_ID:-}" ]; then
    systemd-run --user --quiet --collect --unit "reap-$HEXBENCH_RUN" -p BindsTo="$HEXBENCH_RUN.service" \
      -p After="$HEXBENCH_RUN.service" -p RemainAfterExit=yes \
      -p "ExecStop=/usr/bin/bash $(dirname "$(realpath "${BASH_SOURCE[0]}")")/reap.sh $HEXBENCH_RUN" /usr/bin/true \
      || echo "containers.sh: no reaper for $HEXBENCH_RUN (systemd-run reap-$HEXBENCH_RUN failed):" \
        "a SIGKILL of it would leave its containers" >&2
  fi
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

# bench_trap <cleanup>: on exit, the script's own cleanup, then its containers. EXIT alone: bash runs it at once on a
# fatal SIGTERM or SIGINT, where a trap of its own for one would wait for the foreground `docker run` to end, and that
# one passes the signal to its container's PID 1, which ignores it, until the unit's stop times out and SIGKILLs all.
bench_trap() {
  trap "$1; bench_reap" EXIT
}
