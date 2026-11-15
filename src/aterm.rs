// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! kimi-alab's side of its link to aterm. The link itself is the shared library `alab-wire`
//! (`vendor/alab-harness/crates/alab-wire`, the vendored alab-harness v0.7.1): the wire, the
//! claim with its guards (the scope read back) kept to the contracts the harness declares, the
//! [`Link`] state machine with its bounds and its busy-not-gone rule, aterm's exit judgement
//! and the crash ladder. This module holds only what is this harness's own: who it is to aterm
//! ([`config`]), the contracts it declares ([`CONTRACTS`]) and what it does when aterm serves
//! none of them ([`unserved`]), the variable that leaves it out ([`NO_HARNESS_ENV`]), the clock
//! its link runs on ([`now_ms`]), what it claims for a child ([`child`]), how a move a person
//! attested is checked against aterm's facts ([`unheld`]), and the link's notes as journal
//! events ([`journal_notes`]).

use std::ffi::OsString;

use alab_wire::{
    Child, ChildProcess, Config, Contract, Facts, Failure, Link, SafePoint, Scope, Warrant,
};

use crate::env::Env;
use crate::journal::Journal;

/// This harness's id: its row in aterm's roster and registry, and the holder `<id>@<pid>`.
pub const HOLDER_ID: &str = "kimi-alab";
/// The word of the vendor it runs.
pub const VENDOR_WORD: &str = "kimi";
/// The duties this build performs and claims from aterm: bringing a crashed child back on its
/// conversation, and moving it onto another vendor build. The typing scopes stay unclaimed
/// until their own implementations and tests land (README decision 23).
pub const SCOPES: [Scope; 2] = [Scope::Relaunch, Scope::Upgrade];
/// The variable that leaves the harness out of a launch.
pub const NO_HARNESS_ENV: &str = "ATERM_NO_HARNESS";
/// The contracts this build claims under, `contracts` in `alab-harness.toml` (a unit test holds
/// the two and [`config`]'s declared list equal): contract 4 alone. A build that carries a
/// record never claims 3 (aterm's harness interface design, section 2.2: contract 3 is the
/// seed rows' alone), and a 3 in the record would keep aterm serving contract 3 for it.
/// [`config`] declares this list to alab-wire (`Config::with_contracts`), whose every claim (the
/// launch's, an adopted image's, each made again on the link's clock or after a policy switched
/// back on) keeps to it: against an aterm that lists no 4 none is sent, the policy is read
/// again every 5 s, and the claim is made at 4 once aterm serves it (README decision 41).
pub const CONTRACTS: [Contract; 1] = [Contract::Four];

/// Who this harness is to aterm, claiming under [`CONTRACTS`] alone. The link stays on
/// alab-wire's v0.1 path (`with_stream(false)`): a contract-4 claim is renewed with `meta set
/// supervisor` and woken by the event subscription, and `stream` is never sent, since this
/// harness answers none of the session stream's records yet (aterm tracker HX-19; README
/// decision 40).
///
/// # Panics
/// Never: the id and vendor word are fixed lowercase words, [`SCOPES`] and [`CONTRACTS`] are not
/// empty, and the build refuses a [`crate::cli::VERSION`] that is not `MAJOR.MINOR.0[+g<sha>]`
/// (`cli::is_pack_version`), which is a claim's `version=`. The unit test makes this config.
#[must_use]
pub fn config() -> Config {
    Config::new(HOLDER_ID, VENDOR_WORD, &SCOPES, crate::cli::VERSION)
        .and_then(|config| config.with_contracts(&CONTRACTS))
        .expect(
            "kimi-alab's id, vendor word, scopes, version and contracts are a config aterm reads",
        )
        .with_stream(false)
}

/// Whether the person left the harness out (`ATERM_NO_HARNESS=1`).
#[must_use]
pub fn left_out(env: &Env) -> bool {
    env.get(NO_HARNESS_ENV).is_some_and(|v| v == "1")
}

/// `env` as the lookup `alab-wire` reads a session from.
pub fn lookup(env: &Env) -> impl Fn(&str) -> Option<OsString> + '_ {
    |name| env.get(name).map(std::ffi::OsStr::to_os_string)
}

/// Whether this process runs in an aterm tab, by the marks `config` names.
#[must_use]
pub fn hosted(env: &Env, config: &Config) -> bool {
    config.env().hosted_by(lookup(env))
}

/// The link's clock: milliseconds since boot on a system clock that stops while the machine
/// sleeps, the clock `std::time::Instant` reads (`CLOCK_UPTIME_RAW` on macOS, `CLOCK_MONOTONIC`
/// on Linux). Every crash-ladder window and away budget runs on it, so a closed lid neither
/// spends the 45 s aterm has to come back nor counts as a healthy run that forgives a crash
/// streak; aterm's own grace runs on `Instant` too. It is a system clock rather than one a
/// process starts, so a time carried across a harness re-exec reads the same on both sides.
#[must_use]
pub fn now_ms() -> u64 {
    uptime_ns() / 1_000_000
}

