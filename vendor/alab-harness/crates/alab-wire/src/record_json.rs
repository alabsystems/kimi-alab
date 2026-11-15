// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The record codec: a bounded JSON writer and reader for [`Record`], written by hand, with no
//! dependency. It is the one parser a harness and aterm both run.
//!
//! * **Bounded.** A line longer than [`LINE_MAX`] is refused before a byte of it is parsed, and
//!   one nested deeper than [`DEPTH_MAX`] is refused where the depth is passed, so reading
//!   recurses at most that deep. The writer stops once its line passes [`LINE_MAX`], so a record
//!   too long to send costs no more than the bound to refuse.
//! * **Strict JSON.** The line must be UTF-8 and one JSON object. An escape is one of JSON's, a
//!   `\u` surrogate only in a high-low pair.
//! * **Escaped.** A string holds raw none of the characters aterm's hidden-character table names
//!   (`aterm-types` `hidden_char.rs`: C0 and C1 controls, DEL, noncharacters, and the format
//!   characters that re-order or hide text, such as bidi controls, zero-width characters and the
//!   line and paragraph separators): the writer escapes each one, with `"` and `\`, and the
//!   reader refuses one written raw. So no escape the writer makes is longer than what it was
//!   read from, and a line that reads writes back within the bound, equal.
//! * **Exact.** A known field appears once in its object and in its grammar, or the line is
//!   refused. A field of a known kind this library does not know is passed over and counted, at
//!   every depth; a kind it does not know reads as [`Body::Unknown`].
//! * **Total.** Reading never panics: every input is a record or a [`LineError`].

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::claim::{
    Account, ChildProcess, Scope, account_cause, canonical_u64, claim_version, login_word,
    model_word, restart_cause, vendor_word,
};
use crate::record::{
    AskKind, Body, By, CompactState, Composer, DEPTH_MAX, Decoded, Did, Dropped, HeldBy, Item,
    ItemState, Kind, LINE_MAX, LandHold, LandRead, LandState, Landing, Limit, LineError, Mode,
    NEEDS_MAX, NeedKind, Offer, Record, Reply, Side, State, Step, TurnState, UpgradePhase,
    WaitReason, is_word,
};

impl Record {
    /// The record as one line, without its newline: `k`, `seq` and `ms` first, then the kind's
    /// fields in the order the module documentation lists them. An absent field and an empty
    /// list are left out (a `claim`'s narrowed `scope` is written even when empty).
    ///
    /// # Errors
    /// A field is not in its grammar, so the line would not read back as this record (`seq` 0,
    /// a word with a space, a percent over 100, a scope word or a `takes` kind named twice, an
    /// `answer` with both or neither of `option` and `text`, an unknown `k` that is not a word
    /// or spells a known kind), or the line would be longer than [`LINE_MAX`].
    pub fn line(&self) -> Result<String, LineError> {
        let mut out = Out {
            line: String::new(),
            first: true,
            fault: None,
        };
        out.line.push('{');
        out.text("k", self.body.word());
        out.count("seq", self.seq);
        out.number("ms", self.ms);
        out.body(&self.body);
        out.line.push('}');
        if let Some(fault) = out.fault {
            return Err(fault);
        }
        if out.line.len() > LINE_MAX {
            return Err(LineError::Bound(out.line.len()));
        }
        Ok(out.line)
    }

    /// Read one line: the bytes before its newline, every one of them, a `\r` among them, counted
    /// against [`LINE_MAX`] (a `\r` within the bound is JSON whitespace).
    ///
    /// # Errors
    /// The line is longer than [`LINE_MAX`] (refused before it is parsed), is not UTF-8, is not
    /// one JSON object, nests deeper than [`DEPTH_MAX`], lacks `k` (a word), `seq` (from 1) or
    /// `ms`, or a known kind's field is absent where required, repeated, not in its grammar, or
    /// on a record whose other fields leave it no place.
    pub fn read(line: &[u8]) -> Result<Decoded, LineError> {
        if line.len() > LINE_MAX {
            return Err(LineError::Bound(line.len()));
        }
        let source = std::str::from_utf8(line).map_err(|_| LineError::Utf8)?;
        let mut parser = Parser {
            text: source,
            at: 0,
        };
        let value = parser.value(0)?;
        parser.space();
        if parser.at != source.len() {
            return Err(LineError::Syntax(parser.at));
        }
        let Value::Object(entries) = value else {
            return Err(LineError::Syntax(0));
        };
        let mut object = Object::new(entries);
        let k = required("k", object.take("k")?, word)?;
        let seq = required("seq", object.take("seq")?, ordinal)?;
        let ms = required("ms", object.take("ms")?, count)?;
        let Some(kind) = Kind::from_word(&k) else {
            let body = Body::Unknown { k };
            let record = Record { seq, ms, body };
            return Ok(Decoded { record, unknown: 0 });
        };
        let mut unknown = 0;
        let body = body(kind, &mut object, &mut unknown)?;
        let unknown = unknown.saturating_add(object.unknown());
        let record = Record { seq, ms, body };
        Ok(Decoded { record, unknown })
    }
}

/// Whether aterm's hidden-character table names `c` (`aterm-types` `hidden_char.rs`,
/// `is_hidden_char`, copied whole): the writer escapes each one, and the reader refuses one raw.
pub(crate) const fn hidden(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
        || matches!(
            c,
            '\u{00ad}'
                | '\u{034f}'
                | '\u{061c}'
                | '\u{115f}'..='\u{1160}'
                | '\u{17b4}'..='\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{200b}'
                | '\u{200e}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{3164}'
                | '\u{fdd0}'..='\u{fdef}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{fff0}'..='\u{fffb}'
                | '\u{13430}'..='\u{1343f}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0000}'..='\u{e007f}'
        )
        || (c as u32) & 0xfffe == 0xfffe
}

/// Whether `text` is an account label: the claim's `login=` ([`login_word`]).
fn account_word(text: &str) -> bool {
    login_word(Some(text)).is_some()
}

