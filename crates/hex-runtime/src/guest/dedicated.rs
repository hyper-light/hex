//! A selected box's VM route and lifetime. Durable box records remain the only
//! device-holder authority. One short-lock route table holds per-box reservations;
//! no table or state lock is held over file, guest, channel or callback operations.

use super::selected::{self, Phase, Record};
use super::*;
use crate::devices::Transaction;
use hex_core::record::BoxRecord;

pub(super) struct Guest {
    receipt: Record,
    booted: triomphe::Arc<Booted>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::{Bounds, BoxState};
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    struct Controlled {
        ended: AtomicBool,
        released: AtomicBool,
        stops: AtomicUsize,
        wait: Box<dyn Fn() + Send + Sync>,
    }

    impl shards_api::Guest for Controlled {
        fn process_exited(&self) -> Result<bool, shards_api::Refusal> {
            Ok(self.ended.load(Ordering::SeqCst))
        }
        fn terminal_scope(&self) -> Result<Option<shards_api::TerminalScope>, shards_api::Refusal> {
            Ok(self.ended.load(Ordering::SeqCst).then(|| {
                if self.released.load(Ordering::SeqCst) {
                    shards_api::TerminalScope::Released
                } else {
                    shards_api::TerminalScope::Process
                }
            }))
        }
        fn connect(
            &self,
            port: u32,
            _: Duration,
        ) -> Result<Box<dyn shards_api::Stream>, shards_api::Refusal> {
            Err(shards_api::Refusal::Channel {
                port,
                why: "this model relay is deliberately unavailable".into(),
            })
        }
        fn identity(&self) -> Result<shards_api::Identity, shards_api::Refusal> {
            Ok(shards_api::Identity {
                pid: 42,
                started: 7,
                vsock: "/model/private.sock".into(),
            })
        }
        fn stop(&self) -> Result<(), shards_api::Refusal> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn wait_for(&self, _: Duration) -> Result<Option<shards_api::End>, shards_api::Refusal> {
            (self.wait)();
            Ok(if self.ended.load(Ordering::SeqCst) {
                Some(shards_api::End::Stopped)
            } else {
                None
            })
        }
    }

