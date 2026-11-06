// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! THE SAFE POINT: when the harness may stop a running Kimi to move it onto a new build.
//!
//! The ruling is "turn ended, composer empty, no person typing", and fail closed: never stop a
//! busy child. The harness inherits the terminal and does not proxy it, so it cannot read the
//! screen; aterm's own Kimi reader does not yet call any phase authoritative either ("a busy
//! turn is unmeasured", aterm `crates/aterm-phase/src/kimi.rs:11`). So in 0.1 the decision has
//! two halves, and BOTH must hold:
//!
//! 1. **An idle attestation** — `SIGUSR1` to the harness — from something that can see the
//!    screen (a person today; aterm under contract 3, the next milestone) saying the turn has
//!    ended, the composer is empty and no background task runs. It is good for
//!    [`Knobs::attest_valid`].
//! 2. **The harness's own backstops**, over a quiet window of [`Knobs::quiet`] measured from the
//!    attestation: no keystroke was read and nothing was drawn since the attestation (the tty's
//!    own times, [`crate::tty`]), the child's process tree did not change, and the tree used no
//!    more than [`Knobs::cpu_budget`] of CPU.
//!
//! Any reading that cannot be taken counts as busy. A busy verdict ends that attestation; the
//! harness then waits for a new one and never retries on its own.

use std::time::{Duration, Instant, SystemTime};

use crate::proc::Tree;
use crate::tty::TtyTimes;

/// The timing of the decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Knobs {
    /// How long, from the attestation, the terminal and the process tree must stay quiet.
    pub quiet: Duration,
    /// How long an attestation stays good.
    pub attest_valid: Duration,
    /// The most CPU the child's tree may use over the quiet window.
    pub cpu_budget: Duration,
    /// How far a tty time may lag what it records ([`crate::tty::GRANULARITY`]).
    pub tty_granularity: Duration,
}

impl Knobs {
    /// The defaults: 10 s quiet (beyond Linux's 8 s tty granularity), attestations good for
    /// two minutes, and 2% of the window in CPU.
    pub fn defaults() -> Self {
        Self::with_quiet(Duration::from_secs(10), Duration::from_secs(120))
    }

    /// Knobs for a quiet window of `quiet` (clamped to at least 200 ms) and attestations good
    /// for `attest_valid` (at least the window); the CPU budget is 2% of the window, at least
    /// 20 ms.
    pub fn with_quiet(quiet: Duration, attest_valid: Duration) -> Self {
        let quiet = quiet.max(Duration::from_millis(200));
        Self {
            quiet,
            attest_valid: attest_valid.max(quiet),
            cpu_budget: (quiet / 50).max(Duration::from_millis(20)),
            tty_granularity: crate::tty::GRANULARITY,
        }
    }
}

/// An idle attestation: when it arrived, on both clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attestation {
    /// Wall-clock time, compared with the tty's times.
    pub wall: SystemTime,
    /// Monotonic time, for the windows.
    pub mono: Instant,
}

impl Attestation {
    /// An attestation arriving now.
    pub fn now() -> Self {
        Self {
            wall: SystemTime::now(),
            mono: Instant::now(),
        }
    }
}

/// One reading of everything the backstops look at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    /// When it was taken.
    pub at: Instant,
    /// The terminal's times, if there is a terminal.
    pub tty: Option<TtyTimes>,
    /// The child's process tree, if it could be read.
    pub tree: Option<Tree>,
}

/// Why the child counts as busy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Busy {
    /// No terminal: nothing can witness that nobody is typing.
    NoTerminal,
    /// A keystroke was read at or after the attestation.
    Typing,
    /// The screen was drawn at or after the attestation.
    Drawing,
    /// The child's process tree could not be read.
    Unreadable,
    /// A process joined or left the child's tree during the window.
    TreeChanged,
    /// The tree used this much CPU during the window, over the budget.
    Working(Duration),
}

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTerminal => {
                write!(f, "no terminal, so nothing witnesses that nobody is typing")
            }
            Self::Typing => write!(f, "a keystroke was read after the attestation"),
            Self::Drawing => write!(f, "the screen was drawn after the attestation"),
            Self::Unreadable => write!(f, "the child's process tree could not be read"),
            Self::TreeChanged => write!(f, "a process joined or left the child's tree"),
            Self::Working(cpu) => write!(f, "the child's tree used {} ms of CPU", cpu.as_millis()),
        }
    }
}

