//! The `oci` class through the hex-api `Runtime` seam, in process: oci_bench.py's lifecycle and
//! throughput phases (create, start, exec of `true` and delete of a `sleep` container, each timed
//! alone; then create+start+delete at concurrency 1, 8 and 32) with the node agent in this process,
//! so no call spawns a process but the container's own init and exec helper. It writes oci_bench.py's
//! JSON under the runtime `hex-seam`, which summarize.py sets beside libcrun (the in-process rival),
//! so runtime design goal 3's targets — exec p50 at most a quarter of libcrun's, start p50 within
//! 10% of libcrun's (argument A3) — are read from it. Decision 3 of docs/design/runtime-architecture.md.
//!
//!   seam_oci <out.json> <n> <warmup> <lifecycles> <hex-binary> <sleep-bundle> <true-bundle>
//! (`cargo bench -p hex-bench --bench seam_oci -- ...`, or its binary as host.sh's ociseam pass runs
//! it). HEXBENCH_PHASES names the phases to run. The `oci` class needs root (a container's cgroup
//! device program is an eBPF program, and its namespaces are made without a user namespace), so this
//! runs in the OCI pass's privileged container, where the cgroup root is vacated to a leaf and its
//! controllers handed down, as oci_inside.sh leaves it; off that it refuses, naming why.
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
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Exit, Program, Runtime, Stdio};
    use hex_node::Node;
    use serde_json::{Map, Value, json};

    /// Each call's bound, as oci_bench.py's CALL_TIMEOUT_S.
    const CALL_TIMEOUT: Duration = Duration::from_secs(60);

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

    /// One container's create, start, exec of `true` and delete, each timed alone; a slot for the op
    /// that failed records the error. The init is the bundle's `sleep infinity`, so exec runs beside it.
    fn lifecycle_once(
        node: &Node,
        bundle: &Path,
        id: &str,
        ms: &mut Ops,
        timed: bool,
    ) -> Result<(), String> {
        let bid = BoxId::new(id).map_err(|e| e.to_string())?;
        let create = Instant::now();
        node.create(spec(&bid, bundle))
            .map_err(|e| format!("create {id}: {e}"))?;
        let create = create.elapsed();
        // start and exec, timed; delete runs whether or not they did, so a box is never left behind.
        let inner = (|| -> Result<(Duration, Duration), String> {
            let start = Instant::now();
            node.start(&bid).map_err(|e| format!("start {id}: {e}"))?;
            let start = start.elapsed();
            let exec = Instant::now();
            let mut p = node
                .exec(&bid, Program::of(&["/usr/bin/true"]), Stdio::null())
                .map_err(|e| format!("exec in {id}: {e}"))?;
            let ended = p
                .wait_for(CALL_TIMEOUT)
                .map_err(|e| format!("wait in {id}: {e}"))?
                .ok_or_else(|| format!("`true` in {id} still running after {CALL_TIMEOUT:?}"))?;
            let exec = exec.elapsed();
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

    /// n timed lifecycles after `warmup` untimed ones, each operation timed alone.
    fn lifecycle(node: &Node, bundle: &Path, n: usize, warmup: usize) -> Result<Value, String> {
        let mut ms = Ops::default();
        for i in 0..warmup + n {
            lifecycle_once(node, bundle, &format!("l{i:06}"), &mut ms, i >= warmup)?;
        }
        Ok(
            json!({"ms": {"create": ms.create, "start": ms.start, "exec": ms.exec, "delete": ms.delete}}),
        )
    }

    /// create+start+delete of a `true` container, at each concurrency that many threads share the
    /// node, each its share of the lifecycles; the clock starts once every thread is at the barrier.
    /// `own` is the subtree the agent and its boxes share, whose cpu.stat the per-lifecycle CPU is.
    fn throughput(
        node: &Node,
        bundle: &Path,
        own: &Path,
        lifecycles: usize,
    ) -> Result<Value, String> {
        let mut out = Map::new();
        for conc in [1usize, 8, 32] {
            let per = (lifecycles / conc).max(1);
            let start = Barrier::new(conc + 1);
            let (wall, cpu_s, errs) = {
                std::thread::scope(|s| {
                    let threads: Vec<_> = (0..conc)
                        .map(|w| {
                            let start = &start;
                            s.spawn(move || {
                                start.wait();
                                (0..per).try_for_each(|j| {
                                    run_true_container(node, bundle, &format!("t{conc}-{w}-{j:06}"))
                                })
                            })
                        })
                        .collect();
                    start.wait();
                    let (c0, t0) = (cpu_usec(own), Instant::now());
                    let errs: Vec<String> = threads
                        .into_iter()
                        .filter_map(|t| {
                            t.join()
                                .unwrap_or_else(|_| Err("a thread panicked".to_owned()))
                                .err()
                        })
                        .collect();
                    (t0.elapsed(), (cpu_usec(own) - c0) as f64 / 1e6, errs)
                })
            };
            if let Some(e) = errs.into_iter().next() {
                return Err(format!("concurrency {conc}: {e}"));
            }
            let total = per * conc;
            out.insert(
                conc.to_string(),
                json!({"lifecycles": total, "wall_s": wall.as_secs_f64(),
                       "per_s": total as f64 / wall.as_secs_f64(),
                       "cpu_s_per_lifecycle": cpu_s / total as f64}),
            );
        }
        Ok(Value::Object(out))
    }

    /// create+start+delete of a `true` container, with no exec: the throughput lifecycle.
    fn run_true_container(node: &Node, bundle: &Path, id: &str) -> Result<(), String> {
        let bid = BoxId::new(id).map_err(|e| e.to_string())?;
        node.create(spec(&bid, bundle))
            .map_err(|e| format!("create {id}: {e}"))?;
        let started = node.start(&bid).map_err(|e| format!("start {id}: {e}"));
        node.delete(&bid).map_err(|e| format!("delete {id}: {e}"))?;
        started
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
        let [out, n, warmup, lifecycles, hex, sleep_bundle, true_bundle] = args.as_slice() else {
            panic!(
                "usage: seam_oci <out.json> <n> <warmup> <lifecycles> <hex-binary> <sleep-bundle> <true-bundle>"
            );
        };
        let (n, warmup, lifecycles): (usize, usize, usize) = (
            n.parse().unwrap(),
            warmup.parse().unwrap(),
            lifecycles.parse().unwrap(),
        );
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
        let root = parent.join("hex-oci-seam");
        let node = Node::open(&root, state.path(), state.path())
            .unwrap_or_else(|e| {
                panic!(
                    "the node agent (box root {}; the oci pass runs this as root in its privileged container): {e}",
                    root.display()
                )
            })
            .with_oci_helper(PathBuf::from(hex));
        let wanted = std::env::var("HEXBENCH_PHASES")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| "lifecycle throughput".to_owned());
        let (mut phases, mut errors) = (Map::new(), Map::new());
        for phase in ["lifecycle", "throughput"] {
            if !wanted.split_whitespace().any(|w| w == phase) {
                continue;
            }
            let t0 = Instant::now();
            let r = match phase {
                "lifecycle" => lifecycle(&node, &sleep_bundle, n, warmup),
                _ => throughput(&node, &true_bundle, &parent, lifecycles),
            };
            let note = r
                .as_ref()
                .err()
                .map(|e| format!(" ERROR {e}"))
                .unwrap_or_default();
            println!("hex-seam {phase}: {:.0}s{note}", t0.elapsed().as_secs_f64());
            match r {
                Ok(v) => phases.insert(phase.to_owned(), v),
                Err(e) => errors.insert(phase.to_owned(), Value::String(e)),
            };
        }
        drop(node);
        let _ = std::fs::remove_dir(&root);
        let result = json!({"runtime": "hex-seam", "n": n, "warmup": warmup,
                            "phases": phases, "errors": errors, "cgroup": parent});
        std::fs::write(out, result.to_string()).expect("writing the result");
        ExitCode::SUCCESS
    }
}
