//! A node VM the node boots for itself through the image class (joint plan step 39;
//! docs/design/node-under-shards-init.md §4 to §6): Hex's own init under shards-init, from Hex's node image, which its
//! node reaches through its run's agent-control endpoint alone (§5). It is an image VM, so its boot, its stop and its
//! use of its host go through the class's own runs (ROOT, 2026-11-13: one code path, not a third); and it is never a
//! box: its record is a node VM's, named by its run, which no box call reads.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hex_api::{Allocation, Exit, HostUse, NodeRun, NodeVmPhase, Refusal};
use hex_core::patience::Patience;
use hex_core::record::{NodeVmRecord, RECORD_VERSION};
use shards_api::image::{
    BootStop, ControlAsk, ControlAt, ImageGuest, ImageRefusal, ImageShare, ImageSpec, Keep,
    Narrowing, Ready, SocketGrant, StatsScope,
};

use super::{
    End, ImageClass, Run, Settled, Slot, Told, admitted_size, boot_on_its_thread, diagnostics_file,
    host_use_of, timed_stats,
};
use crate::devices::Books;

/// A node VM as its node boots it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVm {
    /// Its run, minted by the node's store as the VM is booked in its ledger.
    pub run: NodeRun,
    /// The node image, pinned by digest (`HEX_GUEST_IMAGE`): Hex's own init at `/init`, declaring `NESTING`.
    pub image: String,
    /// Its admitted allocation: its receipt in the node's ledger, and the size Shards boots.
    pub allocation: Allocation,
    /// The host directories it shares, each where Hex's init finds it (`/run/hex/shares/<i>`).
    pub shares: Vec<ImageShare>,
    /// The SHA-256 of its node's own key: the one key its run's endpoint takes (D252).
    pub controller: [u8; 32],
    /// The most lanes its run admits at once to the relay's sockets: what its node fitted (hex-node's `serve::fit`).
    pub lanes: u32,
}

/// A node VM whose init has said it is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeBooted {
    pub run: NodeRun,
    /// Its run's agent-control endpoint, and its microVM's key, which its node pins (D252).
    pub control: ControlAt,
    /// How its init said it.
    pub ready: Ready,
    /// From the boot's ask to its READY.
    pub took: Duration,
}

/// `guest.hosted.ready`, W*: the slowest READY of a node VM this process has seen, none before the first (decision 9).
static READY: Patience = Patience::measuring("guest.hosted.ready");

const BOOTING: &str = "booting the node VM under shards-init";
const WAITING: &str = "waiting for the node VM under shards-init to say it is ready";

impl ImageClass {
    /// Boots node VM `vm` and follows it to its init's READY.
    /// - **Its record** is written first, Booting, durable before any effect; Ready once its init says it is, and
    ///   Ended with its end where its boot fails.
    /// - **The boot** runs on a thread of its own, as a box's does, and is followed to its answer: within the boot's
    ///   W*, its answer; past it, the run-out told and the boot followed by its thread's life (decision 9), never
    ///   Pending, as the node waits for its own VM.
    /// - **READY** is followed as [`follow_ready`] says, its answer the sample W* rises to.
    /// - **A boot that fails to READY** has its run stopped, and is refused, typed by what failed.
    pub fn boot_node(&self, vm: &NodeVm) -> Result<NodeBooted, Refusal> {
        let image = self.image()?;
        let images = Arc::clone(image.images()?);
        let waits = image.waits()?;
        let books = self.authority()?;
        let spec = self.node_spec(vm)?;
        let diagnostics = image
            .diagnostics
            .join(format!("node-{}.log", run_name(&vm.run)));
        let mut record = NodeVmRecord {
            version: RECORD_VERSION,
            run: vm.run,
            image: vm.image.clone(),
            allocation: vm.allocation,
            phase: NodeVmPhase::Booting,
            diagnostics: Some(diagnostics.clone()),
            exit: None,
        };
        // The boot's intent, durable before any effect.
        books.put_node_vm(&record)?;
        // Its file made before its boot, so the run host keeps there all it says from its start on.
        let kept = match diagnostics_file(&diagnostics) {
            Ok(file) => file,
            Err(why) => {
                end_record(books.as_ref(), &mut record, Exit::Unknown)?;
                return Err(why);
            }
        };
        let slot = Arc::new(Slot::new());
        let stop = Arc::new(BootStop::new());
        image
            .runs
            .insert(vm.run, Run::Booting(Arc::clone(&slot), Arc::clone(&stop)))?;
        if let Err(why) = boot_on_its_thread(images, spec, kept, Arc::clone(&slot), stop) {
            image.runs.insert(vm.run, Run::Ended(End::unknown(None)))?;
            image.runs.forget(vm.run)?;
            end_record(books.as_ref(), &mut record, Exit::Unknown)?;
            return Err(Refusal::os("starting a node VM's boot", why));
        }
        let bound = waits.boot.deadline();
        let mut told = slot.told_within(bound)?;
        if let (Told::NotYet, Some(bound)) = (&told, bound) {
            waits.boot.ran_out(bound);
            told = slot.told_within(None)?;
        }
        if !matches!(told, Told::Came) {
            // Its thread ended with no answer: the guest it would have answered, if any, went with it (D231 stops a run
            // it drops).
            end_record(books.as_ref(), &mut record, Exit::Unknown)?;
            return Err(Refusal::Broken(format!(
                "node VM {}: its boot's thread ended with no answer, so its run's start is unknown",
                run_name(&vm.run)
            )));
        }
        if let Err(why) = image.runs.booted(vm.run, &slot, waits)? {
            image.runs.forget(vm.run)?;
            end_record(books.as_ref(), &mut record, Exit::Unknown)?;
            return Err(refused(BOOTING, why));
        }
        let guest = image.runs.guest(vm.run)?.ok_or_else(|| {
            Refusal::Broken("a node VM's run started with no guest held".to_owned())
        })?;
        let ready = match guest.control().copied() {
            None => Err(Refusal::Broken(
                "the node VM's run host bound no agent-control endpoint, though its boot asked one"
                    .to_owned(),
            )),
            Some(control) => follow_ready(guest.as_ref(), READY.deadline(), slot.asked, &READY)
                .map(|ready| (control, ready)),
        };
        let (control, ready) = match ready {
            Ok(said) => said,
            Err(why) => {
                drop(guest);
                return match self.stop_node(vm.run) {
                    Ok(_) => Err(why),
                    Err(cleanup) => Err(Refusal::Broken(format!(
                        "node VM {}: {why}; and its stop: {cleanup}",
                        run_name(&vm.run)
                    ))),
                };
            }
        };
        let took = slot.asked.elapsed();
        READY.answered(took);
        record.phase = NodeVmPhase::Ready;
        books.put_node_vm(&record)?;
        Ok(NodeBooted {
            run: vm.run,
            control,
            ready,
            took,
        })
    }

