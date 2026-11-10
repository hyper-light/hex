//! AC-T7.4.1/LB03: the last successfully applied remote offer, in the existing atomic file records.
//! One wire-bounded value per configured peer restores real admission facts before Driver::boot;
//! dense-zero placeholders remain only for peers whose facts have never been applied.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use hex_api::Refusal;
use hex_core::wire::MAX_MESSAGE;
use hex_sim::model::{Cluster, NodeId, NodeSpec, Resources, model_name};
use hex_sim::peer::{Records, Stop};
use serde::Serialize;

use super::{dto, offer};
use crate::records::{Files, MOST};

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "peer offer cache",
        why: why.into(),
    }
}

fn storage(why: Stop) -> Refusal {
    match why {
        Stop::Full { what, bound } => Refusal::Exhausted { what, bound },
        Stop::Damaged(why) => invalid(why),
        Stop::Io(why) => Refusal::Broken(format!("peer offer cache: {why}")),
        Stop::Crashed => Refusal::Broken("peer offer cache write stopped".into()),
    }
}

fn io(why: std::io::Error) -> Refusal {
    Refusal::Broken(format!("peer offer cache: {why}"))
}

struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > MAX_MESSAGE)
        {
            return Err(std::io::Error::other(
                "peer offer cache value exceeds wire bound",
            ));
        }
        self.0
            .try_reserve_exact(bytes.len())
            .map_err(std::io::Error::other)?;
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encoded(value: &impl Serialize) -> Result<String, Refusal> {
    let mut bytes = Bounded(Vec::new());
    serde_json::to_writer(&mut bytes, value).map_err(|why| {
        if why.is_io() {
            Refusal::Exhausted {
                what: "peer offer cache bytes",
                bound: MAX_MESSAGE,
            }
        } else {
            invalid(why.to_string())
        }
    })?;
    String::from_utf8(bytes.0).map_err(|why| invalid(why.to_string()))
}

fn name(peer: NodeId) -> String {
    format!("peer-{}", peer.0)
}

fn peer_of(name: &str, configured: &BTreeSet<u32>) -> Result<NodeId, Refusal> {
    let peer = name
        .strip_prefix("peer-")
        .and_then(|n| n.parse::<u32>().ok())
        .map(NodeId)
        .filter(|peer| self::name(*peer) == name)
        .ok_or_else(|| invalid(format!("invalid record name {name:?}")))?;
    if !configured.contains(&peer.0) {
        return Err(invalid(format!("record for unconfigured peer {}", peer.0)));
    }
    Ok(peer)
}

fn checked_folder(path: &Path) -> Result<(), Refusal> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_dir() => Err(invalid(
            "cache path must be a directory, not a link or other file",
        )),
        Ok(_) => Ok(()),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(why) => Err(io(why)),
    }
}

fn checked_records(path: &Path) -> Result<(), Refusal> {
    // Files already applies this directory bound. Check record types before its reader opens one,
    // so a record symlink cannot redirect the bounded read outside the leased state directory.
    let bound = MOST
        .checked_mul(2)
        .ok_or_else(|| invalid("record directory bound overflow"))?;
    for (at, entry) in fs::read_dir(path).map_err(io)?.enumerate() {
        if at >= bound {
            return Err(Refusal::Exhausted {
                what: "peer offer cache directory entries",
                bound,
            });
        }
        let entry = entry.map_err(io)?;
        if entry.path().extension().is_some_and(|ext| ext == "rec")
            && !entry.file_type().map_err(io)?.is_file()
        {
            return Err(invalid("cache records must be regular files"));
        }
    }
    Ok(())
}

fn checked_models(models: &[String]) -> Result<(), Refusal> {
    encoded(&models)?;
    let mut seen = BTreeSet::new();
    for model in models {
        if model.is_empty() || model == "-" || model_name(model) != *model || !seen.insert(model) {
            return Err(invalid("shared model names must be canonical and unique"));
        }
    }
    Ok(())
}

