//! Joint plan step 14 (AC-T7.4.1; `docs/design/learned-charge.md`): what the scheduler takes of its node's charge
//! learner. The learner lives in the node agent (`crate::node::charges`), the one authority, as the owner of its
//! microVMs' cgroups, and its capacity answers carry what it learned. Each refresh the scheduler gives its node's
//! facts the priors those answers carry (C2), which the placement index books a new VM's receipt at. The node's
//! capacity stays the machine's (C8).

use hex_api::{CapacityResources, Learned, Refusal};
use hex_sim::model::{Class, NodeSpec, Prior};

/// What each VM class's capacity answer learned, taken before the answers are applied.
pub(super) fn learned(
    reports: &[(Class, Option<hex_api::Capacity>)],
) -> Result<Vec<(Class, Learned)>, Refusal> {
    let mut out = Vec::new();
    out.try_reserve_exact(reports.len())
        .map_err(|_| Refusal::Exhausted {
            what: "learned capacity answers",
            bound: reports.len(),
        })?;
    for (class, capacity) in reports {
        if let Some(learned) = capacity.as_ref().and_then(|c| c.learned.as_ref()) {
            out.push((*class, learned.try_clone()?));
        }
    }
    Ok(out)
}

/// Gives `local` the priors `learned` carries (C2): each VM class's prior for the first of its cost profiles (its own,
/// then its routes') whose VMs have warmed, as the answer for that profile and its ceiling says it.
pub(super) fn apply(local: &mut NodeSpec, learned: &[(Class, Learned)]) -> Result<(), Refusal> {
    let mut profiles: Vec<(Class, hex_api::RuntimeCost)> = Vec::new();
    for class in [Class::Shards, Class::Firecracker] {
        if let Some(cost) = local.costs.get(class.index()).copied().flatten() {
            profiles.push((class, cost));
        }
    }
    if let Some(routes) = &local.routes {
        for offer in &routes.offers {
            let class = match offer.key.class {
                hex_api::Class::Shards => Class::Shards,
                hex_api::Class::Firecracker => Class::Firecracker,
                hex_api::Class::Process | hex_api::Class::Oci => continue,
            };
            if let Some(cost) = offer.cost {
                profiles.push((class, cost));
            }
        }
    }
    local.priors = Default::default();
    for (class, cost) in profiles {
        let ceiling = cost.allocate(CapacityResources::default())?.resources;
        let prior = learned
            .iter()
            .filter(|(of, _)| *of == class)
            .flat_map(|(_, learned)| &learned.priors)
            .find(|prior| prior.profile == cost.id && prior.ceiling == ceiling);
        let slot = local
            .priors
            .get_mut(class.index())
            .ok_or_else(|| Refusal::Broken("a runtime class past the classes".into()))?;
        if let (Some(prior), None) = (prior, slot.as_ref()) {
            *slot = Some(Prior {
                profile: cost.id,
                booking: prior.booking,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
