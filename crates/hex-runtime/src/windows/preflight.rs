//! Read-only retained native metadata, before the keeper or any recovery cleanup is entered.
use std::path::Path;

use hex_api::{BoxId, Class, ClassSpec, LifecycleCause, Refusal};
use hex_core::hcs;
use hex_core::record::{BoxRecord, NativeRecord, NativeStage, OwnerRef, PoolRecord};

use super::{Held, WinRuntime, allocation, keeper, owner_index};

impl WinRuntime {
    /// Validate the exact retained HCS mapping without HCS calls, ownership changes or publication.
    pub fn preflight_record(&self, record: &BoxRecord) -> Result<(), Refusal> {
        let spec = &record.spec;
        self.check_native_spec(spec)?;
        let bindings = self.bindings(spec)?;
        let marker = self.marker(&spec.id)?;
        let native = super::oci::read_marker_if_present(&marker)?;
        if matches!(spec.class, ClassSpec::Process) {
            if native.is_some() || record.native_scope.is_some() {
                return Err(NativeRecord::Box(record).refusal(LifecycleCause::Uncertain));
            }
            return self.preflight_legacy_keeper(&spec.id);
        }
        // This reads the existing immutable initialized feature result; it never retries WMI.
        let features = self.features_off.get().ok_or_else(|| {
            invalid("the native feature configuration was not initialized before preflight")
        })?;
        if features.contains(&hex_core::setup::WindowsFeature::Containers) {
            return Err(invalid("the retained HCS backend is not initialized"));
        }
        let container = match &spec.class {
            ClassSpec::Oci(oci) => hcs::from_spec(
                oci,
                &super::oci::scratch_folder(&self.scratch_root, &spec.id)?,
            )?,
            ClassSpec::Bundle(path) => hcs::from_bundle(&super::oci::bundle_config(path)?)?,
            ClassSpec::Process | ClassSpec::Firecracker(_) | ClassSpec::Shards(_) => {
                return Err(NativeRecord::Box(record).refusal(LifecycleCause::Uncertain));
            }
        };
        // Missing metadata is not evidence that an asynchronous HCS create was never submitted.
        let native =
            native.ok_or_else(|| NativeRecord::Box(record).refusal(LifecycleCause::Uncertain))?;
        check_marker(
            &native,
            &spec.id,
            self.node.is_some(),
            &bindings,
            &container.scratch,
        )?;
        super::oci::service::preflight(&marker, &native)?;
        self.preflight_legacy_keeper(&spec.id)
    }

    /// Read the SAME keeper commitment without opening/adopting a job or starting the keeper.
    pub fn preflight_managed(&self, record: NativeRecord<'_>) -> Result<(), Refusal> {
        let bad = || record.refusal(LifecycleCause::Uncertain);
        let scope = record.scope().ok_or_else(bad)?;
        if record.class()? != Class::Shards {
            return Err(bad());
        }
        let commitment = record
            .allocation()
            .map(|_| record.commitment())
            .transpose()?;
        if let Some(allocation) = record.allocation() {
            allocation.guest()?;
            self.check_vm_cost(&allocation.cost)?;
        }
        let present = {
            let boxes = self.boxes()?;
            match owner_index(&boxes, record.owner()) {
                Err(_) => false,
                Ok(at) => match boxes.get(at).and_then(|(_, slot)| slot.physical()) {
                    Some(Held::Process(boxed))
                        if commitment
                            .is_some_and(|commitment| boxed.matches_vm(record, commitment)) =>
                    {
                        true
                    }
                    _ => return Err(bad()),
                },
            }
        };
        let Some(keeping) = self.holding.keeper.as_ref() else {
            return if present || record.stage() == NativeStage::Prepared {
                Ok(())
            } else {
                Err(bad())
            };
        };
        let path = keeper::vm::path(&keeping.state, record.owner())?;
        match keeper::vm::read(&path)? {
            Some(held) if held.commits(commitment.map(|commitment| (scope, commitment))) => Ok(()),
            Some(_) => Err(bad()),
            None if present
                || matches!(
                    record.stage(),
                    NativeStage::Prepared | NativeStage::Released
                ) =>
            {
                // Prepared cannot have native effects; Released is independently checked against
                // the exact backend terminal receipt. This is not a new native absence witness.
                Ok(())
            }
            None => Err(bad()),
        }
    }

