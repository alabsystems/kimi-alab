// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A snapshot of the process environment, so every rule that reads a variable can be tested
//! without touching the real one.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// The environment a harness run reads: taken once at start ([`Env::from_process`]) or built by
/// a test ([`Env::from_pairs`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env {
    vars: BTreeMap<OsString, OsString>,
}

impl Env {
    /// This process's environment.
    pub fn from_process() -> Self {
        Self {
            vars: std::env::vars_os().collect(),
        }
    }

    /// An environment of exactly `pairs`.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        Self {
            vars: pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// The value of `name`, set or not.
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars.get(OsStr::new(name)).map(OsString::as_os_str)
    }

    /// The value of `name` when it is set to something other than the empty string.
    pub fn get_nonempty(&self, name: &str) -> Option<&OsStr> {
        self.get(name).filter(|v| !v.is_empty())
    }

    /// This environment with `pairs` set over it (the environment a child is given).
    #[must_use]
    pub fn overlay<'a, I>(&self, pairs: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut vars = self.vars.clone();
        for (k, v) in pairs {
            vars.insert(OsString::from(k), OsString::from(v));
        }
        Self { vars }
    }

    /// Set `name` to `value` in this snapshot.
    pub fn set(&mut self, name: &str, value: &OsStr) {
        self.vars.insert(OsString::from(name), value.to_os_string());
    }

    /// Remove `name` from this snapshot.
    pub fn remove(&mut self, name: &str) {
        self.vars.remove(OsStr::new(name));
    }

    /// `$HOME`, only when it is set and absolute.
    pub fn home(&self) -> Option<PathBuf> {
        self.get_nonempty("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    }
}

#[cfg(test)]
mod tests {
    use super::Env;
    use std::path::PathBuf;

    #[test]
    fn home_must_be_set_and_absolute() {
        assert_eq!(Env::default().home(), None);
        assert_eq!(Env::from_pairs([("HOME", "")]).home(), None);
        assert_eq!(Env::from_pairs([("HOME", "rel/dir")]).home(), None);
        assert_eq!(
            Env::from_pairs([("HOME", "/home/x")]).home(),
            Some(PathBuf::from("/home/x"))
        );
    }

    #[test]
    fn an_empty_value_is_set_but_not_nonempty() {
        let env = Env::from_pairs([("A", "")]);
        assert!(env.get("A").is_some());
        assert!(env.get_nonempty("A").is_none());
        assert!(env.get("B").is_none());
    }
}
