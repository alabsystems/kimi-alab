// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Parser for `NEEDS YOU:` and `Nothing left:` report lines read by the turn-end policy.

use std::borrow::Cow;

use crate::token::ProcessToken;

/// The head of a line that names what only a person can give.
pub const NEEDS_YOU: &str = "NEEDS YOU:";

/// The head of the worker's defense that nothing is left to do.
pub const NOTHING_LEFT: &str = "Nothing left:";

/// The head of a managed worker's reply that says its work ends unfinished ([`could_not_finish`]).
pub const COULD_NOT_FINISH: &str = "Could not finish:";

/// The restart instruction shared by the continuations and needs acknowledgements.
#[macro_export]
macro_rules! restart_hint {
    () => {
        "Handle agent restarts yourself: save a handoff; `aterm ctl help restart`."
    };
}

/// [`NEEDS_HINT`] as a literal, for the texts `concat!` builds on it.
#[macro_export]
macro_rules! needs_hint {
    () => {
        concat!(
            "Put what only a person can give on a line starting NEEDS YOU: and carry on. ",
            $crate::restart_hint!(),
        )
    };
}

/// Appended after the answers aterm types to a question (`super::turn_end::prose_answer`, `super::turn_end::reversible_answer`), and to a restart's carry-on.
pub const NEEDS_HINT: &str = needs_hint!();

/// Appended to the act a point types when a NEW set of needs stands, once a set.
pub const NEEDS_ACK: &str = concat!("Needs on screen; carry on. ", restart_hint!(),);

/// THE NEED A BARE HEAD NAMES WITH NO WORDS UNDER IT.
pub const UNREAD_NEED: &str = "see the NEEDS YOU line in its report";

/// The most characters one need keeps ([`needs_you`]); a longer one is cut with `…`.
pub const NEED_CHARS: usize = 160;

const NONE_HEADS: &[&str] = &["none", "nothing", "no", "nope", "nil", "zero", "n/a", "na"];

const NOT_NEEDED: &[&str] = &["applicable", "needed", "required", "necessary"];

const NONE_MODS: &[&str] = &[
    "need",
    "needs",
    "action",
    "actions",
    "input",
    "blocker",
    "blockers",
    "blocking",
    "blocked",
    "item",
    "items",
    "ask",
    "asks",
    "request",
    "requests",
    "decision",
    "decisions",
    "waiting",
    "pending",
    "outstanding",
    "open",
    "left",
    "new",
    "is",
    "are",
    "to",
    "do",
    "give",
    "that",
    "approve",
    "sign",
    "review",
    "decide",
    "publish",
    "merge",
    "confirm",
    "check",
    "add",
    "report",
    "flag",
    "or",
    "and",
    "from",
    "by",
    "on",
    "for",
    "of",
    "you",
    "your",
    "side",
    "end",
    "part",
    "me",
    "my",
    "i",
    "we",
    "our",
    "owner",
    "at",
    "all",
    "whatsoever",
    "else",
    "other",
    "further",
    "more",
    "here",
    "the",
    "as",
    "now",
    "right",
    "moment",
    "this",
    "time",
    "point",
    "stage",
    "present",
    "currently",
    "today",
];

const NONE_STILL: &[&str] = &["yet", "so", "far"];

const NONE_NEEDED: &[&str] = &["needed", "required", "necessary"];

const NONE_OBJECT_WORDS: usize = 4;

const NONE_KEEPS: &[&str] = &[
    "but",
    "except",
    "besides",
    "than",
    "apart",
    "aside",
    "save",
    "beyond",
    "and",
    "only",
    "just",
    "merely",
    "unless",
    "however",
    "though",
    "although",
    "yet",
    "bar",
    "barring",
    "excluding",
    "outside",
];

const NONE_OBJECTS: &[&str] = &["no"];

const NONE_JOINS: &[&str] = &["and", "or", "as"];

const NONE_ASKED: &[&str] = &["i", "we", "you", "your", "our", "my", "need", "needs"];

const RESTATE_SUBJECTS: &[&str] = &[
    "all",
    "every",
    "it",
    "of",
    "the",
    "and",
    "work",
    "changes",
    "fixes",
    "commits",
    "pr",
    "prs",
    "tests",
    "ci",
    "crate",
    "branches",
    "tree",
    "worktree",
    "worktrees",
];

const RESTATE_NAMED: &[&str] = &["change", "fix", "commit", "release", "tag", "branch"];

const RESTATE_THE: &[&str] = &["the", "every"];

const RESTATE_LINKS: &[&str] = &[
    "it's",
    "it’s",
    "is",
    "are",
    "was",
    "were",
    "has",
    "have",
    "been",
    "already",
    "all",
    "everything",
];

const LANDED_SAYS: &[&str] = &[
    "landed",
    "merged",
    "pushed",
    "done",
    "complete",
    "completed",
    "finished",
    "clean",
    "green",
    "pass",
    "passes",
    "passed",
    "published",
    "set",
    "removed",
    "deleted",
    "gone",
];

const RESTATE_AT: &[&str] = &["on", "in", "into", "to"];

const RESTATE_PLACES: &[&str] = &[
    "the",
    "remote",
    "main",
    "origin/main",
    "origin",
    "master",
    "branch",
];

const NONE_TIMES: &[&str] = &[
    "for",
    "now",
    "right",
    "at",
    "the",
    "moment",
    "this",
    "time",
    "point",
    "stage",
    "present",
    "currently",
    "today",
];

const YOU_SET: &[&str] = &[
    "all", "set", "good", "to", "go", "done", "ready", "covered", "fine",
];

/// The word lists the none reader reads by ([`none_said`]): the reader's own
/// are [`NONE_WORDS`]; the needs table's mutation check takes one word out
/// at a time, so every word is shown load-bearing.
#[derive(Clone, Copy)]
struct NoneWords<'a> {
    /// [`NONE_HEADS`].
    heads: &'a [&'a str],
    /// [`NOT_NEEDED`].
    not_needed: &'a [&'a str],
    /// [`NONE_MODS`].
    mods: &'a [&'a str],
    /// [`NONE_STILL`].
    still: &'a [&'a str],
    /// [`NONE_NEEDED`].
    needed: &'a [&'a str],
    /// [`NONE_KEEPS`].
    keeps: &'a [&'a str],
    /// [`NONE_OBJECTS`].
    objects: &'a [&'a str],
    /// [`NONE_JOINS`].
    joins: &'a [&'a str],
    /// [`NONE_ASKED`].
    asked: &'a [&'a str],
    /// [`RESTATE_SUBJECTS`].
    subjects: &'a [&'a str],
    /// [`RESTATE_NAMED`].
    named: &'a [&'a str],
    /// [`RESTATE_THE`].
    the: &'a [&'a str],
    /// [`RESTATE_LINKS`].
    links: &'a [&'a str],
    /// [`LANDED_SAYS`].
    landed: &'a [&'a str],
    /// [`RESTATE_AT`].
    at: &'a [&'a str],
    /// [`RESTATE_PLACES`].
    places: &'a [&'a str],
    /// [`NONE_TIMES`].
    times: &'a [&'a str],
    /// [`YOU_SET`].
    set: &'a [&'a str],
}

const NONE_WORDS: NoneWords<'static> = NoneWords {
    heads: NONE_HEADS,
    not_needed: NOT_NEEDED,
    mods: NONE_MODS,
    still: NONE_STILL,
    needed: NONE_NEEDED,
    keeps: NONE_KEEPS,
    objects: NONE_OBJECTS,
    joins: NONE_JOINS,
    asked: NONE_ASKED,
    subjects: RESTATE_SUBJECTS,
    named: RESTATE_NAMED,
    the: RESTATE_THE,
    links: RESTATE_LINKS,
    landed: LANDED_SAYS,
    at: RESTATE_AT,
    places: RESTATE_PLACES,
    times: NONE_TIMES,
    set: YOU_SET,
};

const NONE_CLAUSE_MARKS: &[char] = &[
    '—', '–', ';', ':', ',', '(', ')', '[', ']', '.', '!', '?', '{', '}',
];

const QUOTE_MARKS: [char; 4] = ['>', '▎', '│', '┃'];

fn unquoted(t: &str) -> &str {
    let mut t = t.trim_start();
    while let Some(r) = t.strip_prefix(QUOTE_MARKS) {
        t = r.trim_start();
    }
    t
}

const MENTION_NOUNS: &[&str] = &[
    "line", "lines", "head", "heads", "row", "rows", "block", "blocks", "label", "labels", "token",
    "tokens", "prefix", "marker", "markers",
];

const MENTION_ARTICLES: &[&str] = &["a", "an", "the", "each", "every", "any", "one", "that"];

/// The finite forms of `be`, `have` and `do` that the person's `you` is never the subject of
/// (`you is`, `you has` are no English) and their negatives: a bare head one follows is the
/// subject of its clause, a noun, no head (`NEEDS YOU is loud and never blocks.`:
/// [`subject_of`]). `are`, `were`, `had`, `did`, the modals and the base forms agree with `you`,
/// so a need opens on one (`NEEDS YOU had better sign in`, `NEEDS YOU have the key ready`).
const SINGULAR_FINITES: &[&str] = &[
    "is", "isn't", "was", "wasn't", "has", "hasn't", "does", "doesn't",
];

/// The modals and their negatives, a closed class. A modal agrees with any subject, so a bare head
/// before one names its need (`NEEDS YOU must approve the deploy`, `NEEDS YOU can't push: the
/// token expired`), unless the modal's verb is `be` ([`subject_of`]: `NEEDS YOU can be lost.`).
const MODALS: &[&str] = &[
    "can",
    "cannot",
    "can't",
    "could",
    "couldn't",
    "may",
    "might",
    "mightn't",
    "must",
    "mustn't",
    "shall",
    "shan't",
    "should",
    "shouldn't",
    "will",
    "won't",
    "would",
    "wouldn't",
];

