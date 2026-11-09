//! The rule a macOS `process` box is bounded by (docs/design/runtime.md, decision "each OS runs its own
//! boxes natively"). macOS has no public cap on a process tree's memory or its process count
//! (docs/research/runtime/native-platforms.md §3), so the node's watcher reads the box's members and ends
//! the box once it passes a bound: an eviction by a stated rule (CLAUDE.md §2), where Linux's cgroup
//! refuses the allocation or the fork itself. A box can pass its bound by what it grows between two
//! reads, so each wait is the time its headroom lasts at the fastest the box can grow ([`Rates`]), held
//! between [`FLOOR`] and [`CEILING`]. Pure (CLAUDE.md §9): the shell (`hex_runtime::macos`) lists the
//! members, reads their footprints, waits and kills; this says when to read next, whether to end the
//! box, and what it holds.

use std::collections::BTreeSet;
use std::num::NonZeroU64;
use std::time::Duration;

/// Shortest wait between two reads of a box. The bound held at it on adas-mac-studio-3: a box kept
/// 256 MiB to within 42 MiB against its members allocating up to 164 GiB/s
/// (bench/native/macos/RESULT.md, commit 9694770).
pub const FLOOR: Duration = Duration::from_micros(250);

/// Longest wait between two reads of a box with a member alive: one far from its bounds is read this
/// often all the same, which bounds how long a process that joined its group unseen goes uncounted.
/// RESULT.md measured a box read this often costing "next to nothing".
pub const CEILING: Duration = Duration::from_millis(100);

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// The fastest a box grows, as the rule reckons it: what each wait divides the box's headroom by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rates {
    /// Footprint, in bytes a second.
    pub bytes: NonZeroU64,
    /// Processes started, a second.
    pub processes: NonZeroU64,
}

/// What a box may hold; `None` bounds nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limit {
    /// Its members' footprint summed, in bytes.
    pub memory: Option<u64>,
    /// Its processes, as [`Read::processes`] counts them.
    pub processes: Option<u64>,
}

/// Where a member is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Life {
    Running,
    /// It has ended and waits for its parent to reap it. The process bound counts it, as Linux's
    /// `pids.max` counts a task until it is reaped.
    Ended,
    /// It has ended and the node holds it unreaped: the program leading the box's process group, kept so
    /// the group's id can't be given to another process. The process bound leaves it out, since the node
    /// keeps it, not the box.
    Held,
}

/// One process of the box, as a read found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    pub pid: u32,
    /// When it started, on a clock the shell keeps: with its pid, which process it is.
    pub started: u64,
    /// Its footprint in bytes, 0 once it has ended (XNU zeroes an ended process's,
    /// `bsd/kern/kern_exit.c:2079`).
    pub footprint: u64,
    pub life: Life,
    /// Whether this member carries a fatal kernel memory cap at the box's whole bound, armed by the shell
    /// when it spawned the member (macOS's `posix_spawnattr_setjetsam_ext`; native plan step 1's
    /// `_ext` backstop). A member the box did not spawn (a fork) carries none. The kernel bounds an armed
    /// member with no overshoot, so a box whose every running member is armed, and that holds at most one,
    /// is paced at the [`CEILING`]; any fork or any unarmed member reverts it to headroom pacing.
    pub armed: bool,
}

/// Why a box is ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Over {
    /// Its footprint passed its memory bound.
    Memory { footprint: u64, bound: u64 },
    /// It held more processes than its process bound.
    Processes { count: u64, bound: u64 },
}

/// What the shell does after a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Read again after this long.
    Wait(Duration),
    /// End the box: signal every member KILL, then read again after [`FLOOR`].
    End(Over),
    /// The box is being ended and a member still lives (one that started as the kill went out): signal
    /// every member KILL again and read again after [`FLOOR`], until none lives.
    Kill,
    /// No member lives, so the box can't grow: nothing to read until a program is started in it.
    Idle,
}

/// What a read found and decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Read {
    pub verdict: Verdict,
    /// The box's footprint: its members' summed.
    pub footprint: u64,
    /// Its processes, as its process bound counts them: every member but one [`Life::Held`].
    pub processes: u64,
    /// Its members still running.
    pub running: u64,
}

