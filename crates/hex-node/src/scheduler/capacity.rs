//! AC-T7.4.1: runtime room constrains its classes in the existing placement index.
//! A guest's free memory and disk belong to that guest; host room cannot enlarge them.

use std::collections::BTreeMap;

use hex_api::{Capacity, Refusal};
use hex_sim::model::{Class, Classes, Resources, RuntimePool};

pub(super) mod devices;

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "runtime capacity",
        why: why.into(),
    }
}

fn amounts(value: &hex_api::CapacityResources, gpus: u32) -> Resources {
    Resources {
        cpu_milli: value.cpu_milli,
        mem_bytes: value.mem_bytes,
        disk_bytes: value.disk_bytes,
        // Exclusive availability comes from each offered device's holder; the pool counts its
        // configured physical identities, then the existing Index subtracts local reservations.
        gpus,
    }
}

fn least(a: Resources, b: Resources) -> Resources {
    Resources {
        cpu_milli: a.cpu_milli.min(b.cpu_milli),
        mem_bytes: a.mem_bytes.min(b.mem_bytes),
        disk_bytes: a.disk_bytes.min(b.disk_bytes),
        gpus: a.gpus.min(b.gpus),
    }
}

/// Coalesce aliases of the same physical pool before crediting this driver's measured usage.
/// Different samples of one pool take their componentwise minimum. Host-backed classes add no
/// narrower pool; the global host budget still applies to every placement.
pub(super) fn pools(
    reports: Vec<(Class, Option<Capacity>)>,
    owned: &BTreeMap<Class, Resources>,
    available: Classes,
) -> Result<Vec<RuntimePool>, Refusal> {
    let count = usize::try_from(Classes::ALL.count()).map_err(|e| invalid(e.to_string()))?;
    if reports.len() > count {
        return Err(invalid("more runtime reports than placement classes"));
    }
    let mut groups = BTreeMap::<String, (Classes, Resources, Resources)>::new();
    let mut seen = Classes::NONE;
    for (class, report) in reports {
        if seen.has(class) {
            return Err(invalid("duplicate placement class capacity"));
        }
        seen = seen.with(class);
        let Some(report) = report else {
            continue;
        };
        report.check()?;
        if report.id.len() > hex_core::wire::MAX_MESSAGE {
            return Err(invalid("runtime pool identity exceeds the wire byte bound"));
        }
        let gpus = u32::try_from(
            report
                .devices
                .iter()
                .filter(|device| device.id.kind == hex_api::DeviceKind::Gpu)
                .count(),
        )
        .map_err(|_| invalid("GPU count exceeds resource representation"))?;
        let total = amounts(&report.total, gpus);
        let free = amounts(&report.free, gpus);
        if !available.has(class) {
            continue;
        }
        let group = groups
            .entry(report.id)
            .or_insert((Classes::NONE, total, free));
        group.0 = group.0.with(class);
        group.1 = least(group.1, total);
        group.2 = least(group.2, free);
    }
    groups
        .into_values()
        .map(|(classes, total, free)| {
            let used = owned
                .iter()
                .filter(|(class, _)| classes.has(**class))
                .try_fold(Resources::default(), |used, (_, more)| {
                    used.checked_add(more)
                        .ok_or_else(|| invalid("measured owned runtime usage overflows"))
                })?;
            let credited = free
                .checked_add(&used)
                .ok_or_else(|| invalid("runtime free room plus measured owned usage overflows"))?;
            Ok(RuntimePool {
                classes,
                capacity: least(total, credited),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::CapacityResources;

    fn report(id: &str, total: u64, free: u64) -> Capacity {
        Capacity {
            cost: None,
            devices: Vec::new(),
            id: id.into(),
            total: CapacityResources {
                cpu_milli: 4_000,
                mem_bytes: total,
                disk_bytes: total,
            },
            free: CapacityResources {
                cpu_milli: 4_000,
                mem_bytes: free,
                disk_bytes: free,
            },
        }
    }

    /// AC-T7.4.1: aliases of one VM credit both classes' actual RSS once, in one shared pool.
    #[test]
    fn shared_runtime_room_credits_measured_usage_once() {
        let owned = BTreeMap::from([
            (
                Class::Shards,
                Resources {
                    mem_bytes: 20,
                    ..Resources::default()
                },
            ),
            (
                Class::Oci,
                Resources {
                    mem_bytes: 30,
                    ..Resources::default()
                },
            ),
        ]);
        let found = pools(
            vec![
                (Class::Shards, Some(report("one-guest", 1_024, 100))),
                (Class::Oci, Some(report("one-guest", 1_024, 99))),
            ],
            &owned,
            Classes::ALL,
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        let pool = found.first().unwrap();
        assert_eq!(
            pool.classes,
            Classes::NONE.with(Class::Oci).with(Class::Shards)
        );
        assert_eq!(pool.capacity.mem_bytes, 149);
        assert_eq!(
            pool.capacity.disk_bytes, 99,
            "memory usage cannot invent disk credit"
        );
    }

    /// AC-T7.4.1: a guest's small budget never becomes a cap on an independent host backend.
    #[test]
    fn host_backed_class_keeps_global_capacity() {
        let found = pools(
            vec![
                (Class::Oci, None),
                (Class::Shards, Some(report("guest", 632, 600))),
            ],
            &BTreeMap::new(),
            Classes::ALL,
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found.first().unwrap().classes,
            Classes::NONE.with(Class::Shards)
        );
        assert_eq!(found.first().unwrap().capacity.mem_bytes, 600);
    }

    /// AC-T7.4.1: unreadable or malformed capacity cannot be promoted to physical room.
    #[test]
    fn malformed_reports_and_duplicate_classes_are_refused() {
        for reports in [
            vec![(Class::Shards, Some(report("guest", 632, 633)))],
            vec![(Class::Shards, Some(report("", 632, 600)))],
            vec![(Class::Shards, None), (Class::Shards, None)],
        ] {
            assert!(pools(reports, &BTreeMap::new(), Classes::ALL).is_err());
        }
    }

    /// AC-T7.4.1: credits are checked and bounded by the real pool total, never cap reservations.
    #[test]
    fn measured_credit_is_capped_and_cannot_overflow() {
        let owned = BTreeMap::from([(
            Class::Shards,
            Resources {
                mem_bytes: 40,
                ..Resources::default()
            },
        )]);
        let found = pools(
            vec![(Class::Shards, Some(report("guest", 632, 600)))],
            &owned,
            Classes::ALL,
        )
        .unwrap();
        assert_eq!(found.first().unwrap().capacity.mem_bytes, 632);
        assert!(
            pools(
                vec![(Class::Shards, Some(report("guest", u64::MAX, u64::MAX)))],
                &owned,
                Classes::ALL
            )
            .is_err()
        );
    }
}
