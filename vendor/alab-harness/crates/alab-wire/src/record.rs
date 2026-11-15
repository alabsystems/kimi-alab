// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The session stream's records: the vocabulary a harness and aterm send each other, one JSON
//! object per line, once a contract-4 claim has opened a stream on its own connection.
//!
//! This is the `Record` type of aterm's harness interface design
//! (`aterm/docs/DESIGN-harness-interface-2026-11-12.md`, section 2.3 "The session stream"), which
//! hands the full field list to this crate. Each kind's fields follow the design's "Carries"
//! column. The codec is [`Record::line`] and [`Record::read`]; the verb that opens a stream is
//! [`stream_request`], and its `OK` line is a [`StreamReply`].
//!
//! The module is portable: it names nothing of a platform, reads no clock (the caller passes
//! `ms`), performs no I/O and has no dependency, so a crate that builds on every platform can
//! read it. It names the claim's [`Scope`], [`ChildProcess`], [`Account`] and [`ReplyError`], and
//! the claim's word rules (`canonical_u64`, `claim_version`, `vendor_word`, `login_word`,
//! [`crate::claim::model_word`] and the restart's cause), so a record's words are the claim's.
//! The claim reaches the wire's text helpers (`Fields`, `percent_decode`, `session_id`), never
//! its transport: when a Windows harness comes, those helpers move out of the wire, the claim's
//! import follows them, and this module compiles unchanged.
//!
//! # Every record
//!
//! `k`, `seq` and `ms` come first, in that order, on every line written.
//!
//! * `k`: the kind's word ([`Kind::word`]). A word this library does not know reads as
//!   [`Body::Unknown`], counted, never refused; a `k` that is not a word is refused.
//! * `seq`: the sender's count, from 1 with no gaps. On `ack` alone, `seq` is the harness record
//!   acknowledged: an `ack` is not counted in aterm's `in` and is not answered.
//! * `ms`: the sender's clock, milliseconds since 1970. It is only shown; aterm stamps each record
//!   when it arrives, and order is by `seq`.
//!
//! A field marked *required* must be present; every other field may be absent or `null`, which
//! reads as absent, and a list may be absent, which reads as empty. A *word* is 1 to [`WORD_MAX`]
//! bytes of printable ASCII with no space. *Text* is any string. A *percent* is 0 to 100. An
//! *account* is an account label, the claim's `login=`: `[a-z0-9][a-z0-9._-]{0,63}`, not `help`
//! (an `ac-` key of the account pool is one). A *model word* is what a restart may name
//! ([`crate::claim::model_word`]). A field this library does not know is passed over and
//! counted ([`Decoded::unknown`]); a known field out of its grammar, or on a record whose other
//! fields leave it no place, refuses the line.
//!
//! # Harness to aterm
//!
//! * `session`: `agent` (word, required: the agent word), `version` (required: the harness's, in
//!   the claim's `version=` grammar), `vendor` (the vendor CLI's version, in the claim's
//!   `vendor=` grammar), `account` (account: the one it runs on), `conversation` (word), `cwd`
//!   (text), `resume` (list of text: the arguments that resume this conversation), `mode` (`tui`
//!   or `headless`, required), `takes` (list of words: the kinds of aterm's records it takes,
//!   each once).
//! * `claim`: `child` (object `{"pid","start"}`: pid 2 to 2^31-1, start above 0), `scope` (list
//!   of scope words, each once: the narrowed scope), `version`, `vendor` (as in `session`) and
//!   `login` (account). The claim renewed on the stream: aterm builds its `meta set supervisor`
//!   line from these words, so each is in the grammar of the claim word it becomes.
//! * `state`: `state` (`starting`, `idle`, `working`, `asking`, `walled` or `ended`, required),
//!   `composer` (`empty` or `draft`; in a TUI tab only).
//! * `beat`: nothing. Sent every [`BEAT_EVERY_MS`].
//! * `turn`: `turn` (number, required: the harness's count of turns), `state` (`started`, `done`,
//!   `failed`, `interrupted` or `walled`, required; `walled` ends it on a wall, whose kind the
//!   `wall` record names), `by` (who opened it; required when `started`).
//! * `msg`: `turn` (number), `by` (who said it, required), `text` (text, required).
//! * `tool`: `turn` (number), `id` (word, required), `name` (word, required), `state` (`started`,
//!   `done`, `failed` or `interrupted`, required), `text` (text: what it ran, in one line).
//! * `diff`: `turn` (number), `path` (text), `patch` (text, required).
//! * `subagent`: `id` (word, required), `title` (text), `state` (`started`, `done`, `failed` or
//!   `interrupted`, required).
//! * `title`: `title` (text), `summary` (text: one line).
//! * `progress`: `items` (list of objects `{"text","state"}`: text, and `pending`, `active` or
//!   `done`; both required).
//! * `goal`: `text` (text: the active goal; absent when none is active).
//! * `compact`: `state` (`started` or `done`, required: aterm's compact step, which opens and
//!   closes and never fails), `context_left` (percent).
//! * `ask`: `kind` (`approval` or `question`, required), `title` (text), `options` (list of
//!   text), `will` (text: what the harness will answer), `will_in_ms` (number: how long after this
//!   record it will).
//! * `asked`: `ask` (number, required: the `seq` of the ask), `kind` (`approval` or `question`,
//!   required), `by` (who answered, required), `reply` (`approve`, `deny` or `answer`, required),
//!   `option` (text), `text` (text).
//! * `usage`: `account` (account, required: whose limits), `limits` (list of objects
//!   `{"name","used","resets_ms"}`: name a word, required; used a percent; resets_ms a time in ms
//!   since 1970), `context_left` (percent). aterm takes a reading only for the account the pool
//!   assigned to the tab.
//! * `account`: `account` (account, required), `state` (word, required).
//! * `wall`: `kind` (word, required: aterm's wall words, such as `usage-session`; aterm reads a
//!   kind it does not know as a bare `wall`), `resets_ms` (number).
//! * `model`: `model` (text: the name as the vendor states it), `effort` (word), `key` (boolean:
//!   whether the vendor's top-effort key is on; its word is not sent), `offers` (list of objects
//!   `{"model","levels"}`: a model word, required, and a list of level words), `permission`
//!   (word: the permission mode).
//! * `act`: `act` (word, required: what it did), and the fields of that act, no others:
//!   * `wait` ([`Did::WAIT`]: the continuation observation of `meta set harness_activity`):
//!     `reason` (`person`, `composing`, `dialog`, `paused`, `working`, `ready`, `retry` or
//!     `waiting`, required), `wait_ms` (number: how long it waits; absent for no bound, the
//!     `wait_ms=-` it replaces).
//!   * `held` ([`Did::HELD`]: `meta set harness_held`): `held` (`same-reply` or `no-work`: why
//!     its own typing stopped; absent when its typing goes on again, as `meta unset
//!     harness_held`).
//!   * any other act, taken by a rule: `rule` (word, required: the `<act>` of `meta set
//!     harness_act <act>`, such as `approve-all@v1`).
//! * `need`: `kind` (`review`, `decision` or `help`, required: the need aterm's `mark need`
//!   raises), `entry` (text, required: what it needs from a person), `needs` (list of text, 1 to
//!   [`NEEDS_MAX`], required: each need the entry stands for, in order, as `meta set
//!   harness_needs` tells them).
//! * `clear`: `need` (number, required: the `seq` of the `need` record whose need ended). aterm
//!   lifts what that record raised (`mark clear` under its kind).
//! * `land`: `state` (`held`, `landed` or `open`, required), and the fields of that state, no
//!   others:
//!   * `held`, as `meta set harness_land` tells it: `since_ms` (number, required: held since),
//!     `nudges` (number, required: the nudges typed since), and Land's read of the yield, when
//!     it read one: `legs` (list of words, at least one: what held the landing), then, only
//!     beside `legs`, `open` (list of words: what the agent's list left open), `changed`,
//!     `untracked`, `stashes`, `unpushed`, `unlanded` (numbers: the repository's work) and
//!     `left` (number: the processes left running).
//!   * `landed`: `commit` (word, required: what landed on the remote main branch).
//!   * `open`: nothing. A hold told before is lifted, as `meta unset harness_land`.
//! * `upgrade`: `phase` (`seen`, `waiting`, `moving`, `done` or `abandoned`, required), `to`
//!   (word, required: the target version).
//! * `took`: `in` (number, required: the aterm record it answers).
//! * `refused`: `in` (number, required), `why` (text, required).
//!
//! # aterm to harness
//!
//! * `ack`: `dropped` (word: `bound`, `rate`, `scope`, `applied` or `refused:<word>`; absent
//!   when the record was applied). `seq` is the harness record it acknowledges.
//! * `prompt`: `text` (text, required), `by` (`person`, `ctl` or `peer:<tab>`, required).
//! * `answer`: `ask` (number, required), `option` (text) or `text` (text), exactly one, `by`
//!   (`person`, `ctl` or `peer:<tab>`, required), `human_seq` (number, required: read at the
//!   click).
//! * `touch`: `ask` (number, required).
//! * `hand`: `human_seq` (number, required), `composer` (`empty` or `draft`, required), `hold`
//!   (`0` or `1`, required). Sent at most every [`HAND_EVERY_MS`].
//! * `interrupt`: nothing.
//! * `set`: `model` (model word), `effort` (word), `mode` (`tui` or `headless`).
//! * `restart`: `cause` (required: 1 to 64 ASCII letters, digits, `-` and `_`), `to` (account)
//!   and `home` (text: an absolute path, or `-` for the vendor's default home), both or neither,
//!   and only for the cause `account` or `account-ahead`, and `model` (model word): the rules of
//!   the claim's [`crate::claim::Restart`], which a record reads into.
//! * `policy`: nothing: read the grant again.
//! * `want`: nothing: send the conversation again.
//!
//! `by` is `person`, `ctl`, `harness`, `agent` or `peer:<tab>` ([`By`]); aterm's records name
//! only `person`, `ctl` and `peer:<tab>`, the senders outside the harness.

