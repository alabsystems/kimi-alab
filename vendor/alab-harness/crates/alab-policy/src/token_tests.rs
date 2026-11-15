// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The cases are aterm's (`aterm-uds` `process_end_proven_tests.rs` for the writer and the
//! shaped reader, `harness/leftover_tests.rs` for the reader of either shape).

use super::*;

const APPLE: ProcessToken = ProcessToken {
    pid: 4242,
    birth_us: 1_792_000_000_000_042,
    idversion: Some(7),
};

const LINUX: ProcessToken = ProcessToken {
    pid: 4242,
    birth_us: 51_832_760,
    idversion: None,
};

#[test]
fn a_token_prints_its_version_only_where_the_kernel_keeps_one() {
    assert_eq!(APPLE.token(), "4242@1792000000000042:7");
    assert_eq!(LINUX.token(), "4242@51832760");
}

#[test]
fn a_token_round_trips_in_either_shape() {
    for token in [
        APPLE,
        LINUX,
        ProcessToken {
            pid: 63452,
            birth_us: 1_792_000_000_000_042,
            idversion: Some(331_207),
        },
        ProcessToken {
            pid: i32::MAX,
            birth_us: u64::MAX,
            idversion: Some(0),
        },
        ProcessToken {
            pid: 2,
            birth_us: 0,
            idversion: None,
        },
    ] {
        assert_eq!(
            ProcessToken::parse(&token.token()),
            Some(token),
            "{token:?}"
        );
        let versioned = token.idversion.is_some();
        assert_eq!(
            ProcessToken::parse_shaped(&token.token(), versioned),
            Some(token)
        );
        // Negative control: the other shape does not read it.
        assert_eq!(ProcessToken::parse_shaped(&token.token(), !versioned), None);
    }
}

/// A token reads in either kernel's shape, and a word that is no token reads as none.
#[test]
fn a_token_reads_in_either_shape() {
    assert_eq!(
        ProcessToken::parse("2606@1792098840004410:3").map(|t| t.pid),
        Some(2606)
    );
    assert_eq!(
        ProcessToken::parse("2606@1792098840004410").map(|t| t.idversion),
        Some(None)
    );
    for bad in ["2606", "it", "2606@", "@1", "1@5", "2606@01", "2606@5:"] {
        assert_eq!(ProcessToken::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn a_bare_pid_a_pattern_or_a_word_is_no_token() {
    let bad = [
        "",
        "4242",
        "all",
        "*",
        "4242@*",
        "42*@1792000000000042:7",
        "aterm@1792000000000042:7",
        "1@1792000000000042:7",
        "0@1792000000000042:7",
        "-4242@1792000000000042:7",
        "+4242@1792000000000042:7",
        "4242@+1792000000000042:7",
        "4242@1792000000000042:-7",
        "4242@1792000000000042:+7",
        " 4242@1792000000000042:7",
        "4242@1792000000000042:7 ",
        "4242 @1792000000000042:7",
        "04242@1792000000000042:7",
        "4242@01792000000000042:7",
        "4242@1792000000000042:07",
        "99999999999@1792000000000042:7",
        "4242@99999999999999999999999:7",
        "4242@1792000000000042:99999999999",
        "4242@1792000000000042:7:8",
        "4242@1792000000000042:7@9",
        "4242@1792000000000042@9",
        "4242@",
        "@1792000000000042:7",
        "4242@1792000000000042:",
        "4242@51832760:",
    ];
    for versioned in [true, false] {
        for word in bad {
            assert_eq!(
                ProcessToken::parse_shaped(word, versioned),
                None,
                "{word:?}"
            );
        }
    }
    for word in bad {
        assert_eq!(ProcessToken::parse(word), None, "{word:?}");
    }
}
