//! LB03: fence old peer connections before handing their effects to the FIFO peer core.
//! A sender durably increments its epoch at boot, then numbers each peer's messages. A receiver
//! commits its high-water mark before the core acts. Retrying the current message is idempotent;
//! a delayed older message cannot recreate a box after delete/ended/forget.

use std::collections::BTreeMap;
use std::path::Path;

use hex_api::Refusal;
use hex_sim::peer::Records;

use crate::records::Files;

pub struct Order {
    files: Files,
    pub epoch: u64,
    received: BTreeMap<u32, (u64, u64, [u8; 32])>,
}

fn damaged(why: impl Into<String>) -> Refusal {
    Refusal::Broken(why.into())
}

impl Order {
    pub fn open(path: &Path) -> Result<Self, Refusal> {
        let mut files =
            Files::open(path).map_err(|e| damaged(format!("peer order files: {e:?}")))?;
        let mut epoch = 0u64;
        let mut received = BTreeMap::new();
        for (name, value) in files
            .all()
            .map_err(|e| damaged(format!("peer order rows: {e:?}")))?
        {
            if name == "epoch" {
                epoch = value
                    .parse()
                    .map_err(|_| damaged("invalid peer sender epoch"))?;
            } else if let Some(id) = name.strip_prefix("peer-") {
                let id: u32 = id.parse().map_err(|_| damaged("invalid peer order ID"))?;
                let mut words = value.split_whitespace();
                if words.next() != Some("o1") {
                    return Err(damaged("invalid peer order version"));
                }
                let epoch = words
                    .next()
                    .ok_or_else(|| damaged("missing peer epoch"))?
                    .parse()
                    .map_err(|_| damaged("invalid peer epoch"))?;
                let seq = words
                    .next()
                    .ok_or_else(|| damaged("missing peer sequence"))?
                    .parse()
                    .map_err(|_| damaged("invalid peer sequence"))?;
                let hex = words
                    .next()
                    .ok_or_else(|| damaged("missing peer fingerprint"))?;
                if hex.len() != 64 || words.next().is_some() {
                    return Err(damaged("invalid peer fingerprint"));
                }
                let mut digest = [0u8; 32];
                for (out, pair) in digest.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
                    let pair = std::str::from_utf8(pair)
                        .map_err(|_| damaged("invalid peer fingerprint text"))?;
                    *out = u8::from_str_radix(pair, 16)
                        .map_err(|_| damaged("invalid peer fingerprint hex"))?;
                }
                let stamp = (epoch, seq, digest);
                received.insert(id, stamp);
            } else {
                return Err(damaged("unknown peer order record"));
            }
        }
        epoch = epoch
            .checked_add(1)
            .ok_or_else(|| damaged("peer sender epoch exhausted"))?;
        files
            .put("epoch", &epoch.to_string())
            .map_err(|e| damaged(format!("committing peer epoch: {e:?}")))?;
        Ok(Self {
            files,
            epoch,
            received,
        })
    }

    /// Caller holds its mutex until the matching driver operation has returned. This serializes
    /// old/new connections across physical effects, including a request already queued at reconnect.
    pub fn accept(
        &mut self,
        peer: u32,
        epoch: u64,
        seq: u64,
        digest: [u8; 32],
    ) -> Result<bool, Refusal> {
        if epoch == 0 || seq == 0 {
            return Err(damaged("zero peer message epoch or sequence"));
        }
        let stamp = (epoch, seq);
        if let Some(&(last_epoch, last_seq, last_digest)) = self.received.get(&peer) {
            let last = (last_epoch, last_seq);
            if stamp < last {
                return Ok(false);
            }
            if stamp == last {
                if digest != last_digest {
                    return Err(damaged("a retried peer sequence changed its message"));
                }
                return Ok(true);
            }
        } else if self.received.len() >= crate::records::MOST.saturating_sub(1) {
            return Err(Refusal::Exhausted {
                what: "peer ordering records",
                bound: crate::records::MOST.saturating_sub(1),
            });
        }
        use std::fmt::Write;
        let mut fingerprint = String::new();
        for byte in digest {
            write!(&mut fingerprint, "{byte:02x}").map_err(|e| damaged(e.to_string()))?;
        }
        self.files
            .put(
                &format!("peer-{peer}"),
                &format!("o1 {epoch} {seq} {fingerprint}"),
            )
            .map_err(|e| damaged(format!("committing peer message order: {e:?}")))?;
        self.received.insert(peer, (epoch, seq, digest));
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lb03_delayed_old_start_stays_fenced_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let mut order = Order::open(temp.path()).unwrap();
        let sender_epoch = order.epoch;
        assert!(order.accept(4, 1, 1, [0; 32]).unwrap()); // Start.
        assert!(order.accept(4, 1, 2, [0; 32]).unwrap()); // Delete, with physical deletion completed under lock.
        assert!(order.accept(4, 1, 3, [0; 32]).unwrap()); // Forget.
        assert!(!order.accept(4, 1, 1, [0; 32]).unwrap());
        drop(order);
        let mut order = Order::open(temp.path()).unwrap();
        assert!(order.epoch > sender_epoch);
        assert!(!order.accept(4, 1, 1, [0; 32]).unwrap());
        assert!(order.accept(4, 2, 1, [0; 32]).unwrap()); // New connection generation after owner reboot.
        assert!(!order.accept(4, 1, u64::MAX, [0; 32]).unwrap());
        assert!(order.accept(4, 2, 1, [1; 32]).is_err());
        assert!(order.accept(4, 2, 1, [0; 32]).unwrap()); // Current delivery can repeat after a lost response.
    }
}
