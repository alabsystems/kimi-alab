// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

use super::*;

const MIN: i64 = 60_000;
const T: i64 = 1_790_000_000_000;

fn mins(n: u64) -> Duration {
    Duration::from_secs(n * 60)
}

/// Two boots of one machine.
const BOOT_A: [u8; 16] = [
    0x4D, 0x5A, 0x1B, 0x2C, 0x3E, 0x4F, 0x50, 0x61, 0x72, 0x83, 0x94, 0xA5, 0xB6, 0xC7, 0xD8, 0xE9,
];
const BOOT_B: [u8; 16] = [
    0x4D, 0x5A, 0x1B, 0x2C, 0x3E, 0x4F, 0x50, 0x61, 0x72, 0x83, 0x94, 0xA5, 0xB6, 0xC7, 0xD8, 0xEA,
];

/// A count of `asleep` on the boot `boot` names. aterm builds it with its `BootSpelling::kernel`,
/// which this crate does not have; the case is never read here, since one boot is its 16 bytes
/// however it was spelled.
fn stamp(asleep: Duration, boot: Option<[u8; 16]>) -> SleepStamp {
    SleepStamp {
        asleep,
        boot: boot.map(|id| Boot::new(id, false)),
    }
}

/// The agent whose done row these turns are read from: Claude Code, as aterm's agent answers for
/// every program it supervises ([`crate::claude_code::done_row_step`]).
struct ClaudeCode;

impl Agent for ClaudeCode {
    fn done_row_step(&self, work: Duration) -> Duration {
        crate::claude_code::done_row_step(work)
    }
}

/// A SLEEP PLACED EXACTLY IS WHOLLY INSIDE A SPAN OR NONE OF IT: a turn
/// begins and ends awake, so a 30-minute sleep the system's record places is
/// all inside a span that holds it, and none inside one that holds half of
/// it (the span reaches past the turn) or none.
#[test]
fn a_sleep_placed_exactly_is_wholly_inside_a_span_or_none_of_it() {
    let exact = Sleep {
        len: mins(30),
        from_ms: T,
        to_ms: T + 30 * MIN,
    };
    assert_eq!(exact.most_inside(T - MIN, T + 31 * MIN), mins(30));
    assert_eq!(exact.most_inside(T, T + 30 * MIN), mins(30));
    assert_eq!(
        exact.most_inside(T + 15 * MIN, T + 40 * MIN),
        Duration::ZERO
    );
    assert_eq!(
        exact.most_inside(T + 30 * MIN, T + 40 * MIN),
        Duration::ZERO
    );
    assert_eq!(exact.most_inside(T - 10 * MIN, T), Duration::ZERO);
    assert_eq!(exact.least_inside(T - MIN, T + 31 * MIN), mins(30));
    assert_eq!(exact.least_inside(T + 15 * MIN, T + 40 * MIN), mins(15));
    assert_eq!(exact.least_inside(T - 10 * MIN, T), Duration::ZERO);
    let unplaced = Sleep {
        len: mins(30),
        from_ms: T,
        to_ms: T + 60 * MIN,
    };
    assert_eq!(unplaced.most_inside(T + 50 * MIN, T + 70 * MIN), mins(10));
    assert_eq!(unplaced.most_inside(T + 20 * MIN, T + 70 * MIN), mins(30));
    assert_eq!(
        unplaced.least_inside(T + 50 * MIN, T + 70 * MIN),
        Duration::ZERO
    );
    assert_eq!(unplaced.least_inside(T + 20 * MIN, T + 70 * MIN), mins(10));
    assert_eq!(unplaced.least_inside(T - MIN, T + 61 * MIN), mins(30));
}

/// A TURN'S DONE ROW LESS THE SLEEP INSIDE IT: a try answered at once with
/// the Mac asleep 30 minutes from its Enter (`30m 3s`) held the sleep; ten
/// minutes of work before the same sleep (`10m 0s`, ended as the Mac went to
/// sleep) held none of it; the done row's minute widens the span by its
/// minute; a span is never read as more sleep than it is long.
#[test]
fn a_turns_span_holds_the_sleep_that_lies_inside_it() {
    let mut book = SleepBook::default();
    assert_eq!(
        book.in_turn(&ClaudeCode, mins(40), T, T),
        InTurn::default(),
        "CONTROL"
    );
    book.add(mins(30), T + 1000, T + 30 * MIN + 2000);
    let answered = Duration::from_secs(30 * 60 + 3);
    let end = T + 30 * MIN + 3000;
    assert_eq!(book.in_turn(&ClaudeCode, answered, end, end).most, mins(30));
    assert_eq!(book.in_turn(&ClaudeCode, mins(10), T, T), InTurn::default());
    let read = book.in_turn(&ClaudeCode, mins(10), T, T + 59_000);
    assert!(read.most <= Duration::from_secs(59), "{read:?}");
    assert_eq!(read.least, Duration::ZERO);
    book.add(mins(30), T + 40 * MIN, T + 75 * MIN);
    book.add(mins(30), T + 45 * MIN, T + 80 * MIN);
    assert_eq!(
        book.in_turn(&ClaudeCode, mins(20), T + 70 * MIN, T + 70 * MIN)
            .most,
        mins(20)
    );
}

