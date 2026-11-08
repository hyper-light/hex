//! The one copy left of a program's stdio (native plan step 8): where a program's stream is a connection of its
//! own (a guest's stdio connection, docs/design/shards-guest.md §3) and its caller gave a pipe of its own for it,
//! the caller's client copies between the two, one thread and one buffer per stream. Never the node: through its
//! socket, the node gives that connection's end to the caller's client with the exec's answer.

use std::io::{self, Read, Write};

use hex_api::Refusal;

/// Copies `from` into `to` on a thread of its own until either ends, then hands `to` to `ended` (where a
/// connection's sending half ends, so its program sees its stdin end). Unbounded in time, as a local exec's stdin is
/// the caller's own pipe: a program that doesn't read it holds its copier, which ends with the stream (CLAUDE.md
/// §8). Its end is the stream's: the program sees stdin end, the caller sees stdout end, as with a pipe; there is
/// no one else to tell.
pub(crate) fn copy<R, W>(
    mut from: R,
    mut to: W,
    ended: impl FnOnce(W) + Send + 'static,
) -> Result<(), Refusal>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    std::thread::Builder::new()
        .name("hex-client-stdio".to_owned())
        .spawn(move || {
            let _ = io::copy(&mut from, &mut to);
            ended(to);
        })
        .map(drop)
        .map_err(|e| Refusal::os("starting a stdio copier", e))
}
