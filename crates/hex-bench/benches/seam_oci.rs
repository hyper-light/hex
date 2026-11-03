//! The `oci` class through the hex-api `Runtime` seam, by either of its two drivers: in process (the
//! node agent in this process, `HEXBENCH_DRIVER` unset or `inprocess`) or over the node agent's local
//! socket (`HEXBENCH_DRIVER=socket`: a `hex node` process this starts, reached through `hex-client`).
//! oci_bench.py's phases, as it runs them:
//! - `lifecycle`: create, start, exec of `true` and delete of a `sleep` container, each timed alone;
//! - `lifecycle_nonet`: the same on a bundle without a network namespace of its own, so create and
//!   delete are timed with and without one (decision 4);
//! - `start_true`: create+start of a `true` container, timed together and deleted untimed (AC-T7.1.3's
//!   measure, goal 5's create+start of `true`);
//! - `throughput`: create+start+delete of a `true` container at concurrency 1, 8 and 32;
//! - `sustained`: the throughput lifecycle at concurrency 32 for `HEXBENCH_SUSTAINED` of them
//!   (10,000), its rate in each window of 1,000 and the last window's against the first's;
//! - `rtt`: inspect of a running box, timed alone, whose work is the same either way, so the socket's
//!   cost per call is the two drivers' difference in one run.
//!
//! Around every phase it reads the node's network namespace counts (made ahead, taken from its pool,
//! made on a create's path because the pool was dry or the node keeps none, discarded, failed), in
//! process through `Node::netns` and over the socket through `Client::netns` (the socket's version 2),
//! and records what the phase added: the sustained phase's made-on-the-path count is how often its
//! creates found the pool dry (decision 4's falsifier). In process it reads the node's create seeds'
//! counts the same way (`Node::create_seeds`; decision 5): the creates that waited for a seed, the
//! seeds started, the most busy at once, which the seeds' bound is judged by. No call spawns a process
//! but the container's own init and exec helper. It writes oci_bench.py's JSON under the runtime `hex-seam` (or
//! `hex-seam-socket`), which summarize.py sets beside libcrun (the in-process rival), so runtime design
//! goal 3's targets (exec p50 at most a quarter of libcrun's, start p50 within 10% of libcrun's,
//! argument A3) and goal 4's (create p50 with a network namespace at most 5% over without one) are read
//! from it. Decisions 3 and 4 of docs/design/runtime-architecture.md.
//!
//!   seam_oci <out.json> <n> <warmup> <lifecycles> <hex-binary> <sleep-bundle> <true-bundle> [<sleep-nonet-bundle>]
//! (`cargo bench -p hex-bench --bench seam_oci -- ...`, or its binary as host.sh's ociseam pass runs
//! it). HEXBENCH_PHASES names the phases to run (all six unless set); `lifecycle_nonet` needs the
//! eighth argument, a bundle like the sleep bundle without a network namespace, and records an error
//! naming it when it isn't given. The `oci` class needs root (a container's cgroup
//! device program is an eBPF program, and its namespaces are made without a user namespace), so this
//! runs in the OCI pass's privileged container, where the cgroup root is vacated to a leaf and its
//! controllers handed down, as oci_inside.sh leaves it; off that it refuses, naming why.
//! In process it reads the node's ipc and uts namespace counts the same way (`Node::ipc_uts`, goal 5 extended
//! step 4), and HEXBENCH_IPCUTS=0 drains that pool (bound 0), so a run can time creates with it and without it. The
//! `ipc_uts_pairs` phase, run only when named, in process: the lifecycle on a node with the pool and one with it
//! drained, interleaved one by one, the first of each pair alternating, so a pair meets one host and a difference
//! of a percent or two shows under other tenants' load; create, start, exec and delete each, so work that moved
//! from one to another shows too.
//! HEXBENCH_TRACE turns on the breakdown probes' markers, around the ops HEXBENCH_TRACE_OPS
//! names (`Trace`, bench/competitive/probes/exec_breakdown.md and create_breakdown.md).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros,
    clippy::print_stdout,
    clippy::cast_precision_loss
)]

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("SKIP: the oci seam bench is Linux-only");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux::main()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::{File, OpenOptions};
    use std::io::{BufRead, BufReader, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, Stdio as Std};
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Exit, Program, Runtime, Stdio};
    use hex_client::Client;
    use hex_core::{netns, seeds};
    use hex_node::Node;
    use serde_json::{Map, Value, json};

    /// Each call's bound, as oci_bench.py's CALL_TIMEOUT_S.
    const CALL_TIMEOUT: Duration = Duration::from_secs(60);

    /// The sustained phase's concurrency and the lifecycles in each of its windows, as oci_bench.py's.
    const SUSTAINED_CONCURRENCY: usize = 32;
    const SUSTAINED_WINDOW: usize = 1_000;
    /// The sustained phase's lifecycles unless `HEXBENCH_SUSTAINED` says, as oci_bench.py's.
    const SUSTAINED_DEFAULT: usize = 10_000;

    /// oci_bench.py's bundles bound every container to nothing of their own; the node bounds each box
    /// the way the competitive pass's do, so the comparison is like for like: the kernel's floor plus
    /// the workload, not an unbounded cgroup.
    fn bounds() -> Bounds {
        Bounds {
            memory_max: Some(256 << 20),
            pids_max: Some(256),
            ..Bounds::default()
        }
    }

    fn spec(id: &BoxId, bundle: &Path) -> BoxSpec {
        BoxSpec {
            id: id.clone(),
            bounds: bounds(),
            class: ClassSpec::Bundle(bundle.to_path_buf()),
        }
    }

    /// The cgroup the box root is made under: the OCI pass's container has its cgroup root vacated to a
    /// leaf and its controllers handed down there (oci_inside.sh), so the box root is a child of the
    /// root, beside the runtimes' own container cgroups. `HEXBENCH_CGROUP` overrides it, as oci_bench.py
    /// takes it, for the driver's control-flow check off the bench host.
    fn box_root_parent() -> PathBuf {
        std::env::var_os("HEXBENCH_CGROUP")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/sys/fs/cgroup"))
    }

    /// The CPU the cgroup's tree has used, in microseconds (`cpu.stat` usage_usec).
    fn cpu_usec(cg: &Path) -> i64 {
        std::fs::read_to_string(cg.join("cpu.stat"))
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix("usage_usec "))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    }

    /// The breakdown probes' markers (bench/competitive/probes/exec_breakdown.md, create_breakdown.md).
    /// With `HEXBENCH_TRACE` naming an ftrace instance's directory, the lifecycle phase brackets the
    /// ops `HEXBENCH_TRACE_OPS` names (`create`, `start`, `exec`; start and exec unless it says) of
    /// every other timed lifecycle in the instance's buffer, `hexbd B <op> <id>` before the call and
    /// `hexbd E <op> <id> <ns>` after it, `<ns>` the call's time as this driver measured it, and the
    /// instance records only from one to the other: its trace is those calls and what they ran, and
    /// nothing of the ops it doesn't name or of delete. The lifecycles between run untraced in the same
    /// process, so the traced calls are set against calls that met the host as they did. Each marker
    /// is written outside the time it brackets.
    struct Trace {
        on: File,
        marker: File,
        ops: Vec<String>,
    }

    impl Trace {
        fn from_env() -> Option<Self> {
            let dir = PathBuf::from(std::env::var_os("HEXBENCH_TRACE")?);
            let open = |name: &str| {
                let path = dir.join(name);
                OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap_or_else(|e| panic!("HEXBENCH_TRACE: {}: {e}", path.display()))
            };
            let ops: Vec<String> = std::env::var("HEXBENCH_TRACE_OPS")
                .ok()
                .filter(|o| !o.trim().is_empty())
                .unwrap_or_else(|| "start exec".to_owned())
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            if let Some(other) = ops
                .iter()
                .find(|o| !["create", "start", "exec"].contains(&o.as_str()))
            {
                panic!("HEXBENCH_TRACE_OPS: {other:?} isn't create, start or exec");
            }
            Some(Self {
                on: open("tracing_on"),
                marker: open("trace_marker"),
                ops,
            })
        }

        /// Whether calls of `op` are bracketed.
        fn brackets(&self, op: &str) -> bool {
            self.ops.iter().any(|o| o == op)
        }

        /// Starts the instance recording, then marks the call's beginning.
        fn begin(&self, op: &str, id: &str) -> Result<(), String> {
            (&self.on)
                .write_all(b"1")
                .map_err(|e| format!("tracing_on: {e}"))?;
            (&self.marker)
                .write_all(format!("hexbd B {op} {id}").as_bytes())
                .map_err(|e| format!("trace_marker: {e}"))
        }

        /// Marks the call's end with its time, then stops the instance recording.
        fn end(&self, op: &str, id: &str, took: Duration) -> Result<(), String> {
            (&self.marker)
                .write_all(format!("hexbd E {op} {id} {}", took.as_nanos()).as_bytes())
                .map_err(|e| format!("trace_marker: {e}"))?;
            (&self.on)
                .write_all(b"0")
                .map_err(|e| format!("tracing_on: {e}"))
        }
    }

    /// One container's create, start, exec of `true` and delete, each timed alone; a slot for the op
    /// that failed records the error. The init is the bundle's `sleep infinity`, so exec runs beside it.
    /// A timed op `trace` brackets is bracketed by its markers when it's on.
    fn lifecycle_once(
        node: &dyn Runtime,
        bundle: &Path,
        id: &str,
        ms: &mut Ops,
        timed: bool,
        trace: Option<&Trace>,
    ) -> Result<(), String> {
        let traced = |op: &str| trace.filter(|t| timed && t.brackets(op));
        let bid = BoxId::new(id).map_err(|e| e.to_string())?;
        traced("create").map_or(Ok(()), |t| t.begin("create", id))?;
        let create = Instant::now();
        node.create(spec(&bid, bundle))
            .map_err(|e| format!("create {id}: {e}"))?;
        let create = create.elapsed();
        traced("create").map_or(Ok(()), |t| t.end("create", id, create))?;
        // start and exec, timed; delete runs whether or not they did, so a box is never left behind.
        let inner = (|| -> Result<(Duration, Duration), String> {
            traced("start").map_or(Ok(()), |t| t.begin("start", id))?;
            let start = Instant::now();
            node.start(&bid).map_err(|e| format!("start {id}: {e}"))?;
            let start = start.elapsed();
            traced("start").map_or(Ok(()), |t| t.end("start", id, start))?;
            traced("exec").map_or(Ok(()), |t| t.begin("exec", id))?;
            let exec = Instant::now();
            let mut p = node
                .exec(&bid, Program::of(&["/usr/bin/true"]), Stdio::null())
                .map_err(|e| format!("exec in {id}: {e}"))?;
            let ended = p
                .wait_for(CALL_TIMEOUT)
                .map_err(|e| format!("wait in {id}: {e}"))?
                .ok_or_else(|| format!("`true` in {id} still running after {CALL_TIMEOUT:?}"))?;
            let exec = exec.elapsed();
            traced("exec").map_or(Ok(()), |t| t.end("exec", id, exec))?;
            if ended != Exit::Code(0) {
                return Err(format!("`true` in {id} ended {ended:?}"));
            }
            Ok((start, exec))
        })();
        let delete = Instant::now();
        node.delete(&bid).map_err(|e| format!("delete {id}: {e}"))?;
        let delete = delete.elapsed();
        let (start, exec) = inner?;
        if timed {
            ms.create.push(create.as_secs_f64() * 1e3);
            ms.start.push(start.as_secs_f64() * 1e3);
            ms.exec.push(exec.as_secs_f64() * 1e3);
            ms.delete.push(delete.as_secs_f64() * 1e3);
        }
        Ok(())
    }

    #[derive(Default)]
    struct Ops {
        create: Vec<f64>,
        start: Vec<f64>,
        exec: Vec<f64>,
        delete: Vec<f64>,
    }

    /// The p50, the p99 and, against `other`'s lifecycles in the same pairs, the median difference of one operation.
    fn paired(on: &[f64], drained: &[f64]) -> Value {
        let at = |sorted: &[f64], pct: usize| sorted[(sorted.len() - 1) * pct / 100];
        let sorted = |v: &[f64]| {
            let mut v = v.to_vec();
            v.sort_by(f64::total_cmp);
            v
        };
        let (a, b) = (sorted(on), sorted(drained));
        let diffs: Vec<f64> = on.iter().zip(drained).map(|(x, y)| x - y).collect();
        let faster = diffs.iter().filter(|d| **d < 0.0).count();
        json!({"on_p50": at(&a, 50), "drained_p50": at(&b, 50), "on_p99": at(&a, 99), "drained_p99": at(&b, 99),
               "median_difference": at(&sorted(&diffs), 50), "on_faster": faster as f64 / diffs.len() as f64})
    }

    /// The `ipc_uts_pairs` phase (goal 5 extended, step 4): the module's header says what it times. `on` is the node
    /// under test, with the pool; the drained node is made here beside it, with its own box root and state.
    fn ipc_uts_pairs(
        on: &dyn Runtime,
        parent: &Path,
        hex: &str,
        bundle: &Path,
        n: usize,
        warmup: usize,
    ) -> Result<Value, String> {
        let state = tempfile::tempdir().map_err(|e| e.to_string())?;
        let drained = Node::open(
            &parent.join("hex-oci-seam-drained"),
            state.path(),
            state.path(),
        )
        .map_err(|e| e.to_string())?
        .with_oci_helper(PathBuf::from(hex))
        .with_ipc_uts_bound(0)
        .map_err(|e| e.to_string())?;
        let (mut a, mut b) = (Ops::default(), Ops::default());
        for i in 0..n + warmup {
            let timed = i >= warmup;
            let first_on = i % 2 == 0;
            for with_pool in [first_on, !first_on] {
                if with_pool {
                    lifecycle_once(on, bundle, &format!("pa{i}"), &mut a, timed, None)?;
                } else {
                    lifecycle_once(&drained, bundle, &format!("pd{i}"), &mut b, timed, None)?;
                }
            }
        }
        Ok(
            json!({"pairs": n, "create": paired(&a.create, &b.create), "start": paired(&a.start, &b.start),
                  "exec": paired(&a.exec, &b.exec), "delete": paired(&a.delete, &b.delete)}),
        )
    }

    /// n timed lifecycles after `warmup` untimed ones, each operation timed alone, its boxes named
    /// `tag` and their number.
    fn lifecycle(
        node: &dyn Runtime,
        bundle: &Path,
        n: usize,
        warmup: usize,
        tag: &str,
        trace: Option<&Trace>,
    ) -> Result<Value, String> {
        let mut ms = Ops::default();
        for i in 0..warmup + n {
            lifecycle_once(
                node,
                bundle,
                &format!("{tag}{i:06}"),
                &mut ms,
                i >= warmup,
                trace.filter(|_| i % 2 == warmup % 2),
            )?;
        }
        Ok(
            json!({"ms": {"create": ms.create, "start": ms.start, "exec": ms.exec, "delete": ms.delete}}),
        )
    }

    /// The concurrencies the throughput phase runs at: oci_bench.py's 1, 8 and 32 unless
    /// `HEXBENCH_THROUGHPUT_CONC` names others (space-, tab- or comma-separated, each above zero).
    /// Goal 5 plan step 2's AC-e sets it to `8` to trace the root switch at exactly eight creates in
    /// flight; an empty or unparsable value falls back to the three.
    fn throughput_concurrency() -> Vec<usize> {
        std::env::var("HEXBENCH_THROUGHPUT_CONC")
            .ok()
            .map(|s| {
                s.split([' ', '\t', ','])
                    .filter_map(|t| t.trim().parse::<usize>().ok())
                    .filter(|&c| c > 0)
                    .collect::<Vec<usize>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec![1, 8, 32])
    }

    /// create+start+delete of a `true` container, at each concurrency that many threads share the
    /// node, each its share of the lifecycles; the clock starts once every thread is at the barrier.
    /// `own` is the subtree the agent and its boxes share, whose cpu.stat the per-lifecycle CPU is.
    /// The concurrencies are `throughput_concurrency`'s (1, 8 and 32 by default).
    fn throughput(
        node: &dyn Runtime,
        bundle: &Path,
        own: &Path,
        lifecycles: usize,
    ) -> Result<Value, String> {
        let mut out = Map::new();
        for conc in throughput_concurrency() {
            let per = (lifecycles / conc).max(1);
            let start = Barrier::new(conc + 1);
            let (wall, cpu_s, errs, create_ms) = {
                std::thread::scope(|s| {
                    let threads: Vec<_> = (0..conc)
                        .map(|w| {
                            let start = &start;
                            s.spawn(move || {
                                start.wait();
                                let mut create_ms = Vec::with_capacity(per);
                                for j in 0..per {
                                    let d = run_true_container_timed(
                                        node,
                                        bundle,
                                        &format!("t{conc}-{w}-{j:06}"),
                                    )?;
                                    create_ms.push(d.as_secs_f64() * 1e3);
                                }
                                Ok::<_, String>(create_ms)
                            })
                        })
                        .collect();
                    start.wait();
                    let (c0, t0) = (cpu_usec(own), Instant::now());
                    let (mut errs, mut create_ms) = (Vec::new(), Vec::new());
                    for t in threads {
                        match t
                            .join()
                            .unwrap_or_else(|_| Err("a thread panicked".to_owned()))
                        {
                            Ok(samples) => create_ms.extend(samples),
                            Err(e) => errs.push(e),
                        }
                    }
                    (
                        t0.elapsed(),
                        (cpu_usec(own) - c0) as f64 / 1e6,
                        errs,
                        create_ms,
                    )
                })
            };
            if let Some(e) = errs.into_iter().next() {
                return Err(format!("concurrency {conc}: {e}"));
            }
            let total = per * conc;
            out.insert(
                conc.to_string(),
                json!({"lifecycles": total, "wall_s": wall.as_secs_f64(), "create_ms": create_ms,
                       "per_s": total as f64 / wall.as_secs_f64(),
                       "cpu_s_per_lifecycle": cpu_s / total as f64}),
            );
        }
        Ok(Value::Object(out))
    }

    /// oci_bench.py's sustained phase: the throughput lifecycle at concurrency 32 for `lifecycles` of
    /// them, each thread taking the next until none are left or one has failed, the clock started once
    /// every thread is at the barrier; then the rate in each window of 1,000 by the lifecycles' ends
    /// (from the end of the window before it, or the start), and the last window's against the first's.
    /// A short burst can't show decay under churn, which one production system saw fall from 110 to 45
    /// starts/s over an hour (docs/research/runtime/oci-runtimes-and-fast-start.md §4.4).
    fn sustained(
        node: &dyn Runtime,
        bundle: &Path,
        own: &Path,
        lifecycles: usize,
    ) -> Result<Value, String> {
        let (next, failed) = (AtomicUsize::new(0), AtomicBool::new(false));
        let start = Barrier::new(SUSTAINED_CONCURRENCY + 1);
        let (t0, wall, cpu_s, ends, errs) = std::thread::scope(|s| {
            let threads: Vec<_> = (0..SUSTAINED_CONCURRENCY)
                .map(|_| {
                    s.spawn(|| {
                        start.wait();
                        let mut ends = Vec::new();
                        while !failed.load(Ordering::Relaxed) {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            if i >= lifecycles {
                                break;
                            }
                            if let Err(e) = run_true_container(node, bundle, &format!("s{i:06}")) {
                                failed.store(true, Ordering::Relaxed);
                                return (ends, Some(e));
                            }
                            ends.push(Instant::now());
                        }
                        (ends, None)
                    })
                })
                .collect();
            start.wait();
            let (c0, t0) = (cpu_usec(own), Instant::now());
            let (mut ends, mut errs) = (Vec::with_capacity(lifecycles), Vec::new());
            for t in threads {
                match t.join() {
                    Ok((done, err)) => {
                        ends.extend(done);
                        errs.extend(err);
                    }
                    Err(_) => errs.push("a thread panicked".to_owned()),
                }
            }
            let wall = t0.elapsed();
            (t0, wall, (cpu_usec(own) - c0) as f64 / 1e6, ends, errs)
        });
        if let Some(e) = errs.into_iter().next() {
            return Err(format!(
                "{e} (after {} of {lifecycles} lifecycles)",
                ends.len()
            ));
        }
        let mut at: Vec<f64> = ends
            .iter()
            .map(|end| end.saturating_duration_since(t0).as_secs_f64())
            .collect();
        at.sort_by(f64::total_cmp);
        let (mut windows, mut prev) = (Vec::new(), 0.0);
        for k in (SUSTAINED_WINDOW - 1..at.len()).step_by(SUSTAINED_WINDOW) {
            windows.push(SUSTAINED_WINDOW as f64 / (at[k] - prev));
            prev = at[k];
        }
        let last_over_first = (windows.len() > 1).then(|| windows[windows.len() - 1] / windows[0]);
        Ok(
            json!({"concurrency": SUSTAINED_CONCURRENCY, "lifecycles": lifecycles,
                  "wall_s": wall.as_secs_f64(), "per_s": lifecycles as f64 / wall.as_secs_f64(),
                  "cpu_s_per_lifecycle": cpu_s / lifecycles as f64, "window": SUSTAINED_WINDOW,
                  "window_per_s": windows, "last_over_first": last_over_first}),
        )
    }

    /// n sequential create+start of a `true` container after `warmup` untimed ones, the two calls'
    /// times summed and the box deleted untimed, as oci_bench.py's `start_true` times libcrun's
    /// (AC-T7.1.3's measure): delete runs whether or not start did, so no box is left behind.
    fn start_true(
        node: &dyn Runtime,
        bundle: &Path,
        n: usize,
        warmup: usize,
    ) -> Result<Value, String> {
        let mut ms = Vec::with_capacity(n);
        for i in 0..warmup + n {
            let id = format!("t{i:06}");
            let bid = BoxId::new(id.as_str()).map_err(|e| e.to_string())?;
            let create = Instant::now();
            node.create(spec(&bid, bundle))
                .map_err(|e| format!("create {id}: {e}"))?;
            let create = create.elapsed();
            let start = Instant::now();
            let started = node.start(&bid).map_err(|e| format!("start {id}: {e}"));
            let start = start.elapsed();
            node.delete(&bid).map_err(|e| format!("delete {id}: {e}"))?;
            started?;
            if i >= warmup {
                ms.push((create + start).as_secs_f64() * 1e3);
            }
        }
        Ok(json!({"ms": ms}))
    }

    /// create+start+delete of a `true` container, with no exec: the throughput lifecycle.
    fn run_true_container(node: &dyn Runtime, bundle: &Path, id: &str) -> Result<(), String> {
        run_true_container_timed(node, bundle, id).map(|_| ())
    }

    /// The complete create is timed alongside part 19, under the same concurrency and host load.
    fn run_true_container_timed(
        node: &dyn Runtime,
        bundle: &Path,
        id: &str,
    ) -> Result<Duration, String> {
        let bid = BoxId::new(id).map_err(|e| e.to_string())?;
        let spec = spec(&bid, bundle);
        let t = Instant::now();
        node.create(spec).map_err(|e| format!("create {id}: {e}"))?;
        let create = t.elapsed();
        let started = node.start(&bid).map_err(|e| format!("start {id}: {e}"));
        node.delete(&bid).map_err(|e| format!("delete {id}: {e}"))?;
        started.map(|_| create)
    }

    /// Inspect of one running `sleep` container, `n` times after `warmup`, each timed alone: a call
    /// whose work (the box's record and its container's state, read) is the same through either
    /// driver, so the drivers' difference is the socket's cost per call.
    fn rtt(node: &dyn Runtime, bundle: &Path, n: usize, warmup: usize) -> Result<Value, String> {
        let id = BoxId::new("rtt").map_err(|e| e.to_string())?;
        node.create(spec(&id, bundle))
            .map_err(|e| format!("create rtt: {e}"))?;
        let timed = (|| -> Result<Vec<f64>, String> {
            node.start(&id).map_err(|e| format!("start rtt: {e}"))?;
            let mut ms = Vec::with_capacity(n);
            for i in 0..warmup + n {
                let t = Instant::now();
                node.inspect(&id).map_err(|e| format!("inspect rtt: {e}"))?;
                if i >= warmup {
                    ms.push(t.elapsed().as_secs_f64() * 1e3);
                }
            }
            Ok(ms)
        })();
        node.delete(&id).map_err(|e| format!("delete rtt: {e}"))?;
        Ok(json!({ "ms": timed? }))
    }

    /// A `hex node` serving its socket for the socket driver, its boxes under `box_root`; killed when
    /// dropped.
    struct Agent(Child);

    impl Agent {
        fn start(hex: &str, state: &Path, box_root: &Path) -> (Self, PathBuf) {
            let socket = state.join("node.sock");
            let mut child = Command::new(hex)
                .arg("node")
                .arg("--socket")
                .arg(&socket)
                .arg("--state")
                .arg(state.join("node"))
                .arg("--box-root")
                .arg(box_root)
                .stdout(Std::piped())
                .spawn()
                .expect("start hex node");
            let mut line = String::new();
            BufReader::new(child.stdout.take().expect("hex node's stdout"))
                .read_line(&mut line)
                .expect("hex node's ready line");
            assert!(line.starts_with("ready "), "hex node said {line:?}");
            (Self(child), socket)
        }
    }

    impl Drop for Agent {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// The seam, by one of its drivers.
    enum Driver {
        /// The node agent in this process.
        InProcess(Box<Node>),
        /// A `hex node` of its own, reached through its socket. The client is dropped before the agent
        /// it holds a connection to, which is kept for its drop alone.
        Socket { client: Client, _agent: Agent },
    }

    impl Driver {
        fn runtime(&self) -> &dyn Runtime {
            match self {
                Self::InProcess(node) => node.as_ref(),
                Self::Socket { client, .. } => client,
            }
        }

        /// The node's create seeds' counts, in process; the socket carries none.
        fn seeds(&self) -> Result<Option<seeds::Counts>, String> {
            match self {
                Self::InProcess(node) => node
                    .create_seeds()
                    .map(Some)
                    .map_err(|e| format!("the node's create seeds' counts: {e}")),
                Self::Socket { .. } => Ok(None),
            }
        }

        /// Direct preparation counters; the socket wire remains unchanged.
        fn dev_mounts(&self) -> Result<Option<hex_core::devfs::Counts>, String> {
            match self {
                Self::InProcess(node) => node.dev_mounts().map(Some).map_err(|e| e.to_string()),
                Self::Socket { .. } => Ok(None),
            }
        }

        /// The node's ipc and uts namespace counts, read in process, as its create seeds' are.
        fn ipc_uts(&self) -> Result<Option<netns::Counts>, String> {
            match self {
                Self::InProcess(node) => node.ipc_uts().map(Some).map_err(|e| e.to_string()),
                Self::Socket { .. } => Ok(None),
            }
        }

        /// The node's network namespace counts: a node method in process, a call over the socket.
        fn netns(&self) -> Result<netns::Counts, String> {
            match self {
                Self::InProcess(node) => node.netns(),
                Self::Socket { client, .. } => client.netns(),
            }
            .map_err(|e| format!("the node's network namespace counts: {e}"))
        }
    }

    /// What the node's network namespace counts rose by from `before` to `after`.
    fn rise(before: netns::Counts, after: netns::Counts) -> Value {
        json!({"made_ahead": after.made_ahead - before.made_ahead,
               "taken": after.taken - before.taken,
               "made_on_path": after.made_on_path - before.made_on_path,
               "discarded": after.discarded - before.discarded,
               "failed": after.failed - before.failed})
    }

    /// What the node's create seeds' counts rose by from `before` to `after`, and the most busy at once
    /// so far (a high-water mark, not a rise).
    fn seeds_rise(before: seeds::Counts, after: seeds::Counts) -> Value {
        json!({"asked": after.asked - before.asked,
               "waited": after.waited - before.waited,
               "refused": (after.refused_busy + after.refused_waited + after.refused_died + after.refused_hung)
                   - (before.refused_busy + before.refused_waited + before.refused_died + before.refused_hung),
               "started": after.started - before.started,
               "restarted": after.restarted - before.restarted,
               "start_failed": after.start_failed - before.start_failed,
               "died": after.died - before.died,
               "hung": after.hung - before.hung,
               "most_busy": after.most_busy})
    }

    pub(super) fn main() -> ExitCode {
        let args: Vec<String> = std::env::args_os()
            .skip(1)
            .map(|a| {
                a.into_string()
                    .unwrap_or_else(|a| panic!("argument {a:?} isn't Unicode"))
            })
            .filter(|a| a != "--bench")
            .collect();
        let ([out, n, warmup, lifecycles, hex, sleep_bundle, true_bundle]
        | [
            out,
            n,
            warmup,
            lifecycles,
            hex,
            sleep_bundle,
            true_bundle,
            _,
        ]) = args.as_slice()
        else {
            panic!(
                "usage: seam_oci <out.json> <n> <warmup> <lifecycles> <hex-binary> <sleep-bundle> <true-bundle> [<sleep-nonet-bundle>]"
            );
        };
        let nonet_bundle = args.get(7).map(PathBuf::from);
        let (n, warmup, lifecycles): (usize, usize, usize) = (
            n.parse().unwrap(),
            warmup.parse().unwrap(),
            lifecycles.parse().unwrap(),
        );
        let sustained_lifecycles = std::env::var("HEXBENCH_SUSTAINED")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map_or(SUSTAINED_DEFAULT, |v| {
                v.trim()
                    .parse()
                    .unwrap_or_else(|e| panic!("HEXBENCH_SUSTAINED={v:?}: {e}"))
            });
        let (sleep_bundle, true_bundle) = (PathBuf::from(sleep_bundle), PathBuf::from(true_bundle));
        // The box root beside the runtimes' container cgroups, under the vacated, delegated cgroup
        // root; the node agent itself stays where it is, and the parent's cpu.stat is the CPU a
        // throughput level used (it covers the agent and every container).
        let parent = box_root_parent();
        let cache =
            PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/ociseam");
        std::fs::create_dir_all(&cache).expect("the state's cache dir");
        let state = tempfile::Builder::new()
            .prefix("ociseam-")
            .tempdir_in(&cache)
            .expect("a state dir");
        let socket = match std::env::var("HEXBENCH_DRIVER").as_deref() {
            Ok("socket") => true,
            Ok("inprocess") | Err(_) => false,
            Ok(other) => panic!("HEXBENCH_DRIVER={other:?}: inprocess or socket"),
        };
        let (runtime, root) = if socket {
            ("hex-seam-socket", parent.join("hex-oci-seam-sock"))
        } else {
            ("hex-seam", parent.join("hex-oci-seam"))
        };
        // The socket driver's agent is a process of its own, as a caller that doesn't link Hex meets
        // it; the in-process driver's is this one.
        let driver = if socket {
            let (agent, path) = Agent::start(hex, state.path(), &root);
            let client = Client::connect(&path)
                .unwrap_or_else(|e| panic!("connecting to hex node at {}: {e}", path.display()));
            Driver::Socket {
                client,
                _agent: agent,
            }
        } else {
            let node = Node::open(&root, state.path(), state.path())
                .unwrap_or_else(|e| {
                    panic!(
                        "the node agent (box root {}; the oci pass runs this as root in its privileged container): {e}",
                        root.display()
                    )
                })
                .with_oci_helper(PathBuf::from(hex));
            let node = node
                .with_prepared_dev_mounts(std::env::var("HEXBENCH_DEVFS").as_deref() != Ok("0"))
                .unwrap_or_else(|e| panic!("the prepared /dev pool: {e}"));
            let node = match std::env::var("HEXBENCH_IPCUTS").as_deref() {
                Ok("0") => node
                    .with_ipc_uts_bound(0)
                    .unwrap_or_else(|e| panic!("draining the ipc and uts pool: {e}")),
                _ => node,
            };
            Driver::InProcess(Box::new(node))
        };
        let node = driver.runtime();
        let wanted = std::env::var("HEXBENCH_PHASES")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| {
                "lifecycle lifecycle_nonet start_true throughput sustained rtt".to_owned()
            });
        let trace = Trace::from_env();
        let (mut phases, mut errors) = (Map::new(), Map::new());
        for phase in [
            "lifecycle",
            "lifecycle_nonet",
            "start_true",
            "throughput",
            "sustained",
            "rtt",
            "ipc_uts_pairs",
        ] {
            if !wanted.split_whitespace().any(|w| w == phase) {
                continue;
            }
            let t0 = Instant::now();
            // The node's counts before and after the phase, outside its timing: what it added.
            let r = driver.netns().and_then(|before| {
                let seeds_before = driver.seeds()?;
                let dev_before = driver.dev_mounts()?;
                let ipc_uts_before = driver.ipc_uts()?;
                let mut v = match phase {
                    "lifecycle" => lifecycle(node, &sleep_bundle, n, warmup, "l", trace.as_ref()),
                    "lifecycle_nonet" => match &nonet_bundle {
                        Some(bundle) => lifecycle(node, bundle, n, warmup, "n", None),
                        None => Err("it needs the bundle without a network namespace, \
                                     seam_oci's eighth argument"
                            .to_owned()),
                    },
                    "start_true" => start_true(node, &true_bundle, n, warmup),
                    "throughput" => throughput(node, &true_bundle, &parent, lifecycles),
                    "sustained" => sustained(node, &true_bundle, &parent, sustained_lifecycles),
                    "ipc_uts_pairs" => ipc_uts_pairs(node, &parent, hex, &sleep_bundle, n, warmup),
                    _ => rtt(node, &sleep_bundle, n, warmup),
                }?;
                let rose = rise(before, driver.netns()?);
                let seeds_rose = match (seeds_before, driver.seeds()?) {
                    (Some(before), Some(after)) => Some(seeds_rise(before, after)),
                    _ => None,
                };
                if let Value::Object(fields) = &mut v {
                    fields.insert("netns".to_owned(), rose);
                    if let (Some(before), Some(after)) = (ipc_uts_before, driver.ipc_uts()?) {
                        fields.insert("ipc_uts".to_owned(), rise(before, after));
                    }
                    if let (Some(before), Some(after)) = (dev_before, driver.dev_mounts()?) {
                        fields.insert("dev_mounts".to_owned(), json!({
                            "made":after.made-before.made, "taken":after.taken-before.taken,
                            "on_path":after.on_path-before.on_path, "discarded":after.discarded-before.discarded,
                            "failed":after.failed-before.failed, "cold":after.cold-before.cold,
                            "dry":after.dry-before.dry, "disabled":after.disabled-before.disabled,
                            "after_failure":after.after_failure-before.after_failure,
                            "unpooled_missing":after.unpooled_missing-before.unpooled_missing,
                            "unpooled_devices":after.unpooled_devices-before.unpooled_devices,
                            "unpooled_duplicate":after.unpooled_duplicate-before.unpooled_duplicate,
                            "unpooled_kind":after.unpooled_kind-before.unpooled_kind,
                            "unpooled_readonly":after.unpooled_readonly-before.unpooled_readonly,
                            "unpooled_inodes":after.unpooled_inodes-before.unpooled_inodes,
                            "unpooled_key_bytes":after.unpooled_key_bytes-before.unpooled_key_bytes,
                            "unpooled_capacity":after.unpooled_capacity-before.unpooled_capacity,
                            "unpooled_user_namespace":after.unpooled_user_namespace-before.unpooled_user_namespace,
                            "failed_filesystem":after.failed_filesystem-before.failed_filesystem,
                            "failed_device":after.failed_device-before.failed_device,
                            "failed_link":after.failed_link-before.failed_link,
                            "held_after":after.made-after.taken-after.discarded,
                        }));
                    }
                    if let Some(seeds_rose) = seeds_rose {
                        fields.insert("seeds".to_owned(), seeds_rose);
                    }
                }
                Ok(v)
            });
            let note = r
                .as_ref()
                .err()
                .map(|e| format!(" ERROR {e}"))
                .unwrap_or_default();
            println!(
                "{runtime} {phase}: {:.0}s{note}",
                t0.elapsed().as_secs_f64()
            );
            match r {
                Ok(v) => phases.insert(phase.to_owned(), v),
                Err(e) => errors.insert(phase.to_owned(), Value::String(e)),
            };
        }
        drop(driver);
        let _ = std::fs::remove_dir(&root);
        let result = json!({"runtime": runtime, "n": n, "warmup": warmup,
                            "phases": phases, "errors": errors, "cgroup": parent});
        std::fs::write(out, result.to_string()).expect("writing the result");
        ExitCode::SUCCESS
    }
}
