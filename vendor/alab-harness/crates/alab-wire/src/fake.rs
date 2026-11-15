// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A stand-in for aterm's control socket, for a harness's tests (cargo feature
//! `fake`), and the proof that it and this crate's client agree.
//!
//! [`FakeAterm`] lays what a harness inside an aterm tab finds: the rendezvous
//! directory's `graph/<sid>` entry, a socket in a directory short enough for a
//! Unix socket path, and the token beside it. It answers the five requests a
//! client makes (`harness policy`, `native-act info`, `meta set
//! supervisor`, `meta unset supervisor`, `status`) from state a scenario changes
//! as it goes, and streams `subscribe @<sid> events` with aterm's numeric tags
//! and the ACK aterm was measured to send on 2026-11-13 ([`subscribe_ack`]).
//!
//! It proves nothing itself. A claim is stored when the settable claim reply is
//! `OK`, its words read as aterm reads them (a rostered holder's scope words
//! inside its row included), and no other holder stands; and
//! `status` shows it proven while the settable proven bit is set. As in aterm,
//! a claim is bound to the connection that made it and is dropped when that
//! connection closes, and a halt (`status hold=1`) reads in the policy as
//! `effective=off why=hold`, with no generation move and no event
//! (`harness_policy_verb.rs`, `effective`).
//!
//! The wire text is aterm's at `d29d3887c4d`: the policy line of
//! `crates/aterm-gui/src/harness_policy_verb.rs`, the claim replies of
//! `crates/aterm-gui/src/control_session.rs` (`cmd_meta_supervisor_serving`,
//! `claim_reply`) and the claim grammar of
//! `crates/aterm-agent/src/harness/contract.rs` (`read_in`), the status words of
//! `crates/aterm-gui/src/session_status.rs` (`native_supervisor_fields`) and
//! `session_timeline.rs` (`wire_fields`), the info line of
//! `crates/aterm-gui/src/native_act.rs` (`Request::Info`), the subscription ACK
//! of `crates/aterm-gui/src/control.rs`, and the restart event of
//! `crates/aterm-gui/src/restart_act.rs` (`record_restart`), with HX-06's
//! `model=` ([`FakeAterm::send_model_restart`]).
//!
//! **Contracts.** It serves the uncontracted (v2) claim and contracts 3 and 4
//! (`contract=2,3,4`, [`POLICY_ON`]); [`FakeAterm::serve_contracts`] narrows
//! the set, and a claim naming a contract the policy does not list is refused.
//! A claim is read by [`ClaimWords::read`] and stored by the arm of
//! `State::claim` for its `contract=`.
//!
//! Contract 4 is section 2.2 of aterm's harness interface design
//! (`aterm/docs/DESIGN-harness-interface-2026-11-12.md`): a claim names
//! `scope=` and `version=`, and may name `vendor=`, `child=<pid>:<start>` and
//! `login=`. A scope word the fake does not know ([`FakeAterm::set_known`]) is
//! dropped and the rest are granted, with no roster row consulted, less any a
//! person's cap withholds ([`FakeAterm::set_cap`]); the reply is
//! `OK scope=<granted> revision=<n>`, `scope=-` for none. A cap narrows every
//! live contracted claim (contract 3 too), raises the revision and sends
//! `harness-policy`. The
//! last `child=` a claim named is kept ([`FakeAterm::child`]). A holder no
//! registry row proves ([`FakeAterm::set_registered`]) is answered
//! `ERR unproven` and nothing is stored, as aterm will answer it. Which wire
//! text aterm gives a refused contract-4 claim is not designed yet; the fake's
//! is its own.
//!
//! **The session stream.** [`FakeAterm::serve_stream`] serves `stream @<sid> v=1` on the
//! connection that holds a contract-4 claim, as aterm does from HX-15b1 (the HX-15 plan's
//! D4, D5 and D8) and sends its own records as it does from HX-15c. Until then the verb is
//! answered [`STREAM_UNKNOWN`], as by every aterm before HX-15b1, and the connection stays in
//! request mode with its claim; [`FakeAterm::set_stream_reply`] gives any other answer, such
//! as `ERR busy streams`. A served stream answers `OK stream v=1 epoch=fake-<n> acked=<seq>
//! in=<seq>`, then sends the current `hand` and a `policy` record, and acknowledges each
//! harness record in order:
//!
//! * a line over [`crate::record::LINE_MAX`] is the next record, `dropped=bound`, unparsed;
//! * a `seq` at or below `acked` is `dropped=applied`, and one past `acked + 1` is a gap,
//!   which closes the stream;
//! * a record over the budget ([`FakeAterm::set_stream_rate`]: [`crate::record::RATE_BURST`]
//!   at once, [`crate::record::RATE_PER_S`] a second) is `dropped=rate`, and one the claim's
//!   grant does not cover ([`Body::admitted`], with no act table: an act by a rule is never
//!   covered) `dropped=scope`;
//! * a `claim` record is a claim placement (D7): the fake builds `meta set supervisor <holder>
//!   contract=4 …` from its words, the standing claim's for any it leaves out, and serves it on
//!   the stream's connection; a refusal is `dropped=refused:<the ERR word>`, and an `OK` is
//!   followed by a `policy` record, since the fake's revision moves with every claim;
//! * every other record is acknowledged with no `dropped=`, and `took` and `refused` are kept
//!   for a test to read ([`FakeAterm::took`], [`FakeAterm::refused`]).
//!
//! The stream's `acked` and `in` are the claimant's and outlive a stream that closed, as they
//! do through aterm's re-exec grace; an aterm restart ([`FakeAterm::restart`]) moves the epoch
//! and forgets them. A claim goes with the connection that made it, unless
//! [`FakeAterm::set_reexec_grace`] keeps a contract-4 claim through the grace (D28), which the
//! same holder's claim renews and [`FakeAterm::end_grace`] ends, forgetting the numbers. A policy
//! move, a cap and a restart are sent both as the event they were and, while a stream is open,
//! as a record, unless [`FakeAterm::set_stream_own_records`] makes it an aterm with HX-15b1
//! alone, which sends none, and [`FakeAterm::set_stream_applies_claims`] one that acknowledges a
//! `claim` record without placing it (D9). Each stream writes from its own thread, outside the
//! fake's lock, and its rate budget is full at each open. A stream is closed by
//! [`FakeAterm::close_stream`] (as aterm closes one on a gap or a claim placed elsewhere), by a
//! release, by a claim made on another connection, and by a restart.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::Shutdown;
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::claim::{Contract, Restart, Scope};
use crate::config::{Config, EnvNames};
use crate::record::{
    Body, Composer, Dropped, LINE_MAX, RATE_BURST, RATE_PER_S, Record, STREAM_VERSION,
};
use crate::wire::Locator;

/// The token every connection must present.
pub const TOKEN: &str = "fake-aterm-token";

/// A policy body (everything after `OK gen=<n> `) with contracts 2, 3 and 4
/// served, the policy in effect and every switch a move or a crash relaunch
/// needs on. The unrelated keys are a sample of the dozens aterm sends.
pub const POLICY_ON: &str = "effective=on why=enabled contract=2,3,4 enabled=true autopilot=cruise \
    relaunch=true stall_term=true upgrade=true upgrade_ask=false human_grace_s=120 \
    questions=recommended";

/// The head of a `native-act info` reply, before its `own-image=` rows.
const INFO_HEAD: &str = "OK native-act state=info epoch=d1a48b482e4c6b391c4715137ce246b6 \
    native-act=1 handoff=blocked clock=monotonic-us";

/// What `native-act info` answers when aterm's roster carries exactly the rows
/// `ids`, each allowing `upgrade` (`own-image=-` for none). A fake's own reply
/// is this over its rows ([`FakeAterm::set_row`]) unless
/// [`FakeAterm::set_info`] replaced it.
#[must_use]
pub fn info_with_rows(ids: &[&str]) -> String {
    let rows = ids
        .iter()
        .filter(|id| !id.is_empty())
        .map(|id| format!("{id}:upgrade"))
        .collect::<Vec<_>>();
    format!(
        "{INFO_HEAD} own-image={}",
        if rows.is_empty() {
            "-".to_string()
        } else {
            rows.join(",")
        }
    )
}

/// The `status` fields of a quiet session nobody is typing into. The claim's
/// own fields are added from the fake's claim state; `program=` is the
/// harness's id when the fake is made for a config ([`FakeAterm::for_harness`]).
pub const QUIET_STATUS: &[(&str, &str)] = &[
    ("hold", "0"),
    ("hand", "-"),
    ("agent", "idle"),
    ("composer", "empty"),
    ("input", "clear"),
    ("human_ms", "-"),
    ("need", "-"),
];

/// The subscription ACK aterm sends for `subscribe @<sid> events`, as measured
/// against a real aterm on 2026-11-13: `OK subscribe 1`, then one `sub` row
/// with aterm's local tag `1`, the session ID, and what aterm knows of the
/// session at that moment.
#[must_use]
pub fn subscribe_ack(sid: &str) -> String {
    format!("OK subscribe 1\nsub 1 {sid} role=- attention=- user_title=-\n")
}

