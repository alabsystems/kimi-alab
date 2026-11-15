// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// FNV's published vectors; a wrong prime can agree in the low 40 bits only. (The vectors of
/// aterm-hash's `matches_the_published_vectors`.)
#[test]
fn matches_the_published_vectors() {
    assert_eq!(FNV64_OFFSET, 0xcbf2_9ce4_8422_2325);
    assert_eq!(FNV64_PRIME, 0x0000_0100_0000_01b3);
    assert_eq!(FNV64_PRIME, (1 << 40) + 0x1b3);
    for (bytes, hash) in [
        (&b""[..], FNV64_OFFSET),
        (b"a", 0xaf63_dc4c_8601_ec8c),
        (b"foobar", 0x8594_4171_f739_67e8),
    ] {
        assert_eq!(fnv1a_64(bytes), hash, "{bytes:02x?}");
    }
    // The platform UUID behind aterm-verify's perf-boxes.tsv `m21  fp:4d119fc8de0a3b9e` row.
    assert_eq!(
        fnv1a_64(b"4F2C6182-55E3-51B1-B505-42FD89F13166"),
        0x4d11_9fc8_de0a_3b9e
    );
}

/// The loop is the formula: each byte XORed in, then a multiply by the prime. Negative control:
/// a prime off by one bit misses the vectors.
#[test]
fn the_byte_loop_is_xor_then_multiply() {
    let fold = |prime: u64, bytes: &[u8]| {
        bytes
            .iter()
            .fold(FNV64_OFFSET, |h, &b| (h ^ u64::from(b)).wrapping_mul(prime))
    };
    let inputs: [&[u8]; 5] = [
        b"",
        b"a",
        b"foobar",
        &[0x00, 0x80, 0xff, 0x7f, 0x01],
        &[0xa5; 64],
    ];
    for bytes in inputs {
        assert_eq!(fnv1a_64(bytes), fold(FNV64_PRIME, bytes), "{bytes:02x?}");
    }
    let off_by_one_bit = FNV64_PRIME ^ (1 << 41);
    assert_ne!(fold(off_by_one_bit, b"foobar"), fnv1a_64(b"foobar"));
    assert_ne!(fold(off_by_one_bit, b"a"), fnv1a_64(b"a"));
}