use std::fmt;

use crate::claim::{Account, ChildProcess, ReplyError, Scope, canonical_u64};

/// The longest line, in bytes: every byte [`Record::read`] is passed, which is the line without
/// its newline (a `\r` before the newline is one of those bytes). A longer line is refused before
/// it is parsed ([`LineError::Bound`]). A stream's reader takes the bytes up to the newline and
/// bounds them, and nothing else.
pub const LINE_MAX: usize = 256 * 1024;
/// The deepest nesting a line may hold; the record's own object is depth 1.
pub const DEPTH_MAX: usize = 8;
/// The longest word, in bytes.
pub const WORD_MAX: usize = 256;
/// The most needs one `need` record names: the bound of `meta set harness_needs`.
pub const NEEDS_MAX: usize = 32;
/// The stream's version, `v=` on the verb and on its `OK` line.
pub const STREAM_VERSION: u32 = 1;
/// How often a harness with an open stream sends `beat`, whether or not anything changed.
pub const BEAT_EVERY_MS: u64 = 10_000;
/// How many beats aterm misses before the tab reads `unknown` and the watchdog acts.
pub const BEATS_MISSED: u32 = 3;
/// The records a stream may send each second, on average.
pub const RATE_PER_S: u32 = 50;
/// The records a stream may send at once, above [`RATE_PER_S`].
pub const RATE_BURST: u32 = 500;
/// How long a claim outlives its connection, keyed on the claimant's pid and birth, which an
/// `execve` keeps: twice the claim's 20 s renewal, plus 5 s.
pub const REEXEC_GRACE_MS: u64 = 45_000;
/// The shortest gap between two `hand` records; each carries the latest values.
pub const HAND_EVERY_MS: u64 = 100;