/// The adverbs [`subject_of`] reads past between a modal and its `be` (`NEEDS YOU can never be
/// lost.`, `NEEDS YOU can also be raised by codex-alab.`).
const MODAL_ADVERBS: &[&str] = &[
    "not", "never", "always", "also", "still", "only", "now", "just",
];

/// The subjects a singular finite ([`SINGULAR_FINITES`]) opens a question on, a need (`NEEDS YOU
/// is it OK to force-push main`, `NEEDS YOU was I right to revert the pin`: [`subject_of`]).
const ASKED_SUBJECTS: &[&str] = &[
    "i",
    "it",
    "he",
    "she",
    "someone",
    "somebody",
    "anyone",
    "anybody",
    "everyone",
    "everybody",
];

/// Whether `rest`, the words after a bare head (no separator, no qualifier), make the head the
/// subject of their clause, a noun, so no head: they open on a singular finite
/// ([`SINGULAR_FINITES`]: `NEEDS YOU is loud`) or on a modal whose verb is `be` ([`MODALS`],
/// [`MODAL_ADVERBS`]: `NEEDS YOU can be lost.`, `NEEDS YOU should have been raised`), and they
/// ask the person nothing. They ask, a need, where the singular finite opens on a subject
/// ([`ASKED_SUBJECTS`]: `NEEDS YOU is it OK to force-push main`), where their first sentence ends
/// on a question mark ([`sentence_end`]: `NEEDS YOU is the key still valid? If not, rotate it.`,
/// and not `NEEDS YOU is loud. Why does it matter?`), and where a colon follows their first word,
/// which opens the need on it (`NEEDS YOU is: approve the deploy`).
fn subject_of(rest: &str) -> bool {
    let lead = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
    let first = lead
        .find(|c: char| !c.is_alphanumeric() && !matches!(c, '\'' | '’'))
        .unwrap_or(lead.len());
    if lead[first..].trim_start().starts_with(':') {
        return false;
    }
    let mut words = lead
        .split(|c: char| !c.is_alphanumeric() && !matches!(c, '\'' | '’'))
        .map(|w| w.trim_matches(['\'', '’']).replace('’', "'").to_lowercase())
        .filter(|w| !w.is_empty());
    let subject = match words.next() {
        Some(w) if SINGULAR_FINITES.contains(&w.as_str()) => !words
            .next()
            .is_some_and(|w| ASKED_SUBJECTS.contains(&w.as_str())),
        Some(w) if MODALS.contains(&w.as_str()) => {
            let mut verb = words.skip_while(|w| MODAL_ADVERBS.contains(&w.as_str()));
            match verb.next().as_deref() {
                Some("be") => true,
                Some("have") => verb.next().as_deref() == Some("been"),
                _ => false,
            }
        }
        _ => false,
    };
    subject && sentence_end(rest) != Some('?')
}

