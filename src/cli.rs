// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The harness's own command line. Every word is the vendor's except, at the very front,
//! `--version`/`-V` alone, words in the `--alab-` namespace, and atpkg's `run --` envelope.

use std::ffi::OsString;

use crate::store::{Build, StoreError};

/// The harness's name and version.
pub const NAME: &str = "kimi-alab";

/// This harness's version, independent of the vendor CLI's: the version an atpkg pack signs
/// (`MAJOR.MINOR.0+g<sha9>`), which `atpkg-publish pack` hands the build as
/// `ALAB_HARNESS_VERSION` (aterm's harness design, D46), else the package's `MAJOR.MINOR.0`. A
/// value that is not this package's line ([`is_pack_version`]) fails the build.
pub const VERSION: &str = match option_env!("ALAB_HARNESS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

const _: () = assert!(
    is_pack_version(VERSION, env!("CARGO_PKG_VERSION")),
    "ALAB_HARNESS_VERSION is not this package's MAJOR.MINOR.0, alone or with +g<sha>"
);

const fn release_component(bytes: &[u8]) -> Option<&[u8]> {
    let (first, mut rest) = match bytes.split_first() {
        Some((&first, rest)) if first.is_ascii_digit() => (first, rest),
        _ => return None,
    };
    while let Some((&digit, tail)) = rest.split_first() {
        if digit == b'.' {
            return Some(tail);
        }
        if first == b'0' || !digit.is_ascii_digit() {
            return None;
        }
        rest = tail;
    }
    None
}

const fn release_base(bytes: &[u8]) -> bool {
    let Some(after_major) = release_component(bytes) else {
        return false;
    };
    let Some(after_minor) = release_component(after_major) else {
        return false;
    };
    matches!(after_minor, [b'0'])
}

/// Whether `v` is the `MAJOR.MINOR.0` release line `line` alone, or `line` followed by `+g` and
/// 9 to 40 lowercase hex digits (a dev build's commit): the only versions a pack may hand the
/// build.
#[must_use]
pub const fn is_pack_version(v: &str, line: &str) -> bool {
    let mut base = line.as_bytes();
    if !release_base(base) {
        return false;
    }
    let mut rest = v.as_bytes();
    while let Some((&expected, tail)) = base.split_first() {
        rest = match rest.split_first() {
            Some((&actual, tail)) if actual == expected => tail,
            _ => return false,
        };
        base = tail;
    }
    if rest.is_empty() {
        return true;
    }
    rest = match rest {
        [b'+', b'g', stamp @ ..] => stamp,
        _ => return false,
    };
    if !matches!(rest.len(), 9..=40) {
        return false;
    }
    while let Some((&digit, tail)) = rest.split_first() {
        if !matches!(digit, b'0'..=b'9' | b'a'..=b'f') {
            return false;
        }
        rest = tail;
    }
    true
}

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `--version` or `-V` alone: the harness and vendor versions.
    Version,
    /// `--alab-help`.
    Help,
    /// `--alab-which`: what this run would execute, and why.
    Which,
    /// Run the vendor CLI with `args`.
    Run {
        /// `--alab-no-update`: never move the child onto a new build.
        no_update: bool,
        /// The vendor's words, verbatim.
        args: Vec<OsString>,
    },
}

/// Read the command line (without the program name).
///
/// # Errors
/// An `--alab-` word this version does not know, or atpkg's `run` without its literal `--`.
pub fn parse(args: Vec<OsString>) -> Result<Command, String> {
    if args.first().is_some_and(|word| word == "run") {
        if args.get(1).is_none_or(|word| word != "--") {
            return Err(format!("{NAME}: `run` must be followed by `--`"));
        }
        return Ok(Command::Run {
            no_update: false,
            args: args.into_iter().skip(2).collect(),
        });
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        return Ok(Command::Version);
    }
    let mut no_update = false;
    let mut rest = args.into_iter().peekable();
    while let Some(word) = rest.peek() {
        let Some(s) = word.to_str() else { break };
        if !s.starts_with("--alab-") {
            break;
        }
        match s {
            "--alab-help" => return Ok(Command::Help),
            "--alab-which" => return Ok(Command::Which),
            "--alab-no-update" => no_update = true,
            other => {
                return Err(format!(
                    "{NAME}: unknown option {other:?} (see `{NAME} --alab-help`)"
                ));
            }
        }
        rest.next();
    }
    Ok(Command::Run {
        no_update,
        args: rest.collect(),
    })
}

