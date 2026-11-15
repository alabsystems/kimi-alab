// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn the_claude_code_directory_is_resolved_as_claude_code_resolves_it() {
    let cwd = Path::new("/w/proj");
    let account = || Some(PathBuf::from("/Users/_owner"));
    for (config, home, want) in [
        (Some("/a"), Some("/h"), "/a"),
        (Some("cc"), Some("/h"), "/w/proj/cc"),
        (Some("../cc/./x"), None, "/w/cc/x"),
        (Some(""), Some("/h"), "/w/proj"),
        (None, Some("/h"), "/h/.claude"),
        (None, Some("h"), "/w/proj/h/.claude"),
        (None, Some(""), "/Users/_owner/.claude"),
        (None, None, "/Users/_owner/.claude"),
    ] {
        assert_eq!(
            claude_dir_at(config, home, cwd, account),
            Ok(PathBuf::from(want)),
            "{config:?} {home:?}"
        );
    }
    assert!(
        claude_dir_at(None, None, cwd, || None)
            .expect_err("no home anywhere")
            .contains("no home for its account")
    );
}
