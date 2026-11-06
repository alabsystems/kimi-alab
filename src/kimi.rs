// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! KIMI CODE, THE VENDOR PROFILE: what the harness knows about Moonshot AI's `kimi`.
//!
//! Sources, each read, none guessed:
//! - aterm 5e2c6d0c28 `crates/atpkg/src/vendor_direct/table.rs:659-735`: atpkg's `kimi` row —
//!   program and exposed command `kimi`, `shim_env` `KIMI_CODE_NO_AUTO_UPDATE=1`, floor 2.3.1;
//! - aterm `crates/aterm-agent/src/harness/relaunch.rs:2512-2545` (`resume_argv`, `pool_line`):
//!   aterm resumes a Kimi session as `--session <id>`, the pair AFTER the launch flags it
//!   carries. That is the resume grammar this harness ports; aterm has no Kimi relaunch lane
//!   (`harness/lanes.rs:51`, `Program::Kimi => Lanes::NONE`), so nothing else exists to port.
//!   Kimi itself prints the same resume, spelled with its hidden alias: `kimi -r <id>`
//!   (kimi-code 2.3.1 `apps/kimi-code/src/cli/run-shell.ts:248`);
//! - aterm `crates/aterm-agent/src/harness/upgrade_agy.rs:44-200`: the shape of a carried argv
//!   (a flag table where every flag has a fate on a relaunch, an unknown flag refusing it);
//! - kimi-code 2.3.1 (MoonshotAI/kimi-code, MIT) `apps/kimi-code/src/cli/commands.ts`: the flag
//!   table and the subcommands, parsed by commander;
//! - kimi-code 2.3.1 `apps/kimi-code/src/tui/kimi-tui.ts:1050-1053,2916-2930`: on a resumed
//!   session, `--auto/--yolo/--plan` and `-m` OVERRIDE what the session persisted;
//! - kimi-code 2.3.1 `packages/agent-core-v2/src/persistence/backends/node-fs/sessionLockService.ts`:
//!   a running Kimi holds `<home>/locks/sessions/<session id>.lock`, a JSON object
//!   `{version: 1, token, pid, hostname, startedAt, clientIdentity}`, released at exit;
//!   `KIMI_CODE_SESSION_LOCK` set false turns locks off;
//! - kimi-code 2.3.1 `packages/agent-core-v2/src/app/bootstrap/bootstrap.ts:168-174`: the home
//!   is `$KIMI_CODE_HOME`, else `~/.kimi-code`;
//! - kimi-code 2.3.1 `apps/kimi-code/src/tui/kimi-tui.ts:1170-1201`: SIGTERM is a graceful stop
//!   (the session is closed, the terminal restored) that exits 143; SIGHUP exits 129.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::env::Env;

/// The atpkg store program (`store/kimi`).
pub const PROGRAM: &str = "kimi";

/// The vendor's command, the file under `bin/` (`exposes: &["kimi"]`).
pub const COMMAND: &str = "kimi";

/// The vendor product name, for messages.
pub const PRODUCT: &str = "Kimi Code";

/// The environment the harness always gives the child, over the build's `shim-env`: the harness
/// owns updates, so Kimi's own updater stays off (`KIMI_CODE_NO_AUTO_UPDATE=1`, the value atpkg's
/// `SELF_UPDATE_SWITCHES` and shim use).
pub const HARNESS_ENV: &[(&str, &str)] = &[("KIMI_CODE_NO_AUTO_UPDATE", "1")];

/// The variable that moves Kimi's home.
pub const HOME_ENV: &str = "KIMI_CODE_HOME";

/// Kimi's home under `$HOME` when [`HOME_ENV`] is unset.
pub const DEFAULT_HOME: &str = ".kimi-code";

/// The variable that turns Kimi's session locks off when false.
pub const SESSION_LOCK_ENV: &str = "KIMI_CODE_SESSION_LOCK";

/// The flag a relaunch resumes the session with (aterm `relaunch.rs:2518`).
pub const RESUME_FLAG: &str = "--session";

