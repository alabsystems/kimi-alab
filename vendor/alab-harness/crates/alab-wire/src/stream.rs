// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session stream's client half, which [`crate::Link`] drives: the claim's connection once
//! aterm has answered `stream @<sid> v=1` with `OK`, the harness's records numbered, paced and
//! kept until aterm acknowledges them, and what each of aterm's records becomes.
//!
//! This is the client of aterm's harness interface design, section 2.3 "The session stream", and
//! of the plan's decisions D7, D8, D10, D24, D27 and D28 (`hx15-stream-split.md`). The records
//! and their codec are [`crate::record`]'s; this module carries them.
//!
//! **Numbering and resending.** Each harness record takes the next `seq`, from 1 with no gaps,
//! and is kept until aterm acknowledges it or a later one: aterm acknowledges every record, in
//! order, and an `ack` at `seq` stands for every record up to it (D8). When a stream opens, its
//! `OK` line names `acked=`, and every kept record above it is written again under the number
//! it had, so a `seq` the caller was given stays the record's. A `claim` record kept from before
//! is written as a `beat` in its place, since the claim placed before every open supersedes it,
//! and so is an answer whose `in` names another stream's record. Only when aterm's count went
//! back (a new instance whose count starts again, or one that forgot this claimant when its
//! re-exec grace ended, D28) are the caller's records numbered again from `acked+1`, the link's
//! own dropped, and the moves reported ([`crate::Drained::renumbered`]).
//!
//! **The bounds and the pace.** A caller's records ([`crate::Link::send`]) are kept up to
//! [`RESEND_MAX`] records and [`RESEND_BYTES_MAX`] bytes of lines; past either the record is
//! refused ([`SendError::Full`]) and not numbered. The link's own records (`beat`, `claim`, and
//! the answers `took` and `refused`) are kept up to [`OWN_MAX`]: past it aterm neither reads
//! nor acknowledges what it is sent, and the stream is lost. Every record is written at
//! [`PACE_BURST`] at once and [`PACE_PER_S`] a second at most, under aterm's budget of
//! [`RATE_BURST`] and [`RATE_PER_S`] a stream (D10), so aterm never drops one for the rate; a
//! record over the pace waits, kept, for the link's next pass.
//!
//! **aterm's records.** Each is answered once with `took` or `refused` (D27). The link answers
//! `hand`, `policy` and `restart`, which it acts on itself, and a kind this library does not
//! know, which is ignored and counted (`took`, as aterm takes an unknown kind of the harness's).
//! `want`, and `prompt`, `answer`, `touch`, `interrupt` and `set` when the `session` record the
//! caller last sent lists the kind in `takes`, reach the caller as a [`Received`] with its `in`,
//! and the caller answers it ([`crate::Link::answer`]); one of the five that `session` did not
//! list is refused `not taken`. A line that cannot be read is answered `refused` when its `seq`
//! can be found in it and it is not an `ack`; a record at or below the last one heard was
//! answered before and is passed over. `ack` is not answered, and is not counted in aterm's `in`.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsRawFd as _, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crate::claim::Restart;
use crate::record::{
    Body, By, Composer, Dropped, Kind, LINE_MAX, LineError, Mode, RATE_BURST, RATE_PER_S, Record,
    Side, StreamReply,
};

/// The most records of the caller's a link keeps unacknowledged. With [`OWN_MAX`] it is under
/// [`PACE_BURST`], so a stream that opens again writes everything kept at once.
pub const RESEND_MAX: usize = 256;

/// The most bytes of lines (newlines not counted) a link keeps unacknowledged for the caller.
pub const RESEND_BYTES_MAX: usize = 4 * 1024 * 1024;

/// The most records of the link's own (`beat`, `claim`, `took`, `refused`) kept unacknowledged:
/// past it the stream is lost, since aterm neither reads nor acknowledges what it is sent.
pub const OWN_MAX: usize = 128;

/// The most of aterm's records the caller was given and has not answered: past it the oldest is
/// refused `unanswered`.
pub const AWAITED_MAX: usize = 64;

/// The records a stream may write at once: four fifths of aterm's [`RATE_BURST`].
pub const PACE_BURST: u32 = RATE_BURST / 5 * 4;

