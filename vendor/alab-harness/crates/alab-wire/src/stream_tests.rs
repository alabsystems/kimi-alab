// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The outbox's numbers, bounds and pace, the lines' framing and what aterm's records ask, with
//! no aterm: [`crate::link_tests`] drives them through the fake.

use super::*;
use crate::record::State as AgentState;

fn reply(epoch: &str, acked: u64, sent: u64) -> StreamReply {
    StreamReply {
        epoch: epoch.to_string(),
        acked,
        sent,
    }
}

fn state() -> Body {
    Body::State {
        state: AgentState::Idle,
        composer: None,
    }
}

fn claim() -> Body {
    Body::Claim {
        child: None,
        scope: None,
        version: None,
        vendor: None,
        login: None,
    }
}

fn seqs(records: &[Record]) -> Vec<u64> {
    records.iter().map(|record| record.seq).collect()
}

fn bodies(outbox: &Outbox) -> Vec<(u64, Body)> {
    outbox
        .kept()
        .into_iter()
        .map(|record| (record.seq, record.body))
        .collect()
}

/// Write every kept record the pace allows at `now_ms`, as the link does: the lines written.
fn write(outbox: &mut Outbox, now_ms: u64) -> usize {
    let lines = outbox.due(now_ms);
    for _ in &lines {
        outbox.wrote();
    }
    lines.len()
}

/// The caller's records are kept up to [`RESEND_MAX`] and refused past it, unnumbered; the
/// link's own records are kept up to [`OWN_MAX`]; an acknowledgement makes room.
#[test]
fn the_resend_queue_refuses_the_callers_record_past_its_bound() {
    let mut outbox = Outbox::default();
    assert_eq!(outbox.open(&reply("e1", 0, 0), 0).resent, 0);
    for seq in 1..=RESEND_MAX as u64 {
        assert_eq!(outbox.push(state(), 7, false, 0), Ok(seq));
    }
    assert_eq!(outbox.push(state(), 7, false, 0), Err(SendError::Full));
    let beat = outbox
        .push(Body::Beat, 7, true, 0)
        .expect("the link's own record");
    assert_eq!(
        beat,
        RESEND_MAX as u64 + 1,
        "the refused record took no seq"
    );
    write(&mut outbox, 10_000);
    outbox.ack(1, None).expect("written");
    assert_eq!(outbox.push(state(), 7, false, 0), Ok(RESEND_MAX as u64 + 2));
    assert_eq!(outbox.push(state(), 7, false, 0), Err(SendError::Full));
    for _ in 1..OWN_MAX {
        outbox.push(Body::Beat, 7, true, 0).expect("own");
    }
    assert_eq!(outbox.push(Body::Beat, 7, true, 0), Err(SendError::Full));
}

/// [`RESEND_BYTES_MAX`] bounds the caller's bytes as [`RESEND_MAX`] bounds its records.
#[test]
fn the_resend_queue_bounds_the_callers_bytes() {
    let mut outbox = Outbox::default();
    outbox.open(&reply("e1", 0, 0), 0);
    let big = || Body::Diff {
        turn: None,
        path: None,
        patch: "x".repeat(LINE_MAX - 1024),
    };
    let fits = RESEND_BYTES_MAX / (LINE_MAX - 1024 + 64);
    for _ in 0..fits {
        outbox
            .push(big(), 1, false, 0)
            .expect("within the byte bound");
    }
    assert_eq!(outbox.push(big(), 1, false, 0), Err(SendError::Full));
    assert!(
        outbox.push(state(), 1, false, 0).is_ok(),
        "a small record still fits"
    );
}