/// The exit status of Kimi's graceful SIGTERM stop (`kimi-tui.ts:1186-1194`).
pub const TERM_EXIT: i32 = 143;

/// How a flag takes its value (commander's three option kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arity {
    /// A boolean: never takes a value.
    Switch,
    /// `<value>`: attached or the next token, whatever it looks like.
    Required,
    /// `[value]`: attached, or the next token unless it starts with `-`.
    Optional,
}

/// What a relaunch does with a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fate {
    /// Carried verbatim, as typed.
    Keep,
    /// Dropped with its value.
    Drop,
    /// The launch is no interactive session (it prints and exits): never relaunched.
    OneShot,
}

/// One flag of `kimi` 2.3.1.
struct Flag {
    long: Option<&'static str>,
    short: Option<char>,
    arity: Arity,
    fate: Fate,
}

const fn flag(long: Option<&'static str>, short: Option<char>, arity: Arity, fate: Fate) -> Flag {
    Flag {
        long,
        short,
        arity,
        fate,
    }
}

/// `kimi` 2.3.1's flags (`commands.ts`), each with its fate on a relaunch that resumes the
/// running session:
/// - the resume flags are dropped: the relaunch names the session itself;
/// - `--yolo` and its aliases, `--auto`, `--plan` and `--model` are dropped, because on a resumed
///   session they override what the session persisted (`kimi-tui.ts:1050,2916`) and would undo a
///   mode or model the person changed during the session;
/// - `--agent`/`--agent-file` are dropped: Kimi refuses them beside `--session`, and restores
///   the bound agent on resume (`options.ts` `validateOptions`);
/// - `--skills-dir` and `--add-dir` are per-launch and kept;
/// - print mode, help and version are one-shots.
const FLAGS: &[Flag] = &[
    flag(Some("session"), Some('S'), Arity::Optional, Fate::Drop),
    flag(Some("resume"), Some('r'), Arity::Optional, Fate::Drop),
    flag(Some("continue"), Some('c'), Arity::Switch, Fate::Drop),
    flag(None, Some('C'), Arity::Switch, Fate::Drop),
    flag(Some("yolo"), Some('y'), Arity::Switch, Fate::Drop),
    flag(Some("yes"), None, Arity::Switch, Fate::Drop),
    flag(Some("auto-approve"), None, Arity::Switch, Fate::Drop),
    flag(Some("auto"), None, Arity::Switch, Fate::Drop),
    flag(Some("plan"), None, Arity::Switch, Fate::Drop),
    flag(Some("model"), Some('m'), Arity::Required, Fate::Drop),
    flag(Some("agent"), None, Arity::Required, Fate::Drop),
    flag(Some("agent-file"), None, Arity::Required, Fate::Drop),
    flag(Some("skills-dir"), None, Arity::Required, Fate::Keep),
    flag(Some("add-dir"), None, Arity::Required, Fate::Keep),
    flag(Some("prompt"), Some('p'), Arity::Required, Fate::OneShot),
    flag(Some("output-format"), None, Arity::Required, Fate::OneShot),
    flag(Some("version"), Some('V'), Arity::Switch, Fate::OneShot),
    flag(Some("help"), Some('h'), Arity::Switch, Fate::OneShot),
];

/// `kimi` 2.3.1's subcommands (`commands.ts`, its `register*Command` calls, and commander's
/// implicit `help`): each a tool that exits, never the interactive session.
const SUBCOMMANDS: &[&str] = &[
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
    "upgrade",
    "update",
    "help",
    "__plugin_run_node",
    "__update_download",
];

fn by_long(name: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.long == Some(name))
}

fn by_short(c: char) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.short == Some(c))
}