/// Whether `text` is a vendor CLI's version: the claim's `vendor=` ([`vendor_word`]).
fn vendor(text: &str) -> bool {
    vendor_word(Some(text)).is_some()
}

/// The home an account move names: `-` for the vendor's default (`None`), else an absolute path.
fn account_home(text: &str) -> Option<Option<PathBuf>> {
    if text == "-" {
        return Some(None);
    }
    Path::new(text)
        .is_absolute()
        .then(|| Some(PathBuf::from(text)))
}

/// The line being written, and the first field that was not in its grammar.
struct Out {
    line: String,
    first: bool,
    fault: Option<LineError>,
}

impl Out {
    fn fail(&mut self, fault: LineError) {
        self.fault.get_or_insert(fault);
    }

    /// Whether the line has passed [`LINE_MAX`]; once it has, it is refused and grows no more.
    fn full(&mut self) -> bool {
        let full = self.line.len() > LINE_MAX;
        if full {
            self.fail(LineError::Bound(self.line.len()));
        }
        full
    }

    /// `"key":`, after a comma unless it opens its object.
    fn key(&mut self, key: &str) {
        if !self.first {
            self.line.push(',');
        }
        self.first = false;
        self.line.push('"');
        self.line.push_str(key);
        self.line.push_str("\":");
    }

    fn put_text(&mut self, text: &str) {
        if self.full() {
            return;
        }
        self.line.push('"');
        for c in text.chars() {
            if self.full() {
                return;
            }
            match c {
                '"' => self.line.push_str("\\\""),
                '\\' => self.line.push_str("\\\\"),
                '\n' => self.line.push_str("\\n"),
                '\r' => self.line.push_str("\\r"),
                '\t' => self.line.push_str("\\t"),
                '\u{8}' => self.line.push_str("\\b"),
                '\u{c}' => self.line.push_str("\\f"),
                c if hidden(c) => {
                    let mut units = [0_u16; 2];
                    for unit in c.encode_utf16(&mut units) {
                        let _ = write!(self.line, "\\u{unit:04x}");
                    }
                }
                c => self.line.push(c),
            }
        }
        self.line.push('"');
    }

    /// `text`, which must pass `ok`, its grammar.
    fn put_checked(&mut self, key: &'static str, text: &str, ok: fn(&str) -> bool) {
        if !ok(text) {
            self.fail(LineError::Malformed(key));
        }
        self.put_text(text);
    }

    fn put_object(&mut self, fields: impl FnOnce(&mut Self)) {
        self.line.push('{');
        self.first = true;
        fields(self);
        self.line.push('}');
        self.first = false;
    }

