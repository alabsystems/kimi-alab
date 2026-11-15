// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The copy of alab-harness this harness builds against is the one its pin names
//! (alab-harness README, "How a harness pins it"). `tools/check-pin` in
//! alab-harness also compares the pin with the tag; this test holds the half a
//! checkout alone can: the committed copy is the pinned tree and the working
//! copy changes none of it.

use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[test]
fn the_vendored_alab_harness_is_the_pinned_tree_unchanged() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pin = std::fs::read_to_string(root.join("vendor/alab-harness.pin"))
        .expect("vendor/alab-harness.pin");
    let field = |key: &str| {
        pin.lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_else(|| panic!("the pin names {key}"))
            .to_string()
    };
    let (tag, want) = (field("tag="), field("tree="));
    if !root.join(".git").exists() {
        // A source tree with no history (an unpacked archive) has no commit to compare;
        // the release check runs in a checkout, where this test holds.
        eprintln!(
            "no git checkout at {}: the pin ({tag}) is not compared",
            root.display()
        );
        return;
    }
    let have = git(root, &["rev-parse", "--verify", "HEAD:vendor/alab-harness"])
        .expect("HEAD commits vendor/alab-harness");
    assert_eq!(have, want, "the committed copy is not {tag}'s tree");
    let changed = git(
        root,
        &[
            "status",
            "--porcelain",
            "--ignored=matching",
            "--",
            "vendor/alab-harness",
            "vendor/alab-harness.pin",
        ],
    )
    .expect("git status");
    assert!(
        changed.is_empty(),
        "the working copy changes the pinned copy:\n{changed}"
    );
}
