// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Shell-startup policy tests over snapshots and the other readings aterm's
//! `shell_startup::read` gathers, carried as data ([`StartupInputs`]).

use super::*;

/// Where a test worker's zsh snapshot stands, as a reason shows its path (aterm's tests write it
/// as `shell-snapshots/snapshot-zsh-1-a1b2c3.sh` in a temporary Claude Code directory).
const SNAPSHOT: &str = "/Users/_owner/.claude/shell-snapshots/snapshot-zsh-1-a1b2c3.sh";

/// What aterm's `shell_startup::read` gathers for a Unix worker whose `SHELL` is `/bin/zsh` and
/// whose one snapshot is `snapshot`: its Bash tool runs zsh alone (`tool_kinds`), no `.zshenv`
/// stands for zsh to read, and neither its environment nor Claude Code's settings give it a
/// variable. aterm's tests read the same from a temporary Claude Code directory and home, where a
/// host's `/etc/zshenv` reaches them too; these are the readings of a host with none.
fn zsh_worker(snapshot: &str) -> StartupInputs {
    StartupInputs {
        kinds: vec![Kind::Zsh],
        snapshots: vec![(Kind::Zsh, SNAPSHOT.to_string(), Ok(snapshot.to_string()))],
        env_set: Vec::new(),
        settings_env: Ok(Vec::new()),
        zshenv: Some(Ok((false, Vec::new()))),
        bash: None,
    }
}

const CLAUDE_TAIL: &str = "# Check for rg availability
if ! (unalias rg 2>/dev/null; command -v rg) >/dev/null 2>&1; then
  alias rg='/opt/claude/rg'
