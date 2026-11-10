//! One selected box per GPU guest. This is a route/VM-lifetime table; the existing
//! backend box records and scheduler Index remain the only device-holder authority.

use super::selected::{self, Phase, Record};
use super::*;

pub(super) struct Guest {
    pub receipt: Record,
    pub booted: Arc<Booted>,
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

impl GuestRuntime {
    /// Immutable backend configuration; callers cannot introduce a binding in a BoxSpec.
    pub fn with_gpu_catalog(mut self, catalog: Arc<crate::devices::Catalog>) -> Self {
        self.gpu_catalog = Some(catalog);
        self
    }

    /// The configured dedicated VM envelope. Admission must reserve this before create;
    /// payload bounds are ceilings within it, not permission for an uncharged second VM.
    pub fn dedicated_resources(&self) -> Result<hex_api::CapacityResources, Refusal> {
        selected::resources(&self.config)
    }

    fn selected_dir(&self) -> Result<PathBuf, Refusal> {
        let record = self
            .record
            .as_ref()
            .ok_or_else(|| invalid("exact GPU guests require a durable guest record directory"))?;
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
        let directory = self.selected_dir()?;
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Refusal::os("listing selected guest receipts", error)),
        };
        let mut ids = Vec::new();
        for entry in entries {
            if ids.len() >= crate::MAX_BOXES {
                return Err(Refusal::Exhausted {
                    what: "selected guest receipts",
                    bound: crate::MAX_BOXES,
                });
            }
            let entry =
                entry.map_err(|e| Refusal::os("reading selected guest receipt entry", e))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| invalid("receipt filename is not UTF-8"))?;
            // Atomic replacement may leave its own temporary file after a crash. It is
            // never an independent guest or authority to start one.
            if name.starts_with('.') && name.ends_with(".tmp") {
                continue;
            }
            let id = BoxId::new(name)?;
            ids.try_reserve(1).map_err(|_| Refusal::Exhausted {
                what: "selected guest receipts",
                bound: crate::MAX_BOXES,
            })?;
            ids.push(id);
        }
        ids.sort_unstable();
        Ok(ids)
    }

    fn selected_spec(&self, receipt: &Record) -> Result<shards_api::GuestSpec, Refusal> {
        let catalog = self
            .gpu_catalog
            .as_ref()
            .ok_or_else(|| invalid("no initialized exact Shards GPU catalog"))?;
        let mut spec = self.config.spec();
        spec.gpu = true;
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
        // The dedicated guest's trusted boot configuration, never a peer-provided path or
        // a device ordinal. Guest startup maps its sole virtio renderer to these host IDs.
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

    /// Recovery calls this with its original durable BoxSpec before inspecting/adopting.
    /// A current load order or changed catalog never chooses a replacement physical GPU.
    pub fn validate_binding(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        if spec.devices.is_empty() {
            return Ok(());
        }
        let path = self.selected_path(&spec.id)?;
        if let Some(receipt) = selected::read(&path)? {
            receipt.matches(spec, &self.config)?;
            self.selected_spec(&receipt)?;
        }
        Ok(())
    }

    fn selected_locked(
        &self,
        guests: &mut Vec<Guest>,
        id: &BoxId,
    ) -> Result<Option<Arc<Booted>>, Refusal> {
        if let Some(guest) = guests.iter().find(|guest| guest.receipt.id == *id) {
            if !matches!(guest.receipt.phase, Phase::Running(_)) {
                return Err(invalid("selected guest is stopping"));
            }
            return Ok(Some(Arc::clone(&guest.booted)));
        }
        let path = self.selected_path(id)?;
        let Some(receipt) = selected::read(&path)? else {
            return Ok(None);
        };
        if receipt.id != *id
            || receipt.vcpus != self.config.vcpus
            || receipt.memory_mib != self.config.memory_mib
        {
            return Err(invalid(
                "receipt names another box or configured VM envelope",
            ));
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
                return Ok(None);
            }
            Err(why) => return Err(refusal("taking back a selected GPU guest", &why)),
        };
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
        });
        self.join(&booted)?;
        if guests.len() >= crate::MAX_BOXES {
            return Err(Refusal::Exhausted {
                what: "selected guest routes",
                bound: crate::MAX_BOXES,
            });
        }
        guests.try_reserve(1).map_err(|_| Refusal::Exhausted {
            what: "selected guest routes",
            bound: crate::MAX_BOXES,
        })?;
        guests.push(Guest {
            receipt,
            booted: Arc::clone(&booted),
        });
        Ok(Some(booted))
    }

    pub(super) fn selected_running(&self, id: &BoxId) -> Result<Option<Arc<Booted>>, Refusal> {
        // CPU-only configurations need no selected receipt directory.
        if self.record.is_none() {
            return Ok(None);
        }
        let mut guests = self.dedicated.lock().map_err(poisoned)?;
        if let Some(guest) = guests.iter().find(|guest| guest.receipt.id == *id) {
            match guest.booted.guest.wait_for(Duration::ZERO) {
                Ok(Some(_)) => {
                    selected::remove(&self.selected_path(id)?)?;
                    guests.retain(|guest| guest.receipt.id != *id);
                    return Ok(None);
                }
                Ok(None) => {}
                Err(why) => return Err(refusal("observing selected GPU guest lifetime", &why)),
            }
        }
        self.selected_locked(&mut guests, id)
    }

    pub(super) fn create_selected(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        let mut guests = self.dedicated.lock().map_err(poisoned)?;
        let path = self.selected_path(&spec.id)?;
        if guests.iter().any(|guest| guest.receipt.id == spec.id)
            || selected::read(&path)?.is_some()
        {
            return Err(Refusal::Exists(spec.id));
        }
        let mut receipt = Record::intent(&spec, &self.config)?;
        let guest_spec = self.selected_spec(&receipt)?;
        if guests.len() >= crate::MAX_BOXES {
            return Err(Refusal::Exhausted {
                what: "selected guest routes",
                bound: crate::MAX_BOXES,
            });
        }
        guests.try_reserve(1).map_err(|_| Refusal::Exhausted {
            what: "selected guest routes",
            bound: crate::MAX_BOXES,
        })?;
        std::fs::create_dir_all(self.selected_dir()?)
            .map_err(|e| Refusal::os("creating selected guest receipt directory", e))?;
        if let Some(parent) = self.selected_dir()?.parent() {
            held::sync_folder(parent)
                .map_err(|e| Refusal::os("syncing selected guest receipt directory", e))?;
        }
        selected::write(&path, &receipt)?;
        // There is deliberately no retry here: an uncertain launch cannot authorize a
        // second VM on the same physical selection.
        let guest: Arc<dyn shards_api::Guest> = self
            .guests
            .boot(&guest_spec)
            .map_err(|why| refusal("booting an exactly selected GPU guest", &why))?
            .into();
        let identity = guest
            .identity()
            .map_err(|why| refusal("recording selected guest identity", &why))?;
        receipt.phase = Phase::Running(identity);
        selected::write(&path, &receipt)?;
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
        });
        self.join(&booted)?;
        guests.push(Guest {
            receipt,
            booted: Arc::clone(&booted),
        });
        drop(guests);
        self.create_in(&booted, spec)
    }

    pub(super) fn delete_selected(&self, id: &BoxId) -> Result<bool, Refusal> {
        if self.record.is_none() {
            return Ok(false);
        }
        let mut guests = self.dedicated.lock().map_err(poisoned)?;
        let path = self.selected_path(id)?;
        let Some(mut receipt) = selected::read(&path)? else {
            return Ok(false);
        };
        let guest = match guests.iter().find(|guest| guest.receipt.id == *id) {
            Some(guest) => Arc::clone(&guest.booted.guest),
            None => match self
                .guests
                .attach(receipt.identity()?, &self.selected_spec(&receipt)?)
            {
                Ok(guest) => Arc::<dyn shards_api::Guest>::from(guest),
                Err(why) if absent(&why) => {
                    selected::remove(&path)?;
                    return Ok(true);
                }
                Err(why) => return Err(refusal("taking back a selected guest to stop", &why)),
            },
        };
        // Stopping never depends on a live relay. Ending this VM ends the only box
        // it may hold, including an init stuck before answering its first request.
        receipt.phase = Phase::Stopping(receipt.identity()?.clone());
        selected::write(&path, &receipt)?;
        if let Some(known) = guests.iter_mut().find(|guest| guest.receipt.id == *id) {
            known.receipt.phase = receipt.phase.clone();
        }
        let stopped = guest.stop();
        let terminal = guest
            .wait_for(self.first_dial_bound()?)
            .map_err(|why| refusal("proving selected GPU guest termination", &why))?;
        if terminal.is_none() {
            if let Err(why) = stopped {
                return Err(refusal("stopping selected GPU guest", &why));
            }
            return Err(invalid("selected GPU guest termination remains unproved"));
        }
        selected::remove(&path)?;
        guests.retain(|guest| guest.receipt.id != *id);
        Ok(true)
    }

    /// Event/control tokens remain scoped to the VM which owns this exact BoxId.
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
            .ok_or_else(|| invalid("the guest program has no node event loop"))?;
        let (token, room) = joins.reserve()?;
        let (client, pump) = Client::connect_evented(Arc::clone(&booted.link), room)?;
        joins.join(token, pump)?;
        Ok(Some(Arc::new(client)))
    }
}