/// Reopened in the same epoch, every record above `acked=` keeps its number, a beat between the
/// caller's records included; `in=` is where aterm's records resume.
#[test]
fn a_stream_opened_again_keeps_every_number_in_the_same_epoch() {
    let mut outbox = Outbox::default();
    assert!(outbox.open(&reply("e1", 0, 4), 0).fresh, "the first stream");
    for _ in 0..3 {
        outbox.push(state(), 1, false, 0).expect("kept");
    }
    outbox.push(Body::Beat, 1, true, 0).expect("kept");
    outbox.push(state(), 1, false, 0).expect("kept");
    write(&mut outbox, 0);
    assert_eq!(outbox.ack(2, None), Ok(Some(Kind::State)));
    // aterm applied the third before the stream closed; its ack never came.
    let opened = outbox.open(&reply("e1", 3, 9), 0);
    assert_eq!((opened.fresh, opened.resent), (false, 2));
    assert!(opened.renumbered.is_empty());
    assert_eq!(bodies(&outbox), [(4, Body::Beat), (5, state())]);
    assert_eq!(
        outbox.hear(9),
        Ok(false),
        "aterm's records resume after in="
    );
    assert_eq!(outbox.hear(10), Ok(true));
    assert_eq!(outbox.push(state(), 1, false, 0), Ok(6));
    // A claim kept through an exec: the new image's first record follows the old acked=.
    let mut exec = Outbox::default();
    assert!(exec.open(&reply("e2", 57, 3), 0).fresh);
    assert_eq!(exec.push(state(), 1, false, 0), Ok(58));
}

/// A kept `claim` record is written as a beat in its place, superseded by the claim placed
/// before every open; an answer stays in its own `in` space and becomes a beat outside it; a new
/// epoch whose `acked=` carries the count keeps every number.
#[test]
fn a_stream_opened_again_writes_a_beat_for_a_superseded_claim_or_a_stale_answer() {
    let mut outbox = Outbox::default();
    outbox.open(&reply("e1", 0, 0), 0);
    outbox.push(Body::Took { of: 1 }, 1, true, 0).expect("kept");
    outbox.push(state(), 1, false, 0).expect("kept");
    outbox.push(claim(), 1, true, 0).expect("kept");
    assert!(!outbox.open(&reply("e1", 0, 1), 0).fresh);
    assert_eq!(
        bodies(&outbox),
        [(1, Body::Took { of: 1 }), (2, state()), (3, Body::Beat)]
    );
    // A seamless update carries acked=: a new epoch, the same numbers, no answer of the old one.
    let carried = outbox.open(&reply("e2", 0, 0), 0);
    assert!(carried.fresh && carried.renumbered.is_empty());
    assert_eq!(
        bodies(&outbox),
        [(1, Body::Beat), (2, state()), (3, Body::Beat)]
    );
}

/// When aterm's count went back (an aterm restart's new epoch, or one that forgot this claimant
/// at its grace's end in the same epoch) the caller's records are numbered from its `acked=`,
/// the link's own records dropped, and every move reported.
#[test]
fn a_count_that_went_back_renumbers_the_callers_records_and_reports_each() {
    let mut outbox = Outbox::default();
    outbox.open(&reply("e1", 0, 0), 0);
    for _ in 0..3 {
        outbox.push(state(), 1, false, 0).expect("kept");
    }
    write(&mut outbox, 0);
    outbox.ack(3, None).expect("written");
    assert_eq!(outbox.hear(1), Ok(true));
    outbox.push(Body::Took { of: 1 }, 1, true, 0).expect("kept");
    outbox.push(state(), 1, false, 0).expect("kept");
    outbox.push(Body::Beat, 1, true, 0).expect("kept");
    outbox.push(state(), 1, false, 0).expect("kept");
    assert_eq!(outbox.await_answer(1), None);
    // The grace ended in the same epoch: a new claimant, acked=0 in=0.
    let opened = outbox.open(&reply("e1", 0, 0), 0);
    assert!(opened.fresh);
    assert_eq!(opened.renumbered, [(5, 1), (7, 2)]);
    assert_eq!(bodies(&outbox), [(1, state()), (2, state())]);
    assert!(
        !outbox.awaits(1),
        "the old in= named another stream's record"
    );
    assert_eq!(outbox.hear(1), Ok(true), "the new stream's record 1 is new");
    assert_eq!(outbox.push(state(), 1, false, 0), Ok(3));
    assert_eq!(outbox.counts().renumbered, 2);
}

