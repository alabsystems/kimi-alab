// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

use super::*;

#[test]
fn a_needs_line_is_read_under_every_list_marker_and_emphasis() {
    for line in [
        "NEEDS YOU: the release credential",
        "- NEEDS YOU: the release credential",
        "* NEEDS YOU: the release credential",
        "+ NEEDS YOU: the release credential",
        "• NEEDS YOU: the release credential",
        "1. NEEDS YOU: the release credential",
        "2) NEEDS YOU: the release credential",
        "**NEEDS YOU:** the release credential",
        "**NEEDS YOU**: the release credential",
        "__NEEDS YOU:__ the release credential",
        "- **NEEDS YOU:** the release credential",
        "*NEEDS YOU:* the release credential",
        "**NEEDS YOU: the release credential**",
        "  needs you: the release credential",
        "Needs You : the release credential",
    ] {
        assert_eq!(
            needs_you(&format!("Landed the parser.\n{line}")),
            ["the release credential"],
            "{line:?}"
        );
    }
}

#[test]
fn a_needs_line_in_a_fence_or_named_as_a_thing_is_no_need() {
    for tail in [
        "```\nNEEDS YOU: the release credential\n```",
        "~~~\nNEEDS YOU: the release credential\n~~~",
        "The primer says a NEEDS YOU: line reaches the person.",
        "NEEDS YOUR review, as the brief says.",
        "“NEEDS YOU:” lines reach the person.",
        "⎿ NEEDS YOU: the release credential",
        "Nothing left: no NEEDS YOU, no open work.",
        "See `done. NEEDS YOU: x` in the brief.",
    ] {
        assert!(needs_you(tail).is_empty(), "{tail:?}");
        assert_eq!(without_needs(tail), tail, "{tail:?}");
    }
    assert_eq!(
        needs_you("```\nlog\n```\nNEEDS YOU: a decision on the schema"),
        ["a decision on the schema"]
    );

    for (open, inner, close) in [
        ("~~~~markdown", "```", "~~~~"),
        ("````markdown", "~~~", "````"),
        ("````markdown", "```", "````"),
        ("~~~~markdown", "~~~", "~~~~"),
        ("```markdown", "``` not a close", "```"),
        ("~~~markdown", "~~~ not a close", "~~~"),
        ("```markdown", "~~~", "````"),
    ] {
        let tail = format!(
            "{open}\n{inner}\nNEEDS YOU: the release credential\n\
             Nothing left: all of it is on origin/main.\n{close}"
        );
        assert!(needs_you(&tail).is_empty(), "{tail:?}");
        assert_eq!(without_needs(&tail), tail, "{tail:?}");
        assert_eq!(defense_line(&tail), None, "{tail:?}");
        assert_eq!(defense_body(&tail), tail, "{tail:?}");

        let outside = format!(
            "{tail}\nNEEDS YOU: a decision on the schema\n\
             Nothing left: all of it is on origin/main."
        );
        assert_eq!(needs_you(&outside), ["a decision on the schema"]);
        assert_eq!(
            defense_line(&outside),
            Some("Nothing left: all of it is on origin/main.")
        );
    }
}

#[test]
fn a_needs_line_quoted_ticked_or_inside_a_sentence_is_a_need() {
    for tail in [
        "`NEEDS YOU: the release credential`",
        "> NEEDS YOU: the release credential",
        "▎ NEEDS YOU: the release credential",
        "> **NEEDS YOU:** the release credential",
        "`NEEDS YOU:` the release credential",
        "NEED YOU: the release credential",
        "NEEDS YOU... the release credential",
        "One thing NEEDS YOU: the release credential",
        "- `release`: NEEDS YOU: the release credential",
    ] {
        assert_eq!(needs_you(tail), ["the release credential"], "{tail:?}");
        assert!(!says_no_needs(tail), "{tail:?}");
    }
    assert_eq!(
        needs_you("Nothing left (NEEDS YOU: the TCC grant)."),
        ["the TCC grant)."]
    );
}

#[test]
fn needs_are_one_a_line_each_once_and_the_items_under_a_bare_head() {
    let tail = "Done with the parser.\nNEEDS YOU: the release credential\nNEEDS YOU: a \
                decision on the schema\nNEEDS YOU: the release credential";
    assert_eq!(
        needs_you(tail),
        ["the release credential", "a decision on the schema"]
    );
    let bare = "NEEDS YOU:\n\n- the release credential\n- a decision on the schema\nAll else \
                is landed.";
    assert_eq!(
        needs_you(bare),
        ["the release credential", "a decision on the schema"]
    );
    assert_eq!(without_needs(bare), "All else is landed.");
}

#[test]
fn a_head_that_names_its_need_is_one_need_over_any_list() {
    let clean = "Nothing left: the parser is committed and pushed to origin/main.";
    let mut tall = String::from("Here is the landing report.\nNEEDS YOU: rotate the deploy key\n");
    for i in 1..=80 {
        tall.push_str(&format!("- check {i} passed\n"));
    }
    tall.push_str(clean);
    assert_eq!(needs_you(&tall), ["rotate the deploy key"]);
    assert!(!says_no_needs(&tall));
    let kept = without_needs(&tall);
    assert_eq!(
        kept.lines().filter(|l| l.starts_with("- check ")).count(),
        80
    );
    assert!(!kept.contains("NEEDS YOU"), "{kept}");
    assert_eq!(defense_line(&tall), Some(clean));
    assert_eq!(
        needs_you(
            "NEEDS YOU: rotate the deploy key\nNEEDS YOU: approve the release\n- check 1 passed"
        ),
        ["rotate the deploy key", "approve the release"]
    );
    assert_eq!(
        needs_you("NEEDS YOU: two things:\n- the deploy key\n- the release approval"),
        ["the deploy key", "the release approval"]
    );
    assert_eq!(
        needs_you("NEEDS YOU:\n- the deploy key\n- the release approval"),
        ["the deploy key", "the release approval"]
    );
    assert_eq!(
        needs_you("NEEDS YOU: none\n- the signing key"),
        ["the signing key"]
    );
}