    fn put_list<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Self, &T)) {
        self.line.push('[');
        for (at, item) in items.iter().enumerate() {
            if self.full() {
                return;
            }
            if at > 0 {
                self.line.push(',');
            }
            each(self, item);
        }
        self.line.push(']');
    }

    fn text(&mut self, key: &str, text: &str) {
        self.key(key);
        self.put_text(text);
    }

    fn checked(&mut self, key: &'static str, text: &str, ok: fn(&str) -> bool) {
        self.key(key);
        self.put_checked(key, text, ok);
    }

    fn word(&mut self, key: &'static str, word: &str) {
        self.checked(key, word, is_word);
    }

    fn number(&mut self, key: &str, number: u64) {
        self.key(key);
        let _ = write!(self.line, "{number}");
    }

    /// A count from 1: a `seq`, or the `seq` another record names.
    fn count(&mut self, key: &'static str, number: u64) {
        if number == 0 {
            self.fail(LineError::Malformed(key));
        }
        self.number(key, number);
    }

    fn opt_text(&mut self, key: &str, text: Option<&String>) {
        if let Some(text) = text {
            self.text(key, text);
        }
    }

    fn opt_checked(&mut self, key: &'static str, text: Option<&String>, ok: fn(&str) -> bool) {
        if let Some(text) = text {
            self.checked(key, text, ok);
        }
    }

    fn opt_word(&mut self, key: &'static str, word: Option<&String>) {
        self.opt_checked(key, word, is_word);
    }

    fn opt_number(&mut self, key: &str, number: Option<u64>) {
        if let Some(number) = number {
            self.number(key, number);
        }
    }

    fn opt_count(&mut self, key: &'static str, number: Option<u64>) {
        if let Some(number) = number {
            self.count(key, number);
        }
    }

    fn percent(&mut self, key: &'static str, percent: Option<u8>) {
        if let Some(percent) = percent {
            if percent > 100 {
                self.fail(LineError::Malformed(key));
            }
            self.number(key, u64::from(percent));
        }
    }

    fn by(&mut self, key: &'static str, by: &By) {
        let word = by.word();
        if By::from_word(&word).as_ref() != Some(by) {
            self.fail(LineError::Malformed(key));
        }
        self.text(key, &word);
    }

    /// The `by` of a record aterm sends: someone outside the harness ([`By::outside`]).
    fn sender(&mut self, key: &'static str, by: &By) {
        if !by.outside() {
            self.fail(LineError::Malformed(key));
        }
        self.by(key, by);
    }

    fn texts(&mut self, key: &str, texts: &[String]) {
        if !texts.is_empty() {
            self.key(key);
            self.put_list(texts, |out, text| out.put_text(text));
        }
    }

    fn words(&mut self, key: &'static str, words: &[String]) {
        if !words.is_empty() {
            self.key(key);
            self.put_list(words, |out, word| out.put_checked(key, word, is_word));
        }
    }

    fn option_or_text(&mut self, option: Option<&String>, text: Option<&String>) {
        self.opt_text("option", option);
        self.opt_text("text", text);
    }

    fn body(&mut self, body: &Body) {
        match body {
            Body::Session {
                agent,
                version,
                vendor: vendor_version,
                account,
                conversation,
                cwd,
                resume,
                mode,
                takes,
            } => {
                self.word("agent", agent);
                self.checked("version", version, claim_version);
                self.opt_checked("vendor", vendor_version.as_ref(), vendor);
                self.opt_checked("account", account.as_ref(), account_word);
                self.opt_word("conversation", conversation.as_ref());
                self.opt_text("cwd", cwd.as_ref());
                self.texts("resume", resume);
                self.text("mode", mode.word());
                if !once(takes) {
                    self.fail(LineError::Repeated("takes"));
                }
                if takes.iter().any(|kind| kind.sent_by() != Side::Aterm) {
                    self.fail(LineError::Malformed("takes"));
                }
                let words: Vec<String> = takes.iter().map(|kind| kind.word().into()).collect();
                self.words("takes", &words);
            }
            Body::Claim {
                child,
                scope,
                version,
                vendor: vendor_version,
                login,
            } => {
                if let Some(ChildProcess { pid, start }) = *child {
                    if !(2..=i32::MAX.unsigned_abs()).contains(&pid) || start == 0 {
                        self.fail(LineError::Malformed("child"));
                    }
                    self.key("child");
                    self.put_object(|out| {
                        out.number("pid", u64::from(pid));
                        out.number("start", start);
                    });
                }
                if let Some(scope) = scope {
                    if !once(scope) {
                        self.fail(LineError::Repeated("scope"));
                    }
                    self.key("scope");
                    self.put_list(scope, |out, word| out.put_text(word.word()));
                }
                self.opt_checked("version", version.as_ref(), claim_version);
                self.opt_checked("vendor", vendor_version.as_ref(), vendor);
                self.opt_checked("login", login.as_ref(), account_word);
            }
            Body::State { state, composer } => {
                self.text("state", state.word());
                if let Some(composer) = composer {
                    self.text("composer", composer.word());
                }
            }
            Body::Beat | Body::Interrupt | Body::Policy | Body::Want => {}
            Body::Turn { turn, state, by } => {
                self.count("turn", *turn);
                self.text("state", state.word());
                match by {
                    Some(by) => self.by("by", by),
                    None if *state == TurnState::Started => self.fail(LineError::Missing("by")),
                    None => {}
                }
            }
            Body::Msg { turn, by, text } => {
                self.opt_count("turn", *turn);
                self.by("by", by);
                self.text("text", text);
            }
            Body::Tool {
                turn,
                id,
                name,
                state,
                text,
            } => {
                self.opt_count("turn", *turn);
                self.word("id", id);
                self.word("name", name);
                self.text("state", state.word());
                self.opt_text("text", text.as_ref());
            }
            Body::Diff { turn, path, patch } => {
                self.opt_count("turn", *turn);
                self.opt_text("path", path.as_ref());
                self.text("patch", patch);
            }
            Body::Subagent { id, title, state } => {
                self.word("id", id);
                self.opt_text("title", title.as_ref());
                self.text("state", state.word());
            }
            Body::Title { title, summary } => {
                self.opt_text("title", title.as_ref());
                self.opt_text("summary", summary.as_ref());
            }
            Body::Progress { items } => {
                if !items.is_empty() {
                    self.key("items");
                    self.put_list(items, |out, item| {
                        out.put_object(|out| {
                            out.text("text", &item.text);
                            out.text("state", item.state.word());
                        });
                    });
                }
            }
            Body::Goal { text } => self.opt_text("text", text.as_ref()),
            Body::Compact {
                state,
                context_left,
            } => {
                self.text("state", state.word());
                self.percent("context_left", *context_left);
            }
            Body::Ask {
                kind,
                title,
                options,
                will,
                will_in_ms,
            } => {
                self.text("kind", kind.word());
                self.opt_text("title", title.as_ref());
                self.texts("options", options);
                self.opt_text("will", will.as_ref());
                self.opt_number("will_in_ms", *will_in_ms);
            }
            Body::Asked {
                ask,
                kind,
                by,
                reply,
                option,
                text,
            } => {
                self.count("ask", *ask);
                self.text("kind", kind.word());
                self.by("by", by);
                self.text("reply", reply.word());
                self.option_or_text(option.as_ref(), text.as_ref());
            }
            Body::Usage {
                account,
                limits,
                context_left,
            } => {
                self.checked("account", account, account_word);
                if !limits.is_empty() {
                    self.key("limits");
                    self.put_list(limits, |out, limit| {
                        out.put_object(|out| {
                            out.word("name", &limit.name);
                            out.percent("used", limit.used);
                            out.opt_number("resets_ms", limit.resets_ms);
                        });
                    });
                }
                self.percent("context_left", *context_left);
            }
            Body::Account { account, state } => {
                self.checked("account", account, account_word);
                self.word("state", state);
            }
            Body::Wall { kind, resets_ms } => {
                self.word("kind", kind);
                self.opt_number("resets_ms", *resets_ms);
            }
            Body::Model {
                model,
                effort,
                key,
                offers,
                permission,
            } => {
                self.opt_text("model", model.as_ref());
                self.opt_word("effort", effort.as_ref());
                if let Some(key) = key {
                    self.key("key");
                    self.line.push_str(if *key { "true" } else { "false" });
                }
                if !offers.is_empty() {
                    self.key("offers");
                    self.put_list(offers, |out, offer| {
                        out.put_object(|out| {
                            out.checked("model", &offer.model, model_word);
                            out.words("levels", &offer.levels);
                        });
                    });
                }
                self.opt_word("permission", permission.as_ref());
            }
            Body::Act { did } => self.did(did),
            Body::Need { kind, entry, needs } => {
                self.text("kind", kind.word());
                self.text("entry", entry);
                if needs.is_empty() {
                    self.fail(LineError::Missing("needs"));
                }
                if needs.len() > NEEDS_MAX {
                    self.fail(LineError::Malformed("needs"));
                }
                self.texts("needs", needs);
            }
            Body::Clear { need } => self.count("need", *need),
            Body::Land { state } => self.landing(state),
            Body::Upgrade { phase, to } => {
                self.text("phase", phase.word());
                self.word("to", to);
            }
            Body::Took { of } => self.count("in", *of),
            Body::Refused { of, why } => {
                self.count("in", *of);
                self.text("why", why);
            }
            Body::Ack { dropped } => {
                if let Some(dropped) = dropped {
                    let word = dropped.word();
                    if Dropped::from_word(&word).as_ref() != Some(dropped) {
                        self.fail(LineError::Malformed("dropped"));
                    }
                    self.text("dropped", &word);
                }
            }
            Body::Prompt { text, by } => {
                self.text("text", text);
                self.sender("by", by);
            }
            Body::Answer {
                ask,
                option,
                text,
                by,
                human_seq,
            } => {
                self.count("ask", *ask);
                match (option, text) {
                    (None, None) => self.fail(LineError::Missing("option")),
                    (Some(_), Some(_)) => self.fail(LineError::Malformed("text")),
                    _ => {}
                }
                self.option_or_text(option.as_ref(), text.as_ref());
                self.sender("by", by);
                self.number("human_seq", *human_seq);
            }
            Body::Touch { ask } => self.count("ask", *ask),
            Body::Hand {
                human_seq,
                composer,
                hold,
            } => {
                self.number("human_seq", *human_seq);
                self.text("composer", composer.word());
                self.number("hold", u64::from(*hold));
            }
            Body::Set {
                model,
                effort,
                mode,
            } => {
                self.opt_checked("model", model.as_ref(), model_word);
                self.opt_word("effort", effort.as_ref());
                if let Some(mode) = mode {
                    self.text("mode", mode.word());
                }
            }
            Body::Restart {
                cause,
                account,
                model,
            } => {
                self.checked("cause", cause, restart_cause);
                if let Some(Account { label, home }) = account {
                    if !account_cause(cause) {
                        self.fail(LineError::Malformed("to"));
                    }
                    self.checked("to", label, account_word);
                    match home {
                        None => self.text("home", "-"),
                        Some(path) => match path.to_str() {
                            Some(home) if path.is_absolute() => self.text("home", home),
                            _ => self.fail(LineError::Malformed("home")),
                        },
                    }
                }
                self.opt_checked("model", model.as_ref(), model_word);
            }
            Body::Unknown { k } => {
                if !is_word(k) || Kind::from_word(k).is_some() {
                    self.fail(LineError::Malformed("k"));
                }
            }
        }
    }

    fn did(&mut self, did: &Did) {
        self.word("act", did.act());
        match did {
            Did::Rule { act, rule } => {
                if matches!(act.as_str(), Did::WAIT | Did::HELD) {
                    self.fail(LineError::Malformed("act"));
                }
                self.word("rule", rule);
            }
            Did::Wait { reason, wait_ms } => {
                self.text("reason", reason.word());
                self.opt_number("wait_ms", *wait_ms);
            }
            Did::Held { held } => {
                if let Some(held) = held {
                    self.text("held", held.word());
                }
            }
        }
    }

    fn landing(&mut self, landing: &Landing) {
        self.text("state", landing.state().word());
        match landing {
            Landing::Held(LandHold {
                since_ms,
                nudges,
                read,
            }) => {
                self.number("since_ms", *since_ms);
                self.number("nudges", *nudges);
                if let Some(read) = read {
                    if read.legs.is_empty() {
                        self.fail(LineError::Missing("legs"));
                    }
                    self.words("legs", &read.legs);
                    self.words("open", &read.open);
                    for (key, number) in land_counts(read) {
                        self.opt_number(key, number);
                    }
                }
            }
            Landing::Landed { commit } => self.word("commit", commit),
            Landing::Open => {}
        }
    }
}