/// Why a launch's argv cannot be carried onto a relaunch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A flag `kimi` 2.3.1 does not have (or a spelling of one commander would refuse).
    UnknownFlag(String),
    /// The launch is no interactive session: a one-shot flag, a subcommand or an operand.
    NotInteractive(String),
    /// A word that is not UTF-8.
    NotUtf8,
    /// The session id is not one Kimi writes.
    BadSession(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownFlag(w) => write!(
                f,
                "the launch carries {w:?}, a flag kimi-alab does not know how to carry"
            ),
            Self::NotInteractive(w) => write!(f, "the launch is not an interactive session ({w})"),
            Self::NotUtf8 => write!(f, "the launch carries a word that is not UTF-8"),
            Self::BadSession(s) => write!(f, "{s:?} is not a Kimi session id"),
        }
    }
}

/// The words of `args` (the launch's argv after the program) that a relaunch carries, verbatim
/// and in order, read as commander reads them.
///
/// # Errors
/// [`Refusal::UnknownFlag`] or [`Refusal::NotInteractive`].
pub fn carried(args: &[String]) -> Result<Vec<String>, Refusal> {
    let mut kept = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let tok = args[i].as_str();
        i += 1;
        if tok == "--" {
            return match args.get(i) {
                Some(_) => Err(Refusal::NotInteractive(String::from(
                    "an operand after `--`",
                ))),
                None => Ok(kept),
            };
        }
        if let Some(long) = tok.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            let f = by_long(name).ok_or_else(|| Refusal::UnknownFlag(tok.to_string()))?;
            let mut words = vec![tok.to_string()];
            match (f.arity, attached) {
                (Arity::Switch, Some(_)) => return Err(Refusal::UnknownFlag(tok.to_string())),
                (Arity::Switch, None) | (Arity::Required | Arity::Optional, Some(_)) => {}
                (Arity::Required, None) => {
                    let v = args
                        .get(i)
                        .ok_or_else(|| Refusal::UnknownFlag(tok.to_string()))?;
                    words.push(v.clone());
                    i += 1;
                }
                (Arity::Optional, None) => {
                    if let Some(v) = args.get(i).filter(|v| !is_option_like(v)) {
                        words.push(v.clone());
                        i += 1;
                    }
                }
            }
            apply(f, words, &mut kept)?;
            continue;
        }
        if let Some(shorts) = tok.strip_prefix('-').filter(|s| !s.is_empty()) {
            // `-abc`: commander expands booleans one by one; the first short that takes a value
            // takes the rest of the token as it.
            for (at, c) in shorts.char_indices() {
                let f = by_short(c).ok_or_else(|| Refusal::UnknownFlag(format!("-{c}")))?;
                let rest = &shorts[at + c.len_utf8()..];
                let lone = at == 0 && rest.is_empty();
                match f.arity {
                    Arity::Switch => {
                        let words = if lone {
                            vec![tok.to_string()]
                        } else {
                            vec![format!("-{c}")]
                        };
                        apply(f, words, &mut kept)?;
                    }
                    Arity::Required | Arity::Optional => {
                        let mut words = vec![if lone {
                            tok.to_string()
                        } else {
                            format!("-{c}")
                        }];
                        if rest.is_empty() {
                            let next = args.get(i);
                            let takes = match f.arity {
                                Arity::Required => true,
                                _ => next.is_some_and(|v| !is_option_like(v)),
                            };
                            if takes {
                                let v =
                                    next.ok_or_else(|| Refusal::UnknownFlag(tok.to_string()))?;
                                words.push(v.clone());
                                i += 1;
                            }
                        } else {
                            words = vec![format!("-{c}{rest}")];
                        }
                        apply(f, words, &mut kept)?;
                        break;
                    }
                }
            }
            continue;
        }
        let what = if SUBCOMMANDS.contains(&tok) {
            format!("the `{tok}` subcommand")
        } else {
            String::from("an operand, which kimi refuses")
        };
        return Err(Refusal::NotInteractive(what));
    }
    Ok(kept)
}

/// commander's `maybeOption`: a token of two or more characters starting with `-`.
fn is_option_like(v: &str) -> bool {
    v.len() > 1 && v.starts_with('-')
}