#[test]
fn a_bare_head_names_the_paragraph_under_it_or_a_need_it_cannot_read() {
    let defense = "Nothing left: every branch is merged into the remote main branch and pushed.";
    for (said, want) in [
        (
            "NEEDS YOU:\nThe release needs your signing credential to publish.",
            "The release needs your signing credential to publish.",
        ),
        (
            "NEEDS YOU:\n\nYour signing credential, to publish the\nrelease to the mirror.",
            "Your signing credential, to publish the release to the mirror.",
        ),
        (
            "**NEEDS YOU:**\nThe signing credential for the release.",
            "The signing credential for the release.",
        ),
        (
            "NEEDS YOU: nothing\nNEEDS YOU:\nThe signing credential for the release.",
            "The signing credential for the release.",
        ),
    ] {
        for tail in [format!("{defense}\n{said}"), format!("{said}\n{defense}")] {
            assert_eq!(needs_you(&tail), [want], "{tail:?}");
            assert!(!says_no_needs(&tail), "{tail:?}");
            assert_eq!(without_needs(&tail).trim(), defense, "{tail:?}");
        }
    }
    for tail in [
        format!("{defense}\nNEEDS YOU:"),
        format!("{defense}\nNEEDS YOU:\n\n"),
        format!("NEEDS YOU:\n{defense}"),
        format!("{defense}\nNEEDS YOU:\n```\nlog\n```"),
        format!("{defense}\nNEEDS YOU: ***"),
        format!("{defense}\nNEEDS YOU: \u{200B}\u{202E}"),
        format!("{defense}\nNEEDS YOU:\n- ***"),
    ] {
        assert_eq!(needs_you(&tail), [UNREAD_NEED], "{tail:?}");
        assert!(!says_no_needs(&tail), "{tail:?}");
        assert_eq!(defense_line(&tail), Some(defense), "{tail:?}");
    }
    assert!(says_no_needs("Landed.\nNEEDS YOU:\nNothing."));
    assert!(says_no_needs("Landed.\nNEEDS YOU:\n- none"));
    for tail in [
        format!("{defense}\n> NEEDS YOU:\nThe signing credential."),
        format!("{defense}\n`NEEDS YOU:`\nThe signing credential."),
    ] {
        assert_eq!(needs_you(&tail), ["The signing credential."], "{tail:?}");
    }
    assert_eq!(
        needs_you("NEEDS YOU:\n- the credential\nAll else is landed."),
        ["the credential"]
    );
}

#[test]
fn a_heading_or_a_dash_opens_either_head() {
    for line in [
        "## NEEDS YOU: the signing credential for the release",
        "### NEEDS YOU — the signing credential for the release",
        "NEEDS YOU — the signing credential for the release",
        "NEEDS YOU – the signing credential for the release",
        "NEEDS YOU - the signing credential for the release",
        "**NEEDS YOU** — the signing credential for the release",
    ] {
        assert_eq!(
            needs_you(&format!("Landed.\n{line}")),
            ["the signing credential for the release"],
            "{line:?}"
        );
    }
    for tail in [
        "## Nothing left:\nEverything is merged into origin/main and pushed.",
        "### Nothing left\nEverything is merged into origin/main and pushed.",
        "Nothing left — everything is merged into origin/main and pushed.",
        "Nothing left to do: everything is merged into origin/main and pushed.",
        "Nothing left to land: everything is merged into origin/main and pushed.",
        "**Nothing left**: everything is merged into origin/main and pushed.",
    ] {
        assert!(defense_line(tail).is_some(), "{tail:?}");
    }
    for tail in [
        "Nothing leftover: the cache is warm.",
        "Nothing left-handed here.",
        "Nothing left in the cache, so it rebuilt.",
        "#Nothing left: not a heading without its space",
    ] {
        assert_eq!(defense_line(tail), None, "{tail:?}");
    }
    for tail in [
        "NEEDS YOURSELF: a break",
        "NEEDS YOU-know-what",
        "NEEDS YOU'RE here",
    ] {
        assert!(needs_you(tail).is_empty(), "{tail:?}");
    }
}

#[test]
fn a_wrapped_need_is_joined_and_a_new_sentence_is_not() {
    let wrapped = "NEEDS YOU: the owner's release credential for the staging\nmirror and the \
                   signing key\nEverything else is on main.";
    assert_eq!(
        needs_you(wrapped),
        ["the owner's release credential for the staging mirror and the signing key"]
    );
    assert_eq!(without_needs(wrapped), "Everything else is on main.");
    let stops = "NEEDS YOU: the release credential.\nthen the tag goes out";
    assert_eq!(needs_you(stops), ["the release credential."]);
    assert_eq!(without_needs(stops), "then the tag goes out");
}

#[test]
fn a_dashed_or_qualified_head_still_names_a_need() {
    for line in [
        "NEEDS YOU — the release credential",
        "NEEDS YOU – the release credential",
        "NEEDS YOU - the release credential",
        "NEEDS YOU—the release credential",
        "**NEEDS YOU** — the release credential",
        "NEEDS YOU (owner): the release credential",
        "**NEEDS YOU (owner):** the release credential",
        "- NEEDS YOU (Andrew) — the release credential",
    ] {
        assert_eq!(
            needs_you(&format!("Landed the parser.\n{line}")),
            ["the release credential"],
            "{line:?}"
        );
    }
    assert!(needs_you("NEEDS YOU-related notes are in the brief.").is_empty());
}

