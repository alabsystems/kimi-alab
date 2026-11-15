// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The record vocabulary against the design's tables, and the codec against every line it can
//! be handed.

use std::path::PathBuf;
use std::time::Duration;

use super::*;

/// The design's "Harness to aterm" table, in its order, `took` and `refused` from its line rules.
const HARNESS_TO_ATERM: [&str; 26] = [
    "session", "claim", "state", "beat", "turn", "msg", "tool", "diff", "subagent", "title",
    "progress", "goal", "compact", "ask", "asked", "usage", "account", "wall", "model", "act",
    "need", "clear", "land", "upgrade", "took", "refused",
];

/// The design's "aterm to harness" table, in its order, `ack` from its line rules.
const ATERM_TO_HARNESS: [&str; 10] = [
    "ack",
    "prompt",
    "answer",
    "touch",
    "hand",
    "interrupt",
    "set",
    "restart",
    "policy",
    "want",
];

/// Text that needs every kind of escape: a quote, a backslash, controls, C1, bidi, zero-width,
/// separators, a noncharacter and a tag character, beside text that needs none.
const AWKWARD: &str = concat!(
    "say \"hi\" \\ a\tb\nc\r\u{0}\u{1b}[31m\u{7f}\u{85}",
    " \u{200b}\u{202e}evil\u{2028}\u{2029}\u{feff}\u{fffe}\u{e0041} é 日本 😀 a\u{200d}b /",
);

fn s(text: &str) -> String {
    text.to_string()
}

fn record(body: Body) -> Record {
    Record {
        seq: 42,
        ms: 1_794_500_000_000,
        body,
    }
}

/// A record of `kind` with every field filled, its text awkward.
fn full(kind: Kind) -> Body {
    let text = || s(AWKWARD);
    match kind {
        Kind::Session => Body::Session {
            agent: s("claude"),
            version: s("0.4.0+dev.3"),
            vendor: Some(s("2.1.325")),
            account: Some(s("ac-7f3e")),
            conversation: Some(s("4f1c2a9e-0d6b-4a1e-9c3f-5b7e8d2a1c00")),
            cwd: Some(s("/Users/_owner/a dir/\u{202e}x")),
            resume: vec![s("--resume"), s("4f1c2a9e"), text()],
            mode: Mode::Tui,
            takes: vec![
                Kind::Prompt,
                Kind::Answer,
                Kind::Touch,
                Kind::Interrupt,
                Kind::Set,
            ],
        },
        Kind::Claim => Body::Claim {
            child: Some(ChildProcess {
                pid: 4242,
                start: 1_794_499_999_123_456,
            }),
            scope: Some(vec![Scope::State, Scope::Walls, Scope::Relaunch]),
            version: Some(s("0.4.0")),
            vendor: Some(s("2.1.325+build_7")),
            login: Some(s("work.2")),
        },
        Kind::State => Body::State {
            state: State::Asking,
            composer: Some(Composer::Draft),
        },
        Kind::Beat => Body::Beat,
        Kind::Turn => Body::Turn {
            turn: 3,
            state: TurnState::Started,
            by: Some(By::Peer(s("s-1f2e"))),
        },
        Kind::Msg => Body::Msg {
            turn: Some(3),
            by: By::Agent,
            text: text(),
        },
        Kind::Tool => Body::Tool {
            turn: Some(3),
            id: s("toolu_01ABC"),
            name: s("Bash"),
            state: Step::Interrupted,
            text: Some(text()),
        },
        Kind::Diff => Body::Diff {
            turn: Some(3),
            path: Some(s("src/a b.rs")),
            patch: s("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-\"old\"\n+\t\\new\n"),
        },
        Kind::Subagent => Body::Subagent {
            id: s("agent-7"),
            title: Some(text()),
            state: Step::Done,
        },
        Kind::Title => Body::Title {
            title: Some(text()),
            summary: Some(s("one line")),
        },
        Kind::Progress => Body::Progress {
            items: vec![
                Item {
                    text: s("read"),
                    state: ItemState::Done,
                },
                Item {
                    text: text(),
                    state: ItemState::Active,
                },
                Item {
                    text: s("land"),
                    state: ItemState::Pending,
                },
            ],
        },
        Kind::Goal => Body::Goal { text: Some(text()) },
        Kind::Compact => Body::Compact {
            state: CompactState::Done,
            context_left: Some(100),
        },
        Kind::Ask => Body::Ask {
            kind: AskKind::Approval,
            title: Some(text()),
            options: vec![s("Yes"), s("Yes, and don't ask again"), text()],
            will: Some(s("Yes")),
            will_in_ms: Some(30_000),
        },
        Kind::Asked => Body::Asked {
            ask: 41,
            kind: AskKind::Question,
            by: By::Person,
            reply: Reply::Answer,
            option: Some(s("2")),
            text: Some(text()),
        },
        Kind::Usage => Body::Usage {
            account: s("ac-7f3e"),
            limits: vec![
                Limit {
                    name: s("session"),
                    used: Some(87),
                    resets_ms: Some(1_794_510_000_000),
                },
                Limit {
                    name: s("weekly"),
                    used: Some(0),
                    resets_ms: None,
                },
            ],
            context_left: Some(0),
        },
        Kind::Account => Body::Account {
            account: s("ac-7f3e"),
            state: s("signed-out"),
        },
        Kind::Wall => Body::Wall {
            kind: s("usage-weekly"),
            resets_ms: Some(1_795_000_000_000),
        },
        Kind::Model => Body::Model {
            model: Some(s("Opus 5.5")),
            effort: Some(s("high")),
            key: Some(false),
            offers: vec![
                Offer {
                    model: s("opus[1m]"),
                    levels: vec![s("low"), s("medium"), s("high")],
                },
                Offer {
                    model: s("sonnet"),
                    levels: Vec::new(),
                },
            ],
            permission: Some(s("plan")),
        },
        Kind::Act => Body::Act {
            did: Did::Rule {
                act: s("approve"),
                rule: s("approve-all@v1"),
            },
        },
        Kind::Need => Body::Need {
            kind: NeedKind::Help,
            entry: text(),
            needs: vec![s("approval:bash"), text()],
        },
        Kind::Clear => Body::Clear { need: 40 },
        Kind::Land => Body::Land {
            state: Landing::Held(LandHold {
                since_ms: 1_794_400_000_000,
                nudges: 2,
                read: Some(LandRead {
                    legs: vec![s("repo"), s("open"), s("left")],
                    open: vec![s("tests"), s("docs")],
                    changed: Some(3),
                    untracked: Some(0),
                    stashes: Some(1),
                    unpushed: Some(2),
                    unlanded: Some(4),
                    left: Some(1),
                }),
            }),
        },
        Kind::Upgrade => Body::Upgrade {
            phase: UpgradePhase::Abandoned,
            to: s("2.1.326"),
        },
        Kind::Took => Body::Took { of: 9 },
        Kind::Refused => Body::Refused { of: 9, why: text() },
        Kind::Ack => Body::Ack {
            dropped: Some(Dropped::Refused(s("usage"))),
        },
        Kind::Prompt => Body::Prompt {
            text: text(),
            by: By::Ctl,
        },
        Kind::Answer => Body::Answer {
            ask: 41,
            option: Some(s("Yes")),
            text: None,
            by: By::Person,
            human_seq: 9_001,
        },
        Kind::Touch => Body::Touch { ask: 41 },
        Kind::Hand => Body::Hand {
            human_seq: u64::MAX,
            composer: Composer::Draft,
            hold: true,
        },
        Kind::Interrupt => Body::Interrupt,
        Kind::Set => Body::Set {
            model: Some(s("us.anthropic.claude-x:0")),
            effort: Some(s("max")),
            mode: Some(Mode::Headless),
        },
        Kind::Restart => Body::Restart {
            cause: s("account"),
            account: Some(Account {
                label: s("work"),
                home: Some(PathBuf::from("/Users/_owner/.claude homes/work")),
            }),
            model: Some(s("opus[1m]")),
        },
        Kind::Policy => Body::Policy,
        Kind::Want => Body::Want,
    }
}

