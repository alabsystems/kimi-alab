// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn reads_in(dir: &Path) -> GitReads {
    GitReads {
        dirs: vec![dir.to_path_buf()],
        ..GitReads::default()
    }
}

#[test]
fn a_line_without_git_has_no_git_reads() {
    for line in ["ls -la", "grep -rn git src", "cat .gitignore"] {
        assert_eq!(
            git_reads(line, &[PathBuf::from("/w")], None),
            Ok(None),
            "{line}"
        );
    }
}

#[test]
fn a_git_read_runs_in_the_cwd_and_every_directory_the_line_names() {
    let home = Path::new("/Users/_owner");
    let dirs = |line: &str| {
        git_reads(line, &[PathBuf::from("/w/p")], Some(home))
            .expect("resolvable")
            .expect("a git read")
            .dirs
    };
    assert_eq!(dirs("git status --short"), [PathBuf::from("/w/p")]);
    assert_eq!(
        dirs("git -C sub log -1"),
        [PathBuf::from("/w/p/sub")],
        "-C replaces the directory for that invocation"
    );
    assert_eq!(
        dirs("git -C a -C ../b status"),
        [PathBuf::from("/w/p/b")],
        "-C chains"
    );
    let cd = dirs("cd /elsewhere/r && git status");
    assert!(cd.contains(&PathBuf::from("/w/p")), "{cd:?}");
    assert!(cd.contains(&PathBuf::from("/elsewhere/r")), "{cd:?}");
    let tilde = dirs("cd ~/r; git log -1");
    assert!(
        tilde.contains(&PathBuf::from("/Users/_owner/r")),
        "{tilde:?}"
    );
    let bare = dirs("cd && git status");
    assert!(bare.contains(&PathBuf::from("/Users/_owner")), "{bare:?}");
    let quoted = dirs("git -C \"my dir\" status");
    assert_eq!(quoted, [PathBuf::from("/w/p/my dir")]);
    let led = dirs("cd ./sub && git status");
    assert!(led.contains(&PathBuf::from("/w/p/sub")), "{led:?}");
}

/// A `cd` target bash and zsh may look up in a `CDPATH` (zsh's `cdpath`, which the worker's
/// environment or a `.zshenv` may set) may land anywhere, so a git read after it is refused;
/// `cd ./sub`, `cd ..` and a rooted target are not looked up.
#[test]
fn a_git_read_after_a_cd_a_cdpath_may_place_is_refused() {
    for line in [
        "cd sub && git status",
        "pushd sub && git log -1",
        "cd a/b; git status",
    ] {
        let why = git_reads(line, &[PathBuf::from("/w")], Some(Path::new("/h"))).expect_err(line);
        assert!(why.contains("CDPATH"), "{line}: {why}");
    }
    // NEGATIVE CONTROL
    for line in [
        "cd ./sub && git status",
        "cd ../r && git status",
        "cd /r && git status",
        "cd ~/r && git status",
    ] {
        assert!(
            git_reads(line, &[PathBuf::from("/w")], Some(Path::new("/h"))).is_ok(),
            "{line}"
        );
    }
}

#[test]
fn a_git_read_runs_from_every_directory_the_shell_may_stand_in() {
    let from = [PathBuf::from("/w"), PathBuf::from("/w/sub")];
    let dirs = |line: &str| {
        git_reads(line, &from, None)
            .expect("placed")
            .expect("a git read")
            .dirs
    };
    assert_eq!(dirs("git status"), from);
    assert_eq!(
        dirs("git -C x log"),
        [PathBuf::from("/w/x"), PathBuf::from("/w/sub/x")]
    );
    assert_eq!(
        dirs("cd .. && git status"),
        [
            PathBuf::from("/w"),
            PathBuf::from("/w/sub"),
            PathBuf::from("/")
        ]
    );
}

