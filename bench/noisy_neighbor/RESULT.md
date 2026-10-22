# noisy_neighbor (AC-T7.1.2) — of-record

Requirement AC-T7.1.2 / T-T7.1.2: while one `process` box builds at full tilt, another box's fsync
p99 is at most 2× its idle p99 (the fix for the 2026-10-19 disk-starvation incident).

Bench: `crates/hex-cli/benches/noisy_neighbor.rs` — a *victim* box with a tight `io.latency` target
(10 ms) runs an fsync probe (N=400); a *builder* box with a low `io.weight` (10) saturates the disk
via `hex selftest-box disk-hog`. It reports the victim's fsync p99 idle and under load, and the ratio.

## Run (of-record)

- Hardware: `fedora.attlocal.net` — linux/x86_64, 32 cores, 65,555,328 kB (≈64 GiB) RAM, NVMe disk.
  (The requirement names adacachy; per the coordinator, noisy_neighbor runs on fedora — also NVMe —
  to keep adacachy for production, stated here as the evidence of record.)
- Date: 2026-10-22 05:11:33 UTC.
- Load: 1-minute load average **1.16** at measurement, after a sustained window below 2 for ≥5 min
  (CLAUDE §7), gated by the self-pacing `bench/run_detached.sh`.
- Build: `cargo bench --bench noisy_neighbor` (release bench profile), hex `cp/t7-runtime` at the
  2026-10-22 01:30 UTC bench re-launch (≈`a7efec9`); the `process` class and this bench are unchanged
  since.

```
noisy_neighbor: victim fsync p99 idle=3616us loaded=6267us ratio=1.73x
  (builder io_weight=10, victim io.latency=10000us)
AC-T7.1.2 (ratio<=2x): PASS
```

Earlier provisional (not of-record): 1.30× at ~load-3 (idle 4822 µs → loaded 6262 µs). This run is
on fedora NVMe in a sustained quiet window.
