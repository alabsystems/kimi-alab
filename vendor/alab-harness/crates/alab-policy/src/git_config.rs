// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Git configuration hazard check for read-only command approval.
//!
//! The pure half of aterm's `supervise/policy/git_config.rs`: where a line's git reads run
//! ([`git_reads`]), what a git there loads as the caller read it ([`GitView`], and the part a
//! harness tape keeps, [`GitView::project`]), and the first thing in that view a read would run
//! ([`hazard`]). aterm keeps the live half, which starts git to read a [`GitView`] (`WorkerEnv`
//! and `Probe`), in its file of the same name.

use std::path::{Component, Path, PathBuf};

use crate::classify::{SUBSTITUTION, command_words, program, runs};

/// Where a line's git reads run, and what they may do beyond reading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitReads {
    /// Every directory a git invocation on the line may run in, deduplicated.
    pub dirs: Vec<PathBuf>,
    /// A read that displays signatures on its own.
    pub signatures: bool,
    /// A `log`/`show`/`whatchanged`/`reflog`.
    pub log_like: bool,
    /// A `grep` that reads the files the index tracks (no `--no-index`), an assume-unchanged
    /// one from the index ([`GitView::assume_unchanged`]).
    pub grep_index: bool,
    /// The first read that contacts a remote, as a reason.
    pub remote: Option<String>,
}

/// The git reads on `line`, run from any of `cwds`.
pub fn git_reads(
    line: &str,
    cwds: &[PathBuf],
    home: Option<&Path>,
) -> Result<Option<GitReads>, String> {
    let segments = command_words(line)?;
    let mut commands = Vec::new();
    for seg in &segments {
        if let Some(run) = runs(seg)? {
            commands.push((seg, run));
        }
    }
    let is_git = |seg: &[String], at: usize| program(&seg[at]) == "git";
    if !commands.iter().any(|(seg, run)| is_git(seg, run.at)) {
        return Ok(None);
    }
    let looped = segments.iter().any(|seg| {
        seg.iter()
            .find(|w| w.as_str() != "!")
            .is_some_and(|w| matches!(w.as_str(), "for" | "while" | "until" | "select"))
    });
    let mut bases = cwds.to_vec();
    for (seg, run) in &commands {
        if !matches!(program(&seg[run.at]), "cd" | "pushd") {
            continue;
        }
        let Ok(target) = cd_operand(&seg[run.at + 1..]) else {
            return Err(format!(
                "a git read after `{}`, a move this check cannot follow",
                seg[run.at..].join(" ")
            ));
        };
        // On Windows a drive path (`C:/…`) is rooted too: bash there looks it up in no `CDPATH`.
        let relative =
            target.filter(|t| !t.starts_with(['/', '~']) && !(cfg!(windows) && rooted(t)));
        if looped && let Some(t) = relative {
            return Err(format!("a git read on a line whose loop repeats `cd {t}`"));
        }
        let found = match target {
            None => vec![
                home.map(Path::to_path_buf)
                    .ok_or_else(|| "a git read after `cd` with no home".to_string())?,
            ],
            Some(t) => {
                let mut found = Vec::new();
                for base in &bases {
                    found.extend(resolve(base, t, home)?);
                }
                found
            }
        };
        if let Some(t) = relative.filter(|t| cdpath_searched(t)) {
            return Err(format!(
                "a git read after `cd {t}`, which a `CDPATH` (zsh's `cdpath`, which the worker's \
                 environment or a .zshenv may set) may place where this check cannot follow \
                 (`cd ./{t}` stays where it stands)"
            ));
        }
        bases.extend(found);
    }
    let mut out = GitReads::default();
    for (seg, run) in &commands {
        let i = run.at;
        if !is_git(seg, i) {
            continue;
        }
        let fed = run.fed;
        let mut dirs = bases.clone();
        if !run.chdir.is_empty() {
            if fed {
                return Err(format!(
                    "env -C {} behind xargs (its input may name the directory)",
                    run.chdir[0]
                ));
            }
            let mut moved = Vec::new();
            for d in &run.chdir {
                for base in &bases {
                    moved.extend(resolve(base, d, home)?);
                }
            }
            dirs = moved;
        }
        let mut j = i + 1;
        while let Some(t) = seg.get(j).map(String::as_str) {
            if let Some(glued) = t.strip_prefix("-C") {
                let (d, step) = if glued.is_empty() {
                    let Some(d) = seg.get(j + 1).map(String::as_str) else {
                        return Err("git -C without a directory".to_string());
                    };
                    (d, 2)
                } else {
                    (glued, 1)
                };
                if fed {
                    return Err(format!(
                        "git -C {d} behind xargs (its input names the directory)"
                    ));
                }
                let mut next = Vec::new();
                for base in &dirs {
                    next.extend(resolve(base, d, home)?);
                }
                dirs = next;
                j += step;
            } else if t.starts_with("-c")
                || t.starts_with("--git-dir")
                || t.starts_with("--work-tree")
                || t.starts_with("--config-env")
                || t.starts_with("--exec-path")
            {
                return Err(format!("git {t}"));
            } else if t == "--" {
                j += 1;
                break;
            } else if matches!(t, "--namespace" | "--super-prefix" | "--attr-source") {
                j += 2;
            } else if t.starts_with('-') {
                j += 1;
            } else {
                break;
            }
        }
        let sub = seg.get(j).map(String::as_str).unwrap_or("");
        let args = seg.get(j + 1..).unwrap_or(&[]);
        let has = |f: &str| args.iter().any(|a| a == f);
        if args
            .iter()
            .any(|a| a == "--show-signature" || a.contains("%G"))
            || (sub == "tag" && (has("-v") || has("--verify")))
            || matches!(sub, "verify-commit" | "verify-tag")
        {
            out.signatures = true;
        }
        if matches!(sub, "log" | "show" | "whatchanged" | "reflog") {
            out.log_like = true;
        }
        if sub == "grep" && !has("--no-index") {
            out.grep_index = true;
        }
        let contacts_remote = sub == "ls-remote"
            || (sub == "remote" && crate::classify::git_remote_show_contacts(args));
        if contacts_remote && out.remote.is_none() {
            out.remote = Some(format!(
                "git {sub} contacts a remote (credential helpers, ssh and the remote's \
                 programs run)"
            ));
        }
        for d in dirs {
            if !out.dirs.contains(&d) {
                out.dirs.push(d);
            }
        }
    }
    Ok(Some(out))
}

