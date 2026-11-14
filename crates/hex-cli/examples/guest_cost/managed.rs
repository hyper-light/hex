//! The Shards class's driver where a node boots its guests managed (Windows' WHP): each guest booted `--host-managed`
//! through the managed launch a node takes (`hex_runtime::guest::managed_plan`, then
//! `hex_runtime::guest::boot_managed`: Shards' own plan with this run's runtime folders, the branch a node whose cost
//! evidence declares no native images takes), its native owner a node's own (`WinRuntime`, with no keeper, so its
//! guests end with this process), its record a canonical one kept in memory ([`Scope`]: a backing of the Shards
//! class's shared execution, as a node's shared guest is, the producer keeping no records). Each guest is held to the
//! census's rate by its own governor (Shards' D191, D255, `ManagedGuest::rate`), whose period is the window
//! ([`Driver::governed_window`]), and read as its node reads it: its vCPUs' guest time first, from its VM process's
//! own answer (`ManagedGuest::host_use`, D260: WHP's run-time counter, which only that process reads), then its job's
//! kernel accounting (`WinRuntime::managed_usage`: the job's CPU time and committed peak), so its VMM's own CPU, the
//! whole less its vCPUs', is never less than none; a job that counts less CPU than its VM process says it used is
//! refused, told. Its guest RAM's part of its job's commit is its committed region of that size, which Shards' WHP
//! `reserve_ram` commits whole from the start (`hex_runtime::windows::sys::region`). It is ready when its agent
//! answers, and its own report is read from its agent's `oci` capacity, as every OS's.

use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hex_api::{
    BackingKey, BackingPhase, CapacityResources, Class, Execution, Refusal, RuntimeCost,
};
use hex_client::guest::{Client, own_link, shards::ShardsLink};
use hex_core::record::{NativeRecord, NativeStage, PoolRecord, ProcId, RECORD_VERSION};
use hex_runtime::devices::{ManagedLaunch, PoolRecords, Records, Transaction};
use hex_runtime::guest::GuestConfig;
use hex_runtime::windows::WinRuntime;
use hex_runtime::windows::sys::{self, Handle};
use sha2::{Digest, Sha256};

use super::guest::guest_range;
use super::run::{Window, walk};
use super::seam::{Driver, Envelope, Files, Holding, Launched, Lines, Patience, Probe, Use};

pub struct Managed {
    config: GuestConfig,
    runtime: Arc<WinRuntime>,
    paths: shards_api::RuntimePaths,
    cache: PathBuf,
    home: PathBuf,
    /// The run's own folder of runtime folders, its guests' sockets among them.
    folder: PathBuf,
    /// The cost each guest is booted under while its own is measured: its envelope's, floor and ceiling, in this
    /// host's quanta, its identity the digest of what this run measures. The cost the run writes is its evidence's.
    cost: RuntimeCost,
    /// The census's rate and the window its governor answered for it ([`Driver::governed_window`]).
    held: Option<(u64, Window)>,
    /// The next guest's number: its backing key's and its socket's.
    next: u64,
}

/// A launched guest: its VM, its scope (its record and native owner), and, once it is ready, its agent's client.
pub struct Guest {
    vm: Arc<shards_api::ManagedGuest>,
    scope: Arc<Scope>,
    client: Option<Client>,
}

/// One guest's canonical record with a node's native owner: the transaction a node's records give its managed launch
/// (`hex_runtime::devices::Transaction`), held in memory, the producer keeping no records. Its checks are the node's
/// own (hex-node's `recorded::Transaction`): a fresh creation launches, its controller is introduced to its record as it
/// is published, and only a draining or released backing is finalized.
pub struct Scope {
    runtime: Arc<WinRuntime>,
    record: Mutex<PoolRecord>,
}

fn kept_nowhere(what: &str) -> Refusal {
    Refusal::Invalid {
        what: "the guest-cost producer's records",
        why: format!("{what}: the producer keeps one backing's record in memory, and no other"),
    }
}

impl Scope {
    fn lock(&self) -> Result<MutexGuard<'_, PoolRecord>, Refusal> {
        self.record.lock().map_err(|_| {
            Refusal::Broken("the guest-cost producer's record lock is poisoned".to_owned())
        })
    }

    /// A copy of the record as it is now, as a node's store reads one back.
    fn snapshot(&self) -> Result<PoolRecord, Refusal> {
        PoolRecord::decode(&self.lock()?.encode()?)
    }

    /// The backing moves to `phase`, as a node's records move it (draining before its VM stops, released after).
    fn moved(&self, phase: BackingPhase) -> Result<(), Refusal> {
        self.lock()?.phase = phase;
        Ok(())
    }
}