#[test]
fn a_need_is_sanitized_and_cut() {
    let long = format!("NEEDS YOU: {}", "word ".repeat(80));
    let need = &needs_you(&long)[0];
    assert!(need.chars().count() <= NEED_CHARS, "{need}");
    assert!(need.ends_with('…'), "{need}");
    assert_eq!(
        needs_you("NEEDS YOU: the\u{7}  release\u{200B}\tcredential\u{202E}  **"),
        ["the release credential"]
    );
    assert_eq!(
        needs_you("NEEDS YOU: \u{200B}\nNEEDS YOU: the release credential"),
        ["the release credential"]
    );
}

#[test]
fn the_defense_line_is_read_by_the_same_line_test() {
    for line in [
        "Nothing left: every branch is on remote main.",
        "- Nothing left: every branch is on remote main.",
        "**Nothing left:** every branch is on remote main.",
        "1. NOTHING LEFT: every branch is on remote main.",
        "nothing left: every branch is on remote main.",
    ] {
        let tail = format!("Merged and pushed.\n{line}\nThe worktree is removed.");
        assert_eq!(defense_line(&tail), Some(line.trim()), "{line:?}");
    }
    for tail in [
        "There is nothing left: every branch is on remote main.",
        "`Nothing left:` is the head the harness reads.",
        "```\nNothing left: x\n```",
        "> Nothing left: quoted from the worker before",
        "Nothing left to do.",
    ] {
        assert_eq!(defense_line(tail), None, "{tail:?}");
    }
}

#[test]
fn the_defense_body_drops_the_head_and_the_needs() {
    let tail = "Merged.\nNothing left: the WIP branch is parked\nNEEDS YOU: the credential";
    assert_eq!(defense_body(tail), "Merged.\nThe WIP branch is parked");
}

#[test]
fn equal_sets_key_equal_and_none_keys_zero() {
    let a = vec!["x".to_string(), "y".to_string()];
    assert_eq!(needs_key(&a), needs_key(&a.clone()));
    assert_ne!(needs_key(&a), needs_key(&["x".to_string()]));
    assert_eq!(needs_key(&[]), 0);
}

fn none_list<'b, 'a>(
    by: &'b mut NoneWords<'a>,
    k: usize,
) -> Option<(&'static str, &'b mut &'a [&'a str])> {
    Some(match k {
        0 => ("NONE_HEADS", &mut by.heads),
        1 => ("NOT_NEEDED", &mut by.not_needed),
        2 => ("NONE_MODS", &mut by.mods),
        3 => ("NONE_STILL", &mut by.still),
        4 => ("NONE_NEEDED", &mut by.needed),
        5 => ("NONE_KEEPS", &mut by.keeps),
        6 => ("RESTATE_SUBJECTS", &mut by.subjects),
        7 => ("RESTATE_NAMED", &mut by.named),
        8 => ("RESTATE_THE", &mut by.the),
        9 => ("RESTATE_LINKS", &mut by.links),
        10 => ("LANDED_SAYS", &mut by.landed),
        11 => ("RESTATE_AT", &mut by.at),
        12 => ("RESTATE_PLACES", &mut by.places),
        13 => ("NONE_TIMES", &mut by.times),
        14 => ("YOU_SET", &mut by.set),
        15 => ("NONE_OBJECTS", &mut by.objects),
        16 => ("NONE_JOINS", &mut by.joins),
        17 => ("NONE_ASKED", &mut by.asked),
        _ => return None,
    })
}

fn misread_by(by: &NoneWords<'_>) -> Vec<String> {
    needs_table()
        .iter()
        .filter_map(|(row, report)| misread(row, report, by))
        .collect()
}

fn misread(row: &NeedsRow, report: &str, by: &NoneWords<'_>) -> Option<String> {
    let named: Vec<String> = needs_lines_by(report, by, wrap_join)
        .needs
        .iter()
        .map(|n| sanitized(n))
        .filter(|n| !n.is_empty())
        .collect();
    let misread = match row {
        NeedsRow::None => !named.is_empty(),
        NeedsRow::Need(words) => !named
            .iter()
            .any(|n| n.to_lowercase().contains(&words.to_lowercase())),
        NeedsRow::Headless(_) => false,
    };
    misread.then(|| format!("{row:?} {report:?} -> {named:?}"))
}

#[test]
fn every_none_word_is_load_bearing() {
    assert_eq!(misread_by(&NONE_WORDS), Vec::<String>::new());
    let table = needs_table();
    let lowered: Vec<String> = table.iter().map(|(_, r)| r.to_lowercase()).collect();
    let mut dead = Vec::new();
    for k in 0.. {
        let mut by = NONE_WORDS;
        let Some((name, list)) = none_list(&mut by, k) else {
            break;
        };
        let words: &[&str] = list;
        for (i, word) in words.iter().enumerate() {
            let kept: Vec<&str> = words
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, w)| *w)
                .collect();
            let mut by = NONE_WORDS;
            if let Some((_, list)) = none_list(&mut by, k) {
                *list = &kept;
            }
            let (first, rest): (Vec<usize>, Vec<usize>) =
                (0..table.len()).partition(|&r| lowered[r].contains(word));
            let bears = first.iter().chain(&rest).any(|&r| {
                let (row, report) = &table[r];
                misread(row, report, &by).is_some()
            });
            if !bears {
                dead.push(format!("{name}: {word:?}"));
            }
        }
    }
    assert!(dead.is_empty(), "words no row of the table needs: {dead:?}");
    let loose: Vec<&str> = NONE_TIMES
        .iter()
        .copied()
        .chain([
            "just", "please", "publish", "key", "tag", "approve", "the", "pr", "signing",
        ])
        .collect();
    let mut by = NONE_WORDS;
    by.times = &loose;
    let lost = misread_by(&by);
    assert!(
        lost.iter().any(|m| m.starts_with("Need(")),
        "a loose reader lost no need: {lost:?}"
    );
}

