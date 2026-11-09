# Scale and shards: leases, takeover, heap accounting and kube-scheduler's knobs

The sources for implementation goal 6 (CP-T7.11) that the other notes don't hold: what a lease
promises and what a lost leaseholder's successor must wait for, how a Rust program counts its own
heap, and the kube-scheduler and `scheduler_perf` settings a fair comparison against kwok has to
match. Method and kinds follow [the research method](../README.md#method). Every source was opened and
read on 2026-11-04; source files are pinned to a tag and cited by line, papers by page and section.
Sharding facts (Twine, Shard Manager, Gödel, Hydra, Borg) are already in
`cluster-managers-at-scale.md` and `placement-algorithms-and-fairness.md`, and kwok in
`deterministic-simulation.md` DS[9]; they are not repeated here.

## Findings

1. **A lease is a time-bounded right that survives its holder's silence, so a successor waits it
   out.** "A lease is a contract that gives its holder specified rights over property for a limited
   period of time" SS[1] p. 1 (DE). A recovering server "must honor the leases it granted before it
   crashed", most easily by remembering the longest term it granted and delaying writes for that
   long SS[1] p. 2 (DE). The cost is clocks: "Leases depend on well-behaved clocks" SS[1] p. 6, and the
   term can be sent as a duration when clocks have "known bounded drift" SS[1] p. 7 (DE).
2. **A new holder fences the old one with a number that only goes up.** Chubby's master lease is a
   promise that replicas "will not elect a different master for an interval of a few seconds"; reads
   by the master alone are safe "provided the master lease has not expired, as no other master can
   possibly exist" SS[2] §2.2 p. 3 (DE). A newly elected master "picks a new client epoch number,
   which clients are required to present on every call" and rejects older ones SS[2] §2.9 p. 8-9 (DE);
   a lock's sequencer carries "the lock generation number", so a server can reject a stale holder's
   request SS[2] §2.4 p. 5 (DE). Takeover time in production: "two recent elections took 6s and 4s, but
   we see values as high as 30s" SS[2] §2.2 p. 3 (MV).
3. **A failed-over master rebuilds from the durable record; clients ride it out.** A failing master
   "discards its in-memory state about sessions, handles, and locks"; the session lease timer stops
   until a new master is elected, "equivalent to extending the client's lease", and a grace period
   lets sessions survive fail-overs longer than the lease SS[2] §2.9 p. 8 (DE). Borg's takeover
   "typically takes about 10 s, but can take up to a minute in a big cell because some in-memory state
   has to be reconstructed" (`cluster-managers-at-scale.md` [1] §3.1). kube-scheduler's own leader
   lease is 15 s, renew deadline 10 s, retry 2 s SS[3] `:37-44` (DE).
4. **Raft's defaults put failure detection at one second.** etcd's leader heartbeats every 100 ms by
   default SS[11] `:14`, a follower that hears none for the 1,000 ms election timeout tries to lead
   SS[11] `:16`, and the timeout "must be at least 10 times the round-trip time" SS[11] `:20` (DE).
   Chubby's session lease is coarser on purpose: "The default extension is 12s", raised by an
   overloaded master SS[2] §2.8 p. 7 (DE).
5. **A Rust program can count every heap byte by wrapping the system allocator.** `GlobalAlloc` is "A
   memory allocator that can be registered as the standard library's default" SS[4] `:7` (DE), and
   `#[global_allocator]` routes "all default allocation requests to a custom object" SS[5] `:12-14`
   (DE). The trait is `pub unsafe trait GlobalAlloc` SS[4] `:189`, with `alloc`, `dealloc`,
   `alloc_zeroed` and `realloc` SS[4] `:223,238,261,331`; std's own example forwards to `System`
   inside an `unsafe impl` SS[5] `:21-32` (DE). So the counter sees every allocation of the process,
   and implementing it is `unsafe` code. Two crates keep that `unsafe impl` inside them behind a safe
   API. `cap` wraps an allocator "tracking memory usage", registered as
   `static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::max_value())` and read with
   `allocated()` SS[12] `lib.rs:5,15-16,22,138` (DE). `stats_alloc` registers
   `&INSTRUMENTED_SYSTEM` and reads a `Region`'s `change()` in `bytes_allocated` and
   `bytes_deallocated` SS[13] `lib.rs:14-15,18-20,80-82` (DE). The `unsafe impl`s are at SS[12]
   `lib.rs:179` and SS[13] `lib.rs:233,251`.
6. **kube-scheduler's binding rate is capped by its API client unless raised.** The scheduler "has an
   opinion about QPS/Burst": 50 QPS and burst 100 unless set SS[6] `:149-154` (DE), the same as
   component-base's generic default SS[3] `:69-73`. Every bind is an API call
   (`cluster-managers-at-scale.md`, kube-scheduler section). YuniKorn's KWOK runs measured
   kube-scheduler at a flat 50.5 pods/s, consistent with that cap (`cluster-managers-at-scale.md` [39],
   MI, the cause an inference); Gödel's benchmark raised kube-scheduler to QPS and burst 10,000 on a
   30,000-node KWOK cluster and measured about 300 pods/s (`placement-algorithms-and-fairness.md` [4c],
   MV).
7. **`scheduler_perf` fixes the shapes and the method a comparison should reuse.** Its fake node is 4
   CPU, 32 GiB and 110 pods SS[7] `:7-10`; its pod is one pause container requesting and limited to
   100m CPU and 500Mi SS[8] `:11-17` (DE). Its regression floors are 790 pods/s at 5,000 nodes and
   50,000 pods and 680 at 5,000 and 10,000 SS[9] `:57-68` (MV), each "the minimum average value from
   the most recent 250 test runs", set "somewhat pessimistically" SS[10] `:229,233` (DE). Its README
   pins a run to six CPUs with `numactl --physcpubind=0-5` and suggests `-count=6` for "statistically
   relevant" comparisons SS[10] `:53,58` (DE).
8. **kube-scheduler's client limiter can be switched off, not raised to a picked number.** client-go:
   "Setting this to a negative value will disable client-side ratelimiting unless `Ratelimiter` is also
   set" SS[14] `config.go:120-121`, and it builds a token bucket only `if qps > 0` `:383-385` (DE).
   kube-scheduler copies its configuration's `clientConnection.qps` into that client unchanged SS[14]
   `options.go:396`, and the configuration's validation checks only that burst isn't negative SS[14]
   `validation.go:27-28` (DE). So `qps: -1` takes the client limit out of the comparison without
   choosing a rate.

## Gaps

- No source publishes a sharded scheduler's shard-takeover time at a stated workload count; Chubby's
  6-30 s is a lock service's master election, Borg's 10-60 s a whole cell's master. `T-T7.11.1`'s 5 s
  is Hex's own bar.
- No source publishes control-plane heap per workload (as `comparison.md` also notes); the nearest
  are whole-process figures (Twine's 80 GB shard for ~170K machines, Shard Manager's 64 GB mini-SM at
  ~38%, Borg's 50 GiB Borgmaster: `cluster-managers-at-scale.md` `:479-480`, `:646-647`, [1] §3.4).
- `scheduler_perf`'s floors carry no hardware; they are a lead, not a number Hex beats until run on
  fedora beside Hex (research README, "our own measurements supersede claims").

## References

[SS1] C. G. Gray, D. R. Cheriton (Stanford University), "Leases: An Efficient Fault-Tolerant Mechanism
for Distributed File Cache Consistency", SOSP 1989, pp. 202-210, https://doi.org/10.1145/74850.74870 .
Copy read: https://web.eecs.umich.edu/~mosharaf/Readings/Leases.pdf (9 pages). Kind: DE.
- p. 1: "A lease is a contract that gives its holder specified rights over property for a limited period of time."
- p. 2: "When a server is recovering after crashing, it must honor the leases it granted before it crashed."
- p. 6: "Leases depend on well-behaved clocks."
- p. 7: "known bounded drift, in which case the lease term can be communicated as its duration"

[SS2] M. Burrows (Google), "The Chubby lock service for loosely-coupled distributed systems", OSDI 2006.
https://static.googleusercontent.com/media/research.google.com/en//archive/chubby-osdi06.pdf
Kinds: DE; MV for the election times.
- p. 3 §2.2: "will not elect a different master for an interval of a few seconds known as the master lease"
- p. 3 §2.2: "this is safe provided the master lease has not expired, as no other master can possibly exist"
- p. 3 §2.2: "two recent elections took 6s and 4s, but we see values as high as 30s" (MV)
- p. 5 §2.4: "the mode in which it was acquired (exclusive or shared), and the lock generation number"
- p. 7 §2.8: "The default extension is 12s, but an overloaded master may use higher values"
- p. 8 §2.9: "When a master fails or otherwise loses mastership, it discards its in-memory state about sessions, handles, and locks."
- p. 8 §2.9: "this is legal because it is equivalent to extending the client's lease"
- p. 9 §2.9: "It first picks a new client epoch number, which clients are required to present on every call."

[SS3] Kubernetes source, tag v1.37.1, `staging/src/k8s.io/component-base/config/v1alpha1/defaults.go`.
https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/staging/src/k8s.io/component-base/config/v1alpha1/defaults.go
Kind: DE.
- `:37-44`: `LeaseDuration` 15 s, `RenewDeadline` 10 s, `RetryPeriod` 2 s.
- `:69-73`: `QPS = 50.0`, `Burst = 100`.

[SS4] Rust source, tag 1.99.1 (Hex's toolchain), `library/core/src/alloc/global.rs`.
https://raw.githubusercontent.com/rust-lang/rust/1.99.1/library/core/src/alloc/global.rs
Kind: DE.
- `:7`: "A memory allocator that can be registered as the standard library’s default"
- `:189`: `pub unsafe trait GlobalAlloc {`
- `:223`, `:238`, `:261`, `:331`: `unsafe fn alloc`, `dealloc`, `alloc_zeroed`, `realloc`.

[SS5] Rust source, tag 1.99.1, `library/std/src/alloc.rs` (module documentation).
https://raw.githubusercontent.com/rust-lang/rust/1.99.1/library/std/src/alloc.rs
Kind: DE.
- `:12-14`: "This attribute allows configuring the choice of global allocator." ... "to route all[^system-alloc] default allocation requests to a custom object."
- `:21-32`: `unsafe impl GlobalAlloc for MyAllocator` forwarding to `System`, registered with `#[global_allocator]`.

[SS6] Kubernetes source, tag v1.37.1, `pkg/scheduler/apis/config/v1/defaults.go`.
https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/pkg/scheduler/apis/config/v1/defaults.go
Kind: DE.
- `:149`: "Scheduler has an opinion about QPS/Burst, setting specific defaults for itself, instead of generic settings."
- `:150-154`: `QPS = 50.0`, `Burst = 100`.

[SS7] Kubernetes source, tag v1.37.1, `test/integration/scheduler_perf/templates/node-default.yaml`.
https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/test/integration/scheduler_perf/templates/node-default.yaml
Kind: DE.
- `:7-10`: capacity `pods: "110"`, `cpu: "4"`, `memory: 32Gi`.

[SS8] Kubernetes source, tag v1.37.1, `test/integration/scheduler_perf/templates/pod-default.yaml`.
https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/test/integration/scheduler_perf/templates/pod-default.yaml
Kind: DE.
- `:7`: `image: registry.k8s.io/pause:3.10.2`
- `:11-17`: limits and requests `cpu: 100m`, `memory: 500Mi`.

[SS9] Kubernetes source, tag v1.37.1, `test/integration/scheduler_perf/misc/performance-config.yaml`.
https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/test/integration/scheduler_perf/misc/performance-config.yaml
Kind: MV (the thresholds).
- `:57-59`: `5000Nodes_10000Pods`, `threshold: 680`.
- `:64-68`: `5000Nodes_50000Pods`, `threshold: 790`.

[SS10] Kubernetes source, tag v1.37.1, `test/integration/scheduler_perf/README.md`.
https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/test/integration/scheduler_perf/README.md
Kind: DE.
- `:53`: `numactl --physcpubind=0-5 make test-integration ...`
- `:58`: "Add `-count=6` to `KUBE_TEST_ARGS` to get statistically relevant"
- `:229`: "taking the minimum average value from the most recent 250 test runs"
- `:233`: "Our goal is to set the thresholds somewhat pessimistically to minimize flakiness."

[SS11] etcd project, "Tuning" (v3.5 documentation), `content/en/docs/v3.5/tuning.md`, at commit
29b874f031ddbb78c82f498a04b39bb7eaa3835b of etcd-io/website.
https://raw.githubusercontent.com/etcd-io/website/29b874f031ddbb78c82f498a04b39bb7eaa3835b/content/en/docs/v3.5/tuning.md
Kind: DE.
- `:14`: "By default, etcd uses a `100ms` heartbeat interval."
- `:16`: "This timeout is how long a follower node will go without hearing a heartbeat before attempting to become leader itself. By default, etcd uses a `1000ms` election timeout."
- `:20`: "Election timeouts must be at least 10 times the round-trip time so it can account for variance in the network."

[SS12] A. Mocatta, `cap` 0.1.2 (MIT OR Apache-2.0), `src/lib.rs` from the crates.io package
https://static.crates.io/crates/cap/cap-0.1.2.crate ; docs https://docs.rs/cap/0.1.2/cap/ ; repository
https://github.com/alecmocatta/cap . Kind: DE.
- `:5`: "This crate provides a generic allocator that wraps another allocator, tracking memory usage and enabling limits to be set."
- `:15-16`: `#[global_allocator]` / `static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::max_value());`
- `:22`: `println!("Currently allocated: {}B", ALLOCATOR.allocated());`
- `:137-138`: "Return the number of bytes allocated. Always less than the limit." `pub fn allocated(&self) -> usize`
- `:179`: `unsafe impl<H> GlobalAlloc for Cap<H>`

[SS13] M. Griep, `stats_alloc` 0.1.10 (MIT), `src/lib.rs` from the crates.io package
https://static.crates.io/crates/stats_alloc/stats_alloc-0.1.10.crate ; docs
https://docs.rs/stats_alloc/0.1.10/stats_alloc/ ; repository https://github.com/neoeinstein/stats_alloc .
Kind: DE.
- `:2-4`: "An instrumenting middleware for global allocators in Rust, useful in testing for validating assumptions regarding allocation patterns"
- `:14-15`: `#[global_allocator]` / `static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;`
- `:18-20`: `let reg = Region::new(&GLOBAL);` ... `reg.change()`
- `:80-82`: `pub bytes_allocated: usize`, `pub bytes_deallocated: usize` (fields of `Stats`)
- `:233`, `:251`: `unsafe impl ... GlobalAlloc for &'a StatsAlloc<T>` and `for StatsAlloc<T>`

[SS14] Kubernetes v1.37.1, files of kubernetes/kubernetes at tag v1.37.1, under
`https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/`, read 2026-11-06. Kind: DE.
- `staging/src/k8s.io/client-go/rest/config.go:120-121`: "Setting this to a negative value will disable client-side ratelimiting unless `Ratelimiter` is also set."
- `staging/src/k8s.io/client-go/rest/config.go:383-385`: `if qps > 0 { rateLimiter = flowcontrol.NewTokenBucketRateLimiter(qps, burst) }`
- `cmd/kube-scheduler/app/options/options.go:396`: `kubeConfig.QPS = config.QPS`
- `staging/src/k8s.io/component-base/config/validation/validation.go:27-28`: `if cc.Burst < 0 {` ... `"must be non-negative"`