/// The records a stream may write a second after its burst: four fifths of aterm's
/// [`RATE_PER_S`], so a clock that runs a little apart from aterm's never spends aterm's budget.
pub const PACE_PER_S: u32 = RATE_PER_S / 5 * 4;

/// The highest `seq`, `acked=` or `in=` either side counts to. A number above it from aterm is a
/// fault that loses the stream, so no count here can wrap.
pub const SEQ_MAX: u64 = u64::MAX >> 1;

/// The most bytes one read pass takes from the stream; the rest waits for the next pass, and the
/// descriptor stays readable meanwhile.
const DRAIN_BYTES: usize = 1024 * 1024;

/// The most bytes kept of lines read while a write waits: past it the write waits for room
/// alone, and its bound ends it if aterm waits for the harness to read.
const PENDING_MAX: usize = 4 * LINE_MAX;

/// The bytes kept of the start of a line over [`LINE_MAX`], where its `k` and `seq` are found.
const HEAD_MAX: usize = 256;

/// What one of aterm's records asks of the caller: `want`, and the kinds a harness takes by
/// naming them in its `session` record's `takes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incoming {
    /// `want`: send the conversation again.
    Want,
    /// `prompt`: text for the agent.
    Prompt {
        /// The text.
        text: String,
        /// Who sent it: `person`, `ctl` or `peer:<tab>`.
        by: By,
    },
    /// `answer`: a choice on an `ask`.
    Answer {
        /// The `seq` of the harness's `ask`.
        ask: u64,
        /// The option chosen.
        option: Option<String>,
        /// The text given.
        text: Option<String>,
        /// Who chose.
        by: By,
        /// aterm's `human_seq` at the click, which fences the harness's press.
        human_seq: u64,
    },
    /// `touch`: a person put a hand on the ask; hold its automatic answer.
    Touch {
        /// The `seq` of the harness's `ask`.
        ask: u64,
    },
    /// `interrupt`: stop the turn.
    Interrupt,
    /// `set`: change the model, the effort or the mode.
    Set {
        /// The model: a model word.
        model: Option<String>,
        /// The effort level.
        effort: Option<String>,
        /// The mode.
        mode: Option<Mode>,
    },
}

impl Incoming {
    /// Its kind.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        match self {
            Self::Want => Kind::Want,
            Self::Prompt { .. } => Kind::Prompt,
            Self::Answer { .. } => Kind::Answer,
            Self::Touch { .. } => Kind::Touch,
            Self::Interrupt => Kind::Interrupt,
            Self::Set { .. } => Kind::Set,
        }
    }
}

/// One of aterm's records for the caller ([`crate::Drained::incoming`]), with the `in` its
/// answer names. The caller answers it once, `took` or `refused` ([`crate::Link::answer`]): it
/// may refuse one it cannot act on, such as an `answer` for an ask already answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    /// aterm's `seq` for the record: its `in`.
    pub of: u64,
    /// The record.
    pub incoming: Incoming,
}

/// A `hand` record heard ([`crate::Drained::hand`]): a person's hand, which also fills the hand
/// fields of [`crate::Link::status`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hand {
    /// aterm's `seq` for the record: its `in`.
    pub of: u64,
    /// aterm's count of a person's keys and gestures.
    pub human_seq: u64,
    /// What the input box holds.
    pub composer: Composer,
    /// Whether a halt stands.
    pub hold: bool,
}

/// Why [`crate::Link::send`] did not take a record. The text is one journal word group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// No stream is open, and none was open on this claim to be opened again: aterm did not
    /// grant one (contract 3, an aterm that answered `stream` with an `ERR`, or a config that
    /// does not stream), or the claim was given back. The harness keeps its v0.1 path.
    NoStream,
    /// [`RESEND_MAX`] records or [`RESEND_BYTES_MAX`] bytes of the caller's wait for aterm's
    /// acknowledgement; the record was not numbered.
    Full,
    /// A kind the caller does not send: one of aterm's, an unknown one, or one the link sends
    /// itself (`claim`, `beat`, `took`, `refused`). Its word.
    Kind(String),
    /// The record cannot be written ([`Record::line`]).
    Line(LineError),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoStream => f.write_str("stream=none"),
            Self::Full => f.write_str("queue=full"),
            Self::Kind(word) => write!(f, "k={} unsendable", crate::claim::journal_word(word)),
            Self::Line(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SendError {}

/// Why [`crate::Link::answer`] did not answer. The text is one journal word group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnswerError {
    /// No record at this `in` awaits the caller's answer: it was answered, it was refused
    /// `unanswered` past [`AWAITED_MAX`], it was never given to the caller, or it named a stream
    /// aterm no longer has.
    NotAwaited(u64),
    /// The refusal cannot be written ([`Record::line`]); the record still awaits an answer.
    Line(LineError),
}