/// A read's counts, in the order they are written.
fn land_counts(read: &LandRead) -> [(&'static str, Option<u64>); 6] {
    [
        ("changed", read.changed),
        ("untracked", read.untracked),
        ("stashes", read.stashes),
        ("unpushed", read.unpushed),
        ("unlanded", read.unlanded),
        ("left", read.left),
    ]
}

/// Whether each of `items` is named once.
fn once<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(at, item)| !items[..at].contains(item))
}

/// A JSON value as read, before it is typed. A number keeps its text.
enum Value<'a> {
    Null,
    Bool(bool),
    Number(&'a str),
    Text(String),
    List(Vec<Value<'a>>),
    Object(Vec<(String, Value<'a>)>),
}

/// A reader over one line already known to be UTF-8 and within [`LINE_MAX`]. It steps whole
/// characters, so every slice it takes is on a character boundary.
struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn fault<T>(&self) -> Result<T, LineError> {
        Err(LineError::Syntax(self.at))
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    /// The value at the cursor, inside containers `depth` deep.
    fn value(&mut self, depth: usize) -> Result<Value<'a>, LineError> {
        self.space();
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.list(depth + 1),
            Some(b'"') => self.string().map(Value::Text),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => self.fault(),
        }
    }

    fn literal(&mut self, word: &str, value: Value<'a>) -> Result<Value<'a>, LineError> {
        if self.text.as_bytes()[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            self.fault()
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value<'a>, LineError> {
        if depth > DEPTH_MAX {
            return Err(LineError::Depth);
        }
        self.at += 1;
        let mut entries = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(entries));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return self.fault();
            }
            let key = self.string()?;
            self.space();
            if self.peek() != Some(b':') {
                return self.fault();
            }
            self.at += 1;
            let value = self.value(depth)?;
            entries.push((key, value));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(entries));
                }
                _ => return self.fault(),
            }
        }
    }

    fn list(&mut self, depth: usize) -> Result<Value<'a>, LineError> {
        if depth > DEPTH_MAX {
            return Err(LineError::Depth);
        }
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::List(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::List(items));
                }
                _ => return self.fault(),
            }
        }
    }

    /// The string whose opening quote is at the cursor. A character the hidden-character table
    /// names must be escaped.
    fn string(&mut self) -> Result<String, LineError> {
        self.at += 1;
        let mut out = String::new();
        let mut run = self.at;
        loop {
            let Some(c) = self
                .text
                .get(self.at..)
                .and_then(|rest| rest.chars().next())
            else {
                return self.fault();
            };
            match c {
                '"' => {
                    out.push_str(self.text.get(run..self.at).ok_or(LineError::Syntax(run))?);
                    self.at += 1;
                    return Ok(out);
                }
                '\\' => {
                    out.push_str(self.text.get(run..self.at).ok_or(LineError::Syntax(run))?);
                    self.at += 1;
                    out.push(self.escape()?);
                    run = self.at;
                }
                c if hidden(c) => return self.fault(),
                c => self.at += c.len_utf8(),
            }
        }
    }

    /// The character the escape after a backslash names.
    fn escape(&mut self) -> Result<char, LineError> {
        let byte = self.peek();
        let c = match byte {
            Some(b'"') => '"',
            Some(b'\\') => '\\',
            Some(b'/') => '/',
            Some(b'b') => '\u{8}',
            Some(b'f') => '\u{c}',
            Some(b'n') => '\n',
            Some(b'r') => '\r',
            Some(b't') => '\t',
            Some(b'u') => {
                self.at += 1;
                return self.unicode();
            }
            _ => return self.fault(),
        };
        self.at += 1;
        Ok(c)
    }

    /// The character a `\u` escape names, the cursor after its `u`: a surrogate only as a
    /// high-low pair.
    fn unicode(&mut self) -> Result<char, LineError> {
        let high = self.hex4()?;
        let code = match high {
            0xd800..=0xdbff => {
                if !self.text.as_bytes()[self.at..].starts_with(b"\\u") {
                    return self.fault();
                }
                self.at += 2;
                let low = self.hex4()?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return self.fault();
                }
                0x1_0000 + ((high - 0xd800) << 10) + (low - 0xdc00)
            }
            0xdc00..=0xdfff => return self.fault(),
            code => code,
        };
        char::from_u32(code).map_or_else(|| self.fault(), Ok)
    }

    fn hex4(&mut self) -> Result<u32, LineError> {
        let Some(digits) = self.text.as_bytes().get(self.at..self.at + 4) else {
            return self.fault();
        };
        let mut value = 0;
        for digit in digits {
            let Some(digit) = char::from(*digit).to_digit(16) else {
                return self.fault();
            };
            value = value * 16 + digit;
        }
        self.at += 4;
        Ok(value)
    }

    fn digits(&mut self) -> Result<(), LineError> {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return self.fault();
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        Ok(())
    }

    /// A number in JSON's grammar, kept as its text.
    fn number(&mut self) -> Result<Value<'a>, LineError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
        } else {
            self.digits()?;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
        }
        match self.text.get(start..self.at) {
            Some(number) => Ok(Value::Number(number)),
            None => Err(LineError::Syntax(start)),
        }
    }
}

