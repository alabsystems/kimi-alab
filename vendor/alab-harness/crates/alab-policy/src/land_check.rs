// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! THE LAND CHECK (`land-check@v1`, `docs/DESIGN-harness-autopilot-2026-10-15.md` §14.17 item 5):
//! the one prompt that decides a landing. At a Land point whose reply defends the work with
//! `Nothing left:` over a clean repository read, Land does not land on the defense's words: it
//! types this check once, and the agent's literal answer decides. `LANDED` alone lands the task;
//! a `NEEDS YOU:` line is a need, shown quietly; anything else is the agent's own list of what is
//! left, which Land's next prompt quotes ([`listed_suffix`]).
//!
//! The words are frozen as `land-check@v1` (their sha256 is in KI-1020-2 and the slice's As built
//! note); any change to them is a `land-check@v2`, measured afresh. The word reader is only a
//! hint here (aterm's `hint_clause`): it picks the one clause of the report the check quotes
//! first.

use super::needs;
use super::words::{RULES_OPEN, VERBATIM_UNITS, with_rules};

/// The land check's rule (the plain check: [`crate::land_check_head!`] then
/// [`LAND_CHECK_BODY`]): a fourth Land act beside `land@v1`, `land-nudge@v1` and
/// `land-answer@v1`. The landing it decides is still noted under `land@v1`.
pub const RULE_LAND_CHECK: &str = "land-check@v1";

/// THE LAND CHECK'S FIXED HEAD, after the harness's mark: a literal, or a macro call that expands
/// to one, as `concat!` takes it (aterm passes its `harness_mark!()` for its `LAND_CHECK_HEAD`).
/// Every reader of a typed land check keys on it (the policy's `turn_end::is_land_words`, the
/// ledger's, the transcript's and the stats').
#[macro_export]
macro_rules! land_check_head {
    ($($mark:tt)+) => {
        concat!($($mark)+, " Land check:")
    };
}

/// [`LAND_CHECK_BODY`] as a literal: the frozen words after the head (and after the hint, where
/// one is quoted). Its no-repository sentence is [`crate::words::NO_REPOSITORY`]'s one source.
/// THE LAND CHECK, plain (`land-check@v1`), is [`crate::land_check_head!`] then this, as aterm's
/// `LAND_CHECK` builds it with `concat!`.
#[macro_export]
macro_rules! land_check_body {
    () => {
        concat!(
            " Reread your report, since git sees only part of the work. Not landed: a change not \
             on the remote main branch (with no remote, not committed); a check skipped, flaky, \
             failing or not run; work deferred, partial, or on another branch or repository; \
             something only a person can give, written as a NEEDS YOU: line. ",
            $crate::no_repository!(),
            " Land what you can, never by deleting work, then reply with only LANDED if none of \
             these applies, else with what is left."
        )
    };
}

/// The land check's words after its head ([`land_check_body!`]).
pub const LAND_CHECK_BODY: &str = land_check_body!();

/// What opens the hinted check's quote of the report, after the head.
pub const LAND_CHECK_HINT_OPEN: &str = " Your report says \u{201c}";

/// What closes it, before the body.
pub const LAND_CHECK_HINT_CLOSE: &str = "\u{201d}.";

/// What a rest on a need says, in the journal's `WAITING … rule=land-check@v1` line: Land types
/// nothing until someone else's turn (4b's glass words for the Needs leg alone).
pub const WAITS_ON_YOU: &str = "waits only on you";

/// The one answer that lands a task ([`landed_word`]).
pub const LANDED: &str = "LANDED";

/// The most UTF-16 units the hinted check's quoted clause holds (aterm's `hint_clause`).
pub const HINT_UNITS: usize = 120;

/// The most bytes the list Land's next prompt quotes holds ([`listed_suffix`]).
pub const LISTED_CAP: usize = 160;

/// The words that open a quote of the agent's list after its subject, one source for Land's
/// nudge ([`LISTED_OPEN`]) and its contradiction (`turn_end::CONTRADICTED_LISTED`).
#[macro_export]
macro_rules! said_left {
    () => {
        "said this is left: \u{201c}"
    };
}

