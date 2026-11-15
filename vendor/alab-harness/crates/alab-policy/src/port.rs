// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! What a decider asks of the agent it decides for, where agents differ.
//!
//! A decider takes its readings as data. A fact that is the agent's own (how its screen shows a
//! thing, which a harness knows by the agent it drives) it asks through [`Agent`], which each
//! harness implements for its agent. A method lands here in the step where a decider in this
//! crate first calls it.

use std::time::Duration;

/// The agent a harness supervises, as this crate's deciders ask it for the facts that differ
/// from one agent to the next. [`crate::claude_code`] holds Claude Code's answers.
pub trait Agent {
    /// The step to which this agent's done row shows a turn of `work`.
    fn done_row_step(&self, work: Duration) -> Duration;
}