/// The mark the first sentence of `words` ends on outside brackets: a `.`, `!` or `?` that
/// whitespace or the words' end follows, past any closing mark (`lost.**`, but not `docs?page`,
/// `v1.2` or `(is it ever hidden? no)`), or `None` where none of them ends a sentence.
fn sentence_end(words: &str) -> Option<char> {
    const CLOSERS: &[char] = &['*', '_', '`', '"', '\'', '”', '’', ')', ']'];
    let mut depth = 0usize;
    for (at, c) in words.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '.' | '!' | '?' if depth == 0 => {
                let after = words[at + c.len_utf8()..].trim_start_matches(CLOSERS);
                if after.is_empty() || after.starts_with(char::is_whitespace) {
                    return Some(c);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the words of `text` at byte `at` open on the `you` of a NEEDS YOU token (`needs_token`,
/// any case) whose clause has it for its subject (`subject_of`: `needs you can be lost`): a
/// noun naming the head, not the person, so no phrase that opens on that `you` (`you can`,
/// `you must` in `NEEDS YOU must be answered`) asks them anything. Where the clause does not
/// (`NEEDS YOU must log in to the portal`), the `you` is the person's and its phrase asks.
pub fn token_you(text: &str, at: usize) -> bool {
    let before = text[..at].trim_end_matches([' ', '\t', '\u{a0}', '\u{202f}', '-', '_']);
    let start = before.trim_end_matches(char::is_alphabetic).len();
    !text[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
        && needs_token(&text[start..]).is_some_and(|(len, _)| start + len == at + "you".len())
        && subject_of(&text[at + "you".len()..])
}

/// The demonstratives: behind one, as behind an article, NEEDS YOU is a noun (`This NEEDS YOU
/// (…) is cleared`).
const DEMONSTRATIVES: &[&str] = &["this", "these", "those"];

/// The possessive determiners behind which NEEDS YOU is a noun (`its NEEDS YOU (…) came back`,
/// `your NEEDS YOU (sign in) is stale`). The writer's own `my` and `our` are not among them: a
/// head behind one is the report's own, read as behind any other word (`Here is my NEEDS YOU
/// (owner) approve the release`).
const POSSESSIVES: &[&str] = &["your", "its", "their", "his", "her", "whose"];

/// The determiners no clause ends on, as `one`, `that`, `any`, a demonstrative, `his` and `her`
/// can (`I checked each one`, `I told her`): a row that ends on one, or on a genitive, leaves its
/// clause open for the row the screen wrapped under it ([`wrapped_into`]).
const OPEN_DETERMINERS: &[&str] = &[
    "a", "an", "the", "each", "every", "your", "its", "their", "whose",
];

/// The words whose `'s` is a contracted `is` or `has` (`it's`, `here's`), not a genitive.
const CONTRACTED: &[&str] = &[
    "it", "he", "she", "that", "here", "there", "what", "who", "where", "how", "let",
];

/// The word the words `lead` before a head end on, emphasis and ticks aside (`1's` in `Tab 1's
/// **NEEDS YOU**`), or `None` where they end on a mark (`Done.`, `(`, `` `ship`: ``, the `'` that
/// closes a quote: `said 'all checks pass'`) or on nothing. A plural genitive's apostrophe (`the
/// workers'`) is its word's. Behind a word the head sits inside the clause that word is in;
/// behind a mark it opens a clause of its own.
fn clause_word(lead: &str) -> Option<&str> {
    let lead = lead.trim_end().trim_end_matches(['*', '_', '`']).trim_end();
    let word = lead
        .rsplit(char::is_whitespace)
        .next()?
        .trim_start_matches(|c: char| !c.is_alphanumeric());
    if word.ends_with(char::is_alphanumeric) {
        return Some(word);
    }
    let mark = word.chars().next_back()?;
    (genitive(word) && !quote_open(&lead[..lead.len() - mark.len_utf8()])).then_some(word)
}

/// Whether a single quote `text` opens is still open at its end. A `‘`, or a `'` with no letter
/// or digit before it and a mark or word after it (`'all`, `='%s`), opens one; a `'` or `’` after
/// a letter or digit with none after it (`pass'`) closes one. An apostrophe inside a word (`it's`,
/// `Tab 1's`) is no quote mark.
fn quote_open(text: &str) -> bool {
    let mut open = 0_usize;
    let mut prev: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let after_word = prev.is_some_and(char::is_alphanumeric);
        let next = chars.peek().copied();
        match c {
            '‘' => open += 1,
            '\'' if !after_word && next.is_some_and(|n| !n.is_whitespace()) => open += 1,
            '\'' | '’' if after_word && !next.is_some_and(char::is_alphanumeric) => {
                open = open.saturating_sub(1);
            }
            _ => {}
        }
        prev = Some(c);
    }
    open > 0
}

/// Whether `word` is a genitive (`Tab 1's`, `the worker’s`, `the workers'`), not a contraction
/// ([`CONTRACTED`]: `it's`, `here's`). Whether a plural's `'` closes a quote instead is for the
/// words before it to say ([`clause_word`]).
fn genitive(word: &str) -> bool {
    let word = word.to_lowercase().replace('’', "'");
    match word.strip_suffix("'s") {
        Some(stem) => !stem.is_empty() && !CONTRACTED.contains(&stem),
        None => word.strip_suffix("s'").is_some_and(|stem| !stem.is_empty()),
    }
}

/// Whether `word` ([`clause_word`]) is a determiner: an article or a quantifier
/// ([`MENTION_ARTICLES`]), a demonstrative, a possessive ([`POSSESSIVES`]) or a genitive. A
/// NEEDS YOU behind one is a noun of the clause the determiner is in.
fn determiner(word: &str) -> bool {
    let lower = word.to_lowercase();
    let w = lower.as_str();
    MENTION_ARTICLES.contains(&w)
        || DEMONSTRATIVES.contains(&w)
        || POSSESSIVES.contains(&w)
        || genitive(word)
}

/// Whether `row` may be the rest of `above`'s line, the screen having wrapped it: `above` is
/// prose that ends on a word, not a heading (`## Owner's`), a fence, or a closing emphasis or
/// mark (`**Andrew's**`, `answered 'Yes'`), and `row` opens on its words, not on a list item's,
/// a checkbox's or a heading's mark of its own.
fn wrapped_into(above: &str, row: &str) -> bool {
    let above = unquoted(above).trim_end();
    let row = unquoted(row);
    let ends_on_word = above.ends_with(char::is_alphanumeric)
        || (above.ends_with(['\'', '’']) && clause_word(above).is_some());
    ends_on_word
        && !past_heading(above).1
        && !fence(above)
        && past_marker(row).is_none()
        && past_checkbox(row).is_none()
        && !past_heading(row).1
}

const HEAD_NAMING: &[&str] = &[
    "starting",
    "starts",
    "beginning",
    "begins",
    "prefixed",
    "headed",
];

const HEAD_NAMING_LINKS: &[&str] = &["with", "by"];

fn names_a_head(lead: &str) -> bool {
    let said: Vec<&str> = lead
        .rsplit(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(3)
        .collect();
    let words: Vec<String> = said.iter().map(|w| w.to_lowercase()).collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let (naming, verb_at) = match words.as_slice() {
        [link, rest @ ..] if HEAD_NAMING_LINKS.contains(link) => (rest, 1),
        all => (all, 0),
    };
    match naming {
        [verb, noun, ..] => HEAD_NAMING.contains(verb) && MENTION_NOUNS.contains(noun),
        [verb] => {
            HEAD_NAMING.contains(verb)
                && said[verb_at].chars().next().is_some_and(char::is_lowercase)
        }
        [] => false,
    }
}

fn past_marker(t: &str) -> Option<&str> {
    for m in ["- ", "* ", "+ ", "• ", "◦ ", "‣ "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some(rest.trim_start());
        }
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    let rest = t.get(digits..).filter(|_| (1..=3).contains(&digits))?;
    rest.strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .map(str::trim_start)
}

fn past_checkbox(t: &str) -> Option<&str> {
    ["[ ]", "[x]", "[X]", "[]"]
        .into_iter()
        .find_map(|b| t.strip_prefix(b))
        .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
        .map(str::trim_start)
}

fn leads_need(c: char) -> bool {
    !c.is_ascii()
        && !c.is_alphanumeric()
        && !c.is_whitespace()
        && !QUOTE_MARKS.contains(&c)
        && !matches!(
            c,
            '“' | '”'
                | '‘'
                | '’'
                | '«'
                | '»'
                | '„'
                | '‚'
                | '‹'
                | '›'
                | '「'
                | '」'
                | '『'
                | '』'
                | '⎿'
        )
        && !('\u{2500}'..='\u{257F}').contains(&c)
}

fn past_need_lead(t: &str) -> &str {
    let mut t = t.trim_start();
    loop {
        let before = t.len();
        t = past_heading(t).0;
        t = past_marker(t).unwrap_or(t);
        t = past_checkbox(t).unwrap_or(t);
        if let Some(r) = ["**", "__", "*", "_"]
            .into_iter()
            .find_map(|m| t.strip_prefix(m))
        {
            t = r.trim_start();
        }
        let lead: usize = t
            .chars()
            .take_while(|c| leads_need(*c))
            .map(char::len_utf8)
            .sum();
        t = t[lead..].trim_start();
        if t.len() == before {
            return t;
        }
    }
}

fn needs_token(t: &str) -> Option<(usize, bool)> {
    let need = t.get(..4).filter(|w| w.eq_ignore_ascii_case("need"))?;
    let s = usize::from(t[4..].starts_with(['s', 'S']));
    let word = 4 + s;
    let gap: usize = t[word..]
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t' | '\u{a0}' | '\u{202f}' | '-' | '_'))
        .map(char::len_utf8)
        .sum();
    let you = t
        .get(word + gap..word + 3 + gap)
        .filter(|w| gap > 0 && w.eq_ignore_ascii_case("you"))?;
    let capitals = need == "NEED" && (s == 0 || &t[4..5] == "S") && you == "YOU";
    Some((word + 3 + gap, capitals))
}

fn your_head(rest: &str) -> Option<&str> {
    let words = rest
        .get(1..)
        .filter(|w| rest.starts_with(['r', 'R']) && w.starts_with([' ', '\t']))?;
    let end = words.find([':', '—', '–']).or_else(|| words.find(" - "))?;
    let named = words[..end].trim().trim_matches(['*', '_']).trim();
    let count = named.split_whitespace().count();
    if !(1..=3).contains(&count)
        || !named
            .chars()
            .all(|c| c.is_alphabetic() || c.is_whitespace() || matches!(c, '-' | '\'' | '’'))
    {
        return None;
    }
    past_separator(words[end..].trim_start()).map(str::trim)
}

fn need_separator(rest: &str, capitals: bool) -> Option<&str> {
    rest.strip_prefix("--")
        .or_else(|| rest.strip_prefix("..."))
        .or_else(|| rest.strip_prefix('…'))
        .or_else(|| past_separator(rest))
        .or_else(|| {
            ["→", "->", "=>"]
                .into_iter()
                .find_map(|a| rest.strip_prefix(a))
        })
        .or_else(|| rest.strip_prefix(['!', ';', '?']).filter(|_| capitals))
}

fn need_rest(line: &str) -> Option<&str> {
    head_from(past_need_lead(unquoted(line)), false, false)
}

/// The words of the need the head that opens `t` names (empty where its row names none), or
/// `None` where `t` opens no head. `named`: words before the head may name it as a thing (an
/// article, a quote mark), so a mention noun after it (`line`, `head`) makes it no head.
/// `inside`: the head is a noun behind a determiner of the clause it sits in ([`determiner`]:
/// `The NEEDS YOU (…) was raised`, `Tab 1's NEEDS YOU, the release go, is cleared`), so its
/// need's words are its own only past a separator that ends that clause (`Tab 1's NEEDS YOU:
/// approve the release`), as [`mid_head`] holds `NEEDS YOU` itself to a closing mark. A qualifier
/// in brackets, or a name after a comma, ends no clause: words after them that no separator
/// opens go on with the clause the head sits in, and a qualifier with none after it names the
/// noun (`No session has a NEEDS YOU (checked all six tabs).`); neither is a need, unless the
/// qualifier says none (`the NEEDS YOU (none)`). A bare head, one no separator or qualifier
/// follows, is no head where its words make it the subject of their clause and ask the person
/// nothing ([`subject_of`]: `NEEDS YOU is loud`, `NEEDS YOU can be lost.`); a modal and its verb,
/// an imperative or a noun after it is its need (`NEEDS YOU must approve the deploy`, `NEEDS YOU
/// approve the deploy`, `NEEDS YOU the release credential`).
fn head_from(t: &str, named: bool, inside: bool) -> Option<&str> {
    head_parts(t, named, inside)
        .filter(|&(rest, bare)| !(bare && subject_of(rest)))
        .map(|(rest, _)| rest)
}

/// The head that opens `t` as [`head_from`] reads it, whatever the words after it say, and
/// whether it is bare: no separator and no qualifier follows it. A head so read opens a line of
/// its own ([`a_head`]) even where its words make it the subject of their clause.
fn head_parts(t: &str, named: bool, inside: bool) -> Option<(&str, bool)> {
    let wrap = t.chars().next().filter(|c| matches!(c, '[' | '|' | '`'));
    let t = match wrap {
        Some(c) => past_need_lead(&t[c.len_utf8()..]),
        None => t,
    };
    let named = named || wrap == Some('`');
    let (len, capitals) = needs_token(t)?;
    let rest = &t[len..];
    if let Some(words) = your_head(rest) {
        let words = if says_none(words) || words.is_empty() {
            words
        } else {
            t[len - 3..].trim()
        };
        return Some((words, false));
    }
    let mut next = rest.chars();
    match next.next() {
        Some(c) if c.is_alphanumeric() || matches!(c, '\'' | '’') => return None,
        Some('-') if next.next().is_some_and(char::is_alphanumeric) => return None,
        _ => {}
    }
    fn emphasis(r: &str) -> &str {
        r.trim_start_matches(['*', '_']).trim_start()
    }
    let after = rest.trim_start();
    let mut emphasized = after.starts_with(['*', '_']);
    let mut rest = emphasis(after);
    let mut separated = false;
    if let Some(open) = wrap {
        let close = if open == '[' { ']' } else { open };
        match rest.strip_prefix(close) {
            Some(r) => {
                rest = emphasis(r.trim_start());
                emphasized = true;
            }
            None if open == '`' => {
                let r = emphasis(need_separator(rest, capitals)?);
                rest = match r.strip_prefix('`') {
                    Some(after) => emphasis(after),
                    None => r.trim_end().trim_end_matches('`').trim_end(),
                };
                separated = true;
            }
            None => {}
        }
        if open == '|' {
            rest = rest.trim_end().trim_end_matches('|').trim_end();
        }
    }
    let mut qualifier = None;
    if !separated {
        for (open, close) in [('(', ')'), ('[', ']')] {
            if let Some(inner) = rest.strip_prefix(open)
                && let Some(end) = inner.find(close)
            {
                qualifier = Some(inner[..end].trim());
                rest = emphasis(inner[end + close.len_utf8()..].trim_start());
                break;
            }
        }
    }
    if !separated
        && qualifier.is_none()
        && let Some(inner) = rest.strip_prefix(',')
    {
        let who = inner.find([':', '—', '–']).and_then(|end| {
            let who = inner[..end].trim().trim_matches(['*', '_']);
            let words = who.split_whitespace().count();
            ((1..=3).contains(&words)
                && who.chars().all(|c| {
                    c.is_alphabetic() || c.is_whitespace() || matches!(c, '.' | '\'' | '’')
                }))
            .then_some((who, end))
        });
        match who {
            Some((who, end)) => {
                qualifier = Some(who);
                rest = emphasis(inner[end..].trim_start());
            }
            None if capitals && !inside => {
                rest = emphasis(inner);
                separated = true;
            }
            None => return None,
        }
    }
    if !separated && let Some(r) = need_separator(rest, capitals) {
        rest = emphasis(r);
        separated = true;
    }
    let mut rest = rest.trim();
    if !separated && qualifier.is_none() {
        if rest.starts_with(['.', ',', '?', '!', ';']) {
            return None;
        }
        if !capitals && !emphasized {
            return None;
        }
    }
    let bare = !separated && qualifier.is_none();
    if named || bare {
        let first = rest
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or("")
            .to_lowercase();
        if MENTION_NOUNS.contains(&first.as_str()) {
            return None;
        }
    }
    if qualifier.is_some() && !separated && rest.chars().all(|c| c == '.') {
        rest = "";
    }
    if inside && !separated && (!rest.is_empty() || qualifier.is_some_and(|q| !says_none(q))) {
        return None;
    }
    let words = match qualifier {
        Some(q) if rest.is_empty() && says_none(q) => q,
        Some(q) if rest.is_empty() && !separated && q.split_whitespace().count() >= 3 => q,
        _ => rest,
    };
    Some((words, bare))
}

fn past_heading(t: &str) -> (&str, bool) {
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    match t.get(hashes..) {
        Some(rest) if (1..=6).contains(&hashes) && rest.starts_with([' ', '\t']) => {
            (rest.trim_start(), true)
        }
        _ => (t, false),
    }
}

fn head_words(head: &str) -> &'static [&'static str] {
    if head == NOTHING_LEFT {
        &[" to do", " to land"]
    } else {
        &[]
    }
}

fn past_separator(rest: &str) -> Option<&str> {
    rest.strip_prefix(':')
        .or_else(|| rest.strip_prefix('—'))
        .or_else(|| rest.strip_prefix('–'))
        .or_else(|| {
            rest.strip_prefix('-')
                .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
        })
}

fn head_rest<'a>(line: &'a str, head: &str) -> Option<&'a str> {
    let t = line.trim_start();
    if t.starts_with(QUOTE_MARKS) {
        return None;
    }
    let (t, heading) = past_heading(t);
    let t = past_marker(t).unwrap_or(t);
    let mark = ["**", "__", "*", "_"]
        .into_iter()
        .find(|m| t.starts_with(m))
        .unwrap_or("");
    let t = &t[mark.len()..];
    let name = head.trim_end_matches(':');
    let said = t.get(..name.len())?;
    if !said.eq_ignore_ascii_case(name) {
        return None;
    }
    let mut rest = &t[name.len()..];
    for words in head_words(head) {
        if let Some(r) = rest
            .get(..words.len())
            .filter(|w| w.eq_ignore_ascii_case(words))
            .map(|_| &rest[words.len()..])
        {
            rest = r;
            break;
        }
    }
    let mut rest = rest.trim_start();
    let mut closed = mark.is_empty();
    if !closed && let Some(r) = rest.strip_prefix(mark) {
        rest = r.trim_start();
        closed = true;
    }
    rest = match past_separator(rest) {
        Some(r) => r,
        None if heading && rest.trim().is_empty() => return Some(""),
        None => return None,
    };
    if !closed && let Some(r) = rest.strip_prefix(mark) {
        rest = r;
        closed = true;
    }
    let mut rest = rest.trim();
    if !closed {
        rest = rest.strip_suffix(mark).unwrap_or(rest).trim_end();
    }
    Some(rest)
}

/// A Markdown code-fence run and the text after it.
pub fn code_fence(line: &str) -> Option<(u8, usize, &str)> {
    let line = line.trim();
    let mark = *line.as_bytes().first()?;
    if !matches!(mark, b'`' | b'~') {
        return None;
    }
    let width = line.bytes().take_while(|&b| b == mark).count();
    (width >= 3).then(|| (mark, width, &line[width..]))
}

fn fence(line: &str) -> bool {
    code_fence(line).is_some()
}

/// Whether `line` belongs to a fenced block, including its delimiter rows.
pub fn fenced_line(line: &str, open: &mut Option<(u8, usize)>) -> bool {
    if let Some((mark, width)) = *open {
        if code_fence(line)
            .is_some_and(|(m, n, rest)| m == mark && n >= width && rest.trim().is_empty())
        {
            *open = None;
        }
        return true;
    }
    if let Some((mark, width, _)) = code_fence(line) {
        *open = Some((mark, width));
        return true;
    }
    false
}

fn clause_words(clause: &str) -> Vec<&str> {
    clause
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '/' | '\'' | '’' | '-')))
        .map(|w| w.trim_matches(|c: char| matches!(c, '/' | '\'' | '’' | '-')))
        .filter(|w| !w.is_empty())
        .collect()
}