/// A SLEEP THAT ENDED BEFORE A TURN BEGAN IS NO PART OF IT: the Mac slept
/// eight hours, the system's record placing it; a person woke it and pressed
/// Enter five seconds after the wake, and their turn worked 620 s, its done
/// row naming the minute it ended 30 s into.
#[test]
fn a_sleep_that_ended_before_a_turn_began_is_no_part_of_it() {
    let woke = T;
    let night = Duration::from_secs(8 * 3600);
    let mut book = SleepBook::default();
    book.add(night, woke - 8 * 60 * MIN, woke);
    let read = |enter: i64, work: Duration| {
        let end = enter + i64::try_from(work.as_millis()).unwrap_or(0);
        let lo = end - 30_000;
        book.in_turn(&ClaudeCode, work, lo, lo + 59_999)
    };
    assert_eq!(
        read(woke + 5_000, Duration::from_secs(620)),
        InTurn::default()
    );
    let across = read(
        woke - 8 * 60 * MIN - 5_000,
        night + Duration::from_secs(620),
    );
    assert_eq!(across.most, night, "CONTROL");
}

/// THE LEAST SLEEP THAT MUST LIE INSIDE A TURN, for whether a person's
/// keystroke is inside it ([`InTurn::least`]), where the most that can is
/// for the work it is judged by ([`InTurn::most`]).
#[test]
fn a_turn_holds_at_least_the_sleep_that_must_lie_inside_it() {
    let act = T;
    let mut hour = SleepBook::default();
    hour.add(mins(60), act, act + 600 * MIN);
    let at = act + 300 * MIN;
    let read = hour.in_turn(&ClaudeCode, Duration::from_secs(328), at, at + 59_999);
    assert_eq!(read.most, Duration::from_secs(328));
    assert_eq!(read.least, Duration::ZERO);
    let mut night = SleepBook::default();
    night.add(mins(590), act, act + 600 * MIN);
    let read = night.in_turn(&ClaudeCode, mins(30), at, at);
    assert!((mins(19)..=mins(20)).contains(&read.least), "{read:?}");
    assert_eq!(read.most, mins(30), "{read:?}");
    let mut placed = SleepBook::default();
    placed.add(mins(30), T + 5 * MIN, T + 35 * MIN);
    let read = placed.in_turn(&ClaudeCode, mins(40), T + 40 * MIN, T + 40 * MIN + 59_999);
    assert_eq!(
        read,
        InTurn {
            most: mins(30),
            least: mins(30)
        }
    );
    let enter = T;
    let mut long = SleepBook::default();
    long.add(
        Duration::from_secs(8 * 3600),
        enter + 10_000,
        enter + 8 * 60 * MIN + 10_000,
    );
    let end = enter + (9 * 3600 + 20) * 1000;
    let read = long.in_turn(
        &ClaudeCode,
        Duration::from_secs(9 * 3600),
        end,
        end + 59_999,
    );
    assert_eq!(read.most, Duration::from_secs(8 * 3600), "{read:?}");
    for book in [&hour, &night, &placed, &long] {
        for (work, lo) in [(mins(5), T + 30 * MIN), (mins(500), T + 540 * MIN)] {
            let read = book.in_turn(&ClaudeCode, work, lo, lo + 59_999);
            assert!(read.least <= read.most, "{read:?}");
        }
    }
}