/// THE GUARD'S CUT TAKES NO GO-AHEAD JOIN (the merge of 2026-10-29 with main's [`apart`]): the
/// irreversible guard reads [`without_needs`], whose walk ([`needs_lines`]) joins a need's
/// wrapped rows by [`wrap_join`] alone; only [`go_ahead_view`] passes [`go_ahead_wrap`]. For
/// every text of the go-ahead's corpus (every table row at every width, every repro, every echo)
/// the guard's walk books no row only the go-ahead's join takes, and on every pair of rows
/// [`wraps`] is main's veto ([`apart`]) and the base's lowercase rule (246d1ddc2f: a row that
/// opens lowercase or with a digit), computed here from those two alone, with no quote mark
/// passed over and no mid-clause stop read. The go-ahead's join only adds to it: where the rule
/// joins, it joins as the guard does. The control: the go-ahead's view takes rows the guard's
/// does not in some text, so the walk the guard reads is not that view.
#[test]
fn the_guards_cut_takes_no_go_ahead_join() {
    use crate::go_ahead::testing::guard_corpus;
    let base_rule = |need: &str, next: &str| {
        !apart(need, next)
            && unquoted(next)
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c.is_ascii_digit())
    };
    let mut wrong = Vec::new();
    let mut viewed = 0;
    for text in guard_corpus() {
        if !needs_lines(&text).moved.is_empty() {
            wrong.push(format!("the guard's walk took a go-ahead row: {text:?}"));
        }
        let rows: Vec<&str> = text.lines().collect();
        for pair in rows.windows(2) {
            let base = base_rule(pair[0], pair[1]);
            if wraps(pair[0], pair[1]) != base {
                wrong.push(format!("wraps is not the base rule: {pair:?}"));
            }
            if base && go_ahead_wrap(pair[0], pair[1]) != Some(false) {
                wrong.push(format!(
                    "the go-ahead's join reads a wrap otherwise: {pair:?}"
                ));
            }
        }
        viewed += usize::from(!go_ahead_view(&text).moved.is_empty());
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(
        viewed > 0,
        "control: no text's need took a row in the go-ahead's view alone"
    );
}

/// THE AGENT'S WORD ON NEEDS (v6 §14.17.13, the post-landing review of 2026-11-02): the last
/// message's own none retracts every need the turn's earlier messages named; a none in an earlier
/// message, in the agent's own words, retracts the needs named before it in that same message and
/// only those; quoted or fenced text is never the agent's word, and one paragraph that names a
/// need and a none both keeps its need. Where the reader cannot tell the messages apart, the
/// earlier messages are one. The go-ahead reader's cut keeps the prose of what was retracted,
/// without its needs, and the fold's list ([`retracted_in`]) names what an earlier message took
/// back. Negative controls: [`standing_before`] reading the earlier messages from the last
/// paragraph that says none, quoted lines and message bounds aside (the rule of 2026-11-01 on
/// `origin/main` `21b1e42025`), drops the lead's own need under a relayed worker's none (`[]`);
/// reading only the last message (this branch's first cut, d55d270c1a) keeps a need its own
/// message retracted.
#[test]
fn the_agents_word_on_needs_retracts_what_it_named_before_it() {
    let need = "./check.sh fails on main. NEEDS YOU: decide whether to delete tests/test_clock.sh.";
    let none = "The flake was mine: fixed it and pushed. NEEDS YOU: nothing";
    let kept = ["decide whether to delete tests/test_clock.sh."];
    let turn =
        |messages: &[&str]| joined(Some(messages.iter().map(|m| (*m).to_string()).collect()));
    let named = |messages: &[&str], last: &str| {
        let (text, starts) = turn(messages);
        let text = text.unwrap_or_default();
        standing_before(Some(Before::new(&text, &starts)), last)
            .map_or_else(Vec::new, |t| needs_you(&t))
    };
    let lead = "NEEDS YOU: approve the notarization identity in Keychain Access (only you can \
                unlock it)";
    let lead_kept =
        ["approve the notarization identity in Keychain Access (only you can unlock it)"];
    let relayed = "@s-a reported:\n> Landed the fix on origin/main.\n> NEEDS YOU: nothing";
    assert_eq!(
        named(&[lead, relayed], "Moving on to the docs while that waits."),
        lead_kept,
        "a relayed worker's quoted none, in a message of its own"
    );
    assert_eq!(
        named(&[&format!("{lead}\n\n{relayed}")], "Moving on."),
        lead_kept,
        "a relayed worker's quoted none, in the lead's own message"
    );
    assert_eq!(
        named(
            &[lead, "@s-a reported:\n\nNEEDS YOU: nothing"],
            "Moving on."
        ),
        lead_kept,
        "an unquoted none in a later message retracts nothing named before that message"
    );
    let fenced = format!("{lead}\n\n```\nNEEDS YOU: nothing\n```");
    assert_eq!(named(&[&fenced], "Moving on."), lead_kept, "a fenced none");
    assert_eq!(
        named(&[&format!("{need}\n\n{none}")], "LANDED"),
        Vec::<String>::new(),
        "a none in the need's own message, after it"
    );
    assert_eq!(
        named(&[&format!("{none}\n\n{need}")], "LANDED"),
        kept,
        "a none before the need retracts nothing"
    );
    assert_eq!(
        named(&[need, &format!("Then: {lead}\n\n{none}")], "LANDED"),
        kept,
        "only the needs of its own message"
    );
    assert_eq!(
        named(&[&format!("{need}\nNEEDS YOU: nothing")], "LANDED"),
        kept,
        "one paragraph that names both keeps its need"
    );
    assert_eq!(
        named(&[need], "> NEEDS YOU: nothing"),
        kept,
        "a quoted last word"
    );
    assert_eq!(named(&[need], "LANDED"), kept);
    assert_eq!(
        named(&[need, relayed], "NEEDS YOU: nothing\nLANDED"),
        Vec::<String>::new()
    );
    assert_eq!(
        standing_before(Some(need.into()), "All done here.\n\nNEEDS YOU: nothing"),
        None,
        "the last message's own none retracts every earlier need"
    );
    assert_eq!(standing_before(None, "LANDED"), None);
    // Messages a reader cannot tell apart are one: the earlier none retracts the need above it.
    let whole = format!("{need}\n\n{none}");
    assert_eq!(
        standing_before(Some(whole.as_str().into()), "LANDED")
            .map_or_else(Vec::new, |t| needs_you(&t)),
        Vec::<String>::new()
    );
    // The fold's list: what an earlier message's own none took back, and nothing else.
    let (text, starts) = turn(&[lead, &format!("{need}\n\n{none}"), relayed]);
    let text = text.unwrap_or_default();
    let before = Before::new(&text, &starts);
    assert_eq!(retracted_in(before), kept);
    // Cut at the front (the act's own rows), the messages are still told apart.
    let rest = &text[lead.len() + 2..];
    assert_eq!(retracted_in(before.past(rest)), kept);
    assert_eq!(
        standing_before(Some(before.past(rest)), "LANDED").map_or_else(Vec::new, |t| needs_you(&t)),
        Vec::<String>::new()
    );
    // The go-ahead reader's view: the prose kept, the retracted need cut.
    let separate = turn(&[need, none]);
    let separate_text = separate.0.unwrap_or_default();
    assert_eq!(
        retracted_cut(Before::new(&separate_text, &separate.1), "LANDED"),
        separate_text.as_str(),
        "nothing retracted, nothing cut"
    );
    let cut = retracted_cut(whole.as_str().into(), "LANDED");
    assert!(
        cut.contains("./check.sh fails on main.")
            && cut.contains("fixed it")
            && needs_you(&cut).is_empty(),
        "{cut}"
    );
    let all = retracted_cut(
        Before::new(&separate_text, &separate.1),
        "NEEDS YOU: nothing",
    );
    assert!(
        all.contains("./check.sh fails on main.")
            && all.contains("fixed it")
            && needs_you(&all).is_empty(),
        "{all}"
    );
}