impl fmt::Display for AnswerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAwaited(of) => write!(f, "in={of} unawaited"),
            Self::Line(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AnswerError {}

/// What a link's streams have carried, over the link's life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Streams opened.
    pub opened: u64,
    /// Records written, resends included.
    pub written: u64,
    /// Records kept above a stream's `acked=` when it opened, to write again.
    pub resent: u64,
    /// aterm's acknowledgements read.
    pub acks: u64,
    /// Acknowledged `dropped=bound`.
    pub bound: u64,
    /// Acknowledged `dropped=rate`.
    pub rate: u64,
    /// Acknowledged `dropped=scope`.
    pub scope: u64,
    /// Acknowledged `dropped=applied`.
    pub applied: u64,
    /// Acknowledged `dropped=refused:<word>`.
    pub refused: u64,
    /// Acknowledged with a `dropped=` word this library does not know.
    pub other: u64,
    /// aterm's records answered `took`.
    pub took: u64,
    /// aterm's records answered `refused`.
    pub declined: u64,
    /// aterm's lines that could not be read (over [`LINE_MAX`] or out of the grammar).
    pub unreadable: u64,
    /// aterm's records at or below the last one heard, passed over.
    pub repeated: u64,
    /// aterm's records of a kind this library does not know, answered `took` and ignored.
    pub unknown_kinds: u64,
    /// Fields of aterm's records this library does not know, at every depth, passed over.
    pub unknown_fields: u64,
    /// `hand` records heard.
    pub hands: u64,
    /// `policy` records heard.
    pub policies: u64,
    /// `restart` records heard.
    pub restarts: u64,
    /// The caller's records given up because no stream would write them again.
    pub given_up: u64,
    /// The caller's records numbered again when a stream opened on an aterm whose count of this
    /// claimant's records went back.
    pub renumbered: u64,
    /// aterm's records refused `unanswered` because the caller left [`AWAITED_MAX`] unanswered.
    pub unanswered: u64,
}

/// One kept record, its line, and whether the link made it.
#[derive(Debug)]
struct Kept {
    record: Record,
    line: String,
    own: bool,
}

/// The writing pace, on the caller's `now_ms`: [`PACE_BURST`] records at once, [`PACE_PER_S`]
/// more a second.
#[derive(Debug, Default)]
struct Pace {
    /// Thousandths of a record.
    milli: u64,
    /// When (ms) `milli` was last brought up to date.
    at_ms: u64,
}

impl Pace {
    const FULL: u64 = PACE_BURST as u64 * 1_000;

    fn full(now_ms: u64) -> Self {
        Self {
            milli: Self::FULL,
            at_ms: now_ms,
        }
    }

    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.at_ms);
        self.at_ms = self.at_ms.max(now_ms);
        self.milli = self
            .milli
            .saturating_add(elapsed.saturating_mul(u64::from(PACE_PER_S)))
            .min(Self::FULL);
    }

    /// Spend one record, if the pace holds one at `now_ms`.
    fn take(&mut self, now_ms: u64) -> bool {
        self.refill(now_ms);
        if self.milli < 1_000 {
            return false;
        }
        self.milli -= 1_000;
        true
    }

    /// When (ms) the pace holds a record again.
    fn next_ms(&self) -> u64 {
        if self.milli >= 1_000 {
            self.at_ms
        } else {
            self.at_ms
                .saturating_add((1_000 - self.milli).div_ceil(u64::from(PACE_PER_S)))
        }
    }
}

/// What a stream's open did to the kept records.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Opened {
    /// aterm holds nothing the caller sent on an earlier stream: the first stream, a new epoch,
    /// or an aterm whose count of this claimant went back.
    pub(crate) fresh: bool,
    /// The caller's records numbered again, as `(old, new)`.
    pub(crate) renumbered: Vec<(u64, u64)>,
    /// The records kept above `acked=`, to write again.
    pub(crate) resent: usize,
}