/// A record of `kind` with its required fields only.
fn bare(kind: Kind) -> Body {
    match kind {
        Kind::Session => Body::Session {
            agent: s("agy"),
            version: s("1.0.0"),
            vendor: None,
            account: None,
            conversation: None,
            cwd: None,
            resume: Vec::new(),
            mode: Mode::Headless,
            takes: Vec::new(),
        },
        Kind::Claim => Body::Claim {
            child: None,
            scope: None,
            version: None,
            vendor: None,
            login: None,
        },
        Kind::State => Body::State {
            state: State::Ended,
            composer: None,
        },
        Kind::Turn => Body::Turn {
            turn: 1,
            state: TurnState::Done,
            by: None,
        },
        Kind::Msg => Body::Msg {
            turn: None,
            by: By::Harness,
            text: String::new(),
        },
        Kind::Tool => Body::Tool {
            turn: None,
            id: s("1"),
            name: s("Read"),
            state: Step::Started,
            text: None,
        },
        Kind::Diff => Body::Diff {
            turn: None,
            path: None,
            patch: String::new(),
        },
        Kind::Subagent => Body::Subagent {
            id: s("a"),
            title: None,
            state: Step::Started,
        },
        Kind::Title => Body::Title {
            title: None,
            summary: None,
        },
        Kind::Progress => Body::Progress { items: Vec::new() },
        Kind::Goal => Body::Goal { text: None },
        Kind::Compact => Body::Compact {
            state: CompactState::Started,
            context_left: None,
        },
        Kind::Ask => Body::Ask {
            kind: AskKind::Question,
            title: None,
            options: Vec::new(),
            will: None,
            will_in_ms: None,
        },
        Kind::Asked => Body::Asked {
            ask: 1,
            kind: AskKind::Approval,
            by: By::Harness,
            reply: Reply::Deny,
            option: None,
            text: None,
        },
        Kind::Usage => Body::Usage {
            account: s("work"),
            limits: Vec::new(),
            context_left: None,
        },
        Kind::Account => Body::Account {
            account: s("work"),
            state: s("ready"),
        },
        Kind::Wall => Body::Wall {
            kind: s("context"),
            resets_ms: None,
        },
        Kind::Model => Body::Model {
            model: None,
            effort: None,
            key: None,
            offers: Vec::new(),
            permission: None,
        },
        Kind::Act => Body::Act {
            did: Did::Held { held: None },
        },
        Kind::Need => Body::Need {
            kind: NeedKind::Review,
            entry: s("needs you"),
            needs: vec![s("needs you")],
        },
        Kind::Land => Body::Land {
            state: Landing::Open,
        },
        Kind::Ack => Body::Ack { dropped: None },
        Kind::Answer => Body::Answer {
            ask: 1,
            option: None,
            text: Some(String::new()),
            by: By::Peer(s("s-ab")),
            human_seq: 0,
        },
        Kind::Hand => Body::Hand {
            human_seq: 0,
            composer: Composer::Empty,
            hold: false,
        },
        Kind::Set => Body::Set {
            model: None,
            effort: None,
            mode: None,
        },
        Kind::Restart => Body::Restart {
            cause: s("stall"),
            account: None,
            model: None,
        },
        other => full(other),
    }
}

/// `record` written, then read back: equal, with nothing unknown.
fn round_trip(record: &Record) -> String {
    let line = record
        .line()
        .unwrap_or_else(|error| panic!("{record:?} writes: {error}"));
    let decoded =
        Record::read(line.as_bytes()).unwrap_or_else(|error| panic!("{line} reads: {error}"));
    assert_eq!(decoded.record, *record, "{line}");
    assert_eq!(decoded.unknown, 0, "{line}");
    line
}

fn read(line: &str) -> Result<Decoded, LineError> {
    Record::read(line.as_bytes())
}

fn body(line: &str) -> Body {
    read(line)
        .unwrap_or_else(|error| panic!("{line} reads: {error}"))
        .record
        .body
}

fn unknown(line: &str) -> u32 {
    read(line)
        .unwrap_or_else(|error| panic!("{line} reads: {error}"))
        .unknown
}

