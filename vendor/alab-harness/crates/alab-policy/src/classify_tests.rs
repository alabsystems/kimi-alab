// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

use super::*;

fn ro(cmd: &str) -> bool {
    classify_command(cmd).read_only
}

#[test]
fn whole_long_option_names_keep_flags_from_hiding_file_operands() {
    for (line, paths) in [
        ("jq --slurp . a.json b.json", vec!["a.json", "b.json"]),
        (
            "jq -R --slurp . a ~/.ssh/id_rsa",
            vec!["a", "~/.ssh/id_rsa"],
        ),
        ("rg --ignore token ~/.ssh/id_rsa", vec!["~/.ssh/id_rsa"]),
        (
            "rg --ignore ~/.ssh/id_rsa ordinary.txt",
            vec!["ordinary.txt"],
        ),
        // Whole value options and the other grammars retain their meanings.
        ("jq --slurpfile n f.json . a", vec!["f.json", "a"]),
        ("rg --ignore-file .ignore x f", vec![".ignore", "f"]),
        ("grep --incl '*.rs' x f", vec!["f"]),
        ("grep --include '*.rs' x f", vec!["f"]),
        ("git log --author alice -- file", vec!["file"]),
        ("git log --auth alice -- file", vec!["alice", "file"]),
    ] {
        assert_eq!(path_operands(line).expect(line), paths, "{line}");
    }
}

/// Real worker command lines, each with the verdict a person gave it.
#[test]
fn the_session_corpus_classifies_as_the_manager_did() {
    let corpus: &[(&str, bool)] = &[
        (
            "git status --short --branch && git pull 2>&1 | tail -20",
            false,
        ),
        (
            "git show --stat --format='%h %ad %s' d48f | head -60; echo ----; git log --oneline --all --grep='handoff' -i | head -20",
            true,
        ),
        (
            "echo \"== x ==\"; for f in /tmp/a b; do if [ -e \"$f\" ]; then echo \"PRESENT $f ($(stat -f '%Sm' \"$f\"))\"; else echo MISSING; fi; done; uptime; df -h ~ | tail -1",
            true,
        ),
        (
            "timeout 120 python3 scripts/sat2026_standing.py 2>&1 | tail -40",
            false,
        ),
        (
            "find ~ -maxdepth 5 -name 'x.sh' -not -path '*/Library/*' 2>/dev/null | head",
            true,
        ),
        ("find ~ -name '*.stage' -delete", false),
        ("rm -rf ~/x/*.stage", false),
        ("ls ~/x | xargs rm", false),
        ("cat a.txt > b.txt", false),
        ("git log -3 && git push origin main", false),
        ("targo --unverified build --release", false),
        ("python3 -c 'import os; os.remove(\"x\")'", false),
        ("git stash list; git branch -a", true),
        ("sed -i '' 's/a/b/' file", false),
        (
            "cd ~/ay && grep -rn 'x' reports/H.md | head -3; git -C ~/veripb-alab log -3 --format='%h' 2>/dev/null || ls -d ~/*veripb* 2>/dev/null",
            true,
        ),
        (
            "ps -Ao pid=,etime=,args= | grep -iE 'bank' | grep -v grep | head; ls /Volumes",
            true,
        ),
        (
            "git log -1 --format='%h %an <%ae> %ad%n%(trailers)' --date=iso 2769; ls ~/ay/proofs | wc -l",
            true,
        ),
        (
            "perl -e 'alarm 110; exec @ARGV' find / -xdev \\( -name 'a.json' -o -name 'b.tsv' \\) -not -path '/System/*' 2>/dev/null; echo \"find-exit=$?\"",
            true,
        ),
        (
            "find benchmarks/sat -type f \\( -name '*.cnf' -o -name '*.cnf.xz' \\) -exec du -k {} + | sort -rn | head -40",
            true,
        ),
        (
            "wc -l $(grep -rl 'x' crates --include='*.rs' | head -3) 2>/dev/null | tail -4",
            true,
        ),
        (
            "tmutil listlocalsnapshots / | head; tmutil destinationinfo | head -8",
            true,
        ),
        ("python3 - <<'EOF'\nimport json\nEOF", false),
    ];
    for (cmd, want) in corpus {
        let v = classify_command(cmd);
        assert_eq!(
            v.read_only, *want,
            "{cmd:?}: expected read_only={want}, got {v:?}"
        );
    }
}

/// A `>` inside a quoted string is data; a redirect to /dev/null and a descriptor dup are not
/// writes; an unquoted `>` is.
#[test]
fn quotes_hide_redirect_glyphs_but_not_substitutions() {
    assert!(ro("git log --format='%h <%ae> -> %s' | head"));
    assert!(ro("grep x file 2>/dev/null; ls >/dev/null 2>&1"));
    assert!(!ro("ls >> log.txt"));
    assert!(!ro("ls 2> err.txt"));
    assert!(!ro("ls &> all.txt"));
    assert!(!ro("echo \"now: $(rm -rf x)\""));
    assert!(ro("echo \"now: $(date)\""));
    assert!(ro("grep -n 'rm -rf' notes.md"));
    assert!(ro("echo \"git push later\""));
}

/// The alarm wrapper is stripped, with either quote; any other `perl -e` is inline code.
#[test]
fn the_alarm_idiom_is_a_wrapper_not_a_program() {
    assert!(ro("perl -e \"alarm 30; exec @ARGV\" ls -la"));
    assert!(ro("perl -e 'alarm 30; exec @ARGV' git status"));
    assert!(!ro("perl -e 'alarm 30; exec @ARGV' rm -rf x"));
    assert!(!ro("perl -e 'unlink \"x\"'"));
    assert_eq!(classify_command("perl -e 'print 1'").reason, "perl -e");
}

/// Danger tokens anywhere, behind a path or a shell keyword, and the inline-code interpreters.
#[test]
fn danger_tokens_anywhere_fail_the_line() {
    assert!(!ro("ls | xargs /bin/rm"));
    assert!(!ro("for f in *; do rm $f; done"));
    assert!(!ro("if true; then cp a b; fi"));
    assert!(!ro("bash -c 'ls'"));
    assert!(!ro("sh -c ls"));
    assert!(!ro("zsh -c 'echo hi'"));
    assert!(!ro("eval ls"));
    assert!(!ro("source ~/.zshrc"));
    assert!(!ro(". ~/.zshrc"));
    assert!(!ro("sudo ls"));
    assert!(!ro("kill -9 123"));
    assert!(!ro("open ."));
    assert!(!ro("curl https://x.y"));
    assert!(!ro("tee out.txt"));
    assert!(!ro("nohup ls &"));
    assert!(!ro("git -C ~/x commit -m x"));
    assert!(!ro("git -c core.pager=cat fetch origin"));
    assert!(!ro("git worktree prune"));
    assert!(!ro("git worktree remove x"));
    assert!(!ro("find . -execdir mv {} /tmp \\;"));
    assert!(!ro("find . -ok rm {} \\;"));
    assert!(!ro("sed -Ei 's/a/b/' f"));
    assert!(!ro("sed --in-place 's/a/b/' f"));
}

/// An unknown head is not read-only even with no danger token; keyword laundering (`do
/// <cmd>`) is seen through; git needs a read subcommand; read subcommands' write forms fail.
#[test]
fn segment_heads_must_be_known_reads() {
    assert!(!ro("./deploy.sh"));
    assert!(!ro("for f in *; do ./x.sh $f; done"));
    assert!(!ro("if true; then exec claude; fi"));
    assert!(ro("for f in *; do ls $f; done"));
    assert!(ro("VAR=1 OTHER=2 ls"));
    assert!(ro("timeout 5 ls; timeout -k 1 5 git status"));
    assert!(ro(
        "timeout -s TERM 5 ls; timeout --signal=TERM 5 ls; timeout --signal TERM 5 ls"
    ));
    assert!(!ro("timeout -s ls 5 rm -rf /"));
    assert!(!ro("timeout 5 10s ls"));
    assert!(!ro("timeout 5 1.sh ls"));
    assert!(!ro("timeout 5 .sh ls"));
    assert!(!ro("timeout 5 . ls"));
    assert!(ro("git -C ~/x --no-pager log -1"));
    assert!(ro("git --namespace ns status"));
    assert!(!ro("git --namespace status push --force"));
    assert!(!ro("git --super-prefix status push --force"));
    assert!(!ro("git --attr-source status push --force"));
    assert!(!ro("git --attr-source=HEAD status"));
    assert!(!ro("git --git-dir=.git status"));
    assert!(ro("git worktree list; git stash list"));
    assert!(!ro("git worktree"));
    assert!(!ro("git branch -D old"));
    assert!(!ro("git branch -m new"));
    assert!(!ro("git tag v1.0"));
    assert!(!ro("git tag -d v1.0"));
    assert!(ro("git tag -l 'v*'"));
    assert!(ro("git tag --contains abc"));
    assert!(!ro("git remote add x url"));
    assert!(!ro("git remote -v; git remote show origin"));
    assert!(ro("git remote -v; git remote show -n origin"));
    assert!(!ro("git config user.name Bob"));
    assert!(!ro("git config --unset user.name"));
    assert!(ro("git config --get user.name; git config --list"));
    assert!(!ro("tmutil deletelocalsnapshots 2026-01-01"));
    assert!(!ro("tmutil"));
    assert!(!ro("python3 scripts/wipe.py"));
    assert!(!ro("python3 scripts/sat_score_table.py"));
    assert!(!ro("python3 -m http.server"));
    assert!(!ro("python3"));
    let allow = ["tools/*.py".to_string()];
    assert!(classify_command_with("python3 tools/audit.py", &allow).read_only);
    assert!(!classify_command_with("python3 scripts/sat2026_standing.py", &allow).read_only);
    assert!(!ro("echo hi; ruby -e 'puts 1'"));
    assert!(ro("[ -d x ] && echo yes || echo no"));
    assert!(ro("[[ -d x ]] && true"));
    assert!(ro("test -f x && cat x"));
    assert!(ro("which rg; type -a git; env | sort"));
    assert!(ro("ls\ncat x"));
    assert!(!ro("ls\nmake"));
    assert!(!ro("ls `rm x`"));
    assert!(!ro(""));
    assert_eq!(classify_command("   ").reason, "empty command");
}

/// A wrapper's command is the head: each write behind one is refused with its deciding rule,
/// and the neighbours stay reads.
#[test]
fn a_wrapper_cannot_launder_the_program_it_runs() {
    assert_eq!(classify_command("ls | xargs touch").reason, "touch");
    assert_eq!(
        classify_command("find . -name '*.log' | xargs -n1 ./deploy.sh").reason,
        "./deploy.sh (a program named by path)"
    );
    assert!(!ro("echo x | xargs python3 evil.py"));
    assert!(!ro("ls | xargs -I {} sh -c 'cat {}'"));
    assert!(ro("ls | xargs -n1 wc -l"));
    assert!(ro("ls | xargs -0 -I {} cat {}"));
    assert!(ro("ls | xargs"));
    assert_eq!(
        classify_command("env FOO=1 ./deploy.sh").reason,
        "./deploy.sh (a program named by path)"
    );
    assert_eq!(classify_command("env touch /tmp/x").reason, "touch");
    assert!(!ro("env -S 'rm x'"));
    assert!(!ro("/usr/bin/env python3 wipe.py"));
    assert!(ro("env FOO=1 ls; env -i PATH=/bin ls; env -u HOME ls"));
    assert!(ro("env | sort"));
    assert!(ro("timeout 5 env FOO=1 git status"));
    assert!(!ro("timeout 5 xargs ./x.sh"));
    assert!(ro("env -P /usr/bin git status"));
    for line in [
        "env -P /tmp/evil git status",
        "env -P/tmp/evil git status",
        "env -iP /tmp/evil ls",
        "env -P /bin:/tmp/evil ls",
    ] {
        assert!(!ro(line), "{line}");
        assert!(classify_command(line).reason.contains("env -P"), "{line}");
    }
    assert!(ro("env -iC.. git status"));
    assert!(ro("env -iC .. git status"));
    assert!(ro("env -uSOME ls"));
    assert!(!ro("env -iS 'rm x'"));
    assert!(!ro("env --split-string='rm x'"));
    assert!(!ro("env --split='rm x'"));
    assert!(!ro("env -a ls rm -rf /"));
    assert!(!ro("env --argv0 ls rm -rf /"));
    assert!(!ro("env --argv0=ls rm -rf /"));
    assert!(!ro("ls | xargs -0I ls rm -rf /"));
    assert!(!ro("ls | xargs --max-args 1 rm -rf /"));
    assert!(!ro("ls | xargs 2>/dev/null -n 1 ./deploy.sh"));
    assert!(!ro("env 2>/dev/null -u FOO ./deploy.sh"));
}

#[test]
fn a_backgrounded_command_is_a_head_and_a_redirect_ampersand_is_not() {
    assert_eq!(
        classify_command("ls & ./deploy.sh").reason,
        "./deploy.sh (a program named by path)"
    );
    assert_eq!(classify_command("ls & touch /tmp/x").reason, "touch");
    assert!(ro("ls & wc -l x"));
    assert!(ro("ls >/dev/null 2>&1 & echo done"));
    assert!(ro("ls &> /dev/null"));
    assert!(!ro("ls &> all.txt"));
    assert!(ro("cat <&3 2>&1"));
}

#[test]
fn a_backtick_inside_double_quotes_still_runs() {
    assert_eq!(classify_command("echo \"now: `rm -rf x`\"").reason, "rm");
    assert!(ro("echo \"now: `date`\""));
    assert_eq!(
        strip_quotes("echo \"a `date` b\""),
        format!("echo \"\"{QUOTE_SUB_MARK}`date`")
    );
}

#[test]
fn the_alarm_idiom_is_exactly_alarm_then_exec() {
    assert!(!ro(
        "perl -e 'alarm 5; system(\"rm -rf x\"); exec @ARGV' ls"
    ));
    assert_eq!(
        classify_command("perl -e 'alarm 5; unlink \"x\"; exec @ARGV' ls").reason,
        "perl -e"
    );
    assert!(ro("perl -e 'alarm 5 ; exec @ARGV;' ls"));
    assert!(ro("perl -e \"alarm 110;exec @ARGV\" git status"));
    assert!(is_alarm_body("alarm 30; exec @ARGV"));
    assert!(is_alarm_body(" alarm 30 ; exec @ARGV ; "));
    assert!(!is_alarm_body("alarm 30; exec @ARGV; print 1"));
    assert!(!is_alarm_body("alarm; exec @ARGV"));
    assert!(!is_alarm_body("alarm 5; system('x'); exec @ARGV"));
}