/// An object's fields, each taken once by name; what is left is unknown.
struct Object<'a> {
    entries: Vec<(String, Option<Value<'a>>)>,
}

impl<'a> Object<'a> {
    fn new(entries: Vec<(String, Value<'a>)>) -> Self {
        let entries = entries
            .into_iter()
            .map(|(key, value)| (key, Some(value)))
            .collect();
        Self { entries }
    }

    /// The value of `key`, if present; `null` reads as absent.
    fn take(&mut self, key: &'static str) -> Result<Option<Value<'a>>, LineError> {
        let mut found = None;
        for (at, (name, _)) in self.entries.iter().enumerate() {
            if name == key {
                if found.is_some() {
                    return Err(LineError::Repeated(key));
                }
                found = Some(at);
            }
        }
        let Some(at) = found else {
            return Ok(None);
        };
        Ok(match self.entries[at].1.take() {
            Some(Value::Null) | None => None,
            value => value,
        })
    }

    /// The fields no `take` asked for.
    fn unknown(&self) -> u32 {
        let left = self.entries.iter().filter(|(_, value)| value.is_some());
        u32::try_from(left.count()).unwrap_or(u32::MAX)
    }
}

fn required<'a, T>(
    key: &'static str,
    value: Option<Value<'a>>,
    read: impl FnOnce(Value<'a>) -> Option<T>,
) -> Result<T, LineError> {
    read(value.ok_or(LineError::Missing(key))?).ok_or(LineError::Malformed(key))
}

fn optional<'a, T>(
    key: &'static str,
    value: Option<Value<'a>>,
    read: impl FnOnce(Value<'a>) -> Option<T>,
) -> Result<Option<T>, LineError> {
    value
        .map(|value| read(value).ok_or(LineError::Malformed(key)))
        .transpose()
}

/// `Ok` when a field its record leaves no place for is absent.
fn absent<T>(key: &'static str, value: Option<&T>) -> Result<(), LineError> {
    match value {
        Some(_) => Err(LineError::Malformed(key)),
        None => Ok(()),
    }
}

fn text(value: Value<'_>) -> Option<String> {
    match value {
        Value::Text(text) => Some(text),
        _ => None,
    }
}

/// A string in the grammar `ok` is.
fn checked<'a>(ok: fn(&str) -> bool) -> impl FnOnce(Value<'a>) -> Option<String> {
    move |value| text(value).filter(|text| ok(text))
}

fn word(value: Value<'_>) -> Option<String> {
    checked(is_word)(value)
}

/// A non-negative integer in `u64`: digits only, no sign, fraction or exponent.
fn count(value: Value<'_>) -> Option<u64> {
    match value {
        Value::Number(number) => canonical_u64(number),
        _ => None,
    }
}

/// A count from 1.
fn ordinal(value: Value<'_>) -> Option<u64> {
    count(value).filter(|number| *number > 0)
}

fn percent(value: Value<'_>) -> Option<u8> {
    count(value)
        .filter(|number| *number <= 100)
        .and_then(|number| u8::try_from(number).ok())
}

fn by(value: Value<'_>) -> Option<By> {
    text(value).and_then(|word| By::from_word(&word))
}

/// The `by` of a record aterm sends: someone outside the harness ([`By::outside`]).
fn sender(value: Value<'_>) -> Option<By> {
    by(value).filter(By::outside)
}

fn list(value: Value<'_>) -> Option<Vec<Value<'_>>> {
    match value {
        Value::List(items) => Some(items),
        _ => None,
    }
}

fn object(value: Value<'_>) -> Option<Object<'_>> {
    match value {
        Value::Object(entries) => Some(Object::new(entries)),
        _ => None,
    }
}

/// A closed word list's reader: the word's value, if the word is one.
fn of<'a, T>(from_word: fn(&str) -> Option<T>) -> impl FnOnce(Value<'a>) -> Option<T> {
    move |value| text(value).and_then(|word| from_word(&word))
}

/// A list whose every item `read` reads; absent reads as empty.
fn items<'a, T>(
    key: &'static str,
    value: Option<Value<'a>>,
    mut read: impl FnMut(Value<'a>) -> Result<T, LineError>,
) -> Result<Vec<T>, LineError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    list(value)
        .ok_or(LineError::Malformed(key))?
        .into_iter()
        .map(&mut read)
        .collect()
}

fn texts(key: &'static str, value: Option<Value<'_>>) -> Result<Vec<String>, LineError> {
    items(key, value, |item| {
        text(item).ok_or(LineError::Malformed(key))
    })
}

fn words(key: &'static str, value: Option<Value<'_>>) -> Result<Vec<String>, LineError> {
    items(key, value, |item| {
        word(item).ok_or(LineError::Malformed(key))
    })
}

/// `Ok` when no word of `words` is named twice, known to this library or not. Sorted, so a list
/// as long as the line allows is checked in `n log n`.
fn named_once(key: &'static str, words: &[String]) -> Result<(), LineError> {
    let mut sorted: Vec<&String> = words.iter().collect();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != words.len() {
        return Err(LineError::Repeated(key));
    }
    Ok(())
}

/// The items of an object list, each read by `read`; the unknown fields of each are counted.
fn objects<'a, T>(
    key: &'static str,
    value: Option<Value<'a>>,
    unknown: &mut u32,
    mut read: impl FnMut(&mut Object<'a>) -> Result<T, LineError>,
) -> Result<Vec<T>, LineError> {
    items(key, value, |item| {
        let mut fields = object(item).ok_or(LineError::Malformed(key))?;
        let read = read(&mut fields)?;
        *unknown = unknown.saturating_add(fields.unknown());
        Ok(read)
    })
}

/// A scope word aterm's claim grammar would drop rather than refuse: `[a-z][a-z-]*`.
fn well_formed_scope(word: &str) -> bool {
    word.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && word
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
}

/// A narrowed scope: each word once, known or not, in the order sent; a well-formed word this
/// library does not know is dropped and counted, as aterm drops it.
fn scope(value: Value<'_>, unknown: &mut u32) -> Result<Vec<Scope>, LineError> {
    let raw = texts("scope", Some(value))?;
    if raw.iter().any(|word| !well_formed_scope(word)) {
        return Err(LineError::Malformed("scope"));
    }
    named_once("scope", &raw)?;
    let mut scope = Vec::new();
    for word in &raw {
        match Scope::from_word(word) {
            Some(word) => scope.push(word),
            None => *unknown = unknown.saturating_add(1),
        }
    }
    Ok(scope)
}

/// The kinds of aterm's records a `session` takes: each word once, known or not; a word that
/// names no kind aterm sends is dropped and counted.
fn takes(value: Option<Value<'_>>, unknown: &mut u32) -> Result<Vec<Kind>, LineError> {
    let raw = words("takes", value)?;
    named_once("takes", &raw)?;
    let mut takes = Vec::new();
    for word in &raw {
        match Kind::from_word(word).filter(|kind| kind.sent_by() == Side::Aterm) {
            Some(kind) => takes.push(kind),
            None => *unknown = unknown.saturating_add(1),
        }
    }
    Ok(takes)
}

/// A `claim`'s `child`: `pid` and `start`, each once.
fn child(value: Value<'_>, unknown: &mut u32) -> Result<ChildProcess, LineError> {
    let mut fields = object(value).ok_or(LineError::Malformed("child"))?;
    let pid = fields
        .take("pid")?
        .and_then(count)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| (2..=i32::MAX.unsigned_abs()).contains(pid))
        .ok_or(LineError::Malformed("child"))?;
    let start = fields
        .take("start")?
        .and_then(ordinal)
        .ok_or(LineError::Malformed("child"))?;
    *unknown = unknown.saturating_add(fields.unknown());
    Ok(ChildProcess { pid, start })
}