    impl shards_api::ManagedProcess for Controlled {
        fn identity(&self) -> Result<&shards_api::Identity, shards_api::Refusal> {
            static ID: std::sync::LazyLock<shards_api::Identity> =
                std::sync::LazyLock::new(|| shards_api::Identity {
                    pid: 42,
                    started: 7,
                    vsock: "/model/private.sock".into(),
                });
            Ok(&ID)
        }
        fn stop(&self) -> Result<(), shards_api::Refusal> {
            shards_api::Guest::stop(self)
        }
        fn terminal(&self) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
            Ok(
                shards_api::Guest::terminal_scope(self)?.map(|scope| shards_api::ManagedEnd {
                    status: shards_api::ManagedStatus::Code(0),
                    scope,
                }),
            )
        }
        fn wait_for(
            &self,
            _: Duration,
        ) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
            (self.wait)();
            Ok(shards_api::ManagedProcess::terminal(self)?
                .filter(|end| end.scope == shards_api::TerminalScope::Released))
        }
    }

    struct Cleanup {
        record: BoxRecord,
        process: triomphe::Arc<Controlled>,
        fail_finalize: AtomicBool,
    }
    impl Transaction for Cleanup {
        fn pool_records(&self) -> Result<crate::devices::PoolRecords, Refusal> {
            panic!("box cleanup must not inspect unrelated pools")
        }
        fn read_pool(
            &self,
            _: &hex_api::BackingKey,
        ) -> Result<Option<hex_core::record::PoolRecord>, Refusal> {
            panic!("box cleanup must not read a pool")
        }
        fn put_pool(&self, _: &hex_core::record::PoolRecord) -> Result<(), Refusal> {
            panic!("box cleanup must not write a pool")
        }
        fn grant_pool(
            &self,
            _: &hex_api::BackingGrant,
        ) -> Result<hex_core::record::PoolRecord, Refusal> {
            panic!("box cleanup must not grant a pool")
        }
        fn remove_pool(&self, _: &hex_api::BackingKey) -> Result<(), Refusal> {
            panic!("box cleanup must not remove a pool")
        }
        fn records(&self) -> Result<crate::devices::Records, Refusal> {
            panic!("cleanup has no inventory effect")
        }
        fn read(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
            Ok((self.record.spec.id == *id).then(|| self.record.clone()))
        }
        fn put(&self, _: &BoxRecord) -> Result<(), Refusal> {
            panic!("outer owner persists")
        }
        fn remove(&self, _: &BoxId) -> Result<(), Refusal> {
            panic!("outer owner removes")
        }
        fn finalize_managed(&self, _: Option<&shards_api::Identity>) -> Result<(), Refusal> {
            if self.fail_finalize.load(Ordering::SeqCst) {
                return Err(super::super::books::uncertain(&self.record.spec.id)?);
            }
            Ok(())
        }
        fn managed_usage(&self) -> Result<Usage, Refusal> {
            Err(Refusal::Broken(
                "this cleanup model has no native counters".into(),
            ))
        }
        fn launch_managed(
            &self,
            _: &crate::devices::ManagedLaunch<'_>,
            _: shards_api::LaunchIo,
            _: Duration,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            panic!("cleanup cannot launch")
        }
        fn attach_managed(
            &self,
            identity: Option<&shards_api::Identity>,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            if let Some(identity) = identity {
                assert_eq!(
                    identity,
                    shards_api::ManagedProcess::identity(self.process.as_ref()).unwrap()
                );
            }
            Ok(shards_api::managed_receipt(self.process.clone()))
        }
    }

    fn model_record(runtime: &GuestRuntime, id: &BoxId) -> BoxRecord {
        let mut spec = super::super::tests::oci(hex_api::Rootfs::Dir("/rootfs".into()), Vec::new());
        let ClassSpec::Oci(payload) = spec.class else {
            panic!("model OCI")
        };
        spec.id = id.clone();
        spec.class = ClassSpec::Shards(payload);
        spec.bounds.cpu_max_milli = Some(1);
        spec.bounds.memory_max = Some(1);
        let floor = runtime.dedicated_resources().unwrap();
        let cost = hex_api::RuntimeCost {
            id: [1; 32],
            floor,
            ceiling: floor,
            quantum: hex_api::CapacityResources {
                cpu_milli: 1000,
                mem_bytes: 1 << 20,
                disk_bytes: 1,
            },
            guest_overhead: hex_api::CapacityResources::default(),
            overhead: hex_api::CapacityResources {
                cpu_milli: 1,
                mem_bytes: 1,
                disk_bytes: 0,
            },
        };
        spec.allocation = Some(
            cost.allocate(hex_api::CapacityResources {
                cpu_milli: 1,
                mem_bytes: 1,
                disk_bytes: 0,
            })
            .unwrap(),
        );
        let mut record = BoxRecord::new(spec, hex_core::record::Phase::Deleting, Vec::new());
        record.native_scope = Some(hex_core::record::NativeScope::from_bytes([7; 16]));
        record.init = Some(hex_core::record::ProcId {
            pid: 42,
            start_ticks: 7,
        });
        record
    }

    fn runtime(path: &std::path::Path) -> Arc<GuestRuntime> {
        Arc::new(
            GuestRuntime::new(
                Arc::new(shards_api::sim::SimGuests::new(|_| {
                    shards_api::End::PowerOff
                })),
                GuestConfig {
                    binary: "/model/shards".into(),
                    kernel: "/model/kernel".into(),
                    initrd: "/model/initrd".into(),
                    vcpus: 1,
                    memory_mib: 256,
                    gpu: false,
                    shares: Vec::new(),
                    staged: "/model/staged".into(),
                },
            )
            .recorded_at(path.join("guest")),
        )
    }

    fn record(id: &BoxId) -> Record {
        Record {
            id: id.clone(),
            devices: Vec::new(),
            class: Class::Shards,
            scope: Some(hex_core::record::NativeScope::from_bytes([7; 16])),
            route: None,
            physical: Vec::new(),
            vcpus: 1,
            memory_mib: 256,
            phase: Phase::Running(shards_api::Identity {
                pid: 42,
                started: 7,
                vsock: "/model/private.sock".into(),
            }),
        }
    }

    fn info(id: &BoxId) -> BoxInfo {
        BoxInfo {
            id: id.clone(),
            class: Class::Oci,
            allocation: None,
            route: None,
            state: BoxState::Empty,
            bounds: Bounds::default(),
            adopted: false,
            membership: None,
            exit: None,
            abandoned: 0,
        }
    }

    fn install(
        runtime: &GuestRuntime,
        id: &BoxId,
        guest: Arc<dyn shards_api::Guest>,
    ) -> triomphe::Arc<Booted> {
        let (link, client) = super::super::tests::model_connection();
        let booted = booted_owner(Booted {
            guest: GuestHandle::Shared(guest),
            link,
            client,
            evented: Mutex::new(None),
            owner: Some(Owner {
                id: id.clone(),
                class: Class::Shards,
                allocation: None,
                route: None,
            }),
            backing: None,
        })
        .unwrap();
        let receipt = record(id);
        std::fs::create_dir_all(runtime.selected_dir().unwrap()).unwrap();
        selected::write(&runtime.selected_path(id).unwrap(), &receipt).unwrap();
        runtime
            .reserve(id, Operation::Creating)
            .unwrap()
            .publish(Some(Guest {
                receipt,
                booted: triomphe::Arc::clone(&booted),
            }))
            .unwrap();
        booted
    }

    /// AC-T7.4.1: an actual selected-route wait may reenter a sibling and CPU route;
    /// the same box is excluded by a reservation, without a held table mutex.
    #[test]
    fn selected_route_callback_cannot_block_independent_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let id = BoxId::new("selected-a").unwrap();
        let sibling = BoxId::new("selected-b").unwrap();
        let weak = Arc::downgrade(&runtime);
        let asked = id.clone();
        let guest = Arc::new(Controlled {
            ended: AtomicBool::new(false),
            released: AtomicBool::new(false),
            stops: AtomicUsize::new(0),
            wait: Box::new(move || {
                let runtime = weak.upgrade().unwrap();
                assert!(matches!(
                    runtime.selected_running(&asked),
                    Err(Refusal::Exhausted { bound: 1, .. })
                ));
                assert!(runtime.selected_running(&sibling).unwrap().is_none());
                assert!(runtime.running().unwrap().is_none());
            }),
        });
        let expected = install(&runtime, &id, guest);
        assert!(triomphe::Arc::ptr_eq(
            &runtime.selected_running(&id).unwrap().unwrap(),
            &expected
        ));
    }

    /// AC-T7.4.1: a selected route succeeds even when the unrelated shared CPU
    /// guest lock is broken; no eager fallback may override the selected result.
    #[test]
    fn selected_route_does_not_consult_unrelated_cpu_guest() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let id = BoxId::new("selected-only").unwrap();
        install(
            &runtime,
            &id,
            Arc::new(Controlled {
                ended: AtomicBool::new(false),
                released: AtomicBool::new(false),
                stops: AtomicUsize::new(0),
                wait: Box::new(|| {}),
            }),
        );
        let poisoned = Arc::clone(&runtime);
        assert!(
            std::thread::spawn(move || {
                let _guard = poisoned.booted.lock().unwrap();
                panic!("controlled CPU lock poison");
            })
            .join()
            .is_err()
        );
        assert_eq!(runtime.with(&id, |_| Ok(17)), Ok(17));
    }

    /// AC-T7.4.1: stop accepted with no terminal witness keeps the exact receipt;
    /// a later positive terminal result permits deletion without a relay call.
    #[test]
    fn selected_stop_retains_ownership_until_terminal_proof() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let id = BoxId::new("selected-stop").unwrap();
        let guest = triomphe::Arc::try_new(Controlled {
            ended: AtomicBool::new(false),
            released: AtomicBool::new(false),
            stops: AtomicUsize::new(0),
            wait: Box::new(|| {}),
        })
        .unwrap();
        std::fs::create_dir_all(runtime.selected_dir().unwrap()).unwrap();
        selected::write(&runtime.selected_path(&id).unwrap(), &record(&id)).unwrap();
        let transaction = Cleanup {
            record: model_record(&runtime, &id),
            process: guest.clone(),
            fail_finalize: AtomicBool::new(true),
        };
        assert!(runtime.delete_selected(&id, &transaction).is_err());
        assert_eq!(guest.stops.load(Ordering::SeqCst), 1);
        assert!(matches!(
            selected::read(&runtime.selected_path(&id).unwrap())
                .unwrap()
                .unwrap()
                .phase,
            Phase::Stopping(_)
        ));
        assert!(
            shards_api::ManagedProcess::terminal(guest.as_ref())
                .unwrap()
                .is_none()
        );
        guest.ended.store(true, Ordering::SeqCst);
        assert!(
            runtime.delete_selected(&id, &transaction).is_err(),
            "controller exit leaves helpers uncertain"
        );
        guest.released.store(true, Ordering::SeqCst);
        assert_eq!(runtime.delete_selected(&id, &transaction), Ok(()));
        let stops = guest.stops.load(Ordering::SeqCst);
        assert_eq!(runtime.delete_selected(&id, &transaction), Ok(()));
        assert_eq!(
            guest.stops.load(Ordering::SeqCst),
            stops,
            "durable Released never stops a name-reused native scope"
        );
        assert!(runtime.finalize_selected(&id, &transaction).is_err());
        assert!(matches!(
            selected::read(&runtime.selected_path(&id).unwrap())
                .unwrap()
                .unwrap()
                .phase,
            Phase::Released {
                identity: Some(_),
                status: shards_api::ManagedStatus::Code(0),
            }
        ));
        transaction.fail_finalize.store(false, Ordering::SeqCst);
        runtime.finalize_selected(&id, &transaction).unwrap();
        assert!(
            selected::read(&runtime.selected_path(&id).unwrap())
                .unwrap()
                .is_some()
        );
        runtime.forget_selected(&id).unwrap();
        assert!(
            selected::read(&runtime.selected_path(&id).unwrap())
                .unwrap()
                .is_none()
        );
    }

    /// AC-T7.4.1: normalize only the exact dedicated box's inner OCI result.
    #[test]
    fn selected_host_class_preserves_backend_and_rejects_foreign_box() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let id = BoxId::new("selected-class").unwrap();
        let booted = install(
            &runtime,
            &id,
            Arc::new(Controlled {
                ended: AtomicBool::new(false),
                released: AtomicBool::new(false),
                stops: AtomicUsize::new(0),
                wait: Box::new(|| {}),
            }),
        );
        assert_eq!(booted.host_info(info(&id)).unwrap().class, Class::Shards);
        let allocation = model_record(&runtime, &id).spec.allocation;
        let route = Some(hex_api::RouteReceipt {
            key: hex_api::RouteKey {
                class: Class::Shards,
                image: hex_api::ImagePlatform {
                    os: hex_api::ImageOs::Linux,
                    arch: super::super::route::host_platform().unwrap().arch,
                },
                execution: hex_api::Execution::ShardsDedicated,
                gpu: hex_api::GpuEligibility::None,
            },
            backing: None,
        });
        let (link, client) = super::super::tests::model_connection();
        let projected = Booted {
            guest: GuestHandle::Shared(Arc::new(Controlled {
                ended: AtomicBool::new(false),
                released: AtomicBool::new(false),
                stops: AtomicUsize::new(0),
                wait: Box::new(|| {}),
            })),
            link,
            client,
            evented: Mutex::new(None),
            owner: Some(Owner {
                id: id.clone(),
                class: Class::Shards,
                allocation,
                route,
            }),
            backing: None,
        };
        let mut exited = info(&id);
        exited.state = BoxState::Empty;
        exited.exit = Some(hex_api::Exit::Code(0));
        let outer = projected.host_info(exited).unwrap();
        assert_eq!(outer.allocation, allocation);
        assert_eq!(outer.route, route);
        let mut forged = info(&id);
        forged.allocation = allocation;
        assert!(
            projected.host_info(forged).is_err(),
            "inner OCI cannot mint a host VM allocation"
        );
        let mut forged = info(&id);
        forged.route = route;
        assert!(
            projected.host_info(forged).is_err(),
            "inner OCI cannot mint a host execution route"
        );
        assert!(
            booted
                .host_info(info(&BoxId::new("foreign-box").unwrap()))
                .is_err()
        );
    }
    /// AC-T7.4.1: old allocation and actual VM units survive a new initialized
    /// admission profile; recovery cannot resize the old VM to today's config.
    #[test]
    fn retained_allocation_survives_current_config_changes() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let id = BoxId::new("retained-cost").unwrap();
        let record = model_record(&runtime, &id);
        let receipt = Record::retained(&record).unwrap();
        receipt.matches_record(&record).unwrap();
        let mut changed = runtime.config.clone();
        changed.vcpus = 2;
        assert!(Record::intent(&record, &changed).is_err());
        receipt.matches_record(&record).unwrap();
        let mut forged = record.spec.clone();
        forged.allocation.as_mut().unwrap().resources.mem_bytes = 1;
        assert!(receipt.matches(&forged).is_err());
    }

    /// AC-T7.4.1: a reused BoxId/socket/PID cannot transplant another boot's
    /// cleanup authority, including legacy records with no native incarnation.
    #[test]
    fn selected_scope_must_match_canonical_before_any_native_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let id = BoxId::new("selected-scope").unwrap();
        let process = triomphe::Arc::try_new(Controlled {
            ended: AtomicBool::new(true),
            released: AtomicBool::new(true),
            stops: AtomicUsize::new(0),
            wait: Box::new(|| {}),
        })
        .unwrap();
        std::fs::create_dir_all(runtime.selected_dir().unwrap()).unwrap();
        let path = runtime.selected_path(&id).unwrap();
        for (canonical_scope, receipt_scope) in [
            (Some([8; 16]), Some([7; 16])),
            (None, Some([7; 16])),
            (Some([7; 16]), None),
            (None, None),
        ] {
            let mut canonical = model_record(&runtime, &id);
            canonical.native_scope = canonical_scope.map(hex_core::record::NativeScope::from_bytes);
            let mut receipt = record(&id);
            receipt.scope = receipt_scope.map(hex_core::record::NativeScope::from_bytes);
            selected::write(&path, &receipt).unwrap();
            assert!(receipt.matches_record(&canonical).is_err());
            let transaction = Cleanup {
                record: canonical,
                process: process.clone(),
                fail_finalize: AtomicBool::new(false),
            };
            assert!(runtime.delete_selected(&id, &transaction).is_err());
            assert!(runtime.finalize_selected(&id, &transaction).is_err());
            assert_eq!(selected::read(&path).unwrap(), Some(receipt));
        }
        assert_eq!(process.stops.load(Ordering::SeqCst), 0);
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Loading,
    Creating,
    Stopping,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Loading => "same-box guest recovery in flight",
            Self::Creating => "same-box guest create in flight",
            Self::Stopping => "same-box guest stop in flight",
        }
    }
}

