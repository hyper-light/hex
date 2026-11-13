//! No reader of a run's processes and sockets on this OS yet: every reading is empty, so each check of the host's
//! fails rather than passing what it can't see.

use super::{Process, Socket};

/// How this reader reads (told with every reading).
pub const HOW: &str = "no reader on this OS yet: nothing read";

/// No process read.
pub fn processes() -> Vec<Process> {
    Vec::new()
}

/// No socket read.
pub fn sockets(_: &[u32]) -> Vec<Socket> {
    Vec::new()
}
