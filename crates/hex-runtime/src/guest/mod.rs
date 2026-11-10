//! The `oci` class where the host's kernel isn't Linux (docs/design/shards-guest.md §1, §5): one Shards
//! guest per node, booted on the node's first `oci` create, running Hex's own Linux node agent, unchanged
//! (CLAUDE.md §8). This runtime is a client of that node: it stages each box's root into the guest
//! ([`hex_client::guest::stage`]), rewrites the spec to the guest's paths ([`rewrite`]), and forwards every
//! call over [`hex_client::guest::Client`]. Where Shards has no backend (macOS x86_64, Windows arm64) the
//! class is refused by name before anything is booted.

pub mod held;
pub mod rewrite;
pub mod shares;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, ClassSpec, Proc, Program, Refusal, Rootfs, Runtime, Signal,
    Stdio, Usage,
};
use hex_client::guest::shards::{ShardsLink, refusal};
use hex_client::guest::stage::{self, Item};
use hex_client::guest::{Client, Link};
use hex_core::stream::{self, files};

use shares::Share;

/// The guest a node boots: its kernel and initramfs (whose `/init` is Hex's static `hex`), its vCPUs and
/// memory (the node's to give, §1), and the host folders it shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestConfig {
    /// The `shards` binary that boots it (on macOS the signed build, run where it is installed).
    pub binary: PathBuf,
    pub kernel: PathBuf,
    pub initrd: PathBuf,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub shares: Vec<Share>,
    /// Where the guest's relay stages each box's root: [`stage::STAGED`], the guest's tmpfs.
    pub staged: String,
}

/// The environment variables a node reads its guest from.
pub const ENV_SHARDS_BIN: &str = "SHARDS_BIN";
pub const ENV_KERNEL: &str = "HEX_GUEST_KERNEL";
pub const ENV_INITRD: &str = "HEX_GUEST_INITRD";
pub const ENV_CPUS: &str = "HEX_GUEST_CPUS";
pub const ENV_MEMORY_MIB: &str = "HEX_GUEST_MEMORY_MIB";

