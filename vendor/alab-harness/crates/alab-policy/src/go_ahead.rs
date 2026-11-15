// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! THE GO-AHEAD READER (owner, 2026-10-29: "how dare you block on me for go ahead!"): whether a
//! reply parks the agent's OWN REVERSIBLE work on a person's go-ahead, and what Autopilot Cruise
//! and Land type to one ([`RULE_GO_AHEAD`]). Anything outward, irreversible, someone else's or
//! only a person's keeps the policy's other acts. It is lexical, like every reader beside it
//! (ATERM-KI-1020-2): `fixtures/go-ahead-table.txt` is the behaviour it holds to.

use super::needs;
use super::words::{DESTRUCTIVE_WORDS, fit_units, force_push};

/// The answer's rule: the owner's standing go-ahead for the agent's own reversible work.
pub const RULE_GO_AHEAD: &str = "go-ahead@v1";

/// [`NEVER_A_NEED`] as a literal, for the texts `concat!` builds on it (Cruise's continuation,
/// the carry at a break).
#[macro_export]
macro_rules! never_a_need {
    () => {
        "Your own reversible work needs no one's go-ahead and is never a NEEDS YOU line; a hold a \
         person gave you still stands."
    };
}

/// THE OWNER'S STANDING GO-AHEAD: what [`RULE_GO_AHEAD`] types, its limits before the owner's
/// words, after the harness's mark: a literal, or a macro call that expands to one, as `concat!`
/// takes it (aterm passes its `harness_mark!()`). It limits itself, so a park the reader misreads
/// is answered safely too: it covers the agent's own reversible work only, says a hold a person
/// gave still stands (the agent sees it in its own context, and the harness reads no person's
/// words for one), and grants nothing outward, irreversible, someone else's or only a person's.
/// Like every text the harness types about the go-ahead, it holds no stop phrase (aterm's
/// `turn_end::STOP_PHRASES`), so an agent that quotes it asks nothing.
#[macro_export]
macro_rules! go_ahead_answer {
    ($($mark:tt)+) => {
        concat!(
            $($mark)+,
            " The owner's standing go-ahead: proceed now with your own reversible work (build, \
             test, commit, push or merge your own branch to its main). A person's hold still \
             stands. It grants nothing outward or irreversible (delete, reset, force-push, \
             release, deploy, tag, publish, send, spend), nothing another made, and no person's \
             own act. That work is never a NEEDS YOU line. The owner ruled it: \"how dare you \
             block on me for go ahead!\""
        )
    };
}

/// The sentence Cruise's continuation and its carry say outside the hour the person's last key
/// holds the go-ahead (aterm's `turn_end::TurnEndState::go_ahead_held`); within it the sentence
/// is cut ([`unless_held`]), as the go-ahead itself is not typed there. It limits itself as
/// [`go_ahead_answer!`] does: a hold a person gave stands, and the harness reads no words for one.
pub const NEVER_A_NEED: &str = never_a_need!();

/// `text` without [`NEVER_A_NEED`] where the person's last key `held` the go-ahead
/// (aterm's `turn_end::TurnEndState::go_ahead_held`): they may just have said to hold.
#[must_use]
pub fn unless_held(text: &str, held: bool) -> std::borrow::Cow<'_, str> {
    if !held || !text.contains(NEVER_A_NEED) {
        return std::borrow::Cow::Borrowed(text);
    }
    let spaced = format!(" {NEVER_A_NEED}");
    let cut = if text.contains(&spaced) {
        spaced.as_str()
    } else {
        NEVER_A_NEED
    };
    std::borrow::Cow::Owned(text.replacen(cut, "", 1))
}

/// The answer's sentence for a park that names memory or disk room ([`Park::waits_room`]): the
/// answer is the ask Claude Code's low-memory stop note waits for, and the work starts once there
/// is room. It names waits `aterm ctl @self await sys` takes (a bare one is refused): memory's,
/// and disk's predicate. It says no stop a person would say (`Wait …`).
pub const GO_AHEAD_ROOM: &str =
    "Start once `aterm ctl @self await sys mem=normal` (or `'free>=room'` for disk) returns.";

/// The answer's sentence for a park on a `NEEDS YOU:` line ([`Park::need`]): the line goes now,
/// not once the work runs, so a line kept after it is the agent's own.
pub const GO_AHEAD_DROP: &str = "Drop that NEEDS YOU line now.";

/// The answer's sentence when the reply's other needs are a person's ([`Park::others`]).
pub const GO_AHEAD_OTHERS: &str = "Act on none of your other NEEDS YOU lines.";

/// The most UTF-16 code units of the park's words the answer quotes.
pub const QUOTE_UNITS: usize = 150;

/// The most UTF-16 code units of the word the agent asked the person to say.
pub const TOKEN_UNITS: usize = 80;

/// The frame of [`Park::said`] around the word the agent asked the person to say: the owner's
/// ruling answers the ask for the agent's own reversible work only, and never says the word was
/// said, which stays a person's.
pub const TOKEN_OPEN: &str = "You asked for \"";

/// The close of [`TOKEN_OPEN`]'s frame.
pub const TOKEN_CLOSE: &str = "\": the owner's ruling covers your own reversible work.";

/// The frame of [`Park::said`] around the park's own words, where it named no word to say.
pub const QUOTE_OPEN: &str = "You parked on: \"";

/// The close of [`QUOTE_OPEN`]'s frame.
pub const QUOTE_CLOSE: &str = "\".";

/// A reply's park on a person's go-ahead for its own reversible work ([`parked`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Park {
    /// The sentence or need that parks, as written (hidden characters spaced, cut to [`QUOTE_UNITS`]).
    pub quote: String,
    /// The word the agent asked the person to say (`say "rerun the checks"`), as written; none
    /// where another need or sentence of the reply asks the same word for an act the go-ahead
    /// does not cover ([`parked`]).
    pub token: Option<String>,
    /// The park names low memory, disk or room: the answer says to wait for room first.
    pub waits_room: bool,
    /// The park is a `NEEDS YOU:` line.
    pub need: bool,
    /// Another `NEEDS YOU:` line of the reply asks what only a person can give.
    pub others: bool,
    /// THE LINES THE GO-AHEAD ANSWERS, as the menu bar names them (§15.6): each `NEEDS YOU:` line
    /// of the turn this reader parks on, cleaned as [`needs::needs_you`] cleans it, and, where the
    /// need took rows only this reader's view joins, also as every other reader reads it
    /// ([`needs::Moved::base`]). A line that parks nothing (only a person's, outward,
    /// irreversible, someone else's, a hold, a decline, a choice, or any of these beside the
    /// agent's own work in one line) is never one, nor is a line it cleans to that a parking need
    /// cleans to as well (two needs `needs_you` reads as one: their words before a row only this
    /// reader joins, or past [`needs::NEED_CHARS`]); a park in prose answers none.
    pub needs: Vec<String>,
    /// The park is work to land by the agent's own words, wherever the worktree is (Land's
    /// go-ahead rides it, aterm's `turn_end::TurnEndState::go_ahead_owed`): it names a landing
    /// act (`LANDING`: `so I can merge … and push it`, `landing waits on that check`), or it or
    /// a sentence of the turn's prose names a branch, a worktree or main.
    pub lands: bool,
}

impl Park {
    /// The answer's sentence naming the park, in at most `room` units: the token when there is
    /// one, said only for the agent's own reversible work, else the quote; empty when even its
    /// frame does not fit.
    #[must_use]
    pub fn said(&self, room: usize) -> String {
        let (open, close, words, most) = match &self.token {
            Some(t) => (TOKEN_OPEN, TOKEN_CLOSE, t, TOKEN_UNITS),
            None => (QUOTE_OPEN, QUOTE_CLOSE, &self.quote, QUOTE_UNITS),
        };
        let frame = open.encode_utf16().count() + close.encode_utf16().count();
        let fit = fit_units(words, most.min(room.saturating_sub(frame)));
        if fit.is_empty() {
            return String::new();
        }
        format!("{open}{fit}{close}")
    }
}

/// Whether `words` park the agent's own reversible work on a person's go-ahead, read as
/// [`parked`] reads a reply. A line of a reply is short and asked about once, so it is read and
/// kept out of `REMEMBERED`, where it could push out the reply being looked at.
#[must_use]
pub fn parks(words: &str) -> bool {
    read_park("", words).is_some()
}

/// The park on a person's go-ahead that `turn_said` (the turn's earlier messages) and `tail` (its
/// last) make, if any: a consent ask with a pending marker, own reversible work from the allow
/// list named in the same sentence or need, and no exclusion (only a person's, outward or
/// irreversible, someone else's, a hold the person gave, a decline, a choice) in it, the code it
/// carries or the sentence before, nor only a person's in the sentence after (for a need, in a
/// sentence of the prose that says its work lacks it: `blocked`). An outward or destructive
/// act the turn's prose names anywhere and ties to the work the go-ahead would cover
/// (`planned`: `the next push runs the publish`, `Plan: delete the tags, then push`) is one the
/// go-ahead would be read as covering: such a turn parks nothing. The park's word to say
/// ([`Park::token`]) is dropped where a need or sentence that parks nothing asks the same word
/// (`NEEDS YOU: say go so I can delete the release tags`).
#[must_use]
pub fn parked(turn_said: &str, tail: &str) -> Option<Park> {
    if turn_said.len() + tail.len() > REMEMBERED_REPLY_BYTES || dropped_parts() {
        return read_park(turn_said, tail);
    }
    let known = REMEMBERED.with_borrow(|known| {
        known
            .iter()
            .find(|(said, last, _)| said == turn_said && last == tail)
            .map(|(_, _, park)| park.clone())
    });
    known.unwrap_or_else(|| {
        let park = read_park(turn_said, tail);
        REMEMBERED.with_borrow_mut(|known| {
            if known.len() == REMEMBERED_REPLIES {
                known.pop_front();
            }
            known.push_back((turn_said.to_owned(), tail.to_owned(), park.clone()));
        });
        park
    })
}

/// How many replies each thread keeps its [`parked`] reading of. One read of a screen asks for the
/// same reply's reading several times (the state's observation, the decision, the hour's end), and
/// the reading is a function of the words alone.
const REMEMBERED_REPLIES: usize = 4;

/// The longest reply [`REMEMBERED`] holds, so that a thread's memory stays small.
const REMEMBERED_REPLY_BYTES: usize = 256 * 1024;

/// A park this thread's [`parked`] read, by the words it read it from.
type RememberedPark = (String, String, Option<Park>);

thread_local! {
    /// This thread's last [`REMEMBERED_REPLIES`] readings, oldest first.
    static REMEMBERED: std::cell::RefCell<std::collections::VecDeque<RememberedPark>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

/// [`parked`]'s reading, from the words.
fn read_park(turn_said: &str, tail: &str) -> Option<Park> {
    let text = if turn_said.trim().is_empty() {
        tail.to_string()
    } else {
        format!("{turn_said}\n\n{tail}")
    };
    // The reader's own view of the reply: a need wrapped mid-clause takes its rows here only
    // ([`needs::go_ahead_view`]); the irreversible guard reads the reply as the base did.
    let view = needs::go_ahead_view(&text);
    let units: Vec<Unit> = sentences(&view.prose)
        .iter()
        .map(|(s, code, prior_clause)| Unit::clause(s, false, code, *prior_clause))
        .collect();
    let mut park: Option<(Option<usize>, Unit, Option<String>)> = None;
    let mut others = false;
    let mut room = false;
    // The lines the go-ahead answers ([`Park::needs`]): each parking need as `needs_you` cleans
    // it, and as every other reader reads it where it took rows only this view joins.
    let mut answers: Vec<String> = Vec::new();
    // The same lines of each need that parks nothing: the menu bar matches by cleaned words, so a
    // line one of them cleans to is a person's, whichever need also cleans to it.
    let mut theirs: Vec<String> = Vec::new();
    // The words to say that a need or sentence which parks nothing asks for (`NEEDS YOU: say go
    // so I can delete the release tags`).
    let mut withheld: Vec<String> = Vec::new();
    let mut withhold = |unit: &Unit| {
        if let Some((_, Some(token))) = person_says(unit) {
            withheld.push(token_mark(&token));
        }
    };
    for need in &view.needs {
        let (words, code) = uncoded(need);
        let code: Vec<&str> = code.iter().map(|(_, c)| c.as_str()).collect();
        let unit = Unit::coded(&words, true, &code.join(" "));
        let base = view.moved.iter().find(|m| m.need == *need).map(|m| &m.base);
        let lines = std::iter::once(need)
            .chain(base)
            .map(|l| needs::sanitized(l))
            .filter(|l| !l.is_empty());
        match judged(&unit, None, None).filter(|_| !blocked(&unit, &units)) {
            Some(token) => {
                room |= names_room(&unit);
                for line in lines {
                    if !answers.contains(&line) {
                        answers.push(line);
                    }
                }
                if park.is_none() {
                    park = Some((None, unit, token));
                }
            }
            None => {
                others |= !needs::sanitized(need).is_empty();
                theirs.extend(lines);
                withhold(&unit);
            }
        }
    }
    answers.retain(|line| !theirs.contains(line));
    for (i, unit) in units.iter().enumerate() {
        let before = i.checked_sub(1).map(|b| &units[b]);
        let after = units.get(i + 1);
        // A clause a `;` joins reads its sentence's context as the sentence would: the clauses
        // before it back to the sentence's start, and the sentence before that, but for the
        // agent's own report of finished work, whose nouns are its code (`Rewrote the token
        // parser`, `Fixed the OAuth callback`).
        let start = (1..=i).rev().find(|&k| !units[k].prior_clause).unwrap_or(0);
        let outer = &units[start.saturating_sub(1)..i.saturating_sub(1)];
        if let Some(token) = judged(unit, before, after)
            .filter(|_| !outer.iter().any(|o| !reports_done(o) && excluded(o, None)))
        {
            room |= [before, Some(unit), after]
                .into_iter()
                .flatten()
                .any(names_room);
            match &mut park {
                None => park = Some((Some(i), unit.clone(), token)),
                Some((_, _, held)) if held.is_none() => *held = token,
                Some(_) => {}
            }
        } else {
            withhold(unit);
        }
    }
    let (at, unit, token) = park?;
    if kept(Part::Outward) && units.iter().any(planned) {
        return None;
    }
    // A word another need or sentence asks for an act the go-ahead does not cover is never said:
    // the answer quotes the park instead.
    let token = token
        .map(|t| needs::sanitized(&t))
        .filter(|t| !t.is_empty() && !withheld.contains(&token_mark(t)));
    let names_where = |u: &Unit| u.any(&["branch", "branches", "worktree", "worktrees", "main"]);
    let names_landing = |u: &Unit| {
        u.words
            .iter()
            .any(|(_, w)| w.split('-').any(|part| inflects_any(part, LANDING)))
    };
    let prior = at
        .filter(|_| unit.prior_clause)
        .and_then(|i| i.checked_sub(1))
        .map(|b| &units[b]);
    let lands = names_where(&unit)
        || names_landing(&unit)
        || prior.is_some_and(names_landing)
        || units.iter().any(names_where);
    Some(Park {
        quote: fit_units(&needs::sanitized(&unit.said), QUOTE_UNITS).into_owned(),
        token,
        waits_room: room,
        need: unit.need,
        others,
        needs: answers,
        lands,
    })
}

/// Whether `unit` opens as the agent's report of work it finished: a past-tense verb first, after
/// an `I`, `I've` or `we` (`Rewrote the token parser`, `I fixed the login form`).
fn reports_done(unit: &Unit) -> bool {
    const IRREGULAR: &[&str] = &[
        "rewrote", "wrote", "made", "built", "rebuilt", "ran", "reran", "split", "cut", "put",
        "set", "kept", "found", "took", "brought",
    ];
    let first = usize::from(matches!(
        unit.word(0),
        "i" | "i've" | "we" | "we've" | "just"
    ));
    let w = unit.word(first);
    (w.len() > 4 && w.ends_with("ed")) || IRREGULAR.contains(&w)
}

/// The acts that land the agent's work, in any inflection: a park that names one parks unlanded
/// work by its own words ([`Park::lands`]).
const LANDING: &[&str] = &["push", "merge", "commit", "land", "rebase"];

/// A word to say as [`parked`] compares it with a word withheld: hidden characters spaced,
/// lowercased, its digits folded.
fn token_mark(token: &str) -> String {
    needs::sanitized(token)
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { lowered(c) })
        .collect()
}

