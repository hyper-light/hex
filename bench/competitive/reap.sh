#!/usr/bin/env bash
# reap.sh <run>: the run's bench containers (labelled hex.bench.run=<run> by containers.sh), removed. The ExecStop of
# the run's reaper (containers.sh starts it), so a unit SIGKILLed mid-pass, whose scripts' traps never ran, leaves
# none running; by hand for a run that had none. Only the run's own: a container without its label is never touched.
# Exits 0 once none is left, 1 naming any that is.
set -u
RUN=${1:?usage: reap.sh <run>}
ids=$(docker ps -aq --filter "label=hex.bench.run=$RUN") || { echo "reap.sh: docker ps failed for $RUN" >&2; exit 1; }
[ -z "$ids" ] || docker rm -f $ids >/dev/null
left=$(docker ps -aq --filter "label=hex.bench.run=$RUN")
[ -z "$left" ] || { echo "reap.sh: $RUN's containers left: $left" >&2; exit 1; }
[ -z "$ids" ] || echo "reap.sh: removed $RUN's containers: $(echo $ids)"