impl GuestConfig {
    /// The guest named by the environment ([`GuestConfig::from_vars`] over the process's).
    pub fn from_env() -> Result<Self, Refusal> {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// The guest named by `var`: `SHARDS_BIN`, `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`,
    /// `HEX_GUEST_CPUS` and `HEX_GUEST_MEMORY_MIB`, each needed (Shards is never taken from `PATH`
    /// unasked, and the guest's vCPUs and memory are the node's to give from its budget, §1, so none is
    /// guessed), and `HEX_GUEST_SHARES` (else [`shares::defaults`]). Every one missing is named in one
    /// refusal, read before anything boots, so an operator fixes them in one go. Shards refuses vCPUs or
    /// memory past the host's (its `Bounds`) when it boots.
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Result<Self, Refusal> {
        let needed = [
            ENV_SHARDS_BIN,
            ENV_KERNEL,
            ENV_INITRD,
            ENV_CPUS,
            ENV_MEMORY_MIB,
        ];
        let missing: Vec<&str> = needed
            .into_iter()
            .filter(|name| var(name).is_none_or(|v| v.is_empty()))
            .collect();
        if !missing.is_empty() {
            return Err(Refusal::Invalid {
                what: "the oci class's guest",
                why: format!(
                    "{} unset: the node boots its Linux guest from them",
                    missing.join(", ")
                ),
            });
        }
        let path = |name: &str| PathBuf::from(var(name).unwrap_or_default());
        Ok(Self {
            binary: path(ENV_SHARDS_BIN),
            kernel: path(ENV_KERNEL),
            initrd: path(ENV_INITRD),
            vcpus: number(&var, ENV_CPUS)?,
            memory_mib: number(&var, ENV_MEMORY_MIB)?,
            shares: shares::from_var(var(shares::ENV))?,
            staged: stage::STAGED.to_owned(),
        })
    }

    /// The spec Shards boots: the initramfs's `/init` is `hex`, which as PID 1 is the guest's init
    /// whatever words the kernel passes it (HEX-GUEST-12); its command line the serial console, a panic
    /// resetting the guest so the host sees it end, and `hex.shares=` naming each share's mode in tag
    /// order, none when there are none (HEX-GUEST-11). The guest is reached on the channel's three
    /// ports alone.
    pub fn spec(&self) -> shards_api::GuestSpec {
        shards_api::GuestSpec {
            kernel: self.kernel.clone(),
            init: shards_api::Init::Initrd(self.initrd.clone()),
            cmdline: self.cmdline(),
            vcpus: self.vcpus,
            memory_mib: self.memory_mib,
            disks: Vec::new(),
            binary: Some(self.binary.clone()),
            ports: vec![stream::CONTROL_PORT, stream::STDIO_PORT, files::FILES_PORT],
            shares: self
                .shares
                .iter()
                .map(|s| shards_api::Share {
                    dir: s.dir.clone(),
                    read_only: s.read_only,
                })
                .collect(),
        }
    }
}

impl GuestConfig {
    fn cmdline(&self) -> String {
        let modes: Vec<&str> = self
            .shares
            .iter()
            .map(|s| if s.read_only { "ro" } else { "rw" })
            .collect();
        // `panic=-1`: a negative timeout reboots at once on a panic (kernel-parameters.txt, `panic=`;
        // kernel/panic.c `panic_timeout`), so the VM ends and the host sees it rather than a guest that
        // hangs.
        let mut cmdline = "console=ttyS0 panic=-1".to_owned();
        if !modes.is_empty() {
            cmdline.push_str(&format!(" {}={}", shares::CMDLINE, modes.join(",")));
        }
        cmdline
    }
}

fn number<T: std::str::FromStr>(
    var: &impl Fn(&str) -> Option<OsString>,
    name: &'static str,
) -> Result<T, Refusal>
where
    T::Err: std::fmt::Display,
{
    let text = var(name).unwrap_or_default();
    let text = text.to_string_lossy();
    text.parse::<T>().map_err(|e| Refusal::Invalid {
        what: name,
        why: format!("{text:?}: {e}"),
    })
}

/// The booted guest and the client of its node.
struct Booted {
    guest: Arc<dyn shards_api::Guest>,
    link: Arc<dyn Link>,
    client: Client,
}

/// The slowest recorded value of each wait that seeds a bound, by host class
/// (bench/native/guest/records.tsv). A class with no row for a measure seeds from twice the slowest row of
/// any class ([`seed_in`]); adding its own row, once its record is taken, makes that its seed, with no code
/// change.
const RECORDS: &str = include_str!("../../../../bench/native/guest/records.tsv");

/// This host's class, as the records name it: the hypervisor Shards boots its guests with here.
pub const CLASS: &str = if cfg!(target_os = "macos") {
    "macOS-HVF"
} else if cfg!(target_os = "windows") {
    "Windows-WHP"
} else {
    "Linux-KVM"
};

/// The slowest value `table` records for `measure` on host class `class`: each line `measure`, `class`,
/// microseconds and its record, tab-separated; `#` lines are comments. `None` where no row names both.
pub fn slowest_in(table: &str, measure: &str, class: &str) -> Option<Duration> {
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t'))
        .find_map(|mut row| {
            (row.next()? == measure && row.next()? == class)
                .then(|| row.next()?.parse().ok().map(Duration::from_micros))?
        })
}

/// The seed `table` gives `measure` on host class `class`: its own slowest row where it has one; else, by the
/// lead's seed rule (2026-11-07), twice the slowest row of any class, a class with no record of its own
/// starting from the slowest the others have seen with room to spare, then rising with its node's own (the
/// learned bounds take the max with what the node sees). `None` where no class records the measure.
pub fn seed_in(table: &str, measure: &str, class: &str) -> Option<Duration> {
    if let Some(own) = slowest_in(table, measure, class) {
        return Some(own);
    }
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut row = line.split('\t');
            if row.next()? != measure {
                return None;
            }
            row.next()?;
            row.next()?.parse().ok().map(Duration::from_micros)
        })
        .max()
        .map(|slowest| slowest.saturating_mul(2))
}

/// The seed for `measure` on this host's class ([`seed_in`] over records.tsv), or the refusal naming the
/// record to take where no class records it.
pub fn recorded(measure: &str) -> Result<Duration, Refusal> {
    seed_in(RECORDS, measure, CLASS).ok_or_else(|| Refusal::Platform {
        what: "the oci class's guest",
        platform: std::env::consts::OS,
        why: format!(
            "no {measure} record for any host class yet (bench/native/guest/records.tsv): record one first"
        ),
    })
}

/// The seed of the first dial's bound before this node has seen a boot: the slowest boot to ready on record
/// for this host's class, or with none, twice the slowest of any class (bench/native/guest/boot-ready/
/// RECORD.md, its rows in records.tsv).
pub fn recorded_boot() -> Result<Duration, Refusal> {
    recorded("boot_ready")
}

/// Boots of one guest: the first, and one more where the first hung alive past its bound, as a hung
/// boot has another guest to try before its caller is refused.
const BOOTS: usize = 2;

/// What tells a node's loop that bytes wait to be written on a connection it watches.
pub type Room = Arc<dyn Fn() + Send + Sync>;