impl Transaction for Scope {
    fn records(&self) -> Result<Records, Refusal> {
        Ok(Records::new())
    }
    fn pool_records(&self) -> Result<PoolRecords, Refusal> {
        Ok(vec![self.snapshot()?])
    }
    fn read_pool(&self, key: &BackingKey) -> Result<Option<PoolRecord>, Refusal> {
        let record = self.snapshot()?;
        Ok(if record.key == *key {
            Some(record)
        } else {
            None
        })
    }
    fn put_pool(&self, record: &PoolRecord) -> Result<(), Refusal> {
        let mut held = self.lock()?;
        if held.key != record.key {
            return Err(kept_nowhere("another backing's record"));
        }
        *held = PoolRecord::decode(&record.encode()?)?;
        Ok(())
    }
    fn grant_pool(&self, _: &hex_api::BackingGrant) -> Result<PoolRecord, Refusal> {
        Err(kept_nowhere("a grant"))
    }
    fn remove_pool(&self, _: &BackingKey) -> Result<(), Refusal> {
        Ok(())
    }
    fn check_unsubmitted_pool(&self) -> Result<(), Refusal> {
        Err(kept_nowhere("an unsubmitted backing"))
    }
    fn read(&self, _: &hex_api::BoxId) -> Result<Option<hex_core::record::BoxRecord>, Refusal> {
        Ok(None)
    }
    fn put(&self, _: &hex_core::record::BoxRecord) -> Result<(), Refusal> {
        Err(kept_nowhere("a box's record"))
    }
    fn remove(&self, _: &hex_api::BoxId) -> Result<(), Refusal> {
        Ok(())
    }
    fn launch_managed(
        &self,
        plan: &ManagedLaunch<'_>,
        io: shards_api::LaunchIo,
        patience: Duration,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        let record = self.snapshot()?;
        let native = NativeRecord::Pool(&record);
        if native.class()? != plan.class()
            || native.allocation().is_none()
            || native.stage() != NativeStage::Creating
            || native.init().is_some()
        {
            return Err(native
                .uncertain("its record isn't a fresh VM creation as the producer launches it"));
        }
        let mut introduced = false;
        let mut publish = |identity: &shards_api::Identity| {
            if introduced
                || identity.pid == 0
                || identity.started == 0
                || identity.vsock != plan.vsock()
            {
                return Err(native
                    .uncertain("its launched controller's identity or vsock isn't its plan's"));
            }
            self.lock()?.init = Some(ProcId {
                pid: identity.pid,
                start_ticks: identity.started,
            });
            introduced = true;
            Ok(())
        };
        let receipt = self
            .runtime
            .launch_managed(native, plan, io, patience, &mut publish)?;
        if !introduced {
            return Err(native.uncertain("its launch's controller wasn't introduced to its record"));
        }
        Ok(receipt)
    }
    fn attach_managed(
        &self,
        identity: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        let record = self.snapshot()?;
        self.runtime
            .attach_managed(NativeRecord::Pool(&record), identity)
    }
    fn finalize_managed(&self, identity: Option<&shards_api::Identity>) -> Result<(), Refusal> {
        let record = self.snapshot()?;
        let native = NativeRecord::Pool(&record);
        if !matches!(
            native.stage(),
            NativeStage::Deleting | NativeStage::Released
        ) {
            return Err(native.uncertain("it isn't draining or released as it is finalized"));
        }
        match (native.init(), identity) {
            (Some(expected), Some(actual))
                if expected.pid == actual.pid && expected.start_ticks == actual.started => {}
            (None, None) => {}
            _ => return Err(native.uncertain("its finalized controller isn't its record's")),
        }
        self.runtime.finalize_managed(native, identity)
    }
    fn managed_usage(&self) -> Result<hex_api::Usage, Refusal> {
        let record = self.snapshot()?;
        self.runtime.managed_usage(NativeRecord::Pool(&record))
    }
}

impl Managed {
    /// The guest the environment configures, as `hex node` reads it (`GuestConfig::from_env`).
    pub fn from_env(out: &Path) -> Result<Self, String> {
        Self::configured(GuestConfig::from_env().map_err(|e| e.to_string())?, out)
    }

