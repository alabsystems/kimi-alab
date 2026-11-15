// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Step-table and doubling retry back-off schedules.

use std::time::Duration;

/// Returns `base` doubled `n` times and clamped to `max` without overflow.
#[must_use]
pub fn doubling(base: Duration, n: u32, max: Duration) -> Duration {
    base.checked_mul(1u32.checked_shl(n).unwrap_or(u32::MAX))
        .unwrap_or(max)
        .min(max)
}

/// A table-driven schedule of retry pauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ladder<'a>(pub &'a [Duration]);

impl Ladder<'_> {
    /// Returns the pause at 0-based step `n`, clamping to the last entry, or `None` if empty.
    #[must_use]
    pub fn at(self, n: usize) -> Option<Duration> {
        self.0.get(n.min(self.0.len().checked_sub(1)?)).copied()
    }

    /// [`Self::at`] for a table that is never empty (a `const` schedule).
    #[must_use]
    pub fn step(self, n: usize) -> Duration {
        self.at(n).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "ladder_tests.rs"]
mod tests;
