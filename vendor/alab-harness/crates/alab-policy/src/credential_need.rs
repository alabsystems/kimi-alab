// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! A CREDENTIAL IS NEVER A PERSON'S NEEDS YOU (owner, 2026-11-12: "credentials must never reach
//! the person as a NEEDS YOU at all"). A `NEEDS YOU:` line that asks for a credential
//! ([`wants_of`]) is taken off the person's list where every reader parses needs
//! ([`super::needs::persons_needs`]), and aterm resolves it on the one path instead: the host looks
//! each want up in the secrets registry (names, pointers and purposes, never a value) and files
//! `aterm secrets need <want>` for one it does not hold ([`CredentialRead`]). The agent is told
//! what was found or filed in one short line ([`RULE_CREDENTIAL`], [`told_line`]), each fact once
//! a session, and records the blocked step and continues. A release is cut on the release
//! machine, whose ring has no want to file.

use std::collections::BTreeSet;

/// The act that tells the agent how aterm resolved its credential line.
pub const RULE_CREDENTIAL: &str = "credential@v1";

/// The want a release, signing or notarized-build need names: the release machine's, never filed.
pub const RELEASE_WANT: &str = "release";

/// One registry entry, value-free: its name, where its file lands, what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultEntry {
    /// The entry's name in the registry.
    pub name: String,
    /// Where its file lands (`~/…`), `None` for an entry with no file.
    pub pointer: Option<String>,
    /// What it is for.
    pub purpose: String,
}

/// How the host resolved one want.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The registry holds these entries for it.
    Held(Vec<VaultEntry>),
    /// The registry holds none: aterm filed `aterm secrets need <want>`.
    Filed,
    /// Neither was possible, and why.
    Failed(String),
}

/// WHAT THE HOST RESOLVED of the credential wants a turn named (`IdleHost::credentials`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CredentialRead {
    /// Nothing was asked: no host, a tape without the door, no credential line, or a platform
    /// with no vault.
    #[default]
    NotAsked,
    /// Each want asked, with how it resolved.
    Resolved(Vec<(String, Resolution)>),
}

impl CredentialRead {
    fn of(&self, want: &str) -> Option<&Resolution> {
        match self {
            Self::NotAsked => None,
            Self::Resolved(all) => all.iter().find(|(w, _)| w == want).map(|(_, r)| r),
        }
    }
}

/// Words that make a line ask for a credential. A vendor sign-in (log in, sign in) is a person's
/// act at a window, not a vault entry, so it is not among them.
const CREDENTIAL_WORDS: &[&str] = &[
    "credential",
    "credentials",
    "token",
    "tokens",
    "keychain",
    "password",
    "passwords",
    "passphrase",
    "secret",
    "secrets",
    "certificate",
    "certificates",
    "cert",
    "certs",
    "devid",
    "pat",
];

/// A key is a credential only when its noun phrase says what kind: a keyboard key or a
/// security-key gesture still needs the person. Possessives such as "deployer's key" count.
const KEY_KINDS: &[&str] = &[
    "api",
    "ssh",
    "private",
    "public",
    "signing",
    "codesign",
    "codesigning",
    "gpg",
    "github",
    "access",
    "secret",
    "deploy",
    "deployer",
    "deployment",
    "notary",
    "release",
    "account",
    "auth",
    "authentication",
    "encryption",
    "decryption",
    "cryptographic",
];

const CREDENTIAL_PHRASES: &[&str] = &["app specific", "access token", "developer id"];

const NOTARY_WORDS: &[&str] = &[
    "notary",
    "notarize",
    "notarise",
    "notarized",
    "notarised",
    "notarization",
    "notarisation",
    "notarytool",
];

const SIGNING_WORDS: &[&str] = &[
    "signing",
    "codesign",
    "codesigning",
    "signature",
    "devid",
    "certificate",
    "certificates",
    "cert",
    "certs",
];

const RELEASE_WORDS: &[&str] = &["release", "releases", "alabsystems"];

