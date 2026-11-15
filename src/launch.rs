// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! THE LAUNCHER'S BLOCKS, CARRIED INTO `run`: what atpkg's managed `kimi` launcher does before it
//! starts Kimi Code, done by the harness in its own process for a launch through atpkg's
//! `run --` envelope, in the launcher's order (aterm's harness interface design, section 2.1,
//! "The launcher keeps its other duties"). Once the harness leads the word, atpkg's launcher
//! keeps only the check that this build is live and `exec <tool> run -- "$@"`.
//!
//! Sources, each read at aterm `origin/main` 617e45d8c95:
//! - `crates/atpkg/src/vendor_direct/table.rs:797-868`: the `kimi` row: `self_update_verbs`,
//!   `yolo_flags`, `yolo_markers`, `yolo_after` (none), `yolo_skip`, `shim_env`, and
//!   `signin_gate: None`;
//! - `crates/atpkg/src/platform/mod.rs:609-640` (`twin_prelude_with_launcher`): the order, the
//!   self-update block first (`sh_selfupdate_prelude`, 970-1010), the sign-in block where the
//!   row has a gate (`sh_signin_prelude`, 720-900), then the yolo block
//!   (`sh_yolo_prelude_with_launcher`, 1023-1460), then the hand-off to the harness;
//! - `crates/atpkg/src/yolo.rs`: `decide` (340-423), the word scan `first_word` (244-290),
//!   `marker_matches` (82-96), kimi's `word_options` (211-227), the echo lines (425-445) and
//!   `strip_no_harness` (450-472), the Rust model aterm's own tests hold the `sh` block to
//!   (`platform/mod_sh_shim_tests.rs`, "the twin runs exactly what `decide` says");
//! - `crates/atpkg/src/selfupdate.rs:12-18`: the hidden verb `__selfupdate` and the re-entry
//!   guard `__ATPKG_SELFUPDATE_PID`.
//!
//! The four blocks:
//! 1. **The self-update intercept** ([`self_update`]): `kimi upgrade` or `kimi update` (the
//!    first word, as typed) is answered by aterm's package manager, `aterm pkg __selfupdate kimi
//!    <prefix> -- <words>`, which classifies the shape and runs `aterm pkg update kimi`.
//! 2. **The sign-in gate**: none. Kimi's row has `signin_gate: None`, and `aterm harness
//!    signin` serves `--cli grok` alone, so the launcher runs a signed-out Kimi launch as it
//!    is, and so does `run`.
//! 3. **The yolo block** ([`strip_no_harness`], [`decide`]): `--no-harness` and
//!    `--no-alab-harness` ahead of a literal `--` are taken off and set `ATERM_NO_HARNESS=1`;
//!    a typed `--yolo` becomes `--auto`; a launch that names nothing of its posture gets
//!    `--auto` in front, unless the launch leaves the harness out. The command as run is
//!    printed on standard error, byte for byte as the launcher prints it.
//! 4. **The shim's environment**: `KIMI_CODE_NO_AUTO_UPDATE=1` (the row's `shim_env`), which
//!    the launcher exports only after its hand-off, so a harness-led launch never had it from
//!    there. Every child the harness starts gets its build's `<build>.shim-env` (the same
//!    entries atpkg exports from that build's shim) and [`crate::kimi::HARNESS_ENV`] at spawn
//!    (`supervise.rs`, `main.rs` `pass_through`), so a relaunch onto another build takes that
//!    build's.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::env::Env;

/// The vendor's own self-update words, which atpkg's package manager answers instead
/// (`self_update_verbs`, exactly as typed as the first word).
pub const SELF_UPDATE_VERBS: [&str; 2] = ["upgrade", "update"];

/// atpkg's hidden verb for a self-update the launcher intercepted (`selfupdate::HIDDEN_VERB`).
pub const HIDDEN_VERB: &str = "__selfupdate";

/// The re-entry guard (`selfupdate::REENTRY_VAR`): set to the intercepting process's pid before
/// it hands over, so a forward that comes back through the launcher under the same pid (atpkg
/// runs the vendor's own `update --help` by `exec`) runs the launch instead of looping.
pub const REENTRY_VAR: &str = "__ATPKG_SELFUPDATE_PID";

