// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! The press guard: bind an approving keystroke to the box that was judged.

/// The longest pattern the server compiles for a guard
/// (`aterm_observe::row_matcher`'s `MAX_REGEX_PATTERN_LEN`).
pub const MAX_GUARD_BYTES: usize = 1024;

/// How every row guard ends: whatever whitespace the row trails, to the
/// row's end ([`row_guard`]).
pub const ROW_END: &str = "\\s*$";

/// An anchored, whitespace-free pattern that matches `row` exactly (its
/// indentation, its text, and any trailing whitespace the grid pads it with)
/// and nothing else; past [`MAX_GUARD_BYTES`], the longest anchored prefix
/// that fits.
pub fn row_guard(row: &str) -> String {
    let body = row.trim_end();
    let indent = body.chars().take_while(|c| *c == ' ').count();
    let text = &body[indent..];
    let mut out = String::from("^");
    if indent > 0 {
        out.push_str(&format!("\\x20{{{indent}}}"));
    }
    const END: &str = ROW_END;
    for c in text.chars() {
        let piece = escape_char(c);
        if out.len() + piece.len() + END.len() > MAX_GUARD_BYTES {
            return out;
        }
        out.push_str(&piece);
    }
    out.push_str(END);
    out
}

/// `text` as a literal in the server's pattern syntax, as [`row_guard`]
/// writes each character of a row: what a tape's scrub takes a name in
/// within a press guard (aterm's `tape::corpus::Scrub`).
pub fn literal(text: &str) -> String {
    text.chars().map(escape_char).collect()
}

/// One character as a literal in the server's pattern syntax.
fn escape_char(c: char) -> String {
    if c.is_ascii_alphanumeric() || c == '_' {
        c.to_string()
    } else if c == ' ' {
        "\\x20".to_string()
    } else if c.is_ascii_control() || c.is_ascii_whitespace() {
        format!("\\x{:02X}", c as u32)
    } else if "\\.+*?()|[]{}^$".contains(c) {
        format!("\\{c}")
    } else if c.is_whitespace() || c.is_control() {
        format!("\\x{{{:X}}}", c as u32)
    } else {
        c.to_string()
    }
}

/// The arguments of the `key` verb that press `key` under `guard`, fenced on
/// the judged read's screen generation when the server supports it:
/// `if-gen=<epoch>.<seq> if=<guard> <key>`, else `if=<guard> <key>`.
pub fn key_args(guard: &str, key: &str, gen_fence: Option<&str>) -> String {
    match gen_fence {
        Some(generation) => format!("if-gen={generation} if={guard} {key}"),
        None => format!("if={guard} {key}"),
    }
}

/// Whether the server's `help` text (the whole catalog, or `help key`) says
/// `key` takes the `if-gen=<epoch>.<seq>` fence.
pub fn server_fences_gen(help: &str) -> bool {
    entry_names_gen(help, "key")
}

/// Whether the server's `help` text says `send` takes the `if-gen=` fence.
pub fn server_fences_send(help: &str) -> bool {
    entry_names_gen(help, "send")
}

/// Whether the server's `help` text says `key` takes `act=`:
/// what a press means for the box it dismisses.
pub fn server_takes_act(help: &str) -> bool {
    entry_names(help, "key", "act=approve")
}

/// What a dismissing key means for the box it dismisses:
/// the leading `act=<word>` the server's `key` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAct {
    /// The box's command is approved.
    Approve,
    /// The box is declined.
    Deny,
    /// The question is answered.
    Answer,
}

impl KeyAct {
    /// The leading option, `act=<word>`.
    #[must_use]
    pub const fn option(self) -> &'static str {
        match self {
            KeyAct::Approve => "act=approve",
            KeyAct::Deny => "act=deny",
            KeyAct::Answer => "act=answer",
        }
    }
}

/// Whether the help entry of `verb` names `if-gen=`.
fn entry_names_gen(help: &str, verb: &str) -> bool {
    entry_names(help, verb, "if-gen=")
}

/// Whether the help entry of `verb` names `token`.
fn entry_names(help: &str, verb: &str, token: &str) -> bool {
    let mut in_entry = false;
    for line in help.lines() {
        if !line.starts_with(char::is_whitespace) {
            in_entry = line.split_whitespace().next() == Some(verb);
        }
        if in_entry && line.contains(token) {
            return true;
        }
    }
    false
}