    /// Stops node VM `run`'s run, followed to its end: D231's stop is never refused, and its thread answers once the
    /// run host has gone. Past the stop's W*, the run-out told, the stop is followed by its thread's life (decision 9).
    /// Its end is kept in its record, Ended, and its run let go.
    pub fn stop_node(&self, run: NodeRun) -> Result<Exit, Refusal> {
        let image = self.image()?;
        let waits = image.waits()?;
        let books = self.authority()?;
        let slot = match image.runs.settle(run, Some(waits))? {
            Settled::Started(guest) | Settled::Running(guest) => {
                let slot = image.stop(guest, None)?;
                image.runs.stop_asked(run, &slot)?;
                slot
            }
            Settled::Stopping(slot, _) => slot,
            Settled::Ends(end) => return self.ended_node(books.as_ref(), run, &end),
            Settled::Booting => {
                return Err(Refusal::Invalid {
                    what: "node VM",
                    why: format!(
                        "run {}: its boot is under way, which the call that asked it follows",
                        run_name(&run)
                    ),
                });
            }
            Settled::None => return Err(not_running(&run)),
        };
        let bound = waits.stop.deadline();
        if !slot.came_within(bound)? {
            if let Some(bound) = bound {
                waits.stop.ran_out(bound);
            }
            if !matches!(slot.told_within(None)?, Told::Came) {
                return Err(Refusal::Broken(format!(
                    "node VM {}: its stop's thread ended with no answer, so its run's end is unknown",
                    run_name(&run)
                )));
            }
        }
        match image.runs.settle(run, Some(waits))? {
            Settled::Ends(end) => self.ended_node(books.as_ref(), run, &end),
            _ => Err(Refusal::Broken(format!(
                "node VM {}: its stop answered, and its run hasn't ended",
                run_name(&run)
            ))),
        }
    }

    /// Whether node VM `run` still runs here: a run that ended on its own is reaped through its stop, its end kept in
    /// its record; one booting, stopping or never run here doesn't.
    pub fn node_runs(&self, run: NodeRun) -> Result<bool, Refusal> {
        let image = self.image()?;
        let guest = match image.runs.settle(run, image.waits.as_ref().ok())? {
            Settled::Started(guest) | Settled::Running(guest) => guest,
            Settled::Ends(end) => {
                self.ended_node(self.authority()?.as_ref(), run, &end)?;
                return Ok(false);
            }
            Settled::Booting | Settled::Stopping(..) | Settled::None => return Ok(false),
        };
        match guest.wait_for(Duration::ZERO) {
            Ok(None) => Ok(true),
            Ok(Some(_)) | Err(ImageRefusal::Ended(_)) => {
                drop(guest);
                self.stop_node(run)?;
                Ok(false)
            }
            Err(why) => Err(refused("reading a node VM's end", why)),
        }
    }