/// The word the launcher's alias claims, exactly as typed (`yolo::WORD`).
pub const WORD: &str = "--yolo";

/// What a by-the-flag echo starts with (`yolo::ECHO_PREFIX`).
pub const ECHO_PREFIX: &str = "aterm: --yolo: running ";

/// What a by-default echo starts with (`yolo::DEFAULT_ECHO_PREFIX`).
pub const DEFAULT_ECHO_PREFIX: &str = "aterm: yolo (default): running ";

/// The words that leave the harness out of a launch (`yolo::strip_no_harness`).
pub const NO_HARNESS_WORDS: [&str; 2] = ["--no-harness", "--no-alab-harness"];

/// The flags the posture expands to (`yolo_flags`).
const FLAGS: [&str; 1] = ["--auto"];

/// Tokens that already decide the posture: a launch holding one and no `--yolo` runs as typed
/// (`yolo_markers`). A marker ending in `=` matches only its `=value` form; a short one also
/// matches its attached forms.
const MARKERS: [&str; 17] = [
    "--auto",
    "-y",
    "--yes",
    "--auto-approve",
    "--plan",
    "-p",
    "--prompt",
    "--prompt=",
    "-S",
    "--session",
    "--session=",
    "-r",
    "--resume",
    "--resume=",
    "-c",
    "--continue",
    "-C",
];

/// First words the default never lands on (`yolo_skip`); [`SELF_UPDATE_VERBS`] skip too.
const SKIP: [&str; 12] = [
    "export",
    "fork",
    "provider",
    "session",
    "acp",
    "web",
    "login",
    "doctor",
    "vis",
    "install-desktop",
    "app",
    "migrate",
];

/// kimi 2.3.1's leading options that take the next token as their value
/// (`yolo::word_options`, from `apps/kimi-code/src/cli/commands.ts`).
const VALUES: [&str; 9] = [
    "-m",
    "--model",
    "-p",
    "--prompt",
    "--agent",
    "--agent-file",
    "--add-dir",
    "--skills-dir",
    "--output-format",
];

/// Its options whose value is optional: the next token is theirs unless it starts with `-`.
const OPTIONAL: [&str; 4] = ["-S", "--session", "-r", "--resume"];

/// Its options that take no value.
const SWITCHES: [&str; 4] = ["-c", "--continue", "-C", "--plan"];

/// Kimi's own weaker spellings of the posture, which its parser refuses beside `--auto`: an
/// explicit `--yolo` drops each one ahead of the first word along with itself.
const OWN_YOLO: [&str; 3] = ["-y", "--yes", "--auto-approve"];

/// The word the launcher's lines name the program by.
const PROGRAM: &str = crate::kimi::PROGRAM;

fn named(set: &[&str], token: &[u8]) -> bool {
    set.iter().any(|word| word.as_bytes() == token)
}

/// The `aterm pkg __selfupdate kimi <prefix> -- <words>` that answers a self-update word, by
/// the launcher's rule: the first word is one of [`SELF_UPDATE_VERBS`], [`REENTRY_VAR`] does not
/// name this process (`pid`), and an executable `aterm` is on `PATH`. The command carries
/// [`REENTRY_VAR`]` = pid` and is meant to replace this process, so the pid the guard names is
/// the one a forward comes back under. `None` runs the launch as it is.
///
/// The launcher runs the `atpkg` it was laid by; the harness has no such path and runs the
/// `aterm` on `PATH`, as the launcher's own sign-in block does, with `pkg` ahead of the verb
/// (`aterm pkg <words>` is `atpkg <words>`).
pub fn self_update(args: &[OsString], env: &Env, prefix: &Path, pid: u32) -> Option<Command> {
    let first = args.first()?;
    if !named(&SELF_UPDATE_VERBS, first.as_bytes()) {
        return None;
    }
    let me = pid.to_string();
    if env
        .get(REENTRY_VAR)
        .is_some_and(|guard| guard == me.as_str())
    {
        return None;
    }
    let aterm = on_path(env, "aterm")?;
    let mut command = Command::new(aterm);
    command
        .arg("pkg")
        .arg(HIDDEN_VERB)
        .arg(PROGRAM)
        .arg(prefix)
        .arg("--")
        .args(args)
        .env(REENTRY_VAR, me);
    Some(command)
}