/// Bounded by the configured peer set and Files' existing record budget. Models retain the same
/// append-only intern-table semantics as offer::decode, with its common-wire byte bound.
#[derive(Debug)]
pub struct Cache {
    files: Files,
    configured: BTreeSet<u32>,
    models: Vec<String>,
}

impl Cache {
    /// Restores matching nodes atomically: no caller node/model mutation on any corrupt record.
    pub fn open(
        path: &Path,
        configured: &BTreeSet<u32>,
        cluster: &mut Cluster,
    ) -> Result<Self, Refusal> {
        if configured.len() > MOST {
            return Err(Refusal::Exhausted {
                what: "peer offer cache peers",
                bound: MOST,
            });
        }
        checked_models(&cluster.models)?;
        for peer in configured {
            if usize::try_from(*peer)
                .ok()
                .and_then(|at| cluster.nodes.get(at))
                .is_none_or(|node| node.id != NodeId(*peer))
            {
                return Err(invalid("configured peer must name its dense cluster node"));
            }
        }
        checked_folder(path)?;
        let mut files = Files::open(path).map_err(storage)?;
        checked_records(path)?;
        let mut staged = Cluster::single_owner(cluster.nodes.clone(), Vec::new());
        staged.models = cluster.models.clone();
        for (record, value) in files.all().map_err(storage)? {
            let peer = peer_of(&record, configured)?;
            let offer: dto::Offer =
                serde_json::from_str(&value).map_err(|why| invalid(why.to_string()))?;
            let spec = offer::decode(&offer, peer, &mut staged)?;
            let slot = usize::try_from(peer.0)
                .ok()
                .and_then(|at| staged.nodes.get_mut(at))
                .filter(|node| node.id == peer)
                .ok_or_else(|| invalid("cached peer has no matching cluster slot"))?;
            *slot = spec;
        }
        cluster.nodes = staged.nodes;
        cluster.models = staged.models;
        Ok(Self {
            files,
            configured: configured.clone(),
            models: cluster.models.clone(),
        })
    }