enum Route {
    Busy(Operation),
    Ready(Guest),
}

pub(super) struct Slot {
    id: BoxId,
    route: Route,
}

/// Owning a reservation excludes a second same-ID operation without keeping a
/// mutex across callbacks. A failed operation restores its last known route;
/// its on-disk receipt still decides whether physical absence is proved.
struct Reservation<'a> {
    runtime: &'a GuestRuntime,
    id: BoxId,
    previous: Option<Guest>,
    finished: bool,
}

impl Reservation<'_> {
    fn publish(mut self, next: Option<Guest>) -> Result<(), Refusal> {
        self.restore(next)?;
        self.finished = true;
        Ok(())
    }

    fn restore(&self, next: Option<Guest>) -> Result<(), Refusal> {
        let mut routes = self.runtime.dedicated.lock().map_err(poisoned)?;
        let index = routes
            .iter()
            .position(|slot| slot.id == self.id)
            .ok_or_else(|| invalid("guest reservation lost its route"))?;
        if let Some(next) = next {
            routes
                .get_mut(index)
                .ok_or_else(|| invalid("guest route index changed"))?
                .route = Route::Ready(next);
        } else {
            routes.swap_remove(index);
        }
        Ok(())
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // A failed operation restores its last owned route even after a test panic;
        // this destructor performs no native effect or durable receipt removal.
        let mut routes = self
            .runtime
            .dedicated
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = routes.iter().position(|slot| slot.id == self.id) {
            match self.previous.take() {
                Some(previous) => {
                    if let Some(slot) = routes.get_mut(index) {
                        slot.route = Route::Ready(previous);
                    }
                }
                None => {
                    routes.swap_remove(index);
                }
            }
        }
    }
}

