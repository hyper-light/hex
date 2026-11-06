//! The macOS box users' pool, grown on demand (native plan step 2, the lead's call 2026-11-06 under Ada's
//! rule: no picked pool size). The privileged helper makes a box user only when a box needs one and every
//! user it has is busy. The account it makes comes from this core alone, never from its caller: the name
//! after the highest index the pool holds (`_hexbox<n>`), at the first uid in Apple's role-account range
//! that no user, group or process holds. Other installers (Nix) share that range, so the helper takes no
//! more ids than it needs. Pure (CLAUDE.md §9): the helper reads the directory and makes the account.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

/// Apple's role-account uids: `sysadminctl`'s usage on macOS 26.4, "Role accounts require name starting with
/// _ and UID in 450-499 range" (docs/research/runtime/installer-per-os.md §3.1).
pub const ROLE_UIDS: RangeInclusive<u32> = 450..=499;

/// Every pool user's name is this and its index: `_hexbox1`, `_hexbox2`, …
pub const POOL_PREFIX: &str = "_hexbox";

/// An account the helper makes: its name and uid, the rest of its attributes the helper's fixed template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    pub uid: u32,
}

/// The index a pool user's name holds: `_hexbox<n>`, `n` a decimal from 1 with no leading zero that fits a
/// `u32`; `None` for any other name.
pub fn pool_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix(POOL_PREFIX)?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The next account to make for a pool whose users are named `pool`: the name after the highest pool index
/// among them, at the first role uid not in `taken` (every user's uid, group's gid and running process's
/// uid the helper read just now). `None` when every role uid is taken, or the index can't grow.
pub fn next_account(pool: &[String], taken: &BTreeSet<u32>) -> Option<Account> {
    let index = pool
        .iter()
        .filter_map(|n| pool_index(n))
        .max()
        .unwrap_or(0)
        .checked_add(1)?;
    let uid = ROLE_UIDS.clone().find(|u| !taken.contains(u))?;
    Some(Account {
        name: format!("{POOL_PREFIX}{index}"),
        uid,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The role range is Apple's own: `sysadminctl`'s usage on macOS 26.4, "Role accounts require name
    /// starting with _ and UID in 450-499 range" (docs/research/runtime/installer-per-os.md §3.1).
    #[test]
    fn the_role_range_is_apples() {
        assert_eq!(ROLE_UIDS, 450..=499);
        assert_eq!(POOL_PREFIX, "_hexbox");
    }

    /// HEX-MAC-16: only `_hexbox<n>`, n a decimal from 1 with no leading zero, is a pool name. Do read
    /// pool and other names; expect the index of the first, nothing for the rest.
    #[test]
    fn only_hexbox_and_an_index_is_a_pool_name() {
        assert_eq!(pool_index("_hexbox1"), Some(1));
        assert_eq!(pool_index("_hexbox42"), Some(42));
        for other in [
            "_hexbox",
            "_hexbox0",
            "_hexbox01",
            "_hexboxa",
            "_hexbox1x",
            "hexbox1",
            "_nixbld1",
            "_hexbox-1",
            "_hexbox 1",
            "_hexbox99999999999",
        ] {
            assert_eq!(pool_index(other), None, "{other}");
        }
    }

    /// HEX-MAC-16: the next account follows the pool's highest index, at the first role uid nobody holds.
    /// Do ask with gaps in the names and taken ids; expect `_hexbox4` at 452, skipping 450 and 451.
    #[test]
    fn the_next_account_follows_the_pool_at_the_first_free_role_uid() {
        let taken = BTreeSet::from([450, 451, 9000]);
        assert_eq!(
            next_account(&names(&["_hexbox1", "_hexbox3", "_nixbld9"]), &taken),
            Some(Account {
                name: "_hexbox4".to_owned(),
                uid: 452
            })
        );
        assert_eq!(
            next_account(&[], &BTreeSet::new()),
            Some(Account {
                name: "_hexbox1".to_owned(),
                uid: 450
            })
        );
    }

    /// CLAUDE.md §2, §3: a full role range is a refusal, not an id outside it. Do take every role uid;
    /// expect no account.
    #[test]
    fn a_full_role_range_makes_no_account() {
        let taken: BTreeSet<u32> = ROLE_UIDS.collect();
        assert_eq!(next_account(&names(&["_hexbox1"]), &taken), None);
    }

    proptest! {
        /// HEX-MAC-16, model-based (CLAUDE.md §6): whatever the pool's names and whatever ids are taken,
        /// an account made is in the role range, untaken, a pool name, and new to the pool; and none is
        /// made only when every role uid is taken.
        #[test]
        fn an_account_is_always_in_range_untaken_and_new(
            pool in proptest::collection::vec("_hexbox[0-9]{1,3}|_[a-z]{1,8}[0-9]{0,2}|[a-z]{1,6}", 0..40),
            taken in proptest::collection::btree_set(400u32..520, 0..80),
        ) {
            match next_account(&pool, &taken) {
                Some(a) => {
                    prop_assert!(ROLE_UIDS.contains(&a.uid));
                    prop_assert!(!taken.contains(&a.uid));
                    prop_assert!(pool_index(&a.name).is_some());
                    prop_assert!(!pool.contains(&a.name));
                }
                None => prop_assert!(ROLE_UIDS.clone().all(|u| taken.contains(&u))
                    || pool.iter().filter_map(|n| pool_index(n)).any(|i| i == u32::MAX)),
            }
        }
    }
}