/// What joins a guest's control connection, in the event form, to its node's loop: the node gives one
/// ([`GuestRuntime::join_loop`]), and every guest booted or taken back from then is joined through it.
pub trait Joins: Send + Sync {
    /// A token for a connection to come, and what tells the loop that bytes wait to be written on it.
    fn reserve(&self) -> Result<(u64, Room), Refusal>;
    /// Watches `pump` under `token` from now.
    fn join(&self, token: u64, pump: hex_client::guest::Pump) -> Result<(), Refusal>;
}

/// The `oci` class through a Shards guest.
pub struct GuestRuntime {
    guests: Arc<dyn shards_api::Guests>,
    config: GuestConfig,
    booted: Mutex<Option<Arc<Booted>>>,
    /// The slowest boot to ready this node has seen, the learned half of the first dial's bound.
    slowest: Mutex<Duration>,
    /// Guests stopped for hanging alive past the bound, never ready.
    retired: AtomicU64,
    /// The digests learned for layer tarballs not named by theirs, as they crossed (§4a).
    digests: Mutex<Digests>,
    /// The Shards binary's identity when the running guest booted it.
    booted_binary: Mutex<Option<stage::Identity>>,
    /// The warm-up of a Shards binary that changed while the guest ran ([`GuestRuntime::warm_if_changed`]).
    warm: Mutex<Warm>,
    /// Where the guest it holds is recorded ([`GuestRuntime::recorded_at`]).
    record: Option<PathBuf>,
    /// The event form's client of the guest's node: its answers are read by the node's loop, so a call
    /// through it holds no thread while the guest answers.
    evented: Mutex<Option<Arc<Client>>>,
    /// What joins each guest's evented connection to the node's loop ([`GuestRuntime::join_loop`]).
    joins: Mutex<Option<Arc<dyn Joins>>>,
}

/// A throwaway guest booted on a Shards binary that changed while the node's guest ran, so the node's next boot
/// of it is warm: an upgrade's first boot pays the host's first-run checks of its new code, 1.5-5.3 s against a
/// warm boot's 0.17 s, once per binary and for every process after it
/// (bench/native/guest/boot-ready/RECORD.md). At most one is in flight.
#[derive(Default)]
struct Warm {
    /// The binary warmed, or being warmed.
    identity: Option<stage::Identity>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The Shards binary's identity, if it can be read (a binary that can't is the next boot's to refuse).
fn binary_identity(binary: &std::path::Path) -> Option<stage::Identity> {
    std::fs::metadata(binary).ok().map(|m| stage::identity(&m))
}

/// The digests learned for layer tarballs not named by theirs (docker save's `<id>/layer.tar`), each with the
/// identity its file had when it was read whole ([`stage::Identity`]): a create names such a layer by it
/// without reading it, while its file's identity is that one; one whose identity changed is hashed again
/// as it crosses. Bounded by the layers the guest's store can hold (its last budget's capacity), one entry
/// each; past it, the least recently used goes, and that layer is hashed again when it next crosses.
#[derive(Default)]
struct Digests {
    by_path: std::collections::BTreeMap<PathBuf, (stage::Identity, files::Digest, u64)>,
    by_use: std::collections::BTreeMap<u64, PathBuf>,
    used: u64,
    bound: usize,
}

impl Digests {
    /// The digests of `paths` this remembers, for files whose identity is the one each was learned with.
    fn known(&mut self, paths: &[PathBuf]) -> rewrite::Known {
        let mut known = rewrite::Known::new();
        for path in paths {
            let Ok(meta) = std::fs::metadata(path) else {
                continue;
            };
            let Some((identity, digest, last)) = self.by_path.get_mut(path) else {
                continue;
            };
            if *identity != stage::identity(&meta) {
                continue;
            }
            self.used = self.used.saturating_add(1);
            self.by_use.remove(last);
            *last = self.used;
            self.by_use.insert(self.used, path.clone());
            known.insert(path.clone(), *digest);
        }
        known
    }

