from pathlib import Path
p=Path('crates/hex-core/src/wire.rs');s=p.read_text();s=s.replace('    Bounds, BoxId, BoxInfo,','    Allocation, Bounds, BoxId, BoxInfo,').replace('    OciSpec, Program, Refusal, Rootfs, Signal, Usage, User,','    OciSpec, Program, Refusal, Rootfs, RuntimeCost, Signal, Usage, User,')
s=s.replace('''    #[error("it carries more descriptors than the {MAX_FDS} a message takes")]
    DescriptorsCut,
''','''    #[error("it carries more descriptors than the {MAX_FDS} a message takes")]
    DescriptorsCut,
    #[error("{what} storage for {bound} bytes is unavailable")]
    Storage { what: &'static str, bound: usize },
''')
old='''        let what = match fault {
            Fault::Wire { .. } => "wire",
            _ => "message",
        };
        Refusal::Invalid {
            what,
            why: fault.to_string(),
        }''';new='''        match fault {
            Fault::Storage { what, bound } => Refusal::Exhausted { what, bound },
            fault => {
                let what = match &fault {
                    Fault::Wire { .. } => "wire",
                    _ => "message",
                };
                Refusal::Invalid { what, why: fault.to_string() }
            }
        }''';assert s.count(old)==1;s=s.replace(old,new)
old='''                    w.option_with(device.holder.as_ref(), |w, id| w.string(id.as_str()))?;
                }
                Ok(())''';new='''                    w.option_with(device.holder.as_ref(), |w, id| w.string(id.as_str()))?;
                }
                put_cost_option(w, capacity.cost)
''';assert s.count(old)==1;s=s.replace(old,new)
old='''            let mut resources = || -> Result<CapacityResources, Fault> {
                Ok(CapacityResources {
                    cpu_milli: r.u64()?,
                    mem_bytes: r.u64()?,
                    disk_bytes: r.u64()?,
                })
            };
            let capacity = Capacity {
                cost: None,
                id,
                total: resources()?,
                free: resources()?,''';new='''            let capacity = Capacity {
                id,
                total: resources(r)?,
                free: resources(r)?,''';assert s.count(old)==1;s=s.replace(old,new)
old='''                        holder,
                    })
                })?,
            };
            capacity''';new='''                        holder,
                    })
                })?,
                cost: r.option(runtime_cost)?,
            };
            capacity''';assert s.count(old)==1;s=s.replace(old,new)
old='''fn put_box_spec(w: &mut Writer, spec: &BoxSpec) -> Result<(), Fault> {
    spec.check_devices()''';new='''fn put_box_spec(w: &mut Writer, spec: &BoxSpec) -> Result<(), Fault> {
    if let Some(allocation) = spec.allocation {
        allocation.covers(workload_caps(spec)).map_err(|e| Fault::value("allocation", e.to_string()))?;
    }
    spec.check_devices()''';assert s.count(old)==1;s=s.replace(old,new)