#[test]
fn git_branch_creation_and_remote_set_branches_are_writes() {
    assert!(!ro("git branch feature-x"));
    assert!(!ro("git branch -f main HEAD~3"));
    assert!(!ro("git branch --force main HEAD~3"));
    assert!(!ro("git branch --track x origin/x"));
    assert!(!ro("git branch -t x origin/x"));
    assert!(!ro("git branch --set-upstream-to=origin/x"));
    assert!(!ro("git branch -u origin/x"));
    assert!(!ro("git branch --create-reflog x"));
    assert_eq!(
        classify_command("git branch feature-x").reason,
        "git branch (write form)"
    );
    assert!(ro("git branch"));
    assert!(ro(
        "git branch -a; git branch -r; git branch -avv; git branch -vv"
    ));
    assert!(ro("git branch --list 'feat/*'; git branch -l"));
    assert!(ro("git branch --contains abc; git branch --merged main"));
    assert!(ro("git branch --show-current; git branch --points-at HEAD"));
    assert!(ro("git branch --format='%(refname:short)'"));
    assert!(!ro("git remote set-branches origin main"));
    assert!(!ro("git remote set-branches --add origin main"));
    assert!(ro("git remote get-url origin"));
}

/// Shell arithmetic expands an array subscript held in a variable's value and runs its
/// substitution from behind an opaque quote: arithmetic on a non-literal is refused, literal
/// forms stay reads.
#[test]
fn arithmetic_that_can_expand_a_subscript_is_not_a_read() {
    for line in [
        "let 'x=a[$(touch M)]'",
        "local -i x=1",
        "[[ 'a[$(touch M)]' -eq 1 ]]",
        "x='a[$(touch M)]'; [[ $x -lt 1 ]]",
        "if [[ $n -ge 2 ]]; then echo big; fi",
        "[[ -v a[x] ]]",
        "[ -v 'a[$(touch M)]' ]",
        "test -v 'a[$(touch M)]'",
        "x='a[$(touch M)]'; echo ${a[x]}",
        "x='a[$(touch M)]'; echo ${#a[x]}",
        "x='a[$(touch M)]'; echo \"${a[$x]}\"",
        "x='a[$(touch M)]'; echo ${y:x}",
        "x='a[$(touch M)]'; echo ${y:0:x}",
        "x='a[$(touch M)]'; echo ${a[@]:x}",
        "x='a[$(touch M)]'; echo $[x]",
        "x='a[$(touch M)]'; echo \"$[x]\"",
        "x='a[$(touch M)]'; a=(1 2); echo $a[x]",
        "echo ${(e)x}",
        "cat <<EOF\n${a[x]}\nEOF",
        "cat <<EOF\n$a[x]\nEOF",
    ] {
        let v = classify_command(line);
        assert!(!v.read_only, "{line:?} read-only ({})", v.reason);
    }
    for line in [
        "echo ${PIPESTATUS[0]} ${X[@]} ${#X[*]} ${a[-1]}",
        "echo ${y:1} ${y: -2} ${y:1:3} ${a[@]:1:2}",
        "echo ${X:-default} ${X:+set} ${X:?unset} ${#X} ${#} ${!}",
        "echo ${f%.rs} ${f##*/} ${f/a/b} ${!pre*}",
        "[ \"$n\" -eq 1 ] && echo one; test \"$n\" -gt 2",
        "[[ $a == b* ]] && echo match",
        "cat <<'EOF'\n${a[x]} $[x]\nEOF",
        "cat <<EOF\n${HOME} and $USER\nEOF",
        "echo 'a[$x]' \"$HOME\"",
    ] {
        let v = classify_command(line);
        assert!(v.read_only, "{line:?} refused: {}", v.reason);
    }
}

/// Repository-config hazards (`core.fsmonitor`, `diff.external`, textconv)
/// are checked on the machine (`policy::git_config`), not here.
#[test]
fn git_reads_of_a_chosen_repository_or_through_its_drivers_are_refused() {
    for line in [
        "git --git-dir=/tmp/evil status",
        "git --git-dir /tmp/evil log",
        "git --work-tree=/tmp/x diff",
        "git cat-file --filters HEAD:x",
        "git cat-file --textconv HEAD:x",
        "git log -p --textconv -1",
    ] {
        assert!(!ro(line), "{line}");
    }
    assert!(ro("git -C /tmp/x status"));
    assert!(ro("git cat-file -p HEAD:x; git log -p -1"));
}

/// Read tools' write forms: `git branch -v <name>` creates, `sysctl` sets
/// with `=`/`-w`, `hostname` sets with an operand or `-F`, `file -C` writes
/// `magic.mgc`, `tree -R` writes `00Tree.html`, and `<>` opens read-write.
#[test]
fn the_write_forms_of_system_reads_are_refused() {
    for line in [
        "git branch -v newb1",
        "git branch --verbose newb2",
        "git branch -vv nb6",
        "sysctl kern.maxfiles=1",
        "sysctl -w kern.maxfiles=1",
        "hostname evil",
        "hostname -F /tmp/name",
        "file -C -m /tmp/magic",
        "file -bC x",
        "tree -R -H . -L 1",
        "cat <>new.txt",
        "exec 3<>/tmp/x",
    ] {
        assert!(!ro(line), "{line}");
    }
    for line in [
        "git branch -v; git branch -vv; git branch -av; git branch -v --list 'f*'",
        "sysctl -n hw.ncpu; sysctl kern.maxfiles",
        "hostname; hostname -s 2>/dev/null",
        "file -b x; file --mime-type x",
        "tree -L 2 -a",
        "cat < in.txt",
    ] {
        let v = classify_command(line);
        assert!(v.read_only, "{line} refused: {}", v.reason);
    }
}

#[test]
fn output_file_flags_of_the_read_tools_are_writes() {
    assert_eq!(
        classify_command("sort -o /etc/hosts /etc/hosts").reason,
        "sort -o"
    );
    assert!(!ro("sort --output=x f"));
    assert!(!ro("sort --output x f"));
    assert!(!ro("sort -ro out.txt f"));
    assert!(ro("sort -rn f | head; sort -t: -k2 f; sort -u f"));
    assert_eq!(
        classify_command("uniq in.txt out.txt").reason,
        "uniq <output file>"
    );
    assert!(ro(
        "uniq -c in.txt; sort x | uniq -c | sort -rn; uniq -f 1 in.txt"
    ));
    assert!(!ro("uniq -f 1 in.txt out.txt"));
    assert_eq!(classify_command("tree -o /tmp/out.txt").reason, "tree -o");
    assert!(ro("tree -L 2 -a"));
    assert_eq!(
        classify_command("find . -newer x -fprint /tmp/out").reason,
        "find -fprint"
    );
    assert!(!ro("find . -fprintf /tmp/o '%p\\n'"));
    assert!(!ro("find . -fls /tmp/o"));
    assert!(!ro("find . -fprint0 /tmp/o"));
    assert!(ro("find . -name '*.rs' -print0 | xargs -0 wc -l"));
}

#[test]
fn a_python_path_cannot_step_out_of_the_allowlist() {
    assert!(!ro("python3 scripts/../../tmp/evil_score.py"));
    assert!(
        classify_command("python3 scripts/../../tmp/evil_score.py")
            .reason
            .contains("`..`")
    );
    let allow: Vec<String> = ["scripts/*score*.py", "scripts/*report*.py"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let ro_with = |cmd: &str| classify_command_with(cmd, &allow).read_only;
    assert!(!ro_with("python3 scripts/../../tmp/evil_score.py"));
    assert!(!ro_with("python3 scripts/x/../wipe_report.py"));
    assert!(ro_with("python3 scripts/sat_score_table.py"));
    assert!(ro_with("python3 scripts/sub/report_x.py"));
}

/// The awk program and the sed script are read from the raw words: what
/// the quote-stripped scan cannot see.
#[test]
fn awk_programs_and_sed_scripts_are_read_not_dropped() {
    assert!(!ro("awk 'BEGIN{system(\"rm -rf x\")}'"));
    assert!(!ro("awk '{print > \"out\"}' f"));
    assert!(!ro("awk '{print $1 > out}' f"));
    assert!(!ro("awk '{print >> \"out\"}' f"));
    assert!(!ro("awk '{print | \"sh\"}' f"));
    assert!(!ro("awk '{print |& \"sh\"}' f"));
    assert!(!ro("awk -f prog.awk f"));
    assert!(!ro("ls | xargs awk 'BEGIN{system(\"rm x\")}'"));
    assert!(!ro("echo \"$(awk 'BEGIN{system(\"rm x\")}')\""));
    assert!(ro("awk -F: '{print $1}' /etc/passwd"));
    assert!(ro("awk '$3 > 100 {print $1}' f"));
    assert!(ro("awk '$1 > $2' f; awk 'NR>1' f; awk '$3 >= 4' f"));
    assert!(ro("awk '/foo|bar/' f; awk '/a/ || /b/ {print}' f"));
    assert!(!ro("awk -v n=3 'NR > n' f"));
    assert!(ro("awk '{print \"a>b|c\"}' f"));
    assert!(ro("awk '{ s += $1 } END { print s / NR }' f"));
    assert!(ro("awk -- '{print}' f"));

    assert!(!ro("sed 's/a/b/w /tmp/out' file"));
    assert!(!ro("sed -n '/x/w out' f"));
    assert!(!ro("sed 's/a/b/e' f"));
    assert!(!ro("sed '1e date' f"));
    assert!(!ro("sed -f script.sed f"));
    assert!(!ro("sed -e 's/a/b/' -e 'w out' f"));
    assert!(!ro("sed -ne 's/a/b/w out' f"));
    assert!(!ro("sed --expression='w out' f"));
    assert_eq!(
        classify_command("sed 's/a/b/w /tmp/out' file").reason,
        "sed s///w (writes a file)"
    );
    assert!(ro("sed -n 's/new/old/p' f"));
    assert!(ro("sed -e 's/a/b/g' -e '/^$/d' f"));
    assert!(ro("sed '1,10p;$!d' f; sed -n '5,$p' f; sed '/^#/d' f"));
    assert!(ro("sed -E 's/(a|b)+/x/' f; sed 's|/usr|/opt|' f"));
    assert!(ro("sed 's/w/e/; s/e/w/g' f"));
    assert!(ro("sed -n '/start/,/end/{p}' f; sed 'y/abc/xyz/' f"));
    assert!(ro("sed '$!N;P;D' f; sed -n '$=' f; sed 2q f"));
    assert!(ro("sed -e '1i\\header' f"));
    assert_eq!(sed_script_writes("s/a/b/"), None);
    assert_eq!(
        sed_script_writes("/x/w out").as_deref(),
        Some("sed w (writes a file)")
    );
    assert!(
        sed_script_writes("s/a/b").is_some(),
        "unterminated is unparsed"
    );
    assert!(
        sed_script_writes("1,3").is_some(),
        "an address without a command is unparsed"
    );
    assert!(
        sed_script_writes("k").is_some(),
        "an unknown command is unparsed"
    );
}

/// `sed`'s script options hide a write in every spelling sed reads.
#[test]
fn sed_script_options_cannot_hide_a_write() {
    for cmd in [
        "sed -fs.sed input.txt",          // -f attached
        "sed -nfs.sed input.txt",         // -f clustered behind -n
        "sed -f- input.txt",              // -f reading the script from stdin
        "sed --fi s.sed input.txt",       // --file by getopt_long abbreviation
        "sed --fi=s.sed input.txt",       // --file abbreviated, glued
        "sed --file=s.sed input.txt",     // --file glued
        "sed -e'w out' input.txt",        // -e attached, a w that writes
        "sed -ew\\ out input.txt",        // -e attached, an escaped blank
        "sed --e='w out' input.txt",      // --expression abbreviation, glued
        "sed --expr='w out' input.txt",   // --expression abbreviation, glued
        "sed -ne 'w out' input.txt",      // -e clustered, script in the next word
        "sed -l 'w out' input.txt",       // -l takes no value; the next word writes
        "sed --in s/a/b/ input.txt",      // --in-place by abbreviation
        "sed ':x;s/a/b/w out' input.txt", // GNU ends the label at `;`
    ] {
        assert!(!ro(cmd), "{cmd:?} writes but was called read-only");
    }
    assert_eq!(
        classify_command("sed -fs.sed input.txt").reason,
        "sed -f (a script file)"
    );
    for cmd in [
        "sed -n 's/a/b/p' f",
        "sed -e 's/a/b/g' -e '/^$/d' f",
        "sed -ne 's/a/b/p' f",
        "sed --expression='s/x/y/g' f",
        "sed -l 's/a/b/p' f",
    ] {
        assert!(ro(cmd), "{cmd:?} is a read but was refused");
    }
    assert_eq!(
        sed_script_writes(":x;s/a/b/w out").as_deref(),
        Some("sed s///w (writes a file)")
    );
    assert_eq!(sed_script_writes(":a;N;$!ba"), None);
}

/// A `sed` regex delimiter inside a `[…]` bracket expression is literal, so the regex does not
/// end at it.
#[test]
fn a_sed_bracket_delimiter_does_not_end_the_regex_early() {
    for cmd in [
        "sed 's/[/]/g;/w d/p' input.txt",
        "sed 's/[/]/x/w out' f",
        "sed '/[/]/w out' f",
    ] {
        assert!(!ro(cmd), "{cmd:?} writes but was called read-only");
    }
    for cmd in [
        "sed 's/[0-9]/x/g' f",
        "sed '/[abc]/d' f",
        "sed 's/[[:digit:]]/N/g' f",
        "sed 's/[]]/x/g' f",
        "sed 's/[^/]/x/p' f",
    ] {
        assert!(ro(cmd), "{cmd:?} is a read but was refused");
    }
    assert_eq!(
        sed_script_writes("s/[/]/x/w f").as_deref(),
        Some("sed s///w (writes a file)")
    );
    assert_eq!(sed_script_writes("s/[0-9]/x/g"), None);
    assert_eq!(sed_script_writes("s/[^/]/x/p"), None);
    assert_eq!(sed_script_writes("/[/]/d"), None);
}

/// `sysctl` sets the kernel from a file of `name=value` pairs whose values never reach argv.
#[test]
fn sysctl_sets_the_kernel_from_a_file() {
    for cmd in [
        "sysctl -f /tmp/conf",
        "sysctl --file /tmp/conf",
        "sysctl --file=/tmp/conf",
        "sysctl -nf /tmp/conf",
        "sysctl -p",
        "sysctl --system",
    ] {
        assert!(!ro(cmd), "{cmd:?} sets the kernel but was called read-only");
        assert!(
            classify_command(cmd).reason.contains("sysctl"),
            "{cmd:?}: {}",
            classify_command(cmd).reason
        );
    }
    for cmd in [
        "sysctl -n hw.ncpu",
        "sysctl kern.maxfiles",
        "sysctl -a",
        "sysctl hw.ncpu vm.swapusage",
    ] {
        assert!(ro(cmd), "{cmd:?} is a read but was refused");
    }
}

/// A bare input redirection with no command runs `$READNULLCMD` under zsh and cannot be approved.
#[test]
fn a_bare_redirection_with_no_command_is_not_read_only() {
    for cmd in ["< f", "<f", "< /dev/null", "cat x; < f"] {
        assert!(!ro(cmd), "{cmd:?} runs $READNULLCMD but was read-only");
    }
    assert!(
        classify_command("< f")
            .reason
            .contains("bare input redirection"),
        "{}",
        classify_command("< f").reason
    );
    for cmd in [
        "cat < f",
        "> /dev/null",
        "2> /dev/null",
        "cat > /dev/null",
        "ls > /dev/null 2>&1",
        "diff <(ls a) <(ls b)",
    ] {
        assert!(ro(cmd), "{cmd:?} is a read but was refused");
    }
    for cmd in ["< f", "rm -f /tmp/x; < f"] {
        let v = classify_except_rm(cmd, DEFAULT_PYTHON_ALLOW);
        assert!(
            !v.read_only,
            "{cmd:?} runs $READNULLCMD but was read-only: {v:?}"
        );
    }
}

/// A file read by substitution and a subshell's redirect run no null command.
#[test]
fn a_file_read_by_substitution_or_a_subshell_redirect_is_read_only() {
    for cmd in [
        "echo \"$(< VERSION)\"",
        "cat $(<files.txt)",
        "wc -l $(<files.txt)",
        "cat $(< list.txt) | wc -l",
        "echo $( < f )",
        "echo `<f`",
        "echo \"`< f`\"",
        "echo $(0<f)",
        "(cat) < in.txt",
        "(cat) <in.txt",
        "(cat; ls in.txt) < in.txt",
    ] {
        assert!(
            ro(cmd),
            "{cmd:?} reads a file but was refused: {:?}",
            classify_command(cmd)
        );
        let v = classify_except_rm(cmd, DEFAULT_PYTHON_ALLOW);
        assert!(v.read_only, "{cmd:?} (rm breaker) was refused: {v:?}");
    }
    for cmd in [
        "(< f)",
        "echo $(< f; true)",
        "echo $(true; < f)",
        "echo $(<f <f)",
        "echo $(<f &)",
        "echo $(3<f)",
        "(cat); < f",
        "(cat) | < f",
    ] {
        assert!(!ro(cmd), "{cmd:?} runs a null command but was read-only");
    }
}

/// An awk program runs or writes in every spelling awk reads.
#[test]
fn an_awk_program_that_runs_or_writes_in_any_spelling_is_no_read() {
    for cmd in [
        "awk 'BEGIN{system (\"touch m1\")}'",
        "awk '{system}' cmds.txt",
        "awk '{print $1 > $1}' f",
        "awk 'BEGIN{print 1 > 4}'",
        "awk 'BEGIN { if (1) print \"y\" > 5 }'",
        "awk 'BEGIN { printf \"%s\", \"z\" > -6 }'",
        "awk 'BEGIN { print \"a\",\n \"b\" > 10 }'",
        "awk 'BEGIN { # \"\nprint 1 > 2\n# \"\n}'",
        "awk 'BEGIN { print /\"/ > 9 }'",
        "awk '{ x = 1 }\n/\"/ { print \"touch m8\" | \"sh\" }' in1",
        "awk '{ n++ / 2 }' f",
        "awk -E prog.awk f",
        "awk --exec=prog.awk f",
        "awk -d/tmp/x 'BEGIN{}'",
        "awk -o 'BEGIN{}'",
        "awk -W dump-variables=/tmp/x 'BEGIN{}'",
        "awk -e 'BEGIN{}' -e 'BEGIN{system(\"x\")}'",
        "awk --source='BEGIN{system(\"x\")}' f",
        "awk --source 'BEGIN { print 1 > 2 }'",
        "awk '@include \"x.awk\"'",
        "awk 'BEGIN { f = \"sys\" \"tem\"; @f(\"date\") }'",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} is no read");
    }
    for cmd in [
        "awk '{ if ($1 > 3) print $2 }' f",
        "awk '{print ($1 > 3)}' f",
        "awk '{print \"a>b\"}' f",
        "awk '{ n++ } END { print n }' f",
        "awk -F'\\t' -v OFS=, '{print $1, $2}' f",
        "awk '/a\\/b/ {print}' f",
        "awk '# a \"quoted\" comment\n{print $1}' f",
        "awk '{print \"systemd\"}' f",
        "awk '{ print $1 }\n$2 > 3 { print $3 }' f",
        "awk --assign=n=3 --field-separator=: '{print $n}' f",
        "awk -e '{print $1}' f",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} refused: {}", v.reason);
    }
}