    /// Native filenames are joined against the full cohort before a sibling can be recovered.
    pub fn preflight_retained(
        &self,
        records: &crate::devices::Records,
        pools: &[PoolRecord],
    ) -> Result<(), Refusal> {
        // Same bound as the keeper service: folder, marker, next marker, request directory per box.
        let bound = crate::MAX_BOXES
            .checked_mul(4)
            .ok_or_else(|| allocation(crate::MAX_BOXES))?;
        let entries = match std::fs::read_dir(&self.scratch_root) {
            Ok(entries) => Some(entries),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(Refusal::os("reading retained native marker names", error)),
        };
        if let Some(entries) = entries {
            for (count, entry) in entries.enumerate() {
                if count >= bound {
                    return Err(Refusal::Full(crate::MAX_BOXES));
                }
                let entry = entry
                    .map_err(|error| Refusal::os("reading retained native marker names", error))?;
                let name = entry.file_name();
                let name = name
                    .to_str()
                    .ok_or_else(|| invalid("a native marker name is not Unicode"))?;
                let base = name
                    .strip_suffix(".next")
                    .or_else(|| name.strip_suffix(".create"))
                    .unwrap_or(name);
                if let Some(id) = base.strip_suffix(".scratch.json") {
                    let id = BoxId::new(id)?;
                    let record = records
                        .get(&id)
                        .and_then(|record| record.as_ref().ok())
                        .ok_or_else(|| {
                            invalid("native metadata has no retained canonical owner")
                        })?;
                    self.preflight_record(record)?;
                } else if Path::new(base).extension().is_some_and(|extension| {
                    extension.as_encoded_bytes().eq_ignore_ascii_case(b"json")
                }) {
                    return Err(invalid("a retained native marker name is not canonical"));
                }
            }
        }
        let Some(keeping) = self.holding.keeper.as_ref() else {
            return Ok(());
        };
        let entries = match std::fs::read_dir(&keeping.state) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(Refusal::os("reading retained keeper names", error)),
        };
        // Box and Pool receipts plus one unpublished replacement each; global boxes directory,
        // keeper identity, ready file and native diagnostic. This is a work bound, never cleanup.
        let bound = crate::MAX_BOXES
            .checked_mul(4)
            .and_then(|bound| bound.checked_add(4))
            .ok_or_else(|| allocation(crate::MAX_BOXES))?;
        for (count, entry) in entries.enumerate() {
            if count >= bound {
                return Err(Refusal::Full(crate::MAX_BOXES));
            }
            let entry =
                entry.map_err(|error| Refusal::os("reading retained keeper names", error))?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| invalid("a retained keeper name is not Unicode"))?;
            if let Some(id) = name.strip_prefix("held-") {
                let id = BoxId::new(id)?;
                let record = records
                    .get(&id)
                    .and_then(|record| record.as_ref().ok())
                    .ok_or_else(|| invalid("a keeper receipt has no retained canonical box"))?;
                if record.spec.backend_class()? == Class::Shards {
                    self.preflight_managed(NativeRecord::Box(record))?;
                } else {
                    self.preflight_legacy_keeper(&id)?;
                }
            } else if name.starts_with("pool-held-") {
                let pool = keeper::vm::pool_from_name(name)?;
                let at = pools
                    .binary_search_by_key(&pool, |record| record.key.pool)
                    .map_err(|_| {
                        invalid("a VM keeper receipt has no retained canonical backing")
                    })?;
                let pool = pools.get(at).ok_or_else(|| allocation(pools.len()))?;
                if entry.path() != keeper::vm::path(&keeping.state, OwnerRef::Pool(&pool.key.pool))?
                {
                    return Err(invalid("a VM keeper receipt name is not canonical"));
                }
                self.preflight_managed(NativeRecord::Pool(pool))?;
            }
        }
        Ok(())
    }

    fn preflight_legacy_keeper(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(keeping) = self.holding.keeper.as_ref() {
            keeper::read_held_strict(&keeping.state, id)?;
        }
        Ok(())
    }

    /// Called only after the complete Store/configuration/metadata barrier by Recorded.
    pub fn initialize_native(&self) -> Result<(), Refusal> {
        if let Some(keeping) = self.holding.keeper.as_ref() {
            self.holding.ensure_keeper(keeping)?;
        }
        Ok(())
    }
}