/// What the backstops say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing busy yet, but the window is not complete.
    Wait,
    /// The window is complete and quiet: the child may be stopped.
    Safe,
    /// The child is busy: this attestation ends.
    Busy(Busy),
}

/// The backstops' verdict for attestation `att`, from the reading `start` taken when the window
/// opened and the reading `now` taken just now.
pub fn judge(att: &Attestation, start: &Probe, now: &Probe, knobs: &Knobs) -> Verdict {
    let Some(tty) = now.tty else {
        return Verdict::Busy(Busy::NoTerminal);
    };
    // A tty time `t` may record an event as late as `t + granularity`; only a bucket that ends
    // before the attestation proves the event came before it.
    let before = |t: SystemTime| {
        t.checked_add(knobs.tty_granularity)
            .is_some_and(|end| end < att.wall)
    };
    if !before(tty.input) {
        return Verdict::Busy(Busy::Typing);
    }
    if !before(tty.output) {
        return Verdict::Busy(Busy::Drawing);
    }
    let (Some(first), Some(last)) = (&start.tree, &now.tree) else {
        return Verdict::Busy(Busy::Unreadable);
    };
    if first.pids != last.pids {
        return Verdict::Busy(Busy::TreeChanged);
    }
    let used = last.cpu.saturating_sub(first.cpu);
    if used > knobs.cpu_budget {
        return Verdict::Busy(Busy::Working(used));
    }
    let window = now.at.saturating_duration_since(start.at);
    let since_attest = now.at.saturating_duration_since(att.mono);
    if window < knobs.quiet || since_attest < knobs.quiet {
        return Verdict::Wait;
    }
    Verdict::Safe
}

