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
    received: BTreeMap<u32, (u64, u64)>,
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
                let (epoch, seq) = value
                    .split_once(' ')
                    .ok_or_else(|| damaged("invalid peer order tuple"))?;
                let stamp = (
                    epoch.parse().map_err(|_| damaged("invalid peer epoch"))?,
                    seq.parse().map_err(|_| damaged("invalid peer sequence"))?,
                );
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
    pub fn accept(&mut self, peer: u32, epoch: u64, seq: u64) -> Result<bool, Refusal> {
        if epoch == 0 || seq == 0 {
            return Err(damaged("zero peer message epoch or sequence"));
        }
        let stamp = (epoch, seq);
        if let Some(&last) = self.received.get(&peer) {
            if stamp < last {
                return Ok(false);
            }
            if stamp == last {
                return Ok(true);
            }
        } else if self.received.len() >= crate::records::MOST.saturating_sub(1) {
            return Err(Refusal::Exhausted {
                what: "peer ordering records",
                bound: crate::records::MOST.saturating_sub(1),
            });
        }
        self.files
            .put(&format!("peer-{peer}"), &format!("{epoch} {seq}"))
            .map_err(|e| damaged(format!("committing peer message order: {e:?}")))?;
        self.received.insert(peer, stamp);
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
        assert!(order.accept(4, 1, 1).unwrap()); // Start.
        assert!(order.accept(4, 1, 2).unwrap()); // Delete, with physical deletion completed under lock.
        assert!(order.accept(4, 1, 3).unwrap()); // Forget.
        assert!(!order.accept(4, 1, 1).unwrap());
        drop(order);
        let mut order = Order::open(temp.path()).unwrap();
        assert!(order.epoch > sender_epoch);
        assert!(!order.accept(4, 1, 1).unwrap());
        assert!(order.accept(4, 2, 1).unwrap()); // New connection generation after owner reboot.
        assert!(!order.accept(4, 1, u64::MAX).unwrap());
        assert!(order.accept(4, 2, 1).unwrap()); // Current delivery can repeat after a lost response.
    }
}