fn check_marker(
    record: &hcs::BoxRecord,
    id: &BoxId,
    outlives: bool,
    bindings: &[hcs::DeviceBinding],
    scratch: &hcs::Scratch,
) -> Result<(), Refusal> {
    if record.devices != bindings
        || record.outlives != outlives
        || record.system.as_ref().is_some_and(|system| system != id)
        || ((outlives || !bindings.is_empty()) && record.system.as_ref() != Some(id))
        || record
            .init
            .is_some_and(|init| init.pid == 0 || init.created == 0 || !record.started)
    {
        return Err(invalid(
            "native marker identity, lifetime or physical binding disagrees",
        ));
    }
    super::oci::validate_storage(record, scratch)?;
    if let hcs::Scratch::Folder(folder) = scratch {
        if record.folder != *folder {
            return Err(invalid(
                "native marker scratch folder disagrees with its configured owner",
            ));
        }
        let root = Path::new(folder);
        // These are the only two paths scratch() ever introduces. No canonicalization, links or
        // path prefix grants cleanup authority to another directory.
        let mut previous: Option<bool> = None;
        for made in &record.made {
            let path = Path::new(made);
            let folder = path == root;
            let sandbox = path.parent() == Some(root)
                && path.file_name().is_some_and(|name| name == "sandbox.vhdx");
            if (!folder && !sandbox) || previous.is_some_and(|was_folder| !was_folder || folder) {
                return Err(invalid(
                    "native marker cleanup paths disagree with its owned scratch",
                ));
            }
            previous = Some(folder);
        }
    }
    Ok(())
}

