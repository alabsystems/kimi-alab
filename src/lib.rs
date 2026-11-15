// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! kimi-alab: the ALab harness for Moonshot AI's Kimi Code CLI.
//!
//! It runs the UNMODIFIED vendor `kimi`, resolved from the atpkg store, as its child on the
//! inherited terminal; passes every argument through (after, for a launch through atpkg's
//! `run --`, what atpkg's launcher does: [`launch`]); follows the child's signals and stops;
//! exits with the child's own status; and, when atpkg makes a different `kimi` build current
//! while the session runs, resumes the same session on it at a safe point without the harness
//! process exiting. A new build of the harness itself crosses the same safe point by `exec`,
//! preserving the child and its session state through [`handoff`]. It never ships, forks or
//! patches vendor bytes.
//!
//! Modules, in the order a run uses them: [`cli`] (the harness's own words), [`launch`] (what
//! atpkg's `kimi` launcher does before the vendor starts, for a launch through `run --`),
//! [`store`] (the vendor binary, from the atpkg store only), [`kimi`] (the vendor profile:
//! resume grammar, session locks), [`supervise`] (the child, its signals, the move),
//! [`safepoint`] with [`tty`] and [`proc`] (when a move is allowed), [`journal`], and [`aterm`]
//! (this harness's side of its link to aterm). The link itself, the claim with its guards, the
//! claim held, lost and claimed again, aterm's rule for whose exit it was, and the crash back-off,
//! is the shared library `alab-wire`; the harness claims at contract 4 alone.

#![cfg(unix)]

pub mod aterm;
pub mod cli;
pub mod config;
pub mod env;
pub mod handoff;
pub mod journal;
pub mod kimi;
pub mod launch;
pub mod proc;
pub mod safepoint;
pub mod store;
pub mod supervise;
pub mod tty;
