#!/usr/bin/env bash
# bench/competitive/probes: how fast the bench host runs a short burst of work, with the host at rest and with one
# busy loop on a CPU outside the bench's CPUs holding the package awake (exec_ab.md, "The host's state"). The burst
# is a 100,000-step dependent multiply-add chain with no system call, timed alone, 200 times 2 ms apart, so each
# finds the core as an idle host leaves it; the p50 is printed for a P-core set and an E-core set, at rest and held.
#   host_clock.sh
# Env: P (4-15) and E (16-31), the CPU sets; HOLD (0), the CPU the busy loop runs on, outside both.
set -euo pipefail
P=${P:-4-15}; E=${E:-16-31}; HOLD=${HOLD:-0}
DIR=$(mktemp -d "${XDG_CACHE_HOME:-$HOME/.cache}/host-clock.XXXXXX")
trap 'rm -rf "${DIR:?}"' EXIT
cat > "$DIR/burst.c" <<'C'
#define _GNU_SOURCE
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
enum { REPS = 200, STEPS = 100000 };
volatile uint64_t sink;
static double ms(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e3 + t.tv_nsec / 1e6; }
static int up(const void *a, const void *b) { double x = *(const double *)a, y = *(const double *)b; return (x > y) - (x < y); }
int main(void) {
  double took[REPS];
  struct timespec gap = { 0, 2000000 };
  for (int r = 0; r < REPS; r++) {
    uint64_t x = (uint64_t)r;
    double t0 = ms();
    for (int i = 0; i < STEPS; i++) x = x * 6364136223846793005ULL + 1442695040888963407ULL;
    sink = x;
    took[r] = ms() - t0;
    nanosleep(&gap, NULL);
  }
  qsort(took, REPS, sizeof took[0], up);
  printf("%.3f ms p50 on cpu %d\n", took[REPS / 2], sched_getcpu());
  return 0;
}
C
gcc -O2 -Wall -Wextra -Werror -o "$DIR/burst" "$DIR/burst.c"
echo "load: $(cut -d' ' -f1-3 /proc/loadavg)"
echo "at rest:  P-cores $(taskset -c "$P" "$DIR/burst")"
echo "at rest:  E-cores $(taskset -c "$E" "$DIR/burst")"
taskset -c "$HOLD" sh -c 'while :; do :; done' &
busy=$!
trap 'kill "$busy" 2>/dev/null; rm -rf "${DIR:?}"' EXIT
sleep 1
echo "held:     P-cores $(taskset -c "$P" "$DIR/burst")"
echo "held:     E-cores $(taskset -c "$E" "$DIR/burst")"