/// Whether a sentence of the turn's prose names an outward or destructive act ([`outward`]) as
/// part of the work the go-ahead would cover: beside a push, merge, landing or rebase (a
/// `force-push` included), as a plan (`next`, `then`, `plan`, `will`, `I'll`, `also`), asked
/// (`Should I force-push the old branch?`), tied to the person's word by an ask of its own (`the
/// cleanup job deletes the release tags once you say go`), a publish of any tense (`Tagged v1.4.0
/// for the crates.io release`, `CI uploads every green build of main to TestFlight`, `the release
/// was sent`), or an outward act in the present tense of a third person (`CI releases every green
/// build`, `the hook sends`): a publish or a job already set up runs on the push the go-ahead
/// would cover. A report of what a test checks (`it fails if that fallback is removed`) or of
/// another's files (`deleting it is up to the person`) plans nothing, nor does a word only inside
/// an identifier or a path ([`names_code`]: `for_launch`, `ship/release.rs`), which Claude Code
/// draws bare where the reply wrote it as inline code; the exclusions still read those.
fn planned(unit: &Unit) -> bool {
    let unit = &unit.without_code_names();
    const PUBLISHES: &[&str] = &[
        "tag",
        "publish",
        "deploy",
        "ship",
        "upload",
        "promote",
        "notarize",
        "yank",
        "unpublish",
        "release",
        "send",
        "notify",
        "email",
        "launch",
    ];
    outward(unit, None)
        && (unit.words.iter().any(|(_, w)| {
            w.split('-')
                .any(|part| inflects_any(part, &["push", "merge", "land", "rebase"]))
        }) || unit.any(&["next", "then", "plan", "will", "i'll", "also"])
            || unit.lower.trim_end().ends_with('?')
            || unit
                .words
                .iter()
                .any(|(_, w)| inflects_any(w, PUBLISHES) || w == "sent" || standing_act(w))
            || person_says(unit).is_some()
            || slotted(unit))
}

/// Whether `token`, a run of text between spaces, names code rather than words: an identifier
/// (`tap_of`, `Self::plan`, `plan()`), a path of two slashes or one naming a file
/// (`crates/aterm-cli/src`, `ship/release.rs`), or a bare file name with a source or config
/// extension (`release.rs`, `deploy.toml`). A URL (`https://…`), a domain (`crates.io`) and a pair
/// of words (`tag/release`, `upstream/main`) are words.
fn names_code(token: &str) -> bool {
    let t = token.trim_matches(|c: char| {
        !(c.is_alphanumeric() || matches!(c, '_' | '/' | ':' | '(' | ')' | '.'))
    });
    let path = t.contains('/')
        && (t.matches('/').count() >= 2 || t.split('/').any(|part| part.contains('.')));
    t.chars().any(char::is_alphanumeric)
        && !t.contains("://")
        && (t.contains('_')
            || t.contains("::")
            || t.contains("()")
            || path
            || bare_code_file(token))
}

fn bare_code_file(token: &str) -> bool {
    const CODE_EXTS: &[&str] = &[
        "rs", "toml", "lock", "json", "jsonl", "yaml", "yml", "md", "txt", "sh", "bash", "zsh",
        "fish", "ps1", "py", "rb", "go", "js", "mjs", "cjs", "ts", "mts", "cts", "tsx", "jsx", "c",
        "h", "cc", "cpp", "cxx", "hpp", "swift", "m", "mm", "java", "kt", "kts", "zig", "lua",
        "sql", "html", "css", "scss", "xml", "plist", "proto", "wgsl", "metal", "glsl", "hlsl",
        "nix", "cmake", "mk", "cfg", "ini", "conf",
    ];
    const HOSTS: &[&str] = &["docs.rs", "lib.rs", "platform.sh", "surge.sh"];
    let f = token.trim_matches(|c: char| !c.is_alphanumeric());
    let f = f.strip_suffix("'s").unwrap_or(f);
    if HOSTS.iter().any(|h| f.eq_ignore_ascii_case(h)) {
        return false;
    }
    let Some((stem, ext)) = f.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !stem.contains(['/', ':'])
        && stem.chars().any(char::is_alphabetic)
        && stem
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | '_'))
        && CODE_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// Whether `word` is an outward verb in the present tense of a third person (`releases`, `sends`,
/// `deletes`, `notifies`): an act set up to run by itself.
fn standing_act(word: &str) -> bool {
    let stems = [
        word.strip_suffix("ies").map(|s| format!("{s}y")),
        word.strip_suffix("es").map(str::to_string),
        word.strip_suffix('s')
            .filter(|s| !s.ends_with('s'))
            .map(str::to_string),
    ];
    stems.into_iter().flatten().any(|stem| {
        let stem = stem.as_str();
        OUTWARD_INFLECTED.contains(&stem)
            || OUTWARD_PREFIXES.contains(&stem)
            || DESTRUCTIVE_WORDS.contains(&stem)
    })
}

/// Whether a sentence of the turn's prose says the work `need` would do lacks or waits on what
/// only a person can give: a [`person_only`] word [`lacks`] reads so, in a sentence that names
/// some of the need's work ([`work_bases`]: `The build needs your GitHub token, which I don't
/// have.` or `I can't push without credentials.` above `NEEDS YOU: say "go" so I can rebuild the
/// branch and push it to main`). A sentence that names other work (`The commit needs your
/// password`), only names the word (`The census check now flags a token`) or says the work does
/// not need it (`I don't need your password to rebuild it`) blocks nothing, and the prose's
/// other exclusions stay its own sentences'.
fn blocked(need: &Unit, prose: &[Unit]) -> bool {
    let work = work_bases(need);
    kept(Part::PersonOnly)
        && prose.iter().any(|u| {
            work_bases(u).iter().any(|base| work.contains(base))
                && person_only_words(u).any(|(i, _)| lacks(u, i))
        })
}

/// The work `unit` names in its words or its code (`` `git push` ``), each as its base: a
/// [`WORK`] verb, a merge, rebase, push, run or restart, or a cleanup verb, in any inflection,
/// and a redo as the work it redoes (`rebuild` and `rebuilt` are `build`, `reran` is `run`).
fn work_bases(unit: &Unit) -> Vec<&'static str> {
    const VERBS: &[&str] = &[
        "merge", "rebase", "push", "run", "restart", "remove", "delete", "drop", "clean",
    ];
    let code = Unit::new(&unit.code, false);
    let mut out = Vec::new();
    for (_, w) in unit.words.iter().chain(&code.words) {
        let w = match w.as_str() {
            "built" | "rebuilt" => "build",
            "reran" | "re-ran" => "run",
            w => w,
        };
        for &base in WORK.iter().chain(VERBS).filter(|&&base| inflects(w, base)) {
            let redone = base
                .strip_prefix("re-")
                .or_else(|| base.strip_prefix("re"))
                .filter(|done| WORK.contains(done) || VERBS.contains(done));
            out.push(redone.unwrap_or(base));
        }
    }
    out
}

/// Whether the [`person_only`] word at `i` is one the work lacks or waits on: a need or a wait
/// (`needs your GitHub password`, `a token is required`) within five words before it or three
/// after; a lack within five words before it (`without`, `no`, `lacks`, `missing`, `unable`,
/// `can't`, a negated have: `I don't have push access`, `I haven't got the credentials`, `no
/// GitHub token here`); a negation right before it (`not logged in`) or in it (`not
/// permitted`); or a failure within three words after it (`the token expired`, `permission
/// denied`). A need a negation undoes (`I don't need your password`, `no password is needed`,
/// `needs no token`, `without needing`) makes it lack nothing.
fn lacks(unit: &Unit, i: usize) -> bool {
    let negation = |w: &str| matches!(w, "not" | "no" | "never") || w.ends_with("n't");
    let before = i.saturating_sub(5)..i;
    let needs: Vec<usize> = before
        .clone()
        .chain(i + 1..i + 4)
        .filter(|&m| inflects_any(unit.word(m), &["need", "require", "wait", "await"]))
        .collect();
    if needs.iter().any(|&m| {
        unit.before(m, 1) == "without"
            || (m.min(i).saturating_sub(2)..m.max(i)).any(|k| k != m && negation(unit.word(k)))
    }) {
        return false;
    }
    let lack = |k: usize| {
        let w = unit.word(k);
        matches!(
            w,
            "without"
                | "no"
                | "missing"
                | "unable"
                | "cannot"
                | "can't"
                | "couldn't"
                | "haven't"
                | "hasn't"
                | "hadn't"
        ) || inflects(w, "lack")
            || (negation(w) && matches!(unit.word(k + 1), "have" | "has" | "got"))
    };
    let failed = |k: usize| {
        matches!(
            unit.word(k),
            "denied" | "failed" | "fails" | "expired" | "revoked" | "rejected" | "missing"
        )
    };
    !needs.is_empty()
        || before.into_iter().any(lack)
        || negation(unit.before(i, 1))
        || negation(unit.word(i))
        || (i + 1..i + 4).any(failed)
}

/// One sentence or need the reader judges: its words as written (quote marks and dashes
/// folded), lowercased (one char for one, so a range of one is a range of the other), its
/// words with their byte offsets in the lowercased form, and the code it carried (inline spans,
/// a fenced block after its paragraph), which only the exclusions read.
#[derive(Clone)]
struct Unit {
    said: String,
    lower: String,
    words: Vec<(usize, String)>,
    need: bool,
    code: String,
    prior_clause: bool,
}

impl Unit {
    fn new(said: &str, need: bool) -> Self {
        Self::coded(said, need, "")
    }

    fn coded(said: &str, need: bool, code: &str) -> Self {
        Self::clause(said, need, code, false)
    }

    fn clause(said: &str, need: bool, code: &str, prior_clause: bool) -> Self {
        let said: String = said.chars().map(folded).collect();
        let lower: String = said.chars().map(lowered).collect();
        let words = words_of(&lower);
        Self {
            said,
            lower,
            words,
            need,
            code: code.to_string(),
            prior_clause,
        }
    }

    fn word(&self, i: usize) -> &str {
        self.words.get(i).map_or("", |(_, w)| w.as_str())
    }

    fn before(&self, i: usize, back: usize) -> &str {
        i.checked_sub(back).map_or("", |b| self.word(b))
    }

    fn len(&self) -> usize {
        self.words.len()
    }

    fn at(&self, i: usize, phrase: &[&str]) -> bool {
        phrase
            .iter()
            .enumerate()
            .all(|(k, p)| self.words.get(i + k).is_some_and(|(_, w)| w == p))
    }

    fn has(&self, phrase: &[&str]) -> bool {
        (0..self.len()).any(|i| self.at(i, phrase))
    }

    fn any(&self, words: &[&str]) -> bool {
        self.words.iter().any(|(_, w)| words.contains(&w.as_str()))
    }

    /// The unit with the tokens that name code put aside ([`names_code`]), its code kept.
    fn without_code_names(&self) -> Self {
        let words: Vec<&str> = self
            .said
            .split(' ')
            .map(|token| if names_code(token) { "" } else { token })
            .collect();
        Self::clause(&words.join(" "), self.need, &self.code, self.prior_clause)
    }

    /// Whether the unit is negated (`not`, `never`, `won't`, `no longer`, `refuse`).
    fn negated(&self) -> bool {
        self.negated_outside(0..0)
    }

    /// Whether a word outside `skip` (word indexes) negates the unit, as [`Self::negated`] reads.
    fn negated_outside(&self, skip: std::ops::Range<usize>) -> bool {
        (0..self.len()).filter(|i| !skip.contains(i)).any(|i| {
            let w = self.word(i);
            matches!(
                w,
                "not" | "never" | "cannot" | "refuse" | "refuses" | "refused" | "refusing"
            ) || w.ends_with("n't")
                || (w == "no" && self.word(i + 1) == "longer")
        })
    }

    /// Whether a negation comes within three words before word `i` (`don't have to wait`, `no
    /// need to wait`, `nothing here is waiting`).
    fn undone(&self, i: usize) -> bool {
        (1..=3).any(|back| {
            let w = self.before(i, back);
            matches!(w, "not" | "no" | "never" | "nothing" | "nobody" | "none")
                || w.ends_with("n't")
        })
    }

    /// The text of `lower`'s char range `from..to` (byte offsets) as written.
    fn said_at(&self, from: usize, to: usize) -> String {
        let skip = self.lower[..from].chars().count();
        let take = self.lower[from..to].chars().count();
        self.said.chars().skip(skip).take(take).collect()
    }
}

fn folded(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201C}' | '\u{201D}' => '"',
        '\u{2010}'..='\u{2013}' | '\u{2212}' => '-',
        c => c,
    }
}