pub(super) fn invalid(message: &'static str) -> Refusal {
    match super::copy_text(message) {
        Ok(why) => Refusal::Invalid {
            what: "retained native preflight",
            why,
        },
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::num::NonZeroU32;
    use std::sync::{Arc, Mutex, OnceLock};

    fn runtime(root: &Path) -> WinRuntime {
        WinRuntime {
            processors: NonZeroU32::MIN,
            catalog: super::super::NativeCatalog::Empty(crate::devices::Catalog::default()),
            scratch_root: root.to_path_buf(),
            launcher: None,
            holding: Arc::new(super::super::Holding {
                boxes: Mutex::new(Vec::new()),
                keeper: None,
            }),
            node: Some("retained-preflight-fixture".into()),
            features_off: OnceLock::from(BTreeSet::new()),
            feature_reads: std::sync::atomic::AtomicU32::new(0),
            reclaim_failures: Vec::new(),
        }
    }

    fn spec() -> hex_api::BoxSpec {
        hex_api::BoxSpec {
            id: BoxId::new("owned-marker").unwrap(),
            class: ClassSpec::Oci(Box::new(hex_api::OciSpec {
                rootfs: hex_api::Rootfs::Layers(vec![r"C:\private-fixture\base".into()]),
                init: hex_api::Program::of(&["cmd.exe", "/c", "exit", "0"]),
                mounts: Vec::new(),
                hostname: None,
                network: hex_api::Network::Private,
                hosts: Vec::new(),
            })),
            bounds: hex_api::Bounds::default(),
            owner: None,
            devices: Vec::new(),
            allocation: None,
            route: None,
        }
    }

    fn marker(runtime: &WinRuntime, record: &BoxRecord) -> hcs::BoxRecord {
        let folder =
            super::super::oci::scratch_folder(&runtime.scratch_root, &record.spec.id).unwrap();
        hcs::BoxRecord {
            made: vec![folder.clone()],
            folder,
            outlives: true,
            system: Some(record.spec.id.clone()),
            started: false,
            init: None,
            devices: Vec::new(),
            storage: hcs::RecordStorage::OwnedFolder,
            creation: hcs::NativeCreate::Created,
        }
    }

    /// T-T7.1.2: actual marker reads reject CPU-only foreign cleanup authority before adoption.
    /// Only private files and initialized metadata are used; zero HCS/WMI/job operations.
    #[test]
    fn retained_cpu_marker_preflight_preserves_foreign_system_and_storage_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let record = BoxRecord::new(spec(), hex_core::record::Phase::Ready, Vec::new());
        let native = marker(&runtime, &record);
        let path = runtime.marker(&record.spec.id).unwrap();
        std::fs::write(&path, hcs::write_record(&native).unwrap()).unwrap();
        runtime.preflight_record(&record).unwrap();
        for case in 0..5 {
            let mut malformed = native.clone();
            match case {
                0 => malformed.system = Some(BoxId::new("foreign-system").unwrap()),
                1 => malformed.folder = dir.path().join("foreign").to_str().unwrap().into(),
                2 => malformed
                    .made
                    .push(dir.path().join("foreign").to_str().unwrap().into()),
                3 => malformed.outlives = false,
                _ => {
                    malformed.storage = hcs::RecordStorage::BorrowedVolume;
                    malformed.made.clear();
                }
            }
            let bytes = hcs::write_record(&malformed).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(runtime.preflight_record(&record).is_err(), "case {case}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert!(runtime.boxes().unwrap().is_empty());
        }
        assert_eq!(
            runtime
                .feature_reads
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    /// T-T7.1.2: absent native metadata supplies no create/cancel/termination witness.
    #[test]
    fn pending_hcs_marker_absence_or_missing_service_receipt_never_initializes_native_state() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let record = BoxRecord::new(spec(), hex_core::record::Phase::Creating, Vec::new());
        assert!(runtime.preflight_record(&record).is_err());
        let mut native = marker(&runtime, &record);
        native.creation = hcs::NativeCreate::Pending;
        let bytes = hcs::write_record(&native).unwrap();
        let path = runtime.marker(&record.spec.id).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(runtime.preflight_record(&record).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(runtime.boxes().unwrap().is_empty());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// T-T7.1.2: a native marker not represented by the canonical cohort cannot be swept first.
    #[test]
    fn orphan_native_marker_refuses_without_reclaiming_or_starting_a_keeper() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = runtime(dir.path());
        let record = BoxRecord::new(spec(), hex_core::record::Phase::Ready, Vec::new());
        let path = runtime.marker(&record.spec.id).unwrap();
        let bytes = hcs::write_record(&marker(&runtime, &record)).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            runtime
                .preflight_retained(&crate::devices::Records::new(), &[])
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(runtime.holding.keeper.is_none());
    }

    /// T-T7.1.2: configuration retention cannot execute a queued native request before the barrier.
    #[test]
    fn keeper_configuration_is_read_only_and_legacy_cpu_preflight_stays_positive() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("not-created");
        let keeping =
            super::super::Keeping::configured(&state, &["hex.exe".into(), "keep".into()]).unwrap();
        assert!(matches!(
            *keeping.keeper.lock().unwrap(),
            super::super::KeeperState::Configured
        ));
        assert!(!state.exists());
        let runtime = runtime(dir.path());
        let mut cpu = spec();
        cpu.class = ClassSpec::Process;
        runtime
            .preflight_record(&BoxRecord::new(
                cpu,
                hex_core::record::Phase::Ready,
                Vec::new(),
            ))
            .unwrap();
        assert!(runtime.boxes().unwrap().is_empty());
    }
}