/// The refusal aterm gives a claim whose words it cannot read
/// (`control_session.rs`, the `Words::Refused` arm).
fn usage(why: &str) -> String {
    format!(
        "ERR usage: meta set supervisor <id>@<pid> contract=3 scope=<word>[,<word>…] \
         [version=<MAJOR.MINOR.0>] [vendor=<version>]: {why}"
    )
}

/// The fake's answer to a contract-4 claim from a holder no registry row
/// proves. aterm answers such a holder `ERR unproven` and stores nothing
/// (HX-06, `contract.rs` `read_in`); the text after the colon is the fake's,
/// since aterm's is not designed.
pub const UNPROVEN_4: &str = "ERR unproven: no registry row proves this holder";

/// The fake's refusal of a contract-4 claim whose words it cannot read, with
/// the grammar of the interface design's section 2.2.
fn usage_4(why: &str) -> String {
    format!(
        "ERR usage: meta set supervisor <id>@<pid> contract=4 scope=<word>[,<word>…] \
         version=<v> [vendor=<v>] [child=<pid>:<start>] [login=<account>]: {why}"
    )
}

/// What an aterm before HX-15b1 answers `stream @<sid> v=1`: a verb it does not know.
pub const STREAM_UNKNOWN: &str = "ERR unknown verb";

/// A token bucket: `burst` records at once, `per_s` more each second.
#[derive(Debug)]
struct Bucket {
    burst: u64,
    per_s: u64,
    /// Thousandths of a record.
    milli: u64,
    since: Instant,
}

impl Bucket {
    fn new(burst: u32, per_s: u32) -> Self {
        Self {
            burst: u64::from(burst),
            per_s: u64::from(per_s),
            milli: u64::from(burst) * 1_000,
            since: Instant::now(),
        }
    }

    /// Spend one record, if the budget holds one.
    fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = u64::try_from(now.duration_since(self.since).as_millis()).unwrap_or(u64::MAX);
        self.since = now;
        self.milli = self
            .milli
            .saturating_add(elapsed.saturating_mul(self.per_s))
            .min(self.burst * 1_000);
        if self.milli < 1_000 {
            return false;
        }
        self.milli -= 1_000;
        true
    }
}

/// Milliseconds since 1970, for the `ms` of the fake's own records (only shown).
fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The session stream as aterm serves it: its knobs, the claimant's numbers, the open stream and
/// what was read on it.
#[derive(Debug)]
struct Streams {
    /// Whether `stream @<sid> v=1` is served; otherwise it is answered [`STREAM_UNKNOWN`].
    serve: bool,
    /// A whole reply sent in place of the stream's `OK`, the connection kept in request mode.
    reply: Option<String>,
    /// The aterm instance: the stream's `epoch=fake-<n>`.
    epoch: u64,
    /// Whose numbers `acked` and `sent` are: the claimant that last opened a stream.
    holder: Option<String>,
    /// The last harness record acknowledged.
    acked: u64,
    /// The last record the fake sent on a stream (`in`).
    sent: u64,
    /// The open stream: its connection, the socket (to shut it down), and the queue its own
    /// writer thread writes from, in order, outside the state's lock.
    open: Option<(u64, UnixStream, Sender<String>)>,
    /// How many `stream` requests were answered, whatever the answer.
    asked: usize,
    /// The connection each stream was opened on, in order.
    opened: Vec<u64>,
    /// Every harness record read, applied or not, in arrival order.
    records: Vec<Record>,
    /// Every acknowledgement sent: the `seq` and its `dropped=`.
    acks: Vec<(u64, Option<Dropped>)>,
    /// The `in` of every `took` read.
    took: Vec<u64>,
    /// The `in` and `why` of every `refused` read.
    refused: Vec<(u64, String)>,
    /// Whether lines are read and dropped unacknowledged, as by a reader that stopped.
    deaf: bool,
    /// Reasons the next harness records are acknowledged with, applying nothing.
    drop_next: VecDeque<Dropped>,
    /// The rate budget of the stream now open: `rate_burst` and `rate_per_s`, full at each open.
    rate: Bucket,
    /// The burst of each stream's rate budget.
    rate_burst: u32,
    /// The refill of each stream's rate budget, a second.
    rate_per_s: u32,
    /// Whether the fake sends records of its own (HX-15c): the current `hand` and a `policy`
    /// at every open, `policy` on a policy move or a placement, `restart` with every restart.
    own_records: bool,
    /// Whether a `claim` record is placed as a claim (HX-15b2), or acknowledged and not applied,
    /// as by an aterm with HX-15b1 alone (D9).
    applies_claims: bool,
    /// `claim` records acknowledged and not applied.
    unapplied: usize,
    /// Whether a contract-4 claim whose connection closed is kept until [`FakeAterm::end_grace`],
    /// as through aterm's re-exec grace (D28).
    grace: bool,
    /// The person's hand: `human_seq`, the composer and the halt.
    hand: (u64, Composer, bool),
}

impl Streams {
    fn new() -> Self {
        Self {
            serve: false,
            reply: None,
            epoch: 1,
            holder: None,
            acked: 0,
            sent: 0,
            open: None,
            asked: 0,
            opened: Vec::new(),
            records: Vec::new(),
            acks: Vec::new(),
            took: Vec::new(),
            refused: Vec::new(),
            deaf: false,
            drop_next: VecDeque::new(),
            rate: Bucket::new(RATE_BURST, RATE_PER_S),
            rate_burst: RATE_BURST,
            rate_per_s: RATE_PER_S,
            own_records: true,
            applies_claims: true,
            unapplied: 0,
            grace: false,
            hand: (0, Composer::Empty, false),
        }
    }

    /// Queue `line` and its newline for the open stream's writer. Whether one was open.
    fn write(&self, line: &str) -> bool {
        self.open
            .as_ref()
            .is_some_and(|(_, _, out)| out.send(format!("{line}\n")).is_ok())
    }

    /// Send one of aterm's records on the open stream, numbered in `in`. Whether one was open.
    fn emit(&mut self, body: Body) -> bool {
        if self.open.is_none() {
            return false;
        }
        self.sent += 1;
        let record = Record {
            seq: self.sent,
            ms: wall_ms(),
            body,
        };
        if let Ok(line) = record.line() {
            self.write(&line);
        }
        true
    }

    /// [`Streams::emit`] for a record the fake sends of its own accord, which an aterm without
    /// HX-15c does not send ([`FakeAterm::set_stream_own_records`]).
    fn emit_own(&mut self, body: Body) -> bool {
        self.own_records && self.emit(body)
    }

    /// Acknowledge harness record `seq`, moving `acked` to it.
    fn ack(&mut self, seq: u64, dropped: Option<Dropped>) {
        self.acked = self.acked.max(seq);
        self.acks.push((seq, dropped.clone()));
        let _ = self.emit_ack(seq, dropped);
    }

    /// Write an acknowledgement: it is not counted in `in`.
    fn emit_ack(&self, seq: u64, dropped: Option<Dropped>) -> bool {
        let record = Record {
            seq,
            ms: wall_ms(),
            body: Body::Ack { dropped },
        };
        record.line().is_ok_and(|line| self.write(&line))
    }

    /// Close the open stream: its connection is shut down, which ends its writer, and a claim
    /// bound to it goes.
    fn close(&mut self) {
        if let Some((_, out, _)) = self.open.take() {
            let _ = out.shutdown(Shutdown::Both);
        }
    }

    /// Forget the claimant's numbers, as aterm does when the claim they belong to ends.
    fn forget(&mut self) {
        self.holder = None;
        self.acked = 0;
        self.sent = 0;
    }
}

/// The word after `ERR` in a refusal, as `dropped=refused:<word>` names it.
fn refusal_word(reply: &str) -> String {
    reply
        .split_ascii_whitespace()
        .nth(1)
        .map(|word| word.trim_end_matches(':'))
        .filter(|word| crate::record::is_word(word) && !word.contains(':'))
        .unwrap_or("refused")
        .to_string()
}

/// The words of one `meta set supervisor <holder> …` request, read as aterm's
/// `contract.rs` `read_in` reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaimWords {
    /// The holder, `<id>@<pid>`.
    pub holder: String,
    /// `contract=`, when named.
    pub contract: Option<String>,
    /// `scope=`, when named.
    pub scope: Option<String>,
    /// `version=`, when named.
    pub version: Option<String>,
    /// `vendor=`, when named.
    pub vendor: Option<String>,
    /// `login=`, when named.
    pub login: Option<String>,
    /// `child=`, when named (contract 4 only).
    pub child: Option<String>,
}