/// Each `dropped=` reason is counted; an acknowledgement stands for every record before it;
/// one of a record never written, or a number past [`SEQ_MAX`], is a fault, never wrapped.
#[test]
fn an_ack_counts_its_reason_and_releases_every_record_up_to_it() {
    let mut outbox = Outbox::default();
    outbox.open(&reply("e1", 0, 0), 0);
    for _ in 0..6 {
        outbox.push(state(), 1, false, 0).expect("kept");
    }
    write(&mut outbox, 0);
    for (seq, dropped) in [
        (1, Dropped::Bound),
        (2, Dropped::Rate),
        (3, Dropped::Scope),
        (4, Dropped::Applied),
        (5, Dropped::Refused("usage".to_string())),
        (6, Dropped::Other("later".to_string())),
    ] {
        outbox.ack(seq, Some(&dropped)).expect("written");
    }
    let counts = outbox.counts();
    assert_eq!(
        (
            counts.bound,
            counts.rate,
            counts.scope,
            counts.applied,
            counts.refused,
            counts.other
        ),
        (1, 1, 1, 1, 1, 1)
    );
    assert!(outbox.kept().is_empty());
    for _ in 0..3 {
        outbox.push(state(), 1, false, 0).expect("kept");
    }
    write(&mut outbox, 0);
    assert_eq!(outbox.ack(8, None), Ok(Some(Kind::State)));
    assert_eq!(seqs(&outbox.kept()), [9]);
    assert_eq!(outbox.ack(10, None), Err(()), "never written");
    assert_eq!(outbox.ack(u64::MAX, None), Err(()));
    assert_eq!(outbox.hear(SEQ_MAX + 1), Err(()));
    assert_eq!(outbox.hear(u64::MAX), Err(()));
    assert_eq!(outbox.push(state(), 1, false, 0), Ok(10));
}

/// Records are written at [`PACE_BURST`] at once and [`PACE_PER_S`] a second after; the rest
/// wait, kept, and the wait's end is known.
#[test]
fn records_are_written_at_the_pace_under_aterms_budget() {
    const { assert!(PACE_BURST < RATE_BURST && PACE_PER_S < RATE_PER_S) };
    const { assert!(RESEND_MAX + OWN_MAX <= PACE_BURST as usize) };
    let mut outbox = Outbox::default();
    outbox.open(&reply("e1", 0, 0), 1_000);
    let fill = |outbox: &mut Outbox| {
        while outbox.push(state(), 1, false, 1_000).is_ok() {}
        while outbox.push(Body::Beat, 1, true, 1_000).is_ok() {}
    };
    fill(&mut outbox);
    let kept = RESEND_MAX + OWN_MAX;
    assert_eq!(
        write(&mut outbox, 1_000),
        kept,
        "everything kept goes at once"
    );
    outbox.ack(kept as u64, None).expect("written");
    fill(&mut outbox);
    let left = PACE_BURST as usize - kept;
    assert_eq!(write(&mut outbox, 1_000), left);
    assert_eq!(write(&mut outbox, 1_000), 0);
    let gap = 1_000 / u64::from(PACE_PER_S);
    assert_eq!(outbox.pace_due_ms(), Some(1_000 + gap));
    assert_eq!(write(&mut outbox, 2_000), PACE_PER_S as usize);
    // A stream that opens again starts with a full pace and writes everything kept again.
    outbox.open(&reply("e1", kept as u64, 0), 2_000);
    assert_eq!(write(&mut outbox, 2_000), kept);
    assert_eq!(outbox.pace_due_ms(), None);
}