/// The `0` or `1` of `hand`'s `hold`.
fn hold(value: Value<'_>) -> Option<bool> {
    match value {
        Value::Number("0") => Some(false),
        Value::Number("1") => Some(true),
        _ => None,
    }
}

fn flag(value: Value<'_>) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(flag),
        _ => None,
    }
}

/// An `act`'s act and the fields of that act.
fn did(f: &mut Object<'_>) -> Result<Did, LineError> {
    let act = required("act", f.take("act")?, word)?;
    let rule = optional("rule", f.take("rule")?, word)?;
    let reason = optional("reason", f.take("reason")?, of(WaitReason::from_word))?;
    let wait_ms = optional("wait_ms", f.take("wait_ms")?, count)?;
    let held = optional("held", f.take("held")?, of(HeldBy::from_word))?;
    Ok(match act.as_str() {
        Did::WAIT => {
            absent("rule", rule.as_ref())?;
            absent("held", held.as_ref())?;
            Did::Wait {
                reason: reason.ok_or(LineError::Missing("reason"))?,
                wait_ms,
            }
        }
        Did::HELD => {
            absent("rule", rule.as_ref())?;
            absent("reason", reason.as_ref())?;
            absent("wait_ms", wait_ms.as_ref())?;
            Did::Held { held }
        }
        _ => {
            absent("reason", reason.as_ref())?;
            absent("wait_ms", wait_ms.as_ref())?;
            absent("held", held.as_ref())?;
            Did::Rule {
                act,
                rule: rule.ok_or(LineError::Missing("rule"))?,
            }
        }
    })
}