impl ClaimWords {
    /// Read the text after `meta set supervisor `. A claim with no
    /// `contract=` word is an uncontracted (v2) claim and its other words are
    /// not read. A contracted claim takes `contract=`, `scope=`, `version=`,
    /// `vendor=`, `child=` and `login=`, each once, and nothing else; the arm
    /// of its contract judges which of them it may name.
    ///
    /// # Errors
    /// aterm's refusal line.
    pub fn read(rest: &str) -> Result<Self, String> {
        let mut words = rest.split_ascii_whitespace();
        let Some(holder) = words.next() else {
            return Err(
                "ERR usage: meta set supervisor <holder> [ttl=<ms>] | meta unset supervisor \
                 [holder=<holder>]"
                    .to_string(),
            );
        };
        let mut claim = Self {
            holder: holder.to_string(),
            ..Self::default()
        };
        let words: Vec<&str> = words.collect();
        if !words.iter().any(|word| word.starts_with("contract=")) {
            return Ok(claim);
        }
        for word in words {
            let Some((key, value)) = word.split_once('=') else {
                return Err(usage("a claim word is key=value"));
            };
            let slot = match key {
                "contract" => &mut claim.contract,
                "scope" => &mut claim.scope,
                "version" => &mut claim.version,
                "vendor" => &mut claim.vendor,
                "login" => &mut claim.login,
                "child" => &mut claim.child,
                _ => {
                    return Err(usage(
                        "a v3 claim takes contract= scope= version= vendor= login=",
                    ));
                }
            };
            if slot.replace(value.to_string()).is_some() {
                return Err(usage("a claim word is named twice"));
            }
        }
        Ok(claim)
    }
}

/// The refusal aterm gives a contracted claim from a rostered holder that
/// names a scope word outside its row (`contract.rs`, `read_in`).
const OUTSIDE_ROW: &str = "a scope word outside the harness's row";

/// The refusal aterm gives an uncontracted claim from a rostered holder.
const ROSTERED_V2: &str = "a rostered harness claims under contract=3 with its scope=";

/// A stored claim.
#[derive(Clone, Debug)]
struct Claim {
    words: ClaimWords,
    /// The connection it is bound to.
    connection: u64,
    /// Its granted duties; every duty for an uncontracted claim.
    scopes: Vec<Scope>,
}

impl Claim {
    /// `supervisor_scope=` as `status` spells it: `-` for none.
    fn scope_word(&self) -> String {
        if self.scopes.is_empty() {
            "-".to_string()
        } else {
            crate::claim::scope_words(&self.scopes)
        }
    }
}

/// What a [`FakeAterm`] answers, and what it was asked.
#[derive(Debug)]
struct State {
    generation: u64,
    policy: String,
    /// A whole reply sent in place of the policy line, when set.
    policy_reply: Option<String>,
    /// aterm's roster: each row's harness id and the scope words it allows.
    rows: Vec<(String, Vec<Scope>)>,
    /// A whole reply sent in place of the `native-act info` line built from
    /// `rows`, when set.
    info_reply: Option<String>,
    /// A whole reply sent in place of the subscription ACK, after which the
    /// connection is closed, when set.
    subscribe_reply: Option<String>,
    claim_reply: String,
    /// A whole reply sent in place of `meta unset supervisor`'s `OK`, when
    /// set; the claim then stands ([`FakeAterm::set_release_reply`]).
    release_reply: Option<String>,
    proven: bool,
    status: Vec<(String, String)>,
    claim: Option<Claim>,
    revision: u64,
    /// Every addressed request, without its `@<sid> `, in arrival order.
    verbs: Vec<String>,
    subscriptions: usize,
    /// Every event line pushed so far.
    events: Vec<String>,
    connections: u64,
    /// Every accepted connection, so [`FakeAterm::restart`] can drop them.
    accepted: Vec<UnixStream>,
    /// How many times [`FakeAterm::end_events`] ended every event stream: a
    /// stream that began at another count ends at its next look.
    events_ended: u64,
    /// How long each addressed request waits before it is answered.
    delay: Duration,
    /// A path whose existence is recorded as each addressed request arrives.
    witness: Option<PathBuf>,
    /// For each of `verbs`, whether the witness path existed when it arrived.
    witnessed: Vec<bool>,
    /// The scope words a contract-4 claim is granted when it names them; any
    /// other word is dropped.
    known: Vec<Scope>,
    /// A person's cap (`[harness.<word>] scopes`), when set: a contract-4
    /// claim is granted only words inside it.
    cap: Option<Vec<Scope>>,
    /// Whether a cap change also gives a live claim back the words it named
    /// that the new cap allows ([`FakeAterm::set_cap_restores`]).
    cap_restores: bool,
    /// A cap applied right after the next claim is stored, before any other
    /// request is answered ([`FakeAterm::set_cap_after_next_claim`]).
    cap_after_claim: Option<Option<Vec<Scope>>>,
    /// Words a contract-4 claim's reply grants that its stored claim does not
    /// take ([`FakeAterm::set_unstored`]).
    unstored: Vec<Scope>,
    /// A whole reply sent in place of the `status` line, when set.
    status_reply: Option<String>,
    /// Whether a contract-4 holder has a registry row that proves it
    /// ([`FakeAterm::set_registered`]); one without is answered
    /// [`UNPROVEN_4`] and nothing is stored.
    registered: bool,
    /// Every addressed request with the connection it came on, in arrival order.
    requests: Vec<(u64, String)>,
    /// The session stream.
    streams: Streams,
}

impl State {
    /// Drop the standing claim, and the stream it opened with it (the plan's D23: a release
    /// closes the stream).
    fn release(&mut self) {
        if self.claim.take().is_some() {
            self.revision += 1;
        }
        self.streams.close();
    }

