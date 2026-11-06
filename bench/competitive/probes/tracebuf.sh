# bench/competitive/probes: a trace instance's per-CPU buffer, learned from the runs' own overruns rather than picked
# (Ada's 2026-11-06 rule). A probe starts at the size its runs of record used, and a trace that lost events is run
# again at twice the size, until one loses none; the bound is the kernel's: buffer_size_kb's write fails (ENOMEM)
# for a buffer it can't allocate (Documentation/trace/ftrace.rst, "buffer_size_kb"), and the probe stops there
# saying so. A run that lost nothing at the starting size is the run it was before. Sourced by the probes' inside
# scripts; each size tried and what it lost goes to $OUT/tracebuf.txt.

# lost_in <stats>...: the events the per-CPU counts in <stats> (an instance's per_cpu/cpu*/stats, or a probe's saved
# copy of them) say overran or were dropped, summed.
lost_in() { cat "$@" 2>/dev/null | awk '/^overrun:/{s+=$2} /^dropped events:/{s+=$3} END{print s+0}'; }

# grown <start KiB> <stats file> <fn> <args>...: runs <fn> <args> with TRACE_KB, its instance's per-CPU buffer in KiB,
# at <start KiB>, and again at twice it while <stats file> (the per-CPU counts <fn> saves) shows events lost. Returns
# <fn>'s status, nonzero too when it fails at a size the kernel refuses.
grown() {
  local kb=$1 stats=$2 fn=$3 lost; shift 3
  while :; do
    TRACE_KB=$kb "$fn" "$@" || { echo "$fn $*: failed at ${kb} KiB a CPU" >> "$OUT/tracebuf.txt"; return 1; }
    lost=$(lost_in "$stats")
    echo "$fn $*: ${kb} KiB a CPU, ${lost} events lost" >> "$OUT/tracebuf.txt"
    [ "$lost" = 0 ] && return 0
    kb=$((kb * 2))
  done
}