/// One line of the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The sender's count, from 1 with no gaps; on an `ack`, the harness record acknowledged.
    pub seq: u64,
    /// The sender's clock, milliseconds since 1970, as the caller passes it. Only shown.
    pub ms: u64,
    /// The kind and its fields.
    pub body: Body,
}

/// A line read whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// The record.
    pub record: Record,
    /// The fields of a known kind this library does not know, at every depth: passed over and
    /// counted. A record of an unknown kind counts none; the kind is what is counted.
    pub unknown: u32,
}

/// Why a line is not a record this library reads, or a record cannot be written. Its text is one
/// journal word group, as `claim::ReplyError`'s is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineError {
    /// The line is longer than [`LINE_MAX`]. Read: its length, and it was not parsed. Written:
    /// the length the line had reached, past the bound, when writing stopped.
    Bound(usize),
    /// The line is not UTF-8.
    Utf8,
    /// The line is not one JSON object: a syntax fault, an escape that is not JSON's, a lone
    /// surrogate, a raw character a string must escape, or a value other than an object. The byte
    /// where reading stopped.
    Syntax(usize),
    /// The line nests deeper than [`DEPTH_MAX`].
    Depth,
    /// A required field is absent.
    Missing(&'static str),
    /// A field is not in its grammar.
    Malformed(&'static str),
    /// A field appears more than once in one object, or a word more than once in a list that
    /// names each once, so it has no one value.
    Repeated(&'static str),
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bound(len) => write!(f, "line=bound len={len}"),
            Self::Utf8 => f.write_str("line=utf8"),
            Self::Syntax(at) => write!(f, "line=syntax at={at}"),
            Self::Depth => f.write_str("line=depth"),
            Self::Missing(key) => write!(f, "{key}=absent"),
            Self::Malformed(key) => write!(f, "{key}=malformed"),
            Self::Repeated(key) => write!(f, "{key}=repeated"),
        }
    }
}

impl std::error::Error for LineError {}

/// Who sends a kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// The harness: the design's "Harness to aterm" table.
    Harness,
    /// aterm: the design's "aterm to harness" table.
    Aterm,
}

/// What a claim must hold for aterm to apply a harness record of a kind: the design's scope
/// table. A record its claim's grant does not cover is acknowledged `dropped=scope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Needs {
    /// None beyond an open contract-4 stream: `session`, `claim`, `model`, `upgrade` and `beat`,
    /// `title`, the answers `took` and `refused`, and every record aterm sends.
    Stream,
    /// This scope.
    Scope(Scope),
    /// `approvals` or `questions`, by the ask's kind: `ask` and `asked`.
    AskKind,
    /// The scope of the act it replaces (section 2.2's act table): an `act` taken by a rule
    /// needs what aterm's act table names for its rule (`acts::act_scope`, which this library
    /// does not hold), `wait` needs `state` (`harness_activity`) and `held` needs `turn-end`
    /// (`harness_held`); `need` and `clear` take any of `approvals`, `questions` or `walls`
    /// (`harness_needs`), and `land` takes `turn-end` (`harness_land`).
    ActScope,
}

