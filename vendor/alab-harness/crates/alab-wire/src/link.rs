// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The link: a harness's claim (contract 4, or 3) held over time, the reads that judge
//! a safe point and a relaunch by aterm's facts, and the event stream that
//! wakes the harness and carries aterm's restart requests.
//!
//! [`crate::claim`] holds the texts and the judges and performs no I/O;
//! [`crate::wire`] carries lines. [`Link`] is the state machine between them:
//! one request connection and one event subscription, or, while a session
//! stream is open, three connections (the claim's, which carries the stream, a
//! second request connection and the subscription), and what a harness does
//! when any of them stops answering. It keeps no clock (every call takes the
//! caller's `now_ms`), starts no process and writes no journal: what it would
//! journal it queues as lines the caller takes with [`Link::take_notes`].
//!
//! **The claim sequence** (`claim`), run once before the first vendor start
//! and again every [`RECLAIM_EVERY_MS`] while the link is lost:
//!
//! 1. connect and authenticate;
//! 2. `harness policy`: a contract the config declares served
//!    ([`Config::with_contracts`]; 3 and 4 unless the harness declares
//!    fewer) and `effective=on`; the claim is made at the highest of them
//!    the policy lists ([`crate::claim::Contract::choose`]), so a harness
//!    that declares `[4]` never claims 3. A policy listing none of them is
//!    [`Failure::Unserved`] and read again on the clock, since an aterm
//!    update can serve them;
//! 3. under contract 3 only, the roster guard: a read-only `native-act info`
//!    must name the harness's row. Nothing is claimed otherwise, not even to
//!    find out: aterm stores a contract-3 claim from a holder off its roster
//!    as a foreign claim that takes every duty ([`crate::claim::rostered`]).
//!    Contract 4 has no roster to ask: its proof reads the registry row,
//!    aterm answers a holder that row does not prove `ERR unproven` (refused,
//!    with nothing stored), and step 5 releases a claim `status` shows
//!    unproven. Under contract 3 a harness
//!    that asks for neither `relaunch` nor `upgrade` claims nothing, and the
//!    sequence is run again on the clock, so an updated aterm that lists
//!    contract 4 is claimed at 4;
//! 4. the claim, with the config's scope words that the contract names; a
//!    contract-4 reply states the grant ([`Grant::of_reply`]), which may be
//!    narrower than the claim;
//! 5. `status` must read the claim back as this harness's, proven, and taking
//!    every granted scope word (under contract 3, every word claimed), or the
//!    claim is released. A contract-4 `status` at a newer revision than the
//!    reply's (a cap that narrowed the claim in between) is the grant;
//! 6. the event subscription.
//!
//! **The grant.** The harness acts only on what aterm granted ([`Link::grant`],
//! [`Link::takes`]). A grant can change while the claim stands: a person's cap
//! narrows every live claim it covers, raises the claim's revision and aterm
//! sends `harness-policy`, on which the link reads `status` and keeps what it
//! shows of the words the claim asked for, and that revision. Every child
//! start is announced by sending the claim again with the new `child=`
//! ([`Link::reannounce`]), and a contract-4 reply is the grant from then on.
//!
//! A step that fails at launch leaves the link absent: the session then runs
//! on the harness's own readings for its whole life. The exceptions are an
//! answer that stands for now only ([`Failure::Later`]), an aterm that serves
//! no contract this harness can claim at ([`Failure::Unserved`]) and a policy
//! switched off by a setting that can be switched on again while the session
//! runs ([`Failure::Off`]): the session is unclaimed and the sequence is run again
//! every [`RECLAIM_EVERY_MS`], as for a lost link. [`Link::unclaimed`] says
//! which, so a caller that would rather get out of the way of an aterm that
//! serves none of its contracts can.
//!
//! **Busy is not gone.** Once aterm has answered the handshake on the request
//! connection it is there: a later exchange on that connection that outlives
//! its bound is aterm busy (it answers through its main thread), not aterm
//! away, so it is [`Failure::Later`] and the claim is made again on the clock
//! (kimi-alab's rule, measured there). Only a timeout is busy. A reply no
//! client may read (over its size bound, not UTF-8, without an `OK`/`ERR`
//! header) or a connection aterm closed stays [`Failure::Gone`], and an event
//! subscription aterm answered with anything but its ACK is
//! [`Failure::Refused`] (an incompatible aterm): either ends a launch as
//! absent, as the client this was ported from did, rather than claiming and
//! releasing again every [`RECLAIM_EVERY_MS`] and switching aterm's built-in
//! engine between standing down and resuming each time.
//!
//! **Bounds.** Opening at launch may take [`CONNECT_TIMEOUT`]; every later
//! exchange, the reopening of a lost link included, is bounded by
//! [`REQUEST_TIMEOUT`], because it runs in the harness's loop. An exchange that
//! fails or outlives its bound drops the connection and the link is lost. aterm
//! drops a contract-3 claim with the connection that made it, and keeps a
//! contract-4 claim for its re-exec grace, [`record::REEXEC_GRACE_MS`] (the
//! plan's D28), which the next claim renews. So a contract-4 claim the link
//! gives up is given back explicitly: a release, sent on a connection of its
//! own when the link is lost ([`Link::release`]), and when a claim made again
//! inside the grace is refused.
//!
//! **The session stream** (the plan's D24). Once a contract-4 claim is
//! granted, and unless the config says not to ([`Config::with_stream`]), the
//! link sends `stream @<sid> v=1` on the claim's connection. An `OK` turns that
//! connection into the stream ([`crate::stream`]); the link opens a second
//! request connection for every read, the release and [`Link::request`] included,
//! and keeps the event subscription. While the stream is open:
//!
//! * the caller's records go out with [`Link::send`], numbered, paced under
//!   aterm's rate budget and kept until aterm acknowledges them, and a stream
//!   opened again writes them again from its `acked=` under their numbers. Only
//!   when aterm's count went back (a new epoch, or a claim it forgot when its
//!   grace ended) are they numbered again, each move returned in
//!   [`Drained::renumbered`]; [`Drained::fresh`] tells the caller to send its
//!   `session` and `state` again;
//! * a `beat` goes out every [`BEAT_EVERY_MS`] of the caller's `now_ms`, unless
//!   the caller paused beats ([`Link::set_beating`]);
//! * [`Link::reannounce`] sends a `claim` record, and a `dropped=` answer to
//!   it loses the link as a refused claim line does;
//! * every record from aterm is answered `took` or `refused` once: `hand` fills
//!   the hand fields of [`Link::status`] and is returned as [`Drained::hand`],
//!   `policy` reads the grant again on the second connection, `restart` reaches
//!   the caller as [`Drained::restart`], and `want`, `prompt`, `answer`, `touch`,
//!   `interrupt` and `set` reach it as [`Drained::incoming`], which the caller
//!   answers ([`Link::answer`]);
//! * once the stream has brought one of aterm's own records, `EVENT restart`
//!   and `EVENT harness-policy` on the subscription are ignored, since the
//!   stream carries both, and so is the loss of the subscription, which hides
//!   neither. An aterm that opens streams and sends no record (HX-15b1 without
//!   HX-15c) is still heard through its events;
//! * an aterm hold is read again before its time only on a record or event that
//!   can end it (`hand`, `policy`, `restart`, the subscription's activity), not
//!   on the acknowledgements of the harness's own records.
//!
//! A stream that closes loses the link: the claim is made again on a new
//! connection, inside aterm's re-exec grace, and the stream asked for again,
//! within [`RECLAIM_EVERY_MS`]. A reply to `stream` that asks to be retried
//! (`ERR busy streams`, or an `ERR` ending in `retry`) keeps the v0.1 path above
//! and the records kept, and asks again on the same connection after
//! [`RECLAIM_EVERY_MS`]. Any other reply but `OK` (an aterm without the verb)
//! keeps the v0.1 path, unnoted, and asks again only on the next claim's
//! connection, since an aterm gains the verb only by an update or a restart,
//! which ends the connection. An `OK` that does not read ends the stream for
//! the link's life. Contract 3 never asks.
//!
//! The sockets are close-on-exec ([`crate::wire`]): a vendor child never
//! inherits them.

use std::ffi::OsString;
use std::os::fd::{AsRawFd as _, RawFd};
use std::time::Duration;

use crate::claim::{self, Child, Contract, Grant, Policy, Restart, SafePoint, Scope, Status};
use crate::config::Config;
use crate::record::{
    self, BEAT_EVERY_MS, Body, Dropped, LINE_MAX, LineError, REEXEC_GRACE_MS, Record, StreamReply,
};
use crate::relaunch;
use crate::stream::{
    self, AnswerError, Counts, Hand, Heard, Line, Lines, Outbox, Received, SEQ_MAX, SendError,
};
use crate::wire::{Connection, EventReader, Locator};

/// How long the launch may spend opening each of the two connections.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// The bound on every exchange made from the harness's loop, and on opening a
/// connection again from it.
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

/// How often a lost link is claimed again, an ended event stream reopened, and
/// at most how long an aterm hold waits before `status` is read again.
pub const RECLAIM_EVERY_MS: u64 = 5_000;

/// How soon a question aterm answered with [`Failure::Later`] is asked again on
/// a link that is still held.
pub const LATER_RETRY_MS: u64 = 1_000;

/// Why a step of the link did not succeed. The text is one journal word group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The connection could not be found, opened or authenticated, or a held
    /// connection cannot be used again.
    Gone(String),
    /// aterm answered, and the answer admits nothing.
    Refused(String),
    /// aterm answered that the policy is off by a setting a person can switch
    /// on again while the session runs (`why=disabled`, `agent-off` or
    /// `autopilot-off`; [`Policy::switched_off`]), which aterm announces to no
    /// link without a claim: a standing claim is released as for
    /// [`Failure::Refused`], and the policy is read again on the clock.
    Off(String),
    /// aterm answered, and the answer stands for now only, so the question is
    /// asked again: a read it refused with an `ERR` that asks to be retried
    /// (`ERR aterm is showing a dialog; answer it, then retry`, `ERR main
    /// thread stalled …; retry`), a policy that is off for a reason that
    /// passes, or a step after the handshake that aterm did not answer in
    /// time. aterm's last hold still stands through it.
    Later(String),
    /// aterm answered that it serves no contract this harness can claim at: a
    /// policy that lists none of the contracts the harness declares
    /// ([`Config::contracts`]: contract 2 alone, or `2,3` for a harness that
    /// declares `[4]`), or one whose highest such contract names none of the
    /// harness's duties (contract 3 for a harness without `relaunch` or
    /// `upgrade`). An aterm
    /// update can change either while the harness runs, so the question is
    /// asked again on the clock and a standing claim is kept, as for
    /// [`Failure::Later`]. Unlike it, the answer is aterm's last word on a
    /// lost link: no hold aterm stated before stands over it, as for
    /// [`Failure::Refused`].
    Unserved(String),
}

impl Failure {
    /// The journal text.
    #[must_use]
    pub fn why(&self) -> &str {
        match self {
            Self::Gone(why)
            | Self::Refused(why)
            | Self::Off(why)
            | Self::Later(why)
            | Self::Unserved(why) => why,
        }
    }

    /// Whether aterm answered that it takes no claim now, so a claim attempt
    /// made under its re-exec grace gives back the claim aterm may have kept
    /// (`claim`) and nothing of it stands after: the grace ends with the
    /// answer, at an adopt ([`Link::adopt_with`]) as on a reclaim
    /// ([`Link::tick`]).
    fn gives_back(&self) -> bool {
        matches!(self, Self::Refused(_) | Self::Off(_) | Self::Unserved(_))
    }
}

/// A reply to a read that could not be parsed as `step`'s: `Later` when aterm
/// refused it with an `ERR` that asks to be retried, which it does when it
/// cannot read its state now (`aterm/crates/aterm-gui/src/control_media.rs`,
/// `main_thread_refusal`; [`claim::ReplyError::Retry`]); `Refused` for any
/// other `ERR` (an unknown session, a principal aterm denies, a verb it does
/// not serve), which stands, and for an `OK` reply that does not state what
/// is needed.
fn unreadable(step: &str, error: &claim::ReplyError) -> Failure {
    match error {
        claim::ReplyError::Retry(_) => Failure::Later(format!("{step} {error}")),
        _ => Failure::Refused(format!("{step} {error}")),
    }
}

/// Whether `reply` is an `OK` line.
fn ok(reply: &str) -> bool {
    reply == "OK" || reply.starts_with("OK ")
}

/// The first two words of a reply aterm refused with, as `reply=<w>:<w>`.
fn reply_words(reply: &str) -> String {
    let words: Vec<String> = reply
        .split_ascii_whitespace()
        .take(2)
        .map(claim::journal_word)
        .collect();
    if words.is_empty() {
        "reply=(empty)".to_string()
    } else {
        format!("reply={}", words.join(":"))
    }
}