/// The first `name` on `env`'s `PATH` that is an executable regular file, as the launcher's
/// `[ -x … ]` reads one. An empty entry (the shell's current directory) is never searched.
fn on_path(env: &Env, name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    let path = env.get_nonempty("PATH")?;
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| {
            std::fs::metadata(candidate)
                .is_ok_and(|md| md.is_file() && md.permissions().mode() & 0o111 != 0)
        })
}

/// `args` without [`NO_HARNESS_WORDS`] ahead of a literal `--`, and whether one was there.
pub fn strip_no_harness(args: Vec<OsString>) -> (Vec<OsString>, bool) {
    let end = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    let mut stripped = false;
    let kept = args
        .into_iter()
        .enumerate()
        .filter_map(|(i, arg)| {
            if i < end && named(&NO_HARNESS_WORDS, arg.as_bytes()) {
                stripped = true;
                None
            } else {
                Some(arg)
            }
        })
        .collect();
    (kept, stripped)
}

/// Whether `token` is decided by `marker`, in every spelling Kimi's parser gives the same flag
/// (`yolo::marker_matches`): a long flag also matches its `--flag=value` form, and a short flag
/// its attached forms (`-phi`).
fn marker_matches(marker: &str, token: &[u8]) -> bool {
    let marker = marker.as_bytes();
    if let Some(word) = marker.strip_suffix(b"=") {
        return token.len() > word.len() && token.starts_with(word) && token[word.len()] == b'=';
    }
    if marker.len() == 2 && marker[0] == b'-' && marker[1] != b'-' {
        return token.starts_with(marker);
    }
    token == marker
        || token
            .strip_prefix(marker)
            .is_some_and(|rest| rest.first() == Some(&b'='))
}

/// Why the word scan stopped short of the first word (`yolo::Stop`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// An option the scan does not know, or a value missing at the end.
    Unknown,
    /// A typed `--yolo`, or one of [`OWN_YOLO`], sits right after a bare optional-value option
    /// and before a bare word: removing it would hand that word to the option as its value.
    Joins,
}

/// The position of the first word, past known leading options and their values
/// (`yolo::first_word`).
fn first_word(head: &[&[u8]]) -> Result<Option<usize>, Stop> {
    let removable = |a: &[u8]| a == WORD.as_bytes() || named(&OWN_YOLO, a);
    let mut value = false;
    let mut maybe = false;
    let mut gap = false;
    for (i, &a) in head.iter().enumerate() {
        if value {
            value = false;
            continue;
        }
        if maybe {
            maybe = false;
            if !a.starts_with(b"-") {
                // The optional value.
                continue;
            }
            gap = removable(a);
        } else if gap && !removable(a) {
            if !a.starts_with(b"-") {
                return Err(Stop::Joins);
            }
            gap = false;
        }
        if removable(a) || named(&FLAGS, a) || named(&SWITCHES, a) {
            continue;
        }
        if named(&VALUES, a) {
            value = true;
        } else if named(&OPTIONAL, a) {
            maybe = true;
        } else if VALUES
            .iter()
            .chain(OPTIONAL.iter())
            .any(|flag| marker_matches(flag, a))
        {
            // A long `=value` or an attached short value is already consumed.
        } else if a.starts_with(b"-") {
            return Err(Stop::Unknown);
        } else {
            return Ok(Some(i));
        }
    }
    if value { Err(Stop::Unknown) } else { Ok(None) }
}

/// What a launch runs as ([`decide`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// The words as typed, nothing printed.
    AsTyped,
    /// `--yolo` was typed: the words with it expanded.
    Flag(Vec<OsString>),
    /// Nothing decided the posture: the words with [`FLAGS`] in front.
    Default(Vec<OsString>),
}