    /// Remembers what a stage learned, within the layers the guest's store can hold.
    fn learn(&mut self, staged: stage::Staged) {
        self.bound = usize::try_from(staged.capacity).unwrap_or(usize::MAX);
        for learned in staged.learned {
            self.used = self.used.saturating_add(1);
            if let Some((_, _, last)) = self.by_path.insert(
                learned.path.clone(),
                (learned.identity, learned.digest, self.used),
            ) {
                self.by_use.remove(&last);
            }
            self.by_use.insert(self.used, learned.path);
        }
        while self.by_path.len() > self.bound {
            let Some((_, path)) = self.by_use.pop_first() else {
                break;
            };
            self.by_path.remove(&path);
        }
    }
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("the guest runtime's lock is poisoned".to_owned())
}

impl GuestRuntime {
    /// The class over `guests`, booting `config`'s guest when first asked. A boot's first dial waits at
    /// most [`Self::first_dial_bound`] for the guest to listen ([`ShardsLink`]).
    pub fn new(guests: Arc<dyn shards_api::Guests>, config: GuestConfig) -> Self {
        Self {
            guests,
            config,
            booted: Mutex::new(None),
            slowest: Mutex::new(Duration::ZERO),
            retired: AtomicU64::new(0),
            digests: Mutex::new(Digests::default()),
            booted_binary: Mutex::new(None),
            warm: Mutex::new(Warm::default()),
            record: None,
            evented: Mutex::new(None),
            joins: Mutex::new(None),
        }
    }

    /// Records the guest it boots at `path` ([`held`]), and takes back the guest recorded there when the
    /// node starts ([`GuestRuntime::boot`]): a guest outlives its node's crash (its VM is a process of its
    /// own, reparented), so the next node holds it, and the boxes in it, rather than boot another beside it
    /// and leave it an orphan (native plan step 8, T-T7.1.2).
    pub fn recorded_at(mut self, path: PathBuf) -> Self {
        self.record = Some(path);
        self
    }

    /// Boots the node's guest now, so no create pays a boot, an upgrade's first least of all (HEX-GUEST-39): the
    /// node calls it when it starts, before it accepts a box ([`WithGuest::new`]). Where Shards has no backend
    /// there is no guest to boot, and the `oci` class is refused at create by name (§5).
    pub fn boot(&self) -> Result<(), Refusal> {
        if self.guests.backend().is_err() {
            return Ok(());
        }
        self.attach_recorded()?;
        self.guest().map(drop)
    }

    /// Takes back the guest a node before this one recorded, if there is one: attached, dialed and
    /// greeted as a boot's is, its boxes then the node's. One gone or ended is forgotten, and the node boots
    /// its own; one that runs and won't answer is stopped first, so none is left beside the next. A guest
    /// that can't be told gone or taken back refuses the node by type, naming why, rather than boot
    /// another beside one that may run.
    fn attach_recorded(&self) -> Result<(), Refusal> {
        let Some(path) = &self.record else {
            return Ok(());
        };
        let Some(identity) = held::read(path)? else {
            return Ok(());
        };
        let guest: Arc<dyn shards_api::Guest> =
            match self.guests.attach(&identity, &self.config.spec()) {
                Ok(guest) => guest.into(),
                Err(shards_api::Refusal::Gone { .. } | shards_api::Refusal::Ended(_)) => {
                    return held::remove(path);
                }
                Err(why) => return Err(refusal("taking back the node's guest", &why)),
            };
        let link: Arc<dyn Link> = Arc::new(ShardsLink::new(
            Arc::clone(&guest),
            self.first_dial_bound()?,
        ));
        match Client::connect(Arc::clone(&link)) {
            Ok(client) => {
                // The binary that booted it isn't known here: no warm-up is asked for it.
                let b = Arc::new(Booted {
                    guest,
                    link,
                    client,
                });
                self.join(&b)?;
                *self.lock()? = Some(b);
                Ok(())
            }
            Err(_) => {
                guest.stop().map_err(|why| {
                    refusal("stopping a guest taken back that won't answer", &why)
                })?;
                held::remove(path)
            }
        }
    }

    /// Joins the guest's control connection, in the event form, to the node's loop through `joins`, and
    /// every guest's booted or taken back after: a call through it is answered from the loop, holding no
    /// thread while the guest answers (native plan step 8 (1)). With no guest running, the next boot joins.
    pub fn join_loop(&self, joins: Arc<dyn Joins>) -> Result<(), Refusal> {
        *self.joins.lock().map_err(poisoned)? = Some(joins);
        match self.running()? {
            Some(b) => self.join(&b),
            None => Ok(()),
        }
    }

    /// Opens booted guest `b`'s evented connection and joins it to the loop, where one is given.
    fn join(&self, b: &Booted) -> Result<(), Refusal> {
        let joins = self.joins.lock().map_err(poisoned)?.clone();
        let Some(joins) = joins else {
            return Ok(());
        };
        let (token, room) = joins.reserve()?;
        let (client, pump) = Client::connect_evented(Arc::clone(&b.link), room)?;
        *self.evented.lock().map_err(poisoned)? = Some(Arc::new(client));
        joins.join(token, pump)
    }

