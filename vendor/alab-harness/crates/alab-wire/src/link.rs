// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The link: a harness's claim (contract 4, or 3) held over time, the reads that judge
//! a safe point and a relaunch by aterm's facts, and the event stream that
//! wakes the harness and carries aterm's restart requests.
//!
//! [`crate::claim`] holds the texts and the judges and performs no I/O;
//! [`crate::wire`] carries lines. [`Link`] is the state machine between them:
//! one request connection, one event subscription, and what a harness does when
//! either stops answering. It keeps no clock (every call takes the caller's
//! `now_ms`), starts no process and writes no journal: what it would journal it
//! queues as lines the caller takes with [`Link::take_notes`].
//!
//! **The claim sequence** (`claim`), run once before the first vendor start
//! and again every [`RECLAIM_EVERY_MS`] while the link is lost:
//!
//! 1. connect and authenticate;
//! 2. `harness policy`: contract 3 or 4 served and `effective=on`; the claim
//!    is made at the highest of the two the policy lists
//!    ([`crate::claim::Contract::choose`]). A policy listing neither is read
//!    again on the clock, since an aterm update can serve them;
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
//! every [`RECLAIM_EVERY_MS`], as for a lost link.
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
//! fails or outlives its bound drops the connection (aterm drops a claim with
//! the connection that made it) and the link is lost.
//!
//! Both sockets are close-on-exec ([`crate::wire`]): a vendor child never
//! inherits them.

use std::ffi::OsString;
use std::os::fd::{AsRawFd as _, RawFd};
use std::time::Duration;

use crate::claim::{self, Child, Contract, Grant, Policy, Restart, SafePoint, Scope, Status};
use crate::config::Config;
use crate::relaunch;
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
    /// policy that lists no contract this library speaks (contract 2 alone),
    /// or one whose highest contract names none of the harness's duties
    /// (contract 3 for a harness without `relaunch` or `upgrade`). An aterm
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