fn none_clause(words: &[&str], by: &NoneWords<'_>) -> bool {
    let (head, rest) = match words {
        ["not", w, rest @ ..] if by.not_needed.contains(w) => ("not", rest),
        [w, rest @ ..] if by.heads.contains(w) => (*w, rest),
        _ => return false,
    };
    if rest
        .windows(2)
        .any(|p| by.joins.contains(&p[0]) && by.asked.contains(&p[1]))
    {
        return false;
    }
    if !rest.is_empty() && rest.iter().all(|w| by.still.contains(w)) {
        return true;
    }
    let rest = match rest.iter().position(|w| by.needed.contains(w)) {
        Some(k)
            if k <= NONE_OBJECT_WORDS
                && !rest[..k].iter().any(|w| by.keeps.contains(w))
                && (by.objects.contains(&head)
                    || rest[..k].iter().all(|w| by.mods.contains(w))) =>
        {
            &rest[k + 1..]
        }
        _ => rest,
    };
    rest.iter().all(|w| by.mods.contains(w))
}

fn you_set(words: &[&str], by: &NoneWords<'_>) -> bool {
    let rest = match words {
        ["you're" | "you’re", rest @ ..] | ["you", "are", rest @ ..] => rest,
        _ => return false,
    };
    !rest.is_empty() && rest.iter().all(|w| by.set.contains(w))
}

fn restated(words: &[&str], by: &NoneWords<'_>) -> bool {
    let mut k = 0;
    while let Some(w) = words.get(k) {
        let named = by.named.contains(w) && k > 0 && by.the.contains(&words[k - 1]);
        if !(by.subjects.contains(w) || named || w.bytes().all(|b| b.is_ascii_digit())) {
            break;
        }
        k += 1;
    }
    while words.get(k).is_some_and(|w| by.links.contains(w)) {
        k += 1;
    }
    let linked = words[..k].iter().any(|w| by.links.contains(w));
    let place = |at: usize| {
        let end = at
            + 1
            + words[at + 1..]
                .iter()
                .take_while(|w| by.places.contains(w))
                .count();
        (end > at + 1).then_some(end)
    };
    match words.get(k) {
        Some(w) if by.landed.contains(w) => k += 1,
        Some(w) if linked && by.at.contains(w) => match place(k) {
            Some(end) => k = end,
            None => return false,
        },
        _ => return false,
    }
    while let Some(w) = words.get(k) {
        if *w == "and" && words.get(k + 1).is_some_and(|w| by.landed.contains(w)) {
            k += 2;
        } else if by.at.contains(w) {
            match place(k) {
                Some(end) => k = end,
                None => return false,
            }
        } else if by.times.contains(w) {
            k += 1;
        } else {
            return false;
        }
    }
    true
}

fn says_none(rest: &str) -> bool {
    none_said(rest, &NONE_WORDS).is_some()
}

fn none_said(rest: &str, by: &NoneWords<'_>) -> Option<String> {
    let cleaned: String = rest.chars().filter(|c| *c != '`').collect();
    let cleaned = cleaned.trim().to_string();
    let lower = cleaned.to_lowercase();
    let mut lower_chars = lower.chars();
    let same_offsets = cleaned.chars().all(|c| {
        lower_chars
            .next()
            .is_some_and(|lower| c.len_utf8() == lower.len_utf8())
    }) && lower_chars.next().is_none();
    let written = if same_offsets {
        cleaned.as_str()
    } else {
        lower.as_str()
    };
    let body = lower.trim_start_matches(|c: char| {
        matches!(c, '—' | '–' | '-' | '*' | '_') || c.is_whitespace()
    });
    if body.is_empty() {
        return lower.contains(['—', '–', '-']).then(String::new);
    }
    let from = lower.len() - body.len();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start = from;
    for (at, c) in lower[from..].char_indices().map(|(k, c)| (k + from, c)) {
        let spaced_dash = c == '-'
            && lower[..at].ends_with(' ')
            && lower[at..].trim_start_matches('-').starts_with(' ');
        if NONE_CLAUSE_MARKS.contains(&c) || spaced_dash {
            spans.push((start, at));
            start = at + c.len_utf8();
        }
    }
    spans.push((start, lower.len()));
    let mut clauses = spans
        .into_iter()
        .map(|(a, b)| (b, clause_words(&lower[a..b])))
        .filter(|(_, words)| !words.is_empty());
    let (after, first) = clauses.next()?;
    if !none_clause(&first, by) {
        return None;
    }
    let mut restates = false;
    let bare = first.len() == 1 || first[0] == "not" && first.len() == 2;
    for (_, words) in clauses {
        let timed = words
            .iter()
            .all(|w| by.times.contains(w) || bare && by.still.contains(w));
        if none_clause(&words, by) || timed || you_set(&words, by) {
            continue;
        }
        if !restated(&words, by) {
            return None;
        }
        restates = true;
    }
    if !restates {
        return Some(String::new());
    }
    let clause = written[after..].trim_start_matches(|c: char| !c.is_alphanumeric());
    let clause =
        clause.trim_end_matches(|c: char| matches!(c, ')' | ']' | '*' | '_') || c.is_whitespace());
    Some(clause.to_string())
}

/// The NEEDS YOU lines of a report, read once ([`needs_lines`]).
#[derive(Default)]
struct NeedLines {
    /// Which lines of the tail are needs (heads, their items, their wrapped
    /// rows), by index.
    lines: Vec<usize>,
    /// The lines a head opens past their start ([`mid_head`]: `Nothing
    /// left: all on origin/main. NEEDS YOU: none`), by index, each with
    /// where its head starts: the words before it stay every other
    /// reader's.
    cuts: Vec<(usize, usize)>,
    /// Each need's words as written, wrapped rows joined.
    needs: Vec<String>,
    /// A line said none stands ([`none_said`]).
    cleared: bool,
    /// The clauses none lines said after their none (`nothing — everything
    /// is on origin/main`: [`none_said`]), by the index of the line that
    /// keeps each in [`without_needs`]: no need, but words the readers of
    /// open work read.
    clauses: Vec<(usize, String)>,
    /// In the go-ahead reader's view ([`go_ahead_view`]), the rows a need took only there
    /// ([`go_ahead_wrap`]), and the rows it took after them, each run with the need's words joined
    /// and before it ([`Moved`]); every other reader's view takes none.
    moved: Vec<Moved>,
}

