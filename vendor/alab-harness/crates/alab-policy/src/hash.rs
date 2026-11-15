// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! FNV-1a/64, the hash a decider keys a remembered text by (a set of needs, say), so that every
//! consumer computes the same key aterm stores.
//!
//! A copy of aterm's `aterm_hash::fnv1a_64` with the byte loop written out (aterm's runs
//! astream-wire's); aterm keeps its own and holds this copy to it with a parity test.

/// The FNV-1a/64 offset basis: the hash of no bytes.
pub const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// The FNV-64 prime, 2^40 + 0x1b3.
pub const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a/64 of `bytes`.
#[inline]
#[must_use]
pub const fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = FNV64_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(FNV64_PRIME);
        i += 1;
    }
    hash
}

#[cfg(test)]
#[path = "hash_tests.rs"]
mod tests;