/// What `--version` says when atpkg has no `current` build of the vendor.
pub const NOT_INSTALLED: &str = "not installed";

/// What `--version` says when the vendor's version cannot be read: no readable store, a
/// `current` that names no complete build, or a `.vendor` record that is missing, unreadable or
/// names another version than its build.
pub const UNKNOWN: &str = "unknown";

/// The `--version` line, `kimi-alab <version> (kimi <vendor version>)`, from what the store
/// says of its `current` build: the version its `.vendor` record names for that build
/// ([`crate::store::build_version`]), else `(kimi not installed)` or `(kimi unknown)`. The
/// vendor binary is never run for it, and a version is never decoded from the build id alone.
pub fn version_line(current: &Result<Build, StoreError>) -> String {
    let vendor = match current {
        Ok(b) => b.version.as_deref().unwrap_or(UNKNOWN),
        Err(StoreError::NotInstalled { .. }) => NOT_INSTALLED,
        Err(_) => UNKNOWN,
    };
    format!("{NAME} {VERSION} ({} {vendor})", crate::kimi::COMMAND)
}

/// The text of `--alab-help`.
pub fn help() -> String {
    format!(
        "\
{NAME} {VERSION} — the ALab harness for {product}

USAGE
  {NAME} [--alab-no-update] [kimi arguments…]
  {NAME} run -- [kimi arguments…]  atpkg's managed-launch envelope
  {NAME} --version | -V        the harness and {cmd} versions
  {NAME} --alab-which          what would run, from where, and why
  {NAME} --alab-help           this text

{NAME} runs the unmodified `{cmd}` from the atpkg store (never from PATH) as its child on
this terminal, passes every argument through, and exits with the child's own status.

When atpkg makes a different {cmd} build current while the session runs, {NAME} waits for a
safe point and resumes the same session on the new build (`{cmd} … --session <id>`) without
exiting. A new {NAME} build crosses the same safe point by exec: the harness keeps its pid and
the child runs on, with its session and account state carried to the new image. Under a proven
aterm contract-3 claim (scope=relaunch,upgrade), aterm's own idle and empty-composer status
supplies the attestation. Elsewhere `kill -USR1 <{NAME} pid>` supplies it. Both then need
{quiet} s with no keystroke read, nothing drawn, no process started or ended under {cmd}, and
almost no CPU. A busy {cmd} is never stopped. If aterm cannot prove the claim, {NAME} execs {cmd}
in its place instead of leaving two supervisors.

ENVIRONMENT
  KIMI_ALAB_PKG_PREFIX   the atpkg prefix (default: aterm.toml [packages] prefix, else
                         atpkg's default under $HOME)
  KIMI_ALAB_JOURNAL      the journal file, or `off`
",
        product = crate::kimi::PRODUCT,
        cmd = crate::kimi::COMMAND,
        quiet = crate::safepoint::Knobs::defaults().quiet.as_secs(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn version_is_the_harness_only_when_alone() {
        assert_eq!(parse(os(&["--version"])).unwrap(), Command::Version);
        assert_eq!(parse(os(&["-V"])).unwrap(), Command::Version);
        assert_eq!(
            parse(os(&["--version", "x"])).unwrap(),
            Command::Run {
                no_update: false,
                args: os(&["--version", "x"])
            }
        );
    }

    #[test]
    fn atpkg_run_envelope_passes_every_following_word_to_the_vendor() {
        assert_eq!(
            parse(os(&["run", "--", "--version"])).unwrap(),
            Command::Run {
                no_update: false,
                args: os(&["--version"])
            }
        );
        assert_eq!(
            parse(os(&["run", "--", "--alab-no-update", "", "a b"])).unwrap(),
            Command::Run {
                no_update: false,
                args: os(&["--alab-no-update", "", "a b"])
            }
        );
        assert!(parse(os(&["run"])).is_err());
        assert!(parse(os(&["run", "--version"])).is_err());
    }

    #[test]
    fn alab_words_lead_and_everything_after_is_the_vendors() {
        assert_eq!(parse(os(&["--alab-which"])).unwrap(), Command::Which);
        assert_eq!(parse(os(&["--alab-help", "x"])).unwrap(), Command::Help);
        assert_eq!(
            parse(os(&["--alab-no-update", "-y", "--alab-which"])).unwrap(),
            Command::Run {
                no_update: true,
                args: os(&["-y", "--alab-which"])
            }
        );
        assert_eq!(
            parse(os(&[])).unwrap(),
            Command::Run {
                no_update: false,
                args: vec![]
            }
        );
        assert!(parse(os(&["--alab-frob"])).is_err());
    }

    #[test]
    fn the_version_line_names_the_vendor_version() {
        let b = Build {
            id: 1_000_002_000_003_000_001,
            dir: "/s/kimi/1".into(),
            exe: "/s/kimi/1/bin/kimi".into(),
            version: Some("2.3.1".into()),
            shim_env: vec![],
        };
        assert_eq!(
            version_line(&Ok(b.clone())),
            format!("kimi-alab {VERSION} (kimi 2.3.1)")
        );
        assert_eq!(
            version_line(&Ok(Build { version: None, ..b })),
            format!("kimi-alab {VERSION} (kimi unknown)")
        );
        assert_eq!(
            version_line(&Err(StoreError::NotInstalled {
                program: "kimi".into(),
                current: "/s/kimi/current".into(),
            })),
            format!("kimi-alab {VERSION} (kimi not installed)")
        );
        for err in [
            StoreError::NoHome,
            StoreError::BadPrefixEnv("rel".into()),
            StoreError::Config("dotted".into()),
            StoreError::Layout("no build".into()),
            StoreError::Incomplete("/s/kimi/1".into()),
            StoreError::NoBinary("/s/kimi/1/bin/kimi".into()),
        ] {
            assert_eq!(
                version_line(&Err(err.clone())),
                format!("kimi-alab {VERSION} (kimi unknown)"),
                "{err:?}"
            );
        }
        let line = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = line.split('.').collect();
        assert_eq!(parts.len(), 3, "MAJOR.MINOR.0: {line}");
        assert_eq!(parts[2], "0", "the patch slot is always 0: {line}");
    }

    #[test]
    fn a_pack_hands_the_release_line_or_the_line_and_its_commit() {
        let line = "0.1.0";
        for (v, ok) in [
            ("0.1.0", true),
            ("0.1.0+gabcdef012", true),
            ("0.1.0+g0123456789abcdef0123456789abcdef01234567", true),
            ("0.1.0+gabcdef01", false),
            ("0.1.0+g0123456789abcdef0123456789abcdef012345678", false),
            ("0.1.0+gABCDEF012", false),
            ("0.1.0+gabcdef01z", false),
            ("0.1.0+abcdef012", false),
            ("0.1.0+g", false),
            ("0.1.0+", false),
            ("0.1.0-dev.3+gabcdef012", false),
            ("0.2.0+gabcdef012", false),
            ("0.1", false),
            ("", false),
        ] {
            assert_eq!(is_pack_version(v, line), ok, "{v:?}");
        }
        for bad_line in ["0.1.2", "01.1.0", "0.01.0", "0.1", "0.1.0.0", ""] {
            assert!(!is_pack_version(bad_line, bad_line), "{bad_line:?}");
            assert!(
                !is_pack_version(&format!("{bad_line}+gabcdef012"), bad_line),
                "{bad_line:?}"
            );
        }
        assert!(is_pack_version("10.20.0+gabcdef012", "10.20.0"));
        assert!(is_pack_version(VERSION, env!("CARGO_PKG_VERSION")));
        assert_eq!(
            VERSION,
            option_env!("ALAB_HARNESS_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
        );
    }
}