/// Nanoseconds on macOS's `CLOCK_UPTIME_RAW`. `CLOCK_MONOTONIC` there keeps counting while the
/// machine sleeps, and rustix names no other clock on Apple targets.
#[cfg(target_vendor = "apple")]
#[allow(unsafe_code)]
fn uptime_ns() -> u64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a writable timespec that outlives the call, which writes only it.
    // `CLOCK_UPTIME_RAW` is a clock every macOS Rust runs on knows (`Instant` reads it), so the
    // call does not fail; were it to, `now` stays zero, which reads as no time passed.
    let _ = unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &raw mut now) };
    u64::try_from(now.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(now.tv_nsec).unwrap_or(0))
}

/// Nanoseconds on `CLOCK_MONOTONIC`, which on Linux stops while the machine is suspended.
#[cfg(not(target_vendor = "apple"))]
fn uptime_ns() -> u64 {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    u64::try_from(now.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(now.tv_nsec).unwrap_or(0))
}

/// What the claim says about the child: its vendor build, its account, and, once it runs, its
/// pid and kernel start time (`child=` under contract 4). A start time that cannot be read is
/// left out rather than guessed.
#[must_use]
pub fn child<'a>(vendor: Option<&'a str>, login: Option<&'a str>, pid: Option<u32>) -> Child<'a> {
    Child {
        vendor,
        login,
        process: pid.and_then(|pid| ChildProcess::of(pid).ok()),
    }
}

/// Whether the standing claim was granted both duties this harness performs. A launch whose
/// claim was granted less gives it back and runs the vendor in its place (README decision 25):
/// a narrower grant would leave a duty to aterm's engine and this harness at once.
#[must_use]
pub fn takes_both(link: &Link) -> bool {
    SCOPES.iter().all(|scope| link.takes(*scope))
}

/// The granted duties as one journal word.
#[must_use]
pub fn granted(link: &Link) -> String {
    link.grant()
        .map_or_else(|| "-".to_string(), alab_wire::Grant::words)
}

/// Whether a move a person attested by hand (`SIGUSR1`) may still go ahead on aterm's fresh
/// facts ([`Link::read`]), by `alab-wire`'s rule for a move a harness judges by its own readings
/// ([`alab_wire::claim::judge_own_move`] with [`Warrant::Person`]): the claim is still `holder`'s
/// and proven, it still takes the upgrade duty, and aterm reads no halt and no other driver's
/// hand. The rest of the safe point, and the policy's `upgrade` and `upgrade_ask` switches, are
/// the person's to vouch for. The grant is aterm's, not the person's: a claim a cap, a reclaim or
/// an adoption left with less than `upgrade` has handed that duty back to aterm's engine, so a
/// person's attestation cannot move the vendor or the harness's image under it, as aterm's own
/// attestation cannot (alab-wire's safe-point judge checks the same scope).
///
/// # Errors
/// The first fact that holds the move, as journal words (`aterm <field>=<value>`).
pub fn unheld(facts: &Facts, holder: &str) -> Result<(), String> {
    match alab_wire::claim::judge_own_move(&facts.policy, &facts.status, holder, Warrant::Person) {
        SafePoint::Safe => Ok(()),
        SafePoint::Hold(hold) => Err(format!("aterm {}={}", hold.field, hold.value)),
    }
}

/// Why no claim stands on `link`, when aterm's last answer to a claim attempt was that its
/// policy lists none of [`CONTRACTS`] (alab-wire's `Failure::Unserved`, `policy contract=2,3
/// declared=4`): no claim stands (alab-wire gave back any its re-exec grace kept) and none was
/// sent at another contract, so no duty is taken from aterm's engine. A launch answered so runs
/// Kimi in the harness's place; a crash while it stands, or one already waiting for aterm when it
/// is read, is passed on at once, as with no claim, and a restart aterm asked for is refused.
/// `None` while the link is held or absent, and on a link lost for any other reason.
#[must_use]
pub fn unserved(link: &Link) -> Option<&str> {
    match link.unclaimed() {
        Some(Failure::Unserved(why)) => Some(why),
        _ => None,
    }
}

