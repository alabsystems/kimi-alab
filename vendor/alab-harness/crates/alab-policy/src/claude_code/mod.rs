// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Claude Code's facts: what a decider needs to know of Claude Code's screen that another agent
//! shows differently. A harness that supervises Claude Code answers [`crate::port::Agent`] with
//! them.
//!
//! Cut out of aterm's supervisor: `done_row_step` from `supervise/policy/sleeps.rs`,
//! `claude_dir_at` from `supervise/policy/git_config.rs`, and `CLAUDE_PART` and the rest of the
//! pure half of `supervise/policy/shell_startup.rs`, which judges what Claude Code's Bash tool's
//! shell starts with ([`shell_startup`]).

pub mod shell_startup;

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::git_config::normalise;

/// How finely Claude Code's done row shows a turn's span of `work` (`✻ Baked for 30m 3s`, `✻ Crunched for 2h 58m`): to the second, and from an hour on to the minute.
#[must_use]
pub fn done_row_step(work: Duration) -> Duration {
    if work >= Duration::from_secs(3600) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(1)
    }
}

/// The Claude Code directory of a process whose `CLAUDE_CONFIG_DIR` is `config_dir`.
pub fn claude_dir_at(
    config_dir: Option<&str>,
    home: Option<&str>,
    cwd: &Path,
    account_home: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf, String> {
    let dir = match (config_dir, home.filter(|h| !h.is_empty())) {
        (Some(dir), _) => PathBuf::from(dir),
        (None, Some(home)) => Path::new(home).join(".claude"),
        (None, None) => account_home()
            .ok_or("no CLAUDE_CONFIG_DIR, no HOME, and no home for its account")?
            .join(".claude"),
    };
    Ok(normalise(&cwd.join(dir)))
}

/// The line Claude Code writes before its own additions to a snapshot (the
/// `rg` fallback, its `find`/`grep`/`pkill` shadows, `PATH`).
pub const CLAUDE_PART: &str = "# Check for rg availability";

#[cfg(test)]
mod tests;