/// The harness's side of the stream's numbers: its records kept until aterm acknowledges them,
/// the last of aterm's records heard, and the records the caller has to answer. It outlives a
/// stream that closes: the next one resends from its `acked=`.
#[derive(Debug, Default)]
pub(crate) struct Outbox {
    /// Whether records are kept: a stream opened on this claim and was not given up since.
    live: bool,
    /// The next record's `seq`.
    next: u64,
    /// The last record aterm acknowledged.
    acked: u64,
    /// The last record written on the stream now open; those above it wait for the pace.
    written: u64,
    /// The records written or to write, above `acked`, in `seq` order.
    queue: VecDeque<Kept>,
    /// The caller's bytes in `queue`.
    bytes: usize,
    /// The caller's records in `queue`.
    callers: usize,
    /// The link's own records in `queue`.
    owns: usize,
    /// The epoch of the stream last opened.
    epoch: Option<String>,
    /// The last of aterm's records heard in that epoch.
    heard: u64,
    /// The `in` of each of aterm's records given to the caller and not answered yet, oldest first.
    awaited: VecDeque<u64>,
    /// The kinds the caller's last `session` record takes. It is the caller's declaration, which
    /// aterm keeps with the tab's applied `session`, so it outlives a stream given up.
    takes: Vec<Kind>,
    /// The caller's last wall clock and the `now_ms` it came with, from which the link's own
    /// records are stamped: this crate reads no clock.
    wall: Option<(u64, u64)>,
    /// The writing pace of the stream now open.
    pace: Pace,
    /// What the streams carried.
    counts: Counts,
}

impl Outbox {
    /// Whether records are kept for a stream: one is open or is to be opened again.
    pub(crate) const fn live(&self) -> bool {
        self.live
    }

    /// The epoch of the stream last opened.
    pub(crate) fn epoch(&self) -> Option<&str> {
        self.epoch.as_deref()
    }

    /// What the streams carried.
    pub(crate) const fn counts(&self) -> Counts {
        self.counts
    }

    /// What the streams carried, to count in.
    pub(crate) const fn counts_mut(&mut self) -> &mut Counts {
        &mut self.counts
    }

    /// Whether the caller's last `session` record takes `kind`.
    pub(crate) fn takes(&self, kind: Kind) -> bool {
        self.takes.contains(&kind)
    }

    /// Give up what is kept: no stream will resend it (a claim given back, an aterm that
    /// refused the stream for good, a link absent from now on). The counts, the wall clock and
    /// what the caller takes are kept. The caller's records given up.
    pub(crate) fn clear(&mut self) -> u64 {
        let given_up = self.callers as u64;
        let mut counts = self.counts;
        counts.given_up += given_up;
        *self = Self {
            counts,
            wall: self.wall,
            takes: std::mem::take(&mut self.takes),
            ..Self::default()
        };
        given_up
    }

    /// The caller's wall clock `ms` (milliseconds since 1970) at `now_ms`, from which the link's
    /// own records are stamped.
    pub(crate) const fn set_wall(&mut self, ms: u64, now_ms: u64) {
        self.wall = Some((ms, now_ms));
    }

    /// The wall clock for a record the link makes at `now_ms`: the caller's last one moved on by
    /// `now_ms`, or 0 before the caller has given one. `ms` is only shown.
    pub(crate) fn stamp(&self, now_ms: u64) -> u64 {
        self.wall
            .map_or(0, |(ms, at)| ms.saturating_add(now_ms.saturating_sub(at)))
    }