/// What opens the quote of the agent's list in Land's next prompt ([`listed_suffix`]).
pub const LISTED_OPEN: &str = concat!("You ", said_left!());

/// What closes it.
pub const LISTED_CLOSE: &str = "\u{201d}.";

/// LAND KEEPS RESOLVING (§14.17 item 1, slice 4b): what follows the quote of the agent's list in
/// Land's next prompt, whose own words already ask for only what a person can give on a
/// `NEEDS YOU:` line (`turn_end::LAND_NUDGE`), so it is not said twice in one prompt. Land holds
/// only where the agent gives the same answer again, and the hold then quotes that answer
/// (`harness::hold_said`).
pub const LISTED_RESOLVE: &str = "Resolve each issue you can.";

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// THE CHECK AS TYPED: the plain form, or the hinted one quoting `hint` (a clause aterm's
/// `hint_clause` chose), then the needs ack where `ack`, then the standing `rules`, its head after
/// the harness's `mark` ([`crate::land_check_head!`]). The base check, the ack and the rules come
/// first: where the hint would cut the rules, the plain form is typed, since the rules are the
/// owner's own words and the hint is only a hint.
#[must_use]
pub fn land_check_text(mark: &str, hint: Option<&str>, ack: bool, rules: Option<&str>) -> String {
    let ack = if ack {
        format!(" {}", needs::NEEDS_ACK)
    } else {
        String::new()
    };
    let head = format!("{mark}{}", land_check_head!(""));
    let plain = format!("{head}{LAND_CHECK_BODY}{ack}");
    let Some(hint) = hint.filter(|h| !h.is_empty()) else {
        return with_rules(&plain, rules);
    };
    let hinted =
        format!("{head}{LAND_CHECK_HINT_OPEN}{hint}{LAND_CHECK_HINT_CLOSE}{LAND_CHECK_BODY}{ack}");
    let rules_units = rules
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map_or(0, |r| units(RULES_OPEN) + units(r) + 1);
    if units(&hinted) + rules_units <= VERBATIM_UNITS {
        with_rules(&hinted, rules)
    } else {
        with_rules(&plain, rules)
    }
}

/// THE AGENT'S ANSWER TO THE LAND CHECK ([`land_check_answer`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckAnswer {
    /// `LANDED`, alone ([`landed_word`]).
    Landed,
    /// A `NEEDS YOU:` line; `rest_landed` where nothing else stands beside it but `LANDED`.
    Needs {
        /// Nothing but `LANDED`, or nothing at all, stands beside the need, in a reply not cut.
        rest_landed: bool,
    },
    /// Anything else: the agent's own list of what is left (the reply without its needs lines;
    /// empty where it held nothing else, as `NEEDS YOU: none` alone).
    Left(String),
}

/// THE ANSWER `tail` (the check's reply) GIVES, read literally. `turn_said` is the agent's earlier
/// messages in the same turn, read for needs only, and only those no later word of the turn
/// retracted ([`needs::standing_before`]: an earlier message's own none for that message, the
/// reply's for all); `cut` is the reply having been taller than the rows read, so it is never
/// [`CheckAnswer::Landed`]: a reply of the word alone cannot be cut.
#[must_use]
pub fn land_check_answer(
    tail: &str,
    turn_said: Option<needs::Before<'_>>,
    cut: bool,
) -> CheckAnswer {
    let turn_said = needs::standing_before(turn_said, tail);
    let needs_said = [Some(tail), turn_said.as_deref()]
        .into_iter()
        .flatten()
        .any(|t| !needs::persons_needs(t).is_empty());
    let body = needs::without_needs(tail);
    let body = body.trim();
    let word = !cut && landed_word(body);
    if needs_said {
        CheckAnswer::Needs {
            rest_landed: !cut && (body.is_empty() || word),
        }
    } else if word {
        CheckAnswer::Landed
    } else {
        CheckAnswer::Left(body.to_string())
    }
}

