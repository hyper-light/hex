//! The machine's capacity view in the node agent (joint plan step 13, `docs/design/capacity-view.md`;
//! AC-T7.4.1): the keeper's shell. Its records are a folder of their own, written whole by the node's
//! record files (CV2). The room it keeps is the node's facts' capacity, and the node admits its own boxes
//! against that room less every holder's reservations (CV3), under the placement lock that serializes
//! both, so the two admissions are one ledger's. Holders reach it over Hex's transport (Hex step 4);
//! until that is on main, in process.

use std::collections::BTreeMap;
use std::path::Path;

use hex_api::capacity::{self, Amounts, Forgotten, Keeper, NotForgotten, ToHolder, ToKeeper};
use hex_sim::model::{NodeSpec, Resources};
use hex_sim::peer::{Records as _, Stop};

use crate::records::{Files, MAX_BYTES};

/// The bytes a holder's record spends besides its name: its epoch, a u64 in decimal, and one space.
const EPOCH_FIELD: usize = 21;

/// The keeper's records in the node's record files.
struct KeeperFiles(Files);

impl capacity::Records for KeeperFiles {
    fn put(&mut self, name: &str, value: &str) -> Result<(), capacity::Stop> {
        self.0.put(name, value).map_err(to_capacity)
    }

    fn remove(&mut self, name: &str) -> Result<(), capacity::Stop> {
        self.0.remove(name).map_err(to_capacity)
    }

    fn all(&mut self) -> Result<Vec<(String, String)>, capacity::Stop> {
        self.0.all().map_err(to_capacity)
    }
}

fn to_capacity(stop: Stop) -> capacity::Stop {
    match stop {
        Stop::Crashed => capacity::Stop::Crashed,
        Stop::Damaged(why) => capacity::Stop::Damaged(why),
        Stop::Io(why) => capacity::Stop::Io(why),
        Stop::Full { what, bound } => capacity::Stop::Full { what, bound },
    }
}

pub(super) fn to_core(stop: capacity::Stop) -> Stop {
    match stop {
        capacity::Stop::Crashed => Stop::Crashed,
        capacity::Stop::Damaged(why) => Stop::Damaged(why),
        capacity::Stop::Io(why) => Stop::Io(why),
        capacity::Stop::Full { what, bound } => Stop::Full { what, bound },
    }
}

/// A node's resources as the view counts them.
pub(super) fn amounts(r: &Resources) -> Amounts {
    Amounts {
        cpu_milli: r.cpu_milli,
        mem_bytes: r.mem_bytes,
        disk_bytes: r.disk_bytes,
        gpus: u64::from(r.gpus),
    }
}

/// The keeper with its records, the node's facts and the links its holders came on (module docs).
pub(super) struct View {
    keeper: Keeper,
    records: KeeperFiles,
    /// The node's facts as last refreshed, before any holder's reservations come off.
    facts: Option<NodeSpec>,
    /// The host's own held as the holders were last owed it.
    told: Amounts,
    /// Each link holding a current session, and that session.
    links: BTreeMap<u64, u64>,
}