/// The link's records are stamped from the caller's wall clock, moved on by its `now_ms`; a
/// `session` record names the kinds the caller takes, which clearing keeps with the counts.
#[test]
fn the_callers_records_set_the_stamp_and_what_it_takes() {
    let mut outbox = Outbox::default();
    assert_eq!(
        outbox.stamp(500),
        0,
        "no wall clock before the caller gives one"
    );
    outbox.set_wall(1_700_000_000_000, 400);
    assert_eq!(outbox.stamp(500), 1_700_000_000_100);
    outbox.open(&reply("e1", 0, 0), 0);
    let session = Body::Session {
        agent: "example".to_string(),
        version: "0.1.0".to_string(),
        vendor: None,
        account: None,
        conversation: None,
        cwd: None,
        resume: Vec::new(),
        mode: Mode::Tui,
        takes: vec![Kind::Prompt],
    };
    outbox
        .push(session, 1_800_000_000_000, false, 1_000)
        .expect("kept");
    assert_eq!(outbox.stamp(1_250), 1_800_000_000_250);
    assert!(outbox.takes(Kind::Prompt) && !outbox.takes(Kind::Set));
    assert_eq!(outbox.clear(), 1, "the session record given up");
    assert!(!outbox.live() && outbox.kept().is_empty() && outbox.takes(Kind::Prompt));
    assert_eq!((outbox.counts().opened, outbox.counts().given_up), (1, 1));
}

/// Records the caller was given await its answer; past [`AWAITED_MAX`] the oldest is to be
/// refused `unanswered`.
#[test]
fn the_callers_answers_are_awaited_and_bounded() {
    let mut outbox = Outbox::default();
    outbox.open(&reply("e1", 0, 0), 0);
    for of in 1..=AWAITED_MAX as u64 {
        assert_eq!(outbox.await_answer(of), None);
    }
    assert_eq!(outbox.await_answer(AWAITED_MAX as u64 + 1), Some(1));
    assert!(!outbox.awaits(1) && outbox.awaits(2));
    outbox.answering(2);
    assert!(!outbox.awaits(2));
    assert_eq!(outbox.counts().unanswered, 1);
}

/// The caller sends the harness's kinds, except those the link sends itself.
#[test]
fn the_caller_sends_only_the_harness_kinds_the_link_does_not_make() {
    assert_eq!(sendable(&state()), Ok(()));
    for body in [
        Body::Beat,
        Body::Took { of: 1 },
        Body::Refused {
            of: 1,
            why: "no".to_string(),
        },
        Body::Want,
        Body::Policy,
        Body::Unknown {
            k: "later".to_string(),
        },
    ] {
        assert_eq!(
            sendable(&body),
            Err(SendError::Kind(body.word().to_string()))
        );
    }
}

/// aterm's records map to what the link does with each; a kind it does not know is ignored.
#[test]
fn each_aterm_kind_maps_to_what_the_link_does() {
    assert_eq!(heard(Body::Policy), Heard::Policy);
    assert_eq!(
        heard(Body::Restart {
            cause: "model".to_string(),
            account: None,
            model: Some("opus".to_string()),
        }),
        Heard::Restart(Restart {
            cause: "model".to_string(),
            account: None,
            model: Some("opus".to_string()),
        })
    );
    assert_eq!(heard(Body::Interrupt), Heard::Incoming(Incoming::Interrupt));
    assert_eq!(heard(Body::Want), Heard::Incoming(Incoming::Want));
    assert_eq!(
        heard(Body::Unknown {
            k: "later".to_string()
        }),
        Heard::Unknown
    );
    assert_eq!(heard(state()), Heard::Refuse("harness kind"));
}