    /// The event form's client of the running guest, joined to the loop: one whose connection ended while
    /// its guest runs is joined again first. `None` with no guest running.
    pub fn evented(&self) -> Result<Option<Arc<Client>>, Refusal> {
        let Some(b) = self.running()? else {
            return Ok(None);
        };
        let held = self.evented.lock().map_err(poisoned)?.clone();
        if let Some(client) = held.filter(|c| !c.closed()) {
            return Ok(Some(client));
        }
        self.join(&b)?;
        Ok(self.evented.lock().map_err(poisoned)?.clone())
    }

    /// A program's own evented control connection, held until its start/end or cancellation.
    /// Windows' caller-owned sockets cannot be lent through another process. Keeping their offered exec on
    /// its own control connection makes cancellation close every early stream without affecting other programs.
    pub fn program_client(&self) -> Result<Option<Arc<Client>>, Refusal> {
        let Some(b) = self.running()? else {
            return Ok(None);
        };
        let joins = self
            .joins
            .lock()
            .map_err(poisoned)?
            .clone()
            .ok_or_else(|| {
                Refusal::Broken("the guest program has no node event loop".to_owned())
            })?;
        let (token, room) = joins.reserve()?;
        let (client, pump) = Client::connect_evented(Arc::clone(&b.link), room)?;
        joins.join(token, pump)?;
        Ok(Some(Arc::new(client)))
    }

    /// Records `guest` as the one this node holds, where a record is kept: one whose identity Shards can't
    /// give leaves no record, so a node after a crash finds none to take back.
    fn record(&self, guest: &dyn shards_api::Guest) -> Result<(), Refusal> {
        let Some(path) = &self.record else {
            return Ok(());
        };
        match guest.identity() {
            Ok(identity) => held::write(path, &identity),
            Err(_) => held::remove(path),
        }
    }

    /// Where the Shards binary changed since the running guest booted it, boots one throwaway guest on it in the
    /// background, at Shards' least memory and with no shares, and stops it, so the node's next boot is warm
    /// (HEX-GUEST-40). The running guest is never booted again for it: that would end its boxes. At most one
    /// warm-up is in flight, and a binary is warmed once. Called on each create: a stat, no polling.
    fn warm_if_changed(&self) -> Result<(), Refusal> {
        let Some(now) = binary_identity(&self.config.binary) else {
            return Ok(());
        };
        let booted = *self.booted_binary.lock().map_err(poisoned)?;
        if booted.is_none_or(|b| b == now) {
            return Ok(());
        }
        let mut warm = self.warm.lock().map_err(poisoned)?;
        if warm.identity == Some(now) || warm.thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return Ok(());
        }
        let mut least = self.config.clone();
        least.vcpus = 1;
        least.memory_mib = shards_api::min_memory_mib(std::env::consts::ARCH);
        least.shares.clear();
        let spec = least.spec();
        let guests = Arc::clone(&self.guests);
        let bound = self.first_dial_bound()?;
        let thread = std::thread::Builder::new()
            .name("hex-guest-warm".to_owned())
            .spawn(move || {
                // Ready, so its VM process ran too; then gone. A warm-up that fails leaves the next boot to
                // pay the first run, as it would have.
                if let Ok(guest) = guests.boot(&spec) {
                    let _ = guest.connect(stream::CONTROL_PORT, bound);
                    let _ = guest.stop();
                }
            })
            .map_err(|e| Refusal::os("starting a guest warm-up", e))?;
        warm.identity = Some(now);
        warm.thread = Some(thread);
        Ok(())
    }

    /// Waits for a warm-up in flight to end: its boot and dial are bounded by the first dial's bound.
    fn finish_warm(&self) {
        let thread = self.warm.lock().ok().and_then(|mut w| w.thread.take());
        if let Some(t) = thread {
            let _ = t.join();
        }
    }

    /// How long a boot may take to be ready (the lead's rule for learned waits): twice (the margin) the slowest of
    /// the record's and this node's own, so it only rises; before any boot here, twice the record's.
    pub fn first_dial_bound(&self) -> Result<Duration, Refusal> {
        let seen = *self.slowest.lock().map_err(poisoned)?;
        Ok(seen.max(recorded_boot()?).saturating_mul(2))
    }

    /// A boot to ready this node has seen, taking `took`: the bound rises with it, never falls.
    pub fn learn(&self, took: Duration) {
        if let Ok(mut slowest) = self.slowest.lock() {
            *slowest = (*slowest).max(took);
        }
    }