    /// A stream opened at `now_ms` with `reply`, whose numbers are at most [`SEQ_MAX`]: give up
    /// what it acknowledged, and keep the rest to write again (module documentation).
    pub(crate) fn open(&mut self, reply: &StreamReply, now_ms: u64) -> Opened {
        while self
            .queue
            .front()
            .is_some_and(|kept| kept.record.seq <= reply.acked)
        {
            self.pop();
        }
        let same_epoch = self.epoch.as_deref() == Some(reply.epoch.as_str());
        let same_in = same_epoch && reply.sent >= self.heard;
        let went_back = reply.acked < self.acked;
        let fresh = !same_in || went_back;
        let mut renumbered = Vec::new();
        let mut kept = std::mem::take(&mut self.queue);
        if went_back {
            kept.retain(|kept| !kept.own);
            let mut seq = reply.acked;
            for record in &mut kept {
                seq = seq.saturating_add(1);
                if record.record.seq != seq {
                    renumbered.push((record.record.seq, seq));
                    record.record.seq = seq;
                }
            }
        } else {
            for record in &mut kept {
                let stale = match record.record.body {
                    Body::Claim { .. } => true,
                    Body::Took { .. } | Body::Refused { .. } => !same_in,
                    _ => false,
                };
                if stale {
                    record.record.body = Body::Beat;
                }
            }
        }
        // A line written again under a longer number can pass the bound only by a digit; such a
        // record is given up rather than sent to be dropped.
        self.queue = kept
            .into_iter()
            .filter_map(|mut kept| {
                kept.line = kept.record.line().ok()?;
                Some(kept)
            })
            .collect();
        self.bytes = 0;
        self.callers = 0;
        self.owns = 0;
        for kept in &self.queue {
            if kept.own {
                self.owns += 1;
            } else {
                self.callers += 1;
                self.bytes += kept.line.len();
            }
        }
        self.next = self
            .queue
            .back()
            .map_or(reply.acked, |kept| kept.record.seq)
            .saturating_add(1);
        self.acked = reply.acked;
        self.written = reply.acked;
        if !same_in {
            self.awaited.clear();
        }
        self.heard = reply.sent;
        self.epoch = Some(reply.epoch.clone());
        self.live = true;
        self.pace = Pace::full(now_ms);
        self.counts.opened += 1;
        self.counts.resent += self.queue.len() as u64;
        self.counts.renumbered += renumbered.len() as u64;
        Opened {
            fresh,
            renumbered,
            resent: self.queue.len(),
        }
    }

    fn pop(&mut self) -> Option<Kept> {
        let kept = self.queue.pop_front()?;
        if kept.own {
            self.owns -= 1;
        } else {
            self.bytes -= kept.line.len();
            self.callers -= 1;
        }
        Some(kept)
    }

    /// Number `body` and keep it with its line. `own` is a record the link makes, kept up to
    /// [`OWN_MAX`]; a caller's is kept up to [`RESEND_MAX`] and [`RESEND_BYTES_MAX`]. A caller's
    /// `session` record names what it takes, and a caller's `ms` stamps the link's records
    /// from here. Its `seq`.
    pub(crate) fn push(
        &mut self,
        body: Body,
        ms: u64,
        own: bool,
        now_ms: u64,
    ) -> Result<u64, SendError> {
        let seq = self.next.max(1);
        if seq > SEQ_MAX {
            return Err(SendError::Full);
        }
        let record = Record { seq, ms, body };
        let line = record.line().map_err(SendError::Line)?;
        if own {
            if self.owns >= OWN_MAX {
                return Err(SendError::Full);
            }
            self.owns += 1;
        } else {
            if self.callers >= RESEND_MAX
                || self.bytes.saturating_add(line.len()) > RESEND_BYTES_MAX
            {
                return Err(SendError::Full);
            }
            self.wall = Some((ms, now_ms));
            self.bytes += line.len();
            self.callers += 1;
            if let Body::Session { takes, .. } = &record.body {
                self.takes.clone_from(takes);
            }
        }
        self.next = seq + 1;
        self.queue.push_back(Kept { record, line, own });
        Ok(seq)
    }

    /// The lines of the kept records not yet written on the stream now open, in order, as many
    /// as the pace allows at `now_ms`.
    pub(crate) fn due(&mut self, now_ms: u64) -> Vec<String> {
        let mut lines = Vec::new();
        let written = self.written;
        for kept in self
            .queue
            .iter()
            .skip_while(|kept| kept.record.seq <= written)
        {
            if !self.pace.take(now_ms) {
                break;
            }
            lines.push(kept.line.clone());
            self.written = kept.record.seq;
        }
        lines
    }

    /// When (ms) a kept record waits for the pace to be written, if one does.
    pub(crate) fn pace_due_ms(&self) -> Option<u64> {
        self.queue
            .back()
            .is_some_and(|kept| kept.record.seq > self.written)
            .then(|| self.pace.next_ms())
    }

