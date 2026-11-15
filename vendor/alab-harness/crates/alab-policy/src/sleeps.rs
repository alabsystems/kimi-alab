// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! THE SLEEPS OF THE MACHINE A LOOP KNOWS OF, on the wall clock: what a turn's done row counts that was no work.

use std::collections::VecDeque;
use std::time::Duration;

use crate::port::Agent;

/// How many sleeps a book keeps, the newest: a turn's span reaches back a day or two at most, and a machine sleeps a few times a day.
pub const SLEEPS_KEPT: usize = 64;

/// A sleep shorter than this is not written in a book from a count alone ([`SleepBook::take_up`]'s rest): the counts are read at different moments, and a minute is under every measure a turn's work is judged by (the two-minute slack of an act's turn, the five minutes that end a limit's episode).
pub const SLEEP_FLOOR: Duration = Duration::from_secs(60);

/// ONE SLEEP OF THE MACHINE: `len` long, lying somewhere inside the wall window `from_ms..to_ms` (unix milliseconds) — exactly there when the window is as long as the sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sleep {
    /// How long the machine slept.
    pub len: Duration,
    /// The window it lies in: from…
    pub from_ms: i64,
    /// …to.
    pub to_ms: i64,
}

impl Sleep {
    /// PLACED EXACTLY: its window is no longer than the sleep (the system's record of it, a watch with no awake time between its two reads).
    #[must_use]
    pub fn placed(&self) -> bool {
        self.to_ms.saturating_sub(self.from_ms) <= millis(self.len)
    }

    /// The most of this sleep that can lie inside `start_ms..end_ms`, every moment a turn's span can reach: placed exactly, all of it where its window lies within the span, else none (it lies outside the turn: a sleep that ended before the Enter is no part of the turn, however far the done row's minute reaches back into it); else no more than the sleep, nor than the stretch of its window inside the span.
    #[must_use]
    pub fn most_inside(&self, start_ms: i64, end_ms: i64) -> Duration {
        if self.placed() {
            return if start_ms <= self.from_ms && self.to_ms <= end_ms {
                self.len
            } else {
                Duration::ZERO
            };
        }
        self.len.min(ms(self.overlap(start_ms, end_ms)))
    }

    /// The least of this sleep that must lie inside `start_ms..end_ms`, the moments every span a turn can have holds: what the stretch of its window outside them cannot hold — none, where that stretch is as long as the sleep (a count the ledger cannot place, somewhere in hours).
    #[must_use]
    pub fn least_inside(&self, start_ms: i64, end_ms: i64) -> Duration {
        let window = self.to_ms.saturating_sub(self.from_ms);
        let outside = window.saturating_sub(self.overlap(start_ms, end_ms));
        self.len.saturating_sub(ms(outside))
    }

    /// How much of its window lies inside `start_ms..end_ms`, in ms.
    fn overlap(&self, start_ms: i64, end_ms: i64) -> i64 {
        let (lo, hi) = (self.from_ms.max(start_ms), self.to_ms.min(end_ms));
        hi.saturating_sub(lo).max(0)
    }
}

/// THE SLEEP INSIDE A TURN ([`SleepBook::in_turn`]): the most of the sleeps known that can lie inside it, and the least that must.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InTurn {
    /// The most that can lie inside the turn.
    pub most: Duration,
    /// The least that must, never more than [`Self::most`].
    pub least: Duration,
}

/// THE MACHINE'S COUNT OF ITS SLEEP at a moment: how long it slept since it booted, and the boot it counts in (`aterm_uds::process::asleep_since_boot`, `astream_sys::boot_session`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepStamp {
    /// The sleep since boot.
    pub asleep: Duration,
    /// The boot ([`Boot`]: the kernel's boot id, `astream_sys::boot_session`, the reading aterm-link's broker opens its log in); `None` where the kernel names none, or names the all-zero id a damaged answer reads as, and then the count compares with no other process's.
    pub boot: Option<Boot>,
}

impl SleepStamp {
    /// How long the machine slept since `then`, read on this boot: `None` on another boot, or a boot either does not name.
    #[must_use]
    pub fn since(&self, then: &SleepStamp) -> Option<Duration> {
        let same = matches!((&self.boot, &then.boot), (Some(a), Some(b)) if a == b);
        same.then(|| self.asleep.saturating_sub(then.asleep))
    }
}

