// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! kimi-alab: the ALab harness for Moonshot AI's Kimi Code CLI.
//!
//! It runs the UNMODIFIED vendor `kimi`, resolved from the atpkg store, as its child on the
//! inherited terminal; passes every argument through; follows the child's signals and stops;
//! exits with the child's own status; and, when atpkg makes a different `kimi` build current
//! while the session runs, resumes the same session on it at a safe point without the harness
//! process exiting. It never ships, forks or patches vendor bytes.
//!
//! Modules, in the order a run uses them: [`cli`] (the harness's own words), [`store`] (the
//! vendor binary, from the atpkg store only), [`kimi`] (the vendor profile: resume grammar,
//! session locks), [`supervise`] (the child, its signals, the move), [`safepoint`] with
//! [`tty`] and [`proc`] (when a move is allowed), [`journal`], and [`aterm`] (the contract-3
//! client boundary, inert in 0.1).

#![cfg(unix)]

pub mod aterm;
pub mod cli;
pub mod config;
pub mod env;
pub mod journal;
pub mod kimi;
pub mod proc;
pub mod safepoint;
pub mod store;
pub mod supervise;
pub mod tty;