    /// Guests stopped because they stayed alive past the bound without being ready.
    pub fn retired(&self) -> u64 {
        self.retired.load(Ordering::Relaxed)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<Arc<Booted>>>, Refusal> {
        self.booted.lock().map_err(poisoned)
    }

    /// The guest, booted now if it isn't, or booted again if it ended (every box in it ended with it).
    fn guest(&self) -> Result<Arc<Booted>, Refusal> {
        let mut booted = self.lock()?;
        if let Some(b) = booted.as_ref() {
            match b.guest.wait_for(Duration::ZERO) {
                Ok(None) => return Ok(Arc::clone(b)),
                Ok(Some(_)) | Err(_) => *booted = None,
            }
        }
        // Refused by name before anything boots where Shards has no backend (§5).
        self.guests
            .backend()
            .map_err(|why| refusal("finding Shards' backend", &why))?;
        let binary = binary_identity(&self.config.binary);
        let mut hung = Vec::with_capacity(BOOTS);
        for _ in 0..BOOTS {
            let bound = self.first_dial_bound()?;
            let began = Instant::now();
            let guest: Arc<dyn shards_api::Guest> = self
                .guests
                .boot(&self.config.spec())
                .map_err(|why| refusal("booting the node's guest", &why))?
                .into();
            let patience = bound.saturating_sub(began.elapsed());
            let link: Arc<dyn Link> = Arc::new(ShardsLink::new(Arc::clone(&guest), patience));
            match Client::connect(Arc::clone(&link)) {
                Ok(client) => {
                    self.learn(began.elapsed());
                    if let Err(why) = self.record(guest.as_ref()) {
                        // Unrecorded, a crash would leave it an orphan: ended, and the boot refused.
                        let _ = guest.stop();
                        return Err(why);
                    }
                    *self.booted_binary.lock().map_err(poisoned)? = binary;
                    let b = Arc::new(Booted {
                        guest,
                        link,
                        client,
                    });
                    // The new guest's control connection joins the loop as the one before's did.
                    self.join(&b)?;
                    *booted = Some(Arc::clone(&b));
                    return Ok(b);
                }
                Err(why) => {
                    // Alive and not ready within the bound: a hang, retired for another boot. Ended,
                    // or refused otherwise, the boot's failure is its caller's.
                    let alive = matches!(guest.wait_for(Duration::ZERO), Ok(None));
                    let _ = guest.stop();
                    if !alive {
                        return Err(why);
                    }
                    self.retired.fetch_add(1, Ordering::Relaxed);
                    hung.push(format!("not ready within {bound:?}: {why}"));
                }
            }
        }
        Err(Refusal::Os {
            op: "booting the node's guest",
            why: format!(
                "{BOOTS} guests booted, each alive and not ready within twice the slowest boot to \
                 ready seen (bench/native/guest/boot-ready/RECORD.md): {}",
                hung.join("; ")
            ),
        })
    }

    /// The guest if it is booted: a call about a box needs no boot, as there's no box before one.
    fn running(&self) -> Result<Option<Arc<Booted>>, Refusal> {
        Ok(self.lock()?.clone())
    }

    fn with<T>(
        &self,
        id: &BoxId,
        call: impl FnOnce(&Client) -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        match self.running()? {
            Some(b) => call(&b.client),
            None => Err(Refusal::NoSuchBox(id.clone())),
        }
    }

    /// Stages `items` for `id`. A copy left by a box the guest's node no longer holds (a host that
    /// ended between a stage and its create) is removed and staged again; one of a box it holds is
    /// refused as [`Refusal::Exists`].
    fn stage(&self, b: &Booted, id: &BoxId, items: &[Item]) -> Result<stage::Staged, Refusal> {
        match stage::stage(b.link.as_ref(), id, items, &files::LIMITS) {
            Err(Refusal::Exists(_)) => match b.client.inspect(id) {
                Err(Refusal::NoSuchBox(_)) => {
                    stage::unstage(b.link.as_ref(), id)?;
                    stage::stage(b.link.as_ref(), id, items, &files::LIMITS)
                }
                Ok(_) => Err(Refusal::Exists(id.clone())),
                Err(why) => Err(why),
            },
            other => other,
        }
    }
}

impl Drop for GuestRuntime {
    fn drop(&mut self) {
        self.finish_warm();
        if let Ok(Some(b)) = self.booted.get_mut().map(Option::take) {
            let _ = b.guest.stop();
            if let Some(path) = &self.record {
                let _ = held::remove(path);
            }
        }
    }
}

/// AC-T7.4.1: configured vCPU/memory are ceilings, never host-room substitutes. Disk remains the
/// actual guest's backing writable filesystem; PID plus start fences a replacement guest's pool.
fn guest_capacity(
    mut capacity: hex_api::Capacity,
    config: &GuestConfig,
    identity: &shards_api::Identity,
) -> Result<hex_api::Capacity, Refusal> {
    capacity.check()?;
    let cpu = u64::from(config.vcpus)
        .checked_mul(1000)
        .ok_or_else(|| Refusal::Broken("guest CPU milli overflow".to_owned()))?;
    let memory = config
        .memory_mib
        .checked_mul(1 << 20)
        .ok_or_else(|| Refusal::Broken("guest memory bytes overflow".to_owned()))?;
    capacity.id = format!("guest:{}:{}", identity.pid, identity.started);
    capacity.total.cpu_milli = capacity.total.cpu_milli.min(cpu);
    capacity.total.mem_bytes = capacity.total.mem_bytes.min(memory);
    capacity.free.cpu_milli = capacity.free.cpu_milli.min(capacity.total.cpu_milli);
    capacity.free.mem_bytes = capacity.free.mem_bytes.min(capacity.total.mem_bytes);
    capacity.check()?;
    Ok(capacity)
}

impl Runtime for GuestRuntime {
    fn capacity(&self, class: Class) -> Result<Option<hex_api::Capacity>, Refusal> {
        if class != Class::Oci {
            return Err(Refusal::Invalid {
                what: "capacity class",
                why: "the guest runs only oci".to_owned(),
            });
        }
        let booted = self.guest()?;
        let capacity = booted.client.capacity(class)?.ok_or_else(|| {
            Refusal::Broken("the guest node reported no resource pool".to_owned())
        })?;
        let identity = booted
            .guest
            .identity()
            .map_err(|why| refusal("reading the guest pool identity", &why))?;
        guest_capacity(capacity, &self.config, &identity).map(Some)
    }

