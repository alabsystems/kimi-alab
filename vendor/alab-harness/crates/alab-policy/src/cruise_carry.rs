// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! CRUISE NEVER PARKS AT A BACKGROUND-WORK BREAK.
//!
//! A turn that ends while the agent's own background work runs (a dynamic workflow, a background
//! agent, a shell or a monitor) draws Claude Code's `Waiting for 1 dynamic workflow` row or a
//! footer pill that counts the work, which the phase reader reads busy: the turn-end policy never
//! sees a point there (aterm's `turn_end::decide_turn_end` judges idle points alone), and the
//! occupancy door under its default `observe` types nothing. Under Cruise that break is a turn
//! end like any other: aterm's `BACKGROUND_CARRY` tells the agent to check on that work and keep
//! going, paced by the policy's own short-turn ladder ([`BackgroundCarries`]) so an agent that has
//! nothing else to do is asked again at 2, 4, 8 … and at most every 60 minutes, never left. Real
//! work is the busy time of the turn a break ends, judged once at that break, never wall time
//! (since the last carry, or a break stood), so a short turn of the program's own wake (a
//! background task's notification) keeps the ladder.

use std::time::{Duration, Instant};

/// The ledger's and the journal's name for the carry typed at a break of the agent's own
/// background work under Cruise (aterm's `BACKGROUND_CARRY`).
pub const RULE_BACKGROUND: &str = "continue-background@v1";

/// The ledger's name for the books of [`RULE_BACKGROUND`]'s carries started again in memory where
/// they held a carry ([`BackgroundCarries`]: a turn of real work, someone else's message, a turn
/// whose work went unread, a person's gesture after a loop before this one's last carry): a later
/// loop's replay of the ladder starts after it, as after a judged point.
pub const RULE_BACKGROUND_RESET: &str = "continue-background-reset@v1";

/// THE TIMINGS THE CARRIES WAIT ON: the turn-end policy's short-turn ladder and what it counts
/// as real work, as data (aterm's `TurnEndTiming::carry` gives its own), so a replay and each
/// harness pass the timings they run with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarryTiming {
    /// The busy time of a turn that is real work ([`BackgroundCarries::ended`]).
    pub min_work: Duration,
    /// The back-off after the first short answer ([`Self::short_wait_at`]).
    pub short_backoff: Duration,
    /// The ladder's cap ([`Self::short_wait_at`]), and how far past its rung a ladder lapses.
    pub short_backoff_max: Duration,
}

impl CarryTiming {
    /// The back-off before the next act after a streak of `streak` short turns
    /// (aterm's `TurnEndState::short_wait`): [`Self::short_backoff`] after the first, doubled at
    /// each turn past it up to [`Self::short_backoff_max`]; `None` before any.
    #[must_use]
    pub fn short_wait_at(&self, streak: u32) -> Option<Duration> {
        let n = streak.checked_sub(1)?;
        Some(crate::ladder::doubling(
            self.short_backoff,
            n,
            self.short_backoff_max,
        ))
    }
}

impl Default for CarryTiming {
    /// aterm's shipped timings (`TurnEndTiming::default`): a turn of 2 minutes is real work
    /// (`MIN_WORK`), and the ladder waits 2 minutes, doubled to 60.
    fn default() -> Self {
        let min = |m: u64| Duration::from_secs(m * 60);
        Self {
            min_work: min(2),
            short_backoff: min(2),
            short_backoff_max: min(60),
        }
    }
}

/// CRUISE'S CARRIES AT BACKGROUND BREAKS ([`RULE_BACKGROUND`]): when the last went and how many in a
/// row the agent answered short, which the next one waits on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackgroundCarries {
    /// When the last carry was typed: the ladder's back-off runs from it.
    last: Option<Instant>,
    /// Carries in a row whose answers ran short of [`CarryTiming::min_work`].
    streak: u32,
}

impl BackgroundCarries {
    /// When the next carry is due at the break that began at `began`: at once (the break's settle
    /// is the caller's) with none typed since the books were last started, else the last carry
    /// plus the short-turn back-off of the streak ([`CarryTiming::short_wait_at`]: 2 min,
    /// doubled each time, to 60). A TURN IS JUDGED ONCE, at the settled break that ends it
    /// ([`Self::ended`]): when this break began, read again or read afresh after the loop came
    /// back to it, is no work of any turn.
    #[must_use]
    pub fn due(&self, timing: &CarryTiming, began: Instant) -> Instant {
        self.due_within(timing, began, Duration::MAX)
    }

    /// [`Self::due`] with the back-off at most `cap`: the owner's standing go-ahead in the
    /// carry's place waits no longer from the last carry than a point's waits from the last act
    /// (aterm's `turn_end::GO_AHEAD_WAIT`).
    #[must_use]
    pub fn due_within(&self, timing: &CarryTiming, began: Instant, cap: Duration) -> Instant {
        self.last.map_or(began, |last| {
            let wait = timing
                .short_wait_at(self.streak)
                .unwrap_or(Duration::ZERO)
                .min(cap);
            last.checked_add(wait).unwrap_or(last)
        })
    }

