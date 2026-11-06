// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The one aterm setting the harness reads: `[packages] prefix` in `aterm.toml`, which moves
//! the atpkg store.
//!
//! Ported from aterm's `crates/atpkg/src/config.rs` (`config_path`, `PackagesConfig::prefix_path`;
//! aterm 5e2c6d0c28). aterm parses the whole file with a TOML library; the harness reads only this
//! key, by a deliberately small grammar, and REFUSES any spelling of it outside that grammar
//! rather than guess: a prefix read wrong would run a different `kimi` than atpkg manages.

use std::path::{Component, Path, PathBuf};

use crate::env::Env;

/// The largest config file the harness reads (aterm's own bound is of the same order).
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// `aterm.toml`'s path: `$XDG_CONFIG_HOME/aterm/aterm.toml`, else `$HOME/.config/aterm/aterm.toml`
/// (aterm `crates/atpkg/src/config.rs:242-256`).
pub fn config_path(env: &Env) -> Option<PathBuf> {
    if let Some(x) = env.get_nonempty("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(x).join("aterm").join("aterm.toml"));
    }
    env.home().map(|h| h.join(".config/aterm/aterm.toml"))
}

/// The configured prefix, read from the config file at `path`: `Ok(None)` when the file is
/// absent or unreadable (aterm reads that as "no config") or names no prefix.
///
/// # Errors
/// The file names a prefix the harness cannot read with certainty.
pub fn configured_prefix(path: &Path, home: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let Some(text) = crate::store::read_bounded(path, MAX_CONFIG_BYTES) else {
        return Ok(None);
    };
    match packages_prefix(&text)? {
        None => Ok(None),
        Some(raw) => expand_prefix(&raw, home).map(Some).ok_or_else(|| {
            format!(
                "{}: [packages] prefix = {raw:?} is not absolute or `~/…`",
                path.display()
            )
        }),
    }
}

/// `prefix` as aterm reads it (`PackagesConfig::prefix_path`): blank is unset, `~` and `~/…`
/// are under `home`, anything else must be absolute. `..` is refused (aterm's `vet_prefix`).
pub fn expand_prefix(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    let v = raw.trim();
    let path = if let Some(rest) = v.strip_prefix('~') {
        let home = home?;
        match rest.strip_prefix('/') {
            Some(tail) => home.join(tail),
            None if rest.is_empty() => home.to_path_buf(),
            None => return None,
        }
    } else {
        PathBuf::from(v)
    };
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    Some(path)
}

/// The raw value of `prefix` in the `[packages]` table of `text`, or `None` when it names none.
///
/// The grammar: a `[packages]` header line, then `prefix = "<basic string>"` or
/// `prefix = '<literal string>'`, optionally followed by a `#` comment, before the next header.
///
/// # Errors
/// Any other spelling that could name the prefix (a dotted `packages.prefix`, an inline
/// `packages = { … }`, a multi-line string, an escape outside `\\ \" \t \n \r`, two prefixes,
/// or trailing text).
pub fn packages_prefix(text: &str) -> Result<Option<String>, String> {
    let mut table: Option<String> = Some(String::new());
    let mut multi: Option<&'static str> = None;
    let mut found: Option<String> = None;
    for (n, raw_line) in text.lines().enumerate() {
        let at = n.saturating_add(1);
        if let Some(delim) = multi {
            if raw_line.matches(delim).count() % 2 == 1 {
                multi = None;
            }
            continue;
        }
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let opens = ["\"\"\"", "'''"]
            .into_iter()
            .find(|d| line.matches(d).count() % 2 == 1);
        if let Some(rest) = line.strip_prefix('[') {
            table = header_name(rest);
            multi = opens;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            multi = opens;
            continue;
        };
        let key = key.trim();
        let in_root = table.as_deref() == Some("");
        let in_packages = table.as_deref() == Some("packages");
        if in_root && key == "packages" {
            return Err(format!(
                "aterm.toml line {at}: an inline `packages = …` table is not read by kimi-alab; \
                 write a [packages] table or set KIMI_ALAB_PKG_PREFIX"
            ));
        }
        if (in_packages || in_root) && (key.contains('"') || key.contains('\'')) {
            let bare: String = key.chars().filter(|c| *c != '"' && *c != '\'').collect();
            if bare.trim() == "prefix" || dotted_is(&bare, &["packages", "prefix"]) {
                return Err(format!(
                    "aterm.toml line {at}: a quoted `prefix` key is not read by kimi-alab; \
                     write it bare or set KIMI_ALAB_PKG_PREFIX"
                ));
            }
        }
        let names_prefix = (in_packages && key == "prefix")
            || (in_root && dotted_is(key, &["packages", "prefix"]));
        if !names_prefix {
            multi = opens;
            continue;
        }
        if found.is_some() {
            return Err(format!(
                "aterm.toml line {at}: [packages] prefix is set twice"
            ));
        }
        found = Some(
            string_value(value.trim())
                .map_err(|why| format!("aterm.toml line {at}: [packages] prefix: {why}"))?,
        );
    }
    Ok(found)
}