fn apply(f: &Flag, words: Vec<String>, kept: &mut Vec<String>) -> Result<(), Refusal> {
    match f.fate {
        Fate::Keep => {
            kept.extend(words);
            Ok(())
        }
        Fate::Drop => Ok(()),
        Fate::OneShot => Err(Refusal::NotInteractive(words.join(" "))),
    }
}

/// Whether `args` (the argv after the program) launch Kimi's interactive session as far as the
/// flag table can tell.
pub fn is_interactive(args: &[String]) -> bool {
    carried(args).is_ok()
}

/// THE RELAUNCH ARGV: the launch's carried flags, then `--session <session>` (aterm
/// `relaunch.rs:2512-2545`: Kimi's resume pair follows the flags).
///
/// # Errors
/// A [`Refusal`] from [`carried`], or [`Refusal::BadSession`].
pub fn resume_argv(args: &[String], session: &str) -> Result<Vec<String>, Refusal> {
    if !is_session_id(session) {
        return Err(Refusal::BadSession(session.to_string()));
    }
    let mut out = carried(args)?;
    out.push(RESUME_FLAG.to_string());
    out.push(session.to_string());
    Ok(out)
}

/// Whether `s` is a session id as Kimi names its lock files and takes them back: 1-128
/// characters of `[A-Za-z0-9._-]`, starting with a letter or digit (never mistaken for a flag).
pub fn is_session_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Kimi's home for a child running under `env` (`bootstrap.ts:168-174`).
///
/// # Errors
/// `$KIMI_CODE_HOME` is set but empty or relative (Kimi would then resolve its locks against its
/// working directory), or `$HOME` is unusable.
pub fn home(env: &Env) -> Result<PathBuf, String> {
    match env.get(HOME_ENV) {
        Some(v) if v.is_empty() => Err(format!("{HOME_ENV} is set but empty")),
        Some(v) => {
            let p = PathBuf::from(v);
            if p.is_absolute() {
                Ok(p)
            } else {
                Err(format!("{HOME_ENV}={} is not absolute", p.display()))
            }
        }
        None => env
            .home()
            .map(|h| h.join(DEFAULT_HOME))
            .ok_or_else(|| String::from("$HOME is unset or not absolute")),
    }
}

/// Whether Kimi holds session locks under `env`: off only when `KIMI_CODE_SESSION_LOCK` reads
/// false as Kimi reads it (`0`, `false`, `no`, `off`, trimmed, any case: `_base/utils/env.ts`).
pub fn locks_enabled(env: &Env) -> bool {
    let Some(v) = env.get(SESSION_LOCK_ENV) else {
        return true;
    };
    let v = v.to_string_lossy().trim().to_ascii_lowercase();
    !matches!(v.as_str(), "0" | "false" | "no" | "off")
}

/// The largest lock file read (Kimi's are a short JSON line).
const MAX_LOCK_BYTES: u64 = 64 * 1024;

