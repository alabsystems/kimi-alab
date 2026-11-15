// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The relaunch grammar: the words aterm types to start a harness again on a conversation.
//!
//! When aterm's watchdog ends a dead harness, it relaunches the tab by typing the registry row's
//! own verified tool and these words (the HX-15 plan's D21; HX-15f types them):
//!
//! ```text
//! <tool> run [--account <label>] [--home <dir>] [--model <word>] -- <resume…>
//! ```
//!
//! * `--account <label>`: the account the child runs on, an account label as the claim's
//!   `login=` reads it (`[a-z0-9][a-z0-9._-]{0,63}`, not `help`; an account pool's `ac-` key is
//!   one). Until the account pool (HX-29) it is the claim's `login=`.
//! * `--home <dir>`: the home the vendor reads that account's credentials from, an absolute
//!   path. It comes only from a pool assignment, so only beside `--account`.
//! * `--model <word>`: the model to start the child on, a model word as a restart names it
//!   ([`crate::claim::model_word`]). aterm types it only for a `restart model=`.
//! * `--` and then the arguments that resume the conversation, passed through as they are. The
//!   `--` is always there; the arguments after it may be none.
//!
//! [`Run::parse`] reads the words a harness's `main` was given after its own name, `run` first,
//! and refuses an unknown flag, a flag named twice, a flag without its value, a value out of its
//! grammar and a line without `--`. [`Run::args`] writes them back. Both sides take the grammar
//! from here, so what aterm types is what every harness reads. A harness that opens a stream
//! accepts it (the plan's D21). Nothing here runs, types or opens anything.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::claim::{Account, journal_word, login_word, model_word};

/// The subcommand: the first word after the tool.
pub const VERB: &str = "run";
/// The flag naming the account.
pub const ACCOUNT: &str = "--account";
/// The flag naming the account's home.
pub const HOME: &str = "--home";
/// The flag naming the model.
pub const MODEL: &str = "--model";
/// The word that ends the flags: every word after it resumes the conversation.
pub const SEPARATOR: &str = "--";

/// One relaunch, as [`Run::parse`] read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The account to start the child on (`--account`), with its home (`--home`) when a pool
    /// assignment named one; `None` for the account the vendor would use.
    pub account: Option<Account>,
    /// The model to start the child on (`--model`), a model word.
    pub model: Option<String>,
    /// The arguments that resume the conversation, after `--`, as they were given.
    pub resume: Vec<OsString>,
}

/// Why words are not a relaunch. Its text is one journal word group, as
/// [`crate::claim::ReplyError`]'s is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    /// The first word is not `run`.
    NotRun,
    /// A word before `--` that is not one of the three flags: the word, made journal-safe.
    Unknown(String),
    /// A flag named twice.
    Repeated(&'static str),
    /// A flag with no value after it.
    Missing(&'static str),
    /// A flag's value is not in its grammar.
    Malformed(&'static str),
    /// `--home` without `--account`: a home is an account's.
    HomeWithoutAccount,
    /// No `--` ends the flags.
    NoSeparator,
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRun => f.write_str("run=absent"),
            Self::Unknown(word) => write!(f, "flag={word} unknown"),
            Self::Repeated(flag) => write!(f, "{flag}=repeated"),
            Self::Missing(flag) => write!(f, "{flag}=absent"),
            Self::Malformed(flag) => write!(f, "{flag}=malformed"),
            Self::HomeWithoutAccount => f.write_str("--home=without-account"),
            Self::NoSeparator => f.write_str("--=absent"),
        }
    }
}

impl std::error::Error for RunError {}

/// `value` as text, or the flag's [`RunError::Malformed`].
fn text<'a>(flag: &'static str, value: &'a OsStr) -> Result<&'a str, RunError> {
    value.to_str().ok_or(RunError::Malformed(flag))
}

/// Whether `home` is a home `--home` may name: an absolute path.
fn home_path(home: &Path) -> bool {
    home.is_absolute()
}