const GITHUB_WORDS: &[&str] = &["github", "agh"];

const DEPLOY_WORDS: &[&str] = &["deploy", "deployer", "deployment", "deploying"];

/// The line's words, lower-cased, each run of letters and digits one word, padded with a space
/// on each side so a phrase is matched whole.
fn words(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 2);
    out.push(' ');
    for c in line.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

fn has_word(words: &str, list: &[&str]) -> bool {
    list.iter().any(|w| words.contains(&format!(" {w} ")))
}

fn credential_key(words: &str) -> bool {
    let mut before = "";
    let mut previous = "";
    for word in words.split_whitespace() {
        let kind = if previous == "s" { before } else { previous };
        if matches!(word, "key" | "keys") && KEY_KINDS.contains(&kind) {
            return true;
        }
        before = previous;
        previous = word;
    }
    false
}

/// THE WANTS A CREDENTIAL LINE NAMES, as `aterm secrets need` spells them, and [`RELEASE_WANT`]
/// for the release machine's: `None` where `line` asks for no credential (a decision, a grant, a
/// vendor sign-in), `Some` of none where it asks for one the vocabulary cannot name.
#[must_use]
pub fn wants_of(line: &str) -> Option<Vec<&'static str>> {
    let w = words(line);
    let notary = has_word(&w, NOTARY_WORDS) || w.contains(" app specific ");
    let credential = notary
        || has_word(&w, CREDENTIAL_WORDS)
        || credential_key(&w)
        || CREDENTIAL_PHRASES
            .iter()
            .any(|p| w.contains(&format!(" {p} ")));
    if !credential {
        return None;
    }
    let release = has_word(&w, RELEASE_WORDS);
    let github = has_word(&w, GITHUB_WORDS);
    let signing = has_word(&w, SIGNING_WORDS) || w.contains(" developer id ");
    let mut wants = Vec::new();
    if notary {
        wants.push("notary");
    }
    if github {
        wants.push(if release { "gh:release" } else { "gh:dev" });
    }
    if signing || (release && !github) {
        wants.push(RELEASE_WANT);
    }
    if has_word(&w, DEPLOY_WORDS) || w.contains(" service account ") {
        wants.push("site-deployer-key");
    }
    Some(wants)
}

/// Whether `line` asks for a credential ([`wants_of`]).
#[must_use]
pub fn is_credential(line: &str) -> bool {
    wants_of(line).is_some()
}

/// The wants every line of `lines` names, each once, [`RELEASE_WANT`] left out: the ones the host
/// resolves.
#[must_use]
pub fn host_wants(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for want in lines.iter().filter_map(|l| wants_of(l)).flatten() {
        if want != RELEASE_WANT && !out.iter().any(|w| w == want) {
            out.push(want.to_owned());
        }
    }
    out
}

/// One thing the agent is told, keyed so it is told once a session.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fact {
    Held(VaultEntry),
    Release,
    Filed(String),
    Failed(String, String),
    /// A want the host did not resolve, or the vocabulary cannot name (`<want>`).
    File(String),
}

impl Fact {
    fn key(&self) -> String {
        match self {
            Self::Held(entry) => format!("held:{}", entry.name),
            Self::Release => RELEASE_WANT.to_owned(),
            Self::Filed(want) => format!("filed:{want}"),
            Self::Failed(want, _) => format!("failed:{want}"),
            Self::File(want) => format!("file:{want}"),
        }
    }
}

/// The most UTF-16 code units one entry's description takes.
const ENTRY_UNITS: usize = 110;

/// The most entries one line names.
const NAMED_ENTRIES: usize = 4;

fn described(entry: &VaultEntry) -> String {
    let text = match entry.pointer.as_deref().filter(|p| *p != entry.name) {
        Some(pointer) => format!("`{}` at `{pointer}` ({})", entry.name, entry.purpose),
        None => format!("`{}` ({})", entry.name, entry.purpose),
    };
    super::words::fit_units(&text, ENTRY_UNITS).into_owned()
}

