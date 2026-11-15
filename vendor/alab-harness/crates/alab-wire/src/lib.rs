// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `alab-wire`: an ALab harness's link to aterm.
//!
//! Every harness that runs a vendor CLI inside an aterm tab needs the same
//! half: find the session's control socket, authenticate, claim the duties it
//! performs at the highest contract aterm serves of those the harness declares
//! (4, else 3; [`Config::with_contracts`]) with every guard, act only on what
//! aterm granted, keep that claim as aterm restarts
//! and its policy moves, judge a safe point and a crash
//! relaunch by aterm's facts, and back off a crashing child on aterm's ladder.
//! This crate is that half, generic over the harness: everything it says
//! about the harness comes from the [`Config`] the caller passes.
//!
//! * [`wire`]: the transport ([`Locator`], [`Connection`], [`EventReader`]).
//! * [`claim`]: the claim's texts and replies, and the safe-point judge.
//! * [`link`]: the claim sequence and the [`Link`] state machine, which streams
//!   once aterm grants a contract-4 claim a session stream.
//! * [`kernel`]: a child's kernel start time as aterm checks `child=`
//!   ([`kernel::start_us`], [`ChildProcess::of`]).
//! * [`record`]: the session stream's records ([`Record`], [`Body`], [`Kind`]) and their codec
//!   ([`Record::line`], [`Record::read`]), and the verb that opens a stream
//!   ([`record::stream_request`], [`StreamReply`]). Portable: it names nothing of a platform.
//! * [`relaunch`]: whose exit it was ([`relaunch::on_exit`],
//!   [`relaunch::permitted`], [`relaunch::abnormal_exit`]) and the crash
//!   ladder ([`relaunch::Ladder`]: 1 s, 10 s, 60 s, then 10 minutes).
//! * [`reexec`]: the harness's re-exec of itself onto a newer build, with the
//!   same pid and child: the probe ([`reexec::probe`]), the child's kernel
//!   identity ([`reexec::Identity`]), the carry over one inherited pipe
//!   ([`reexec::Carry`], [`reexec::exec`], [`reexec::receive`]) and the caught
//!   signals held across it ([`reexec::block`], [`reexec::take_pending`]).
//! * [`stream`]: the session stream's client half, which [`Link`] drives: the
//!   records numbered, paced under aterm's budget and kept until aterm acknowledges
//!   them, and what aterm's records become ([`Received`], [`Incoming`], [`Hand`],
//!   [`SendError`], [`AnswerError`], [`Counts`]).
//! * [`run`]: the words aterm types to relaunch a harness on its conversation,
//!   `<tool> run [--account <label>] [--home <dir>] [--model <word>] -- <resume…>`
//!   ([`Run::parse`], [`Run::args`]).
//! * `fake` (cargo feature `fake`): a fake aterm for a harness's own tests.
//!
//! It supervises no process: nothing here forks, execs, signals or reaps a
//! vendor. The process facts it reads are a child's kernel start and, for a
//! re-exec, its kernel identity, when a harness asks for them; the one program
//! it starts is a harness build's probe, and the one it execs is that build,
//! in the harness's own process ([`reexec`]). It keeps no clock (callers pass
//! `now_ms`) and writes no journal ([`Link::take_notes`]).

pub mod claim;
pub mod config;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod kernel;
pub mod link;
pub mod record;
mod record_json;
pub mod reexec;
pub mod relaunch;
pub mod run;
pub mod stream;
pub mod wire;

pub use claim::{
    Account, Child, ChildProcess, Contract, Grant, Policy, Restart, SafePoint, Scope, Status,
    Warrant,
};
pub use config::{Config, ConfigError, EnvNames, HostedMark};
pub use link::{Drained, Facts, Failure, Link, Reading, Relaunch, Verdict};
pub use record::{Body, Decoded, Kind, LineError, Needs, Record, StreamReply};
pub use run::{Run, RunError};
pub use stream::{AnswerError, Counts, Hand, Incoming, Received, SendError};
pub use wire::{Connection, EventReader, EventUpdate, Fields, Locator};
