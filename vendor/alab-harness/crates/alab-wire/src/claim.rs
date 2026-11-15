// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The claim: what a contract-3 or contract-4 claim says and what aterm's
//! replies to it mean.
//!
//! This module is the pure half of the client: the request texts, the reply
//! parsers and the safe-point judge. It performs no I/O; [`crate::wire`]
//! carries the lines and [`crate::link`] runs the sequence. Every text names the
//! harness from the caller's [`Config`], never a harness of its own.
//!
//! The rules a client of this boundary keeps:
//!
//! * **Never a trial claim.** aterm stores a contract-3 claim from a holder that
//!   is not on its roster as an ordinary foreign claim, and a stored foreign
//!   claim takes every duty from aterm's built-in engine
//!   (`aterm/crates/aterm-agent/src/harness/contract.rs`, `Words::Foreign`). So
//!   a harness claims only after a read-only `native-act info` reply names its
//!   row ([`rostered`]), under either contract.
//! * **Claim at the highest contract both sides list.** `harness policy`'s
//!   `contract=` is a set; the claim is made at the highest member this
//!   library speaks ([`Contract::choose`]: 4, else 3), and only while the
//!   policy reads `effective=on` ([`Policy`]). The claim is [`claim_verb`]'s
//!   text for that contract.
//! * **Act only on the grant.** A contract-4 claim is answered
//!   `OK scope=<granted> revision=<n>` ([`Grant::of_reply`]): aterm drops a
//!   word it does not know, and a person's cap narrows the rest, so the grant
//!   may be narrower than the claim and the harness takes only what was
//!   granted. A contract-3 claim's grant is its words inside
//!   [`Scope::CONTRACT_3`], as `status` proves them ([`Grant::of_status`]).
//! * **A claim stands only when `status` proves it.** aterm proves a claim by
//!   kernel peer pid, the claimant leading the terminal's foreground process
//!   group, and its image sitting in a harness package. The harness reads that
//!   back as `supervisor=<holder> supervisor_proven=1` with every scope word it
//!   was granted ([`Status::held_by`], [`Status::takes`]) and releases a claim
//!   that does not read so ([`release_verb`]).
//! * **Fail closed.** A field a judgement needs that is absent or malformed
//!   holds; no parser here supplies a default.
//! * **Type only through the fence.** Any `send`/`key`/`paste`/`turn` a harness
//!   ever sends carries `if-supervisor=<holder>`
//!   (`crates/aterm-gui/src/control_input.rs`), so a write after the claim is
//!   lost can never land. Nothing here types.
//!
//! Contract 4 is section 2.2 of aterm's harness interface design
//! (`aterm/docs/DESIGN-harness-interface-2026-11-12.md`): `child=<pid>:<start>`,
//! the words `state` and `accounts`, dropped unknown words, and the
//! `OK scope=<granted> revision=<n>` reply.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::config::Config;
use crate::wire::{Fields, percent_decode, session_id};

/// An aterm control contract this library claims under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Contract {
    /// Contract 3: a rostered harness's claim, answered by a bare `OK`, whose
    /// scope words aterm's roster row limits to [`Scope::CONTRACT_3`].
    Three,
    /// Contract 4: one claim for every harness, with `child=<pid>:<start>`,
    /// answered `OK scope=<granted> revision=<n>` ([`Grant`]).
    Four,
}

impl Contract {
    /// Every contract this library speaks, highest first: the order a claim
    /// is chosen in.
    pub const SPOKEN: [Self; 2] = [Self::Four, Self::Three];

    /// The number `contract=` spells.
    #[must_use]
    pub const fn number(self) -> u32 {
        match self {
            Self::Three => 3,
            Self::Four => 4,
        }
    }

    /// The highest contract this library speaks that `served` (a policy's
    /// `contract=` set) lists, or `None` when it lists none of them. A
    /// harness claims at the highest number both sides list (the interface
    /// design, section 2.2).
    #[must_use]
    pub fn choose(served: &[u32]) -> Option<Self> {
        Self::SPOKEN
            .into_iter()
            .find(|contract| served.contains(&contract.number()))
    }

    /// The scope words a claim under this contract names, of the `wanted`
    /// ones, once each in aterm's order. Contract 3 names only words of
    /// [`Scope::CONTRACT_3`], the only ones aterm's roster rows allow (a claim
    /// naming any other is refused whole); contract 4 names every word, since
    /// aterm drops what it does not know and grants the rest.
    #[must_use]
    pub fn scopes(self, wanted: &[Scope]) -> Vec<Scope> {
        let mut scopes: Vec<Scope> = wanted
            .iter()
            .copied()
            .filter(|scope| self == Self::Four || Scope::CONTRACT_3.contains(scope))
            .collect();
        scopes.sort_unstable();
        scopes.dedup();
        scopes
    }
}

/// The contract a claim would be made at when an `OK` policy reply advertises a
/// contract set (`OK gen=… effective=… contract=2,3,4 …`): its highest member
/// this library speaks ([`Contract::choose`]). An absent, malformed or
/// duplicate field, or a set with no member this library speaks, returns
/// `None`.
#[must_use]
pub fn advertised_contract(policy_reply: &str) -> Option<Contract> {
    let mut words = policy_reply.split_ascii_whitespace();
    if words.next() != Some("OK") {
        return None;
    }
    let mut advertised = None;
    for word in words {
        if let Some(value) = word.strip_prefix("contract=") {
            if advertised.is_some() {
                return None;
            }
            advertised = Some(value);
        }
    }
    Contract::choose(&contract_set(advertised?)?)
}

/// The longest `version=` a claim may carry (`contract.rs`, `VERSION_MAX`).
pub(crate) const VERSION_MAX: usize = 128;
/// The longest `vendor=` a claim may carry (`contract.rs`, `VENDOR_MAX`).
const VENDOR_MAX: usize = 64;
/// The longest `login=` a claim may carry (`accounts.rs`, `LABEL_MAX`).
const LOGIN_MAX: usize = 64;
/// The one account label aterm reserves (`accounts.rs`, `RESERVED_LABEL`).
const LOGIN_RESERVED: &str = "help";
/// The longest value a journal line repeats from a reply.
const JOURNAL_WORD_MAX: usize = 64;

/// One duty a claim takes from aterm's built-in engine, in aterm's canonical
/// order (`contract.rs`, `Scope::ALL`, and the interface design's section
/// 2.2, which adds `state` and `accounts` under contract 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Scope {
    /// `relaunch`: restart a crashed child on its conversation.
    Relaunch,
    /// `upgrade`: move the child to another vendor build at a safe point.
    Upgrade,
    /// `turn-end`: Cruise, Land and continuations.
    TurnEnd,
    /// `approvals`: permission, trust and plan boxes.
    Approvals,
    /// `questions`: question dialogs.
    Questions,
    /// `walls`: usage, context, login and API-retry limits.
    Walls,
    /// `state`: say what the agent is doing (contract 4).
    State,
    /// `accounts`: sign-in, and which account a session runs on (contract 4).
    Accounts,
}

impl Scope {
    /// Every word this library knows, in aterm's order.
    pub const ALL: [Self; 8] = [
        Self::Relaunch,
        Self::Upgrade,
        Self::TurnEnd,
        Self::Approvals,
        Self::Questions,
        Self::Walls,
        Self::State,
        Self::Accounts,
    ];

    /// The words a contract-3 claim can take today. Every row of aterm's roster
    /// allows `relaunch` and `upgrade` and nothing else, and aterm refuses a
    /// whole claim that names a word outside its row (`contract.rs`, `ROSTER`
    /// and `read_in`: `a scope word outside the harness's row`). The other words
    /// contract 3 knows are in its grammar, so a claim naming one is read and
    /// then refused; `state` and `accounts` are not ([`Scope::since`]).
    pub const CONTRACT_3: [Self; 2] = [Self::Relaunch, Self::Upgrade];

    /// The first contract whose grammar knows this word: contract 3 for the
    /// six words it has, contract 4 for `state` and `accounts`.
    #[must_use]
    pub const fn since(self) -> Contract {
        match self {
            Self::State | Self::Accounts => Contract::Four,
            _ => Contract::Three,
        }
    }

    /// The wire word (`contract.rs`, `Scope::word`).
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Relaunch => "relaunch",
            Self::Upgrade => "upgrade",
            Self::TurnEnd => "turn-end",
            Self::Approvals => "approvals",
            Self::Questions => "questions",
            Self::Walls => "walls",
            Self::State => "state",
            Self::Accounts => "accounts",
        }
    }

    /// The scope a wire word names, if this library knows it.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scope| scope.word() == word)
    }
}

/// `scopes` spelled as a claim spells them: each once, in aterm's order,
/// comma-joined.
#[must_use]
pub fn scope_words(scopes: &[Scope]) -> String {
    let mut scopes = scopes.to_vec();
    scopes.sort_unstable();
    scopes.dedup();
    scopes
        .iter()
        .map(|scope| scope.word())
        .collect::<Vec<_>>()
        .join(",")
}

/// The claim holder for harness `id`'s process `pid`: `<id>@<pid>`. aterm
/// reads the pid back as canonical decimal above 1 and proves it against the
/// kernel's peer pid (`contract.rs`, `holder_in`).
#[must_use]
pub fn holder(id: &str, pid: u32) -> String {
    format!("{id}@{pid}")
}

/// Whether aterm would read `text` as a claim's `version=`: `MAJOR.MINOR.0`, each
/// part canonical decimal, then optionally `+` and dot-separated identifiers of
/// ASCII alphanumerics and `-` (`contract.rs`, `ClaimVersion::parse`).
pub(crate) fn claim_version(text: &str) -> bool {
    if text.len() > VERSION_MAX {
        return false;
    }
    let (core, meta) = match text.split_once('+') {
        Some((core, meta)) => (core, Some(meta)),
        None => (text, None),
    };
    if meta.is_some_and(|meta| {
        meta.split('.').any(|id| {
            id.is_empty()
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    }) {
        return false;
    }
    let number = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
            && part.parse::<u32>().is_ok()
    };
    let mut parts = core.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some("0"), None) if number(major) && number(minor)
    )
}

/// `version` when aterm would read it as a claim's `vendor=`: 1 to 64 bytes of
/// ASCII alphanumerics and `.`, `-`, `+`, `_` (`contract.rs`, `vendor_ok`).
fn vendor_word(version: Option<&str>) -> Option<&str> {
    version.filter(|version| {
        !version.is_empty()
            && version.len() <= VENDOR_MAX
            && version.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_')
            })
    })
}

/// `label` when aterm would read it as a claim's `login=`: an account label,
/// `[a-z0-9][a-z0-9._-]{0,63}` and not `help` (`accounts.rs`, `label_refusal`).
fn login_word(label: Option<&str>) -> Option<&str> {
    let word = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    label.filter(|label| {
        let bytes = label.as_bytes();
        bytes.len() <= LOGIN_MAX
            && bytes.first().is_some_and(word)
            && bytes
                .iter()
                .all(|byte| word(byte) || matches!(byte, b'.' | b'_' | b'-'))
            && *label != LOGIN_RESERVED
    })
}

/// The vendor process a contract-4 claim names (`child=<pid>:<start>`): its
/// pid and its kernel start time, so that aterm can tell it from a later
/// process given the same pid. aterm keeps it only if the kernel says the
/// process descends from the claimant and is in the tab's foreground group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChildProcess {
    /// The child's pid, above 1.
    pub pid: u32,
    /// The child's kernel start time, in microseconds since the Unix epoch,
    /// as the harness read it from the kernel when the child started
    /// ([`ChildProcess::of`] reads it as aterm checks it).
    pub start: u64,
}