    fn classes(&self) -> Vec<Class> {
        vec![Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.check_no_devices("shards-guest")?;
        if let ClassSpec::Process = spec.class {
            return Err(Refusal::Invalid {
                what: "class",
                why: "the node's guest runs the oci class; process boxes are the host's".to_owned(),
            });
        }
        // Refused as on any node, naming the field, before anything is mapped, staged or booted.
        if let ClassSpec::Oci(oci) = &spec.class {
            rewrite::check(oci)?;
        }
        let id = spec.id.clone();
        let known = match &spec.class {
            ClassSpec::Oci(oci) => match &oci.rootfs {
                Rootfs::Layers(paths) => self.digests.lock().map_err(poisoned)?.known(paths),
                Rootfs::Dir(_) => rewrite::Known::new(),
            },
            _ => rewrite::Known::new(),
        };
        let rewritten = rewrite::rewrite(spec, &self.config.shares, &self.config.staged, &known)?;
        let b = self.guest()?;
        self.warm_if_changed()?;
        // An id the guest's node already holds is refused as the node refuses it, before anything is
        // staged: staging first would send a box's whole root only to be refused (CLAUDE.md §3).
        match b.client.inspect(&id) {
            Ok(_) => return Err(Refusal::Exists(id)),
            Err(Refusal::NoSuchBox(_)) => {}
            Err(why) => return Err(why),
        }
        let staged = self.stage(&b, &id, &rewritten.items)?;
        self.digests.lock().map_err(poisoned)?.learn(staged);
        b.client.create(rewritten.spec).inspect_err(|_| {
            // The copy goes with the create that failed; a removal that fails leaves it for the
            // next stage of this id, which removes it (above).
            let _ = stage::unstage(b.link.as_ref(), &id);
        })
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.with(id, |c| c.start(id))
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.with(id, |c| c.exec(id, program, stdio))
    }

    /// The guest runs the `oci` class, whose spec's run is refused ([`BoxSpec::check_run`]); a run's `process` box
    /// is the host's, refused here as its create is.
    fn run(
        &self,
        spec: BoxSpec,
        _program: Program,
        _stdio: Stdio,
    ) -> Result<Box<dyn Proc>, Refusal> {
        spec.check_no_devices("shards-guest")?;
        spec.check_run()?;
        Err(Refusal::Invalid {
            what: "class",
            why: "the node's guest runs the oci class; process boxes are the host's".to_owned(),
        })
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.with(id, |c| c.signal(id, signal))
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.with(id, |c| c.inspect(id))
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.with(id, |c| c.usage(id))
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        match self.running()? {
            Some(b) => b.client.list(),
            None => Ok(Vec::new()),
        }
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        let b = self
            .running()?
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        b.client.delete(id)?;
        match stage::unstage(b.link.as_ref(), id) {
            // A box staged nothing only if its spec named no root, which the class refuses; either
            // way there is nothing left to remove.
            Ok(()) | Err(Refusal::NoSuchBox(_)) => Ok(()),
            Err(why) => Err(why),
        }
    }
}

/// Which of a [`WithGuest`]'s runtimes takes a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The host's own runtime.
    Host,
    /// The guest's.
    Guest,
}

/// The routing by class of §1: `process` boxes the host's, every `oci` box the guest's, for a host that
/// runs no `oci` class of its own (macOS).
pub fn by_class<P>(_host: &P, class: &ClassSpec) -> Result<Side, Refusal> {
    Ok(match class {
        ClassSpec::Process => Side::Host,
        ClassSpec::Oci(_) | ClassSpec::Bundle(_) => Side::Guest,
    })
}

/// A node's runtime where Linux images are its guest's: the host's own runtime (`MacRuntime`,
/// `WinRuntime`) takes what its `route` says it runs, the guest the rest, one box id naming one box across
/// both (CLAUDE.md §8). macOS routes by class ([`by_class`]); Windows runs its own images through HCS and
/// sends only Linux images to the guest (`WinRuntime::oci_side`).
pub struct WithGuest<P> {
    process: P,
    oci: GuestRuntime,
    route: fn(&P, &ClassSpec) -> Result<Side, Refusal>,
}

impl<P: Runtime> WithGuest<P> {
    /// The host's own runtime, which runs the `process` class: a node's recovery takes its boxes back
    /// through it (native plan step 8).
    pub fn host(&self) -> &P {
        &self.process
    }