/// Whether `policy` lets a rostered harness claim: contract 3 or 4 served and
/// the policy in effect. A policy that lists neither (an aterm that serves only
/// contract 2, or none this library speaks) is [`Failure::Unserved`]: an aterm
/// update can serve 3 or 4 while the harness runs, and the harness claims again
/// after one (the interface design, section 2.2). A policy off by a setting
/// that can be switched on again
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
pub fn admits(policy: &Policy) -> Result<(), Failure> {
    if !policy.serves_contract() {
        let contracts = policy
            .contracts
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        return Err(Failure::Unserved(format!("policy contract={contracts}")));
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
/// roster guard (contract 3), the claim at the contract the policy chose, and its proof
/// read back from `status`. A claim `status` does not show as this harness's,
/// proven and taking every granted scope word is released before the refusal
/// is returned. The grant, when it stands.
fn guarded_claim(
    conn: &mut Connection,
    config: &Config,
    sid: &str,
    pid: u32,
    child: Child<'_>,
) -> Result<Grant, Failure> {
    let policy = read_policy(conn, sid)?;
    admits(&policy)?;
    let Some(contract) = policy.contract() else {
        return Err(Failure::Refused("policy contract=-".to_string()));
    };
    let asked = contract.scopes(config.scopes());
    if asked.is_empty() {
        // A harness that asks only for duties contract 3 does not name: an
        // aterm updated while it runs may list contract 4, at which it claims
        // (the interface design, section 2.2: a harness process outlives an
        // aterm update and claims again afterwards), so the policy is read
        // again on the clock. `Config` is never empty, so contract 4 always
        // names a word.
        return Err(Failure::Unserved(format!(
            "contract={} scope=-",
            contract.number()
        )));
    }
    // Never a trial claim under contract 3: without the row, nothing below is
    // sent. Contract 4 has no such list: its image rule reads the registry
    // row, a verified record may claim any word, and a claim `status` does
    // not show proven is released below (the interface design, section 2.2
    // and HX-11).
    if contract == Contract::Three
        && !claim::rostered(&ask(conn, "roster", claim::info_verb(sid))?, config.id())
    {
        return Err(Failure::Refused("unrostered".to_string()));
    }
    let reply = ask(
        conn,
        "claim",
        claim::claim_verb(config, contract, sid, pid, child),
    )?;
    if !ok(&reply) {
        return Err(Failure::Refused(format!("claim {}", reply_words(&reply))));
    }
    // A claim this sequence made and then gives back. A release aterm did not
    // take, or did not answer, may leave the claim standing on this
    // connection: the failure is then [`Failure::Gone`], so a caller that
    // kept the connection (a claim made again on a held link) drops it, and
    // aterm drops a claim with the connection it is bound to. At launch the
    // connection goes either way.
    let release = |conn: &mut Connection, failure: Failure| -> Failure {
        match ask(conn, "release", claim::release_verb(config, sid, pid)) {
            Ok(reply) if ok(&reply) => failure,
            Ok(reply) => Failure::Gone(format!(
                "{} (release {})",
                failure.why(),
                reply_words(&reply)
            )),
            Err(gone) => Failure::Gone(format!("{} ({})", failure.why(), gone.why())),
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
        proof(&status, &holder, needed).map(|()| grant)
    });
    match proven {
        Err(failure @ (Failure::Refused(_) | Failure::Later(_))) => Err(release(conn, failure)),
        other => other,
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
/// included. A claim made before a later step failed is released.
fn claim(
    config: &Config,
    locator: &Locator,
    pid: u32,
    child: Child<'_>,
    opening: Duration,
) -> Result<Claimed, Failure> {
    let socket = locator
        .socket()
        .map_err(|error| Failure::Gone(format!("graph {error}")))?;
    let mut conn = Connection::connect(&socket, opening)
        .map_err(|error| Failure::Gone(format!("connect {error}")))?;
    conn.set_timeout(REQUEST_TIMEOUT);
    let grant = guarded_claim(&mut conn, config, locator.sid(), pid, child)
        .map_err(|failure| busy(failure, conn.timed_out()))?;
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
}

impl Claimed {
    /// Give the claim back, keeping the connection. The policy is read again
    /// every [`RECLAIM_EVERY_MS`] from here ([`Link::tick`]): aterm sends no
    /// event when a session's own Autopilot word changes, so nothing else would
    /// find the policy on again.
    fn release(&mut self, config: &Config, now_ms: u64) -> Result<(), Failure> {
        self.standing = false;
        self.policy_due_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
        let reply = ask(
            &mut self.conn,
            "release",
            claim::release_verb(config, self.locator.sid(), self.pid),
        )?;
        if ok(&reply) {
            Ok(())
        } else {
            // A release aterm did not take is made certain by closing the
            // connection the claim is bound to.
            Err(Failure::Gone(format!("release {}", reply_words(&reply))))
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
        let line = match read.and_then(|policy| admits(&policy)) {
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
                Ok(grant) => {
                    self.standing = true;
                    self.recheck_at_ms = 0;
                    self.policy_due_ms = 0;
                    self.last_hold = None;
                    let line = claimed_line(&grant, &self.holder, child);
                    self.grant = grant;
                    line
                }
                Err(Failure::Gone(why)) => return Err(Failure::Gone(why)),
                Err(
                    Failure::Refused(why)
                    | Failure::Off(why)
                    | Failure::Later(why)
                    | Failure::Unserved(why),
                ) => {
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
        match admits(&policy) {
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
        Ok(
            match relaunch::permitted(&policy, &status, &self.holder, asked) {
                Ok(()) => Leave::Granted,
                Err(why) if !status.held_by(&self.holder) => Leave::Taken(why),
                Err(why) => Leave::Denied(why),
            },
        )
    }
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
        /// The last reason an attempt failed, so it is noted once.
        noted: String,
        /// Whether a claim stood before: the link was held. A launch that has
        /// not claimed yet has no duty towards the child.
        claimed: bool,
        /// The hold aterm stated last before the link was lost
        /// ([`Claimed::last_hold`]), as the line of the wait it keeps: nothing
        /// moves on the harness's own readings over it. It stands until aterm
        /// answers again: a proven claim, or a refusal, which is aterm's newer
        /// word.
        held: Option<String>,
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
    /// The policy, which admits the claim (contract 3 or 4 served and in effect).
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
    /// The last restart aterm asked the claim holder to make, if one came. It
    /// is a request, never permission: [`Link::relaunch_word`] with `asked`
    /// still decides it.
    pub restart: Option<Restart>,
}

/// A harness's link to aterm over the session's life.
#[derive(Debug)]
pub struct Link {
    config: Config,
    pid: u32,
    state: State,
    notes: Vec<String>,
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
        let mut link = Self::absent(config, pid);
        match claim(&link.config, locator, pid, child, CONNECT_TIMEOUT) {
            Ok(claimed) => {
                link.notes
                    .push(claimed_line(&claimed.grant, &claimed.holder, child));
                link.state = State::Held(Box::new(claimed));
            }
            Err(Failure::Later(why) | Failure::Off(why) | Failure::Unserved(why)) => {
                link.notes.push(format!("aterm unclaimed {why}"));
                link.state = State::Lost {
                    locator: locator.clone(),
                    retry_at_ms: now_ms.saturating_add(RECLAIM_EVERY_MS),
                    noted: why,
                    claimed: false,
                    held: None,
                };
            }
            Err(failure) => link
                .notes
                .push(format!("aterm unclaimed {}", failure.why())),
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
    /// child from an earlier image of itself. The earlier image's claim ended
    /// with its connections at the `exec` (both are close-on-exec), but its
    /// duty towards the child did not: the child runs, and nothing else
    /// supervises it. So the link [`Link::claims`] from the start, and a
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
        let mut link = Self::launch_at(config, pid, &locator, child, now_ms);
        match &mut link.state {
            State::Held(_) => {}
            State::Lost { claimed, .. } => *claimed = true,
            State::Absent => {
                link.state = State::Lost {
                    locator,
                    retry_at_ms: now_ms.saturating_add(RECLAIM_EVERY_MS),
                    noted: link
                        .notes
                        .last()
                        .and_then(|note| note.strip_prefix("aterm unclaimed "))
                        .unwrap_or_default()
                        .to_string(),
                    claimed: true,
                    held: None,
                };
            }
        }
        link
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
    /// and close the link: the harness is about to exit.
    pub fn release(&mut self, now_ms: u64) {
        if let State::Held(claimed) = &mut self.state
            && claimed.standing
        {
            let _ = claimed.release(&self.config, now_ms);
        }
        self.state = State::Absent;
    }

    /// The held link stopped answering (or its claim is no longer this
    /// harness's): drop both connections, which also drops the claim at aterm,
    /// and claim again on the reclaim clock.
    fn lose(&mut self, why: &str, now_ms: u64) {
        let State::Held(claimed) = std::mem::replace(&mut self.state, State::Absent) else {
            return;
        };
        self.notes.push(format!("aterm lost {why}"));
        self.state = State::Lost {
            locator: claimed.locator.clone(),
            retry_at_ms: now_ms.saturating_add(RECLAIM_EVERY_MS),
            noted: String::new(),
            claimed: true,
            // Only a standing claim's answer holds: a released claim's looks
            // were already the harness's own.
            held: claimed.last_hold.clone().filter(|_| claimed.standing),
        };
    }

    /// The link's own clock, run on every pass of the harness's loop: claim a
    /// lost link again for `child`, reopen an ended event stream, and read the
    /// policy again when it is due. An ended stream is reopened every
    /// [`RECLAIM_EVERY_MS`], and the policy is read at each attempt (the
    /// stream that would announce a change is down) and once more when it is
    /// back. Each is bounded by [`REQUEST_TIMEOUT`] per step and stops at the
    /// first step that fails.
    pub fn tick(&mut self, child: Child<'_>, now_ms: u64) {
        match &mut self.state {
            State::Absent => {}
            State::Lost {
                locator,
                retry_at_ms,
                noted,
                claimed,
                held,
            } => {
                if now_ms < *retry_at_ms {
                    return;
                }
                match claim(&self.config, locator, self.pid, child, REQUEST_TIMEOUT) {
                    Ok(made) => {
                        self.notes
                            .push(claimed_line(&made.grant, &made.holder, child));
                        self.state = State::Held(Box::new(made));
                    }
                    Err(failure) => {
                        *retry_at_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
                        if noted != failure.why() {
                            failure.why().clone_into(noted);
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
                        if !*claimed && matches!(failure, Failure::Refused(_)) {
                            self.state = State::Absent;
                        }
                    }
                }
            }
            State::Held(claimed) => {
                let mut policy_due = claimed.policy_due_ms != 0 && now_ms >= claimed.policy_due_ms;
                if claimed.events.is_none() && now_ms >= claimed.events_retry_ms {
                    let reopened = claimed.locator.socket().and_then(|socket| {
                        EventReader::connect(&socket, claimed.locator.sid(), REQUEST_TIMEOUT)
                    });
                    match reopened {
                        Ok(events) => {
                            claimed.events = Some(events);
                            self.notes.push("aterm events-reopened".to_string());
                            // What the stream said while it was down was not heard.
                            policy_due = true;
                        }
                        Err(_) => {
                            claimed.events_retry_ms = now_ms.saturating_add(RECLAIM_EVERY_MS);
                            // No event can say the policy moved while the
                            // stream is down, so it is read on the reopen
                            // clock until the stream is back (Decision 18).
                            policy_due = true;
                        }
                    }
                }
                if policy_due {
                    self.follow_policy(child, now_ms);
                }
            }
        }
    }

    /// The event descriptor is readable: drain it without waiting. A policy
    /// event (or any loss, which may hide one) is answered by a fresh policy
    /// read, made for `child`; a restart request is returned for the harness to
    /// decide. A harness whose child is about to be replaced anyway ignores
    /// it: aterm sent it for that child.
    pub fn events(&mut self, child: Child<'_>, now_ms: u64) -> Drained {
        let State::Held(claimed) = &mut self.state else {
            return Drained::default();
        };
        let Some(events) = claimed.events.as_mut() else {
            return Drained::default();
        };
        let update = events.drain();
        claimed.recheck_at_ms = 0;
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
        if update.requires_refresh() {
            self.follow_policy(child, now_ms);
        }
        Drained {
            drained: true,
            restart: update.restart,
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