/// A NEEDS YOU NAMED AS A NOUN IN ANOTHER CLAUSE IS NO NEED (2026-11-02, aterm 0.148.0: a Cruise
/// reply's bullet that said Tab 1's need was cleared raised `is cleared. It came back once, …`
/// as this session's own). Behind a determiner (an article, a demonstrative, `its`, `your`, a
/// genitive) a head is a noun of that determiner's clause: a qualifier in brackets or a comma
/// ends no clause, so the words after either go on with it, and a qualifier with none after it
/// names the noun. The same holds for a row the screen wrapped after a determiner no clause ends
/// on, and a later head on such a row is still read. Negative control: on the reader before this
/// rule (origin/main at bd65a84448) every mention below names a need (its words, a paragraph
/// under it or the unread-need line), and every report names the same need it names here, but
/// the three behind a mention on their row, which name the mention's words with theirs.
#[test]
fn a_needs_you_named_as_a_noun_in_another_clause_is_no_need() {
    let incident = "- **Alerts:** the three join alerts are dismissed and the 10 open \"later\" \
         messages are filed. Tab 1's NEEDS YOU (the 0.148.0 release \"go\") is cleared. It came \
         back once, so I cleared it again and sent that session a note not to re-raise it each \
         turn.";
    for tail in [
        incident.to_string(),
        format!("Merged and pushed.\n{incident}\nNothing left: all on origin/main."),
        "Tab 1's NEEDS YOU (the 0.148.0 release \"go\") is cleared.".to_string(),
        "The NEEDS YOU (rotate the deploy key) was raised twice.".to_string(),
        "Checked: the NEEDS YOU [release go] stays up until the owner answers.".to_string(),
        "This NEEDS YOU (the release go) is cleared.".to_string(),
        "Your NEEDS YOU (sign in) is stale.".to_string(),
        "His NEEDS YOU (release go) is cleared.".to_string(),
        "Cleared the worker’s **NEEDS YOU** (sign in) after it came back.".to_string(),
        "Tab 1's NEEDS YOU, the release go, is cleared.".to_string(),
        "The NEEDS YOU, the release go, is cleared.".to_string(),
        "Tab 1's NEEDS YOU (approve the release).".to_string(),
        "Both workers' NEEDS YOU (sign in) came back.".to_string(),
        "No session has a NEEDS YOU (checked all six tabs).".to_string(),
        "Tab 1 still has a NEEDS YOU (the release go).".to_string(),
        "The alerts are cleared and the NEEDS YOU (the release go).\n\nNext I merged main."
            .to_string(),
        "The messages are filed. Tab 1's\nNEEDS YOU (the 0.148.0 release \"go\") is cleared."
            .to_string(),
        "The messages are filed. Tab 1's NEEDS YOU (the 0.148.0\nrelease \"go\") is cleared."
            .to_string(),
        "The alerts are cleared and the\nNEEDS YOU (the release go) was raised once.".to_string(),
        "The alerts are cleared and the\nNEEDS YOU (release go).\n\nNext I merged main."
            .to_string(),
        "The alerts are cleared and the\nNEEDS YOU (release go).".to_string(),
        "No session has a\nNEEDS YOU (checked all six tabs).".to_string(),
        "All four requests are in. Tab 1's\nNEEDS YOU came back. Checking what it says now:"
            .to_string(),
        "- lists the logins the Mac held. Rotating those at the vendor is a\nNEEDS YOU when a \
         Mac is stolen."
            .to_string(),
    ] {
        assert!(
            needs_you(&tail).is_empty(),
            "{tail:?}: {:?}",
            needs_you(&tail)
        );
        assert!(!says_no_needs(&tail), "{tail:?}");
        assert_eq!(without_needs(&tail), tail, "{tail:?}");
    }
    for (tail, want) in [
        ("Done. NEEDS YOU: rotate the key", "rotate the key"),
        ("**NEEDS YOU (owner):** sign in", "sign in"),
        (
            "Blocked — NEEDS YOU: approve the release",
            "approve the release",
        ),
        (
            "Done. NEEDS YOU (owner) sign the release",
            "sign the release",
        ),
        (
            "One thing NEEDS YOU: the release credential",
            "the release credential",
        ),
        ("The release NEEDS YOU (owner): approve it", "approve it"),
        ("The release NEEDS YOU, Andrew: approve it", "approve it"),
        (
            "Here's the NEEDS YOU: sign in to GitHub",
            "sign in to GitHub",
        ),
        ("Here's NEEDS YOU: sign in to GitHub", "sign in to GitHub"),
        ("My NEEDS YOU: rotate the key", "rotate the key"),
        (
            "Landed. My NEEDS YOU (owner) — rotate the key",
            "rotate the key",
        ),
        ("Landed. 🚨 NEEDS YOU! the signing key", "the signing key"),
        (
            "R7: `secrets set notary` (NEEDS YOU: the password).",
            "the password).",
        ),
        (
            "- `ship release`: NEEDS YOU (release credential not on this machine)",
            "release credential not on this machine",
        ),
        (
            "Merged the parser and pushed main.\nNEEDS YOU (owner) the release credential",
            "the release credential",
        ),
        (
            "Here is the\nNEEDS YOU (owner):\n\n- sign in to GitHub",
            "sign in to GitHub",
        ),
        (
            "The messages are filed. Tab 1's NEEDS YOU (release go) is cleared. NEEDS YOU: \
             rotate the key",
            "rotate the key",
        ),
        (
            "The messages are filed. Tab 1's\nNEEDS YOU (release go) is cleared. NEEDS YOU: \
             rotate the key",
            "rotate the key",
        ),
        (
            "The messages are filed and the\nNEEDS YOU (release go) is cleared. NEEDS YOU: \
             rotate the key",
            "rotate the key",
        ),
    ] {
        assert_eq!(needs_you(tail), [want], "{tail:?}");
        assert!(!says_no_needs(tail), "{tail:?}");
    }
}