/// Declares [`Kind`], its words and [`Kind::ALL`] from one list.
macro_rules! kinds {
    ($($variant:ident $word:literal $side:ident,)*) => {
        /// A record's kind: every kind in the design's two tables, harness to aterm first.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum Kind {
            $(#[doc = concat!("`", $word, "`")] $variant,)*
        }

        impl Kind {
            /// Every kind, in the order of the design's tables.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];

            /// The word `k` spells.
            #[must_use]
            pub const fn word(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)*
                }
            }

            /// The kind `word` names, if this library knows it.
            #[must_use]
            pub fn from_word(word: &str) -> Option<Self> {
                match word {
                    $($word => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// Who sends it.
            #[must_use]
            pub const fn sent_by(self) -> Side {
                match self {
                    $(Self::$variant => Side::$side,)*
                }
            }
        }
    };
}

kinds! {
    Session "session" Harness,
    Claim "claim" Harness,
    State "state" Harness,
    Beat "beat" Harness,
    Turn "turn" Harness,
    Msg "msg" Harness,
    Tool "tool" Harness,
    Diff "diff" Harness,
    Subagent "subagent" Harness,
    Title "title" Harness,
    Progress "progress" Harness,
    Goal "goal" Harness,
    Compact "compact" Harness,
    Ask "ask" Harness,
    Asked "asked" Harness,
    Usage "usage" Harness,
    Account "account" Harness,
    Wall "wall" Harness,
    Model "model" Harness,
    Act "act" Harness,
    Need "need" Harness,
    Clear "clear" Harness,
    Land "land" Harness,
    Upgrade "upgrade" Harness,
    Took "took" Harness,
    Refused "refused" Harness,
    Ack "ack" Aterm,
    Prompt "prompt" Aterm,
    Answer "answer" Aterm,
    Touch "touch" Aterm,
    Hand "hand" Aterm,
    Interrupt "interrupt" Aterm,
    Set "set" Aterm,
    Restart "restart" Aterm,
    Policy "policy" Aterm,
    Want "want" Aterm,
}

impl Kind {
    /// What a claim must hold for aterm to apply a record of this kind (the design's scope
    /// table). `title`, which that table does not list, needs only the stream: a title is a
    /// description claim ranked by its author, not a duty taken from aterm's engine.
    /// [`Body::admitted`] reads a record's own kind of ask or act.
    #[must_use]
    pub const fn needs(self) -> Needs {
        match self {
            Self::Ask | Self::Asked => Needs::AskKind,
            Self::Wall | Self::Usage => Needs::Scope(Scope::Walls),
            Self::Account => Needs::Scope(Scope::Accounts),
            Self::State
            | Self::Turn
            | Self::Tool
            | Self::Subagent
            | Self::Msg
            | Self::Diff
            | Self::Progress
            | Self::Goal
            | Self::Compact => Needs::Scope(Scope::State),
            Self::Act | Self::Need | Self::Clear | Self::Land => Needs::ActScope,
            Self::Session
            | Self::Claim
            | Self::Model
            | Self::Upgrade
            | Self::Beat
            | Self::Title
            | Self::Took
            | Self::Refused
            | Self::Ack
            | Self::Prompt
            | Self::Answer
            | Self::Touch
            | Self::Hand
            | Self::Interrupt
            | Self::Set
            | Self::Restart
            | Self::Policy
            | Self::Want => Needs::Stream,
        }
    }
}

/// Declares a closed word list: the enum, its words and `ALL`.
macro_rules! words {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident $word:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)*
        }

        impl $name {
            /// Every word, in the design's order.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];

            /// The wire word.
            #[must_use]
            pub const fn word(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)*
                }
            }

            /// The value `word` names.
            #[must_use]
            pub fn from_word(word: &str) -> Option<Self> {
                match word {
                    $($word => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

words! {
    /// What the agent is doing (`state`). aterm writes it as `mark agent`.
    State {
        /// `starting`: read as `busy` with the detail `starting`.
        Starting "starting",
        /// `idle`.
        Idle "idle",
        /// `working`: read as `busy`.
        Working "working",
        /// `asking`: an `ask` is open; `prompt` for an approval, `question` for a question.
        Asking "asking",
        /// `walled`: a limit stops it; `wall:<kind>` from the `wall` record.
        Walled "walled",
        /// `ended`: read as `-`.
        Ended "ended",
    }
}

words! {
    /// What the agent's input box holds.
    Composer {
        /// `empty`.
        Empty "empty",
        /// `draft`: a person is writing; the harness makes no automatic prompt or answer.
        Draft "draft",
    }
}

words! {
    /// How the vendor runs.
    Mode {
        /// `tui`: the vendor's own screen in the tab.
        Tui "tui",
        /// `headless`: no vendor screen; aterm's conversation page.
        Headless "headless",
    }
}

words! {
    /// Where a turn is (`turn`): how aterm's turn closes, as its screen and transcript readers
    /// close one.
    TurnState {
        /// `started`.
        Started "started",
        /// `done`: the agent came to rest.
        Done "done",
        /// `failed`: an error ended it.
        Failed "failed",
        /// `interrupted`: it was stopped (an `interrupt`, or a person's Esc).
        Interrupted "interrupted",
        /// `walled`: it ended on a wall, whose kind the `wall` record names.
        Walled "walled",
    }
}

words! {
    /// Where a tool call or a subagent is (`tool`, `subagent`).
    Step {
        /// `started`.
        Started "started",
        /// `done`.
        Done "done",
        /// `failed`: it ended without doing what it set out to.
        Failed "failed",
        /// `interrupted`: it was stopped with its turn.
        Interrupted "interrupted",
    }
}

words! {
    /// Where a compaction is (`compact`): aterm's compact step opens and closes, and never fails.
    CompactState {
        /// `started`.
        Started "started",
        /// `done`.
        Done "done",
    }
}

words! {
    /// Where one item of the plan is (`progress`).
    ItemState {
        /// `pending`.
        Pending "pending",
        /// `active`: being worked on.
        Active "active",
        /// `done`.
        Done "done",
    }
}

words! {
    /// What an `ask` asks for.
    AskKind {
        /// `approval`: a permission, trust or plan box; the scope `approvals`.
        Approval "approval",
        /// `question`: a question dialog; the scope `questions`.
        Question "question",
    }
}

words! {
    /// How an `ask` was answered (`asked`), as aterm logs it (`word=`).
    Reply {
        /// `approve`.
        Approve "approve",
        /// `deny`.
        Deny "deny",
        /// `answer`: an option or text that is neither.
        Answer "answer",
    }
}

words! {
    /// Why a harness waits (`act` `wait`), as `meta set harness_activity` words it.
    WaitReason {
        /// `person`: after a person's input (aterm says "waiting after input").
        Person "person",
        /// `composing`: a person is writing.
        Composing "composing",
        /// `dialog`: a dialog is open.
        Dialog "dialog",
        /// `paused`: a person paused it.
        Paused "paused",
        /// `working`: the agent works.
        Working "working",
        /// `ready`: aterm words it as working.
        Ready "ready",
        /// `retry`: it is retrying.
        Retry "retry",
        /// `waiting`: anything else.
        Waiting "waiting",
    }
}

words! {
    /// Why a harness's own typing stopped (`act` `held`), as `meta set harness_held` words it.
    HeldBy {
        /// `same-reply`: the newest reply came again with no work.
        SameReply "same-reply",
        /// `no-work`: no-work answers in a row, whatever their wording.
        NoWork "no-work",
    }
}

words! {
    /// What a `need` raises, as aterm's `mark need` words it.
    NeedKind {
        /// `review`.
        Review "review",
        /// `decision`.
        Decision "decision",
        /// `help`.
        Help "help",
    }
}

words! {
    /// Land's state (`land`).
    LandState {
        /// `held`: Land holds the task.
        Held "held",
        /// `landed`: the task is on the remote main branch.
        Landed "landed",
        /// `open`: the task is open and nothing holds it; a hold told before is lifted.
        Open "open",
    }
}

words! {
    /// Where a move to a newer vendor build is (`upgrade`).
    UpgradePhase {
        /// `seen`: a newer build is known.
        Seen "seen",
        /// `waiting`: for a safe point.
        Waiting "waiting",
        /// `moving`: the child is being restarted onto it.
        Moving "moving",
        /// `done`.
        Done "done",
        /// `abandoned`.
        Abandoned "abandoned",
    }
}

/// Who did something: opened a turn, said a message, answered an ask, or sent a prompt.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum By {
    /// `person`: a person, at the keyboard or on aterm's page.
    Person,
    /// `ctl`: an `aterm ctl` command.
    Ctl,
    /// `harness`: the harness itself, by its own rule.
    Harness,
    /// `agent`: the vendor's agent itself.
    Agent,
    /// `peer:<tab>`: another tab; the word after `peer:`.
    Peer(String),
}

impl By {
    /// The wire word.
    #[must_use]
    pub fn word(&self) -> String {
        match self {
            Self::Person => "person".to_string(),
            Self::Ctl => "ctl".to_string(),
            Self::Harness => "harness".to_string(),
            Self::Agent => "agent".to_string(),
            Self::Peer(tab) => format!("peer:{tab}"),
        }
    }

    /// The `By` a wire word names: one of the four words, or `peer:` and a word.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "person" => Some(Self::Person),
            "ctl" => Some(Self::Ctl),
            "harness" => Some(Self::Harness),
            "agent" => Some(Self::Agent),
            _ => word
                .strip_prefix("peer:")
                .filter(|tab| is_word(tab))
                .map(|tab| Self::Peer(tab.to_string())),
        }
    }

    /// Whether aterm's records may name it: `person`, `ctl` or `peer:<tab>`, who send a
    /// `prompt` or an `answer` from outside the harness.
    #[must_use]
    pub const fn outside(&self) -> bool {
        matches!(self, Self::Person | Self::Ctl | Self::Peer(_))
    }
}