pub(super) fn copy_path(path: &std::path::Path) -> Result<PathBuf, Refusal> {
    let mut text = OsString::new();
    text.try_reserve_exact(path.as_os_str().len())
        .map_err(|_| Refusal::Exhausted {
            what: "native guest path bytes",
            bound: path.as_os_str().len(),
        })?;
    text.push(path.as_os_str());
    Ok(PathBuf::from(text))
}

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "dedicated GPU guest",
        why: why.into(),
    }
}

impl Owner {
    fn project(&self, mut info: BoxInfo) -> Result<BoxInfo, Refusal> {
        self.project_in(&mut info)?;
        Ok(info)
    }

    fn project_in(&self, info: &mut BoxInfo) -> Result<(), Refusal> {
        if info.id != self.id
            || info.class != Class::Oci
            || info.allocation.is_some()
            || info.route.is_some()
        {
            return Err(invalid(
                "dedicated guest returned a foreign box, workload class or native charge",
            ));
        }
        info.class = self.class;
        info.allocation = self.allocation;
        info.route = self.route;
        Ok(())
    }
}

impl Booted {
    pub(super) fn host_info(&self, info: BoxInfo) -> Result<BoxInfo, Refusal> {
        match &self.owner {
            Some(owner) => owner.project(info),
            None => Ok(info),
        }
    }
}

impl GuestRuntime {
    pub(super) fn project_info(&self, booted: &Booted, info: BoxInfo) -> Result<BoxInfo, Refusal> {
        if booted.owner.is_some() || !self.uses_books() {
            return booted.host_info(info);
        }
        let mut info = info;
        self.project_info_in(booted, &mut info)?;
        Ok(info)
    }

    pub(super) fn project_info_in(
        &self,
        booted: &Booted,
        info: &mut BoxInfo,
    ) -> Result<(), Refusal> {
        if let Some(owner) = &booted.owner {
            return owner.project_in(info);
        }
        if !self.uses_books() {
            return Ok(());
        }
        let Some(record) = self.recorded_box(&info.id)? else {
            // A shared guest may also list a foreign entry. Its host receipt is
            // unknown here; never attach another box's class/route/charge to it.
            return Ok(());
        };
        route::guest_spec(&record.spec)?;
        if self.owns_record(&record.spec) {
            return Err(super::books::uncertain(&info.id)?);
        }
        let owner = Owner {
            class: record.spec.class.class(),
            id: record.spec.id,
            allocation: record.spec.allocation,
            route: record.spec.route,
        };
        owner.project_in(info)
    }