    /// A record was written.
    pub(crate) const fn wrote(&mut self) {
        self.counts.written += 1;
    }

    /// aterm acknowledged `seq`: it and every record before it are no longer kept. The kind of
    /// the record acknowledged, when it was kept; `Err` for an acknowledgement of a record not
    /// written on this stream, a fault.
    pub(crate) fn ack(&mut self, seq: u64, dropped: Option<&Dropped>) -> Result<Option<Kind>, ()> {
        if seq > self.written {
            return Err(());
        }
        self.counts.acks += 1;
        match dropped {
            None => {}
            Some(Dropped::Bound) => self.counts.bound += 1,
            Some(Dropped::Rate) => self.counts.rate += 1,
            Some(Dropped::Scope) => self.counts.scope += 1,
            Some(Dropped::Applied) => self.counts.applied += 1,
            Some(Dropped::Refused(_)) => self.counts.refused += 1,
            Some(Dropped::Other(_)) => self.counts.other += 1,
        }
        let mut kind = None;
        while self
            .queue
            .front()
            .is_some_and(|kept| kept.record.seq <= seq)
        {
            if let Some(kept) = self.pop()
                && kept.record.seq == seq
            {
                kind = kept.record.body.kind();
            }
        }
        self.acked = self.acked.max(seq);
        Ok(kind)
    }

    /// One of aterm's records, at `seq`, was read: whether it is new (above the last heard).
    /// `Err` for a `seq` above [`SEQ_MAX`], a fault.
    pub(crate) fn hear(&mut self, seq: u64) -> Result<bool, ()> {
        if seq > SEQ_MAX {
            return Err(());
        }
        if seq <= self.heard {
            self.counts.repeated += 1;
            return Ok(false);
        }
        self.heard = seq;
        Ok(true)
    }

    /// A line of aterm's that could not be read, whose `seq`, when one was found in it, is
    /// `seq`: the `in` to refuse, when that `seq` is new. A line with no `seq` found moves
    /// nothing, since aterm's numbers may have gaps and an `ack` is not counted in them.
    pub(crate) fn unreadable(&mut self, seq: Option<u64>) -> Option<u64> {
        self.counts.unreadable += 1;
        let seq = seq.filter(|seq| *seq <= SEQ_MAX && *seq > self.heard)?;
        self.heard = seq;
        Some(seq)
    }

    /// One of aterm's records was answered: `took` or not.
    pub(crate) const fn answered(&mut self, took: bool) {
        if took {
            self.counts.took += 1;
        } else {
            self.counts.declined += 1;
        }
    }

    /// aterm's record `of` was given to the caller to answer. The oldest record waiting, when
    /// more than [`AWAITED_MAX`] wait: it is to be refused `unanswered`.
    pub(crate) fn await_answer(&mut self, of: u64) -> Option<u64> {
        self.awaited.push_back(of);
        if self.awaited.len() <= AWAITED_MAX {
            return None;
        }
        self.counts.unanswered += 1;
        self.awaited.pop_front()
    }

    /// Whether aterm's record `of` awaits the caller's answer.
    pub(crate) fn awaits(&self, of: u64) -> bool {
        self.awaited.contains(&of)
    }

    /// The caller answered aterm's record `of`.
    pub(crate) fn answering(&mut self, of: u64) {
        self.awaited.retain(|awaited| *awaited != of);
    }

    /// The records kept, oldest first.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> Vec<Record> {
        self.queue.iter().map(|kept| kept.record.clone()).collect()
    }
}

/// Whether the caller may send a record of `body`'s kind: the harness's own kinds, except those
/// the link sends itself.
pub(crate) fn sendable(body: &Body) -> Result<(), SendError> {
    match body.kind() {
        Some(Kind::Claim | Kind::Beat | Kind::Took | Kind::Refused) | None => {
            Err(SendError::Kind(body.word().to_string()))
        }
        Some(kind) if kind.sent_by() == Side::Aterm => {
            Err(SendError::Kind(body.word().to_string()))
        }
        Some(_) => Ok(()),
    }
}

