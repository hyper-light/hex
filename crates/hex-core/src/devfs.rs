//! Fresh `/dev` mounts made ahead (goal 5 extended step 3, runtime architecture decision 5).
//! Pure ownership and refill decisions: four LRU keys, eight fresh mounts per key. A take moves a
//! mount out forever; a cold/dry take uses ordinary setup. One maker fills outside the take's lock.
//! An evicted or shutdown job is discarded, and a failed key stops asking until another take.

use std::collections::VecDeque;

/// Four distinct mount configurations retained, least recently requested evicted on overflow.
pub const KEYS: usize = 4;
/// Eight fresh mounts per configuration: the acceptance test's concurrent creates. At most 32 held.
pub const PER_KEY: usize = 8;

/// Counts saturate rather than wrap. Every completed successful make is held, taken or discarded.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub made: u64,
    pub taken: u64,
    pub on_path: u64,
    pub discarded: u64,
    pub failed: u64,
    /// Fixed miss reasons; their sum is `on_path`.
    pub cold: u64,
    pub dry: u64,
    pub disabled: u64,
    pub after_failure: u64,
    pub unpooled_missing: u64,
    pub unpooled_devices: u64,
    pub unpooled_duplicate: u64,
    pub unpooled_kind: u64,
    pub unpooled_readonly: u64,
    pub unpooled_inodes: u64,
    pub unpooled_key_bytes: u64,
    pub unpooled_capacity: u64,
    pub unpooled_user_namespace: u64,
    pub unpooled_pid_namespace: u64,
    pub unpooled_beneath: u64,
    /// Fixed preparation stages; their sum is `failed`.
    pub failed_filesystem: u64,
    pub failed_device: u64,
    pub failed_link: u64,
    pub failed_namespace: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("the prepared /dev pool is shut down")]
    ShutDown,
    #[error("the prepared /dev pool exhausted its configuration generations")]
    Generations,
}

/// The failed preparation stage, not a user-controlled label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MakeFailure {
    Filesystem,
    Device,
    Link,
    /// A prepared proc's pid namespace, which the kernel wouldn't hand over (decision 6's update, part 2).
    Namespace,
}

/// Fixed reasons ordinary construction is required before a key can be requested. These are
/// optimization misses, never new specification refusals or user-controlled metric labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unpooled {
    Missing,
    Devices,
    Duplicate,
    Kind,
    Readonly,
    Inodes,
    KeyBytes,
    Capacity,
    /// A container with a user namespace of its own, made through the shim: the node's default nodes
    /// can't serve it, so it binds the host's in (goal 5 extended, step 3).
    UserNamespace,
    /// A prepared proc (decision 6's update, part 2): the container has no pid namespace of its own for
    /// one to be made ahead for.
    PidNamespace,
    /// A prepared proc: another mount at or beneath `/proc`, or a mask or read-only path of `/proc` itself
    /// or reached through `..`, which ordinary construction orders against the proc.
    Beneath,
}

#[derive(Clone, Copy)]
enum Miss {
    Cold,
    Dry,
    Disabled,
    AfterFailure,
}

/// One make reserved by the single worker; its generation cannot insert into a replacement key.
pub struct Job<K> {
    key: K,
    generation: u64,
}

impl<K> Job<K> {
    pub fn key(&self) -> &K {
        &self.key
    }
}

struct Entry<K, T> {
    key: K,
    generation: u64,
    asking: bool,
    ready: VecDeque<T>,
}

pub struct Pool<K, T> {
    entries: VecDeque<Entry<K, T>>,
    next_generation: u64,
    working: bool,
    closed: bool,
    enabled: bool,
    counts: Counts,
}

impl<K: Clone + Eq, T> Pool<K, T> {
    pub fn new(enabled: bool) -> Self {
        Self {
            entries: VecDeque::new(),
            next_generation: 0,
            working: false,
            closed: false,
            enabled,
            counts: Counts::default(),
        }
    }