    pub(super) fn booted_for(&self, id: &BoxId) -> Result<Option<triomphe::Arc<Booted>>, Refusal> {
        let dedicated = if self.uses_books() {
            self.recorded_box(id)?
                .is_some_and(|record| self.owns_record(&record.spec))
        } else if self.record.is_some() {
            selected::read(&self.selected_path(id)?)?.is_some()
        } else {
            false
        };
        if dedicated {
            // Forward already owns its canonical method turn. No recursive enter;
            // recovery/create installs the native route before accepting forwards.
            self.cached_selected(id)
        } else if self.managed_shared() {
            let record = self.recorded_box(id)?.ok_or(super::books::uncertain(id)?)?;
            self.shared_for_record(&record)
        } else {
            self.running()
        }
    }

    pub fn with_gpu_catalog(mut self, catalog: crate::devices::SharedCatalog) -> Self {
        self.gpu_catalog = Some(catalog);
        self
    }

    /// The exact pair already held by the configured probe provider. Native
    /// launch borrows the same validated values from its prepared plan.
    pub fn with_runtime_paths(mut self, paths: shards_api::RuntimePaths) -> Self {
        self.native_paths = Some(paths);
        self
    }

    /// Charge this configured VM envelope before create; the box's payload ceiling
    /// is within it. Parent/VMM overhead still needs its separately measured charge.
    pub fn dedicated_resources(&self) -> Result<hex_api::CapacityResources, Refusal> {
        selected::resources(&self.config)
    }