    /// What node VM `run` costs its host (ROOT, 2026-11-13: its HostUse readable, as an image VM's): its run's microVM
    /// part (D241), read through the same stats as an image box's, its window from its run's start to this read.
    pub fn host_use_node(&self, run: NodeRun) -> Result<HostUse, Refusal> {
        let image = self.image()?;
        let waits = image.waits()?;
        let guest = match image.runs.settle(run, Some(waits))? {
            Settled::Started(guest) | Settled::Running(guest) => guest,
            _ => return Err(not_running(&run)),
        };
        let vcpus = self
            .authority()?
            .node_vm(&run)?
            .ok_or_else(|| Refusal::Broken("a running node VM has no record".to_owned()))?
            .allocation
            .vm_size()?
            .vcpus;
        let stats = match timed_stats(waits, guest.as_ref()) {
            Ok(stats) => stats,
            Err(ImageRefusal::Ended(_)) => return Err(not_running(&run)),
            Err(why) => return Err(refused("reading a node VM's stats", why)),
        };
        let window = image
            .runs
            .started(run)?
            .map(|at| at.elapsed())
            .ok_or_else(|| not_running(&run))?;
        host_use_of(&stats, vcpus, window)
    }

    /// Node VM `run`'s end, `end`, kept in its record where it lacks one, and its run let go: the end its record holds.
    fn ended_node(&self, books: &dyn Books, run: NodeRun, end: &End) -> Result<Exit, Refusal> {
        let mut record = books
            .node_vm(&run)?
            .ok_or_else(|| Refusal::Broken("an ended node VM has no record".to_owned()))?;
        let exit = match record.exit {
            Some(exit) => exit,
            None => {
                end_record(books, &mut record, end.exit)?;
                end.exit
            }
        };
        self.image()?.runs.forget(run)?;
        Ok(exit)
    }

    /// The image boot that runs Hex's own init under the nesting grant, as the hosted boot's
    /// ([`crate::guest::hosted::HostedConfig::spec`]), with its run's agent-control endpoint (§5):
    /// - **command**: `/init` alone, Hex's own init;
    /// - **no ready-by**: Hex's init would end the boot at one, and a node's learned bound is where its wait tells the
    ///   census a run-out, never where a boot ends (decision 9);
    /// - **no member, no published port, no grant past the VM, no credential** (default deny);
    /// - **its endpoint** takes its node's key alone and admits its lanes to the relay's sockets
    ///   ([`hex_client::guest::lanes::DIR`]), each lane's bytes in flight a guest frame each way, the most one control
    ///   message is (`hex_core::stream::MAX_FRAME`).
    fn node_spec(&self, vm: &NodeVm) -> Result<ImageSpec, Refusal> {
        let size = admitted_size(&vm.allocation)?;
        let window = u32::try_from(hex_core::stream::MAX_FRAME).map_err(|_| {
            Refusal::Broken("a guest frame is wider than a lane's window".to_owned())
        })?;
        Ok(ImageSpec {
            image: vm.image.clone(),
            vcpus: size.vcpus,
            memory_mib: size.memory_mib,
            command: Some(vec![crate::guest::hosted::INIT.to_owned()]),
            members: Vec::new(),
            publish: Vec::new(),
            egress: Narrowing {
                egress: Keep::Only(Vec::new()),
                mcp: Keep::Only(Vec::new()),
                destinations: Keep::Only(Vec::new()),
                dns: false,
                members: None,
            },
            credentials: Vec::new(),
            lessor: None,
            shares: vm.shares.clone(),
            nesting: true,
            ready_by: None,
            binary: Some(self.binary.clone()),
            env: self.run_host_env(),
            control: Some(ControlAsk {
                controller: vm.controller,
                sockets: Some(SocketGrant {
                    dir: hex_client::guest::lanes::DIR.to_owned(),
                    lanes: vm.lanes,
                    window,
                }),
            }),
            gpu_devices: Vec::new(),
            // No sign-in grant (Shards' D242 step 8): a node VM signs in to nothing.
            sign_ins: Vec::new(),
            // No upstream root (Shards' D242, image channel v26): it grants no credential to fill in.
            upstream_roots: Vec::new(),
        })
    }
}

/// Writes `record`'s end, `exit`, Ended.
fn end_record(books: &dyn Books, record: &mut NodeVmRecord, exit: Exit) -> Result<(), Refusal> {
    record.phase = NodeVmPhase::Ended;
    record.exit = Some(exit);
    books.put_node_vm(record)
}