fn lowered(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn words_of(lower: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    let keep = |c: char| c.is_alphanumeric() || matches!(c, '-' | '\'' | '$');
    for (at, c) in lower
        .char_indices()
        .chain(std::iter::once((lower.len(), ' ')))
    {
        match (start, keep(c)) {
            (None, true) => start = Some(at),
            (Some(s), false) => {
                let raw = &lower[s..at];
                let w = raw.trim_start_matches(['-', '\'']);
                let from = s + raw.len() - w.len();
                let w = w.trim_end_matches(['-', '\'']);
                let w = w.strip_suffix("'s").unwrap_or(w);
                if !w.is_empty() {
                    out.push((from, w.to_string()));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// `word` is `base` or one of its inflections: plurals, tenses (`overwritten`, `overwrote`), a
/// doubled final consonant, and the nouns `-ion`, `-ation`, `-ment` and `-al` make
/// (`deployment`, `migration`, `removal`).
fn inflects(word: &str, base: &str) -> bool {
    if word == base {
        return true;
    }
    // Every form below opens as `base` opens, no base being under two bytes, and most words open
    // otherwise.
    if word.as_bytes().first() != base.as_bytes().first() {
        return false;
    }
    if let Some(stem) = base.strip_suffix("write")
        && word == format!("{stem}wrote")
    {
        return true;
    }
    const ENDS: &[&str] = &[
        "s", "es", "d", "ed", "ing", "ten", "ion", "ions", "ation", "ations", "ment", "ments", "al",
    ];
    let stem = base.strip_suffix('e').unwrap_or(base);
    if [base, stem].iter().any(|s| {
        word.strip_prefix(s)
            .is_some_and(|rest| ENDS.contains(&rest))
    }) {
        return true;
    }
    if let Some(y) = base.strip_suffix('y')
        && word
            .strip_prefix(y)
            .is_some_and(|rest| matches!(rest, "ies" | "ied" | "ication" | "ications"))
    {
        return true;
    }
    let last = base.chars().last().unwrap_or(' ');
    word.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix(last))
        .is_some_and(|rest| matches!(rest, "ed" | "ing" | "er"))
}

fn inflects_any(word: &str, bases: &[&str]) -> bool {
    bases.iter().any(|b| inflects(word, b))
}

const QUOTE_ROW_MARKS: [char; 4] = ['>', '▎', '│', '┃'];

/// The sentences `prose` reads as, each with the code its words carried and whether a semicolon
/// in the same paragraph joins it to the clause before it: quoted rows dropped, wrapped rows of a
/// paragraph joined, inline code and attributed quotes dropped, split at `. ! ? ;` and a space.
/// The inline code goes beside its sentence and a fenced block beside the last sentence of the
/// paragraph before it, where only the exclusions read them.
fn sentences(prose: &str) -> Vec<(String, String, bool)> {
    let mut paragraphs: Vec<(String, String)> = Vec::new();
    let mut open = String::new();
    let mut fence = None;
    let flush = |open: &mut String, paragraphs: &mut Vec<(String, String)>| {
        if !open.trim().is_empty() {
            paragraphs.push((std::mem::take(open), String::new()));
        }
        open.clear();
    };
    for row in prose.lines() {
        let t = row.trim();
        if needs::fenced_line(row, &mut fence) {
            flush(&mut open, &mut paragraphs);
            if let Some((_, fenced)) = paragraphs.last_mut() {
                fenced.push_str(t);
                fenced.push('\n');
            }
            continue;
        }
        if t.is_empty() || t.starts_with(QUOTE_ROW_MARKS) {
            flush(&mut open, &mut paragraphs);
            continue;
        }
        if list_start(t) {
            flush(&mut open, &mut paragraphs);
        }
        if !open.is_empty() && !open.ends_with('-') {
            open.push(' ');
        }
        open.push_str(t);
    }
    flush(&mut open, &mut paragraphs);
    let mut out = Vec::new();
    for (paragraph, fenced) in &paragraphs {
        let (words, spans) = uncoded(paragraph);
        let mut said: Vec<(String, String, bool)> = split_sentences(&words)
            .into_iter()
            .map(|(range, sentence)| {
                let code: Vec<&str> = spans
                    .iter()
                    .filter(|(at, _)| range.contains(at))
                    .map(|(_, c)| c.as_str())
                    .collect();
                (sentence, code.join(" "), false)
            })
            .collect();
        for k in 1..said.len() {
            said[k].2 = said[k - 1].0.ends_with(';');
        }
        if let Some((_, code, _)) = said.last_mut()
            && !fenced.is_empty()
        {
            code.push(' ');
            code.push_str(fenced);
        }
        out.append(&mut said);
    }
    out
}

fn list_start(t: &str) -> bool {
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("• ") {
        return true;
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))
}

/// The verbs that ask the person to say a word (`say "rerun the checks"`).
const SAYS: &[&str] = &["say", "type", "reply", "answer", "send", "write"];

/// The verbs that attribute a quote to someone (`the note said "…"`, `the worker posted: "…"`):
/// its words are theirs.
const ATTRIBUTES: &[&str] = &[
    "wrote",
    "said",
    "asked",
    "commented",
    "says",
    "reads",
    "read",
    "posted",
    "replied",
    "reported",
    "printed",
    "relayed",
];

/// `paragraph` without its inline code (kept, as a quote, right after a [`SAYS`] verb) and the
/// quotes an [`ATTRIBUTES`] verb attributes, and the code it dropped, each span with the byte
/// offset in the text returned where it stood.
fn uncoded(paragraph: &str) -> (String, Vec<(usize, String)>) {
    let mut out = String::new();
    let mut code = Vec::new();
    let mut rest = paragraph;
    while let Some(at) = rest.find(['`', '"', '\u{201C}']) {
        let mark = rest[at..].chars().next().unwrap_or('"');
        let close = if mark == '\u{201C}' { '\u{201D}' } else { mark };
        let after = &rest[at + mark.len_utf8()..];
        let Some(end) = after.find(close) else {
            break;
        };
        let lead = last_word(&format!("{out}{}", &rest[..at]));
        out.push_str(&rest[..at]);
        let inner = &after[..end];
        if mark == '`' {
            if SAYS.contains(&lead.as_str()) || lead == "with" {
                out.push('"');
                out.push_str(inner);
                out.push('"');
            } else {
                code.push((out.len(), inner.to_string()));
                out.push(' ');
            }
        } else if ATTRIBUTES.contains(&lead.as_str()) {
            out.push(' ');
        } else {
            out.push(mark);
            out.push_str(inner);
            out.push(close);
        }
        rest = &after[end + close.len_utf8()..];
    }
    out.push_str(rest);
    (out, code)
}

fn last_word(text: &str) -> String {
    text.trim_end()
        .trim_end_matches([':', ','])
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or_default()
        .chars()
        .map(lowered)
        .collect()
}

/// `paragraph`'s sentences, each with the byte range of `paragraph` it was cut from.
fn split_sentences(paragraph: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    let bytes = paragraph.as_bytes();
    let mut cut = |range: std::ops::Range<usize>| {
        let s = paragraph[range.clone()].trim();
        if !s.is_empty() {
            out.push((range, s.to_string()));
        }
    };
    for (i, b) in bytes.iter().enumerate() {
        if matches!(b, b'.' | b'!' | b'?' | b';') && matches!(bytes.get(i + 1), None | Some(b' ')) {
            cut(from..i + 1);
            from = i + 1;
        }
    }
    cut(from..paragraph.len());
    out
}

/// Whether `unit` parks, with the word it asked the person to say: a consent ask, own work,
/// no exclusion in it or in `before`, nothing only a person can give in `after`, and no answer
/// it echoes in it or `after`.
fn judged(unit: &Unit, before: Option<&Unit>, after: Option<&Unit>) -> Option<Option<String>> {
    let ask = asks(unit)?;
    if !works(unit, before, ask.stop_note) {
        return None;
    }
    if excluded(unit, ask.says_at)
        || before.is_some_and(|b| excluded(b, None))
        || after.is_some_and(|a| kept(Part::PersonOnly) && person_only(a))
    {
        return None;
    }
    if kept(Part::Echoes) && (echoes(unit) || after.is_some_and(echoes)) {
        return None;
    }
    Some(ask.token)
}

/// The reader's families and exclusions, each of which a test drops to show some table row
/// needs it ([`kept`]).
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) enum Part {
    Consent = 1,
    Says = 1 << 1,
    Fixed = 1 << 2,
    Slots = 1 << 3,
    OwnInitiative = 1 << 4,
    StopNote = 1 << 5,
    PersonOnly = 1 << 6,
    PersonFirst = 1 << 7,
    Outward = 1 << 8,
    SomeoneElse = 1 << 9,
    Declined = 1 << 10,
    Chooses = 1 << 11,
    Echoes = 1 << 12,
    Attributed = 1 << 13,
}

#[cfg(test)]
thread_local! {
    /// The parts this thread's reader drops ([`Part`]).
    pub(super) static DROPPED: std::cell::Cell<u16> = const { std::cell::Cell::new(0) };
}

/// Whether a test has this thread's reader drop a part, so that its reading is not the words' alone.
fn dropped_parts() -> bool {
    #[cfg(test)]
    {
        DROPPED.with(std::cell::Cell::get) != 0
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// Whether the reader reads with `part` (every part, outside a test that drops one).
fn kept(part: Part) -> bool {
    #[cfg(test)]
    {
        DROPPED.with(|d| d.get() & part as u16 == 0)
    }
    #[cfg(not(test))]
    {
        let _ = part;
        true
    }
}

struct Ask {
    token: Option<String>,
    /// The [`SAYS`] verb's word index, which no exclusion reads (`send "go"` sends nothing).
    says_at: Option<usize>,
    /// A stop note's negated restart: the restart is the stopped work.
    stop_note: bool,
}

fn asks(unit: &Unit) -> Option<Ask> {
    if let Some((at, token)) = person_says(unit).filter(|_| kept(Part::Says)) {
        return Some(Ask {
            token,
            says_at: Some(at),
            stop_note: false,
        });
    }
    let stop_note = kept(Part::StopNote) && stop_note(unit);
    let asked = (kept(Part::Consent) && consent(unit))
        || (kept(Part::Fixed) && fixed_ask(unit))
        || (kept(Part::Slots) && slotted(unit))
        || (kept(Part::OwnInitiative) && on_my_own(unit));
    (asked || stop_note).then_some(Ask {
        token: None,
        says_at: None,
        stop_note,
    })
}

/// The words before a `go ahead` that make it the noun (`your go ahead`).
const DETERMINERS: &[&str] = &[
    "your", "the", "a", "an", "my", "our", "their", "that", "this", "any", "explicit", "owner",
    "person", "user", "human", "someone", "somebody", "final", "formal",
];

/// The consent noun at word `i` (family B), and its length in words.
fn consent_at(unit: &Unit, i: usize) -> Option<usize> {
    let prev = unit.before(i, 1);
    let next = unit.word(i + 1);
    match unit.word(i) {
        "go-ahead" | "goahead" | "say-so" | "sign-off" | "signoff" | "thumbs-up" | "greenlight"
        | "approval" | "confirmation" | "authorization" | "authorisation" | "clearance" => Some(1),
        "go" if next == "ahead" && DETERMINERS.contains(&prev) => Some(2),
        "green" if next == "light" => Some(2),
        "thumbs" if next == "up" => Some(2),
        "permission" if matches!(prev, "your" | "owner" | "person" | "user") && next == "to" => {
            Some(1)
        }
        "ok" | "okay" if matches!(prev, "the" | "an" | "your") => Some(1),
        "nod" if matches!(prev, "the" | "a" | "your") => Some(1),
        "word" if prev == "the" => Some(1),
        "yes" if prev == "a" => Some(1),
        _ => None,
    }
}

/// Family B: a consent noun with a pending marker near it, or any consent noun in a need (its
/// head is the marker). A wait for room after the consent noun waits on no person (`per the
/// go-ahead I'm waiting for room first`, `I'm waiting until memory frees up`); one before it may
/// (`waiting for memory and for your go-ahead`). A consent noun its own clause sets aside marks
/// no wait ([`disowned`]: `waiting for room, not for a go-ahead`).
fn consent(unit: &Unit) -> bool {
    (0..unit.len()).any(|i| consent_at(unit, i).is_some_and(|n| awaited(unit, i, n)))
}

/// Whether the unit waits on the consent noun at word `i`, `n` words long, as [`consent`]
/// reads it: not set aside ([`disowned`]), and in a need or with a pending marker near it.
fn awaited(unit: &Unit, i: usize, n: usize) -> bool {
    !disowned(unit, i)
        && (unit.need
            || (i.saturating_sub(6)..(i + n + 3).min(unit.len()))
                .any(|m| pending(unit, m) && !(m > i && waits_for_room(unit, m))))
}

/// Whether the consent noun at word `i` is set aside by a negation in front of its own phrase
/// (`not for a go-ahead`, `not on your approval`, `never the go-ahead`): the wait beside it is for
/// something else. `without your go-ahead` and `no go-ahead yet` still wait on it.
fn disowned(unit: &Unit, i: usize) -> bool {
    const DETS: &[&str] = &[
        "a", "an", "the", "your", "my", "our", "their", "any", "owner", "person",
    ];
    let mut k = i;
    while k > 0 && DETS.contains(&unit.word(k - 1)) {
        k -= 1;
    }
    if k > 0 && matches!(unit.word(k - 1), "for" | "on" | "upon") {
        k -= 1;
    }
    k > 0 && matches!(unit.word(k - 1), "not" | "never" | "nor")
}

/// Whether the wait at word `i` waits for memory, disk or room: a word of room ([`ROOM_WORDS`])
/// within the six words after it, in its clause (`waiting for room`, `waiting on the disk`,
/// `waiting until memory frees up`, `until the Mac has memory to spare`).
fn waits_for_room(unit: &Unit, i: usize) -> bool {
    let Some((from, _)) = unit.words.get(i) else {
        return false;
    };
    (i + 1..(i + 7).min(unit.len()))
        .take_while(|&k| {
            let (at, _) = &unit.words[k];
            !unit.lower[*from..*at].contains([',', ';', ':', '(', '\u{2014}'])
        })
        .any(|k| ROOM_WORDS.contains(&unit.word(k)))
}

/// Whether word `i` marks a wait (`waiting for`, `until`, `needs your`, `only with`, `so I can`):
/// a wait or a need a negation undoes (`don't have to wait`, `no need to wait`) marks none.
fn pending(unit: &Unit, i: usize) -> bool {
    let prev = unit.before(i, 1);
    if matches!(prev, "not" | "no" | "never" | "longer" | "without") || prev.ends_with("n't") {
        return false;
    }
    let next = unit.word(i + 1);
    let w = unit.word(i);
    // A wait in the past tense is a report (`the rebuild that was waiting for your go-ahead is
    // done`).
    let past = matches!(prev, "was" | "were")
        || (prev == "been" && (unit.before(i, 2) == "had" || unit.before(i, 2).ends_with("'d")));
    match w {
        w if w.starts_with("wait") || w.starts_with("await") => !unit.undone(i) && !past,
        "until" | "till" | "unless" | "pending" | "before" => true,
        "need" | "needs" | "needed" | "require" | "requires" => {
            !unit.undone(i)
                && [next, unit.word(i + 2)]
                    .iter()
                    .any(|n| matches!(*n, "your" | "the" | "a" | "an" | "owner" | "person"))
        }
        "only" => matches!(
            next,
            "with" | "after" | "on" | "once" | "when" | "if" | "upon"
        ),
        "without" => (0..i).any(|b| {
            matches!(
                unit.word(b),
                "won't" | "can't" | "cannot" | "don't" | "shouldn't"
            )
        }),
        "so" => unit.at(i + 1, &["i", "can"]) || unit.at(i + 1, &["that", "i", "can"]),
        "as" => unit.at(i + 1, &["soon", "as", "i", "get"]),
        "once" | "when" => unit.at(i + 1, &["i", "get"]),
        _ => false,
    }
}

/// The words of the people a [`SAYS`] verb's subject names (`you'll need to say`).
const PEOPLE: &[&str] = &[
    "you", "you'll", "you'd", "owner", "person", "someone", "somebody", "user", "they", "human",
];

/// The words that make a [`SAYS`] verb someone else's (`the tests say "ok"`).
const NOT_PEOPLE: &[&str] = &[
    "i", "i'll", "i'd", "i'm", "we", "we'll", "it", "he", "she", "harness", "note", "notice",
    "prompt", "test", "tests", "check", "checks", "doc", "docs", "rule", "rules", "which", "who",
    "the", "a", "an", "my", "your", "this", "that", "its", "our", "their",
];

/// The first words that make what follows a [`SAYS`] verb its object or a question, not a word
/// to say (`answer the security prompt`, `say whether it should`).
const NOT_TOKENS: &[&str] = &[
    "the", "a", "an", "your", "my", "this", "that", "its", "our", "their", "these", "those",
    "whether", "what", "which", "how", "why", "who",
];

/// Family C's person-says: a [`SAYS`] verb (or `tell me`) the person is to use, then an optional
/// `:` or `with`, then the word to say, quoted (inline code is a quote here) or bare (in a need,
/// or in prose after `just`, `please` or `simply`, or opening it); a bare word is a word to say,
/// never an object (`send the report`). A verb after a person's own act (`click Allow, then say
/// "go"`) asks no word: the act comes first, unless it is a wait for room. Its verb's word index
/// and the word as written.
fn person_says(unit: &Unit) -> Option<(usize, Option<String>)> {
    (0..unit.len()).find_map(|i| {
        let w = unit.word(i);
        let tell = w == "tell" && unit.word(i + 1) == "me";
        if !(SAYS.contains(&w) || tell) || !says_lead(unit, i) || acts_first(unit, i) {
            return None;
        }
        let (start, word) = &unit.words[i + usize::from(tell)];
        let rest = &unit.lower[start + word.len()..];
        let trimmed = rest.trim_start().trim_start_matches(':').trim_start();
        let trimmed = trimmed
            .strip_prefix("with ")
            .unwrap_or(trimmed)
            .trim_start();
        let from = unit.lower.len() - trimmed.len();
        if let Some(inner) = trimmed.strip_prefix('"') {
            let end = inner.find('"')?;
            let token = inner[..end].trim();
            return (!token.is_empty()).then(|| (i, Some(unit.said_at(from + 1, from + 1 + end))));
        }
        let bare = unit.need || i == 0 || matches!(unit.before(i, 1), "just" | "please" | "simply");
        if !bare || tell {
            return tell.then_some((i, None));
        }
        if matches!(w, "send" | "write")
            || unit
                .words
                .iter()
                .find(|(at, _)| *at >= from)
                .is_none_or(|(_, first)| NOT_TOKENS.contains(&first.as_str()))
        {
            return None;
        }
        let mut end = from;
        let mut n = 0;
        for (at, word) in unit.words.iter().skip(i + 1) {
            if *at < from {
                continue;
            }
            if matches!(
                word.as_str(),
                "so" | "to" | "and" | "when" | "once" | "then" | "if" | "after" | "before" | "or"
            ) || unit.lower[end..*at].contains([',', '.', ';', ':', '('])
                || n == 8
            {
                break;
            }
            end = at + word.len();
            n += 1;
        }
        (n > 0).then(|| (i, Some(unit.said_at(from, end))))
    })
}

/// Whether the [`SAYS`] verb at `i`, after `then` or `and`, follows a clause of the person's own
/// act (`click Allow on the dialog, then say "go"`) that neither says a word itself nor waits for
/// room (`free disk room or let the other builds finish, then say …`).
fn acts_first(unit: &Unit, i: usize) -> bool {
    if !matches!(unit.before(i, 1), "then" | "and") {
        return false;
    }
    let Some((at, _)) = i.checked_sub(1).and_then(|k| unit.words.get(k)) else {
        return false;
    };
    let head = unit.lower[..*at].trim_end().trim_end_matches(',');
    let clause = Unit::new(
        &head[head.rfind([',', ';', ':', '(']).map_or(0, |k| k + 1)..],
        false,
    );
    clause.len() > 0
        && !SAYS.contains(&clause.word(0))
        && !clause.any(ROOM_NOUNS)
        && !clause.any(ROOM_VERBS)
}

/// Whether the [`SAYS`] verb at `i` is the person's: it opens the unit, follows `then`,
/// `just`, `please`, `and`, `or` or a person, or follows `to`/`can` after a person
/// (`you'll need to say`).
fn says_lead(unit: &Unit, i: usize) -> bool {
    let prev = unit.before(i, 1);
    if i == 0 || matches!(prev, "then" | "just" | "please" | "simply" | "and" | "or") {
        return true;
    }
    if PEOPLE.contains(&prev) {
        return true;
    }
    if NOT_PEOPLE.contains(&prev) {
        return false;
    }
    matches!(prev, "to" | "can" | "could" | "would" | "will" | "only")
        && (2..=4).any(|b| PEOPLE.contains(&unit.before(i, b)))
}

/// Family C's fixed phrases (`say the word`, `only when asked`, `without being asked` with a
/// negation).
fn fixed_ask(unit: &Unit) -> bool {
    const ASKS: &[&[&str]] = &[
        &["say", "the", "word"],
        &["give", "the", "word"],
        &["give", "me", "the", "word"],
        &["tell", "me", "to"],
        &["ask", "me", "to"],
        &["only", "when", "asked"],
        &["only", "if", "asked"],
        &["until", "asked"],
        &["unless", "asked"],
        &["until", "told"],
        &["unless", "told"],
    ];
    ASKS.iter().any(|p| unit.has(p))
        || (unit.negated()
            && (unit.has(&["without", "being", "asked"])
                || unit.has(&["without", "being", "told"])))
}

const SLOT_WHEN: &[&[&str]] = &[
    &["until"],
    &["unless"],
    &["till"],
    &["once"],
    &["before"],
    &["when"],
    &["if"],
    &["as", "soon", "as"],
    &["after"],
    &["the", "moment"],
    &["only", "when"],
    &["only", "if"],
];

const SLOT_WHO: &[&[&str]] = &[
    &["you"],
    &["someone"],
    &["somebody"],
    &["a", "person"],
    &["the", "person"],
    &["the", "owner"],
    &["the", "user"],
    &["a", "human"],
    &["they"],
];

const SLOT_ADVERBS: &[&str] = &["explicitly", "actually", "specifically"];

const SLOT_SAYS: &[&str] = &[
    "ask", "asks", "say", "says", "tell", "tells", "confirm", "confirms", "approve", "approves",
    "give", "gives", "ok", "okay", "oks", "okays",
];

/// The length of `phrases`' phrase at word `i`, if one is there.
fn slot(unit: &Unit, i: usize, phrases: &[&[&str]]) -> Option<usize> {
    phrases.iter().find(|p| unit.at(i, p)).map(|p| p.len())
}

fn skip_adverb(unit: &Unit, i: usize) -> usize {
    i + usize::from(SLOT_ADVERBS.contains(&unit.word(i)))
}

/// Family C's three slots (`until a person asks`, `unless you explicitly say`), its wait
/// (`waiting for you to ask`) and its passive (`until I'm asked`).
fn slotted(unit: &Unit) -> bool {
    (0..unit.len()).any(|i| {
        let w = unit.word(i);
        if (w.starts_with("wait") || w.starts_with("await"))
            && unit.word(i + 1) == "for"
            && !unit.undone(i)
            && let Some(n) = slot(unit, i + 2, SLOT_WHO)
            && unit.word(i + 2 + n) == "to"
            && SLOT_SAYS.contains(&unit.word(i + 3 + n))
        {
            return true;
        }
        let Some(when) = slot(unit, i, SLOT_WHEN) else {
            return false;
        };
        let at = skip_adverb(unit, i + when);
        if let Some(who) = slot(unit, at, SLOT_WHO) {
            let at = skip_adverb(unit, at + who);
            if SLOT_SAYS.contains(&unit.word(at)) {
                return true;
            }
        }
        let passive = matches!(unit.word(i), "until" | "unless")
            || unit.at(i, &["only", "when"])
            || unit.at(i, &["only", "if"]);
        let at = i + if unit.word(i) == "only" { 2 } else { 1 };
        passive
            && (unit.word(at) == "i'm" || unit.at(at, &["i", "am"]) || unit.at(at, &["i", "get"]))
            && {
                let after = at + if unit.word(at) == "i'm" { 1 } else { 2 };
                matches!(unit.word(after), "asked" | "told")
            }
    })
}

/// Family D: `on my own`, `by myself` or `myself` with a negation (`I won't restart it on my
/// own`).
fn on_my_own(unit: &Unit) -> bool {
    unit.negated()
        && (unit.has(&["on", "my", "own"]) || unit.has(&["by", "myself"]) || unit.any(&["myself"]))
}

/// Family D's stop note: a stop note or notice and a negated restart (`Per the stop notice, I'm
/// not restarting either.`). A restart held only until there is room (`until memory frees up`,
/// `once the disk has room`) waits on no person: it does as the note says.
fn stop_note(unit: &Unit) -> bool {
    (unit.has(&["stop", "note"]) || unit.has(&["stop", "notice"]))
        && unit.negated()
        && restarts(unit)
        && !(0..unit.len()).any(|k| {
            matches!(unit.word(k), "until" | "till" | "once" | "when") && waits_for_room(unit, k)
        })
}

fn restarts(unit: &Unit) -> bool {
    (0..unit.len()).any(|i| {
        inflects(unit.word(i), "restart")
            || (inflects(unit.word(i), "start") && (i + 1..i + 5).any(|k| unit.word(k) == "again"))
    })
}

/// The agent's own reversible work: verbs in any inflection.
const WORK: &[&str] = &[
    "build", "rebuild", "compile", "test", "retest", "rerun", "re-run", "check", "verify", "retry",
    "resume", "finish", "continue", "commit", "land",
];

/// The [`WORK`] verbs that name no work of their own: each counts only over the agent's own work
/// ([`OWN_OBJECTS`]) or with no object (`I'll continue once you say so`), never over another
/// (`continue the rollout to the remaining hosts`, `resume the backfill`).
const GENERIC_WORK: &[&str] = &["retry", "resume", "finish", "continue"];

/// The objects that make a [`GENERIC_WORK`] verb the agent's own work.
const OWN_OBJECTS: &[&str] = &[
    "build",
    "builds",
    "rebuild",
    "compile",
    "test",
    "tests",
    "check",
    "checks",
    "verify",
    "verification",
    "run",
    "runs",
    "suite",
    "merge",
    "rebase",
    "branch",
    "work",
    "fix",
    "fixes",
    "change",
    "changes",
    "commit",
    "commits",
    "worktree",
    "task",
    "tasks",
    "round",
    "it",
    "this",
    "that",
    "them",
    "mine",
];

/// The words a [`GENERIC_WORK`] verb's object follows (`continue the build`).
const OBJECT_LEADS: &[&str] = &[
    "the", "my", "this", "that", "its", "our", "a", "an", "with", "on",
];

/// Whether the [`GENERIC_WORK`] verb at word `i` is over the agent's own work: its object, past
/// a determiner, is one of [`OWN_OBJECTS`], or it has none (the clause ends, or a marker of when
/// or a person's ask follows).
fn generic_own(unit: &Unit, i: usize) -> bool {
    let mut at = i + 1;
    while OBJECT_LEADS.contains(&unit.word(at)) {
        at += 1;
    }
    let object = unit.word(at);
    let ends = unit.words.get(at).is_none_or(|(to, _)| {
        let (from, w) = &unit.words[i];
        unit.lower[from + w.len()..*to].contains([',', ';', ':', '.', '('])
    });
    ends || OWN_OBJECTS.contains(&object)
        || matches!(
            object,
            "once"
                | "when"
                | "if"
                | "until"
                | "after"
                | "as"
                | "now"
                | "and"
                | "so"
                | "then"
                | "from"
                | "where"
                | "unless"
                | "before"
                | "again"
        )
}

const WORK_IRREGULAR: &[&str] = &["built", "rebuilt", "reran", "re-ran"];

/// What a restart restarts when the unit or the one before names it (`the check run … I won't
/// restart it`).
const RESTARTED: &[&str] = &[
    "build", "rebuild", "compile", "test", "check", "verify", "run",
];

/// Whether `unit` names own reversible work from the allow list: a [`WORK`] verb, a merge of
/// main or a branch, a push (a tag, a release or upstream is an exclusion's), a rebase onto
/// main, its own cleanup, a restart of a stopped build (`stop_note`, or a [`RESTARTED`] word
/// in it or `before`), or pending own work in a semicolon-joined prior clause ([`prior_clause_works`]).
fn works(unit: &Unit, before: Option<&Unit>, stop_note: bool) -> bool {
    let names = |u: &Unit, bases: &[&str]| u.words.iter().any(|(_, w)| inflects_any(w, bases));
    // What a condition names is what the go-ahead waits for, not the agent's work: `the test
    // iPhone` in `once the test iPhone is connected` is no test the agent runs.
    let condition = in_condition(unit);
    let own = |i: usize| !condition[i];
    let worked = (0..unit.len()).filter(|&i| own(i)).any(|i| {
        let w = unit.word(i);
        inflects_any(w, WORK) && (!inflects_any(w, GENERIC_WORK) || generic_own(unit, i))
    });
    if worked
        || (0..unit.len()).any(|i| own(i) && WORK_IRREGULAR.contains(&unit.word(i)))
        || own_cleanup(unit).into_iter().any(own)
    {
        return true;
    }
    let mains = unit.any(&["main", "branch"]);
    if (0..unit.len()).filter(|&i| own(i)).any(|i| {
        let w = unit.word(i);
        (inflects(w, "merge") && mains)
            || (inflects(w, "rebase") && unit.any(&["main"]))
            || inflects(w, "push")
    }) {
        return true;
    }
    (restarts(unit)
        && (stop_note || names(unit, RESTARTED) || before.is_some_and(|b| names(b, RESTARTED))))
        || before.is_some_and(|b| prior_clause_works(unit, b))
}

/// Which words of `unit` stand in a condition the go-ahead waits on: from `once`, `when` or
/// `after` to the clause's end (a comma, semicolon or colon, `so` or `then`, or the sentence's
/// end),
/// as in `Waiting on your go-ahead once the test iPhone is connected.` or `… once you connect the
/// test iPhone.`
fn in_condition(unit: &Unit) -> Vec<bool> {
    // Not `until`: `until you tell me to rebuild` is the go-ahead itself, its act the agent's.
    const OPENS: &[&str] = &["once", "when", "after"];
    // `when you're back so I can rebuild`: what follows is the agent's again.
    const CLOSES: &[&str] = &["so", "then"];
    let mut open = false;
    let mut end = 0;
    unit.words
        .iter()
        .map(|(at, w)| {
            if open
                && (unit.lower[end.min(*at)..*at].contains([',', ';', ':'])
                    || CLOSES.contains(&w.as_str()))
            {
                open = false;
            }
            end = at + w.len();
            open |= OPENS.contains(&w.as_str());
            open
        })
        .collect()
}

/// Whether `b`, joined to `unit` by a semicolon in the same paragraph (`Ready to commit and
/// push; waiting on your sign-off.`), names pending own reversible work that `unit` waits on.
/// Past completed work (`I built and tested the fix; waiting on your sign-off.`) and bare
/// stage/phase asks (`waiting for your go-ahead on the next stage`) stay plain.
fn prior_clause_works(unit: &Unit, b: &Unit) -> bool {
    const PENDING_WORDS: &[&str] = &["ready", "pending", "remaining", "queued"];
    const UNDONE_WORK: &[&str] = &["uncommitted", "unpushed", "unmerged", "untested", "unbuilt"];
    const PENDING_PHRASES: &[&[&str]] = &[
        &["left", "to"],
        &["what's", "left"],
        &["was", "stopped"],
        &["were", "stopped"],
        &["is", "stopped"],
        &["is", "paused"],
        &["on", "hold"],
    ];
    const MODAL_WORDS: &[&str] = &["will", "i'll", "we'll", "can"];
    const INFINITIVE_LEADS: &[&str] = &["need", "needs", "going", "about", "set", "prepared"];
    const STAGED: &[&str] = &["stage", "stages", "phase", "phases"];
    if !unit.prior_clause
        || asks(b).is_some()
        || (kept(Part::Echoes) && echoes(b))
        || unit.any(STAGED)
        || (0..unit.len())
            .any(|i| inflects_any(unit.word(i), GENERIC_WORK) && !generic_own(unit, i))
        || !works(b, None, false)
    {
        return false;
    }
    let pending_state = (0..b.len()).any(|i| PENDING_WORDS.contains(&b.word(i)) && !b.undone(i))
        || b.any(UNDONE_WORK)
        || PENDING_PHRASES
            .iter()
            .any(|p| (0..b.len()).any(|i| b.at(i, p) && !b.undone(i)));
    if pending_state {
        return true;
    }
    let cleanup = own_cleanup(b);
    let restarts_known = b.words.iter().any(|(_, rw)| inflects_any(rw, RESTARTED));
    let work_verb_at = |i: usize| {
        let w = b.word(i);
        (inflects_any(w, WORK) && (!inflects_any(w, GENERIC_WORK) || generic_own(b, i)))
            || cleanup.contains(&i)
            || (inflects(w, "merge") && b.any(&["main", "branch"]))
            || (inflects(w, "rebase") && b.any(&["main"]))
            || inflects(w, "push")
            || (inflects(w, "restart") && restarts_known)
    };
    let same_clause = |from: usize, to: usize| {
        let (Some((a, aw)), Some((z, _))) = (b.words.get(from), b.words.get(to)) else {
            return false;
        };
        !b.lower[a + aw.len()..*z].contains([',', ';', ':', '('])
    };
    (0..b.len()).any(|i| {
        if !(work_verb_at(i) || WORK_IRREGULAR.contains(&b.word(i))) {
            return false;
        }
        let negated_verb = (1..=3).any(|back| {
            i.checked_sub(back).is_some_and(|k| {
                let w = b.word(k);
                (matches!(w, "not" | "never") || w.ends_with("n't")) && same_clause(k, i)
            })
        });
        if negated_verb {
            return true;
        }
        if !work_verb_at(i) || WORK_IRREGULAR.contains(&b.word(i)) || b.word(i).ends_with("ed") {
            return false;
        }
        (1..=4).any(|back| {
            i.checked_sub(back).is_some_and(|k| {
                same_clause(k, i)
                    && (MODAL_WORDS.contains(&b.word(k))
                        || (INFINITIVE_LEADS.contains(&b.word(k)) && b.word(k + 1) == "to"))
            })
        })
    })
}

/// The branches the agent's own cleanup deletes (`delete my merged branch`).
const OWN_BRANCHES: &[&[&str]] = &[
    &["my", "merged", "branch"],
    &["the", "merged", "branch"],
    &["my", "local", "branch"],
    &["the", "local", "branch"],
    &["merged", "branch"],
    &["local", "branch"],
];

/// The words that may stand between an own cleanup verb and the worktree it takes as its object
/// (`clean up the stale worktree`): determiners and the words of the agent's own finished work.
const CLEANUP_BETWEEN: &[&str] = &[
    "up",
    "the",
    "my",
    "this",
    "that",
    "our",
    "its",
    "stale",
    "old",
    "merged",
    "finished",
    "local",
    "own",
    "temporary",
    "extra",
    "unused",
    "landed",
];

/// The words that, right after a worktree or branch, name what it holds (`the worktree's
/// uncommitted changes`, `the worktree files`): an act on them is no cleanup of the agent's.
const CONTENTS: &[&str] = &[
    "changes",
    "change",
    "files",
    "file",
    "commits",
    "commit",
    "stash",
    "stashes",
    "edits",
    "uncommitted",
    "untracked",
    "unpushed",
    "contents",
    "content",
    "history",
    "data",
    "diff",
    "diffs",
    "state",
];

/// The word indexes of the agent's own cleanup verbs (`remove the worktree`, `delete my merged
/// branch`), which no outward word reads: the worktree or branch is the verb's object, past only
/// [`CLEANUP_BETWEEN`]'s words. A worktree or branch someone else owns (`remove your old
/// worktree`, `the worktree you made`) is no cleanup of the agent's, nor is what one holds (`the
/// worktree's uncommitted changes`, `the worktree files`).
fn own_cleanup(unit: &Unit) -> Vec<usize> {
    (0..unit.len())
        .filter(|&i| {
            if !inflects_any(unit.word(i), &["remove", "delete", "drop", "clean"]) {
                return false;
            }
            let mut k = i + 1;
            while k < i + 5 && CLEANUP_BETWEEN.contains(&unit.word(k)) {
                k += 1;
            }
            let noun = (unit.word(k) == "worktree").then_some(k).or_else(|| {
                OWN_BRANCHES
                    .iter()
                    .find(|p| unit.at(i + 1, p))
                    .map(|p| i + p.len())
            });
            noun.is_some_and(|n| {
                let (at, w) = &unit.words[n];
                !owned_after(unit, n)
                    && !unit.lower[at + w.len()..].starts_with('\'')
                    && !CONTENTS.contains(&unit.word(n + 1))
                    && !(i + 1..n).any(|k| foreign(unit, k))
            })
        })
        .collect()
}

/// The byte ranges in `text` of the agent's own cleanup verbs (`own_cleanup`: `remove` in
/// `push it to main and remove the worktree`), read as one unit.
pub fn own_cleanup_verbs(text: &str) -> Vec<std::ops::Range<usize>> {
    let unit = Unit::new(text, false);
    // The unit's words are a char for each of `text`'s: count chars, not bytes.
    let byte = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(b, _)| b)
    };
    own_cleanup(&unit)
        .into_iter()
        .map(|i| {
            let (at, w) = &unit.words[i];
            let from = unit.lower[..*at].chars().count();
            byte(from)..byte(from + w.chars().count())
        })
        .collect()
}

/// Whether an exclusion word at `i` counts: one right after `no`, `never`, `won't`,
/// `without`, `nothing` or `not` does not (`no force-push`).
fn counts(unit: &Unit, i: usize) -> bool {
    !matches!(
        unit.before(i, 1),
        "no" | "never" | "won't" | "without" | "nothing" | "not"
    )
}

/// Whether an exclusion fires on `unit`, or on the code it carries: a command it names is read
/// for what it would do (`git push --force`, `gh auth login`).
fn excluded(unit: &Unit, says_at: Option<usize>) -> bool {
    let code = Unit::new(&unit.code, false);
    (kept(Part::PersonOnly) && (person_only(unit) || person_only(&code)))
        || (kept(Part::PersonFirst) && person_first(unit))
        || (kept(Part::Outward) && (outward(unit, says_at) || outward(&code, None)))
        || (kept(Part::SomeoneElse) && someone_else(unit))
        || (kept(Part::Declined) && declined(unit))
        || (kept(Part::Chooses) && chooses(unit))
        || (kept(Part::Attributed) && attributed(unit))
}

/// A hold the person gave, attributed to them (`as you asked`, `you told me`, `per your
/// instruction`, `you stopped the build`): a go-ahead never lifts their own word.
fn attributed(unit: &Unit) -> bool {
    const PHRASES: &[&[&str]] = &[
        &["as", "you", "asked"],
        &["you", "asked", "me"],
        &["as", "you", "said"],
        &["you", "said"],
        &["you", "told", "me"],
        &["as", "you", "told"],
        &["you", "stopped"],
        &["you", "wanted"],
        &["per", "your"],
        &["at", "your", "request"],
        &["as", "requested"],
        &["as", "instructed"],
        &["your", "instruction"],
        &["your", "instructions"],
    ];
    PHRASES.iter().any(|p| unit.has(p))
}

/// What only a person can give: a credential, a secret or sign-in, physical access, a Mac
/// restart, an OS grant or its dialog, access, money (credits, a budget, a quota), customers,
/// mail, production.
fn person_only(unit: &Unit) -> bool {
    person_only_words(unit).any(|(i, sigil)| sigil || counts(unit, i))
}

/// The indexes of the words [`person_only`] reads, each with whether it is a `$` word or a
/// `.env`, which [`person_only`] reads whatever stands before it.
fn person_only_words(unit: &Unit) -> impl Iterator<Item = (usize, bool)> + '_ {
    const WORDS: &[&str] = &[
        "credential",
        "secret",
        "dialog",
        "automation",
        "camera",
        "microphone",
        "credit",
        "budget",
        "quota",
        "billable",
        "password",
        "passphrase",
        "passcode",
        "keychain",
        "login",
        "signin",
        "re-login",
        "relogin",
        "2fa",
        "mfa",
        "otp",
        "captcha",
        "oauth",
        "token",
        "pat",
        "physical",
        "physically",
        "cable",
        "usb",
        "yubikey",
        "accessibility",
        "privacy",
        "tcc",
        "sudo",
        "administrator",
        "admin",
        "admins",
        "access",
        "collaborator",
        "role",
        "roles",
        "scopes",
        "billing",
        "invoice",
        "subscription",
        "license",
        "licence",
        "customer",
        "email",
        "production",
        "prod",
        "certificate",
        "bought",
        "paid",
        "spent",
        "cost",
        "bills",
        "1password",
        "bitwarden",
        "lastpass",
    ];
    const INFLECTED: &[&str] = &[
        "authenticate",
        "click",
        "grant",
        "plug",
        "reboot",
        "pay",
        "purchase",
        "buy",
        "spend",
        "announce",
    ];
    const PHRASES: &[&[&str]] = &[
        &["api", "key"],
        &["ssh", "key"],
        &["signing", "key"],
        &["deploy", "key"],
        &["private", "key"],
        &["log", "in"],
        &["logs", "in"],
        &["logged", "in"],
        &["one-time", "code"],
        &["verification", "code"],
        &["security", "key"],
        &["touch", "id"],
        &["face", "id"],
        &["restart", "the", "mac"],
        &["restart", "your", "mac"],
        &["restart", "this", "mac"],
        &["power", "cycle"],
        &["full", "disk", "access"],
        &["screen", "recording"],
        &["system", "settings"],
        &["permission", "denied"],
        &["not", "permitted"],
        &["credit", "card"],
        &["your", "signature"],
        &["refund", "the"],
        &["refund", "them"],
        &["refund", "it"],
    ];
    unit.words.iter().enumerate().filter_map(|(i, (at, w))| {
        let sigil = (w.contains('$') && w.len() > 1)
            || (w.starts_with("env") && unit.lower[..*at].ends_with('.'));
        let w = w.as_str();
        let plural = w.strip_suffix('s').unwrap_or(w);
        let hit = sigil
            || WORDS.contains(&w)
            || WORDS.contains(&plural)
            || inflects_any(w, INFLECTED)
            || PHRASES.iter().any(|p| unit.at(i, p))
            || (matches!(w, "sign" | "signs") && (i + 1..i + 5).any(|k| unit.word(k) == "in"))
            || (matches!(w, "permission" | "permissions") && consent_at(unit, i).is_none());
        hit.then_some((i, sigil))
    })
}

/// A precondition only the person can meet: `after`, `once`, `when` or `if` a person signs,
/// logs in, reviews, reads, approves, decides…
fn person_first(unit: &Unit) -> bool {
    const ACTS: &[&str] = &[
        "sign", "log", "enter", "review", "read", "look", "accept", "approve", "grant", "plug",
        "confirm", "decide", "choose", "pick",
    ];
    const WHEN: &[&[&str]] = &[
        &["after"],
        &["once"],
        &["when"],
        &["if"],
        &["as", "soon", "as"],
    ];
    const WHO: &[&[&str]] = &[
        &["you"],
        &["you've"],
        &["you'll"],
        &["the", "owner"],
        &["a", "person"],
        &["the", "person"],
        &["someone"],
        &["somebody"],
        &["the", "user"],
        &["they"],
        &["they've"],
    ];
    (0..unit.len()).any(|i| {
        let Some(when) = slot(unit, i, WHEN) else {
            return false;
        };
        let Some(who) = slot(unit, i + when, WHO) else {
            return false;
        };
        let mut at = skip_adverb(unit, i + when + who);
        if matches!(unit.word(at), "have" | "has") {
            at += 1;
        }
        // A light verb before the act (`once you've had a look`, `after you take a look`, `once
        // you've given it a read`).
        if LIGHT.contains(&unit.word(at)) {
            at += 1;
            for _ in 0..2 {
                if FILLERS.contains(&unit.word(at)) {
                    at += 1;
                }
            }
        }
        let act = unit.word(at);
        inflects_any(act, ACTS) || matches!(act, "chose" | "chosen" | "picked")
    }) || reviewed_first(unit)
        || (unit.any(&["yourself", "yourselves"])
            && !unit.any(&["rather", "prefer", "or", "unless", "want", "like"]))
        || (0..unit.len()).any(|i| {
            // `you need to choose between the two schemas`, `you have to pick the theme`.
            unit.word(i) == "you"
                && matches!(unit.word(i + 1), "need" | "needs" | "have" | "must")
                && {
                    let at = i + 2 + usize::from(unit.word(i + 2) == "to");
                    inflects_any(unit.word(at), &["choose", "pick", "decide"])
                }
        })
}

/// The light verbs a person's act may follow (`had a look`, `take a look`, `give it a read`).
const LIGHT: &[&str] = &[
    "have", "has", "had", "take", "takes", "took", "taken", "give", "gives", "gave", "given",
    "get", "gets", "got", "gotten", "do", "did", "done",
];

/// The words between a [`LIGHT`] verb and the act (`a quick look`, `it a final read`).
const FILLERS: &[&str] = &[
    "a", "an", "the", "it", "another", "quick", "final", "second", "closer", "proper", "last",
];

/// A review asked of the person, a precondition only they meet: `ready for your review`,
/// `pending review`, `after your look`, `please review the diff`.
fn reviewed_first(unit: &Unit) -> bool {
    const REVIEWS: &[&str] = &["review", "look", "read", "check"];
    const ASKS: &[&str] = &[
        "review", "look", "check", "read", "confirm", "approve", "sign", "verify",
    ];
    (0..unit.len()).any(|i| {
        let w = unit.word(i);
        let next = unit.word(i + 1);
        let after_next = unit.word(i + 2);
        let hit = (matches!(
            w,
            "for" | "after" | "once" | "following" | "pending" | "with" | "upon"
        ) && (REVIEWS.contains(&next)
            || (next == "your" && REVIEWS.contains(&after_next))))
            || (w == "please" && ASKS.contains(&next));
        hit && counts(unit, i)
    })
}

/// Git's forms that destroy work no [`DESTRUCTIVE_WORDS`] word names, read on the lowercased
/// text (the words drop a flag's dashes).
const GIT_DESTROYS: &[&str] = &[
    "clean -f",
    "clean -x",
    "clean -d",
    "checkout -- ",
    "checkout .",
    "restore .",
    "restore --",
    "branch -d",
    "filter-repo",
    "filter-branch",
    "stash clear",
    "update-ref -d",
    "reflog expire",
];

/// The verbs of outward work in any inflection ([`outward`]).
const OUTWARD_INFLECTED: &[&str] = &[
    "promote",
    "tag",
    "ship",
    "upload",
    "send",
    "post",
    "notify",
    "rotate",
    "revoke",
    "retire",
    "deprecate",
    "archive",
    "yank",
    "unpublish",
    "launch",
    "notarize",
    "codesign",
    "sign",
    "prune",
    "deinit",
    "amend",
    "squash",
    "kill",
    "pkill",
    "signal",
    "terminate",
    "quit",
];

/// The words whose every form is outward work (`deployment`, `published`, `releases`).
const OUTWARD_PREFIXES: &[&str] = &["deploy", "publish", "release"];

/// [`DESTRUCTIVE_WORDS`] as the runs of words [`outward`] looks for, a flag's dashes dropped: all
/// but `push -f`, which [`force_push`] reads, and `drop`, which takes an object.
static DESTRUCTIVE_RUNS: std::sync::LazyLock<Vec<Vec<&'static str>>> =
    std::sync::LazyLock::new(|| {
        DESTRUCTIVE_WORDS
            .iter()
            .filter(|d| !matches!(**d, "push -f" | "drop"))
            .map(|d| d.split(' ').map(|p| p.trim_start_matches('-')).collect())
            .collect()
    });

/// Outward or irreversible work: [`DESTRUCTIVE_WORDS`], [`GIT_DESTROYS`] and what leaves the
/// machine or ends a process (`kill`, `aterm ctl end`, `end`, `stop` or `shut down` before a
/// process noun: [`ends_a_process`]); the allow list's own cleanup is the only exception.
fn outward(unit: &Unit, says_at: Option<usize>) -> bool {
    const WORDS: &[&str] = &[
        "sent",
        "gc",
        "gh-pages",
        "website",
        "hosting",
        "domain",
        "dns",
        "terraform",
        "public",
        "registry",
        "registries",
        "pypi",
        "rubygems",
        "ghcr",
        "dockerhub",
        "rollout",
        "rollouts",
        "backfill",
        "backfills",
        "backfilling",
        "cutover",
        "canary",
        "hosts",
        "servers",
        "cluster",
        "clusters",
        "kubectl",
        "helm",
    ];
    const PHRASES: &[&[&str]] = &[
        &["message", "the"],
        &["message", "them"],
        &["comment", "on"],
        &["open", "a", "pr"],
        &["open", "a", "pull", "request"],
        &["open", "an", "issue"],
        &["file", "an", "issue"],
        &["rename", "the", "repo"],
        &["rename", "a", "repo"],
        &["go", "live"],
        &["close", "the", "tab"],
        &["close", "a", "tab"],
        &["close", "this", "tab"],
        &["close", "the", "window"],
        &["restart", "aterm"],
        &["relaunch", "aterm"],
        &["ctl", "end"],
        &["apply", "the", "plan"],
        &["host", "it"],
        &["host", "the"],
        &["roll", "out"],
        &["rolls", "out"],
        &["rolling", "out"],
        &["rolled", "out"],
        &["cut", "over"],
        &["to", "the", "app", "store"],
        &["to", "the", "play", "store"],
        &["to", "testflight"],
        &["on", "for", "all", "users"],
        &["out", "to", "all", "users"],
        &["on", "for", "every", "user"],
        &["out", "to", "every", "user"],
        &["against", "the", "live", "database"],
        &["on", "the", "live", "database"],
        &["against", "the", "production", "database"],
        &["on", "the", "production", "database"],
        &["no", "snapshot", "to", "go"],
        &["no", "snapshot", "to", "restore"],
        &["no", "snapshot", "to", "roll"],
        &["no", "backup", "to", "go"],
        &["no", "backup", "to", "restore"],
        &["no", "backup", "to", "roll"],
        &["transfer", "ownership"],
        &["mail", "the"],
        &["mail", "it"],
        &["mail", "everyone"],
        &["file", "a", "bug"],
        &["file", "the", "bug"],
        &["feedback", "assistant"],
    ];
    if force_push(&unit.lower)
        || unit.lower.contains("push --force")
        || unit.lower.contains("/applications")
        || GIT_DESTROYS.iter().any(|g| unit.lower.contains(g))
    {
        return true;
    }
    let cleanup = own_cleanup(unit);
    // A report of what was done (`the build was stopped`), never one to come (`can be deleted`,
    // `will be purged`).
    let passive = |i: usize| {
        let reports = |w: &str| matches!(w, "was" | "were" | "been" | "got");
        reports(unit.before(i, 1)) || (unit.before(i, 1) == "already" && reports(unit.before(i, 2)))
    };
    (0..unit.len()).any(|i| {
        if cleanup.contains(&i) || says_at == Some(i) || !counts(unit, i) || passive(i) {
            return false;
        }
        let w = unit.word(i);
        let destructive = (inflects(w, "drop") && dropped_object(unit.word(i + 1)))
            || DESTRUCTIVE_RUNS.iter().any(|want| {
                want.iter()
                    .enumerate()
                    .all(|(k, base)| inflects(unit.word(i + k), base))
            });
        destructive
            || inflects_any(w, OUTWARD_INFLECTED)
            || WORDS.contains(&w)
            || OUTWARD_PREFIXES.iter().any(|p| w.starts_with(p))
            || PHRASES.iter().any(|p| unit.at(i, p))
            || (inflects(w, "push") && pushes_elsewhere(unit, i))
            // `message Alice`, `email the reviewer`, `respond to the reviewer`, `invite Bob`: a
            // word to someone else (`the message author field` is code).
            || (inflects_any(w, &["message", "email", "ping", "dm", "invite"])
                && addressed(unit, i + 1))
            || (inflects_any(w, &["respond", "reply"])
                && unit.word(i + 1) == "to"
                && addressed(unit, i + 2))
            || ends_a_process(unit, i)
    })
}

/// Whether word `k` addresses someone: a name (`Alice`), a handle (`@sam`), or past an article a
/// [`PEOPLE_NOUNS`] or [`PARTIES`] word but the person, one modifier allowed (`the reviewer`,
/// `the platform team`; `the Trust toolchain` is a thing).
fn addressed(unit: &Unit, k: usize) -> bool {
    let party = |k: usize| {
        let w = unit.word(k);
        let one = w.trim_end_matches('s');
        (PEOPLE_NOUNS.contains(&one) || PARTIES.contains(&one)) && !THE_PERSON.contains(&w)
    };
    if matches!(unit.word(k), "the" | "a" | "an") {
        return party(k + 1) || party(k + 2);
    }
    named(unit, k)
        || unit
            .words
            .get(k)
            .is_some_and(|(at, _)| unit.lower[..*at].ends_with('@'))
}

/// Whether word `i` is `end`, `stop` or `shut down` (`shutdown`), in any form (`ending`,
/// `stopped`, `shutting down`), before a process noun ([`PROCESS_NOUNS`]): past its determiners
/// (`the`, `my`, `those`, …), within the next three words, its clause unbroken (`end the leftover
/// processes`, `ending my stale dev server`, `stop the preview server`), no preposition between.
/// Ending a process is never undone, so a question about one gets no standing go-ahead. `end the
/// day`, `the end of the job` and `stop the build` end none.
fn ends_a_process(unit: &Unit, i: usize) -> bool {
    const DETERMINERS: &[&str] = &[
        "the", "a", "an", "my", "our", "its", "this", "that", "these", "those", "every", "each",
        "all", "both", "any",
    ];
    const ENDS: &[&str] = &[
        "and", "or", "then", "so", "once", "when", "if", "before", "after", "of", "in", "on",
        "for", "to", "with", "at", "by",
    ];
    let w = unit.word(i);
    let object = if inflects_any(w, &["end", "stop", "shutdown"]) {
        i + 1
    } else if inflects(w, "shut") && unit.word(i + 1) == "down" {
        i + 2
    } else {
        return false;
    };
    let from = (object..unit.len())
        .find(|&k| !DETERMINERS.contains(&unit.word(k)))
        .unwrap_or(unit.len());
    (from..(from + 3).min(unit.len()))
        .take_while(|&k| !ENDS.contains(&unit.word(k)))
        .any(|k| PROCESS_NOUNS.contains(&unit.word(k)))
}

/// The nouns of a process `end` may take ([`ends_a_process`]).
const PROCESS_NOUNS: &[&str] = &[
    "process",
    "processes",
    "leftover",
    "leftovers",
    "watcher",
    "watchers",
    "server",
    "servers",
    "job",
    "jobs",
    "daemon",
    "daemons",
    "pid",
    "pids",
];

/// Whether the push at word `i` names a destination that is not the agent's branch or main:
/// `to <X>` in its clause within six words, X (past a determiner) not main, master, origin, the
/// remote, `it`, nor a branch or worktree, or such a destination with a place after it in the
/// clause (`on`, `of` or `in` <Y>) that is not origin, the remote, this repo or its tip. `push 1.4.0
/// to crates.io`, `push the image to the registry` and `push my branch to main on
/// alabsystems/aterm` publish; a bare push (`I'll push as soon as I get the go-ahead`), `push it
/// to main` and `push it to main on origin` do not.
fn pushes_elsewhere(unit: &Unit, i: usize) -> bool {
    const ENDS: &[&str] = &[
        "and", "or", "then", "so", "once", "when", "if", "before", "after", "as", "but", "until",
    ];
    const DETERMINERS: &[&str] = &["the", "my", "our", "its", "this", "that"];
    let Some((from, _)) = unit.words.get(i) else {
        return false;
    };
    // Whether word `k` is still in the push's clause: no mark since the push.
    let in_clause = |k: usize| {
        unit.words
            .get(k)
            .is_some_and(|(at, _)| !unit.lower[*from..*at].contains([',', ';', ':', '(']))
    };
    let Some(to) = (i + 1..(i + 7).min(unit.len())).find(|&k| unit.word(k) == "to" && in_clause(k))
    else {
        return false;
    };
    if (i + 1..to).any(|k| ENDS.contains(&unit.word(k))) {
        return false;
    }
    let past = |k: usize| {
        (k..unit.len())
            .find(|&k| !DETERMINERS.contains(&unit.word(k)))
            .unwrap_or(unit.len())
    };
    let at = past(to + 1);
    let own = matches!(
        unit.word(at),
        "main" | "master" | "origin" | "remote" | "it" | "branch" | "branches" | ""
    ) || (at..at + 4).any(|k| matches!(unit.word(k), "branch" | "worktree"));
    if !own {
        return true;
    }
    // The rest of the clause: a place the destination is on that is not the agent's own remote.
    (at + 1..unit.len())
        .take_while(|&k| in_clause(k) && !ENDS.contains(&unit.word(k)))
        .any(|k| {
            matches!(unit.word(k), "on" | "of" | "in")
                && !matches!(
                    unit.word(past(k + 1)),
                    "origin"
                        | "remote"
                        | "repo"
                        | "repository"
                        | "here"
                        | "top"
                        | "tip"
                        | "head"
                        | ""
                )
        })
}

/// Whether a `drop` takes `next` as its object (`drop the table`), not a fall (`memory dropped
/// below`).
fn dropped_object(next: &str) -> bool {
    matches!(
        next,
        "the"
            | "a"
            | "an"
            | "this"
            | "that"
            | "these"
            | "those"
            | "my"
            | "your"
            | "its"
            | "all"
            | "it"
            | "them"
            | "table"
            | "tables"
            | "database"
            | "column"
            | "stash"
            | "commit"
            | "commits"
            | "branch"
    )
}

/// The words that make a clause naming another session's work a wait for room, not their work
/// (`disk room, which other sessions' build caches are holding`).
const ROOM_NOUNS: &[&str] = &[
    "disk", "memory", "room", "space", "swap", "cache", "caches", "cores", "ram", "floor",
];

/// The verbs that make such a clause a wait for room only after the other session's words
/// (`let the other sessions' builds finish`), not before them, where they are the agent's own
/// work on the other session's (`finish the other session's merge`).
const ROOM_VERBS: &[&str] = &["finish", "finishes", "clear", "free", "frees"];

/// The work a possessor other than the agent can own (`your feature branch`, `Alice's PR`).
const WORK_NOUNS: &[&str] = &[
    "pr",
    "prs",
    "branch",
    "branches",
    "worktree",
    "worktrees",
    "change",
    "changes",
    "commit",
    "commits",
    "patch",
    "patches",
    "work",
    "fork",
    "checkout",
    "stash",
    "repo",
    "repository",
    "diff",
    "edits",
];

/// The people, and the other agents, sessions and tabs, whose possessive names their work (`the
/// owner's branch`, `the other agent's branch`) or their consent (`the reviewer's approval`), as
/// does a consent's source (`approval from the security team`).
const PEOPLE_NOUNS: &[&str] = &[
    "owner",
    "person",
    "user",
    "worker",
    "colleague",
    "teammate",
    "team",
    "maintainer",
    "contributor",
    "reviewer",
    "approver",
    "author",
    "human",
    "agent",
    "session",
    "tab",
];

/// The words that name the person a consent comes from (`the owner's`, `from you`), as `your`
/// and `their` do before it.
const THE_PERSON: &[&str] = &["you", "owner", "person", "user", "human"];

/// The capitalised words that name no owner (`Main's changes`). These structural exceptions
/// also apply in definite possessive and work-owner syntax; calendar prose does not.
const NOT_POSSESSORS: &[&str] = &[
    "main", "origin", "it", "that", "there", "here", "what", "who", "today", "aterm", "git",
];

/// Calendar words name no person in consent-source or deadline prose (`by Friday`, `by May`).
/// Definite ownership still keeps a person's name with that spelling (`MAY's branch`).
const CALENDAR_WORDS: &[&str] = &[
    "tomorrow",
    "tonight",
    "yesterday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// Whether word `k` names a possessor other than the agent: `your`, `their`, `his`, `her`, or
/// a person's possessive (`the owner's`, `Alice's`).
fn foreign(unit: &Unit, k: usize) -> bool {
    let Some((at, w)) = unit.words.get(k) else {
        return false;
    };
    if matches!(w.as_str(), "your" | "their" | "his" | "her") {
        return true;
    }
    let possessive = unit.lower[at + w.len()..].starts_with("'s")
        || (w.ends_with('s') && unit.lower[at + w.len()..].starts_with('\''));
    possessive && (named_owner(unit, k) || PEOPLE_NOUNS.contains(&w.trim_end_matches('s')))
}

/// A capitalised owner in possessive or work-owner syntax, including an uppercase name
/// (`ALICE's branch`, `the branch ALICE made`). Neither `I` nor a word [`NOT_POSSESSORS`]
/// names owns someone else's work. Acronym-like spelling cannot undo an ownership relation.
fn named_owner(unit: &Unit, k: usize) -> bool {
    let Some((at, w)) = unit.words.get(k) else {
        return false;
    };
    let said = unit.said_at(*at, at + w.len());
    let me = w == "i" || w.starts_with("i'");
    said.chars().next().is_some_and(char::is_uppercase)
        && !me
        && !NOT_POSSESSORS.contains(&w.as_str())
}

/// Whether word `k` names someone in source prose: a [`named_owner`] spelling, but not an
/// acronym (`EOD`, `PR`, a row in capitals) or a calendar word. Ownership has its own syntax
/// and keeps uppercase and calendar-spelled names; a deadline or ordinary source mention does
/// not establish that relation.
fn named(unit: &Unit, k: usize) -> bool {
    let Some((at, w)) = unit.words.get(k) else {
        return false;
    };
    let said = unit.said_at(*at, at + w.len());
    let acronym = said.chars().filter(|c| c.is_alphabetic()).count() > 1
        && !said.chars().any(char::is_lowercase);
    named_owner(unit, k) && !acronym && !CALENDAR_WORDS.contains(&w.as_str())
}

/// The subjects that make a clause right after a work noun its owner's (`the worktree you
/// made`, `the branch the other agent pushed`).
const OWNERS_AFTER: &[&[&str]] = &[
    &["you"],
    &["you've"],
    &["you'd"],
    &["you're"],
    &["they"],
    &["they've"],
    &["someone"],
    &["somebody"],
    &["the", "owner"],
    &["the", "person"],
    &["the", "user"],
    &["another", "session"],
    &["another", "agent"],
    &["another", "tab"],
    &["the", "other", "session"],
    &["the", "other", "agent"],
    &["the", "other", "tab"],
    &["the", "other", "sessions"],
    &["the", "other", "agents"],
    &["other", "sessions"],
    &["other", "agents"],
];

/// Whether the work noun at word `n` is followed by a clause whose subject is someone else and
/// a verb (`the worktree you made`, `the PR Alice opened`, `the branch that the other agent
/// pushed`): the work is theirs. The subject stands right after the noun, or after `that`,
/// `which` or `whom`, with no mark between (`remove the worktree. The build …` names no owner);
/// a capitalised name counts, `I` and the words [`NOT_POSSESSORS`] names do not.
fn owned_after(unit: &Unit, n: usize) -> bool {
    let at = n + 1 + usize::from(matches!(unit.word(n + 1), "that" | "which" | "whom"));
    // A new phrase names no owner, even in an uppercase reply: BRANCH TO MAIN, or BRANCH
    // BEFORE I RERUN. The owner slot is a subject, not a preposition or clause marker.
    if slot(unit, at, SLOT_WHEN).is_some()
        || matches!(
            unit.word(at),
            "and"
                | "or"
                | "to"
                | "so"
                | "in"
                | "on"
                | "at"
                | "into"
                | "for"
                | "from"
                | "with"
                | "by"
                | "of"
        )
    {
        return false;
    }
    let (Some((noun, w)), Some((subject, _))) = (unit.words.get(n), unit.words.get(at)) else {
        return false;
    };
    if unit.lower[noun + w.len()..*subject]
        .chars()
        .any(|c| !c.is_alphanumeric() && !c.is_whitespace())
    {
        return false;
    }
    let Some(len) = slot(unit, at, OWNERS_AFTER).or_else(|| named_owner(unit, at).then_some(1))
    else {
        return false;
    };
    let verb = unit.word(at + len);
    !verb.is_empty()
        && !matches!(
            verb,
            "and" | "or" | "to" | "so" | "once" | "when" | "if" | "then" | "say" | "says"
        )
}

/// Whether a work noun has a foreign possessor ([`foreign`]) before it, at most two words
/// between them and none a consent noun or a word that starts another phrase (`your go-ahead to
/// merge the branch` is the agent's branch), or an owner named after it ([`owned_after`]).
fn possessed(unit: &Unit) -> bool {
    const BETWEEN: &[&str] = &[
        "to", "on", "for", "of", "with", "my", "the", "a", "an", "this", "that", "its", "and",
        "or", "in", "at", "into", "before", "after",
    ];
    // A fix or a review is someone's by a possessive that names them (`Priya's fix`, `Dana's
    // review`; `your review` is the person's) or a `from` or `by` (`the fix from Priya`).
    const THEIRS: &[&str] = &["fix", "fixes", "review", "reviews"];
    (0..unit.len()).any(|i| {
        let w = unit.word(i);
        let named_possessor = THEIRS.contains(&w)
            && i.checked_sub(1).is_some_and(|k| {
                foreign(unit, k) && !matches!(unit.word(k), "your" | "their" | "his" | "her")
            });
        named_possessor
            || ((WORK_NOUNS.contains(&w) || THEIRS.contains(&w))
                && matches!(unit.word(i + 1), "from" | "by")
                && addressed(unit, i + 2))
            || WORK_NOUNS.contains(&w)
                && (owned_after(unit, i)
                    || (1..=3).any(|back| {
                        i.checked_sub(back).is_some_and(|k| {
                            foreign(unit, k)
                                && (k + 1..i).all(|j| {
                                    !BETWEEN.contains(&unit.word(j))
                                        && consent_at(unit, j).is_none()
                                })
                        })
                    }))
    })
}

/// Whether a consent noun the unit waits on ([`awaited`]) is someone else's to give: someone gives
/// it ([`given_by`]), its possessor is not the person ([`foreign`], at most one [`DETERMINERS`]
/// word between: `Alice's approval`, `the reviewer's final sign-off`; `your`, `their` as in
/// [`someone_else`], and a [`THE_PERSON`] word are the person's), or it has no possessor and
/// someone ([`someone`]) is its source: a `from` anywhere after it (`approval on my PR 812 from a
/// reviewer`, `approval to merge my branch into main from @alice`) or a `by` after it, before a
/// `to`, that is no passive's (`sign-off by a second reviewer`, `approval on my PR 812 by a
/// reviewer`; `the go-ahead to rebuild by the reviewer's deadline` and `before I rerun the checks
/// requested by the reviewer` are the work's). A consent the unit reports given (`my PR 812 has
/// Alice's approval`) waits on no one, and the person's stays theirs whatever follows it (`your
/// go-ahead by Friday`, `your go-ahead from Slack`).
fn consent_owned_elsewhere(unit: &Unit) -> bool {
    (0..unit.len()).any(|i| {
        let Some(n) = consent_at(unit, i).filter(|&n| awaited(unit, i, n)) else {
            return false;
        };
        if given_by(unit, i) {
            return true;
        }
        let owner = (1..=2)
            .filter_map(|back| i.checked_sub(back))
            .find(|&k| foreign(unit, k) && (k + 1..i).all(|j| DETERMINERS.contains(&unit.word(j))));
        if let Some(k) = owner {
            let w = unit.word(k);
            return !matches!(w, "your" | "their") && !THE_PERSON.contains(&w);
        }
        let source = |j: usize| (j + 1..j + 4).any(|k| someone(unit, k));
        let passive = |j: usize| {
            let w = unit.before(j, 1);
            (w.len() > 3 && w.ends_with("ed")) || matches!(w, "built" | "rebuilt" | "made" | "done")
        };
        (i + n..unit.len())
            .take_while(|&j| unit.word(j) != "to")
            .any(|j| unit.word(j) == "by" && !passive(j) && source(j))
            || (i + n..unit.len()).any(|j| unit.word(j) == "from" && source(j))
    })
}

/// Whether someone ([`someone`]) gives the consent noun at word `i`: a `give` before it, at
/// most two [`DETERMINERS`] words, `me` or `us` between, whose subject stands right before the
/// `give`, past a `to` and one auxiliary (`until the reviewer gives the go-ahead`, `the security
/// team to give the go-ahead`, `until Alice has given the OK`); `once you give the OK` is the
/// person's.
fn given_by(unit: &Unit, i: usize) -> bool {
    let Some(g) = (1..=3).filter_map(|back| i.checked_sub(back)).find(|&g| {
        let w = unit.word(g);
        (inflects(w, "give") || matches!(w, "gave" | "given"))
            && (g + 1..i).all(|j| {
                let w = unit.word(j);
                DETERMINERS.contains(&w) || matches!(w, "me" | "us")
            })
    }) else {
        return false;
    };
    let mut at = g;
    if unit.before(at, 1) == "to" {
        at -= 1;
    }
    if matches!(
        unit.before(at, 1),
        "has"
            | "have"
            | "had"
            | "will"
            | "would"
            | "can"
            | "could"
            | "should"
            | "must"
            | "does"
            | "did"
    ) {
        at -= 1;
    }
    at.checked_sub(1).is_some_and(|k| someone(unit, k))
}

/// Whether word `k` names someone other than the person: a [`PEOPLE_NOUNS`] word, plural too
/// (`the code owners`), that is not a [`THE_PERSON`] word as written nor one `this` points at
/// (`this tab`); a name past the unit's first word; or a handle (`@alice`).
fn someone(unit: &Unit, k: usize) -> bool {
    let Some((at, w)) = unit.words.get(k) else {
        return false;
    };
    (PEOPLE_NOUNS.contains(&w.trim_end_matches('s'))
        && !THE_PERSON.contains(&w.as_str())
        && unit.before(k, 1) != "this")
        || (k > 0 && named(unit, k))
        || unit.lower[..*at].ends_with('@')
}

/// The agent's acts on work, which make the work someone else's where their words are the act's
/// object or where it is (`clear the other session's build cache`, `stop tqn5r's build`,
/// `stop the build in the other session`), whatever room the act would free.
const ACTS_ON: &[&str] = &[
    "clear",
    "empty",
    "stop",
    "cancel",
    "pause",
    "free",
    "trim",
    "flush",
    "kill",
    "delete",
    "remove",
    "wipe",
    "purge",
    "prune",
    "reset",
    "restart",
    "interrupt",
    "finish",
    "touch",
];

/// Someone else's work or consent: another session's or agent's, the owner's, the person's
/// (`your branch`), a named person's, upstream's, a worker's, or a consent another gives
/// ([`consent_owned_elsewhere`]); skipped in a clause that waits on their use of room
/// ([`ROOM_NOUNS`], or a [`ROOM_VERBS`] verb after the other session's words), unless an act of
/// the agent's takes their words as its object ([`ACTS_ON`]: `clear the other session's build
/// cache`).
fn someone_else(unit: &Unit) -> bool {
    const WORDS: &[&str] = &[
        "upstream",
        "maintainer",
        "maintainers",
        "contributor",
        "contributors",
        "worker",
        "workers",
    ];
    if possessed(unit)
        || others_approve(unit)
        || (0..unit.len()).any(|i| {
            // `my teammate will rebase it`; `the teammate struct` is code.
            matches!(
                unit.word(i),
                "teammate" | "teammates" | "colleague" | "colleagues"
            ) && matches!(unit.before(i, 1), "my" | "our" | "your")
        })
        || unclaimed_request(unit)
        || bot_branch(unit)
        || consent_owned_elsewhere(unit)
    {
        return true;
    }
    const PHRASES: &[&[&str]] = &[
        &["another", "session"],
        &["other", "session"],
        &["other", "sessions"],
        &["another", "agent"],
        &["other", "agent"],
        &["other", "agents"],
        &["someone", "else"],
        &["somebody", "else"],
        &["belongs", "to"],
        &["belong", "to"],
        &["not", "mine"],
        &["isn't", "mine"],
        &["a", "worker"],
    ];
    unit.lower
        .split([',', ';', '(', ')', '\u{2014}', ':'])
        .any(|clause| {
            let c = Unit::new(clause, false);
            let others = (0..c.len()).find(|&i| {
                matches!(
                    c.word(i),
                    "session" | "sessions" | "agent" | "agents" | "tab" | "tabs"
                ) || session_id(c.word(i))
            });
            let waits =
                others.is_some_and(|k| (k..c.len()).any(|i| ROOM_VERBS.contains(&c.word(i))));
            let past = |mut k: usize| {
                while matches!(
                    c.word(k),
                    "the" | "a" | "an" | "all" | "that" | "those" | "these" | "any" | "some"
                ) {
                    k += 1;
                }
                k
            };
            let theirs_at = |k: usize| {
                matches!(c.word(k), "other" | "another")
                    || session_id(c.word(k))
                    || (foreign(&c, k) && c.word(k) != "your")
            };
            let acts_on_theirs = (0..c.len()).any(|i| {
                let k = past(i + 1);
                inflects_any(c.word(i), ACTS_ON)
                    && (theirs_at(k)
                        || (k + 1..(k + 4).min(c.len())).any(|p| {
                            matches!(c.word(p), "in" | "of" | "from" | "inside")
                                && theirs_at(past(p + 1))
                        }))
            });
            if acts_on_theirs {
                return true;
            }
            if c.any(ROOM_NOUNS) || waits {
                return false;
            }
            let owned = |who: &str| {
                (0..c.len()).any(|i| {
                    c.word(i) == who && consent_at(&c, i + 1).is_none() && c.word(i + 1) != "go"
                })
            };
            c.any(WORDS)
                || PHRASES.iter().any(|p| c.has(p))
                || (0..c.len()).any(|i| {
                    c.at(i, &["other", "team"]) || c.at(i, &["other", "teams"])
                } && counts(&c, i))
                || owned("their")
                || (clause.contains("owner's") && owned("owner"))
                || clause.contains("worker's")
                || c.words.iter().any(|(_, w)| session_id(w))
        })
}

/// The parties besides [`PEOPLE_NOUNS`] whose approval a reply can wait on (`the client hasn't
/// approved the mockups`).
const PARTIES: &[&str] = &[
    "client",
    "vendor",
    "manager",
    "compliance",
    "legal",
    "board",
    "lead",
    "codeowners",
    "marketing",
];

/// Whether someone other than the person has yet to approve, review, sign or agree: a name or a
/// party ([`PEOPLE_NOUNS`] but the person, [`PARTIES`]) before such a verb, pending by an
/// auxiliary between (at most four: `Marcus still has to approve the design`, `Ana has not
/// reviewed it`, `Lena asked to review it first`, `the security team must review it first`,
/// `until Sam has finished reviewing it`, `once Priya has had a look`) or by a condition before
/// (`after the client approves the copy`, `once Priya OKs it`, `once the security team gives the
/// green light`). A report of what code does is no approval (`the client accepts stale frames`),
/// nor is a name before a noun (`the GitHub review comment parser`) or a tool's state (`Clippy is
/// happy`: `happy` is a party's alone).
fn others_approve(unit: &Unit) -> bool {
    const BETWEEN: &[&str] = &[
        "still", "has", "have", "had", "to", "must", "needs", "need", "not", "hasn't", "haven't",
        "yet", "finished", "finishes", "is", "are", "isn't", "aren't", "will", "should", "first",
        "also", "gets", "got", "asked", "wants", "wanted", "take", "takes", "taken",
    ];
    const OPENS: &[&str] = &[
        "once", "after", "until", "till", "when", "unless", "provided", "but", "though",
    ];
    const APPROVES: &[&str] = &[
        "approve", "review", "sign", "agree", "confirm", "accept", "ok", "okay",
    ];
    (0..unit.len()).any(|i| {
        let w = unit.word(i);
        let one = w.trim_end_matches('s');
        let party =
            (PEOPLE_NOUNS.contains(&one) && !THE_PERSON.contains(&w)) || PARTIES.contains(&one);
        let name = named(unit, i) && !matches!(w, "please" | "ready" | "your" | "their");
        let possessive = unit.lower[unit.words[i].0 + w.len()..].starts_with('\'');
        if !(party || name) || possessive || unit.before(i, 1) == "you" {
            return false;
        }
        // The condition stands right before the subject, or before its article and at most one
        // modifier (`once the security team`).
        let conditioned = (1..=3).filter_map(|back| i.checked_sub(back)).any(|j| {
            OPENS.contains(&unit.word(j))
                && (j + 1..i)
                    .all(|m| (m == j + 1 && unit.word(m) == "the") || (m == j + 2 && party))
        });
        let mut k = i + 1;
        while k < i + 5 && BETWEEN.contains(&unit.word(k)) {
            k += 1;
        }
        let v = unit.word(k);
        let gives = inflects(v, "give") || matches!(v, "gave" | "given");
        let approves = (inflects_any(v, APPROVES) && !matches!(v, "approval" | "approvals"))
            || matches!(v, "oks" | "okays")
            || (party && v == "happy")
            || (k > i + 1 && matches!(v, "a" | "another") && unit.word(k + 1) == "look")
            || (gives && (k + 1..k + 4).any(|g| consent_at(unit, g).is_some()));
        approves && (k > i + 1 || conditioned)
    })
}

/// Whether `unit` names a pull request, patch or fork no `my` or `our` claims in the two words
/// before it (`the Dependabot PR`, `PR #412`, `the three open PRs`): someone else's until the
/// agent says it is its own (`my PR`).
fn unclaimed_request(unit: &Unit) -> bool {
    (0..unit.len()).any(|i| {
        let request = matches!(
            unit.word(i),
            "pr" | "prs" | "patch" | "patches" | "fork" | "forks" | "mr" | "mrs"
        ) || unit.at(i, &["pull", "request"])
            || unit.at(i, &["pull", "requests"]);
        request && !(1..=2).any(|back| matches!(unit.before(i, back), "my" | "our"))
    })
}

/// Whether `unit` (in its words or in the inline/fenced code it carries) names a bot-prefixed
/// branch (`renovate/lodash-4.x`, `dependabot/cargo/serde-1.0`, `snyk-fix-…`), optionally behind
/// `origin/`.
fn bot_branch(unit: &Unit) -> bool {
    const BOT_BRANCHES: &[&str] = &[
        "renovate/",
        "dependabot/",
        "depfu/",
        "snyk/",
        "snyk-fix-",
        "snyk-upgrade-",
        "greenkeeper/",
        "github-actions/",
        "allcontributors/",
        "imgbot/",
        "mergify/",
        "scala-steward/",
        "changeset-release/",
        "release-please--",
    ];
    let has_bot_ref = |text: &str| {
        text.split(|c: char| {
            c.is_whitespace() || matches!(c, ',' | ';' | ':' | '(' | ')' | '[' | ']' | '"' | '\'')
        })
        .any(|tok| {
            let r = tok.trim_matches(|c: char| matches!(c, '.' | '+' | '-' | '`'));
            let r = r
                .strip_prefix("refs/heads/")
                .or_else(|| r.strip_prefix("refs/remotes/"))
                .unwrap_or(r);
            let r = r.strip_prefix("remotes/").unwrap_or(r);
            let r = r.strip_prefix("origin/").unwrap_or(r);
            BOT_BRANCHES.iter().any(|p| {
                r.strip_prefix(p)
                    .is_some_and(|rest| rest.chars().any(char::is_alphanumeric))
            })
        })
    };
    let code_lower: String = unit.code.chars().map(lowered).collect();
    has_bot_ref(&unit.lower) || has_bot_ref(&code_lower)
}

/// Whether `word` (a word of the reply's lower-cased text) names another session: its full tab
/// ID (`tqn5r6e0148`), or the 5 characters aterm shows of one (`tqn5r`,
/// [`crate::tab_id::SHORT_LEN`]) where its 4 after the `t` hold both a digit and a letter. A
/// shown ID is `t` and 4 alphabet characters, and prose has words of that shape: English ones
/// (`there`, `taken`, `tests`), which hold no digit, and a `t` before a number (`t1000`, `t2026`),
/// which holds no letter. A shown ID usually holds both, so one that does not reads as prose: that
/// misses some shown IDs rather than reading prose as a session. A tab opened before the `t` form
/// keeps its `s-` ID, named in prose by 6 or more of its hex digits (`s-1a2b3c`).
fn session_id(word: &str) -> bool {
    use crate::tab_id::{ALPHABET, SHORT_LEN, is_tab_id};
    is_tab_id(word)
        || (word.len() == SHORT_LEN
            && word.strip_prefix('t').is_some_and(|rest| {
                rest.bytes().all(|b| ALPHABET.contains(&b))
                    && rest.bytes().any(|b| b.is_ascii_digit())
                    && rest.bytes().any(|b| b.is_ascii_lowercase())
            }))
        || word
            .strip_prefix("s-")
            .is_some_and(|hex| hex.len() >= 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A decline, a rejection or a deadlock: a go-ahead does not lift those. The agent's own refusal
/// to land this work (`I won't merge this into main, because…`, `I've decided not to push it`) is
/// one, unless the person's word is what it waits on (`I won't land it without your go-ahead`) or
/// it narrows what lands (`I won't commit the generated files, only the sources`).
fn declined(unit: &Unit) -> bool {
    const WORDS: &[&str] = &[
        "syspolicyd",
        "stuck",
        "deadlock",
        "deadlocked",
        "declined",
        "rejected",
        "denied",
        "hook",
        "hooks",
    ];
    const REFUSALS: &[&[&str]] = &[
        &["i", "won't"],
        &["we", "won't"],
        &["rather", "not"],
        &["decided", "not", "to"],
        &["don't", "think", "we", "should"],
        &["don't", "think", "i", "should"],
        &["not", "going", "to"],
        &["prefer", "not", "to"],
        &["don't", "want", "to"],
    ];
    let refuses = (0..unit.len()).any(|i| {
        REFUSALS.iter().any(|r| {
            unit.at(i, r)
                && inflects_any(unit.word(i + r.len()), LANDING)
                && matches!(
                    unit.word(i + r.len() + 1),
                    "this" | "it" | "that" | "these" | "them"
                )
        })
    }) && !unit.any(&[
        "without", "until", "till", "before", "unless", "only", "just", "yet", "own", "myself",
        "alone", "first",
    ]);
    refuses
        || unit.any(WORDS)
        || unit.has(&["not", "the", "user"])
        || unit.has(&["doesn't", "want", "to", "proceed"])
        || unit.has(&["does", "not", "want", "to", "proceed"])
}

/// A choice for the person (`which one`, `either … or …`, a question with ` or `), unless every
/// alternative is go on or wait.
fn chooses(unit: &Unit) -> bool {
    let question = unit.lower.trim_end().ends_with('?');
    let which = (0..unit.len()).any(|i| {
        unit.word(i) == "which"
            && (question
                || matches!(
                    unit.word(i + 1),
                    "one" | "option" | "options" | "would" | "do" | "should" | "of"
                ))
    });
    let or = unit.lower.rfind(" or ");
    let either = or.is_some() && (unit.any(&["either"]) || question);
    let waits = or.is_some_and(|at| {
        let after = unit.lower[at + 4..].trim_start();
        ["wait", "hold", "stop", "not ", "keep waiting", "leave it"]
            .iter()
            .any(|w| after.starts_with(w))
    });
    (which || either) && !waits
}

/// Whether `unit` echoes the answer (`consider it said`, `the standing go-ahead`, `per the
/// go-ahead`, `under the owner's go-ahead`) with no negation outside the echo: an agent acting on
/// it parks nothing, and one that argues against it (`the harness's "consider it said" is not the
/// person`) still parks.
fn echoes(unit: &Unit) -> bool {
    const PHRASES: &[&[&str]] = &[
        &["per", "the", "go-ahead"],
        &["as", "the", "go-ahead"],
        &["the", "go-ahead", "says"],
        &["consider", "it", "said"],
        &["standing", "go-ahead"],
        &["go-ahead", "stands"],
        &["that", "ask"],
        &["got", "the", "go-ahead"],
        &["have", "the", "go-ahead"],
        &["had", "the", "go-ahead"],
        &["given", "the", "go-ahead"],
        &["gave", "the", "go-ahead"],
        &["got", "your", "go-ahead"],
        &["have", "your", "go-ahead"],
        &["proceed", "now", "with", "your", "own", "work"],
        &["never", "a", "needs", "you", "item"],
        &["needs", "no", "one", "go-ahead"],
        &["proceed", "now", "with", "your", "own", "reversible"],
        &["grants", "nothing", "outward"],
        &["lifts", "no", "hold"],
        &["how", "dare", "you", "block", "on", "me"],
    ];
    if unit.lower.contains("[aterm harness]") {
        return !unit.negated();
    }
    // The answer's own go-ahead named as acted under (`per the owner's go-ahead`, `with the
    // harness's go-ahead`): the possessive's `'s` is no word here.
    let under = |i: usize| {
        matches!(unit.word(i), "per" | "under" | "following" | "with" | "on")
            && unit.word(i + 1) == "the"
            && matches!(
                unit.word(i + 2),
                "owner" | "owners" | "person" | "harness" | "standing"
            )
            && unit.word(i + 3) == "go-ahead"
    };
    (0..unit.len()).any(|i| {
        (under(i) && !unit.negated_outside(i..i + 4))
            || PHRASES
                .iter()
                .any(|p| unit.at(i, p) && !unit.negated_outside(i..i + p.len()))
    })
}

/// The words of memory, disk and room.
const ROOM_WORDS: &[&str] = &[
    "memory",
    "low-memory",
    "disk",
    "room",
    "swap",
    "space",
    "free-space",
    "floor",
    "ram",
];

/// Whether `unit` names memory, disk or room: the answer says to wait for room first.
fn names_room(unit: &Unit) -> bool {
    unit.any(ROOM_WORDS)
}

#[cfg(any(test, feature = "testing"))]
#[path = "go_ahead_tests.rs"]
pub mod testing;