/// How long after a hold `status` is read again when no event arrives first:
/// the rest of the person's grace plus a second, and at most
/// [`RECLAIM_EVERY_MS`]. An ageing `human_ms` emits no event, so its end has to
/// be read for.
#[must_use]
pub fn recheck_after_ms(grace_left: Option<Duration>) -> u64 {
    grace_left.map_or(RECLAIM_EVERY_MS, |left| {
        u64::try_from(left.as_millis())
            .unwrap_or(u64::MAX)
            .saturating_add(1_000)
            .min(RECLAIM_EVERY_MS)
    })
}

/// Send `request` for `step` and read its one reply line. The line is read
/// leniently: a reply such as `native-act info`'s carries a bare word, which
/// the strict field reader would refuse and poison the connection over.
fn ask(conn: &mut Connection, step: &str, request: Option<String>) -> Result<String, Failure> {
    let request = request.ok_or_else(|| Failure::Refused(format!("{step} request-unsendable")))?;
    conn.request_line(&request)
        .map_err(|error| Failure::Gone(format!("{step} {error}")))
}

/// One fresh `harness policy`.
fn read_policy(conn: &mut Connection, sid: &str) -> Result<Policy, Failure> {
    let reply = ask(conn, "policy", claim::policy_verb(sid))?;
    Policy::parse(&reply).map_err(|error| unreadable("policy", &error))
}

/// One fresh `status`.
fn read_status(conn: &mut Connection, sid: &str) -> Result<Status, Failure> {
    let reply = ask(conn, "status", claim::status_verb(sid))?;
    Status::parse(&reply).map_err(|error| unreadable("status", &error))
}

/// Whether `policy` lets a rostered harness that declares the contracts
/// `declared` ([`Config::contracts`]) claim: one of them served and the policy
/// in effect. A policy that lists none of them (an aterm that serves only
/// contract 2, or `2,3` to a harness that declares `[4]`) is
/// [`Failure::Unserved`], `policy contract=<served> declared=<declared>`: an
/// aterm update can serve one while the harness runs, and the harness claims
/// again after one (the interface design, section 2.2). A policy off by a
/// setting that can be switched on again
/// is [`Failure::Off`], and one off by a lasting fact (`unhosted`, `headless`)
/// [`Failure::Refused`]. A policy off for a reason that passes (a halt, aterm
/// suspended for its own update, a reason not known here) is
/// [`Failure::Later`]: a standing claim is kept through it and nothing moves
/// meanwhile, because aterm derives a halt's `hold=1` and the policy's
/// `why=hold` from the same fact (`harness_policy_verb.rs`, `effective`) and
/// emits no event when either changes.
///
/// # Errors
/// The policy admits no claim now, as above.
pub fn admits(policy: &Policy, declared: &[Contract]) -> Result<(), Failure> {
    if policy.contract(declared).is_none() {
        let contracts = policy
            .contracts
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        return Err(Failure::Unserved(format!(
            "policy contract={contracts} declared={}",
            claim::contract_words(declared)
        )));
    }
    if policy.switched_off() {
        return Err(Failure::Off(format!("policy {}", policy.off_words())));
    }
    if policy.off_by_configuration() {
        return Err(Failure::Refused(format!("policy {}", policy.off_words())));
    }
    if !policy.effective {
        return Err(Failure::Later(format!("policy {}", policy.off_words())));
    }
    Ok(())
}

/// Whether `status` reads back `holder`'s claim as aterm's proven claim taking
/// every word of `scopes` (the granted words): step 5 of the claim sequence. A
/// claim aterm stored with fewer duties than it granted would leave a duty
/// nobody performs.
///
/// # Errors
/// The first fact that does not read so, as [`Failure::Refused`].
pub fn proof(status: &Status, holder: &str, scopes: &[Scope]) -> Result<(), Failure> {
    if !status.held_by(holder) {
        return Err(Failure::Refused(format!(
            "unproven supervisor={} supervisor_proven={}",
            status
                .supervisor
                .as_deref()
                .map_or_else(|| "-".to_string(), claim::journal_word),
            u8::from(status.supervisor_proven)
        )));
    }
    if let Some(missing) = scopes.iter().find(|scope| !status.takes(**scope)) {
        let scope = status.supervisor_scope.join(",");
        return Err(Failure::Refused(format!(
            "unproven scope={} supervisor_scope={}",
            missing.word(),
            if scope.is_empty() {
                "-".to_string()
            } else {
                claim::journal_word(&scope)
            }
        )));
    }
    Ok(())
}

/// Whether `status` states a claim revision newer than the one `grant` holds:
/// aterm changed the claim since that grant was read.
fn newer(status: &Status, grant: &Grant) -> bool {
    match (status.supervisor_revision, grant.revision) {
        (Some(now), Some(held)) => now > held,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// The claim line for the journal.
fn claimed_line(grant: &Grant, holder: &str, child: Child<'_>) -> String {
    format!(
        "aterm claimed holder={holder} contract={} scope={} revision={} vendor={}",
        grant.contract.number(),
        grant.words(),
        grant.revision_word(),
        child
            .vendor
            .map_or_else(|| "-".to_string(), claim::journal_word)
    )
}

/// Steps 2 to 5 of the claim sequence on an open connection: the policy, the
/// roster guard (contract 3), the claim at the contract chosen of those the
/// policy lists and the config declares, and its proof
/// read back from `status`. A claim `status` does not show as this harness's,
/// proven and taking every granted scope word is released before the refusal
/// is returned. The grant, when it stands, and the `status` that proved it;
/// or the failure, and whether a claim this sequence made may still stand
/// because aterm did not take or did not answer its release.
fn guarded_claim(
    conn: &mut Connection,
    config: &Config,
    sid: &str,
    pid: u32,
    child: Child<'_>,
) -> Result<(Grant, Status), (Failure, bool)> {
    let made = |failure: Failure| (failure, false);
    let policy = read_policy(conn, sid).map_err(made)?;
    admits(&policy, config.contracts()).map_err(made)?;
    let Some(contract) = policy.contract(config.contracts()) else {
        return Err(made(Failure::Refused("policy contract=-".to_string())));
    };
    let asked = contract.scopes(config.scopes());
    if asked.is_empty() {
        // A harness that asks only for duties contract 3 does not name: an
        // aterm updated while it runs may list contract 4, at which it claims
        // (the interface design, section 2.2: a harness process outlives an
        // aterm update and claims again afterwards), so the policy is read
        // again on the clock. `Config` is never empty, so contract 4 always
        // names a word.
        return Err(made(Failure::Unserved(format!(
            "contract={} scope=-",
            contract.number()
        ))));
    }
    // Never a trial claim under contract 3: without the row, nothing below is
    // sent. Contract 4 has no such list: its image rule reads the registry
    // row, a verified record may claim any word, and a claim `status` does
    // not show proven is released below (the interface design, section 2.2
    // and HX-11).
    if contract == Contract::Three
        && !claim::rostered(
            &ask(conn, "roster", claim::info_verb(sid)).map_err(made)?,
            config.id(),
        )
    {
        return Err(made(Failure::Refused("unrostered".to_string())));
    }
    let reply = ask(
        conn,
        "claim",
        claim::claim_verb(config, contract, sid, pid, child),
    )
    .map_err(made)?;
    if !ok(&reply) {
        return Err(made(Failure::Refused(format!(
            "claim {}",
            reply_words(&reply)
        ))));
    }
    // A claim this sequence made and then gives back. A release aterm did not
    // take, or did not answer, may leave the claim standing: the failure is
    // then [`Failure::Gone`], so a caller that kept the connection (a claim
    // made again on a held link) drops it, and the claim is marked as one that
    // may stand. aterm drops a contract-3 claim with its connection; a
    // contract-4 claim it keeps through its re-exec grace (D28), so the caller
    // gives that one back on a connection of its own.
    let release = |conn: &mut Connection, failure: Failure| -> (Failure, bool) {
        match ask(conn, "release", claim::release_verb(config, sid, pid)) {
            Ok(reply) if ok(&reply) => (failure, false),
            Ok(reply) => (
                Failure::Gone(format!(
                    "{} (release {})",
                    failure.why(),
                    reply_words(&reply)
                )),
                true,
            ),
            Err(gone) => (
                Failure::Gone(format!("{} ({})", failure.why(), gone.why())),
                true,
            ),
        }
    };
    let granted = match contract {
        Contract::Three => None,
        Contract::Four => match Grant::of_reply(&reply, &asked) {
            Ok(grant) => Some(grant),
            Err(error) => return Err(release(conn, Failure::Refused(format!("claim {error}")))),
        },
    };
    let holder = config.holder(pid);
    let proven = read_status(conn, sid).and_then(|status| {
        let grant = match &granted {
            // A cap that narrowed the claim between its reply and this read
            // shows as a newer revision: aterm's later word is the grant.
            Some(reply) if status.held_by(&holder) && newer(&status, reply) => {
                Grant::of_status(contract, &status, &asked)
            }
            Some(reply) => reply.clone(),
            None => Grant::of_status(contract, &status, &asked),
        };
        let needed = if contract == Contract::Three {
            &asked
        } else {
            &grant.scopes
        };
        proof(&status, &holder, needed).map(|()| (grant, status))
    });
    match proven {
        Err(failure @ (Failure::Refused(_) | Failure::Later(_))) => Err(release(conn, failure)),
        Err(failure) => Err(made(failure)),
        Ok(proven) => Ok(proven),
    }
}

/// A step after the handshake on `conn` that aterm did not answer in time
/// (`timed_out`): aterm is busy, not away (module documentation). Any other
/// failure is kept as it is.
fn busy(failure: Failure, timed_out: bool) -> Failure {
    match failure {
        Failure::Gone(why) if timed_out => {
            Failure::Later(format!("{why} (aterm answered the handshake)"))
        }
        other => other,
    }
}

/// The event subscription could not be made: aterm answered it with anything
/// but its ACK ([`std::io::ErrorKind::InvalidData`], which the strict reader
/// gives an `ERR` or an ACK of another shape) is [`Failure::Refused`], since
/// asking again will be answered the same; a socket that could not be opened,
/// authenticated or answered in time is [`Failure::Gone`].
fn events_failure(error: &std::io::Error) -> Failure {
    if error.kind() == std::io::ErrorKind::InvalidData {
        Failure::Refused(format!("events {error}"))
    } else {
        Failure::Gone(format!("events {error}"))
    }
}

/// Give `config`'s claim by harness process `pid` back on a connection of its
/// own, opened and answered within [`REQUEST_TIMEOUT`], its answer ignored:
/// the release of a contract-4 claim aterm may keep through its re-exec grace
/// after the connection that made it closed (D28), which `meta unset
/// supervisor` from any connection the claimant's process proves ends at once
/// (D23).
fn release_anew(config: &Config, locator: &Locator, pid: u32) {
    let Ok(socket) = locator.socket() else {
        return;
    };
    let Ok(mut conn) = Connection::connect(&socket, REQUEST_TIMEOUT) else {
        return;
    };
    conn.set_timeout(REQUEST_TIMEOUT);
    let _ = ask(
        &mut conn,
        "release",
        claim::release_verb(config, locator.sid(), pid),
    );
}

/// The whole claim sequence for the session `locator` names, by `config`'s
/// harness process `pid` running `child`. Each connection may take `opening`
/// to open; every exchange after that is bounded by [`REQUEST_TIMEOUT`].
///
/// # Errors
/// [`Failure::Gone`] when the socket cannot be found, opened or
/// authenticated, or a reply could not be read; [`Failure::Later`] when aterm
/// answered the handshake and then a step asked to be retried or was not
/// answered in time; [`Failure::Unserved`] when aterm serves no contract
/// the harness can claim at; [`Failure::Off`] or [`Failure::Refused`] when
/// aterm answered and the answer admits no claim, the event subscription's
/// included. A claim made before a later step failed is released, on a
/// connection of its own when the claim's connection could not release it.
/// `graced` is a contract-4 claim of this harness's that aterm may keep in its
/// re-exec grace (a link lost, or an image adopted): when aterm answers that
/// it takes no claim now, that claim is given back too, since nothing else
/// would end it before the grace does.
fn claim(
    config: &Config,
    locator: &Locator,
    pid: u32,
    child: Child<'_>,
    opening: Duration,
    graced: bool,
) -> Result<Claimed, Failure> {
    let socket = locator
        .socket()
        .map_err(|error| Failure::Gone(format!("graph {error}")))?;
    let mut conn = Connection::connect(&socket, opening)
        .map_err(|error| Failure::Gone(format!("connect {error}")))?;
    conn.set_timeout(REQUEST_TIMEOUT);
    let (grant, status) = match guarded_claim(&mut conn, config, locator.sid(), pid, child) {
        Ok(proven) => proven,
        Err((failure, stands)) => {
            let failure = busy(failure, conn.timed_out());
            if stands {
                drop(conn);
                release_anew(config, locator, pid);
            } else if graced && failure.gives_back() {
                let _ = ask(
                    &mut conn,
                    "release",
                    claim::release_verb(config, locator.sid(), pid),
                );
            }
            return Err(failure);
        }
    };
    match EventReader::connect(&socket, locator.sid(), opening) {
        Ok(events) => Ok(Claimed {
            conn,
            locator: locator.clone(),
            pid,
            holder: config.holder(pid),
            standing: true,
            grant,
            events: Some(events),
            events_retry_ms: 0,
            recheck_at_ms: 0,
            policy_due_ms: 0,
            noted: String::new(),
            owed: false,
            last_hold: None,
            unreleased: false,
            stream: None,
            stream_ask: Ask::At(0),
            status: Some(status),
        }),
        Err(error) => {
            let _ = ask(
                &mut conn,
                "release",
                claim::release_verb(config, locator.sid(), pid),
            );
            Err(events_failure(&error))
        }
    }
}

/// What one fresh policy and status pair said, and what the claim did about it.
enum Judged {
    /// aterm sees nobody the move would interrupt.
    Safe,
    /// A field holds; the journal line.
    Hold(String),
    /// `status` no longer shows this harness's proven claim; the journal line
    /// of the hold. The connection is of no further use.
    Taken(String),
    /// The policy no longer admits a claim and the claim was released; the
    /// journal line.
    Released(String),
}

/// aterm's word on starting the child again, from one fresh policy and status
/// pair ([`relaunch::permitted`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Relaunch {
    /// The claim is this harness's, proven and takes `relaunch`, the policy
    /// permits it, and the exit is nobody else's.
    Permitted,
    /// aterm answered, or no claim stands, and nothing is started; the reason
    /// as journal words.
    Denied(String),
    /// aterm did not answer, or the link was already lost: the claim may be
    /// proven again within [`relaunch::AWAY_BUDGET`]; the reason as journal
    /// words.
    Away(String),
    /// aterm answered on a link that is still held, with an answer that stands
    /// for now only ([`Failure::Later`]): ask again after [`LATER_RETRY_MS`],
    /// within [`relaunch::AWAY_BUDGET`]; the reason as journal words.
    Later(String),
}

/// What one fresh policy and status pair said about starting the child again.
enum Leave {
    /// Everything [`relaunch::permitted`] asks for holds.
    Granted,
    /// The claim stands and something else does not; the reason.
    Denied(String),
    /// `status` no longer shows this harness's proven claim; the reason. The
    /// connection is of no further use.
    Taken(String),
    /// aterm could not answer now; the reason. The claim stands.
    Later(String),
}

/// One fresh policy and status pair, or what stood in for it.
enum Pair {
    /// Both read, on a claim that stands and is still this harness's.
    Fresh(Box<Facts>),
    /// aterm could not give the pair now, or its policy is off for now; the
    /// journal line of the hold.
    Hold(String),
    /// `status` no longer shows this harness's proven claim; the journal line
    /// of the hold. The connection is of no further use.
    Taken(String),
    /// The policy no longer admits a claim and the claim was released; the
    /// journal line.
    Released(String),
}

/// An open, authenticated link on which the harness's claim was proven.
#[derive(Debug)]
struct Claimed {
    /// The request connection the claim is bound to.
    conn: Connection,
    /// Where the session is found again.
    locator: Locator,
    /// The harness's pid, as claimed.
    pid: u32,
    /// `<id>@<pid>`.
    holder: String,
    /// Whether the claim stands. It does not while aterm's policy is off for
    /// the session: the claim was released, the connection and the event
    /// stream are kept, and a policy that reads on again claims again.
    standing: bool,
    /// What aterm granted the claim, as of its last claim, announcement or
    /// `status` read after a policy event. While the claim does not stand it is the grant it last had,
    /// and nothing acts on it.
    grant: Grant,
    /// The event subscription, until it ends.
    events: Option<EventReader>,
    /// When (ms) an ended event stream is opened again.
    events_retry_ms: u64,
    /// Until when (ms) an aterm hold stands without `status` being read again;
    /// an event ends the wait at once.
    recheck_at_ms: u64,
    /// When (ms) the policy is read again without an event, or 0 for not on a
    /// clock: set while the claim does not stand and after a policy read
    /// aterm could answer only later. A session's Autopilot word and a halt
    /// change `effective=` with no `harness-policy` event.
    policy_due_ms: u64,
    /// The last line [`Claimed::follow_policy`] noted, so a read on the clock
    /// that finds the same is not noted again.
    noted: String,
    /// Whether the last policy read [`Claimed::follow_policy`] made was one
    /// aterm could answer only later, so an owed read is noted once.
    owed: bool,
    /// The hold aterm's last safe-point answer stated (`wait aterm …`), or
    /// `None` when it called the moment safe or has not been asked. A link
    /// lost after a hold keeps holding until the claim is proven again.
    last_hold: Option<String>,
    /// Whether a release of the claim went untaken or unanswered, so the claim
    /// may stand at aterm although [`Claimed::standing`] says it does not.
    unreleased: bool,
    /// The session stream, while one is open on the connection the claim was
    /// made on; [`Claimed::conn`] is then the second request connection.
    stream: Option<Open>,
    /// When the stream is asked for on this connection.
    stream_ask: Ask,
    /// The last `status` aterm gave, with the hand fields its `hand` records
    /// moved since.
    status: Option<Status>,
}

/// When the session stream is asked for on the claim's connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Once `now_ms` reaches this (0: at once).
    At(u64),
    /// Not on this connection: aterm answered `stream` with an `ERR` that asks
    /// for no retry, which an aterm process answers for its life.
    Never,
}