#[test]
fn a_directory_this_check_cannot_resolve_is_refused() {
    for line in [
        "cd \"$X\" && git status",
        "cd $(mktemp -d) && git status",
        "git -C \"$D\" log",
        "cd - && git status",
        "cd ~bob/r && git status",
        "pushd +1 && git status",
    ] {
        assert!(
            git_reads(line, &[PathBuf::from("/w")], Some(Path::new("/h"))).is_err(),
            "{line}"
        );
    }
}

#[test]
fn a_git_behind_a_wrapper_is_still_placed() {
    let home = Path::new("/Users/_owner");
    let dirs = |line: &str| {
        git_reads(line, &[PathBuf::from("/w/p")], Some(home))
            .unwrap_or_else(|e| panic!("{line}: {e}"))
            .unwrap_or_else(|| panic!("{line}: a git read the check must see"))
            .dirs
    };
    let here = [PathBuf::from("/w/p")];
    assert_eq!(dirs("env -u FOO git status"), here);
    assert_eq!(dirs("env -i git status"), here);
    assert_eq!(dirs("env -P /usr/bin git status"), here);
    assert_eq!(dirs("echo x | xargs -d , git status"), here);
    assert_eq!(dirs("ls | xargs -n1 git log -1"), here);
    assert_eq!(dirs("2>/dev/null timeout 5 git status"), here);
    assert_eq!(dirs("perl -e 'alarm 30; exec @ARGV' git status"), here);
    assert_eq!(dirs("env -C /r git status"), [PathBuf::from("/r")]);
    assert_eq!(
        dirs("env --chdir=sub git status"),
        [PathBuf::from("/w/p/sub")]
    );
    assert_eq!(
        dirs("env --chdir sub git status"),
        [PathBuf::from("/w/p/sub")]
    );
    assert_eq!(dirs("env -iC.. git status"), [PathBuf::from("/w")]);
    assert_eq!(dirs("env -C /r git -C x log"), [PathBuf::from("/r/x")]);
    assert_eq!(dirs("env -C /r git -Cx log"), [PathBuf::from("/r/x")]);
    assert!(dirs("for i in 1 2; do cd /r && git status; done").contains(&PathBuf::from("/r")));
    assert_eq!(
        git_reads("grep -rn git src", &[PathBuf::from("/w")], None),
        Ok(None)
    );
}

#[test]
fn a_git_this_check_cannot_place_is_refused() {
    for line in [
        "echo . | xargs -I {} git -C {} status",
        "echo . | xargs -0I{} git -C {} status",
        "echo . | xargs -0I {} git -C {} status",
        "echo . | xargs -I {} git -C{} status",
        "echo . | xargs -I {} env -C {} git status",
        "env -C \"$D\" git status",
        "env -C $(mktemp -d) git status",
        "git -C\"$D\" log",
        "git -c core.fsmonitor=evil status",
        "git -ccore.fsmonitor=evil status",
        "env -S 'git status'",
        "env -P /tmp/evil git status",
        "while true; do cd ..; git status; done",
        "for i in 1 2 3; do cd sub && git log -1; done",
    ] {
        assert!(
            git_reads(line, &[PathBuf::from("/w")], Some(Path::new("/h"))).is_err(),
            "{line}"
        );
    }
}

#[test]
fn what_a_git_read_does_beyond_reading_is_read_off_the_line() {
    let reads = |line: &str| {
        git_reads(line, &[PathBuf::from("/w")], None)
            .expect("resolvable")
            .expect("a git read")
    };
    assert!(reads("git log --show-signature -1").signatures);
    assert!(reads("git log --format='%h %G?' -3").signatures);
    assert!(reads("git tag -v v1").signatures);
    assert!(!reads("git log --oneline -3").signatures);
    assert!(reads("git log --oneline -3").log_like);
    assert!(!reads("git status").log_like);
    assert!(reads("git remote show origin").remote.is_some());
    assert!(reads("git remote show -n origin").remote.is_none());
    assert!(reads("git remote -v").remote.is_none());
}

