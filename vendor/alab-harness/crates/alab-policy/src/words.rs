// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Words several deciders share: how many bytes a quoted sentence keeps and how it is cut
//! around its match, how many UTF-16 code units a typed text may hold and how a text is cut to
//! fit them, the standing rules a typed text carries, Land's sentence for files no repository
//! holds, and the words that make an ask one a person must answer.
//!
//! Cut out of aterm's turn-end policy (`supervise/policy/turn_end.rs`), which keeps the rest of
//! that file and imports these.

use std::borrow::Cow;
use std::ops::Range;

/// The most bytes a `Found::said` (aterm's turn-end reader) holds.
pub const SAID_CAP: usize = 160;

/// `said` cut to at most `cap` bytes around `matched`, which it keeps whole, at a word's edge
/// where one stands outside the match, with `…` at each cut end. A match longer than the room
/// keeps its start and as many of its whole words as fit.
pub fn cut_around(said: &str, matched: Range<usize>, cap: usize) -> (String, Range<usize>) {
    const CUT: &str = "…";
    let matched = matched.start.min(said.len())..matched.end.min(said.len());
    if said.len() <= cap {
        return (said.to_string(), matched);
    }
    let room = cap.saturating_sub(CUT.len());
    let (from, to) = if matched.end <= room {
        (0, room)
    } else if said.len() - matched.start <= room {
        (said.len() - room, said.len())
    } else {
        let room = cap.saturating_sub(2 * CUT.len());
        let spare = room.saturating_sub(matched.len());
        let from = matched.start - (spare / 2).min(matched.start);
        (from, (from + room).min(said.len()))
    };
    let mut from = from;
    while !said.is_char_boundary(from) {
        from += 1;
    }
    let mut to = to;
    while !said.is_char_boundary(to) {
        to -= 1;
    }
    if from > 0
        && let Some(space) = said[from..matched.start.max(from)].find(' ')
    {
        from += space + 1;
    }
    // The cut's end goes back to a word's edge: past a match that fits, and inside one that
    // does not, which then keeps its start and whole words of it.
    let (floor, inside) = if matched.end <= to {
        (matched.end, false)
    } else {
        (matched.start.max(from), true)
    };
    if to < said.len()
        && let Some(space) = said
            .get(floor..to)
            .and_then(|s| s.rfind(' '))
            .filter(|&s| !inside || s > 0)
    {
        to = floor + space;
    }
    let mut out = String::with_capacity(cap);
    if from > 0 {
        out.push_str(CUT);
    }
    let shift = out.len();
    out.push_str(&said[from..to]);
    if to < said.len() {
        out.push_str(CUT);
    }
    let start = (matched.start.max(from) - from + shift).min(out.len());
    let end = (matched.end.min(to).max(from) - from + shift).clamp(start, out.len());
    (out, start..end)
}

/// What Land's instruction and nudge say of files no git repository holds ([`NO_REPOSITORY`]).
#[macro_export]
macro_rules! no_repository {
    () => {
        "Files no git repository holds land where they are once verified."
    };
}

/// FILES NO REPOSITORY HOLDS LAND WHERE THEY ARE: `LAND`'s and `LAND_NUDGE`'s sentence for them. It names the files, not the folder, so a worker whose tab is no repository still pushes the work it did in one; the probe still reads the session's repository at a defense (`repo_work_suffix`). It is short so the standing rules keep room on Land's first act with both its clauses ([`VERBATIM_UNITS`]).
pub const NO_REPOSITORY: &str = no_repository!();

/// Words that make an offer, stop, choice or question one a person must answer.
pub const DESTRUCTIVE_WORDS: &[&str] = &[
    "force",
    "force-push",
    "--force",
    "push -f",
    "force-with-lease",
    "delete",
    "deleting",
    "deletion",
    "drop",
    "dropping",
    "remove",
    "removing",
    "removal",
    "rm",
    "deploy",
    "deploying",
    "migrate",
    "migrating",
    "publish",
    "publishing",
    "release",
    "releasing",
    "wipe",
    "purge",
    "reset",
    "overwrite",
    "truncate",
    "destroy",
    "uninstall",
    "revert",
    "rewrite history",
];