impl ChildProcess {
    /// The claim word's value, `<pid>:<start>` in canonical decimals, or
    /// `None` when the pid is not above 1 (aterm would read no such process).
    #[must_use]
    pub fn word(self) -> Option<String> {
        (self.pid > 1).then(|| format!("{}:{}", self.pid, self.start))
    }
}

/// What a claim says about the child the harness runs now. Each word is sent
/// again whenever it changes, and the whole claim after every child start
/// ([`crate::link::Link::reannounce`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Child<'a> {
    /// The vendor build the child runs (`vendor=`), when known.
    pub vendor: Option<&'a str>,
    /// The account label the child runs on (`login=`), for a harness that
    /// manages accounts.
    pub login: Option<&'a str>,
    /// The vendor process (`child=<pid>:<start>`), once it has started. Only
    /// a contract-4 claim names it; contract 3 has no such word.
    pub process: Option<ChildProcess>,
}

/// `@<sid> <verb>`, when `sid` is a session ID.
fn addressed(sid: &str, verb: &str) -> Option<String> {
    session_id(sid).then(|| format!("@{sid} {verb}"))
}

/// The claim under `contract` for session `sid` by `config`'s harness process
/// `pid`:
///
/// `@<sid> meta set supervisor <id>@<pid> contract=3 scope=<words> version=<version>[ vendor=<vendor>][ login=<label>]`
///
/// `@<sid> meta set supervisor <id>@<pid> contract=4 scope=<words> version=<version>[ vendor=<vendor>][ child=<pid>:<start>][ login=<label>]`
///
/// The scope words are `config`'s that the contract names
/// ([`Contract::scopes`]), spelled once each in aterm's canonical order.
/// `vendor`, `child` and `login` are the child's, and each is left out when it
/// is unknown or is not a word aterm would accept, because aterm refuses the
/// whole claim over one bad word (`contract.rs`, `read_in`). `child=` is sent
/// under contract 4 only.
///
/// `None`, and nothing may be sent, when `sid` is not a session ID, `pid` is
/// not above 1, or the contract names none of `config`'s scope words (a
/// contract-3 claim from a harness that claims only contract-4 duties).
/// ([`Config`] has already refused an empty scope and a version aterm would
/// not read.)
///
/// The text alone claims nothing, and sending it is permitted only after
/// [`Policy::admits_claim`] holds, at the contract [`Policy::contract`]
/// chose, and, at contract 3, [`rostered`] holds too.
#[must_use]
pub fn claim_verb(
    config: &Config,
    contract: Contract,
    sid: &str,
    pid: u32,
    child: Child<'_>,
) -> Option<String> {
    let scopes = contract.scopes(config.scopes());
    if pid <= 1 || scopes.is_empty() {
        return None;
    }
    let mut verb = format!(
        "meta set supervisor {} contract={} scope={} version={}",
        holder(config.id(), pid),
        contract.number(),
        scope_words(&scopes),
        config.version(),
    );
    if let Some(vendor) = vendor_word(child.vendor) {
        verb.push_str(" vendor=");
        verb.push_str(vendor);
    }
    if contract == Contract::Four
        && let Some(process) = child.process.and_then(ChildProcess::word)
    {
        verb.push_str(" child=");
        verb.push_str(&process);
    }
    if let Some(login) = login_word(child.login) {
        verb.push_str(" login=");
        verb.push_str(login);
    }
    addressed(sid, &verb)
}

/// The release of `config`'s process `pid`'s claim on session `sid`:
/// `@<sid> meta unset supervisor holder=<id>@<pid>`. Naming the holder makes it
/// a no-op against anyone else's claim
/// (`aterm/crates/aterm-gui/src/control_session.rs`, `cmd_meta_supervisor_serving`).
/// `None` when `sid` is not a session ID or `pid` is not above 1.
#[must_use]
pub fn release_verb(config: &Config, sid: &str, pid: u32) -> Option<String> {
    if pid <= 1 {
        return None;
    }
    addressed(
        sid,
        &format!("meta unset supervisor holder={}", holder(config.id(), pid)),
    )
}

/// The read of session `sid`'s policy, `@<sid> harness policy`, answered by a
/// [`Policy`]. `None` when `sid` is not a session ID.
#[must_use]
pub fn policy_verb(sid: &str) -> Option<String> {
    addressed(sid, "harness policy")
}

/// The read of session `sid`'s status, `@<sid> status`, answered by a
/// [`Status`]. `None` when `sid` is not a session ID.
#[must_use]
pub fn status_verb(sid: &str) -> Option<String> {
    addressed(sid, "status")
}

/// The read-only roster question, `@<sid> native-act info`, whose reply
/// [`rostered`] judges. `None` when `sid` is not a session ID.
#[must_use]
pub fn info_verb(sid: &str) -> Option<String> {
    addressed(sid, "native-act info")
}

/// The roster guard: whether a `native-act info` reply names harness `id`'s row
/// with the `upgrade` duty.
///
/// The reply is `OK native-act state=info … own-image=<id>:upgrade[,<id>:upgrade…]`,
/// or `own-image=-` when no row allows it
/// (`aterm/crates/aterm-gui/src/native_act.rs`, `own_image_rows` and
/// `Request::Info`). It carries the bare word `native-act`, so it is read
/// leniently; the one field that matters must be there exactly once.
///
/// `false` for anything else: an `ERR`, a host that does not know the verb, a
/// missing or repeated `own-image=`, or a list without the exact member
/// `<id>:upgrade`. A harness that is not rostered must never send a contract-3
/// claim, not even to find out: aterm would store it as a foreign claim that
/// takes every duty until it is released. Contract 4 does not ask: the list is
/// built from compiled roster rows, which its registry row replaces (the
/// interface design, section 2.2, HX-11 and HX-14).
#[must_use]
pub fn rostered(info_reply: &str, id: &str) -> bool {
    let mut words = info_reply.split_ascii_whitespace();
    if words.next() != Some("OK") {
        return false;
    }
    let mut rows = None;
    for word in words {
        if let Some(value) = word.strip_prefix("own-image=")
            && rows.replace(value).is_some()
        {
            return false;
        }
    }
    let wanted = (id, Scope::Upgrade.word());
    rows.is_some_and(|rows| {
        rows.split(',')
            .any(|row| row.split_once(':') == Some(wanted))
    })
}

/// `value` as one journal word: ASCII graphic bytes kept, any other byte
/// percent-encoded, cut at 64 bytes. An empty value reads `(empty)`, which no
/// wire value spells.
#[must_use]
pub fn journal_word(value: &str) -> String {
    if value.is_empty() {
        return "(empty)".to_string();
    }
    let mut word = String::new();
    for byte in value.bytes() {
        let piece = if byte.is_ascii_graphic() && byte != b'%' {
            char::from(byte).to_string()
        } else {
            format!("%{byte:02X}")
        };
        if word.len() + piece.len() > JOURNAL_WORD_MAX {
            break;
        }
        word.push_str(&piece);
    }
    word
}

/// Why a reply could not be read as what was asked for. Its text is one journal
/// word group, as in `aterm unclaimed <why>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplyError {
    /// The reply is not an `OK` line; its first two words, made journal-safe.
    Refused(String),
    /// The reply is not an `OK` line, and it asks to be retried: aterm could
    /// not read its own state now (`ERR aterm is showing a dialog; answer it,
    /// then retry`, `ERR main thread stalled 6s; retry`;
    /// `aterm/crates/aterm-gui/src/control_media.rs`, `main_thread_refusal`).
    /// Its first two words, made journal-safe.
    Retry(String),
    /// A field the reader needs is absent.
    Missing(&'static str),
    /// A field the reader needs is not in its grammar.
    Malformed(&'static str),
    /// A field the reader needs appears more than once, so it has no one value.
    Repeated(&'static str),
}

impl ReplyError {
    /// A reply that is not an `OK` line: [`ReplyError::Retry`] when its last
    /// word is `retry`, which is how aterm words a refusal that stands for now
    /// only, else [`ReplyError::Refused`].
    fn refused(reply: &str) -> Self {
        let words: Vec<String> = reply
            .split_ascii_whitespace()
            .take(2)
            .map(journal_word)
            .collect();
        let words = if words.is_empty() {
            "(empty)".to_string()
        } else {
            words.join(":")
        };
        if reply.split_ascii_whitespace().next_back() == Some("retry") {
            Self::Retry(words)
        } else {
            Self::Refused(words)
        }
    }
}

impl fmt::Display for ReplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(words) | Self::Retry(words) => write!(f, "reply={words}"),
            Self::Missing(key) => write!(f, "{key}=absent"),
            Self::Malformed(key) => write!(f, "{key}=malformed"),
            Self::Repeated(key) => write!(f, "{key}=repeated"),
        }
    }
}

impl std::error::Error for ReplyError {}

/// The members of a `contract=` value: canonical nonzero decimals, comma-joined.
fn contract_set(value: &str) -> Option<Vec<u32>> {
    value
        .split(',')
        .map(|member| {
            if member.is_empty()
                || member.starts_with('0')
                || !member.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            member.parse::<u32>().ok()
        })
        .collect()
}

fn canonical_u64(value: &str) -> Option<u64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

/// The parts of a `harness policy` reply a claim holder acts on
/// (`aterm/crates/aterm-gui/src/harness_policy_verb.rs`):
///
/// `OK gen=<n> effective=<on|off> why=<word> contract=2,3 … relaunch=<bool> … upgrade=<bool> upgrade_ask=<bool> human_grace_s=<n> …`
///
/// The reader is strict about these seven fields, reads `why=` when it is there,
/// and is blind to every other word: the reply carries dozens of keys this
/// harness has no use for and aterm adds more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// `gen=`: the policy cell's generation, which a `harness-policy` event moves.
    pub generation: u64,
    /// `contract=`: every contract the host serves.
    pub contracts: Vec<u32>,
    /// `effective=on`: aterm's harness policy is in effect for this session.
    pub effective: bool,
    /// `why=`: the first reason the policy is off for the session (`enabled`
    /// while it is on), as one journal word; `None` when aterm did not say.
    pub why: Option<String>,
    /// `[harness] relaunch`: a crashed agent may be brought back.
    pub relaunch: bool,
    /// `[harness] upgrade`: an agent may be moved onto another build.
    pub upgrade: bool,
    /// `[harness] upgrade_ask`: a person says when; nothing moves by itself.
    pub upgrade_ask: bool,
    /// `[harness] human_grace_s`: how long after a person's last key the session
    /// is still theirs.
    pub human_grace: Duration,
}

impl Policy {
    /// Parse one `harness policy` reply.
    ///
    /// # Errors
    /// The reply is not an `OK` line, or one of `gen=`, `contract=`,
    /// `effective=`, `relaunch=`, `upgrade=`, `upgrade_ask=` and `human_grace_s=`
    /// is absent, repeated or outside its grammar. Nothing is defaulted: a host
    /// that does not say whether it permits a move has not permitted it. The
    /// short `OK effective=<on|off>` reply aterm gives a lesser principal is
    /// such an error. `why=` may be absent; repeated, it is an error too.
    pub fn parse(reply: &str) -> Result<Self, ReplyError> {
        const KEYS: [&str; 7] = [
            "gen",
            "contract",
            "effective",
            "relaunch",
            "upgrade",
            "upgrade_ask",
            "human_grace_s",
        ];
        let mut words = reply.split_ascii_whitespace();
        if words.next() != Some("OK") {
            return Err(ReplyError::refused(reply));
        }
        let mut values: [Option<&str>; 7] = [None; 7];
        let mut why = None;
        for word in words {
            let Some((key, value)) = word.split_once('=') else {
                continue;
            };
            if key == "why" && why.replace(value).is_some() {
                return Err(ReplyError::Repeated("why"));
            }
            if let Some(slot) = KEYS.iter().position(|known| *known == key)
                && values[slot].replace(value).is_some()
            {
                return Err(ReplyError::Repeated(KEYS[slot]));
            }
        }
        let value = |slot: usize| values[slot].ok_or(ReplyError::Missing(KEYS[slot]));
        let switch = |slot: usize| match value(slot)? {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(ReplyError::Malformed(KEYS[slot])),
        };
        Ok(Self {
            generation: canonical_u64(value(0)?).ok_or(ReplyError::Malformed(KEYS[0]))?,
            contracts: contract_set(value(1)?).ok_or(ReplyError::Malformed(KEYS[1]))?,
            effective: match value(2)? {
                "on" => true,
                "off" => false,
                _ => return Err(ReplyError::Malformed(KEYS[2])),
            },
            why: why.map(journal_word),
            relaunch: switch(3)?,
            upgrade: switch(4)?,
            upgrade_ask: switch(5)?,
            human_grace: canonical_u64(value(6)?)
                .map(Duration::from_secs)
                .ok_or(ReplyError::Malformed(KEYS[6]))?,
        })
    }

