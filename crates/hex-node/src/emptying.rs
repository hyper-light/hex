//! How long a killed box takes to empty, learned (decision 9 of docs/design/runtime-architecture.md): one wait for
//! every OS's node, since the socket's core waits on its programs' ends by it too ([`crate::serve`]), and Linux's node
//! on its boxes' emptying, their programs' reaping and their inits' ends.

use std::time::{Duration, Instant};

use hex_core::patience::Patience;

/// How long a killed box takes to empty, from its kill (decision 9; Ada's rules of 2026-11-06 and 2026-11-13): the
/// slowest emptying this process's nodes have seen, from every delete, scrub and removal, since all wait on one kernel.
/// Before the first, and after one that ran out (a fault to report for that kill, yet a slow emptying, not a wedged
/// kernel: run 3's census seed ran out at 255.505 ms where run 5 answered in up to 904.705 ms), the wait follows the
/// box until it empties. Its programs' reaping and its init's end, which a box's emptying waits on, share the wait, as
/// do the socket's waits for the programs it killed. The `node.emptying` rows of
/// `bench/competitive/probes/waits_census.md` are the evidence the rule is held to, never a seed.
pub(crate) static EMPTYING: Patience = Patience::measuring("node.emptying");

/// What is left at the time of asking of the wait for a box killed at `killed` to empty ([`EMPTYING`]):
/// `Duration::MAX`, unbounded, while it is measuring with no emptying seen.
pub(crate) fn empty_patience(killed: Instant) -> Duration {
    EMPTYING
        .deadline()
        .map_or(Duration::MAX, |wait| wait.saturating_sub(killed.elapsed()))
}