    /// The `config` guest, booted with `out/runtime/{cache,home}` as its runtime folders, which this user's profile
    /// keeps private as every folder made in it.
    pub fn configured(config: GuestConfig, out: &Path) -> Result<Self, String> {
        let runtime = out.join("runtime");
        let (cache, home) = (runtime.join("cache"), runtime.join("home"));
        for dir in [&runtime, &cache, &home] {
            std::fs::create_dir_all(dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
        }
        let absolute = |dir: &Path| std::path::absolute(dir).map_err(|e| e.to_string());
        let (cache, home, folder) = (absolute(&cache)?, absolute(&home)?, absolute(&runtime)?);
        let paths = shards_api::RuntimePaths::new(cache.clone(), home.clone())
            .map_err(|e| e.to_string())?;
        let envelope = super::guest::envelope(&config)?.resources;
        let (page, block) = super::run::quanta(&home)?;
        let mut id = Sha256::new();
        id.update(b"hex-guest-cost provisional\n");
        for (name, value) in super::guest::describe(&config)? {
            id.update(format!("{name} {value}\n").as_bytes());
        }
        let cost = RuntimeCost {
            id: id.finalize().into(),
            floor: envelope,
            ceiling: envelope,
            quantum: CapacityResources {
                cpu_milli: 1,
                mem_bytes: page,
                disk_bytes: block,
            },
            guest_overhead: CapacityResources::default(),
            overhead: CapacityResources::default(),
        };
        cost.check().map_err(|e| e.to_string())?;
        Ok(Self {
            config,
            runtime: Arc::new(WinRuntime::new()),
            paths,
            cache,
            home,
            folder,
            cost,
            held: None,
            next: 0,
        })
    }

    /// Guest `n`'s backing key: digests of this run's folder and its number, so no two of its guests share one.
    fn key(&self, n: u64) -> BackingKey {
        let digest = |role: &[u8]| -> [u8; 32] {
            let mut hash = Sha256::new();
            hash.update(self.folder.as_os_str().as_encoded_bytes());
            hash.update(role);
            hash.update(n.to_le_bytes());
            hash.finalize().into()
        };
        let mut generation = [0u8; 16];
        generation.copy_from_slice(digest(b"generation").get(..16).unwrap_or(&[0; 16]));
        BackingKey {
            pool: digest(b"pool"),
            generation,
        }
    }

    /// Boots the next guest managed, its record granted the envelope, its plan's guest told what is left until
    /// `deadline`.
    fn boot(
        &mut self,
        deadline: Instant,
    ) -> Result<(Arc<shards_api::ManagedGuest>, Arc<Scope>), String> {
        let n = self.next;
        self.next = n.checked_add(1).ok_or("the guests' count overflows")?;
        let record = PoolRecord {
            version: RECORD_VERSION,
            key: self.key(n),
            execution: Execution::ShardsShared,
            cost: self.cost,
            allocation: Some(
                self.cost
                    .allocate(super::guest::envelope(&self.config)?.resources)
                    .map_err(|e| e.to_string())?,
            ),
            devices: Vec::new(),
            phase: BackingPhase::Granted,
            init: None,
            exit: None,
        };
        record.check().map_err(|e| e.to_string())?;
        let scope = Arc::new(Scope {
            runtime: Arc::clone(&self.runtime),
            record: Mutex::new(record),
        });
        let socket = self.folder.join(format!("vm-{n}.sock"));
        let left = deadline.saturating_duration_since(Instant::now());
        let spec = self.config.spec().map_err(|e| e.to_string())?;
        let plan = hex_runtime::guest::managed_plan(
            &spec,
            &self.config.binary,
            &socket,
            &self.paths,
            None,
            left,
        )
        .map_err(|e| format!("preparing the guest's managed plan: {e}"))?;
        let vm = hex_runtime::guest::boot_managed(scope.as_ref(), plan, left)
            .map_err(|e| format!("booting the guest managed: {e}"))?;
        Ok((Arc::new(vm), scope))
    }

    /// Asks `vm`'s governor to hold `rate` milli-CPU, as a node's learner asks a managed VM's (D255), and the period it
    /// holds it over.
    fn hold(vm: &shards_api::ManagedGuest, rate: u64) -> Result<Duration, String> {
        vm.rate(NonZeroU64::new(rate), Duration::MAX)
            .map_err(|e| format!("asking the guest's governor to hold {rate} milli-CPU: {e}"))?;
        vm.rate_period()
            .ok_or_else(|| "the guest's governor answered no period".to_owned())
    }

    /// Ends `vm` as a node deletes a backing: drained, stopped and waited for until `deadline`, its whole scope
    /// released, then released and finalized through its record.
    fn end(vm: &shards_api::ManagedGuest, scope: &Scope, deadline: Instant) -> Result<(), String> {
        scope
            .moved(BackingPhase::Draining)
            .map_err(|e| e.to_string())?;
        let identity = shards_api::Guest::identity(vm).ok();
        shards_api::Guest::stop(vm).map_err(|e| format!("stopping the guest: {e}"))?;
        let left = deadline.saturating_duration_since(Instant::now());
        match shards_api::Guest::wait_for(vm, left) {
            Ok(Some(_)) => {}
            Ok(None) => return Err(format!("the guest didn't end within {left:?} of its stop")),
            Err(e) => return Err(format!("waiting for the guest's end: {e}")),
        }
        match shards_api::Guest::terminal_scope(vm) {
            Ok(Some(shards_api::TerminalScope::Released)) => {}
            other => {
                return Err(format!(
                    "the guest's scope ended {other:?}, not released whole"
                ));
            }
        }
        scope
            .moved(BackingPhase::Released)
            .map_err(|e| e.to_string())?;
        scope
            .finalize_managed(identity.as_ref())
            .map_err(|e| format!("finalizing the guest's scope: {e}"))
    }

    /// The probes of `vm`, whose record and native owner `scope` holds.
    fn probes(
        &self,
        vm: &Arc<shards_api::ManagedGuest>,
        scope: &Arc<Scope>,
    ) -> Result<(VmmCpu, VmmHolds), String> {
        let identity = shards_api::Guest::identity(vm.as_ref())
            .map_err(|e| format!("naming the guest's VM process: {e}"))?;
        let process = sys::open_for_reading(identity.pid)
            .map_err(|e| format!("opening the guest's VM process: {e}"))?
            .ok_or("the guest's VM process is gone")?;
        let socket = identity
            .vsock
            .parent()
            .ok_or("the guest's socket has no folder")?
            .to_owned();
        Ok((
            VmmCpu {
                vm: Arc::clone(vm),
                scope: Arc::clone(scope),
            },
            VmmHolds {
                scope: Arc::clone(scope),
                process,
                ram: super::guest::ram(&self.config)?,
                guest: None,
                folders: vec![self.cache.clone(), self.home.clone(), socket],
            },
        ))
    }
}

impl Driver for Managed {
    type Guest = Guest;
    type Cpu = VmmCpu;
    type Holds = VmmHolds;

