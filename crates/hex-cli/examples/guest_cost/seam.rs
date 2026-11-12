//! The seam a VM class's driver implements: one guest at a time, booted, held idle, reported on by its
//! own agent, and torn down. The census, the evidence and the output are the producer's ([`super::run`]).

use std::path::Path;
use std::time::{Duration, Instant};

use hex_api::CapacityResources;

/// What a driver says of its class for the evidence: one `name value` pair a line, each naming what it
/// is and where it came from (a path with its SHA-256, a variable and its value).
pub type Lines = Vec<(String, String)>;

/// The class's configured envelope, and where its configuration says so.
pub struct Envelope {
    pub resources: CapacityResources,
    pub source: String,
}

/// The patience a guest is booted within, before any run-out doubles it, and its record.
pub struct Patience {
    pub bound: Duration,
    pub source: String,
}

/// What one guest's VMM cost its host over an idle hold: its CPU time over the hold, its peak memory so
/// far, and the bytes it has written so far, each as its OS reads them ([`Driver::host_source`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    pub cpu: Duration,
    pub peak_bytes: u64,
    pub written_bytes: u64,
}

pub trait Driver {
    /// A booted, ready guest.
    type Guest;

    /// The class, as its records name it (`hex_runtime::guest::CLASS` for Shards).
    fn class(&self) -> String;
    /// What this run measures: the class's images and configuration.
    fn describe(&self) -> Result<Lines, String>;
    /// The envelope floor and ceiling hold.
    fn envelope(&self) -> Result<Envelope, String>;
    /// The first boot's patience.
    fn patience(&self) -> Result<Patience, String>;
    /// The folder the VMM's own files go on: its filesystem's block size is the disk quantum.
    fn runtime(&self) -> &Path;
    /// How the driver reads its VMM's CPU, peak memory and writes (its OS's interfaces), for the evidence.
    fn host_source(&self) -> String;

    /// Boots one guest and waits, until `deadline`, for it to be ready (its agent answering): `None` when
    /// the deadline passed first, the guest already torn down.
    fn boot(&mut self, deadline: Instant) -> Result<Option<Self::Guest>, String>;
    /// Holds the ready guest idle for `span`, reading its VMM's use over it.
    fn hold(&mut self, guest: &Self::Guest, span: Duration) -> Result<Host, String>;
    /// The guest agent's own report of what it has free inside the envelope, until `deadline`.
    fn report(
        &mut self,
        guest: &Self::Guest,
        deadline: Instant,
    ) -> Result<CapacityResources, String>;
    /// Stops the guest and waits, until `deadline`, for its end.
    fn tear_down(&mut self, guest: Self::Guest, deadline: Instant) -> Result<(), String>;
}