/// Whether `text` names a force-push git's short forms spell: a push whose clause has a short
/// flag cluster with an `f` (`push -f`, `push -uf`, `push origin main -f`), or a refspec forced
/// with `+` (`push origin +main`).
pub fn force_push(text: &str) -> bool {
    text.split([';', '|', '&', '\n', '?', '!']).any(|clause| {
        let mut push = false;
        let flagged = clause
            .split(|c: char| !(c.is_alphanumeric() || c == '-'))
            .filter(|word| !word.is_empty())
            .any(|word| match word {
                word if inflects(word, "push") => {
                    push = true;
                    false
                }
                _ => {
                    push && word.strip_prefix('-').is_some_and(|flags| {
                        !flags.starts_with('-')
                            && flags.bytes().take_while(|&b| b != b'o').any(|b| b == b'f')
                    })
                }
            });
        let mut push = false;
        let forced = clause.split_whitespace().any(|token| {
            let token = token.trim_matches(['`', '"', '\'', '(', ')', ',', '.', ':']);
            if token
                .split(|c: char| !(c.is_alphanumeric() || c == '-'))
                .any(|w| inflects(w, "push"))
            {
                push = true;
                return false;
            }
            push && token
                .strip_prefix('+')
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_alphabetic()))
        });
        flagged || forced
    })
}

/// Whether `word` is `base` or an inflection of it: `base` and `s`, `es`, `d`, `ed`, `ing` or
/// `ten`; `base` with its last letter doubled and `ed`, `ing` or `en`; `base` without its closing
/// `e` and `ing`, `ed` or `ten`; or, for a `write`, its `wrote`.
pub fn inflects(word: &str, base: &str) -> bool {
    if word == base {
        return true;
    }
    let Some(rest) = word.strip_prefix(base) else {
        let stem = base.strip_suffix('e').unwrap_or(base);
        return (stem.len() < base.len()
            && word
                .strip_prefix(stem)
                .is_some_and(|rest| matches!(rest, "ing" | "ed" | "ten")))
            || (base.ends_with("write") && word == format!("{}ote", &base[..base.len() - 3]));
    };
    if matches!(rest, "s" | "es" | "d" | "ed" | "ing" | "ten") {
        return true;
    }
    let last = base.chars().last().unwrap_or(' ');
    rest.strip_prefix(last)
        .is_some_and(|rest| matches!(rest, "ed" | "ing" | "en"))
}

/// The length of `s` in UTF-16 code units, the unit [`VERBATIM_UNITS`] counts.
pub fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The most UTF-16 code units a text this policy types may hold and still be drawn as typed.
pub const VERBATIM_UNITS: usize = 800;

/// `text` with the standing `rules` after it, opened by [`RULES_OPEN`] and closed by `)`, the
/// rules cut ([`fit_units`]) so the whole holds at most [`VERBATIM_UNITS`]; `text` alone where
/// `rules` is none or blank or no room is left for them.
pub fn with_rules(text: &str, rules: Option<&str>) -> String {
    let Some(r) = rules.map(str::trim).filter(|r| !r.is_empty()) else {
        return text.to_string();
    };
    let frame = text.encode_utf16().count() + RULES_OPEN.len() + ")".len();
    let r = fit_units(r, VERBATIM_UNITS.saturating_sub(frame));
    if r.is_empty() {
        return text.to_string();
    }
    format!("{text}{RULES_OPEN}{r})")
}

/// `s` in at most `room` UTF-16 code units: `s` itself where it fits, else cut back to a word's
/// edge with room for a closing `…`, or empty where not one word fits.
pub fn fit_units(s: &str, room: usize) -> Cow<'_, str> {
    if s.encode_utf16().count() <= room {
        return Cow::Borrowed(s);
    }
    let budget = room.saturating_sub('…'.len_utf16());
    let mut units = 0;
    let mut end = 0;
    for (at, c) in s.char_indices() {
        units += c.len_utf16();
        if units > budget {
            break;
        }
        end = at + c.len_utf8();
    }
    let cut = if s[end..].starts_with(char::is_whitespace) {
        end
    } else {
        s[..end].rfind(char::is_whitespace).unwrap_or(0)
    };
    match s[..cut].trim_end() {
        "" => Cow::Borrowed(""),
        kept => Cow::Owned(format!("{kept}…")),
    }
}

/// What opens the standing rules [`with_rules`] puts after a text.
pub const RULES_OPEN: &str = " (standing rules: ";