#[test]
fn every_kind_in_the_design_tables_round_trips() {
    let words: Vec<&str> = Kind::ALL.iter().map(|kind| kind.word()).collect();
    let design: Vec<&str> = HARNESS_TO_ATERM
        .into_iter()
        .chain(ATERM_TO_HARNESS)
        .collect();
    assert_eq!(
        words, design,
        "one kind per row of the design's tables, in their order"
    );
    for kind in Kind::ALL {
        assert_eq!(Kind::from_word(kind.word()), Some(*kind));
        let side = if HARNESS_TO_ATERM.contains(&kind.word()) {
            Side::Harness
        } else {
            Side::Aterm
        };
        assert_eq!(kind.sent_by(), side, "{kind:?}");
        for made in [full(*kind), bare(*kind)] {
            assert_eq!(made.kind(), Some(*kind));
            assert_eq!(made.word(), kind.word());
            let line = round_trip(&record(made));
            let head = format!(
                "{{\"k\":\"{}\",\"seq\":42,\"ms\":1794500000000",
                kind.word()
            );
            assert!(line.starts_with(&head), "k, seq and ms first: {line}");
            assert!(matches!(line.as_bytes()[head.len()], b',' | b'}'), "{line}");
        }
    }
    for (seq, ms) in [(1, 0), (u64::MAX, u64::MAX)] {
        round_trip(&Record {
            seq,
            ms,
            body: Body::Beat,
        });
    }
    round_trip(&record(Body::Unknown { k: s("later") }));

    // The design's own example is a line, read and written exactly.
    let example = r#"{"k":"state","seq":7,"ms":1794500000000,"state":"asking","composer":"empty"}"#;
    let decoded = read(example).expect("the design's example reads");
    let state = Body::State {
        state: State::Asking,
        composer: Some(Composer::Empty),
    };
    assert_eq!(
        decoded,
        Decoded {
            record: Record {
                seq: 7,
                ms: 1_794_500_000_000,
                body: state,
            },
            unknown: 0,
        }
    );
    assert_eq!(decoded.record.line().as_deref(), Ok(example));

    // Fields may come in any order and with JSON's whitespace; `null` is absent.
    let loose = " {\"composer\" : null ,\t\"state\":\"idle\",\"ms\":0,\"seq\":1,\"k\":\"state\"}\r";
    assert_eq!(
        body(loose),
        Body::State {
            state: State::Idle,
            composer: None
        }
    );

    // An ack's `seq` is the harness record it acknowledges.
    let ack = Record {
        seq: 17,
        ms: 5,
        body: Body::Ack {
            dropped: Some(Dropped::Scope),
        },
    };
    assert_eq!(
        ack.line().as_deref(),
        Ok(r#"{"k":"ack","seq":17,"ms":5,"dropped":"scope"}"#)
    );
    for dropped in [
        Dropped::Bound,
        Dropped::Rate,
        Dropped::Scope,
        Dropped::Applied,
        Dropped::Refused(s("busy")),
        Dropped::Other(s("later")),
    ] {
        round_trip(&record(Body::Ack {
            dropped: Some(dropped),
        }));
    }
    for by in [
        By::Person,
        By::Ctl,
        By::Harness,
        By::Agent,
        By::Peer(s("person")),
    ] {
        if by.outside() {
            round_trip(&record(Body::Prompt {
                text: s("go on"),
                by: by.clone(),
            }));
        }
        round_trip(&record(Body::Msg {
            turn: None,
            by,
            text: s("go on"),
        }));
    }
    assert_eq!(
        record(Body::Hand {
            human_seq: 3,
            composer: Composer::Empty,
            hold: true,
        })
        .line()
        .as_deref(),
        Ok(r#"{"k":"hand","seq":42,"ms":1794500000000,"human_seq":3,"composer":"empty","hold":1}"#)
    );
    assert_eq!(
        body(r#"{"k":"took","seq":3,"ms":0,"in":9}"#),
        Body::Took { of: 9 }
    );
    // A narrowed scope that names nothing is not an absent one.
    round_trip(&record(Body::Claim {
        child: None,
        scope: Some(Vec::new()),
        version: None,
        vendor: None,
        login: None,
    }));
    for state in State::ALL {
        round_trip(&record(Body::State {
            state: *state,
            composer: None,
        }));
    }
    for phase in UpgradePhase::ALL {
        round_trip(&record(Body::Upgrade {
            phase: *phase,
            to: s("1.0"),
        }));
    }
    for state in [
        Landing::Held(LandHold {
            since_ms: 0,
            nudges: 0,
            read: None,
        }),
        Landing::Held(LandHold {
            since_ms: 1,
            nudges: 9,
            read: Some(LandRead {
                legs: vec![s("defense")],
                open: Vec::new(),
                changed: None,
                untracked: None,
                stashes: None,
                unpushed: None,
                unlanded: None,
                left: None,
            }),
        }),
        Landing::Landed {
            commit: s("03ebc77a1b2c"),
        },
        Landing::Open,
    ] {
        round_trip(&record(Body::Land { state }));
    }
    assert_eq!(
        LandState::ALL,
        [LandState::Held, LandState::Landed, LandState::Open]
    );
    let mut acts = vec![
        Did::Held { held: None },
        Did::Wait {
            reason: WaitReason::Waiting,
            wait_ms: None,
        },
    ];
    acts.extend(
        HeldBy::ALL
            .iter()
            .map(|held| Did::Held { held: Some(*held) }),
    );
    acts.extend(WaitReason::ALL.iter().map(|reason| Did::Wait {
        reason: *reason,
        wait_ms: Some(12_000),
    }));
    for did in acts {
        round_trip(&record(Body::Act { did }));
    }
    for state in TurnState::ALL {
        round_trip(&record(Body::Turn {
            turn: 2,
            state: *state,
            by: Some(By::Person),
        }));
    }
    for state in Step::ALL {
        round_trip(&record(Body::Subagent {
            id: s("a"),
            title: None,
            state: *state,
        }));
    }
    for state in CompactState::ALL {
        round_trip(&record(Body::Compact {
            state: *state,
            context_left: None,
        }));
    }
    for kind in NeedKind::ALL {
        round_trip(&record(Body::Need {
            kind: *kind,
            entry: s("x"),
            needs: vec![s("y"); NEEDS_MAX],
        }));
    }
    // A move to the vendor's default home is `-`.
    let default_home = record(Body::Restart {
        cause: s("account-ahead"),
        account: Some(Account {
            label: s("ac-7f3e"),
            home: None,
        }),
        model: None,
    });
    let line = round_trip(&default_home);
    assert!(
        line.ends_with(r#""cause":"account-ahead","to":"ac-7f3e","home":"-"}"#),
        "{line}"
    );
    for reply in Reply::ALL {
        for kind in AskKind::ALL {
            round_trip(&record(Body::Asked {
                ask: 2,
                kind: *kind,
                by: By::Person,
                reply: *reply,
                option: None,
                text: None,
            }));
        }
    }
    for state in ItemState::ALL {
        round_trip(&record(Body::Progress {
            items: vec![Item {
                text: s("x"),
                state: *state,
            }],
        }));
    }
}

#[test]
fn an_unknown_kind_or_field_is_counted_never_refused() {
    let future = read(r#"{"k":"telepathy","seq":3,"ms":1,"x":{"deep":[1,2.5e-3,true]},"x":null}"#)
        .expect("an unknown kind reads");
    assert_eq!(future.record.body, Body::Unknown { k: s("telepathy") });
    assert_eq!(future.record.body.kind(), None);
    assert_eq!(future.record.body.word(), "telepathy");
    assert_eq!(future.unknown, 0, "the kind is counted, not its fields");
    assert!(
        future.record.body.admitted(&[], act_table),
        "counted, not refused for scope"
    );
    // Any word is a kind word this library may not know; a `k` that is not a word is refused.
    for k in ["STATE", "state-later", "x@v2", &"k".repeat(WORD_MAX)] {
        let line = format!(r#"{{"k":"{k}","seq":1,"ms":1}}"#);
        assert_eq!(body(&line), Body::Unknown { k: s(k) }, "{line}");
    }

    let line = concat!(
        r#"{"k":"state","seq":1,"ms":2,"state":"idle","mood":"good","#,
        r#""n":123456789012345678901234567890e999,"deep":[[{"a":null}]]}"#,
    );
    assert_eq!(
        body(line),
        Body::State {
            state: State::Idle,
            composer: None
        }
    );
    assert_eq!(unknown(line), 3);
    // An unknown field named twice is counted twice; a known one is refused.
    assert_eq!(
        unknown(r#"{"k":"beat","seq":1,"ms":2,"later":1,"later":2}"#),
        2
    );
    // Unknown fields inside a known field's objects are counted too.
    let nested = concat!(
        r#"{"k":"progress","seq":1,"ms":2,"items":[{"text":"a","state":"done","#,
        r#""colour":"red"},{"text":"b","state":"pending","due":1,"by":2}]}"#,
    );
    assert_eq!(unknown(nested), 3);
    let limits = concat!(
        r#"{"k":"usage","seq":1,"ms":2,"account":"work","#,
        r#""limits":[{"name":"weekly","used":5,"unit":"%"}]}"#,
    );
    assert_eq!(unknown(limits), 1);
    let offers = r#"{"k":"model","seq":1,"ms":2,"offers":[{"model":"x","levels":["a"],"cost":1}]}"#;
    assert_eq!(unknown(offers), 1);
    let child = r#"{"k":"claim","seq":1,"ms":2,"child":{"pid":4242,"start":7,"gen":1}}"#;
    assert_eq!(unknown(child), 1);
    // A scope word this library does not know is dropped and counted, as aterm drops it; one
    // of only such words is a narrowing to no duty this library knows.
    let scope = r#"{"k":"claim","seq":1,"ms":2,"scope":["telepathy","state","walls-later"]}"#;
    let claim = |scope: Vec<Scope>| Body::Claim {
        child: None,
        scope: Some(scope),
        version: None,
        vendor: None,
        login: None,
    };
    assert_eq!(body(scope), claim(vec![Scope::State]));
    assert_eq!(unknown(scope), 2);
    let later = r#"{"k":"claim","seq":1,"ms":2,"scope":["telepathy"]}"#;
    assert_eq!(body(later), claim(Vec::new()));
    assert_eq!(unknown(later), 1);
    // An act names no scope of its own: one sent is a field this library does not know, and the
    // act is admitted by its rule alone.
    let act =
        r#"{"k":"act","seq":1,"ms":2,"act":"approve","rule":"approve-all@v1","scope":"state"}"#;
    assert_eq!(unknown(act), 1);
    assert!(!body(act).admitted(&[Scope::State], act_table));
    assert!(body(act).admitted(&[Scope::Approvals], act_table));
    // A kind aterm does not send, or does not have yet, is dropped from `takes` and counted.
    let takes = concat!(
        r#"{"k":"session","seq":1,"ms":2,"agent":"muse","version":"1.0.0","#,
        r#""mode":"tui","takes":["prompt","state","teleport"]}"#,
    );
    let Body::Session { takes: taken, .. } = body(takes) else {
        panic!("a session");
    };
    assert_eq!(taken, vec![Kind::Prompt]);
    assert_eq!(unknown(takes), 2);
    // A dropped word this library does not know is still a drop.
    assert_eq!(
        body(r#"{"k":"ack","seq":1,"ms":2,"dropped":"malformed"}"#),
        Body::Ack {
            dropped: Some(Dropped::Other(s("malformed")))
        }
    );
}

/// `text` as a JSON string, written as the codec writes text.
fn record_text(text: &str) -> String {
    let line = record(Body::Goal {
        text: Some(s(text)),
    })
    .line()
    .expect("a goal writes");
    let start = line.find(r#""text":"#).expect("a text field") + r#""text":"#.len();
    line[start..line.len() - 1].to_string()
}

#[test]
fn a_line_over_the_bound_is_refused_before_it_is_parsed() {
    assert_eq!(LINE_MAX, 256 * 1024);
    let head = r#"{"k":"msg","seq":1,"ms":0,"by":"agent","text":""#;
    let fill = LINE_MAX - head.len() - 2;
    let at_bound = format!("{head}{}\"}}", "x".repeat(fill));
    assert_eq!(at_bound.len(), LINE_MAX);
    let Body::Msg { text, .. } = body(&at_bound) else {
        panic!("a msg");
    };
    assert_eq!(text.len(), fill);

    let over = format!("{head}{}\"}}", "x".repeat(fill + 1));
    assert_eq!(read(&over), Err(LineError::Bound(LINE_MAX + 1)));
    // Not parsed: bytes that are neither UTF-8 nor JSON are refused for their length alone.
    let garbage = vec![0xff_u8; LINE_MAX + 1];
    assert_eq!(Record::read(&garbage), Err(LineError::Bound(LINE_MAX + 1)));
    assert_eq!(
        Record::read(&vec![b'['; 4 * LINE_MAX]),
        Err(LineError::Bound(4 * LINE_MAX))
    );
    assert_eq!(Record::read(&garbage[..LINE_MAX]), Err(LineError::Utf8));

    // The writer refuses what the reader would.
    let too_long = record(Body::Prompt {
        text: "x".repeat(LINE_MAX),
        by: By::Person,
    });
    assert!(matches!(too_long.line(), Err(LineError::Bound(len)) if len > LINE_MAX));
    // Escapes count: a text within the bound raw can be over it written.
    let hidden = record(Body::Prompt {
        text: "\u{1}".repeat(LINE_MAX / 4),
        by: By::Person,
    });
    assert!(matches!(hidden.line(), Err(LineError::Bound(len)) if len > LINE_MAX));
    // The writer stops where the bound is passed: what is refused costs no more than the bound,
    // however long the text or the list.
    let stops = |body: Body| match record(body).line() {
        Err(LineError::Bound(len)) => {
            assert!((LINE_MAX + 1..=LINE_MAX + 16).contains(&len), "{len}");
        }
        other => panic!("{other:?}"),
    };
    stops(Body::Diff {
        turn: None,
        path: None,
        patch: "\u{1}".repeat(4 * LINE_MAX),
    });
    stops(Body::Diff {
        turn: None,
        path: Some("\u{e0041}".repeat(LINE_MAX)),
        patch: String::new(),
    });
    let Body::Session { agent, version, .. } = bare(Kind::Session) else {
        panic!("a session");
    };
    stops(Body::Session {
        agent,
        version,
        vendor: None,
        account: None,
        conversation: None,
        cwd: None,
        resume: vec![String::new(); LINE_MAX],
        mode: Mode::Tui,
        takes: Vec::new(),
    });

    // The bound counts every byte passed, a `\r` before the newline among them: a stream's
    // reader passes the bytes before the newline, and nothing more is taken off.
    let head = r#"{"k":"goal","seq":1,"ms":0,"text":""#;
    let full = format!("{head}{}\"}}", "x".repeat(LINE_MAX - head.len() - 2));
    assert_eq!(full.len(), LINE_MAX);
    assert!(read(&full).is_ok());
    assert_eq!(
        read(&format!("{full}\r")),
        Err(LineError::Bound(LINE_MAX + 1))
    );
    let short = format!("{head}{}\"}}\r", "x".repeat(LINE_MAX - head.len() - 3));
    assert!(read(&short).is_ok(), "a \\r within the bound is whitespace");
}

#[test]
fn a_line_that_reads_writes_back_within_the_bound() {
    // A string holds no raw character the writer would escape, so no escape the writer makes is
    // longer than what it read: a line at the bound writes back at or under it, equal.
    let head = r#"{"k":"goal","seq":1,"ms":0,"text":""#;
    for escape in [
        r"\u007f", r"\u0085", r"\u2028", r"\u202e", r"\ufffe", r"\u00e9", r"\/",
    ] {
        let count = (LINE_MAX - head.len() - 2) / escape.len();
        let line = format!("{head}{}\"}}", escape.repeat(count));
        let decoded = read(&line).unwrap_or_else(|error| panic!("{escape}: {error}"));
        let written = decoded
            .record
            .line()
            .unwrap_or_else(|error| panic!("{escape}: {error}"));
        assert!(written.len() <= line.len(), "{escape}");
        assert_eq!(read(&written).map(|again| again.record), Ok(decoded.record));
    }
    let pair = r"\udb40\udc41";
    let line = format!(
        "{head}{}\"}}",
        pair.repeat((LINE_MAX - head.len() - 2) / pair.len())
    );
    let decoded = read(&line).expect("escaped tag characters read");
    assert_eq!(decoded.record.line().as_deref(), Ok(line.as_str()));

    // The same characters raw are refused where they stand, at the bound as anywhere.
    for raw in [
        '\u{7f}',
        '\u{85}',
        '\u{2028}',
        '\u{202e}',
        '\u{fffe}',
        '\u{200b}',
        '\u{e0041}',
    ] {
        let count = (LINE_MAX - head.len() - 2) / raw.len_utf8();
        let line = format!("{head}{}\"}}", raw.to_string().repeat(count));
        assert!(line.len() <= LINE_MAX);
        assert_eq!(read(&line), Err(LineError::Syntax(head.len())), "{raw:?}");
    }
}

/// aterm's act table as a test holds it: two rules and the scopes `acts::act_scope` names for
/// them; any other rule names none.
fn act_table(rule: &str) -> Option<Scope> {
    match rule {
        "approve-all@v1" => Some(Scope::Approvals),
        "continue@v1" => Some(Scope::TurnEnd),
        _ => None,
    }
}

#[test]
fn each_kind_needs_the_scope_the_design_table_names() {
    // The design's scope table, row by row; `took`, `refused` and aterm's own kinds need
    // nothing beyond the stream, and `title`, which the table leaves out, needs nothing more
    // either: a title is a description claim, not a duty.
    let table: [(&[&str], Needs); 7] = [
        (&["ask", "asked"], Needs::AskKind),
        (&["wall", "usage"], Needs::Scope(Scope::Walls)),
        (&["account"], Needs::Scope(Scope::Accounts)),
        (
            &[
                "state", "turn", "tool", "subagent", "msg", "diff", "progress", "goal", "compact",
            ],
            Needs::Scope(Scope::State),
        ),
        (&["act", "need", "clear", "land"], Needs::ActScope),
        (
            &[
                "session", "claim", "model", "upgrade", "beat", "title", "took", "refused",
            ],
            Needs::Stream,
        ),
        (&ATERM_TO_HARNESS, Needs::Stream),
    ];
    let mut seen = Vec::new();
    for (words, needs) in table {
        for word in words {
            let kind = Kind::from_word(word).expect("a design kind");
            assert_eq!(kind.needs(), needs, "{word}");
            seen.push(kind);
        }
    }
    seen.sort_unstable();
    assert_eq!(seen, Kind::ALL, "every kind is in exactly one row");

    let others = |scopes: &[Scope]| -> Vec<Scope> {
        Scope::ALL
            .into_iter()
            .filter(|scope| !scopes.contains(scope))
            .collect()
    };
    // A record is admitted by its own ask kind.
    let ask = |kind| Body::Ask {
        kind,
        title: None,
        options: Vec::new(),
        will: None,
        will_in_ms: None,
    };
    assert!(ask(AskKind::Approval).admitted(&[Scope::Approvals], act_table));
    assert!(!ask(AskKind::Approval).admitted(&others(&[Scope::Approvals]), act_table));
    assert!(ask(AskKind::Question).admitted(&[Scope::Questions], act_table));
    assert!(!ask(AskKind::Question).admitted(&others(&[Scope::Questions]), act_table));
    let asked = |kind| Body::Asked {
        ask: 1,
        kind,
        by: By::Person,
        reply: Reply::Approve,
        option: None,
        text: None,
    };
    assert!(asked(AskKind::Approval).admitted(&[Scope::Approvals], act_table));
    assert!(!asked(AskKind::Question).admitted(&[Scope::Approvals], act_table));

    // An act is admitted by the act it replaces: a rule's by aterm's act table, never by a
    // scope the record names, `wait` by `state` and `held` by `turn-end`.
    let rule = |rule: &str| Body::Act {
        did: Did::Rule {
            act: s("approve"),
            rule: s(rule),
        },
    };
    for (body, scope) in [
        (rule("approve-all@v1"), Scope::Approvals),
        (rule("continue@v1"), Scope::TurnEnd),
        (
            Body::Act {
                did: Did::Wait {
                    reason: WaitReason::Composing,
                    wait_ms: None,
                },
            },
            Scope::State,
        ),
        (
            Body::Act {
                did: Did::Held {
                    held: Some(HeldBy::NoWork),
                },
            },
            Scope::TurnEnd,
        ),
    ] {
        assert!(body.admitted(&[scope], act_table), "{body:?}");
        assert!(!body.admitted(&others(&[scope]), act_table), "{body:?}");
    }
    assert!(
        !rule("telepathy@v1").admitted(&Scope::ALL, act_table),
        "a rule the act table does not name is admitted by no grant"
    );
    for needs in [full(Kind::Need), full(Kind::Clear)] {
        for scope in [Scope::Approvals, Scope::Questions, Scope::Walls] {
            assert!(needs.admitted(&[scope], act_table), "{needs:?} {scope:?}");
        }
        assert!(!needs.admitted(
            &others(&[Scope::Approvals, Scope::Questions, Scope::Walls]),
            act_table
        ));
    }
    for land in [full(Kind::Land), bare(Kind::Land)] {
        assert!(land.admitted(&[Scope::TurnEnd], act_table));
        assert!(!land.admitted(&others(&[Scope::TurnEnd]), act_table));
    }
    for kind in Kind::ALL {
        let made = full(*kind);
        match kind.needs() {
            Needs::Stream => assert!(made.admitted(&[], act_table), "{kind:?}"),
            Needs::Scope(scope) => {
                assert!(made.admitted(&[scope], act_table), "{kind:?}");
                assert!(!made.admitted(&others(&[scope]), act_table), "{kind:?}");
            }
            Needs::AskKind | Needs::ActScope => {
                assert!(!made.admitted(&[], act_table), "{kind:?}");
            }
        }
    }
}

#[test]
fn the_stream_reply_reads_epoch_acked_and_in() {
    assert_eq!(stream_request("s-1f").as_deref(), Some("stream @s-1f v=1"));
    for sid in [
        "self",
        "@self",
        "s-",
        "s-xyz",
        "s-1 x",
        "s-1\nstatus",
        ".",
        "",
    ] {
        assert_eq!(stream_request(sid), None, "{sid:?}");
        assert!(!crate::wire::session_id(sid), "the wire agrees: {sid:?}");
    }
    for sid in ["s-0", "s-1f", "s-DEADbeef"] {
        assert!(
            stream_request(sid).is_some() && crate::wire::session_id(sid),
            "{sid}"
        );
    }

    let epoch = "p4242-0123456789abcdef01234567";
    let ok = format!("OK stream v=1 epoch={epoch} acked=12 in=3");
    let reply = StreamReply::parse(&ok).expect("the OK line reads");
    assert_eq!(
        reply,
        StreamReply {
            epoch: s(epoch),
            acked: 12,
            sent: 3,
        }
    );
    // A new claimant or epoch starts at 0; words may come in any order; others are passed over.
    assert_eq!(
        StreamReply::parse("OK stream in=0 later=yes acked=0 epoch=p1-123 v=1\n"),
        Ok(StreamReply {
            epoch: s("p1-123"),
            acked: 0,
            sent: 0
        })
    );
    let max = format!("OK stream v=1 epoch=e acked={} in={}", u64::MAX, u64::MAX);
    assert_eq!(
        StreamReply::parse(&max).map(|reply| reply.acked),
        Ok(u64::MAX)
    );

    let refused = |reply: &str| StreamReply::parse(reply).expect_err(reply);
    assert_eq!(
        refused("ERR busy streams"),
        ReplyError::Refused(s("ERR:busy"))
    );
    assert_eq!(
        refused("ERR unknown verb: stream"),
        ReplyError::Refused(s("ERR:unknown"))
    );
    assert_eq!(refused(""), ReplyError::Refused(s("(empty)")));
    assert_eq!(
        refused("ERR main thread stalled 6s; retry"),
        ReplyError::Retry(s("ERR:main"))
    );
    assert_eq!(
        refused("OK subscribe 1"),
        ReplyError::Refused(s("OK:subscribe"))
    );
    assert_eq!(refused("OK"), ReplyError::Refused(s("OK")));
    assert_eq!(refused("OK stream"), ReplyError::Missing("v"));
    for version in ["2", "01", "1.0", "", "one"] {
        let line = format!("OK stream v={version} epoch=e acked=1 in=1");
        assert_eq!(refused(&line), ReplyError::Malformed("v"), "{line}");
    }
    let asked = stream_request("s-1").expect("a request");
    let version = asked.rsplit_once("v=").map(|(_, version)| version);
    let ok = format!(
        "OK stream v={} epoch=e acked=0 in=0",
        version.unwrap_or("?")
    );
    assert!(
        StreamReply::parse(&ok).is_ok(),
        "the reply reads the version the request asked for"
    );
    assert_eq!(
        refused("OK stream v=1 acked=1 in=1"),
        ReplyError::Missing("epoch")
    );
    assert_eq!(
        refused("OK stream v=1 epoch= acked=1 in=1"),
        ReplyError::Malformed("epoch")
    );
    assert_eq!(
        refused("OK stream v=1 epoch=p1/2 acked=1 in=1"),
        ReplyError::Malformed("epoch")
    );
    let long = format!("OK stream v=1 epoch={} acked=1 in=1", "e".repeat(129));
    assert_eq!(refused(&long), ReplyError::Malformed("epoch"));
    assert_eq!(
        refused("OK stream v=1 epoch=e in=1"),
        ReplyError::Missing("acked")
    );
    assert_eq!(
        refused("OK stream v=1 epoch=e acked=1"),
        ReplyError::Missing("in")
    );
    for bad in ["01", "-1", "1.0", "", "x", "18446744073709551616"] {
        let line = format!("OK stream v=1 epoch=e acked={bad} in=1");
        assert_eq!(refused(&line), ReplyError::Malformed("acked"), "{line}");
        let line = format!("OK stream v=1 epoch=e acked=1 in={bad}");
        assert_eq!(refused(&line), ReplyError::Malformed("in"), "{line}");
    }
    assert_eq!(
        refused("OK stream v=1 epoch=e acked=1 acked=1 in=1"),
        ReplyError::Repeated("acked")
    );
    assert_eq!(
        refused("OK stream v=1 v=1 epoch=e acked=1 in=1"),
        ReplyError::Repeated("v")
    );
}

#[test]
fn the_record_module_names_no_platform_item() {
    let sources = [
        ("record.rs", include_str!("record.rs")),
        ("record_json.rs", include_str!("record_json.rs")),
    ];
    let refused = [
        "std::os",
        "libc",
        "crate::wire",
        "crate::kernel",
        "crate::link",
        "crate::fake",
        "unix",
        "windows",
        "target_",
        "unsafe",
        "extern",
        "std::fs",
        "std::net",
        "std::io",
        "std::process",
        "std::thread",
        "std::env",
        "std::time",
        "SystemTime",
        "Instant",
    ];
    for (name, source) in sources {
        for item in refused {
            assert!(!source.contains(item), "{name} names {item}");
        }
        for (at, _) in source.match_indices("cfg(") {
            assert!(
                source[at..].starts_with("cfg(test)"),
                "{name}: only tests are gated"
            );
        }
        for (at, _) in source.match_indices("crate::") {
            let path = &source[at..];
            assert!(
                path.starts_with("crate::claim::") || path.starts_with("crate::record"),
                "{name} reaches {}",
                &path[..path
                    .find(|c: char| !c.is_alphanumeric() && c != ':' && c != '_')
                    .unwrap_or(path.len())]
            );
        }
        // A relative path would reach the modules the paths above may not: these files sit
        // beside the transport, so every path names the crate.
        for relative in ["super::", "self::"] {
            assert!(!source.contains(relative), "{name} names {relative}");
        }
        for line in source.lines().filter(|line| line.starts_with("use ")) {
            assert!(
                ["use std::fmt", "use std::path::", "use crate::"]
                    .iter()
                    .any(|allowed| line.starts_with(allowed)),
                "{name}: {line}"
            );
        }
    }
}

#[test]
fn the_streams_numbers_are_the_designs() {
    assert_eq!(DEPTH_MAX, 8);
    assert_eq!(BEAT_EVERY_MS, 10_000);
    assert_eq!(BEATS_MISSED, 3);
    assert_eq!((RATE_PER_S, RATE_BURST), (50, 500));
    assert_eq!(HAND_EVERY_MS, 100);
    // Twice the claim's renewal, plus 5 s, and the grace the link already waits out.
    assert_eq!(REEXEC_GRACE_MS, 2 * 20_000 + 5_000);
    assert_eq!(
        crate::relaunch::AWAY_BUDGET,
        Duration::from_millis(REEXEC_GRACE_MS)
    );
}

#[test]
fn text_is_escaped_and_reads_back_exactly() {
    let prompt = Record {
        seq: 1,
        ms: 2,
        body: Body::Prompt {
            text: s("a\"b\\c\nd\u{1b}e\u{2028}f\u{e0041}g/"),
            by: By::Person,
        },
    };
    let line = round_trip(&prompt);
    assert_eq!(
        line,
        concat!(
            r#"{"k":"prompt","seq":1,"ms":2,"#,
            r#""text":"a\"b\\c\nd\u001be\u2028f\udb40\udc41g/","by":"person"}"#,
        )
    );
    for kind in Kind::ALL {
        let line = round_trip(&record(full(*kind)));
        assert!(
            line.chars().all(|c| !crate::record_json::hidden(c)),
            "{line}"
        );
        assert!(
            line.bytes().all(|byte| byte >= 0x20 && byte != 0x7f),
            "{line}"
        );
    }
    // What needs no escape is written as it is.
    let plain = record_text("é 日本 😀 a\u{200d}b");
    assert_eq!(plain, "\"é 日本 😀 a\u{200d}b\"");

    // Every escape JSON has reads.
    let line =
        r#"{"k":"goal","seq":1,"ms":0,"text":"\"\\\/\b\f\n\r\t\u00e9\u00E9\ud83d\ude00\u0000"}"#;
    assert_eq!(
        body(line),
        Body::Goal {
            text: Some(s("\"\\/\u{8}\u{c}\n\r\té\u{e9}😀\u{0}"))
        }
    );
    // Raw non-ASCII the table does not name reads as it is; a raw DEL or bidi control does not.
    assert_eq!(
        body("{\"k\":\"goal\",\"seq\":1,\"ms\":0,\"text\":\"é 日本 😀 a\u{200d}b\"}"),
        Body::Goal {
            text: Some(s("é 日本 😀 a\u{200d}b"))
        }
    );
    for raw in ["\u{7f}", "\u{202e}"] {
        let line = format!(r#"{{"k":"goal","seq":1,"ms":0,"text":"{raw}"}}"#);
        assert!(matches!(read(&line), Err(LineError::Syntax(_))), "{raw:?}");
    }
}

#[test]
fn a_bad_escape_a_lone_surrogate_or_a_raw_control_is_refused() {
    for text in [
        r"\x",
        r"\u12",
        r"\u12g4",
        r"\ud800",
        r"\ud800x",
        r"\ud800\u0041",
        r"\ud800\ud800",
        r"\udc00",
        r"\udfff\ud800",
        r"\U0041",
        "\\",
        "\t",
        "\n",
        "\u{0}",
        "\u{1f}",
        "\u{7f}",
        "\u{85}",
        "\u{9f}",
        "\u{ad}",
        "\u{200b}",
        "\u{200e}",
        "\u{2028}",
        "\u{2029}",
        "\u{202e}",
        "\u{2066}",
        "\u{feff}",
        "\u{fffe}",
        "\u{1fffe}",
        "\u{e0041}",
    ] {
        for line in [
            format!(r#"{{"k":"goal","seq":1,"ms":0,"text":"{text}"}}"#),
            format!(r#"{{"k":"goal","seq":1,"ms":0,"x{text}":1}}"#),
            format!(r#"{{"k":"session","seq":1,"ms":0,"resume":["{text}"]}}"#),
        ] {
            assert!(
                matches!(read(&line), Err(LineError::Syntax(_))),
                "{line:?}: {:?}",
                read(&line)
            );
        }
    }
}

#[test]
fn a_line_that_is_not_utf8_is_refused() {
    let head = br#"{"k":"goal","seq":1,"ms":0,"text":""#;
    for bad in [
        &b"\xff"[..],
        b"\xc0\xaf",
        b"\xed\xa0\x80",
        b"\xe2\x82",
        b"\xf4\x90\x80\x80",
        b"\x80",
    ] {
        let mut line = head.to_vec();
        line.extend_from_slice(bad);
        line.extend_from_slice(b"\"}");
        assert_eq!(Record::read(&line), Err(LineError::Utf8), "{bad:?}");
    }
}

#[test]
fn nesting_deeper_than_eight_is_refused() {
    // The record's object is depth 1; seven lists inside it make 8.
    let nested = |depth: usize| {
        format!(
            r#"{{"k":"beat","seq":1,"ms":0,"x":{}1{}}}"#,
            "[".repeat(depth - 1),
            "]".repeat(depth - 1)
        )
    };
    assert_eq!(unknown(&nested(DEPTH_MAX)), 1);
    assert_eq!(read(&nested(DEPTH_MAX + 1)), Err(LineError::Depth));
    let objects = format!(
        r#"{{"k":"beat","seq":1,"ms":0,"x":{}1{}}}"#,
        r#"{"a":"#.repeat(DEPTH_MAX),
        "}".repeat(DEPTH_MAX)
    );
    assert_eq!(read(&objects), Err(LineError::Depth));
    // Far deeper than any stack would hold if reading recursed without the bound.
    let deep = "[".repeat(LINE_MAX);
    assert_eq!(read(&deep), Err(LineError::Depth));
    let deep = format!(
        "{}{}",
        r#"{"a":"#.repeat(LINE_MAX / 5),
        "}".repeat(LINE_MAX / 5)
    );
    assert_eq!(read(&deep[..LINE_MAX]), Err(LineError::Depth));
}

#[test]
fn a_line_that_is_not_one_object_is_refused() {
    for line in [
        "",
        " ",
        "[]",
        "\"k\"",
        "1",
        "null",
        "true",
        "{",
        "}",
        r#"{"k":"beat","seq":1,"ms":0}x"#,
        r#"{"k":"beat","seq":1,"ms":0}{"k":"beat","seq":2,"ms":0}"#,
        r#"{"k":"beat","seq":1,"ms":0,}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":[1,]}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":01}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":-}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":1.}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":.5}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":1e}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":+1}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":tru}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":NaN}"#,
        r#"{"k":"beat","seq":1,"ms":0,"x":'a'}"#,
        r#"{k:"beat","seq":1,"ms":0}"#,
        r#"{"k" "beat","seq":1,"ms":0}"#,
        "{\"k\":\"beat\",\"seq\":1,\"ms\":0}\u{0}",
    ] {
        assert!(
            matches!(read(line), Err(LineError::Syntax(_))),
            "{line:?}: {:?}",
            read(line)
        );
    }
}

#[test]
fn a_known_field_out_of_its_grammar_is_refused() {
    let cases: &[(&str, LineError)] = &[
        (r#"{"seq":1,"ms":0}"#, LineError::Missing("k")),
        (r#"{"k":"beat","ms":0}"#, LineError::Missing("seq")),
        (r#"{"k":"beat","seq":1}"#, LineError::Missing("ms")),
        (
            r#"{"k":"beat","seq":null,"ms":0}"#,
            LineError::Missing("seq"),
        ),
        (r#"{"k":3,"seq":1,"ms":0}"#, LineError::Malformed("k")),
        (
            r#"{"k":"beat","k":"beat","seq":1,"ms":0}"#,
            LineError::Repeated("k"),
        ),
        (
            r#"{"k":"beat","seq":1,"seq":1,"ms":0}"#,
            LineError::Repeated("seq"),
        ),
        (
            r#"{"k":"beat","seq":0,"ms":0}"#,
            LineError::Malformed("seq"),
        ),
        (
            r#"{"k":"beat","seq":-1,"ms":0}"#,
            LineError::Malformed("seq"),
        ),
        (
            r#"{"k":"beat","seq":1.0,"ms":0}"#,
            LineError::Malformed("seq"),
        ),
        (
            r#"{"k":"beat","seq":1e3,"ms":0}"#,
            LineError::Malformed("seq"),
        ),
        (
            r#"{"k":"beat","seq":"1","ms":0}"#,
            LineError::Malformed("seq"),
        ),
        (
            r#"{"k":"beat","seq":18446744073709551616,"ms":0}"#,
            LineError::Malformed("seq"),
        ),
        (
            r#"{"k":"beat","seq":1,"ms":99999999999999999999999999999999999999}"#,
            LineError::Malformed("ms"),
        ),
        (
            r#"{"k":"state","seq":1,"ms":0}"#,
            LineError::Missing("state"),
        ),
        (
            r#"{"k":"state","seq":1,"ms":0,"state":"asleep"}"#,
            LineError::Malformed("state"),
        ),
        (
            r#"{"k":"state","seq":1,"ms":0,"state":"idle","state":"idle"}"#,
            LineError::Repeated("state"),
        ),
        (
            r#"{"k":"state","seq":1,"ms":0,"state":"idle","composer":"full"}"#,
            LineError::Malformed("composer"),
        ),
        (
            r#"{"k":"compact","seq":1,"ms":0,"state":"done","context_left":101}"#,
            LineError::Malformed("context_left"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"child":{"pid":1,"start":5}}"#,
            LineError::Malformed("child"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"child":{"pid":2147483648,"start":5}}"#,
            LineError::Malformed("child"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"child":{"pid":4242,"start":0}}"#,
            LineError::Malformed("child"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"scope":["state","state"]}"#,
            LineError::Repeated("scope"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"scope":["State"]}"#,
            LineError::Malformed("scope"),
        ),
        (
            r#"{"k":"answer","seq":1,"ms":0,"ask":1,"by":"person","human_seq":1}"#,
            LineError::Missing("option"),
        ),
        (
            concat!(
                r#"{"k":"answer","seq":1,"ms":0,"ask":1,"option":"a","text":"b","#,
                r#""by":"person","human_seq":1}"#,
            ),
            LineError::Malformed("text"),
        ),
        (
            r#"{"k":"hand","seq":1,"ms":0,"human_seq":1,"composer":"empty","hold":true}"#,
            LineError::Malformed("hold"),
        ),
        (
            r#"{"k":"prompt","seq":1,"ms":0,"text":"x","by":"peer:"}"#,
            LineError::Malformed("by"),
        ),
        (
            r#"{"k":"tool","seq":1,"ms":0,"id":"a b","name":"Bash","state":"done"}"#,
            LineError::Malformed("id"),
        ),
        // `k` is a word, known or not.
        (r#"{"k":"","seq":1,"ms":0}"#, LineError::Malformed("k")),
        (r#"{"k":" beat","seq":1,"ms":0}"#, LineError::Malformed("k")),
        (
            r#"{"k":"be\nat","seq":1,"ms":0}"#,
            LineError::Malformed("k"),
        ),
        (
            r#"{"k":"\u0000","seq":1,"ms":0}"#,
            LineError::Malformed("k"),
        ),
        (
            r#"{"k":"\u202ebeat","seq":1,"ms":0}"#,
            LineError::Malformed("k"),
        ),
        (r#"{"k":"é","seq":1,"ms":0}"#, LineError::Malformed("k")),
        // A list that names each word once names each once, known to this library or not.
        (
            r#"{"k":"claim","seq":1,"ms":0,"scope":["zzz","zzz"]}"#,
            LineError::Repeated("scope"),
        ),
        (
            concat!(
                r#"{"k":"session","seq":1,"ms":0,"agent":"x","version":"1.0.0","mode":"tui","#,
                r#""takes":["zzz","state","zzz"]}"#,
            ),
            LineError::Repeated("takes"),
        ),
        // A child names its pid and start once each.
        (
            r#"{"k":"claim","seq":1,"ms":0,"child":{"pid":5,"pid":6,"start":1}}"#,
            LineError::Repeated("pid"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"child":{"pid":5,"start":1,"start":2}}"#,
            LineError::Repeated("start"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"child":{"start":1}}"#,
            LineError::Malformed("child"),
        ),
        // The claim's words are in the grammar of the claim words aterm builds from them.
        (
            r#"{"k":"claim","seq":1,"ms":0,"version":"0.4.1"}"#,
            LineError::Malformed("version"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"vendor":"2.1/325"}"#,
            LineError::Malformed("vendor"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"login":"Work"}"#,
            LineError::Malformed("login"),
        ),
        (
            r#"{"k":"claim","seq":1,"ms":0,"login":"help"}"#,
            LineError::Malformed("login"),
        ),
        (
            r#"{"k":"session","seq":1,"ms":0,"agent":"x","version":"1","mode":"tui"}"#,
            LineError::Malformed("version"),
        ),
        (
            concat!(
                r#"{"k":"session","seq":1,"ms":0,"agent":"x","version":"1.0.0","mode":"tui","#,
                r#""account":"-work"}"#,
            ),
            LineError::Malformed("account"),
        ),
        // A turn that starts says who opened it; its end and a step's are words of their own.
        (
            r#"{"k":"turn","seq":1,"ms":0,"turn":1,"state":"started"}"#,
            LineError::Missing("by"),
        ),
        (
            r#"{"k":"tool","seq":1,"ms":0,"id":"a","name":"Bash","state":"walled"}"#,
            LineError::Malformed("state"),
        ),
        (
            r#"{"k":"compact","seq":1,"ms":0,"state":"failed"}"#,
            LineError::Malformed("state"),
        ),
        // A reading names its account.
        (
            r#"{"k":"usage","seq":1,"ms":0,"context_left":5}"#,
            LineError::Missing("account"),
        ),
        (
            r#"{"k":"account","seq":1,"ms":0,"state":"ready"}"#,
            LineError::Missing("account"),
        ),
        (
            r#"{"k":"usage","seq":1,"ms":0,"account":"a b"}"#,
            LineError::Malformed("account"),
        ),
        // aterm's records are sent by someone outside the harness.
        (
            r#"{"k":"prompt","seq":1,"ms":0,"text":"x","by":"agent"}"#,
            LineError::Malformed("by"),
        ),
        (
            r#"{"k":"prompt","seq":1,"ms":0,"text":"x","by":"harness"}"#,
            LineError::Malformed("by"),
        ),
        (
            r#"{"k":"answer","seq":1,"ms":0,"ask":1,"text":"x","by":"agent","human_seq":1}"#,
            LineError::Malformed("by"),
        ),
        // An act carries its own fields and no other's.
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"approve"}"#,
            LineError::Missing("rule"),
        ),
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"approve","rule":"r","held":"no-work"}"#,
            LineError::Malformed("held"),
        ),
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"approve","rule":"r","wait_ms":5}"#,
            LineError::Malformed("wait_ms"),
        ),
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"wait","wait_ms":5}"#,
            LineError::Missing("reason"),
        ),
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"wait","reason":"bored"}"#,
            LineError::Malformed("reason"),
        ),
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"wait","reason":"retry","rule":"r"}"#,
            LineError::Malformed("rule"),
        ),
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"held","held":"tired"}"#,
            LineError::Malformed("held"),
        ),
        (
            r#"{"k":"act","seq":1,"ms":0,"act":"held","reason":"person"}"#,
            LineError::Malformed("reason"),
        ),
        // A need names what it raises, and 1 to 32 needs.
        (
            r#"{"k":"need","seq":1,"ms":0,"entry":"x","needs":["x"]}"#,
            LineError::Missing("kind"),
        ),
        (
            r#"{"k":"need","seq":1,"ms":0,"kind":"approval","entry":"x","needs":["x"]}"#,
            LineError::Malformed("kind"),
        ),
        (
            r#"{"k":"need","seq":1,"ms":0,"kind":"help","entry":"x"}"#,
            LineError::Missing("needs"),
        ),
        (
            r#"{"k":"need","seq":1,"ms":0,"kind":"help","entry":"x","needs":[]}"#,
            LineError::Missing("needs"),
        ),
        // Land's state carries its own fields and no other's.
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"held","nudges":0}"#,
            LineError::Missing("since_ms"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"held","since_ms":1}"#,
            LineError::Missing("nudges"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"landed"}"#,
            LineError::Missing("commit"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"held","since_ms":1,"nudges":0,"changed":2}"#,
            LineError::Missing("legs"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"held","since_ms":1,"nudges":0,"open":["x"]}"#,
            LineError::Missing("legs"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"held","since_ms":1,"nudges":0,"commit":"c"}"#,
            LineError::Malformed("commit"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"landed","commit":"c","nudges":1}"#,
            LineError::Malformed("nudges"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"open","since_ms":1}"#,
            LineError::Malformed("since_ms"),
        ),
        (
            r#"{"k":"land","seq":1,"ms":0,"state":"open","legs":["repo"]}"#,
            LineError::Malformed("legs"),
        ),
        // A restart is read by the claim's rules, so every one that reads maps to its Restart.
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"x.y!"}"#,
            LineError::Malformed("cause"),
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"stall","to":"work","home":"-"}"#,
            LineError::Malformed("to"),
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"account","to":"work"}"#,
            LineError::Missing("home"),
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"account","home":"/h"}"#,
            LineError::Missing("to"),
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"account","to":"Work","home":"/h"}"#,
            LineError::Malformed("to"),
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"account","to":"work","home":"relative/dir"}"#,
            LineError::Malformed("home"),
        ),
        (
            concat!(
                r#"{"k":"restart","seq":1,"ms":0,"cause":"model","#,
                r#""model":"--dangerously-skip-permissions"}"#,
            ),
            LineError::Malformed("model"),
        ),
        (
            r#"{"k":"set","seq":1,"ms":0,"model":"--yolo"}"#,
            LineError::Malformed("model"),
        ),
        (
            r#"{"k":"model","seq":1,"ms":0,"offers":[{"model":"-x"}]}"#,
            LineError::Malformed("model"),
        ),
    ];
    for (line, error) in cases {
        assert_eq!(read(line), Err(error.clone()), "{line}");
    }
    let long_k = format!(r#"{{"k":"{}","seq":1,"ms":0}}"#, "k".repeat(WORD_MAX + 1));
    assert_eq!(read(&long_k), Err(LineError::Malformed("k")));
    let long_cause = format!(
        r#"{{"k":"restart","seq":1,"ms":0,"cause":"{}"}}"#,
        "c".repeat(65)
    );
    assert_eq!(read(&long_cause), Err(LineError::Malformed("cause")));
    let needs = vec![r#""x""#; NEEDS_MAX + 1].join(",");
    let over =
        format!(r#"{{"k":"need","seq":1,"ms":0,"kind":"help","entry":"x","needs":[{needs}]}}"#);
    assert_eq!(read(&over), Err(LineError::Malformed("needs")));
    // What reads as a restart maps to the claim's Restart, word for word.
    for (line, words) in [
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"account","to":"work","home":"/h/k b"}"#,
            "cause=account to=work home=%2Fh%2Fk%20b",
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"account-ahead","to":"ac-1","home":"-"}"#,
            "cause=account-ahead to=ac-1 home=-",
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"model","model":"opus[1m]"}"#,
            "cause=model model=opus[1m]",
        ),
        (
            r#"{"k":"restart","seq":1,"ms":0,"cause":"stall"}"#,
            "cause=stall",
        ),
    ] {
        let Body::Restart {
            cause,
            account,
            model,
        } = body(line)
        else {
            panic!("a restart: {line}");
        };
        let restart = crate::claim::Restart::parse(words.split(' ')).expect(words);
        assert_eq!(
            (cause, account, model),
            (restart.cause, restart.account, restart.model)
        );
    }
    assert_eq!(
        read(r#"{"k":"took","seq":1,"ms":0,"in":0}"#),
        Err(LineError::Malformed("in"))
    );
    assert_eq!(
        read(concat!(
            r#"{"k":"session","seq":1,"ms":0,"agent":"x","version":"1.0.0","mode":"tui","#,
            r#""takes":["prompt","prompt"]}"#,
        )),
        Err(LineError::Repeated("takes"))
    );
    assert_eq!(
        read(r#"{"k":"ack","seq":1,"ms":0,"dropped":"two words"}"#),
        Err(LineError::Malformed("dropped"))
    );
}

#[test]
fn the_writer_refuses_what_the_reader_would() {
    let refused = |body: Body| record(body).line().expect_err("refused");
    assert_eq!(
        Record {
            seq: 0,
            ms: 0,
            body: Body::Beat
        }
        .line(),
        Err(LineError::Malformed("seq"))
    );
    let mut session = bare(Kind::Session);
    if let Body::Session { agent, .. } = &mut session {
        *agent = s("claude code");
    }
    assert_eq!(refused(session), LineError::Malformed("agent"));
    let mut session = bare(Kind::Session);
    if let Body::Session { takes, .. } = &mut session {
        *takes = vec![Kind::State];
    }
    assert_eq!(refused(session), LineError::Malformed("takes"));
    let mut session = bare(Kind::Session);
    if let Body::Session { takes, .. } = &mut session {
        *takes = vec![Kind::Want, Kind::Want];
    }
    assert_eq!(refused(session), LineError::Repeated("takes"));
    assert_eq!(
        refused(Body::Wall {
            kind: String::new(),
            resets_ms: None
        }),
        LineError::Malformed("kind")
    );
    let claim = |child, scope, version: Option<&str>, vendor: Option<&str>, login: Option<&str>| {
        Body::Claim {
            child,
            scope,
            version: version.map(s),
            vendor: vendor.map(s),
            login: login.map(s),
        }
    };
    for child in [
        ChildProcess { pid: 1, start: 1 },
        ChildProcess {
            pid: 1 << 31,
            start: 1,
        },
        ChildProcess { pid: 2, start: 0 },
    ] {
        assert_eq!(
            refused(claim(Some(child), None, None, None, None)),
            LineError::Malformed("child")
        );
    }
    assert_eq!(
        refused(claim(
            None,
            Some(vec![Scope::State, Scope::State]),
            None,
            None,
            None
        )),
        LineError::Repeated("scope")
    );
    assert_eq!(
        refused(claim(None, None, Some("0.4"), None, None)),
        LineError::Malformed("version")
    );
    assert_eq!(
        refused(claim(None, None, None, Some("2 1"), None)),
        LineError::Malformed("vendor")
    );
    assert_eq!(
        refused(claim(None, None, None, None, Some("help"))),
        LineError::Malformed("login")
    );
    assert_eq!(
        refused(Body::Account {
            account: s("Work"),
            state: s("ready")
        }),
        LineError::Malformed("account")
    );
    assert_eq!(
        refused(Body::Turn {
            turn: 1,
            state: TurnState::Started,
            by: None
        }),
        LineError::Missing("by")
    );
    for by in [By::Agent, By::Harness] {
        assert_eq!(
            refused(Body::Prompt {
                text: s("x"),
                by: by.clone()
            }),
            LineError::Malformed("by")
        );
        assert_eq!(
            refused(Body::Answer {
                ask: 1,
                option: None,
                text: Some(s("x")),
                by,
                human_seq: 1,
            }),
            LineError::Malformed("by")
        );
    }
    for act in [Did::WAIT, Did::HELD] {
        assert_eq!(
            refused(Body::Act {
                did: Did::Rule {
                    act: s(act),
                    rule: s("r")
                }
            }),
            LineError::Malformed("act")
        );
    }
    assert_eq!(
        refused(Body::Need {
            kind: NeedKind::Help,
            entry: s("x"),
            needs: Vec::new()
        }),
        LineError::Missing("needs")
    );
    assert_eq!(
        refused(Body::Need {
            kind: NeedKind::Help,
            entry: s("x"),
            needs: vec![s("x"); NEEDS_MAX + 1]
        }),
        LineError::Malformed("needs")
    );
    assert_eq!(
        refused(Body::Land {
            state: Landing::Held(LandHold {
                since_ms: 1,
                nudges: 0,
                read: Some(LandRead {
                    legs: Vec::new(),
                    open: Vec::new(),
                    changed: Some(1),
                    untracked: None,
                    stashes: None,
                    unpushed: None,
                    unlanded: None,
                    left: None,
                }),
            })
        }),
        LineError::Missing("legs")
    );
    let restart = |cause: &str, home: Option<&str>, model: Option<&str>| Body::Restart {
        cause: s(cause),
        account: home.map(|home| Account {
            label: s("work"),
            home: Some(PathBuf::from(home)),
        }),
        model: model.map(s),
    };
    assert_eq!(
        refused(restart("x.y", None, None)),
        LineError::Malformed("cause")
    );
    assert_eq!(
        refused(restart("stall", Some("/h"), None)),
        LineError::Malformed("to")
    );
    assert_eq!(
        refused(restart("account", Some("relative"), None)),
        LineError::Malformed("home")
    );
    assert_eq!(
        refused(restart("model", None, Some("-m"))),
        LineError::Malformed("model")
    );
    assert_eq!(
        refused(Body::Set {
            model: Some(s("--yolo")),
            effort: None,
            mode: None
        }),
        LineError::Malformed("model")
    );
    let answer = |option: Option<&str>, text: Option<&str>| Body::Answer {
        ask: 1,
        option: option.map(s),
        text: text.map(s),
        by: By::Person,
        human_seq: 1,
    };
    assert_eq!(refused(answer(None, None)), LineError::Missing("option"));
    assert_eq!(
        refused(answer(Some("a"), Some("b"))),
        LineError::Malformed("text")
    );
    for k in ["state", "", "two words", "\u{202e}x"] {
        assert_eq!(
            refused(Body::Unknown { k: s(k) }),
            LineError::Malformed("k"),
            "{k:?}"
        );
    }
    assert_eq!(
        refused(Body::Prompt {
            text: s("x"),
            by: By::Peer(s("a b"))
        }),
        LineError::Malformed("by")
    );
    assert_eq!(
        refused(Body::Ack {
            dropped: Some(Dropped::Refused(String::new()))
        }),
        LineError::Malformed("dropped")
    );
    assert_eq!(
        refused(Body::Ack {
            dropped: Some(Dropped::Other(s("bound")))
        }),
        LineError::Malformed("dropped")
    );
    assert_eq!(
        refused(Body::Clear { need: 0 }),
        LineError::Malformed("need")
    );
    assert_eq!(refused(Body::Touch { ask: 0 }), LineError::Malformed("ask"));
    assert_eq!(
        refused(Body::Model {
            model: None,
            effort: None,
            key: None,
            offers: vec![Offer {
                model: s("x"),
                levels: vec![s("very high")]
            }],
            permission: None
        }),
        LineError::Malformed("levels")
    );
    assert_eq!(
        refused(Body::Compact {
            state: CompactState::Done,
            context_left: Some(101)
        }),
        LineError::Malformed("context_left")
    );
    let long = "w".repeat(WORD_MAX + 1);
    assert_eq!(
        refused(Body::Upgrade {
            phase: UpgradePhase::Seen,
            to: long
        }),
        LineError::Malformed("to")
    );
    assert!(is_word(&"w".repeat(WORD_MAX)) && !is_word("") && !is_word("é"));
}

#[test]
fn a_truncated_line_never_reads_and_never_panics() {
    for kind in Kind::ALL {
        for made in [full(*kind), bare(*kind)] {
            let line = round_trip(&record(made));
            let bytes = line.as_bytes();
            for cut in 0..bytes.len() {
                assert!(
                    Record::read(&bytes[..cut]).is_err(),
                    "{kind:?} cut at {cut} of {line}"
                );
            }
        }
    }
}

/// A small deterministic generator, so a failure names its seed.
struct Xorshift(u64);

impl Xorshift {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.step() % bound as u64).unwrap_or(0)
    }
}