/// A HEAD BEHIND A WORD THAT IS NO DETERMINER, OR PAST A SEPARATOR, STILL NAMES ITS NEED (the
/// review of the mention rule above): a separator ends a determiner's clause, so a head behind a
/// genitive or a possessive names what follows its colon or dash, and a none there retracts the
/// turn's earlier needs. The writer's own `my` and `our` are no determiner, and neither is a
/// subject, a label or a step number, which a head as a verb or after a label follows. A quote
/// closed by `'` is a mark, not a plural genitive, and a row the screen did not wrap (a heading,
/// an emphasised label, a list item of its own, a row that ends on a word a clause can end on:
/// `one`, `that`, `any`) leaves the head under it its own. Negative control: on the reader that
/// read every word before a head as its clause's and every genitive or possessive as another's
/// need (c3063e1b9d), every row below named no need, and `This turn's NEEDS YOU: none` no none.
#[test]
fn a_head_behind_no_determiner_or_past_a_separator_still_names_its_need() {
    for (tail, want) in [
        (
            "This session's NEEDS YOU: sign in to GitHub",
            "sign in to GitHub",
        ),
        (
            "**Today's NEEDS YOU:** approve the release",
            "approve the release",
        ),
        (
            "s-2's NEEDS YOU: approve the release",
            "approve the release",
        ),
        (
            "- Worker s-2's NEEDS YOU: sign in to GitHub (only you can)",
            "sign in to GitHub (only you can)",
        ),
        (
            "Landed. Your NEEDS YOU (owner): approve the release",
            "approve the release",
        ),
        (
            "Landed the fix and the worker's\nNEEDS YOU (owner): sign in",
            "sign in",
        ),
        (
            "Here's your NEEDS YOU: sign in to GitHub.",
            "sign in to GitHub.",
        ),
        (
            "My NEEDS YOU: token for the npm publish",
            "token for the npm publish",
        ),
        (
            "Your NEEDS YOU: token for the npm publish",
            "token for the npm publish",
        ),
        (
            "Here is my NEEDS YOU (owner) approve the release",
            "approve the release",
        ),
        (
            "Everything landed. Here is my\nNEEDS YOU (owner) approve the release",
            "approve the release",
        ),
        (
            "Landed. Still NEEDS YOU (owner) the notary password",
            "the notary password",
        ),
        (
            "R7 NEEDS YOU (owner) the notary password",
            "the notary password",
        ),
        (
            "**Blocked** NEEDS YOU (owner) sign the release",
            "sign the release",
        ),
        (
            "Step 2 NEEDS YOU (owner) sign the release",
            "sign the release",
        ),
        (
            "One thing NEEDS YOU, approve the release",
            "approve the release",
        ),
        (
            "Ran the gate; it said 'all checks pass'\nNEEDS YOU: approve the release",
            "approve the release",
        ),
        (
            "Ran the gate; it said 'all checks pass'\nNEEDS YOU (owner) approve the release",
            "approve the release",
        ),
        (
            "The check printed ‘all tests pass’\nNEEDS YOU: approve the release",
            "approve the release",
        ),
        (
            "The gate said 'all checks pass' NEEDS YOU: approve the release",
            "approve the release",
        ),
        (
            "- Ran `git log --format='%s'`\nNEEDS YOU: approve the release",
            "approve the release",
        ),
        (
            "- Trust dialog: answered 'Yes'\n- NEEDS YOU: approve the release",
            "approve the release",
        ),
        (
            "## Owner's\nNEEDS YOU (owner) approve the release",
            "approve the release",
        ),
        (
            "**Andrew's**\nNEEDS YOU (owner) approve the release",
            "approve the release",
        ),
        (
            "- Tab 1's\n- NEEDS YOU (owner) approve the release",
            "approve the release",
        ),
        (
            "I checked each one\nNEEDS YOU (owner) sign the release",
            "sign the release",
        ),
        (
            "- Removed the stale worktrees, every one\nNEEDS YOU, approve the release",
            "approve the release",
        ),
    ] {
        assert_eq!(needs_you(tail), [want], "{tail:?}");
        assert!(!says_no_needs(tail), "{tail:?}");
    }
    assert_eq!(
        needs_you(
            "Ran the gate; it said 'all checks pass'\nNEEDS YOU:\n- approve the release\n- rotate the key"
        ),
        ["approve the release", "rotate the key"]
    );
    for none in ["This turn's NEEDS YOU: none", "Today's NEEDS YOU: nothing"] {
        assert!(says_no_needs(none), "{none:?}");
        assert_eq!(
            standing_before(Some("NEEDS YOU: approve the release.".into()), none),
            None,
            "{none:?}"
        );
    }
}

