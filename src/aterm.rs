// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! THE ATERM CLIENT BOUNDARY — INERT IN 0.1. NEXT MILESTONE.
//!
//! What goes here (contract v3, as aterm's harness split survey designs it; aterm keeps the
//! contract, this module speaks it):
//!
//! - **Claim.** Over the session's control socket, `meta set supervisor kimi-alab@<pid>
//!   contract=3 scope=relaunch,upgrade version=<version> [vendor=<vendor version>]` (grok-alab's
//!   grammar: `version=` is [`crate::cli::VERSION`], `vendor=` the `--version` line's vendor
//!   version, absent when that is unknown, and re-announced after a move), proven by the kernel: the claimant leads the terminal's foreground
//!   process group, which this harness does because it runs the vendor child in its own group
//!   (codex-alab's contract v2 is the precedent: `alab/CONTRACT.md` §1, and aterm's
//!   `crates/aterm-gui/src/self_claim.rs`, whose `SELF_CLAIM_PREFIX` today admits only
//!   `codex-alab@`).
//! - **Fence.** Anything typed into the session goes through aterm's `if-supervisor=<holder>`
//!   fence (`crates/aterm-gui/src/control_input.rs`), so a stale claim can never type.
//! - **Idle attestation.** aterm reads the screen (its Kimi reader,
//!   `crates/aterm-phase/src/kimi.rs`) and, at a turn end with an empty composer, raises the
//!   attestation this harness now takes as `SIGUSR1` ([`crate::safepoint`]).
//! - **Program mapping.** aterm maps `kimi-alab` to `Program::Kimi` through `aterm-agent-names`,
//!   since `status program=` names the group leader. This part is live; the duties above are not.
//!
//! aterm does not advertise contract 3 today (its `harness policy` reply has no `contract=`
//! key, and Kimi is `supervisable() == false` there), so [`advertised`] answers
//! [`Advertised::None`] without touching any socket and [`may_claim`] is false: this harness
//! never claims supervision of an aterm session in 0.1.

/// The claim holder this harness will announce: `kimi-alab@<pid>`.
pub const HOLDER_PREFIX: &str = "kimi-alab@";

/// The only contract this harness will speak to aterm.
pub const CONTRACT: u32 = 3;

/// What the hosting aterm advertises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advertised {
    /// No contract (or not inside aterm).
    None,
    /// Contract `n`.
    Contract(u32),
}

/// The duties a claim may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Relaunching the vendor child.
    Relaunch,
    /// Moving the vendor child onto a new build.
    Upgrade,
}

/// What the hosting aterm advertises. Always [`Advertised::None`] in 0.1: the probe is the next
/// milestone and nothing is sent to aterm.
pub fn advertised() -> Advertised {
    Advertised::None
}

/// Whether the harness may claim supervision under `advertised`: only under exactly
/// [`CONTRACT`].
pub fn may_claim(advertised: Advertised) -> bool {
    advertised == Advertised::Contract(CONTRACT)
}

/// The claim this harness would make, for `pid` (used by the next milestone; printed by
/// `--alab-which`).
pub fn holder(pid: u32) -> String {
    format!("{HOLDER_PREFIX}{pid}")
}

/// The scopes the first claim will name.
pub const FIRST_SCOPES: &[Scope] = &[Scope::Relaunch, Scope::Upgrade];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_claimed_without_contract_3() {
        assert_eq!(advertised(), Advertised::None);
        assert!(!may_claim(advertised()));
        assert!(!may_claim(Advertised::Contract(2)));
        assert!(may_claim(Advertised::Contract(3)));
        assert_eq!(holder(42), "kimi-alab@42");
        assert_eq!(FIRST_SCOPES, &[Scope::Relaunch, Scope::Upgrade]);
    }
}