/// What one of aterm's records asks of the link.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// `ack`: aterm acknowledged a harness record.
    Ack(Option<Dropped>),
    /// `hand`: the hand fields of `status`.
    Hand {
        /// aterm's count of a person's keys.
        human_seq: u64,
        /// What the input box holds.
        composer: Composer,
        /// Whether a halt stands.
        hold: bool,
    },
    /// `policy`: read the grant again.
    Policy,
    /// `restart`: aterm asks the claim holder to restart its child.
    Restart(Restart),
    /// A record for the caller.
    Incoming(Incoming),
    /// A kind this library does not know: ignored, counted and taken.
    Unknown,
    /// A record the link refuses: why.
    Refuse(&'static str),
}

/// What `body`, one of aterm's records, asks of the link.
pub(crate) fn heard(body: Body) -> Heard {
    match body {
        Body::Ack { dropped } => Heard::Ack(dropped),
        Body::Hand {
            human_seq,
            composer,
            hold,
        } => Heard::Hand {
            human_seq,
            composer,
            hold,
        },
        Body::Policy => Heard::Policy,
        Body::Restart {
            cause,
            account,
            model,
        } => Heard::Restart(Restart {
            cause,
            account,
            model,
        }),
        Body::Want => Heard::Incoming(Incoming::Want),
        Body::Prompt { text, by } => Heard::Incoming(Incoming::Prompt { text, by }),
        Body::Answer {
            ask,
            option,
            text,
            by,
            human_seq,
        } => Heard::Incoming(Incoming::Answer {
            ask,
            option,
            text,
            by,
            human_seq,
        }),
        Body::Touch { ask } => Heard::Incoming(Incoming::Touch { ask }),
        Body::Interrupt => Heard::Incoming(Incoming::Interrupt),
        Body::Set {
            model,
            effort,
            mode,
        } => Heard::Incoming(Incoming::Set {
            model,
            effort,
            mode,
        }),
        Body::Unknown { .. } => Heard::Unknown,
        _ => Heard::Refuse("harness kind"),
    }
}

/// The `k` and `seq` of a line that does not read as a record, found loosely: the string after
/// the first `"k"` key and the decimal after the first `"seq"` key, either `None` when it is not
/// there in that shape.
pub(crate) fn loose(bytes: &[u8]) -> (Option<String>, Option<u64>) {
    let k = after_key(bytes, b"\"k\"").and_then(|rest| {
        let rest = rest.strip_prefix(b"\"")?;
        let end = rest.iter().take(32).position(|byte| *byte == b'"')?;
        std::str::from_utf8(&rest[..end]).ok().map(str::to_string)
    });
    let seq = after_key(bytes, b"\"seq\"").and_then(|rest| {
        let end = rest
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .unwrap_or(rest.len());
        std::str::from_utf8(&rest[..end]).ok()?.parse().ok()
    });
    (k, seq)
}

/// The bytes after `key` and its colon, spaces passed over.
fn after_key<'a>(bytes: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let at = bytes.windows(key.len()).position(|window| window == key)?;
    let rest = bytes[at + key.len()..].trim_ascii_start();
    Some(rest.strip_prefix(b":")?.trim_ascii_start())
}

/// One line read from the stream.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A line within [`LINE_MAX`], without its newline.
    Text(Vec<u8>),
    /// A line over [`LINE_MAX`], passed over to its newline unread.
    Over {
        /// Its length.
        len: usize,
        /// Its first bytes, at most [`HEAD_MAX`].
        head: Vec<u8>,
    },
}

/// What one read pass found.
#[derive(Debug, Default)]
pub(crate) struct Read {
    /// The whole lines, in order.
    pub(crate) lines: Vec<Line>,
    /// Why the stream ended (`closed`, or the read's error), when it did.
    pub(crate) ended: Option<String>,
}

/// The claim's connection after `OK stream`: one record per line each way. Reads never wait; a
/// write waits for room within its bound and reads what aterm sends meanwhile, so neither side
/// can wait on the other for ever.
#[derive(Debug)]
pub(crate) struct Lines {
    stream: UnixStream,
    pending: Vec<u8>,
    /// The bytes at the start of `pending` known to hold no newline.
    scanned: usize,
    /// A line over [`LINE_MAX`] whose newline has not come: the bytes passed over, and its head.
    skipped: Option<(usize, Vec<u8>)>,
    /// Whether `pending` holds a newline: a whole line [`Lines::read`] returns, which the
    /// descriptor does not show.
    whole: bool,
}