/// An open session stream.
#[derive(Debug)]
struct Open {
    /// The claim's connection, which carries records.
    lines: Lines,
    /// When (ms) it opened.
    opened_ms: u64,
    /// When (ms) the next `beat` is due.
    beat_due_ms: u64,
    /// The `claim` record whose acknowledgement is awaited: its `seq` and its
    /// body, written again under a new number if aterm drops it for the rate.
    claim: Option<(u64, Body)>,
    /// Whether the stream has brought one of aterm's own records (any but an
    /// `ack`): from then on it carries `restart` and `policy`, and the
    /// subscription's events for them are ignored.
    heard: bool,
    /// The `dropped=` reasons noted on this stream, so each is noted once.
    noted: Vec<String>,
}

impl Claimed {
    /// Give the claim back, keeping the connection. The policy is read again
    /// every [`RECLAIM_EVERY_MS`] from here ([`Link::tick`]): aterm sends no
    /// event when a session's own Autopilot word changes, so nothing else would
    /// find the policy on again.
    ///
    /// While a stream is open, `conn` is the second connection, which the
    /// release goes on, and the stream, which a release closes (the plan's
    /// D23), is dropped.
    fn release(&mut self, config: &Config, now_ms: u64) -> Result<(), Failure> {
        self.standing = false;
        self.policy_due_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
        let reply = ask(
            &mut self.conn,
            "release",
            claim::release_verb(config, self.locator.sid(), self.pid),
        );
        self.stream = None;
        match reply {
            Ok(reply) if ok(&reply) => Ok(()),
            // A release aterm did not take or answer: the connection the claim
            // is bound to is closed, which ends a contract-3 claim, and the
            // claim is marked as one that may stand, which a contract-4 claim
            // does through aterm's re-exec grace until a release ends it.
            Ok(reply) => {
                self.unreleased = true;
                Err(Failure::Gone(format!("release {}", reply_words(&reply))))
            }
            Err(failure) => {
                self.unreleased = true;
                Err(failure)
            }
        }
    }

    /// Read the policy afresh and make the claim follow it: release a standing
    /// claim the policy no longer admits by configuration, keep it through a
    /// policy that is off for now, and claim again (through the whole guarded
    /// sequence) once the policy admits a claim. The line to note, when
    /// anything changed. A read aterm could answer only later (`ERR … retry`)
    /// on a standing claim is owed: it is made again every
    /// [`RECLAIM_EVERY_MS`] until aterm answers it, and `aterm policy-owed
    /// <why>` is noted when it begins to be owed (Decision 18).
    fn follow_policy(
        &mut self,
        config: &Config,
        child: Child<'_>,
        now_ms: u64,
    ) -> Result<Option<String>, Failure> {
        let sid = self.locator.sid().to_string();
        let again = now_ms.saturating_add(RECLAIM_EVERY_MS);
        let read = read_policy(&mut self.conn, &sid);
        let owed = matches!(read, Err(Failure::Later(_)));
        let was_owed = std::mem::replace(&mut self.owed, owed);
        let line = match read.and_then(|policy| admits(&policy, config.contracts())) {
            Err(Failure::Gone(why)) => return Err(Failure::Gone(why)),
            Err(Failure::Refused(why) | Failure::Off(why)) if self.standing => {
                self.release(config, now_ms)?;
                format!("aterm released {why}")
            }
            Err(
                Failure::Refused(why)
                | Failure::Off(why)
                | Failure::Later(why)
                | Failure::Unserved(why),
            ) => {
                self.policy_due_ms = again;
                if self.standing {
                    // Off for now (a halt), or not read: the claim stands, and
                    // each look holds on aterm's word meanwhile.
                    return Ok((owed && !was_owed).then(|| format!("aterm policy-owed {why}")));
                }
                format!("aterm unclaimed {why}")
            }
            Ok(()) if self.standing => {
                self.policy_due_ms = 0;
                match self.follow_grant(config, &sid, now_ms)? {
                    Some(line) => line,
                    None => return Ok(None),
                }
            }
            Ok(()) => match guarded_claim(&mut self.conn, config, &sid, self.pid, child) {
                Ok((grant, status)) => {
                    self.standing = true;
                    self.unreleased = false;
                    self.recheck_at_ms = 0;
                    self.policy_due_ms = 0;
                    if self.stream_ask != Ask::Never {
                        self.stream_ask = Ask::At(0);
                    }
                    self.last_hold = None;
                    self.status = Some(status);
                    let line = claimed_line(&grant, &self.holder, child);
                    self.grant = grant;
                    line
                }
                Err((Failure::Gone(why), stands)) => {
                    self.unreleased |= stands;
                    return Err(Failure::Gone(why));
                }
                Err((
                    Failure::Refused(why)
                    | Failure::Off(why)
                    | Failure::Later(why)
                    | Failure::Unserved(why),
                    _,
                )) => {
                    self.policy_due_ms = again;
                    format!("aterm unclaimed {why}")
                }
            },
        };
        if line == self.noted {
            return Ok(None);
        }
        self.noted.clone_from(&line);
        Ok(Some(line))
    }

    /// After a policy event on a standing claim, of either contract: read
    /// `status` and keep the grant aterm now shows, of the words the claim
    /// asked for. A person's cap narrows every live claim it covers (the
    /// interface design, section 2.2), and the built-in engine acts on what
    /// `status` shows, so the held grant follows it both ways: a word aterm
    /// withdrew is no longer acted on, and one it shows again is taken up,
    /// since aterm's engine stands down for it. A `status` at an older
    /// revision than the grant's is passed over. The line `aterm narrowed
    /// scope=<granted> revision=<n>` when words were withdrawn and none added,
    /// `aterm widened …` when words were added and none withdrawn, and `aterm
    /// regranted …` for both; a newer revision alone is kept without a line. A
    /// `status` that does not show this harness's proven claim changes nothing
    /// here (the next look judges it), and one aterm cannot give now is read
    /// again after [`LATER_RETRY_MS`]. Only a connection that failed is an
    /// error.
    fn follow_grant(
        &mut self,
        config: &Config,
        sid: &str,
        now_ms: u64,
    ) -> Result<Option<String>, Failure> {
        let status = match read_status(&mut self.conn, sid) {
            Ok(status) => status,
            Err(Failure::Refused(_) | Failure::Off(_) | Failure::Later(_)) => {
                self.policy_due_ms = now_ms.saturating_add(LATER_RETRY_MS);
                return Ok(None);
            }
            Err(gone) => return Err(gone),
        };
        self.status = Some(status.clone());
        if !status.held_by(&self.holder) {
            return Ok(None);
        }
        let contract = self.grant.contract;
        let now = Grant::of_status(contract, &status, &contract.scopes(config.scopes()));
        let older = matches!(
            (now.revision, self.grant.revision),
            (Some(now), Some(held)) if now < held
        );
        if older || now == self.grant {
            return Ok(None);
        }
        let withdrawn = self.grant.scopes.iter().any(|scope| !now.takes(*scope));
        let added = now.scopes.iter().any(|scope| !self.grant.takes(*scope));
        let revision = if newer(&status, &self.grant) {
            now.revision
        } else {
            self.grant.revision
        };
        self.grant = Grant { revision, ..now };
        let verb = match (withdrawn, added) {
            (false, false) => return Ok(None),
            (true, false) => "narrowed",
            (false, true) => "widened",
            (true, true) => "regranted",
        };
        Ok(Some(format!(
            "aterm {verb} scope={} revision={}",
            self.grant.words(),
            self.grant.revision_word()
        )))
    }