fn view(entries: &[(&str, &str, Option<&str>)]) -> GitView {
    GitView {
        config: entries
            .iter()
            .map(|(s, k, v)| (s.to_string(), k.to_string(), v.map(str::to_string)))
            .collect(),
        ..GitView::default()
    }
}

#[test]
fn each_key_that_runs_a_program_on_a_read_is_a_hazard() {
    let r = reads_in(Path::new("/w"));
    for key in [
        "core.fsmonitor",
        "diff.external",
        "diff.pdf.textconv",
        "diff.pdf.command",
        "filter.x.clean",
        "filter.x.smudge",
        "filter.x.process",
    ] {
        let why = hazard(&view(&[("local", key, Some("touch /tmp/p"))]), &r)
            .unwrap_or_else(|| panic!("{key} must be a hazard"));
        assert!(why.contains(key) && why.contains("local"), "{why}");
    }
}

#[test]
fn a_partial_clone_is_a_hazard() {
    let r = reads_in(Path::new("/w"));
    for (key, value) in [
        ("extensions.partialclone", Some("origin")),
        ("remote.origin.promisor", Some("true")),
        ("remote.origin.promisor", None),
    ] {
        let why = hazard(&view(&[("local", key, value)]), &r)
            .unwrap_or_else(|| panic!("{key} must be a hazard"));
        assert!(why.contains("partial clone"), "{why}");
    }
    assert_eq!(
        hazard(
            &view(&[("local", "remote.origin.promisor", Some("false"))]),
            &r
        ),
        None
    );
}

#[test]
fn keys_that_cannot_run_on_this_read_are_not_hazards() {
    let r = reads_in(Path::new("/w"));
    for (key, value) in [
        ("core.fsmonitor", Some("true")),
        ("core.fsmonitor", Some("false")),
        ("core.fsmonitor", None),
        ("core.pager", Some("sh -c 'touch /tmp/p'")),
        ("pager.log", Some("sh -c 'touch /tmp/p'")),
        ("gpg.program", Some("/tmp/evil")),
        ("gpg.ssh.program", Some("/tmp/evil")),
        ("user.name", Some("x")),
        ("credential.helper", Some("osxkeychain")),
    ] {
        assert_eq!(hazard(&view(&[("global", key, value)]), &r), None, "{key}");
    }
}

#[test]
fn a_signing_program_is_a_hazard_only_on_a_read_that_displays_signatures() {
    let v = view(&[("local", "gpg.program", Some("/tmp/evil"))]);
    let mut r = reads_in(Path::new("/w"));
    assert_eq!(hazard(&v, &r), None);
    r.signatures = true;
    assert!(hazard(&v, &r).is_some_and(|w| w.contains("gpg.program")));
    let v = view(&[
        ("local", "gpg.program", Some("/tmp/evil")),
        ("local", "log.showsignature", Some("true")),
    ]);
    let mut r = reads_in(Path::new("/w"));
    assert_eq!(hazard(&v, &r), None, "not a log");
    r.log_like = true;
    assert!(hazard(&v, &r).is_some_and(|w| w.contains("gpg.program")));
}

#[test]
fn an_index_hook_is_a_hazard() {
    let v = GitView {
        index_hook: Some(PathBuf::from("/w/.git/hooks/post-index-change")),
        ..GitView::default()
    };
    let why = hazard(&v, &reads_in(Path::new("/w"))).expect("a hazard");
    assert!(why.contains("post-index-change"), "{why}");
}

fn every_read() -> Vec<GitReads> {
    let mut out = Vec::new();
    for signatures in [false, true] {
        for log_like in [false, true] {
            for grep_index in [false, true] {
                out.push(GitReads {
                    dirs: vec![PathBuf::from("/w")],
                    signatures,
                    log_like,
                    grep_index,
                    remote: None,
                });
            }
        }
    }
    out
}