/// WHETHER `tail` ANSWERS THE CHECK WITH `LANDED` ([`land_check_answer`]), its needs aside.
#[must_use]
pub fn landed_reply(tail: Option<&str>) -> bool {
    tail.is_some_and(|t| landed_word(needs::without_needs(t).trim()))
}

const WRAPPERS: [&str; 4] = ["**", "__", "*", "_"];

/// WHETHER `body` IS THE WORD `LANDED` and nothing else: Unicode whitespace trimmed at both ends,
/// at most one closing full stop removed (before or after the wrappers), markdown's matching
/// wrapper pairs (`**`, `__`, `*`, `_`, an equal run of backticks at both ends: a code span or a
/// fenced block holding the word alone) removed, and ASCII case ignored. Nothing else is
/// trimmed: a word after it, `!`, quotes, a list or heading mark, two full stops.
#[must_use]
pub fn landed_word(body: &str) -> bool {
    let mut s = body.trim();
    let mut stop = false;
    loop {
        if let Some(inner) = unwrapped(s) {
            s = inner.trim();
            continue;
        }
        if !stop && let Some(inner) = s.strip_suffix('.') {
            stop = true;
            s = inner.trim_end();
            continue;
        }
        break;
    }
    s.eq_ignore_ascii_case(LANDED)
}

fn unwrapped(s: &str) -> Option<&str> {
    let ticks = s.len() - s.trim_start_matches('`').len();
    if ticks > 0 {
        let close = s.len() - s.trim_end_matches('`').len();
        return (close == ticks && s.len() > 2 * ticks).then(|| &s[ticks..s.len() - ticks]);
    }
    WRAPPERS.iter().find_map(|w| {
        (s.len() > 2 * w.len() && s.starts_with(w) && s.ends_with(w))
            .then(|| &s[w.len()..s.len() - w.len()])
    })
}

/// THE LIST LAND'S NEXT PROMPT QUOTES (`You said this is left: “<list>”.`), then
/// [`LISTED_RESOLVE`]: the agent's answer to the check, its lines with markdown's marks dropped,
/// joined with `; `, curly double quotes made straight, and cut to [`LISTED_CAP`] bytes at a
/// word's edge with `…`. `None` for an empty list. Not under the freeze: 4b owns the final words
/// of Land's nudge, which types [`LISTED_RESOLVE`] as a part of its own, so a fit that leaves the
/// quote out for room never leaves the ask to resolve out with it.
#[must_use]
pub fn listed_suffix(list: &str) -> Option<String> {
    listed_in(list, LISTED_CAP).map(|quote| format!("{quote} {LISTED_RESOLVE}"))
}

/// The quote alone (`You said this is left: “<list>”.`), the list cut to `cap` bytes.
pub fn listed_in(list: &str, cap: usize) -> Option<String> {
    let said = listed_words(list);
    if said.is_empty() || cap == 0 {
        return None;
    }
    let said = cut_bytes(&said, cap)?;
    Some(format!("{LISTED_OPEN}{said}{LISTED_CLOSE}"))
}

/// The list's words as [`listed_suffix`] quotes them, uncut: its items ([`listed_items`]) joined
/// with `; `.
#[must_use]
pub fn listed_words(list: &str) -> String {
    listed_items(list).join("; ")
}

