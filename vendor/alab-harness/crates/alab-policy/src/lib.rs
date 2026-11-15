// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! `alab-policy`: the deciders every ALab harness shares.
//!
//! aterm's supervisor decides what a harness types, approves and waits for: whether a shell
//! command only reads, how long to back off, what a turn's last message asks of a person. Every
//! harness needs the same answers, so the deciders move here out of aterm, one tracker step at a
//! time, and aterm and each harness link this crate instead of keeping a copy.
//!
//! A decider takes its readings as data and returns its answer: this crate has no dependencies,
//! no `unsafe` code and no input or output. Its `clippy.toml` refuses the std doors to files,
//! processes, the environment, the network, the clock and sleep; a caller passes what it read
//! and the time it means.
//!
//! # Where each module came from
//!
//! Each module is aterm's code at the revision in this crate's newest `Moved-From:` or
//! `Synced-To:` commit trailer. A move is two commits. The first copies aterm's code unchanged:
//! a whole file keeps aterm's blob, and an item cut repeats aterm's lines at the file and line
//! ranges that commit names. The second changes `use` lines, visibility, docs and `cfg`s, and
//! puts in the seams the HX-21 design fixes, which the table's last column names. A whole file
//! then resyncs by a three-way merge of the aterm file, and an item cut item by item (aterm's
//! paths below are relative to its root).
//!
//! | Module | From aterm | Moved as |
//! |---|---|---|
//! | [`text`] | `crates/aterm-types/src/hidden_char.rs` (`is_hidden_char`), `crates/aterm-phase/src/reader.rs` (`MESSAGE_GAP`, `join_messages`, here [`text::join`]), `crates/aterm-agent/src/lib.rs` (`parse_uint`) | item copies; aterm keeps its originals |
//! | [`tab_id`] | `crates/aterm-types/src/tab_id.rs` (`ALPHABET`, `SHORT_LEN`, `is_tab_id`) | the grammar alone, without aterm's `TabId`; aterm keeps its original |
//! | [`hash`] | `crates/aterm-hash/src/fnv.rs` (`fnv1a_64`) | FNV-1a/64 with its byte loop written out (aterm's runs astream-wire's); aterm keeps its original |
//! | [`token`] | `crates/aterm-uds/src/process.rs` (`Identity::{token, parse_shaped}`), `crates/aterm-agent/src/harness/leftover.rs` (`token_of`) | a new [`token::ProcessToken`]; aterm keeps `Identity` and converts |
//! | [`ladder`] | `crates/aterm-agent/src/supervise/ladder.rs`, `ladder_tests.rs` | whole file |
//! | [`classify`] | `crates/aterm-agent/src/supervise/classify.rs`, `classify_tests.rs` | whole file |
//! | [`words`] | `crates/aterm-agent/src/supervise/policy/turn_end.rs` (`SAID_CAP`, `cut_around`, `no_repository!`, `NO_REPOSITORY`, `DESTRUCTIVE_WORDS`, `force_push`, `inflects`, `units`, `VERBATIM_UNITS`, `with_rules`, `fit_units`, `RULES_OPEN`) | item cuts; aterm's `turn_end.rs` keeps the rest and imports these |
//! | [`credential_need`] | `crates/aterm-agent/src/supervise/policy/credential_need.rs`, `credential_need_tests.rs` | whole file |
//! | [`needs`] | `crates/aterm-agent/src/supervise/policy/needs.rs`, `needs_tests.rs`, `fixtures/needs-table.txt` | whole file |
//! | [`go_ahead`] | `crates/aterm-agent/src/supervise/policy/go_ahead.rs`, `go_ahead_tests.rs`, `fixtures/go-ahead-table.txt` | whole file; its test module, whose table and corpora aterm's tests read, is `go_ahead::testing` (feature `testing`) |
//! | [`land_check`] | `crates/aterm-agent/src/supervise/policy/land_check.rs`, `land_check_tests.rs` | whole file but `hint_clause` and `cut_units`, which read aterm's open-work reader and stay with it |
//! | [`cruise_carry`] | `crates/aterm-agent/src/supervise/policy/cruise_carry.rs`, `cruise_carry_tests.rs` | whole file but `BACKGROUND_CARRY`, `carry_words` and `recovery_words`, which aterm's `act_words` fits and which stay with it; the timings are a new [`cruise_carry::CarryTiming`] in place of aterm's `TurnEndTiming` |
//! | [`guard`] | `crates/aterm-agent/src/supervise/policy/guard.rs` | whole file; its tests (`guard_tests.rs`) need aterm's row matcher and stay there |
//! | [`port`] | none | new: the [`port::Agent`] trait a decider asks an agent's own facts through |
//! | [`sleeps`] | `crates/aterm-agent/src/supervise/policy/sleeps.rs`, `sleeps_tests.rs` | whole file but `done_row_step`, now [`claude_code::done_row_step`], which [`sleeps::SleepBook::in_turn`] asks of a [`port::Agent`]; and `Boot::{kernel, of, text}`, which spell a boot id by `astream_sys` and stay in aterm as its `BootSpelling`, with the two tests that spell one |
//! | [`claude_code`] | `crates/aterm-agent/src/supervise/policy/sleeps.rs` (`done_row_step`), `git_config.rs` (`claude_dir_at`), `git_config_tests.rs` (its test), `shell_startup.rs` (`CLAUDE_PART`, and the rest of its pure half as [`claude_code::shell_startup`]) and `shell_startup_tests.rs` | item cuts; aterm keeps `WorkerEnv::claude_dir`, which reads the worker's environment and account home and calls [`claude_code::claude_dir_at`]; and `shell_startup`'s live half, in its file of the same name: `read`'s checks and its gathering of a [`claude_code::shell_startup::StartupInputs`], which it hands to [`claude_code::shell_startup::ShellStartup::assemble`] (`snapshots`, `hook_env_scripts`, `zshenv`, `bash_env`, `tool_kinds`, the `MAX_*` bounds), with the tests that read files or the worker's environment |
//! | [`git_config`] | `crates/aterm-agent/src/supervise/policy/git_config.rs`, `git_config_tests.rs`, and `approval.rs` (`rooted`) | item cuts: the pure half. aterm keeps the live half, which starts git to read a [`git_config::GitView`] (`WorkerEnv` with `digest`, `Probe`, `git_output*`, `is_executable`, `claude_dir`), in its file of the same name, with the tests that run real git or read a `WorkerEnv` |
//! | `rng` (tests only) | `crates/aterm-spec/src/rng.rs` | whole file: the deterministic generator the moved tests of generated inputs draw from, so they draw the same inputs as in aterm |

#![forbid(unsafe_code)]

/// aterm's harness mark (its `harness_mark!()`), which this crate's tests write a marked text
/// with, so that each text a test checks is the one aterm types. Defined before the modules, so
/// every test module sees it.
#[cfg(test)]
macro_rules! aterm_mark {
    () => {
        "[aterm harness]"
    };
}

pub mod classify;
pub mod claude_code;
pub mod credential_need;
pub mod cruise_carry;
pub mod git_config;
pub mod go_ahead;
pub mod guard;
pub mod hash;
pub mod ladder;
pub mod land_check;
pub mod needs;
pub mod port;
#[cfg(test)]
mod rng;
pub mod sleeps;
pub mod tab_id;
pub mod text;
pub mod token;
pub mod words;