/// A `land`'s state and the fields of that state.
fn landing(f: &mut Object<'_>) -> Result<Landing, LineError> {
    let state = required("state", f.take("state")?, of(LandState::from_word))?;
    let since_ms = optional("since_ms", f.take("since_ms")?, count)?;
    let nudges = optional("nudges", f.take("nudges")?, count)?;
    let legs = words("legs", f.take("legs")?)?;
    let open = words("open", f.take("open")?)?;
    let mut counts = [None; 6];
    for (slot, key) in counts.iter_mut().zip([
        "changed",
        "untracked",
        "stashes",
        "unpushed",
        "unlanded",
        "left",
    ]) {
        *slot = optional(key, f.take(key)?, count)?;
    }
    let commit = optional("commit", f.take("commit")?, word)?;
    let [changed, untracked, stashes, unpushed, unlanded, left] = counts;
    let read = if legs.is_empty() {
        if !open.is_empty() || counts.iter().any(Option::is_some) {
            return Err(LineError::Missing("legs"));
        }
        None
    } else {
        Some(LandRead {
            legs,
            open,
            changed,
            untracked,
            stashes,
            unpushed,
            unlanded,
            left,
        })
    };
    if state != LandState::Held {
        absent("since_ms", since_ms.as_ref())?;
        absent("nudges", nudges.as_ref())?;
        absent("legs", read.as_ref())?;
    }
    if state != LandState::Landed {
        absent("commit", commit.as_ref())?;
    }
    Ok(match state {
        LandState::Held => Landing::Held(LandHold {
            since_ms: since_ms.ok_or(LineError::Missing("since_ms"))?,
            nudges: nudges.ok_or(LineError::Missing("nudges"))?,
            read,
        }),
        LandState::Landed => Landing::Landed {
            commit: commit.ok_or(LineError::Missing("commit"))?,
        },
        LandState::Open => Landing::Open,
    })
}

/// A `restart`'s account move: `to` and `home` together, for an account cause only.
fn account_move(
    cause: &str,
    to: Option<String>,
    home: Option<String>,
) -> Result<Option<Account>, LineError> {
    match (to, home) {
        (None, None) => Ok(None),
        (Some(_), _) if !account_cause(cause) => Err(LineError::Malformed("to")),
        (None, Some(_)) => Err(LineError::Missing("to")),
        (Some(_), None) => Err(LineError::Missing("home")),
        (Some(label), Some(home)) => Ok(Some(Account {
            label,
            home: account_home(&home).ok_or(LineError::Malformed("home"))?,
        })),
    }
}