    /// The policy body as aterm would answer it now: a halt on the session
    /// turns a policy in effect off, `why=hold`, from the same fact, and a cap
    /// is served as `scopes=` (the interface design, section 2.4).
    fn policy_body(&self) -> String {
        let held = self
            .status
            .iter()
            .any(|(key, value)| key == "hold" && value == "1");
        let mut body = if !held
            || !self
                .policy
                .split_ascii_whitespace()
                .any(|w| w == "effective=on")
        {
            self.policy.clone()
        } else {
            self.policy
                .split_ascii_whitespace()
                .map(|word| match word.split_once('=') {
                    Some(("effective", _)) => "effective=off",
                    Some(("why", _)) => "why=hold",
                    _ => word,
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        if let Some(cap) = &self.cap {
            body.push_str(" scopes=");
            body.push_str(&if cap.is_empty() {
                "-".to_string()
            } else {
                crate::claim::scope_words(cap)
            });
        }
        body
    }

    /// The contracts the policy body lists (`contract=`), none when it lists
    /// none or spells the set badly.
    fn served(&self) -> Vec<u32> {
        self.policy
            .split_ascii_whitespace()
            .find_map(|word| word.strip_prefix("contract="))
            .map(|set| set.split(',').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_default()
    }

    /// The policy cell moved: a new generation, said on every subscription and, as a `policy`
    /// record, on an open stream (HX-15c).
    fn policy_moved(&mut self) {
        self.generation += 1;
        let line = format!("EVENT 1 harness-policy gen={}", self.generation);
        self.events.push(line);
        self.streams.emit_own(Body::Policy);
    }

    /// `restart <words>`: said on every subscription and, as a `restart` record, on an open
    /// stream (HX-15c sends one for every restart row while the stream is open).
    fn restart_asked(&mut self, words: &str) {
        self.events.push(format!("EVENT 1 restart {words}"));
        if let Some(restart) = Restart::parse(words.split_ascii_whitespace()) {
            self.streams.emit_own(Body::Restart {
                cause: restart.cause,
                account: restart.account,
                model: restart.model,
            });
        }
    }

    /// `stream @<sid> v=1` on connection `id`, which answers on `out`: whether the stream
    /// opened. A refusal is written here and the connection stays in request mode.
    fn open_stream(&mut self, id: u64, request: &str, sid: &str, out: &mut UnixStream) -> bool {
        self.streams.asked += 1;
        let refusal = if let Some(reply) = &self.streams.reply {
            Some(reply.clone())
        } else if !self.streams.serve {
            Some(STREAM_UNKNOWN.to_string())
        } else if request != format!("stream @{sid} v={STREAM_VERSION}") {
            Some("ERR usage: stream @<sid> v=1".to_string())
        } else if !self.claim.as_ref().is_some_and(|claim| {
            claim.connection == id && claim.words.contract.as_deref() == Some("4")
        }) {
            Some("ERR usage: stream: no contract 4 claim on this connection".to_string())
        } else {
            None
        };
        let (shut, mut writer) = match refusal {
            Some(reply) => {
                let _ = out.write_all(format!("{reply}\n").as_bytes());
                return false;
            }
            None => match (out.try_clone(), out.try_clone()) {
                (Ok(shut), Ok(writer)) => (shut, writer),
                _ => {
                    let _ = out.write_all(b"ERR busy streams\n");
                    return false;
                }
            },
        };
        let holder = self.claim.as_ref().map(|claim| claim.words.holder.clone());
        if self.streams.holder != holder {
            self.streams.forget();
            self.streams.holder = holder;
        }
        self.streams.close();
        let streams = &mut self.streams;
        let reply = format!(
            "OK stream v={STREAM_VERSION} epoch=fake-{} acked={} in={}\n",
            streams.epoch, streams.acked, streams.sent
        );
        if out.write_all(reply.as_bytes()).is_err() {
            return false;
        }
        // Each stream writes from its own thread, outside the state's lock: a client that stops
        // reading holds up its own stream, never the fake. Shutting the socket down ends it.
        let (queue, lines) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for line in lines {
                if writer.write_all(line.as_bytes()).is_err() {
                    return;
                }
            }
        });
        streams.open = Some((id, shut, queue));
        streams.opened.push(id);
        streams.rate = Bucket::new(streams.rate_burst, streams.rate_per_s);
        let (human_seq, composer, hold) = streams.hand;
        streams.emit_own(Body::Hand {
            human_seq,
            composer,
            hold,
        });
        streams.emit_own(Body::Policy);
        true
    }

    /// One line read on the stream of connection `id`, without its newline.
    fn stream_line(&mut self, id: u64, line: &[u8]) {
        if self
            .streams
            .open
            .as_ref()
            .is_none_or(|(open, _, _)| *open != id)
            || self.streams.deaf
        {
            return;
        }
        let next = self.streams.acked + 1;
        if line.len() > LINE_MAX {
            self.streams.ack(next, Some(Dropped::Bound));
            return;
        }
        let Ok(decoded) = Record::read(line) else {
            self.streams
                .ack(next, Some(Dropped::Refused("usage".to_string())));
            return;
        };
        let record = decoded.record;
        if record.seq <= self.streams.acked {
            self.streams.acks.push((record.seq, Some(Dropped::Applied)));
            let _ = self.streams.emit_ack(record.seq, Some(Dropped::Applied));
            return;
        }
        if record.seq > next {
            // A gap: aterm closes the stream, and the harness sends again from `acked`.
            self.streams.close();
            return;
        }
        self.streams.records.push(record.clone());
        let granted = self
            .claim
            .as_ref()
            .map(|claim| claim.scopes.clone())
            .unwrap_or_default();
        let dropped = if let Some(dropped) = self.streams.drop_next.pop_front() {
            Some(dropped)
        } else if !self.streams.rate.take() {
            Some(Dropped::Rate)
        } else if !record.body.admitted(&granted, |_| None) {
            Some(Dropped::Scope)
        } else {
            None
        };
        if dropped.is_some() {
            self.streams.ack(record.seq, dropped);
            return;
        }
        match record.body {
            Body::Claim { .. } if !self.streams.applies_claims => {
                self.streams.unapplied += 1;
                self.streams.ack(record.seq, None);
            }
            Body::Claim { .. } => {
                let reply = self.claim_record(id, &record.body);
                if reply == "OK" || reply.starts_with("OK ") {
                    self.streams.ack(record.seq, None);
                    // The fake's revision moves with every claim: read the grant again.
                    self.streams.emit_own(Body::Policy);
                } else {
                    let word = refusal_word(&reply);
                    self.streams.ack(record.seq, Some(Dropped::Refused(word)));
                }
            }
            Body::Took { of } => {
                self.streams.took.push(of);
                self.streams.ack(record.seq, None);
            }
            Body::Refused { of, ref why } => {
                self.streams.refused.push((of, why.clone()));
                self.streams.ack(record.seq, None);
            }
            _ => self.streams.ack(record.seq, None),
        }
    }

    /// A `claim` record served as the claim placement it is (D7): the line aterm builds from its
    /// words, the standing claim's for any it leaves out, served on the stream's connection.
    fn claim_record(&mut self, id: u64, body: &Body) -> String {
        let Body::Claim {
            child,
            scope,
            version,
            vendor,
            login,
        } = body
        else {
            return "ERR usage".to_string();
        };
        let (Some(holder), Some(standing)) = (
            self.streams.holder.clone(),
            self.claim.as_ref().map(|claim| claim.words.clone()),
        ) else {
            return "ERR usage: no claim stands".to_string();
        };
        let scope = scope.as_ref().map_or_else(
            || standing.scope.clone().unwrap_or_default(),
            |scope| crate::claim::scope_words(scope),
        );
        let mut line = format!("{holder} contract=4 scope={scope}");
        for (key, value) in [
            ("version", version.clone().or(standing.version)),
            ("vendor", vendor.clone().or(standing.vendor)),
            (
                "child",
                child
                    .and_then(crate::claim::ChildProcess::word)
                    .or(standing.child),
            ),
            ("login", login.clone().or(standing.login)),
        ] {
            if let Some(value) = value {
                line.push_str(&format!(" {key}={value}"));
            }
        }
        self.claim(id, &line)
    }

    fn status_line(&self) -> String {
        let mut line = "OK schema=1 sid=1".to_string();
        for (key, value) in &self.status {
            line.push_str(&format!(" {key}={value}"));
        }
        let word = |value: Option<&String>| value.map_or("-", String::as_str).to_string();
        let claim = self.claim.as_ref();
        line.push_str(&format!(
            " supervisor_contract={} supervisor_scope={} supervisor_revision={} supervisor={} \
             supervisor_proven={} supervisor_version={} supervisor_vendor={}",
            word(claim.and_then(|claim| claim.words.contract.as_ref())),
            word(claim.map(Claim::scope_word).as_ref()),
            self.revision,
            word(claim.map(|claim| &claim.words.holder)),
            u8::from(claim.is_some() && self.proven),
            word(claim.and_then(|claim| claim.words.version.as_ref())),
            word(claim.and_then(|claim| claim.words.vendor.as_ref())),
        ));
        line
    }

    /// The `native-act info` reply: the rows that allow `upgrade`, as aterm's
    /// `own_image_rows` lists them, unless a whole reply was set.
    fn info(&self) -> String {
        self.info_reply.clone().unwrap_or_else(|| {
            let ids: Vec<&str> = self
                .rows
                .iter()
                .filter(|(_, allows)| allows.contains(&Scope::Upgrade))
                .map(|(id, _)| id.as_str())
                .collect();
            info_with_rows(&ids)
        })
    }

    /// The scope words the roster row of `holder` (`<id>@<pid>`) allows, when
    /// its id is rostered.
    fn row_of(&self, holder: &str) -> Option<&[Scope]> {
        let id = holder.split_once('@').map_or(holder, |(id, _)| id);
        self.rows
            .iter()
            .find(|(row, _)| row == id)
            .map(|(_, allows)| allows.as_slice())
    }

    /// `meta set supervisor <holder> <words…>` from connection `id`.
    fn claim(&mut self, id: u64, rest: &str) -> String {
        if self.claim_reply != "OK" {
            return self.claim_reply.clone();
        }
        let words = match ClaimWords::read(rest) {
            Ok(words) => words,
            Err(refusal) => return refusal,
        };
        let row = self.row_of(&words.holder);
        let served = self.served();
        let contract = words.contract.as_deref();
        if contract.is_some_and(|named| !served.iter().any(|n| n.to_string() == named)) {
            return usage("a scoped claim names a contract aterm serves");
        }
        let scopes = match contract {
            // A rostered holder never claims under the v2 grammar.
            None if row.is_some() => return usage(ROSTERED_V2),
            None => Scope::ALL.to_vec(),
            Some("3") => match contract_3_scope(&words, row) {
                Ok(scopes) => scopes,
                Err(why) => return usage(why),
            },
            Some("4") => match contract_4_scope(&words, &self.known, self.cap.as_deref()) {
                Ok(scopes) => scopes,
                Err(why) => return usage_4(why),
            },
            Some(_) => return usage("a scoped claim names a contract aterm serves"),
        };
        if let Some(other) = self
            .claim
            .as_ref()
            .filter(|claim| claim.words.holder != words.holder)
        {
            return format!("ERR busy supervisor={}", other.words.holder);
        }
        let four = contract == Some("4");
        if four && !self.registered {
            return UNPROVEN_4.to_string();
        }
        let mut claim = Claim {
            words,
            connection: id,
            scopes,
        };
        let reply = if four {
            let reply = format!(
                "OK scope={} revision={}",
                claim.scope_word(),
                self.revision + 1
            );
            claim.scopes.retain(|scope| !self.unstored.contains(scope));
            reply
        } else {
            "OK".to_string()
        };
        self.claim = Some(claim);
        self.revision += 1;
        // A claim placed on another connection closes the stream (HX-15b1).
        if self
            .streams
            .open
            .as_ref()
            .is_some_and(|(open, _, _)| *open != id)
        {
            self.streams.close();
        }
        if let Some(cap) = self.cap_after_claim.take() {
            self.cap(cap);
        }
        reply
    }

    /// The words the live claim would take under `cap` were it made now: its
    /// named words its contract's arm grants, inside the cap. `None` without
    /// a contracted claim.
    fn regranted(&self, cap: Option<&[Scope]>) -> Option<Vec<Scope>> {
        let claim = self.claim.as_ref()?;
        let named = match claim.words.contract.as_deref()? {
            "3" => contract_3_scope(&claim.words, self.row_of(&claim.words.holder)).ok()?,
            "4" => contract_4_scope(&claim.words, &self.known, None).ok()?,
            _ => return None,
        };
        Some(
            named
                .into_iter()
                .filter(|scope| cap.is_none_or(|cap| cap.contains(scope)))
                .collect(),
        )
    }

    /// A person's cap changed: every live contracted claim (contract 3 or 4)
    /// is narrowed to it, and given back what it named that the cap allows
    /// when [`State::cap_restores`] is set; a claim whose words changed has its
    /// revision raised. The policy moves either way, which aterm says with
    /// `harness-policy`.
    fn cap(&mut self, cap: Option<Vec<Scope>>) {
        if let Some(regranted) = self.regranted(cap.as_deref())
            && let Some(claim) = self.claim.as_mut()
        {
            let scopes = if self.cap_restores {
                regranted
            } else {
                let mut kept = claim.scopes.clone();
                kept.retain(|scope| regranted.contains(scope));
                kept
            };
            if scopes != claim.scopes {
                claim.scopes = scopes;
                self.revision += 1;
            }
        }
        self.cap = cap;
        self.policy_moved();
    }

    /// One addressed request from connection `id`.
    fn answer(&mut self, id: u64, verb: &str) -> String {
        self.verbs.push(verb.to_string());
        self.requests.push((id, verb.to_string()));
        self.witnessed
            .push(self.witness.as_ref().is_some_and(|path| path.exists()));
        if verb == "harness policy" {
            self.policy_reply
                .clone()
                .unwrap_or_else(|| format!("OK gen={} {}", self.generation, self.policy_body()))
        } else if verb == "native-act info" {
            self.info()
        } else if verb == "status" {
            self.status_reply
                .clone()
                .unwrap_or_else(|| self.status_line())
        } else if let Some(rest) = verb.strip_prefix("meta set supervisor ") {
            self.claim(id, rest)
        } else if let Some(rest) = verb.strip_prefix("meta unset supervisor") {
            if let Some(reply) = &self.release_reply {
                return reply.clone();
            }
            let named = rest.trim().strip_prefix("holder=");
            if self
                .claim
                .as_ref()
                .is_some_and(|claim| named.is_none_or(|holder| holder == claim.words.holder))
            {
                self.release();
            }
            "OK".to_string()
        } else {
            "ERR unknown verb".to_string()
        }
    }
}

/// A contract-3 claim's scope words as aterm stores them: named, each known
/// to contract 3 and once, each allowed by the holder's roster `row` when it
/// has one, in aterm's order. Contract 3 has no `child=`.
fn contract_3_scope(words: &ClaimWords, row: Option<&[Scope]>) -> Result<Vec<Scope>, &'static str> {
    if words.child.is_some() {
        return Err("a v3 claim takes contract= scope= version= vendor= login=");
    }
    let Some(scope) = words.scope.as_deref() else {
        return Err("a v3 claim names its scope=");
    };
    let mut scopes = Vec::new();
    for word in scope.split(',') {
        let Some(scope) = Scope::from_word(word).filter(|scope| scope.since() == Contract::Three)
        else {
            return Err("an unknown scope word");
        };
        if scopes.contains(&scope) {
            return Err("a scope word is named twice");
        }
        scopes.push(scope);
    }
    if row.is_some_and(|allows| scopes.iter().any(|scope| !allows.contains(scope))) {
        return Err(OUTSIDE_ROW);
    }
    scopes.sort_unstable();
    Ok(scopes)
}

/// Whether `value` is a `child=` value: `<pid>:<start>`, each a canonical
/// decimal, the pid above 1.
fn child_word(value: &str) -> bool {
    let canonical = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    value.split_once(':').is_some_and(|(pid, start)| {
        canonical(pid)
            && canonical(start)
            && pid.parse::<u32>().is_ok_and(|pid| pid > 1)
            && start.parse::<u64>().is_ok()
    })
}

/// A contract-4 claim's grant: of its named scope words (each once), the ones
/// in `known`, less any outside a person's `cap`, in aterm's order. A word not
/// known is dropped, not refused, and no roster row is consulted: a verified
/// record may claim any word (the interface design, section 2.2).
fn contract_4_scope(
    words: &ClaimWords,
    known: &[Scope],
    cap: Option<&[Scope]>,
) -> Result<Vec<Scope>, &'static str> {
    let Some(scope) = words.scope.as_deref() else {
        return Err("a v4 claim names its scope=");
    };
    if words.version.is_none() {
        return Err("a v4 claim names its version=");
    }
    if words
        .child
        .as_deref()
        .is_some_and(|child| !child_word(child))
    {
        return Err("child= is <pid>:<start>");
    }
    let mut named: Vec<&str> = Vec::new();
    for word in scope.split(',') {
        if word.is_empty() {
            return Err("an empty scope word");
        }
        if named.contains(&word) {
            return Err("a scope word is named twice");
        }
        named.push(word);
    }
    let mut scopes: Vec<Scope> = named
        .into_iter()
        .filter_map(Scope::from_word)
        .filter(|scope| known.contains(scope))
        .filter(|scope| cap.is_none_or(|cap| cap.contains(scope)))
        .collect();
    scopes.sort_unstable();
    Ok(scopes)
}