impl NeedLines {
    fn none_at(&mut self, at: usize, clause: String) {
        self.cleared = true;
        if !clause.is_empty() {
            self.clauses.push((at, clause));
        }
    }

    fn said(&mut self, at: usize, words: &str, by: &NoneWords<'_>) {
        match none_said(words, by) {
            Some(clause) => self.none_at(at, clause),
            None => self.needs.push(words.to_string()),
        }
    }
}

/// Whether `line` opens on a head: a NEEDS YOU head whatever its words say ([`head_parts`]: a
/// subject's too, `NEEDS YOU can be lost.`, so a question after a bare head stays a line of its
/// own), or a defense's.
fn a_head(line: &str) -> bool {
    opens_head(line) || defense_rest(line).is_some()
}

/// Whether `line` opens on a NEEDS YOU head, whatever its words say ([`head_parts`]).
fn opens_head(line: &str) -> bool {
    head_parts(past_need_lead(unquoted(line)), false, false).is_some()
}

fn paragraph(under: &str) -> bool {
    let t = unquoted(under).trim_end();
    !t.is_empty() && !fence(t) && !a_head(under)
}

/// Whether the row `next` continues `need`, a row the screen wrapped: the two are not [`apart`]
/// and the row opens lowercase or with a digit. THE IRREVERSIBLE GUARD'S CUT ([`without_needs`])
/// and every reader of the needs join a wrapped row so and only so: 246d1ddc2f's rule, which no
/// go-ahead code reads into or widens. Only the go-ahead reader's own view ([`go_ahead_view`])
/// takes more ([`go_ahead_wrap`]).
fn wraps(need: &str, next: &str) -> bool {
    !apart(need, next) && opens_lower(unquoted(next))
}

/// Whether `t` opens on a lowercase letter or a digit.
fn opens_lower(t: &str) -> bool {
    t.chars()
        .next()
        .is_some_and(|c| c.is_lowercase() || c.is_ascii_digit())
}

/// Whether row `next` opens a line of its own under the line `line`, whatever its first letter
/// and the room the row above it left: a blank row, a fence, a list item, a need's or a defense's
/// head, or any row under a line that ends a sentence or opens a list. A screen's reader joins a
/// row the pane's width wrapped only where this says it may (aterm's `aterm_phase::reader::Apart`).
pub fn apart(line: &str, next: &str) -> bool {
    let t = unquoted(next).trim_end();
    t.is_empty()
        || fence(t)
        || past_marker(t).is_some()
        || a_head(next)
        || line.trim_end().ends_with(['.', '!', '?', ':'])
}

/// How a need takes the row under it ([`needs_lines_by`]): `None` where it does not, `Some(false)`
/// where it [`wraps`] it, `Some(true)` where it takes it only in the go-ahead reader's own view.
type Join = fn(&str, &str) -> Option<bool>;

/// The join of every reader but the go-ahead reader's own view: [`wraps`] alone.
fn wrap_join(need: &str, next: &str) -> Option<bool> {
    wraps(need, next).then_some(false)
}

/// THE GO-AHEAD READER'S JOIN ([`go_ahead_view`]): `Some(false)` where the row [`wraps`] the
/// need, and `Some(true)` where, the two not [`apart`], it joins only in that view: it opens
/// lowercase past an opening quote or bracket (`… then say` above `"rerun the checks" so …`), or
/// the need stops mid-clause (`… say "rerun the checks" so` above `I can build …`). No reader but
/// [`go_ahead_view`] passes it.
fn go_ahead_wrap(need: &str, next: &str) -> Option<bool> {
    if wraps(need, next) {
        return Some(false);
    }
    (!apart(need, next)
        && (opens_lower(unquoted(next).trim_start_matches(['"', '\'', '`', '(', '“', '‘']))
            || mid_clause(need)))
    .then_some(true)
}

/// Joins to `need` the rows from `i` on that `join` takes, each pushed to `out.lines`; the rows
/// from the first that joins only in the go-ahead reader's view on are booked in `out.moved`,
/// with the need's words before that row. Returns the row after the last joined.
fn join_wrapped(
    rows: &[&str],
    mut i: usize,
    need: &mut String,
    join: Join,
    out: &mut NeedLines,
) -> usize {
    let mut moved = Vec::new();
    let mut base = None;
    while let Some(late) = rows.get(i).and_then(|next| join(need, next)) {
        if late && moved.is_empty() {
            base = Some(need.clone());
        }
        need.push(' ');
        need.push_str(unquoted(rows[i]).trim_end());
        out.lines.push(i);
        if late || !moved.is_empty() {
            moved.push(i);
        }
        i += 1;
    }
    if let Some(base) = base {
        out.moved.push(Moved {
            need: need.clone(),
            base,
            rows: moved,
        });
    }
    i
}

/// Whether `need` stops mid-clause: on a comma, or on a word no sentence ends on.
fn mid_clause(need: &str) -> bool {
    const OPEN_ENDS: &[&str] = &[
        "so", "and", "or", "but", "then", "to", "the", "a", "an", "of", "for", "with", "which",
        "if", "when", "once", "until", "unless", "before", "after", "because", "since", "while",
        "whether", "can", "will", "would", "could", "should", "my", "your", "its", "our", "their",
        "is", "are", "was", "were", "be", "on", "in", "at", "into", "onto", "from", "by", "as",
        "than",
    ];
    let t = need.trim_end();
    t.ends_with(',')
        || t.rsplit(char::is_whitespace)
            .next()
            .is_some_and(|w| OPEN_ENDS.contains(&w.to_lowercase().as_str()))
}

const CUT_INSTRUCTIONS: [&str; 3] = [
    "and carry on.",
    "and carry on with everything else.",
    "and land the rest.",
];

fn cut_instruction(rest: &str, rows: &[&str]) -> bool {
    const CLOSERS: &[char] = &[')', ']', '"', '\'', '`', '”', '’', '*', '_'];
    let is_cut = |words: &str| {
        let words = words.trim_end_matches(CLOSERS);
        CUT_INSTRUCTIONS.iter().any(|inst| {
            words == *inst
                || words
                    .strip_prefix(inst)
                    .is_some_and(|after| after.trim_start_matches(CLOSERS).starts_with(' '))
        })
    };
    let mut words = rest.to_string();
    if is_cut(&words) {
        return true;
    }
    for row in rows {
        if !wraps(&words, row) {
            break;
        }
        words.push(' ');
        words.push_str(unquoted(row).trim_end());
        if is_cut(&words) {
            return true;
        }
    }
    false
}

/// The need's words of the head `rows[i]` opens, as [`need_rest`] reads them, but a bare head is
/// judged on its line's first sentence read on into the rows the writer or the screen wrapped
/// under it ([`wraps`]): `NEEDS YOU is the release key still` above `valid?` asks the person, a
/// need, and `NEEDS YOU can` above `be lost.` is its clause's subject, none ([`subject_of`]).
fn head_need<'a>(rows: &[&'a str], i: usize) -> Option<&'a str> {
    let (rest, bare) = head_parts(past_need_lead(unquoted(rows[i])), false, false)?;
    if !bare {
        return Some(rest);
    }
    let mut line = rest.to_string();
    for next in &rows[i + 1..] {
        if sentence_end(&line).is_some() || !wraps(&line, next) {
            break;
        }
        line.push(' ');
        line.push_str(unquoted(next).trim_end());
    }
    (!subject_of(&line)).then_some(rest)
}

const MID_OPENS: &[char] = &['*', '_', '`', '['];

const MID_QUOTES: &[char] = &['"', '“', '‘', '\''];

