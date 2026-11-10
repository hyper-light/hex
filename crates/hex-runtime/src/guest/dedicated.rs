//! A selected box's VM route and lifetime. Durable box records remain the only
//! device-holder authority. One short-lock route table holds per-box reservations;
//! no table or state lock is held over file, guest, channel or callback operations.

use super::selected::{self, Phase, Record};
use super::*;

pub(super) struct Guest {
    receipt: Record,
    booted: Arc<Booted>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::{Bounds, BoxState};
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    struct Controlled {
        ended: AtomicBool,
        stops: AtomicUsize,
        wait: Box<dyn Fn() + Send + Sync>,
    }

    impl shards_api::Guest for Controlled {
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
            Ok(self
                .ended
                .load(Ordering::SeqCst)
                .then_some(shards_api::End::Stopped))
        }
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
    ) -> Arc<Booted> {
        let (link, client) = super::super::tests::model_connection();
        let booted = Arc::new(Booted {
            guest,
            link,
            client,
            evented: Mutex::new(None),
            owner: Some((id.clone(), Class::Shards)),
        });
        let receipt = record(id);
        std::fs::create_dir_all(runtime.selected_dir().unwrap()).unwrap();
        selected::write(&runtime.selected_path(id).unwrap(), &receipt).unwrap();
        runtime
            .reserve(id, Operation::Creating)
            .unwrap()
            .publish(Some(Guest {
                receipt,
                booted: Arc::clone(&booted),
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
        assert!(Arc::ptr_eq(
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
        let guest = Arc::new(Controlled {
            ended: AtomicBool::new(false),
            stops: AtomicUsize::new(0),
            wait: Box::new(|| {}),
        });
        install(&runtime, &id, guest.clone());
        assert!(runtime.delete_selected(&id).is_err());
        assert_eq!(guest.stops.load(Ordering::SeqCst), 1);
        assert!(matches!(
            selected::read(&runtime.selected_path(&id).unwrap())
                .unwrap()
                .unwrap()
                .phase,
            Phase::Stopping(_)
        ));
        assert!(runtime.selected_running(&id).is_err());
        guest.ended.store(true, Ordering::SeqCst);
        assert_eq!(runtime.delete_selected(&id), Ok(true));
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
                stops: AtomicUsize::new(0),
                wait: Box::new(|| {}),
            }),
        );
        assert_eq!(booted.host_info(info(&id)).unwrap().class, Class::Shards);
        assert!(
            booted
                .host_info(info(&BoxId::new("foreign-box").unwrap()))
                .is_err()
        );
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Loading,
    Creating,
    Stopping,
}

enum Route {
    Idle,
    Busy(Operation),
    Ready(Guest),
    Retired,
}

pub(super) struct Slot {
    id: BoxId,
    route: Mutex<Route>,
}

/// Owning a reservation excludes a second same-ID operation without keeping a
/// mutex across callbacks. A failed operation restores its last known route;
/// its on-disk receipt still decides whether physical absence is proved.
struct Reservation<'a> {
    runtime: &'a GuestRuntime,
    slot: Arc<Slot>,
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
        let empty = next.is_none();
        *self.slot.route.lock().map_err(poisoned)? = next.map_or(Route::Retired, Route::Ready);
        if empty {
            self.runtime
                .dedicated
                .lock()
                .map_err(poisoned)?
                .retain(|slot| !Arc::ptr_eq(slot, &self.slot));
        }
        Ok(())
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Restore ownership even after an unrelated test panic poisoned a lock.
        // This destructor neither starts a VM nor deletes a durable receipt.
        let next = self.previous.take();
        let empty = next.is_none();
        *self
            .slot
            .route
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            next.map_or(Route::Retired, Route::Ready);
        if empty {
            self.runtime
                .dedicated
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|slot| !Arc::ptr_eq(slot, &self.slot));
        }
    }
}

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "dedicated GPU guest",
        why: why.into(),
    }
}