    fn class(&self) -> String {
        hex_runtime::guest::CLASS.to_owned()
    }

    fn describe(&self) -> Result<Lines, String> {
        let mut lines = super::guest::describe(&self.config)?;
        lines.push((
            "host".to_owned(),
            format!(
                "windows {} {} logical processors",
                std::env::consts::ARCH,
                sys::active_processors()
            ),
        ));
        lines.push((
            "plan".to_owned(),
            "Shards' own (prepare_boot) with this run's runtime folders, booted --host-managed: the branch a node whose cost evidence declares no native images takes (hex_runtime::guest::managed_plan, boot_managed)".to_owned(),
        ));
        lines.push((
            "booked".to_owned(),
            format!(
                "each guest a Shards backing of shared execution granted its envelope under a provisional cost {} (its identity the digest of the lines above)",
                super::run::hex(&self.cost.id)
            ),
        ));
        Ok(lines)
    }

    fn envelope(&self) -> Result<Envelope, String> {
        super::guest::envelope(&self.config)
    }

    fn patience(&self) -> Result<Patience, String> {
        super::guest::patience()
    }

    fn runtime(&self) -> &Path {
        &self.home
    }

    fn sources(&self) -> Lines {
        vec![
            ("cpu".to_owned(), "as its node reads it (D260): each vCPU's guest time from its VM process's answer, read first (ManagedGuest::host_use: WHP's run-time counter, TotalRuntime100ns less HypervisorRuntime100ns), then its job's CPU time (WinRuntime::managed_usage: the job's kernel and user time, its VM process's and every process it starts), so its VMM's own, the whole less its vCPUs', is never less than none; a job counting less than its VM process says it used (HostUse::cpu_ns) refused, told".to_owned()),
            ("memory".to_owned(), "as its node reads it: its job's committed peak (WinRuntime::managed_usage: PeakJobMemoryUsed, at most its bound), less its guest RAM's commit, the committed private read/write region of exactly its RAM (VirtualQueryEx), which Shards' WHP reserve_ram commits whole from the start".to_owned()),
            ("disk".to_owned(), format!(
                "the allocated bytes (FileStandardInfo's AllocationSize, a file linked twice once, by its volume and index) of every path under SHARDS_CACHE {}, SHARDS_HOME {} and the guest's socket folder",
                self.cache.display(),
                self.home.display()
            )),
        ]
    }

