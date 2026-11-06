//! The `oci` class where the host's kernel isn't Linux (docs/design/shards-guest.md §1, §5): one Shards
//! guest per node, booted on the node's first `oci` create, running Hex's own Linux node agent, unchanged
//! (CLAUDE.md §8). This runtime is a client of that node: it stages each box's root into the guest
//! ([`hex_client::guest::stage`]), rewrites the spec to the guest's paths ([`rewrite`]), and forwards every
//! call over [`hex_client::guest::Client`]. Where Shards has no backend (macOS x86_64, Windows arm64) the
//! class is refused by name before anything is booted.

pub mod rewrite;
pub mod shares;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, ClassSpec, Proc, Program, Refusal, Runtime, Signal, Stdio,
    Usage,
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

/// The slowest boot to ready on record for this host's class, the seed of the first dial's bound before
/// this node has seen a boot (bench/native/guest/boot-ready/RECORD.md). macOS (HVF): the most of every
/// run, quiet and under its hosts' own load, across both Mac Studios. Linux (KVM) and Windows (WHP) have
/// no record yet, so they have no seed: a guest there is refused by name until their records are taken,
/// never bounded by another class's.
pub fn recorded_boot() -> Result<Duration, Refusal> {
    #[cfg(target_os = "macos")]
    {
        Ok(Duration::from_millis(265))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(Refusal::Platform {
            what: "the oci class's guest",
            platform: std::env::consts::OS,
            why: "this host class's boot-to-ready record is pending \
                  (bench/native/guest/boot-ready/RECORD.md), so its first dial has no bound to seed"
                .to_owned(),
        })
    }
}

/// Boots of one guest: the first, and one more where the first hung alive past its bound, as a hung
/// boot has another guest to try before its caller is refused.
const BOOTS: usize = 2;

/// The `oci` class through a Shards guest.
pub struct GuestRuntime {
    guests: Arc<dyn shards_api::Guests>,
    config: GuestConfig,
    booted: Mutex<Option<Arc<Booted>>>,
    /// The slowest boot to ready this node has seen, the learned half of the first dial's bound.
    slowest: Mutex<Duration>,
    /// Guests stopped for hanging alive past the bound, never ready.
    retired: AtomicU64,
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
                    let b = Arc::new(Booted {
                        guest,
                        link,
                        client,
                    });
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
    fn stage(&self, b: &Booted, id: &BoxId, items: &[Item]) -> Result<(), Refusal> {
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
        if let Ok(Some(b)) = self.booted.get_mut().map(Option::take) {
            let _ = b.guest.stop();
        }
    }
}

impl Runtime for GuestRuntime {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        if let ClassSpec::Process = spec.class {
            return Err(Refusal::Invalid {
                what: "class",
                why: "the node's guest runs the oci class; process boxes are the host's".to_owned(),
            });
        }
        spec.bounds.check()?;
        // Refused as on any node, naming the field, before anything is mapped, staged or booted.
        if let ClassSpec::Oci(oci) = &spec.class {
            rewrite::check(oci)?;
        }
        let id = spec.id.clone();
        let rewritten = rewrite::rewrite(spec, &self.config.shares, &self.config.staged)?;
        let b = self.guest()?;
        self.stage(&b, &id, &rewritten.items)?;
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

/// A node's runtime where the `oci` class is its guest's: `process` boxes are the host's own runtime's
/// (`MacRuntime`, `WinRuntime`), `oci` boxes the guest's, one box id naming one box across both. This is
/// the routing by class of §1, the same for macOS and Windows (CLAUDE.md §8).
pub struct WithGuest<P> {
    process: P,
    oci: GuestRuntime,
}

impl<P: Runtime> WithGuest<P> {
    pub fn new(process: P, oci: GuestRuntime) -> Self {
        Self { process, oci }
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
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process, Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        // An id is one box's across both classes: the other side holding it refuses the create.
        let (this, other): (&dyn Runtime, &dyn Runtime) = match spec.class {
            ClassSpec::Process => (&self.process, &self.oci),
            ClassSpec::Oci(_) | ClassSpec::Bundle(_) => (&self.oci, &self.process),
        };
        match other.inspect(&spec.id) {
            Ok(_) => return Err(Refusal::Exists(spec.id)),
            Err(Refusal::NoSuchBox(_)) => {}
            Err(why) => return Err(why),
        }
        this.create(spec)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.holder(id)?.start(id)
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.holder(id)?.exec(id, program, stdio)
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