/// The head `row` opens past its start (`Done. NEEDS YOU: rotate the key`), at byte `from` or
/// later: where the words before it end, and its need's words. A head here is a capitalised
/// `NEEDS YOU` a closing mark ends (`:`, `—`, `(`, …). Behind a determiner ([`clause_word`],
/// [`determiner`]: `Tab 1's NEEDS YOU (…) is cleared`) it is a noun inside that determiner's
/// clause ([`head_from`]'s `inside`).
fn mid_head<'a>(
    row: &'a str,
    above: &str,
    below: &[&str],
    from: usize,
) -> Option<(usize, &'a str)> {
    if unquoted(row).starts_with('⎿') {
        return None;
    }
    row.match_indices("NEED").find_map(|(at, _)| {
        if at < from
            || row[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            return None;
        }
        let (len, capitals) = needs_token(&row[at..])?;
        let marked = row[at + len..].trim_start_matches(['*', '_', '`', '"', '”', '’', '\'']);
        let after = marked.trim_start();
        let closed = after.starts_with([':', '—', '–', '(', '[', ',', '?', '!', ';', '…', '→'])
            || ["--", "->", "=>", "..."]
                .iter()
                .any(|p| after.starts_with(p))
            || (after.starts_with("- ") && marked.len() > after.len());
        if !capitals || !closed {
            return None;
        }
        let start = row[..at].trim_end_matches(MID_OPENS).len();
        if row[..at].matches('`').count() % 2 == 1 && !row[start..at].contains('`') {
            return None;
        }
        let lead = row[..start].trim_end_matches(MID_QUOTES);
        let quoted = lead.len() < start;
        let before = lead
            .trim_end()
            .rsplit(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or("")
            .to_lowercase();
        if matches!(before.as_str(), "no" | "zero" | "without") {
            return None;
        }
        if names_a_head(&format!("{above} {lead}")) {
            return None;
        }
        let article = MENTION_ARTICLES.contains(&before.as_str());
        let cut = lead.trim_end().len();
        let own =
            lead.trim_end().is_empty() || lead.trim_end().ends_with(['.', ';', '!', '?', '—', '–']);
        head_from(
            past_need_lead(&row[start..]),
            quoted || article,
            clause_word(lead).is_some_and(determiner),
        )
        .filter(|rest| !cut_instruction(rest, below))
        .filter(|rest| own || rest.chars().any(char::is_alphanumeric))
        .map(|rest| (cut, rest))
    })
}

fn items_under(
    rows: &[&str],
    (head, mut i): (usize, usize),
    colon: bool,
    out: &mut NeedLines,
    by: &NoneWords<'_>,
) -> (usize, usize) {
    let indent = |r: &str| r.len() - r.trim_start().len();
    let sibling = past_marker(rows[head].trim_start()).is_some();
    let start = i;
    let mut items = 0;
    let mut blanks = Vec::new();
    while let Some(&under) = rows.get(i) {
        if colon && items == 0 && unquoted(under).is_empty() {
            blanks.push(i);
            i += 1;
            continue;
        }
        let Some(item) = past_marker(unquoted(under)) else {
            break;
        };
        if fence(under) || a_head(under) || (sibling && indent(under) <= indent(rows[head])) {
            break;
        }
        out.lines.append(&mut blanks);
        out.lines.push(i);
        out.said(i, item, by);
        items += 1;
        i += 1;
    }
    if items == 0 {
        return (start, 0);
    }
    (i, items)
}

/// The NEEDS YOU lines of `tail` as the irreversible guard and every reader of the needs read them,
/// a need's wrapped rows joined by [`wraps`] alone ([`wrap_join`]).
fn needs_lines(tail: &str) -> NeedLines {
    needs_lines_by(tail, &NONE_WORDS, wrap_join)
}

/// The NEEDS YOU lines of `tail`, a need's wrapped rows joined as `join` takes them: [`wrap_join`]
/// for every reader, [`go_ahead_wrap`] for the go-ahead reader's own view alone.
fn needs_lines_by(tail: &str, by: &NoneWords<'_>, join: Join) -> NeedLines {
    let rows: Vec<&str> = tail.lines().collect();
    let mut out = NeedLines::default();
    let mut block = None;
    let mut i = 0;
    while i < rows.len() {
        let row = rows[i];
        if fenced_line(row, &mut block) {
            i += 1;
            continue;
        }
        let above = if i > 0 { rows[i - 1] } else { "" };
        if names_a_head(above) && opens_head(row) {
            i += 1;
            continue;
        }
        if need_rest(row).is_some_and(|rest| cut_instruction(rest, &rows[i + 1..])) {
            i += 1;
            continue;
        }
        let head = i;
        // A row the screen wrapped out of the row above's line ([`wrapped_into`]) reads on in
        // that line's clause: past an article the head is named (`… a` above `NEEDS YOU: line
        // …`), and past a determiner no clause ends on, a genitive among them, it is a noun
        // inside the row above's clause (`… Tab 1's` above `NEEDS YOU (…) is cleared`). A head
        // that is no head so is a mention, and a later head on its row is still read past it.
        let wrapped = wrapped_into(above, row);
        let named_above = wrapped
            && above
                .rsplit(|c: char| !c.is_alphanumeric())
                .find(|w| !w.is_empty())
                .is_some_and(|w| MENTION_ARTICLES.contains(&w.to_lowercase().as_str()));
        let inside_above = wrapped
            && clause_word(above).is_some_and(|w| {
                OPEN_DETERMINERS.contains(&w.to_lowercase().as_str()) || genitive(w)
            });
        let opens = if named_above || inside_above {
            head_from(past_need_lead(unquoted(row)), named_above, inside_above)
        } else {
            head_need(&rows, i)
        };
        let rest = match opens {
            Some(rest) => {
                out.lines.push(i);
                rest
            }
            None => {
                let from = if need_rest(row).is_some() {
                    row.len() - past_need_lead(unquoted(row)).len() + "NEED".len()
                } else {
                    0
                };
                match mid_head(row, above, &rows[i + 1..], from) {
                    Some((cut, rest)) => {
                        out.cuts.push((i, cut));
                        rest
                    }
                    None => {
                        i += 1;
                        continue;
                    }
                }
            }
        };
        i += 1;
        if rest.is_empty() {
            let mut items = 0;
            let mut blanks = Vec::new();
            while let Some(&under) = rows.get(i) {
                if unquoted(under).is_empty() && items == 0 {
                    blanks.push(i);
                    i += 1;
                    continue;
                }
                let Some(item) = past_marker(unquoted(under)) else {
                    break;
                };
                if a_head(under) {
                    break;
                }
                out.lines.append(&mut blanks);
                out.lines.push(i);
                out.said(i, item, by);
                items += 1;
                i += 1;
            }
            if items > 0 {
                continue;
            }
            match rows.get(i).filter(|under| paragraph(under)) {
                Some(&under) => {
                    out.lines.append(&mut blanks);
                    out.lines.push(i);
                    let at = i;
                    i += 1;
                    let mut need = unquoted(under).trim_end().to_string();
                    i = join_wrapped(&rows, i, &mut need, join, &mut out);
                    out.said(at, &need, by);
                }
                None => out.needs.push(UNREAD_NEED.to_string()),
            }
            continue;
        }
        let mut words = rest.to_string();
        i = join_wrapped(&rows, i, &mut words, join, &mut out);
        let colon = words.trim_end().ends_with(':');
        if colon {
            let (next, items) = items_under(&rows, (head, i), true, &mut out, by);
            if items == 0 {
                out.said(head, &words, by);
            }
            i = next;
        } else if none_said(&words, by).is_some() {
            out.said(head, &words, by);
            i = items_under(&rows, (head, i), false, &mut out, by).0;
        } else {
            out.said(head, &words, by);
        }
    }
    out
}

/// A need as it is shown: each control or hidden character ([`crate::text::is_hidden_char`]) a
/// space, each run of whitespace one space, `*` and `_` trimmed from its ends, and at most
/// [`NEED_CHARS`] characters, a longer one cut with `…`.
pub fn sanitized(need: &str) -> String {
    let clean: String = need
        .chars()
        .map(|c| {
            if c.is_control() || crate::text::is_hidden_char(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let words = words.trim_matches(|c: char| matches!(c, '*' | '_') || c.is_whitespace());
    if words.chars().count() <= NEED_CHARS {
        return words.to_string();
    }
    let mut cut: String = words.chars().take(NEED_CHARS - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// What the report `tail` names after a `NEEDS YOU:` head, each need one line: the grammar's
/// read, credential lines among them ([`persons_needs`] leaves those out).
#[must_use]
pub fn needs_you(tail: &str) -> Vec<String> {
    let read = needs_lines(tail);
    let mut out: Vec<String> = Vec::new();
    for need in &read.needs {
        let need = sanitized(need);
        if !need.is_empty() && !out.contains(&need) {
            out.push(need);
        }
    }
    if out.is_empty() && !read.cleared && !read.needs.is_empty() {
        out.push(UNREAD_NEED.to_string());
    }
    out
}

/// What the report `tail` names only a person can give ([`needs_you`]): a line that asks for a
/// credential is no person's ([`super::credential_need`]); it is in [`credential_needs`].
#[must_use]
pub fn persons_needs(tail: &str) -> Vec<String> {
    let mut needs = needs_you(tail);
    needs.retain(|need| !super::credential_need::is_credential(need));
    needs
}

/// The `NEEDS YOU:` lines of `tail` that ask for a credential, which aterm resolves on the
/// secrets path instead of showing them to a person ([`super::credential_need::wants_of`]).
#[must_use]
pub fn credential_needs(tail: &str) -> Vec<String> {
    let mut needs = needs_you(tail);
    needs.retain(|need| super::credential_need::is_credential(need));
    needs
}

/// Whether `tail` says no need stands.
#[must_use]
pub fn says_no_needs(tail: &str) -> bool {
    let read = needs_lines(tail);
    read.cleared && read.needs.iter().all(|n| sanitized(n).is_empty())
}

/// WHERE A TURN'S EARLIER MESSAGES START in the texts its readers hold
/// (`TurnEndReading::turn_said` and `TurnEndReading::turn_lines`, each its messages joined by
/// [`crate::text::MESSAGE_GAP`]): the byte each message after the first opens at, in
/// each. Empty where the reader cannot tell the messages apart (a program whose reader answers
/// one text, a test's words): the text is then read as one message ([`Before::whole`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Breaks {
    /// In the messages as drawn (`turn_said`).
    pub said: Vec<usize>,
    /// In the messages each line the program wrapped read as one (`turn_lines`).
    pub lines: Vec<usize>,
}

/// `messages` joined as the readers hold them ([`crate::text::join`]), and the
/// byte each one after the first opens at.
#[must_use]
pub fn joined(messages: Option<Vec<String>>) -> (Option<String>, Vec<usize>) {
    let Some(messages) = messages else {
        return (None, Vec::new());
    };
    let gap = crate::text::MESSAGE_GAP.len();
    let mut starts = Vec::with_capacity(messages.len().saturating_sub(1));
    let mut at = 0;
    for m in messages.iter().take(messages.len().saturating_sub(1)) {
        at += m.len() + gap;
        starts.push(at);
    }
    (Some(crate::text::join(&messages)), starts)
}

/// A TURN'S EARLIER MESSAGES as the readers of its needs read them: their joined text, and where
/// each message after the first opens ([`Breaks`]), counted in the text as read before its front
/// lost `dropped` bytes ([`Self::past`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Before<'a> {
    text: &'a str,
    starts: &'a [usize],
    dropped: usize,
}

impl<'a> Before<'a> {
    /// `text`, its messages opening at `starts` ([`Breaks`]).
    #[must_use]
    pub const fn new(text: &'a str, starts: &'a [usize]) -> Self {
        Self {
            text,
            starts,
            dropped: 0,
        }
    }

    /// `text` read as one message.
    #[must_use]
    pub const fn whole(text: &'a str) -> Self {
        Self::new(text, &[])
    }

    /// The joined text.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }

    /// The messages from where `rest`, the end of [`Self::text`] (the act's own rows cut from the
    /// turn's opening), begins, still told apart. A `rest` that is not its end is one message.
    #[must_use]
    pub fn past(self, rest: &'a str) -> Self {
        if !self.text.ends_with(rest) {
            return Self::whole(rest);
        }
        Self {
            text: rest,
            starts: self.starts,
            dropped: self.dropped + (self.text.len() - rest.len()),
        }
    }

    /// Each message's text, oldest first, the gap that joins them left out. A start that does not
    /// fall inside the text past the last, right after a gap, is no start.
    pub fn messages(self) -> Vec<&'a str> {
        let gap = crate::text::MESSAGE_GAP;
        let mut out = Vec::with_capacity(self.starts.len() + 1);
        let mut at = 0;
        for &start in self.starts {
            let Some(s) = start.checked_sub(self.dropped) else {
                continue;
            };
            if s <= at
                || s >= self.text.len()
                || !self.text.is_char_boundary(s)
                || !self.text[at..s].ends_with(gap)
            {
                continue;
            }
            out.push(&self.text[at..s - gap.len()]);
            at = s;
        }
        out.push(&self.text[at..]);
        out
    }
}

impl<'a> From<&'a str> for Before<'a> {
    fn from(text: &'a str) -> Self {
        Self::whole(text)
    }
}

/// THE AGENT'S WORD ON NEEDS in a turn (v6 §14.17.13, the post-landing reviews of 2026-11-01 and
/// 2026-11-02): what the turn's earlier messages `before` named still stands beside its last
/// message `last`, unless the agent's own word retracts it. A need stands until retracted, by:
///
/// - THE LAST WORD: the last message's own word that says none stands ([`retracts`]: `NEEDS YOU:
///   nothing`, no need beside it) retracts every need the turn named before it, and every need an
///   earlier point raised ([`retracts`]' other reader, `TurnEndState::fold_turn_needs`): `before`
///   is read for none (`None`).
/// - A NONE IN AN EARLIER MESSAGE: a paragraph of an earlier message that says none stands alone
///   in the agent's own words retracts the needs named before it in that same message, and only
///   those (`retracted_at`); `before` is read from that paragraph on in that message. A need an
///   earlier message, or an earlier point, named stands, so a relayed worker's closing line in a
///   message of its own takes no need down. Where the reader cannot tell the messages apart
///   ([`Breaks`] empty), `before` is one message.
///
/// Quoted or fenced text is never the agent's word: a quoted none retracts nothing, and a fenced
/// line is no need line at all. One paragraph that names a need and a none both is a kept need
/// ([`needs_you`]), and so is the last message, read whole. The one rule by which every reader of a
/// turn's needs reads it: the land check's answer (`land_check::land_check_answer`), the fold
/// that raises the needs on the menu bar (`TurnEndState::fold_turn_needs`, with
/// [`retracted_in`]), the readers of `TurnEndReading::needs_before`, and the go-ahead reader
/// ([`retracted_cut`]).
#[must_use]
pub fn standing_before<'a>(before: Option<Before<'a>>, last: &str) -> Option<Cow<'a, str>> {
    let before = before?;
    (!retracts(last)).then(|| cut_messages(before, |m, at| Cow::Borrowed(&m[at..])))
}