    /// THE LADDER A LOOP BEFORE THIS ONE LEFT: its last carry typed at `last`, the `streak`-th in a
    /// row ([`Self::replayed`]), so a restart's next carry waits as that loop's would have.
    #[must_use]
    pub const fn carried(last: Instant, streak: u32) -> Self {
        Self {
            last: Some(last),
            streak,
        }
    }

    /// THE BOOKS' LADDER REPLAYED AS THE LIVE LADDER TYPED IT, at `now_ms`, from the carries
    /// `typed` (unix ms and the streak each row names, oldest first, none before a row that
    /// started the books again: aterm's `approvals::background_carries_in`): the
    /// newest carry's time and its streak, which [`Self::carried`] takes up. A carry whose row
    /// names its streak is at that streak, the live ladder's, books started again in memory alone
    /// included. One written before rows named it is one more in the run unless it came on books
    /// started again where no row says so (`Self::next_in_run`). `None` with no carry, or where
    /// the newest has lapsed (`Self::lapsed`): a restart there takes no ladder.
    #[must_use]
    pub fn replayed(
        timing: &CarryTiming,
        typed: &[(i64, Option<u32>)],
        now_ms: i64,
    ) -> Option<(i64, u32)> {
        let mut run: Option<(i64, u32)> = None;
        for &(at, named) in typed {
            run = Some(match (named, run) {
                (Some(streak), _) => (at, streak),
                (None, Some((last, streak))) if Self::next_in_run(timing, streak, at - last) => {
                    (at, streak.saturating_add(1))
                }
                (None, _) => (at, 1),
            });
        }
        run.filter(|&(last, streak)| !Self::lapsed(timing, streak, now_ms - last))
    }

    /// Whether a carry `gap_ms` after the one before it, the `streak`-th in a row, is the next of
    /// their run, for a row that names no streak. THE LIVE LADDER TYPES NONE SOONER THAN THE
    /// STREAK'S RUNG, so one closer came on books started again in memory alone (a turn of real
    /// work, someone else's message at a break); an eighth of the rung is the stamps' slack (a row
    /// is stamped after its typing, on a wall clock that slews). And one past the rung by the lapse
    /// ([`Self::lapsed`]) came where a restart takes no ladder. Books started again after a turn
    /// of real work longer than the rung show in no gap, which is why rows name their streak.
    fn next_in_run(timing: &CarryTiming, streak: u32, gap_ms: i64) -> bool {
        let rung = Self::rung(timing, streak);
        let gap = Duration::from_millis(u64::try_from(gap_ms).unwrap_or(0));
        gap >= rung.saturating_sub(rung / 8) && !Self::lapsed(timing, streak, gap_ms)
    }

    /// Whether the ladder lapsed `since_ms` after a carry, the `streak`-th in a row: THE LADDER'S
    /// CAP PAST THE RUNG that streak waits. A session at its break is carried at the rung, so what
    /// kept it that long (real work, a person, no loop running, a hold) is read as its books
    /// started again. Measured from the rung, a ladder on its cap never lapses at the carry due.
    fn lapsed(timing: &CarryTiming, streak: u32, since_ms: i64) -> bool {
        let since = Duration::from_millis(u64::try_from(since_ms).unwrap_or(0));
        since >= Self::rung(timing, streak).saturating_add(timing.short_backoff_max)
    }

    /// The wait after `streak` carries in a row ([`CarryTiming::short_wait_at`]).
    fn rung(timing: &CarryTiming, streak: u32) -> Duration {
        timing.short_wait_at(streak).unwrap_or(Duration::ZERO)
    }

    /// A carry was typed at `now`: one more short answer in a row, or the first since the books
    /// were last started.
    pub fn typed(&mut self, now: Instant) {
        self.streak = if self.last.is_some() {
            self.streak.saturating_add(1)
        } else {
            1
        };
        self.last = Some(now);
    }

    /// REAL WORK IS STICKY: a turn that was busy `worked` ended at a settled break, and a turn of
    /// real work ([`CarryTiming::min_work`]) starts the books again there, so a short wake or a
    /// held carry after it takes nothing back. A short one leaves them as they stand.
    pub fn ended(&mut self, timing: &CarryTiming, worked: Duration) {
        if worked >= timing.min_work {
            self.reset();
        }
    }

    /// A person's turn, someone else's message, a turn whose work went unread or a judged point:
    /// the next break is carried at its settle again.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Carries in a row the agent answered short.
    #[must_use]
    pub const fn streak(&self) -> u32 {
        self.streak
    }
}

#[cfg(test)]
#[path = "cruise_carry_tests.rs"]
mod tests;
