// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The word lists are aterm-types' own (`tab_id_tests.rs`), asked of [`is_tab_id`] alone.

use super::*;

#[test]
fn a_tab_id_is_t_and_ten_crockford_characters() {
    for good in [
        "tqn5r6e0148",
        "t0000000000",
        "tzzzzzzzzzz",
        "tabcdefghjk",
        "tmnpqrstvwx",
    ] {
        assert!(is_tab_id(good), "{good:?}");
    }
    // Negative controls: each breaks exactly one rule of the grammar.
    for bad in [
        "",
        "t",
        "tqn5r6e014",              // 10 bytes
        "tqn5r6e01488",            // 12 bytes
        "sqn5r6e0148",             // not `t`
        "Tqn5r6e0148",             // upper-case `T`
        "tQN5R6E0148",             // upper-case characters
        "tqn5r6e014i",             // Crockford's alias for 1
        "tqn5r6e014l",             // Crockford's alias for 1
        "tqn5r6e014o",             // Crockford's alias for 0
        "tqn5r6e014u",             // excluded by Crockford
        "tqn5r6e014-",             // punctuation
        "tqn5r6e014 ",             // whitespace
        "s-0f1e",                  // an earlier ID is 20 hex digits
        "s-0123456789abcdef012",   // 19
        "s-0123456789abcdef01234", // 21
        "s-0123456789ABCDEF0123",  // upper-case hex
        "s-0123456789abcdeg0123",  // not hex
        "S-0123456789abcdef0123",
        "s_0123456789abcdef0123",
        "t-qn5r6e014",
        "tqn5r6e01\u{e9}", // a multi-byte character at the end
    ] {
        assert!(!is_tab_id(bad), "{bad:?}");
    }
}

#[test]
fn a_tab_opened_before_the_t_form_keeps_its_s_id() {
    assert!(is_tab_id("s-0123456789abcdef0123"));
    assert!(is_tab_id("s-ffffffffffffffffffff"));
    // Its short form is no ID; only a reader that takes short forms reads it.
    assert!(!is_tab_id("s-0123"));
}

#[test]
fn every_alphabet_character_round_trips_through_its_value() {
    for (value, &ch) in ALPHABET.iter().enumerate() {
        assert_eq!(digit(ch), Some(value as u8), "{}", ch as char);
    }
    for b in 0u8..=255 {
        assert_eq!(digit(b).is_some(), ALPHABET.contains(&b), "{b}");
    }
}

/// [`ALPHABET`] ascends in ASCII, as its doc says, and a minted ID's short form is its `t` and
/// 4 characters.
#[test]
fn the_alphabet_ascends_and_the_short_form_is_five_bytes() {
    assert!(ALPHABET.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(SHORT_LEN, 5);
    assert_eq!("tqn5r6e0148".get(..SHORT_LEN), Some("tqn5r"));
    assert_eq!(LEN, 1 + 10);
    assert_eq!(EARLIER_LEN, 2 + 20);
}