/// A line that does not read gives up its `k` and its decimal `seq` when they are there.
#[test]
fn an_unreadable_line_gives_its_kind_and_seq_loosely() {
    assert_eq!(
        loose(br#"{"k":"want","seq":7,"ms":"x"}"#),
        (Some("want".to_string()), Some(7))
    );
    assert_eq!(
        loose(br#"{ "k" : "ack" , "seq":"3" }"#),
        (Some("ack".to_string()), None)
    );
    assert_eq!(loose(b"not json"), (None, None));
    assert_eq!(loose(br#"{"seq":99999999999999999999999}"#), (None, None));
    let mut outbox = Outbox::default();
    outbox.open(&reply("e1", 0, 4), 0);
    assert_eq!(outbox.unreadable(None), None, "no seq moves nothing");
    assert_eq!(outbox.unreadable(Some(3)), None, "an old seq");
    assert_eq!(outbox.unreadable(Some(SEQ_MAX + 1)), None);
    assert_eq!(outbox.unreadable(Some(6)), Some(6));
    assert_eq!(outbox.hear(5), Ok(false));
    assert_eq!(outbox.counts().unreadable, 4);
}

/// Lines split on their newline; a line over [`LINE_MAX`] is passed over to its newline and
/// reported with its length and head, whether it came in one read or many.
#[test]
fn lines_pass_over_a_line_past_the_bound() {
    let (near, mut far) = UnixStream::pair().expect("a pair");
    let mut lines = Lines::new(near, b"{\"a\":1}\n{\"b\"".to_vec()).expect("lines");
    assert!(lines.pending_line(), "a line came with the OK");
    far.write_all(b":2}\n").expect("write");
    let read = lines.read();
    assert_eq!(
        read.lines,
        [
            Line::Text(b"{\"a\":1}".to_vec()),
            Line::Text(b"{\"b\":2}".to_vec())
        ]
    );
    assert!(read.ended.is_none() && !lines.pending_line());
    let mut long = br#"{"k":"want","seq":4,"x":""#.to_vec();
    long.resize(LINE_MAX + 10, b'x');
    let head = long[..HEAD_MAX].to_vec();
    let writer = std::thread::spawn(move || {
        far.write_all(&long).expect("write");
        far.write_all(b"\nok\n").expect("write");
        far
    });
    let mut got = Vec::new();
    while got.len() < 2 {
        got.extend(lines.read().lines);
    }
    let far = writer.join().expect("the writer");
    assert_eq!(
        got,
        [
            Line::Over {
                len: LINE_MAX + 10,
                head
            },
            Line::Text(b"ok".to_vec())
        ]
    );
    drop(far);
    let mut ended = None;
    while ended.is_none() {
        ended = lines.read().ended;
    }
    assert_eq!(ended.as_deref(), Some("closed"));
}

/// Splitting scans each byte once: a megabyte of short lines splits in one pass.
#[test]
fn lines_split_a_megabyte_of_short_lines_in_one_pass() {
    let (near, _far) = UnixStream::pair().expect("a pair");
    let mut many = Vec::new();
    while many.len() < 1024 * 1024 {
        many.extend_from_slice(b"{\"k\":\"ack\",\"seq\":1}\n");
    }
    let count = many.len() / 20;
    let mut lines = Lines::new(near, many).expect("lines");
    let started = Instant::now();
    let read = lines.read();
    assert_eq!(read.lines.len(), count);
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "{:?}",
        started.elapsed()
    );
}

/// A write that finds no room reads what aterm sends meanwhile, so two sides that both write
/// first do not wait on each other; the lines it read come with the next read, and they show as
/// pending, since the descriptor no longer does.
#[test]
fn a_write_reads_while_it_waits_for_room() {
    let (near, far) = UnixStream::pair().expect("a pair");
    let mut lines = Lines::new(near, Vec::new()).expect("lines");
    let line = "y".repeat(64 * 1024);
    let echo = std::thread::spawn(move || {
        use std::io::{BufRead as _, BufReader};
        let mut writer = far.try_clone().expect("a writer");
        let mut reader = BufReader::new(far);
        // Write an answer larger than the socket's buffer before reading each line, so the
        // two sides wait on each other unless the write reads.
        let mut answer = vec![b'a'; 16 * 1024];
        answer.push(b'\n');
        for _ in 0..32 {
            writer.write_all(&answer).expect("answer");
            let mut text = String::new();
            reader.read_line(&mut text).expect("a line");
        }
    });
    let mut pending = false;
    for _ in 0..32 {
        lines
            .write_line(&line, Duration::from_secs(20))
            .expect("written");
        pending |= lines.pending_line();
    }
    echo.join().expect("the echo");
    assert!(pending, "a write read whole lines");
    let mut acks = 0;
    loop {
        let read = lines.read();
        acks += read.lines.len();
        if read.ended.is_some() {
            break;
        }
    }
    assert_eq!(acks, 32);
}