    /// The contract a claim is made at: the highest one this library speaks
    /// that the host serves ([`Contract::choose`]).
    #[must_use]
    pub fn contract(&self) -> Option<Contract> {
        Contract::choose(&self.contracts)
    }

    /// Whether the host serves a contract this library claims under.
    #[must_use]
    pub fn serves_contract(&self) -> bool {
        self.contract().is_some()
    }

    /// Whether this policy lets a rostered harness claim at all: contract 3 or
    /// 4 is served and the policy is in effect for the session. It says
    /// nothing about any one duty; [`judge_safe_point`] and
    /// [`Policy::relaunch`] do.
    #[must_use]
    pub fn admits_claim(&self) -> bool {
        self.serves_contract() && self.effective
    }

    /// Whether the policy is off for the session by configuration: aterm has
    /// no harness host (`unhosted`), its supervisor is off (`disabled`), off
    /// for a headless instance (`headless`) or for this agent (`agent-off`),
    /// or the session's own Autopilot is off (`autopilot-off`)
    /// (`aterm/crates/aterm-gui/src/harness_policy_verb.rs`, `effective`).
    /// The other reasons pass on their own: a halt on the session (`hold`)
    /// and aterm suspended for its own update (`suspended`); so does a reason
    /// this harness does not know, or none given.
    #[must_use]
    pub fn off_by_configuration(&self) -> bool {
        !self.effective
            && matches!(
                self.why.as_deref(),
                Some("unhosted" | "disabled" | "headless" | "agent-off" | "autopilot-off")
            )
    }

    /// Whether the policy is off by a setting a person can switch on again
    /// while the session runs: `[harness] enabled` (`disabled`), the agent's
    /// own switch (`agent-off`), or the session's Autopilot (`autopilot-off`).
    /// Each is [`Policy::off_by_configuration`]; `unhosted` and `headless`
    /// are facts of the window and stand.
    #[must_use]
    pub fn switched_off(&self) -> bool {
        !self.effective
            && matches!(
                self.why.as_deref(),
                Some("disabled" | "agent-off" | "autopilot-off")
            )
    }

    /// `effective=off`, with `why=<word>` when aterm said why: the journal
    /// words of a policy that is off.
    #[must_use]
    pub fn off_words(&self) -> String {
        self.why.as_deref().map_or_else(
            || "effective=off".to_string(),
            |why| format!("effective=off why={why}"),
        )
    }
}

/// When a person last typed, as `status human_ms=` reports it
/// (`aterm/crates/aterm-agent/src/supervise/screen.rs`, `HumanInput::of_status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Human {
    /// `human_ms=-`: nobody has typed since aterm began serving the session.
    Never,
    /// The last key was this long before the read.
    Ago(Duration),
    /// The field is absent or is not a number: nothing is known, which holds.
    Unknown,
}

/// The parts of a `status` reply that witness a claim and a safe point.
///
/// `status` is a line of some two hundred fields that aterm extends freely, so
/// it is read leniently ([`Fields::parse_lenient`]): the last word for a key
/// wins and a bare word is skipped. Leniency ends there. Every field is kept as
/// aterm sent it, or `None` when it did not, and [`judge_safe_point`] holds on
/// each one it cannot read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// `supervisor=`: the standing claim's holder, percent-decoded. `None` for
    /// `-` (no claim), an absent field, or a value that does not decode.
    pub supervisor: Option<String>,
    /// `supervisor_proven=1`: aterm proved the standing claim. Anything else,
    /// absence included, is unproven.
    pub supervisor_proven: bool,
    /// `supervisor_scope=`: the duty words the standing claim takes, as sent.
    /// Empty for `-` or an absent field. A claim without `contract=` lists every
    /// duty (`aterm/crates/aterm-gui/src/session_timeline.rs`,
    /// `SupervisorAuthority::wire_fields`).
    pub supervisor_scope: Vec<String>,
    /// `supervisor_revision=`: the claim cell's revision, which moves with every
    /// claim, release and replacement.
    pub supervisor_revision: Option<u64>,
    /// `agent=`: the agent's phase as aterm's reader sees it (`idle` at rest).
    pub agent: Option<String>,
    /// `composer=`: what the agent's input box holds (`empty`, `placeholder`, or
    /// a draft).
    pub composer: Option<String>,
    /// `human_ms=`.
    pub human: Human,
    /// `hand=`: who drives the session (`-` for nobody).
    pub hand: Option<String>,
    /// `hold=`: `1` while a halt stands on the session.
    pub hold: Option<String>,
    /// `input=`: `clear` once the session's program has read everything typed.
    pub input: Option<String>,
    /// `need=`: what the session waits on a person for (`-` for nothing).
    pub need: Option<String>,
}

impl Status {
    /// Parse one schema-1 `status` reply.
    ///
    /// # Errors
    /// The reply is not an `OK` line, or its `schema=` is absent or its major
    /// version is not 1. Every other field may be absent here and is judged
    /// where it is needed.
    pub fn parse(reply: &str) -> Result<Self, ReplyError> {
        let fields = Fields::parse_lenient(reply).map_err(|_| ReplyError::refused(reply))?;
        let schema = fields.get("schema").ok_or(ReplyError::Missing("schema"))?;
        if schema.split('.').next() != Some("1") {
            return Err(ReplyError::Malformed("schema"));
        }
        let word = |key: &str| fields.get(key).map(str::to_string);
        Ok(Self {
            supervisor: fields
                .get("supervisor")
                .filter(|value| *value != "-" && !value.is_empty())
                .and_then(|value| percent_decode(value).ok()),
            supervisor_proven: fields.get("supervisor_proven") == Some("1"),
            supervisor_scope: fields
                .get("supervisor_scope")
                .filter(|value| *value != "-")
                .map(|value| {
                    value
                        .split(',')
                        .filter(|word| !word.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            supervisor_revision: fields.get("supervisor_revision").and_then(canonical_u64),
            agent: word("agent"),
            composer: word("composer"),
            human: match fields.get("human_ms") {
                Some("-") => Human::Never,
                Some(value) => canonical_u64(value)
                    .map_or(Human::Unknown, |ms| Human::Ago(Duration::from_millis(ms))),
                None => Human::Unknown,
            },
            hand: word("hand"),
            hold: word("hold"),
            input: word("input"),
            need: word("need"),
        })
    }

    /// Whether this status shows `holder`'s claim standing and proven.
    #[must_use]
    pub fn held_by(&self, holder: &str) -> bool {
        self.supervisor_proven && self.supervisor.as_deref() == Some(holder)
    }

    /// Whether the standing claim takes `scope`.
    #[must_use]
    pub fn takes(&self, scope: Scope) -> bool {
        self.supervisor_scope
            .iter()
            .any(|word| word == scope.word())
    }
}

/// The words of `granted` that name a scope of `asked`, once each in aterm's
/// order. A word this library does not know is passed over: the harness acts
/// only on what it asked for and was granted.
fn granted_of<'a>(granted: impl Iterator<Item = &'a str>, asked: &[Scope]) -> Vec<Scope> {
    let mut scopes: Vec<Scope> = granted
        .filter_map(Scope::from_word)
        .filter(|scope| asked.contains(scope))
        .collect();
    scopes.sort_unstable();
    scopes.dedup();
    scopes
}

/// What aterm granted a standing claim: the duties the harness performs, which
/// may be fewer than it claimed, and the claim's revision, which a typing
/// fence names (`if-supervisor-revision=`) and which a cap raises.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The contract the claim was made under.
    pub contract: Contract,
    /// The granted duties, once each in aterm's order. Empty is a grant too: a
    /// contract-4 claim that takes no duty still names the child.
    pub scopes: Vec<Scope>,
    /// The claim's revision, or `None` when aterm did not state one (a
    /// contract-3 `status` without `supervisor_revision=`).
    pub revision: Option<u64>,
}

impl Grant {
    /// Read a contract-4 claim's reply, `OK scope=<granted> revision=<n>`,
    /// keeping the granted words that name a scope of `asked`. A granted set
    /// narrower than `asked` is normal: aterm dropped a word it does not know,
    /// or a person's cap withheld it. A granted word this library does not
    /// know is passed over, and so is any other word of the reply. `scope=-`
    /// and `scope=` grant nothing.
    ///
    /// # Errors
    /// The reply is not an `OK` line, or `scope=` or `revision=` is absent or
    /// repeated, or `revision=` is not a canonical decimal.
    pub fn of_reply(reply: &str, asked: &[Scope]) -> Result<Self, ReplyError> {
        let mut words = reply.split_ascii_whitespace();
        if words.next() != Some("OK") {
            return Err(ReplyError::refused(reply));
        }
        let (mut scope, mut revision) = (None, None);
        for word in words {
            let (slot, key) = match word.split_once('=') {
                Some(("scope", value)) => ((&mut scope, value), "scope"),
                Some(("revision", value)) => ((&mut revision, value), "revision"),
                _ => continue,
            };
            if slot.0.replace(slot.1).is_some() {
                return Err(ReplyError::Repeated(key));
            }
        }
        let scope: &str = scope.ok_or(ReplyError::Missing("scope"))?;
        let revision = canonical_u64(revision.ok_or(ReplyError::Missing("revision"))?)
            .ok_or(ReplyError::Malformed("revision"))?;
        Ok(Self {
            contract: Contract::Four,
            scopes: granted_of(scope.split(','), asked),
            revision: Some(revision),
        })
    }

    /// A claim's grant as `status` shows it: the words of `supervisor_scope`
    /// that name a scope of `asked`, at `supervisor_revision`. This is a
    /// contract-3 claim's grant (its reply is a bare `OK`), and how a
    /// contract-4 holder learns that a cap narrowed its live claim.
    #[must_use]
    pub fn of_status(contract: Contract, status: &Status, asked: &[Scope]) -> Self {
        Self {
            contract,
            scopes: granted_of(status.supervisor_scope.iter().map(String::as_str), asked),
            revision: status.supervisor_revision,
        }
    }

    /// Whether the grant takes `scope`: the harness performs that duty.
    #[must_use]
    pub fn takes(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope)
    }

    /// The granted words as a claim spells them, or `-` for none.
    #[must_use]
    pub fn words(&self) -> String {
        if self.scopes.is_empty() {
            "-".to_string()
        } else {
            scope_words(&self.scopes)
        }
    }

    /// The revision as one journal word, `-` when aterm stated none.
    #[must_use]
    pub fn revision_word(&self) -> String {
        self.revision
            .map_or_else(|| "-".to_string(), |revision| revision.to_string())
    }
}

/// The account aterm's account pool moved the session to: the row's label and
/// the home the vendor reads its credentials from (for Kimi Code,
/// `KIMI_CODE_HOME`). No credential crosses the wire, only these two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The account row's label (`to=`), an account label aterm admits
    /// (`[a-z0-9][a-z0-9._-]{0,63}`, not `help`).
    pub label: String,
    /// Its home (`home=`, percent-decoded and absolute), or `None` for the
    /// vendor's default home (`home=-`).
    pub home: Option<PathBuf>,
}