    fn held(&self) -> Result<Files, String> {
        walk(&[self.cache.clone(), self.home.clone()])
    }

    fn launch(&mut self, deadline: Instant) -> Result<Launched<Self>, String> {
        let (rate, window) = self
            .held
            .clone()
            .ok_or("no census window was asked before a guest's launch")?;
        let (vm, scope) = self.boot(deadline)?;
        let launched = Self::hold(&vm, rate).and_then(|period| {
            if period != window.length {
                return Err(format!(
                    "the guest's governor answered a period of {period:?}, not the census's window of {:?}",
                    window.length
                ));
            }
            self.probes(&vm, &scope)
        });
        match launched {
            Ok((cpu, holds)) => Ok((
                Guest {
                    vm,
                    scope,
                    client: None,
                },
                cpu,
                holds,
            )),
            // Whatever kept it from being read, the guest is ended first: none is left running.
            Err(why) => {
                let ended = self
                    .patience()
                    .and_then(|patience| {
                        Instant::now()
                            .checked_add(patience.bound)
                            .ok_or_else(|| "the end's wait overflows".to_owned())
                    })
                    .and_then(|at| Self::end(&vm, &scope, at));
                Err(match ended {
                    Ok(()) => why,
                    Err(end) => format!("{why}; then {end}"),
                })
            }
        }
    }

    fn ready(&mut self, guest: &mut Guest, deadline: Instant) -> Result<bool, String> {
        let span = deadline.saturating_duration_since(Instant::now());
        let vm: Arc<dyn shards_api::Guest> = Arc::clone(&guest.vm) as Arc<dyn shards_api::Guest>;
        match own_link(ShardsLink::new(vm, span))
            .and_then(|link| Client::connect_before(link, deadline))
        {
            Ok(client) => {
                guest.client = Some(client);
                Ok(true)
            }
            Err(_) if Instant::now() >= deadline => Ok(false),
            Err(why) => Err(format!("reaching the guest's agent: {why}")),
        }
    }

    fn idle(&mut self, guest: &Guest, until: Instant) -> Result<(), String> {
        // The hold is a wait on the guest's end, until `until`: it must not come.
        match shards_api::Guest::wait_for(
            guest.vm.as_ref(),
            until.saturating_duration_since(Instant::now()),
        ) {
            Ok(None) => Ok(()),
            Ok(Some(end)) => Err(format!("the guest ended in its idle hold: {end:?}")),
            Err(e) => Err(format!("holding the guest: {e}")),
        }
    }

    fn report(&mut self, guest: &Guest, deadline: Instant) -> Result<CapacityResources, String> {
        guest
            .client
            .as_ref()
            .ok_or("the guest's agent never answered")?
            .capacity_before(Class::Oci, deadline)
            .map_err(|e| format!("reading the guest agent's capacity: {e}"))?
            .map(|capacity| capacity.free)
            .ok_or_else(|| "the guest's agent reports no oci capacity".to_owned())
    }

    fn tear_down(&mut self, guest: Guest, deadline: Instant) -> Result<(), String> {
        let Guest { vm, scope, client } = guest;
        drop(client);
        Self::end(&vm, &scope, deadline)
    }

