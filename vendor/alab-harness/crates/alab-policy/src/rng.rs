// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Deterministic `xorshift64` generator for generated-input tests.

/// xorshift64 over a non-zero state.
#[derive(Clone, Debug)]
pub struct XorShift(u64);

impl XorShift {
    /// A generator seeded with `seed`. Zero is xorshift's one fixed point, so it
    /// is replaced by a fixed odd constant.
    #[inline]
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// The next 64 bits.
    #[inline]
    pub const fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A value in `0..n` (modulo bias is immaterial at test sizes).
    ///
    /// # Panics
    /// When `n` is zero.
    #[inline]
    pub const fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// An index in `0..n`.
    ///
    /// # Panics
    /// When `n` is zero.
    #[inline]
    pub fn index(&mut self, n: usize) -> usize {
        let n = u64::try_from(n).unwrap_or(u64::MAX);
        usize::try_from(self.below(n)).unwrap_or(usize::MAX)
    }

    /// A value in `lo..hi`.
    ///
    /// # Panics
    /// When `hi <= lo`.
    #[inline]
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        assert!(lo < hi, "XorShift::range requires lo < hi");
        lo.saturating_add(self.index(hi - lo))
    }

    /// One element of `from`.
    ///
    /// # Panics
    /// When `from` is empty.
    #[inline]
    pub fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.index(from.len())]
    }
}