const KEY_POOL: &[&str] = &[
    "core.fsmonitor",
    "diff.external",
    "diff.pdf.textconv",
    "diff.pdf.command",
    "diff.textconv",
    "filter.lfs.clean",
    "filter.lfs.smudge",
    "filter.crypt.process",
    "filter.lfs.required",
    "extensions.partialclone",
    "remote.origin.promisor",
    "remote.origin.url",
    "gpg.program",
    "gpg.ssh.program",
    "gpg.format",
    "log.showsignature",
    "url.https://planted-user:planted-token@example.com/.insteadof",
    "http.https://example.com/.extraheader",
    "credential.helper",
    "core.pager",
    "pager.log",
    "user.name",
];

const VALUE_POOL: &[Option<&str>] = &[
    None,
    Some("true"),
    Some("TRUE"),
    Some("yes"),
    Some("On"),
    Some("1"),
    Some("false"),
    Some("no"),
    Some("OFF"),
    Some("0"),
    Some(""),
    Some("fsmonitor-watchman"),
    Some("/tmp/planted-program"),
    Some("Authorization: Bearer planted-token"),
    Some("origin"),
];

fn generated(rng: &mut crate::rng::XorShift, depth: usize) -> GitView {
    let scopes = ["system", "global", "local", "worktree", "command"];
    let config = (0..rng.range(0, 7))
        .map(|_| {
            (
                rng.pick(&scopes).to_string(),
                rng.pick(KEY_POOL).to_string(),
                rng.pick(VALUE_POOL).map(str::to_string),
            )
        })
        .collect();
    let drivers = ["lfs", "crypt", "other"];
    GitView {
        config,
        index_hook: (rng.below(5) == 0).then(|| PathBuf::from("/w/.git/hooks/post-index-change")),
        filters_selected: match rng.below(3) {
            0 => None,
            1 => Some(Vec::new()),
            _ => Some(vec![rng.pick(&drivers).to_string()]),
        },
        submodules: if depth == 0 {
            Vec::new()
        } else {
            (0..rng.range(0, 2))
                .map(|i| {
                    (
                        PathBuf::from(format!("/w/sub{i}")),
                        generated(rng, depth - 1),
                    )
                })
                .collect()
        },
        external_diff: (rng.below(6) == 0).then(|| "/tmp/planted-difftool".to_string()),
        assume_unchanged: rng.below(4) == 0,
        pathspec_env: (rng.below(6) == 0).then(|| "GIT_ICASE_PATHSPECS".to_string()),
    }
}

fn fixture_views() -> Vec<GitView> {
    let mut out: Vec<GitView> = [
        ("local", "core.fsmonitor", Some("touch /tmp/p")),
        ("local", "diff.external", Some("touch /tmp/p")),
        ("local", "diff.pdf.textconv", Some("touch /tmp/p")),
        ("local", "diff.pdf.command", Some("touch /tmp/p")),
        ("local", "filter.x.clean", Some("touch /tmp/p")),
        ("local", "filter.x.smudge", Some("touch /tmp/p")),
        ("local", "filter.x.process", Some("touch /tmp/p")),
        ("local", "extensions.partialclone", Some("origin")),
        ("local", "remote.origin.promisor", Some("true")),
        ("local", "remote.origin.promisor", None),
        ("local", "remote.origin.promisor", Some("false")),
        ("global", "core.fsmonitor", Some("true")),
        ("global", "core.fsmonitor", Some("false")),
        ("global", "core.fsmonitor", None),
        ("global", "core.pager", Some("sh -c 'touch /tmp/p'")),
        ("global", "pager.log", Some("sh -c 'touch /tmp/p'")),
        ("global", "gpg.program", Some("/tmp/evil")),
        ("global", "gpg.ssh.program", Some("/tmp/evil")),
        ("global", "user.name", Some("x")),
        ("global", "credential.helper", Some("osxkeychain")),
    ]
    .iter()
    .map(|e| view(&[*e]))
    .collect();
    out.push(view(&[
        ("local", "gpg.program", Some("/tmp/evil")),
        ("local", "log.showsignature", Some("true")),
    ]));
    out.push(GitView {
        index_hook: Some(PathBuf::from("/w/.git/hooks/post-index-change")),
        ..GitView::default()
    });
    out.push(GitView {
        assume_unchanged: true,
        ..GitView::default()
    });
    out.push(GitView {
        pathspec_env: Some("GIT_LITERAL_PATHSPECS".to_string()),
        ..GitView::default()
    });
    out
}