/// THE WORKER'S OWN `Could not finish:` starts its reply, with a reason. The same head further down
/// or as a list's item, a head with no reason, a task's own placeholder, a quote, a fence, a sentence
/// that names it, and a reply that also defends the work are none. NEGATIVE CONTROL: the first rows
/// are the word itself.
#[test]
fn could_not_finish_reads_a_head_with_a_reason_and_nothing_else() {
    for (tail, why) in [
        (
            "Could not finish: the grammar is gone.",
            Some("the grammar is gone."),
        ),
        (
            "\n\n  could not finish: no write access\nI tried twice.",
            Some("no write access"),
        ),
        (
            "**Could not finish:** the grammar is gone",
            Some("the grammar is gone"),
        ),
        ("I tried.\n\nCould not finish: no write access", None),
        ("- **Could not finish:** the grammar is gone", None),
        (
            "Progress:\n- Parser pushed\n- Could not finish: the docs pass, doing that next",
            None,
        ),
        ("Could not finish:", None),
        ("Could not finish:   ", None),
        ("Could not finish: <why>", None),
        ("> Could not finish: their words", None),
        ("```\nCould not finish: in a fence\n```", None),
        (
            "The task says to reply `Could not finish: x` when stuck.",
            None,
        ),
        ("I could not finish: the build runs.", None),
        (
            "Could not finish: a part.\nNothing left: the rest is on origin/main.",
            None,
        ),
        (
            "Nothing left: it is on origin/main.\nCould not finish: a part.",
            None,
        ),
        ("The parser is pushed.", None),
    ] {
        assert_eq!(could_not_finish(tail), why, "{tail:?}");
    }
}