/// The operand of `cd` with `args`: `None` for the home, else its one operand; `Err` when it
/// takes an option other than `-L`, `-P`, `-e`, `-@`, `-q` or `-s` (zsh's `-N` reads the
/// directory stack), names `-` (`OLDPWD`) or a stack entry (`+N`), or takes two operands.
#[expect(
    clippy::result_unit_err,
    reason = "aterm's signature, which the move keeps: the error says only that this check \
              cannot follow the `cd`, and each caller words that itself"
)]
pub fn cd_operand(args: &[String]) -> Result<Option<&str>, ()> {
    let mut k = 0;
    while let Some(w) = args.get(k).filter(|w| w.len() > 1 && w.starts_with('-')) {
        k += 1;
        if w == "--" {
            break;
        }
        if !w[1..]
            .chars()
            .all(|c| matches!(c, 'L' | 'P' | 'e' | '@' | 'q' | 's'))
        {
            return Err(());
        }
    }
    match args.get(k..).unwrap_or(&[]) {
        [] => Ok(None),
        [t] if t != "-" && !t.starts_with('+') => Ok(Some(t)),
        _ => Err(()),
    }
}

/// Whether bash and zsh look `cd t` up in `CDPATH` (zsh's `cdpath`), `t` a relative target:
/// unless it is `.` or `..` or led by `./` or `../`.
pub fn cdpath_searched(t: &str) -> bool {
    !matches!(t.split('/').next(), Some("." | ".."))
}