/// The table a header line opens: `Some(name)` for `[name]`, `None` (no table this reader
/// knows) for an array table, a quoted name, or a malformed header.
fn header_name(rest: &str) -> Option<String> {
    if rest.starts_with('[') {
        return Some(String::from("\u{0}array"));
    }
    let (inner, tail) = rest.split_once(']')?;
    let tail = tail.trim();
    if !(tail.is_empty() || tail.starts_with('#')) {
        return None;
    }
    let name = inner.trim();
    if name.contains('"') || name.contains('\'') {
        return Some(String::from("\u{0}quoted"));
    }
    Some(name.split('.').map(str::trim).collect::<Vec<_>>().join("."))
}

/// Whether the dotted key `key` is exactly `parts` (bare segments, any spaces around dots).
fn dotted_is(key: &str, parts: &[&str]) -> bool {
    let segs: Vec<&str> = key.split('.').map(str::trim).collect();
    segs == parts
}

/// One TOML basic (`"…"`) or literal (`'…'`) single-line string, with nothing but an optional
/// comment after it.
fn string_value(v: &str) -> Result<String, String> {
    if v.starts_with("\"\"\"") || v.starts_with("'''") {
        return Err(String::from("a multi-line string is not read"));
    }
    let mut chars = v.chars();
    let quote = chars.next().ok_or("no value")?;
    let mut out = String::new();
    let mut closed = false;
    match quote {
        '\'' => {
            for c in chars.by_ref() {
                if c == '\'' {
                    closed = true;
                    break;
                }
                out.push(c);
            }
        }
        '"' => {
            while let Some(c) = chars.next() {
                match c {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => match chars.next() {
                        Some('\\') => out.push('\\'),
                        Some('"') => out.push('"'),
                        Some('t') => out.push('\t'),
                        Some('n') => out.push('\n'),
                        Some('r') => out.push('\r'),
                        Some(other) => return Err(format!("escape \\{other} is not read")),
                        None => return Err(String::from("unterminated escape")),
                    },
                    c => out.push(c),
                }
            }
        }
        _ => return Err(String::from("not a quoted string")),
    }
    if !closed {
        return Err(String::from("unterminated string"));
    }
    let rest: String = chars.collect();
    let rest = rest.trim();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return Err(format!("trailing text {rest:?}"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{expand_prefix, packages_prefix};
    use std::path::{Path, PathBuf};

    #[test]
    fn no_packages_table_names_no_prefix() {
        assert_eq!(packages_prefix("").unwrap(), None);
        assert_eq!(
            packages_prefix("[window]\nprefix = \"/x\"\n").unwrap(),
            None
        );
        assert_eq!(
            packages_prefix("[packages]\nexclude = [\"ay\"]\n").unwrap(),
            None
        );
        assert_eq!(
            packages_prefix("note = \"\"\"\n[packages]\nprefix = \"/no\"\n\"\"\"\n").unwrap(),
            None
        );
    }

    #[test]
    fn the_packages_prefix_is_read_in_both_string_forms() {
        let basic = "[packages]\nprefix = \"/opt/pkg\" # moved\n[other]\nprefix = \"/no\"\n";
        assert_eq!(packages_prefix(basic).unwrap().as_deref(), Some("/opt/pkg"));
        let literal = "[packages]\n  prefix='~/pkg'\n";
        assert_eq!(packages_prefix(literal).unwrap().as_deref(), Some("~/pkg"));
        let escaped = "[ packages ]\nprefix = \"/a\\\\b \\\"c\\\"\"\n";
        assert_eq!(
            packages_prefix(escaped).unwrap().as_deref(),
            Some("/a\\b \"c\"")
        );
        let dotted_root = "packages.prefix = \"/root/pkg\"\n[x]\n";
        assert_eq!(
            packages_prefix(dotted_root).unwrap().as_deref(),
            Some("/root/pkg")
        );
    }

    #[test]
    fn a_spelling_outside_the_grammar_is_refused_not_guessed() {
        for text in [
            "packages = { prefix = \"/x\" }\n",
            "[packages]\nprefix = \"\"\"/x\"\"\"\n",
            "[packages]\nprefix = \"/x\\u0041\"\n",
            "[packages]\nprefix = \"/x\" trailing\n",
            "[packages]\nprefix = /x\n",
            "[packages]\nprefix = \"/x\"\nprefix = \"/y\"\n",
            "[packages]\nprefix = \"/x\n",
        ] {
            assert!(packages_prefix(text).is_err(), "{text:?} should be refused");
        }
    }

    #[test]
    fn an_array_table_or_quoted_header_is_not_the_packages_table() {
        assert_eq!(
            packages_prefix("[[packages]]\nprefix = \"/x\"\n").unwrap(),
            None
        );
        assert_eq!(
            packages_prefix("[\"packages\"]\nprefix = \"/x\"\n").unwrap(),
            None
        );
    }

    #[test]
    fn prefixes_expand_like_atpkg() {
        let home = Path::new("/home/x");
        assert_eq!(
            expand_prefix("~/pkg", Some(home)),
            Some(PathBuf::from("/home/x/pkg"))
        );
        assert_eq!(
            expand_prefix("~", Some(home)),
            Some(PathBuf::from("/home/x"))
        );
        assert_eq!(expand_prefix("~bob/pkg", Some(home)), None);
        assert_eq!(expand_prefix("~/pkg", None), None);
        assert_eq!(expand_prefix("rel/pkg", Some(home)), None);
        assert_eq!(expand_prefix("/a/../b", Some(home)), None);
        assert_eq!(
            expand_prefix(" /opt/pkg ", Some(home)),
            Some(PathBuf::from("/opt/pkg"))
        );
    }
}