/// A BARE HEAD ITS CLAUSE HAS FOR SUBJECT IS PROSE (live 2026-11-04: an agent's bullet `- **NEEDS
/// YOU can be lost.** A NEEDS YOU line in a turn that ended on a usage wall …` showed in `aterm
/// ctl survey` as `claude needs you: can be lost.`). Before a singular finite (`is`, `has`, …), or
/// a modal whose verb is `be`, NEEDS YOU is the subject of its clause ([`subject_of`]): the row
/// names no need and its prose stays every other reader's. The row still opens a line of its own
/// ([`apart`]), and the scan for a head past a row's start ([`mid_head`], from the row's first
/// byte) reads none on it. A modal and any other verb is a need (`NEEDS YOU must approve the
/// deploy`), as are an imperative, a noun, a head past a separator or a qualifier, and a question:
/// a singular finite before a subject pronoun, or a first sentence that ends on a question mark,
/// on a row wrapped under the head's too. A question mark in a later sentence, in brackets or in a
/// URL asks nothing. Under a line that names a head (`… on a line starting`), a row that opens on
/// one is skipped whatever its words say ([`opens_head`]), its question too.
#[test]
fn a_bare_head_its_clause_has_for_subject_is_prose_not_a_need() {
    let incident =
        "- **NEEDS YOU can be lost.** A NEEDS YOU line in a turn that ended on a usage wall …";
    for tail in [
        incident.to_string(),
        format!("Merged and pushed main.\n{incident}\nNothing left: all on origin/main."),
        "NEEDS YOU is now read only past a colon.".to_string(),
        "NEEDS YOU isn't hidden by a wall.".to_string(),
        "## NEEDS YOU was raised twice by tab 3".to_string(),
        "NEEDS YOU wasn't raised again.".to_string(),
        "NEEDS YOU has its own row in the survey.".to_string(),
        "NEEDS YOU hasn't changed since Round 3.".to_string(),
        "NEEDS YOU does not block.".to_string(),
        "NEEDS YOU doesn't block Land any more.".to_string(),
        "NEEDS YOU doesn’t block Land any more.".to_string(),
        "**NEEDS YOU** should be on screen until answered.".to_string(),
        "NEEDS YOU can never be lost.".to_string(),
        "NEEDS YOU will always be shown.".to_string(),
        "NEEDS YOU can not be answered by the harness.".to_string(),
        "NEEDS YOU should have been raised once.".to_string(),
        "  - **NEEDS YOU is loud and never blocks.** An agent puts what only a person can give on \
         a line"
            .to_string(),
        "`NEEDS YOU` is the head the harness reads.".to_string(),
        "NEEDS YOU is loud and never blocks\nwhen a wall ends the turn.".to_string(),
        "NEEDS YOU can\nbe lost when a wall ends the turn.".to_string(),
        "- **NEEDS YOU can be lost.** What happens when a turn ends on a wall? The line is dropped."
            .to_string(),
        "- **NEEDS YOU is loud and never blocks.** Why does it matter?".to_string(),
        "NEEDS YOU is loud and never blocks. Does Land hold on it? No.".to_string(),
        "NEEDS YOU can be lost when a turn ends on a\nwall. Why? The wall cuts the line.".to_string(),
        "NEEDS YOU is never hidden (is it ever hidden? no).".to_string(),
        "NEEDS YOU is documented at https://example.com/docs?page=needs".to_string(),
        "- **NEEDS YOU is never hidden.**\n  is that fixed? Yes.".to_string(),
        "Put what only a person can give on a line starting\nNEEDS YOU is the release key still\n\
         valid? and carry on."
            .to_string(),
    ] {
        assert!(
            needs_you(&tail).is_empty(),
            "{tail:?}: {:?}",
            needs_you(&tail)
        );
        assert!(!says_no_needs(&tail), "{tail:?}");
        assert_eq!(without_needs(&tail), tail, "{tail:?}");
    }
    assert_eq!(mid_head(incident, "", &[], 0), None);
    let above = "Merged the parser, and a turn that ended on a usage wall and the";
    for row in [
        "NEEDS YOU can be lost.",
        "NEEDS YOU is the release key still",
        "NEEDS YOU: sign in to GitHub",
    ] {
        assert!(apart(above, row), "{row:?}");
    }
    assert!(!apart(above, "needs you can be lost."));
    for modal in [
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
    ] {
        let prose = format!("NEEDS YOU {modal} be read past a colon.");
        assert!(needs_you(&prose).is_empty(), "{prose:?}");
        let need = format!("NEEDS YOU {modal} approve the deploy");
        assert_eq!(needs_you(&need), [&need["NEEDS YOU ".len()..]], "{need:?}");
    }
    for adverb in [
        "not", "never", "always", "also", "still", "only", "now", "just",
    ] {
        let prose = format!("NEEDS YOU can {adverb} be raised by codex-alab.");
        assert!(needs_you(&prose).is_empty(), "{prose:?}");
        let need = format!("NEEDS YOU must {adverb} approve the deploy");
        assert_eq!(needs_you(&need), [&need["NEEDS YOU ".len()..]], "{need:?}");
    }
    for (tail, want) in [
        (
            "NEEDS YOU must approve the deploy at https://example.com/deploy",
            "must approve the deploy at https://example.com/deploy",
        ),
        (
            "NEEDS YOU should sign in to GitHub (gh auth login) so I can push.",
            "should sign in to GitHub (gh auth login) so I can push.",
        ),
        (
            "NEEDS YOU will need to accept the Xcode license.",
            "will need to accept the Xcode license.",
        ),
        (
            "**NEEDS YOU** must sign the notarization agreement.",
            "must sign the notarization agreement.",
        ),
        (
            "NEEDS YOU can't push: the token for alabsystems expired",
            "can't push: the token for alabsystems expired",
        ),
        (
            "NEEDS YOU could the owner rotate the deploy key",
            "could the owner rotate the deploy key",
        ),
        ("NEEDS YOU is: approve the deploy", "is: approve the deploy"),
        (
            "NEEDS YOU can you sign in to GitHub?",
            "can you sign in to GitHub?",
        ),
        (
            "NEEDS YOU should I force-push main?",
            "should I force-push main?",
        ),
        (
            "NEEDS YOU is the release key still valid? If not, rotate it.",
            "is the release key still valid? If not, rotate it.",
        ),
        (
            "NEEDS YOU is the release key still\nvalid for the notary?",
            "is the release key still valid for the notary?",
        ),
        (
            "NEEDS YOU will be at the desk for the notary call?",
            "will be at the desk for the notary call?",
        ),
        (
            "NEEDS YOU is it OK to force-push main",
            "is it OK to force-push main",
        ),
        (
            "NEEDS YOU can the release go out\ntoday?",
            "can the release go out today?",
        ),
        (
            "NEEDS YOU can\nyou sign in to GitHub",
            "can you sign in to GitHub",
        ),
        (
            "- **NEEDS YOU:** can you sign in to GitHub",
            "can you sign in to GitHub",
        ),
        (
            "NEEDS YOU — must rotate the deploy key before Friday",
            "must rotate the deploy key before Friday",
        ),
        (
            "NEEDS YOU (owner) is blocked on the notary key",
            "is blocked on the notary key",
        ),
        (
            "Merged the parser. NEEDS YOU: is blocked on the notary key",
            "is blocked on the notary key",
        ),
        ("NEEDS YOU the release credential", "the release credential"),
        ("NEEDS-YOU approve the deploy", "approve the deploy"),
        (
            "NEEDS YOU have the notary password ready",
            "have the notary password ready",
        ),
        (
            "NEEDS YOU is loud. NEEDS YOU: rotate the key",
            "rotate the key",
        ),
    ] {
        assert_eq!(needs_you(tail), [want], "{tail:?}");
        assert!(!says_no_needs(tail), "{tail:?}");
    }
}