fn resolve(base: &Path, t: &str, home: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    if t.contains(SUBSTITUTION) || t.contains('$') {
        return Err(format!(
            "a git read in a directory this check cannot resolve ({})",
            t.replace(SUBSTITUTION, "$(…)")
        ));
    }
    if t == "-" || (t.starts_with('+') && t.len() > 1) {
        return Err(format!("a git read after `cd {t}` (the directory stack)"));
    }
    let mut out = Vec::new();
    if let Some(rest) = t.strip_prefix('~') {
        if !(rest.is_empty() || rest.starts_with('/')) {
            return Err(format!("a git read in `{t}` (another user's home)"));
        }
        let home = home.ok_or_else(|| format!("a git read in `{t}` with no home"))?;
        out.push(normalise(&home.join(rest.trim_start_matches('/'))));
    }
    out.push(normalise(&base.join(t)));
    Ok(out)
}

/// Resolve dot and parent components lexically.
pub fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// What a git command in one directory would load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitView {
    /// `(scope, key, value)` as `git config --list --show-scope` reports them.
    pub config: Vec<(String, String, Option<String>)>,
    /// The executable `post-index-change` hook of the repository.
    pub index_hook: Option<PathBuf>,
    /// The filter drivers the repository's attributes select for a tracked path.
    pub filters_selected: Option<Vec<String>>,
    /// Each populated submodule, recursively, with what git loads there.
    pub submodules: Vec<(PathBuf, GitView)>,
    /// The worker's `GIT_EXTERNAL_DIFF` environment value.
    pub external_diff: Option<String>,
    /// Whether the index marks an entry assume-unchanged, whose copy in the index `git grep`
    /// reads in place of the file (MEASURED 2026-11-05 with git 2.56.0: after `git update-index
    /// --assume-unchanged a/.ssh/id_rsa` and the file's removal, `git grep token` printed
    /// `a/.ssh/id_rsa:token rsa`).
    pub assume_unchanged: bool,
    /// The first of `GIT_LITERAL_PATHSPECS` and `GIT_ICASE_PATHSPECS` the worker's environment
    /// sets, under which git reads every pathspec literally or in any case, otherwise than the
    /// secret check reads it.
    pub pathspec_env: Option<String>,
}

/// Whether the variable `key` is a dynamic-loader control (`LD_…`, `DYLD_…`), which acts before
/// git reads its configuration, even with an empty value.
pub fn loader_control(key: &str) -> bool {
    key.starts_with("LD_") || key.starts_with("DYLD_")
}

/// One `git ls-files -z --stage -v` entry: whether it is marked assume-unchanged (a lowercase
/// tag), whether it is a gitlink (mode `160000`), and its path; `None` for no entry.
pub fn index_entry(entry: &[u8]) -> Option<(bool, bool, &[u8])> {
    let tab = entry.iter().position(|b| *b == b'\t')?;
    let (meta, path) = (&entry[..tab], &entry[tab + 1..]);
    let (tag, stage) = match meta.iter().position(|b| *b == b' ') {
        Some(space) => (&meta[..space], &meta[space + 1..]),
        None => (&meta[..0], meta),
    };
    Some((
        tag.iter().any(u8::is_ascii_lowercase),
        stage.starts_with(b"160000 "),
        path,
    ))
}

/// Whether the variable `key` changes which git the worker runs or what that git loads, so a
/// value the probe does not run with (Claude Code's settings', a `.zshenv`'s) makes a git read
/// unreadable.
pub fn git_relevant(key: &str) -> bool {
    matches!(
        key,
        "PATH" | "HOME" | "XDG_CONFIG_HOME" | "DEVELOPER_DIR" | "SDKROOT"
    ) || loader_control(key)
        || (key.starts_with("GIT_") && !matches!(key, "GIT_PAGER" | "GIT_EDITOR"))
}