/// The sessions whose lock under `home` is held by one of `pids` on `host`: the session the
/// running child has open.
///
/// # Errors
/// A lock that cannot be read or parsed (it might be the child's), or one of the child's with a
/// lock-file version this harness does not know.
pub fn held_sessions(home: &Path, pids: &BTreeSet<i32>, host: &str) -> Result<Vec<String>, String> {
    use std::io::Read as _;
    let dir = home.join("locks").join("sessions");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut held = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(session) = name.strip_suffix(".lock") else {
            continue;
        };
        if session.contains(".lock") || !is_session_id(session) {
            continue;
        }
        let path = entry.path();
        let file = {
            use rustix::fs::{Mode, OFlags, open};
            match open(
                &path,
                OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW,
                Mode::empty(),
            ) {
                Ok(fd) => std::fs::File::from(fd),
                Err(rustix::io::Errno::NOENT) => continue,
                Err(e) => return Err(format!("{}: {e}", path.display())),
            }
        };
        let meta = file
            .metadata()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        if meta.len() > MAX_LOCK_BYTES {
            return Err(format!("{} is too large to be a Kimi lock", path.display()));
        }
        let mut text = String::new();
        file.take(MAX_LOCK_BYTES.saturating_add(1))
            .read_to_string(&mut text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_LOCK_BYTES {
            return Err(format!("{} is too large to be a Kimi lock", path.display()));
        }
        let holder: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} does not parse: {e}", path.display()))?;
        let pid = holder
            .get("pid")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("{} names no pid", path.display()))?;
        let lock_host = holder
            .get("hostname")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let ours = i32::try_from(pid).is_ok_and(|p| pids.contains(&p)) && lock_host == host;
        if !ours {
            continue;
        }
        if holder.get("version").and_then(serde_json::Value::as_i64) != Some(1) {
            return Err(format!(
                "{} is held by the child with a lock version kimi-alab does not know",
                path.display()
            ));
        }
        held.push(session.to_string());
    }
    held.sort();
    Ok(held)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn the_resume_pair_follows_the_carried_flags() {
        let args = v(&[
            "--skills-dir",
            "/s",
            "--add-dir=../x",
            "-y",
            "-m",
            "k2",
            "--plan",
        ]);
        assert_eq!(
            resume_argv(&args, "01HZXYZ").unwrap(),
            v(&[
                "--skills-dir",
                "/s",
                "--add-dir=../x",
                "--session",
                "01HZXYZ"
            ])
        );
    }

    #[test]
    fn every_resume_spelling_is_replaced() {
        for args in [
            v(&["-S", "old"]),
            v(&["-S"]),
            v(&["-Sold"]),
            v(&["--session=old"]),
            v(&["--session", "old"]),
            v(&["-r", "old"]),
            v(&["--resume"]),
            v(&["-c"]),
            v(&["--continue"]),
            v(&["-C"]),
            v(&["-yc"]),
        ] {
            assert_eq!(
                resume_argv(&args, "new").unwrap(),
                v(&["--session", "new"]),
                "{args:?}"
            );
        }
    }

    #[test]
    fn an_optional_value_is_not_taken_from_a_flag() {
        // `-S --plan`: commander leaves `--plan` a flag, so `-S` opens the picker.
        assert_eq!(
            carried(&v(&["-S", "--skills-dir", "/s"])).unwrap(),
            v(&["--skills-dir", "/s"])
        );
    }

    #[test]
    fn a_required_value_is_taken_whatever_it_looks_like() {
        assert_eq!(
            carried(&v(&["-m", "--skills-dir"])).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            carried(&v(&["--skills-dir", "-weird"])).unwrap(),
            v(&["--skills-dir", "-weird"])
        );
    }

    #[test]
    fn mode_model_and_agent_flags_are_dropped_on_resume() {
        for flag in [
            "--yolo",
            "-y",
            "--yes",
            "--auto-approve",
            "--auto",
            "--plan",
        ] {
            assert_eq!(
                carried(&v(&[flag])).unwrap(),
                Vec::<String>::new(),
                "{flag}"
            );
        }
        assert_eq!(
            carried(&v(&[
                "--model=k2",
                "--agent",
                "rev",
                "--agent-file",
                "a.md"
            ]))
            .unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn one_shots_subcommands_and_operands_are_not_interactive() {
        for args in [
            v(&["-p", "hi"]),
            v(&["--prompt=hi"]),
            v(&["--output-format", "text"]),
            v(&["--version"]),
            v(&["-V"]),
            v(&["-h"]),
            v(&["login"]),
            v(&["--yolo", "session"]),
            v(&["upgrade"]),
            v(&["hello"]),
            v(&["--", "x"]),
            v(&["-yp", "hi"]),
        ] {
            assert!(
                matches!(carried(&args), Err(Refusal::NotInteractive(_))),
                "{args:?}"
            );
            assert!(!is_interactive(&args));
        }
        assert!(is_interactive(&v(&[])));
        assert!(is_interactive(&v(&["--"])));
    }

    #[test]
    fn an_unknown_flag_refuses_the_relaunch() {
        for args in [
            v(&["--frobnicate"]),
            v(&["-z"]),
            v(&["--yolo=1"]),
            v(&["-yz"]),
            v(&["-m"]),
            v(&["--model"]),
        ] {
            assert!(
                matches!(carried(&args), Err(Refusal::UnknownFlag(_))),
                "{args:?}"
            );
        }
    }

    #[test]
    fn session_ids_are_safe_words() {
        assert!(is_session_id("01HZABCDEF"));
        assert!(is_session_id("a.b_c-d"));
        for bad in ["", "-x", ".x", "a b", "a/b", &"x".repeat(129)] {
            assert!(!is_session_id(bad), "{bad:?}");
        }
        assert!(matches!(
            resume_argv(&[], "-x"),
            Err(Refusal::BadSession(_))
        ));
    }

    #[test]
    fn kimis_home_and_lock_switch_are_read_as_kimi_reads_them() {
        let env = Env::from_pairs([("HOME", "/home/x")]);
        assert_eq!(home(&env).unwrap(), PathBuf::from("/home/x/.kimi-code"));
        let env = Env::from_pairs([("HOME", "/home/x"), (HOME_ENV, "/k")]);
        assert_eq!(home(&env).unwrap(), PathBuf::from("/k"));
        assert!(home(&Env::from_pairs([(HOME_ENV, "")])).is_err());
        assert!(home(&Env::from_pairs([(HOME_ENV, "rel")])).is_err());
        assert!(locks_enabled(&Env::default()));
        assert!(locks_enabled(&Env::from_pairs([(
            SESSION_LOCK_ENV,
            "maybe"
        )])));
        assert!(locks_enabled(&Env::from_pairs([(SESSION_LOCK_ENV, "1")])));
        for off in ["0", "false", " OFF ", "No"] {
            assert!(
                !locks_enabled(&Env::from_pairs([(SESSION_LOCK_ENV, off)])),
                "{off}"
            );
        }
    }

    #[test]
    fn the_childs_lock_names_its_session() {
        let home = std::env::temp_dir().join(format!("kimi-alab-locks-{}", std::process::id()));
        let dir = home.join("locks/sessions");
        std::fs::create_dir_all(&dir).unwrap();
        let lock = |id: &str, pid: i32, host: &str, version: i32| {
            std::fs::write(
                dir.join(format!("{id}.lock")),
                format!(r#"{{"version":{version},"token":"{pid}:t","pid":{pid},"hostname":"{host}","startedAt":1,"clientIdentity":"kimi_code_cli"}}"#),
            )
            .unwrap();
        };
        lock("mine", 4242, "h", 1);
        lock("other-process", 999, "h", 1);
        lock("other-host", 4242, "elsewhere", 1);
        std::fs::write(dir.join("mine.lock.tmp-4242-x"), "partial").unwrap();
        let pids: BTreeSet<i32> = [4242, 4243].into_iter().collect();
        assert_eq!(held_sessions(&home, &pids, "h").unwrap(), v(&["mine"]));
        lock("mine-too", 4243, "h", 1);
        assert_eq!(
            held_sessions(&home, &pids, "h").unwrap(),
            v(&["mine", "mine-too"])
        );
        lock("future", 4242, "h", 2);
        assert!(held_sessions(&home, &pids, "h").is_err());
        std::fs::remove_file(dir.join("future.lock")).unwrap();
        std::fs::write(dir.join("torn.lock"), "{not json").unwrap();
        assert!(held_sessions(&home, &pids, "h").is_err());
        std::fs::remove_file(dir.join("torn.lock")).unwrap();
        let fifo = dir.join("fifo.lock");
        let made = std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        assert!(held_sessions(&home, &pids, "h").is_err());
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(
            held_sessions(&home, &pids, "h").unwrap(),
            Vec::<String>::new()
        );
    }
}
