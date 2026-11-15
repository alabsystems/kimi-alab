// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `alab-wire`: an ALab harness's link to aterm.
//!
//! Every harness that runs a vendor CLI inside an aterm tab needs the same
//! half: find the session's control socket, authenticate, claim the duties it
//! performs at the highest contract both sides speak (4, else 3) with every
//! guard, act only on what aterm granted, keep that claim as aterm restarts
//! and its policy moves, judge a safe point and a crash
//! relaunch by aterm's facts, and back off a crashing child on aterm's ladder.
//! This crate is that half, generic over the harness: everything it says
//! about the harness comes from the [`Config`] the caller passes.
//!
//! * [`wire`]: the transport ([`Locator`], [`Connection`], [`EventReader`]).
//! * [`claim`]: the claim's texts and replies, and the safe-point judge.
//! * [`link`]: the claim sequence and the [`Link`] state machine.
//! * [`kernel`]: a child's kernel start time as aterm checks `child=`
//!   ([`kernel::start_us`], [`ChildProcess::of`]).
//! * [`relaunch`]: whose exit it was ([`relaunch::on_exit`],
//!   [`relaunch::permitted`], [`relaunch::abnormal_exit`]) and the crash
//!   ladder ([`relaunch::Ladder`]: 1 s, 10 s, 60 s, then 10 minutes).
//! * `fake` (cargo feature `fake`): a fake aterm for a harness's own tests.
//!
//! It supervises no process: nothing here forks, execs, signals or reaps a
//! vendor. The one process fact it reads is a child's kernel start, when a
//! harness asks for it. It keeps no clock (callers pass `now_ms`) and writes no journal
//! ([`Link::take_notes`]).

pub mod claim;
pub mod config;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod kernel;
pub mod link;
pub mod relaunch;
pub mod wire;

pub use claim::{
    Account, Child, ChildProcess, Contract, Grant, Policy, Restart, SafePoint, Scope, Status,
    Warrant,
};
pub use config::{Config, ConfigError, EnvNames, HostedMark};
pub use link::{Drained, Facts, Failure, Link, Reading, Relaunch, Verdict};
pub use wire::{Connection, EventReader, EventUpdate, Fields, Locator};