/// Why aterm did not apply a harness record (`ack`'s `dropped`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Dropped {
    /// `bound`: the line was longer than [`LINE_MAX`].
    Bound,
    /// `rate`: over the stream's budget ([`RATE_PER_S`], [`RATE_BURST`]).
    Rate,
    /// `scope`: the claim's grant does not cover it ([`Body::admitted`]).
    Scope,
    /// `applied`: its `seq` was applied before (a resend after a reconnect).
    Applied,
    /// `refused:<word>`: aterm refused what it asked, with the `ERR` word.
    Refused(String),
    /// Any other word: dropped for a reason this library does not know.
    Other(String),
}

impl Dropped {
    /// The wire word.
    #[must_use]
    pub fn word(&self) -> String {
        match self {
            Self::Bound => "bound".to_string(),
            Self::Rate => "rate".to_string(),
            Self::Scope => "scope".to_string(),
            Self::Applied => "applied".to_string(),
            Self::Refused(why) => format!("refused:{why}"),
            Self::Other(word) => word.clone(),
        }
    }

    /// The `Dropped` a word names; `None` when `word` is not a word.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        if !is_word(word) {
            return None;
        }
        Some(match word {
            "bound" => Self::Bound,
            "rate" => Self::Rate,
            "scope" => Self::Scope,
            "applied" => Self::Applied,
            _ => match word.strip_prefix("refused:") {
                Some(why) if !why.is_empty() => Self::Refused(why.to_string()),
                _ => Self::Other(word.to_string()),
            },
        })
    }
}

/// One item of the plan (`progress`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Item {
    /// What it is.
    pub text: String,
    /// Where it is.
    pub state: ItemState,
}

/// One limit (`usage`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Limit {
    /// Which, as the vendor names it (for example `session` or `weekly`).
    pub name: String,
    /// How much of it is used, in percent.
    pub used: Option<u8>,
    /// When it resets, in milliseconds since 1970.
    pub resets_ms: Option<u64>,
}

/// One model on offer and its levels (`model`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Offer {
    /// The model, as `set` names it: a model word.
    pub model: String,
    /// Its effort levels, as `set` names them.
    pub levels: Vec<String>,
}

/// What an `act` record says the harness did. Each act carries its own fields only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Did {
    /// An act a rule took (`meta set harness_act`). aterm admits it by the scope its act table
    /// names for the rule.
    Rule {
        /// What it did (`act`): a word, neither [`Did::WAIT`] nor [`Did::HELD`].
        act: String,
        /// The rule it acted by (`rule`): the `<act>` of `meta set harness_act <act>`.
        rule: String,
    },
    /// `wait`: the continuation observation `meta set harness_activity` carries. Scope `state`.
    Wait {
        /// Why it waits.
        reason: WaitReason,
        /// How long it waits; `None` for no bound (`wait_ms=-`).
        wait_ms: Option<u64>,
    },
    /// `held`: its own typing stopped or goes on (`meta set` or `unset harness_held`). Scope
    /// `turn-end`.
    Held {
        /// Why its typing stopped; `None` when it goes on again.
        held: Option<HeldBy>,
    },
}

impl Did {
    /// The act of [`Did::Wait`].
    pub const WAIT: &'static str = "wait";
    /// The act of [`Did::Held`].
    pub const HELD: &'static str = "held";

    /// The `act` word.
    #[must_use]
    pub fn act(&self) -> &str {
        match self {
            Self::Rule { act, .. } => act,
            Self::Wait { .. } => Self::WAIT,
            Self::Held { .. } => Self::HELD,
        }
    }
}

/// Land's state (`land`), as `meta set harness_land` and `meta unset harness_land` tell it, and
/// the commit that landed. Each state carries its own fields only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Landing {
    /// `held`: Land holds the task.
    Held(LandHold),
    /// `landed`: the task is on the remote main branch.
    Landed {
        /// The commit that landed.
        commit: String,
    },
    /// `open`: nothing holds it; a hold told before is lifted.
    Open,
}