/// The fields of a known kind.
fn body(kind: Kind, f: &mut Object<'_>, unknown: &mut u32) -> Result<Body, LineError> {
    Ok(match kind {
        Kind::Session => Body::Session {
            agent: required("agent", f.take("agent")?, word)?,
            version: required("version", f.take("version")?, checked(claim_version))?,
            vendor: optional("vendor", f.take("vendor")?, checked(vendor))?,
            account: optional("account", f.take("account")?, checked(account_word))?,
            conversation: optional("conversation", f.take("conversation")?, word)?,
            cwd: optional("cwd", f.take("cwd")?, text)?,
            resume: texts("resume", f.take("resume")?)?,
            mode: required("mode", f.take("mode")?, of(Mode::from_word))?,
            takes: takes(f.take("takes")?, unknown)?,
        },
        Kind::Claim => Body::Claim {
            child: f
                .take("child")?
                .map(|value| child(value, unknown))
                .transpose()?,
            scope: f
                .take("scope")?
                .map(|value| scope(value, unknown))
                .transpose()?,
            version: optional("version", f.take("version")?, checked(claim_version))?,
            vendor: optional("vendor", f.take("vendor")?, checked(vendor))?,
            login: optional("login", f.take("login")?, checked(account_word))?,
        },
        Kind::State => Body::State {
            state: required("state", f.take("state")?, of(State::from_word))?,
            composer: optional("composer", f.take("composer")?, of(Composer::from_word))?,
        },
        Kind::Beat => Body::Beat,
        Kind::Turn => {
            let turn = required("turn", f.take("turn")?, ordinal)?;
            let state = required("state", f.take("state")?, of(TurnState::from_word))?;
            let by = optional("by", f.take("by")?, by)?;
            if state == TurnState::Started && by.is_none() {
                return Err(LineError::Missing("by"));
            }
            Body::Turn { turn, state, by }
        }
        Kind::Msg => Body::Msg {
            turn: optional("turn", f.take("turn")?, ordinal)?,
            by: required("by", f.take("by")?, by)?,
            text: required("text", f.take("text")?, text)?,
        },
        Kind::Tool => Body::Tool {
            turn: optional("turn", f.take("turn")?, ordinal)?,
            id: required("id", f.take("id")?, word)?,
            name: required("name", f.take("name")?, word)?,
            state: required("state", f.take("state")?, of(Step::from_word))?,
            text: optional("text", f.take("text")?, text)?,
        },
        Kind::Diff => Body::Diff {
            turn: optional("turn", f.take("turn")?, ordinal)?,
            path: optional("path", f.take("path")?, text)?,
            patch: required("patch", f.take("patch")?, text)?,
        },
        Kind::Subagent => Body::Subagent {
            id: required("id", f.take("id")?, word)?,
            title: optional("title", f.take("title")?, text)?,
            state: required("state", f.take("state")?, of(Step::from_word))?,
        },
        Kind::Title => Body::Title {
            title: optional("title", f.take("title")?, text)?,
            summary: optional("summary", f.take("summary")?, text)?,
        },
        Kind::Progress => Body::Progress {
            items: objects("items", f.take("items")?, unknown, |item| {
                Ok(Item {
                    text: required("text", item.take("text")?, text)?,
                    state: required("state", item.take("state")?, of(ItemState::from_word))?,
                })
            })?,
        },
        Kind::Goal => Body::Goal {
            text: optional("text", f.take("text")?, text)?,
        },
        Kind::Compact => Body::Compact {
            state: required("state", f.take("state")?, of(CompactState::from_word))?,
            context_left: optional("context_left", f.take("context_left")?, percent)?,
        },
        Kind::Ask => Body::Ask {
            kind: required("kind", f.take("kind")?, of(AskKind::from_word))?,
            title: optional("title", f.take("title")?, text)?,
            options: texts("options", f.take("options")?)?,
            will: optional("will", f.take("will")?, text)?,
            will_in_ms: optional("will_in_ms", f.take("will_in_ms")?, count)?,
        },
        Kind::Asked => Body::Asked {
            ask: required("ask", f.take("ask")?, ordinal)?,
            kind: required("kind", f.take("kind")?, of(AskKind::from_word))?,
            by: required("by", f.take("by")?, by)?,
            reply: required("reply", f.take("reply")?, of(Reply::from_word))?,
            option: optional("option", f.take("option")?, text)?,
            text: optional("text", f.take("text")?, text)?,
        },
        Kind::Usage => Body::Usage {
            account: required("account", f.take("account")?, checked(account_word))?,
            limits: objects("limits", f.take("limits")?, unknown, |limit| {
                Ok(Limit {
                    name: required("name", limit.take("name")?, word)?,
                    used: optional("used", limit.take("used")?, percent)?,
                    resets_ms: optional("resets_ms", limit.take("resets_ms")?, count)?,
                })
            })?,
            context_left: optional("context_left", f.take("context_left")?, percent)?,
        },
        Kind::Account => Body::Account {
            account: required("account", f.take("account")?, checked(account_word))?,
            state: required("state", f.take("state")?, word)?,
        },
        Kind::Wall => Body::Wall {
            kind: required("kind", f.take("kind")?, word)?,
            resets_ms: optional("resets_ms", f.take("resets_ms")?, count)?,
        },
        Kind::Model => Body::Model {
            model: optional("model", f.take("model")?, text)?,
            effort: optional("effort", f.take("effort")?, word)?,
            key: optional("key", f.take("key")?, flag)?,
            offers: objects("offers", f.take("offers")?, unknown, |offer| {
                Ok(Offer {
                    model: required("model", offer.take("model")?, checked(model_word))?,
                    levels: words("levels", offer.take("levels")?)?,
                })
            })?,
            permission: optional("permission", f.take("permission")?, word)?,
        },
        Kind::Act => Body::Act { did: did(f)? },
        Kind::Need => {
            let kind = required("kind", f.take("kind")?, of(NeedKind::from_word))?;
            let entry = required("entry", f.take("entry")?, text)?;
            let needs = texts("needs", f.take("needs")?)?;
            if needs.is_empty() {
                return Err(LineError::Missing("needs"));
            }
            if needs.len() > NEEDS_MAX {
                return Err(LineError::Malformed("needs"));
            }
            Body::Need { kind, entry, needs }
        }
        Kind::Clear => Body::Clear {
            need: required("need", f.take("need")?, ordinal)?,
        },
        Kind::Land => Body::Land { state: landing(f)? },
        Kind::Upgrade => Body::Upgrade {
            phase: required("phase", f.take("phase")?, of(UpgradePhase::from_word))?,
            to: required("to", f.take("to")?, word)?,
        },
        Kind::Took => Body::Took {
            of: required("in", f.take("in")?, ordinal)?,
        },
        Kind::Refused => Body::Refused {
            of: required("in", f.take("in")?, ordinal)?,
            why: required("why", f.take("why")?, text)?,
        },
        Kind::Ack => Body::Ack {
            dropped: optional("dropped", f.take("dropped")?, of(Dropped::from_word))?,
        },
        Kind::Prompt => Body::Prompt {
            text: required("text", f.take("text")?, text)?,
            by: required("by", f.take("by")?, sender)?,
        },
        Kind::Answer => {
            let ask = required("ask", f.take("ask")?, ordinal)?;
            let option = optional("option", f.take("option")?, text)?;
            let text = optional("text", f.take("text")?, text)?;
            match (&option, &text) {
                (None, None) => return Err(LineError::Missing("option")),
                (Some(_), Some(_)) => return Err(LineError::Malformed("text")),
                _ => {}
            }
            Body::Answer {
                ask,
                option,
                text,
                by: required("by", f.take("by")?, sender)?,
                human_seq: required("human_seq", f.take("human_seq")?, count)?,
            }
        }
        Kind::Touch => Body::Touch {
            ask: required("ask", f.take("ask")?, ordinal)?,
        },
        Kind::Hand => Body::Hand {
            human_seq: required("human_seq", f.take("human_seq")?, count)?,
            composer: required("composer", f.take("composer")?, of(Composer::from_word))?,
            hold: required("hold", f.take("hold")?, hold)?,
        },
        Kind::Interrupt => Body::Interrupt,
        Kind::Set => Body::Set {
            model: optional("model", f.take("model")?, checked(model_word))?,
            effort: optional("effort", f.take("effort")?, word)?,
            mode: optional("mode", f.take("mode")?, of(Mode::from_word))?,
        },
        Kind::Restart => {
            let cause = required("cause", f.take("cause")?, checked(restart_cause))?;
            let to = optional("to", f.take("to")?, checked(account_word))?;
            let home = optional("home", f.take("home")?, text)?;
            Body::Restart {
                account: account_move(&cause, to, home)?,
                cause,
                model: optional("model", f.take("model")?, checked(model_word))?,
            }
        }
        Kind::Policy => Body::Policy,
        Kind::Want => Body::Want,
    })
}