impl Lines {
    /// The connection `stream`, with `pending`, the bytes read after the `OK` line.
    pub(crate) fn new(stream: UnixStream, pending: Vec<u8>) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            whole: pending.contains(&b'\n'),
            stream,
            pending,
            scanned: 0,
            skipped: None,
        })
    }

    /// Read what has arrived, without waiting, up to [`DRAIN_BYTES`].
    pub(crate) fn read(&mut self) -> Read {
        let mut read = Read::default();
        let mut received = 0;
        while received < DRAIN_BYTES {
            let mut bytes = [0_u8; 16 * 1024];
            match self.stream.read(&mut bytes) {
                Ok(0) => {
                    read.ended = Some("closed".to_string());
                    break;
                }
                Ok(count) => {
                    received += count;
                    self.take(&bytes[..count], &mut read.lines);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    read.ended = Some(error.to_string());
                    break;
                }
            }
        }
        self.take(&[], &mut read.lines);
        read
    }

    /// Add `bytes` to what is pending and split off every whole line, scanning each byte once.
    fn take(&mut self, bytes: &[u8], lines: &mut Vec<Line>) {
        self.pending.extend_from_slice(bytes);
        let mut from = 0;
        let mut at = self.scanned;
        while let Some(offset) = self.pending[at..].iter().position(|byte| *byte == b'\n') {
            let end = at + offset;
            let line = &self.pending[from..end];
            lines.push(match self.skipped.take() {
                Some((len, head)) => Line::Over {
                    len: len.saturating_add(line.len()),
                    head,
                },
                None if line.len() > LINE_MAX => Line::Over {
                    len: line.len(),
                    head: line[..HEAD_MAX].to_vec(),
                },
                None => Line::Text(line.to_vec()),
            });
            from = end + 1;
            at = from;
        }
        self.pending.drain(..from);
        if self.skipped.is_some() || self.pending.len() > LINE_MAX {
            let head = self.pending[..self.pending.len().min(HEAD_MAX)].to_vec();
            let (len, _) = self.skipped.get_or_insert((0, head));
            *len = len.saturating_add(self.pending.len());
            self.pending.clear();
        }
        self.scanned = self.pending.len();
        self.whole = false;
    }

    /// Whether a whole line waits in what was read, for [`Lines::read`] to return although the
    /// descriptor may not be readable.
    pub(crate) const fn pending_line(&self) -> bool {
        self.whole
    }

    /// Write `line` and its newline within `bound`, reading what aterm sends while the socket
    /// has no room; the lines it read are returned by the next [`Lines::read`].
    pub(crate) fn write_line(&mut self, line: &str, bound: Duration) -> io::Result<()> {
        let until = Instant::now()
            .checked_add(bound)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "write bound overflows"))?;
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        let mut at = 0;
        while at < bytes.len() {
            match self.stream.write(&bytes[at..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "aterm took no record bytes",
                    ));
                }
                Ok(count) => at += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => self.wait(until)?,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Wait until the socket has room or `until`, reading what arrives meanwhile into the
    /// pending bytes while they are under [`PENDING_MAX`].
    fn wait(&mut self, until: Instant) -> io::Result<()> {
        let left = until
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "stream write bound elapsed"))?;
        let reading = self.pending.len() < PENDING_MAX;
        let mut poll = libc::pollfd {
            fd: self.stream.as_raw_fd(),
            events: libc::POLLOUT | if reading { libc::POLLIN } else { 0 },
            revents: 0,
        };
        let millis = libc::c_int::try_from(left.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
        // SAFETY: poll reads and writes one initialized pollfd for a descriptor this owns.
        let ready = unsafe { libc::poll(&raw mut poll, 1, millis) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(())
            } else {
                Err(error)
            };
        }
        if reading && poll.revents & libc::POLLIN != 0 {
            let mut bytes = [0_u8; 16 * 1024];
            let room = bytes.len().min(PENDING_MAX - self.pending.len());
            match self.stream.read(&mut bytes[..room]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "aterm closed the stream",
                    ));
                }
                Ok(count) => {
                    self.whole |= bytes[..count].contains(&b'\n');
                    self.pending.extend_from_slice(&bytes[..count]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// The descriptor, for the caller's wait.
    pub(crate) fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