/// Decide `kimi <args…>` as the launcher's yolo block does (`yolo::decide` for `kimi`), reading
/// the words ahead of a literal `--`. A scan that stops at a removal that would join a bare word
/// to an optional value runs as typed, and so does an unknown leading option without a typed
/// `--yolo`. Else a typed `--yolo` becomes `--auto` (once), dropping Kimi's own spellings of
/// the posture ahead of the first word; else a marker, a skip word or a self-update word runs
/// as typed; else `--auto` goes in front.
pub fn decide(args: &[OsString]) -> Launch {
    let head_len = args.iter().take_while(|arg| *arg != "--").count();
    let head: Vec<&[u8]> = args[..head_len].iter().map(|a| a.as_bytes()).collect();
    let word = first_word(&head);
    let explicit = head.iter().any(|a| *a == WORD.as_bytes());
    match word {
        Err(Stop::Joins) => return Launch::AsTyped,
        Err(Stop::Unknown) if !explicit => return Launch::AsTyped,
        _ => {}
    }
    // The leading options an explicit `--yolo` may drop Kimi's own spellings from: none past an
    // option the scan does not know.
    let leading = match word {
        Ok(Some(i)) => i,
        Ok(None) => head_len,
        Err(_) => 0,
    };
    let word = word.ok().flatten();
    if explicit {
        let mut present: Vec<&[u8]> = head.clone();
        let mut out: Vec<OsString> = Vec::with_capacity(args.len() + FLAGS.len());
        for (i, (arg, &a)) in args.iter().zip(&head).enumerate() {
            if a == WORD.as_bytes() {
                for flag in FLAGS {
                    if !present.contains(&flag.as_bytes()) {
                        present.push(flag.as_bytes());
                        out.push(OsString::from(flag));
                    }
                }
            } else if i < leading && named(&OWN_YOLO, a) {
                // Kimi's own spelling of the posture the flag sets.
            } else {
                out.push(arg.clone());
            }
        }
        out.extend(args[head_len..].iter().cloned());
        return Launch::Flag(out);
    }
    if head
        .iter()
        .any(|a| MARKERS.iter().any(|marker| marker_matches(marker, a)))
    {
        return Launch::AsTyped;
    }
    if let Some(i) = word {
        // The self-update words skip too: that intercept decides an update shape first, and
        // the default must not land on what it forwards (`kimi update --help`).
        if named(&SKIP, head[i]) || named(&SELF_UPDATE_VERBS, head[i]) {
            return Launch::AsTyped;
        }
    }
    let mut out: Vec<OsString> = FLAGS.iter().map(OsString::from).collect();
    out.extend(args.iter().cloned());
    Launch::Default(out)
}

/// `s` in the launcher's single quotes (`platform::sh_quote_str`): `'` becomes `'\''`.
fn sh_quote(s: &[u8], out: &mut Vec<u8>) {
    out.push(b'\'');
    for &byte in s {
        if byte == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(byte);
        }
    }
    out.push(b'\'');
}

/// The one standard-error line the launcher prints for a launch it changed, with its newline:
/// `prefix`, then the program and each word in single quotes (`yolo::echo_line` and
/// `default_echo_line`; the `sh` block prints the same bytes).
fn echo(prefix: &str, args: &[OsString]) -> Vec<u8> {
    let mut line = prefix.as_bytes().to_vec();
    sh_quote(PROGRAM.as_bytes(), &mut line);
    for arg in args {
        line.push(b' ');
        sh_quote(arg.as_bytes(), &mut line);
    }
    line.push(b'\n');
    line
}

