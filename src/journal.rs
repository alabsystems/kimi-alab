// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The harness's journal: one line per decision, in a file. The harness never writes to the
//! terminal while the child owns it (that would tear the child's screen), so this is where an
//! update waiting, a refusal and a relaunch are recorded.

use std::fs::File;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{Mode, OFlags};

use crate::env::Env;

/// The variable naming the journal file; `off` disables it.
pub const JOURNAL_ENV: &str = "KIMI_ALAB_JOURNAL";

/// A journal file past this size is moved aside to `<file>.old` when a run opens it.
const ROTATE_BYTES: u64 = 4 * 1024 * 1024;

/// An open journal (or none: every write is best-effort).
#[derive(Debug)]
pub struct Journal {
    file: Option<File>,
    path: Option<PathBuf>,
    pid: u32,
}

impl Journal {
    /// A journal that records nothing.
    pub fn none() -> Self {
        Self {
            file: None,
            path: None,
            pid: std::process::id(),
        }
    }

    /// The journal this run writes: [`JOURNAL_ENV`], else `~/Library/Logs/kimi-alab/journal.log`
    /// on macOS and `$XDG_STATE_HOME/kimi-alab/journal.log` (or `~/.local/state/…`) elsewhere.
    pub fn open(env: &Env) -> Self {
        let Some(path) = path_for(env) else {
            return Self::none();
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir);
        }
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() > ROTATE_BYTES) {
            let _ = std::fs::rename(&path, path.with_extension("log.old"));
        }
        let file = rustix::fs::open(
            &path,
            OFlags::CREATE | OFlags::APPEND | OFlags::WRONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )
        .ok()
        .map(File::from)
        .filter(|f| f.metadata().is_ok_and(|m| m.is_file()));
        Self {
            file,
            path: Some(path),
            pid: std::process::id(),
        }
    }

    /// The journal's path, when one is open.
    pub fn path(&self) -> Option<&std::path::Path> {
        self.file.as_ref().and(self.path.as_deref())
    }

    /// Record `event` with `detail`.
    pub fn note(&mut self, event: &str, detail: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let detail: String = detail
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let line = format!("{now} pid={} {event} {detail}\n", self.pid);
        let _ = file.write_all(line.as_bytes());
    }
}

fn path_for(env: &Env) -> Option<PathBuf> {
    if let Some(v) = env.get(JOURNAL_ENV) {
        if v == "off" || v.is_empty() {
            return None;
        }
        let p = PathBuf::from(v);
        return p.is_absolute().then_some(p);
    }
    let home = env.home()?;
    if cfg!(target_os = "macos") {
        return Some(home.join("Library/Logs/kimi-alab/journal.log"));
    }
    let state = env
        .get_nonempty("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/state"));
    Some(state.join("kimi-alab/journal.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_journal_path_follows_the_env() {
        assert_eq!(
            path_for(&Env::from_pairs([(JOURNAL_ENV, "off"), ("HOME", "/h")])),
            None
        );
        assert_eq!(path_for(&Env::from_pairs([(JOURNAL_ENV, "rel.log")])), None);
        assert_eq!(
            path_for(&Env::from_pairs([(JOURNAL_ENV, "/j.log")])),
            Some(PathBuf::from("/j.log"))
        );
        let default = path_for(&Env::from_pairs([("HOME", "/h")])).unwrap();
        assert!(
            default.starts_with("/h") && default.ends_with("kimi-alab/journal.log"),
            "{default:?}"
        );
        assert_eq!(path_for(&Env::default()), None);
    }

    #[test]
    fn notes_are_one_line_each() {
        let path =
            std::env::temp_dir().join(format!("kimi-alab-journal-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut j = Journal::open(&Env::from_pairs([(JOURNAL_ENV, path.as_os_str())]));
        assert_eq!(j.path(), Some(path.as_path()));
        j.note("EVENT", "two\nlines");
        drop(j);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("EVENT two lines"), "{text}");
        std::fs::remove_file(&path).unwrap();
    }
}