/// macOS awk (BWK 20200816) ignores every long option and `-e` and runs the next word as its
/// program, so an awk line it reads otherwise than gawk is no read (MEASURED on `/usr/bin/awk`:
/// `docs/measured/classifier-audit-3-fixes-2026-10-29.md`).
#[test]
fn an_awk_line_macos_awk_reads_otherwise_than_gawk_is_no_read() {
    for cmd in [
        "awk --field-separator 'BEGIN{system(\"touch m1\")}' in.txt",
        "awk --assign 'BEGIN{system(\"touch m2\")}' in.txt",
        "awk --field-separator 'BEGIN{system(\"touch m1\")}'",
        "awk --field-separator '{print}' ~/.ssh/id_rsa",
        "awk --field-separator , '{print $1}' in.txt",
        "awk --source='{print}' 'BEGIN{system(\"touch m3\")}'",
        "awk -e '{print}' -e key",
        "awk -e '{print $1}' -F , in.txt",
        "awk -e '{print}' -- key",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} is no read");
    }
    let v = classify_command("awk --assign 'BEGIN{system(\"touch m2\")}' in.txt");
    assert!(v.reason.contains("macOS awk (BWK)"), "{}", v.reason);
    // NEGATIVE CONTROLS
    for cmd in [
        "awk --field-separator=, '{print $1}' in.txt",
        "awk --assign=n=3 '{print $n}' in.txt",
        "awk -F , '{print $2}' in.txt",
        "awk -F, -e '{print $2}' in.txt",
        "awk -v OFS=, -e '{print $1, $2}' in.txt",
        "awk -e '{print}' in.txt",
        "awk -- '{print}' in.txt",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} refused: {}", v.reason);
    }
}

/// A glob is its own command's words and a substitution's output its parent's, so a glob in
/// `ls`'s words before `| sort` is never sort's; an unquoted parameter holding one (`$?`) counts.
#[test]
fn a_glob_belongs_to_the_command_whose_words_hold_it() {
    let globs = |line: &str| -> Vec<(String, Option<char>)> {
        command_words_globbed(line)
            .expect(line)
            .into_iter()
            .map(|(seg, g)| (seg[0].clone(), g))
            .collect()
    };
    let s = |p: &str, g: Option<char>| (p.to_string(), g);
    assert_eq!(
        globs("ls /etc/*.conf | sort"),
        [s("ls", Some('*')), s("sort", None)]
    );
    assert_eq!(
        globs("cd /etc && ls *.conf"),
        [s("cd", None), s("ls", Some('*'))]
    );
    assert_eq!(
        globs("echo \"$(ls *.md)\" | awk 'NR<3'"),
        [s("ls", Some('*')), s("echo", Some('*')), s("awk", None)]
    );
    assert_eq!(
        globs("awk 'NR<3' `ls src/?.rs`"),
        [s("ls", Some('?')), s("awk", Some('?'))]
    );
    assert_eq!(
        globs("cat x; echo $?"),
        [s("cat", None), s("echo", Some('?'))]
    );
    // NEGATIVE CONTROLS
    assert_eq!(globs("ls '*.md' \\*.txt \"a?\" \"$*\""), [s("ls", None)]);
    assert_eq!(
        globs("cat README.md | sort"),
        [s("cat", None), s("sort", None)]
    );
}

/// SORT AND GIT TAKE AN ABBREVIATED LONG OPTION FOR THE WHOLE ONE
/// ([`long_option_spells`]).
#[test]
fn an_abbreviated_write_option_is_the_write() {
    for cmd in [
        "sort --o=out f",
        "sort --outp out f",
        "sort --compress=./x.sh -S 100K f",
        "sort --co=./x.sh f",
        "git config --unset-a user.name",
        "git config --rem a",
        "git config --ed",
        "git branch --unset-u",
        "git branch --edit-d",
        "git tag -l --del v1",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} is no read");
    }
    for cmd in [
        "sort -t: -k2 f",
        "sort --numeric-sort --reverse --unique --key=2 f",
        "sort --random-sort f",
        "git config --get user.name",
        "git config --get-all k",
        "git config --list --show-origin",
        "git branch --list --sort=-committerdate",
        "git branch --format='%(refname)' --list",
        "git branch --contains HEAD --no-color",
        "git tag --list --sort=v:refname",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} refused: {}", v.reason);
    }
}

/// A substitution is a word of its command here too, its body's words a
/// segment of their own, first; a parameter is a substitution too.
#[test]
fn program_words_resolve_quotes_and_split_where_the_segments_do() {
    assert_eq!(
        program_words("awk 'a b' \"c $x\" d\\ e | grep x; echo `date`"),
        vec![
            vec!["awk", "a b", "c $…", "d e"],
            vec!["grep", "x"],
            vec!["date"],
            vec!["echo", SUBSTITUTION],
        ]
    );
    assert_eq!(
        program_words("echo \"x $(stat -f '%m' \"$f\") y\""),
        vec![vec!["stat", "-f", "%m", "$…"], vec!["echo", "x $… y"]]
    );
    assert_eq!(
        program_words("awk \"$(cat p.awk)\" f; a$(b)c"),
        vec![
            vec!["cat", "p.awk"],
            vec!["awk", SUBSTITUTION, "f"],
            vec!["b"],
            vec!["a$…c"],
        ]
    );
    assert_eq!(
        program_words("echo \"a \\\" b\" 'c\\d' '$x' \\$y"),
        vec![vec!["echo", "a \" b", "c\\d", "$x", "$y"]]
    );
}

/// Lines that must not classify read-only, each a write or a program of the line's choosing.
#[test]
fn the_measured_bypasses_are_not_read_only() {
    let bypasses = [
        "git log --oneline -3 # what's new\ngit push --force",
        "ls # don't worry\nmv a b",
        "ls # don't\nrm -rf \"$S/x\"",
        "ls # it's\nrm -rf / # '",
        "cat <<EOF\necho '\nEOF\nrm -rf /",
        "echo $'a\\'b'\nrm -rf /\necho '",
        "rm>/dev/null -rf /",
        "ls >&out.txt",
        "git diff --output=/Users/_owner/.zshrc",
        "git log --output=/tmp/x",
        "git -c core.fsmonitor='touch /tmp/pwn' status",
        "git -c core.pager='sh -c \"rm -rf ~\"' log",
        "git -C ~/x -c core.pager=cat log",
        "git grep -O\"touch /tmp/x\" foo",
        "git diff --ext-diff",
        "rg --pre ./x.sh foo",
        "rg --pre=./x.sh foo",
        "sort --compress-program=./x.sh f",
        "/tmp/evil/ls",
        "./ls",
        "env -i PATH=/tmp/evil ls",
        "printf -v PATH /evil",
        "export PATH=/tmp/evil:$PATH; ls",
        "PATH=/tmp/evil ls",
        "GIT_EXTERNAL_DIFF=./x.sh git diff",
        "HOME=/tmp/evil git log",
        "less +!touch\\ x file",
        "less -o copy.txt file",
        "date -s 12:00",
        "date 0101120026",
        "python3 scripts/purge_report.py --delete-all",
        "ls $'x\\ny'",
        "echo ${x:-$(rm -rf y)}",
        "echo \"unterminated",
        "cat <<EOF\n$(rm -rf x)\nEOF",
        "cat <<EOF",
        "awk>/dev/null 'BEGIN{system(\"rm x\")}'",
    ];
    for cmd in bypasses {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
}

/// `git grep -O`/`--open-files-in-pager` runs the pager without a terminal,
/// in any cluster or abbreviation; `git <sub> --help`, `git --help <sub>`
/// and `git -h <sub>` run the configured viewer.
#[test]
fn git_grep_pager_spellings_and_git_help_are_not_read_only() {
    for cmd in [
        "git grep -nO hello",
        "git grep -iO hello",
        "git grep -iOless hello",
        "git grep -3O hello",
        "git grep --open hello",
        "git grep --op hello",
        "git grep --open-files hello",
        "git grep --o hello",
        "git grep --open=less hello",
        "git log --help",
        "git status --help",
        "git grep --help",
        "git stash --help",
        "git --help log",
        "git -h log",
        "git -C . log --help",
        "git --no-pager log --help",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "git grep -eOops x",
        "git grep -A1 -n hello",
        "git grep -n hello",
        "git grep --only-matching hello",
        "git grep --or -e a -e b",
        "git log -h",
        "git log --oneline",
    ] {
        assert!(ro(cmd), "{cmd:?} stays a read: {:?}", classify_command(cmd));
    }
}

/// Negative controls for the pre-pass: shapes it reads rather than refuses
/// stay reads.
#[test]
fn comments_heredocs_and_quoting_that_only_read_stay_reads() {
    assert!(ro("git log --oneline -3 # what's new"));
    assert!(ro("ls # don't worry\ncat x"));
    assert!(ro("cat <<'EOF'\n$(this is data)\nEOF"));
    assert!(ro("cat <<EOF\nplain body with 'quote\nEOF\nls"));
    assert!(ro("grep -c x <<-EOF\n\tbody\n\tEOF"));
    assert!(ro("cat <<< 'here string'"));
    assert!(ro("echo $'plain'; echo $\"x\""));
    assert!(ro("echo ${HOME} ${x:-y} ${#x}"));
    assert!(ro("echo \"#not a comment\" x#y $#"));
    assert!(ro("ls >&2; ls 2>&1; ls >&-"));
    assert!(ro("ls>/dev/null"));
    assert_eq!(
        prelex("cat <<'EOF'\n$(rm x)\nEOF\nls").as_deref(),
        Ok("cat <<'EOF'\nls")
    );
    assert_eq!(prelex("ls # a 'b\ncat").as_deref(), Ok("ls \ncat"));
    assert!(prelex("cat <<EOF\n$(rm x)\nEOF").is_err());
    assert!(!ro("python3 <<'EOF'\nprint(1)\nEOF"));
    assert!(!ro("bash <<'EOF'\nls\nEOF"));
}

/// A `#` right after an escaped blank or newline opens no comment.
#[test]
fn a_hash_after_an_escaped_blank_or_newline_opens_no_comment() {
    for cmd in [
        "echo ab\\ #; tee x",
        "echo ab\\\t#; tee x",
        "echo ab\\\n#; tee x",
        "echo ab \\\n#; tee x",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?}: {v:?}");
        assert_eq!(v.reason, "tee", "{cmd:?}");
    }
    for cmd in [
        "echo ab #; tee x",
        "echo ab\t#; tee x",
        "echo ab\\# #; tee x",
    ] {
        assert!(ro(cmd), "{cmd:?}: {:?}", classify_command(cmd));
    }
    assert!(!ro("echo ab\\#; tee x"));
}

/// A program word is judged where it runs: as an argument it is data; at a
/// head (also behind a wrapper, keyword or substitution) it is the program.
#[test]
fn a_program_word_is_judged_only_at_a_head() {
    assert!(ro("grep -rn open src"));
    assert!(ro("rg -n make Cargo.toml"));
    assert!(ro("grep -c kill crates"));
    assert!(ro("git log --author make"));
    assert!(ro("echo rm; grep 'rm -rf' notes.md"));
    assert!(!ro("open src"));
    assert!(!ro("ls; make"));
    assert!(!ro("ls | xargs kill"));
    assert!(!ro("timeout 5 rm x"));
    assert!(!ro("env rm x"));
    assert!(!ro("for f in *; do make; done"));
    assert!(!ro("echo $(make)"));
    assert!(!ro("ls & open ."));
    assert_eq!(classify_command("ls; make").reason, "make");
}