old='''    for id in &spec.devices {
        put_device_id(w, id);
    }
    Ok(())
}

fn box_spec''';new='''    for id in &spec.devices {
        put_device_id(w, id);
    }
    w.reserve_fixed(1 + if spec.allocation.is_some() { ALLOCATION_BYTES } else { 0 })?;
    w.option_with(spec.allocation, |w, allocation| {
        put_resources(w, allocation.resources);
        put_cost(w, allocation.cost)
    })
}

fn box_spec''';assert s.count(old)==1;s=s.replace(old,new)
old='''    let spec = BoxSpec {
        allocation: None,
        id,
        bounds,
        class,
        owner,
        devices: r.device_list(9, device_id)?,
    };
    spec.check_devices()
        .map_err(|e| Fault::value("devices", e.to_string()))?;
    Ok(spec)
}''';new='''    let spec = BoxSpec {
        id,
        bounds,
        class,
        owner,
        devices: r.device_list(9, device_id)?,
        allocation: r.option(|r| Ok(Allocation { resources: resources(r)?, cost: runtime_cost(r)? }))?,
    };
    spec.check_devices()
        .map_err(|e| Fault::value("devices", e.to_string()))?;
    if let Some(allocation) = spec.allocation {
        allocation.covers(workload_caps(&spec)).map_err(|e| Fault::value("allocation", e.to_string()))?;
    }
    Ok(spec)
}''';assert s.count(old)==1;s=s.replace(old,new)
needle='// Fixed tags independent of Rust discriminants; the catalog order is retained because scheduler'
helper='''// AC-T7.4.1: fixed native integer widths; new optional fields follow the complete old device tail.
const RESOURCE_BYTES: usize = 3 * size_of::<u64>();
const COST_BYTES: usize = 32 + 5 * RESOURCE_BYTES;
const ALLOCATION_BYTES: usize = RESOURCE_BYTES + COST_BYTES;

fn workload_caps(spec: &BoxSpec) -> CapacityResources {
    CapacityResources {
        cpu_milli: spec.bounds.cpu_max_milli.map_or(0, u64::from),
        mem_bytes: spec.bounds.memory_max.unwrap_or(0),
        disk_bytes: 0,
    }
}

fn put_resources(w: &mut Writer, resources: CapacityResources) {
    w.u64(resources.cpu_milli);
    w.u64(resources.mem_bytes);
    w.u64(resources.disk_bytes);
}

fn resources(r: &mut Reader<'_>) -> Result<CapacityResources, Fault> {
    Ok(CapacityResources { cpu_milli: r.u64()?, mem_bytes: r.u64()?, disk_bytes: r.u64()? })
}

fn put_cost_option(w: &mut Writer, cost: Option<RuntimeCost>) -> Result<(), Fault> {
    w.reserve_fixed(1 + if cost.is_some() { COST_BYTES } else { 0 })?;
    w.option_with(cost, put_cost)
}

fn put_cost(w: &mut Writer, cost: RuntimeCost) -> Result<(), Fault> {
    cost.check().map_err(|e| Fault::value("runtime cost", e.to_string()))?;
    w.reserve_fixed(COST_BYTES)?;
    w.raw(&cost.id);
    for resources in [cost.floor, cost.ceiling, cost.quantum, cost.guest_overhead, cost.overhead] {
        put_resources(w, resources);
    }
    Ok(())
}

fn runtime_cost(r: &mut Reader<'_>) -> Result<RuntimeCost, Fault> {
    let cost = RuntimeCost {
        id: r.array::<32>()?, floor: resources(r)?, ceiling: resources(r)?,
        quantum: resources(r)?, guest_overhead: resources(r)?, overhead: resources(r)?,
    };
    cost.check().map_err(|e| Fault::value("runtime cost", e.to_string()))?;
    Ok(cost)
}

''';assert s.count(needle)==1;s=s.replace(needle,helper+needle)
needle='''    /// Bytes as they are, with no length before them: another message's header.
    pub(crate) fn raw''';reserve='''    /// The new fixed-width cost/charge tail is checked and reserved before writing any of it.
    fn reserve_fixed(&mut self, bytes: usize) -> Result<(), Fault> {
        let after = self.out.len().checked_add(bytes);
        if after.is_none_or(|after| after > self.bound) {
            return Err(Fault::Oversized { bytes: after.unwrap_or(usize::MAX), bound: self.bound });
        }
        self.out.try_reserve_exact(bytes).map_err(|_| Fault::Storage {
            what: "wire allocation receipt", bound: bytes,
        })
    }

'''+needle;assert s.count(needle)==1;s=s.replace(needle,reserve)
p.write_text(s)
p=Path('crates/hex-runtime/src/guest/rewrite.rs');s=p.read_text();s=s.replace('''        owner,
        devices,
    } = spec;''','''        owner,
        devices,
        allocation,
    } = spec;''').replace('''            allocation: None,
            id,''','''            allocation,
            id,''');p.write_text(s)
p=Path('crates/hex-core/src/wire/tests.rs');s=p.read_text();old='''    bytes.truncate(bytes.len() - 4);
    bytes.extend_from_slice(tail);
    bytes
}''';new='''    // The empty old device count and new optional cost/allocation tag are both replaced;
    // malformed device input still carries a valid absent receipt after its device tail.
    bytes.truncate(bytes.len() - 5);
    bytes.extend_from_slice(tail);
    bytes.push(0);
    bytes
}''';assert s.count(old)==1;s=s.replace(old,new);p.write_text(s)