impl View {
    /// Opens the keeper's records in `dir`, its bounds derived from the node's (CV8): `holders`, the
    /// holders the node authorizes; each holding at most `ran`, as many as the node's records keep of its
    /// own boxes; a name at most a record's bytes less its epoch field.
    pub(super) fn open(dir: &Path, holders: usize, ran: usize) -> Result<Self, Stop> {
        let full = |what| Stop::Full {
            what,
            bound: usize::MAX,
        };
        let name_bytes = MAX_BYTES
            .checked_sub(EPOCH_FIELD)
            .ok_or_else(|| full("a capacity holder's name"))?;
        let budget = capacity::Budget {
            holders,
            reservations: ran,
            name_bytes,
        };
        // A record per reservation and per holder, and the keeper's epoch.
        let most = holders
            .checked_mul(ran)
            .and_then(|n| n.checked_add(holders))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| full("capacity records"))?;
        let mut records = KeeperFiles(Files::open_with_limit(dir, most)?);
        let keeper = Keeper::boot(budget, &mut records).map_err(to_core)?;
        Ok(Self {
            keeper,
            records,
            facts: None,
            told: Amounts::ZERO,
            links: BTreeMap::new(),
        })
    }

    /// The node's facts refreshed: the room the keeper keeps, and the spec the node admits against.
    pub(super) fn refresh(&mut self, facts: &NodeSpec) -> Result<NodeSpec, Stop> {
        self.keeper.set_room(amounts(&facts.capacity));
        let admits = self.admits(facts)?;
        self.facts = Some(cloned(facts)?);
        Ok(admits)
    }

    /// The spec the node admits against now, from the facts last refreshed (none before the first).
    pub(super) fn admitted(&self) -> Result<Option<NodeSpec>, Stop> {
        self.facts
            .as_ref()
            .map(|facts| self.admits(facts))
            .transpose()
    }

    /// `facts` with every holder's reservations off its capacity. A holder holds no GPU (CV3), so the
    /// node's devices still agree with its count.
    fn admits(&self, facts: &NodeSpec) -> Result<NodeSpec, Stop> {
        let foreign = self.keeper.foreign();
        let mut spec = cloned(facts)?;
        spec.capacity = Resources {
            cpu_milli: facts.capacity.cpu_milli.saturating_sub(foreign.cpu_milli),
            mem_bytes: facts.capacity.mem_bytes.saturating_sub(foreign.mem_bytes),
            disk_bytes: facts.capacity.disk_bytes.saturating_sub(foreign.disk_bytes),
            gpus: facts.capacity.gpus,
        };
        Ok(spec)
    }

    /// A holder's word on `link`, decided beside the node's own `local`: the answer, and whether the
    /// holders' reservations changed, so the node's admission must be given its new room first.
    pub(super) fn hear(
        &mut self,
        link: u64,
        msg: ToKeeper,
        local: Amounts,
    ) -> Result<(ToHolder, bool), Stop> {
        let before = self.keeper.foreign();
        let answer = self
            .keeper
            .hear(msg, local, &mut self.records)
            .map_err(to_core)?;
        if let ToHolder::Welcome { fence, .. } = &answer {
            let keeper = &self.keeper;
            self.links.retain(|_, session| keeper.current(*session));
            self.links.insert(link, fence.session);
        }
        Ok((answer, self.keeper.foreign() != before))
    }

    /// Link `link` dropped: its holder is away and keeps what it holds (CV5).
    pub(super) fn link_down(&mut self, link: u64) {
        if let Some(session) = self.links.remove(&link) {
            self.keeper.link_down(session);
        }
    }

    /// The hints owed, by link: every synced holder is owed one when the node's own `local` moved since
    /// they were last owed it (a box admitted or ended, a booking risen), as when another holder's moved.
    pub(super) fn hints(&mut self, local: Amounts) -> Result<Vec<(u64, ToHolder)>, Stop> {
        if local != self.told {
            self.told = local;
            self.keeper.local_changed();
        }
        let mut out = Vec::new();
        for (session, hint) in self.keeper.hints(local) {
            let link = self
                .links
                .iter()
                .find(|(_, s)| **s == session)
                .map(|(link, _)| *link)
                .ok_or_else(|| Stop::Damaged("a hint owed to a session no link holds".into()))?;
            out.push((link, hint));
        }
        Ok(out)
    }

    /// The person's release of holder `name`'s reservations (CV6), and whether the holders' reservations
    /// changed.
    pub(super) fn forget(
        &mut self,
        name: &str,
    ) -> Result<(Result<Forgotten, NotForgotten>, bool), Stop> {
        let before = self.keeper.foreign();
        let forgotten = self
            .keeper
            .forget(name, &mut self.records)
            .map_err(to_core)?;
        Ok((forgotten, self.keeper.foreign() != before))
    }

    /// The view the keeper answers with the node's own `local`.
    pub(super) fn view(&self, local: Amounts) -> capacity::View {
        self.keeper.view(local)
    }
}

fn cloned(spec: &NodeSpec) -> Result<NodeSpec, Stop> {
    spec.try_clone()
        .map_err(|e| Stop::Damaged(format!("the node's facts: {e}")))
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
