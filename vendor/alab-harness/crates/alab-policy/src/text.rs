// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Text readings the deciders share: the characters no surface prints, how a turn's messages
//! join into one text, and a count spelled in digits.
//!
//! Copies of aterm's `aterm_types::hidden_char::is_hidden_char`,
//! `aterm_phase::reader::{MESSAGE_GAP, join_messages}` and `aterm_agent::parse_uint`, which
//! aterm keeps and holds to these with parity tests.

/// A character no chrome surface prints: a control (C0 and C1, so every ASCII line break and tab), a noncharacter, or an invisible format character that re-orders or hides the text around it (bidi controls, zero-width and filler characters, separators, the tag block's invisible ASCII that an agent reading the text still reads).
#[inline]
#[must_use]
pub const fn is_hidden_char(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}')
        || matches!(
            c,
            '\u{00ad}'
                | '\u{034f}'
                | '\u{061c}'
                | '\u{115f}'..='\u{1160}'
                | '\u{17b4}'..='\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{200b}'
                | '\u{200e}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{3164}'
                | '\u{fdd0}'..='\u{fdef}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{fff0}'..='\u{fffb}'
                | '\u{13430}'..='\u{1343f}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0000}'..='\u{e007f}'
        )
        || (c as u32) & 0xfffe == 0xfffe
}

/// What joins a turn's earlier messages into one text (aterm's
/// `ScreenReader::turn_said_before`): a blank line, as a message's own paragraphs are joined.
pub const MESSAGE_GAP: &str = "\n\n";

/// A turn's earlier `messages` as one text ([`MESSAGE_GAP`]).
#[must_use]
pub fn join(messages: &[String]) -> String {
    messages.join(MESSAGE_GAP)
}

/// `s` as a count: ASCII digits only (no sign, space or separator; leading zeros allowed) that
/// fit `T`, else `None`.
#[inline]
pub fn parse_uint<T: std::str::FromStr>(s: &str) -> Option<T> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