impl Landing {
    /// Its `state` word.
    #[must_use]
    pub const fn state(&self) -> LandState {
        match self {
            Self::Held(_) => LandState::Held,
            Self::Landed { .. } => LandState::Landed,
            Self::Open => LandState::Open,
        }
    }
}

/// Why Land holds the task (`land` `held`): the words of `meta set harness_land`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LandHold {
    /// When the act that began the hold was taken, in milliseconds since 1970 (`since=`).
    pub since_ms: u64,
    /// The nudges typed since (`nudges=`).
    pub nudges: u64,
    /// Land's read of the yield (`read=1` and its group), when it read one.
    pub read: Option<LandRead>,
}

/// What Land read at the yield that held the landing (`read=1`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LandRead {
    /// What held it (`legs=`): at least one word, aterm's hold legs (`needs`, `repo`, `open`,
    /// `defense`, `cut`, `repo-unread`, `left`).
    pub legs: Vec<String>,
    /// What the agent's list left open (`open=`), as Land's strict reading spelled it.
    pub open: Vec<String>,
    /// Changed files (`changed=`).
    pub changed: Option<u64>,
    /// Untracked files (`untracked=`).
    pub untracked: Option<u64>,
    /// Stashes (`stashes=`).
    pub stashes: Option<u64>,
    /// Commits no remote has (`unpushed=`).
    pub unpushed: Option<u64>,
    /// Commits not on the remote main branch (`unlanded=`).
    pub unlanded: Option<u64>,
    /// Processes the agent left running, unkept (`left=`).
    pub left: Option<u64>,
}

