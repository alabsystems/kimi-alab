// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// `is_hidden_char` spells out `char::is_control` (not a `const fn` on
/// stable Rust): every control character is hidden, at every code point.
/// (aterm-messages' test of the same name.)
#[test]
fn every_control_character_is_hidden() {
    for c in (0..=u32::from(char::MAX)).filter_map(char::from_u32) {
        if c.is_control() {
            assert!(is_hidden_char(c), "{c:?}");
        }
    }
}

/// The table hides what re-orders or hides text, and keeps every printing character: the
/// zero-width joiner and non-joiner among them, which emoji sequences and Indic scripts need.
#[test]
fn format_characters_are_hidden_and_joiners_are_kept() {
    for hidden in [
        '\u{00ad}',  // soft hyphen
        '\u{200b}',  // zero-width space
        '\u{200e}',  // left-to-right mark
        '\u{202e}',  // right-to-left override
        '\u{2063}',  // invisible separator
        '\u{2066}',  // left-to-right isolate
        '\u{feff}',  // byte order mark
        '\u{e0041}', // tag latin capital letter A
        '\u{fdd0}',  // noncharacter
        '\u{fffe}',  // noncharacter
        '\u{1fffe}', // noncharacter, plane 1
        '\u{10ffff}',
    ] {
        assert!(is_hidden_char(hidden), "{hidden:?}");
    }
    // Negative controls: printing text, spaces and the joiners stay.
    for kept in [
        'a',
        ' ',
        '\u{e9}',
        '\u{6f22}',
        '\u{1f600}',
        '\u{200c}',
        '\u{200d}',
    ] {
        assert!(!is_hidden_char(kept), "{kept:?}");
    }
}

/// A turn's messages join with a blank line, so the joined text holds each message whole.
#[test]
fn messages_join_with_a_blank_line() {
    assert_eq!(MESSAGE_GAP, "\n\n");
    let messages = vec![
        "one\n\ntwo".to_string(),
        "three".to_string(),
        "four".to_string(),
    ];
    assert_eq!(join(&messages), "one\n\ntwo\n\nthree\n\nfour");
    assert_eq!(join(&messages[..1]), "one\n\ntwo");
    assert_eq!(join(&[]), "");
}

/// A count is ASCII digits that fit the type; a sign, a space, a separator, another script's
/// digit or an overflow is none.
#[test]
fn a_count_is_ascii_digits_that_fit() {
    assert_eq!(parse_uint::<u64>("0"), Some(0));
    assert_eq!(parse_uint::<u64>("42"), Some(42));
    assert_eq!(parse_uint::<u64>("007"), Some(7));
    assert_eq!(parse_uint::<u64>("18446744073709551615"), Some(u64::MAX));
    assert_eq!(parse_uint::<u8>("255"), Some(255));
    for bad in [
        "",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1_000",
        "1,000",
        "1.0",
        "0x10",
        "\u{661}",
        "18446744073709551616",
    ] {
        assert_eq!(parse_uint::<u64>(bad), None, "{bad:?}");
    }
    assert_eq!(parse_uint::<u8>("256"), None);
}