    /// One fresh `harness policy` and one fresh `status`, judged as an upgrade
    /// safe point ([`claim::judge_safe_point`]). A reply that cannot be read
    /// holds; only a connection that failed is an error.
    fn judge(&mut self, config: &Config, grace: Duration, now_ms: u64) -> Result<Judged, Failure> {
        let judged = self.judge_pair(config, grace, now_ms);
        match &judged {
            Ok(Judged::Hold(hold)) => self.last_hold = Some(hold.clone()),
            // A claim taken by another holder states no hold on the session:
            // the duty is that holder's, and this harness's looks are its own.
            Ok(Judged::Safe | Judged::Taken(_) | Judged::Released(_)) => self.last_hold = None,
            Err(_) => {}
        }
        judged
    }

    /// [`Claimed::judge`] without remembering the answer.
    fn judge_pair(
        &mut self,
        config: &Config,
        grace: Duration,
        now_ms: u64,
    ) -> Result<Judged, Failure> {
        let Facts { policy, status } = match self.read_pair(config, now_ms)? {
            Pair::Fresh(facts) => *facts,
            Pair::Hold(why) => return Ok(Judged::Hold(why)),
            Pair::Taken(why) => return Ok(Judged::Taken(why)),
            Pair::Released(line) => return Ok(Judged::Released(line)),
        };
        match claim::judge_safe_point(&policy, &status, &self.holder, grace) {
            SafePoint::Safe => {
                self.recheck_at_ms = 0;
                Ok(Judged::Safe)
            }
            SafePoint::Hold(hold) => {
                self.recheck_at_ms = now_ms.saturating_add(recheck_after_ms(hold.grace_left));
                Ok(Judged::Hold(hold.to_string()))
            }
        }
    }

    /// One fresh `harness policy` and one fresh `status` on a standing claim.
    /// A policy that no longer admits the claim by configuration releases it;
    /// one off for now, or a reply aterm cannot give now, holds; a `status`
    /// that no longer shows the claim as this harness's, proven, is
    /// [`Judged::Taken`]. Only a connection that failed is an error.
    fn read_pair(&mut self, config: &Config, now_ms: u64) -> Result<Pair, Failure> {
        let sid = self.locator.sid().to_string();
        self.recheck_at_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
        let hold = |why: String| Ok(Pair::Hold(format!("wait aterm {why}")));
        let policy = match read_policy(&mut self.conn, &sid) {
            Ok(policy) => policy,
            Err(Failure::Refused(why) | Failure::Later(why)) => return hold(why),
            Err(gone) => return Err(gone),
        };
        match admits(&policy, config.contracts()) {
            Err(Failure::Refused(why) | Failure::Off(why)) => {
                self.release(config, now_ms)?;
                let line = format!("aterm released {why}");
                // A claim made again later is noted ([`Claimed::noted`]).
                self.noted.clone_from(&line);
                return Ok(Pair::Released(line));
            }
            // Off for now: the claim stands and the move waits, and the policy
            // is read again on the recheck clock, since nothing announces the
            // end of a halt. A contract no longer served comes only with
            // aterm's update, which ends the connection first.
            Err(Failure::Later(why) | Failure::Unserved(why)) => return hold(why),
            Err(gone @ Failure::Gone(_)) => return Err(gone),
            Ok(()) => {}
        }
        let status = match read_status(&mut self.conn, &sid) {
            Ok(status) => status,
            Err(Failure::Refused(why) | Failure::Later(why)) => return hold(why),
            Err(gone) => return Err(gone),
        };
        self.status = Some(status.clone());
        if !status.held_by(&self.holder) {
            // The judge names the claim's own field first.
            let taken =
                match claim::judge_safe_point(&policy, &status, &self.holder, Duration::ZERO) {
                    SafePoint::Hold(hold) => hold.to_string(),
                    SafePoint::Safe => "wait aterm supervisor=absent".to_string(),
                };
            return Ok(Pair::Taken(taken));
        }
        Ok(Pair::Fresh(Box::new(Facts { policy, status })))
    }

    /// One fresh `harness policy` and one fresh `status`, judged as leave to
    /// start the child again ([`relaunch::permitted`]). An `OK` reply that
    /// does not state what is needed grants nothing. A reply aterm could give
    /// only later is [`Leave::Later`]: the question is asked again. For a
    /// crash so is a policy off for a reason that is neither configuration
    /// nor a halt (aterm suspended for its own update, a reason not known
    /// here); a halt denies it, since the exit is the holder's. A restart aterm
    /// asked for (`asked`) is not judged by `effective=`: aterm routed it to
    /// this claim without reading the policy, and its own host makes an asked
    /// restart whatever the switches say. Only a connection that failed is an
    /// error.
    fn relaunch_word(&mut self, asked: bool) -> Result<Leave, Failure> {
        let sid = self.locator.sid().to_string();
        let policy = match read_policy(&mut self.conn, &sid) {
            Ok(policy) => policy,
            Err(Failure::Refused(why)) => return Ok(Leave::Denied(why)),
            Err(Failure::Later(why)) => return Ok(Leave::Later(why)),
            Err(gone) => return Err(gone),
        };
        if !asked
            && !policy.effective
            && !policy.off_by_configuration()
            && policy.why.as_deref() != Some("hold")
        {
            return Ok(Leave::Later(format!("policy {}", policy.off_words())));
        }
        let status = match read_status(&mut self.conn, &sid) {
            Ok(status) => status,
            Err(Failure::Refused(why)) => return Ok(Leave::Denied(why)),
            Err(Failure::Later(why)) => return Ok(Leave::Later(why)),
            Err(gone) => return Err(gone),
        };
        self.status = Some(status.clone());
        Ok(
            match relaunch::permitted(&policy, &status, &self.holder, asked) {
                Ok(()) => Leave::Granted,
                Err(why) if !status.held_by(&self.holder) => Leave::Taken(why),
                Err(why) => Leave::Denied(why),
            },
        )
    }

    /// Ask for the session stream on the claim's connection. On `OK` that
    /// connection carries records from here, and a second request connection,
    /// opened within `opening`, takes its place as [`Claimed::conn`]. Any other
    /// reply leaves the connection in request mode with the claim, and nothing
    /// is noted. An exchange that fails, or a second connection that cannot be
    /// opened, is [`Failure::Gone`]: the claim's connection is of no further
    /// use.
    fn open_stream(&mut self, opening: Duration, now_ms: u64) -> Result<Asked, Failure> {
        let reply = match ask(
            &mut self.conn,
            "stream",
            record::stream_request(self.locator.sid()),
        ) {
            Ok(reply) => reply,
            // A session ID the verb cannot name: never asked.
            Err(Failure::Refused(_)) => return Ok(Asked::Refused),
            Err(failure) => return Err(failure),
        };
        if !ok(&reply) {
            let retry = reply.starts_with("ERR busy streams")
                || matches!(
                    claim::ReplyError::refused(&reply),
                    claim::ReplyError::Retry(_)
                );
            return Ok(if retry { Asked::Busy } else { Asked::Refused });
        }
        let reply = match StreamReply::parse(&reply) {
            Ok(reply) if reply.acked <= SEQ_MAX && reply.sent <= SEQ_MAX => reply,
            Ok(reply) => {
                return Ok(Asked::Unreadable(format!(
                    "acked={} in={} over-seq-max",
                    reply.acked, reply.sent
                )));
            }
            Err(error) => return Ok(Asked::Unreadable(error.to_string())),
        };
        let mut second = self
            .locator
            .socket()
            .and_then(|socket| Connection::connect(&socket, opening))
            .map_err(|error| Failure::Gone(format!("stream second {error}")))?;
        second.set_timeout(REQUEST_TIMEOUT);
        let claims = std::mem::replace(&mut self.conn, second);
        let lines = claims
            .into_parts()
            .and_then(|(socket, pending)| Lines::new(socket, pending))
            .map_err(|error| Failure::Gone(format!("stream {error}")))?;
        self.stream = Some(Open {
            lines,
            opened_ms: now_ms,
            beat_due_ms: now_ms.saturating_add(BEAT_EVERY_MS),
            claim: None,
            heard: false,
            noted: Vec::new(),
        });
        Ok(Asked::Open(reply))
    }
}

/// aterm's answer to `stream @<sid> v=1`.
#[derive(Debug)]
enum Asked {
    /// `OK`: the stream is open.
    Open(StreamReply),
    /// An `ERR` that asks to be retried (`ERR busy streams`, the pool full, or
    /// one ending in `retry`): asked again on this connection later.
    Busy,
    /// Any other `ERR`: not asked again on this connection.
    Refused,
    /// An `OK` that does not read as the stream's, or names a number over
    /// [`SEQ_MAX`]; why. aterm may have turned the connection into a stream.
    Unreadable(String),
}

/// What the link holds of aterm.
#[derive(Debug)]
enum State {
    /// Nothing, for the rest of the session: the harness is outside aterm, or
    /// the launch's claim sequence was refused. Never retried.
    Absent,
    /// An open link on which the claim was proven.
    Held(Box<Claimed>),
    /// No link, and a claim to make through the whole guarded sequence once
    /// `retry_at_ms` has passed: a link that was held and stopped answering,
    /// or a launch whose claim aterm could answer only later, or whose policy
    /// was switched off.
    Lost {
        /// Where the session is found again.
        locator: Locator,
        /// When (ms) the claim is next attempted.
        retry_at_ms: u64,
        /// aterm's answer to the last claim attempt, so a reason is noted
        /// once ([`Link::unclaimed`]); `None` before the first attempt since
        /// the link was lost.
        unclaimed: Option<Failure>,
        /// Whether a claim stood before: the link was held. A launch that has
        /// not claimed yet has no duty towards the child.
        claimed: bool,
        /// The hold aterm stated last before the link was lost
        /// ([`Claimed::last_hold`]), as the line of the wait it keeps: nothing
        /// moves on the harness's own readings over it. It stands until aterm
        /// answers again: a proven claim, or a refusal, which is aterm's newer
        /// word.
        held: Option<String>,
        /// Until when (ms) a contract-4 claim of this harness's may stand at
        /// aterm through its re-exec grace (D28), unreleased; 0 for none. A
        /// refused claim attempt or the link's release gives it back before.
        graced_until_ms: u64,
    },
}

/// aterm's word on ending the child now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The policy and status pair is a safe point.
    Safe,
    /// It is not; the journal line (`wait aterm <field>=<value>`).
    Hold(String),
    /// aterm did not answer or no claim stands: the harness's own readings
    /// decide this look.
    Local,
}

/// One fresh policy and status pair ([`Reading::Fresh`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The policy, which admits the claim (a contract the harness declares
    /// served, and in effect).
    pub policy: Policy,
    /// The session's status, which shows the claim as this harness's, proven.
    pub status: Status,
}

/// aterm's facts now, for a harness that judges a move by its own rule
/// ([`Link::read`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading {
    /// One fresh policy and status pair, read on a standing claim that `status`
    /// shows as this harness's and proven. Every other fact is the harness's
    /// to judge: a halt (`hold=`), another driver's hand (`hand=`), the person
    /// (`human_ms=`), and `upgrade_ask=`, under which a person says when.
    Fresh(Box<Facts>),
    /// aterm stated a hold in place of a pair (`wait aterm <field>=<value>`):
    /// a reply it could give only later, a policy off for now, a claim that is
    /// no longer this harness's, or, on a link just lost, the hold aterm
    /// stated last ([`Link::lost_hold`]). Nothing moves on it.
    Hold(String),
    /// No standing claim, or aterm did not answer and stated no hold before:
    /// the harness's own readings decide.
    Local,
}

/// What one drain of the event stream found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drained {
    /// Something was read, so the harness's next look is due now.
    pub drained: bool,
    /// The last restart aterm asked the claim holder to make, if one came: an
    /// `EVENT restart`, or, while a stream is open, a `restart` record. It is a
    /// request, never permission: [`Link::relaunch_word`] with `asked` still
    /// decides it.
    pub restart: Option<Restart>,
    /// aterm's records for the caller that came on the stream, in order, each
    /// with its `in`: `want`, and the kinds its last `session` record takes.
    /// The caller answers each once ([`Link::answer`]); past
    /// [`stream::AWAITED_MAX`] left unanswered the oldest is refused
    /// `unanswered`.
    pub incoming: Vec<Received>,
    /// The newest `hand` record heard, already answered `took` and kept in
    /// [`Link::status`].
    pub hand: Option<Hand>,
    /// The `in` of the newest `policy` record heard, already answered `took`
    /// and followed by a fresh read of the grant.
    pub policy: Option<u64>,
    /// A stream opened on which aterm holds nothing the caller sent before:
    /// the first, one of a new aterm epoch, or one whose aterm forgot this
    /// claimant. The caller sends its `session` record, and its current
    /// `state`, again.
    pub fresh: bool,
    /// The caller's records numbered again as a stream opened, `(old, new)`:
    /// only when aterm's count of this claimant's records went back (an aterm
    /// restart, or a re-exec grace that ended). An `answer` or `touch` that
    /// names an `ask` names it by its new number.
    pub renumbered: Vec<(u64, u64)>,
}

