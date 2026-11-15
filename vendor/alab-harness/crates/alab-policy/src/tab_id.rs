// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The grammar of an aterm tab's ID: `t` and 10 lower-case Crockford base32 characters
//! (`tqn5r6e0148`, shown as its first 5, `tqn5r`), or a tab opened before that form's `s-` and
//! 20 lower-case hex digits.
//!
//! A copy of the grammar in aterm's `aterm_types::tab_id`, which stays THE parser of a tab ID in
//! aterm (its `TabId` holds, mints and orders one); aterm holds this copy to it with a parity
//! test. A decider here only asks whether a word is one.

/// The 32 characters of a tab ID after its `t`, in value order: Crockford's base32 alphabet in
/// lower case (`0-9` and `a-z` without `i`, `l`, `o` and `u`). It ascends in ASCII, so two IDs
/// order as text exactly as their values do.
pub const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// The bytes of a minted tab ID: `t` and 10 characters.
pub const LEN: usize = 11;

/// The bytes a minted tab ID is shown as: its first 5 (`t` and 4 characters).
pub const SHORT_LEN: usize = 5;

/// The bytes of a tab ID minted before the `t` form: `s-` and 20 hex digits.
pub const EARLIER_LEN: usize = 22;

/// The value of one ID character, or `None` for a byte outside [`ALPHABET`].
const fn digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'h' => Some(b - b'a' + 10),
        b'j' | b'k' => Some(b - b'j' + 18),
        b'm' | b'n' => Some(b - b'm' + 20),
        b'p'..=b't' => Some(b - b'p' + 22),
        b'v'..=b'z' => Some(b - b'v' + 27),
        _ => None,
    }
}

const fn is_lower_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f')
}

/// Whether `s` is a tab ID, in either form: exactly `t` and 10 characters of [`ALPHABET`], or
/// an earlier tab's `s-` and 20 lower-case hex digits (aterm's `TabId::parse`).
#[must_use]
#[inline]
pub const fn is_tab_id(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() == LEN && b[0] == b't' {
        let mut i = 0;
        while i < LEN {
            if i > 0 && digit(b[i]).is_none() {
                return false;
            }
            i += 1;
        }
        return true;
    }
    if b.len() == EARLIER_LEN && b[0] == b's' && b[1] == b'-' {
        let mut i = 0;
        while i < EARLIER_LEN {
            if i > 1 && !is_lower_hex(b[i]) {
                return false;
            }
            i += 1;
        }
        return true;
    }
    false
}

#[cfg(test)]
#[path = "tab_id_tests.rs"]
mod tests;
