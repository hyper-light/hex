//! ROOT-only Linux stand-in controls for the actual Supervisor publication/exec gate.
//! No VM/GPU claim. ROOT registers this leaf only after the matching native-scope cohort.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio as ChildStdio};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use hex_api::{BoxId, BoxSpec, Class, Exit, Program, Refusal, Stdio};
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_runtime::devices::managed_scope_id;
use hex_runtime::linux::cgroup::{BoxCgroup, BoxRoot, CGROUP_ROOT};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::{Pid, PidfdFlags, Signal, WaitId, WaitIdOptions};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use shards_api::{ManagedProcess, ManagedStatus, TerminalScope};

use super::{ManagedVm, Stat, Supervisor};
use crate::store::Store;

type Check<T> = Result<T, String>;
const READY: &str = "hex-completion-gate-ready ";
const PARENT_CASE: &str =
    "supervise::completion_native_gate_tests::completion_controller_parent_death_cancels_gate";

fn text<T, E: std::fmt::Display>(value: Result<T, E>) -> Check<T> {
    value.map_err(|error| error.to_string())
}

fn need(value: bool, reason: &str) -> Check<()> {
    if value {
        Ok(())
    } else {
        Err(reason.to_owned())
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    delegated_parent: PathBuf,
    lab: PathBuf,
    spec: BoxSpec,
    controller: Program,
    controller_sha256: String,
    test_binary_sha256: String,
    pidfd_budget: usize,
    guard_ms: u64,
    gate_budget_ms: u64,
    observation_ms: u64,
    /// ROOT supplies the matching kernel ABI's ppoll syscall number, never a guessed architecture.
    gate_wait_syscall: u64,
    byte_limit: u64,
}

impl Config {
    fn load() -> Check<Self> {
        need(
            std::env::var("HEX_COMPLETION_ROOT_AUTHORIZED").as_deref() == Ok("1"),
            "ROOT authorization required",
        )?;
        let limit = text(text(std::env::var("HEX_COMPLETION_GATE_BYTES"))?.parse::<u64>())?;
        need(limit > 0, "positive configuration byte budget required")?;
        let path = PathBuf::from(text(
            std::env::var_os("HEX_COMPLETION_GATE_CONFIG").ok_or("missing gate config"),
        )?);
        let config: Self = text(serde_json::from_slice(&read_bounded(&path, limit)?))?;
        need(
            config.byte_limit == limit
                && config.guard_ms > 0
                && config.gate_budget_ms > config.guard_ms
                && config.observation_ms > 0
                && config.observation_ms <= config.guard_ms
                && config.pidfd_budget >= 2,
            "explicit finite ROOT bounds required; gate budget must exceed guard",
        )?;
        deadline(config.guard())?;
        deadline(config.gate())?;
        text(config.spec.check())?;
        for suffix in ["first", "sibling", "published", "refused"] {
            text(BoxId::new(&format!("{}-{suffix}", config.spec.id.as_str())))?;
        }
        text(config.controller.check())?;
        need(
            (matches!(
                config.spec.class.class(),
                Class::Shards | Class::Firecracker
            ) || (config.spec.class.class() == Class::Oci && config.spec.allocation.is_some()))
                && config.spec.devices.is_empty()
                && config.spec.bounds.cpu_max_milli.is_some()
                && config.spec.bounds.memory_max.is_some()
                && config.spec.bounds.pids_max.is_some(),
            "bounded managed record required for Store's native scope; controller remains a stand-in",
        )?;
        let parent = text(config.delegated_parent.canonicalize())?;
        need(
            parent.starts_with(CGROUP_ROOT) && parent != Path::new(CGROUP_ROOT),
            "private delegated cgroup parent required",
        )?;
        need(
            text(config.lab.canonicalize())?.is_dir(),
            "existing ROOT private lab required",
        )?;
        let executable = config
            .controller
            .args
            .first()
            .ok_or("controller argv missing")?;
        need(
            Path::new(executable).is_absolute(),
            "absolute configured stand-in required",
        )?;
        need(
            digest(Path::new(executable))? == config.controller_sha256,
            "stand-in source/binary pin mismatch",
        )?;
        need(
            digest(&text(std::env::current_exe())?)? == config.test_binary_sha256,
            "native fixture binary pin mismatch",
        )?;
        Ok(config)
    }

    fn guard(&self) -> Duration {
        Duration::from_millis(self.guard_ms)
    }
    fn gate(&self) -> Duration {
        Duration::from_millis(self.gate_budget_ms)
    }
    fn interval(&self) -> Duration {
        Duration::from_millis(self.observation_ms)
    }
}

#[derive(Serialize, Deserialize)]
struct GateIdentity {
    pid: u32,
    started: u64,
    vsock: PathBuf,
}

impl GateIdentity {
    fn native(self) -> shards_api::Identity {
        shards_api::Identity {
            pid: self.pid,
            started: self.started,
            vsock: self.vsock,
        }
    }
}

fn digest(path: &Path) -> Check<String> {
    let mut file = text(File::open(path))?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 8192];
    loop {
        let count = text(file.read(&mut bytes))?;
        if count == 0 {
            break;
        }
        digest.update(bytes.get(..count).ok_or("invalid file read count")?);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn read_bounded(path: &Path, limit: u64) -> Check<Vec<u8>> {
    let mut bytes = Vec::new();
    text(
        text(File::open(path))?
            .take(limit.checked_add(1).ok_or("byte budget overflow")?)
            .read_to_end(&mut bytes),
    )?;
    need(
        u64::try_from(bytes.len()).map_err(|error| error.to_string())? <= limit,
        "artifact byte budget exceeded",
    )?;
    Ok(bytes)
}

fn deadline(budget: Duration) -> Check<Instant> {
    Instant::now()
        .checked_add(budget)
        .ok_or_else(|| "deadline overflow".to_owned())
}

fn until<F: FnMut() -> Check<bool>>(config: &Config, end: Instant, mut observed: F) -> Check<()> {
    loop {
        if observed()? {
            return Ok(());
        }
        let left = end.saturating_duration_since(Instant::now());
        need(
            !left.is_zero(),
            "owned observation guard expired; retain as harness uncertainty",
        )?;
        std::thread::sleep(config.interval().min(left));
    }
}

fn poll_exit(fd: &OwnedFd, end: Instant) -> Check<()> {
    loop {
        let left = end.saturating_duration_since(Instant::now());
        need(!left.is_zero(), "exact pidfd exit guard expired")?;
        let timeout = Timespec {
            tv_sec: text(i64::try_from(left.as_secs()))?,
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [PollFd::new(fd, PollFlags::IN)];
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(_)
                if fds
                    .first()
                    .is_some_and(|fd| fd.revents().contains(PollFlags::IN)) =>
            {
                return Ok(());
            }
            Ok(_) | Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn new_scope(config: &Config) -> Check<PathBuf> {
    Ok(text(
        tempfile::Builder::new()
            .prefix("gate-")
            .tempdir_in(&config.lab),
    )?
    .keep())
}

fn cgroup_path(config: &Config, scope: &Path) -> Check<PathBuf> {
    Ok(config
        .delegated_parent
        .join(scope.file_name().ok_or("private scope name missing")?))
}

struct Lab {
    config: Config,
    scope: PathBuf,
    groups: BoxRoot,
    store: Store,
    supervisor: Arc<Supervisor>,
    intents: Vec<BoxRecord>,
    natives: Vec<(BoxRecord, triomphe::Arc<ManagedVm>)>,
}

impl Lab {
    fn open(config: Config, scope: PathBuf) -> Check<Self> {
        let native_root = text(BoxRoot::open(&cgroup_path(&config, &scope)?, Vec::new()))?;
        let parent = text(BoxRoot::open(&native_root.dir().join(".vm"), Vec::new()))?;
        let groups = text(BoxRoot::open(&parent.dir().join("gate-owner"), Vec::new()))?;
        let store = text(Store::open(&scope.join("records"), &scope.join("run")))?;
        let supervisor = text(Supervisor::start(
            native_root.dir(),
            store.clone(),
            config.pidfd_budget,
        ))?;
        let lease = text(
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(scope.join("native-vm-owner.lock")),
        )?;
        text(lease.try_lock())?;
        text(supervisor.bind_managed_root(groups.dir(), lease))?;
        Ok(Self {
            config,
            scope,
            groups,
            store,
            supervisor,
            intents: Vec::new(),
            natives: Vec::new(),
        })
    }

    fn reserve(
        &mut self,
        suffix: &str,
    ) -> Check<(
        BoxRecord,
        triomphe::Arc<ManagedVm>,
        PathBuf,
        String,
        Program,
    )> {
        let mut spec = self.config.spec.clone();
        spec.id = text(BoxId::new(&format!("{}-{suffix}", spec.id.as_str())))?;
        // The production Store alone generates the incarnation on its FIRST
        // Creating publication; this fixture neither injects nor regenerates it.
        let intent = BoxRecord::new(spec.clone(), Phase::Creating, vec![self.scope.clone()]);
        text(self.store.begin(&intent))?;
        let record =
            text(self.store.read_lifecycle(&spec.id))?.ok_or("fresh Creating record missing")?;
        need(
            record.phase == Phase::Creating && record.native_scope.is_some(),
            "Store did not introduce native scope before effects",
        )?;
        self.intents.push(record.clone());
        text(fs::write(
            self.scope.join(format!("{suffix}.creating.record.json")),
            text(record.encode())?,
        ))?;
        let group = text(
            self.groups
                .create(&text(managed_scope_id(&record))?, &spec.bounds),
        )?;
        let native = match self.supervisor.reserve_managed(
            hex_core::record::NativeRecord::Box(&record),
            group.clone(),
            &self.scope.join(format!("{suffix}.sock")),
        ) {
            Ok(native) => native,
            Err(error) => {
                let cleanup = group.remove();
                return Err(format!(
                    "reserve: {error}; unsubmitted group cleanup: {cleanup:?}"
                ));
            }
        };
        self.natives
            .push((record.clone(), triomphe::Arc::clone(&native)));
        need(
            Some(native.scope) == record.native_scope
                && native.commitment == text(record.native_commitment())?,
            "native binding does not retain the original scope/spec commitment",
        )?;
        let marker = self.scope.join(format!("{suffix}.marker"));
        need(
            !text(marker.try_exists())?,
            "marker existed before owned launch",
        )?;
        let token = format!(
            "{}-{suffix}",
            self.scope
                .file_name()
                .ok_or("scope name missing")?
                .to_string_lossy()
        );
        let mut program = self.config.controller.clone();
        for arg in &mut program.args {
            *arg = arg
                .replace("@MARKER@", &marker.to_string_lossy())
                .replace("@TOKEN@", &token)
                .replace("@SCOPE@", &self.scope.to_string_lossy());
        }
        need(
            program
                .args
                .iter()
                .any(|arg| arg.contains(&*marker.to_string_lossy()))
                && program.args.iter().any(|arg| arg.contains(&token)),
            "stand-in must receive exact marker path and token",
        )?;
        Ok((record, native, marker, token, program))
    }

    fn publish(&self, original: &BoxRecord, identity: &shards_api::Identity) -> Check<()> {
        let before = self.store.syncs();
        let mut record = original.clone();
        record.init = Some(ProcId {
            pid: identity.pid,
            start_ticks: identity.started,
        });
        text(self.store.put_lifecycle(&record))?;
        text(self.store.made(&record))?;
        record.phase = Phase::Ready;
        let after = self.store.syncs();
        need(
            after.files > before.files && after.dirs > before.dirs,
            "publication was not durably fsynced; tmpfs is not a durable control",
        )?;
        let read = text(self.store.get(&record.spec.id))?.ok_or("publication record missing")?;
        need(
            text(read)? == record,
            "durable publication readback differs",
        )
    }

    fn cleanup(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for original in &self.intents {
            let result = (|| -> Check<()> {
                let mut record = text(self.store.read_lifecycle(&original.spec.id))?
                    .ok_or("owned scope record missing during cleanup")?;
                need(
                    record.native_scope == original.native_scope
                        && text(record.native_commitment())? == text(original.native_commitment())?,
                    "cleanup original scope/spec changed",
                )?;
                let native = self
                    .natives
                    .iter()
                    .find(|(known, _)| known.spec.id == original.spec.id)
                    .map(|(_, native)| native);
                let status = if let Some(native) = native {
                    text(native.stop())?;
                    let end = text(native.wait_for(self.config.guard()))?
                        .ok_or("native cleanup remains unverified")?;
                    need(
                        end.scope == TerminalScope::Released,
                        "controller exit is not complete native release",
                    )?;
                    match end.status {
                        ManagedStatus::Code(code) => Exit::Code(code),
                        ManagedStatus::Signal(signal) => Exit::Signal(signal),
                        ManagedStatus::Unknown => Exit::Unknown,
                    }
                } else {
                    let group = BoxCgroup::at(
                        self.groups
                            .dir()
                            .join(text(managed_scope_id(original))?.as_str()),
                    );
                    if text(group.populated_if_present())?.is_some() {
                        text(group.kill())?;
                        need(
                            text(group.wait_empty(self.config.guard()))?,
                            "unsubmitted owned scope remains populated",
                        )?;
                        text(group.remove())?;
                    }
                    Exit::Unknown
                };
                record.phase = Phase::Deleting;
                record.exit = Some(status);
                text(self.store.put_lifecycle(&record))?;
                if let Some(native) = native {
                    text(self.supervisor.finalize_managed(
                        hex_core::record::NativeRecord::Box(&record),
                        native.identity().ok(),
                    ))?;
                }
                text(self.store.remove(&record.spec.id))?;
                need(
                    text(self.store.get(&record.spec.id))?.is_none(),
                    "owned record remains after release",
                )
            })();
            if let Err(error) = result {
                errors.push(format!("{}: {error}", original.spec.id.as_str()));
            }
        }
        if errors.is_empty() {
            let mut paths = vec![self.groups.dir().to_path_buf()];
            match self.groups.dir().parent() {
                Some(parent) => paths.push(parent.to_path_buf()),
                None => errors.push("managed parent root is unobserved".to_owned()),
            }
            match cgroup_path(&self.config, &self.scope) {
                Ok(root) => paths.push(root),
                Err(error) => errors.push(error),
            }
            for path in paths {
                if let Err(error) = fs::remove_dir(&path) {
                    errors.push(format!(
                        "private cgroup root cleanup {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        errors
    }
}

fn record_result(scope: &Path, case: &str, observed: &Check<()>, cleanup: &[String]) -> Check<()> {
    text(fs::write(
        scope.join("gate-result.json"),
        text(serde_json::to_vec_pretty(&json!({
            "schema":"hex-completion-native-gate-1", "case":case, "scope":"root-native-standin",
            "vm_gpu_claim":false, "observation":observed, "cleanup_uncertainty":cleanup
        })))?,
    ))
}

/// AC-T7.1.2: publication refusal must cancel the actual native gate before any foreign exec.
#[test]
#[ignore = "ROOT only: requires configured private delegated cgroup, durable lab and finite bounds"]
fn completion_publication_refusal_never_executes_marker() {
    let result = (|| -> Check<()> {
        let config = Config::load()?;
        let scope = new_scope(&config)?;
        let mut lab = Lab::open(config, scope.clone())?;
        let observed = (|| -> Check<()> {
            let (_, native, marker, _, program) = lab.reserve("refused")?;
            let mut published = false;
            let result = lab.supervisor.bind_managed(
                &native,
                &program,
                Stdio::null(),
                None,
                lab.config.gate(),
                &mut |_| {
                    published = true;
                    Err(Refusal::Broken(
                        "ROOT publication-refusal control".to_owned(),
                    ))
                },
            );
            need(
                published
                    && matches!(result, Err(Refusal::Broken(ref why)) if why == "ROOT publication-refusal control"),
                "publication refusal was not the actual cancellation source",
            )?;
            let end = text(native.wait_for(lab.config.guard()))?
                .ok_or("refused native controller did not terminate")?;
            need(
                end.scope == TerminalScope::Released && end.status == ManagedStatus::Code(127),
                "gate cancellation exit/scope mismatch",
            )?;
            need(
                !text(marker.try_exists())?,
                "refused publication executed the marker",
            )
        })();
        let cleanup = lab.cleanup();
        record_result(&scope, "publication-refused", &observed, &cleanup)?;
        observed?;
        need(
            cleanup.is_empty(),
            &format!("cleanup uncertainty: {cleanup:?}"),
        )
    })();
    assert!(result.is_ok(), "{result:?}");
}

/// AC-T7.1.2: the same callback durably publishes before release, then executes and reaps.
#[test]
#[ignore = "ROOT only: requires configured private delegated cgroup, durable lab and finite bounds"]
fn completion_durable_publication_releases_and_reaps() {
    let result = (|| -> Check<()> {
        let config = Config::load()?;
        let scope = new_scope(&config)?;
        let mut lab = Lab::open(config, scope.clone())?;
        let observed = (|| -> Check<()> {
            let (spec, native, marker, token, program) = lab.reserve("published")?;
            let mut published = false;
            text(lab.supervisor.bind_managed(
                &native,
                &program,
                Stdio::null(),
                None,
                lab.config.gate(),
                &mut |identity| {
                    let result = (|| -> Check<()> {
                        need(
                            !text(marker.try_exists())?,
                            "stand-in ran before durable publication",
                        )?;
                        lab.publish(&spec, identity)?;
                        published = true;
                        Ok(())
                    })();
                    result.map_err(Refusal::Broken)
                },
            ))?;
            let end = text(native.wait_for(lab.config.guard()))?
                .ok_or("published native controller did not terminate")?;
            need(
                published
                    && end.scope == TerminalScope::Released
                    && end.status == ManagedStatus::Code(0),
                "positive gate execution/reap mismatch",
            )?;
            need(
                read_bounded(&marker, lab.config.byte_limit)? == token.as_bytes(),
                "stand-in marker output mismatch",
            )
        })();
        let cleanup = lab.cleanup();
        record_result(&scope, "durable-publication", &observed, &cleanup)?;
        observed?;
        need(
            cleanup.is_empty(),
            &format!("cleanup uncertainty: {cleanup:?}"),
        )
    })();
    assert!(result.is_ok(), "{result:?}");
}

fn exact_pidfd(identity: &shards_api::Identity, parent: u32) -> Check<OwnedFd> {
    let pid = text(i32::try_from(identity.pid))?;
    let pidfd = text(rustix::process::pidfd_open(
        Pid::from_raw(pid).ok_or("invalid owned PID")?,
        PidfdFlags::empty(),
    ))?;
    // Validate after opening: a PID reused between a prior /proc read and
    // pidfd_open must never grant this fixture signal authority over its reuse.
    let stat = Stat::of(pid).ok_or("owned process identity unavailable")?;
    need(
        stat.start_ticks == identity.started && stat.ppid == text(i32::try_from(parent))?,
        "owned process PID/start/parent mismatch",
    )?;
    Ok(pidfd)
}

fn wait_gate(config: &Config, identity: &shards_api::Identity) -> Check<()> {
    until(config, deadline(config.guard())?, || {
        let syscall = read_bounded(
            &PathBuf::from(format!("/proc/{}/syscall", identity.pid)),
            config.byte_limit,
        )?;
        let text = text(std::str::from_utf8(&syscall))?;
        Ok(text
            .split_whitespace()
            .next()
            .and_then(|number| number.parse::<u64>().ok())
            == Some(config.gate_wait_syscall))
    })
}

fn parent_body(config: Config, scope: PathBuf) -> Check<()> {
    let mut lab = Lab::open(config, scope.clone())?;
    let stop = Arc::new((Mutex::new(false), Condvar::new()));
    let mut workers = Vec::new();
    let (ready_tx, ready_rx) = mpsc::sync_channel(2);
    let observed = (|| -> Check<()> {
        let mut identities = Vec::new();
        for suffix in ["first", "sibling"] {
            let (_, native, _, _, program) = lab.reserve(suffix)?;
            let supervisor = Arc::clone(&lab.supervisor);
            let stop = Arc::clone(&stop);
            let ready = ready_tx.clone();
            let guard = lab.config.guard();
            let gate = lab.config.gate();
            workers.push(text(
                std::thread::Builder::new()
                    .name(format!("gate-{suffix}"))
                    .spawn(move || {
                        supervisor
                            .bind_managed(
                                &native,
                                &program,
                                Stdio::null(),
                                None,
                                gate,
                                &mut |identity| {
                                    ready
                                        .send(identity.clone())
                                        .map_err(|error| Refusal::Broken(error.to_string()))?;
                                    let (lock, changed) = &*stop;
                                    let stopped = lock
                                        .lock()
                                        .map_err(|error| Refusal::Broken(error.to_string()))?;
                                    drop(
                                        changed
                                            .wait_timeout_while(stopped, guard, |stopped| !*stopped)
                                            .map_err(|error| Refusal::Broken(error.to_string()))?,
                                    );
                                    Err(Refusal::Broken("ROOT parent held publication".to_owned()))
                                },
                            )
                            .map(|_| ())
                    }),
            )?);
            // Start the sibling only AFTER the first bind owns its writer; otherwise
            // the fork may never inherit the descriptor the isolation control targets.
            let identity = text(ready_rx.recv_timeout(lab.config.guard()))?;
            wait_gate(&lab.config, &identity)?;
            identities.push(identity);
        }
        let sibling = identities.get(1).ok_or("sibling identity missing")?;
        let pidfd = exact_pidfd(sibling, std::process::id())?;
        text(rustix::process::pidfd_send_signal(&pidfd, Signal::STOP))?;
        until(&lab.config, deadline(lab.config.guard())?, || {
            Ok(text(rustix::process::waitid(
                WaitId::PidFd(pidfd.as_fd()),
                WaitIdOptions::STOPPED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            ))?
            .is_some())
        })?;
        let identities: Vec<_> = identities
            .into_iter()
            .map(|identity| GateIdentity {
                pid: identity.pid,
                started: identity.started,
                vsock: identity.vsock,
            })
            .collect();
        let raw = text(serde_json::to_string(
            &json!({"identities":identities,"sibling_stopped":true}),
        ))?;
        let mut output = std::io::stdout().lock();
        text(writeln!(output, "{READY}{raw}"))?;
        text(output.flush())?;
        // The outer fixture kills this exact parent after readiness. This guard
        // owns every alternative branch; it never releases the stand-in gate.
        let (lock, changed) = &*stop;
        let stopped = text(lock.lock())?;
        drop(text(changed.wait_timeout_while(
            stopped,
            lab.config.guard(),
            |stopped| !*stopped,
        ))?);
        Err("outer parent-death control did not kill its owned parent".to_owned())
    })();
    if let Ok(mut stopped) = stop.0.lock() {
        *stopped = true;
    }
    stop.1.notify_all();
    for (_, native) in &lab.natives {
        let _ = native.stop();
    }
    let mut joins = Vec::new();
    for worker in workers {
        joins.push(
            worker
                .join()
                .map_err(|_| "owned gate worker panicked".to_owned()),
        );
    }
    let cleanup = lab.cleanup();
    record_result(&scope, "controller-parent-actor", &observed, &cleanup)?;
    observed?;
    need(
        joins.iter().all(Result::is_ok) && cleanup.is_empty(),
        "parent actor cleanup uncertain",
    )
}

fn read_ready(
    output: &mut std::process::ChildStdout,
    config: &Config,
    raw: &mut Vec<u8>,
) -> Check<serde_json::Value> {
    let end = deadline(config.guard())?;
    text(rustix::fs::fcntl_setfl(
        output.as_fd(),
        rustix::fs::OFlags::NONBLOCK,
    ))?;
    loop {
        let mut bytes = [0; 4096];
        match output.read(&mut bytes) {
            Ok(0) => return Err("owned parent ended before held-child readiness".to_owned()),
            Ok(count) => {
                raw.extend_from_slice(bytes.get(..count).ok_or("invalid readiness read count")?);
                need(
                    text(u64::try_from(raw.len()))? <= config.byte_limit,
                    "parent output byte budget exceeded",
                )?;
                let text = text(std::str::from_utf8(raw))?;
                for line in text.split_inclusive('\n') {
                    if line.ends_with('\n')
                        && let Some(value) = line.strip_prefix(READY)
                    {
                        return text_result(value);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let left = end.saturating_duration_since(Instant::now());
                need(!left.is_zero(), "parent readiness guard expired")?;
                let timeout = Timespec {
                    tv_sec: text(i64::try_from(left.as_secs()))?,
                    tv_nsec: i64::from(left.subsec_nanos()),
                };
                let mut fds = [PollFd::new(&*output, PollFlags::IN | PollFlags::HUP)];
                text(rustix::event::poll(&mut fds, Some(&timeout)))?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn text_result(raw: &str) -> Check<serde_json::Value> {
    text(serde_json::from_str(raw))
}

fn child_end(child: &mut Child, config: &Config) -> Check<()> {
    until(config, deadline(config.guard())?, || {
        Ok(text(child.try_wait())?.is_some())
    })
}

/// AC-T7.1.2: parent death produces EOF before the gate deadline, with a stopped sibling
/// unable to hide a foreign writer by exiting first. Only this fixture's exact PIDs are signaled.
#[test]
#[ignore = "ROOT only: same-binary controller parent, configured ppoll ABI and private delegated cgroup"]
fn completion_controller_parent_death_cancels_gate() {
    let result = (|| -> Check<()> {
        let config = Config::load()?;
        if std::env::var("HEX_COMPLETION_GATE_ROLE").as_deref() == Ok("controller-parent") {
            return parent_body(
                config,
                PathBuf::from(text(std::env::var("HEX_COMPLETION_GATE_SCOPE"))?),
            );
        }
        let scope = new_scope(&config)?;
        let group_root = cgroup_path(&config, &scope)?;
        need(
            !text(group_root.try_exists())?,
            "private cgroup root existed before fixture",
        )?;
        let prior_subreaper = text(rustix::process::child_subreaper())?;
        let mut command = Command::new(text(std::env::current_exe())?);
        command
            .args([
                "--ignored",
                "--exact",
                PARENT_CASE,
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HEX_COMPLETION_GATE_ROLE", "controller-parent")
            .env("HEX_COMPLETION_GATE_SCOPE", &scope)
            .stdin(ChildStdio::null())
            .stdout(ChildStdio::piped())
            .stderr(text(File::create(scope.join("parent.stderr")))?);
        text(rustix::process::set_child_subreaper(Some(
            rustix::process::getpid(),
        )))?;
        let began = Instant::now();
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let restore = rustix::process::set_child_subreaper(prior_subreaper);
                return Err(format!(
                    "parent spawn: {error}; subreaper restore: {restore:?}"
                ));
            }
        };
        let mut owned = Vec::new();
        let mut raw = Vec::new();
        let observed = (|| -> Check<()> {
            let mut output = child.stdout.take().ok_or("owned parent output missing")?;
            let ready = read_ready(&mut output, &config, &mut raw)?;
            need(
                ready
                    .get("sibling_stopped")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true),
                "sibling stop was not observed",
            )?;
            let identities: Vec<GateIdentity> = text(serde_json::from_value(
                ready
                    .get("identities")
                    .cloned()
                    .ok_or("owned identities missing")?,
            ))?;
            let identities: Vec<_> = identities.into_iter().map(GateIdentity::native).collect();
            need(identities.len() == 2, "exactly two held stand-ins required")?;
            for identity in &identities {
                owned.push(exact_pidfd(identity, child.id())?);
            }
            text(child.kill())?;
            child_end(&mut child, &config)?;
            let first = owned.first().ok_or("first pidfd missing")?;
            let earliest_gate = began
                .checked_add(config.gate())
                .ok_or("gate deadline overflow")?;
            poll_exit(first, deadline(config.guard())?.min(earliest_gate))?;
            need(
                Instant::now() < earliest_gate,
                "gate timing window missed; cannot attribute exit to EOF",
            )?;
            let status = text(rustix::process::waitid(
                WaitId::PidFd(first.as_fd()),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
            ))?
            .ok_or("first held stand-in was not reaped")?;
            need(
                status.exit_status() == Some(127),
                "first held stand-in did not cancel before exec",
            )?;
            let sibling = owned.get(1).ok_or("sibling pidfd missing")?;
            let identity = identities.get(1).ok_or("sibling identity missing")?;
            let stat = read_bounded(
                &PathBuf::from(format!("/proc/{}/stat", identity.pid)),
                config.byte_limit,
            )?;
            let stat = text(std::str::from_utf8(&stat))?;
            need(
                stat.rsplit_once(')')
                    .and_then(|(_, tail)| tail.split_whitespace().next())
                    == Some("T"),
                "sibling did not remain stopped during EOF observation",
            )?;
            text(rustix::process::pidfd_send_signal(sibling, Signal::CONT))?;
            poll_exit(sibling, deadline(config.guard())?.min(earliest_gate))?;
            let status = text(rustix::process::waitid(
                WaitId::PidFd(sibling.as_fd()),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
            ))?
            .ok_or("sibling was not reaped")?;
            need(
                status.exit_status() == Some(127),
                "sibling did not cancel before exec",
            )?;
            for suffix in ["first", "sibling"] {
                need(
                    !text(scope.join(format!("{suffix}.marker")).try_exists())?,
                    "parent death executed a stand-in marker",
                )?;
            }
            Ok(())
        })();
        // Every observation branch reaches owned-child cleanup before assertions.
        let mut cleanup = Vec::new();
        match child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                if let Err(error) = child.kill() {
                    cleanup.push(error.to_string());
                }
            }
        }
        if let Err(error) = child_end(&mut child, &config) {
            cleanup.push(error);
        }
        for fd in &owned {
            match rustix::process::pidfd_send_signal(fd, Signal::CONT) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => cleanup.push(error.to_string()),
            }
            match rustix::process::pidfd_send_signal(fd, Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => cleanup.push(error.to_string()),
            }
            match deadline(config.guard()).and_then(|end| poll_exit(fd, end)) {
                Ok(()) => {}
                Err(error) => cleanup.push(error),
            }
            match rustix::process::waitid(
                WaitId::PidFd(fd.as_fd()),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
            ) {
                Ok(Some(_)) | Err(rustix::io::Errno::CHILD) => {}
                other => cleanup.push(format!("exact owned orphan reap uncertain: {other:?}")),
            }
        }
        let lease = (|| -> Check<File> {
            let lease = text(
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(scope.join("native-vm-owner.lock")),
            )?;
            text(lease.try_lock())?;
            Ok(lease)
        })();
        let lease = match lease {
            Ok(lease) => Some(lease),
            Err(error) => {
                cleanup.push(format!("native owner lease remains uncertain: {error}"));
                None
            }
        };
        for suffix in ["first", "sibling"] {
            if lease.is_none() {
                break;
            }
            let result = (|| -> Check<()> {
                let record = text(BoxRecord::decode(&read_bounded(
                    &scope.join(format!("{suffix}.creating.record.json")),
                    config.byte_limit,
                )?))?;
                need(
                    record.spec.id.as_str() == format!("{}-{suffix}", config.spec.id.as_str())
                        && record.native_scope.is_some(),
                    "owned Creating scope/BoxId mismatch",
                )?;
                let path = group_root
                    .join(".vm/gate-owner")
                    .join(text(managed_scope_id(&record))?.as_str());
                let group = BoxCgroup::at(path);
                if text(group.populated_if_present())?.is_some() {
                    text(group.kill())?;
                    need(
                        text(group.wait_empty(config.guard()))?,
                        "owned cgroup remains populated",
                    )?;
                    text(group.remove())?;
                }
                let store = text(Store::open(&scope.join("records"), &scope.join("run")))?;
                let mut retained = text(store.read_lifecycle(&record.spec.id))?
                    .ok_or("owned post-parent record missing")?;
                need(
                    retained.native_scope == record.native_scope
                        && text(retained.native_commitment())? == text(record.native_commitment())?,
                    "owned parent-death scope/spec was transplanted",
                )?;
                retained.phase = Phase::Deleting;
                retained.exit = Some(Exit::Unknown);
                text(store.put_lifecycle(&retained))?;
                text(store.remove(&retained.spec.id))
            })();
            if let Err(error) = result {
                cleanup.push(error);
            }
        }
        for path in [
            group_root.join(".vm/gate-owner"),
            group_root.join(".vm"),
            group_root.clone(),
        ] {
            if lease.is_none() {
                break;
            }
            match path.try_exists() {
                Ok(true) => {
                    if let Err(error) = fs::remove_dir(&path) {
                        cleanup.push(error.to_string());
                    }
                }
                Ok(false) => {}
                Err(error) => cleanup.push(error.to_string()),
            }
        }
        if owned.len() != 2 {
            cleanup.push(
                "pre-readiness native identities incomplete; retain private lab for ROOT review"
                    .to_owned(),
            );
        }
        if let Err(error) = rustix::process::set_child_subreaper(prior_subreaper) {
            cleanup.push(error.to_string());
        }
        text(fs::write(scope.join("parent.stdout"), raw))?;
        record_result(&scope, "controller-parent-death", &observed, &cleanup)?;
        observed?;
        need(
            cleanup.is_empty(),
            &format!("cleanup uncertainty: {cleanup:?}"),
        )
    })();
    assert!(result.is_ok(), "{result:?}");
}