fn projection_holds(project: impl Fn(&GitView) -> GitView) -> Option<(GitView, GitReads)> {
    let mut rng = crate::rng::XorShift::new(0x7a9e_0005);
    let views = fixture_views()
        .into_iter()
        .chain((0..1000).map(|_| generated(&mut rng, 2)));
    for v in views {
        let p = project(&v);
        for r in every_read() {
            if hazard(&v, &r) != hazard(&p, &r) {
                return Some((v, r));
            }
        }
    }
    None
}

#[test]
fn hazard_judges_the_projection_as_it_judges_the_view() {
    assert_eq!(projection_holds(GitView::project), None);
    let dropping = |v: &GitView| {
        let mut p = v.project();
        p.config.retain(|(_, k, _)| k != "log.showsignature");
        p
    };
    let (v, r) = projection_holds(dropping).expect("the control is caught");
    assert!(r.log_like && !r.signatures, "{v:?} {r:?}");
    let forgetting = |v: &GitView| GitView {
        assume_unchanged: false,
        ..v.project()
    };
    let (v, r) = projection_holds(forgetting).expect("the second control is caught");
    assert!(v.assume_unchanged && r.grep_index, "{v:?} {r:?}");
    let unset = |v: &GitView| GitView {
        pathspec_env: None,
        ..v.project()
    };
    let (v, _) = projection_holds(unset).expect("the third control is caught");
    assert!(v.pathspec_env.is_some(), "{v:?}");
}

#[test]
fn the_projection_keeps_only_what_hazard_reads() {
    let secrets = [
        "planted-user",
        "planted-token",
        "osxkeychain",
        "/tmp/planted-program",
        "/tmp/planted-difftool",
        "example.com",
    ];
    let planted = GitView {
        config: vec![
            (
                "global".to_string(),
                "url.https://planted-user:planted-token@example.com/.insteadof".to_string(),
                Some("https://example.com/".to_string()),
            ),
            (
                "global".to_string(),
                "http.https://example.com/.extraheader".to_string(),
                Some("Authorization: Bearer planted-token".to_string()),
            ),
            (
                "global".to_string(),
                "credential.helper".to_string(),
                Some("osxkeychain".to_string()),
            ),
            (
                "local".to_string(),
                "core.fsmonitor".to_string(),
                Some("/tmp/planted-program".to_string()),
            ),
        ],
        external_diff: Some("/tmp/planted-difftool".to_string()),
        ..GitView::default()
    };
    let view = GitView {
        submodules: vec![(PathBuf::from("/w/sub"), planted.clone())],
        ..planted
    };
    let projected = format!("{:?}", view.project());
    for secret in secrets {
        assert!(!projected.contains(secret), "{secret} kept: {projected}");
    }
    let whole = format!("{view:?}");
    for secret in secrets {
        assert!(whole.contains(secret), "{secret} was not planted");
    }
    assert!(
        view.project()
            .config
            .iter()
            .any(|(_, k, v)| k == "core.fsmonitor" && v.as_deref() == Some(VALUE_SET))
    );
}

#[test]
fn a_value_class_reads_as_its_values() {
    for v in VALUE_POOL {
        let class = value_class(*v);
        assert_eq!(is_bool(class.as_deref()), is_bool(*v), "{v:?}");
        assert_eq!(is_false(class.as_deref()), is_false(*v), "{v:?}");
        assert_eq!(is_true(class.as_deref()), is_true(*v), "{v:?}");
    }
}