/// One box under the rule: its bounds, the rates it is read at, and what its reads have found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    limit: Limit,
    rates: Rates,
    /// Why the box is being ended, from the read that decided it until a read finds none of its members
    /// running.
    ending: Option<Over>,
    /// The highest footprint a read found.
    peak: u64,
    reads: u64,
    /// The running-member count the last read found: while it rises, the box is still taking on members
    /// (a node starts a box's processes one after another), each beginning to allocate before the next is
    /// there, so the read paces at the floor rather than by a headroom the next new member would blow past.
    last_running: u64,
}

impl Watch {
    pub fn new(limit: Limit, rates: Rates) -> Self {
        Self {
            limit,
            rates,
            ending: None,
            peak: 0,
            reads: 0,
            last_running: 0,
        }
    }

    /// One read of the box: its members as listed now.
    pub fn read(&mut self, members: &[Member]) -> Read {
        // A count, so saturating: 2^64 reads are never made.
        self.reads = self.reads.saturating_add(1);
        // Saturating, as a footprint summed past 2^64 bytes is past any bound.
        let footprint = members
            .iter()
            .fold(0u64, |sum, m| sum.saturating_add(m.footprint));
        let processes = count(members, |m| m.life != Life::Held);
        let running = count(members, |m| m.life == Life::Running);
        // Every running member carries its own fatal kernel cap at the whole bound. `.all` is true of no
        // members, but running == 0 is the idle branch, so this is read only with a member running.
        let all_running_armed = members
            .iter()
            .filter(|m| m.life == Life::Running)
            .all(|m| m.armed);
        let taking_on = running > self.last_running;
        self.last_running = running;
        self.peak = self.peak.max(footprint);
        let verdict = if running == 0 {
            self.ending = None;
            Verdict::Idle
        } else if self.ending.is_some() {
            Verdict::Kill
        } else if let Some(over) = self.over(footprint, processes) {
            self.ending = Some(over);
            Verdict::End(over)
        } else if running <= 1 && all_running_armed {
            // The one member carries a fatal kernel cap at the whole bound (native plan step 1's `_ext`
            // backstop), so the kernel bounds it with no overshoot; the watcher need only read at the
            // ceiling to keep its accounting current and to notice a fork. A second member, or any member
            // the box didn't arm (a fork), drops this and the headroom rule below paces the reads.
            Verdict::Wait(CEILING)
        } else if taking_on {
            // A member just joined and begins allocating at once; the headroom the formula would pace by
            // doesn't account for it, so read again at the floor until the membership settles.
            Verdict::Wait(FLOOR)
        } else {
            Verdict::Wait(next(&self.limit, &self.rates, footprint, processes))
        };
        Read {
            verdict,
            footprint,
            processes,
            running,
        }
    }

    /// The bound the box is past, memory's first.
    fn over(&self, footprint: u64, processes: u64) -> Option<Over> {
        if let Some(bound) = self.limit.memory
            && footprint > bound
        {
            return Some(Over::Memory { footprint, bound });
        }
        match self.limit.processes {
            Some(bound) if processes > bound => Some(Over::Processes {
                count: processes,
                bound,
            }),
            _ => None,
        }
    }

    pub fn limit(&self) -> Limit {
        self.limit
    }

    /// Why the box is being ended, while it is.
    pub fn ending(&self) -> Option<Over> {
        self.ending
    }

    /// The highest footprint a read found.
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// How many reads the box has had.
    pub fn reads(&self) -> u64 {
        self.reads
    }
}

/// Members for which `which` holds.
fn count(members: &[Member], which: impl Fn(&Member) -> bool) -> u64 {
    // A slice holds at most isize::MAX members, so its count fits a u64.
    u64::try_from(members.iter().filter(|m| which(m)).count()).unwrap_or(u64::MAX)
}

/// The wait after a read of a box under its bounds: the shorter of the times its memory headroom and its
/// process headroom last, a bound it doesn't have lasting [`CEILING`].
pub fn next(limit: &Limit, rates: &Rates, footprint: u64, processes: u64) -> Duration {
    // Saturating: a box at its bound has no headroom.
    let memory = limit.memory.map_or(CEILING, |bound| {
        wait(bound.saturating_sub(footprint), rates.bytes)
    });
    let processes = limit.processes.map_or(CEILING, |bound| {
        wait(bound.saturating_sub(processes), rates.processes)
    });
    memory.min(processes)
}