/// A restart aterm asks the claim holder to make, from an
/// `EVENT <local> restart cause=<why>[ to=<label> home=<path>][ model=<word>]`
/// line (`aterm/crates/aterm-gui/src/restart_act.rs`, `record_restart` and
/// `record_scoped_account`; `model=` from HX-06). aterm sends it instead of
/// signalling a harness whose proven claim takes `relaunch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restart {
    /// Why: `asked` (a person's `aterm ctl restart`), `stall` (the stall's
    /// remedy), or an account cause (`account`, `account-ahead`).
    pub cause: String,
    /// The account to start the next child on, when aterm's account pool moved
    /// the session (`to=` and `home=`, sent only with `cause=account` or
    /// `cause=account-ahead`). aterm leaves the switch to the holder: a harness
    /// that runs its child on the same account again restarts onto the wall
    /// that caused the move.
    pub account: Option<Account>,
    /// The model to start the next child on (`model=`), when aterm chose one:
    /// `cause=model` names a model move, and aterm may name the model with any
    /// cause. aterm never sends it in `to=`. A harness passes it to the vendor
    /// as the vendor's model option on the next start ([`model_word`]).
    pub model: Option<String>,
}

/// The longest model word a restart may name: the bound of aterm's model
/// identifier (`harness/upgrade.rs` `is_model_id`).
pub const MODEL_MAX: usize = 256;

/// Whether `model` is a model word a restart may name: what aterm's model
/// identifier check (`aterm-agent/src/harness/upgrade.rs` `is_model_id`)
/// accepts, 1 to [`MODEL_MAX`] bytes of ASCII alphanumerics and `-`, `_`,
/// `.`, `:`, `/`, `@`, `[` and `]` in any order, except a word starting with
/// `-`, which a vendor would read as an option (aterm refuses those too where
/// it passes a model on, `harness/agy_quota.rs`). It is wider than aterm's
/// other model reader (`agent_settings.rs` `valid_model`, at most 128 bytes,
/// alphanumeric first, no `@[]`), whose words all pass. So `opus[1m]`,
/// `gemini-3-pro`, `us.anthropic.claude-x:0` and `@cf/meta/llama` pass.
#[must_use]
pub fn model_word(model: &str) -> bool {
    let bytes = model.as_bytes();
    (1..=MODEL_MAX).contains(&bytes.len())
        && bytes.first() != Some(&b'-')
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'@' | b'[' | b']')
        })
}

impl Restart {
    /// Read the words after `restart` in an event line. The first must be
    /// `cause=<word>`, a word of 1 to 64 ASCII alphanumerics, `-` and `_`, and no
    /// later word may name a cause again. `to=` and `home=` come together or not
    /// at all, each once, and only for an account cause; `to=` must be an
    /// account label and `home=` `-` or a percent-encoded absolute path.
    /// `model=` may come with any cause, once, and must be a model word
    /// ([`model_word`]). Any other trailing word is passed over. `None`
    /// otherwise: a restart nobody can read whole is not acted on.
    #[must_use]
    pub fn parse<'a>(mut words: impl Iterator<Item = &'a str>) -> Option<Self> {
        let cause = words.next()?.strip_prefix("cause=")?;
        if cause.is_empty()
            || cause.len() > JOURNAL_WORD_MAX
            || !cause
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return None;
        }
        let (mut label, mut home, mut model) = (None, None, None);
        for word in words {
            let slot = match word.split_once('=') {
                Some(("cause", _)) => return None,
                Some(("to", value)) => (&mut label, value),
                Some(("home", value)) => (&mut home, value),
                Some(("model", value)) => (&mut model, value),
                _ => continue,
            };
            if slot.0.replace(slot.1).is_some() {
                return None;
            }
        }
        let account = match (label, home) {
            (None, None) => None,
            (Some(label), Some(home)) if matches!(cause, "account" | "account-ahead") => {
                let label = login_word(Some(label))?.to_string();
                let home = if home == "-" {
                    None
                } else {
                    let path = PathBuf::from(percent_decode(home).ok()?);
                    if !path.is_absolute() {
                        return None;
                    }
                    Some(path)
                };
                Some(Account { label, home })
            }
            _ => return None,
        };
        let model = match model {
            None => None,
            Some(model) if model_word(model) => Some(model.to_string()),
            Some(_) => return None,
        };
        Some(Self {
            cause: cause.to_string(),
            account,
            model,
        })
    }
}

/// The one field that keeps a moment from being a safe point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    /// The `status` or `harness policy` key that holds.
    pub field: &'static str,
    /// What aterm said for it, as one journal word; `absent` when it said
    /// nothing, `unknown` for a `human_ms` that is not a number.
    pub value: String,
    /// For a `human_ms` hold, how much of the person's grace is still to run.
    /// Nothing announces its end (an ageing key emits no event), so the holder
    /// reads again after it.
    pub grace_left: Option<Duration>,
}

impl Hold {
    fn on(field: &'static str, value: Option<&str>) -> SafePoint {
        SafePoint::Hold(Self {
            field,
            value: value.map_or_else(|| "absent".to_string(), journal_word),
            grace_left: None,
        })
    }
}

impl fmt::Display for Hold {
    /// The journal line: `wait aterm <field>=<value>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wait aterm {}={}", self.field, self.value)
    }
}

/// What one fresh policy and status pair says about moving the child now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafePoint {
    /// Every condition holds: aterm sees nobody the move would interrupt.
    Safe,
    /// The first condition that does not.
    Hold(Hold),
}

impl SafePoint {
    /// Whether the move may proceed.
    #[must_use]
    pub const fn is_safe(&self) -> bool {
        matches!(self, Self::Safe)
    }
}

/// Judge one fresh `harness policy` and `status` pair as an upgrade safe point
/// for the claim `holder`, by aterm's own facts.
///
/// It is [`SafePoint::Safe`] only when all of these read exactly so, checked in
/// this order, and otherwise the first that does not is the [`Hold`]:
///
/// 1. `supervisor=<holder>`: the standing claim is this harness's;
/// 2. `supervisor_proven=1`: aterm proved it;
/// 3. `supervisor_scope` names `upgrade`: the claim took this duty;
/// 4. `contract=` serves 3 or 4 and `effective=on`: the policy still stands;
/// 5. `upgrade=true`;
/// 6. `upgrade_ask=false`: nobody has to say when;
/// 7. `agent=idle`;
/// 8. `composer=empty` or `composer=placeholder`: no draft;
/// 9. `human_ms=-`, or at least the grace: the larger of the policy's
///    `human_grace_s` and the caller's own `grace`;
/// 10. `hand=-`, `hold=0`, `input=clear`, `need=-`.
///
/// A field that is absent or unreadable holds; nothing is assumed. `grace` can
/// lengthen aterm's grace and never shorten it.
///
/// This judges only what aterm sees. The harness's own kernel checks (the
/// foreground group, the session record, background work, the re-validated
/// plan) stand beside it and are not replaced by it.
#[must_use]
pub fn judge_safe_point(
    policy: &Policy,
    status: &Status,
    holder: &str,
    grace: Duration,
) -> SafePoint {
    if status.supervisor.as_deref() != Some(holder) {
        return Hold::on(
            "supervisor",
            Some(status.supervisor.as_deref().unwrap_or("-")),
        );
    }
    if !status.supervisor_proven {
        return Hold::on("supervisor_proven", Some("0"));
    }
    if !status.takes(Scope::Upgrade) {
        let scope = status.supervisor_scope.join(",");
        return Hold::on(
            "supervisor_scope",
            Some(if scope.is_empty() { "-" } else { &scope }),
        );
    }
    if !policy.serves_contract() {
        let contracts = policy
            .contracts
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        return Hold::on("contract", Some(&contracts));
    }
    if !policy.effective {
        return Hold::on("effective", Some("off"));
    }
    if !policy.upgrade {
        return Hold::on("upgrade", Some("false"));
    }
    if policy.upgrade_ask {
        return Hold::on("upgrade_ask", Some("true"));
    }
    if status.agent.as_deref() != Some("idle") {
        return Hold::on("agent", status.agent.as_deref());
    }
    if !matches!(status.composer.as_deref(), Some("empty" | "placeholder")) {
        return Hold::on("composer", status.composer.as_deref());
    }
    let grace = policy.human_grace.max(grace);
    match status.human {
        Human::Never => {}
        Human::Ago(ago) if ago >= grace => {}
        Human::Ago(ago) => {
            return SafePoint::Hold(Hold {
                field: "human_ms",
                value: ago.as_millis().to_string(),
                grace_left: Some(grace.saturating_sub(ago)),
            });
        }
        Human::Unknown => return Hold::on("human_ms", Some("unknown")),
    }
    for (field, value, wanted) in [
        ("hand", status.hand.as_deref(), "-"),
        ("hold", status.hold.as_deref(), "0"),
        ("input", status.input.as_deref(), "clear"),
        ("need", status.need.as_deref(), "-"),
    ] {
        if value != Some(wanted) {
            return Hold::on(field, value);
        }
    }
    SafePoint::Safe
}

/// Who vouches for a move a harness judges by its own readings instead of by
/// [`judge_safe_point`] ([`judge_own_move`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warrant {
    /// A person attested the moment by hand (kimi-alab's `SIGUSR1`): they are
    /// the one `upgrade_ask=true` waits for, and their word stands over the
    /// policy's `upgrade` switch.
    Person,
    /// The harness's own readings of the vendor (its terminal, its turn, its
    /// background work) call the moment safe, and nobody said when: the
    /// policy's `upgrade=true` and `upgrade_ask=false` must hold too.
    Harness,
}

