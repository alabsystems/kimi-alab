// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

use super::*;

const MIN: Duration = Duration::from_secs(60);

#[test]
fn short_answers_back_off_on_the_policys_ladder_and_real_work_resets_it() {
    let timing = CarryTiming::default();
    #[expect(
        clippy::disallowed_methods,
        reason = "the test's origin for the carries' times: the ladder reads no clock"
    )]
    let t0 = Instant::now();
    let mut c = BackgroundCarries::default();
    assert_eq!(c.due(&timing, t0), t0, "the first break is carried at once");
    c.typed(t0);
    let mut last = t0;
    for (n, wait) in [2, 4, 8, 16, 32, 60, 60].into_iter().enumerate() {
        // The agent answered in 20 s and its turn ended on the work again.
        c.ended(&timing, Duration::from_secs(20));
        let began = last + Duration::from_secs(20);
        let due = c.due(&timing, began);
        assert_eq!(due, last + wait * MIN, "carry {} waits {wait} min", n + 2);
        c.typed(due);
        last = due;
    }
    assert_eq!(c.streak(), 8);
    // The agent worked min_work on the last carry: the next break goes at once.
    c.ended(&timing, timing.min_work);
    let began = last + timing.min_work;
    assert_eq!(c.due(&timing, began), began);
    c.typed(began);
    assert_eq!(c.streak(), 1, "real work starts the ladder again");
    c.reset();
    let later = began + Duration::from_secs(5);
    assert_eq!(
        c.due(&timing, later),
        later,
        "someone else's turn resets it"
    );
}

/// The books' carries replayed: a whole ladder to its cap (each carry stamped 5 s past its
/// rung) is one run, a carry a second short of its rung still is, and one closer than seven
/// eighths of its rung or the cap past it starts the run again; a newest the cap past its rung
/// is none.
#[test]
fn the_books_carries_replay_the_ladder_the_live_loop_kept() {
    let timing = CarryTiming::default();
    let min = |m: i64| m * 60_000;
    let mut typed = vec![0];
    for wait in [2, 4, 8, 16, 32, 60, 60, 60] {
        typed.push(typed[typed.len() - 1] + min(wait) + 5_000);
    }
    let last = typed[typed.len() - 1];
    let replay = |typed: &[i64], now: i64| {
        let unnamed: Vec<(i64, Option<u32>)> = typed.iter().map(|&at| (at, None)).collect();
        BackgroundCarries::replayed(&timing, &unnamed, now)
    };
    assert_eq!(replay(&typed, last + min(1)), Some((last, 9)));
    assert_eq!(
        replay(&typed, last + min(119)),
        Some((last, 9)),
        "past due, not lapsed"
    );
    assert_eq!(
        replay(&typed, last + min(120)),
        None,
        "the cap past its rung"
    );
    assert_eq!(replay(&[], 0), None);
    // A second short of the 4 minutes the second carry's streak waits: the stamps' slack.
    assert_eq!(
        replay(&[0, min(2), min(6) - 1_000], min(7)),
        Some((min(6) - 1_000, 3))
    );
    for (why, gap) in [("short", min(3)), ("lapsed", min(64))] {
        let at = min(2) + gap;
        let after = [0, min(2), at, at + min(2)];
        assert_eq!(replay(&after, at + min(3)), Some((at + min(2), 2)), "{why}");
    }
}

/// A row that names its streak is at it, whatever its gap: two carries 15 minutes apart, each
/// named at one (real work between them), are no run, and one a minute after the carry before
/// it is at the two it names, where its gap alone reads books started again. A row that names
/// none, written before rows named it, counts on from the named one before it by its gap. A
/// named newest still lapses.
#[test]
fn a_carrys_named_streak_is_replayed_as_its_row_says() {
    let timing = CarryTiming::default();
    let min = |m: i64| m * 60_000;
    let replay =
        |typed: &[(i64, Option<u32>)], now: i64| BackgroundCarries::replayed(&timing, typed, now);
    assert_eq!(
        replay(&[(0, Some(1)), (min(15), Some(1))], min(16)),
        Some((min(15), 1))
    );
    assert_eq!(
        replay(&[(0, None), (min(15), None)], min(16)),
        Some((min(15), 2)),
        "CONTROL: their gap alone reads a run"
    );
    assert_eq!(
        replay(&[(0, None), (min(1), Some(2))], min(2)),
        Some((min(1), 2))
    );
    assert_eq!(
        replay(&[(0, Some(3)), (min(8), None)], min(9)),
        Some((min(8), 4))
    );
    assert_eq!(replay(&[(0, Some(6))], min(120)), None, "lapsed");
}

#[test]
fn a_short_wake_turn_keeps_the_ladder_to_its_cap_and_its_real_work_resets_it() {
    let timing = CarryTiming::default();
    #[expect(
        clippy::disallowed_methods,
        reason = "the test's origin for the carries' times: the ladder reads no clock"
    )]
    let t0 = Instant::now();
    let mut c = BackgroundCarries::default();
    c.typed(t0);
    let mut last = t0;
    for (n, wait) in [2, 4, 8, 16, 32, 60, 60, 60].into_iter().enumerate() {
        // The carry's answer and then the program's own wake each ran 20 s and ended on the
        // work; the break is read once more a second short of the ladder's carry, from a
        // start the loop read afresh: none of that wall time is any turn's work.
        c.ended(&timing, Duration::from_secs(20));
        c.ended(&timing, Duration::from_secs(20));
        let began = last + wait * MIN - Duration::from_secs(1);
        let due = c.due(&timing, began);
        assert_eq!(due, last + wait * MIN, "carry {} waits {wait} min", n + 2);
        assert!(due <= last + timing.short_backoff_max, "the cap carries it");
        c.typed(due);
        last = due;
    }
    assert_eq!(c.streak(), 9);
    // A wake that ran min_work: its break goes at once and the ladder starts again.
    c.ended(&timing, timing.min_work);
    let began = last + 3 * MIN;
    assert_eq!(c.due(&timing, began), began);
    // The carry held at that break, a short wake after it takes nothing back.
    c.ended(&timing, Duration::from_secs(20));
    let began = began + MIN;
    assert_eq!(c.due(&timing, began), began, "real work is sticky");
    c.typed(began);
    assert_eq!(c.streak(), 1, "real work starts the ladder again");
}