/// A fake aterm serving one session. Dropping it closes every connection and
/// removes its socket directory (and its scratch root, when it made one).
pub struct FakeAterm {
    /// `XDG_RUNTIME_DIR` for the client: it holds `aterm/graph/<sid>`.
    pub xdg: PathBuf,
    /// The short directory holding the socket and its token.
    pub sock_dir: PathBuf,
    /// The session ID.
    pub sid: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    /// A root this fake made and removes when dropped.
    scratch: Option<PathBuf>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // A scenario that panicked while holding the state has already failed; the
    // fake keeps answering so its cleanup can run.
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The short parent of every fake's socket directory: a Unix socket path holds
/// about a hundred bytes, and a test's own directory is often longer.
const SOCKET_PARENT: &str = "/tmp/.ubd";

impl FakeAterm {
    /// A fake aterm whose rendezvous directory is under `root` (a scenario's
    /// private directory). It starts with [`POLICY_ON`] at generation 1, an
    /// empty roster (`own-image=-`, so no harness id is rostered until
    /// [`FakeAterm::set_row`] adds it), [`QUIET_STATUS`], a claim reply of
    /// `OK`, the proven bit set and no claim.
    ///
    /// # Panics
    /// The directories, the token, the graph entry or the socket cannot be
    /// made: the test cannot run.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let sid = format!("s-{:x}a{n:x}", std::process::id());
        let xdg = root.join("xdg");
        let graph = xdg.join("aterm").join("graph");
        std::fs::create_dir_all(&graph).expect("graph directory");
        let parent = Path::new(SOCKET_PARENT);
        std::fs::create_dir_all(parent).expect("socket parent");
        let sock_dir = parent.join(format!("fa-{}-{n}.noindex", std::process::id()));
        let _ = std::fs::remove_dir_all(&sock_dir);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&sock_dir)
            .expect("socket directory");
        let sock = sock_dir.join("c.sock");
        std::fs::write(sock_dir.join("c.sock.token"), format!("{TOKEN}\n")).expect("token");
        std::fs::write(
            graph.join(&sid),
            format!("pid 1\nsock {}\n", sock.display()),
        )
        .expect("graph entry");
        let listener = UnixListener::bind(&sock).expect("bind the fake socket");
        listener.set_nonblocking(true).expect("nonblocking accept");
        let state = Arc::new(Mutex::new(State {
            generation: 1,
            policy: POLICY_ON.to_string(),
            policy_reply: None,
            rows: Vec::new(),
            info_reply: None,
            subscribe_reply: None,
            claim_reply: "OK".to_string(),
            release_reply: None,
            proven: true,
            registered: true,
            status: QUIET_STATUS
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
            claim: None,
            revision: 0,
            verbs: Vec::new(),
            subscriptions: 0,
            events: Vec::new(),
            connections: 0,
            accepted: Vec::new(),
            events_ended: 0,
            delay: Duration::ZERO,
            witness: None,
            witnessed: Vec::new(),
            known: Scope::ALL.to_vec(),
            cap: None,
            cap_restores: false,
            cap_after_claim: None,
            unstored: Vec::new(),
            status_reply: None,
            requests: Vec::new(),
            streams: Streams::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (accept_state, accept_stop, accept_sid) =
            (Arc::clone(&state), Arc::clone(&stop), sid.clone());
        std::thread::spawn(move || {
            let mut id = 0_u64;
            while !accept_stop.load(Ordering::Relaxed) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                id += 1;
                // Darwin hands the listener's nonblocking flag to accepted sockets.
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                if let Ok(clone) = stream.try_clone() {
                    lock(&accept_state).accepted.push(clone);
                }
                let (state, stop, sid) = (
                    Arc::clone(&accept_state),
                    Arc::clone(&accept_stop),
                    accept_sid.clone(),
                );
                std::thread::spawn(move || serve(stream, id, &state, &stop, &sid));
            }
        });
        Self {
            xdg,
            sock_dir,
            sid,
            state,
            stop,
            scratch: None,
        }
    }

    /// A fake aterm under a fresh private root in the system's temporary
    /// directory, removed when the fake is dropped.
    ///
    /// # Panics
    /// As [`FakeAterm::new`].
    #[must_use]
    pub fn scratch(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "alab-wire-fake-{name}-{}-{}.noindex",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch root");
        let mut fake = Self::new(&root);
        fake.scratch = Some(root);
        fake
    }

    /// A [`FakeAterm::scratch`] fake laid for `config`'s harness: its roster
    /// carries the harness's row allowing [`Scope::CONTRACT_3`], as every row
    /// of aterm's does, and `status` reads `program=<id>`.
    ///
    /// aterm's `program=` is the file name of the image of the terminal's
    /// foreground group leader, and while a claim stands proven that leader is
    /// the harness, never the vendor CLI it runs. A harness whose executable is
    /// not named after its id (claude-code-alab's is `claude-alab`) sets it
    /// with [`FakeAterm::set_status`].
    ///
    /// # Panics
    /// As [`FakeAterm::new`].
    #[must_use]
    pub fn for_harness(name: &str, config: &Config) -> Self {
        let fake = Self::scratch(name);
        fake.set_row(config.id(), &Scope::CONTRACT_3);
        fake.set_status("program", Some(config.id()));
        fake
    }

    /// The environment that puts a harness in this fake's tab, by aterm's
    /// default [`EnvNames`].
    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        self.env_for(&EnvNames::default())
    }