/// Judge one fresh `harness policy` and `status` pair (from
/// [`crate::Link::read`]) for a move the harness judged safe by its own
/// readings, under `warrant`: what aterm's facts must still say for it to go
/// ahead. aterm's `agent=`, `composer=`, `human_ms=`, `input=` and `need=` are
/// left to those readings; what is checked is what only aterm knows.
///
/// [`SafePoint::Safe`] only when all of these hold, checked in this order, and
/// otherwise the first that does not is the [`Hold`]:
///
/// 1. `supervisor=<holder>`, `supervisor_proven=1`, and `supervisor_scope`
///    names `upgrade`: the duty is still this harness's (a person's cap, a
///    reclaim or an adoption that left less hands it back to aterm's engine,
///    whatever the warrant);
/// 2. no halt: `hold=0`, and the policy's `why=` is not `hold`;
/// 3. `hand=` names no other driver ([`crate::relaunch::driver_hand`]; an
///    absent `hand=` holds);
/// 4. under [`Warrant::Harness`], `upgrade=true` and `upgrade_ask=false`.
#[must_use]
pub fn judge_own_move(
    policy: &Policy,
    status: &Status,
    holder: &str,
    warrant: Warrant,
) -> SafePoint {
    if status.supervisor.as_deref() != Some(holder) {
        return Hold::on(
            "supervisor",
            Some(status.supervisor.as_deref().unwrap_or("-")),
        );
    }
    if !status.supervisor_proven {
        return Hold::on("supervisor_proven", Some("0"));
    }
    if !status.takes(Scope::Upgrade) {
        let scope = status.supervisor_scope.join(",");
        return Hold::on(
            "supervisor_scope",
            Some(if scope.is_empty() { "-" } else { &scope }),
        );
    }
    if policy.why.as_deref() == Some("hold") {
        return Hold::on("hold", Some("1"));
    }
    if status.hold.as_deref() != Some("0") {
        return Hold::on("hold", status.hold.as_deref());
    }
    match status.hand.as_deref() {
        Some(hand) if !crate::relaunch::driver_hand(hand) => {}
        hand => return Hold::on("hand", hand),
    }
    if warrant == Warrant::Harness {
        if !policy.upgrade {
            return Hold::on("upgrade", Some("false"));
        }
        if policy.upgrade_ask {
            return Hold::on("upgrade_ask", Some("true"));
        }
    }
    SafePoint::Safe
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A move the harness judged by its own readings needs only what aterm
    /// alone knows: the claim with `upgrade`, no halt, no other driver, and,
    /// with nobody's word for the moment, upgrades on and nobody asked to say
    /// when. A person's attestation stands over the two switches, never over
    /// the claim, a halt or a driver.
    #[test]
    fn a_move_judged_by_the_harness_needs_the_claim_no_halt_and_no_driver() {
        let holder = "example-alab@4242";
        let judge = |policy: &str, status: &str, warrant| {
            judge_own_move(
                &Policy::parse(policy).unwrap(),
                &Status::parse(status).unwrap(),
                holder,
                warrant,
            )
        };
        let held = |policy: &str, status: &str, warrant| match judge(policy, status, warrant) {
            SafePoint::Hold(hold) => hold.to_string(),
            SafePoint::Safe => "safe".to_string(),
        };
        for warrant in [Warrant::Person, Warrant::Harness] {
            assert_eq!(judge(LIVE_POLICY, LIVE_STATUS, warrant), SafePoint::Safe);
            // aterm's own readings of the vendor are the harness's to make.
            let busy = with(
                &with(
                    &with(LIVE_STATUS, "agent", Some("busy")),
                    "composer",
                    Some("draft"),
                ),
                "human_ms",
                Some("5"),
            );
            assert_eq!(judge(LIVE_POLICY, &busy, warrant), SafePoint::Safe);
            let turn = with(LIVE_STATUS, "hand", Some("turn:7"));
            assert_eq!(judge(LIVE_POLICY, &turn, warrant), SafePoint::Safe);
            for (status, why) in [
                (
                    with(LIVE_STATUS, "supervisor", Some("other-alab%404")),
                    "wait aterm supervisor=other-alab@4",
                ),
                (
                    with(LIVE_STATUS, "supervisor_proven", Some("0")),
                    "wait aterm supervisor_proven=0",
                ),
                (
                    with(LIVE_STATUS, "supervisor_scope", Some("relaunch")),
                    "wait aterm supervisor_scope=relaunch",
                ),
                (
                    with(LIVE_STATUS, "supervisor_scope", Some("-")),
                    "wait aterm supervisor_scope=-",
                ),
                (with(LIVE_STATUS, "hold", Some("1")), "wait aterm hold=1"),
                (with(LIVE_STATUS, "hold", None), "wait aterm hold=absent"),
                (
                    with(LIVE_STATUS, "hand", Some("lease:driver%404")),
                    "wait aterm hand=lease:driver%25404",
                ),
                (
                    with(LIVE_STATUS, "hand", Some("turn:7:driver%404")),
                    "wait aterm hand=turn:7:driver%25404",
                ),
                (with(LIVE_STATUS, "hand", None), "wait aterm hand=absent"),
            ] {
                assert_eq!(held(LIVE_POLICY, &status, warrant), why, "{warrant:?}");
            }
            let halted = with(
                &with(LIVE_POLICY, "effective", Some("off")),
                "why",
                Some("hold"),
            );
            assert_eq!(held(&halted, LIVE_STATUS, warrant), "wait aterm hold=1");
        }
        let off = with(LIVE_POLICY, "upgrade", Some("false"));
        let ask = with(LIVE_POLICY, "upgrade_ask", Some("true"));
        assert_eq!(
            held(&off, LIVE_STATUS, Warrant::Harness),
            "wait aterm upgrade=false"
        );
        assert_eq!(
            held(&ask, LIVE_STATUS, Warrant::Harness),
            "wait aterm upgrade_ask=true"
        );
        // NEGATIVE CONTROL: a person's word is the one the switches wait for.
        assert_eq!(judge(&off, LIVE_STATUS, Warrant::Person), SafePoint::Safe);
        assert_eq!(judge(&ask, LIVE_STATUS, Warrant::Person), SafePoint::Safe);
    }

    #[test]
    fn an_older_policy_without_contracts_admits_no_claim() {
        let older = "OK gen=12 effective=on why=config continue=on land=off questions=on";
        assert_eq!(advertised_contract(older), None);
    }

    /// A harness claims at the highest number both sides list: 4 wherever it
    /// is listed, 3 against an aterm from before contract 4, and nothing
    /// against one that lists neither.
    #[test]
    fn the_claim_is_made_at_the_highest_contract_both_sides_list() {
        for reply in [
            "OK gen=1 contract=3",
            "OK gen=1 effective=on why=enabled contract=2,3 continue=on",
            "OK contract=3,2",
            "OK contract=2,3,5",
        ] {
            assert_eq!(advertised_contract(reply), Some(Contract::Three), "{reply}");
        }
        for reply in [
            "OK contract=2,3,4",
            "OK contract=4",
            "OK contract=4,3",
            "OK contract=3,4,5",
        ] {
            assert_eq!(advertised_contract(reply), Some(Contract::Four), "{reply}");
        }
        assert_eq!(Contract::choose(&[2, 3, 4]), Some(Contract::Four));
        assert_eq!(Contract::choose(&[3]), Some(Contract::Three));
        assert_eq!(Contract::choose(&[2, 5]), None);
        assert_eq!(Contract::choose(&[]), None);
        assert_eq!(
            Contract::SPOKEN.map(Contract::number),
            [4, 3],
            "highest first"
        );
        for reply in [
            "",
            "ERR contract=3",
            "OKAY contract=3",
            "OK contract=2",
            "OK contract=5",
            "OK contract=",
            "OK contract=x",
            "OK contract=03",
            "OK contract=+3",
            "OK contract=2,,3",
            "OK contract=3,",
            "OK contract=,3",
            "OK contract=3,x",
            "OK contract=3,0",
            "OK contract=3,4294967296",
            "OK contract=3 contract=2,3",
        ] {
            assert_eq!(advertised_contract(reply), None, "{reply:?}");
        }
    }

    #[test]
    fn recognizing_a_host_contract_alone_admits_no_claim() {
        assert!(advertised_contract("OK contract=2,3").is_some());
        // The contract set is one of seven facts a policy must state, and the
        // policy one of three gates (with the roster and the proof read-back).
        assert_eq!(
            Policy::parse("OK contract=2,3"),
            Err(ReplyError::Missing("gen"))
        );
        assert!(!rostered("OK contract=2,3", "example-alab"));
    }

    const SID: &str = "s-4d7a2a97";
    const ID: &str = "example-alab";
    const ME: &str = "example-alab@4242";

    /// A reply cut from a running aterm (2026-11-12), text keys shortened.
    const LIVE_POLICY: &str = "OK gen=1 effective=on why=enabled contract=2,3 codex_rotation=1 \
        enabled=true headless=true autopilot=cruise land_at=- approve=all trust_roots=- \
        answer_text=Nobody%20is%20here. continue=true continue_per_hour=0 relaunch=true \
        stall_term_after_s=600 stall_term=true memory_ceiling=auto upgrade=true \
        upgrade_ask=false human_grace_s=120 occupancy=observe facts=true questions=recommended";

    /// A reply cut from a running aterm (2026-11-12) with this harness's proven
    /// claim written in where its built-in holder stood.
    const LIVE_STATUS: &str = "OK schema=1 sid=16 subject=%E2%97%90%20overview observed=true \
        phase=running hold=0 hold_advisory=- fabric=connected hand=- hand_pid=- level=quiet \
        needs=- needs_n=- hold_by=- typed_by=aterm program=example agent=idle agent_detail=- \
        agent_rev=30 composer=empty program_state=- integration=on input=clear input_bytes=0 \
        input_wait_ms=0 supervisor_contract=3 supervisor_scope=relaunch,upgrade \
        supervisor_revision=136 supervisor=example-alab@4242 supervisor_proven=1 \
        supervisor_version=0.1.0 supervisor_vendor=2.1.311 supervisor_act=on autopilot=cruise \
        human_ms=2154931 human_typed_ms=2154931 human_seq=1474 upgrade=- need=- \
        need_since_ms=- gen=1.90262 seq=90262 hash=5633b312ee9f9b16";

    fn policy() -> Policy {
        Policy::parse(LIVE_POLICY).expect("live policy")
    }

    fn status() -> Status {
        Status::parse(LIVE_STATUS).expect("live status")
    }

    /// `reply` with the word for `key` replaced (`Some`) or removed (`None`).
    fn with(reply: &str, key: &str, value: Option<&str>) -> String {
        let prefix = format!("{key}=");
        let mut found = false;
        let mut words: Vec<String> = Vec::new();
        for word in reply.split_ascii_whitespace() {
            if word.starts_with(&prefix) {
                found = true;
                if let Some(value) = value {
                    words.push(format!("{prefix}{value}"));
                }
            } else {
                words.push(word.to_string());
            }
        }
        assert!(found, "{key} is not in the reply");
        words.join(" ")
    }

    #[test]
    fn a_live_policy_reply_parses_and_unknown_keys_are_passed_over() {
        let policy = policy();
        assert_eq!(
            policy,
            Policy {
                generation: 1,
                contracts: vec![2, 3],
                effective: true,
                why: Some("enabled".to_string()),
                relaunch: true,
                upgrade: true,
                upgrade_ask: false,
                human_grace: Duration::from_secs(120),
            }
        );
        assert!(policy.serves_contract() && policy.admits_claim());
        let more = format!("{LIVE_POLICY} bare-word a_new_key=1 =x");
        assert_eq!(Policy::parse(&more), Ok(policy));
    }

    #[test]
    fn a_policy_off_by_configuration_is_told_from_one_off_for_now() {
        for why in [
            "unhosted",
            "disabled",
            "headless",
            "agent-off",
            "autopilot-off",
        ] {
            let reply = with(
                &with(LIVE_POLICY, "effective", Some("off")),
                "why",
                Some(why),
            );
            let off = Policy::parse(&reply).unwrap();
            assert!(off.off_by_configuration(), "{why}");
            assert_eq!(off.off_words(), format!("effective=off why={why}"));
        }
        for why in [Some("hold"), Some("suspended"), Some("a-new-reason"), None] {
            let reply = with(&with(LIVE_POLICY, "effective", Some("off")), "why", why);
            let off = Policy::parse(&reply).unwrap();
            assert!(
                !off.off_by_configuration() && !off.admits_claim(),
                "{why:?}"
            );
        }
        assert_eq!(
            Policy::parse(&with(
                &with(LIVE_POLICY, "effective", Some("off")),
                "why",
                None
            ))
            .unwrap()
            .off_words(),
            "effective=off"
        );
        // On is never off by configuration, whatever `why=` says.
        let on = Policy::parse(&with(LIVE_POLICY, "why", Some("disabled"))).unwrap();
        assert!(on.effective && !on.off_by_configuration());
        assert_eq!(
            Policy::parse(&format!("{LIVE_POLICY} why=hold")),
            Err(ReplyError::Repeated("why"))
        );
    }

    #[test]
    fn a_policy_that_is_off_or_older_parses_but_admits_no_claim() {
        let off = Policy::parse(&with(LIVE_POLICY, "effective", Some("off"))).unwrap();
        assert!(off.serves_contract() && !off.effective && !off.admits_claim());
        let older = Policy::parse(&with(LIVE_POLICY, "contract", Some("2"))).unwrap();
        assert!(!older.serves_contract() && older.effective && !older.admits_claim());
    }

    #[test]
    fn a_policy_missing_repeating_or_garbling_a_needed_field_is_refused() {
        for key in [
            "gen",
            "contract",
            "effective",
            "relaunch",
            "upgrade",
            "upgrade_ask",
            "human_grace_s",
        ] {
            assert_eq!(
                Policy::parse(&with(LIVE_POLICY, key, None)),
                Err(ReplyError::Missing(key)),
                "{key} absent"
            );
            assert_eq!(
                Policy::parse(&format!("{LIVE_POLICY} {key}=1")),
                Err(ReplyError::Repeated(key)),
                "{key} repeated"
            );
            for bad in ["", "x", "01", "-1", "TRUE", "On", "yes", "3,", "1.5"] {
                assert_eq!(
                    Policy::parse(&with(LIVE_POLICY, key, Some(bad))),
                    Err(ReplyError::Malformed(key)),
                    "{key}={bad}"
                );
            }
        }
        for (reply, why) in [
            ("", "reply=(empty)"),
            ("ERR denied", "reply=ERR:denied"),
            ("ERR no such session: s-1", "reply=ERR:no"),
            ("OKAY gen=1", "reply=OKAY:gen=1"),
            ("OK effective=on", "gen=absent"),
            ("OK\u{7}x effective=on", "reply=OK%07x:effective=on"),
        ] {
            assert_eq!(
                Policy::parse(reply).unwrap_err().to_string(),
                why,
                "{reply:?}"
            );
        }
        assert_eq!(ReplyError::Malformed("gen").to_string(), "gen=malformed");
        assert_eq!(ReplyError::Repeated("gen").to_string(), "gen=repeated");
    }

    #[test]
    fn a_live_status_reply_parses_leniently() {
        let status = status();
        assert_eq!(
            status,
            Status {
                supervisor: Some(ME.to_string()),
                supervisor_proven: true,
                supervisor_scope: vec!["relaunch".to_string(), "upgrade".to_string()],
                supervisor_revision: Some(136),
                agent: Some("idle".to_string()),
                composer: Some("empty".to_string()),
                human: Human::Ago(Duration::from_millis(2_154_931)),
                hand: Some("-".to_string()),
                hold: Some("0".to_string()),
                input: Some("clear".to_string()),
                need: Some("-".to_string()),
            }
        );
        assert!(status.held_by(ME) && !status.held_by("example-alab@4243"));
        assert!(status.takes(Scope::Upgrade) && status.takes(Scope::Relaunch));
        // The last word for a key wins, and bare words and empty keys are skipped.
        let later = format!("{LIVE_STATUS} bare =x agent=working hold=1");
        let later = Status::parse(&later).unwrap();
        assert_eq!(later.agent.as_deref(), Some("working"));
        assert_eq!(later.hold.as_deref(), Some("1"));
        // A holder aterm percent-encodes reads the same.
        let encoded = with(LIVE_STATUS, "supervisor", Some("example-alab%404242"));
        assert!(Status::parse(&encoded).unwrap().held_by(ME));
    }

    #[test]
    fn a_status_reply_states_no_claim_and_no_fact_it_does_not_carry() {
        let bare = Status::parse("OK schema=1").unwrap();
        assert_eq!(
            bare,
            Status {
                supervisor: None,
                supervisor_proven: false,
                supervisor_scope: Vec::new(),
                supervisor_revision: None,
                agent: None,
                composer: None,
                human: Human::Unknown,
                hand: None,
                hold: None,
                input: None,
                need: None,
            }
        );
        assert!(!bare.held_by(ME) && !bare.takes(Scope::Upgrade));
        let unclaimed = Status::parse(
            "OK schema=1.2 supervisor=- supervisor_proven=0 supervisor_scope=- \
             supervisor_revision=7 human_ms=-",
        )
        .unwrap();
        assert_eq!(unclaimed.supervisor, None);
        assert!(unclaimed.supervisor_scope.is_empty());
        assert_eq!(unclaimed.supervisor_revision, Some(7));
        assert_eq!(unclaimed.human, Human::Never);
        // A value outside its grammar is no claim, no proof and no reading.
        let garbled =
            |key: &str, value: &str| Status::parse(&with(LIVE_STATUS, key, Some(value))).unwrap();
        assert_eq!(garbled("supervisor", "%zz").supervisor, None);
        assert_eq!(garbled("supervisor", "").supervisor, None);
        assert!(!garbled("supervisor_proven", "true").held_by(ME));
        assert!(!garbled("supervisor_proven", "01").held_by(ME));
        assert_eq!(
            garbled("supervisor_revision", "x").supervisor_revision,
            None
        );
        for value in ["soon", "-5", "007", ""] {
            assert_eq!(
                garbled("human_ms", value).human,
                Human::Unknown,
                "{value:?}"
            );
        }
        for (reply, why) in [
            ("ERR no such session", "reply=ERR:no"),
            ("", "reply=(empty)"),
            ("OK sid=1", "schema=absent"),
            ("OK schema=2", "schema=malformed"),
            ("OK schema=", "schema=malformed"),
        ] {
            assert_eq!(
                Status::parse(reply).unwrap_err().to_string(),
                why,
                "{reply:?}"
            );
        }
    }

    /// A harness's config with `scopes` and `version`.
    fn config(scopes: &[Scope], version: &str) -> Config {
        Config::new(ID, "example", scopes, version).expect("a config")
    }

    fn vendor(vendor: &str) -> Child<'_> {
        Child {
            vendor: Some(vendor),
            login: None,
            process: None,
        }
    }

    #[test]
    fn the_holder_and_the_claim_text_are_exactly_contract_3s() {
        assert_eq!(holder(ID, 4242), ME);
        let upgrade = config(&[Scope::Upgrade], "0.1.0");
        assert_eq!(upgrade.holder(4242), ME);
        assert_eq!(
            claim_verb(&upgrade, Contract::Three, SID, 4242, vendor("2.1.311")).as_deref(),
            Some(
                "@s-4d7a2a97 meta set supervisor example-alab@4242 contract=3 scope=upgrade \
                 version=0.1.0 vendor=2.1.311"
            )
        );
        // Scopes are spelled once each, in aterm's order, however they are given.
        let both = config(
            &[Scope::Upgrade, Scope::Relaunch, Scope::Upgrade],
            "0.1.0+g1a2b3c4",
        );
        assert_eq!(
            claim_verb(&both, Contract::Three, SID, 4242, Child::default()).as_deref(),
            Some(
                "@s-4d7a2a97 meta set supervisor example-alab@4242 contract=3 \
                 scope=relaunch,upgrade version=0.1.0+g1a2b3c4"
            )
        );
        assert_eq!(
            scope_words(&[Scope::Walls, Scope::TurnEnd, Scope::Relaunch]),
            "relaunch,turn-end,walls"
        );
        // A vendor word aterm would refuse the whole claim over is left out.
        for bad in ["", "2.1 311", "2.1.311\n", "caf\u{e9}", &"9".repeat(65)] {
            assert_eq!(
                claim_verb(&upgrade, Contract::Three, SID, 4242, vendor(bad)).as_deref(),
                Some(
                    "@s-4d7a2a97 meta set supervisor example-alab@4242 contract=3 \
                     scope=upgrade version=0.1.0"
                ),
                "{bad:?}"
            );
        }
        assert!(
            claim_verb(
                &upgrade,
                Contract::Three,
                SID,
                4242,
                vendor("2.1.311-beta_1+x")
            )
            .unwrap()
            .ends_with(" vendor=2.1.311-beta_1+x")
        );
    }

    #[test]
    fn a_login_is_sent_only_as_an_account_label_aterm_reads() {
        let upgrade = config(&[Scope::Upgrade], "0.1.0");
        let with_login = |login: &str| {
            claim_verb(
                &upgrade,
                Contract::Three,
                SID,
                4242,
                Child {
                    vendor: Some("2.5.1"),
                    login: Some(login),
                    process: None,
                },
            )
            .unwrap()
        };
        for good in ["work", "a", "0x", "me.home_2-b", &"a".repeat(64)] {
            assert!(
                with_login(good).ends_with(&format!(" vendor=2.5.1 login={good}")),
                "{good:?}"
            );
        }
        for bad in [
            "",
            "help",
            "Work",
            "-work",
            ".work",
            "_work",
            "wo rk",
            "w@rk",
            "caf\u{e9}",
            &"a".repeat(65),
        ] {
            assert!(with_login(bad).ends_with(" vendor=2.5.1"), "{bad:?}");
        }
    }

    #[test]
    fn nothing_is_sent_for_a_claim_aterm_could_not_read() {
        let upgrade = config(&[Scope::Upgrade], "0.1.0");
        for pid in [0, 1] {
            assert_eq!(
                claim_verb(&upgrade, Contract::Three, SID, pid, Child::default()),
                None
            );
            assert_eq!(release_verb(&upgrade, SID, pid), None);
        }
        for sid in [
            "",
            "s-",
            "16",
            "s-xyz",
            "s-1 status",
            "s-1\n@s-2",
            "*",
            "../s-1",
        ] {
            assert_eq!(
                claim_verb(&upgrade, Contract::Three, sid, 4242, Child::default()),
                None
            );
            assert_eq!(release_verb(&upgrade, sid, 4242), None);
            assert_eq!(policy_verb(sid), None);
            assert_eq!(status_verb(sid), None);
            assert_eq!(info_verb(sid), None);
        }
    }

    #[test]
    fn a_version_aterm_could_not_read_is_no_config() {
        for version in [
            "",
            "0.1",
            "0.1.1",
            "0.1.0-rc1",
            "0.1.0+",
            "0.1.0+a..b",
            "0.1.0+a_b",
            "01.1.0",
            "0.01.0",
            "0.1.0.0",
            "v0.1.0",
            "0.1.0 x",
            "4294967296.0.0",
        ] {
            assert!(!claim_version(version), "{version:?}");
        }
        assert!(claim_version(&format!("1.2.0+{}", "a".repeat(122))));
        assert!(!claim_version(&format!("1.2.0+{}", "a".repeat(123))));
        assert!(claim_version("0.1.0") && claim_version("12.34.0+g1a2b3c4.dirty"));
    }

    #[test]
    fn the_release_and_the_reads_are_addressed_to_the_session() {
        assert_eq!(
            release_verb(&config(&[Scope::Upgrade], "0.1.0"), SID, 4242).as_deref(),
            Some("@s-4d7a2a97 meta unset supervisor holder=example-alab@4242")
        );
        assert_eq!(
            policy_verb(SID).as_deref(),
            Some("@s-4d7a2a97 harness policy")
        );
        assert_eq!(status_verb(SID).as_deref(), Some("@s-4d7a2a97 status"));
        assert_eq!(
            info_verb(SID).as_deref(),
            Some("@s-4d7a2a97 native-act info")
        );
    }

    #[test]
    fn every_scope_word_reads_back_and_names_the_contract_that_knows_it() {
        for scope in Scope::ALL {
            assert_eq!(Scope::from_word(scope.word()), Some(scope));
        }
        for word in ["", "-", "Relaunch", "turn_end", "telepathy"] {
            assert_eq!(Scope::from_word(word), None, "{word:?}");
        }
        let mut sorted = Scope::ALL;
        sorted.sort_unstable();
        assert_eq!(sorted, Scope::ALL, "the enum's order is aterm's");
        assert_eq!(
            Scope::ALL.map(Scope::word),
            [
                "relaunch",
                "upgrade",
                "turn-end",
                "approvals",
                "questions",
                "walls",
                "state",
                "accounts"
            ]
        );
        let since_4: Vec<Scope> = Scope::ALL
            .into_iter()
            .filter(|scope| scope.since() == Contract::Four)
            .collect();
        assert_eq!(since_4, [Scope::State, Scope::Accounts]);
    }

    /// Contract 3 names only the words aterm's roster rows allow; contract 4
    /// names every word asked for, since aterm drops what it does not know.
    #[test]
    fn under_contract_3_the_old_limit_stays_and_under_contract_4_it_goes() {
        let every = config(&Scope::ALL, "0.1.0");
        assert_eq!(
            Contract::Three.scopes(every.scopes()),
            Scope::CONTRACT_3.to_vec()
        );
        assert_eq!(Contract::Four.scopes(every.scopes()), Scope::ALL.to_vec());
        assert_eq!(
            claim_verb(&every, Contract::Three, SID, 4242, Child::default()).as_deref(),
            Some(
                "@s-4d7a2a97 meta set supervisor example-alab@4242 contract=3 \
                 scope=relaunch,upgrade version=0.1.0"
            )
        );
        assert_eq!(
            claim_verb(&every, Contract::Four, SID, 4242, Child::default()).as_deref(),
            Some(
                "@s-4d7a2a97 meta set supervisor example-alab@4242 contract=4 \
                 scope=relaunch,upgrade,turn-end,approvals,questions,walls,state,accounts \
                 version=0.1.0"
            )
        );
        // A harness asking only for contract-4 duties has no contract-3 claim.
        let state_only = config(&[Scope::State, Scope::Accounts], "0.1.0");
        assert_eq!(
            claim_verb(&state_only, Contract::Three, SID, 4242, Child::default()),
            None
        );
        assert!(claim_verb(&state_only, Contract::Four, SID, 4242, Child::default()).is_some());
    }

    /// `child=<pid>:<start>` goes between `vendor=` and `login=`, under
    /// contract 4 only, and only for a pid aterm could read.
    #[test]
    fn the_contract_4_claim_names_the_child_by_pid_and_start() {
        let both = config(&[Scope::Relaunch, Scope::Upgrade, Scope::State], "0.3.0");
        let child = Child {
            vendor: Some("2.1.311"),
            login: Some("work"),
            process: Some(ChildProcess {
                pid: 5151,
                start: 1_763_000_000_123_456,
            }),
        };
        assert_eq!(
            claim_verb(&both, Contract::Four, SID, 4242, child).as_deref(),
            Some(
                "@s-4d7a2a97 meta set supervisor example-alab@4242 contract=4 \
                 scope=relaunch,upgrade,state version=0.3.0 vendor=2.1.311 \
                 child=5151:1763000000123456 login=work"
            )
        );
        // Contract 3 has no `child=` and no `state`.
        assert_eq!(
            claim_verb(&both, Contract::Three, SID, 4242, child).as_deref(),
            Some(
                "@s-4d7a2a97 meta set supervisor example-alab@4242 contract=3 \
                 scope=relaunch,upgrade version=0.3.0 vendor=2.1.311 login=work"
            )
        );
        for pid in [0, 1] {
            let unreadable = Child {
                process: Some(ChildProcess { pid, start: 9 }),
                ..child
            };
            assert!(
                !claim_verb(&both, Contract::Four, SID, 4242, unreadable)
                    .unwrap()
                    .contains("child="),
                "{pid}"
            );
        }
        assert_eq!(
            ChildProcess { pid: 2, start: 0 }.word().as_deref(),
            Some("2:0")
        );
    }

    /// The reply names what was granted; the harness keeps what it asked for
    /// of that, passes over a word it does not know, and reads the revision.
    #[test]
    fn a_contract_4_reply_is_a_grant_narrower_or_equal_to_the_claim() {
        let asked = [Scope::Relaunch, Scope::Upgrade, Scope::State];
        let grant = Grant::of_reply("OK scope=relaunch,state revision=7", &asked).unwrap();
        assert_eq!(
            grant,
            Grant {
                contract: Contract::Four,
                scopes: vec![Scope::Relaunch, Scope::State],
                revision: Some(7),
            }
        );
        assert!(grant.takes(Scope::State) && !grant.takes(Scope::Upgrade));
        assert_eq!(grant.words(), "relaunch,state");
        assert_eq!(grant.revision_word(), "7");
        // Unknown words, words not asked for, and other reply words are passed
        // over; the order is aterm's.
        let odd = Grant::of_reply(
            "OK later=1 scope=telepathy,state,walls,relaunch,state revision=12 x",
            &asked,
        )
        .unwrap();
        assert_eq!(odd.scopes, [Scope::Relaunch, Scope::State]);
        assert_eq!(odd.revision, Some(12));
        for none in ["OK scope=- revision=3", "OK scope= revision=3"] {
            let empty = Grant::of_reply(none, &asked).unwrap();
            assert!(empty.scopes.is_empty(), "{none}");
            assert_eq!(empty.words(), "-");
        }
        for (reply, error) in [
            ("OK", ReplyError::Missing("scope")),
            ("OK scope=relaunch", ReplyError::Missing("revision")),
            ("OK revision=1", ReplyError::Missing("scope")),
            (
                "OK scope=relaunch revision=01",
                ReplyError::Malformed("revision"),
            ),
            (
                "OK scope=relaunch revision=-",
                ReplyError::Malformed("revision"),
            ),
            (
                "OK scope=relaunch scope=state revision=1",
                ReplyError::Repeated("scope"),
            ),
            (
                "OK scope=relaunch revision=1 revision=2",
                ReplyError::Repeated("revision"),
            ),
            ("ERR busy", ReplyError::Refused("ERR:busy".to_string())),
        ] {
            assert_eq!(Grant::of_reply(reply, &asked), Err(error), "{reply}");
        }
    }

    /// A grant read from `status`: the claim's stored scope words that were
    /// asked for, at the claim cell's revision.
    #[test]
    fn a_grant_reads_from_status_as_aterm_stores_it() {
        let status = |fields: &str| Status::parse(&format!("OK schema=1 {fields}")).unwrap();
        let asked = [Scope::Relaunch, Scope::Upgrade];
        let three = Grant::of_status(
            Contract::Three,
            &status("supervisor_scope=upgrade,relaunch,walls supervisor_revision=40"),
            &asked,
        );
        assert_eq!(
            three,
            Grant {
                contract: Contract::Three,
                scopes: asked.to_vec(),
                revision: Some(40),
            }
        );
        let unstated = Grant::of_status(Contract::Four, &status("supervisor_scope=-"), &asked);
        assert!(unstated.scopes.is_empty() && unstated.revision.is_none());
        assert_eq!(unstated.revision_word(), "-");
    }

    #[test]
    fn the_roster_guard_needs_this_harnesss_own_upgrade_row() {
        const INFO: &str = "OK native-act state=info epoch=d1a48b482e4c6b391c4715137ce246b6 \
            native-act=1 handoff=blocked clock=monotonic-us own-image=";
        for rows in [
            "example-alab:upgrade",
            "grok-alab:upgrade,muse-alab:upgrade,kimi-alab:upgrade,example-alab:upgrade",
            "example-alab:upgrade,grok-alab:upgrade",
        ] {
            assert!(rostered(&format!("{INFO}{rows}"), ID), "{rows}");
            assert!(
                rostered(&format!("{INFO}{rows} a_later_key=1"), ID),
                "{rows}"
            );
        }
        for rows in [
            // What aterm answers today (2026-11-12): the row is not shipped.
            "grok-alab:upgrade,muse-alab:upgrade,kimi-alab:upgrade",
            "-",
            "",
            "example-alab",
            "example-alab:relaunch",
            "example-alab:upgrade:x",
            "example-alabs:upgrade",
            "xexample-alab:upgrade",
            "Example-Alab:upgrade",
            "example-alab:upgrade;grok-alab:upgrade",
        ] {
            assert!(!rostered(&format!("{INFO}{rows}"), ID), "{rows:?}");
        }
        for reply in [
            "",
            "OK",
            "OK native-act state=info",
            "ERR usage: native-act",
            "ERR own-image=example-alab:upgrade",
            "OKAY own-image=example-alab:upgrade",
            "own-image=example-alab:upgrade",
            "OK own-image=example-alab:upgrade own-image=-",
            "OK own-image=- own-image=example-alab:upgrade",
            "OK own_image=example-alab:upgrade",
        ] {
            assert!(!rostered(reply, ID), "{reply:?}");
        }
    }

    #[test]
    fn a_restart_event_names_exactly_one_cause_and_an_account_move_whole() {
        let parse = |tail: &str| Restart::parse(tail.split_ascii_whitespace());
        let account = |label: &str, home: Option<&str>| {
            Some(Account {
                label: label.to_string(),
                home: home.map(PathBuf::from),
            })
        };
        for (tail, cause, moved) in [
            ("cause=asked", "asked", None),
            ("cause=stall", "stall", None),
            ("cause=asked a_later_key=1 bare", "asked", None),
            // An account cause need not move: aterm names the cause alone
            // when it has no row to move to.
            ("cause=account", "account", None),
            (
                "cause=account to=work home=%2FUsers%2F_owner",
                "account",
                account("work", Some("/Users/_owner")),
            ),
            (
                "cause=account-ahead home=%2Fh%2Fk%20b to=me.home_2-b",
                "account-ahead",
                account("me.home_2-b", Some("/h/k b")),
            ),
            (
                "cause=account-ahead to=work home=-",
                "account-ahead",
                account("work", None),
            ),
            (
                "cause=account to=b home=%2Fh later=1",
                "account",
                account("b", Some("/h")),
            ),
        ] {
            assert_eq!(
                parse(tail),
                Some(Restart {
                    cause: cause.to_string(),
                    account: moved,
                    model: None,
                }),
                "{tail}"
            );
        }
        for tail in [
            "",
            "cause=",
            "asked",
            "to=work cause=account",
            "cause=asked cause=stall",
            "cause=a/b",
            "cause=a%20b",
            "Cause=asked",
            // A move is whole: both words, once each, readable, and only
            // for an account cause.
            "cause=account to=work",
            "cause=account home=%2Fh",
            "cause=account to=work to=play home=%2Fh",
            "cause=account to=work home=%2Fh home=-",
            "cause=asked to=work home=%2Fh",
            "cause=stall to=work home=-",
            "cause=account to=Work home=%2Fh",
            "cause=account to=help home=%2Fh",
            "cause=account to= home=%2Fh",
            "cause=account to=work home=relative%2Fh",
            "cause=account to=work home=",
            "cause=account to=work home=%zz",
            "cause=account to=work home=%FF",
            // A model is named once, as a model word, and never in `to=`.
            "cause=model to=gemini-3-pro home=-",
            "cause=model model=a model=b",
            "cause=model model=",
            "cause=model model=-flag",
            "cause=model model=a%20b",
            "cause=model model=a,b",
        ] {
            assert_eq!(parse(tail), None, "{tail:?}");
        }
        assert_eq!(
            parse(&format!("cause={}", "a".repeat(64))).map(|r| r.cause.len()),
            Some(64)
        );
        assert_eq!(parse(&format!("cause={}", "a".repeat(65))), None);
    }

    /// aterm's model move (HX-06): `restart cause=model model=<word>`, and a
    /// model named beside any other cause, an account move's included. A
    /// later word aterm adds is still passed over.
    #[test]
    fn a_restart_names_the_model_to_start_the_next_child_on() {
        let parse = |tail: &str| Restart::parse(tail.split_ascii_whitespace());
        for (tail, cause, account, model) in [
            (
                "cause=model model=gemini-3-pro",
                "model",
                None,
                "gemini-3-pro",
            ),
            (
                "cause=model model=opus[1m] later=1",
                "model",
                None,
                "opus[1m]",
            ),
            (
                "cause=model model=us.anthropic.claude-x:0/b@c_d",
                "model",
                None,
                "us.anthropic.claude-x:0/b@c_d",
            ),
            ("cause=stall model=k2", "stall", None, "k2"),
            (
                "cause=account model=m to=b home=-",
                "account",
                Some(Account {
                    label: "b".to_string(),
                    home: None,
                }),
                "m",
            ),
        ] {
            assert_eq!(
                parse(tail),
                Some(Restart {
                    cause: cause.to_string(),
                    account,
                    model: Some(model.to_string()),
                }),
                "{tail}"
            );
        }
        // A model move with no model named is a restart on the same model.
        assert_eq!(
            parse("cause=model"),
            Some(Restart {
                cause: "model".to_string(),
                account: None,
                model: None,
            })
        );
        assert!(model_word(&"m".repeat(MODEL_MAX)));
        assert!(!model_word(&"m".repeat(MODEL_MAX + 1)));
        assert_eq!(
            parse(&format!("cause=model model={}", "m".repeat(MODEL_MAX + 1))),
            None
        );
    }

    /// Every model aterm's identifier check (`is_model_id`: 1 to 256 bytes of
    /// alphanumerics and `._-:@/[]`, any first byte) would send is read,
    /// except one starting with `-`, which a vendor would take as an option.
    /// NEGATIVE CONTROL: a byte outside that set, an empty word and a word
    /// past 256 bytes are not model words.
    #[test]
    fn a_model_word_is_any_model_aterm_would_send_but_an_option() {
        let is_model_id = |m: &str| {
            (1..=256).contains(&m.len())
                && m.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-:@/[]".contains(&b))
        };
        let long = "m".repeat(200);
        for model in [
            long.as_str(),
            "m",
            ".hidden",
            "_private",
            "@cf/meta/llama",
            ":tag",
            "/models/x",
            "[1m]",
            "opus[1m]",
            "a-",
        ] {
            assert!(is_model_id(model) && model_word(model), "{model}");
            assert_eq!(
                Restart::parse(format!("cause=model model={model}").split_ascii_whitespace())
                    .and_then(|restart| restart.model)
                    .as_deref(),
                Some(model),
                "{model}"
            );
        }
        // aterm's other reader's words all pass.
        assert!(model_word(&format!("a{}", "-_.:/".repeat(25))));
        assert!(is_model_id("-flag") && !model_word("-flag"));
        assert!(!model_word("--model=x"));
        for model in ["", "a b", "a,b", "a%20b", "a=b", "\u{e9}"] {
            assert!(!is_model_id(model) && !model_word(model), "{model:?}");
        }
        let past = "m".repeat(257);
        assert!(!is_model_id(&past) && !model_word(&past));
    }

    fn held(policy: &Policy, status: &Status) -> String {
        match judge_safe_point(policy, status, ME, Duration::ZERO) {
            SafePoint::Safe => "safe".to_string(),
            SafePoint::Hold(hold) => hold.to_string(),
        }
    }

    #[test]
    fn a_quiet_proven_session_is_a_safe_point() {
        let verdict = judge_safe_point(&policy(), &status(), ME, Duration::ZERO);
        assert_eq!(verdict, SafePoint::Safe);
        assert!(verdict.is_safe());
        for (key, value) in [
            ("composer", "placeholder"),
            ("human_ms", "-"),
            ("human_ms", "120000"),
        ] {
            let status = Status::parse(&with(LIVE_STATUS, key, Some(value))).unwrap();
            assert_eq!(held(&policy(), &status), "safe", "{key}={value}");
        }
        // A claim that also took relaunch, or only upgrade, is still ours to move under.
        let narrow =
            Status::parse(&with(LIVE_STATUS, "supervisor_scope", Some("upgrade"))).unwrap();
        assert_eq!(held(&policy(), &narrow), "safe");
    }

    #[test]
    fn each_status_field_holds_alone_and_is_journalled_by_name() {
        for (key, value, line) in [
            ("supervisor", Some("-"), "wait aterm supervisor=-"),
            ("supervisor", None, "wait aterm supervisor=-"),
            (
                "supervisor",
                Some("example-alab@4243"),
                "wait aterm supervisor=example-alab@4243",
            ),
            (
                "supervisor",
                Some("aterm-harness@40450"),
                "wait aterm supervisor=aterm-harness@40450",
            ),
            (
                "supervisor",
                Some("a%20b%0A"),
                "wait aterm supervisor=a%20b%0A",
            ),
            (
                "supervisor_proven",
                Some("0"),
                "wait aterm supervisor_proven=0",
            ),
            ("supervisor_proven", None, "wait aterm supervisor_proven=0"),
            (
                "supervisor_scope",
                Some("-"),
                "wait aterm supervisor_scope=-",
            ),
            ("supervisor_scope", None, "wait aterm supervisor_scope=-"),
            (
                "supervisor_scope",
                Some("relaunch"),
                "wait aterm supervisor_scope=relaunch",
            ),
            ("agent", Some("working"), "wait aterm agent=working"),
            ("agent", Some("unknown"), "wait aterm agent=unknown"),
            ("agent", Some(""), "wait aterm agent=(empty)"),
            ("agent", None, "wait aterm agent=absent"),
            ("composer", Some("draft"), "wait aterm composer=draft"),
            ("composer", None, "wait aterm composer=absent"),
            ("human_ms", Some("119999"), "wait aterm human_ms=119999"),
            ("human_ms", Some("0"), "wait aterm human_ms=0"),
            ("human_ms", Some("soon"), "wait aterm human_ms=unknown"),
            ("human_ms", None, "wait aterm human_ms=unknown"),
            (
                "hand",
                Some("lease:someone"),
                "wait aterm hand=lease:someone",
            ),
            ("hand", Some("turn:7:s-12"), "wait aterm hand=turn:7:s-12"),
            ("hand", None, "wait aterm hand=absent"),
            ("hold", Some("1"), "wait aterm hold=1"),
            ("hold", None, "wait aterm hold=absent"),
            ("input", Some("unread"), "wait aterm input=unread"),
            ("input", None, "wait aterm input=absent"),
            ("need", Some("question"), "wait aterm need=question"),
            ("need", None, "wait aterm need=absent"),
        ] {
            let status = Status::parse(&with(LIVE_STATUS, key, value)).unwrap();
            assert_eq!(held(&policy(), &status), line, "{key}={value:?}");
        }
        // Nothing but the schema: the first unmet condition is the claim.
        let bare = Status::parse("OK schema=1").unwrap();
        assert_eq!(held(&policy(), &bare), "wait aterm supervisor=-");
    }

    #[test]
    fn each_policy_field_holds_alone() {
        for (key, value, line) in [
            ("contract", "2", "wait aterm contract=2"),
            ("contract", "2,5", "wait aterm contract=2,5"),
            ("effective", "off", "wait aterm effective=off"),
            ("upgrade", "false", "wait aterm upgrade=false"),
            ("upgrade_ask", "true", "wait aterm upgrade_ask=true"),
        ] {
            let policy = Policy::parse(&with(LIVE_POLICY, key, Some(value))).unwrap();
            assert_eq!(held(&policy, &status()), line, "{key}={value}");
        }
        // `relaunch=false` limits the crash relaunch, never a move.
        let policy = Policy::parse(&with(LIVE_POLICY, "relaunch", Some("false"))).unwrap();
        assert_eq!(held(&policy, &status()), "safe");
    }

    #[test]
    fn the_first_unmet_condition_is_the_one_reported() {
        // Everything is wrong at once; each repair uncovers the next condition.
        let mut policy_reply = with(
            &with(LIVE_POLICY, "upgrade", Some("false")),
            "upgrade_ask",
            Some("true"),
        );
        let mut reply = LIVE_STATUS.to_string();
        for (key, value) in [
            ("supervisor_proven", "0"),
            ("supervisor_scope", "relaunch"),
            ("agent", "working"),
            ("composer", "draft"),
            ("human_ms", "5"),
            ("hand", "lease:x"),
            ("hold", "1"),
            ("input", "unread"),
            ("need", "question"),
        ] {
            reply = with(&reply, key, Some(value));
        }
        let wrong = Policy::parse(&policy_reply).unwrap();
        for (key, good, line) in [
            ("supervisor_proven", "1", "wait aterm supervisor_proven=0"),
            (
                "supervisor_scope",
                "relaunch,upgrade",
                "wait aterm supervisor_scope=relaunch",
            ),
        ] {
            assert_eq!(held(&wrong, &Status::parse(&reply).unwrap()), line);
            reply = with(&reply, key, Some(good));
        }
        let status = Status::parse(&reply).unwrap();
        for (key, good, line) in [
            ("upgrade", "true", "wait aterm upgrade=false"),
            ("upgrade_ask", "false", "wait aterm upgrade_ask=true"),
        ] {
            assert_eq!(held(&Policy::parse(&policy_reply).unwrap(), &status), line);
            policy_reply = with(&policy_reply, key, Some(good));
        }
        let policy = Policy::parse(&policy_reply).unwrap();
        for (key, good, line) in [
            ("agent", "idle", "wait aterm agent=working"),
            ("composer", "empty", "wait aterm composer=draft"),
            ("human_ms", "-", "wait aterm human_ms=5"),
            ("hand", "-", "wait aterm hand=lease:x"),
            ("hold", "0", "wait aterm hold=1"),
            ("input", "clear", "wait aterm input=unread"),
            ("need", "-", "wait aterm need=question"),
        ] {
            assert_eq!(held(&policy, &Status::parse(&reply).unwrap()), line);
            reply = with(&reply, key, Some(good));
        }
        assert_eq!(held(&policy, &Status::parse(&reply).unwrap()), "safe");
    }

    #[test]
    fn the_persons_grace_is_aterms_or_longer_and_says_how_long_is_left() {
        let typed = Status::parse(&with(LIVE_STATUS, "human_ms", Some("90000"))).unwrap();
        let SafePoint::Hold(hold) = judge_safe_point(&policy(), &typed, ME, Duration::ZERO) else {
            panic!("typing 90 s ago is inside a 120 s grace");
        };
        assert_eq!((hold.field, hold.value.as_str()), ("human_ms", "90000"));
        assert_eq!(hold.grace_left, Some(Duration::from_secs(30)));
        // The caller's own grace lengthens aterm's and never shortens it.
        let aged = Status::parse(&with(LIVE_STATUS, "human_ms", Some("150000"))).unwrap();
        assert!(judge_safe_point(&policy(), &aged, ME, Duration::from_secs(1)).is_safe());
        let SafePoint::Hold(hold) =
            judge_safe_point(&policy(), &aged, ME, Duration::from_secs(200))
        else {
            panic!("150 s is inside the caller's 200 s grace");
        };
        assert_eq!(hold.grace_left, Some(Duration::from_secs(50)));
        // A grace of zero is a policy aterm may state: only `-` and any age pass.
        let none = Policy::parse(&with(LIVE_POLICY, "human_grace_s", Some("0"))).unwrap();
        let now = Status::parse(&with(LIVE_STATUS, "human_ms", Some("0"))).unwrap();
        assert!(judge_safe_point(&none, &now, ME, Duration::ZERO).is_safe());
        // Any other hold carries no grace.
        let busy = Status::parse(&with(LIVE_STATUS, "agent", Some("working"))).unwrap();
        let SafePoint::Hold(hold) = judge_safe_point(&policy(), &busy, ME, Duration::ZERO) else {
            panic!("a working agent holds");
        };
        assert_eq!(hold.grace_left, None);
    }

    #[test]
    fn a_long_or_hostile_value_is_one_bounded_journal_word() {
        let long = "x".repeat(300);
        let status = Status::parse(&with(LIVE_STATUS, "agent", Some(&long))).unwrap();
        let line = held(&policy(), &status);
        assert_eq!(line, format!("wait aterm agent={}", "x".repeat(64)));
        let odd = Status::parse(&with(LIVE_STATUS, "need", Some("caf\u{e9}%"))).unwrap();
        assert_eq!(held(&policy(), &odd), "wait aterm need=caf%C3%A9%25");
        assert!(!held(&policy(), &odd).contains(char::is_control));
    }
}