/// Whether `view`'s configuration sets `core.fsmonitor` to a program (a value that is not a
/// boolean), which git runs on every read of the index.
pub fn fsmonitor_program(view: &GitView) -> bool {
    view.config
        .iter()
        .any(|(_, k, v)| k == "core.fsmonitor" && !is_bool(v.as_deref()))
}

/// The driver a `filter.<driver>.clean`, `.smudge` or `.process` key names; `None` for any other
/// key.
pub fn filter_driver(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("filter.")?;
    let (driver, var) = rest.rsplit_once('.')?;
    matches!(var, "clean" | "smudge" | "process").then_some(driver)
}

/// The `(scope, key, value)` entries of `out`, the output of `git config --list -z
/// --show-scope`; a key git lists without a value has `None`.
pub fn parse_config_list(out: &[u8]) -> Vec<(String, String, Option<String>)> {
    let text = String::from_utf8_lossy(out);
    let mut fields = text.split('\0');
    let mut config = Vec::new();
    while let (Some(scope), Some(entry)) = (fields.next(), fields.next()) {
        if scope.is_empty() && entry.is_empty() {
            break;
        }
        let (key, value) = match entry.split_once('\n') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (entry, None),
        };
        config.push((scope.to_string(), key.to_string(), value));
    }
    config
}

fn is_bool(v: Option<&str>) -> bool {
    match v {
        None => true,
        Some(v) => matches!(
            v.to_ascii_lowercase().as_str(),
            "" | "true" | "yes" | "on" | "1" | "false" | "no" | "off" | "0"
        ),
    }
}

const PARTIAL_CLONE: &str = "a partial clone: a read may fetch a missing object from its \
                             promisor remote, and credential helpers, ssh and the remote's \
                             programs run there";

fn is_false(v: Option<&str>) -> bool {
    v.is_some_and(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "false" | "no" | "off" | "0" | ""
        )
    })
}

fn is_true(v: Option<&str>) -> bool {
    v.is_none_or(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
}

/// What a configuration key is to [`hazard`]: the ONE place a key's name is
/// tested, read by [`hazard`] to judge a view and by [`GitView::project`] to
/// decide which entries a harness tape keeps ([`consulted`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Consulted<'a> {
    /// `core.fsmonitor`: a program unless its value is a boolean.
    Fsmonitor,
    /// `diff.external`.
    DiffExternal,
    /// `extensions.partialclone`.
    PartialClone,
    /// `remote.<name>.promisor`.
    Promisor,
    /// `diff.<driver>.textconv` / `.command`.
    DiffDriver,
    /// `filter.<driver>.clean|smudge|process`, with its driver.
    Filter(&'a str),
    /// `gpg.program`, `gpg.<format>.program`.
    Signer,
    /// `log.showsignature`: makes a plain `log` display signatures.
    ShowSignature,
}

fn consult(key: &str) -> Option<Consulted<'_>> {
    Some(match key {
        "core.fsmonitor" => Consulted::Fsmonitor,
        "diff.external" => Consulted::DiffExternal,
        "extensions.partialclone" => Consulted::PartialClone,
        "log.showsignature" => Consulted::ShowSignature,
        k if k.starts_with("remote.") && k.ends_with(".promisor") => Consulted::Promisor,
        k if k.starts_with("diff.") && (k.ends_with(".textconv") || k.ends_with(".command")) => {
            Consulted::DiffDriver
        }
        k if k.starts_with("gpg.") && k.ends_with("program") => Consulted::Signer,
        k => Consulted::Filter(filter_driver(k)?),
    })
}

/// Whether [`hazard`] reads a configuration key named `key` at all.
#[must_use]
pub(crate) fn consulted(key: &str) -> bool {
    consult(key).is_some()
}

/// The version of [`GitView::project`]'s rule.
pub const PROJECTION: u32 = 2;

/// The one value a projected entry keeps in place of anything but a boolean ([`GitView::project`]).
pub const VALUE_SET: &str = "<set>";