    /// The shell calls this after successful Driver::refresh_node and before any retry/placement.
    /// It treats failure as fatal until restart; no in-memory cache metadata commits before rename.
    pub fn put(&mut self, peer: NodeId, offer: &dto::Offer) -> Result<(), Refusal> {
        if !self.configured.contains(&peer.0) {
            return Err(invalid("cannot cache an unconfigured peer"));
        }
        let text = encoded(offer)?;
        // Decode requires configured membership, not dense slots. One placeholder validates this
        // offer while only the bounded intern table is copied, rather than the whole live fleet.
        let mut staged =
            Cluster::single_owner(vec![NodeSpec::new(peer, Resources::default())], Vec::new());
        staged.models = self.models.clone();
        offer::decode(offer, peer, &mut staged)?;
        self.files.put(&name(peer), &text).map_err(storage)?;
        self.models = staged.models;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_sim::model::{Device, ModelId};

    fn configured() -> BTreeSet<u32> {
        BTreeSet::from([1])
    }
    fn cluster() -> Cluster {
        Cluster::single_owner(
            vec![
                NodeSpec::new(NodeId(0), Resources::default()),
                NodeSpec::new(NodeId(1), Resources::default()),
            ],
            Vec::new(),
        )
    }
    fn advertised() -> dto::Offer {
        let mut spec = NodeSpec::new(
            NodeId(1),
            Resources {
                cpu_milli: 4_000,
                mem_bytes: 8 << 30,
                gpus: 2,
                ..Resources::default()
            },
        );
        spec.devices = vec![
            Device::gpu(ModelId(0), 8 << 30),
            Device::gpu(ModelId(1), 16 << 30),
        ];
        offer::from_local(&spec, &["nvidia-rtx".into(), "amd-radeon".into()]).unwrap()
    }

    /// AC-T7.4.1/LB03: the latest applied offer survives reboot and replaces zero placeholders with
    /// full facts, remapping named GPUs into the rebooted process's independently ordered table.
    #[test]
    fn cached_offer_survives_reboot_and_remaps_models() {
        let dir = tempfile::tempdir().unwrap();
        let mut before = cluster();
        let mut cache = Cache::open(dir.path(), &configured(), &mut before).unwrap();
        let mut offer = advertised();
        cache.put(NodeId(1), &offer).unwrap();
        let mut spec: NodeSpec = serde_json::from_str(&offer.spec).unwrap();
        spec.capacity.mem_bytes += 1;
        offer = offer::from_local(&spec, &offer.models).unwrap();
        cache.put(NodeId(1), &offer).unwrap();
        drop(cache);
        let mut after = cluster();
        after.models = vec!["amd-radeon".into(), "nvidia-rtx".into()];
        Cache::open(dir.path(), &configured(), &mut after).unwrap();
        assert_eq!(after.nodes[1].capacity, spec.capacity);
        assert_eq!(after.nodes[1].devices[0].model, ModelId(1));
        assert_eq!(after.nodes[1].devices[1].model, ModelId(0));
        assert_eq!(after.nodes[1].devices[0].size, spec.devices[0].size);
        assert_eq!(after.models.len(), 2);
        assert_eq!(Files::open(dir.path()).unwrap().all().unwrap().len(), 1);
    }

    /// AC-T7.4.1/LB03: unknown peers, aliases, malformed JSON and missing models are input errors;
    /// none partially changes the caller's boot facts.
    #[test]
    fn unknown_peers_and_corrupt_cached_records_are_refused() {
        let good = serde_json::to_string(&advertised()).unwrap();
        let mut bad = advertised();
        bad.models.clear();
        for (name, value) in [
            ("peer-2", good.clone()),
            ("peer-01", good),
            ("peer-1", "{".into()),
            ("peer-1", serde_json::to_string(&bad).unwrap()),
        ] {
            let dir = tempfile::tempdir().unwrap();
            Files::open(dir.path()).unwrap().put(name, &value).unwrap();
            let mut facts = cluster();
            let before = facts.clone();
            assert!(
                Cache::open(dir.path(), &configured(), &mut facts).is_err(),
                "{name}"
            );
            assert_eq!(facts, before);
        }
    }

    /// AC-T7.4.1/LB03: a corrupt later offer cannot leave an earlier cached peer/model half applied.
    #[test]
    fn boot_cache_loading_is_atomic_across_peers() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = Files::open(dir.path()).unwrap();
        files
            .put("peer-1", &serde_json::to_string(&advertised()).unwrap())
            .unwrap();
        files.put("peer-2", "broken").unwrap();
        let mut facts = cluster();
        facts
            .nodes
            .push(NodeSpec::new(NodeId(2), Resources::default()));
        let before = facts.clone();
        assert!(Cache::open(dir.path(), &BTreeSet::from([1, 2]), &mut facts).is_err());
        assert_eq!(facts, before);
    }

    /// CLAUDE.md §2/AC-T7.4.1: an oversized replacement is refused before it alters durable state.
    #[test]
    fn oversized_offer_cannot_replace_a_valid_cached_offer() {
        let dir = tempfile::tempdir().unwrap();
        let mut facts = cluster();
        let mut cache = Cache::open(dir.path(), &configured(), &mut facts).unwrap();
        let good = advertised();
        cache.put(NodeId(1), &good).unwrap();
        let bad = dto::Offer {
            spec: "x".repeat(MAX_MESSAGE + 1),
            models: Vec::new(),
        };
        assert!(matches!(
            cache.put(NodeId(1), &bad),
            Err(Refusal::Exhausted { .. })
        ));
        assert_eq!(
            cache.files.all().unwrap()[0].1,
            serde_json::to_string(&good).unwrap()
        );
    }

    /// AC-T7.4.1/LB03: a record link must not redirect boot's file read outside the leased state.
    #[cfg(unix)]
    #[test]
    fn linked_cache_record_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            outside.path(),
            serde_json::to_string(&advertised()).unwrap(),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("peer-1.rec")).unwrap();
        assert!(Cache::open(dir.path(), &configured(), &mut cluster()).is_err());
    }
}