/// A record's kind and fields. The module documentation lists each kind's fields and which are
/// required.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Body {
    /// `session`: the agent, the versions, the account, the conversation and how to resume it.
    Session {
        /// The agent word.
        agent: String,
        /// The harness's version, in the claim's `version=` grammar.
        version: String,
        /// The vendor CLI's version, in the claim's `vendor=` grammar.
        vendor: Option<String>,
        /// The account it runs on: an account label, the claim's `login=`.
        account: Option<String>,
        /// The vendor's conversation.
        conversation: Option<String>,
        /// The working directory.
        cwd: Option<String>,
        /// The arguments that resume this conversation.
        resume: Vec<String>,
        /// How the vendor runs.
        mode: Mode,
        /// The kinds of aterm's records it takes ([`Side::Aterm`] kinds only).
        takes: Vec<Kind>,
    },
    /// `claim`: the claim renewed on the stream's connection, each word in the grammar of the
    /// claim word aterm builds from it.
    Claim {
        /// The vendor process the claim keeps (`child=`).
        child: Option<ChildProcess>,
        /// The narrowed scope, each word once (`scope=`).
        scope: Option<Vec<Scope>>,
        /// The harness's version (`version=`).
        version: Option<String>,
        /// The vendor CLI's version (`vendor=`).
        vendor: Option<String>,
        /// The account label the child runs on (`login=`).
        login: Option<String>,
    },
    /// `state`: what the agent is doing.
    State {
        /// The state.
        state: State,
        /// What the input box holds, in a TUI tab.
        composer: Option<Composer>,
    },
    /// `beat`: the harness lives.
    Beat,
    /// `turn`: a turn started or ended.
    Turn {
        /// The harness's count of turns in this conversation.
        turn: u64,
        /// Where it is.
        state: TurnState,
        /// Who opened it; always named when it starts.
        by: Option<By>,
    },
    /// `msg`: text the conversation holds.
    Msg {
        /// The turn it belongs to.
        turn: Option<u64>,
        /// Who said it.
        by: By,
        /// What was said.
        text: String,
    },
    /// `tool`: a tool call and its state.
    Tool {
        /// The turn it belongs to.
        turn: Option<u64>,
        /// The call, as the vendor names it.
        id: String,
        /// The tool.
        name: String,
        /// Where it is.
        state: Step,
        /// What it ran, in one line.
        text: Option<String>,
    },
    /// `diff`: a patch the session made.
    Diff {
        /// The turn it belongs to.
        turn: Option<u64>,
        /// The file it changes, when it changes one.
        path: Option<String>,
        /// The patch.
        patch: String,
    },
    /// `subagent`: a subagent inside the agent started or ended.
    Subagent {
        /// The subagent, as the vendor names it.
        id: String,
        /// Its title.
        title: Option<String>,
        /// Where it is.
        state: Step,
    },
    /// `title`: the conversation's title and a one-line summary.
    Title {
        /// The title.
        title: Option<String>,
        /// The summary.
        summary: Option<String>,
    },
    /// `progress`: the plan's items and their state.
    Progress {
        /// The items, in the plan's order.
        items: Vec<Item>,
    },
    /// `goal`: the active goal.
    Goal {
        /// The goal; `None` when no goal is active.
        text: Option<String>,
    },
    /// `compact`: the vendor compacts its context (aterm's transcript compact step).
    Compact {
        /// Where it is.
        state: CompactState,
        /// The context left, in percent.
        context_left: Option<u8>,
    },
    /// `ask`: a permission box or a question.
    Ask {
        /// What it asks for.
        kind: AskKind,
        /// Its title.
        title: Option<String>,
        /// Its options, in order.
        options: Vec<String>,
        /// What the harness will answer; `None` when it waits for a person.
        will: Option<String>,
        /// How long after this record the harness will answer.
        will_in_ms: Option<u64>,
    },
    /// `asked`: who answered an `ask`, and with what.
    Asked {
        /// The `seq` of the ask.
        ask: u64,
        /// What the ask asked for.
        kind: AskKind,
        /// Who answered.
        by: By,
        /// How.
        reply: Reply,
        /// The option chosen.
        option: Option<String>,
        /// The text given.
        text: Option<String>,
    },
    /// `usage`: one account's limits with reset times, and the context left.
    Usage {
        /// The account the limits are of: an account label.
        account: String,
        /// The limits.
        limits: Vec<Limit>,
        /// The context left, in percent.
        context_left: Option<u8>,
    },
    /// `account`: the account and its state.
    Account {
        /// The account: an account label.
        account: String,
        /// Its state, as the harness words it.
        state: String,
    },
    /// `wall`: the kind of wall.
    Wall {
        /// The kind, in aterm's wall words.
        kind: String,
        /// When it lifts, in milliseconds since 1970.
        resets_ms: Option<u64>,
    },
    /// `model`: the model, its effort and what is on offer, each only as the vendor states it.
    Model {
        /// The model's name.
        model: Option<String>,
        /// Its effort level.
        effort: Option<String>,
        /// Whether the vendor's top-effort key is on; `None` when the vendor did not say.
        key: Option<bool>,
        /// The models and levels on offer.
        offers: Vec<Offer>,
        /// The permission mode.
        permission: Option<String>,
    },
    /// `act`: what the harness did and by which rule.
    Act {
        /// The act and its fields.
        did: Did,
    },
    /// `need`: what the harness needs from a person.
    Need {
        /// The need it raises.
        kind: NeedKind,
        /// The entry it raised.
        entry: String,
        /// Each need the entry stands for, in order: 1 to [`NEEDS_MAX`].
        needs: Vec<String>,
    },
    /// `clear`: a need that ended.
    Clear {
        /// The `seq` of the `need` record.
        need: u64,
    },
    /// `land`: Land's state.
    Land {
        /// The state and its fields.
        state: Landing,
    },
    /// `upgrade`: a move to a newer vendor build.
    Upgrade {
        /// Where it is.
        phase: UpgradePhase,
        /// The target version.
        to: String,
    },
    /// `took`: the harness took an aterm record.
    Took {
        /// The aterm record's `seq` (`in`).
        of: u64,
    },
    /// `refused`: the harness cannot act on an aterm record.
    Refused {
        /// The aterm record's `seq` (`in`).
        of: u64,
        /// Why.
        why: String,
    },
    /// `ack`: aterm acknowledges the harness record [`Record::seq`] names.
    Ack {
        /// Why it was not applied; `None` when it was.
        dropped: Option<Dropped>,
    },
    /// `prompt`: text for the agent.
    Prompt {
        /// The text.
        text: String,
        /// Who sent it ([`By::outside`]).
        by: By,
    },
    /// `answer`: a choice on an `ask`, an option or a text.
    Answer {
        /// The `seq` of the ask.
        ask: u64,
        /// The option chosen.
        option: Option<String>,
        /// The text given.
        text: Option<String>,
        /// Who chose ([`By::outside`]).
        by: By,
        /// aterm's `human_seq` read at the click, which fences the harness's press.
        human_seq: u64,
    },
    /// `touch`: a person has put a hand on an ask; the harness holds its automatic answer.
    Touch {
        /// The `seq` of the ask.
        ask: u64,
    },
    /// `hand`: what a person is doing in the tab.
    Hand {
        /// aterm's count of a person's keys.
        human_seq: u64,
        /// What the input box holds.
        composer: Composer,
        /// Whether a halt stands (`hold` 1).
        hold: bool,
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
    /// `restart`: restart the child, as the claim's [`crate::claim::Restart`] says it.
    Restart {
        /// Why: a word of 1 to 64 ASCII letters, digits, `-` and `_`.
        cause: String,
        /// The account to start it on (`to` and `home`), only for the cause `account` or
        /// `account-ahead`.
        account: Option<Account>,
        /// The model to start it on: a model word.
        model: Option<String>,
    },
    /// `policy`: the person's policy or the claim's grant changed; read it again.
    Policy,
    /// `want`: send the conversation again.
    Want,
    /// A kind this library does not know: counted, never refused.
    Unknown {
        /// Its `k`: a word.
        k: String,
    },
}

impl Body {
    /// Its kind; `None` for [`Body::Unknown`].
    #[must_use]
    pub const fn kind(&self) -> Option<Kind> {
        Some(match self {
            Self::Session { .. } => Kind::Session,
            Self::Claim { .. } => Kind::Claim,
            Self::State { .. } => Kind::State,
            Self::Beat => Kind::Beat,
            Self::Turn { .. } => Kind::Turn,
            Self::Msg { .. } => Kind::Msg,
            Self::Tool { .. } => Kind::Tool,
            Self::Diff { .. } => Kind::Diff,
            Self::Subagent { .. } => Kind::Subagent,
            Self::Title { .. } => Kind::Title,
            Self::Progress { .. } => Kind::Progress,
            Self::Goal { .. } => Kind::Goal,
            Self::Compact { .. } => Kind::Compact,
            Self::Ask { .. } => Kind::Ask,
            Self::Asked { .. } => Kind::Asked,
            Self::Usage { .. } => Kind::Usage,
            Self::Account { .. } => Kind::Account,
            Self::Wall { .. } => Kind::Wall,
            Self::Model { .. } => Kind::Model,
            Self::Act { .. } => Kind::Act,
            Self::Need { .. } => Kind::Need,
            Self::Clear { .. } => Kind::Clear,
            Self::Land { .. } => Kind::Land,
            Self::Upgrade { .. } => Kind::Upgrade,
            Self::Took { .. } => Kind::Took,
            Self::Refused { .. } => Kind::Refused,
            Self::Ack { .. } => Kind::Ack,
            Self::Prompt { .. } => Kind::Prompt,
            Self::Answer { .. } => Kind::Answer,
            Self::Touch { .. } => Kind::Touch,
            Self::Hand { .. } => Kind::Hand,
            Self::Interrupt => Kind::Interrupt,
            Self::Set { .. } => Kind::Set,
            Self::Restart { .. } => Kind::Restart,
            Self::Policy => Kind::Policy,
            Self::Want => Kind::Want,
            Self::Unknown { .. } => return None,
        })
    }

