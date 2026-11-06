//! The macOS box users' pool, grown on demand (native plan step 2, the lead's call 2026-11-06 under Ada's
//! rule: no picked pool size). The privileged helper makes a box user only when a box needs one and every
//! user it has is busy. The account it makes comes from this core alone, never from its caller: the name
//! after the highest index the pool holds (`_hexbox<n>`), at the first uid in Apple's role-account range
//! that no user, group or process holds. Other installers (Nix) share that range, so the helper takes no
//! more ids than it needs. Pure (CLAUDE.md §9): the helper reads the directory and makes the account.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

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