    fn reserve(&self, id: &BoxId, operation: Operation) -> Result<Reservation<'_>, Refusal> {
        let owned = id.try_clone()?;
        let mut routes = self.dedicated.lock().map_err(poisoned)?;
        let previous = if let Some(slot) = routes.iter_mut().find(|slot| slot.id == *id) {
            match &slot.route {
                Route::Busy(existing) => {
                    return Err(Refusal::Exhausted {
                        what: existing.name(),
                        bound: 1,
                    });
                }
                Route::Ready(_) => {}
            }
            match std::mem::replace(&mut slot.route, Route::Busy(operation)) {
                Route::Ready(guest) => Some(guest),
                Route::Busy(_) => return Err(invalid("guest reservation changed under its lock")),
            }
        } else {
            if routes.len() >= crate::MAX_BOXES {
                return Err(Refusal::Full(crate::MAX_BOXES));
            }
            routes.try_reserve(1).map_err(|_| Refusal::Exhausted {
                what: "guest routes",
                bound: crate::MAX_BOXES,
            })?;
            routes.push(Slot {
                id: id.try_clone()?,
                route: Route::Busy(operation),
            });
            None
        };
        drop(routes);
        Ok(Reservation {
            runtime: self,
            id: owned,
            previous,
            finished: false,
        })
    }

    fn selected_dir(&self) -> Result<PathBuf, Refusal> {
        let record = self
            .record
            .as_ref()
            .ok_or_else(|| invalid("dedicated guests require durable receipts"))?;
        let mut text = OsString::new();
        text.try_reserve_exact(
            record
                .as_os_str()
                .len()
                .checked_add(".gpu".len())
                .ok_or_else(|| invalid("receipt directory length overflows"))?,
        )
        .map_err(|_| invalid("receipt directory storage unavailable"))?;
        text.push(record.as_os_str());
        text.push(".gpu");
        Ok(PathBuf::from(text))
    }

    fn selected_path(&self, id: &BoxId) -> Result<PathBuf, Refusal> {
        let mut path = self.selected_dir()?;
        path.try_reserve(
            id.as_str()
                .len()
                .checked_add(1)
                .ok_or_else(|| invalid("receipt path length overflows"))?,
        )
        .map_err(|_| invalid("receipt path storage unavailable"))?;
        path.push(id.as_str());
        Ok(path)
    }

    fn selected_socket(&self, id: &BoxId) -> Result<PathBuf, Refusal> {
        let mut dir = self.selected_dir()?;
        let suffix = "-sockets";
        let mut text = OsString::new();
        text.try_reserve_exact(
            dir.as_os_str()
                .len()
                .checked_add(suffix.len())
                .ok_or_else(|| invalid("socket directory length overflows"))?,
        )
        .map_err(|_| invalid("socket directory storage unavailable"))?;
        text.push(dir.as_os_str());
        text.push(suffix);
        dir = PathBuf::from(text);
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&dir)
            .map_err(|e| Refusal::os("creating private selected socket directory", e))?;
        held::sync_folder(&dir).map_err(|e| Refusal::os("syncing selected socket directory", e))?;
        dir.try_reserve(
            id.as_str()
                .len()
                .checked_add(1)
                .ok_or_else(|| invalid("socket path length overflows"))?,
        )
        .map_err(|_| invalid("socket path storage unavailable"))?;
        dir.push(id.as_str());
        Ok(dir)
    }

    pub(super) fn selected_ids(&self) -> Result<Vec<BoxId>, Refusal> {
        if self.record.is_none() {
            return Ok(Vec::new());
        }
        let entries = match std::fs::read_dir(self.selected_dir()?) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Refusal::os("listing selected guest receipts", e)),
        };
        let mut ids = Vec::new();
        for (seen, entry) in entries.enumerate() {
            // A durable receipt and at most one replacement temporary per box.
            if seen
                >= crate::MAX_BOXES
                    .checked_mul(2)
                    .ok_or(Refusal::Full(crate::MAX_BOXES))?
            {
                return Err(Refusal::Full(crate::MAX_BOXES));
            }
            let entry =
                entry.map_err(|e| Refusal::os("reading selected guest receipt entry", e))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| invalid("receipt filename is not UTF-8"))?;
            if name.starts_with('.') && name.ends_with(".tmp") {
                continue;
            }
            let id = BoxId::new(name)?;
            if ids.len() >= crate::MAX_BOXES {
                return Err(Refusal::Full(crate::MAX_BOXES));
            }
            ids.try_reserve(1)
                .map_err(|_| Refusal::Full(crate::MAX_BOXES))?;
            ids.push(id);
        }
        ids.sort_unstable();
        Ok(ids)
    }

    fn selected_spec(&self, receipt: &Record) -> Result<shards_api::GuestSpec, Refusal> {
        self.check_native_binding(receipt)?;
        self.receipt_spec(receipt)
    }

    fn check_native_binding(&self, receipt: &Record) -> Result<(), Refusal> {
        if receipt.devices.is_empty() {
            return Ok(());
        }
        if self.catalog_physical(&receipt.devices)? != receipt.physical {
            return Err(invalid(
                "configured physical binding changed since guest launch",
            ));
        }
        Ok(())
    }

    fn receipt_spec(&self, receipt: &Record) -> Result<shards_api::GuestSpec, Refusal> {
        let mut spec = self.config.spec();
        spec.gpu = !receipt.devices.is_empty();
        spec.vcpus = receipt.vcpus;
        spec.memory_mib = receipt.memory_mib;
        if receipt.devices.is_empty() {
            return Ok(spec);
        }
        spec.gpu_devices
            .try_reserve_exact(receipt.physical.len())
            .map_err(|_| Refusal::Exhausted {
                what: "selected Shards GPU identities",
                bound: receipt.physical.len(),
            })?;
        spec.gpu_devices.extend_from_slice(&receipt.physical);
        if receipt.physical.len() != receipt.devices.len() {
            return Err(invalid("receipt lost a physical selector"));
        }
        gpu::append_authority(&mut spec.cmdline, &receipt.id, &receipt.devices)?;
        Ok(spec)
    }

    pub(super) fn physical_for(
        &self,
        devices: &[hex_api::DeviceId],
    ) -> Result<Vec<shards_api::PhysicalGpu>, Refusal> {
        if devices.is_empty() {
            return Ok(Vec::new());
        }
        let physical = self.catalog_physical(devices)?;
        self.checked_physical(devices, &physical)?;
        Ok(physical)
    }

    pub fn validate_binding(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        if spec.devices.is_empty() && !matches!(spec.class, ClassSpec::Shards(_)) {
            return Ok(());
        }
        let receipt =
            selected::read(&self.selected_path(&spec.id)?)?.ok_or(Refusal::Lifecycle {
                id: spec.id.try_clone()?,
                cause: hex_api::LifecycleCause::Uncertain,
            })?;
        receipt.matches(spec)?;
        self.selected_spec(&receipt)?;
        Ok(())
    }

    fn booted(
        &self,
        guest: shards_api::ManagedGuest,
        receipt: &Record,
        record: &BoxRecord,
        budget: managed::Budget,
    ) -> Result<triomphe::Arc<Booted>, Refusal> {
        let owner = Owner {
            id: receipt.id.try_clone()?,
            class: receipt.class,
            allocation: record.spec.allocation,
            route: record.spec.route,
        };
        self.connect_managed(guest, Some(owner), None, budget)
    }

    pub(super) fn connect_managed(
        &self,
        guest: shards_api::ManagedGuest,
        owner: Option<Owner>,
        backing: Option<shared::Binding>,
        budget: managed::Budget,
    ) -> Result<triomphe::Arc<Booted>, Refusal> {
        let link: LinkOwner = own_link(ShardsLink::managed(
            guest
                .try_clone()
                .map_err(|why| refusal("retaining managed guest connection", &why))?,
            budget.remaining()?,
        ))?;
        let client = Client::connect(triomphe::Arc::clone(&link))?;
        let booted = booted_owner(Booted {
            guest: GuestHandle::Managed(guest),
            link,
            client,
            evented: Mutex::new(None),
            owner,
            backing,
        })?;
        budget.remaining()?;
        self.join(&booted)?;
        Ok(booted)
    }

    fn cached_selected(&self, id: &BoxId) -> Result<Option<triomphe::Arc<Booted>>, Refusal> {
        let mut reservation = self.reserve(id, Operation::Loading)?;
        let Some(known) = reservation.previous.take() else {
            reservation.publish(None)?;
            return if self.uses_books() {
                Err(super::books::uncertain(id)?)
            } else {
                Ok(None)
            };
        };
        let booted = triomphe::Arc::clone(&known.booted);
        reservation.previous = Some(known);
        let terminal = booted
            .guest
            .wait_for(Duration::ZERO)
            .map_err(|why| refusal("observing selected guest lifetime", &why))?;
        let scope = booted
            .guest
            .terminal_scope()
            .map_err(|why| refusal("observing selected native scope", &why))?;
        if terminal.is_some() || scope.is_some() {
            if scope == Some(shards_api::TerminalScope::Released) {
                let known = reservation.previous.take();
                reservation.publish(known)?;
                return Ok(None);
            }
            return Err(super::books::uncertain(id)?);
        }
        if reservation
            .previous
            .as_ref()
            .is_some_and(|known| !matches!(known.receipt.phase, Phase::Running(_)))
        {
            return Err(super::books::uncertain(id)?);
        }
        if self.uses_books() {
            let record = self.recorded_box(id)?.ok_or(super::books::uncertain(id)?)?;
            reservation
                .previous
                .as_ref()
                .ok_or(super::books::uncertain(id)?)?
                .receipt
                .matches_record(&record)?;
            self.validate_binding(&record.spec)?;
            let identity = booted
                .guest
                .identity()
                .map_err(|why| refusal("validating selected route identity", &why))?;
            managed::same_init(&record, &identity)?;
            if booted.owner.as_ref().is_none_or(|owner| {
                owner.allocation != record.spec.allocation
                    || owner.class != record.spec.class.class()
                    || owner.route != record.spec.route
            }) {
                return Err(super::books::uncertain(id)?);
            }
        }
        let known = reservation.previous.take();
        reservation.publish(known)?;
        Ok(Some(booted))
    }

    fn has_selected_route(&self, id: &BoxId) -> Result<bool, Refusal> {
        Ok(self
            .dedicated
            .lock()
            .map_err(poisoned)?
            .iter()
            .any(|slot| slot.id == *id))
    }

    pub(super) fn selected_running(
        &self,
        id: &BoxId,
    ) -> Result<Option<triomphe::Arc<Booted>>, Refusal> {
        if !self.uses_books() {
            return self.cached_selected(id);
        }
        let record = self.recorded_box(id)?.ok_or(super::books::uncertain(id)?)?;
        let keys = self.turn_keys(&record.spec.devices)?;
        let books = self.books.get().ok_or(super::books::uncertain(id)?)?;
        crate::devices::with_transaction(books, id, &keys, |transaction| {
            let current = transaction.read(id)?.ok_or(super::books::uncertain(id)?)?;
            if current.spec != record.spec
                || current.init != record.init
                || current.native_scope != record.native_scope
            {
                return Err(super::books::uncertain(id)?);
            }
            self.selected_running_in(id, transaction, &current)
        })
    }

    pub fn recover_managed(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        if !self.owns_record(spec) {
            return Ok(());
        }
        self.validate_binding(spec)?;
        self.selected_running(&spec.id)?
            .ok_or(super::books::uncertain(&spec.id)?)?;
        Ok(())
    }

    pub(super) fn selected_running_in(
        &self,
        id: &BoxId,
        transaction: &dyn Transaction,
        record: &BoxRecord,
    ) -> Result<Option<triomphe::Arc<Booted>>, Refusal> {
        if self.has_selected_route(id)? {
            return self.cached_selected(id);
        }
        let reservation = self.reserve(id, Operation::Loading)?;
        let budget = managed::Budget::new(self.first_dial_bound()?)?;
        let receipt =
            selected::read(&self.selected_path(id)?)?.ok_or(super::books::uncertain(id)?)?;
        receipt.matches_record(record)?;
        let identity = receipt.identity()?;
        managed::same_init(record, identity)?;
        if !matches!(receipt.phase, Phase::Running(_)) {
            return Err(super::books::uncertain(id)?);
        }
        let spec = self.selected_spec(&receipt)?;
        self.checked_physical(&receipt.devices, &receipt.physical)?;
        let plan = self.managed_plan(&spec, &identity.vsock)?;
        let guest = managed::attach(transaction, plan, identity, budget)?;
        let booted = self.booted(guest, &receipt, record, budget)?;
        reservation.publish(Some(Guest {
            receipt,
            booted: triomphe::Arc::clone(&booted),
        }))?;
        Ok(Some(booted))
    }

    pub(super) fn managed_plan(
        &self,
        spec: &shards_api::GuestSpec,
        socket: &std::path::Path,
    ) -> Result<shards_api::PreparedBoot, Refusal> {
        let paths = self.native_paths.as_ref().ok_or_else(|| Refusal::Invalid {
            what: "managed Shards runtime paths",
            why: "the node's configured probe and native launch require the same explicit paths"
                .into(),
        })?;
        shards_api::prepare_boot(
            spec,
            &self.config.binary,
            socket,
            shards_api::recorded_console_bytes(&spec.kernel),
        )
        .and_then(|plan| plan.with_runtime_paths(paths))
        .map_err(|why| refusal("preparing admitted Shards scope", &why))
    }

    pub(super) fn prepare_selected_create(&self, id: &BoxId) -> Result<(), Refusal> {
        if self.has_selected_route(id)? {
            return Err(super::books::uncertain(id)?);
        }
        let path = self.selected_path(id)?;
        if let Some(receipt) = selected::read(&path)? {
            if receipt.id != *id || !matches!(receipt.phase, Phase::Released { .. }) {
                return Err(super::books::uncertain(id)?);
            }
            // The caller proved the sole canonical record absent under its turn.
            // This is a completed removal whose final receipt unlink was interrupted.
            selected::remove(&path)?;
        }
        Ok(())
    }

    pub(super) fn create_selected(
        &self,
        spec: BoxSpec,
        transaction: &dyn Transaction,
        budget: managed::Budget,
    ) -> Result<BoxInfo, Refusal> {
        budget.remaining()?;
        self.check_allocation(&spec)?;
        let reservation = self.reserve(&spec.id, Operation::Creating)?;
        let path = self.selected_path(&spec.id)?;
        if reservation.previous.is_some() || selected::read(&path)?.is_some() {
            return Err(Refusal::Exists(spec.id));
        }
        // Store's first fresh Creating put persists this incarnation. The local
        // pre-put shell cannot supply it, and a reusable BoxId socket is no proof.
        let canonical = transaction
            .read(&spec.id)?
            .ok_or(super::books::uncertain(&spec.id)?)?;
        if canonical.phase != hex_core::record::Phase::Creating
            || canonical.spec != spec
            || canonical.init.is_some()
        {
            return Err(super::books::uncertain(&spec.id)?);
        }
        let mut receipt = Record::intent(&canonical, &self.config)?;
        receipt.physical = self.physical_for(&receipt.devices)?;
        let guest_spec = self.selected_spec(&receipt)?;
        let rewritten = self.rewrite_spec(spec)?;
        let dir = self.selected_dir()?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| Refusal::os("creating selected guest receipt directory", e))?;
        if let Some(parent) = dir.parent() {
            held::sync_folder(parent)
                .map_err(|e| Refusal::os("syncing selected guest receipt directory", e))?;
        }
        let socket = self.selected_socket(&receipt.id)?;
        let plan = self.managed_plan(&guest_spec, &socket)?;
        budget.remaining()?;
        selected::write(&path, &receipt)?;
        let guest = managed::boot(transaction, plan, budget)?;
        let identity = shards_api::Guest::identity(&guest)
            .map_err(|why| refusal("recording selected guest identity", &why))?;
        let record = transaction
            .read(&receipt.id)?
            .ok_or(super::books::uncertain(&receipt.id)?)?;
        managed::same_init(&record, &identity)?;
        receipt.matches_record(&record)?;
        receipt.phase = Phase::Running(identity);
        selected::write(&path, &receipt)?;
        let booted = self.booted(guest, &receipt, &record, budget)?;
        budget.remaining()?;
        let created = self.create_in(&booted, rewritten);
        reservation.publish(Some(Guest { receipt, booted }))?;
        created
    }

    pub(super) fn delete_selected(
        &self,
        id: &BoxId,
        transaction: &dyn Transaction,
    ) -> Result<(), Refusal> {
        let mut reservation = self.reserve(id, Operation::Stopping)?;
        let path = self.selected_path(id)?;
        let receipt = selected::read(&path)?;
        let record = transaction.read(id)?.ok_or(super::books::uncertain(id)?)?;
        if record.native_scope.is_none() {
            return Err(super::books::uncertain(id)?);
        }
        let identity = match receipt.as_ref() {
            Some(receipt) => {
                receipt.matches_record(&record)?;
                match &receipt.phase {
                    Phase::Running(identity) | Phase::Stopping(identity) => {
                        managed::same_init(&record, identity)?;
                        Some(identity)
                    }
                    Phase::Released {
                        identity: Some(identity),
                        ..
                    } => {
                        managed::same_init(&record, identity)?;
                        reservation.publish(None)?;
                        return Ok(());
                    }
                    Phase::Released { identity: None, .. } if record.init.is_none() => {
                        reservation.publish(None)?;
                        return Ok(());
                    }
                    Phase::Released { identity: None, .. } => {
                        return Err(super::books::uncertain(id)?);
                    }
                    Phase::Intent => None,
                }
            }
            None if reservation.previous.is_some() => return Err(super::books::uncertain(id)?),
            None => None,
        };
        if let Some(identity) = identity {
            let mut stopping = selected::read(&path)?.ok_or(super::books::uncertain(id)?)?;
            stopping.phase = Phase::Stopping(shards_api::Identity {
                pid: identity.pid,
                started: identity.started,
                vsock: copy_path(&identity.vsock)?,
            });
            selected::write(&path, &stopping)?;
            if let Some(known) = reservation.previous.as_mut() {
                known.receipt = stopping;
            }
        }
        // The native transaction names the original scope even before publication.
        // PID exit/Gone/observer error never substitutes for complete scope release.
        let terminal = managed::stop(transaction, id, identity, self.first_dial_bound()?)?;
        let released_identity = identity
            .map(|identity| -> Result<_, Refusal> {
                Ok(shards_api::Identity {
                    pid: identity.pid,
                    started: identity.started,
                    vsock: copy_path(&identity.vsock)?,
                })
            })
            .transpose()?;
        let mut released = match receipt {
            Some(receipt) => receipt,
            None => {
                // No effect can precede Intent. Recover a pre-intent failure only
                // after the native owner positively proved the original scope empty.
                let mut receipt = Record::retained(&record)?;
                receipt.physical = self.catalog_physical(&receipt.devices)?;
                receipt
            }
        };
        released.phase = Phase::Released {
            identity: released_identity,
            status: terminal.status,
        };
        selected::write(&path, &released)?;
        reservation.publish(None)
    }

    pub(super) fn finalize_selected(
        &self,
        id: &BoxId,
        transaction: &dyn Transaction,
    ) -> Result<(), Refusal> {
        let receipt =
            selected::read(&self.selected_path(id)?)?.ok_or(super::books::uncertain(id)?)?;
        let record = transaction.read(id)?.ok_or(super::books::uncertain(id)?)?;
        receipt.matches_record(&record)?;
        let Phase::Released { identity, .. } = &receipt.phase else {
            return Err(super::books::uncertain(id)?);
        };
        match identity {
            Some(identity) => managed::same_init(&record, identity)?,
            None if record.init.is_none() => {}
            None => return Err(super::books::uncertain(id)?),
        }
        transaction.finalize_managed(identity.as_ref())
    }

    pub(super) fn forget_selected(&self, id: &BoxId) -> Result<(), Refusal> {
        selected::remove(&self.selected_path(id)?)
    }

    pub fn evented_for(&self, id: &BoxId) -> Result<Option<Arc<Client>>, Refusal> {
        let booted = self.booted_for(id)?;
        let Some(booted) = booted else {
            return Ok(None);
        };
        let cached = booted.evented.lock().map_err(poisoned)?.clone();
        if let Some(client) = cached.filter(|client| !client.closed()) {
            return Ok(Some(client));
        }
        self.join(&booted)?;
        Ok(booted.evented.lock().map_err(poisoned)?.clone())
    }

    pub fn program_client_for(&self, id: &BoxId) -> Result<Option<Arc<Client>>, Refusal> {
        let booted = self.booted_for(id)?;
        let Some(booted) = booted else {
            return Ok(None);
        };
        let joins = self
            .joins
            .lock()
            .map_err(poisoned)?
            .clone()
            .ok_or_else(|| invalid("guest program has no node event loop"))?;
        let (token, room) = joins.reserve()?;
        let (client, pump) = Client::connect_evented(triomphe::Arc::clone(&booted.link), room)?;
        joins.join(token, pump)?;
        Ok(Some(Arc::new(client)))
    }
}