    /// Never waits for preparation. A new key evicts the least recently requested; all its unused
    /// mounts are handed back to drop, bounded by PER_KEY. A failed key resumes after this request.
    pub fn take(&mut self, key: K) -> Result<(Option<T>, Vec<T>), Refused> {
        if self.closed {
            return Err(Refused::ShutDown);
        }
        let mut discarded = Vec::new();
        let mut miss = Miss::Disabled;
        let taken = if !self.enabled {
            None
        } else {
            let entry = self
                .entries
                .iter()
                .position(|e| e.key == key)
                .and_then(|i| self.entries.remove(i));
            let mut entry = match entry {
                Some(entry) => {
                    miss = if entry.asking {
                        Miss::Dry
                    } else {
                        Miss::AfterFailure
                    };
                    entry
                }
                None => {
                    miss = Miss::Cold;
                    let generation = self
                        .next_generation
                        .checked_add(1)
                        .ok_or(Refused::Generations)?;
                    self.next_generation = generation;
                    if self.entries.len() >= KEYS
                        && let Some(old) = self.entries.pop_front()
                    {
                        discarded.extend(old.ready);
                    }
                    Entry {
                        key,
                        generation,
                        asking: true,
                        ready: VecDeque::new(),
                    }
                }
            };
            entry.asking = true;
            let taken = entry.ready.pop_front();
            self.entries.push_back(entry);
            taken
        };
        if taken.is_some() {
            self.counts.taken = self.counts.taken.saturating_add(1);
        } else {
            self.counts.on_path = self.counts.on_path.saturating_add(1);
            let counter = match miss {
                Miss::Cold => &mut self.counts.cold,
                Miss::Dry => &mut self.counts.dry,
                Miss::Disabled => &mut self.counts.disabled,
                Miss::AfterFailure => &mut self.counts.after_failure,
            };
            *counter = counter.saturating_add(1);
        }
        self.counts.discarded = self
            .counts
            .discarded
            .saturating_add(u64::try_from(discarded.len()).unwrap_or(u64::MAX));
        Ok((taken, discarded))
    }

    /// Count a specification or descriptor-capacity miss without retaining its configuration or
    /// waking the maker. The create continues through ordinary setup, with its original outcome.
    pub fn unpooled(&mut self, reason: Unpooled) -> Result<(), Refused> {
        if self.closed {
            return Err(Refused::ShutDown);
        }
        self.counts.on_path = self.counts.on_path.saturating_add(1);
        let count = match reason {
            Unpooled::Missing => &mut self.counts.unpooled_missing,
            Unpooled::Devices => &mut self.counts.unpooled_devices,
            Unpooled::Duplicate => &mut self.counts.unpooled_duplicate,
            Unpooled::Kind => &mut self.counts.unpooled_kind,
            Unpooled::Readonly => &mut self.counts.unpooled_readonly,
            Unpooled::Inodes => &mut self.counts.unpooled_inodes,
            Unpooled::KeyBytes => &mut self.counts.unpooled_key_bytes,
            Unpooled::Capacity => &mut self.counts.unpooled_capacity,
            Unpooled::UserNamespace => &mut self.counts.unpooled_user_namespace,
            Unpooled::PidNamespace => &mut self.counts.unpooled_pid_namespace,
            Unpooled::Beneath => &mut self.counts.unpooled_beneath,
        };
        *count = count.saturating_add(1);
        Ok(())
    }

    pub fn wants_work(&self) -> bool {
        !self.closed
            && !self.working
            && self
                .entries
                .iter()
                .any(|e| e.asking && e.ready.len() < PER_KEY)
    }

    pub fn next_job(&mut self) -> Option<Job<K>> {
        if !self.wants_work() {
            return None;
        }
        let e = self
            .entries
            .iter()
            .find(|e| e.asking && e.ready.len() < PER_KEY)?;
        let job = Job {
            key: e.key.clone(),
            generation: e.generation,
        };
        self.working = true;
        Some(job)
    }