/// The line's opening ([`told_line`]) after the harness's mark: a literal, or a macro call that
/// expands to one, as `concat!` takes it (aterm passes its `harness_mark!()`).
#[macro_export]
macro_rules! credential_head {
    ($($mark:tt)+) => {
        concat!(
            $($mark)+,
            " A credential is no NEEDS YOU, so aterm took that line off the person's list."
        )
    };
}

/// The line's close.
pub const CREDENTIAL_TAIL: &str = "Record any step it blocks and continue.";

/// The clause for a release, signing or notarized-build need.
pub const RELEASE_MACHINE: &str = "Releases are cut on the release machine.";

/// THE LINE THAT TELLS THE AGENT how its credential `lines` resolved by `read`, saying only what
/// `told` does not hold yet, and the keys it tells; `None` where every fact was told. The line
/// opens with the harness's `mark` ([`credential_head!`]).
#[must_use]
pub fn told_line(
    mark: &str,
    lines: &[String],
    read: &CredentialRead,
    told: &BTreeSet<String>,
) -> Option<(String, Vec<String>)> {
    let mut facts: Vec<Fact> = Vec::new();
    let mut add = |fact: Fact| {
        if !told.contains(&fact.key()) && !facts.contains(&fact) {
            facts.push(fact);
        }
    };
    for line in lines {
        let Some(wants) = wants_of(line) else {
            continue;
        };
        if wants.is_empty() {
            add(Fact::File("<want>".to_owned()));
        }
        for want in wants {
            if want == RELEASE_WANT {
                add(Fact::Release);
                continue;
            }
            match read.of(want) {
                Some(Resolution::Held(entries)) if !entries.is_empty() => {
                    for entry in entries {
                        add(Fact::Held(entry.clone()));
                    }
                }
                Some(Resolution::Filed) => add(Fact::Filed(want.to_owned())),
                Some(Resolution::Failed(why)) => add(Fact::Failed(want.to_owned(), why.clone())),
                Some(Resolution::Held(_)) | None => add(Fact::File(want.to_owned())),
            }
        }
    }
    if facts.is_empty() {
        return None;
    }
    let keys = facts.iter().map(Fact::key).collect();
    let held: Vec<String> = facts
        .iter()
        .filter_map(|f| match f {
            Fact::Held(e) => Some(described(e)),
            _ => None,
        })
        .collect();
    let mut parts: Vec<String> = Vec::new();
    if !held.is_empty() {
        let more = held.len().saturating_sub(NAMED_ENTRIES);
        let mut named = held.into_iter().take(NAMED_ENTRIES).collect::<Vec<_>>();
        if more > 0 {
            named.push(format!("{more} more (`aterm secrets ls --describe`)"));
        }
        parts.push(format!("The vault holds {}.", named.join("; ")));
    }
    if facts.contains(&Fact::Release) {
        parts.push(RELEASE_MACHINE.to_owned());
    }
    let filed: Vec<&str> = facts
        .iter()
        .filter_map(|f| match f {
            Fact::Filed(w) => Some(w.as_str()),
            _ => None,
        })
        .collect();
    if !filed.is_empty() {
        parts.push(format!(
            "aterm filed `aterm secrets need {}`.",
            filed.join("`, `aterm secrets need ")
        ));
    }
    for fact in &facts {
        match fact {
            Fact::Failed(want, why) => parts.push(format!(
                "aterm could not file `aterm secrets need {want}` ({}).",
                super::words::fit_units(why.trim(), 120)
            )),
            Fact::File(want) => parts.push(format!("File it: `aterm secrets need {want}`.")),
            _ => {}
        }
    }
    parts.push(CREDENTIAL_TAIL.to_owned());
    Some((
        format!("{mark}{} {}", credential_head!(""), parts.join(" ")),
        keys,
    ))
}

#[cfg(test)]
#[path = "credential_need_tests.rs"]
mod tests;