#[test]
fn the_added_reads_are_reads_and_their_neighbours_are_not() {
    assert!(ro(
        "pgrep -f aterm; sleep 5; strings /bin/ls | head; lsof -p 1"
    ));
    assert!(ro("uname -a; whoami; id; sw_vers; fold -w 80 f"));
    assert!(ro("trustc -vV; rustc --version; trustc --print sysroot"));
    assert!(!ro("trustc x.rs"));
    assert!(!ro("trustc -vV x.rs"));
    assert!(!ro("trustc"));
    assert!(!ro("pkill aterm"));
    assert!(ro("env -i PATH=/bin:/usr/bin ls"));
    assert!(ro(
        "date +%s; date -u; date -r 1700000000; date -j -f %s 1 +%Y"
    ));
    assert!(ro("less -R f; more f; printf '%s\\n' x"));
    assert!(ro("git log -3 --format=%h; git -C ~/x --no-pager log -1"));
    assert!(ro("rg --pre-glob '*.gz' x; sort -rn f"));
    assert!(ro("/bin/ls; /usr/bin/wc -l f"));
    assert!(!ro("/bin/x/ls"));
    assert!(ro("FOO=1 ls; LC_ALL=C sort f"));
    assert!(ro("date 2>/dev/null; date +%s 2> /dev/null"));
    assert!(!ro("date 2>/dev/null 0101120026"));
    assert!(ro("time ls; time -p git status"));
    assert!(!ro("time rm x"));
    assert!(ro("for d in *; do n=$(ls $d | wc -l); echo $n; done"));
    assert!(!ro("for d in *; do n=$(rm $d); done"));
}

/// `aterm ctl` is read-only by verb and form: `meta` reads and `meta set` writes the owner's
/// badge; `inbox` reads and `inbox seen` writes.
#[test]
fn aterm_forms_are_judged_by_their_full_subform() {
    for read in [
        "aterm ctl status",
        "aterm ctl @s-1 status",
        "aterm ctl --sock /tmp/a.sock @s-1 text",
        "aterm ctl ls",
        "aterm ctl windows",
        "aterm ctl help",
        "aterm ctl @s-1 meta",
        "aterm ctl @s-1 inbox",
        "aterm ctl @s-1 inbox get 3",
        "aterm ctl @s-1 resizes 20 since=4",
        "aterm ctl turn --help",
        "aterm-ctl status",
        "aterm --version",
        "aterm help introspection",
        "aterm pkg list",
        "aterm pkg which claude",
        "aterm pkg doctor",
        "aterm drive phase",
        "aterm drive classify -- 'ls'",
    ] {
        assert!(ro(read), "{read:?}: {:?}", classify_command(read));
    }
    for write in [
        "aterm ctl @s-1 meta set attention hi",
        "aterm ctl @s-1 meta unset attention",
        "aterm ctl @s-1 inbox seen 3 handled",
        "aterm ctl @s-1 key 1",
        "aterm ctl @s-1 send x",
        "aterm ctl @s-1 turn 'go'",
        "aterm ctl @s-1 image /tmp/x.png",
        "aterm ctl @s-1 post to=@s-2 kind=note hi",
        "aterm ctl",
        "aterm pkg install x",
        "aterm pkg repair",
        "aterm drive watch",
        "aterm",
        "aterm ctl status > /tmp/out",
    ] {
        assert!(!ro(write), "{write:?} must not be read-only");
    }
}

/// A manager's worker verbs: the task record, the wait on it, the result and
/// the survey only read, so a box that runs one is a read; `task mark` writes
/// the record and `spawn agent=claude` starts a worker on a task, so a box
/// that runs either goes to a person under `approve = "safe"`.
#[test]
fn the_worker_verbs_are_judged_by_their_form() {
    for read in [
        "aterm ctl @s-1 task",
        "aterm ctl await task @s-1,@s-2",
        "aterm ctl await task @s-1,@s-2 all timeout=600000",
        "aterm ctl await task @s-1,@s-2 since=0",
        "aterm ctl await task @s-1:7,@s-2:4",
        "aterm ctl @s-1 await task since=3",
        "aterm ctl @s-1 result",
        "aterm ctl @s-1 result full",
        "aterm ctl survey",
        "aterm ctl survey --json --budget 4000",
        "aterm ctl survey --sid s-1",
        "aterm ctl --sock /tmp/a.sock survey --fleet",
        "aterm-ctl survey",
        "aterm drive ledger @s-1",
        "aterm drive ledger @s-1 --format md --since 2026-10-15",
        "aterm drive --socket /tmp/a.sock ledger @s-1",
        "aterm drive --help",
        "aterm-drive ledger @s-1",
    ] {
        assert!(ro(read), "{read:?}: {:?}", classify_command(read));
    }
    for (write, reason) in [
        ("aterm ctl @s-1 task mark done", "aterm ctl task mark"),
        (
            "aterm ctl @self spawn agent=claude - <<'EOF'\nrun the suite\nEOF",
            "aterm ctl spawn agent=claude",
        ),
        (
            "aterm ctl spawn agent=claude model=opus - < task.md",
            "aterm ctl spawn agent=claude",
        ),
        (
            "aterm drive ledger @s-1 --format html --out /tmp/l.html",
            "aterm drive ledger --out (writes a file)",
        ),
        (
            "aterm-drive ledger --out /tmp/l.html",
            "aterm-drive ledger --out (writes a file)",
        ),
        (
            "aterm drive --socket /tmp/a.sock answer @s-1 2",
            "aterm drive answer",
        ),
    ] {
        let v = classify_command(write);
        assert!(!v.read_only, "{write:?} must not be read-only");
        assert_eq!(v.reason, reason, "{write:?}");
    }
}

/// `mark` writes a claim into a session's log: a worker's `aterm ctl … mark …` box is judged a
/// write, while the session log's reads (`log`, `brief`, `await log …`) stay reads.
#[test]
fn mark_is_not_a_read_only_ctl_verb() {
    for write in [
        "aterm ctl @s-1 mark - note text=hi",
        "aterm ctl @s-1 mark p3 act word=approve",
        "aterm-ctl mark - verdict word=done",
    ] {
        assert!(!ro(write), "{write:?} must not be read-only");
    }
    for read in [
        "aterm ctl @s-1 log",
        "aterm ctl @s-1 brief",
        "aterm ctl @s-1 await log need timeout=1000",
        "aterm ctl mark --help",
    ] {
        assert!(ro(read), "{read:?}: {:?}", classify_command(read));
    }
}

#[test]
fn the_python_allowlist_is_opt_in() {
    assert!(DEFAULT_PYTHON_ALLOW.is_empty());
    assert!(!ro("python3 scripts/x_report.py"));
    let allow = ["scripts/*standing*.py".to_string()];
    assert!(
        classify_command_with(
            "timeout 120 python3 scripts/sat2026_standing.py 2>&1 | tail -40",
            &allow
        )
        .read_only
    );
}

#[test]
fn glob_matches_like_fnmatch() {
    assert!(glob_match(
        "scripts/*standing*.py",
        "scripts/sat2026_standing.py"
    ));
    assert!(glob_match("scripts/*report*.py", "scripts/a/b/report_x.py"));
    assert!(!glob_match("scripts/*standing*.py", "scripts/standing.txt"));
    assert!(glob_match("?.py", "a.py"));
    assert!(!glob_match("?.py", "ab.py"));
    assert!(glob_match("*", ""));
    assert!(!glob_match("a", ""));
    assert!(glob_match("a*b*c", "aXXbYYc"));
    assert!(!glob_match("a*b*c", "aXXbYY"));
}

#[test]
fn strip_quotes_keeps_shape() {
    assert_eq!(strip_quotes("echo 'a b' \"c\""), "echo \"\" \"\"");
    assert_eq!(
        strip_quotes("find \\( -name 'x' \\)"),
        "find ( -name \"\" )"
    );
    assert_eq!(
        strip_quotes("echo \"x $(stat -f '%m' \"$f\")\""),
        format!("echo \"\"{QUOTE_SUB_MARK}$(stat -f \"\" \"\"{QUOTED_PARAMETER})")
    );
    assert_eq!(strip_quotes("find . \\;"), "find . \\;");
    assert_eq!(strip_quotes("echo 'unterminated"), "echo \"\"");
}

/// A substitution is no separator: it is a word of the segment around
/// it, and its body's segments come first.
#[test]
fn split_segments_sees_every_separator() {
    let segs = split_segments("a; b && c || d | e $(f) (g) x `h`\ni \\; j & k 2>&1 &>l m&>n");
    let heads: Vec<&str> = segs.iter().map(|s| s[0].as_str()).collect();
    assert_eq!(
        heads,
        ["a", "b", "c", "d", "f", "e", "g", "h", "x", "i", "k"]
    );
    assert_eq!(segs[5], ["e", SUBSTITUTION]);
    assert_eq!(segs[8], ["x", SUBSTITUTION]);
    assert_eq!(segs[9], ["i", "\\;", "j"]);
    assert_eq!(segs[10], ["k", "2>&1", "&>l", "m", "&>n"]);
    assert_eq!(
        split_segments("git tag $(date +%s); x=\"\"$(y $(z))w"),
        [
            vec!["date", "+%s"],
            vec!["git", "tag", SUBSTITUTION],
            vec!["z"],
            vec!["y", SUBSTITUTION],
            vec!["x=\"\"$…w"],
        ]
    );
}

/// A command substitution is a word of the command around it and its body a command line of
/// its own; a command whose name is unreadable cannot be cleared (`R=rm; $R -rf /usr`).
#[test]
fn a_substitution_is_a_word_of_the_command_around_it() {
    for (cmd, reason) in MEASURED_SUBSTITUTION_BYPASSES {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(v.reason, *reason, "{cmd:?}");
    }
    for (cmd, name) in [
        ("R=rm; $R -rf /usr", "$R"),
        ("R=rm; ${R} -rf $HOME", "${R}"),
        ("P=/bin/rm; $P -rf /usr", "$P"),
        ("P=rm; $P -rf /usr", "$P"),
        ("/bin/$R -rf /usr", "/bin/$R"),
        ("\"$(printf 'r''m')\" -rf /usr", "\"\"$(…)"),
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?}");
        assert_eq!(
            v.reason,
            format!("a command whose name this check cannot read ({name})"),
            "{cmd:?}"
        );
    }
    for (cmd, reason) in [
        (
            "awk \"$(cat prog.awk)\" f",
            "awk: a program this check cannot read ($(…))",
        ),
        (
            "sed -n \"$(cat script.sed)\" f",
            "sed: a script this check cannot read ($(…))",
        ),
        (
            "export $(printf 'PA''TH=/tmp/evil'); ls",
            "export $(…) (an assignment this check cannot read)",
        ),
        (
            "export \"PATH=/tmp/evil\"; ls",
            "export \"\" (an assignment this check cannot read)",
        ),
        (
            "export $X; ls",
            "export $X (an assignment this check cannot read)",
        ),
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(v.reason, reason, "{cmd:?}");
    }
    let allow = ["scripts/*standing*.py".to_string()];
    for (cmd, path) in [
        (
            "python3 scripts/$(echo x)_standing.py",
            "scripts/$(…)_standing.py",
        ),
        (
            "python3 scripts/${X}_standing.py",
            "scripts/${X}_standing.py",
        ),
    ] {
        let v = classify_command_with(cmd, &allow);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(
            v.reason,
            format!("python3 {path}: a script path this check cannot read"),
            "{cmd:?}"
        );
    }
    assert!(classify_command_with("python3 scripts/sat2026_standing.py", &allow).read_only);
    for (cmd, reason) in [
        (
            "sort $(echo -o /etc/hosts) /etc/hosts",
            "sort $(…) (a word this check cannot read may be a flag)",
        ),
        (
            "printf $(echo -v PATH) /tmp/evil; ls",
            "printf $(…) (a word this check cannot read may be a flag)",
        ),
        (
            "rg foo $(echo --pre=./x.sh)",
            "rg $(…) (a word this check cannot read may be a flag)",
        ),
        (
            "sort f $(echo -o /etc/hosts)",
            "sort $(…) (a word this check cannot read may be a flag)",
        ),
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(v.reason, reason, "{cmd:?}");
    }
    for cmd in [
        "echo $(date)",
        "echo \"now: $(date)\"",
        "ls `pwd`",
        "ls $(git rev-parse --show-toplevel)",
        "cat $(find . -name x.txt)",
        "wc -l $(git ls-files) | tail -1",
        "grep -n foo $(git ls-files '*.rs') | head",
        "cd $(git rev-parse --show-toplevel) && ls",
        "x=$(date); echo $x",
        "for f in $(ls); do wc -l $f; done",
        "echo $(echo $(echo $(date)))",
        "x=\"$(git rev-parse HEAD)\"; echo $x",
        "export X=\"$(date)\"; echo $X",
        "export FOO=1 BAR 2>/dev/null; export -p",
        "cat \"$(git rev-parse --show-toplevel)/README.md\"",
        "printf '%s\\n' $(git ls-files) | head",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
    for cmd in [
        "echo `echo \\`rm -rf /usr\\``",
        "echo `echo \"\\`rm -rf /usr\\`\"`",
        "echo `echo $(echo \\`rm -rf /usr\\`)`",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(
            v.reason, "a backtick substitution nested in another (\\`)",
            "{cmd:?}"
        );
    }
    assert!(ro("echo \\` `date`; echo \"a \\` b\""));
    let nest = |n: usize| format!("{}date{}", "echo $(".repeat(n), ")".repeat(n));
    assert!(ro(&nest(MAX_SUBSTITUTION_DEPTH)), "the bound is read");
    let v = classify_command(&nest(MAX_SUBSTITUTION_DEPTH + 1));
    assert!(!v.read_only);
    assert_eq!(
        v.reason,
        format!("command substitutions nested deeper than {MAX_SUBSTITUTION_DEPTH}")
    );
}