/// A BOOT A COUNT OF THE MACHINE'S SLEEP IS ON: the kernel's boot id, its 16 bytes (`astream_sys::boot_session`), and the case its text was spelled in, which an act's ledger row (`approvals::BOOT`) and a tape's count (`tape::core`) write it back in (aterm's `BootSpelling::text`).
///
/// This crate holds the bytes and the case and spells no boot id: aterm reads and writes the text (its `BootSpelling`: `kernel`, `of` and `text`, over `astream_sys`), because the supervisor and the bus must spell a boot id the same way.
#[derive(Debug, Clone, Copy)]
pub struct Boot {
    /// The id's 16 bytes, as `astream_sys::parse_boot_id` reads them.
    id: [u8; 16],
    /// Spelled lower-case (Linux's `boot_id`); upper-case otherwise.
    lower: bool,
}

impl Boot {
    /// The boot whose id is `id`'s 16 bytes, its text spelled lower-case if `lower` (Linux's `boot_id`), upper-case otherwise: what aterm's `BootSpelling` makes of this host's kernel's reading (`kernel`) and of a row's or a tape's text (`of`).
    #[must_use]
    pub const fn new(id: [u8; 16], lower: bool) -> Self {
        Self { id, lower }
    }

    /// The id's 16 bytes.
    #[must_use]
    pub const fn id(&self) -> [u8; 16] {
        self.id
    }

    /// Its text is spelled lower-case (Linux's `boot_id`); upper-case otherwise.
    #[must_use]
    pub const fn lower(&self) -> bool {
        self.lower
    }
}

/// One boot is its 16 bytes, however it was spelled ([`Boot`]).
impl PartialEq for Boot {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Boot {}

/// A LOOP'S FIRST LOOK AT THE MACHINE'S SLEEP: what a seed sets the ledger's last act against ([`SleepBook::count_since_act`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstLook {
    /// When, unix ms.
    pub at_ms: i64,
    /// The machine's count of its sleep then.
    pub count: Option<SleepStamp>,
    /// The system's record of its last sleep then (unix ms, asleep and woke).
    pub last: Option<(i64, i64)>,
}

/// THE SLEEPS A LOOP KNOWS OF ([module docs](self)), the newest [`SLEEPS_KEPT`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SleepBook {
    sleeps: VecDeque<Sleep>,
}