/// What the yolo block makes of `args` for a launch that `left_out` the harness or not: the
/// words to run and the line to print, or `None` to run them as typed. A typed `--yolo` is
/// expanded either way, as the launcher writes it; the default is the harness's, and a launch
/// that leaves the harness out (`--no-harness`, or `ATERM_NO_HARNESS=1`, the variable the
/// launcher turns the word into for every later block) gets none.
pub fn yolo(args: &[OsString], left_out: bool) -> Option<(Vec<OsString>, Vec<u8>)> {
    match decide(args) {
        Launch::AsTyped => None,
        Launch::Flag(run) => {
            let line = echo(ECHO_PREFIX, &run);
            Some((run, line))
        }
        Launch::Default(_) if left_out => None,
        Launch::Default(run) => {
            let line = echo(DEFAULT_ECHO_PREFIX, &run);
            Some((run, line))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn words(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn d(typed: &[&str]) -> Launch {
        decide(&words(typed))
    }

    /// aterm's `grok_and_kimi_take_their_one_flag_where_their_parsers_read_it`, every kimi case
    /// (yolo_tests.rs:515-702), with the answers aterm's `yolo::decide` gives.
    #[test]
    fn the_yolo_block_decides_as_atpkgs_decide_does_for_kimi() {
        assert_eq!(d(&[]), Launch::Default(words(&["--auto"])));
        assert_eq!(d(&["--yolo"]), Launch::Flag(words(&["--auto"])));
        assert_eq!(
            d(&["-m", "k2", "--yolo", "--", "--yolo"]),
            Launch::Flag(words(&["-m", "k2", "--auto", "--", "--yolo"]))
        );
        assert_eq!(
            d(&["--agent", "coder", "hello"]),
            Launch::Default(words(&["--auto", "--agent", "coder", "hello"]))
        );
        for decided in [
            &["--auto"][..],
            &["-y"],
            &["--yes"],
            &["--auto-approve"],
            &["--plan"],
            &["-p", "hi"],
            &["--prompt=hi"],
            &["-S", "abc"],
            &["--session=abc"],
            &["-r"],
            &["--resume=abc"],
            &["-c"],
            &["--continue"],
            &["-C"],
        ] {
            assert_eq!(d(decided), Launch::AsTyped, "{decided:?}");
        }
        for typed in [
            &["--yolo", "-y"][..],
            &["-y", "--yolo"],
            &["--yes", "--yolo", "--auto-approve"],
            &["--yolo", "--auto", "-y"],
        ] {
            assert_eq!(d(typed), Launch::Flag(words(&["--auto"])), "{typed:?}");
        }
        assert_eq!(
            d(&["-y", "-m", "k2", "--yolo", "--add-dir", "/w"]),
            Launch::Flag(words(&["-m", "k2", "--auto", "--add-dir", "/w"]))
        );
        assert_eq!(
            d(&["--yolo", "upgrade", "-y"]),
            Launch::Flag(words(&["--auto", "upgrade", "-y"])),
            "a subcommand's own -y is its own"
        );
        assert_eq!(
            d(&["--yolo", "-y", "--", "-y"]),
            Launch::Flag(words(&["--auto", "--", "-y"]))
        );
        assert_eq!(
            d(&["--yolo", "-S", "-y", "login"]),
            Launch::AsTyped,
            "dropping -y would hand `login` to -S as a session id"
        );
        assert_eq!(
            d(&["--yolo", "-S", "abc", "-y"]),
            Launch::Flag(words(&["--auto", "-S", "abc"]))
        );
        for word in [
            "login", "acp", "web", "provider", "export", "upgrade", "update",
        ] {
            assert_eq!(d(&[word]), Launch::AsTyped, "{word}");
        }
        assert_eq!(
            d(&["--unknown", "--yolo"]),
            Launch::Flag(words(&["--unknown", "--auto"]))
        );
        // The twin cases aterm's mod_sh_shim_tests.rs:830-850 runs through the laid `sh` block.
        assert_eq!(d(&["-phi"]), Launch::AsTyped);
        assert_eq!(d(&["acp"]), Launch::AsTyped);
        assert_eq!(
            d(&["--unknown", "--yolo", "-y"]),
            Launch::Flag(words(&["--unknown", "--auto", "-y"])),
            "nothing past an option the scan does not know is dropped"
        );
        assert_eq!(d(&["--unknown"]), Launch::AsTyped);
        assert_eq!(d(&["--model"]), Launch::AsTyped, "a value is missing");
    }

    #[test]
    fn a_word_that_is_not_utf8_is_read_as_its_bytes() {
        use std::os::unix::ffi::OsStringExt as _;
        let odd = OsString::from_vec(vec![b'h', 0xff, b'i']);
        assert_eq!(
            decide(std::slice::from_ref(&odd)),
            Launch::Default(vec![OsString::from("--auto"), odd.clone()])
        );
        let mut dashed = vec![b'-'];
        dashed.push(0xff);
        assert_eq!(
            decide(&[OsString::from_vec(dashed)]),
            Launch::AsTyped,
            "an option the scan does not know"
        );
    }

    /// aterm's `the_echo_lines_name_the_command_as_run` (yolo_tests.rs:450-462), for kimi.
    #[test]
    fn the_echo_lines_are_the_launchers_bytes() {
        assert_eq!(
            echo(ECHO_PREFIX, &words(&["--auto"])),
            b"aterm: --yolo: running 'kimi' '--auto'\n"
        );
        assert_eq!(
            echo(
                DEFAULT_ECHO_PREFIX,
                &words(&["--auto", "-p", "it's $x", ""])
            ),
            b"aterm: yolo (default): running 'kimi' '--auto' '-p' 'it'\\''s $x' ''\n".to_vec()
        );
    }

    #[test]
    fn the_default_is_left_out_with_the_harness_and_the_flag_is_not() {
        assert_eq!(
            yolo(&words(&["hello"]), false),
            Some((
                words(&["--auto", "hello"]),
                b"aterm: yolo (default): running 'kimi' '--auto' 'hello'\n".to_vec()
            ))
        );
        assert_eq!(yolo(&words(&["hello"]), true), None);
        assert_eq!(
            yolo(&words(&["--yolo"]), true),
            Some((
                words(&["--auto"]),
                b"aterm: --yolo: running 'kimi' '--auto'\n".to_vec()
            ))
        );
        assert_eq!(yolo(&words(&["--plan"]), false), None);
    }

    /// `yolo::strip_no_harness` (yolo_tests.rs:463-478): only ahead of a literal `--`.
    #[test]
    fn the_no_harness_words_are_taken_off_ahead_of_the_separator() {
        assert_eq!(
            strip_no_harness(words(&["--no-harness", "-m", "k2"])),
            (words(&["-m", "k2"]), true)
        );
        assert_eq!(
            strip_no_harness(words(&["--no-alab-harness", "--", "--no-harness"])),
            (words(&["--", "--no-harness"]), true)
        );
        assert_eq!(
            strip_no_harness(words(&["--", "--no-harness"])),
            (words(&["--", "--no-harness"]), false)
        );
        assert_eq!(strip_no_harness(words(&[])), (words(&[]), false));
    }

    fn lay_aterm(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::create_dir_all(dir).unwrap();
        let aterm = dir.join("aterm");
        std::fs::write(&aterm, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&aterm, std::fs::Permissions::from_mode(0o755)).unwrap();
        aterm
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kimi-alab-launch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_self_update_word_is_handed_to_aterms_package_manager() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = scratch("update");
        let plain = root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("aterm"), "not executable").unwrap();
        std::fs::set_permissions(plain.join("aterm"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        let aterm = lay_aterm(&root.join("bin"));
        let path = std::env::join_paths([plain, root.join("bin")]).unwrap();
        let env = Env::from_pairs([("PATH", path.clone())]);
        let prefix = Path::new("/p/pkg");
        let command = self_update(&words(&["update", "--help"]), &env, prefix, 42).unwrap();
        assert_eq!(command.get_program(), aterm.as_os_str());
        let argv: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(
            argv,
            [
                "pkg",
                "__selfupdate",
                "kimi",
                "/p/pkg",
                "--",
                "update",
                "--help"
            ]
        );
        let set: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert_eq!(set, [(OsStr::new(REENTRY_VAR), Some(OsStr::new("42")))]);
        assert!(self_update(&words(&["upgrade"]), &env, prefix, 42).is_some());
        // Only the first word, exactly as typed.
        for typed in [&["-m", "k2", "update"][..], &["Update"], &["updates"], &[]] {
            assert!(
                self_update(&words(typed), &env, prefix, 42).is_none(),
                "{typed:?}"
            );
        }
        // The guard names this process: a forward came back.
        let back = Env::from_pairs([("PATH", path.clone()), (REENTRY_VAR, OsString::from("42"))]);
        assert!(self_update(&words(&["update"]), &back, prefix, 42).is_none());
        let other = Env::from_pairs([("PATH", path), (REENTRY_VAR, OsString::from("41"))]);
        assert!(self_update(&words(&["update"]), &other, prefix, 42).is_some());
        // No executable aterm on PATH, or an empty entry only: the launch runs as it is.
        let none = Env::from_pairs([("PATH", root.join("plain"))]);
        assert!(self_update(&words(&["update"]), &none, prefix, 42).is_none());
        assert!(self_update(&words(&["update"]), &Env::default(), prefix, 42).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