/// THE AGENT'S LIST, ITEM BY ITEM, as Land's next prompt and the hold quote it (one source): each
/// line with markdown's marks dropped and curly double quotes made straight, a blank line none. A
/// row the screen wrapped joins the item before it with a space: an unmarked line after an item
/// with no closing mark, where it starts in lower case, carries on a list-marked item, or follows
/// a row of at least [`WRAPPED_CHARS`] characters (a wrap may start with a capital, `GitHub`). A
/// heading above the items (a first line that ends in `:`, as `Not landed:`, or a markdown
/// heading) is not an item. A short unmarked line with no closing mark followed by one starting
/// with a capital stays apart: a list of plain lines is not run together.
#[must_use]
pub fn listed_items(list: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    let mut heading = false;
    let mut in_marked = false;
    let mut wide = false;
    for raw in list.lines() {
        let line = unmarked(raw).replace(['\u{201c}', '\u{201d}'], "\"");
        if line.is_empty() {
            continue;
        }
        let marked = list_marked(raw);
        let carries = !marked
            && !(heading && items.len() == 1)
            && items
                .last()
                .is_some_and(|item| !item.ends_with(['.', ';', ':', '!', '?']))
            && (in_marked || wide || raw.trim_start().starts_with(|c: char| c.is_lowercase()));
        wide = raw.trim().chars().count() >= WRAPPED_CHARS;
        match items.last_mut() {
            Some(item) if carries => {
                item.push(' ');
                item.push_str(&line);
            }
            _ => {
                if items.is_empty() {
                    heading = !marked && (line.ends_with(':') || raw.trim_start().starts_with('#'));
                }
                items.push(line);
                in_marked = marked;
            }
        }
    }
    if heading && items.len() > 1 {
        items.remove(0);
    }
    items
}

/// The fewest characters a row the screen wrapped holds: a row this wide with no closing mark
/// ran on to the next ([`listed_items`]), where a plain list's short lines do not.
pub const WRAPPED_CHARS: usize = 60;

/// Whether `raw` opens with a list item's mark (`- `, `* `, `+ `, `• `, a task box, `1. `, `1) `).
fn list_marked(raw: &str) -> bool {
    let t = raw.trim_start().trim_start_matches('>').trim_start();
    if ["- ", "* ", "+ ", "• "].iter().any(|m| t.starts_with(m)) {
        return true;
    }
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))
}

fn unmarked(line: &str) -> String {
    let mut t = line.trim();
    loop {
        let before = t;
        t = t.trim_start_matches(['#', '>', '|']).trim_start();
        for mark in ["- [ ] ", "- [x] ", "- ", "* ", "+ ", "• "] {
            if let Some(rest) = t.strip_prefix(mark) {
                t = rest.trim_start();
            }
        }
        let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0
            && let Some(rest) = t[digits..]
                .strip_prefix(". ")
                .or_else(|| t[digits..].strip_prefix(") "))
        {
            t = rest.trim_start();
        }
        if t == before {
            break;
        }
    }
    let t = t.trim_end_matches('|').trim();
    let kept: String = t.chars().filter(|&c| c != '`' && c != '*').collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `s` cut to at most `cap` bytes at a word's edge, with `…`; `None` where no word fits.
pub fn cut_bytes(s: &str, cap: usize) -> Option<String> {
    if s.len() <= cap {
        return Some(s.to_string());
    }
    let room = cap.saturating_sub('…'.len_utf8());
    let mut end = room.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let cut = if s[end..].starts_with(' ') {
        end
    } else {
        s[..end].rfind(' ').unwrap_or(0)
    };
    let kept = s[..cut].trim_end_matches([' ', ';', ',']);
    (!kept.is_empty()).then(|| format!("{kept}…"))
}

/// THE AUDITED DEFENSE KEPT FOR THE LANDING'S RECEIPT: at most this many bytes of it.
pub const AUDITED_CAP: usize = 240;

/// The defense the check audits, as the landing's receipt quotes it
/// (`harness::land_receipt::LandReceipt::said`): `tail`'s defense line and every non-blank line
/// after it ([`needs::defense_and_after`]; the whole tail where it holds none), one per line, cut
/// to [`AUDITED_CAP`] bytes at a character's edge.
#[must_use]
pub fn audited(tail: &str) -> String {
    let kept = needs::defense_and_after(tail).map_or_else(|| tail.to_string(), |l| l.join("\n"));
    let mut end = kept.len().min(AUDITED_CAP);
    while !kept.is_char_boundary(end) {
        end -= 1;
    }
    kept[..end].to_string()
}

#[cfg(test)]
#[path = "land_check_tests.rs"]
mod tests;