/// `before` WITHOUT THE NEEDS THE TURN'S WORD RETRACTED ([`standing_before`]), its prose kept: the
/// go-ahead reader's view of a turn's earlier messages (`go_ahead::parked`), whose sentences
/// still say what the work plans and lacks, though a need they named was taken back.
#[must_use]
pub fn retracted_cut<'a>(before: Before<'a>, last: &str) -> Cow<'a, str> {
    if retracts(last) {
        return without_needs(before.text);
    }
    cut_messages(before, |m, at| {
        Cow::Owned(format!(
            "{}{}{}",
            without_needs(&m[..at]).trim_end(),
            crate::text::MESSAGE_GAP,
            &m[at..]
        ))
    })
}

/// THE NEEDS AN EARLIER MESSAGE'S OWN NONE RETRACTED ([`standing_before`]): each named before
/// the paragraph that retracted it in its message, as [`needs_you`] names it. The fold takes
/// them off the needs an earlier point raised (`TurnEndState::fold_turn_needs`).
#[must_use]
pub fn retracted_in(before: Before<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in before.messages() {
        if let Some(at) = retracted_at(m) {
            for need in needs_you(&m[..at]) {
                if !out.contains(&need) {
                    out.push(need);
                }
            }
        }
    }
    out
}

/// `before`, each message that retracts needs ([`retracted_at`]) as `cut` makes it.
fn cut_messages<'a>(
    before: Before<'a>,
    cut: impl Fn(&'a str, usize) -> Cow<'a, str>,
) -> Cow<'a, str> {
    let messages = before.messages();
    let cuts: Vec<Option<usize>> = messages.iter().map(|m| retracted_at(m)).collect();
    if cuts.iter().all(Option::is_none) {
        return Cow::Borrowed(before.text);
    }
    let kept: Vec<Cow<'a, str>> = messages
        .into_iter()
        .zip(cuts)
        .map(|(m, at)| at.map_or(Cow::Borrowed(m), |at| cut(m, at)))
        .collect();
    Cow::Owned(kept.join(crate::text::MESSAGE_GAP))
}

/// WHERE `message`'S OWN NONE RETRACTS THE NEEDS IT NAMED BEFORE IT ([`standing_before`]): the
/// byte its last paragraph that says no need stands ([`says_no_needs`]) in the agent's own words
/// ([`own_line`]) opens at, where a need line comes before it; `None` where none does. A
/// paragraph ends at a blank line outside a fence.
fn retracted_at(message: &str) -> Option<usize> {
    let mut fence = None;
    let mut at = 0;
    let mut paragraph: Option<(usize, String)> = None;
    let mut last = None;
    let close = |p: Option<(usize, String)>, last: &mut Option<usize>| {
        if let Some((start, own)) = p
            && says_no_needs(&own)
        {
            *last = Some(start);
        }
    };
    for line in message.split_inclusive('\n') {
        let fenced = fenced_line(line.trim_end_matches(['\n', '\r']), &mut fence);
        if !fenced && line.trim().is_empty() {
            close(paragraph.take(), &mut last);
        } else {
            let (_, own) = paragraph.get_or_insert_with(|| (at, String::new()));
            if !fenced && own_line(line) {
                own.push_str(line);
            }
        }
        at += line.len();
    }
    close(paragraph.take(), &mut last);
    last.filter(|&at| !needs_you(&message[..at]).is_empty())
}

/// Whether `line` is the agent's own word, not quoted ([`QUOTE_MARKS`]).
fn own_line(line: &str) -> bool {
    !line.trim_start().starts_with(QUOTE_MARKS)
}

/// WHETHER THE TURN'S LAST MESSAGE `last` IS THE AGENT'S WORD THAT NO NEED STANDS
/// ([`standing_before`]): it says no need stands ([`says_no_needs`]) in the agent's own words, its
/// quoted lines left out (`own_line`), so a quoted `NEEDS YOU: nothing` retracts nothing. It
/// retracts every need the turn named and every need an earlier point raised.
#[must_use]
pub fn retracts(last: &str) -> bool {
    let own: String = last.split_inclusive('\n').filter(|l| own_line(l)).collect();
    says_no_needs(&own)
}

/// `tail` without its NEEDS YOU lines (heads, the items under a head, their wrapped rows).
#[must_use]
pub fn without_needs(tail: &str) -> Cow<'_, str> {
    cut_needs(tail, &needs_lines(tail))
}

/// THE GO-AHEAD READER'S OWN VIEW of the report `tail` (`go_ahead::parked`): its prose without
/// its NEEDS YOU lines, each need's words, and the rows a need took only here. A row the screen
/// wrapped after a need that stops mid-clause (`… say "rerun the checks" so` above `I can build
/// …`), or that opens lowercase past a quote mark, is the need's here (`go_ahead_wrap`), so its
/// reading does not hang on the tab's width. No other reader reads this view: the irreversible
/// guard and every reader of open work read [`without_needs`], as before the go-ahead reader.
pub struct GoAheadView<'a> {
    /// `tail` without its needs, the mid-clause rows cut with them.
    pub prose: Cow<'a, str>,
    /// Each need's words as written, wrapped rows joined, before [`needs_you`] cleans and cuts
    /// it: the reader judges a need on all of its words.
    pub needs: Vec<String>,
    /// Each run of rows a need took only in this view: every other reader reads them as the
    /// reply's own prose.
    pub moved: Vec<Moved>,
}

/// A run of rows a need took only in the go-ahead reader's view ([`GoAheadView::moved`]).
pub struct Moved {
    /// The need's words, the run joined.
    pub need: String,
    /// The need's words before the run: the need as every other reader joins it (`wrap_join`),
    /// and so the line [`needs_you`] names for it, once cleaned.
    pub base: String,
    /// The rows of `tail`, by index.
    pub rows: Vec<usize>,
}

/// Where the words of `row` start past its quote marks and indent: the row as a need's wrap
/// joins it is `row[quote_lead(row)..]`, its end trimmed.
pub fn quote_lead(row: &str) -> usize {
    row.len() - unquoted(row).len()
}

/// [`GoAheadView`] of `tail`.
pub fn go_ahead_view(tail: &str) -> GoAheadView<'_> {
    let read = needs_lines_by(tail, &NONE_WORDS, go_ahead_wrap);
    GoAheadView {
        prose: cut_needs(tail, &read),
        moved: read.moved,
        needs: read.needs,
    }
}