fi
# Shadow find/grep with embedded bfs/ugrep
unalias find 2>/dev/null || true
unalias grep 2>/dev/null || true
find () {
  ARGV0=bfs \"$_cc_bin\" -S dfs \"$@\"
}
# Shadow pkill to refuse patterns matching the CLI process
unalias pkill 2>/dev/null || true
function pkill {
  command pkill ${1+\"$@\"}
}
export PATH='/usr/bin:/bin'
";

fn zsh_snapshot(functions: &str, options: &str, aliases: &str) -> String {
    format!(
        "# Snapshot file
# Unset all aliases to avoid conflicts with functions
unalias -a 2>/dev/null || true
# Functions
{functions}# Shell Options
setopt nohashdirs
setopt login
{options}# Aliases
alias -- run-help=man
alias -- which-command=whence
{aliases}{CLAUDE_TAIL}"
    )
}

#[test]
fn a_default_zsh_snapshot_fails_no_line_and_its_aliases_fail_the_lines_naming_them() {
    let snapshot = zsh_snapshot(
        "ll2 () {\n\tls -l \"$@\"\n}\n",
        "setopt autocd\nsetopt histignoredups\nsetopt interactivecomments\n",
        "alias -- ll='ls -l'\nalias -- 'we ird'=echo\n",
    );
    let state = ShellStartup::assemble(zsh_worker(&snapshot)).expect("a clean startup");
    assert_eq!(state.admits("rm -rf /private/tmp/x/y"), Ok(()));
    assert_eq!(state.admits("git status --short"), Ok(()));
    let why = state.admits("cd /tmp && ll").expect_err("an alias");
    assert!(why.contains("`ll` is an alias"), "{why}");
    assert!(state.admits("x; ll2 .").is_err(), "a function");
    assert_eq!(state.admits("find . -name x | grep y; rg z"), Ok(()));
    assert_eq!(state.admits("/bin/ll x; ll-x; x.ll"), Ok(()));
    assert!(
        state.admits("\\ll").is_err(),
        "a backslash skips an alias, not a function"
    );
}

#[test]
fn an_option_the_models_do_not_assume_fails_every_line_naming_it() {
    for opt in [
        "globsubst",
        "shwordsplit",
        "ksharrays",
        "rcexpandparam",
        "nullglob",
        "cshnullglob",
        "nonomatch",
        "magicequalsubst",
        "kshglob",
        "globdots",
        "nocaseglob",
        "rcquotes",
        "ignorebraces",
        "noequals",
        "noshortloops",
        "noaliases",
        "posixbuiltins",
        "cdablevars",
        "pathdirs",
        "aliasfuncdef",
        "no_such_option_yet",
    ] {
        let snapshot = zsh_snapshot("", &format!("setopt {opt}\n"), "");
        let why = ShellStartup::assemble(zsh_worker(&snapshot)).expect_err(opt);
        assert!(why.contains(&format!("`setopt {opt}`")), "{opt}: {why}");
    }
    let snapshot = zsh_snapshot("", "setopt noaliases             off\n", "");
    assert!(ShellStartup::assemble(zsh_worker(&snapshot)).is_err());
    let snapshot = zsh_snapshot("", "setopt extendedglob\nsetopt nobareglobqual\n", "");
    assert!(ShellStartup::assemble(zsh_worker(&snapshot)).is_ok());
    let snapshot = zsh_snapshot("f () {\n\tsetopt localoptions globsubst\n}\n", "", "");
    assert!(ShellStartup::assemble(zsh_worker(&snapshot)).is_ok());
}

#[test]
fn a_hook_or_function_that_runs_unnamed_fails_every_line() {
    for f in [
        "chpwd",
        "zshexit",
        "TRAPEXIT",
        "TRAPDEBUG",
        "command_not_found_handler",
        "eval",
        "setopt",
    ] {
        let snapshot = zsh_snapshot(&format!("{f} () {{\n\tprint hi\n}}\n"), "", "");
        let why = ShellStartup::assemble(zsh_worker(&snapshot)).expect_err(f);
        assert!(why.contains(&format!("`{f}` is a function")), "{why}");
    }
}

#[test]
fn a_word_is_named_only_whole() {
    for (line, hit) in [
        ("ls", true),
        ("x; ls -l", true),
        ("(ls)", true),
        ("\"ls\" x", true),
        ("a|ls", true),
        ("/bin/ls", false),
        ("lsof", false),
        ("x-ls", false),
        ("ls.txt", false),
        ("k=ls", false),
    ] {
        assert_eq!(names_word(line, "ls"), hit, "{line}");
    }
}

#[test]
fn a_prompt_or_history_hook_is_an_ordinary_function() {
    let snapshot = zsh_snapshot(
        "precmd () {\n\tvcs_info\n}\npreexec () {\n\tprint -Pn \"\\e]0;$1\\a\"\n}\n\
         periodic () {\n\t:\n}\nzshaddhistory () {\n\t:\n}\n",
        "",
        "",
    );
    let state = ShellStartup::assemble(zsh_worker(&snapshot)).expect("an ordinary startup");
    assert_eq!(state.admits("git status --short"), Ok(()));
    for f in ["precmd", "preexec", "periodic", "zshaddhistory"] {
        let why = state.admits(&format!("x; {f}")).expect_err(f);
        assert!(why.contains(&format!("`{f}` is a function")), "{why}");
    }
}

#[test]
fn a_function_is_named_by_any_quoting_of_its_name() {
    let snapshot = zsh_snapshot(
        "rm () {\n\tcommand rm -i \"$@\"\n}\ngit () {\n\tcommand git \"$@\"\n}\n",
        "",
        "",
    );
    let state = ShellStartup::assemble(zsh_worker(&snapshot)).expect("defaults");
    for line in [
        "r\\m -rf /private/tmp/x",
        "r\"\"m -rf /private/tmp/x",
        "r''m -rf /private/tmp/x",
        "\"r\"m -rf /private/tmp/x",
        "$'\\x72m' -rf /private/tmp/x",
        "$'\\162m' -rf /private/tmp/x",
        "$'r\\x6d' -rf /private/tmp/x",
        "$\"rm\" -rf /private/tmp/x",
        "r\\\nm -rf /private/tmp/x",
        "eval \"r\\\\m d\"",
        "eval \"\\$'\\\\x72m' e\"",
        "g''it status",
        "c\\d /tmp && g\\it log",
    ] {
        assert!(state.admits(line).is_err(), "{line:?}");
    }
    for line in ["/bin/rm -f /private/tmp/x", "rmdir x", "echo arm", "gitk"] {
        assert_eq!(state.admits(line), Ok(()), "{line:?}");
    }
}

#[test]
fn an_alias_fails_only_a_line_that_may_run_it() {
    let mut aliases = String::from("alias -- -='cd -'\nalias -- ...=../..\n");
    for n in 1..=9 {
        aliases.push_str(&format!("alias -- {n}='cd -{n}'\n"));
    }
    aliases.push_str("alias -- _='sudo '\nalias -- ll='ls -lh'\nalias -- md='mkdir -p'\n");
    let snapshot = zsh_snapshot(
        "",
        "setopt autopushd\nsetopt pushdignoredups\nsetopt pushdminus\nsetopt pushdtohome\n",
        &aliases,
    );
    let state = ShellStartup::assemble(zsh_worker(&snapshot))
        .expect("oh-my-zsh's directory options change nothing the models read");
    for line in [
        "cargo test 2>&1 | tail -n 5",
        "git log -1 --oneline",
        "cd - && ls",
        "cd ../.. && ls ...",
        "sleep 1; echo 2 3",
        "head -5 x | sort -k 2",
        "find . -maxdepth 2 -name ll",
        "echo ll md",
        "sudo ll",
        "nice -n 5 ls",
    ] {
        assert_eq!(state.admits(line), Ok(()), "{line:?}");
    }
    for line in [
        "5",
        "true && 1",
        "x; 2",
        "x || 3",
        "x | 4 y",
        "(6)",
        "{ 7; }",
        "echo $(ll)",
        "echo `ll`",
        "echo \"$(ll)\"",
        "X=1 ll",
        "X=1 Y=2 ll",
        "time ll",
        "time -p ll",
        "! ll",
        "if ll; then :; fi",
        "if [[ -n x ]] ll",
        "if (( 1 )) ll",
        "while [[ -z $d ]] { ll; d=1 }",
        "while :; do ll; done",
        "for i in a; do md x; done",
        "> /dev/null ll",
        ">/dev/null ll",
        "2>&1 ll",
        ">| /tmp/f ll",
        "noglob ll",
        "nocorrect ll",
        "repeat 2 ll",
        "_ ll",
        "x\nll",
        "-",
        "cd /tmp; -",
        "...",
        "eval \"ll x\"",
        "eval ll",
        "\\ll",
    ] {
        assert!(state.admits(line).is_err(), "{line:?}");
    }
}

/// `admits` for `line` under `state` names the startup's `$var` (a `READER_ENV` or git
/// variable it may set).
fn fails_by(state: &ShellStartup, line: &str, var: &str) {
    let why = state.admits(line).expect_err(line);
    assert!(why.contains(&format!("`${var}`")), "{line}: {why}");
}

/// A `READER_ENV` variable the worker's environment sets fails a line that may run a reader it
/// redirects, by name or by a path's last part, behind `xargs`, `find -exec` or a substitution
/// (`GREP_OPTIONS='-e x f'` makes BSD grep read `f`, MEASURED 2026-11-05 on macOS 27.0.1); a
/// startup that sets none, or only another, passes the line.
#[test]
fn a_reader_variable_the_worker_sets_fails_the_lines_running_its_reader() {
    let snapshot = zsh_snapshot("", "", "");
    let clean = ShellStartup::assemble(zsh_worker(&snapshot)).expect("a clean startup");
    for line in ["grep x f", "rg x", "python3 r.py"] {
        assert_eq!(clean.admits(line), Ok(()), "{line}");
    }
    // The worker's environment holds GREP_OPTIONS=-e x /etc/passwd.
    let env = ShellStartup::assemble(StartupInputs {
        env_set: vec!["GREP_OPTIONS".to_string()],
        ..zsh_worker(&snapshot)
    })
    .expect("a startup");
    for line in [
        "grep x f",
        "egrep x f",
        "/usr/bin/fgrep x f",
        "find . -exec grep x {} +",
        "ls | xargs grep x",
        "echo $(gr''ep x f)",
    ] {
        fails_by(&env, line, "GREP_OPTIONS");
    }
    // NEGATIVE CONTROL: another reader, or the name inside a longer word.
    for line in ["rg x", "cat grep.txt", "cat my-grep"] {
        assert_eq!(env.admits(line), Ok(()), "{line}");
    }
    // The worker's environment holds PYTHONPATH=/tmp/x.
    let python = ShellStartup::assemble(StartupInputs {
        env_set: vec!["PYTHONPATH".to_string()],
        ..zsh_worker(&snapshot)
    })
    .expect("a startup");
    fails_by(&python, "python3 r.py", "PYTHONPATH");
    assert_eq!(python.admits("grep x f"), Ok(()));
}

/// A `READER_ENV` variable Claude Code's settings for the launch directory give the Bash tool
/// fails a line running its reader, as one the worker's environment sets does.
#[test]
fn a_reader_variable_claude_codes_settings_give_fails_the_lines_running_its_reader() {
    // The launch directory's .claude/settings.json is
    // {"env": {"RIPGREP_CONFIG_PATH": "/x/rg.conf", "LANG": "C"}}.
    let source = "/Users/_owner/project/.claude/settings.json";
    let settings = ShellStartup::assemble(StartupInputs {
        settings_env: Ok(vec![
            (
                source.to_string(),
                "RIPGREP_CONFIG_PATH".to_string(),
                "/x/rg.conf".to_string(),
            ),
            (source.to_string(), "LANG".to_string(), "C".to_string()),
        ]),
        ..zsh_worker(&zsh_snapshot("", "", ""))
    })
    .expect("a startup");
    fails_by(&settings, "rg -n x src", "RIPGREP_CONFIG_PATH");
    // NEGATIVE CONTROL: another reader.
    assert_eq!(settings.admits("grep x f"), Ok(()));
}

/// [`ShellStartup::assemble`] judges the readings in the order aterm's `shell_startup::read` made
/// them, so its first error is the one `read` returned: each snapshot in turn (one that was not
/// read where `read` read it), then the settings' `env`, then each shell's own reading in the
/// order of [`StartupInputs::kinds`]. A shell the Bash tool may run whose own reading is missing
/// is a startup this check does not know; a reading of a shell it does not run is not judged.
/// New with `assemble`: aterm's `read` meets the same errors in the same order as it reads.
#[test]
fn the_readings_are_judged_in_the_order_read_made_them() {
    let clean = zsh_snapshot("", "", "");
    let refused = zsh_snapshot("", "setopt globsubst\n", "");
    let read = |text: &str| (Kind::Zsh, SNAPSHOT.to_string(), Ok(text.to_string()));
    let unread = |why: &str| (Kind::Zsh, SNAPSHOT.to_string(), Err(why.to_string()));
    let failing = |snapshots: Vec<(Kind, String, Result<String, String>)>| StartupInputs {
        kinds: vec![Kind::Bash, Kind::Zsh],
        snapshots,
        env_set: Vec::new(),
        settings_env: Err("the settings".to_string()),
        zshenv: Some(Err("the .zshenv".to_string())),
        bash: Some(Err("the bash variables".to_string())),
    };
    let why = ShellStartup::assemble(failing(vec![read(&refused), unread("the second")]))
        .expect_err("the first snapshot");
    assert!(why.contains("`setopt globsubst`"), "{why}");
    assert_eq!(
        ShellStartup::assemble(failing(vec![unread("the first"), read(&refused)])),
        Err("the first".to_string())
    );
    assert_eq!(
        ShellStartup::assemble(failing(vec![read(&clean)])),
        Err("the settings".to_string())
    );
    let shells = |kinds: Vec<Kind>| StartupInputs {
        kinds,
        settings_env: Ok(Vec::new()),
        ..failing(vec![read(&clean)])
    };
    assert_eq!(
        ShellStartup::assemble(shells(vec![Kind::Bash, Kind::Zsh])),
        Err("the bash variables".to_string())
    );
    assert_eq!(
        ShellStartup::assemble(shells(vec![Kind::Zsh, Kind::Bash])),
        Err("the .zshenv".to_string())
    );
    for (kind, inputs) in [
        (
            Kind::Zsh,
            StartupInputs {
                zshenv: None,
                ..zsh_worker(&clean)
            },
        ),
        (
            Kind::Bash,
            StartupInputs {
                kinds: vec![Kind::Zsh, Kind::Bash],
                ..zsh_worker(&clean)
            },
        ),
    ] {
        let why = ShellStartup::assemble(inputs).expect_err("a shell's own reading is missing");
        assert!(why.contains(&format!("may run {}", kind.name())), "{why}");
        assert!(!unwritten(&why), "{why}");
    }
    // NEGATIVE CONTROL: the reading of a shell the Bash tool does not run.
    let zsh_alone = StartupInputs {
        bash: Some(Err("the bash variables".to_string())),
        ..zsh_worker(&clean)
    };
    assert!(ShellStartup::assemble(zsh_alone).is_ok());
}