    /// Completes one make; a stale/closed job returns its mount for the shell to close outside the
    /// lock. The worker's error is counted and suppresses retries for that key until the next take.
    pub fn complete(&mut self, job: Job<K>, made: Result<T, MakeFailure>) -> Option<T> {
        self.working = false;
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.generation == job.generation);
        match made {
            Ok(mount) => {
                self.counts.made = self.counts.made.saturating_add(1);
                if !self.closed
                    && let Some(e) = e
                    && e.ready.len() < PER_KEY
                {
                    e.ready.push_back(mount);
                    None
                } else {
                    self.counts.discarded = self.counts.discarded.saturating_add(1);
                    Some(mount)
                }
            }
            Err(stage) => {
                self.counts.failed = self.counts.failed.saturating_add(1);
                let counter = match stage {
                    MakeFailure::Filesystem => &mut self.counts.failed_filesystem,
                    MakeFailure::Device => &mut self.counts.failed_device,
                    MakeFailure::Link => &mut self.counts.failed_link,
                    MakeFailure::Namespace => &mut self.counts.failed_namespace,
                };
                *counter = counter.saturating_add(1);
                if let Some(e) = e {
                    e.asking = false;
                }
                None
            }
        }
    }

    /// Stops making for good, where the kernel can make none (decision 6's update, part 2: a pid namespace
    /// made ahead can't be opened below Linux 7.1). What is held is handed back to drop, counted discarded;
    /// every later take is a disabled miss and asks for nothing.
    pub fn disable(&mut self) -> Vec<T> {
        self.enabled = false;
        let held: Vec<T> = self.entries.drain(..).flat_map(|e| e.ready).collect();
        self.counts.discarded = self
            .counts
            .discarded
            .saturating_add(u64::try_from(held.len()).unwrap_or(u64::MAX));
        held
    }

    pub fn shut_down(&mut self) -> Vec<T> {
        self.closed = true;
        let held: Vec<T> = self.entries.drain(..).flat_map(|e| e.ready).collect();
        self.counts.discarded = self
            .counts
            .discarded
            .saturating_add(u64::try_from(held.len()).unwrap_or(u64::MAX));
        held
    }

    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn held(&self) -> usize {
        self.entries.iter().map(|e| e.ready.len()).sum()
    }
    pub fn counts(&self) -> Counts {
        self.counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-T7.1.4, goal 5 step 3: fill four configurations, then churn and evict while a make is
    /// outstanding. Expect bounded resources, unique ownership and exact successful-make accounting.
    #[test]
    fn fresh_mounts_are_bounded_and_never_reused_under_churn() {
        let mut p = Pool::<usize, usize>::new(true);
        let mut made = 0;
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..2000 {
            let (got, _) = p.take(i % 7).unwrap();
            if let Some(got) = got {
                assert!(seen.insert(got));
            }
            let job = p.next_job().unwrap();
            assert!(p.next_job().is_none(), "only one worker");
            if i % 3 == 0 {
                for key in 10..15 {
                    let _ = p.take(key).unwrap();
                }
            }
            made += 1;
            let _ = p.complete(job, Ok(made));
            assert!(p.entries.len() <= KEYS);
            assert!(p.held() <= KEYS * PER_KEY);
            let c = p.counts();
            assert_eq!(c.made, c.taken + c.discarded + p.held() as u64);
        }
        let _ = p.shut_down();
        assert_eq!(p.held(), 0);
        assert_eq!(p.counts().made, p.counts().taken + p.counts().discarded);
    }

    /// HEX-MNTNS-6: take two keys, make one, then disable the pool as a kernel that can make none would.
    /// Expect the held one handed back and counted discarded, no job asked for after, and each later take a
    /// disabled miss that retains no key.
    #[test]
    fn a_disabled_pool_hands_back_what_it_held_and_asks_for_nothing() {
        let mut p = Pool::<usize, usize>::new(true);
        let _ = p.take(1).unwrap();
        let _ = p.take(2).unwrap();
        let j = p.next_job().unwrap();
        assert!(p.complete(j, Ok(7)).is_none());
        assert_eq!(p.disable(), vec![7]);
        assert!(!p.wants_work());
        assert!(p.next_job().is_none());
        assert!(p.take(1).unwrap().0.is_none());
        assert!(!p.wants_work());
        let c = p.counts();
        assert_eq!((c.made, c.discarded, c.disabled, c.cold), (1, 1, 1, 2));
        assert_eq!(p.held(), 0);
    }

    /// AC-T7.1.4: make fails, key is evicted/recreated mid-make, then shutdown races completion.
    /// Expect no retry spin, no stale insertion and every unconsumed resource handed back.
    #[test]
    fn failure_eviction_and_shutdown_do_not_reuse_a_mount() {
        let mut p = Pool::<usize, usize>::new(true);
        assert!(p.take(1).unwrap().0.is_none());
        let j = p.next_job().unwrap();
        assert!(p.complete(j, Err(MakeFailure::Filesystem)).is_none());
        assert!(p.next_job().is_none());
        let _ = p.take(1).unwrap();
        let j = p.next_job().unwrap();
        for k in 2..7 {
            let _ = p.take(k).unwrap();
        }
        let _ = p.take(1).unwrap();
        assert_eq!(p.complete(j, Ok(99)), Some(99));
        let j = p.next_job().unwrap();
        assert!(p.shut_down().is_empty());
        assert_eq!(p.complete(j, Ok(100)), Some(100));
        assert_eq!(p.take(1), Err(Refused::ShutDown));
        assert!(p.next_job().is_none());
    }

    /// AC-T7.1.4: fill one configuration to capacity, then take two. Expect FIFO, eight held at
    /// most, and a refill request only after the take. A disabled pool always uses ordinary setup.
    #[test]
    fn a_full_key_stops_refill_and_takes_are_fifo() {
        let mut p = Pool::new(true);
        let _ = p.take("a").unwrap();
        for i in 0..PER_KEY {
            let j = p.next_job().unwrap();
            assert_eq!(j.key(), &"a");
            assert!(p.complete(j, Ok(i)).is_none());
        }
        assert!(p.next_job().is_none());
        assert_eq!(p.take("a").unwrap().0, Some(0));
        assert_eq!(p.take("a").unwrap().0, Some(1));
        assert!(p.wants_work());
        let mut off = Pool::<&str, usize>::new(false);
        assert!(off.take("a").unwrap().0.is_none());
        assert!(off.next_job().is_none());
    }
    /// AC-T7.1.4: fill all four configurations, touch the oldest, then add a fifth. Expect exactly
    /// the least recently requested key's eight resources evicted, and all counters conserved.
    #[test]
    fn full_pool_evicts_lru_and_counts_fixed_failure_and_miss_reasons() {
        let mut p = Pool::<usize, usize>::new(true);
        for k in 0..KEYS {
            assert!(p.take(k).unwrap().0.is_none());
            for i in 0..PER_KEY {
                let j = p.next_job().unwrap();
                assert_eq!(*j.key(), k);
                assert!(p.complete(j, Ok(k * 100 + i)).is_none());
            }
        }
        assert_eq!(p.held(), KEYS * PER_KEY);
        assert_eq!(p.take(0).unwrap().0, Some(0));
        let (empty, evicted) = p.take(99).unwrap();
        assert!(empty.is_none());
        assert_eq!(evicted, (100..100 + PER_KEY).collect::<Vec<_>>());
        while let Some(j) = p.next_job() {
            assert!(p.complete(j, Ok(999)).is_none());
        }
        assert_eq!(p.held(), KEYS * PER_KEY);
        for stage in [
            MakeFailure::Filesystem,
            MakeFailure::Device,
            MakeFailure::Link,
        ] {
            let _ = p
                .take(1000 + usize::try_from(p.counts().failed).unwrap())
                .unwrap();
            let j = p.next_job().unwrap();
            assert!(p.complete(j, Err(stage)).is_none());
        }
        let key = p.entries.back().unwrap().key;
        assert!(p.take(key).unwrap().0.is_none());
        assert!(p.take(key).unwrap().0.is_none());
        let c = p.counts();
        assert_eq!(
            (c.failed_filesystem, c.failed_device, c.failed_link),
            (1, 1, 1)
        );
        assert_eq!(
            c.failed,
            c.failed_filesystem + c.failed_device + c.failed_link
        );
        assert_eq!((c.after_failure, c.dry), (1, 1));
        assert_eq!(c.on_path, c.cold + c.dry + c.disabled + c.after_failure);
        let mut off = Pool::<usize, usize>::new(false);
        let _ = off.take(0).unwrap();
        assert_eq!(off.counts().disabled, 1);
    }

    /// AC-T7.1.4: count every fixed ineligibility reason without keeping a key or making a mount.
    /// Expect one reason per on-path create, no refill, and a typed refusal after shutdown.
    #[test]
    fn unpooled_specs_are_counted_by_reason_without_allocating_pool_entries() {
        let mut p = Pool::<usize, usize>::new(true);
        for reason in [
            Unpooled::Missing,
            Unpooled::Devices,
            Unpooled::Duplicate,
            Unpooled::Kind,
            Unpooled::Readonly,
            Unpooled::Inodes,
            Unpooled::KeyBytes,
            Unpooled::Capacity,
            Unpooled::UserNamespace,
        ] {
            p.unpooled(reason).unwrap();
        }
        let c = p.counts();
        assert_eq!(
            [
                c.unpooled_missing,
                c.unpooled_devices,
                c.unpooled_duplicate,
                c.unpooled_kind,
                c.unpooled_readonly,
                c.unpooled_inodes,
                c.unpooled_key_bytes,
                c.unpooled_capacity,
                c.unpooled_user_namespace
            ],
            [1; 9]
        );
        assert_eq!(
            c.on_path,
            c.cold
                + c.dry
                + c.disabled
                + c.after_failure
                + c.unpooled_missing
                + c.unpooled_devices
                + c.unpooled_duplicate
                + c.unpooled_kind
                + c.unpooled_readonly
                + c.unpooled_inodes
                + c.unpooled_key_bytes
                + c.unpooled_capacity
                + c.unpooled_user_namespace
        );
        assert!(p.entries.is_empty());
        assert_eq!(p.held(), 0);
        assert!(p.next_job().is_none());
        let _ = p.shut_down();
        assert_eq!(p.unpooled(Unpooled::Missing), Err(Refused::ShutDown));
        assert_eq!(p.counts(), c);
    }
}