    fn governed_window(&mut self, rate: u64) -> Result<Window, String> {
        let patience = self.patience()?;
        let at = |from: Instant| {
            from.checked_add(patience.bound)
                .ok_or_else(|| "the window's boot overflows the clock".to_owned())
        };
        let (vm, scope) = self.boot(at(Instant::now())?)?;
        let period = Self::hold(&vm, rate);
        let ended = at(Instant::now()).and_then(|deadline| Self::end(&vm, &scope, deadline));
        let length = match (period, ended) {
            (Ok(length), Ok(())) => length,
            (Err(why), Ok(())) | (Ok(_), Err(why)) => return Err(why),
            (Err(why), Err(end)) => return Err(format!("{why}; then {end}")),
        };
        let window = Window {
            length,
            source: format!(
                "the period a guest's governor holds {rate} milli-CPU over, as its VM process answered it, a guest booted for it alone (ManagedGuest::rate and rate_period; Shards' D191, D255): the CPU control a node holds a managed VM's rate with, and every guest of the census held to it"
            ),
        };
        self.held = Some((rate, window.clone()));
        Ok(window)
    }
}

/// A launched guest's VMM's CPU, as its node reads it: its vCPUs' guest time from its VM process's answer, then its
/// job's.
pub struct VmmCpu {
    vm: Arc<shards_api::ManagedGuest>,
    scope: Arc<Scope>,
}

impl Probe for VmmCpu {
    type Reading = Use;

    fn read(&mut self) -> Result<Use, String> {
        // D260's order: what its vCPUs ran of its guest first, then its job's whole, so the whole less the vCPUs' is
        // never less than none.
        let used = self
            .vm
            .host_use(Duration::MAX)
            .map_err(|e| format!("asking the guest's VM process its use: {e}"))?;
        let times = used
            .microvm
            .guest
            .map_err(|why| format!("the guest's VM process read no guest time: {why}"))?;
        let usage = self
            .scope
            .managed_usage()
            .map_err(|e| format!("reading the guest's job: {e}"))?;
        // Its VM process's own count, read before its job's: the job, which holds that process, counts no less.
        if let Ok(host) = &used.microvm.host
            && usage.cpu_usec < host.cpu_ns / 1000
        {
            return Err(format!(
                "the guest's job counts {} us of CPU, less than its VM process says it used ({} ns)",
                usage.cpu_usec, host.cpu_ns
            ));
        }
        Ok(Use {
            cpu: Duration::from_micros(usage.cpu_usec),
            guest_cpu: Duration::from_nanos(times.total_ns()),
            guest_threads: Some(u32::try_from(times.vcpus.len()).map_err(|e| e.to_string())?),
        })
    }
}

/// What a launched guest's VMM holds, as its node reads it: its job's committed peak, its guest RAM's commit (its VM
/// process's committed region of that size), and its folders.
pub struct VmmHolds {
    scope: Arc<Scope>,
    process: Handle,
    ram: u64,
    guest: Option<(u64, u64)>,
    folders: Vec<PathBuf>,
}

impl VmmHolds {
    /// The VM process's address space region by region (`VirtualQueryEx`), each with whether it is committed private
    /// read/write memory, as its guest RAM is.
    fn entries(&self) -> Result<Vec<(u64, u64, bool)>, String> {
        let mut entries = Vec::new();
        let mut at = 0u64;
        while let Some(region) = sys::region(&self.process, at)
            .map_err(|e| format!("reading the VM process's memory map: {e}"))?
        {
            let end = region
                .base
                .checked_add(region.size)
                .filter(|end| *end > at)
                .ok_or("the VM process's memory map doesn't advance")?;
            entries.push((region.base, end, region.private_rw));
            at = end;
        }
        Ok(entries)
    }
}

impl Probe for VmmHolds {
    type Reading = Holding;

    fn read(&mut self) -> Result<Holding, String> {
        let usage = self
            .scope
            .managed_usage()
            .map_err(|e| format!("reading the guest's job: {e}"))?;
        if self.guest.is_none() {
            self.guest = guest_range(&self.entries()?, self.ram)?;
        }
        Ok(Holding {
            footprint: usage.memory_bytes,
            file_pages: None,
            peak_footprint: Some(usage.memory_peak_bytes),
            guest_memory: self.guest.map(|(start, end)| end.saturating_sub(start)),
            files: walk(&self.folders)?,
        })
    }

    /// The map read whole again: the guest's RAM is still the one region found, so no reading took another for it.
    fn verify(&mut self) -> Result<(), String> {
        let now = guest_range(&self.entries()?, self.ram)?;
        if now != self.guest {
            return Err(format!(
                "the guest's RAM was read at {:x?} and is now at {now:x?}",
                self.guest
            ));
        }
        Ok(())
    }
}