    /// The environment that puts a harness reading `names` in this fake's tab:
    /// its first hosted mark, its session variable and its runtime directory.
    #[must_use]
    pub fn env_for(&self, names: &EnvNames) -> Vec<(String, String)> {
        let mut env = Vec::new();
        if let Some(mark) = names.hosted.first() {
            env.push((
                mark.name.clone(),
                mark.value.clone().unwrap_or_else(|| "1".to_string()),
            ));
        }
        env.push((names.session.clone(), self.sid.clone()));
        env.push((names.runtime_dir.clone(), self.xdg.display().to_string()));
        env
    }

    /// A lookup over [`FakeAterm::env_for`] `names`, for
    /// [`crate::Link::launch_with`] and [`Locator::from_lookup`]: only this
    /// fake's variables are set.
    pub fn lookup(&self, names: &EnvNames) -> impl Fn(&str) -> Option<OsString> + use<> {
        let env = self.env_for(names);
        move |name| {
            env.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    /// The locator a harness started with [`FakeAterm::env`] resolves.
    ///
    /// # Panics
    /// The fake's own session ID is refused, which is a defect of the fake.
    #[must_use]
    pub fn locator(&self) -> Locator {
        Locator::from_values(&self.sid, Some(self.xdg.as_os_str()), None).expect("locator")
    }

    /// Answer `harness policy` with `OK gen=<n> <body>` at the next generation,
    /// and say so on every subscription (`EVENT 1 harness-policy gen=<n>`), as
    /// aterm does when its policy cell moves.
    pub fn set_policy(&self, body: &str) {
        let mut state = lock(&self.state);
        state.policy = body.to_string();
        state.policy_reply = None;
        state.policy_moved();
    }

    /// Serve exactly the contracts `contracts` from now on: the policy's
    /// `contract=` word is replaced (added when it had none) at the next
    /// generation, with a `harness-policy` event. A claim naming a contract
    /// outside the set is refused. `&[2, 3]` is an aterm from before contract
    /// 4; `&[3]` one that serves contract 3 alone.
    pub fn serve_contracts(&self, contracts: &[u32]) {
        let set = contracts
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut state = lock(&self.state);
        let mut found = false;
        let mut body: Vec<String> = state
            .policy
            .split_ascii_whitespace()
            .map(|word| {
                if word.starts_with("contract=") {
                    found = true;
                    format!("contract={set}")
                } else {
                    word.to_string()
                }
            })
            .collect();
        if !found {
            body.push(format!("contract={set}"));
        }
        state.policy = body.join(" ");
        state.policy_reply = None;
        state.policy_moved();
    }

    /// The scope words a contract-4 claim is granted when it names them: an
    /// aterm that knows fewer words than this library drops the rest. Starts
    /// as [`Scope::ALL`].
    pub fn set_known(&self, known: &[Scope]) {
        lock(&self.state).known = known.to_vec();
    }

    /// A person's cap, `[harness.<word>] scopes = "<words>"` (`None`: no cap),
    /// as aterm applies it to live claims: a standing contracted claim
    /// (contract 3 or 4; the cap narrows every live claim it covers) is
    /// narrowed to the cap and, when that withheld a word, its revision is
    /// raised; later contract-4 claims are granted only words inside it; the
    /// policy reads `scopes=<words>` and moves to the next generation with a
    /// `harness-policy` event. A cap that is lifted or widened gives a live
    /// claim nothing back, so its holder claims again for that, unless
    /// [`FakeAterm::set_cap_restores`] says otherwise.
    pub fn set_cap(&self, cap: Option<&[Scope]>) {
        lock(&self.state).cap(cap.map(<[Scope]>::to_vec));
    }

    /// Whether a cap change also gives a live claim back the words it named
    /// that the new cap allows, raising its revision: an aterm that widens
    /// live claims when a cap is lifted, which the interface design does not
    /// rule out. Off at first.
    pub fn set_cap_restores(&self, restores: bool) {
        lock(&self.state).cap_restores = restores;
    }

    /// Apply `cap` as [`FakeAterm::set_cap`] does right after the next claim
    /// is stored and answered, before any other request: a person's cap that
    /// lands between a claim's reply and its holder's `status` read.
    pub fn set_cap_after_next_claim(&self, cap: Option<&[Scope]>) {
        lock(&self.state).cap_after_claim = Some(cap.map(<[Scope]>::to_vec));
    }

    /// Words a contract-4 claim's reply grants that the stored claim does not
    /// take, so `status` shows fewer than the reply named at the same
    /// revision: an aterm whose claim cell disagrees with its reply.
    pub fn set_unstored(&self, unstored: &[Scope]) {
        lock(&self.state).unstored = unstored.to_vec();
    }

    /// Answer `status` with exactly `reply` (a refusal, or a line an older
    /// aterm would send) in place of the session's own line.
    pub fn set_status_reply(&self, reply: &str) {
        lock(&self.state).status_reply = Some(reply.to_string());
    }

    /// Answer `status` from the session again, after
    /// [`FakeAterm::set_status_reply`].
    pub fn clear_status_reply(&self) {
        lock(&self.state).status_reply = None;
    }

    /// Answer `harness policy` with `OK gen=<n> <body>` at the same generation,
    /// and say nothing: what aterm does when a session's own Autopilot word,
    /// rather than its policy cell, turns the policy on or off for it.
    pub fn set_policy_quietly(&self, body: &str) {
        let mut state = lock(&self.state);
        state.policy = body.to_string();
        state.policy_reply = None;
    }

    /// Answer `harness policy` with exactly `reply`: a refusal, or a line an
    /// older aterm would send. No event is sent.
    pub fn set_policy_reply(&self, reply: &str) {
        lock(&self.state).policy_reply = Some(reply.to_string());
    }

    /// Answer `harness policy` from the policy body again, after
    /// [`FakeAterm::set_policy_reply`]. No event is sent.
    pub fn clear_policy_reply(&self) {
        lock(&self.state).policy_reply = None;
    }

    /// Put harness `id`'s row on the roster, allowing the scope words
    /// `allows`, in place of any row it had; an empty `allows` takes the row
    /// off. A rostered holder's claim is read as aterm reads it: never
    /// uncontracted, and refused whole when it names a word its row does not
    /// allow. `native-act info` lists the rows that allow `upgrade`.
    pub fn set_row(&self, id: &str, allows: &[Scope]) {
        let mut state = lock(&self.state);
        state.rows.retain(|(row, _)| row != id);
        if !allows.is_empty() {
            state.rows.push((id.to_string(), allows.to_vec()));
        }
    }

    /// Answer `native-act info` with exactly `reply` (a refusal, or a line an
    /// older aterm would send) in place of the roster's own line. The roster
    /// a claim is read against is still [`FakeAterm::set_row`]'s.
    pub fn set_info(&self, reply: &str) {
        lock(&self.state).info_reply = Some(reply.to_string());
    }

    /// Answer `native-act info` from the roster again, after
    /// [`FakeAterm::set_info`].
    pub fn clear_info(&self) {
        lock(&self.state).info_reply = None;
    }

    /// Answer `subscribe @<sid> events` with exactly `reply` and close the
    /// connection (`None`: aterm's ACK and the stream again). An `ERR`, or an
    /// ACK an incompatible aterm would send, is refused by the client's
    /// strict reader.
    pub fn set_subscribe_reply(&self, reply: Option<&str>) {
        lock(&self.state).subscribe_reply = reply.map(str::to_string);
    }

    /// Answer `meta set supervisor …` with exactly `reply`. Only `OK` stores a
    /// claim; `ERR unproven: …` is what aterm answers a rostered holder it could
    /// not prove.
    pub fn set_claim_reply(&self, reply: &str) {
        lock(&self.state).claim_reply = reply.to_string();
    }

    /// Answer `meta unset supervisor` with exactly `reply`, the claim left
    /// standing (`None`: `OK`, and the claim named is released). A claim bound
    /// to a connection still goes when that connection closes.
    pub fn set_release_reply(&self, reply: Option<&str>) {
        lock(&self.state).release_reply = reply.map(str::to_string);
    }

    /// Whether a contract-4 holder has a registry row that proves it. Without
    /// one its claim is answered [`UNPROVEN_4`] and nothing is stored, as
    /// aterm answers an unregistered contract-4 holder from HX-06 on. Starts
    /// `true`.
    pub fn set_registered(&self, registered: bool) {
        lock(&self.state).registered = registered;
    }

    /// Whether a stored claim reads `supervisor_proven=1`.
    pub fn set_proven(&self, proven: bool) {
        lock(&self.state).proven = proven;
    }

    /// Set one `status` field, or drop it (`None`: an aterm that does not report
    /// it). The claim's own fields come from the claim state and cannot be set
    /// here.
    pub fn set_status(&self, key: &str, value: Option<&str>) {
        let mut state = lock(&self.state);
        state.status.retain(|(known, _)| known != key);
        if let Some(value) = value {
            state.status.push((key.to_string(), value.to_string()));
        }
    }

    /// Push `EVENT 1 <payload>` to every subscription.
    pub fn event(&self, payload: &str) {
        lock(&self.state).events.push(format!("EVENT 1 {payload}"));
    }

    /// Ask the claim holder to restart its child: `EVENT 1 restart cause=<cause>`,
    /// which aterm sends instead of signalling a harness whose proven claim
    /// takes `relaunch`.
    pub fn send_restart(&self, cause: &str) {
        lock(&self.state).restart_asked(&format!("cause={cause}"));
    }

    /// Move the claim holder to another account, as aterm's account pool does
    /// for a proven `relaunch` holder: `EVENT 1 restart cause=<cause>
    /// to=<label> home=<home, percent-encoded, or ->`
    /// (`restart_act.rs`, `record_scoped_account`).
    pub fn send_account_restart(&self, cause: &str, label: &str, home: Option<&Path>) {
        let home = home.map_or_else(
            || "-".to_string(),
            |home| percent_encode(home.as_os_str().as_encoded_bytes()),
        );
        lock(&self.state).restart_asked(&format!("cause={cause} to={label} home={home}"));
    }

    /// Move the claim holder to another model, as aterm does from HX-06:
    /// `EVENT 1 restart cause=<cause> model=<model>`. aterm names the model in
    /// `model=`, never in `to=`.
    pub fn send_model_restart(&self, cause: &str, model: &str) {
        lock(&self.state).restart_asked(&format!("cause={cause} model={model}"));
    }

    /// An aterm restart: every connection is dropped, and the claim with them.
    /// The socket keeps listening, as a restarted aterm's does. The new
    /// instance is a new stream epoch that knows no claimant's `acked` or `in`.
    pub fn restart(&self) {
        let mut state = lock(&self.state);
        state.release();
        for stream in state.accepted.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        state.streams.epoch += 1;
        state.streams.forget();
    }

    /// End every event stream now open, as aterm's event fan-out can drop a
    /// subscriber while the session and its request connections stay: each
    /// stream's connection is shut down, the claim and every request
    /// connection are kept, and a new subscription is served as before (or
    /// refused, after [`FakeAterm::set_subscribe_reply`]).
    pub fn end_events(&self) {
        lock(&self.state).events_ended += 1;
    }

    /// Every addressed request so far, without its `@<sid> `, in arrival order.
    #[must_use]
    pub fn verbs(&self) -> Vec<String> {
        lock(&self.state).verbs.clone()
    }

    /// Wait `delay` before answering each addressed request from now on: an
    /// aterm that is slow, so whatever a client does while it waits has time to
    /// show.
    pub fn set_reply_delay(&self, delay: Duration) {
        lock(&self.state).delay = delay;
    }

    /// Record, as each addressed request arrives from now on, whether `path`
    /// exists ([`FakeAterm::verbs_with_witness`]): the order of a request and
    /// something a client does outside the socket, such as starting a program
    /// that writes `path`.
    pub fn set_witness(&self, path: &Path) {
        lock(&self.state).witness = Some(path.to_path_buf());
    }

    /// [`FakeAterm::verbs`], each with whether the witness path existed when
    /// the request arrived (`false` while no witness was set).
    #[must_use]
    pub fn verbs_with_witness(&self) -> Vec<(String, bool)> {
        let state = lock(&self.state);
        state
            .verbs
            .iter()
            .cloned()
            .zip(state.witnessed.iter().copied())
            .collect()
    }

    /// The `status` line as it would be answered now, without recording a
    /// request.
    #[must_use]
    pub fn status_line(&self) -> String {
        lock(&self.state).status_line()
    }

    /// The standing claim's holder.
    #[must_use]
    pub fn holder(&self) -> Option<String> {
        lock(&self.state)
            .claim
            .as_ref()
            .map(|claim| claim.words.holder.clone())
    }

    /// The standing claim's words, as the fake read them.
    #[must_use]
    pub fn claim_words(&self) -> Option<ClaimWords> {
        lock(&self.state)
            .claim
            .as_ref()
            .map(|claim| claim.words.clone())
    }

    /// The `child=` the standing claim named last (`<pid>:<start>`), when it
    /// named one.
    #[must_use]
    pub fn child(&self) -> Option<String> {
        self.claim_words().and_then(|words| words.child)
    }

    /// The claim cell's revision now: what `status` reads as
    /// `supervisor_revision=` and a contract-4 reply as `revision=`.
    #[must_use]
    pub fn revision(&self) -> u64 {
        lock(&self.state).revision
    }

    /// How many connections authenticated.
    #[must_use]
    pub fn connections(&self) -> u64 {
        lock(&self.state).connections
    }

    /// How many subscriptions were accepted.
    #[must_use]
    pub fn subscriptions(&self) -> usize {
        lock(&self.state).subscriptions
    }

    /// Every addressed request so far with the connection it came on (connections are numbered
    /// from 1 as they are accepted), without its `@<sid> `, in arrival order.
    #[must_use]
    pub fn requests(&self) -> Vec<(u64, String)> {
        lock(&self.state).requests.clone()
    }

    /// The connection the standing claim is bound to.
    #[must_use]
    pub fn claim_connection(&self) -> Option<u64> {
        lock(&self.state)
            .claim
            .as_ref()
            .map(|claim| claim.connection)
    }

    // ───────────────────────────── the session stream ─────────────────────────────

    /// Whether `stream @<sid> v=1` is served on the connection holding a contract-4 claim (as by
    /// aterm from HX-15b1), or answered [`STREAM_UNKNOWN`], as by every aterm before it (at
    /// first).
    pub fn serve_stream(&self, serve: bool) {
        lock(&self.state).streams.serve = serve;
    }

    /// Answer `stream` with exactly `reply`, the connection kept in request mode with its claim
    /// (`None`: as [`FakeAterm::serve_stream`] says). `ERR busy streams` is what aterm answers
    /// when its stream pool is full.
    pub fn set_stream_reply(&self, reply: Option<&str>) {
        lock(&self.state).streams.reply = reply.map(str::to_string);
    }

    /// The stream's rate budget: `burst` records at once and `per_s` more each second (aterm's
    /// is [`RATE_BURST`] and [`RATE_PER_S`]). A record over it is acknowledged `dropped=rate`.
    pub fn set_stream_rate(&self, burst: u32, per_s: u32) {
        let streams = &mut lock(&self.state).streams;
        (streams.rate_burst, streams.rate_per_s) = (burst, per_s);
        streams.rate = Bucket::new(burst, per_s);
    }

    /// Acknowledge the next harness record `dropped=<dropped>`, applying nothing, whatever it
    /// is. Each call stages one more.
    pub fn drop_next_record(&self, dropped: Dropped) {
        lock(&self.state).streams.drop_next.push_back(dropped);
    }

    /// Whether the open stream's lines are read and dropped unacknowledged, as by an aterm whose
    /// stream reader stopped: `acked` stays where it was.
    pub fn set_stream_deaf(&self, deaf: bool) {
        lock(&self.state).streams.deaf = deaf;
    }

    /// Whether the fake sends records of its own, as aterm does from HX-15c (at first): the
    /// current `hand` and a `policy` at every open, `policy` on every policy move and placement,
    /// `restart` beside every restart event. With `false` it sends none, as an aterm with HX-15b1
    /// alone, whose policy moves and restarts come only as events; [`FakeAterm::send_record`]
    /// still sends what a test names.
    pub fn set_stream_own_records(&self, send: bool) {
        lock(&self.state).streams.own_records = send;
    }

    /// Whether a `claim` record is placed as a claim, as aterm does from HX-15b2 (at first), or
    /// acknowledged with no `dropped=` and not applied, as an aterm with HX-15b1 alone does with
    /// a kind it does not apply yet (D9).
    pub fn set_stream_applies_claims(&self, applies: bool) {
        lock(&self.state).streams.applies_claims = applies;
    }

    /// How many `claim` records were acknowledged and not applied.
    #[must_use]
    pub fn unapplied_claims(&self) -> usize {
        lock(&self.state).streams.unapplied
    }

    /// Whether a contract-4 claim whose connection closes is kept, unbound, through aterm's
    /// re-exec grace (D28) until [`FakeAterm::end_grace`] (at first, no: the claim goes with its
    /// connection). A claim by the same holder renews it and keeps the stream's numbers; `meta
    /// unset supervisor` from any connection releases it at once.
    pub fn set_reexec_grace(&self, grace: bool) {
        lock(&self.state).streams.grace = grace;
    }

    /// The re-exec grace ends: a claim kept unbound is released, and its claimant's stream
    /// numbers are forgotten, so the same holder's next stream starts at `acked=0 in=0`.
    pub fn end_grace(&self) {
        let mut state = lock(&self.state);
        if state
            .claim
            .as_ref()
            .is_some_and(|claim| claim.connection == 0)
        {
            state.release();
            state.streams.forget();
        }
    }

    /// Whether the standing claim is kept unbound through the re-exec grace.
    #[must_use]
    pub fn in_grace(&self) -> bool {
        lock(&self.state)
            .claim
            .as_ref()
            .is_some_and(|claim| claim.connection == 0)
    }

    /// Write `line`, as it is, on the open stream, counted in nothing: a line a test makes,
    /// such as one aterm would never send. Whether a stream was open.
    pub fn send_line(&self, line: &str) -> bool {
        lock(&self.state).streams.write(line)
    }

    /// Close the open stream, as aterm does on a `seq` gap or a claim placed on another
    /// connection: its connection is shut down, and the claim bound to it goes.
    pub fn close_stream(&self) {
        lock(&self.state).streams.close();
    }

    /// A person's hand moved: send `hand` on the open stream with these values, which the next
    /// stream to open is sent too. Whether a stream was open.
    pub fn send_hand(&self, human_seq: u64, composer: Composer, hold: bool) -> bool {
        let mut state = lock(&self.state);
        state.streams.hand = (human_seq, composer, hold);
        state.streams.emit_own(Body::Hand {
            human_seq,
            composer,
            hold,
        })
    }

    /// Send one of aterm's records on the open stream, numbered in its `in` (any body: `want`,
    /// `prompt`, a kind the harness does not know). Whether a stream was open.
    pub fn send_record(&self, body: Body) -> bool {
        lock(&self.state).streams.emit(body)
    }

    /// Whether a stream is open now.
    #[must_use]
    pub fn streaming(&self) -> bool {
        lock(&self.state).streams.open.is_some()
    }

    /// The connection the open stream is on.
    #[must_use]
    pub fn stream_connection(&self) -> Option<u64> {
        lock(&self.state)
            .streams
            .open
            .as_ref()
            .map(|(id, _, _)| *id)
    }

    /// How many `stream` requests were answered, whatever the answer.
    #[must_use]
    pub fn stream_requests(&self) -> usize {
        lock(&self.state).streams.asked
    }

    /// The connection each stream was opened on, in order.
    #[must_use]
    pub fn streams_opened(&self) -> Vec<u64> {
        lock(&self.state).streams.opened.clone()
    }

    /// The stream's epoch word now, `fake-<n>`.
    #[must_use]
    pub fn epoch(&self) -> String {
        format!("fake-{}", lock(&self.state).streams.epoch)
    }

    /// The last harness record acknowledged, and the last record the fake sent (`in`).
    #[must_use]
    pub fn stream_numbers(&self) -> (u64, u64) {
        let state = lock(&self.state);
        (state.streams.acked, state.streams.sent)
    }

    /// Every harness record read on a stream, applied or dropped (not those acknowledged
    /// `dropped=applied`), in arrival order.
    #[must_use]
    pub fn stream_records(&self) -> Vec<Record> {
        lock(&self.state).streams.records.clone()
    }

    /// Every acknowledgement sent: the harness record's `seq` and its `dropped=`.
    #[must_use]
    pub fn acks(&self) -> Vec<(u64, Option<Dropped>)> {
        lock(&self.state).streams.acks.clone()
    }

    /// The `in` of every `took` the harness sent.
    #[must_use]
    pub fn took(&self) -> Vec<u64> {
        lock(&self.state).streams.took.clone()
    }

    /// The `in` and `why` of every `refused` the harness sent.
    #[must_use]
    pub fn refused(&self) -> Vec<(u64, String)> {
        lock(&self.state).streams.refused.clone()
    }

    /// How many `beat` records arrived.
    #[must_use]
    pub fn beats(&self) -> usize {
        lock(&self.state)
            .streams
            .records
            .iter()
            .filter(|record| record.body == Body::Beat)
            .count()
    }
}

impl Drop for FakeAterm {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for stream in lock(&self.state).accepted.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = std::fs::remove_dir_all(&self.sock_dir);
        if let Some(root) = &self.scratch {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

/// Stream the events pushed after this subscription began (`start`: the events pushed and the
/// ends counted before its ACK), until the client goes, the fake stops, a restart drops the
/// connection, or [`FakeAterm::end_events`] ends it.
fn stream_events(
    mut out: UnixStream,
    state: &Mutex<State>,
    stop: &AtomicBool,
    (mut sent, ended): (usize, u64),
) {
    // The read below is the pause between looks and the detector of a closed peer.
    if out
        .set_read_timeout(Some(Duration::from_millis(5)))
        .is_err()
    {
        return;
    }
    while !stop.load(Ordering::Relaxed) {
        let (fresh, cut): (Vec<String>, bool) = {
            let state = lock(state);
            (
                state.events.get(sent..).unwrap_or_default().to_vec(),
                state.events_ended != ended,
            )
        };
        if cut {
            // Shut down, not only dropped: the accept loop keeps a clone of
            // this connection for a restart, which would hold it open.
            let _ = out.shutdown(Shutdown::Both);
            return;
        }
        for line in &fresh {
            if out.write_all(format!("{line}\n").as_bytes()).is_err() {
                return;
            }
        }
        sent += fresh.len();
        let mut probe = [0_u8; 1];
        match out.read(&mut probe) {
            Ok(0) => return,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return,
        }
    }
}

/// `bytes` with every byte but ASCII alphanumerics and `-._~` written `%XX`.
fn percent_encode(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            text.push(char::from(*byte));
        } else {
            text.push_str(&format!("%{byte:02X}"));
        }
    }
    text
}

fn serve(stream: UnixStream, id: u64, state: &Mutex<State>, stop: &AtomicBool, sid: &str) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut authenticated = false;
    // Once `stream` is answered `OK`, every later line is a record.
    let mut streaming = false;
    let mut reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // A last line without its newline is not a whole line.
        if bytes.pop() != Some(b'\n') {
            break;
        }
        if streaming {
            lock(state).stream_line(id, &bytes);
            continue;
        }
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        let Ok(line) = std::str::from_utf8(&bytes) else {
            break;
        };
        if !authenticated {
            // `AUTH <token>` has no reply of its own; the next request's reply is
            // the first line a client reads.
            if line == format!("AUTH {TOKEN}") {
                authenticated = true;
                lock(state).connections += 1;
                continue;
            }
            let _ = out.write_all(b"ERR auth\n");
            break;
        }
        if line.starts_with("subscribe ") {
            if line != format!("subscribe @{sid} events") {
                let _ = out.write_all(b"ERR usage: subscribe\n");
                break;
            }
            let refusal = lock(state).subscribe_reply.clone();
            if let Some(reply) = refusal {
                let _ = out.write_all(format!("{reply}\n").as_bytes());
                break;
            }
            // Where the subscription starts is read before its ACK: an event pushed once the
            // client has read the ACK is the subscription's.
            let start = {
                let mut state = lock(state);
                state.subscriptions += 1;
                (state.events.len(), state.events_ended)
            };
            if out.write_all(subscribe_ack(sid).as_bytes()).is_ok() {
                stream_events(out, state, stop, start);
            }
            break;
        }
        if line == "stream" || line.starts_with("stream ") {
            streaming = lock(state).open_stream(id, line, sid, &mut out);
            continue;
        }
        let reply = if line == "version" {
            "OK version=fake-aterm".to_string()
        } else if let Some(verb) = line.strip_prefix(&format!("@{sid} ")) {
            // The request is recorded as it arrives and answered after the delay.
            let delay = lock(state).delay;
            let reply = lock(state).answer(id, verb);
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            reply
        } else {
            "ERR no such session".to_string()
        };
        if out.write_all(format!("{reply}\n").as_bytes()).is_err() {
            break;
        }
    }
    // The connection is gone, and a claim bound to it goes with it, as does a
    // stream it carried.
    let mut state = lock(state);
    if state
        .streams
        .open
        .as_ref()
        .is_some_and(|(open, _, _)| *open == id)
    {
        state.streams.close();
    }
    let graced = state.streams.grace;
    if let Some(claim) = state.claim.as_mut().filter(|claim| claim.connection == id) {
        if graced && claim.words.contract.as_deref() == Some("4") {
            // Kept, unbound, through the re-exec grace (D28).
            claim.connection = 0;
        } else {
            state.release();
        }
    }
}

#[cfg(test)]
#[path = "fake_tests.rs"]
mod tests;