/// A harness's link to aterm over the session's life.
#[derive(Debug)]
pub struct Link {
    config: Config,
    pid: u32,
    state: State,
    notes: Vec<String>,
    /// The stream's numbers and the records kept for it, which outlive a
    /// stream that closes.
    outbox: Outbox,
    /// What the stream brought for the caller since [`Link::events`] last
    /// returned it.
    inbox: Drained,
    /// Whether a `beat` goes every [`BEAT_EVERY_MS`] on an open stream
    /// ([`Link::set_beating`]).
    beating: bool,
    /// Why the stream is not asked for again in this link's life: an `OK`
    /// that did not read.
    stream_off: Option<String>,
}

impl Link {
    /// No link, for the rest of the session.
    #[must_use]
    pub fn absent(config: Config, pid: u32) -> Self {
        Self {
            config,
            pid,
            state: State::Absent,
            notes: Vec::new(),
            outbox: Outbox::default(),
            inbox: Drained::default(),
            beating: true,
            stream_off: None,
        }
    }

    /// The launch's link for `config`'s harness process `pid` (normally
    /// `std::process::id()`), decided before the first start of `child`,
    /// reading the process environment by `config`'s [`crate::EnvNames`].
    #[must_use]
    pub fn launch(config: Config, pid: u32, child: Child<'_>, now_ms: u64) -> Self {
        Self::launch_with(config, pid, child, now_ms, |name| std::env::var_os(name))
    }

    /// [`Link::launch`] reading the environment through `lookup`. Outside
    /// aterm nothing is attempted and nothing is noted. Inside, a session that
    /// cannot be located is noted and the link is absent; one that can is
    /// [`Link::launch_at`].
    #[must_use]
    pub fn launch_with(
        config: Config,
        pid: u32,
        child: Child<'_>,
        now_ms: u64,
        lookup: impl Fn(&str) -> Option<OsString>,
    ) -> Self {
        if !config.env().hosted_by(&lookup) {
            return Self::absent(config, pid);
        }
        match Locator::from_lookup(config.env(), &lookup) {
            Ok(locator) => Self::launch_at(config, pid, &locator, child, now_ms),
            Err(error) => {
                let mut link = Self::absent(config, pid);
                link.notes.push(format!("aterm unclaimed locator {error}"));
                link
            }
        }
    }

    /// The launch's link to the session `locator` names: the claim sequence,
    /// opening each connection within [`CONNECT_TIMEOUT`]. A proven claim
    /// notes `aterm claimed …` and holds the link. Anything short of one is
    /// noted as `aterm unclaimed <why>` and leaves the link absent, except an
    /// answer that stands for now only ([`Failure::Later`]), an aterm that
    /// serves no contract this harness can claim at ([`Failure::Unserved`]) or
    /// a policy switched off by a setting that can be switched on again
    /// ([`Failure::Off`]): that launch is lost with no claim made, and the
    /// sequence is tried again on the clock as for a lost link.
    #[must_use]
    pub fn launch_at(
        config: Config,
        pid: u32,
        locator: &Locator,
        child: Child<'_>,
        now_ms: u64,
    ) -> Self {
        Self::launch_graced(config, pid, locator, child, now_ms, false)
    }

    /// [`Link::launch_at`], or, when `adopted`, the claim of an image that took
    /// over a running child from an earlier image of itself, whose
    /// contract-4 claim may stand through aterm's re-exec grace
    /// ([`Link::adopt_with`]): any claim aterm could not make keeps that link
    /// lost and claimed, and graced until [`REEXEC_GRACE_MS`] has passed
    /// unless aterm's answer gave the claim back ([`Failure::gives_back`]).
    fn launch_graced(
        config: Config,
        pid: u32,
        locator: &Locator,
        child: Child<'_>,
        now_ms: u64,
        adopted: bool,
    ) -> Self {
        let mut link = Self::absent(config, pid);
        match claim(&link.config, locator, pid, child, CONNECT_TIMEOUT, adopted) {
            Ok(claimed) => {
                link.notes
                    .push(claimed_line(&claimed.grant, &claimed.holder, child));
                link.state = State::Held(Box::new(claimed));
                link.stream_if_due(child, CONNECT_TIMEOUT, now_ms);
            }
            Err(failure) => {
                link.notes
                    .push(format!("aterm unclaimed {}", failure.why()));
                let again = matches!(
                    failure,
                    Failure::Later(_) | Failure::Off(_) | Failure::Unserved(_)
                );
                // An adopted image's claim attempt was made under the grace:
                // an answer that gave the earlier image's claim back leaves
                // none to give back again.
                let graced = adopted && !failure.gives_back();
                if again || adopted {
                    link.state = State::Lost {
                        locator: locator.clone(),
                        retry_at_ms: now_ms.saturating_add(RECLAIM_EVERY_MS),
                        unclaimed: Some(failure),
                        claimed: adopted,
                        held: None,
                        graced_until_ms: if graced {
                            now_ms.saturating_add(REEXEC_GRACE_MS)
                        } else {
                            0
                        },
                    };
                }
            }
        }
        link
    }

    /// The link of a harness image that took over a running child from an
    /// earlier image of itself (a harness that replaced its own executable
    /// with `exec` and kept its child), reading the process environment by
    /// `config`'s [`crate::EnvNames`]: [`Link::adopt_with`].
    #[must_use]
    pub fn adopt(config: Config, pid: u32, child: Child<'_>, now_ms: u64) -> Self {
        Self::adopt_with(config, pid, child, now_ms, |name| std::env::var_os(name))
    }

    /// [`Link::launch_with`] for a harness image that took over a running
    /// child from an earlier image of itself. The earlier image's connections
    /// closed at the `exec` (they are close-on-exec): a contract-3 claim ended
    /// with them, and a contract-4 claim aterm keeps through its re-exec grace
    /// (D28), keyed on the pid and birth an `exec` keeps, so this image's claim
    /// renews it. Either way the duty towards the child did not end: the child
    /// runs, and nothing else supervises it. So the link [`Link::claims`] from the start, and a
    /// claim aterm could not make now, for any reason, keeps the link lost
    /// and is made again every [`RECLAIM_EVERY_MS`] instead of leaving it
    /// absent: a launch can still pass its vendor through, but an adopted
    /// child can only be supervised. A refusal is noted as at launch. Outside
    /// aterm, or with no session to locate, the link is absent as
    /// [`Link::launch_with`] makes it.
    #[must_use]
    pub fn adopt_with(
        config: Config,
        pid: u32,
        child: Child<'_>,
        now_ms: u64,
        lookup: impl Fn(&str) -> Option<OsString>,
    ) -> Self {
        let Some(locator) = config
            .env()
            .hosted_by(&lookup)
            .then(|| Locator::from_lookup(config.env(), &lookup).ok())
            .flatten()
        else {
            return Self::launch_with(config, pid, child, now_ms, lookup);
        };
        Self::launch_graced(config, pid, &locator, child, now_ms, true)
    }

    /// The config the link claims with.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The lines the link would journal since the last call, oldest first: the
    /// harness writes them to its own journal.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// The event stream's descriptor, for the harness's wait. Readable means
    /// [`Link::events`] has something to drain.
    #[must_use]
    pub fn event_fd(&self) -> Option<RawFd> {
        match &self.state {
            State::Held(claimed) => claimed.events.as_ref().map(|events| events.as_raw_fd()),
            State::Absent | State::Lost { .. } => None,
        }
    }

    /// Whether aterm's word replaces the harness's own input readings: the link
    /// is held and the claim stands.
    #[must_use]
    pub fn judges(&self) -> bool {
        matches!(&self.state, State::Held(claimed) if claimed.standing)
    }

    /// What aterm granted the standing claim: the duties the harness performs
    /// now, and the claim's revision. `None` while no claim stands, when the
    /// harness performs none of them.
    #[must_use]
    pub fn grant(&self) -> Option<&Grant> {
        match &self.state {
            State::Held(claimed) if claimed.standing => Some(&claimed.grant),
            State::Held(_) | State::Absent | State::Lost { .. } => None,
        }
    }

    /// Whether the standing claim was granted `scope`: the harness performs
    /// that duty now. A duty it asked for and was not granted is aterm's
    /// built-in engine's.
    #[must_use]
    pub fn takes(&self, scope: Scope) -> bool {
        self.grant().is_some_and(|grant| grant.takes(scope))
    }

    /// Whether an aterm hold still stands at `now_ms` without a fresh read.
    #[must_use]
    pub fn held_back(&self, now_ms: u64) -> bool {
        matches!(&self.state, State::Held(claimed) if claimed.standing && now_ms < claimed.recheck_at_ms)
    }

    /// Whether this session ever had a claim: the link is held or is being
    /// claimed again. Without one the harness has no duty towards a crashed
    /// child, and its end is the session's own.
    #[must_use]
    pub const fn claims(&self) -> bool {
        matches!(
            self.state,
            State::Held(_) | State::Lost { claimed: true, .. }
        )
    }

    /// Whether the link is lost and waiting for its next claim attempt.
    #[must_use]
    pub const fn is_lost(&self) -> bool {
        matches!(self.state, State::Lost { .. })
    }