/// Whether attestation `att` is still good at `now`.
pub fn fresh(att: &Attestation, now: Instant, knobs: &Knobs) -> bool {
    now.saturating_duration_since(att.mono) <= knobs.attest_valid
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn knobs() -> Knobs {
        Knobs {
            quiet: Duration::from_secs(10),
            attest_valid: Duration::from_secs(60),
            cpu_budget: Duration::from_millis(200),
            tty_granularity: Duration::ZERO,
        }
    }

    fn tree(pids: &[i32], cpu_ms: u64) -> Tree {
        Tree {
            pids: pids.iter().copied().collect::<BTreeSet<_>>(),
            cpu: Duration::from_millis(cpu_ms),
        }
    }

    struct Case {
        att: Attestation,
        t0: Instant,
    }

    impl Case {
        fn new() -> Self {
            let t0 = Instant::now();
            Self {
                att: Attestation {
                    wall: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
                    mono: t0,
                },
                t0,
            }
        }
        fn wall(&self, delta_s: i64) -> SystemTime {
            if delta_s >= 0 {
                self.att.wall + Duration::from_secs(delta_s.unsigned_abs())
            } else {
                self.att.wall - Duration::from_secs(delta_s.unsigned_abs())
            }
        }
        fn probe(&self, after_s: u64, input_s: i64, output_s: i64, t: Option<Tree>) -> Probe {
            Probe {
                at: self.t0 + Duration::from_secs(after_s),
                tty: Some(TtyTimes {
                    input: self.wall(input_s),
                    output: self.wall(output_s),
                }),
                tree: t,
            }
        }
    }

    #[test]
    fn a_quiet_window_after_an_attestation_is_safe() {
        let c = Case::new();
        let start = c.probe(0, -30, -2, Some(tree(&[10, 11], 500)));
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(5, -30, -2, Some(tree(&[10, 11], 520))),
                &knobs()
            ),
            Verdict::Wait
        );
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(10, -30, -2, Some(tree(&[10, 11], 600))),
                &knobs()
            ),
            Verdict::Safe
        );
    }

    #[test]
    fn typing_or_drawing_after_the_attestation_is_busy() {
        let c = Case::new();
        let start = c.probe(0, -30, -2, Some(tree(&[10], 0)));
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(3, 1, -2, Some(tree(&[10], 0))),
                &knobs()
            ),
            Verdict::Busy(Busy::Typing)
        );
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(3, 0, -2, Some(tree(&[10], 0))),
                &knobs()
            ),
            Verdict::Busy(Busy::Typing)
        );
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(3, -30, 2, Some(tree(&[10], 0))),
                &knobs()
            ),
            Verdict::Busy(Busy::Drawing)
        );
    }

    #[test]
    fn a_coarse_tty_clock_needs_the_whole_bucket_before_the_attestation() {
        let c = Case::new();
        let mut k = knobs();
        k.tty_granularity = Duration::from_secs(8);
        let start = c.probe(0, -30, -5, Some(tree(&[10], 0)));
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(10, -30, -5, Some(tree(&[10], 0))),
                &k
            ),
            Verdict::Busy(Busy::Drawing)
        );
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(10, -30, -9, Some(tree(&[10], 0))),
                &k
            ),
            Verdict::Safe
        );
    }

    #[test]
    fn no_terminal_or_no_tree_is_busy() {
        let c = Case::new();
        let start = c.probe(0, -30, -30, Some(tree(&[10], 0)));
        let mut headless = c.probe(10, -30, -30, Some(tree(&[10], 0)));
        headless.tty = None;
        assert_eq!(
            judge(&c.att, &start, &headless, &knobs()),
            Verdict::Busy(Busy::NoTerminal)
        );
        assert_eq!(
            judge(&c.att, &start, &c.probe(10, -30, -30, None), &knobs()),
            Verdict::Busy(Busy::Unreadable)
        );
        let blind = c.probe(0, -30, -30, None);
        assert_eq!(
            judge(
                &c.att,
                &blind,
                &c.probe(10, -30, -30, Some(tree(&[10], 0))),
                &knobs()
            ),
            Verdict::Busy(Busy::Unreadable)
        );
    }

    #[test]
    fn a_changed_tree_or_cpu_over_budget_is_busy() {
        let c = Case::new();
        let start = c.probe(0, -30, -30, Some(tree(&[10, 11], 1000)));
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(4, -30, -30, Some(tree(&[10, 11, 12], 1000))),
                &knobs()
            ),
            Verdict::Busy(Busy::TreeChanged)
        );
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(4, -30, -30, Some(tree(&[10], 1000))),
                &knobs()
            ),
            Verdict::Busy(Busy::TreeChanged)
        );
        assert_eq!(
            judge(
                &c.att,
                &start,
                &c.probe(4, -30, -30, Some(tree(&[10, 11], 1300))),
                &knobs()
            ),
            Verdict::Busy(Busy::Working(Duration::from_millis(300)))
        );
    }

    #[test]
    fn the_window_counts_from_both_the_attestation_and_the_first_reading() {
        let c = Case::new();
        let late_start = c.probe(8, -30, -30, Some(tree(&[10], 0)));
        assert_eq!(
            judge(
                &c.att,
                &late_start,
                &c.probe(12, -30, -30, Some(tree(&[10], 0))),
                &knobs()
            ),
            Verdict::Wait
        );
        assert_eq!(
            judge(
                &c.att,
                &late_start,
                &c.probe(18, -30, -30, Some(tree(&[10], 0))),
                &knobs()
            ),
            Verdict::Safe
        );
    }

    #[test]
    fn attestations_expire_and_knobs_clamp() {
        let c = Case::new();
        assert!(fresh(&c.att, c.t0 + Duration::from_secs(60), &knobs()));
        assert!(!fresh(&c.att, c.t0 + Duration::from_secs(61), &knobs()));
        let k = Knobs::with_quiet(Duration::from_millis(1), Duration::ZERO);
        assert_eq!(k.quiet, Duration::from_millis(200));
        assert_eq!(k.attest_valid, Duration::from_millis(200));
        assert_eq!(k.cpu_budget, Duration::from_millis(20));
        let d = Knobs::defaults();
        assert_eq!(
            (d.quiet, d.cpu_budget),
            (Duration::from_secs(10), Duration::from_millis(200))
        );
    }
}
