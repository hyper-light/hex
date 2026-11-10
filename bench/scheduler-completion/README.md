# ROOT comparative profiles

This directory contains executable collection and analysis, with no measured
performance claim. Run only on ROOT's admitted machine and time/resource envelope.
The implementer does not run profiles, benchmarks, sweeps or native workloads.

The collector copies and verifies an exact release binary, takes the explicitly
requested number of samples, preserves every command/PID/exit/log hash, and reads
the supplied private delegated cgroup's CPU/memory/fault counters on Linux. It
never moves a process or changes a runner/cgroup policy. Root must arrange that
the specified cgroup covers caller, runtime, guest and induced kernel work.
Without that arrangement the coverage gap remains explicit.

Every manifest supplies source/binary pins, hardware, OS, architecture, backend,
workload/security/resource/configuration hashes, exact argv, operation count,
sample count, native oracle marker and any instrumented counters file. The
collector records all allocation, reallocation, copy, RSS, retained, CPU, kernel
and page-fault fields; absent instrumentation produces null and an unverified
result. A cgroup's memory.peak is not RSS; it is reported by its actual name.
RUSAGE_CHILDREN's largest-child RSS is never substituted for tree/kernel cost.

Reuse the reviewed index allocation counter at
[allocation-probe](../sched/runtime-pools/allocation-probe/benches/main.rs) for
model-only comparisons. Its null copied_bytes field remains unmeasured. Actual
backend profiles need counters covering their actual external processes and
native driver allocations as well. The fixture's own Rust allocations are not
the runtime's complete allocation cost.

The comparator requires equivalent backend, hardware, workload, security,
resources and configuration. It reports n, inverse-empirical-CDF p50/p90/p99,
min/max, finite-sample tail resolution, and exact binomial order-statistic
confidence bounds for the median. The method follows
[NIST TN 2119](https://nvlpubs.nist.gov/nistpubs/TechnicalNotes/NIST.TN.2119.pdf).
Those bounds assume independent samples; they do not make autocorrelated
back-to-back runs independent. Two separate 95% median intervals imply at least
90% joint coverage by the union bound; no 95% comparative certainty is claimed.
Small samples may have no finite 95% median interval. Zero denominators and
missing counters cannot produce an achieved ratio.

ROOT pins credible equivalent competitors (native Docker/runc, upstream
Firecracker, direct Shards, and scheduler baselines where their work is
equivalent), retains their exact source/security/configuration records, and
interleaves class-for-class runs on the same quiet hardware. Warm/cold, native
containers and VMs are distinct workload configurations. Historical KWOK/model
samples cannot establish native GPU or VM performance.

Concrete invocation, using ROOT-created absolute manifests and unique output
directories:

    python3 bench/scheduler-completion/collect.py --root-authorized --manifest /ABS/profile.json --output /ABS/new-run --byte-limit ROOT_BOUND --timeout ROOT_SECONDS --cleanup-timeout ROOT_SECONDS
    python3 bench/scheduler-completion/compare.py --candidate /ABS/hex/profile.json --baseline /ABS/competitor/profile.json --target-ratio 4 --byte-limit ROOT_BOUND --output /ABS/comparison.json

A valid comparison can miss the target. The report retains misses, distributions
and uncertainty and returns nonzero when the target or coverage is not achieved.