/// A redirect may precede a command's name or be glued to it by `&>`, and
/// zsh runs `- cmd` as `cmd`: `2>/dev/null touch M`, `touch&>/dev/null M`
/// and `- touch M` all run `touch`.
#[test]
fn a_redirect_or_a_dash_before_the_name_does_not_hide_it() {
    for (cmd, reason) in [
        ("- rm -rf /usr", "rm"),
        ("- touch /tmp/x", "touch"),
        ("rm&>/dev/null -rf /usr", "rm"),
        ("/bin/rm&>/dev/null -rf /usr", "rm"),
        ("2>/dev/null rm -rf /usr", "rm"),
        (">/dev/null rm -rf /usr", "rm"),
        ("</dev/null rm -rf /usr", "rm"),
        ("< /dev/null rm -rf /usr", "rm"),
        ("2> /dev/null rm -rf /usr", "rm"),
        ("2>&1 rm -rf /usr", "rm"),
        ("ls; 2>/dev/null touch /tmp/x", "touch"),
        ("if 2>/dev/null rm -rf /usr; then echo; fi", "rm"),
        ("timeout 5 2>/dev/null rm -rf /usr", "rm"),
        ("ls | xargs 2>/dev/null rm -rf", "rm"),
        (
            "2>/dev/null test -v 'a[$(touch M)]'",
            "test -v (evaluates arithmetic)",
        ),
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(v.reason, reason, "{cmd:?}");
    }
    for cmd in [
        "2>/dev/null ls -la",
        ">/dev/null 2>&1 git status",
        "</dev/null wc -l",
        "find . \\( -name a -o -name b \\) 2>/dev/null | head",
        "(ls) 2>/dev/null && echo ok",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// A reserved word is a keyword only where a command begins.
#[test]
fn a_reserved_word_behind_an_assignment_or_a_redirect_is_a_program() {
    for cmd in [
        "X=1 if true; then echo a; fi",
        "X=1 if true; echo b",
        "2>/dev/null if true; then echo a; fi",
        ">/dev/null if true; echo b",
        "X=1 for x in a; do echo; done",
        "X=1 while true; do echo; done",
        "X=1 [[ -n a ]]",
        "X=1 ! true",
        "X=1 { echo a; }",
        "- if true",
        "timeout 5 if true",
        "env if true",
        "env X=1 for x in a",
        "ls | xargs if true",
        "echo a; X=1 if true",
        "X=1 time if true",
        "if true; then X=1 if true; fi",
        "ls | time if true",
        "echo x | time if true",
        "ls | time -p if true",
        "ls |& time if true",
        "echo x | time ! true",
        "time if true; then echo a; fi",
        "time for x in a; do echo; done",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(
            v.reason.contains("where the shell reads a command's name"),
            "{cmd:?}: {v:?}"
        );
    }
    for cmd in [
        "if true; then echo a; fi",
        "for x in a b; do echo $x; done",
        "while test -e f; do echo; done",
        "! grep -q x f && echo y",
        "time ls",
        "ls | time ls",
        "ls | time -p ls",
        "if ! true; then echo; fi",
        "if true; then X=1 ls; fi",
        "if 2>/dev/null ls; then echo; fi",
        "X=1 time ls",
        "X=1 ls",
        "env FOO=1 ls",
        "ls | while test -e f; do echo; done",
    ] {
        assert!(
            ro(cmd),
            "{cmd:?} must stay read-only: {:?}",
            classify_command(cmd)
        );
    }
}

/// An option after `time` behind a pipe or an assignment is `/usr/bin/time`'s own, whose `-o
/// FILE` writes `FILE`.
#[test]
fn an_option_after_time_is_the_time_programs_own() {
    for cmd in [
        "ls | time -o out.txt ls",
        "X=1 time -o out.txt ls",
        "time -o out.txt ls",
        "ls | time -a -o out.txt ls",
        "ls | time -p -o out.txt ls",
        "ls | time -ao out.txt ls",
        "ls | time -l ls",
        "echo a | xargs time -o out.txt ls",
        "env time -o out.txt ls",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(v.reason.contains("time"), "{cmd:?}: {v:?}");
    }
    for cmd in [
        "time ls",
        "time -p ls",
        "ls | time ls",
        "ls | time -p ls",
        "X=1 time -p ls",
        "time -p git status",
    ] {
        assert!(
            ro(cmd),
            "{cmd:?} must stay read-only: {:?}",
            classify_command(cmd)
        );
    }
}

/// WHAT A WRAPPER OR `time` RUNS IS NEVER A KEYWORD, in [`runs`] as in the head check: `env if
/// git status` runs a program `if` (the word `env` execs), not `git`, so `runs` names the `if`.
#[test]
fn what_a_wrapper_or_time_runs_is_never_a_keyword() {
    let placed = |line: &str| -> String {
        let seg: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        match runs(&seg) {
            Ok(Some(r)) => seg[r.at].clone(),
            other => format!("{other:?}"),
        }
    };
    for (line, want) in [
        ("env if git status", "if"),
        ("env X=1 for git log", "for"),
        ("xargs if git status", "if"),
        ("timeout 5 if git status", "if"),
        ("X=1 if git status", "if"),
        ("time if git status", "if"),
        ("time -p while git status", "while"),
    ] {
        assert_eq!(placed(line), want, "{line:?}");
    }
    for (line, want) in [
        ("if git status", "git"),
        ("then git log", "git"),
        ("! git status", "git"),
        ("env git status", "git"),
        ("time git log", "git"),
        ("X=1 git status", "git"),
    ] {
        assert_eq!(placed(line), want, "{line:?}");
    }
}

/// AN ARITHMETIC EVALUATION IS REFUSED WHOLE ([`ARITHMETIC_RUNS`]): `$((…))` and `((…))` assign
/// any variable in their list, the second and later too, and evaluate a value's subscript.
#[test]
fn an_arithmetic_evaluation_is_refused_whole() {
    for cmd in [
        "echo $((x=1,PATH=0)); ls",
        "echo $((a=b=c=PATH=0)); ls",
        "echo $((x=1,HOME=0)); git diff",
        "cat f $((x=1,PATH=0))",
        "echo $((x=1?PATH=0:1)); ls",
        "echo \"$((x=1,PATH=0))\"; ls",
        "echo `echo $((x=1,PATH=0))`; ls",
        "echo $(echo $((x=1,PATH=0))); ls",
        "(( x=1,PATH=0 )); ls",
        "((x=1,PATH=0)); ls",
        "if (( 1 )); then ls; fi",
        "ls='a[$(touch M)]'; echo $((ls))",
        "echo $((ls) )",
        "((ls) )",
        "echo $((1+2))",
        "for ((i=0;i<1;i++)); do ls; done",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(
            v.reason.contains("an arithmetic evaluation"),
            "{cmd:?}: {v:?}"
        );
    }
    for cmd in [
        "echo $( (ls) )",
        "( (ls) )",
        "( ls )",
        "echo '$((x=1,PATH=0))'",
        "x=1,PATH=0; ls",
        "echo \"$(ls)\"",
    ] {
        assert!(
            ro(cmd),
            "{cmd:?} must stay read-only: {:?}",
            classify_command(cmd)
        );
    }
}

/// An awk program or sed script supplied by a parameter is refused like one supplied by a
/// substitution ([`program_words`] marks every expansion).
#[test]
fn an_awk_or_sed_program_from_a_parameter_is_not_read() {
    for (cmd, reason) in [
        (
            "awk \"$x\" f",
            "awk: a program this check cannot read ($(…))",
        ),
        (
            "awk \"${prog}\" f",
            "awk: a program this check cannot read ($(…))",
        ),
        (
            "awk $prog f",
            "awk: a program this check cannot read ($(…))",
        ),
        (
            "ls | xargs awk \"$AWK\"",
            "awk: a program this check cannot read ($(…))",
        ),
        (
            "sed \"$s\" f",
            "sed: a script this check cannot read ($(…))",
        ),
        (
            "sed -n \"${script}\" f",
            "sed: a script this check cannot read ($(…))",
        ),
        ("sed $s f", "sed: a script this check cannot read ($(…))"),
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(v.reason, reason, "{cmd:?}");
    }
    for cmd in [
        "awk '{print $1}' f",
        "awk -F: '$3 > 100 {print $1}' /etc/passwd",
        "sed 's/$x/y/' f",
        "sed -n '/$/p' f",
        "awk \"{print}\" f",
        "echo \"$x\"; awk '{print $2}' f",
        "sed \"s/ *$//\" f",
        "sed \"/^$/d\" f",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
    assert_eq!(
        program_words("cat \"$S/x\""),
        vec![vec!["cat".to_string(), format!("{SUBSTITUTION}/x")]]
    );
}

/// A double-quoted `"$(…)"` or `"$X"` cannot split, but its value can start
/// with `-`, so where a tool reads options it can be the write or run flag
/// (`sort "$X" f`, `rg x "$(echo --pre=./x.sh)"`).
#[test]
fn an_expansion_in_an_option_slot_is_refused_quoted_or_not() {
    for cmd in [
        "sort \"$(echo -o/tmp/x)\" f",
        "sort \"$X\" f",
        "sort $X f",
        "sort $(echo -o /etc/hosts) f",
        "rg foo \"$(echo --pre=./x.sh)\"",
        "rg pat \"$DIR\"",
        "rg pat $DIR",
        "uniq \"$(echo a.txt)\"",
        "find \"$(git rev-parse --show-toplevel)\" -name '*.rs' | head",
        "find \"$d\" -name x",
        "date -r \"$(stat -f %m f)\"",
        "sed -n '1,20p' \"$(echo f)\"",
        "file \"$(echo f)\"",
        "printf \"$fmt\" x",
        "[ $n -gt 2 ] && echo big",
        "test $n -gt 2",
        "[ $x ]",
        "test $(echo -v) 'a[$(touch M)]'",
        "[ \"$a\" = x -o \"$b\" = y ]",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(
            v.reason.contains("may be a flag"),
            "{cmd:?} is refused for its unreadable word: {v:?}"
        );
    }
    assert!(!ro("[ -v \"$x\" ]"));
    for cmd in [
        "[ \"$(grep -c ready log)\" -gt 5 ]",
        "test \"$(wc -l < f)\" -gt 0",
        "[ \"$n\" -eq 1 ] && echo one",
        "sort -- \"$f\"",
        "rg -e pat -- \"$dir\"",
        "[ -z \"$x\" ]",
        "[ -n \"$(git status --porcelain)\" ] && echo dirty",
        "[ \"$(uname)\" = Darwin ]",
        "[ ! -f \"$f\" ]",
        "test -d \"$dir\"",
        "[ \"$a\" != \"$b\" ]",
        "[ \"$x\" ]",
        "sed -n '$p' f",
        "sed -n '1,$p' f",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// zsh arithmetic runs substitutions in array subscripts, and evaluates
/// `printf` arguments under a numeric format, `-t`'s operand in `test`, `[`
/// and `[[`, `export -i`/`-E`/`-F` values and integer-variable assignments,
/// from quoted words, named variables, `"$A"` or globbed names.
#[test]
fn zsh_arithmetic_that_can_run_a_command_is_not_a_read() {
    for cmd in [
        "printf %d 'path[$(print -u2 X)]'",
        "printf -- %d 'path[$(print -u2 X)]'",
        "printf -%d 'path[$(print -u2 X)]'",
        "printf '%s %d\\n' x 'path[`print -u2 X`]'",
        "printf '%*s' 'path[$(print -u2 X)]' x",
        "printf '%d' \"path[\\$(print -u2 X)]\"",
        "A='path[$(print -u2 X)]'; printf %d A",
        "A='path[$(print -u2 X)]'; printf %d \"$A\"",
        "printf %d *",
        "printf '%d\\n' \"$(cat f)\"",
        "[ -t 'path[$(print -u2 X)]' ]",
        "test -t 'path[$(print -u2 X)]'",
        "test ! -t 'path[$(print -u2 X)]'",
        "A='path[$(print -u2 X)]'; [ -t \"$A\" ]",
        "A='path[$(print -u2 X)]'; [ -t A ]",
        "[[ -t 'path[$(print -u2 X)]' ]]",
        "A='path[$(print -u2 X)]'; [ {-t,A} ]",
        "[ * ]",
        "test -t S=[5]",
        "export -i N='path[$(print -u2 X)]'",
        "export -F N=1",
        "SECONDS='path[$(print -u2 X)]'",
        "A='path[$(print -u2 X)]'; COLUMNS=A ls",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(
            v.reason.contains("arithmetic") || v.reason.contains("into `-t`"),
            "{cmd:?}: {v:?}"
        );
    }
    for cmd in [
        "printf '%s\\n' 'path[$(print -u2 X)]'",
        "printf '%d files\\n' 3",
        "printf '%d\\n' 16#ff 0x1f 1e2 -3 ''",
        "printf '%s\\n' \"$A\"",
        "printf %d",
        "[ -t 1 ] && echo tty",
        "test -t 0",
        "[[ -t 2 ]]",
        "[ -t ]",
        "export N='path[$(print -u2 X)]'",
        "COLUMNS=200 ls",
        "[ -e /tmp/w/*.log ] && echo y",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
    let v = classify_except_rm(
        "printf %d 'path[$(print -u2 X)]'; rm -rf /private/tmp/claude-502/w/x",
        DEFAULT_PYTHON_ALLOW,
    );
    assert!(!v.read_only, "{v:?}");
}

/// bash brace-expands `printf {-vS,%s} /etc`, `printf -{vS,-} /etc` and
/// `printf -v{S,} /etc` into `printf -vS …`, as a glob `-v?` does beside a
/// file `-vS`; so an unquoted `{` or glob up to the format is refused.
#[test]
fn a_brace_or_glob_word_before_printfs_format_may_be_its_v_option() {
    for cmd in [
        "printf {-vS,%s} /etc",
        "printf -{vS,-} /etc",
        "printf -{vT,vS} /etc",
        "printf -v{S,} /etc",
        "printf -* /etc",
        "printf -v? /etc",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(
            v.reason.contains("may be a flag") || v.reason.starts_with("printf -v"),
            "{cmd:?}: {v:?}"
        );
    }
    for cmd in [
        "printf '{%s}\\n' x",
        "printf \"{-vS,%s}\" x",
        "printf '%s\\n' {a,b}",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// An unreadable git argument is refused anywhere before `--`: it may be
/// the verb (`remote "$(echo add)"`), change the arity (`config` get vs
/// set) or be a flag (`diff "$(echo --output=f)"`).
#[test]
fn a_git_argument_this_check_cannot_read_is_refused_before_the_double_dash() {
    for cmd in [
        "git config $(echo user.name value)",
        "git config user.name $(printf v)",
        "git config --get $(echo core.editor)",
        "git config --get \"$(echo core.editor)\"",
        "git config \"$(echo --edit)\"",
        "git remote $(echo add r url)",
        "git remote \"$(echo add)\" r url",
        "git branch --contains $(git rev-parse HEAD)",
        "git branch --contains \"$(git rev-parse HEAD)\"",
        "git branch $(echo -D old)",
        "git tag -l $(echo 'v*')",
        "git tag $(date +%s)",
        "git diff $(echo --output=f)",
        "git diff \"$(echo --output=f)\"",
        "git diff $(git merge-base HEAD origin/main)",
        "git log $(git rev-parse HEAD) -1",
        "git log \"$X\"..HEAD",
        "git show \"$(git rev-parse HEAD)\":Cargo.toml",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(
            v.reason.contains("may be a flag or a verb"),
            "{cmd:?} is refused for its unreadable word: {v:?}"
        );
    }
    for cmd in [
        "git log --oneline -- \"$f\"",
        "git diff --stat -- \"$(git ls-files | head -1)\"",
        "git config --get user.name; git config --list",
        "git remote -v; git remote show -n origin",
        "git log --oneline -5",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// A `for` loop assigning [`ZSH_ARITHMETIC_VARS`] evaluates each word as
/// arithmetic (`for SECONDS in 'path[$(…)]'`).
#[test]
fn a_for_loop_over_a_zsh_arithmetic_variable_is_not_a_read() {
    for cmd in [
        "for SECONDS in 'path[$(print -u2 X)]'; do echo; done",
        "A='path[$(print -u2 X)]'; for COLUMNS in A; do echo; done",
        "for x SECONDS in 1 'path[$(print -u2 X)]'; do echo; done",
        "for ERRNO in 'path[$(print -u2 X)]'; do echo; done",
        "for SECONDS in 'path[$(print -u2 X)]'; echo",
        "if true; then for LINES in A; do :; done; fi",
        "for SECONDS; do echo; done",
        "for PATH in /tmp/x; do ls; done",
        "[[ a =~ a ]]; for MEND in 'path[$(print -u2 X)]'; do :; done",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(v.reason.starts_with("for "), "{cmd:?}: {v:?}");
    }
    for cmd in [
        "[[ a =~ a ]]; MBEGIN='path[$(print -u2 X)]'",
        "ZLE_RPROMPT_INDENT='path[$(print -u2 X)]' ls",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(v.reason.contains("arithmetic"), "{cmd:?}: {v:?}");
    }
    for cmd in [
        "for SECONDS in 1 2; do echo; done",
        "for x in 'path[$(print -u2 X)]'; do echo; done",
        "for f in *.log; do echo \"$f\"; done",
        "for i in {1..3}; do echo $i; done",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// zsh's `~` flag makes a value a pattern and an unquoted `(` in a word is
/// a glob qualifier; `(e:…:)` and `(+cmd)` run commands during expansion
/// (`N='/(e:print -u2 X:)'; echo $~N`).
#[test]
fn a_glob_qualifier_that_can_run_a_command_is_not_a_read() {
    for cmd in [
        "N='/(e:print -u2 X:)'; echo $~N",
        "N='/(e:print -u2 X:)'; ls -d $~N",
        "N='/(e:print -u2 X:)'; echo $^~N",
        "N='/(e:print -u2 X:)'; echo $~^N",
        "N='/(e:print -u2 X:)'; echo $=~N",
        "N='/(e:print -u2 X:)'; echo x$~N",
        "N='/(e:print -u2 X:)'; [ -e $~N ]",
        "N='/(e:print -u2 X:)'; for f in $~N; do :; done",
        "N='/(e:print -u2 X:)'; echo ${~N}",
        "N='/(e:print -u2 X:)'; echo ${(~)N}",
        "N='/(e:print -u2 X:)'; echo ${${~N}}",
        "ls -d /(e:'print -u2 X':)",
        "echo \"/\"(e:'print -u2 X':)",
        "echo $(print /)(e:'print -u2 X':)",
        "X=/; echo $X(e:'print -u2 X':)",
        "echo {/,/}(e:'print -u2 X':)",
        "echo ~(e:'print -u2 X':)",
        "echo x=(e:'print -u2 X':)",
        "echo <->(e:'print -u2 X':)",
        "ls -d /(+print)",
        "ls *(e:ls:)",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for (cmd, why) in [
        ("N='/(e:print -u2 X:)'; echo $~N", GLOB_SUBST_RUNS),
        ("N=x; echo $^~N", GLOB_SUBST_RUNS),
        ("echo \"/\"(e:'print -u2 X':)", GLOB_QUALIFIER_RUNS),
        ("echo $(print /)(e:'print -u2 X':)", GLOB_QUALIFIER_RUNS),
    ] {
        assert_eq!(classify_command(cmd).reason, why, "{cmd:?}");
    }
    for cmd in [
        "(cd /tmp && ls)",
        "diff <(ls a) <(ls b)",
        "ls ~/x; echo ~",
        "find . \\( -name a -o -name b \\)",
        "echo \"$HOME\" $HOME",
        "echo '(e:x:)' \"(e:x:)\"",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// Under `EXTENDED_GLOB` `^`, `#` and `~` glob too: beside files `-t`,
/// `-vS` and `S=5`, `[ ^-vS ]` is `[ -t S=5 ]`, and `printf %d 8#A` beside a
/// file `A` reads `A` as arithmetic.
#[test]
fn an_extended_glob_word_may_become_a_flag_or_a_name() {
    for cmd in [
        "[ ^-vS ]",
        "test ^x",
        "[ x~y ]",
        "[ 1#-t S=5 ]",
        "printf ^-vS /etc",
        "printf %d 8#A",
        "A='path[$(print -u2 X)]'; printf %d 1#A",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "[ -d ~/x ] && echo y",
        "[ -e /tmp/w/^x ] && echo y",
        "printf '%d\\n' 16#ff 2#101",
        "printf '%s\\n' ^x",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// `git remote show <name>` contacts the remote in every spelling; with no name it lists the
/// remotes.
#[test]
fn git_remote_show_contacts_the_remote_only_when_it_names_one() {
    for cmd in [
        "git remote show origin",
        "git remote -v show origin",
        "git remote --verbose show origin",
        "git -C repo remote show upstream",
    ] {
        assert_eq!(
            classify_command(cmd).reason,
            "git remote show (contacts a remote)",
            "{cmd:?}"
        );
    }
    for cmd in [
        "git remote show",
        "git remote -v show",
        "git remote show -n origin",
        "git remote -v",
    ] {
        assert!(
            ro(cmd),
            "{cmd:?} contacts nothing but was refused: {:?}",
            classify_command(cmd)
        );
    }
}

/// Sed options that can write, git cat-file driver abbreviations, and shell hazard variables are
/// refused.
#[test]
fn the_v6_spellings_of_a_write_or_a_hazard_are_each_refused() {
    for cmd in [
        "sed -ew\\ out input.txt",
        "sed --expr='w out' input.txt",
        "sed --fi=script.sed input.txt",
        "sed --in s/a/b/ input.txt",
        "git cat-file --t HEAD:f",
        "git cat-file --fil HEAD:f",
        "PYTHONHOME=/tmp ls",
        "FPATH=/tmp/x ls",
    ] {
        assert!(
            !ro(cmd),
            "{cmd:?} writes, runs or sets a hazard but was read-only"
        );
    }
    for cmd in [
        "sed -es/a/b/ input.txt",
        "sed --expr='s/a/b/' input.txt",
        "git cat-file -t HEAD:f",
        "PYTHONPATHS=/tmp ls",
    ] {
        assert!(
            ro(cmd),
            "{cmd:?} is a read but was refused: {:?}",
            classify_command(cmd)
        );
    }
}

/// zsh ties arrays to scalars, so `path=DIR ls`, `for path in DIR; do ls`
/// run `DIR/ls` and `module_path=DIR; echo $terminfo` loads from `DIR`.
#[test]
fn a_tied_array_is_assigned_as_its_scalar() {
    for cmd in [
        "path=/tmp/x ls",
        "path=/tmp/x; ls",
        "for path in /tmp/x; do ls; done",
        "for x path in 1 /tmp/x; do ls; done",
        "path=/bin:/tmp/x ls",
        "module_path=/tmp/x; echo $terminfo",
        "for module_path in /tmp/x; do echo $terminfo; done",
        "cdpath=/tmp/x; cd x",
        "CDPATH=/tmp/x cd x",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    assert_eq!(
        classify_command("path=/tmp/x ls").reason,
        "path= (changes which program a name runs)"
    );
    for cmd in [
        "path=/bin ls",
        "path=/bin:/usr/bin ls",
        "for p in /tmp/x; do ls; done",
        "paths=/tmp/x; ls",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// Parameters whose value zsh runs or loads during reads
/// ([`ZSH_RUNS_A_VALUE`]) and `STTY`: `READNULLCMD=DIR/x; < f` and
/// `NULLCMD=DIR/x; > /dev/null` run `DIR/x`.
#[test]
fn a_parameter_zsh_runs_is_a_hazard() {
    for cmd in [
        "READNULLCMD=/tmp/x; < f",
        "NULLCMD=/tmp/x; > /dev/null",
        "READNULLCMD=/tmp/x < f",
        "for NULLCMD in /tmp/x; do > /dev/null; done",
        "MODULE_PATH=/tmp/x; echo $terminfo",
        "STTY='; cmd' ls",
        "export READNULLCMD=/tmp/x; < f",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for name in ZSH_RUNS_A_VALUE {
        assert!(
            assignment_hazard(&format!("{name}=/tmp/x")).is_some(),
            "{name}"
        );
    }
    for cmd in [
        "REPORTTIME=A; cat f | cat; rm -rf /tmp/w/x",
        "DIRSTACKSIZE=A; cd .; rm -rf /tmp/w/x",
        "PAGER=/tmp/x; git log; rm -rf /tmp/w/x",
        "READNULLCMD=/tmp/x; < f; rm -rf /tmp/w/x",
        "path=/tmp/x; ls; rm -rf /tmp/w/x",
        "PATH=/tmp/x rm -rf /tmp/w/x",
        "PATH=/tmp/x mktemp -d",
    ] {
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in ["cat f", "cat > /dev/null", "echo $terminfo", "ls"] {
        assert!(classify_command(cmd).read_only, "{cmd:?}");
    }
    for cmd in [
        "S=/tmp/w; rm -rf \"$S/x\"",
        "D=$(mktemp -d) && rm -rf \"$D\"",
        "REPORTTIME=5; S=/tmp/w; rm -rf \"$S/x\"",
    ] {
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// Inside a `${…}` operand a `$` form expands as it does bare, so the bare
/// refusals apply there (`${x:-$path[A]}`, `${x:-$~N}`), as do subscripts
/// behind zsh's flags (`$=A[A]`).
#[test]
fn a_dollar_form_inside_a_braced_operand_is_refused_as_it_is_bare() {
    for cmd in [
        "A='path[$(cmd)]'; echo ${x:-$path[A]}",
        "A='path[$(cmd)]'; echo \"${x:-$path[A]}\"",
        "A='path[$(cmd)]'; echo ${x:-$[A]}",
        "A='path[$(cmd)]'; echo \"${x:-$[A]}\"",
        "A='path[$(cmd)]'; echo ${x:-$A[A]}",
        "A='path[$(cmd)]'; echo ${x-$A[A]}",
        "A='path[$(cmd)]'; echo ${x:=$[A]}",
        "A='path[$(cmd)]'; echo ${x#$A[A]}",
        "A='path[$(cmd)]'; echo ${x/$A[A]/y}",
        "A='path[$(cmd)]'; echo ${x:-a$A[A]}",
        "A='path[$(cmd)]'; echo ${x:-$=A[A]}",
        "A='path[$(cmd)]'; echo \"${x:-$#A[A]}\"",
        "N='/(e:cmd:)'; echo ${x:-$~N}",
        "N='/(e:cmd:)'; echo \"${x:-$~N}\"",
        "N='/(e:cmd:)'; echo ${x:-$^~N}",
        "N='/(e:cmd:)'; echo ${x:-$=~N}",
        "A='path[$(cmd)]'; echo $=A[A]",
        "A='path[$(cmd)]'; echo $^A[A]",
        "A='path[$(cmd)]'; echo $#A[A]",
        "A='path[$(cmd)]'; echo \"$+A[A]\"",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "echo ${x:-$HOME} ${x:-$1} \"${x:-$HOME/y}\" ${x:-$#} ${x:-$=y}",
        "echo ${x:-$} ${x:-a$} \"${HOME:-$PWD}\"",
        "echo ${x:-$y} \"${x:+$y}\" $#x $=x",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// zsh subscripts special parameters as it does names (`$*[A]`, `$@[A]`,
/// `$#[A]`… with `A='path[$(cmd)]'`), bare, in `${…}` operands and in
/// unquoted here-document bodies.
#[test]
fn a_subscript_on_a_special_parameter_is_refused() {
    for sub in [
        "$*[A]",
        "$@[A]",
        "\"$@[A]\"",
        "$-[A]",
        "$?[A]",
        "$![A]",
        "$#[A]",
        "$$[A]",
        "$=*[A]",
        "$#*[A]",
        "$^*[A]",
        "$##[A]",
        "$1[A]",
        "${x:-$*[A]}",
        "\"${x:-$@[A]}\"",
        "${x:-$-[A]}",
        "${x:-$#[A]}",
        "${x-$?[A]}",
    ] {
        let cmd = format!("A='path[$(cmd)]'; echo {sub}");
        let v = classify_command(&cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        let body = format!("cat <<EOF\n{}\nEOF", sub.trim_matches('"'));
        let v = classify_command(&body);
        assert!(!v.read_only, "{body:?} must not be read-only: {v:?}");
    }
    let v = classify_except_rm(
        "A=S=0; S=/tmp/w; echo \"$#[A]\"; rm -rf \"$S/x\"",
        &[] as &[&str],
    );
    assert!(!v.read_only, "{v:?}");
    for cmd in [
        "echo $# $? \"$@\" $* $- $! $$ \"$#\" ${x:-$#} ${x:-$?}",
        "echo $x-[ab] $x*[ab] \"$1\"[x] ${#}",
        "ls $HOME/*[0-9]",
        "cat <<EOF\n$# $? $@ $x-[ab]\nEOF",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// `${NAME:=v}` and `${NAME=v}` assign like `NAME=v`: `${REPORTTIME:=A}`
/// before a pipe, `${DIRSTACKSIZE:=A}` before a `cd`, `${ERRNO:=A}` alone
/// run a subscript, and `${NULLCMD:=DIR/x}` acts like `NULLCMD=DIR/x`; also
/// in here-document bodies.
#[test]
fn a_default_assignment_is_judged_as_an_assignment() {
    for cmd in [
        "A='path[$(cmd)]'; echo ${REPORTTIME:=A}; cat f | cat",
        "A='path[$(cmd)]'; echo \"${REPORTTIME:=A}\"; cat f | cat",
        "A='path[$(cmd)]'; echo ${REPORTTIME=A}; cat f | cat",
        "A='path[$(cmd)]'; echo ${REPORTMEMORY:=A}; cat f | cat",
        "A='path[$(cmd)]'; echo ${DIRSTACKSIZE:=A}; cd .",
        "A='path[$(cmd)]'; echo ${ZLE_RPROMPT_INDENT:=A}",
        "A='path[$(cmd)]'; echo ${ERRNO:=A}",
        "A='path[$(cmd)]'; echo ${#ERRNO:=A}",
        "A='path[$(cmd)]'; echo ${ERRNO[1]=A}",
        "echo ${READNULLCMD:=/tmp/x}; < f",
        "echo ${NULLCMD=/tmp/x}; > /dev/null",
        "echo ${PATH:=/tmp/x}; ls",
        "echo ${path=/tmp/x}; ls",
        "echo ${PAGER:=$x}; git log",
        "A='path[$(cmd)]'; cat <<EOF\n${REPORTTIME:=A}\nEOF\ncat f | cat",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "echo ${x:=1} ${y=a} \"${z:=$HOME}\"",
        "echo ${REPORTTIME:=5} ${COLUMNS:=80} ${PATH:=/usr/bin:/bin}",
        "echo ${REPORTTIME:-A} ${REPORTTIME:+A} ${REPORTTIME:?A}",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// [`classify_except_rm`] still judges redirects on the skipped `rm` and
/// `mktemp` segments: `mktemp > ~/.zshrc; rm -rf /tmp/w/x` (and `>>`, `2>`,
/// `&>`, `1<>`, quoted targets) truncates a file.
#[test]
fn a_redirect_on_a_left_out_segment_is_judged() {
    for cmd in [
        "mktemp > /Users/_owner/.zshrc; rm -rf /tmp/w/x",
        "mktemp >> /Users/_owner/.zshrc; rm -rf /tmp/w/x",
        "mktemp 2> /Users/_owner/.zshrc; rm -rf /tmp/w/x",
        "mktemp &> /Users/_owner/.zshrc; rm -rf /tmp/w/x",
        "mktemp 3>/Users/_owner/.zshrc; rm -rf /tmp/w/x",
        "mktemp -d 1<>/Users/_owner/f; rm -rf /tmp/w/x",
        "mktemp >\"$HOME/.zshrc\"; rm -rf /tmp/w/x",
        "mktemp -d > ~/.zshrc && rm -rf /tmp/w/x",
        "rm -rf /tmp/w/x 2>/dev/null; mktemp > .git/HEAD",
        "rm -rf /tmp/w/x > /Users/_owner/f",
    ] {
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(v.reason.starts_with("redirect "), "{cmd:?}: {v:?}");
    }
    for cmd in [
        "mktemp -d > /dev/null; rm -rf /tmp/w/x",
        "mktemp -d >&2; rm -rf /tmp/w/x",
        "mktemp -d 2>&1; rm -rf /tmp/w/x",
        "mktemp -d < /dev/null; rm -rf /tmp/w/x",
        "rm -rf /tmp/w/x 2>/dev/null",
        "D=$(mktemp -d) && rm -rf \"$D\"",
    ] {
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// zsh's `printf` decodes Unicode escapes in its format first, so `\u0025d` is `%d` and runs a
/// subscript; [`numeric_format`] (shared with the rm rule) must see it.
#[test]
fn a_unicode_escape_in_a_printf_format_may_be_a_conversion() {
    for format in [
        "BSu0025d",
        "BSU00000025d",
        "%sBSu0025",
        "%BSu0064",
        "BSu0025BSu0064",
        "BSu0025*s",
        "%BSu002ad",
        "BSu005c%d",
        "BSu",
        "BSuzz%s",
        "BSUffffffff%s",
    ] {
        let format = format.replace("BS", "\\");
        assert!(numeric_format(&format), "{format:?}");
    }
    for format in [
        "%s\\n",
        "BS045d",
        "BSx25d",
        "%sBSn",
        "BSe%s",
        "BSu0025s",
        "BSu0025.1s",
        "%BSu0073",
        "BSu2713 %sBSn",
        "BSU0001F600 %s",
        "BSu25d",
        "BSU25d",
        "BSu00025d",
        "BSu005cu0025d",
        "BSBSu0025d",
    ] {
        let format = format.replace("BS", "\\");
        assert!(!numeric_format(&format), "{format:?}");
    }
    for cmd in [
        "printf 'BSu0025d' 'path[$(cmd)]'",
        "A='path[$(cmd)]'; printf 'BSu0025d' A",
        "printf 'BSU00000025d' 'path[$(cmd)]'",
        "printf \"BSu0025d\" 'path[$(cmd)]'",
    ] {
        let cmd = cmd.replace("BS", "\\");
        let v = classify_command(&cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "printf 'BS045d' 'path[$(cmd)]'",
        "printf '%sBSn' \"$x\"",
        "printf 'BSu2713 %sBSn' done",
    ] {
        let cmd = cmd.replace("BS", "\\");
        let v = classify_command(&cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// macOS sed edits in place under `-I` as under `-i`, alone or clustered.
#[test]
fn sed_capital_i_edits_in_place() {
    for cmd in [
        "sed -I '' s/a/b/ f",
        "sed -I.bak s/a/b/ f",
        "sed -nI '' p f",
        "sed -E -I '' s/a/b/ f",
        "/usr/bin/sed -I '' s/a/b/ f",
        "env sed -I '' s/a/b/ f",
        "git ls-files | xargs sed -I '' s/a/b/",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert_eq!(v.reason, "sed -i", "{cmd:?}");
    }
    let v = classify_except_rm("sed -I '' s/a/b/ f; rm -rf /tmp/a/x", &[] as &[&str]);
    assert!(!v.read_only, "{v:?}");
    for cmd in ["sed -n p f", "sed -E 's/a/b/I' f", "sed -n '/x/Ip' f"] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
    let v = classify_except_rm("sed -n p f; rm -rf /tmp/a/x", &[] as &[&str]);
    assert!(v.read_only, "{v:?}");
}

/// bash's `${!NAME}` expands the variable named by NAME's value, evaluating
/// its subscript (`A='a[$(…)]'; echo "${!A}"`); refused, in here-documents
/// too.
#[test]
fn a_bash_indirection_is_not_literal() {
    for cmd in [
        "A='a[$(echo X >&2)]'; echo \"${!A}\"",
        "A='a[$(echo X >&2)]'; echo ${!A:-x}",
        "A='a[$(echo X >&2)]'; echo \"${!A#x}\"",
        "A='a[$(echo X >&2)]'; echo ${!A[0]}",
        "A='a[$(echo X >&2)]'; [ -n \"${!A}\" ]",
        "A='a[$(echo X >&2)]'; ls \"${!A}\"",
        "echo \"${!1}\"",
        "echo ${!A=v}",
        "A='a[$(echo X >&2)]'; cat <<EOF\n${!A}\nEOF",
        "echo \"${A@P}\"",
        "echo ${A[0]@Q}",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    let v = classify_except_rm("echo \"${!A}\"; rm -rf /tmp/a/x", &[] as &[&str]);
    assert!(!v.read_only, "{v:?}");
    for cmd in [
        "echo ${!A[@]} \"${!A[*]}\" ${!A@} ${!A*}",
        "echo ${!} ${#} ${#A} \"${A:-x}\" ${A#x}",
        "echo {a,b}",
        "printf '%s\\n' \"$x\"",
        "[ -n \"$x\" ]",
        "cat <<EOF\n${!A[@]} ${HOME}\nEOF",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// zsh runs a command written after a closing `}` or `]]` in the same list (`{ ls } always {
/// ./p/x }`, `if [[ -n a ]] ./p/x`), which a word reader would take as an argument.
#[test]
fn a_word_after_a_closing_brace_is_a_command() {
    for cmd in [
        "{ ls } always { ./p/x }",
        "{ ls } always { ./p/x } && ls",
        "{ echo } always { ./p/x }",
        "{ ls } always { touch x }",
        "if ls; then ls; fi; { ls } always { touch ~/.zshrc }",
        "if [[ -n a ]] ./p/x",
        "while [[ -z $d ]] ./p/x",
        "if { ls } ./p/x",
        "if [[ -z a ]] { ls } else { ./p/x }",
        "if [[ -n a ]] then ./p/x; fi",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(
            !v.read_only,
            "{cmd:?} must not be read-only beside an rm: {v:?}"
        );
    }
    for cmd in [
        "{ ls }",
        "{ { ls } }",
        "[[ -n $x ]] && ls",
        "if [[ -n $x ]]; then ls; fi",
        "echo '}' x",
        "awk '{print $1}' f",
        "find . -name x -exec ls {} +",
        "echo {a,b}",
        "ls *.log",
        "grep -n foo *.rs",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// Fail-safe regressions of the tests above: a redirect after a closing word
/// is the compound's own (`{ ls; echo } 2>&1 | head`) and its target is still
/// judged; a `}` or `]]` with no opener closes nothing; `${!#}` is the last
/// positional, not an indirection.
#[test]
fn a_closing_word_redirect_and_an_unopened_close_stay_reads() {
    for cmd in [
        "{ ls } 2>/dev/null",
        "{ ls; echo } 2>&1 | head",
        "{ ls } >/dev/null 2>&1",
        "{ ls } &>/dev/null",
        "[[ -f x ]] 2>/dev/null",
        "[[ -f x ]] 2>/dev/null && cat x",
        "if [[ -f x ]] 2>/dev/null; then cat x; fi",
        "echo } x",
        "echo ]] x",
        "echo \"${!#}\"",
        "cat <<EOF\n${!#}\nEOF",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must be read-only: {v:?}");
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(v.read_only, "{cmd:?} must be read-only beside an rm: {v:?}");
    }
    for cmd in [
        "{ ls } 2>/dev/null ./p/x",
        "{ ls; } 2>&1 ./p/x",
        "{ ls } < /dev/null ./p/x",
        "[[ -f x ]] 2>/dev/null ./p/x",
        "{ ls } > out",
        "{ echo } ./p/x }",
        "echo } ./p/x; { ls; }",
        "if [[ -n a ]] ./p/x",
        "echo \"${!1}\"",
        "echo \"${!A[@]:-x}\" ${!A[*]:-x}",
        "echo \"${!#[0]}\"",
        "echo ${!#:-x}",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(
            !v.read_only,
            "{cmd:?} must not be read-only beside an rm: {v:?}"
        );
    }
}

/// zsh closes a `{` group at a `}` glued to a word's end, unless the word's
/// own braces balance.
#[test]
fn a_brace_glued_to_a_word_closes_the_group() {
    for cmd in [
        "{ ls a} always { ./p/x}",
        "{ ls a} always { ./p/x }",
        "if { ls a} ./p/x",
        "while { ls a} ./p/x",
        "{ ls ${HOME}} always { ./p/x }",
        "{ ls x{a,b}} always { ./p/x }",
        "{ ls \"a\"} always { ./p/x }",
        "{ ls 'a'} always { ./p/x }",
        "{ ls a}} always { ./p/x }",
        "{ ls a} 2>/dev/null ./p/x",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(
            !v.read_only,
            "{cmd:?} must not be read-only beside an rm: {v:?}"
        );
    }
    for cmd in [
        "{ ls a}",
        "{ ls a} 2>/dev/null",
        "{ ls a}; ls b",
        "{ echo {a,b} x }",
        "{ echo ${HOME} x }",
        "{ find . -exec ls {} + }",
        "{ echo a\\} x }",
        "echo a} x",
        "find . -name x -exec ls {} +",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// The reads a worker writes are plain; every form known to run a command under zsh is not, by
/// the character that opens it rather than by a rule of its own, and neither is any expansion
/// but a plain parameter read or command substitution
/// ([`a_plain_line_admits_only_the_expansions_zsh_takes_no_further_than_bash`]), so a form
/// nobody has measured needs one of these characters too.
#[test]
fn a_plain_line_expands_nothing() {
    for line in [
        "ls",
        "ls -la ~/x ~ /tmp",
        "cat < f.txt | head -3",
        "git log --format='%h $x `y` \\z' -3",
        "x 2>/dev/null; y &>/dev/null && z 2>&1 | head || w >&2",
        "x > /dev/null",
        "2>/dev/null ls; </dev/null ls for; < f cat",
        "# $(rm -rf /) is a comment\nls",
        "ls 'a#b' \"a^b\"",
        "find . -name '*.rs' -type f",
        "sort -t= -k2 f --key=1",
        "grep -rn \"TODO: fix {x}!\" src",
        "echo 'it''s' \"a\"'b'",
        "wc -l café.txt [ab]*.md",
        "grep -n for src | grep if; echo 'while' done",
        "ls \"$HOME\"",
        "echo ${HOME}",
    ] {
        for extended_glob in [false, true] {
            assert_eq!(plain_zsh_line(line, extended_glob), Ok(()), "{line:?}");
        }
    }
    for line in [
        "path=/x ls",
        "path=/x; ls",
        "for path in /x; do ls; done",
        "ls; select path in /x; do ls; done",
        "repeat 3 ls",
        "2>/dev/null for path in /x; ls",
        "2>&1 repeat 2 ls",
        "</dev/null for path in /x; ls",
        "< f export -u PATH; ls",
        "ls && ! ls",
        "if ls; then ls; fi",
        "time ls",
        "export PATH",
        "ls $=S",
        "echo \"$S:h\"",
        "diff =(ls) <(ls)",
        "ls *(e:'id':)",
        "=ls",
        "ls ~[x]",
        "ls ~root",
        "echo `id`",
        "ls $'\\x41'",
        "echo \"\\$x\"",
        "\\ls",
        "ls \\\nx",
        "wc -l src/{a,b}.rs",
        "(ls)",
        "{ ls }",
        "[[ -t 1 ]] && ls",
        "a=1 ls",
        "ls x=y",
        "ls > out",
        "ls >> /dev/null",
        "ls >| /dev/null",
        "ls 2>/dev/nullx",
        "ls >&-",
        "cat <<EOF\nx\nEOF",
        "cat <<< x",
        "cat <> f",
        "ls a<1-5>",
        "ls \u{1b}[2J",
        "ls 'open",
        "ls \"open",
    ] {
        for extended_glob in [false, true] {
            assert!(
                plain_zsh_line(line, extended_glob).is_err(),
                "{line:?} must not be plain"
            );
        }
    }
}

/// An unquoted `^`, and an unquoted `#` that opens no comment, are `EXTENDED_GLOB`'s operators
/// (`^x` any name but `x`, `x#` any run of `x`): a line holding one, a substitution's included,
/// is plain only where the startup cannot set that option, where zsh reads both as text
/// (MEASURED 2026-11-05, `docs/measured/classifier-zsh-glob-2026-11-05.md`).
#[test]
fn an_extended_glob_operator_is_plain_only_with_the_option_off() {
    for line in [
        "ls a#b",
        "ls ^x",
        "git show HEAD^",
        "git diff HEAD^^ HEAD",
        "grep -n ^fn src",
        "cat ~/.s#h/config",
        "echo $(ls a#b)",
        "echo \"$(git show HEAD^)\"",
    ] {
        assert_eq!(plain_zsh_line(line, false), Ok(()), "{line:?}");
        let why = plain_zsh_line(line, true).expect_err(line);
        assert!(
            why.contains("a glob operator under EXTENDED_GLOB"),
            "{line:?}: {why}"
        );
    }
    // NEGATIVE CONTROL: quoted, or a comment's.
    for line in ["ls 'a#b' \"a^b\"", "ls # x^y a#b", "grep -n '^fn' src"] {
        for extended_glob in [false, true] {
            assert_eq!(plain_zsh_line(line, extended_glob), Ok(()), "{line:?}");
        }
    }
}

/// A plain line admits only the expansions zsh takes no further than bash.
#[test]
fn a_plain_line_admits_only_the_expansions_zsh_takes_no_further_than_bash() {
    for line in [
        "echo $x",
        "echo ${x}",
        "ls \"$HOME\"/x ${HOME}/y",
        "echo $x-y $x.y $x/y $a$b $_x $x,",
        "echo \"$(ls)\" $(git status --short)",
        "echo $(echo $(ls))",
        "echo \"a $(echo \")\") b\"",
        "echo $(echo 'a)b')",
        "cat $(echo a) < $(echo f)",
        "echo $(<f) $()",
        "printf %d \"path[$(id)]\"",
        "echo $(echo $(echo $(echo $(echo $(echo $(echo $(echo $(echo a))))))))",
    ] {
        for extended_glob in [false, true] {
            assert_eq!(plain_zsh_line(line, extended_glob), Ok(()), "{line:?}");
        }
    }
    assert!(
        plain_zsh_line("echo $((x=1,PATH=0))", false).is_err_and(|e| e.contains("arithmetic")),
        "refused as the arithmetic it is"
    );
    let v = classify_command("printf %d \"path[$(id)]\"");
    assert!(!v.read_only, "{v:?}");
    let v = classify_command("echo \"$(ls)\" $x ${HOME}");
    assert!(v.read_only, "{v:?}");
    for line in [
        "echo ${(e)x}",
        "echo ${~x}",
        "echo ${=x}",
        "echo ${^x}",
        "echo $x[PATH=0]; ls",
        "echo \"$x[PATH=0]\"; ls",
        "echo ${x[PATH=0]}; ls",
        "echo ${x}[1]",
        "echo $x:h",
        "echo \"$x:h\"",
        "echo ${x:h}",
        "echo ${x:=v}",
        "echo ${#x}",
        "echo $=x $~x $^x",
        "echo $~x",
        "echo $^x",
        "echo $((x=1,PATH=0)); ls",
        "echo \"$(( 1 ))\"",
        "echo $1",
        "echo $#",
        "echo $$",
        "echo $?",
        "echo \"$@\"",
        "echo $*",
        "echo $-",
        "echo $!",
        "echo $'a'",
        "echo $\"a\"",
        "echo $",
        "echo \"$\"",
        "echo $x(N)",
        "echo $x{a,b}",
        "echo $x\u{e9}",
        "echo ${x}\u{e9}",
        "echo $(echo a # )\n)",
        "echo $(echo a # ) ')'",
        "echo $(echo a\nls)",
        "echo $(ls",
        "echo $(echo `id`)",
        "echo $( (ls) )",
        "echo $(for x in a; do ls; done)",
        "echo $(path=/x ls)",
        "echo $(echo $((1)))",
        "echo \"$(echo \"$((1))\")\"",
        "echo $(echo \\x)",
        "echo $(echo $(echo $(echo $(echo $(echo $(echo $(echo $(echo $(echo a)))))))))",
        "echo $(echo a)[1]",
        "echo $(echo /a/b):h",
    ] {
        for extended_glob in [false, true] {
            assert!(
                plain_zsh_line(line, extended_glob).is_err(),
                "{line:?} must not be plain"
            );
        }
    }
}

/// A write-or-run flag the classifier keys on is still caught when a single
/// quote, a double quote or a backslash hides it from the quote-stripped scans.
#[test]
fn a_quoted_or_escaped_write_flag_is_still_a_write() {
    for cmd in [
        "find . '-delete'",
        "find . \"-delete\"",
        "find . \\-delete",
        "find . '-exec' rm {} ';'",
        "find . '-execdir' sh {} ';'",
        "find . '-ok' rm {} ';'",
        "find . '-fprint' f",
        "find . '-fls' f",
        "sed '-i' s/a/b/ f",
        "sed \"-i\" s/a/b/ f",
        "sed '-I' s/a/b/ f",
        "sed -n '-i' s/a/b/ f",
        "sort '-o' out in",
        "sort '--output=out' in",
        "sort '--compress-program=./cp.sh' -S 1M big",
        "tree '-o' out.html",
        "less '+!id' f",
        "less '-o' log f",
        "less '-k' key f",
        "more '+1' f",
        "file '-C' -m m",
        "rg '--pre' sh x",
        "rg '--pre=sh' x",
        "printf '-v' X %s y",
        "git log '--output=x'",
        "git log '--ext-diff'",
        "git diff '--textconv'",
        "printf '-vx[1]' hi",
        "ls | sort '-o' out",
        "cd d && find . '-delete'",
        "echo \"$(find . '-delete')\"",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
}

/// A quoted operand that is no flag token stays a read.
#[test]
fn quoting_a_data_word_that_is_not_a_flag_stays_a_read() {
    for cmd in [
        "cat 'my file'",
        "git -C 'my dir' status",
        "git log --format='%h <%ae> -> %s' | head",
        "grep -n 'rm -rf' notes.md",
        "echo \"git push later\"",
        "echo \">\"",
        "echo '-delete is a word'",
        "sort in1 in2",
        "less file.txt",
        "find . -name '*.rs'",
        "printf '%s\\n' hello",
        "git log --format='%H'",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// [`classify_except_rm`] runs the argv-word flag scan on its non-rm segments, so a quoted or
/// escaped write flag there is still a write.
#[test]
fn a_quoted_flag_on_the_rest_of_an_rm_line_is_still_a_write() {
    for cmd in [
        "find /tmp/w '-delete'; rm -rf /tmp/w/x",
        "sort '-o' /tmp/w/out /tmp/w/in; rm -rf /tmp/w/x",
        "sed '-i' s/a/b/ /tmp/w/f && rm -rf /tmp/w/x",
        "rm -rf /tmp/w/x; find /tmp/w '-exec' rm {} ';'",
        "rg '--pre' sh x; rm -rf /tmp/w/x",
        "less '+!id' f; rm -rf /tmp/w/x",
        "file '-C' x; rm -rf /tmp/w/x",
        "git remote 'add' x /p; rm -rf /tmp/w/x",
        "git config '-e'; rm -rf /tmp/w/x",
    ] {
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "find /tmp/w -type f; rm -rf /tmp/w/x",
        "sort /tmp/w/in; rm -rf /tmp/w/x",
        "sed -n p /tmp/w/f && rm -rf /tmp/w/x",
        "rg x /tmp/w; rm -rf /tmp/w/x",
        "git remote -v; rm -rf /tmp/w/x",
        "rm -f -- '-delete'",
    ] {
        let v = classify_except_rm(cmd, &[] as &[&str]);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// A program `find -exec` runs is judged by its own write flags too.
#[test]
fn a_find_exec_programs_write_flag_is_a_write() {
    for cmd in [
        "find . -exec file -C {} ;",
        "find . -exec file --compile {} \\;",
        "find . -execdir file -C {} +",
        "find . -ok /usr/bin/file -C -m x {} ;",
        "find . -exec file '-C' {} ';'",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
        assert!(v.reason.contains("file"), "{cmd:?}: {v:?}");
    }
    for cmd in [
        "find . -exec file {} ;",
        "find . -type f -exec file -b {} \\; -print",
        "find . -exec grep -l foo {} +",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// A git read's write form is read on the argv words too, and a remote's verb is its first
/// operand.
#[test]
fn a_quoted_git_verb_or_flag_is_still_a_write() {
    for cmd in [
        "git remote 'add' x /p",
        "git remote \"rename\" a b",
        "git -C 'my dir' remote 'add' x /p",
        "git remote -v add x /p",
        "git remote --verbose add x /p",
        "git config '--edit'",
        "git config '-e'",
        "git branch -r '-D' origin/x",
        "git branch -a '-m' a b",
        "git branch -a '-c' a c",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "git remote -v",
        "git remote show -n origin",
        "git -C 'my dir' remote -v",
        "git branch -a",
        "git branch --list 'feat*'",
        "git config --get 'user.name'",
        "git config 'user.name'",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// A word the shell may brace-expand or glob into a flag, a verb or another operand is refused
/// where a flag selects a write.
#[test]
fn a_brace_or_glob_word_that_may_become_a_write_flag_is_refused() {
    for cmd in [
        "find d -{delete,true}",
        "find d -del*",
        "find d '-'del*",
        "find . -{exec,true} rm {} ;",
        "sort *.txt",
        "sed -n 1p *.txt",
        "uniq *.txt",
        "uniq a{,2}.txt",
        "file *",
        "sysctl k*",
        "find -- d $X",
        "uniq -- $X",
        "sysctl -- \"$X\"",
        "git remote {add,o} /p",
        "git remote ad? x /p",
        "git log --{output,stat}=x",
        "git config user.{name,email}",
        "git config -- $X",
        "git -C {a,b} status",
    ] {
        let v = classify_command(cmd);
        assert!(!v.read_only, "{cmd:?} must not be read-only: {v:?}");
    }
    for cmd in [
        "git diff HEAD~1",
        "git log HEAD^..HEAD",
        "git log @{u}..",
        "git log -- *.rs",
        "find . -name '*.rs'",
        "find . -name x -exec ls {} +",
        "ls *.rs",
        "wc -l *.rs",
        "file ./*",
        "file -- *",
        "sort -- *.txt",
        "sed -n 1p src/*.rs",
        "uniq in.txt",
    ] {
        let v = classify_command(cmd);
        assert!(v.read_only, "{cmd:?} must stay read-only: {v:?}");
    }
}

/// A grep, rg or git grep value option takes its value, so the next word is the pattern and only later operands are paths.
#[test]
fn a_grep_value_option_takes_its_value_not_the_pattern() {
    for (line, paths) in [
        ("grep -m 1 credentials src/", vec!["src/"]),
        ("grep -m1 credentials src/", vec!["src/"]),
        ("grep -A 2 -B 1 -C 3 id_rsa docs/", vec!["docs/"]),
        ("grep -D skip -d skip credentials src/", vec!["src/"]),
        ("grep --max-count 1 credentials src/", vec!["src/"]),
        ("grep --max-count=1 credentials src/", vec!["src/"]),
        ("grep --after-context 2 --before-context 1 x f", vec!["f"]),
        ("grep --devices skip --directories skip x f", vec!["f"]),
        ("grep --label in --binary-files text x f", vec!["f"]),
        ("grep --group-separator - x f", vec!["f"]),
        ("rg -m 1 -M 200 -j 4 -d 2 credentials src", vec!["src"]),
        ("rg -A 1 -B 1 -C 1 -E utf8 -r X x f", vec!["f"]),
        ("rg -t rust -T js -g '*.rs' x f", vec!["f"]),
        (
            "rg --max-depth 2 --max-columns 80 --threads 4 x f",
            vec!["f"],
        ),
        ("rg --type-add 'web:*.html' --sort path x f", vec!["f"]),
        ("rg --ignore-file .ignore x f", vec![".ignore", "f"]),
        ("git grep -m 1 credentials", vec![]),
        ("git grep -A 1 -C 2 x src", vec!["src"]),
        (
            "git grep --max-c 1 --max-depth 2 --threads 4 x src",
            vec!["src"],
        ),
        ("git grep --cont 1 x src", vec!["src"]),
        ("grep --binary x f", vec!["f"]),
        ("grep --context 3 x f", vec!["x", "f"]),
        ("grep --color always x f", vec!["x", "f"]),
        ("grep --context=3 x f", vec!["3", "f"]),
        ("grep -f pats.txt f", vec!["pats.txt", "f"]),
    ] {
        let got = path_operands(line).expect(line);
        assert_eq!(got, paths, "{line}");
    }
}

/// jq, git log and rg read long options by whole names only, so `jq --slurp` is never `--slurpfile`.
#[test]
fn a_tool_that_takes_no_abbreviation_is_read_by_whole_names() {
    for (line, paths) in [
        ("jq --slurp . a.json b.json", vec!["a.json", "b.json"]),
        (
            "jq -R --slurp . a ~/.ssh/id_rsa",
            vec!["a", "~/.ssh/id_rsa"],
        ),
        ("rg --ignore token ~/.ssh/id_rsa", vec!["~/.ssh/id_rsa"]),
        ("jq --slurpfile n f.json . a", vec!["f.json", "a"]),
        ("rg --ignore-file .ignore x f", vec![".ignore", "f"]),
        ("grep --max-c 1 x f", vec!["f"]),
        ("git grep --max-c 1 x src", vec!["src"]),
    ] {
        let got = path_operands(line).expect(line);
        assert_eq!(got, paths, "{line}");
    }
}

/// Any `LESS…` assignment is a hazard: a `LESSKEYIN` file can set `LESSOPEN`, which less runs.
#[test]
fn a_less_variable_assignment_is_a_hazard() {
    for line in [
        "LESSKEYIN=evil.lesskey less README.md",
        "LESSKEY_CONTENT='#env' less README.md",
        "LESSHISTFILE=/tmp/h less README.md",
        "LESSKEY=k cat README.md",
        "env LESSKEYIN=k less README.md",
        "export LESSKEYIN=k; less README.md",
    ] {
        let v = classify_command(line);
        assert!(!v.read_only, "{line}");
        assert!(v.reason.contains("LESS"), "{line}: {}", v.reason);
    }
    for line in ["less README.md", "NOTLESS=1 cat README.md"] {
        assert!(ro(line), "{line}: {:?}", classify_command(line));
    }
}

/// A line that sets a variable which redirects a reader the classifier keeps (BSD grep's
/// `GREP_OPTIONS`, MEASURED 2026-11-05 on macOS 27.0.1 to read a file it names; the xcrun shim
/// `/usr/bin/git`'s `DEVELOPER_DIR`, MEASURED the same day to run `<dir>/usr/bin/xcrun`, and its
/// `SDKROOT`; python's `PYTHONUSERBASE`, and on Windows its `APPDATA`, below which `site` places
/// the user site whose `usercustomize` it imports, REASONED from `site`'s source) is not
/// read-only, as one that sets `RIPGREP_CONFIG_PATH` or `PYTHONPATH` is not.
#[test]
fn a_reader_redirecting_assignment_is_a_hazard() {
    for (line, var) in [
        (
            "GREP_OPTIONS='-e x /etc/passwd' grep foo README.md",
            "GREP_OPTIONS",
        ),
        ("export GREP_OPTIONS=-r; grep foo .", "GREP_OPTIONS"),
        ("env GREP_OPTIONS=-r grep foo .", "GREP_OPTIONS"),
        ("DEVELOPER_DIR=/tmp/x git status", "DEVELOPER_DIR"),
        ("SDKROOT=/tmp/x git log -1", "SDKROOT"),
        ("PYTHONUSERBASE=/tmp/x python3 r.py", "PYTHONUSERBASE"),
        ("APPDATA=. python3 r.py", "APPDATA"),
    ] {
        let v = classify_command(line);
        assert!(!v.read_only, "{line}");
        assert!(v.reason.contains(var), "{line}: {}", v.reason);
    }
    // NEGATIVE CONTROL: a variable no kept reader reads, or one only spelled alike.
    for line in [
        "GREP_COLOR='1;32' grep foo README.md",
        "MY_GREP_OPTIONS=x cat README.md",
        "LOCALAPPDATA_X=y cat README.md",
    ] {
        assert!(ro(line), "{line}: {:?}", classify_command(line));
    }
}

/// `rg --hostname-bin` runs a program, so it is refused as `--pre` is.
#[test]
fn rg_hostname_bin_runs_a_program() {
    for line in [
        "rg --hostname-bin ./h --hyperlink-format default x src",
        "rg --hostname-bin=./h x src",
        "rg --pre ./p x src",
    ] {
        assert!(!ro(line), "{line}");
    }
    assert!(ro("rg --hyperlink-format default x src"));
}
