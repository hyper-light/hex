//! The `oci` class where the host's kernel isn't Linux (docs/design/shards-guest.md §1, §5): one Shards
//! guest per node, booted on the node's first `oci` create, running Hex's own Linux node agent, unchanged
//! (CLAUDE.md §8). This runtime is a client of that node: it stages each box's root into the guest
//! ([`hex_client::guest::stage`]), rewrites the spec to the guest's paths ([`rewrite`]), and forwards every
//! call over [`hex_client::guest::Client`]. Where Shards has no backend (macOS x86_64, Windows arm64) the
//! class is refused by name before anything is booted.

pub mod rewrite;
pub mod shares;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, ClassSpec, Proc, Program, Refusal, Runtime, Signal, Stdio,
    Usage,
};
use hex_client::ANSWER_PATIENCE;
use hex_client::guest::shards::{ShardsLink, refusal};
use hex_client::guest::stage::{self, Item};
use hex_client::guest::{Client, Link};
use hex_core::stream::{self, files};

use shares::Share;

/// The guest a node boots: its kernel and initramfs (whose `/init` is Hex's static `hex`), its vCPUs and
/// memory (the node's to give, §1), and the host folders it shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestConfig {
    pub kernel: PathBuf,
    pub initrd: PathBuf,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub shares: Vec<Share>,
    /// Where the guest's relay stages each box's root: [`stage::STAGED`], the guest's tmpfs.
    pub staged: String,
}

/// The environment variables a node reads its guest from.
pub const ENV_KERNEL: &str = "HEX_GUEST_KERNEL";
pub const ENV_INITRD: &str = "HEX_GUEST_INITRD";
pub const ENV_CPUS: &str = "HEX_GUEST_CPUS";
pub const ENV_MEMORY_MIB: &str = "HEX_GUEST_MEMORY_MIB";


impl GuestConfig {
    /// The guest named by the environment: `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`, `HEX_GUEST_CPUS`
    /// and `HEX_GUEST_MEMORY_MIB`, each needed (the guest's vCPUs and memory are the node's to give
    /// from its budget, §1, so none is guessed), and `HEX_GUEST_SHARES` (else [`shares::defaults`]).
    /// Shards refuses vCPUs or memory past the host's (its `Bounds`). Shards' binary is the
    /// [`shards_api::Guests`] the node is given.
    pub fn from_env() -> Result<Self, Refusal> {
        let path = |name: &'static str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .ok_or_else(|| Refusal::Invalid {
                    what: name,
                    why: "it isn't set: the oci class's guest needs it".to_owned(),
                })
        };
        Ok(Self {
            kernel: path(ENV_KERNEL)?,
            initrd: path(ENV_INITRD)?,
            vcpus: number(ENV_CPUS)?,
            memory_mib: number(ENV_MEMORY_MIB)?,
            shares: shares::from_env()?,
            staged: stage::STAGED.to_owned(),
        })
    }

    /// The spec Shards boots: the generated initramfs's `/init` is `hex`, its command line saying to
    /// start as the guest's init and mount this many shares; the guest is reached on the channel's
    /// three ports alone.
    pub fn spec(&self) -> shards_api::GuestSpec {
        shards_api::GuestSpec {
            kernel: self.kernel.clone(),
            init: shards_api::Init::Initrd(self.initrd.clone()),
            cmdline: format!("-- guest init --shares {}", self.shares.len()),
            vcpus: self.vcpus,
            memory_mib: self.memory_mib,
            disks: Vec::new(),
            // Shards' own choice: `SHARDS_BIN`, else `shards` on `PATH` (shards-api's `GuestSpec`).
            binary: None,
            ports: vec![
                stream::CONTROL_PORT,
                stream::STDIO_PORT,
                files::FILES_PORT,
            ],
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

fn number<T: std::str::FromStr>(name: &'static str) -> Result<T, Refusal>
where
    T::Err: std::fmt::Display,
{
    let text = std::env::var(name).map_err(|e| Refusal::Invalid {
        what: name,
        why: format!("{e}: the oci class's guest needs it"),
    })?;
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

/// The `oci` class through a Shards guest.
pub struct GuestRuntime {
    guests: Arc<dyn shards_api::Guests>,
    config: GuestConfig,
    /// How long a dial waits for the guest's port: its boot, the first time.
    patience: Duration,
    booted: Mutex<Option<Arc<Booted>>>,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("the guest runtime's lock is poisoned".to_owned())
}

impl GuestRuntime {
    /// The class over `guests`, booting `config`'s guest when first asked. Each dial waits at most the
    /// client's answer patience ([`ANSWER_PATIENCE`]), the boot included.
    pub fn new(guests: Arc<dyn shards_api::Guests>, config: GuestConfig) -> Self {
        Self {
            guests,
            config,
            patience: ANSWER_PATIENCE,
            booted: Mutex::new(None),
        }
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
        let guest: Arc<dyn shards_api::Guest> = self
            .guests
            .boot(&self.config.spec())
            .map_err(|why| refusal("booting the node's guest", &why))?
            .into();
        let link: Arc<dyn Link> = Arc::new(ShardsLink::new(Arc::clone(&guest), self.patience));
        let client = match Client::connect(Arc::clone(&link)) {
            Ok(client) => client,
            Err(why) => {
                let _ = guest.stop();
                return Err(why);
            }
        };
        let b = Arc::new(Booted {
            guest,
            link,
            client,
        });
        *booted = Some(Arc::clone(&b));
        Ok(b)
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
        let b = self.running()?.ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
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