impl Run {
    /// Read the words after the tool's name, `run` first.
    ///
    /// # Errors
    /// The first word is not `run`; a word before `--` is not `--account`, `--home` or
    /// `--model`; a flag is named twice or has no value; `--account` is not an account label,
    /// `--home` not an absolute path or `--model` not a model word; `--home` comes without
    /// `--account`; or no `--` ends the flags.
    pub fn parse<I, S>(args: I) -> Result<Self, RunError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut words = args.into_iter().map(Into::into);
        if words.next().as_deref() != Some(OsStr::new(VERB)) {
            return Err(RunError::NotRun);
        }
        let (mut label, mut home, mut model) = (None, None, None);
        loop {
            let Some(word) = words.next() else {
                return Err(RunError::NoSeparator);
            };
            let (flag, slot) = match word.to_str() {
                Some(SEPARATOR) => break,
                Some(ACCOUNT) => (ACCOUNT, &mut label),
                Some(HOME) => (HOME, &mut home),
                Some(MODEL) => (MODEL, &mut model),
                _ => {
                    return Err(RunError::Unknown(journal_word(&word.to_string_lossy())));
                }
            };
            let value = words.next().ok_or(RunError::Missing(flag))?;
            // A flag's value is never the next flag or the separator: `--account --` is a
            // missing value, not an account named `--`.
            if value.to_str().is_some_and(|value| value.starts_with("--")) {
                return Err(RunError::Missing(flag));
            }
            if slot.replace(value).is_some() {
                return Err(RunError::Repeated(flag));
            }
        }
        let account = match (label, home) {
            (None, None) => None,
            (None, Some(_)) => return Err(RunError::HomeWithoutAccount),
            (Some(label), home) => {
                let label = login_word(Some(text(ACCOUNT, &label)?))
                    .ok_or(RunError::Malformed(ACCOUNT))?
                    .to_string();
                let home = home.map(PathBuf::from);
                if home.as_deref().is_some_and(|home| !home_path(home)) {
                    return Err(RunError::Malformed(HOME));
                }
                Some(Account { label, home })
            }
        };
        let model = match model {
            None => None,
            Some(model) => {
                let model = text(MODEL, &model)?;
                if !model_word(model) {
                    return Err(RunError::Malformed(MODEL));
                }
                Some(model.to_string())
            }
        };
        Ok(Self {
            account,
            model,
            resume: words.collect(),
        })
    }

    /// The words that relaunch this run, `run` first, in the order [`Run::parse`] documents:
    /// `run [--account <label>] [--home <dir>] [--model <word>] -- <resume…>`.
    ///
    /// # Errors
    /// The run would not read back as itself: its account is not an account label, its home
    /// not an absolute path, or its model not a model word.
    pub fn args(&self) -> Result<Vec<OsString>, RunError> {
        let mut args = vec![OsString::from(VERB)];
        if let Some(account) = &self.account {
            if login_word(Some(&account.label)).is_none() {
                return Err(RunError::Malformed(ACCOUNT));
            }
            args.extend([OsString::from(ACCOUNT), OsString::from(&account.label)]);
            if let Some(home) = &account.home {
                if !home_path(home) {
                    return Err(RunError::Malformed(HOME));
                }
                args.extend([OsString::from(HOME), home.clone().into_os_string()]);
            }
        }
        if let Some(model) = &self.model {
            if !model_word(model) {
                return Err(RunError::Malformed(MODEL));
            }
            args.extend([OsString::from(MODEL), OsString::from(model)]);
        }
        args.push(OsString::from(SEPARATOR));
        args.extend(self.resume.iter().cloned());
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStringExt as _;

    use super::*;

    fn run(words: &[&str]) -> Result<Run, RunError> {
        Run::parse(words.iter().copied())
    }

    fn os(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// The grammar of the plan's D21: every flag optional, in any order, each once, then `--`
    /// and the resume arguments as they were given, flags among them.
    #[test]
    fn the_run_grammar_reads_account_home_model_and_resume() {
        assert_eq!(
            run(&["run", "--"]),
            Ok(Run {
                account: None,
                model: None,
                resume: Vec::new(),
            })
        );
        let full = run(&[
            "run",
            "--account",
            "ac-work.2",
            "--home",
            "/Users/_owner/.alab/accounts/ac-work.2",
            "--model",
            "opus[1m]",
            "--",
            "--resume",
            "0b6f2c1e",
            "--model",
            "not-a-flag-here",
        ])
        .expect("a relaunch");
        assert_eq!(
            full.account,
            Some(Account {
                label: "ac-work.2".to_string(),
                home: Some(PathBuf::from("/Users/_owner/.alab/accounts/ac-work.2")),
            })
        );
        assert_eq!(full.model.as_deref(), Some("opus[1m]"));
        assert_eq!(
            full.resume,
            os(&["--resume", "0b6f2c1e", "--model", "not-a-flag-here"])
        );
        // The words write back as they were read.
        assert_eq!(
            full.args().expect("words"),
            os(&[
                "run",
                "--account",
                "ac-work.2",
                "--home",
                "/Users/_owner/.alab/accounts/ac-work.2",
                "--model",
                "opus[1m]",
                "--",
                "--resume",
                "0b6f2c1e",
                "--model",
                "not-a-flag-here",
            ])
        );
        assert_eq!(Run::parse(full.args().expect("words")), Ok(full));
        // Any order; an account without a home is the vendor's default home.
        let reordered = run(&[
            "run",
            "--model",
            "gemini-3-pro",
            "--account",
            "work",
            "--",
            "x",
        ])
        .expect("a relaunch");
        assert_eq!(
            reordered.account,
            Some(Account {
                label: "work".to_string(),
                home: None,
            })
        );
        assert_eq!(reordered.model.as_deref(), Some("gemini-3-pro"));
        assert_eq!(reordered.resume, os(&["x"]));
        // atpkg's envelope passes `run -- "$@"`: that line reads too.
        assert_eq!(
            run(&["run", "--", "--continue"]).map(|run| run.resume),
            Ok(os(&["--continue"]))
        );
    }

    #[test]
    fn the_run_grammar_refuses_what_it_does_not_name() {
        for (words, error) in [
            (&[][..], RunError::NotRun),
            (&["start", "--"][..], RunError::NotRun),
            (&["run"][..], RunError::NoSeparator),
            (&["run", "--account", "work"][..], RunError::NoSeparator),
            (
                &["run", "--profile", "x", "--"][..],
                RunError::Unknown("--profile".to_string()),
            ),
            (
                &["run", "--account=work", "--"][..],
                RunError::Unknown("--account=work".to_string()),
            ),
            (
                &["run", "resume-id", "--"][..],
                RunError::Unknown("resume-id".to_string()),
            ),
            (&["run", "--account"][..], RunError::Missing(ACCOUNT)),
            (&["run", "--account", "--"][..], RunError::Missing(ACCOUNT)),
            (
                &["run", "--model", "--home", "/h", "--"][..],
                RunError::Missing(MODEL),
            ),
            (
                &["run", "--account", "a", "--account", "b", "--"][..],
                RunError::Repeated(ACCOUNT),
            ),
            (
                &["run", "--model", "a", "--model", "b", "--"][..],
                RunError::Repeated(MODEL),
            ),
            (
                &["run", "--account", "Work", "--"][..],
                RunError::Malformed(ACCOUNT),
            ),
            (
                &["run", "--account", "help", "--"][..],
                RunError::Malformed(ACCOUNT),
            ),
            (
                &["run", "--account", "work", "--home", "relative/home", "--"][..],
                RunError::Malformed(HOME),
            ),
            (
                &["run", "--account", "work", "--home", "-", "--"][..],
                RunError::Malformed(HOME),
            ),
            (
                &["run", "--home", "/Users/_owner/h", "--"][..],
                RunError::HomeWithoutAccount,
            ),
            (
                &["run", "--model", "has space", "--"][..],
                RunError::Malformed(MODEL),
            ),
            (
                &["run", "--model", "-x", "--"][..],
                RunError::Malformed(MODEL),
            ),
        ] {
            assert_eq!(run(words), Err(error), "{words:?}");
        }
        // A value that is not text cannot be a label or a model.
        {
            let bad = OsString::from_vec(vec![b'w', 0xff]);
            assert_eq!(
                Run::parse([
                    OsString::from("run"),
                    OsString::from("--account"),
                    bad.clone(),
                    OsString::from("--"),
                ]),
                Err(RunError::Malformed(ACCOUNT))
            );
            // A home is a path: any bytes, once absolute.
            let home = OsString::from_vec(b"/Users/_owner/\xffhome".to_vec());
            let read = Run::parse([
                OsString::from("run"),
                OsString::from("--account"),
                OsString::from("work"),
                OsString::from("--home"),
                home.clone(),
                OsString::from("--"),
                bad.clone(),
            ])
            .expect("a relaunch");
            assert_eq!(
                read.account.and_then(|account| account.home),
                Some(PathBuf::from(home))
            );
            assert_eq!(read.resume, [bad]);
        }
        // Words that would not read back are not written.
        let unwritable = Run {
            account: Some(Account {
                label: "Not A Label".to_string(),
                home: None,
            }),
            model: None,
            resume: Vec::new(),
        };
        assert_eq!(unwritable.args(), Err(RunError::Malformed(ACCOUNT)));
        let unwritable = Run {
            account: None,
            model: Some("-flag".to_string()),
            resume: Vec::new(),
        };
        assert_eq!(unwritable.args(), Err(RunError::Malformed(MODEL)));
        assert_eq!(
            RunError::Unknown("--profile".to_string()).to_string(),
            "flag=--profile unknown"
        );
        assert_eq!(RunError::NoSeparator.to_string(), "--=absent");
    }
}