impl SleepBook {
    /// No sleep known: every done row reads whole.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sleeps.is_empty()
    }

    /// The sleeps known, oldest first.
    pub fn sleeps(&self) -> impl Iterator<Item = &Sleep> {
        self.sleeps.iter()
    }

    /// A sleep of `len` that lies inside `from_ms..to_ms`: the loop's watch measured it between two reads, or the ledger counts it.
    pub fn add(&mut self, len: Duration, from_ms: i64, to_ms: i64) {
        let sleep = Sleep {
            len,
            from_ms,
            to_ms,
        };
        if len.is_zero() || to_ms <= from_ms || self.sleeps.contains(&sleep) {
            return;
        }
        if self.sleeps.len() == SLEEPS_KEPT {
            self.sleeps.pop_front();
        }
        self.sleeps.push_back(sleep);
    }

    /// THE SLEEP INSIDE A TURN whose done row says `work` (its wall span, shown as finely as `agent`'s done row shows it: [`Agent::done_row_step`]) and ended between `end_lo_ms` and `end_hi_ms` (the done row's clock names a minute; with none, the read's time is both): the most of every sleep known that can lie inside some span the turn can have had, and the least that must lie inside every one ([`InTurn`]), each at most the span itself.
    #[must_use]
    pub fn in_turn(
        &self,
        agent: &(impl Agent + ?Sized),
        work: Duration,
        end_lo_ms: i64,
        end_hi_ms: i64,
    ) -> InTurn {
        if self.sleeps.is_empty() || work.is_zero() {
            return InTurn::default();
        }
        let step = millis(agent.done_row_step(work));
        let span_ms = millis(work);
        let (lo, hi) = (end_lo_ms.min(end_hi_ms), end_hi_ms.max(end_lo_ms));
        let reach = (lo.saturating_sub(span_ms.saturating_add(step)), hi);
        let held = (hi.saturating_sub(span_ms.saturating_sub(step)), lo);
        let (most, least) =
            self.sleeps
                .iter()
                .fold((Duration::ZERO, Duration::ZERO), |(most, least), s| {
                    let m = s.most_inside(reach.0, reach.1);
                    let l = if held.0 < held.1 {
                        s.least_inside(held.0, held.1).min(m)
                    } else {
                        Duration::ZERO
                    };
                    (most.saturating_add(m), least.saturating_add(l))
                });
        let most = most.min(work);
        InTurn {
            most,
            least: least.min(most),
        }
    }

    /// THE SLEEP BEFORE THIS LOOP'S FIRST LOOK, at `now_ms`, that no read of its own measured: what
    /// the system's record of its last sleep (`last`) and the sleep count on the ledger's last act
    /// (`act`, against `now`) say of it.
    pub fn take_up(
        &mut self,
        now_ms: i64,
        now: Option<&SleepStamp>,
        act: Option<(i64, Option<&SleepStamp>)>,
        last: Option<(i64, i64)>,
    ) -> TakeUp {
        let placed = self.place_last(now_ms, last);
        TakeUp {
            last: placed,
            since_act: self.count_since_act(now_ms, now, act, last),
        }
    }

    /// [`Self::take_up`]'s first half, at the loop's first look: the system's last sleep, where it ended before `now_ms`.
    pub fn place_last(&mut self, now_ms: i64, last: Option<(i64, i64)>) -> bool {
        let Some((slept, woke)) = before_look(now_ms, last) else {
            return false;
        };
        self.add(ms(woke - slept), slept, woke);
        true
    }

    /// [`Self::take_up`]'s second half, once the ledger is read: the sleep since its last act that the system's last sleep (`last`, as the first look read it) does not place.
    pub fn count_since_act(
        &mut self,
        now_ms: i64,
        now: Option<&SleepStamp>,
        act: Option<(i64, Option<&SleepStamp>)>,
        last: Option<(i64, i64)>,
    ) -> SinceAct {
        let Some((at_ms, stamp)) = act else {
            return SinceAct::NoAct;
        };
        let Some(now) = now else {
            return SinceAct::NoCount;
        };
        let Some(stamp) = stamp else {
            return SinceAct::Uncounted;
        };
        let Some(total) = now.since(stamp) else {
            return SinceAct::OtherBoot;
        };
        let (placed, before) = match before_look(now_ms, last) {
            Some((slept, woke)) if slept >= at_ms => (ms(woke - slept), slept),
            _ => (Duration::ZERO, now_ms),
        };
        let rest = total.saturating_sub(placed);
        if rest >= SLEEP_FLOOR {
            self.add(rest, at_ms, before);
        }
        SinceAct::Counted(total)
    }
}

/// The system's last sleep (`last`), where it is one that ended before the
/// look at `now_ms`.
fn before_look(now_ms: i64, last: Option<(i64, i64)>) -> Option<(i64, i64)> {
    last.filter(|&(slept, woke)| slept < woke && woke <= now_ms)
}

/// What [`SleepBook::take_up`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeUp {
    /// The system's last sleep was written.
    pub last: bool,
    /// What the ledger said of the sleep since its last act.
    pub since_act: SinceAct,
}

/// What the ledger says of the sleep since its last act ([`TakeUp`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinceAct {
    /// No act on the books.
    NoAct,
    /// The act's row holds no count: an older build wrote it.
    Uncounted,
    /// Its count is another boot's, or a boot this machine does not name.
    OtherBoot,
    /// This machine keeps no count of its sleep (Windows).
    NoCount,
    /// The machine slept this long since the act.
    Counted(Duration),
}

impl SinceAct {
    /// WHY THE LEDGER COULD NOT SAY how long the machine slept since its last act, in a journal line's words: the sleep before the loop's first look is then counted by the system's record of its last sleep alone, and a turn's done row reaching back past that look is read as before this book, whole of the rest.
    #[must_use]
    pub const fn unknown(self) -> Option<&'static str> {
        match self {
            Self::Uncounted => {
                Some("the last act on the books counts no sleep (an older build's row)")
            }
            Self::OtherBoot => Some("the last act on the books counts its sleep on another boot"),
            Self::NoCount => Some("this machine keeps no count of its sleep"),
            Self::NoAct | Self::Counted(_) => None,
        }
    }
}

fn ms(d: i64) -> Duration {
    Duration::from_millis(u64::try_from(d).unwrap_or(0))
}

fn millis(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
#[path = "sleeps_tests.rs"]
mod tests;