/// How long `headroom` lasts at `rate` a second, in whole nanoseconds rounded down, held between
/// [`FLOOR`] and [`CEILING`].
pub fn wait(headroom: u64, rate: NonZeroU64) -> Duration {
    // headroom × 10^9 fits a u128 (2^64 × 10^9 < 2^94), and the rate isn't 0, so neither step fails.
    let nanos = u128::from(headroom)
        .checked_mul(NANOS_PER_SECOND)
        .and_then(|n| n.checked_div(u128::from(rate.get())))
        .map_or(u64::MAX, |n| u64::try_from(n).unwrap_or(u64::MAX));
    Duration::from_nanos(nanos).max(FLOOR).min(CEILING)
}

/// How a box's members changed between two listings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Members the new listing adds, by pid: each to watch for its forks and its end.
    pub added: Vec<u32>,
    /// Members that were running at the listing before and the new one no longer holds: each ended and
    /// was reaped, or left the box's process group (setsid(2), as a member that daemonizes does), which
    /// only a look at the process tells apart.
    pub gone: Vec<Member>,
}

/// What changed from listing `before` to listing `now`, a member being its pid and start time.
pub fn changes(before: &[Member], now: &[Member]) -> Changes {
    let was: BTreeSet<(u32, u64)> = before.iter().map(|m| (m.pid, m.started)).collect();
    let is: BTreeSet<(u32, u64)> = now.iter().map(|m| (m.pid, m.started)).collect();
    Changes {
        added: now
            .iter()
            .filter(|m| !was.contains(&(m.pid, m.started)))
            .map(|m| m.pid)
            .collect(),
        gone: before
            .iter()
            .filter(|m| m.life == Life::Running && !is.contains(&(m.pid, m.started)))
            .copied()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use proptest::prelude::*;
    use proptest::test_runner::TestRunner;

    use super::*;

    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;

    /// RESULT.md's: 256 GiB/s, and a process rate high enough not to matter here.
    fn rates() -> Rates {
        Rates {
            bytes: NonZeroU64::new(256 * GIB).unwrap(),
            processes: NonZeroU64::new(1_000_000).unwrap(),
        }
    }

    fn member(pid: u32, footprint: u64, life: Life) -> Member {
        Member {
            pid,
            started: u64::from(pid) * 7,
            footprint,
            life,
            armed: false,
        }
    }

    /// A member carrying the fatal kernel cap (one the box spawned), for the ceiling-pacing cases.
    fn armed(pid: u32, footprint: u64, life: Life) -> Member {
        Member {
            armed: true,
            ..member(pid, footprint, life)
        }
    }

    /// HEX-MAC-4: a box at 95% of a 256 MiB bound is read at the floor; one at 1 MiB waits as long as its
    /// 255 MiB of headroom lasts at 256 GiB/s; one with no bound waits the ceiling.
    #[test]
    fn a_box_is_read_as_often_as_its_headroom_asks() {
        let limit = Limit {
            memory: Some(256 * MIB),
            processes: None,
        };
        assert_eq!(next(&limit, &rates(), 256 * MIB / 100 * 95, 1), FLOOR);
        // 255 MiB at 256 GiB/s: 255 / (256 × 1024) s, 972,747 ns rounded down.
        assert_eq!(
            next(&limit, &rates(), MIB, 1),
            Duration::from_nanos(972_747)
        );
        assert_eq!(next(&Limit::default(), &rates(), 64 * GIB, 900), CEILING);
        // At the bound exactly the box isn't past it, and has no headroom left.
        assert_eq!(next(&limit, &rates(), 256 * MIB, 1), FLOOR);
    }

    /// HEX-MAC-4: the process bound's headroom paces reads too, the shorter wait winning.
    #[test]
    fn the_shorter_of_the_two_headrooms_sets_the_wait() {
        let limit = Limit {
            memory: Some(256 * MIB),
            processes: Some(64),
        };
        let slow_forks = Rates {
            processes: NonZeroU64::new(1_000).unwrap(),
            ..rates()
        };
        // 1 MiB used (972.7 µs of headroom), 63 processes left at 1,000/s (63 ms): memory's wins.
        assert_eq!(
            next(&limit, &slow_forks, MIB, 1),
            Duration::from_nanos(972_747)
        );
        // 1 process left at 10,000/s lasts 100 µs, under memory's 972.7 µs: held at the floor.
        let fast_forks = Rates {
            processes: NonZeroU64::new(10_000).unwrap(),
            ..rates()
        };
        assert_eq!(next(&limit, &fast_forks, MIB, 63), FLOOR);
    }

    /// HEX-MAC-4 (native plan step 1's `_ext` backstop): a box holding at most one running member, every
    /// running member armed with the kernel's fatal cap, is read at the ceiling — the kernel bounds the
    /// member with no overshoot, so the watcher needn't pace by headroom. Unarmed, the same read is paced
    /// by its headroom, far under the ceiling.
    #[test]
    fn a_single_armed_member_is_read_at_the_ceiling() {
        let limit = Limit {
            memory: Some(256 * MIB),
            processes: None,
        };
        let mut w = Watch::new(limit, rates());
        // One armed member well under the bound: the ceiling, though 255 MiB of headroom lasts ~973 µs.
        assert_eq!(
            w.read(&[armed(1, MIB, Life::Running)]).verdict,
            Verdict::Wait(CEILING)
        );
        // Held there: still the ceiling on the next read (not taking on, still one armed member).
        assert_eq!(
            w.read(&[armed(1, MIB, Life::Running)]).verdict,
            Verdict::Wait(CEILING)
        );
        // The contrast: the same member unarmed is paced by its headroom, which is far under the ceiling.
        let mut u = Watch::new(limit, rates());
        let _ = u.read(&[member(1, MIB, Life::Running)]); // taking on -> floor
        let unarmed = u.read(&[member(1, MIB, Life::Running)]).verdict;
        assert_eq!(unarmed, Verdict::Wait(next(&limit, &rates(), MIB, 1)));
        assert_ne!(unarmed, Verdict::Wait(CEILING));
    }

    /// HEX-MAC-4: a two-member box is paced by headroom, not the ceiling, even when both members are
    /// armed — the kernel caps each at the whole bound, which doesn't bound their sum, so the watcher's
    /// headroom rule holds the box.
    #[test]
    fn a_two_member_box_is_paced_by_headroom() {
        let limit = Limit {
            memory: Some(256 * MIB),
            processes: None,
        };
        let mut w = Watch::new(limit, rates());
        let two = [armed(1, MIB, Life::Running), armed(2, MIB, Life::Running)];
        let _ = w.read(&two); // taking on 0 -> 2: floor
        let steady = w.read(&two).verdict;
        assert_eq!(steady, Verdict::Wait(next(&limit, &rates(), 2 * MIB, 2)));
        assert_ne!(steady, Verdict::Wait(CEILING));
    }

    /// HEX-MAC-4: a fork (an unarmed member the box didn't spawn) reverts the box to headroom pacing,
    /// whether it joins an armed member or is all that's left of one — any unarmed running member drops
    /// the ceiling.
    #[test]
    fn a_fork_or_an_unarmed_member_reverts_to_headroom() {
        let limit = Limit {
            memory: Some(256 * MIB),
            processes: None,
        };
        // An armed member and its forked (unarmed) child: two running members, so headroom.
        let mut w = Watch::new(limit, rates());
        let pair = [armed(1, MIB, Life::Running), member(2, MIB, Life::Running)];
        let _ = w.read(&pair);
        let with_fork = w.read(&pair).verdict;
        assert_eq!(with_fork, Verdict::Wait(next(&limit, &rates(), 2 * MIB, 2)));
        assert_ne!(with_fork, Verdict::Wait(CEILING));
        // Only the unarmed fork remains (the armed member exited): one running member, unarmed -> headroom.
        let mut u = Watch::new(limit, rates());
        let _ = u.read(&[member(2, MIB, Life::Running)]);
        let lone_fork = u.read(&[member(2, MIB, Life::Running)]).verdict;
        assert_eq!(lone_fork, Verdict::Wait(next(&limit, &rates(), MIB, 1)));
        assert_ne!(lone_fork, Verdict::Wait(CEILING));
    }

    /// HEX-MAC-4: an uncapped box is read as before — it has no memory bound, so no member is armed, and
    /// the headroom rule returns the ceiling for want of a bound; the arming rule changes nothing here.
    #[test]
    fn an_uncapped_box_is_unchanged() {
        let none = Limit::default();
        let mut w = Watch::new(none, rates());
        // First read takes on -> floor (unchanged); then steady -> ceiling, for want of any bound.
        assert_eq!(
            w.read(&[member(1, MIB, Life::Running)]).verdict,
            Verdict::Wait(FLOOR)
        );
        assert_eq!(
            w.read(&[member(1, MIB, Life::Running)]).verdict,
            Verdict::Wait(CEILING)
        );
        // Two members, steady: still the ceiling, for want of a bound.
        let mut t = Watch::new(none, rates());
        let two = [member(1, MIB, Life::Running), member(2, MIB, Life::Running)];
        let _ = t.read(&two);
        assert_eq!(t.read(&two).verdict, Verdict::Wait(CEILING));
    }

    /// HEX-MAC-1: a read past the memory bound ends the box with the footprint it reached; a read while
    /// a member still runs kills again; a read with none running leaves the box idle, and the box is
    /// watched afresh after.
    #[test]
    fn a_box_past_its_bound_is_ended_once_and_killed_until_empty() {
        let mut w = Watch::new(
            Limit {
                memory: Some(64 * MIB),
                processes: None,
            },
            rates(),
        );
        let under = w.read(&[member(10, 60 * MIB, Life::Running)]);
        assert_eq!(under.verdict, Verdict::Wait(FLOOR));
        let past = w.read(&[
            member(10, 50 * MIB, Life::Running),
            member(11, 20 * MIB, Life::Running),
        ]);
        assert_eq!(
            past.verdict,
            Verdict::End(Over::Memory {
                footprint: 70 * MIB,
                bound: 64 * MIB
            })
        );
        assert_eq!(w.peak(), 70 * MIB);
        // One started as the kill went out: it is killed too, and the end isn't decided twice.
        let late = w.read(&[member(10, 0, Life::Held), member(12, MIB, Life::Running)]);
        assert_eq!(late.verdict, Verdict::Kill);
        let empty = w.read(&[member(10, 0, Life::Held)]);
        assert_eq!(empty.verdict, Verdict::Idle);
        assert_eq!(w.ending(), None);
        let again = w.read(&[member(10, 0, Life::Held), member(13, MIB, Life::Running)]);
        assert!(matches!(again.verdict, Verdict::Wait(_)), "{again:?}");
        assert_eq!(w.reads(), 5);
    }

    /// HEX-MAC-1: the process bound counts a running member and one ended but unreaped, not the node's
    /// held leader; past it the box is ended with the count.
    #[test]
    fn the_process_bound_counts_every_member_but_the_held_leader() {
        let mut w = Watch::new(
            Limit {
                memory: None,
                processes: Some(2),
            },
            rates(),
        );
        let held = w.read(&[
            member(20, 0, Life::Held),
            member(21, MIB, Life::Running),
            member(22, 0, Life::Ended),
        ]);
        assert_eq!((held.processes, held.running), (2, 1));
        assert!(matches!(held.verdict, Verdict::Wait(_)), "{held:?}");
        let past = w.read(&[
            member(20, 0, Life::Held),
            member(21, MIB, Life::Running),
            member(22, 0, Life::Ended),
            member(23, MIB, Life::Running),
        ]);
        assert_eq!(
            past.verdict,
            Verdict::End(Over::Processes { count: 3, bound: 2 })
        );
    }

    /// HEX-MAC-6: a running member the next listing lacks is gone, to be looked at; one already ended
    /// isn't; a pid back with another start time is a new process.
    #[test]
    fn a_listing_says_which_members_are_new_and_which_went() {
        let before = [
            member(30, MIB, Life::Running),
            member(31, MIB, Life::Running),
            member(32, 0, Life::Ended),
        ];
        let reused = Member {
            started: 999,
            ..member(31, MIB, Life::Running)
        };
        let now = [member(30, MIB, Life::Running), reused];
        let c = changes(&before, &now);
        assert_eq!(c.added, vec![31]);
        assert_eq!(c.gone, vec![member(31, MIB, Life::Running)]);
    }

    /// The rule as its decision states it, written apart from the core: the longest whole number of
    /// nanoseconds whose growth at `rate` stays within `headroom`, found by bisection, held between the
    /// floor and the ceiling.
    fn oracle_wait(headroom: u64, rate: u64) -> Duration {
        let ceiling = CEILING.as_nanos();
        let fits = |ns: u128| ns * u128::from(rate) <= u128::from(headroom) * 1_000_000_000;
        // Invariant: `lo` fits; `hi` doesn't, or is past the ceiling.
        let (mut lo, mut hi) = (0u128, ceiling + 1);
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if fits(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Duration::from_nanos(u64::try_from(lo.clamp(FLOOR.as_nanos(), ceiling)).unwrap())
    }

    /// What the oracle says of read `i` of a series: the box is idle with none of its members running;
    /// being ended if a read since it was last idle was past a bound; ended if this read is past one; else
    /// it waits the shorter of its headrooms.
    fn oracle(limit: Limit, rates: Rates, series: &[Vec<Member>], i: usize) -> Verdict {
        let sum = |read: &[Member]| read.iter().map(|m| u128::from(m.footprint)).sum::<u128>();
        let counted = |read: &[Member]| {
            u64::try_from(read.iter().filter(|m| m.life != Life::Held).count()).unwrap()
        };
        let running = |read: &[Member]| read.iter().any(|m| m.life == Life::Running);
        let past = |read: &[Member]| -> Option<Over> {
            let footprint = u64::try_from(sum(read).min(u128::from(u64::MAX))).unwrap();
            if limit
                .memory
                .is_some_and(|b| u128::from(footprint) > u128::from(b))
            {
                return Some(Over::Memory {
                    footprint,
                    bound: limit.memory.unwrap(),
                });
            }
            limit
                .processes
                .filter(|b| counted(read) > *b)
                .map(|bound| Over::Processes {
                    count: counted(read),
                    bound,
                })
        };
        if !running(&series[i]) {
            return Verdict::Idle;
        }
        let since_idle = series[..i]
            .iter()
            .rposition(|read| !running(read))
            .map_or(0, |idle| idle + 1);
        if series[since_idle..i]
            .iter()
            .any(|read| past(read).is_some())
        {
            return Verdict::Kill;
        }
        if let Some(over) = past(&series[i]) {
            return Verdict::End(over);
        }
        let running_n = |read: &[Member]| {
            u64::try_from(read.iter().filter(|m| m.life == Life::Running).count()).unwrap()
        };
        // Every running member armed and at most one running: the kernel caps the member, so the box is
        // read at the ceiling regardless of headroom or taking-on (native plan step 1's `_ext` backstop).
        let all_armed = |read: &[Member]| {
            read.iter()
                .filter(|m| m.life == Life::Running)
                .all(|m| m.armed)
        };
        if running_n(&series[i]) <= 1 && all_armed(&series[i]) {
            return Verdict::Wait(CEILING);
        }
        // A read that finds more running members than the one before it (the box is still taking members
        // on) paces at the floor, regardless of headroom.
        let prev_running = if i == 0 { 0 } else { running_n(&series[i - 1]) };
        if running_n(&series[i]) > prev_running {
            return Verdict::Wait(FLOOR);
        }
        let footprint = u64::try_from(sum(&series[i]).min(u128::from(u64::MAX))).unwrap();
        let memory = limit.memory.map_or(CEILING, |b| {
            oracle_wait(b.saturating_sub(footprint), rates.bytes.get())
        });
        let processes = limit.processes.map_or(CEILING, |b| {
            oracle_wait(b.saturating_sub(counted(&series[i])), rates.processes.get())
        });
        Verdict::Wait(memory.min(processes))
    }

    fn arb_life() -> impl Strategy<Value = Life> {
        prop_oneof![Just(Life::Running), Just(Life::Ended), Just(Life::Held)]
    }

    fn arb_series() -> impl Strategy<Value = Vec<Vec<Member>>> {
        // `armed` weighted toward true so single-armed reads (the ceiling path) are met often.
        let member = (
            1u32..64,
            0u64..(64 * GIB),
            arb_life(),
            proptest::bool::weighted(0.6),
        )
            .prop_map(|(pid, footprint, life, armed)| Member {
                pid,
                started: u64::from(pid),
                footprint: if life == Life::Running { footprint } else { 0 },
                life,
                armed,
            });
        prop::collection::vec(prop::collection::vec(member, 0..8), 1..120)
    }

    fn arb_limit() -> impl Strategy<Value = Limit> {
        (
            prop::option::of(MIB..(64 * GIB)),
            prop::option::of(1u64..64),
        )
            .prop_map(|(memory, processes)| Limit { memory, processes })
    }

    fn arb_rates() -> impl Strategy<Value = Rates> {
        (1u64..(1 << 41), 1u64..10_000_000).prop_map(|(bytes, processes)| Rates {
            bytes: NonZeroU64::new(bytes).unwrap(),
            processes: NonZeroU64::new(processes).unwrap(),
        })
    }

    /// The verdicts of each kind the model-based test met, so it can't pass by never meeting one
    /// (CLAUDE.md §6's non-vacuity counters).
    #[derive(Default)]
    struct Met {
        floor: Cell<u64>,
        between: Cell<u64>,
        ceiling: Cell<u64>,
        /// A ceiling wait the arming rule forced: one running member, armed (native plan step 1).
        armed_ceiling: Cell<u64>,
        memory: Cell<u64>,
        processes: Cell<u64>,
        kill: Cell<u64>,
        idle: Cell<u64>,
    }

    /// HEX-MAC-4 (model-based, CLAUDE.md §6): for random series of reads, each of the core's verdicts is
    /// the oracle's, its peak is the series' highest footprint, and no wait is longer than the headroom
    /// lasts at the stated rate, unless it is the floor or the arming rule paced it at the ceiling (the
    /// kernel caps the one armed member, so the wait is the ceiling on purpose). Every kind of verdict is
    /// met: a wait at the floor, between it and the ceiling, at the ceiling, and a ceiling the arming rule
    /// forced; an end for memory and for processes; a kill; a box gone idle.
    #[test]
    fn the_core_decides_as_the_oracle() {
        let met = Met::default();
        let bump = |c: &Cell<u64>| c.set(c.get() + 1);
        let mut runner = TestRunner::new(ProptestConfig::with_cases(512));
        let cases = (arb_limit(), arb_rates(), arb_series());
        runner
            .run(&cases, |(limit, rates, series)| {
                let mut w = Watch::new(limit, rates);
                let mut peak = 0u64;
                for i in 0..series.len() {
                    let read = w.read(&series[i]);
                    prop_assert_eq!(read.verdict, oracle(limit, rates, &series, i), "read {}", i);
                    peak = peak.max(read.footprint);
                    // The arming rule paces a single armed running member at the ceiling, regardless of
                    // its headroom (the kernel caps it).
                    let armed_rule = read.running == 1
                        && series[i]
                            .iter()
                            .filter(|m| m.life == Life::Running)
                            .all(|m| m.armed);
                    match read.verdict {
                        Verdict::Wait(d) => {
                            prop_assert!((FLOOR..=CEILING).contains(&d));
                            if armed_rule {
                                prop_assert_eq!(d, CEILING);
                                bump(&met.armed_ceiling);
                            }
                            bump(match d {
                                d if d == FLOOR => &met.floor,
                                d if d == CEILING => &met.ceiling,
                                _ => &met.between,
                            });
                            // The headroom bound holds for every paced wait but the floor and a ceiling the
                            // arming rule set (longer than headroom on purpose, the kernel cap behind it).
                            if d > FLOOR && !armed_rule {
                                let ns = d.as_nanos();
                                if let Some(b) = limit.memory {
                                    let room = u128::from(b - read.footprint);
                                    prop_assert!(
                                        ns * u128::from(rates.bytes.get()) <= room * 1_000_000_000
                                    );
                                }
                                if let Some(b) = limit.processes {
                                    let room = u128::from(b - read.processes);
                                    prop_assert!(
                                        ns * u128::from(rates.processes.get())
                                            <= room * 1_000_000_000
                                    );
                                }
                            }
                        }
                        Verdict::End(Over::Memory { .. }) => bump(&met.memory),
                        Verdict::End(Over::Processes { .. }) => bump(&met.processes),
                        Verdict::Kill => bump(&met.kill),
                        Verdict::Idle => bump(&met.idle),
                    }
                }
                prop_assert_eq!(w.peak(), peak);
                prop_assert_eq!(w.reads(), u64::try_from(series.len()).unwrap());
                Ok(())
            })
            .unwrap();
        for (kind, n) in [
            ("a wait at the floor", &met.floor),
            ("a wait between floor and ceiling", &met.between),
            ("a wait at the ceiling", &met.ceiling),
            ("a ceiling the arming rule forced", &met.armed_ceiling),
            ("an end for memory", &met.memory),
            ("an end for processes", &met.processes),
            ("a kill", &met.kill),
            ("an idle box", &met.idle),
        ] {
            assert!(n.get() > 0, "the series met no {kind}");
        }
    }

    proptest! {
        /// HEX-MAC-4: the wait is the oracle's for any headroom and rate.
        #[test]
        fn the_wait_is_the_oracles(headroom in any::<u64>(), rate in 1u64..u64::MAX) {
            prop_assert_eq!(wait(headroom, NonZeroU64::new(rate).unwrap()), oracle_wait(headroom, rate));
        }
    }
}