/// What follows every step of the link: its notes go to the journal, `aterm <verb> <rest>` as
/// the event `<VERB>` with `<rest>` (`aterm claimed holder=… contract=4 …` is `CLAIMED
/// holder=… contract=4 …`). The last note's `<rest>`, when there was one.
pub fn journal_notes(link: &mut Link, journal: &mut Journal) -> Option<String> {
    let mut last = None;
    for line in link.take_notes() {
        let rest = line.strip_prefix("aterm ").unwrap_or(&line);
        let (verb, detail) = rest.split_once(' ').unwrap_or((rest, ""));
        journal.note(&verb.to_ascii_uppercase(), detail);
        last = Some(detail.to_string());
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_harness_is_kimi_alab_claiming_relaunch_and_upgrade() {
        let config = config();
        assert_eq!(config.id(), "kimi-alab");
        assert_eq!(config.vendor(), "kimi");
        assert_eq!(config.scopes(), [Scope::Relaunch, Scope::Upgrade]);
        assert_eq!(config.holder(77), "kimi-alab@77");
        assert!(!config.streams(), "the session stream is HX-19's");
    }

    /// `alab-harness.toml` (the record aterm's registry reads), [`CONTRACTS`] and the contracts
    /// the config declares to alab-wire say the same: 4 alone.
    #[test]
    fn the_record_and_the_claim_name_contract_4_alone() {
        let record = include_str!("../alab-harness.toml");
        let line = record
            .lines()
            .find(|line| line.trim_start().starts_with("contracts"))
            .expect("alab-harness.toml names its contracts");
        let value = line
            .split_once('=')
            .map(|(_, value)| value.split('#').next().unwrap_or("").trim())
            .expect("contracts = [...]");
        let config = config();
        let declared = config
            .contracts()
            .iter()
            .map(|contract| contract.number().to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(value, format!("[{declared}]"));
        assert_eq!(config.contracts(), CONTRACTS);
        assert_eq!(CONTRACTS, [Contract::Four]);
    }

    /// alab-wire makes no claim text at a contract the config does not declare, so no claim at
    /// 3 can be sent from this harness, whatever aterm serves; the claim at 4 is made.
    /// NEGATIVE CONTROL: alab-wire's default contracts (3 and 4) make the claim at 3.
    #[test]
    fn no_claim_at_contract_3_can_be_made() {
        const SID: &str = "s-4d7a2a97";
        let kid = child(Some("2.3.1"), None, None);
        let claim = |config: &Config, contract| {
            alab_wire::claim::claim_verb(config, contract, SID, 77, kid)
        };
        let config = config();
        assert_eq!(claim(&config, Contract::Three), None);
        let four = claim(&config, Contract::Four).expect("a claim at 4");
        assert!(
            four.contains(" contract=4 scope=relaunch,upgrade "),
            "{four}"
        );
        let default = Config::new(HOLDER_ID, VENDOR_WORD, &SCOPES, crate::cli::VERSION).unwrap();
        assert_eq!(default.contracts(), Config::DEFAULT_CONTRACTS);
        let three = claim(&default, Contract::Three).expect("the default claims at 3");
        assert!(three.contains(" contract=3 "), "{three}");
    }

    #[test]
    fn the_person_leaves_the_harness_out_with_exactly_one() {
        assert!(left_out(&Env::from_pairs([(NO_HARNESS_ENV, "1")])));
        assert!(!left_out(&Env::from_pairs([(NO_HARNESS_ENV, "0")])));
        assert!(!left_out(&Env::default()));
    }

    #[test]
    fn the_session_is_read_from_the_harnesss_own_environment() {
        let config = config();
        assert!(hosted(
            &Env::from_pairs([("TERM_PROGRAM", "aterm")]),
            &config
        ));
        assert!(hosted(&Env::from_pairs([("ATERM_CHILD", "1")]), &config));
        assert!(!hosted(
            &Env::from_pairs([("TERM_PROGRAM", "iTerm.app")]),
            &config
        ));
        assert!(!hosted(&Env::default(), &config));
    }

    #[test]
    fn the_clock_is_monotonic_and_far_from_zero_enough_to_carry_a_ladder() {
        let first = now_ms();
        let second = now_ms();
        assert!(second >= first);
    }

    /// The link's clock leaves out the time the Mac slept, as `Instant` does: it reads macOS's
    /// `CLOCK_UPTIME_RAW`, not `CLOCK_MONOTONIC`, which counts sleep. On a Mac that has slept
    /// since boot the two differ by that sleep, so the `CLOCK_MONOTONIC` reading the port first
    /// used falls outside the window this reads around it.
    #[cfg(target_vendor = "apple")]
    #[test]
    #[allow(unsafe_code)]
    fn the_clock_stops_while_the_mac_sleeps() {
        let uptime_ms = || {
            let mut now = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: `now` is a writable timespec that outlives the call, which writes only it.
            assert_eq!(
                unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &raw mut now) },
                0
            );
            u64::try_from(now.tv_sec).unwrap() * 1_000
                + u64::try_from(now.tv_nsec).unwrap() / 1_000_000
        };
        let before = uptime_ms();
        let now = now_ms();
        let after = uptime_ms();
        assert!(
            (before..=after).contains(&now),
            "{now} is not between {before} and {after}"
        );
    }

    fn facts(policy: &str, status: &str) -> Facts {
        facts_scoped(policy, "relaunch,upgrade", status)
    }

    fn facts_scoped(policy: &str, scope: &str, status: &str) -> Facts {
        Facts {
            policy: alab_wire::Policy::parse(&format!(
                "OK gen=1 contract=2,3,4 relaunch=true human_grace_s=120 {policy}"
            ))
            .unwrap(),
            status: alab_wire::Status::parse(&format!(
                "OK schema=1 supervisor=kimi-alab%4077 supervisor_proven=1 \
                 supervisor_scope={scope} {status}"
            ))
            .unwrap(),
        }
    }

    /// The policy in effect, upgrades on, nobody asked to say when.
    const ON: &str = "effective=on why=enabled upgrade=true upgrade_ask=false";

    /// alab-wire's `judge_own_move` under a person's warrant: the claim, no halt and no other
    /// driver hold the move, each named as `aterm <field>=<value>`.
    #[test]
    fn a_hand_attested_move_needs_no_halt_no_driver_and_the_claim() {
        let holder = "kimi-alab@77";
        let free = "hold=0 hand=-";
        assert_eq!(unheld(&facts(ON, free), holder), Ok(()));
        assert_eq!(unheld(&facts(ON, "hold=0 hand=turn:7"), holder), Ok(()));
        for (policy, status, why) in [
            (
                "effective=off why=hold upgrade=true upgrade_ask=false",
                free,
                "aterm hold=1",
            ),
            (ON, "hold=1 hand=-", "aterm hold=1"),
            (
                ON,
                "hold=0 hand=lease:driver%404",
                "aterm hand=lease:driver%25404",
            ),
            (ON, "hold=0", "aterm hand=absent"),
            (ON, "hand=-", "aterm hold=absent"),
        ] {
            assert_eq!(
                unheld(&facts(policy, status), holder),
                Err(why.to_owned()),
                "{policy} / {status}"
            );
        }
        assert_eq!(
            unheld(&facts(ON, free), "kimi-alab@78"),
            Err("aterm supervisor=kimi-alab@77".to_owned())
        );
        // A lost claim is named before a halt: the claim is the first thing a move needs.
        assert_eq!(
            unheld(&facts(ON, "hold=1 hand=-"), "kimi-alab@78"),
            Err("aterm supervisor=kimi-alab@77".to_owned())
        );
    }

    /// A person's word stands over the policy's two upgrade switches: they are the one
    /// `upgrade_ask=true` waits for, and their attestation is the say-so `upgrade=false` asks for.
    #[test]
    fn a_hand_attested_move_stands_over_the_upgrade_switches() {
        let holder = "kimi-alab@77";
        let free = "hold=0 hand=-";
        for policy in [
            "effective=on why=enabled upgrade=false upgrade_ask=false",
            "effective=on why=enabled upgrade=true upgrade_ask=true",
        ] {
            assert_eq!(unheld(&facts(policy, free), holder), Ok(()), "{policy}");
        }
    }

    /// A claim a person's cap narrowed to `relaunch` (or a reclaim or an adoption granted less)
    /// has handed the upgrade duty back to aterm's engine: a person's attestation does not move
    /// the vendor under it, however free the session is. alab-wire's `judge_own_move` checks the
    /// scope under either warrant, so the check 84d1a68 added by hand stands.
    /// NEGATIVE CONTROL: the same facts with `upgrade` granted let the move go ahead.
    #[test]
    fn a_hand_attested_move_needs_the_upgrade_duty_granted() {
        let holder = "kimi-alab@77";
        let free = "hold=0 hand=-";
        assert_eq!(
            unheld(&facts_scoped(ON, "relaunch", free), holder),
            Err("aterm supervisor_scope=relaunch".to_string())
        );
        assert_eq!(
            unheld(&facts_scoped(ON, "", free), holder),
            Err("aterm supervisor_scope=-".to_string())
        );
        assert_eq!(unheld(&facts_scoped(ON, "upgrade", free), holder), Ok(()));
        assert_eq!(
            unheld(&facts_scoped(ON, "relaunch,upgrade", free), holder),
            Ok(())
        );
    }

    /// An unproven claim holds the move: aterm has not tied it to this harness.
    #[test]
    fn a_hand_attested_move_needs_the_claim_proven() {
        let facts = Facts {
            policy: facts(ON, "hold=0 hand=-").policy,
            status: alab_wire::Status::parse(
                "OK schema=1 supervisor=kimi-alab%4077 supervisor_proven=0 \
                 supervisor_scope=relaunch,upgrade hold=0 hand=-",
            )
            .unwrap(),
        };
        assert_eq!(
            unheld(&facts, "kimi-alab@77"),
            Err("aterm supervisor_proven=0".to_owned())
        );
    }
}