    /// The word `k` spells.
    #[must_use]
    pub fn word(&self) -> &str {
        match self {
            Self::Unknown { k } => k,
            known => known.kind().map_or("", Kind::word),
        }
    }

    /// Whether a claim granted `granted` covers this record: [`Kind::needs`], with an ask's own
    /// kind and an act's own act. `act_scope` is aterm's act table: the scope it names for a
    /// rule (`acts::act_scope`), `None` for a rule no duty names, which no grant covers. A record
    /// of an unknown kind needs nothing beyond the stream; it is counted, not applied.
    #[must_use]
    pub fn admitted(
        &self,
        granted: &[Scope],
        act_scope: impl FnOnce(&str) -> Option<Scope>,
    ) -> bool {
        let any = |scopes: &[Scope]| scopes.iter().any(|scope| granted.contains(scope));
        match self {
            Self::Ask { kind, .. } | Self::Asked { kind, .. } => any(&[match kind {
                AskKind::Approval => Scope::Approvals,
                AskKind::Question => Scope::Questions,
            }]),
            Self::Act { did } => match did {
                Did::Rule { rule, .. } => act_scope(rule).is_some_and(|scope| any(&[scope])),
                Did::Wait { .. } => any(&[Scope::State]),
                Did::Held { .. } => any(&[Scope::TurnEnd]),
            },
            Self::Need { .. } | Self::Clear { .. } => {
                any(&[Scope::Approvals, Scope::Questions, Scope::Walls])
            }
            Self::Land { .. } => any(&[Scope::TurnEnd]),
            other => match other.kind().map_or(Needs::Stream, Kind::needs) {
                Needs::Scope(scope) => any(&[scope]),
                Needs::Stream => true,
                Needs::AskKind | Needs::ActScope => false,
            },
        }
    }
}

/// Whether `text` is a word: 1 to [`WORD_MAX`] bytes of printable ASCII with no space.
#[must_use]
pub fn is_word(text: &str) -> bool {
    (1..=WORD_MAX).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_graphic())
}

/// The verb that opens session `sid`'s stream on the claim's own connection,
/// `stream @<sid> v=1`, answered by a [`StreamReply`]. Verb first, the way
/// `subscribe @<sid> events` is spelled. `None` when `sid` is not an `s-<hex>` session ID.
#[must_use]
pub fn stream_request(sid: &str) -> Option<String> {
    let session = sid
        .strip_prefix("s-")
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    session.then(|| format!("stream @{sid} v={STREAM_VERSION}"))
}

/// The longest `epoch=` word.
const EPOCH_MAX: usize = 128;

/// aterm's answer to [`stream_request`]: `OK stream v=1 epoch=<id> acked=<seq> in=<seq>`. After
/// it each side sends one record per line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamReply {
    /// `epoch=`: the aterm instance that answered. A stream is `(claimant, epoch)`; a new epoch
    /// resets which of aterm's records the harness has seen.
    pub epoch: String,
    /// `acked=`: the last harness record aterm acknowledged. The harness sends again from the
    /// record after it.
    pub acked: u64,
    /// `in=`: the last record aterm sent.
    pub sent: u64,
}

impl StreamReply {
    /// Read the `OK` line. `v=` must be [`STREAM_VERSION`], the version [`stream_request`] asked
    /// for; `epoch=` (1 to 128 bytes of ASCII letters, digits, `-`, `_` and `.`), `acked=` and
    /// `in=` (canonical decimals) must each appear once. Another `key=value` word is passed over,
    /// as aterm grows a reply by adding words.
    ///
    /// # Errors
    /// The reply is not an `OK stream` line (an `ERR` from an aterm without the verb, or
    /// `ERR busy streams`), or a field is absent, repeated or malformed.
    pub fn parse(reply: &str) -> Result<Self, ReplyError> {
        let mut words = reply.split_ascii_whitespace();
        if words.next() != Some("OK") || words.next() != Some("stream") {
            return Err(ReplyError::refused(reply));
        }
        let (mut version, mut epoch, mut acked, mut sent) = (None, None, None, None);
        for word in words {
            let Some((key, value)) = word.split_once('=') else {
                continue;
            };
            let (slot, name) = match key {
                "v" => (&mut version, "v"),
                "epoch" => (&mut epoch, "epoch"),
                "acked" => (&mut acked, "acked"),
                "in" => (&mut sent, "in"),
                _ => continue,
            };
            if slot.replace(value).is_some() {
                return Err(ReplyError::Repeated(name));
            }
        }
        let version = version.ok_or(ReplyError::Missing("v"))?;
        if canonical_u64(version) != Some(u64::from(STREAM_VERSION)) {
            return Err(ReplyError::Malformed("v"));
        }
        let epoch = epoch.ok_or(ReplyError::Missing("epoch"))?;
        let epoch_word = (1..=EPOCH_MAX).contains(&epoch.len())
            && epoch
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if !epoch_word {
            return Err(ReplyError::Malformed("epoch"));
        }
        let decimal = |value: Option<&str>, name: &'static str| {
            let value = value.ok_or(ReplyError::Missing(name))?;
            canonical_u64(value).ok_or(ReplyError::Malformed(name))
        };
        Ok(Self {
            epoch: epoch.to_string(),
            acked: decimal(acked, "acked")?,
            sent: decimal(sent, "in")?,
        })
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