/// THE SLEEP BEFORE A LOOP'S FIRST LOOK: the system's last sleep is placed
/// exactly; the ledger's count since the last act, on the same boot, is that
/// sleep where it fell since the act, and the rest lies between the act and
/// it.
#[test]
fn a_followers_take_up_places_what_the_system_and_the_ledger_say() {
    let act = T;
    let look = T + 60 * MIN;
    let then = stamp(mins(100), Some(BOOT_A));
    let mut one = SleepBook::default();
    let last = Some((T + 5 * MIN, T + 35 * MIN));
    let took = one.take_up(
        look,
        Some(&stamp(mins(130), Some(BOOT_A))),
        Some((act, Some(&then))),
        last,
    );
    assert_eq!(took.since_act, SinceAct::Counted(mins(30)));
    assert!(took.last);
    assert_eq!(
        one.sleeps().copied().collect::<Vec<_>>(),
        [Sleep {
            len: mins(30),
            from_ms: T + 5 * MIN,
            to_ms: T + 35 * MIN
        }]
    );
    let mut two = SleepBook::default();
    two.take_up(
        look,
        Some(&stamp(mins(140), Some(BOOT_A))),
        Some((act, Some(&then))),
        last,
    );
    assert_eq!(
        two.sleeps().copied().collect::<Vec<_>>(),
        [
            Sleep {
                len: mins(30),
                from_ms: T + 5 * MIN,
                to_ms: T + 35 * MIN
            },
            Sleep {
                len: mins(10),
                from_ms: act,
                to_ms: T + 5 * MIN
            }
        ]
    );
    let mut counted = SleepBook::default();
    counted.take_up(
        look,
        Some(&stamp(mins(130), Some(BOOT_A))),
        Some((act, Some(&then))),
        None,
    );
    assert_eq!(
        counted.sleeps().copied().collect::<Vec<_>>(),
        [Sleep {
            len: mins(30),
            from_ms: act,
            to_ms: look
        }]
    );
}

/// WHERE NOTHING SAYS, NOTHING IS WRITTEN from the ledger, and the take-up
/// says why ([`SinceAct::unknown`]): an act's row an older build wrote (no
/// count), a count on another boot, a boot neither names, a machine that
/// keeps no count; the system's last sleep is still placed (it needs no
/// ledger), and one that ended after the look (no sleep before it) is not.
#[test]
fn a_take_up_with_nothing_to_say_keeps_the_reading_as_it_was() {
    let act = T;
    let look = T + 60 * MIN;
    let then = stamp(mins(100), Some(BOOT_A));
    let now = stamp(mins(130), Some(BOOT_A));
    let cases = [
        (Some((act, None)), Some(&now), SinceAct::Uncounted),
        (
            Some((act, Some(&then))),
            Some(&stamp(mins(130), Some(BOOT_B))),
            SinceAct::OtherBoot,
        ),
        (
            Some((act, Some(&stamp(mins(100), None)))),
            Some(&stamp(mins(130), None)),
            SinceAct::OtherBoot,
        ),
        (Some((act, Some(&then))), None, SinceAct::NoCount),
        (Some((act, None)), None, SinceAct::NoCount),
        (None, Some(&now), SinceAct::NoAct),
    ];
    for (row, own, said) in cases {
        let mut book = SleepBook::default();
        let took = book.take_up(look, own, row, None);
        assert_eq!(took.since_act, said);
        assert_eq!(
            said.unknown().is_some(),
            said != SinceAct::NoAct,
            "{said:?}"
        );
        assert!(book.is_empty(), "{said:?}");
        let mut with_last = SleepBook::default();
        with_last.take_up(look, own, row, Some((T + 5 * MIN, T + 35 * MIN)));
        assert_eq!(with_last.sleeps().count(), 1, "{said:?}");
        let mut after = SleepBook::default();
        after.take_up(look, own, row, Some((look - MIN, look + MIN)));
        assert!(after.is_empty(), "{said:?}");
    }
    let mut control = SleepBook::default();
    let took = control.take_up(look, Some(&now), Some((act, Some(&then))), None);
    assert_eq!(
        control.in_turn(&ClaudeCode, mins(40), look, look).most,
        mins(30),
        "CONTROL"
    );
    assert_eq!(took.since_act.unknown(), None, "CONTROL");
}

/// The book keeps its newest [`SLEEPS_KEPT`] sleeps.
#[test]
fn a_book_keeps_its_newest_sleeps() {
    let mut book = SleepBook::default();
    for i in 0..=i64::try_from(SLEEPS_KEPT).unwrap_or(64) {
        book.add(mins(1), T + i * 10 * MIN, T + i * 10 * MIN + MIN);
    }
    assert_eq!(book.sleeps().count(), SLEEPS_KEPT);
    assert_eq!(book.sleeps().next().map(|s| s.from_ms), Some(T + 10 * MIN));
}