/// `tail` without the need lines `read` names.
fn cut_needs<'a>(tail: &'a str, read: &NeedLines) -> Cow<'a, str> {
    if read.lines.is_empty() && read.cuts.is_empty() {
        return Cow::Borrowed(tail);
    }
    let clause_at = |i: usize| {
        read.clauses
            .iter()
            .find(|(at, _)| *at == i)
            .map(|(_, clause)| capitalized(clause))
    };
    let kept: Vec<String> = tail
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            if let Some(&(_, cut)) = read.cuts.iter().find(|(at, _)| *at == i) {
                let before = l[..cut].trim_end();
                return Some(match clause_at(i) {
                    Some(clause) => format!("{before} {clause}"),
                    None => before.to_string(),
                });
            }
            if !read.lines.contains(&i) {
                return Some(l.to_string());
            }
            clause_at(i)
        })
        .collect();
    Cow::Owned(kept.join("\n"))
}

fn capitalized(text: &str) -> String {
    let mut cs = text.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().chain(cs).collect(),
        None => String::new(),
    }
}

const DEFENSE_EXCEPTIONS: &[&str] = &[
    "except for",
    "except",
    "excepting",
    "excluding",
    "besides",
    "but for",
    "but",
    "other than",
    "apart from",
    "aside from",
    "save for",
    "save",
    "barring",
    "bar",
    "beyond",
    "short of",
    "outside of",
    "minus",
];

fn excepted(rest: &str) -> Option<&str> {
    let rest = rest.trim_start_matches(['*', '_', ',']).trim_start();
    DEFENSE_EXCEPTIONS.iter().find_map(|w| {
        let after = rest
            .get(..w.len())
            .filter(|p| p.eq_ignore_ascii_case(w))
            .map(|_| &rest[w.len()..])?;
        if !after.starts_with(|c: char| c.is_whitespace() || c == ':') {
            return None;
        }
        let left = after
            .trim_start_matches(|c: char| c.is_whitespace() || c == ':')
            .trim_end();
        (!left.is_empty()).then_some(left)
    })
}

fn defense_rest(line: &str) -> Option<Cow<'_, str>> {
    let except = |left: &str| Cow::Owned(format!("except {left}"));
    if let Some(rest) = head_rest(line, NOTHING_LEFT) {
        return Some(excepted(rest).map_or(Cow::Borrowed(rest), except));
    }
    let t = line.trim_start();
    if t.starts_with(QUOTE_MARKS) {
        return None;
    }
    let t = past_heading(t).0;
    let t = past_marker(t).unwrap_or(t);
    let t = t.trim_start_matches(['*', '_']);
    let name = NOTHING_LEFT.trim_end_matches(':');
    if !t.get(..name.len())?.eq_ignore_ascii_case(name) {
        return None;
    }
    let mut rest = &t[name.len()..];
    for words in head_words(NOTHING_LEFT) {
        if let Some(r) = rest
            .get(..words.len())
            .filter(|w| w.eq_ignore_ascii_case(words))
            .map(|_| &rest[words.len()..])
        {
            rest = r;
            break;
        }
    }
    let rest = rest.trim_start_matches(['*', '_']);
    if !rest.starts_with(|c: char| c.is_whitespace() || c == ',') {
        return None;
    }
    excepted(rest).map(except)
}

/// The worker's defense line, trimmed, and the lines of `tail` after it: the one scan
/// [`defense_line`] and [`defense_and_after`] share.
fn defense_at(tail: &str) -> Option<(&str, std::str::Lines<'_>)> {
    let mut block = None;
    let mut lines = tail.lines();
    while let Some(line) = lines.next() {
        if fenced_line(line, &mut block) {
            continue;
        }
        if defense_rest(line).is_some() {
            return Some((line.trim(), lines));
        }
    }
    None
}

/// The worker's defense line.
#[must_use]
pub fn defense_line(tail: &str) -> Option<&str> {
    defense_at(tail).map(|(line, _)| line)
}

/// THE WORKER'S OWN WORD THAT ITS WORK ENDS UNFINISHED: what it wrote after a `Could not finish:`
/// head that starts the reply (`tail`'s first non-blank line), trimmed. The same words further down,
/// or as an item of a list, are a report on the way to more work: only a reply that starts with the
/// head ends anything. `None` for a head with no reason after it, for the placeholder a task's own
/// wording spells (`<why>`), and for a reply that also defends the work with `Nothing left:`, since
/// the two contradict each other and Land's check is what settles that.
#[must_use]
pub fn could_not_finish(tail: &str) -> Option<&str> {
    if defense_line(tail).is_some() {
        return None;
    }
    tail.lines()
        .find(|line| !line.trim().is_empty())
        .filter(|line| past_marker(line.trim_start()).is_none())
        .and_then(|line| head_rest(line, COULD_NOT_FINISH))
        .map(str::trim)
        .filter(|why| !why.is_empty() && !(why.starts_with('<') && why.ends_with('>')))
}

/// The worker's defense line ([`defense_line`]) and every non-blank line after it, each trimmed;
/// `None` where `tail` holds no defense (a landing's receipt, `harness::land_receipt`).
#[must_use]
pub fn defense_and_after(tail: &str) -> Option<Vec<&str>> {
    let (line, after) = defense_at(tail)?;
    let after = after.map(str::trim).filter(|l| !l.is_empty());
    Some(std::iter::once(line).chain(after).collect())
}

/// `tail` as Land's strict reading of open work takes it: its needs left out, and the keep lines
/// under its defense ([`kept_tokens`]), so a row kept running (`keep <token>: the server you asked
/// for, still running`) never reads as open work.
#[must_use]
pub fn defense_body(tail: &str) -> String {
    let rest = without_needs(tail);
    let mut block = None;
    let mut defended = false;
    let mut out: Vec<String> = Vec::new();
    for line in rest.lines() {
        if fenced_line(line, &mut block) {
            out.push(line.to_string());
            continue;
        }
        if defended && keep_line(line).is_some() {
            continue;
        }
        match defense_rest(line) {
            Some(body) => {
                defended = true;
                out.push(capitalized(&body));
            }
            None => out.push(line.to_string()),
        }
    }
    out.join("\n")
}

/// THE TOKENS A DEFENSE KEEPS RUNNING: each keep line (`keep_line`) under `tail`'s `Nothing
/// left:` defense, outside a code block, in order and each once, read where [`defense_body`]
/// reads them: in `tail` with its needs left out ([`without_needs`]), so a keep line inside a
/// NEEDS YOU block keeps nothing, as the body leaves the whole block out. None where `tail`
/// defends nothing ([`defense_line`]).
#[must_use]
pub fn kept_tokens(tail: &str) -> Vec<ProcessToken> {
    if defense_line(tail).is_none() {
        return Vec::new();
    }
    let rest = without_needs(tail);
    let Some((_, after)) = defense_at(&rest) else {
        return Vec::new();
    };
    let mut block = None;
    let mut kept = Vec::new();
    for line in after {
        if fenced_line(line, &mut block) {
            continue;
        }
        if let Some(token) = keep_line(line)
            && !kept.contains(&token)
        {
            kept.push(token);
        }
    }
    kept
}

/// A KEEP LINE'S TOKEN: past a bullet (`- `, `* `, `1. `, …), the word `keep` in any case (a
/// colon after it read past), then a process token as `sys left` prints it as `end=`
/// (`<pid>@<birth>:<idversion>`, or `<pid>@<birth>` where the kernel keeps no pid version:
/// [`ProcessToken::parse`]), bare or in backticks, copied with its `end=` or not.
/// What follows the token is the line's own words, a bare token's closing mark (`,`, `;`, `:`,
/// `.`, `)`) read past. `None` for any other line: a word that is no token keeps nothing.
fn keep_line(line: &str) -> Option<ProcessToken> {
    let t = line.trim_start();
    let t = past_marker(t).unwrap_or(t);
    let (word, rest) = t.split_once(char::is_whitespace)?;
    if !word
        .strip_suffix(':')
        .unwrap_or(word)
        .eq_ignore_ascii_case("keep")
    {
        return None;
    }
    let rest = rest.trim_start();
    let token = match rest.strip_prefix('`') {
        Some(quoted) => quoted.split_once('`')?.0,
        None => rest
            .split(char::is_whitespace)
            .next()?
            .trim_end_matches([',', ';', ':', '.', ')']),
    };
    ProcessToken::parse(token.strip_prefix("end=").unwrap_or(token))
}

/// The key of a set of needs: equal sets, equal keys; `0` for none.
#[must_use]
pub fn needs_key(needs: &[String]) -> u64 {
    if needs.is_empty() {
        return 0;
    }
    crate::hash::fnv1a_64(needs.join("\n").as_bytes()).max(1)
}

/// What a row of the needs table (`fixtures/needs-table.txt`) says its report must read as.
#[cfg(any(test, feature = "testing"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeedsRow {
    /// No need stands: no need named, and a clean defense lands.
    None,
    /// A need, carrying these words: named, and nothing lands.
    Need(String),
    /// An ask with no head, these words in it: no landing, and the nudge names what it read open.
    Headless(String),
}

/// The rows of the needs table.
#[cfg(any(test, feature = "testing"))]
pub fn needs_table() -> Vec<(NeedsRow, String)> {
    include_str!("fixtures/needs-table.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (label, report) = line
                .split_once(" || ")
                .unwrap_or_else(|| panic!("a row with no ` || `: {line:?}"));
            let row = match label.split_once(' ') {
                None if label == "NONE" => NeedsRow::None,
                Some(("NEED", words)) => NeedsRow::Need(words.to_string()),
                Some(("HEADLESS", words)) => NeedsRow::Headless(words.to_string()),
                _ => panic!("a row with no label: {line:?}"),
            };
            (row, report.replace("\\n", "\n"))
        })
        .collect()
}

#[cfg(test)]
#[path = "needs_tests.rs"]
mod tests;