/// A node VM's run as its record and its diagnostics name it: its 16 bytes in lowercase hex.
fn run_name(run: &NodeRun) -> String {
    run.bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A call about node VM `run` that isn't running here.
fn not_running(run: &NodeRun) -> Refusal {
    Refusal::Invalid {
        what: "node VM",
        why: format!("run {} isn't running here", run_name(run)),
    }
}

/// Follows a node VM's run to its init's READY: the person's rule of 2026-11-13, as decision 9 builds it.
/// - **Within W*** (`bound`, the slowest READY its node has seen, from `began`), READY is the answer.
/// - **Past W*** the run-out is told to the census (`ready`), and is no sample (Karn's point). The wait goes on a W*
///   window at a time while the run progresses, and is refused, stalled, only where a whole window shows none. Its
///   progress is its VM process's CPU time as its host counts it (D241). A window with no reading of it judges no
///   stall: the run is then followed by its life alone, as at a cold start.
/// - **At the cold start**, or after a run-out (no W*), the wait follows the run's life.
/// - **The run's end**, its host's end or a refusal of Shards' ends the wait at any time, typed.
/// - **A ready-by passed**, of a run that carries one (Hex gives none), is waited past to its late READY: D234's table
///   moves from it to ready alone.
pub(in crate::guest) fn follow_ready(
    guest: &dyn ImageGuest,
    bound: Option<Duration>,
    began: Instant,
    ready: &Patience,
) -> Result<Ready, Refusal> {
    let mut seen = None;
    let Some(window) = bound else {
        return wait_to(guest, &mut seen, Duration::MAX)?.ok_or_else(|| {
            Refusal::Broken("Shards answered no readiness to a wait with no timeout".to_owned())
        });
    };
    if let Some(said) = wait_to(guest, &mut seen, window.saturating_sub(began.elapsed()))? {
        return Ok(said);
    }
    ready.ran_out(window);
    let mut last = progress(guest, window)?;
    // Ends at READY, the run's end, or a whole window with its progress standing: the run's progress is its bound.
    loop {
        if let Some(said) = wait_to(guest, &mut seen, window)? {
            return Ok(said);
        }
        let now = progress(guest, window)?;
        if let (Some(was), Some(is)) = (last, now)
            && is <= was
        {
            return Err(Refusal::Os {
                op: WAITING,
                why: format!(
                    "no progress over a whole {window:?} past the slowest READY its node has seen: its VM \
                     process's CPU time stood at {was} ns"
                ),
            });
        }
        last = now.or(last);
    }
}

/// The run's READY within `timeout`, none by then. A ready-by passed, `seen` already or said now, is waited past to its
/// late READY: D234's table moves at most twice before ready, from starting to ready-by passed, then to ready late.
fn wait_to(
    guest: &dyn ImageGuest,
    seen: &mut Option<Ready>,
    timeout: Duration,
) -> Result<Option<Ready>, Refusal> {
    for _ in 0..2 {
        match guest.wait_ready(*seen, timeout) {
            Ok(Some(Ready::Passed(past))) => *seen = Some(Ready::Passed(past)),
            Ok(said) => return Ok(said),
            Err(why) => return Err(refused(WAITING, why)),
        }
    }
    Err(Refusal::Broken(
        "the node VM's readiness moved past D234's table".to_owned(),
    ))
}

/// The run's progress as its host counts it: its VM process's CPU time, D241's `Microvm` in the run's stats, read
/// within `window`. None where its run host has no such reading, or no answer came within the window.
fn progress(guest: &dyn ImageGuest, window: Duration) -> Result<Option<u64>, Refusal> {
    match guest.stats(StatsScope::Run, window) {
        Ok(stats) => Ok(stats
            .microvm
            .and_then(|microvm| microvm.host.ok())
            .map(|host| host.cpu_ns)),
        Err(ImageRefusal::Unsupported { .. }) => Ok(None),
        Err(why) => Err(refused(WAITING, why)),
    }
}

/// Shards' refusal of a node VM's call, typed by what it refused.
pub(in crate::guest) fn refused(op: &'static str, why: ImageRefusal) -> Refusal {
    const WHAT: &str = "the node VM under shards-init";
    match why {
        ImageRefusal::NoBackend { os, arch } => Refusal::Platform {
            what: WHAT,
            platform: os,
            why: format!("shards boots no guest on {os} {arch}"),
        },
        ImageRefusal::Spec { what, why } => Refusal::Invalid {
            what: WHAT,
            why: format!("{what}: {why}"),
        },
        ImageRefusal::Image(why) | ImageRefusal::Widening(why) => Refusal::Invalid {
            what: "the node image (HEX_GUEST_IMAGE)",
            why,
        },
        ImageRefusal::Unsupported { what, why } => Refusal::Platform {
            what,
            platform: std::env::consts::OS,
            why,
        },
        ImageRefusal::Ended(end) => Refusal::Os {
            op,
            why: format!("its run ended before its init said it was ready: {end:?}"),
        },
        other => Refusal::Os {
            op,
            why: other.to_string(),
        },
    }
}