fn value_class(v: Option<&str>) -> Option<String> {
    let v = v?;
    Some(
        if is_true(Some(v)) {
            "true"
        } else if is_false(Some(v)) {
            "false"
        } else {
            VALUE_SET
        }
        .to_string(),
    )
}

impl GitView {
    /// This view as a harness tape keeps it.
    #[must_use]
    pub fn project(&self) -> GitView {
        GitView {
            config: self
                .config
                .iter()
                .filter(|(_, key, _)| consulted(key))
                .map(|(scope, key, value)| {
                    (scope.clone(), key.clone(), value_class(value.as_deref()))
                })
                .collect(),
            index_hook: self.index_hook.clone(),
            filters_selected: self.filters_selected.clone(),
            submodules: self
                .submodules
                .iter()
                .map(|(path, sub)| (path.clone(), sub.project()))
                .collect(),
            external_diff: self.external_diff.as_ref().map(|_| VALUE_SET.to_string()),
            assume_unchanged: self.assume_unchanged,
            pathspec_env: self.pathspec_env.clone(),
        }
    }
}

/// The first thing in `view` that `reads` would run, as a reason naming the key (or the hook).
pub fn hazard(view: &GitView, reads: &GitReads) -> Option<String> {
    let shows = view
        .config
        .iter()
        .rev()
        .find(|(_, k, _)| consult(k) == Some(Consulted::ShowSignature))
        .is_some_and(|(_, _, v)| is_true(v.as_deref()));
    let signatures = reads.signatures || (reads.log_like && shows);
    if view.external_diff.is_some() {
        return Some(
            "git read: `GIT_EXTERNAL_DIFF` is set in the worker's environment: git runs it \
             for every diff"
                .to_string(),
        );
    }
    for (scope, key, value) in &view.config {
        let runs = |what: &str| Some(format!("git read: `{key}` is set ({scope} config): {what}"));
        let value = value.as_deref();
        match consult(key) {
            Some(Consulted::Fsmonitor) if !is_bool(value) => {
                return runs("git runs it on every read of the index");
            }
            Some(Consulted::DiffExternal) => return runs("git runs it for every diff"),
            Some(Consulted::PartialClone) => return runs(PARTIAL_CLONE),
            Some(Consulted::Promisor) if !is_false(value) => return runs(PARTIAL_CLONE),
            Some(Consulted::DiffDriver) => {
                return runs("git runs it on a diff of a file the attributes select");
            }
            Some(Consulted::Filter(driver))
                if view
                    .filters_selected
                    .as_ref()
                    .is_none_or(|selected| selected.iter().any(|d| d == driver)) =>
            {
                return runs("git runs it on a status or diff of a file the attributes select");
            }
            Some(Consulted::Signer) if signatures => {
                return runs("git runs it to verify the signatures this read displays");
            }
            _ => {}
        }
    }
    if let Some(h) = &view.index_hook {
        return Some(format!(
            "git read: {} is an executable post-index-change hook, which a read that \
             refreshes the index (status, diff) runs",
            h.display()
        ));
    }
    if let Some(key) = &view.pathspec_env {
        return Some(format!(
            "git read: `{key}` is set in the worker's environment: git reads every pathspec \
             otherwise than this check reads it"
        ));
    }
    if reads.grep_index && view.assume_unchanged {
        return Some(
            "git read: the index marks a file assume-unchanged, whose copy in the index git grep \
             reads in place of the file, which this check cannot list"
                .to_string(),
        );
    }
    view.submodules.iter().find_map(|(path, sub)| {
        hazard(sub, reads).map(|why| format!("in submodule {}: {why}", path.display()))
    })
}

/// Whether `t` names a path from the root: `/…`, `\…`, or a drive path (`C:\…`, `C:/…`).
pub fn rooted(t: &str) -> bool {
    let b = t.as_bytes();
    t.starts_with(['/', '\\'])
        || (b.len() > 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'/' | b'\\'))
}

#[cfg(test)]
#[path = "git_config_tests.rs"]
mod tests;