    /// The guest's runtime, which runs the `oci` class: a node's recovery asks it which boxes it still holds.
    pub fn guest(&self) -> &GuestRuntime {
        &self.oci
    }

    /// The node's runtime, routed by class ([`by_class`]), its guest booted before it returns, so it accepts
    /// no box before that boot ([`GuestRuntime::boot`]); a guest that can't boot refuses the node by type.
    pub fn new(process: P, oci: GuestRuntime) -> Result<Self, Refusal> {
        Self::routing(process, oci, by_class)
    }

    /// Routed by `route`, which a box it refuses is refused by; its guest booted before it returns, as
    /// [`WithGuest::new`]'s is.
    pub fn routing(
        process: P,
        oci: GuestRuntime,
        route: fn(&P, &ClassSpec) -> Result<Side, Refusal>,
    ) -> Result<Self, Refusal> {
        oci.boot()?;
        Ok(Self {
            process,
            oci,
            route,
        })
    }

    /// The runtime that makes a box of `spec`, as its `route` says. An id is one box's across both classes: the other
    /// side holding it refuses the box as [`Refusal::Exists`].
    fn maker(&self, spec: &BoxSpec) -> Result<&dyn Runtime, Refusal> {
        let (this, other): (&dyn Runtime, &dyn Runtime) =
            match (self.route)(&self.process, &spec.class)? {
                Side::Host => (&self.process, &self.oci),
                Side::Guest => (&self.oci, &self.process),
            };
        match other.inspect(&spec.id) {
            Ok(_) => Err(Refusal::Exists(spec.id.clone())),
            Err(Refusal::NoSuchBox(_)) => Ok(this),
            Err(why) => Err(why),
        }
    }

    /// The runtime holding `id`, or [`Refusal::NoSuchBox`]: the host's, asked first as it answers
    /// without a guest, then the guest's.
    fn holder(&self, id: &BoxId) -> Result<&dyn Runtime, Refusal> {
        match self.process.inspect(id) {
            Ok(_) => Ok(&self.process),
            Err(Refusal::NoSuchBox(_)) => match self.oci.inspect(id) {
                Ok(_) => Ok(&self.oci),
                Err(why) => Err(why),
            },
            Err(why) => Err(why),
        }
    }
}

impl<P: Runtime> Runtime for WithGuest<P> {
    fn capacity(&self, class: Class) -> Result<Option<hex_api::Capacity>, Refusal> {
        match class {
            Class::Process => self.process.capacity(class),
            Class::Oci => self.oci.capacity(class),
        }
    }

    fn classes(&self) -> Vec<Class> {
        vec![Class::Process, Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.check()?;
        self.maker(&spec)?.create(spec)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.holder(id)?.start(id)
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.holder(id)?.exec(id, program, stdio)
    }

    /// A run's box is a `process` box ([`BoxSpec::check_run`]), made where a create of its spec would be.
    fn run(&self, spec: BoxSpec, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        spec.check()?;
        spec.check_run()?;
        self.maker(&spec)?.run(spec, program, stdio)
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.holder(id)?.signal(id, signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.holder(id)?.inspect(id)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.holder(id)?.usage(id)
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        let mut all = self.process.list()?;
        all.extend(self.oci.list()?);
        Ok(all)
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.holder(id)?.delete(id)
    }
}

#[cfg(test)]
mod tests;