#[test]
fn any_mutated_line_reads_or_is_refused_and_what_reads_writes_back() {
    let lines: Vec<Vec<u8>> = Kind::ALL
        .iter()
        .flat_map(|kind| [full(*kind), bare(*kind)])
        .map(|made| round_trip(&record(made)).into_bytes())
        .collect();
    let alphabet =
        b"{}[]\":,\\u0123456789abcdefnrtluesx-+.eE \t\r\n\x00\x7f\xc3\xa9\xed\xa0\x80\xff";
    let mut random = Xorshift(0x5eed_1234_abcd_ef01);
    for round in 0..40_000 {
        let mut line = lines[random.below(lines.len())].clone();
        for _ in 0..=random.below(4) {
            let at = random.below(line.len() + 1);
            let byte = alphabet[random.below(alphabet.len())];
            match random.below(3) {
                0 if at < line.len() => line[at] = byte,
                1 if at < line.len() => {
                    line.remove(at);
                }
                _ => line.insert(at, byte),
            }
        }
        let Ok(decoded) = Record::read(&line) else {
            continue;
        };
        let written = decoded.record.line().unwrap_or_else(|error| {
            panic!(
                "round {round}: {:?} read but writes: {error}",
                String::from_utf8_lossy(&line)
            )
        });
        let again = Record::read(written.as_bytes()).expect("what was written reads");
        assert_eq!(again.record, decoded.record, "round {round}: {written}");
        assert_eq!(again.unknown, 0, "round {round}: {written}");
    }
}
