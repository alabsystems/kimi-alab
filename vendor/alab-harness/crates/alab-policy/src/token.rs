// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A process's identity token as aterm writes it (`sys left`, a Land defense's `keep <token>`
//! line): `<pid>@<birth>:<idversion>`, or `<pid>@<birth>` where the kernel keeps no pid version.
//!
//! aterm's `aterm_uds::process::Identity` holds the same three numbers and is what aterm signals
//! by; a decider here only reads and writes the token, so it gets this plain type, and aterm
//! converts one into the other (Rust's orphan rule forbids a `From` between them here). The
//! reader is aterm's `harness::leftover::token_of` over `Identity::parse_shaped`, the writer
//! `Identity::token`.

/// The three numbers of a process's identity token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessToken {
    /// The pid.
    pub pid: i32,
    /// Its birth, which tells it from the next process given the same number: the number aterm's
    /// `Birth` holds (whole microseconds of the kernel's start record on Apple and Windows; on
    /// Linux, clock ticks after boot).
    pub birth_us: u64,
    /// Its pid version, incremented on exec (`None` where the kernel keeps none: Linux).
    pub idversion: Option<i32>,
}

impl ProcessToken {
    /// Formats the wire token `<pid>@<birth>:<idversion>`, or `<pid>@<birth>` with no version.
    #[must_use]
    pub fn token(&self) -> String {
        match self.idversion {
            Some(version) => format!("{}@{}:{version}", self.pid, self.birth_us),
            None => format!("{}@{}", self.pid, self.birth_us),
        }
    }

    /// A token as `sys left` prints it, in either kernel's shape: `<pid>@<birth>:<idversion>` or
    /// `<pid>@<birth>`; `None` for any other word. Only the exact text [`Self::token`] writes
    /// reads back.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Self::parse_shaped(word, word.contains(':'))
    }

    /// Reads `token` in one shape: `<pid>@<birth>:<idversion>` where `versioned`, else
    /// `<pid>@<birth>`. Only the exact text [`Self::token`] writes reads back: digits only (no
    /// sign, space or leading zero), a pid above 1, every number within its type, and no field
    /// more or fewer. A bare pid, a pattern or a word is no token.
    #[must_use]
    fn parse_shaped(token: &str, versioned: bool) -> Option<Self> {
        fn number<T: std::str::FromStr>(text: &str) -> Option<T> {
            (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
                .then(|| text.parse().ok())
                .flatten()
        }
        let (pid, rest) = token.split_once('@')?;
        let (birth, idversion) = if versioned {
            let (birth, version) = rest.split_once(':')?;
            (birth, Some(number::<i32>(version)?))
        } else {
            (rest, None)
        };
        let parsed = Self {
            pid: number::<i32>(pid).filter(|&pid| pid > 1)?,
            birth_us: number(birth)?,
            idversion,
        };
        (parsed.token() == token).then_some(parsed)
    }
}

#[cfg(test)]
#[path = "token_tests.rs"]
mod tests;