    /// Whether the link is absent for the rest of the session.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self.state, State::Absent)
    }

    /// Why no claim stands on a lost link: aterm's answer to its last claim
    /// attempt, at launch, adoption or on the reclaim clock (noted once as
    /// `aterm unclaimed <why>`). `None` while the link is held or absent, and
    /// on a link lost since its last attempt.
    ///
    /// A launch aterm answered [`Failure::Unserved`] (its policy lists none of
    /// the contracts the config declares, or none that names the harness's
    /// duties) is lost and unclaimed, as one answered [`Failure::Later`] or
    /// [`Failure::Off`] is, and is claimed on the clock once aterm serves one
    /// of them. A caller that would rather run its vendor in its own place
    /// than wait for an aterm update reads it here and does so: nothing was
    /// claimed, so there is nothing to give back.
    #[must_use]
    pub const fn unclaimed(&self) -> Option<&Failure> {
        match &self.state {
            State::Lost { unclaimed, .. } => unclaimed.as_ref(),
            State::Absent | State::Held(_) => None,
        }
    }

    /// The wait a lost link keeps because aterm's last safe-point answer was a
    /// hold: `wait link-lost <that hold>`.
    #[must_use]
    pub fn lost_hold(&self) -> Option<String> {
        match &self.state {
            State::Lost {
                held: Some(hold), ..
            } => Some(format!(
                "wait link-lost {}",
                hold.strip_prefix("wait ").unwrap_or(hold)
            )),
            _ => None,
        }
    }

    /// Give a standing claim back (one bounded exchange, its result ignored)
    /// and close the link: the harness is about to exit, and an explicit
    /// release is what tells aterm the exit was not a crash (the plan's D23).
    ///
    /// While a stream is open the release goes on the second connection, and
    /// aterm closes the stream; the records kept for it are given up. A
    /// contract-4 claim aterm may keep through its re-exec grace (D28) is given
    /// back on a connection of its own: when the held connection did not take
    /// the release, and when the link is lost inside the grace.
    pub fn release(&mut self, now_ms: u64) {
        match std::mem::replace(&mut self.state, State::Absent) {
            State::Held(mut claimed) => {
                let four = claimed.grant.contract == Contract::Four;
                if (claimed.standing || claimed.unreleased)
                    && claimed.release(&self.config, now_ms).is_err()
                    && four
                {
                    let locator = claimed.locator.clone();
                    drop(claimed);
                    release_anew(&self.config, &locator, self.pid);
                }
            }
            State::Lost {
                locator,
                graced_until_ms,
                ..
            } if now_ms < graced_until_ms => release_anew(&self.config, &locator, self.pid),
            State::Lost { .. } | State::Absent => {}
        }
        self.outbox.clear();
    }

    /// The held link stopped answering (or its claim is no longer this
    /// harness's): drop every connection and claim again on the reclaim clock.
    /// aterm drops a contract-3 claim with its connection; a contract-4 claim
    /// that stood, or whose release went untaken, it keeps through its re-exec
    /// grace (D28), which the next claim renews, so the lost link remembers it
    /// until the grace ends.
    fn lose(&mut self, why: &str, now_ms: u64) {
        let State::Held(claimed) = std::mem::replace(&mut self.state, State::Absent) else {
            return;
        };
        self.notes.push(format!("aterm lost {why}"));
        let graced =
            (claimed.standing || claimed.unreleased) && claimed.grant.contract == Contract::Four;
        self.state = State::Lost {
            locator: claimed.locator.clone(),
            retry_at_ms: now_ms.saturating_add(RECLAIM_EVERY_MS),
            unclaimed: None,
            claimed: true,
            // Only a standing claim's answer holds: a released claim's looks
            // were already the harness's own.
            held: claimed.last_hold.clone().filter(|_| claimed.standing),
            graced_until_ms: if graced {
                now_ms.saturating_add(REEXEC_GRACE_MS)
            } else {
                0
            },
        };
    }

    /// The link's own clock, run on every pass of the harness's loop: claim a
    /// lost link again for `child`, reopen an ended event stream, and read the
    /// policy again when it is due. An ended stream is reopened every
    /// [`RECLAIM_EVERY_MS`], and the policy is read at each attempt (the
    /// stream that would announce a change is down) and once more when it is
    /// back. Each is bounded by [`REQUEST_TIMEOUT`] per step and stops at the
    /// first step that fails.
    ///
    /// It also reads and answers what the stream brought (as [`Link::events`]
    /// does, which returns it), sends a due `beat`, and asks for the stream
    /// when a standing contract-4 claim has none and it is due. While a stream
    /// is open, an event subscription that is down is reopened on the same
    /// clock, but no policy read is owed for it: the stream carries `policy`.
    pub fn tick(&mut self, child: Child<'_>, now_ms: u64) {
        self.pump(child, now_ms);
        match &mut self.state {
            State::Absent => {}
            State::Lost {
                locator,
                retry_at_ms,
                unclaimed,
                claimed,
                held,
                graced_until_ms,
            } => {
                if now_ms < *retry_at_ms {
                    return;
                }
                let graced = now_ms < *graced_until_ms;
                match claim(
                    &self.config,
                    locator,
                    self.pid,
                    child,
                    REQUEST_TIMEOUT,
                    graced,
                ) {
                    Ok(made) => {
                        self.notes
                            .push(claimed_line(&made.grant, &made.holder, child));
                        self.state = State::Held(Box::new(made));
                        self.stream_if_due(child, REQUEST_TIMEOUT, now_ms);
                    }
                    Err(failure) => {
                        *retry_at_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
                        // A claim aterm will not take now was given back with
                        // the refusal (`claim`): nothing of it stands.
                        if failure.gives_back() {
                            *graced_until_ms = 0;
                        }
                        if unclaimed.as_ref().map(Failure::why) != Some(failure.why()) {
                            self.notes
                                .push(format!("aterm unclaimed {}", failure.why()));
                        }
                        // aterm answered, and not with an answer for now
                        // only: its earlier hold is no longer its last word,
                        // and the harness's own readings decide. An aterm
                        // that serves no contract this harness can claim at
                        // states no hold on its session.
                        if matches!(
                            failure,
                            Failure::Refused(_) | Failure::Off(_) | Failure::Unserved(_)
                        ) {
                            *held = None;
                        }
                        // A launch that never claimed and is now refused for
                        // good is what the launch would have made it.
                        let gone = !*claimed && matches!(failure, Failure::Refused(_));
                        *unclaimed = Some(failure);
                        if gone {
                            self.state = State::Absent;
                        }
                    }
                }
            }
            State::Held(claimed) => {
                let mut policy_due = claimed.policy_due_ms != 0 && now_ms >= claimed.policy_due_ms;
                // The session stream carries `policy`: a subscription that is
                // down hides nothing from a link that streams.
                let unheard = claimed.stream.is_none();
                if claimed.events.is_none() && now_ms >= claimed.events_retry_ms {
                    let reopened = claimed.locator.socket().and_then(|socket| {
                        EventReader::connect(&socket, claimed.locator.sid(), REQUEST_TIMEOUT)
                    });
                    match reopened {
                        Ok(events) => {
                            claimed.events = Some(events);
                            self.notes.push("aterm events-reopened".to_string());
                            // What the stream said while it was down was not heard.
                            policy_due |= unheard;
                        }
                        Err(_) => {
                            claimed.events_retry_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
                            // No event can say the policy moved while the
                            // stream is down, so it is read on the reopen
                            // clock until the stream is back (Decision 18).
                            policy_due |= unheard;
                        }
                    }
                }
                if policy_due {
                    self.follow_policy(child, now_ms);
                }
            }
        }
        self.beat(now_ms);
        self.stream_if_due(child, REQUEST_TIMEOUT, now_ms);
        self.flush(now_ms);
        self.settle();
    }

    /// The event descriptor is readable: drain it without waiting. A policy
    /// event (or any loss, which may hide one) is answered by a fresh policy
    /// read, made for `child`; a restart request is returned for the harness to
    /// decide. A harness whose child is about to be replaced anyway ignores
    /// it: aterm sent it for that child.
    ///
    /// The stream is read first ([`Link::stream_fd`] readable, or
    /// [`Link::due_ms`] reached, means it has something), and what it brought
    /// since the last call is returned with the drain: its `restart`, its
    /// `hand` and `policy`, the records for the caller ([`Drained::incoming`])
    /// and whether a fresh stream opened. Once the open stream has brought one
    /// of aterm's own records, `EVENT restart`, `EVENT harness-policy` and the
    /// subscription's losses are ignored.
    pub fn events(&mut self, child: Child<'_>, now_ms: u64) -> Drained {
        self.pump(child, now_ms);
        self.settle();
        self.drain_events(child, now_ms);
        std::mem::take(&mut self.inbox)
    }

    /// Drain the subscription into the inbox, without waiting: its restart
    /// request, and a policy read for a policy event or a loss, unless the open
    /// stream carries both.
    fn drain_events(&mut self, child: Child<'_>, now_ms: u64) {
        let State::Held(claimed) = &mut self.state else {
            return;
        };
        let streaming = claimed.stream.is_some();
        let carried = claimed.stream.as_ref().is_some_and(|open| open.heard);
        let Some(events) = claimed.events.as_mut() else {
            return;
        };
        let update = events.drain();
        // Without a stream every drain may have moved what a safe point judges, as in v0.1.
        // While one is open this is also called for the stream's records, which an
        // acknowledgement brings without moving anything: only an event that can end a hold
        // has `status` read again.
        let heard = update.activity
            || (!carried && (update.restart.is_some() || update.requires_refresh()));
        if !streaming || heard {
            claimed.recheck_at_ms = 0;
        }
        if update.ended {
            claimed.events = None;
            claimed.events_retry_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
            self.notes.push(format!(
                "aterm events-lost {}",
                update
                    .fault
                    .as_ref()
                    .map_or_else(|| "closed".to_string(), ToString::to_string)
            ));
        }
        self.inbox.drained = true;
        if carried {
            return;
        }
        let refresh = update.requires_refresh();
        if let Some(restart) = update.restart {
            self.inbox.restart = Some(restart);
        }
        if refresh {
            self.follow_policy(child, now_ms);
            self.settle();
        }
    }

    /// Make the claim follow a fresh policy read ([`Claimed::follow_policy`]).
    fn follow_policy(&mut self, child: Child<'_>, now_ms: u64) {
        let State::Held(claimed) = &mut self.state else {
            return;
        };
        match claimed.follow_policy(&self.config, child, now_ms) {
            Ok(Some(line)) => self.notes.push(line),
            Ok(None) => {}
            Err(failure) => self.lose(failure.why(), now_ms),
        }
        self.settle();
    }

    /// aterm's word on ending the child now: one fresh policy read and one
    /// fresh status read, judged as an upgrade safe point with the harness's
    /// own `grace` (which can lengthen aterm's and never shorten it). A link
    /// that fails here is lost, and the caller decides this look on its own
    /// readings, unless aterm's last word on a held link was a hold.
    pub fn safe_point(&mut self, grace: Duration, now_ms: u64) -> Verdict {
        let State::Held(claimed) = &mut self.state else {
            return Verdict::Local;
        };
        if !claimed.standing {
            return Verdict::Local;
        }
        match claimed.judge(&self.config, grace, now_ms) {
            Ok(Judged::Safe) => Verdict::Safe,
            Ok(Judged::Hold(why)) => Verdict::Hold(why),
            Ok(Judged::Taken(why)) => {
                self.lose(&format!("claim {why}"), now_ms);
                Verdict::Hold(why)
            }
            Ok(Judged::Released(line)) => {
                self.notes.push(line);
                self.settle();
                Verdict::Local
            }
            Err(failure) => {
                // aterm's last word stands over one that never came: after a
                // hold the look holds, and only after a safe answer, or none,
                // do the harness's own readings decide.
                self.lose(failure.why(), now_ms);
                self.lost_hold().map_or(Verdict::Local, Verdict::Hold)
            }
        }
    }

    /// aterm's facts on a move the harness judges by its own rule: one fresh
    /// policy read and one fresh status read on the held connection, under the
    /// rules [`Link::safe_point`] keeps. A policy no longer admitting the claim
    /// releases it, and the harness's own readings decide ([`Reading::Local`]).
    /// A reply aterm cannot give now, a policy off for now, or a claim that is
    /// no longer this harness's (which loses the link) is a
    /// [`Reading::Hold`]. A link that fails here is lost, and the read is
    /// [`Reading::Local`] unless aterm's last word on it was a hold.
    ///
    /// It is for a move [`claim::judge_safe_point`] would hold that the
    /// harness has its own warrant for, such as one a person asked for under
    /// `upgrade_ask=true`: the harness still checks the halt, the hand and the
    /// claim on the pair it gets. A [`Reading::Fresh`] pair clears the hold a
    /// lost link would keep: aterm's last word was facts, not a hold.
    pub fn read(&mut self, now_ms: u64) -> Reading {
        let State::Held(claimed) = &mut self.state else {
            return Reading::Local;
        };
        if !claimed.standing {
            return Reading::Local;
        }
        match claimed.read_pair(&self.config, now_ms) {
            Ok(Pair::Fresh(facts)) => {
                claimed.recheck_at_ms = 0;
                claimed.last_hold = None;
                Reading::Fresh(facts)
            }
            Ok(Pair::Hold(why)) => {
                claimed.last_hold = Some(why.clone());
                Reading::Hold(why)
            }
            Ok(Pair::Taken(why)) => {
                claimed.last_hold = None;
                self.lose(&format!("claim {why}"), now_ms);
                Reading::Hold(why)
            }
            Ok(Pair::Released(line)) => {
                claimed.last_hold = None;
                self.notes.push(line);
                self.settle();
                Reading::Local
            }
            Err(failure) => {
                self.lose(failure.why(), now_ms);
                self.lost_hold().map_or(Reading::Local, Reading::Hold)
            }
        }
    }

    /// aterm's word on starting the child again: one fresh policy read and one
    /// fresh status read, for a crash (`asked` false) or a restart aterm asked
    /// for (`asked` true). With no link, or a claim given back because the
    /// policy went off, nothing is asked and nothing is permitted. A link that
    /// fails here is lost, and the caller may wait for it to be claimed again
    /// ([`relaunch::AwayBudget`]). A `status` that no longer shows the claim as
    /// this harness's loses the link too, and permits nothing: aterm answered,
    /// and the duty is not this harness's.
    pub fn relaunch_word(&mut self, asked: bool, now_ms: u64) -> Relaunch {
        let claimed = match &mut self.state {
            State::Held(claimed) if claimed.standing => claimed,
            State::Held(_) | State::Absent => return Relaunch::Denied("no-claim".to_string()),
            State::Lost { .. } => return Relaunch::Away("link-lost".to_string()),
        };
        match claimed.relaunch_word(asked) {
            Ok(Leave::Granted) => Relaunch::Permitted,
            Ok(Leave::Denied(why)) => Relaunch::Denied(why),
            Ok(Leave::Later(why)) => Relaunch::Later(why),
            Ok(Leave::Taken(why)) => {
                self.lose(&why, now_ms);
                Relaunch::Denied(why)
            }
            Err(failure) => {
                self.lose(failure.why(), now_ms);
                Relaunch::Away(failure.why().to_string())
            }
        }
    }

    /// Send `body` on the session stream: number it, keep it until aterm
    /// acknowledges it, and write it when the stream is open and the pace
    /// allows. `ms` is the caller's wall clock (milliseconds since 1970, only
    /// shown), from which the link's own records are stamped too; `now_ms` is
    /// the link's clock. The record's `seq`, which stays the record's until
    /// aterm acknowledges it, unless a stream opens whose aterm's count went
    /// back ([`Drained::renumbered`]).
    ///
    /// A record sent while the link is lost after a stream is kept and written
    /// when the stream opens again. A stream that cannot take the line is
    /// lost, and the record is resent. A `session` record names the kinds of
    /// aterm's records the caller takes ([`Drained::incoming`]).
    ///
    /// # Errors
    /// No stream is open or to be opened again ([`SendError::NoStream`]: the
    /// harness keeps its v0.1 path), the resend queue is full
    /// ([`SendError::Full`]), the kind is not the caller's to send, or the
    /// record cannot be written; nothing is numbered.
    pub fn send(&mut self, body: Body, ms: u64, now_ms: u64) -> Result<u64, SendError> {
        stream::sendable(&body)?;
        if !self.outbox.live() {
            return Err(SendError::NoStream);
        }
        self.put(body, false, ms, now_ms)
    }

    /// Answer aterm's record `of`, one the link gave the caller in
    /// [`Drained::incoming`]: `took` when `refused` is `None`, else `refused`
    /// with that why. Each record is answered once.
    ///
    /// # Errors
    /// No record at `of` awaits an answer ([`AnswerError::NotAwaited`]), or the
    /// why cannot be written ([`AnswerError::Line`]), when the record still
    /// awaits one.
    pub fn answer(
        &mut self,
        of: u64,
        refused: Option<&str>,
        now_ms: u64,
    ) -> Result<(), AnswerError> {
        if !self.outbox.awaits(of) {
            return Err(AnswerError::NotAwaited(of));
        }
        let body = match refused {
            None => Body::Took { of },
            Some(why) => Body::Refused {
                of,
                why: why.to_string(),
            },
        };
        let ms = self.outbox.stamp(now_ms);
        match self.put(body, true, ms, now_ms) {
            Ok(_) => {}
            Err(SendError::Line(error)) => return Err(AnswerError::Line(error)),
            // The stream is lost, and the record's answer with it.
            Err(_) => {}
        }
        self.outbox.answering(of);
        self.outbox.answered(refused.is_none());
        Ok(())
    }

    /// The caller's wall clock `ms` (milliseconds since 1970) at `now_ms`, from
    /// which the link's own records are stamped until the caller sends one of
    /// its own. Without it, those before the caller's first record read 0.
    pub fn set_wall(&mut self, ms: u64, now_ms: u64) {
        self.outbox.set_wall(ms, now_ms);
    }

    /// Whether a `beat` goes every [`BEAT_EVERY_MS`] on an open stream (from
    /// the start, `true`). With `false` no beat goes while the stream and the
    /// claim stay, as a harness that stopped beating looks to aterm's watchdog;
    /// with `true` again the next beat goes at the next [`Link::tick`].
    pub fn set_beating(&mut self, beating: bool, now_ms: u64) {
        self.beating = beating;
        if let State::Held(claimed) = &mut self.state
            && let Some(open) = claimed.stream.as_mut()
            && beating
        {
            open.beat_due_ms = open.beat_due_ms.min(now_ms);
        }
    }

    /// Send one request, `@<sid> <verb>`, on the held request connection (the
    /// second connection while a stream is open, the claim's otherwise) and
    /// return its reply line: the acts for kinds the stream does not carry
    /// (`meta set harness_*`, the plan's D24), or a read of the harness's own.
    /// A claim, a release, `stream` and `subscribe` are the link's to send and
    /// are refused: `meta set supervisor` on the second connection would bind
    /// the claim to it and close the stream.
    ///
    /// # Errors
    /// The verb is one of those, holds a control character, or names no
    /// session-addressed request ([`Failure::Refused`], nothing sent); no link
    /// is held ([`Failure::Refused`]); or the exchange failed, which loses the
    /// link ([`Failure::Gone`]).
    pub fn request(&mut self, verb: &str, now_ms: u64) -> Result<String, Failure> {
        let words: Vec<&str> = verb.split_ascii_whitespace().collect();
        let linked = matches!(
            words.as_slice(),
            ["meta", "set" | "unset", "supervisor", ..] | ["stream" | "subscribe", ..]
        );
        if linked
            || words.is_empty()
            || words[0].starts_with('@')
            || verb.chars().any(char::is_control)
        {
            return Err(Failure::Refused(format!(
                "request {} unsendable",
                claim::journal_word(words.first().copied().unwrap_or("-"))
            )));
        }
        let State::Held(claimed) = &mut self.state else {
            return Err(Failure::Refused("request no-link".to_string()));
        };
        let request = Some(format!("@{} {}", claimed.locator.sid(), words.join(" ")));
        match ask(&mut claimed.conn, "request", request) {
            Ok(reply) => Ok(reply),
            Err(failure) => {
                self.lose(failure.why(), now_ms);
                self.settle();
                Err(failure)
            }
        }
    }

    /// Whether the session stream is open.
    #[must_use]
    pub fn streaming(&self) -> bool {
        matches!(&self.state, State::Held(claimed) if claimed.stream.is_some())
    }

    /// The stream's descriptor, for the harness's wait beside
    /// [`Link::event_fd`]. Readable means [`Link::events`] has records to read;
    /// so does [`Link::due_ms`] at 0, for lines a write read while it waited.
    #[must_use]
    pub fn stream_fd(&self) -> Option<RawFd> {
        match &self.state {
            State::Held(claimed) => claimed.stream.as_ref().map(|open| open.lines.fd()),
            State::Absent | State::Lost { .. } => None,
        }
    }

    /// When (ms of `now_ms`) the link next has something to do without a
    /// descriptor waking the harness: a beat, a record waiting for the pace,
    /// the stream asked for again, a lost link claimed again, an ended
    /// subscription reopened, a policy read owed; 0 for at once (lines already
    /// read wait). The harness bounds its wait on [`Link::event_fd`] and
    /// [`Link::stream_fd`] by it and then calls [`Link::tick`] and
    /// [`Link::events`]. `None` when nothing is due on the clock.
    #[must_use]
    pub fn due_ms(&self) -> Option<u64> {
        let claimed = match &self.state {
            State::Absent => return None,
            State::Lost { retry_at_ms, .. } => return Some(*retry_at_ms),
            State::Held(claimed) => claimed,
        };
        let mut due = Vec::new();
        if claimed.events.is_none() {
            due.push(claimed.events_retry_ms);
        }
        if claimed.policy_due_ms != 0 {
            due.push(claimed.policy_due_ms);
        }
        match &claimed.stream {
            Some(open) => {
                if open.lines.pending_line() {
                    return Some(0);
                }
                if self.beating {
                    due.push(open.beat_due_ms);
                }
                due.extend(self.outbox.pace_due_ms());
            }
            None => {
                if let Ask::At(at) = claimed.stream_ask
                    && self.asks_stream(claimed)
                {
                    due.push(at);
                }
            }
        }
        due.into_iter().min()
    }

    /// The epoch of the stream last opened, while its records are kept: the
    /// stream is open or is to be opened again.
    #[must_use]
    pub fn epoch(&self) -> Option<&str> {
        self.outbox.live().then(|| self.outbox.epoch()).flatten()
    }

    /// What the link's streams carried, over its life.
    #[must_use]
    pub const fn counts(&self) -> Counts {
        self.outbox.counts()
    }

    /// The last `status` aterm gave on the held link (at the claim, a grant
    /// read, a safe-point or relaunch read), with the hand fields (`human_seq=`,
    /// `composer=`, `hold=`) that the stream's `hand` records moved since.
    #[must_use]
    pub fn status(&self) -> Option<&Status> {
        match &self.state {
            State::Held(claimed) => claimed.status.as_ref(),
            State::Absent | State::Lost { .. } => None,
        }
    }

    /// Whether the stream is asked for on `claimed`: the config streams, no
    /// `OK` failed to read in this link's life, and a standing contract-4
    /// claim has none open.
    fn asks_stream(&self, claimed: &Claimed) -> bool {
        self.config.streams()
            && self.stream_off.is_none()
            && claimed.stream.is_none()
            && claimed.standing
            && claimed.grant.contract == Contract::Four
    }

    /// Ask for the session stream when it is due. The subscription is drained
    /// first, so a restart or policy event aterm sent before the stream opened
    /// is still heard. An `OK` opens the stream, writes again the records kept
    /// above its `acked=`, and notes `aterm streaming epoch=<e> acked=<n> in=<n>
    /// resent=<n>`. A reply that asks to be retried keeps the v0.1 path and the
    /// records kept, and asks again after [`RECLAIM_EVERY_MS`]; any other reply
    /// keeps the v0.1 path, gives up the records kept, and is not asked again
    /// on this connection. An `OK` that does not read loses the link and ends
    /// the stream for its life. A failure loses the link.
    fn stream_if_due(&mut self, child: Child<'_>, opening: Duration, now_ms: u64) {
        let due = |link: &Self| match &link.state {
            State::Held(claimed) => {
                link.asks_stream(claimed)
                    && matches!(claimed.stream_ask, Ask::At(at) if now_ms >= at)
            }
            State::Absent | State::Lost { .. } => false,
        };
        if !due(self) {
            return;
        }
        self.drain_events(child, now_ms);
        if !due(self) {
            return;
        }
        let State::Held(claimed) = &mut self.state else {
            return;
        };
        let reply = match claimed.open_stream(opening, now_ms) {
            Ok(Asked::Open(reply)) => reply,
            Ok(Asked::Busy) => {
                claimed.stream_ask = Ask::At(now_ms.saturating_add(RECLAIM_EVERY_MS));
                return;
            }
            Ok(Asked::Refused) => {
                claimed.stream_ask = Ask::Never;
                self.give_up();
                return;
            }
            Ok(Asked::Unreadable(why)) => {
                let why = format!("stream {}", claim::journal_word(&why));
                self.notes.push(format!("aterm stream-off {why}"));
                self.stream_off = Some(why.clone());
                self.lose(&why, now_ms);
                return;
            }
            Err(failure) => {
                self.lose(failure.why(), now_ms);
                return;
            }
        };
        let opened = self.outbox.open(&reply, now_ms);
        self.notes.push(format!(
            "aterm streaming epoch={} acked={} in={} resent={}",
            claim::journal_word(&reply.epoch),
            reply.acked,
            reply.sent,
            opened.resent
        ));
        self.inbox.fresh |= opened.fresh;
        self.inbox.renumbered.extend(opened.renumbered);
        self.flush(now_ms);
    }

    /// Give up the records kept, noting `aterm stream-given-up records=<n>`
    /// when any of the caller's were among them.
    fn give_up(&mut self) {
        let given_up = self.outbox.clear();
        if given_up > 0 {
            self.notes
                .push(format!("aterm stream-given-up records={given_up}"));
        }
    }

    /// Number `body`, keep it and write what the pace allows if the stream is
    /// open: the caller's record, or the link's own (`own`). An own record past
    /// [`stream::OWN_MAX`] loses the stream, which aterm neither reads nor
    /// acknowledges.
    fn put(&mut self, body: Body, own: bool, ms: u64, now_ms: u64) -> Result<u64, SendError> {
        match self.outbox.push(body, ms, own, now_ms) {
            Ok(seq) => {
                self.flush(now_ms);
                Ok(seq)
            }
            Err(SendError::Full) if own => {
                if self.streaming() {
                    self.stream_lost(&format!("stream own-queue={}", stream::OWN_MAX), now_ms);
                }
                Err(SendError::Full)
            }
            Err(error) => Err(error),
        }
    }

    /// Write the kept records the pace allows on the open stream, if one is. A
    /// stream that cannot take a line within [`REQUEST_TIMEOUT`] is lost; what
    /// is kept is written again when it opens again.
    fn flush(&mut self, now_ms: u64) {
        if !self.streaming() {
            return;
        }
        for line in self.outbox.due(now_ms) {
            let State::Held(claimed) = &mut self.state else {
                return;
            };
            let Some(open) = claimed.stream.as_mut() else {
                return;
            };
            if let Err(error) = open.lines.write_line(&line, REQUEST_TIMEOUT) {
                self.stream_lost(&format!("stream write {error}"), now_ms);
                return;
            }
            self.outbox.wrote();
        }
    }

    /// The stream closed or failed: lose the link and claim again at once, or
    /// [`RECLAIM_EVERY_MS`] after the stream opened when it closed sooner, so
    /// an aterm that closes each stream is not asked faster than a lost link
    /// is. aterm keeps the claim through its re-exec grace; what is kept is
    /// resent.
    fn stream_lost(&mut self, why: &str, now_ms: u64) {
        let opened = match &self.state {
            State::Held(claimed) => claimed.stream.as_ref().map(|open| open.opened_ms),
            State::Absent | State::Lost { .. } => None,
        };
        self.lose(why, now_ms);
        if let (Some(opened), State::Lost { retry_at_ms, .. }) = (opened, &mut self.state) {
            *retry_at_ms = opened.saturating_add(RECLAIM_EVERY_MS).max(now_ms);
        }
    }

    /// Give up the records kept when no stream will resend them: the link is
    /// absent, the claim was given back, or it stands under contract 3. A link
    /// lost after a stream keeps them.
    fn settle(&mut self) {
        let keeps = match &self.state {
            State::Held(claimed) => claimed.standing && claimed.grant.contract == Contract::Four,
            State::Lost { claimed, .. } => *claimed,
            State::Absent => false,
        };
        if !keeps && self.outbox.live() {
            self.give_up();
        }
    }

    /// Send a `beat` when one is due: every [`BEAT_EVERY_MS`] of `now_ms`
    /// from the stream's open, unless beats are paused. A late tick sends one,
    /// not the ones it missed.
    fn beat(&mut self, now_ms: u64) {
        if !self.beating {
            return;
        }
        let State::Held(claimed) = &mut self.state else {
            return;
        };
        let Some(open) = claimed.stream.as_mut() else {
            return;
        };
        if now_ms < open.beat_due_ms {
            return;
        }
        open.beat_due_ms = now_ms.saturating_add(BEAT_EVERY_MS);
        let ms = self.outbox.stamp(now_ms);
        let _ = self.put(Body::Beat, true, ms, now_ms);
    }

    /// [`Link::reannounce`] on a stream: a `claim` record with the words the
    /// claim line would carry. aterm places it as a claim (the plan's D7) and
    /// sends `policy` when the grant moved, on which the grant is read again;
    /// an acknowledgement that names `dropped=` (other than `applied`, and
    /// `rate`, on which it is sent again) loses the link, as a refused claim
    /// line does.
    fn claim_record(&mut self, child: Child<'_>, asked: Vec<Scope>, now_ms: u64) {
        let body = Body::Claim {
            child: child.process,
            scope: Some(asked),
            version: Some(self.config.version().to_string()),
            vendor: claim::vendor_word(child.vendor).map(str::to_string),
            login: claim::login_word(child.login).map(str::to_string),
        };
        match self.put_claim(body, now_ms) {
            Ok(seq) => {
                self.notes.push(format!(
                    "aterm reannounced vendor={} child={} seq={seq}",
                    child
                        .vendor
                        .map_or_else(|| "-".to_string(), claim::journal_word),
                    child
                        .process
                        .and_then(claim::ChildProcess::word)
                        .unwrap_or_else(|| "-".to_string()),
                ));
            }
            Err(error) => self.lose(&format!("reannounce {error}"), now_ms),
        }
    }

    /// Keep and write a `claim` record, awaiting its acknowledgement.
    fn put_claim(&mut self, body: Body, now_ms: u64) -> Result<u64, SendError> {
        let ms = self.outbox.stamp(now_ms);
        let seq = self.put(body.clone(), true, ms, now_ms)?;
        if let State::Held(claimed) = &mut self.state
            && let Some(open) = claimed.stream.as_mut()
        {
            open.claim = Some((seq, body));
        }
        Ok(seq)
    }

    /// Read what the stream brought, without waiting, and answer each of
    /// aterm's records; a `policy` among them is followed once, after the
    /// rest. A stream that ended is lost once its lines are answered.
    fn pump(&mut self, child: Child<'_>, now_ms: u64) {
        let read = match &mut self.state {
            State::Held(claimed) => match claimed.stream.as_mut() {
                Some(open) => open.lines.read(),
                None => return,
            },
            State::Absent | State::Lost { .. } => return,
        };
        if !read.lines.is_empty() {
            self.inbox.drained = true;
        }
        let mut policy = false;
        for line in read.lines {
            if !self.streaming() {
                return;
            }
            policy |= self.hear(line, now_ms);
        }
        if let Some(why) = read.ended {
            if self.streaming() {
                self.stream_lost(&format!("stream {}", claim::journal_word(&why)), now_ms);
            }
            return;
        }
        self.flush(now_ms);
        if policy {
            self.follow_policy(child, now_ms);
        }
    }

    /// One line from aterm: act on it and answer it, or give it to the caller
    /// to answer. Whether it was a `policy` record.
    fn hear(&mut self, line: Line, now_ms: u64) -> bool {
        let (read, bytes) = match line {
            Line::Text(bytes) => (Record::read(&bytes), bytes),
            Line::Over { len, head } => (Err(LineError::Bound(len.max(LINE_MAX + 1))), head),
        };
        let record = match read {
            Ok(decoded) => {
                self.outbox.counts_mut().unknown_fields += u64::from(decoded.unknown);
                decoded.record
            }
            Err(error) => {
                self.unreadable(&bytes, &error, now_ms);
                return false;
            }
        };
        let seq = record.seq;
        let heard = stream::heard(record.body);
        if let Heard::Ack(dropped) = heard {
            self.acked(seq, dropped, now_ms);
            return false;
        }
        match self.outbox.hear(seq) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(()) => {
                self.stream_lost(&format!("stream in={seq} over-seq-max"), now_ms);
                return false;
            }
        }
        let relaunch = self.takes(Scope::Relaunch);
        let State::Held(claimed) = &mut self.state else {
            return false;
        };
        if let Some(open) = claimed.stream.as_mut() {
            open.heard = true;
        }
        let why = match heard {
            Heard::Ack(_) => return false,
            Heard::Hand {
                human_seq,
                composer,
                hold,
            } => {
                self.outbox.counts_mut().hands += 1;
                // A hand may end a hold: the next look reads aterm again.
                claimed.recheck_at_ms = 0;
                if let Some(status) = claimed.status.as_mut() {
                    status.hand(human_seq, composer.word(), hold);
                }
                self.inbox.hand = Some(Hand {
                    of: seq,
                    human_seq,
                    composer,
                    hold,
                });
                None
            }
            Heard::Policy => {
                self.outbox.counts_mut().policies += 1;
                claimed.recheck_at_ms = 0;
                self.inbox.policy = Some(seq);
                self.answer_own(seq, None, now_ms);
                return true;
            }
            Heard::Restart(restart) => {
                self.outbox.counts_mut().restarts += 1;
                claimed.recheck_at_ms = 0;
                if relaunch {
                    self.inbox.restart = Some(restart);
                    None
                } else {
                    Some("no relaunch grant")
                }
            }
            Heard::Incoming(incoming)
                if incoming.kind() == record::Kind::Want || self.outbox.takes(incoming.kind()) =>
            {
                self.inbox.incoming.push(Received { of: seq, incoming });
                if let Some(oldest) = self.outbox.await_answer(seq) {
                    self.inbox.incoming.retain(|received| received.of != oldest);
                    self.answer_own(oldest, Some("unanswered"), now_ms);
                }
                return false;
            }
            Heard::Incoming(_) => Some("not taken"),
            Heard::Unknown => {
                self.outbox.counts_mut().unknown_kinds += 1;
                None
            }
            Heard::Refuse(why) => Some(why),
        };
        self.answer_own(seq, why, now_ms);
        false
    }

    /// A line of aterm's that does not read as a record. Its `k` and `seq` are
    /// found loosely: an `ack` is not counted in aterm's numbers and is not
    /// answered; another with a new `seq` is refused with the reading's fault;
    /// one with no `seq` found is counted and noted, since aterm's numbers may
    /// have gaps and nothing can name it.
    fn unreadable(&mut self, bytes: &[u8], error: &LineError, now_ms: u64) {
        let (kind, seq) = stream::loose(bytes);
        let ack = kind.as_deref() == Some("ack");
        let refused = if ack {
            self.outbox.unreadable(None)
        } else {
            self.outbox.unreadable(seq)
        };
        let why = error.to_string();
        let named = refused.map_or_else(|| "-".to_string(), |of| of.to_string());
        let kind = kind
            .as_deref()
            .map_or_else(|| "-".to_string(), claim::journal_word);
        self.note_drop("unreadable", || {
            format!("aterm stream-unreadable k={kind} in={named} {why}")
        });
        if let Some(of) = refused {
            self.answer_own(of, Some(&why), now_ms);
        }
    }

    /// Answer aterm's record `of` on the link's own account: `took`, or
    /// `refused` with `why`.
    fn answer_own(&mut self, of: u64, why: Option<&str>, now_ms: u64) {
        self.outbox.answered(why.is_none());
        let body = match why {
            None => Body::Took { of },
            Some(why) => Body::Refused {
                of,
                why: why.to_string(),
            },
        };
        let ms = self.outbox.stamp(now_ms);
        let _ = self.put(body, true, ms, now_ms);
    }

    /// aterm acknowledged the harness record `seq`. A `claim` record it did
    /// not apply loses the link, but for one dropped for the rate, which is
    /// sent again; an acknowledgement of a record not written on this stream
    /// is a fault that loses it. The first drop of each reason on a stream is
    /// noted, `aterm stream-dropped seq=<n> k=<kind> dropped=<word>`.
    fn acked(&mut self, seq: u64, dropped: Option<Dropped>, now_ms: u64) {
        let Ok(kind) = self.outbox.ack(seq, dropped.as_ref()) else {
            self.stream_lost(&format!("stream ack={seq} unwritten"), now_ms);
            return;
        };
        let State::Held(claimed) = &mut self.state else {
            return;
        };
        let Some(open) = claimed.stream.as_mut() else {
            return;
        };
        if open.claim.as_ref().is_some_and(|(sent, _)| *sent <= seq)
            && let Some((sent, body)) = open.claim.take()
            && sent == seq
        {
            match dropped.as_ref() {
                None | Some(Dropped::Applied) => {}
                Some(Dropped::Rate) => {
                    if let Err(error) = self.put_claim(body, now_ms) {
                        self.lose(&format!("reannounce {error}"), now_ms);
                        return;
                    }
                }
                Some(dropped) => {
                    self.lose(
                        &format!(
                            "reannounce dropped={}",
                            claim::journal_word(&dropped.word())
                        ),
                        now_ms,
                    );
                    return;
                }
            }
        }
        let Some(dropped) = dropped else {
            return;
        };
        let word = dropped.word();
        let reason = word.split(':').next().unwrap_or_default().to_string();
        self.note_drop(&reason, || {
            format!(
                "aterm stream-dropped seq={seq} k={} dropped={}",
                kind.map_or("-", record::Kind::word),
                claim::journal_word(&word)
            )
        });
    }

    /// Note `line` the first time `reason` comes on the open stream.
    fn note_drop(&mut self, reason: &str, line: impl FnOnce() -> String) {
        let State::Held(claimed) = &mut self.state else {
            return;
        };
        let Some(open) = claimed.stream.as_mut() else {
            return;
        };
        if open.noted.iter().any(|noted| noted == reason) {
            return;
        }
        open.noted.push(reason.to_string());
        self.notes.push(line());
    }

    /// The child was started: announce the claim again with what it now runs,
    /// so `supervisor_vendor=` stays true and, under contract 4, aterm learns
    /// the new child's `child=<pid>:<start>`. The harness calls it after every
    /// child start, the first included once [`Child::process`] is known. The
    /// announcement is the same `meta set supervisor` line as the claim, at the
    /// claim's contract; a contract-4 reply is the grant from then on (aterm
    /// grants what its words and a person's cap allow now). A refusal, or a
    /// contract-4 reply that states no grant, loses the link.
    pub fn reannounce(&mut self, child: Child<'_>, now_ms: u64) {
        let State::Held(claimed) = &mut self.state else {
            return;
        };
        // A hold aterm stated was on the child that ended.
        claimed.last_hold = None;
        if !claimed.standing {
            return;
        }
        claimed.recheck_at_ms = 0;
        let contract = claimed.grant.contract;
        let asked = contract.scopes(self.config.scopes());
        if claimed.stream.is_some() {
            self.claim_record(child, asked, now_ms);
            return;
        }
        let announced = ask(
            &mut claimed.conn,
            "reannounce",
            claim::claim_verb(
                &self.config,
                contract,
                claimed.locator.sid(),
                claimed.pid,
                child,
            ),
        )
        .and_then(|reply| {
            if !ok(&reply) {
                return Err(Failure::Refused(format!(
                    "reannounce {}",
                    reply_words(&reply)
                )));
            }
            match contract {
                Contract::Three => Ok(None),
                Contract::Four => Grant::of_reply(&reply, &asked)
                    .map(Some)
                    .map_err(|error| Failure::Refused(format!("reannounce {error}"))),
            }
        });
        let vendor = child
            .vendor
            .map_or_else(|| "-".to_string(), claim::journal_word);
        match announced {
            Ok(None) => self
                .notes
                .push(format!("aterm reannounced vendor={vendor}")),
            Ok(Some(grant)) => {
                self.notes.push(format!(
                    "aterm reannounced vendor={vendor} child={} scope={} revision={}",
                    child
                        .process
                        .and_then(claim::ChildProcess::word)
                        .unwrap_or_else(|| "-".to_string()),
                    grant.words(),
                    grant.revision_word()
                ));
                claimed.grant = grant;
            }
            Err(failure) => self.lose(failure.why(), now_ms),
        }
    }
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