fn absent(why: &shards_api::Refusal) -> bool {
    matches!(
        why,
        shards_api::Refusal::Gone { .. }
            | shards_api::Refusal::Ended(_)
            | shards_api::Refusal::Process {
                failure: shards_api::ProcessFailure::DifferentStart { .. }
                    | shards_api::ProcessFailure::Exited,
                ..
            }
    )
}

impl Booted {
    fn host_info(&self, mut info: BoxInfo) -> Result<BoxInfo, Refusal> {
        if let Some((id, class)) = &self.owner {
            if info.id != *id || info.class != Class::Oci {
                return Err(invalid(
                    "dedicated guest returned a foreign box or workload class",
                ));
            }
            info.class = *class;
        }
        Ok(info)
    }
}

impl GuestRuntime {
    pub fn with_gpu_catalog(mut self, catalog: Arc<crate::devices::Catalog>) -> Self {
        self.gpu_catalog = Some(catalog);
        self
    }

    /// Charge this configured VM envelope before create; the box's payload ceiling
    /// is within it. Parent/VMM overhead still needs its separately measured charge.
    pub fn dedicated_resources(&self) -> Result<hex_api::CapacityResources, Refusal> {
        selected::resources(&self.config)
    }

    fn reserve(&self, id: &BoxId, operation: Operation) -> Result<Reservation<'_>, Refusal> {
        let slot = {
            let mut routes = self.dedicated.lock().map_err(poisoned)?;
            if let Some(slot) = routes.iter().find(|slot| slot.id == *id) {
                Arc::clone(slot)
            } else {
                if routes.len() >= crate::MAX_BOXES {
                    return Err(Refusal::Full(crate::MAX_BOXES));
                }
                routes.try_reserve(1).map_err(|_| Refusal::Exhausted {
                    what: "guest routes",
                    bound: crate::MAX_BOXES,
                })?;
                let slot = Arc::new(Slot {
                    id: id.try_clone()?,
                    route: Mutex::new(Route::Idle),
                });
                routes.push(Arc::clone(&slot));
                slot
            }
        };
        let previous = {
            let mut route = slot.route.lock().map_err(poisoned)?;
            match std::mem::replace(&mut *route, Route::Busy(operation)) {
                Route::Idle => None,
                Route::Ready(guest) => Some(guest),
                Route::Busy(existing) => {
                    *route = Route::Busy(existing);
                    // One in-flight lifecycle operation per BoxId is the ownership
                    // invariant, not a tuning/concurrency allowance.
                    return Err(Refusal::Exhausted {
                        what: "same-box guest operation in flight",
                        bound: 1,
                    });
                }
                Route::Retired => {
                    *route = Route::Retired;
                    return Err(invalid("the previous guest route retired during admission"));
                }
            }
        };
        Ok(Reservation {
            runtime: self,
            slot,
            previous,
            finished: false,
        })
    }

    fn selected_dir(&self) -> Result<PathBuf, Refusal> {
        let record = self
            .record
            .as_ref()
            .ok_or_else(|| invalid("dedicated guests require durable receipts"))?;
        let mut dir = record.as_os_str().to_os_string();
        dir.push(".gpu");
        Ok(PathBuf::from(dir))
    }

    fn selected_path(&self, id: &BoxId) -> Result<PathBuf, Refusal> {
        Ok(self.selected_dir()?.join(id.as_str()))
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
        let mut spec = self.config.spec();
        spec.gpu = !receipt.devices.is_empty();
        if receipt.devices.is_empty() {
            return Ok(spec);
        }
        let catalog = self
            .gpu_catalog
            .as_ref()
            .ok_or_else(|| invalid("no initialized exact Shards GPU catalog"))?;
        spec.gpu_devices
            .try_reserve_exact(receipt.devices.len())
            .map_err(|_| Refusal::Exhausted {
                what: "selected Shards GPU identities",
                bound: receipt.devices.len(),
            })?;
        for id in &receipt.devices {
            let entry = catalog
                .entries()
                .iter()
                .find(|entry| entry.info.id == *id)
                .ok_or_else(|| invalid("recorded GPU has no configured native binding"))?;
            let crate::devices::Binding::ShardsGpu { physical } = &entry.binding else {
                return Err(invalid("recorded GPU is not bound to the Shards renderer"));
            };
            spec.gpu_devices.push(physical.clone());
        }
        use std::fmt::Write as _;
        write!(spec.cmdline, " hex.gpu.box={} hex.gpu=", receipt.id)
            .map_err(|_| invalid("cannot encode GPU boot authority"))?;
        for (index, id) in receipt.devices.iter().enumerate() {
            if index != 0 {
                spec.cmdline.push(',');
            }
            write!(spec.cmdline, "{}", id.key)
                .map_err(|_| invalid("cannot encode GPU boot authority"))?;
        }
        Ok(spec)
    }

    pub fn validate_binding(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        if spec.devices.is_empty() && !matches!(spec.class, ClassSpec::Shards(_)) {
            return Ok(());
        }
        if let Some(receipt) = selected::read(&self.selected_path(&spec.id)?)? {
            receipt.matches(spec, &self.config)?;
            self.selected_spec(&receipt)?;
        }
        Ok(())
    }

    fn booted(
        &self,
        guest: Arc<dyn shards_api::Guest>,
        receipt: &Record,
    ) -> Result<Arc<Booted>, Refusal> {
        let link: Arc<dyn Link> = Arc::new(ShardsLink::new(
            Arc::clone(&guest),
            self.first_dial_bound()?,
        ));
        let client = Client::connect(Arc::clone(&link))?;
        let booted = Arc::new(Booted {
            guest,
            link,
            client,
            evented: Mutex::new(None),
            owner: Some((receipt.id.try_clone()?, receipt.class)),
        });
        self.join(&booted)?;
        Ok(booted)
    }

    pub(super) fn selected_running(&self, id: &BoxId) -> Result<Option<Arc<Booted>>, Refusal> {
        if self.record.is_none() {
            return Ok(None);
        }
        let mut reservation = self.reserve(id, Operation::Loading)?;
        if let Some(known) = reservation.previous.take() {
            let booted = Arc::clone(&known.booted);
            reservation.previous = Some(known);
            match booted.guest.wait_for(Duration::ZERO) {
                Ok(Some(_)) => {
                    selected::remove(&self.selected_path(id)?)?;
                    reservation.publish(None)?;
                    return Ok(None);
                }
                Ok(None) => {}
                Err(why) => return Err(refusal("observing selected guest lifetime", &why)),
            }
            if reservation
                .previous
                .as_ref()
                .is_some_and(|known| !matches!(known.receipt.phase, Phase::Running(_)))
            {
                return Err(invalid("selected guest is stopping"));
            }
            let known = reservation.previous.take();
            reservation.publish(known)?;
            return Ok(Some(booted));
        }
        let path = self.selected_path(id)?;
        let Some(receipt) = selected::read(&path)? else {
            reservation.publish(None)?;
            return Ok(None);
        };
        if receipt.id != *id
            || receipt.vcpus != self.config.vcpus
            || receipt.memory_mib != self.config.memory_mib
        {
            return Err(invalid("receipt disagrees with its box or VM envelope"));
        }
        let identity = receipt.identity()?;
        if !matches!(receipt.phase, Phase::Running(_)) {
            return Err(invalid("selected guest is stopping"));
        }
        let spec = self.selected_spec(&receipt)?;
        let guest: Arc<dyn shards_api::Guest> = match self.guests.attach(identity, &spec) {
            Ok(guest) => guest.into(),
            Err(why) if absent(&why) => {
                selected::remove(&path)?;
                reservation.publish(None)?;
                return Ok(None);
            }
            Err(why) => return Err(refusal("taking back selected guest", &why)),
        };
        let booted = self.booted(guest, &receipt)?;
        reservation.publish(Some(Guest {
            receipt,
            booted: Arc::clone(&booted),
        }))?;
        Ok(Some(booted))
    }

    pub(super) fn create_selected(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        let reservation = self.reserve(&spec.id, Operation::Creating)?;
        let path = self.selected_path(&spec.id)?;
        if reservation.previous.is_some() || selected::read(&path)?.is_some() {
            return Err(Refusal::Exists(spec.id));
        }
        let mut receipt = Record::intent(&spec, &self.config)?;
        let guest_spec = self.selected_spec(&receipt)?;
        let dir = self.selected_dir()?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| Refusal::os("creating selected guest receipt directory", e))?;
        if let Some(parent) = dir.parent() {
            held::sync_folder(parent)
                .map_err(|e| Refusal::os("syncing selected guest receipt directory", e))?;
        }
        selected::write(&path, &receipt)?;
        let guest: Arc<dyn shards_api::Guest> = self
            .guests
            .boot(&guest_spec)
            .map_err(|why| refusal("booting exactly selected guest", &why))?
            .into();
        receipt.phase = Phase::Running(
            guest
                .identity()
                .map_err(|why| refusal("recording selected guest identity", &why))?,
        );
        selected::write(&path, &receipt)?;
        let booted = self.booted(guest, &receipt)?;
        // The same-ID reservation also covers staging/create; callbacks cannot
        // delete/recreate this slot while its first box is still being made.
        let created = self.create_in(&booted, spec);
        reservation.publish(Some(Guest { receipt, booted }))?;
        created
    }

    pub(super) fn delete_selected(&self, id: &BoxId) -> Result<bool, Refusal> {
        if self.record.is_none() {
            return Ok(false);
        }
        let mut reservation = self.reserve(id, Operation::Stopping)?;
        let path = self.selected_path(id)?;
        let Some(mut receipt) = selected::read(&path)? else {
            if reservation.previous.is_some() {
                return Err(invalid("live selected route lost its receipt"));
            }
            reservation.publish(None)?;
            return Ok(false);
        };
        if receipt.id != *id {
            return Err(invalid("receipt names another box"));
        }
        let guest = match reservation.previous.as_ref() {
            Some(known) => Arc::clone(&known.booted.guest),
            None => match self
                .guests
                .attach(receipt.identity()?, &self.selected_spec(&receipt)?)
            {
                Ok(guest) => Arc::<dyn shards_api::Guest>::from(guest),
                Err(why) if absent(&why) => {
                    selected::remove(&path)?;
                    reservation.publish(None)?;
                    return Ok(true);
                }
                Err(why) => return Err(refusal("taking back selected guest to stop", &why)),
            },
        };
        receipt.phase = Phase::Stopping(receipt.identity()?.clone());
        selected::write(&path, &receipt)?;
        if let Some(known) = reservation.previous.as_mut() {
            known.receipt.phase = receipt.phase.clone();
        }
        let stopped = guest.stop();
        let terminal = guest
            .wait_for(self.first_dial_bound()?)
            .map_err(|why| refusal("proving selected guest termination", &why))?;
        if terminal.is_none() {
            if let Err(why) = stopped {
                return Err(refusal("stopping selected guest", &why));
            }
            return Err(invalid("selected guest termination remains unproved"));
        }
        selected::remove(&path)?;
        reservation.publish(None)?;
        Ok(true)
    }

    pub fn evented_for(&self, id: &BoxId) -> Result<Option<Arc<Client>>, Refusal> {
        let booted = match self.selected_running(id)? {
            Some(guest) => Some(guest),
            None => self.running()?,
        };
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
        let booted = match self.selected_running(id)? {
            Some(guest) => Some(guest),
            None => self.running()?,
        };
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
        let (client, pump) = Client::connect_evented(Arc::clone(&booted.link), room)?;
        joins.join(token, pump)?;
        Ok(Some(Arc::new(client)))
    }
}
