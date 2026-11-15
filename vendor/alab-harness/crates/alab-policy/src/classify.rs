// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Pure, fail-closed shell command read-only classifier for supervisor auto-approval.

/// Read-only classification verdict and deciding rule for a shell command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// `true` when every segment is a known read-only command and no danger token appears.
    pub read_only: bool,
    /// Rule that decided the verdict (`"every segment read-only"` or the offending token/head).
    pub reason: String,
}

impl Verdict {
    fn no(reason: impl Into<String>) -> Self {
        Self {
            read_only: false,
            reason: reason
                .into()
                .replace(QUOTED_SUBSTITUTION, "$(…)")
                .replace(QUOTED_PARAMETER, "\"$…\"")
                .replace(SUBSTITUTION, "$(…)"),
        }
    }
}

/// Placeholder token representing an unquoted command or backtick substitution.
pub const SUBSTITUTION: &str = "$…";

/// Placeholder token representing a double-quoted command substitution.
pub const QUOTED_SUBSTITUTION: &str = "$“…”";

/// Placeholder token representing a double-quoted parameter expansion.
pub const QUOTED_PARAMETER: &str = "$“p”";

const QUOTE_SUB_MARK: char = '\u{1}';

/// Maximum nesting depth for command substitutions before [`prelex`] refuses the line.
pub const MAX_SUBSTITUTION_DEPTH: usize = 16;

const GLOB_QUALIFIER_RUNS: &str =
    "a ( inside a word (zsh reads a glob qualifier, and `(e:…:)` or `(+cmd)` runs a command)";

const GLOB_SUBST_RUNS: &str = "zsh's $~ flag (the value is globbed as a pattern, and a qualifier `(e:…:)` in it runs a command)";

const ARITHMETIC_RUNS: &str = "an arithmetic evaluation ($((…)) or ((…)): it may assign any variable, PATH among \
     them, and a value's subscript runs a substitution)";

/// Default `--allow-python` glob list (empty).
pub const DEFAULT_PYTHON_ALLOW: &[&str] = &[];

const READ_ONLY: &[&str] = &[
    "git",
    "ls",
    "cat",
    "head",
    "tail",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "find",
    "mdfind",
    "wc",
    "sed",
    "awk",
    "sort",
    "uniq",
    "cut",
    "tr",
    "echo",
    "printf",
    "stat",
    "file",
    "which",
    "type",
    "df",
    "du",
    "uptime",
    "ps",
    "hostname",
    "sysctl",
    "date",
    "pwd",
    "true",
    "test",
    "[",
    "[[",
    "for",
    "do",
    "done",
    "if",
    "then",
    "else",
    "elif",
    "fi",
    "while",
    "jq",
    "diff",
    "comm",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "xargs",
    "env",
    "nl",
    "less",
    "more",
    "tree",
    "md5",
    "shasum",
    "sha256sum",
    "column",
    "paste",
    "seq",
    "expr",
    "export",
    "cd",
    "tmutil",
    "pgrep",
    "sleep",
    "strings",
    "lsof",
    "fold",
    "uname",
    "whoami",
    "id",
    "sw_vers",
];

const KEYWORD_PREFIX: &[&str] = &[
    "do", "then", "else", "if", "elif", "while", "until", "!", "{",
];

fn closes_a_brace(tok: &str) -> bool {
    let mut depth = 0usize;
    let mut chars = tok.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '{' => depth += 1,
            '}' if chars.peek().is_none() => return depth == 0,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

const SYSTEM_BIN_DIRS: &[&str] = &["/bin/", "/usr/bin/", "/sbin/", "/usr/sbin/"];

const HAZARD_VARS: &[&str] = &[
    "PATH",
    "CDPATH",
    "NULLCMD",
    "READNULLCMD",
    "MODULE_PATH",
    "FPATH",
    "STTY",
    "IFS",
    "BASH_ENV",
    "ENV",
    "PROMPT_COMMAND",
    "PS4",
    "SHELLOPTS",
    "BASHOPTS",
    "PAGER",
    "MANPAGER",
    "LESS",
    "LESSOPEN",
    "LESSCLOSE",
    "EDITOR",
    "VISUAL",
    "SHELL",
    "HOME",
    "XDG_CONFIG_HOME",
    "ZDOTDIR",
    "RIPGREP_CONFIG_PATH",
    "GREP_OPTIONS",
    "DEVELOPER_DIR",
    "SDKROOT",
    "PERL5OPT",
    "NODE_OPTIONS",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONUSERBASE",
    "APPDATA",
];

/// `LESS` covers every variable less reads: a `LESSKEYIN` file's `#env` can set `LESSOPEN` (MEASURED, less 668).
const HAZARD_VAR_PREFIXES: &[&str] = &["LD_", "DYLD_", "GIT_", "BASH_FUNC_", "LESS"];

const ZSH_TIED: &[(&str, &str)] = &[
    ("cdpath", "CDPATH"),
    ("fignore", "FIGNORE"),
    ("fpath", "FPATH"),
    ("mailpath", "MAILPATH"),
    ("manpath", "MANPATH"),
    ("module_path", "MODULE_PATH"),
    ("path", "PATH"),
    ("psvar", "PSVAR"),
    ("zsh_eval_context", "ZSH_EVAL_CONTEXT"),
];

#[cfg(test)]
const ZSH_RUNS_A_VALUE: &[&str] = &[
    "MODULE_PATH",
    "NULLCMD",
    "PATH",
    "READNULLCMD",
    "module_path",
    "path",
];

const GIT_READ: &[&str] = &[
    "log",
    "show",
    "status",
    "diff",
    "branch",
    "rev-parse",
    "rev-list",
    "ls-files",
    "grep",
    "check-ignore",
    "describe",
    "cat-file",
    "for-each-ref",
    "tag",
    "remote",
    "blame",
    "shortlog",
    "config",
    "merge-base",
    "name-rev",
    "worktree",
    "stash",
];

const GIT_DANGER: &[&str] = &[
    "push",
    "reset",
    "checkout",
    "switch",
    "rebase",
    "merge",
    "commit",
    "clean",
    "restore",
    "rm",
    "mv",
    "am",
    "cherry-pick",
    "revert",
    "pull",
    "fetch",
];

const FIND_EXEC_READ: &[&str] = &["du", "ls", "cat", "head", "wc", "stat", "file", "grep"];

const TMUTIL_READ: &[&str] = &["listlocalsnapshots", "latestbackup", "destinationinfo"];

/// Classify `cmd` using [`DEFAULT_PYTHON_ALLOW`].
pub fn classify_command(cmd: &str) -> Verdict {
    classify_command_with(cmd, DEFAULT_PYTHON_ALLOW)
}

/// Classify `cmd`, permitting `python3 <path>` when `<path>` matches `python_allow`.
pub fn classify_command_with<S: AsRef<str>>(cmd: &str, python_allow: &[S]) -> Verdict {
    classify_command_except(cmd, python_allow, &|_| false)
}

/// [`classify_command_with`], with each segment `passed` holds left to its caller: that
/// segment's command is not judged, while every scan of the whole line (its redirects, flags,
/// assignments, substitutions, network reach and arithmetic) still is.
pub fn classify_command_except<S: AsRef<str>>(
    cmd: &str,
    python_allow: &[S],
    passed: &dyn Fn(&[String]) -> bool,
) -> Verdict {
    let raw = cmd;
    let cmd = match prelex(cmd) {
        Ok(cmd) => cmd,
        Err(reason) => return Verdict::no(reason),
    };
    let cmd = strip_alarm_idiom(&cmd);
    let stripped = strip_quotes(&cmd);
    let segments = split_segments(&stripped);
    if segments.is_empty() {
        return Verdict::no("empty command");
    }

    if let Some(reason) = network_reach(raw) {
        return Verdict::no(reason);
    }
    if let Some(reason) = danger_scan(&segments) {
        return Verdict::no(reason);
    }
    if let Some(reason) = bare_redirection(&stripped) {
        return Verdict::no(reason);
    }
    if let Some(reason) = program_scan(&cmd) {
        return Verdict::no(reason);
    }
    if let Some(reason) = argv_flag_hazard(&cmd) {
        return Verdict::no(reason);
    }
    let mut left = false;
    for seg in &segments {
        if passed(seg) {
            left = true;
            continue;
        }
        if let Some(reason) = segment_head(seg, python_allow) {
            return Verdict::no(reason);
        }
    }
    if let Some(reason) = arithmetic_scan(&cmd) {
        return Verdict::no(reason);
    }
    Verdict {
        read_only: true,
        reason: if left {
            "every segment but its caller's read-only"
        } else {
            "every segment read-only"
        }
        .to_string(),
    }
}

/// Classify every segment of `cmd` except `rm` segments.
pub fn classify_except_rm<S: AsRef<str>>(cmd: &str, python_allow: &[S]) -> Verdict {
    let lexed = match prelex(cmd) {
        Ok(cmd) => cmd,
        Err(reason) => return Verdict::no(reason),
    };
    let lexed = strip_alarm_idiom(&lexed);
    let stripped = strip_quotes(&lexed);
    let (segments, left_out): (Vec<Vec<String>>, Vec<Vec<String>>) =
        split_segments(&stripped).into_iter().partition(|seg| {
            let head = seg.iter().find(|t| !is_assignment(t));
            match head {
                None => false,
                Some(h) => {
                    let p = program(h).to_ascii_lowercase();
                    p != "rm" && p != "mktemp"
                }
            }
        });
    if let Some(reason) = left_out
        .iter()
        .flat_map(|seg| seg.iter().take_while(|t| is_assignment(t)))
        .find_map(|t| assignment_hazard(t))
    {
        return Verdict::no(reason);
    }
    if let Some(reason) = left_out
        .iter()
        .find_map(|seg| (0..seg.len()).find_map(|i| redirect_hazard(seg, i)))
    {
        return Verdict::no(reason);
    }
    if let Some(reason) = danger_scan(&segments) {
        return Verdict::no(reason);
    }
    if let Some(reason) = bare_redirection(&stripped) {
        return Verdict::no(reason);
    }
    if let Some(reason) = program_scan(&lexed) {
        return Verdict::no(reason);
    }
    if let Some(reason) = program_words(&lexed)
        .iter()
        .filter(|seg| {
            seg.iter().find(|t| !is_assignment(t)).is_none_or(|h| {
                let p = program(h).to_ascii_lowercase();
                p != "rm" && p != "mktemp"
            })
        })
        .find_map(|seg| seg_flag_hazard(seg))
    {
        return Verdict::no(reason);
    }
    for seg in &segments {
        if is_mktemp_guard(seg) {
            continue;
        }
        if let Some(reason) = segment_head(seg, python_allow) {
            return Verdict::no(reason);
        }
    }
    if let Some(reason) = arithmetic_scan(&lexed) {
        return Verdict::no(reason);
    }
    Verdict {
        read_only: true,
        reason: "every segment but the rm and mktemp commands read-only".to_string(),
    }
}

/// Verify that `line` uses only plain zsh grammar whose expansions behave as text or paths; with
/// `extended_glob` (the startup may set `EXTENDED_GLOB`,
/// aterm's `ShellStartup::may_extended_glob`),
/// also no unquoted `^` and no unquoted `#` but one opening a comment: that option makes them
/// glob operators (`^x` any name but `x`, `x#` any run of `x`), which the secret check's globs do
/// not read. With it off both are text: MEASURED 2026-11-05 with zsh 5.9, `/bin/zsh -f -c
/// 'setopt NO_EXTENDED_GLOB; print -r -- W'` printed each of `HEAD^`, `HEAD^^`, `^fn`, `x^y`,
/// `a#b`, `~/.config/^x/hosts.yml` and `~/.s#h/config` as written (the last two under the home)
/// in a home holding `.config/gh/hosts.yml` and `.ssh/config` and a directory holding `HEAD` and
/// `fn` (`docs/measured/classifier-zsh-glob-2026-11-05.md`).
pub fn plain_zsh_line(line: &str, extended_glob: bool) -> Result<(), String> {
    let chars: Vec<char> = line.chars().collect();
    plain_zsh(&chars, 0, extended_glob)
}

const PLAIN_NEST: u8 = 8;

fn plain_zsh(chars: &[char], depth: u8, extended_glob: bool) -> Result<(), String> {
    #[derive(PartialEq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let boundary = |c: Option<&char>| c.is_none_or(|c| " \t\n;|&".contains(*c));
    let mut quote = Quote::None;
    let mut word = String::new();
    let mut quoted = false;
    let mut target = false;
    let mut command = true;
    let mut end_word = |word: &mut String, quoted: &mut bool, target: &mut bool, separator| {
        let whole = !word.is_empty() || *quoted;
        if whole && command && !*quoted && ZSH_RESERVED.contains(&word.as_str()) {
            return Err(format!("the reserved word `{word}`"));
        }
        command = separator || (command && (!whole || *target));
        *target &= !whole && !separator;
        word.clear();
        *quoted = false;
        Ok(())
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at_start = word.is_empty() && !quoted;
        if c.is_control() && c != '\t' && c != '\n' {
            return Err(format!("the control character `{}`", c.escape_default()));
        }
        if depth > 0 && c == '\n' && quote == Quote::None {
            return Err("a newline inside `$(…)`".into());
        }
        match quote {
            Quote::Single => {
                if c == '\'' {
                    quote = Quote::None;
                }
            }
            Quote::Double => match c {
                '"' => quote = Quote::None,
                '$' => {
                    i = plain_dollar(chars, i, depth, extended_glob)?;
                    continue;
                }
                '`' | '\\' => return Err(format!("`{c}` inside double quotes")),
                _ => {}
            },
            Quote::None => match c {
                '\'' => (quote, quoted) = (Quote::Single, true),
                '"' => (quote, quoted) = (Quote::Double, true),
                ' ' | '\t' => end_word(&mut word, &mut quoted, &mut target, false)?,
                '\n' | ';' | '|' | '&' => end_word(&mut word, &mut quoted, &mut target, true)?,
                '#' if at_start => {
                    if depth > 0 {
                        return Err("a comment inside `$(…)`".into());
                    }
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                '$' => {
                    i = plain_dollar(chars, i, depth, extended_glob)?;
                    word.push('$');
                    continue;
                }
                '`' | '\\' | '(' | ')' | '{' | '}' => return Err(format!("`{c}`")),
                '^' | '#' if extended_glob => {
                    return Err(format!(
                        "an unquoted `{c}` in a word, a glob operator under EXTENDED_GLOB"
                    ));
                }
                '[' if at_start && chars.get(i + 1) == Some(&'[') => return Err("`[[`".into()),
                '=' if at_start => return Err("a word opening with `=`".into()),
                '=' if !word.starts_with('-') => return Err(format!("the assignment `{word}=`")),
                '~' if !(at_start
                    && (chars.get(i + 1) == Some(&'/') || boundary(chars.get(i + 1)))) =>
                {
                    return Err("a `~` other than `~` or `~/`".into());
                }
                '<' => {
                    if !at_start || chars.get(i + 1).is_some_and(|n| "<>&(".contains(*n)) {
                        return Err("a `<` other than a plain input redirect".into());
                    }
                    target = true;
                }
                '>' => {
                    let mut j = i + 1;
                    if chars.get(j) == Some(&'&') {
                        j += 1;
                        let digits = j;
                        while chars.get(j).is_some_and(char::is_ascii_digit) {
                            j += 1;
                        }
                        if j == digits || !boundary(chars.get(j)) {
                            return Err("a `>&` onto other than a descriptor".into());
                        }
                    } else {
                        while chars.get(j).is_some_and(|c| *c == ' ' || *c == '\t') {
                            j += 1;
                        }
                        let onto: String = chars.iter().skip(j).take(9).collect();
                        if onto != "/dev/null" || !boundary(chars.get(j + 9)) {
                            return Err("a redirect onto other than /dev/null".into());
                        }
                        j += 9;
                    }
                    if !quoted && word.bytes().all(|b| b.is_ascii_digit()) {
                        word.clear();
                    }
                    end_word(&mut word, &mut quoted, &mut target, false)?;
                    i = j;
                    continue;
                }
                _ => word.push(c),
            },
        }
        i += 1;
    }
    if quote != Quote::None {
        return Err("an unterminated quote".into());
    }
    end_word(&mut word, &mut quoted, &mut target, true)
}

fn plain_dollar(
    chars: &[char],
    at: usize,
    depth: u8,
    extended_glob: bool,
) -> Result<usize, String> {
    let name_from = |from: usize| -> Option<usize> {
        let first = chars.get(from)?;
        if !(first.is_ascii_alphabetic() || *first == '_') {
            return None;
        }
        Some(
            (from..chars.len())
                .find(|&k| !(chars[k].is_ascii_alphanumeric() || chars[k] == '_'))
                .unwrap_or(chars.len()),
        )
    };
    let end = match chars.get(at + 1) {
        Some('(') if chars.get(at + 2) == Some(&'(') => {
            return Err("an arithmetic `$((`".into());
        }
        Some('(') => {
            if depth >= PLAIN_NEST {
                return Err("command substitutions nested too deep".into());
            }
            let close = substitution_close(chars, at + 2, 0)?;
            plain_zsh(&chars[at + 2..close], depth + 1, extended_glob)
                .map_err(|why| format!("inside `$(…)`: {why}"))?;
            close + 1
        }
        Some('{') => match name_from(at + 2) {
            Some(end) if chars.get(end) == Some(&'}') => end + 1,
            _ => return Err("a `${…}` other than `${NAME}`".into()),
        },
        _ => name_from(at + 1)
            .ok_or_else(|| "a `$` other than `$NAME`, `${NAME}` or `$(…)`".to_string())?,
    };
    match chars.get(end) {
        Some(c) if !c.is_ascii() || "[:({".contains(*c) => Err(format!(
            "`{c}` right after an expansion (a subscript, a modifier or a qualifier under zsh)"
        )),
        _ => Ok(end),
    }
}

fn substitution_close(chars: &[char], from: usize, nested: u8) -> Result<usize, String> {
    if nested >= PLAIN_NEST {
        return Err("command substitutions nested too deep".into());
    }
    let mut quote: Option<char> = None;
    let mut k = from;
    while k < chars.len() {
        let c = chars[k];
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => {}
            (_, '`' | '\\') => return Err(format!("`{c}` inside `$(…)`")),
            (_, '$') if chars.get(k + 1) == Some(&'(') => {
                if chars.get(k + 2) == Some(&'(') {
                    return Err("an arithmetic `$((`".into());
                }
                k = substitution_close(chars, k + 2, nested + 1)?;
            }
            (None, '\'' | '"') => quote = Some(c),
            (None, ')') => return Ok(k),
            _ => {}
        }
        k += 1;
    }
    Err("a `$(` with no `)`".into())
}

const ZSH_RESERVED: &[&str] = &[
    "!",
    "case",
    "coproc",
    "declare",
    "do",
    "done",
    "elif",
    "else",
    "end",
    "esac",
    "export",
    "fi",
    "float",
    "for",
    "foreach",
    "function",
    "if",
    "integer",
    "local",
    "nocorrect",
    "readonly",
    "repeat",
    "select",
    "then",
    "time",
    "typeset",
    "until",
    "while",
];

fn is_mktemp_guard(seg: &[String]) -> bool {
    match seg {
        [head, ..] if head == ":" => true,
        [head] => head == "exit",
        [head, n] => head == "exit" && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()),
        _ => false,
    }
}

/// Strip `#` comments and non-expanding here-doc bodies, and refuse complex substitutions.
pub fn prelex(src: &str) -> Result<String, String> {
    let mut lx = Prelex {
        chars: src.chars().collect(),
        i: 0,
        out: String::with_capacity(src.len()),
        heredocs: Vec::new(),
        depth: 0,
        backticks: 0,
        escaped_to: 0,
    };
    lx.run(None)?;
    if !lx.heredocs.is_empty() {
        return Err("a here-document with no body".to_string());
    }
    Ok(lx.out)
}

struct Heredoc {
    delim: String,
    strip_tabs: bool,
    quoted: bool,
}

struct Prelex {
    chars: Vec<char>,
    i: usize,
    out: String,
    heredocs: Vec<Heredoc>,
    depth: usize,
    backticks: usize,
    escaped_to: usize,
}

impl Prelex {
    fn peek(&self, k: usize) -> Option<char> {
        self.chars.get(self.i + k).copied()
    }

    fn at_word_start(&self) -> bool {
        self.i == 0
            || (self.escaped_to != self.i
                && (self.chars[self.i - 1].is_whitespace()
                    || matches!(self.chars[self.i - 1], ';' | '&' | '|' | '(' | ')')))
    }

    fn glued_paren(&self) -> bool {
        let boundary = |k: usize| {
            k == 0 || {
                let p = self.chars[k - 1];
                p.is_whitespace() || matches!(p, ';' | '&' | '|' | '(')
            }
        };
        if boundary(self.i) {
            return false;
        }
        let p = self.chars[self.i - 1];
        !(matches!(p, '<' | '>' | '=') && boundary(self.i - 1))
    }

    fn substitution(&mut self, stop: char) -> Result<(), String> {
        if self.depth >= MAX_SUBSTITUTION_DEPTH {
            return Err(format!(
                "command substitutions nested deeper than {MAX_SUBSTITUTION_DEPTH}"
            ));
        }
        let backtick = usize::from(stop == '`');
        self.depth += 1;
        self.backticks += backtick;
        self.run(Some(stop))?;
        self.depth -= 1;
        self.backticks -= backtick;
        Ok(())
    }

    fn nested_backtick(&self) -> Result<(), String> {
        if self.backticks > 0 && self.peek(1) == Some('`') {
            return Err("a backtick substitution nested in another (\\`)".to_string());
        }
        Ok(())
    }

    fn run(&mut self, stop: Option<char>) -> Result<(), String> {
        let mut depth = 0usize;
        while let Some(c) = self.peek(0) {
            match c {
                '\\' => {
                    self.nested_backtick()?;
                    self.out.push(c);
                    self.i += 1;
                    if let Some(n) = self.peek(0) {
                        self.out.push(n);
                        self.i += 1;
                        self.escaped_to = self.i;
                    }
                }
                '\'' => self.single_quoted()?,
                '"' => self.double_quoted()?,
                '$' if self.peek(1) == Some('\'') => {
                    self.i += 2;
                    let start = self.i;
                    while let Some(d) = self.peek(0) {
                        if d == '\\' {
                            return Err("$'…' with a backslash escape".to_string());
                        }
                        if d == '\'' {
                            break;
                        }
                        self.i += 1;
                    }
                    if self.peek(0) != Some('\'') {
                        return Err("an unterminated $' quote".to_string());
                    }
                    let body: String = self.chars[start..self.i].iter().collect();
                    self.i += 1;
                    self.out.push('\'');
                    self.out.push_str(&body);
                    self.out.push('\'');
                }
                '$' if self.peek(1) == Some('"') => {
                    self.i += 1;
                    self.double_quoted()?;
                }
                '$' if self.peek(1) == Some('{') => self.braced_parameter()?,
                '$' if self.peek(1) == Some('(') && self.peek(2) == Some('(') => {
                    return Err(ARITHMETIC_RUNS.to_string());
                }
                '$' if self.peek(1) == Some('(') => {
                    self.out.push_str("$(");
                    self.i += 2;
                    self.substitution(')')?;
                    self.out.push(')');
                }
                '$' => self.bare_parameter()?,
                '`' if stop == Some('`') => {
                    self.i += 1;
                    return Ok(());
                }
                '`' => {
                    self.out.push('`');
                    self.i += 1;
                    self.substitution('`')?;
                    self.out.push('`');
                }
                '(' if self.glued_paren() => {
                    return Err(GLOB_QUALIFIER_RUNS.to_string());
                }
                '(' if self.peek(1) == Some('(') => {
                    return Err(ARITHMETIC_RUNS.to_string());
                }
                '(' => {
                    depth += 1;
                    self.out.push(c);
                    self.i += 1;
                }
                ')' => {
                    self.i += 1;
                    if depth == 0 && stop == Some(')') {
                        return Ok(());
                    }
                    depth = depth.saturating_sub(1);
                    self.out.push(c);
                }
                '#' if self.at_word_start() => {
                    while self.peek(0).is_some_and(|d| d != '\n') {
                        self.i += 1;
                    }
                }
                '<' if self.peek(1) == Some('<') && self.peek(2) == Some('<') => {
                    self.out.push_str("<<<");
                    self.i += 3;
                }
                '<' if self.peek(1) == Some('<') => self.heredoc_operator()?,
                '\n' => {
                    self.out.push('\n');
                    self.i += 1;
                    self.heredoc_bodies()?;
                }
                _ => {
                    self.out.push(c);
                    self.i += 1;
                }
            }
        }
        match stop {
            Some(')') => Err("an unterminated $(".to_string()),
            Some(_) => Err("an unterminated backtick".to_string()),
            None => Ok(()),
        }
    }

    fn single_quoted(&mut self) -> Result<(), String> {
        let start = self.i;
        self.i += 1;
        while self.peek(0).is_some_and(|d| d != '\'') {
            self.i += 1;
        }
        if self.peek(0).is_none() {
            return Err("an unterminated ' quote".to_string());
        }
        self.i += 1;
        self.out.extend(&self.chars[start..self.i]);
        Ok(())
    }

    fn double_quoted(&mut self) -> Result<(), String> {
        self.out.push('"');
        self.i += 1;
        loop {
            let Some(d) = self.peek(0) else {
                return Err("an unterminated \" quote".to_string());
            };
            match d {
                '\\' => {
                    self.nested_backtick()?;
                    self.out.push(d);
                    self.i += 1;
                    if let Some(n) = self.peek(0) {
                        self.out.push(n);
                        self.i += 1;
                    }
                }
                '"' => {
                    self.out.push(d);
                    self.i += 1;
                    return Ok(());
                }
                '$' if self.peek(1) == Some('(') && self.peek(2) == Some('(') => {
                    return Err(ARITHMETIC_RUNS.to_string());
                }
                '$' if self.peek(1) == Some('(') => {
                    self.out.push_str("$(");
                    self.i += 2;
                    self.substitution(')')?;
                    self.out.push(')');
                }
                '$' if self.peek(1) == Some('{') => self.braced_parameter()?,
                '$' => self.bare_parameter()?,
                '`' => {
                    self.out.push('`');
                    self.i += 1;
                    self.substitution('`')?;
                    self.out.push('`');
                }
                _ => {
                    self.out.push(d);
                    self.i += 1;
                }
            }
        }
    }

    fn bare_parameter(&mut self) -> Result<(), String> {
        if let Some(why) = self.parameter_hazard() {
            return Err(why);
        }
        self.out.push('$');
        self.i += 1;
        Ok(())
    }

    fn parameter_hazard(&self) -> Option<String> {
        let after = &self.chars[self.i + 1..];
        if after
            .iter()
            .take_while(|c| ZSH_PARAMETER_FLAGS.contains(**c))
            .any(|&c| c == '~')
        {
            return Some(GLOB_SUBST_RUNS.to_string());
        }
        dollar_arithmetic(after).map(str::to_string)
    }

    fn braced_parameter(&mut self) -> Result<(), String> {
        let start = self.i;
        self.i += 2;
        while let Some(d) = self.peek(0) {
            match d {
                '}' => {
                    let body: String = self.chars[start + 2..self.i].iter().collect();
                    braced_is_literal(&body)?;
                    self.i += 1;
                    self.out.extend(&self.chars[start..self.i]);
                    return Ok(());
                }
                '\'' | '"' | '`' | '\\' | '{' => {
                    return Err(
                        "a ${…} expansion holding a quote or a nested expansion".to_string()
                    );
                }
                '$' if matches!(self.peek(1), Some('(' | '{')) => {
                    return Err("a ${…} expansion holding a substitution".to_string());
                }
                '$' => {
                    if let Some(why) = self.parameter_hazard() {
                        return Err(format!("{why}, inside a ${{…}} expansion"));
                    }
                    self.i += 1;
                }
                _ => self.i += 1,
            }
        }
        Err("an unterminated ${".to_string())
    }

    fn heredoc_operator(&mut self) -> Result<(), String> {
        self.out.push_str("<<");
        self.i += 2;
        let strip_tabs = self.peek(0) == Some('-');
        if strip_tabs {
            self.out.push('-');
            self.i += 1;
        }
        while self.peek(0).is_some_and(|d| d == ' ' || d == '\t') {
            self.out.push(self.chars[self.i]);
            self.i += 1;
        }
        let mut delim = String::new();
        let mut quoted = false;
        while let Some(d) = self.peek(0) {
            if d.is_whitespace() || matches!(d, ';' | '&' | '|' | '(' | ')' | '<' | '>') {
                break;
            }
            self.out.push(d);
            self.i += 1;
            match d {
                '\'' | '"' => {
                    quoted = true;
                    while let Some(e) = self.peek(0) {
                        self.out.push(e);
                        self.i += 1;
                        if e == d {
                            break;
                        }
                        delim.push(e);
                    }
                }
                '\\' => {
                    quoted = true;
                    if let Some(e) = self.peek(0) {
                        self.out.push(e);
                        self.i += 1;
                        delim.push(e);
                    }
                }
                '$' | '`' => {
                    return Err("a here-document delimiter with an expansion".to_string());
                }
                _ => delim.push(d),
            }
        }
        if delim.is_empty() {
            return Err("a here-document with no delimiter".to_string());
        }
        self.heredocs.push(Heredoc {
            delim,
            strip_tabs,
            quoted,
        });
        Ok(())
    }

    fn heredoc_bodies(&mut self) -> Result<(), String> {
        for doc in std::mem::take(&mut self.heredocs) {
            loop {
                if self.i >= self.chars.len() {
                    break;
                }
                let start = self.i;
                while self.peek(0).is_some_and(|d| d != '\n') {
                    self.i += 1;
                }
                let line: String = self.chars[start..self.i].iter().collect();
                if self.peek(0) == Some('\n') {
                    self.i += 1;
                }
                let bare = if doc.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if bare == doc.delim {
                    break;
                }
                if !doc.quoted && (line.contains("$(") || line.contains('`')) {
                    return Err(
                        "a here-document body that runs a command (unquoted delimiter)".to_string(),
                    );
                }
                if !doc.quoted {
                    body_arithmetic_is_literal(&line)?;
                }
            }
        }
        Ok(())
    }
}

fn braced_is_literal(body: &str) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("a ${{{body}}} expansion with {why}"));
    let int = |t: &str| {
        let t = t.trim();
        let t = t.strip_prefix('-').unwrap_or(t);
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
    };
    let mut rest = body;
    if let Some(r) = rest.strip_prefix(['#', '!']) {
        if r.is_empty() {
            return Ok(());
        }
        if body == "!#" {
            return Ok(());
        }
        if body.starts_with('!') {
            let name = ["[@]", "[*]", "@", "*"]
                .iter()
                .find_map(|listing| r.strip_suffix(listing));
            return match name {
                Some(n) if is_name(n) => Ok(()),
                _ => refuse("an indirection (bash expands the parameter its value names)"),
            };
        }
        rest = r;
    }
    let name_len = if rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len())
    } else if rest.starts_with(|c: char| c.is_ascii_digit()) {
        rest.find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len())
    } else if rest.starts_with(['@', '*', '#', '?', '$', '!', '-']) {
        1
    } else {
        return refuse("a form this reader does not follow");
    };
    let name = &rest[..name_len];
    rest = &rest[name_len..];
    if let Some(r) = rest.strip_prefix('[') {
        let Some(end) = r.find(']') else {
            return refuse("an unterminated subscript");
        };
        let sub = &r[..end];
        if !(sub == "@" || sub == "*" || int(sub)) {
            return refuse("a subscript that is not a literal (evaluated as arithmetic)");
        }
        rest = &r[end + 1..];
    }
    if rest.starts_with('@') {
        return refuse("a transformation (bash's @P expands the value as a prompt)");
    }
    if let Some(r) = rest.strip_prefix(':')
        && !r.starts_with(['-', '=', '+', '?'])
        && (!r.split(':').take(2).all(int) || r.split(':').count() > 2)
    {
        return refuse("a substring offset that is not a literal (evaluated as arithmetic)");
    }
    if let Some(value) = rest.strip_prefix(":=").or_else(|| rest.strip_prefix('='))
        && let Some(why) = assignment_hazard(&format!("{name}={value}"))
    {
        return refuse(&format!("a default assignment, {why}"));
    }
    Ok(())
}

const ZSH_PARAMETER_FLAGS: &str = "=^+#~";

fn dollar_arithmetic(after: &[char]) -> Option<&'static str> {
    if after.first() == Some(&'[') {
        return Some("a $[…] arithmetic expansion");
    }
    let name_char = |c: &char| c.is_ascii_alphanumeric() || *c == '_';
    let mut k = after
        .iter()
        .take_while(|c| ZSH_PARAMETER_FLAGS.contains(**c))
        .count();
    if after.get(k).is_some_and(name_char) {
        k += after[k..].iter().take_while(|c| name_char(c)).count();
    } else if after
        .get(k)
        .is_some_and(|c| matches!(c, '*' | '@' | '?' | '-' | '!' | '$' | '#'))
    {
        k += 1;
    }
    (k > 0 && after.get(k) == Some(&'['))
        .then_some("a $name[…] subscript (zsh evaluates it as arithmetic)")
}

fn body_arithmetic_is_literal(line: &str) -> Result<(), String> {
    let mut rest = line;
    while let Some(at) = rest.find('$') {
        let after = &rest[at + 1..];
        let chars: Vec<char> = after.chars().collect();
        if let Some(why) = dollar_arithmetic(&chars) {
            return Err(format!("a here-document body with {why}"));
        }
        if let Some(b) = after.strip_prefix('{') {
            let Some(end) = b.find('}') else {
                return Err("an unterminated ${ in a here-document body".to_string());
            };
            braced_is_literal(&b[..end])?;
        }
        rest = after;
    }
    Ok(())
}

fn strip_alarm_idiom(cmd: &str) -> String {
    let mut s = cmd.to_string();
    let mut from = 0;
    while let Some(rel) = s[from..].find("perl -e ") {
        let start = from + rel;
        let body_start = start + "perl -e ".len();
        let Some(q) = s[body_start..]
            .chars()
            .next()
            .filter(|c| *c == '\'' || *c == '"')
        else {
            from = body_start;
            continue;
        };
        let inner_start = body_start + 1;
        let Some(close_rel) = s[inner_start..].find(q) else {
            break;
        };
        let inner = &s[inner_start..inner_start + close_rel];
        if is_alarm_body(inner) {
            let mut end = inner_start + close_rel + 1;
            while s[end..].starts_with(' ') {
                end += 1;
            }
            s.replace_range(start..end, "");
            from = start;
        } else {
            from = inner_start + close_rel + 1;
        }
    }
    s
}

fn is_alarm_body(body: &str) -> bool {
    let Some(rest) = body.trim().strip_prefix("alarm") else {
        return false;
    };
    let rest = rest.trim_start();
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    let Some(rest) = rest[digits..].trim_start().strip_prefix(';') else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix("exec") else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix("@ARGV") else {
        return false;
    };
    let rest = rest.trim();
    rest.is_empty() || rest == ";"
}

fn strip_quotes(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    strip_into(&chars, &mut i, &mut out, None);
    out
}

fn strip_into(chars: &[char], i: &mut usize, out: &mut String, stop: Option<char>) {
    let mut depth = 0usize;
    while *i < chars.len() {
        let c = chars[*i];
        match c {
            '\\' => {
                match chars.get(*i + 1) {
                    Some(&n) if n == '(' || n == ')' => out.push(n),
                    Some(&n) if n == QUOTE_SUB_MARK => out.push('\\'),
                    Some(&n) => {
                        out.push('\\');
                        out.push(n);
                    }
                    None => out.push('\\'),
                }
                *i += if *i + 1 < chars.len() { 2 } else { 1 };
            }
            '\'' => {
                *i += 1;
                while *i < chars.len() && chars[*i] != '\'' {
                    *i += 1;
                }
                *i += 1;
                out.push_str("\"\"");
            }
            '"' => {
                *i += 1;
                out.push_str("\"\"");
                while *i < chars.len() {
                    let d = chars[*i];
                    if d == '\\' {
                        *i += 2;
                        continue;
                    }
                    if d == '"' {
                        *i += 1;
                        break;
                    }
                    if d == '$' && chars.get(*i + 1) == Some(&'(') {
                        out.push(QUOTE_SUB_MARK);
                        out.push_str("$(");
                        *i += 2;
                        strip_into(chars, i, out, Some(')'));
                        out.push(')');
                        continue;
                    }
                    if d == '`' {
                        out.push(QUOTE_SUB_MARK);
                        out.push('`');
                        *i += 1;
                        strip_into(chars, i, out, Some('`'));
                        out.push('`');
                        continue;
                    }
                    if d == '$'
                        && chars.get(*i + 1).is_some_and(|&n| {
                            n.is_ascii_alphanumeric()
                                || matches!(n, '_' | '{' | '@' | '*' | '#' | '?' | '$' | '!' | '-')
                        })
                    {
                        out.push_str(QUOTED_PARAMETER);
                        *i += 1;
                        match chars.get(*i) {
                            Some('{') => {
                                while *i < chars.len() && chars[*i] != '}' && chars[*i] != '"' {
                                    *i += 1;
                                }
                                if chars.get(*i) == Some(&'}') {
                                    *i += 1;
                                }
                            }
                            Some(&n) if n.is_ascii_alphanumeric() || n == '_' => {
                                while chars
                                    .get(*i)
                                    .is_some_and(|&n| n.is_ascii_alphanumeric() || n == '_')
                                {
                                    *i += 1;
                                }
                            }
                            Some(_) => *i += 1,
                            None => {}
                        }
                        continue;
                    }
                    *i += 1;
                }
            }
            '$' if chars.get(*i + 1) == Some(&'(') => {
                out.push_str("$(");
                *i += 2;
                strip_into(chars, i, out, Some(')'));
                out.push(')');
            }
            '`' if stop == Some('`') => {
                *i += 1;
                return;
            }
            '(' => {
                depth += 1;
                out.push('(');
                *i += 1;
            }
            ')' => {
                *i += 1;
                if depth == 0 && stop == Some(')') {
                    return;
                }
                depth = depth.saturating_sub(1);
                out.push(')');
            }
            _ => {
                if c != QUOTE_SUB_MARK {
                    out.push(c);
                }
                *i += 1;
            }
        }
    }
}

fn split_segments(s: &str) -> Vec<Vec<String>> {
    split_segments_at(s)
        .into_iter()
        .map(|(seg, _)| seg)
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SegAt {
    Command,
    FileRead,
    AfterGroup,
}

fn split_segments_at(s: &str) -> Vec<(Vec<String>, SegAt)> {
    let chars: Vec<char> = s.chars().collect();
    let mut segments = Vec::new();
    let mut i = 0;
    split_level(&chars, &mut i, None, &mut segments);
    segments
}

fn mark_file_read(
    chars: &[char],
    start: usize,
    end: usize,
    first: usize,
    segments: &mut [(Vec<String>, SegAt)],
) {
    let body = &chars[start.min(end)..end];
    if segments.len() != first + 1 || body.iter().any(|c| matches!(c, ';' | '&' | '|')) {
        return;
    }
    let (toks, at) = &mut segments[first];
    let op = toks[0].strip_prefix('0').unwrap_or(&toks[0]);
    let Some(rest) = op.strip_prefix('<') else {
        return;
    };
    if rest.starts_with(['<', '>', '&', '(']) {
        return;
    }
    if (toks.len() == 1 && !rest.is_empty()) || (toks.len() == 2 && rest.is_empty()) {
        *at = SegAt::FileRead;
    }
}

fn split_level(
    chars: &[char],
    i: &mut usize,
    stop: Option<char>,
    segments: &mut Vec<(Vec<String>, SegAt)>,
) {
    let mut cur = String::new();
    let mut at = SegAt::Command;
    let flush = |cur: &mut String, at: &mut SegAt, segments: &mut Vec<(Vec<String>, SegAt)>| {
        let toks: Vec<String> = cur.split_whitespace().map(str::to_string).collect();
        if !toks.is_empty() {
            segments.push((toks, *at));
        }
        cur.clear();
        *at = SegAt::Command;
    };
    let body =
        |chars: &[char], i: &mut usize, close: char, segments: &mut Vec<(Vec<String>, SegAt)>| {
            let (start, first) = (*i, segments.len());
            split_level(chars, i, Some(close), segments);
            mark_file_read(
                chars,
                start,
                i.saturating_sub(1).max(start),
                first,
                segments,
            );
        };
    let mut depth = 0usize;
    while *i < chars.len() {
        let c = chars[*i];
        match c {
            '\\' => {
                cur.push(c);
                if let Some(&n) = chars.get(*i + 1) {
                    cur.push(n);
                    *i += 1;
                }
            }
            _ if c == QUOTE_SUB_MARK
                && chars.get(*i + 1) == Some(&'$')
                && chars.get(*i + 2) == Some(&'(') =>
            {
                *i += 3;
                body(chars, i, ')', segments);
                cur.push_str(QUOTED_SUBSTITUTION);
                continue;
            }
            _ if c == QUOTE_SUB_MARK && chars.get(*i + 1) == Some(&'`') => {
                *i += 2;
                body(chars, i, '`', segments);
                cur.push_str(QUOTED_SUBSTITUTION);
                continue;
            }
            _ if c == QUOTE_SUB_MARK => {
                // Stray mark; consumed by the outer `*i += 1`.
            }
            '$' if chars.get(*i + 1) == Some(&'(') => {
                *i += 2;
                body(chars, i, ')', segments);
                cur.push_str(SUBSTITUTION);
                continue;
            }
            '`' if stop == Some('`') => {
                *i += 1;
                break;
            }
            '`' => {
                *i += 1;
                body(chars, i, '`', segments);
                cur.push_str(SUBSTITUTION);
                continue;
            }
            ')' if depth == 0 && stop == Some(')') => {
                *i += 1;
                break;
            }
            '(' => {
                depth += 1;
                flush(&mut cur, &mut at, segments);
            }
            ')' => {
                let closes_a_group = depth > 0;
                depth = depth.saturating_sub(1);
                flush(&mut cur, &mut at, segments);
                if closes_a_group {
                    at = SegAt::AfterGroup;
                }
            }
            ';' | '|' | '\n' => flush(&mut cur, &mut at, segments),
            '>' | '<' => {
                if glued_to_a_word(&cur) {
                    cur.push(' ');
                }
                cur.push(c);
            }
            '&' if chars.get(*i + 1) == Some(&'&') => {
                flush(&mut cur, &mut at, segments);
                *i += 1;
            }
            '&' => {
                let prev = (*i).checked_sub(1).map(|k| chars[k]);
                let next = chars.get(*i + 1).copied();
                if next == Some('>') && glued_to_a_word(&cur) {
                    cur.push(' ');
                    cur.push(c);
                } else if prev == Some('>') || prev == Some('<') || next == Some('>') {
                    cur.push(c);
                } else {
                    flush(&mut cur, &mut at, segments);
                }
            }
            _ => cur.push(c),
        }
        *i += 1;
    }
    flush(&mut cur, &mut at, segments);
}

fn glued_to_a_word(cur: &str) -> bool {
    let word = cur.rsplit(char::is_whitespace).next().unwrap_or("");
    let Some(last) = word.chars().last() else {
        return false;
    };
    !matches!(last, '>' | '<' | '&') && !word.chars().all(|c| c.is_ascii_digit())
}

/// Basename of command token `tok`.
pub fn program(tok: &str) -> &str {
    tok.rsplit('/').next().unwrap_or(tok)
}

fn redirect_target(tok: &str) -> Option<Option<&str>> {
    let pos = tok.find('>')?;
    if tok[..pos].ends_with('<') {
        return None;
    }
    let rest = tok[pos..].trim_start_matches('>');
    let rest = rest.strip_prefix('|').unwrap_or(rest);
    Some((!rest.is_empty()).then_some(rest))
}

fn redirect_is_safe(target: &str) -> bool {
    target == "/dev/null"
        || target.strip_prefix('&').is_some_and(|fd| {
            fd == "-" || (!fd.is_empty() && fd.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn redirect_hazard(seg: &[String], i: usize) -> Option<String> {
    let tok = seg[i].as_str();
    if tok.contains("<>") {
        return Some(format!("redirect <> {tok}"));
    }
    match redirect_target(tok)?.or(seg.get(i + 1).map(String::as_str)) {
        Some(t) if redirect_is_safe(t) => None,
        Some(t) => Some(format!("redirect > {t}")),
        None => Some("redirect > (no target)".to_string()),
    }
}

/// The subcommand of the `git` at `seg[git_idx]`, with its index: the first word past git's
/// redirects and global options (`-C`, `-c`, `--git-dir`, `--work-tree`, `--namespace`,
/// `--super-prefix` and `--attr-source` with their values, any other flag alone), or the word
/// after a `--`; `None` when no word follows.
pub fn git_subcommand(seg: &[String], git_idx: usize) -> Option<(usize, &str)> {
    let mut j = git_idx + 1;
    while j < seg.len() {
        let t = seg[j].as_str();
        let span = redirect_span(t);
        if span > 0 {
            j += span;
        } else if t == "--" {
            return seg.get(j + 1).map(|s| (j + 1, s.as_str()));
        } else if matches!(
            t,
            "-C" | "-c"
                | "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--super-prefix"
                | "--attr-source"
        ) {
            j += 2;
        } else if t.starts_with('-') {
            j += 1;
        } else {
            return Some((j, t));
        }
    }
    None
}

fn danger_scan(segments: &[Vec<String>]) -> Option<String> {
    let has = |open: &str| segments.iter().flatten().any(|t| t == open);
    let (braces, brackets) = (has("{"), has("[["));
    let closing = |t: &str| (braces && closes_a_brace(t)) || (brackets && t == "]]");
    for seg in segments {
        if let Some(reason) = loop_assignment_hazard(seg) {
            return Some(reason);
        }
        for (i, tok) in seg.iter().enumerate() {
            let prog = program(tok);
            if let Some(reason) = assignment_hazard(tok) {
                return Some(reason);
            }
            if closing(tok) {
                let mut k = i + 1;
                while let Some(span) = seg.get(k).map(|t| redirect_span(t)).filter(|&n| n > 0) {
                    k += span;
                }
                if seg.get(k).is_some_and(|n| !closing(n)) {
                    return Some(format!("a word after a closing {tok} (zsh may run it)"));
                }
            }
            if prog == "xargs"
                && let Some(k) = xargs_command_at(seg, i, false)
                && let Some(fed) = seg.get(k).map(|t| program(t))
                && matches!(fed, "rm" | "mv" | "cp")
            {
                return Some(format!("xargs {fed}"));
            }
            if tok == "." && i == 0 {
                return Some("source".to_string());
            }
            let test_head = tok == "["
                || (tok == "test" && (i == 0 || KEYWORD_PREFIX.contains(&seg[i - 1].as_str())));
            if tok == "[[" || test_head {
                let arith: &[&str] = if tok == "[[" {
                    &["-eq", "-ne", "-lt", "-le", "-gt", "-ge", "-v"]
                } else {
                    &["-v"]
                };
                if let Some(op) = seg[i + 1..]
                    .iter()
                    .take_while(|t| *t != "]]" && *t != "]")
                    .find(|t| arith.contains(&t.as_str()))
                {
                    return Some(format!("{tok} {op} (evaluates arithmetic)"));
                }
                if let Some(t) = seg[i + 1..]
                    .iter()
                    .take_while(|t| *t != "]")
                    .find(|t| tok != "[[" && !t.starts_with('/') && expands_apart(t))
                {
                    return Some(format!(
                        "{tok} {t} (the shell may expand it into `-t` and an operand)"
                    ));
                }
            }
            if let Some(reason) = redirect_hazard(seg, i) {
                return Some(reason);
            }
            if let Some(reason) = flag_hazard_at(seg, i) {
                return Some(reason);
            }
        }
    }
    None
}

fn flag_hazard_at(seg: &[String], i: usize) -> Option<String> {
    let tok = &seg[i];
    let prog = program(tok);
    let next = seg.get(i + 1).map(String::as_str);
    let next_prog = next.map(program);
    if tok == "-delete" {
        return Some("find -delete".to_string());
    }
    if tok.starts_with("-fprint") || tok == "-fls" {
        return Some(format!("find {tok}"));
    }
    if prog == "sort" || prog == "tree" {
        for t in &seg[i + 1..] {
            if long_option_spells(t, "output")
                || (t.starts_with('-') && !t.starts_with("--") && t[1..].contains('o'))
            {
                return Some(format!("{prog} -o"));
            }
        }
    }
    if prog == "uniq" {
        let mut positionals = 0;
        let mut k = i + 1;
        while k < seg.len() {
            let t = seg[k].as_str();
            if matches!(t, "-f" | "-s" | "-w") {
                k += 2;
                continue;
            }
            if t.starts_with('-') && t.len() > 1 {
                k += 1;
                continue;
            }
            positionals += 1;
            k += 1;
        }
        if positionals >= 2 {
            return Some("uniq <output file>".to_string());
        }
    }
    if matches!(tok.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
        match next_prog {
            Some(p) if next.is_some_and(|n| n.contains('/') && !is_system_bin(n)) => {
                return Some(format!("find {tok} {p} (by path)"));
            }
            Some(p) if FIND_EXEC_READ.contains(&p) => {
                let args: Vec<String> = seg[i + 2..]
                    .iter()
                    .take_while(|t| !matches!(t.as_str(), ";" | "\\;" | "+"))
                    .cloned()
                    .collect();
                if let Some(reason) = runs_or_writes_by_flag_reading(p, &args, Reading::Argv) {
                    return Some(format!("find {tok} {reason}"));
                }
            }
            Some(p) => return Some(format!("find {tok} {p}")),
            None => return Some(format!("find {tok}")),
        }
    }
    if prog == "sed" {
        for t in &seg[i + 1..] {
            let short_in_place = t.starts_with('-') && !t.starts_with("--") && {
                let mut hit = false;
                for c in t[1..].chars() {
                    match c {
                        'i' | 'I' => {
                            hit = true;
                            break;
                        }
                        'f' | 'e' => break,
                        _ => {}
                    }
                }
                hit
            };
            if t == "--in-place"
                || t.starts_with("--in-place=")
                || long_option_spells(t, "in-place")
                || short_in_place
            {
                return Some("sed -i".to_string());
            }
        }
    }
    if matches!(prog, "python" | "python2" | "python3") {
        match next {
            Some("-c") => return Some(format!("{prog} -c")),
            Some("-") => return Some(format!("{prog} heredoc")),
            _ => {}
        }
    }
    if prog == "perl" && matches!(next, Some("-e") | Some("-E")) {
        return Some("perl -e".to_string());
    }
    if matches!(prog, "bash" | "sh" | "zsh") && next == Some("-c") {
        return Some(format!("{prog} -c"));
    }
    if prog == "git" {
        let mut j = i + 1;
        while let Some(t) = seg.get(j).map(String::as_str) {
            let span = redirect_span(t);
            if span > 0 {
                j += span;
                continue;
            }
            if t == "-c"
                || (t.starts_with("-c") && t.len() > 2)
                || t.starts_with("--config-env")
                || t.starts_with("--exec-path")
                || t.starts_with("--attr-source")
            {
                return Some(format!("git {t} (sets config for the run)"));
            }
            if t.starts_with("--git-dir") || t.starts_with("--work-tree") {
                return Some(format!("git {t} (a repository of the line's choosing)"));
            }
            if t == "--help" || t == "-h" {
                return Some(format!("git {t} {GIT_HELP_RUNS}"));
            }
            if t == "--" {
                break;
            }
            if matches!(
                t,
                "-C" | "--git-dir"
                    | "--work-tree"
                    | "--namespace"
                    | "--super-prefix"
                    | "--attr-source"
            ) {
                j += 2;
            } else if t.starts_with('-') {
                j += 1;
            } else {
                break;
            }
        }
    }
    if prog == "git"
        && let Some((sub_idx, sub)) = git_subcommand(seg, i)
    {
        let arg = seg.get(sub_idx + 1).map(String::as_str);
        for t in &seg[sub_idx + 1..] {
            let t = t.as_str();
            if t == "--output"
                || t.starts_with("--output=")
                || t == "--ext-diff"
                || t == "--textconv"
                || t == "--filters"
                || t.starts_with("--open-files-in-pager")
                || (t.starts_with("-O") && !t.starts_with("--"))
                || (sub == "grep" && grep_opens_pager(t))
                || (sub == "cat-file"
                    && t.starts_with("--")
                    && (long_option_spells(t, "textconv") || long_option_spells(t, "filters")))
            {
                return Some(format!("git {sub} {t}"));
            }
            if t == "--help" {
                return Some(format!("git {sub} --help {GIT_HELP_RUNS}"));
            }
        }
        if sub == "remote" && git_remote_show_contacts(&seg[sub_idx + 1..]) {
            return Some("git remote show (contacts a remote)".to_string());
        }
        if GIT_DANGER.contains(&sub) {
            return Some(format!("git {sub}"));
        }
        if (sub == "stash" || sub == "worktree") && arg != Some("list") {
            return Some(
                format!("git {sub} {}", arg.unwrap_or(""))
                    .trim()
                    .to_string(),
            );
        }
    }
    None
}

/// Whether `git remote` with the words `args` after `remote` contacts a remote.
pub fn git_remote_show_contacts<S: AsRef<str>>(args: &[S]) -> bool {
    let mut operands = args
        .iter()
        .map(AsRef::as_ref)
        .filter(|a| !a.starts_with('-'));
    operands.next() == Some("show")
        && operands.next().is_some()
        && !args
            .iter()
            .any(|a| matches!(a.as_ref(), "-n" | "--no-query"))
}

fn argv_flag_hazard(cmd: &str) -> Option<String> {
    program_words(cmd)
        .iter()
        .find_map(|seg| seg_flag_hazard(seg))
}

fn seg_flag_hazard(seg: &[String]) -> Option<String> {
    for i in 0..seg.len() {
        if let Some(reason) = flag_hazard_at(seg, i) {
            return Some(reason);
        }
    }
    let Ok(Some(run)) = runs(seg) else {
        return None;
    };
    let head = program(&seg[run.at]);
    if head == "git"
        && let Some((sub_idx, sub)) = git_subcommand(seg, run.at)
        && GIT_READ.contains(&sub)
        && !seg[sub_idx + 1..].iter().any(|a| a.contains(SUBSTITUTION))
        && git_read_writes(sub, &seg[sub_idx + 1..])
    {
        return Some(format!("git {sub} (write form)"));
    }
    runs_or_writes_by_flag_reading(head, &seg[run.at + 1..], Reading::Argv)
}

const GIT_HELP_RUNS: &str = "(runs git help: the man or web viewer the config names)";

fn grep_opens_pager(t: &str) -> bool {
    if let Some(long) = t.strip_prefix("--") {
        let name = long.split('=').next().unwrap_or("");
        return !name.is_empty() && "open-files-in-pager".starts_with(name);
    }
    let Some(cluster) = t.strip_prefix('-') else {
        return false;
    };
    for c in cluster.chars() {
        match c {
            'O' => return true,
            'A' | 'B' | 'C' | 'e' | 'f' | 'm' => return false,
            _ => {}
        }
    }
    false
}

fn is_system_bin(tok: &str) -> bool {
    SYSTEM_BIN_DIRS.iter().any(|dir| {
        tok.strip_prefix(dir)
            .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
    })
}

fn is_system_search_path(value: &str) -> bool {
    value.split(':').all(|entry| {
        let entry = entry.trim_end_matches('/');
        SYSTEM_BIN_DIRS
            .iter()
            .any(|dir| dir.trim_end_matches('/') == entry)
    })
}

fn assignment_hazard(tok: &str) -> Option<String> {
    if !is_assignment(tok) {
        return None;
    }
    let (written, value) = tok.split_once('=')?;
    let name = ZSH_TIED
        .iter()
        .find(|(array, _)| *array == written)
        .map_or(written, |(_, scalar)| *scalar);
    if name == "PATH" {
        return (!is_system_search_path(value))
            .then(|| format!("{written}= (changes which program a name runs)"));
    }
    if ZSH_ARITHMETIC_VARS.contains(&name) && !plain_number(value) {
        return Some(format!(
            "{written}= (zsh evaluates the value as arithmetic, whose subscript can run a command)"
        ));
    }
    (HAZARD_VARS.contains(&name) || HAZARD_VAR_PREFIXES.iter().any(|p| name.starts_with(p)))
        .then(|| format!("{written}= (changes what a later program runs or reads)"))
}

fn loop_assignment_hazard(seg: &[String]) -> Option<String> {
    let j = skip_prefixes(seg, 0);
    if seg.get(j).map(String::as_str) != Some("for") {
        return None;
    }
    let names: Vec<&str> = seg[j + 1..]
        .iter()
        .map(String::as_str)
        .take_while(|t| is_name(t) && *t != "in")
        .collect();
    let after = j + 1 + names.len();
    let unreadable = [SUBSTITUTION.to_string()];
    let words = match seg.get(after).map(String::as_str) {
        Some("in") => &seg[after + 1..],
        _ => &unreadable[..],
    };
    names.iter().find_map(|name| {
        words
            .iter()
            .find_map(|w| assignment_hazard(&format!("{name}={w}")))
            .map(|why| format!("for {why}"))
    })
}

const ZSH_ARITHMETIC_VARS: &[&str] = &[
    "COLUMNS",
    "DIRSTACKSIZE",
    "EGID",
    "ERRNO",
    "EUID",
    "FUNCNEST",
    "GID",
    "HISTSIZE",
    "KEYTIMEOUT",
    "LINES",
    "LISTMAX",
    "MAILCHECK",
    "MBEGIN",
    "MEND",
    "OPTIND",
    "RANDOM",
    "REPORTMEMORY",
    "REPORTTIME",
    "SAVEHIST",
    "SECONDS",
    "SHLVL",
    "TRY_BLOCK_ERROR",
    "TRY_BLOCK_INTERRUPT",
    "UID",
    "ZLE_RPROMPT_INDENT",
];

/// Characters that trigger zsh filename globbing (`EXTENDED_GLOB` included).
pub const GLOB_CHARS: &[char] = &['*', '?', '[', '^', '#', '('];

fn expands_apart(t: &str) -> bool {
    t.contains('{') || t.contains(GLOB_CHARS) || t.chars().skip(1).any(|c| c == '~')
}

fn long_option_spells(t: &str, name: &str) -> bool {
    t.strip_prefix("--")
        .map(|r| r.split_once('=').map_or(r, |(n, _)| n))
        .is_some_and(|n| !n.is_empty() && name.starts_with(n))
}

const SELECTED_BY_AN_ARGUMENT: &[&str] = &[
    "sort", "tree", "sed", "uniq", "less", "more", "sysctl", "file", "date", "rg", "find", "test",
    "[", "trustc", "rustc",
];

fn has_expansion(tok: &str) -> bool {
    let b = tok.as_bytes();
    b.iter()
        .enumerate()
        .any(|(k, &c)| c == b'$' && (k == 0 || b[k - 1] != b'\\'))
}

fn expansion_in_an_option_slot<'a>(head: &str, args: &'a [String]) -> Option<&'a str> {
    if head == "test" || head == "[" {
        let ops = match args.split_last() {
            Some((last, rest)) if head == "[" && last == "]" => rest,
            _ => args,
        };
        if test_expansions_are_operands(ops) {
            return None;
        }
        return ops.iter().find(|t| has_expansion(t)).map(String::as_str);
    }
    let end = option_slots_end(head, args);
    args[..end]
        .iter()
        .find(|t| has_expansion(t))
        .or_else(|| {
            (head == "uniq")
                .then(|| args[end..].iter().find(|t| has_unquoted_expansion(t)))
                .flatten()
        })
        .map(String::as_str)
}

fn option_slots_end(head: &str, args: &[String]) -> usize {
    if matches!(head, "find" | "sysctl") {
        return args.len();
    }
    args.iter().position(|t| t == "--").unwrap_or(args.len())
}

fn glob_in_an_option_slot<'a>(head: &str, args: &'a [String]) -> Option<&'a str> {
    if matches!(head, "test" | "[" | "printf") {
        return None;
    }
    let any_operand = matches!(head, "uniq" | "sysctl");
    let end = if head == "uniq" {
        args.len()
    } else {
        option_slots_end(head, args)
    };
    args[..end]
        .iter()
        .find(|t| may_expand_into_a_flag(t) || (any_operand && expands_apart(&t.replace("{}", ""))))
        .map(String::as_str)
}

fn may_expand_into_a_flag(t: &str) -> bool {
    expands_apart(&t.replace("{}", ""))
        && !t.chars().next().is_some_and(|c| {
            c.is_alphanumeric() || matches!(c, '.' | '/' | '_' | '@' | '%' | ':' | ',' | '=' | '~')
        })
}

fn has_unquoted_expansion(tok: &str) -> bool {
    has_expansion(
        &tok.replace(QUOTED_SUBSTITUTION, "")
            .replace(QUOTED_PARAMETER, ""),
    )
}

fn test_expansions_are_operands(ops: &[String]) -> bool {
    if ops.iter().any(|t| has_unquoted_expansion(t)) {
        return false;
    }
    let is = |k: usize, set: &[&str]| {
        ops.get(k)
            .is_some_and(|t| !has_expansion(t) && set.contains(&t.as_str()))
    };
    match ops.len() {
        0 | 1 => true,
        2 => is(0, &["!"]) || is(0, TEST_UNARY),
        3 => is(1, TEST_BINARY) || (is(0, &["!"]) && is(1, TEST_UNARY)),
        _ => !ops.iter().any(|t| has_expansion(t)),
    }
}

/// Unary `test`/`[` primaries whose operand is a string or file.
pub const TEST_UNARY: &[&str] = &[
    "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-p", "-r", "-s", "-t", "-u", "-w", "-x", "-L",
    "-O", "-G", "-N", "-S", "-z", "-n",
];

/// Binary `test`/`[` primaries.
pub const TEST_BINARY: &[&str] = &[
    "=", "==", "!=", "<", ">", "-nt", "-ot", "-ef", "-eq", "-ne", "-gt", "-ge", "-lt", "-le",
];

fn runs_or_writes_by_flag(head: &str, args: &[String]) -> Option<String> {
    runs_or_writes_by_flag_reading(head, args, Reading::QuoteStripped)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    QuoteStripped,
    Argv,
}

fn runs_or_writes_by_flag_reading(head: &str, args: &[String], reading: Reading) -> Option<String> {
    let flag = |t: &str| format!("{head} {t}");
    let unreadable = |t: &str| format!("{head} {t} (a word this check cannot read may be a flag)");
    if reading == Reading::QuoteStripped
        && SELECTED_BY_AN_ARGUMENT.contains(&head)
        && let Some(t) = expansion_in_an_option_slot(head, args)
    {
        return Some(unreadable(t));
    }
    if reading == Reading::QuoteStripped
        && SELECTED_BY_AN_ARGUMENT.contains(&head)
        && let Some(t) = glob_in_an_option_slot(head, args)
    {
        return Some(format!(
            "{head} {t} (the shell may brace-expand or glob it into a flag or another operand)"
        ));
    }
    match head {
        // `--pre` and `--hostname-bin` (rg 14) run a program.
        "rg" => args
            .iter()
            .find(|t| {
                ["--pre", "--hostname-bin"]
                    .iter()
                    .any(|f| *t == f || t.starts_with(&format!("{f}=")))
            })
            .map(|t| flag(t)),
        "sort" => args
            .iter()
            .find(|t| {
                t.starts_with("--compress-program") || long_option_spells(t, "compress-program")
            })
            .map(|t| flag(t)),
        "printf" => {
            let format = args.iter().position(|t| !t.starts_with('-'));
            let opts = &args[..format.map_or(args.len(), |k| k + 1)];
            opts.iter()
                .find(|t| {
                    reading == Reading::QuoteStripped && (has_expansion(t) || expands_apart(t))
                })
                .map(|t| unreadable(t))
                .or_else(|| {
                    args.iter()
                        .take_while(|t| t.starts_with('-'))
                        .find(|t| t.starts_with("-v"))
                        .map(|t| flag(t))
                })
        }
        "test" => args
            .iter()
            .find(|t| *t == "-v")
            .map(|t| format!("test {t} (evaluates arithmetic)")),
        "less" | "more" => args
            .iter()
            .find(|t| {
                t.starts_with('+')
                    || t.starts_with("--log-file")
                    || t.starts_with("--LOG-FILE")
                    || t.starts_with("--lesskey")
                    || (t.starts_with('-')
                        && !t.starts_with("--")
                        && t[1..].chars().any(|c| matches!(c, 'o' | 'O' | 'k')))
            })
            .map(|t| flag(t)),
        "sysctl" => args
            .iter()
            .find(|t| {
                t.contains('=')
                    || (t.starts_with('-')
                        && !t.starts_with("--")
                        && (t[1..].contains('w') || t[1..].contains('f') || t[1..].contains('p')))
                    || *t == "--file"
                    || t.starts_with("--file=")
                    || *t == "--system"
            })
            .map(|t| format!("sysctl {t} (sets a kernel value)")),
        "hostname" => args
            .iter()
            .filter(|t| redirect_target(t).is_none())
            .find(|t| !t.starts_with('-') || t.starts_with("-F"))
            .map(|t| format!("hostname {t} (sets the host name)")),
        "file" => args
            .iter()
            .find(|t| {
                t.starts_with("--compile")
                    || (t.starts_with('-') && !t.starts_with("--") && t[1..].contains('C'))
            })
            .map(|t| format!("file {t} (writes a compiled magic file)")),
        "tree" => args
            .iter()
            .find(|t| t.starts_with('-') && !t.starts_with("--") && t[1..].contains('R'))
            .map(|t| format!("tree {t} (writes 00Tree.html files)")),
        "date" => {
            let mut k = 0;
            let mut positional = None;
            let mut no_set = false;
            while let Some(t) = args.get(k).map(String::as_str) {
                if let Some(target) = redirect_target(t) {
                    k += if target.is_none() { 2 } else { 1 };
                    continue;
                }
                if t == "-s" || t.starts_with("--set") {
                    return Some(flag(t));
                }
                if t == "-j" {
                    no_set = true;
                }
                if matches!(t, "-r" | "-f" | "-v" | "-d" | "-z") {
                    k += 2;
                    continue;
                }
                if !t.starts_with('-') && !t.starts_with('+') && positional.is_none() {
                    positional = Some(t);
                }
                k += 1;
            }
            positional
                .filter(|_| !no_set)
                .map(|t| format!("date {t} (sets the clock)"))
        }
        _ => None,
    }
}

fn is_assignment(tok: &str) -> bool {
    tok.find('=').is_some_and(|eq| is_name(&tok[..eq]))
}

fn is_name(tok: &str) -> bool {
    let mut cs = tok.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_duration(tok: &str) -> bool {
    let body = tok.strip_suffix(['s', 'm', 'h', 'd']).unwrap_or(tok);
    body.bytes().any(|b| b.is_ascii_digit())
        && body.bytes().filter(|&b| b == b'.').count() <= 1
        && body.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

fn is_fragment(head: &str) -> bool {
    head.starts_with('-') || head.starts_with('<') || redirect_target(head).is_some()
}

const XARGS_SHORT_VALUE: &[char] = &['n', 'I', 'L', 'P', 'd', 'E', 's', 'a', 'J', 'R', 'S'];

const XARGS_LONG_VALUE: &[&str] = &[
    "arg-file",
    "delimiter",
    "max-args",
    "max-chars",
    "max-procs",
    "process-slot-var",
];

fn xargs_command_at(seg: &[String], xargs_idx: usize, skip_redirects: bool) -> Option<usize> {
    let mut k = xargs_idx + 1;
    while k < seg.len() {
        let t = seg[k].as_str();
        if skip_redirects {
            let span = redirect_span(t);
            if span > 0 {
                k += span;
                continue;
            }
        }
        if !t.starts_with('-') || t == "-" {
            break;
        }
        if t == "--" {
            k += 1;
            break;
        }
        if t.starts_with("--") {
            k += if !t.contains('=')
                && XARGS_LONG_VALUE
                    .iter()
                    .any(|name| long_option_spells(t, name))
            {
                2
            } else {
                1
            };
            continue;
        }
        let mut step = 1;
        for (at, c) in t[1..].char_indices() {
            if XARGS_SHORT_VALUE.contains(&c) {
                if t[1 + at + c.len_utf8()..].is_empty() {
                    step = 2;
                }
                break;
            }
        }
        k += step;
    }
    (k < seg.len()).then_some(k)
}

fn segment_head<S: AsRef<str>>(seg: &[String], python_allow: &[S]) -> Option<String> {
    head_from(seg, 0, true, python_allow)
}

fn bare_redirection(stripped: &str) -> Option<String> {
    for (seg, at) in split_segments_at(stripped) {
        if at == SegAt::Command
            && matches!(runs(&seg), Ok(None))
            && has_input_redirect_with_target(&seg)
        {
            return Some(
                "a bare input redirection with no command (zsh runs $READNULLCMD)".to_string(),
            );
        }
    }
    None
}

fn has_input_redirect_with_target(seg: &[String]) -> bool {
    seg.iter().enumerate().any(|(k, t)| {
        let op = t.trim_start_matches(|c: char| c.is_ascii_digit());
        let Some(rest) = op.strip_prefix('<') else {
            return false;
        };
        if rest.starts_with('<') {
            return false;
        }
        let glued = rest.strip_prefix('&').unwrap_or(rest);
        !glued.is_empty() || seg.get(k + 1).is_some()
    })
}

fn redirect_span(tok: &str) -> usize {
    let op = tok.trim_start_matches(|c: char| c.is_ascii_digit());
    if !(op.starts_with(['<', '>']) || op.starts_with("&>")) {
        return 0;
    }
    if op.trim_start_matches(['<', '>', '&', '|']).is_empty() {
        2
    } else {
        1
    }
}

fn skip_prefixes(seg: &[String], j: usize) -> usize {
    skip_prefixes_at(seg, j, true).0
}

fn skip_prefixes_at(seg: &[String], mut j: usize, mut keyword: bool) -> (usize, bool) {
    loop {
        let start = j;
        while let Some(t) = seg.get(j).map(String::as_str) {
            if KEYWORD_PREFIX.contains(&t) {
                if !keyword {
                    return (j, false);
                }
                j += 1;
            } else if is_assignment(t) || t == "-" {
                keyword = false;
                j += 1;
            } else if redirect_span(t) > 0 {
                keyword = false;
                j += redirect_span(t);
            } else {
                break;
            }
        }
        match seg.get(j).map(String::as_str) {
            Some("timeout") => {
                j += 1;
                while j < seg.len() {
                    let t = seg[j].as_str();
                    let span = redirect_span(t);
                    if span > 0 {
                        j += span;
                        continue;
                    }
                    if !t.starts_with('-') {
                        break;
                    }
                    if t == "--" {
                        j += 1;
                        break;
                    }
                    if t.starts_with("--") {
                        j += if !t.contains('=')
                            && (long_option_spells(t, "kill-after")
                                || long_option_spells(t, "signal"))
                        {
                            2
                        } else {
                            1
                        };
                        continue;
                    }
                    let mut step = 1;
                    for (at, c) in t[1..].char_indices() {
                        if matches!(c, 'k' | 's') {
                            if t[1 + at + c.len_utf8()..].is_empty() {
                                step = 2;
                            }
                            break;
                        }
                    }
                    j += step;
                }
                while let Some(span) = seg.get(j).map(|t| redirect_span(t)).filter(|&n| n > 0) {
                    j += span;
                }
                if seg.get(j).is_some_and(|t| is_duration(t)) {
                    j += 1;
                }
                keyword = false;
            }
            Some("time") => {
                let at = j;
                j += 1;
                while seg.get(j).map(String::as_str) == Some("-p") {
                    j += 1;
                }
                if seg.get(j).is_some_and(|t| t.starts_with('-')) {
                    return (at, false);
                }
                keyword = false;
            }
            _ => {}
        }
        if j == start {
            break;
        }
    }
    (j, keyword)
}

const SHELL_RESERVED: &[&str] = &[
    "!", "case", "coproc", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if",
    "in", "select", "then", "until", "while", "{", "}", "[[", "]]",
];

fn wrapper(seg: &[String], j: usize) -> Option<Wrapped<'_>> {
    let tok = seg.get(j)?.as_str();
    if tok.contains('/') && !is_system_bin(tok) {
        return None;
    }
    match program(tok) {
        "xargs" => Some(Ok(xargs_command_at(seg, j, true).map(|k| (k, Vec::new())))),
        "env" => {
            let mut k = j + 1;
            let mut dirs = Vec::new();
            while k < seg.len() {
                let t = seg[k].as_str();
                let span = redirect_span(t);
                if span > 0 {
                    k += span;
                    continue;
                }
                if t == "--split-string"
                    || t.starts_with("--split-string=")
                    || long_option_spells(t, "split-string")
                {
                    return Some(Err("env -S".to_string()));
                }
                if t == "--argv0" || t.starts_with("--argv0=") || long_option_spells(t, "argv0") {
                    return Some(Err("env -a".to_string()));
                }
                if t == "--" {
                    k += 1;
                    break;
                }
                if let Some((_, dir)) = t.split_once('=').filter(|_| long_option_spells(t, "chdir"))
                {
                    dirs.push(dir);
                    k += 1;
                    continue;
                }
                if !t.contains('=')
                    && (long_option_spells(t, "chdir") || long_option_spells(t, "unset"))
                {
                    if long_option_spells(t, "chdir") {
                        let Some(dir) = seg.get(k + 1) else {
                            return Some(Err("env --chdir".to_string()));
                        };
                        dirs.push(dir.as_str());
                    }
                    k += 2;
                    continue;
                }
                if t.starts_with("--") {
                    k += 1;
                    continue;
                }
                if t.starts_with('-') && t.len() > 1 {
                    let mut width = 1;
                    for (at, c) in t.char_indices().skip(1) {
                        if c == 'S' {
                            return Some(Err("env -S".to_string()));
                        }
                        if c == 'a' {
                            return Some(Err("env -a".to_string()));
                        }
                        if !matches!(c, 'u' | 'C' | 'P') {
                            continue;
                        }
                        let glued = &t[at + c.len_utf8()..];
                        let value = if glued.is_empty() {
                            width = 2;
                            seg.get(k + 1).map(String::as_str)
                        } else {
                            Some(glued)
                        };
                        if c == 'P' && !value.is_some_and(is_system_search_path) {
                            return Some(Err(format!(
                                "env -P {} (looks the command up in a directory of the \
                                 line's choosing)",
                                value.unwrap_or("")
                            )));
                        }
                        if c == 'C' {
                            let Some(dir) = value else {
                                return Some(Err("env -C".to_string()));
                            };
                            dirs.push(dir);
                        }
                        break;
                    }
                    k += width;
                    continue;
                }
                if is_assignment(t) {
                    k += 1;
                    continue;
                }
                break;
            }
            Some(Ok((k < seg.len()).then_some((k, dirs))))
        }
        _ => None,
    }
}

type Wrapped<'s> = Result<Option<(usize, Vec<&'s str>)>, String>;

/// Resolved command execution target of a shell segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runs<'s> {
    /// Index of the command word within the segment.
    pub at: usize,
    /// Outer-to-inner `env -C`/`--chdir` directory arguments preceding the command.
    pub chdir: Vec<&'s str>,
    /// Whether `xargs` feeds additional arguments to the command.
    pub fed: bool,
}

/// Resolve the [`Runs`] target of segment `seg` past prefixes and wrappers.
pub fn runs(seg: &[String]) -> Result<Option<Runs<'_>>, String> {
    let mut j = 0;
    let mut keyword = true;
    let mut chdir = Vec::new();
    let mut fed = false;
    loop {
        j = skip_prefixes_at(seg, j, keyword).0;
        if j >= seg.len() {
            return Ok(None);
        }
        match wrapper(seg, j) {
            None => return Ok(Some(Runs { at: j, chdir, fed })),
            Some(Err(reason)) => return Err(reason),
            Some(Ok(None)) => return Ok(None),
            Some(Ok(Some((k, dirs)))) => {
                fed |= program(&seg[j]) == "xargs";
                chdir.extend(dirs);
                j = k;
                keyword = false;
            }
        }
    }
}

/// Parse `line` into per-segment shell words with quotes resolved.
pub fn command_words(line: &str) -> Result<Vec<Vec<String>>, String> {
    Ok(program_words(&strip_alarm_idiom(&prelex(line)?)))
}

/// The words of `line` that may name a path a command reads, for the secret check.
#[cfg(test)]
pub fn path_operands(line: &str) -> Result<Vec<String>, String> {
    path_operands_except(line, &|_| false)
}

/// The words of `line` that may name a path a command reads, for the secret check (the tests'
/// `path_operands`), where a segment `passed` holds, outside any substitution, names only its
/// redirects' targets: its caller knows its command reads no file its words name.
pub fn path_operands_except(
    line: &str,
    passed: &dyn Fn(&[String]) -> bool,
) -> Result<Vec<String>, String> {
    let segments = program_words_nested(&strip_alarm_idiom(&prelex(line)?));
    let feeds_paths = segments.iter().flat_map(|(seg, _)| seg).any(|w| {
        program(w) == "xargs" || w == "-files0-from" || long_option_spells(w, "files0-from")
    });
    let mut out = Vec::new();
    for (seg, nested) in &segments {
        if !nested && passed(seg) {
            redirect_paths(seg, &mut out);
        } else {
            segment_paths(seg, !nested && !feeds_paths, &mut out);
        }
    }
    Ok(out)
}

/// Every word [`path_operands_except`] may read off the segments of `line` (nested ones among
/// them) whose program `of` holds (`None` for a segment whose program the walk finds none of),
/// each read as a segment whose command reads every word: for the secret check, which meets a
/// glob such a program takes by that program's own rules.
pub fn path_operands_of(
    line: &str,
    of: &dyn Fn(Option<&str>) -> bool,
) -> Result<Vec<String>, String> {
    let segments = program_words_nested(&strip_alarm_idiom(&prelex(line)?));
    let mut out = Vec::new();
    for (seg, _) in &segments {
        let head = runs(seg).ok().flatten().map(|r| program(&seg[r.at]));
        if of(head) {
            segment_paths(seg, false, &mut out);
        }
    }
    Ok(out)
}

fn redirect_word(w: &str) -> Option<(&str, bool)> {
    let op = w.trim_start_matches(|c: char| c.is_ascii_digit());
    let op = op
        .strip_prefix('&')
        .filter(|r| r.starts_with('>'))
        .unwrap_or(op);
    if !op.starts_with(['<', '>']) {
        return None;
    }
    Some((
        op.trim_start_matches(['<', '>', '&', '|']),
        op.starts_with("<<"),
    ))
}

/// The files `seg`'s redirects name, pushed onto `out` (a here-document's or here-string's text
/// is none); which of `seg`'s words a redirect took, by index.
fn redirect_paths(seg: &[String], out: &mut Vec<String>) -> Vec<bool> {
    let mut taken = vec![false; seg.len()];
    let mut k = 0;
    while k < seg.len() {
        if let Some((target, text)) = redirect_word(&seg[k]) {
            taken[k] = true;
            if target.is_empty() {
                if let Some(next) = seg.get(k + 1) {
                    taken[k + 1] = true;
                    if !text {
                        out.push(next.clone());
                    }
                    k += 1;
                }
            } else if !text {
                out.push(target.to_string());
            }
        }
        k += 1;
    }
    taken
}

fn segment_paths(seg: &[String], exempt: bool, out: &mut Vec<String>) {
    let taken = redirect_paths(seg, out);
    let words = |from: usize, to: usize| {
        (from..to)
            .filter(|k| !taken[*k])
            .map(|k| seg[k].as_str())
            .collect::<Vec<&str>>()
    };
    let at = match runs(seg) {
        Ok(Some(r)) if !taken[r.at] => r.at,
        _ => {
            words(0, seg.len())
                .into_iter()
                .for_each(|w| push_word(w, out));
            return;
        }
    };
    words(0, at).into_iter().for_each(|w| push_word(w, out));
    let head = program(&seg[at]);
    if head == "git" {
        let start = out.len();
        match git_subcommand(seg, at) {
            Some((sub_idx, sub)) if exempt => {
                words(at + 1, sub_idx)
                    .into_iter()
                    .for_each(|w| push_word(w, out));
                let args = words(sub_idx + 1, seg.len());
                match sub {
                    "grep" => grammar_paths(&GIT_GREP, &args, out),
                    "log" | "show" | "shortlog" | "rev-list" | "whatchanged" => {
                        grammar_paths(&GIT_LOG, &args, out);
                    }
                    "ls-files" => {}
                    _ => args.into_iter().for_each(|w| push_word(w, out)),
                }
            }
            _ => words(at + 1, seg.len())
                .into_iter()
                .for_each(|w| push_word(w, out)),
        }
        // A git word holding a `:` is read whole and as the path past it ([`git_tree_path`]),
        // but a pathspec's closed long magic (`:(icase)src`) as that path alone: its `(` is
        // git's, never a glob the shell expands. Read whole, its other readings (a file of that
        // name under `--literal-pathspecs` or `GIT_LITERAL_PATHSPECS`, each of which the secret
        // and git checks refuse, or the index's entry `(icase)src` as a revision) name a path
        // opening with `(magic)`, which meets a rule only where the path past it does, as no
        // default rule's name holds `(` or `)`.
        let mut in_trees = Vec::new();
        for w in &mut out[start..] {
            let Some(path) = git_tree_path(w)
                .map(|(p, _)| p.to_string())
                .filter(|p| !p.is_empty())
            else {
                continue;
            };
            if w.strip_prefix(":(")
                .is_some_and(|magic| magic.contains(')'))
            {
                *w = path;
            } else {
                in_trees.push(path);
            }
        }
        out.extend(in_trees);
        return;
    }
    let args = words(at + 1, seg.len());
    if matches!(head, "sed" | "gsed") {
        out.extend(sed_reads(&args));
    }
    if !exempt {
        args.into_iter().for_each(|w| push_word(w, out));
        return;
    }
    match head {
        "echo" | "printf" | "which" | "type" | "true" | "sleep" | "seq" | "expr" | "pwd"
        | "uptime" | "uname" | "whoami" | "id" | "sw_vers" | "ps" | "basename" | "dirname"
        | "test" | "[" | "[[" | "sysctl" | "mdfind" | "realpath" | "readlink" | "ls" | "stat"
        | "df" => {}
        "find"
            if !args.iter().any(|a| {
                matches!(*a, "-exec" | "-execdir" | "-ok" | "-okdir" | "-files0-from")
            }) => {}
        "grep" | "egrep" | "fgrep" => grammar_paths(&GREP, &args, out),
        "rg" => grammar_paths(&RG, &args, out),
        "jq" => grammar_paths(&JQ, &args, out),
        "awk" => grammar_paths(&AWK, &args, out),
        "pgrep" => grammar_paths(&PGREP, &args, out),
        "wc" | "du" => grammar_paths(&MEASURE, &args, out),
        _ => args.into_iter().for_each(|w| push_word(w, out)),
    }
}

fn push_word(w: &str, out: &mut Vec<String>) {
    match w.split_once('=') {
        Some((_, v)) if w.starts_with('-') => out.push(v.to_string()),
        Some((_, v)) => {
            out.push(w.to_string());
            out.push(v.to_string());
        }
        None if w.starts_with("--") => {}
        None if w.starts_with('-') => {
            out.extend(w.char_indices().skip(2).map(|(i, _)| w[i..].to_string()))
        }
        None => out.push(w.to_string()),
    }
}

/// Push the paths an awk `program` (`imports` false) or a jq filter (`imports` true) may read:
/// awk's every string literal, and [`SUBSTITUTION`], a word the secret check refuses as built at
/// run time, where the program names a file it reads at run time ([`awk_names_files_at_run_time`]);
/// jq's every `import` or `include` path, also with `.json`.
fn push_literals(program: &str, imports: bool, out: &mut Vec<String>) {
    if !imports && awk_names_files_at_run_time(program) {
        out.push(SUBSTITUTION.to_string());
    }
    let cs: Vec<char> = program.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '"' {
            let before: String = cs[..i].iter().collect();
            let before = before.trim_end();
            let named = before.ends_with("import") || before.ends_with("include");
            let mut lit = String::new();
            i += 1;
            while i < cs.len() && cs[i] != '"' {
                if cs[i] == '\\' {
                    lit.extend(cs.get(i + 1));
                    i += 2;
                    continue;
                }
                lit.push(cs[i]);
                i += 1;
            }
            if !imports {
                out.push(lit);
            } else if named {
                out.push(format!("{lit}.json"));
                out.push(lit);
            }
        }
        i += 1;
    }
}

/// Whether the awk `program` may read a file whose name it builds at run time: it names `ARGV`,
/// whose elements name the files it reads after its `BEGIN`, or a `getline`'s `<` (the first
/// before the statement ends) takes other than one string literal standing alone (`getline l <
/// f`, `getline < "/a" "b"`, `getline < ("/a")`).
fn awk_names_files_at_run_time(program: &str) -> bool {
    if names_awk_word(program, "ARGV") {
        return true;
    }
    let cs: Vec<char> = program.chars().collect();
    let blank = |j: &mut usize| {
        while cs.get(*j).is_some_and(|c| *c == ' ' || *c == '\t') {
            *j += 1;
        }
    };
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let word: Vec<char> = "getline".chars().collect();
    let mut k = 0;
    while k + word.len() <= cs.len() {
        let whole = cs[k..k + word.len()] == word[..]
            && !(k > 0 && ident(cs[k - 1]))
            && !cs.get(k + word.len()).is_some_and(|c| ident(*c));
        if !whole {
            k += 1;
            continue;
        }
        k += word.len();
        let mut j = k;
        while j < cs.len() && !matches!(cs[j], '<' | ';' | '\n' | '{' | '}') {
            j += 1;
        }
        if cs.get(j) != Some(&'<') {
            continue;
        }
        j += 1;
        blank(&mut j);
        if cs.get(j) != Some(&'"') {
            return true;
        }
        j += 1;
        while j < cs.len() && cs[j] != '"' {
            j += if cs[j] == '\\' { 2 } else { 1 };
        }
        j += 1;
        blank(&mut j);
        if !cs
            .get(j)
            .is_none_or(|c| matches!(c, ')' | ';' | '\n' | '}' | '>'))
        {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Val {
    Text,
    Path,
    Pattern,
    Program,
    PatternFile,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum First {
    Operand,
    Pattern,
    Program,
    Filter,
}

struct Grammar {
    short: &'static [(char, Val)],
    long: &'static [(&'static str, &'static [Val])],
    flags: &'static [&'static str],
    /// jq, rg and git log require whole long-option names. Prefix matching
    /// would turn flags such as --slurp and --ignore into value options.
    abbreviates: bool,
    first: First,
    operands_read: bool,
}

const GREP: Grammar = Grammar {
    short: &[
        ('e', Val::Pattern),
        ('f', Val::PatternFile),
        ('A', Val::Text),
        ('B', Val::Text),
        ('C', Val::Text),
        ('D', Val::Text),
        ('d', Val::Text),
        ('m', Val::Text),
    ],
    long: &[
        ("regexp", &[Val::Pattern]),
        ("file", &[Val::PatternFile]),
        ("include", &[Val::Text]),
        ("exclude", &[Val::Text]),
        ("exclude-dir", &[Val::Text]),
        ("include-dir", &[Val::Text]),
        ("exclude-from", &[Val::Path]),
        ("after-context", &[Val::Text]),
        ("before-context", &[Val::Text]),
        ("max-count", &[Val::Text]),
        ("devices", &[Val::Text]),
        ("directories", &[Val::Text]),
        ("label", &[Val::Text]),
        ("binary-files", &[Val::Text]),
        ("group-separator", &[Val::Text]),
    ],
    flags: &["binary"],
    abbreviates: true,
    first: First::Pattern,
    operands_read: true,
};

const RG: Grammar = Grammar {
    short: &[
        ('e', Val::Pattern),
        ('f', Val::PatternFile),
        ('g', Val::Text),
        ('t', Val::Text),
        ('T', Val::Text),
        ('A', Val::Text),
        ('B', Val::Text),
        ('C', Val::Text),
        ('d', Val::Text),
        ('E', Val::Text),
        ('j', Val::Text),
        ('M', Val::Text),
        ('m', Val::Text),
        ('r', Val::Text),
    ],
    long: &[
        ("regexp", &[Val::Pattern]),
        ("file", &[Val::PatternFile]),
        ("glob", &[Val::Text]),
        ("iglob", &[Val::Text]),
        ("type", &[Val::Text]),
        ("type-not", &[Val::Text]),
        ("type-add", &[Val::Text]),
        ("type-clear", &[Val::Text]),
        ("ignore-file", &[Val::Path]),
        ("after-context", &[Val::Text]),
        ("before-context", &[Val::Text]),
        ("context", &[Val::Text]),
        ("max-count", &[Val::Text]),
        ("max-columns", &[Val::Text]),
        ("max-depth", &[Val::Text]),
        ("maxdepth", &[Val::Text]),
        ("max-filesize", &[Val::Text]),
        ("threads", &[Val::Text]),
        ("encoding", &[Val::Text]),
        ("engine", &[Val::Text]),
        ("replace", &[Val::Text]),
        ("color", &[Val::Text]),
        ("colors", &[Val::Text]),
        ("context-separator", &[Val::Text]),
        ("field-context-separator", &[Val::Text]),
        ("field-match-separator", &[Val::Text]),
        ("path-separator", &[Val::Text]),
        ("sort", &[Val::Text]),
        ("sortr", &[Val::Text]),
        ("dfa-size-limit", &[Val::Text]),
        ("regex-size-limit", &[Val::Text]),
        ("hyperlink-format", &[Val::Text]),
        ("generate", &[Val::Text]),
        ("pre-glob", &[Val::Text]),
        ("pre", &[Val::Path]),
        ("hostname-bin", &[Val::Path]),
    ],
    flags: &[],
    abbreviates: false,
    first: First::Pattern,
    operands_read: true,
};

const JQ: Grammar = Grammar {
    short: &[('f', Val::PatternFile), ('L', Val::Path)],
    long: &[
        ("from-file", &[Val::PatternFile]),
        ("arg", &[Val::Text, Val::Text]),
        ("argjson", &[Val::Text, Val::Text]),
        ("slurpfile", &[Val::Text, Val::Path]),
        ("rawfile", &[Val::Text, Val::Path]),
        ("indent", &[Val::Text]),
    ],
    flags: &[],
    abbreviates: false,
    first: First::Filter,
    operands_read: true,
};

/// gawk's options (`abbreviates`: its `getopt_long`, REASONED from its manual). macOS awk ignores
/// every long option, so one reaches this grammar only with its value glued (`--assign=N=P`): a
/// line the two read otherwise is refused ([`bwk_reads_otherwise`]).
const AWK: Grammar = Grammar {
    short: &[
        ('F', Val::Text),
        ('v', Val::Path),
        ('f', Val::PatternFile),
        ('e', Val::Program),
        ('E', Val::PatternFile),
        ('i', Val::Path),
    ],
    long: &[
        ("file", &[Val::PatternFile]),
        ("source", &[Val::Program]),
        ("exec", &[Val::PatternFile]),
        ("include", &[Val::Path]),
        ("assign", &[Val::Path]),
        ("field-separator", &[Val::Text]),
    ],
    flags: &[],
    abbreviates: true,
    first: First::Program,
    operands_read: true,
};

const PGREP: Grammar = Grammar {
    short: &[('F', Val::Path)],
    long: &[("pidfile", &[Val::Path])],
    flags: &[],
    abbreviates: true,
    first: First::Pattern,
    operands_read: true,
};

const MEASURE: Grammar = Grammar {
    short: &[('X', Val::Path)],
    long: &[
        ("files0-from", &[Val::Path]),
        ("exclude-from", &[Val::Path]),
    ],
    flags: &[],
    abbreviates: true,
    first: First::Operand,
    operands_read: false,
};

const GIT_GREP: Grammar = Grammar {
    short: &[
        ('e', Val::Pattern),
        ('f', Val::PatternFile),
        ('A', Val::Text),
        ('B', Val::Text),
        ('C', Val::Text),
        ('m', Val::Text),
    ],
    long: &[
        ("after-context", &[Val::Text]),
        ("before-context", &[Val::Text]),
        ("context", &[Val::Text]),
        ("max-count", &[Val::Text]),
        ("max-depth", &[Val::Text]),
        ("threads", &[Val::Text]),
    ],
    flags: &[],
    abbreviates: true,
    first: First::Pattern,
    operands_read: true,
};

const GIT_LOG: Grammar = Grammar {
    short: &[('S', Val::Text), ('G', Val::Text), ('L', Val::Path)],
    long: &[
        ("grep", &[Val::Text]),
        ("author", &[Val::Text]),
        ("committer", &[Val::Text]),
        ("grep-reflog", &[Val::Text]),
    ],
    flags: &[],
    abbreviates: false,
    first: First::Operand,
    operands_read: true,
};

fn option_values<'a>(g: &Grammar, a: &'a str) -> (Vec<Val>, Option<&'a str>) {
    if let Some(body) = a.strip_prefix("--") {
        let (name, glued) = body
            .split_once('=')
            .map_or((body, None), |(n, v)| (n, Some(v)));
        let listed = g.long.iter().find(|(n, _)| *n == name).or_else(|| {
            g.long.iter().find(|(n, _)| {
                g.abbreviates && !name.is_empty() && n.starts_with(name) && !g.flags.contains(&name)
            })
        });
        return match listed {
            Some((_, kinds)) => (kinds.to_vec(), glued),
            None => (glued.map_or_else(Vec::new, |_| vec![Val::Path]), glued),
        };
    }
    let cluster = &a[1..];
    for (at, c) in cluster.char_indices() {
        if let Some((_, kind)) = g.short.iter().find(|(s, _)| *s == c) {
            let rest = &cluster[at + c.len_utf8()..];
            return (vec![*kind], (!rest.is_empty()).then_some(rest));
        }
    }
    (Vec::new(), None)
}

fn push_value(kind: Val, v: &str, out: &mut Vec<String>) {
    match kind {
        Val::Text | Val::Pattern => {}
        Val::Path | Val::PatternFile => out.push(v.to_string()),
        Val::Program => push_literals(v, false, out),
    }
}

/// A word [`grammar_walk`] reads: an option's value of a kind, or an operand where the grammar
/// takes the next one as `First` (its pattern, program or filter first, then operands).
enum GrammarWord<'a> {
    Value(Val, &'a str),
    Operand(First, &'a str),
}

fn grammar_walk<'a>(g: &Grammar, args: &[&'a str], mut each: impl FnMut(GrammarWord<'a>)) {
    let options_end = args.iter().position(|a| *a == "--").unwrap_or(args.len());
    let is_option = |k: usize, a: &str| k < options_end && a.starts_with('-') && a.len() > 1;
    let supplied = args.iter().enumerate().any(|(k, a)| {
        is_option(k, a)
            && option_values(g, a)
                .0
                .iter()
                .any(|v| matches!(v, Val::Pattern | Val::Program | Val::PatternFile))
    });
    let mut first = if supplied { First::Operand } else { g.first };
    let mut seen_operand = false;
    let mut k = 0;
    while k < args.len() {
        let a = args[k];
        k += 1;
        if is_option(k - 1, a) {
            let (kinds, glued) = option_values(g, a);
            let mut kinds = kinds.into_iter();
            if let Some(v) = glued
                && let Some(kind) = kinds.next()
            {
                each(GrammarWord::Value(kind, v));
            }
            for kind in kinds {
                let Some(&next) = args.get(k) else { break };
                each(GrammarWord::Value(
                    if seen_operand { Val::Path } else { kind },
                    next,
                ));
                k += 1;
            }
            continue;
        }
        if k - 1 == options_end {
            continue;
        }
        each(GrammarWord::Operand(
            std::mem::replace(&mut first, First::Operand),
            a,
        ));
        seen_operand = true;
    }
}

fn grammar_paths(g: &Grammar, args: &[&str], out: &mut Vec<String>) {
    grammar_walk(g, args, |w| match w {
        GrammarWord::Value(kind, v) => push_value(kind, v, out),
        GrammarWord::Operand(First::Program, a) => push_literals(a, false, out),
        GrammarWord::Operand(First::Filter, a) => push_literals(a, true, out),
        GrammarWord::Operand(First::Operand, a) if g.operands_read => out.push(a.to_string()),
        GrammarWord::Operand(..) => {}
    });
}

/// git grep's words `args` (those after `grep`, no redirect among them), read by its grammar: the
/// operands before its `--` past its pattern (none taken for the pattern when an option supplies
/// it), and the words past its `--` (`None` with no `--`).
pub fn git_grep_operands<'a>(args: &[&'a str]) -> (Vec<&'a str>, Option<Vec<&'a str>>) {
    let end = args.iter().position(|a| *a == "--");
    let mut operands = Vec::new();
    grammar_walk(&GIT_GREP, &args[..end.unwrap_or(args.len())], |w| {
        if let GrammarWord::Operand(First::Operand, a) = w {
            operands.push(a);
        }
    });
    (operands, end.map(|k| args[k + 1..].to_vec()))
}

/// The operands of git log's grammar (`log`, `show`) in `args` (those after the subcommand, no
/// redirect among them), an option's value none of them: those before its `--` (revisions, or
/// paths), and the words past it.
pub fn git_log_operands<'a>(args: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    let end = args.iter().position(|a| *a == "--");
    let mut operands = Vec::new();
    grammar_walk(&GIT_LOG, &args[..end.unwrap_or(args.len())], |w| {
        if let GrammarWord::Operand(_, a) = w {
            operands.push(a);
        }
    });
    (
        operands,
        end.map_or_else(Vec::new, |k| args[k + 1..].to_vec()),
    )
}

/// The path a git word `w` names in a tree, the index or a pathspec, past what leads it, and
/// whether git reads that path from its repository's top: `REV:path` (from the top unless led by
/// `./` or `../`, MEASURED 2026-11-05 with git 2.56.0: `git show HEAD:x` run in a subdirectory
/// printed the top's `x`), `:N:path` and `:path` (the index, from the top), `:(magic)path` (from
/// the top under its `top` magic), and `:` with the short magic `/`, `!` or `^` before `path`,
/// a `:` ending it optional (from the top under `/`). `None` for a word holding no `:`. A long
/// magic that never closes is read whole past its `:`, where its `(` may name any name.
pub fn git_tree_path(w: &str) -> Option<(&str, bool)> {
    let (lead, path) = w.split_once(':')?;
    if !lead.is_empty() {
        return Some((path, !(path.starts_with("./") || path.starts_with("../"))));
    }
    if let Some(long) = path.strip_prefix('(') {
        return Some(match long.split_once(')') {
            Some((magic, rest)) => (rest, magic.split(',').any(|m| m == "top")),
            None => (path, false),
        });
    }
    if let Some((stage, rest)) = path.split_once(':')
        && matches!(stage, "0" | "1" | "2" | "3")
    {
        return Some((rest, true));
    }
    let n = path
        .find(|c| !matches!(c, '/' | '!' | '^'))
        .unwrap_or(path.len());
    let rest = &path[n..];
    if n == 0 && !rest.starts_with(':') {
        return Some((rest, true));
    }
    Some((
        rest.strip_prefix(':').unwrap_or(rest),
        path[..n].contains('/'),
    ))
}

/// Whether the grep-family or rg command at `seg[at]` names a file or tree to read, by its
/// grammar: an operand past its pattern (none when an option supplies the pattern), a word a
/// redirect takes not counted. `false` for any other program.
pub fn grammar_names_operand(seg: &[String], at: usize) -> bool {
    let g = match program(&seg[at]) {
        "grep" | "egrep" | "fgrep" => &GREP,
        "rg" => &RG,
        _ => return false,
    };
    let args = unredirected(seg, at + 1);
    let mut named = false;
    grammar_walk(g, &args, |w| {
        named |= matches!(w, GrammarWord::Operand(First::Operand, _));
    });
    named
}

/// `seg`'s words past `from` that no redirect takes (its operator and its target).
pub fn unredirected(seg: &[String], from: usize) -> Vec<&str> {
    let taken = redirect_paths(seg, &mut Vec::new());
    (from..seg.len())
        .filter(|k| !taken[*k])
        .map(|k| seg[k].as_str())
        .collect()
}

fn git_read_writes(sub: &str, args: &[String]) -> bool {
    let has = |flags: &[&str]| {
        args.iter().any(|a| {
            flags
                .iter()
                .any(|f| a == f || (f.starts_with("--") && a.starts_with(&format!("{f}="))))
        })
    };
    let has_write = |flags: &[&str]| {
        has(flags)
            || args.iter().any(|a| {
                flags
                    .iter()
                    .filter_map(|f| f.strip_prefix("--"))
                    .any(|name| long_option_spells(a, name))
            })
    };
    match sub {
        "branch" => {
            let short = |a: &str| a.starts_with('-') && !a.starts_with("--") && a.len() > 1;
            let write_short = args
                .iter()
                .any(|a| short(a) && a[1..].chars().any(|c| "dDmMcCuft".contains(c)));
            let list_short = args
                .iter()
                .any(|a| short(a) && a[1..].chars().any(|c| "arl".contains(c)));
            let list_long = has(&[
                "--list",
                "--all",
                "--remotes",
                "--contains",
                "--no-contains",
                "--merged",
                "--no-merged",
                "--points-at",
                "--show-current",
            ]);
            let positional = args.iter().any(|a| !a.starts_with('-'));
            write_short
                || has_write(&[
                    "--delete",
                    "--move",
                    "--copy",
                    "--force",
                    "--track",
                    "--no-track",
                    "--set-upstream",
                    "--set-upstream-to",
                    "--unset-upstream",
                    "--edit-description",
                    "--create-reflog",
                ])
                || (positional && !list_short && !list_long)
        }
        "tag" => {
            has_write(&["-d", "-a", "-s", "-f", "-m", "--delete", "-F"])
                || (args.iter().any(|a| !a.starts_with('-'))
                    && !has(&[
                        "-l",
                        "--list",
                        "--contains",
                        "--no-contains",
                        "--points-at",
                        "--merged",
                        "--no-merged",
                        "-v",
                        "--verify",
                        "-n",
                    ]))
        }
        "remote" => matches!(
            args.iter()
                .map(String::as_str)
                .find(|a| !a.starts_with('-')),
            Some(
                "add"
                    | "remove"
                    | "rm"
                    | "rename"
                    | "set-url"
                    | "set-head"
                    | "set-branches"
                    | "prune"
                    | "update"
            )
        ),
        "config" => {
            has_write(&[
                "--unset",
                "--unset-all",
                "--add",
                "--replace-all",
                "--edit",
                "-e",
                "--remove-section",
                "--rename-section",
            ]) || args.iter().filter(|a| !a.starts_with('-')).count() >= 2
        }
        _ => false,
    }
}

fn head_from<S: AsRef<str>>(
    seg: &[String],
    j: usize,
    keyword: bool,
    python_allow: &[S],
) -> Option<String> {
    let (j, keyword) = skip_prefixes_at(seg, j, keyword);
    let head_tok = seg.get(j)?;
    if !keyword && SHELL_RESERVED.contains(&head_tok.as_str()) {
        return Some(format!(
            "`{head_tok}` where the shell reads a command's name (behind an assignment, a \
             redirect, a wrapper or `time` it is no keyword: a program named `{head_tok}` runs)"
        ));
    }
    if keyword && is_fragment(head_tok) {
        return None;
    }
    if head_tok.contains('$') {
        return Some(format!(
            "a command whose name this check cannot read ({head_tok})"
        ));
    }
    if head_tok.contains('/') && !is_system_bin(head_tok) {
        return Some(format!("{head_tok} (a program named by path)"));
    }
    let head = program(head_tok);
    let rest = &seg[j + 1..];
    let arg = rest.first().map(String::as_str);
    if let Some(reason) = runs_or_writes_by_flag(head, rest) {
        return Some(reason);
    }
    match head {
        "aterm" | "aterm-ctl" | "aterm-drive" => aterm_read(head, rest),
        "trustc" | "rustc" => {
            let query = !rest.is_empty()
                && rest.iter().enumerate().all(|(k, t)| {
                    matches!(t.as_str(), "-vV" | "-Vv" | "-V" | "-v" | "--version")
                        || t == "--print"
                        || t.starts_with("--print=")
                        || (k > 0 && rest[k - 1] == "--print" && !t.starts_with('-'))
                });
            (!query).then(|| format!("{head} (compiles)"))
        }
        "xargs" | "env" => match wrapper(seg, j) {
            Some(Err(reason)) => Some(reason),
            Some(Ok(Some((k, _)))) => head_from(seg, k, false, python_allow),
            _ => None,
        },
        "git" => {
            let Some((sub_idx, sub)) = git_subcommand(seg, j) else {
                return Some("git without a subcommand".to_string());
            };
            let next = seg.get(sub_idx + 1).map(String::as_str);
            if !GIT_READ.contains(&sub) {
                return Some(format!("git {sub}"));
            }
            if (sub == "worktree" || sub == "stash") && next != Some("list") {
                return Some(
                    format!("git {sub} {}", next.unwrap_or(""))
                        .trim()
                        .to_string(),
                );
            }
            let args = &seg[sub_idx + 1..];
            let before_dashes = args.iter().position(|a| a == "--").unwrap_or(args.len());
            if let Some(t) = args[..before_dashes].iter().find(|a| has_expansion(a)) {
                return Some(format!(
                    "git {sub} {t} (a word this check cannot read may be a flag or a verb)"
                ));
            }
            let operand_counts = matches!(sub, "config" | "remote");
            if let Some(t) = seg[j + 1..sub_idx]
                .iter()
                .find(|a| expands_apart(a))
                .or_else(|| {
                    args[..before_dashes]
                        .iter()
                        .find(|a| may_expand_into_a_flag(a) || (operand_counts && expands_apart(a)))
                })
            {
                return Some(format!(
                    "git {sub} {t} (the shell may brace-expand or glob it into a flag or a verb)"
                ));
            }
            if sub == "config"
                && let Some(t) = args[before_dashes..]
                    .iter()
                    .find(|a| has_unquoted_expansion(a) || expands_apart(a))
            {
                return Some(format!("git config {t} (may split into a set)"));
            }
            if git_read_writes(sub, args) {
                return Some(format!("git {sub} (write form)"));
            }
            None
        }
        "export" => {
            let mut k = 0;
            while let Some(t) = rest.get(k).map(String::as_str) {
                let span = redirect_span(t);
                if span > 0 {
                    k += span;
                    continue;
                }
                if !(t.starts_with('-') || is_assignment(t) || is_name(t)) {
                    return Some(format!("export {t} (an assignment this check cannot read)"));
                }
                if t.starts_with('-') && t.contains(['i', 'E', 'F']) {
                    return Some(format!(
                        "export {t} (zsh evaluates the values as arithmetic)"
                    ));
                }
                k += 1;
            }
            None
        }
        "tmutil" => match arg {
            Some(v) if TMUTIL_READ.contains(&v) => None,
            Some(v) => Some(format!("tmutil {v}")),
            None => Some("tmutil without a verb".to_string()),
        },
        "python" | "python2" | "python3" => match arg {
            Some(path) if !path.starts_with('-') => {
                if path.contains('$') {
                    Some(format!(
                        "{head} {path}: a script path this check cannot read"
                    ))
                } else if path.split('/').any(|c| c == "..") {
                    Some(format!(
                        "{head} {path} steps out of its directory with `..`"
                    ))
                } else if python_allow.iter().any(|g| glob_match(g.as_ref(), path)) {
                    None
                } else {
                    Some(format!(
                        "{head} {path} is not on the read-only script allowlist"
                    ))
                }
            }
            Some(flag) => Some(format!("{head} {flag}")),
            None => Some(format!("{head} without a script")),
        },
        h if READ_ONLY.contains(&h) => None,
        h => Some(h.to_string()),
    }
}

const ATERM_CTL_READ: &[&str] = &[
    "status",
    "text",
    "screen",
    "line",
    "lines",
    "offscreen",
    "cell",
    "cursor",
    "dims",
    "modes",
    "title",
    "cwd",
    "colors",
    "search",
    "blocks",
    "blocktext",
    "metrics",
    "timeline",
    "log",
    "brief",
    "resizes",
    "history",
    "result",
    "survey",
    "panes",
    "ready",
    "await",
    "wait",
    "family",
    "edges",
    "grants",
    "ls",
    "windows",
    "sessions",
    "help",
    "version",
    "who",
    "whoami",
];

const ATERM_PKG_READ: &[&str] = &["list", "which", "doctor"];
const ATERM_DRIVE_READ: &[&str] = &["phase", "classify", "help", "-h", "--help"];

fn aterm_read(head: &str, rest: &[String]) -> Option<String> {
    let args: &[String] = match head {
        "aterm-ctl" => return aterm_ctl_read(rest),
        "aterm-drive" => return aterm_drive_read("aterm-drive", rest),
        _ => rest,
    };
    let sub = args.first().map(String::as_str);
    let verb = args.get(1).map(String::as_str);
    match sub {
        Some("--version" | "-V" | "version" | "help" | "--help" | "-h") => None,
        Some("ctl") => aterm_ctl_read(&args[1..]),
        Some("pkg") if verb.is_some_and(|v| ATERM_PKG_READ.contains(&v)) => None,
        Some("drive") => aterm_drive_read("aterm drive", &args[1..]),
        Some(s) => Some(
            format!("aterm {s} {}", verb.unwrap_or(""))
                .trim()
                .to_string(),
        ),
        None => Some("aterm (opens a session)".to_string()),
    }
}

fn aterm_drive_read(head: &str, args: &[String]) -> Option<String> {
    let mut k = 0;
    while let Some(t) = args.get(k).map(String::as_str) {
        match t {
            "--socket" | "--sock" => k += 2,
            _ => break,
        }
    }
    let verb = args.get(k).map(String::as_str);
    let writes_out = || {
        args[k..]
            .iter()
            .any(|t| t == "--out" || t.starts_with("--out="))
    };
    match verb {
        Some(v) if ATERM_DRIVE_READ.contains(&v) => None,
        Some("ledger") if !writes_out() => None,
        Some("ledger") => Some(format!("{head} ledger --out (writes a file)")),
        v => Some(format!("{head} {}", v.unwrap_or("")).trim().to_string()),
    }
}

fn aterm_ctl_read(args: &[String]) -> Option<String> {
    let mut k = 0;
    while let Some(t) = args.get(k).map(String::as_str) {
        match t {
            "--sock" | "--pid" | "--timeout" => k += 2,
            "-h" | "--help" | "-V" | "--version" => return None,
            _ if t.starts_with("--sock=") || t.starts_with("--pid=") => k += 1,
            _ if t.starts_with("--timeout=") => k += 1,
            _ => break,
        }
    }
    if args.get(k).is_some_and(|t| t.starts_with('@')) {
        k += 1;
    }
    let verb = args.get(k).map(String::as_str);
    let tail = args.get(k + 1..).unwrap_or(&[]);
    let first = tail.first().map(String::as_str);
    let reads = match verb {
        Some(v) if ATERM_CTL_READ.contains(&v) => true,
        Some(_) if tail.iter().all(|t| t == "--help" || t == "-h") && !tail.is_empty() => true,
        Some("meta" | "task") => tail.is_empty(),
        Some("inbox") => first.is_none() || (first == Some("get") && tail.len() == 2),
        _ => false,
    };
    (!reads).then(|| {
        format!("aterm ctl {} {}", verb.unwrap_or(""), first.unwrap_or(""))
            .trim()
            .to_string()
    })
}

fn program_scan(cmd: &str) -> Option<String> {
    for seg in program_words(cmd) {
        for (i, w) in seg.iter().enumerate() {
            let reason = match program(w) {
                "awk" | "gawk" | "mawk" | "nawk" => awk_args_write(&seg[i + 1..]),
                "sed" | "gsed" => sed_args_write(&seg[i + 1..]),
                _ => None,
            };
            if reason.is_some() {
                return reason;
            }
        }
    }
    None
}

/// Whether zsh `printf` evaluates arguments as arithmetic under `format`.
pub fn numeric_format(format: &str) -> bool {
    let Some(format) = decode_unicode_escapes(format) else {
        return true;
    };
    let mut cs = format.chars();
    while let Some(c) = cs.next() {
        if c != '%' {
            continue;
        }
        loop {
            match cs.next() {
                Some('%' | 's' | 'b' | 'q' | 'c') => break,
                Some('0'..='9' | '$' | '.' | '-' | '+' | ' ' | '#' | '\'') => {}
                _ => return true,
            }
        }
    }
    false
}

fn decode_unicode_escapes(format: &str) -> Option<String> {
    let mut out = String::with_capacity(format.len());
    let mut cs = format.chars().peekable();
    while let Some(c) = cs.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let digits = match cs.next() {
            Some('u') => 4,
            Some('U') => 8,
            Some(e) => {
                out.extend([c, e]);
                continue;
            }
            None => {
                out.push(c);
                break;
            }
        };
        let mut code = String::new();
        while code.len() < digits
            && let Some(d) = cs.next_if(char::is_ascii_hexdigit)
        {
            code.push(d);
        }
        out.push(
            u32::from_str_radix(&code, 16)
                .ok()
                .and_then(char::from_u32)?,
        );
    }
    Some(out)
}

fn arithmetic_scan(cmd: &str) -> Option<String> {
    for seg in program_words(cmd) {
        let Ok(Some(r)) = runs(&seg) else {
            continue;
        };
        let head = program(&seg[r.at]);
        let args = &seg[r.at + 1..];
        let evaluated: Vec<&String> = match head {
            "printf" => {
                let f = usize::from(args.first().is_some_and(|a| a == "--"));
                match args.get(f) {
                    Some(format) if !format.contains(SUBSTITUTION) && !numeric_format(format) => {
                        Vec::new()
                    }
                    _ => args.iter().skip(f + 1).collect(),
                }
            }
            "test" | "[" | "[[" => args
                .windows(2)
                .filter(|w| w[0] == "-t" && w[1] != "]" && w[1] != "]]")
                .map(|w| &w[1])
                .collect(),
            _ => Vec::new(),
        };
        if let Some(w) = evaluated.into_iter().find(|w| !plain_number(w)) {
            return Some(format!(
                "{head} {w} (zsh evaluates it as arithmetic, whose subscript can run a command)"
            ));
        }
    }
    None
}

fn plain_number(w: &str) -> bool {
    let digit = |r: &str| r.starts_with(|c: char| c.is_ascii_digit());
    w.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '#' | '.' | '+' | '-' | ' ' | '\t'))
        && w.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '#'))
            .all(|run| {
                run.is_empty()
                    || (digit(run)
                        && !run
                            .get(1..)
                            .and_then(|r| r.strip_prefix('#'))
                            .is_some_and(|r| !digit(r)))
            })
}

fn awk_args_write(args: &[String]) -> Option<String> {
    let mut programs: Vec<(usize, &str)> = Vec::new();
    let mut explicit = false;
    let mut k = 0;
    while k < args.len() {
        let w = args[k].as_str();
        if w == "-f"
            || w == "--file"
            || w.starts_with("--file=")
            || (w.starts_with("-f") && !w.starts_with("--"))
        {
            return Some("awk -f (a program file)".to_string());
        }
        if matches!(w, "-e" | "--source") {
            if let Some(p) = args.get(k + 1) {
                programs.push((k + 1, p));
            }
            explicit = true;
            k += 2;
            continue;
        }
        if let Some(p) = w.strip_prefix("--source=") {
            programs.push((k, p));
            explicit = true;
            k += 1;
            continue;
        }
        if matches!(w, "-F" | "-v" | "--field-separator" | "--assign") {
            k += 2;
            continue;
        }
        if w.starts_with("--field-separator=")
            || w.starts_with("--assign=")
            || ((w.starts_with("-F") || w.starts_with("-v")) && !w.starts_with("--"))
        {
            k += 1;
            continue;
        }
        if w == "--" {
            k += 1;
            break;
        }
        if w.starts_with('-') && w.len() > 1 {
            return Some(format!("awk {w} (an option this check does not read)"));
        }
        break;
    }
    if !explicit && let Some(p) = args.get(k) {
        programs.push((k, p));
    }
    let operands = if explicit { k } else { k + 1 };
    if let Some(why) = bwk_reads_otherwise(args, &programs, operands) {
        return Some(why);
    }
    if let Some((_, program)) = programs.iter().find(|(_, p)| p.contains(SUBSTITUTION)) {
        return Some(format!("awk: a program this check cannot read ({program})"));
    }
    programs
        .iter()
        .any(|(_, p)| awk_program_writes(p))
        .then(|| "awk writes (system, a redirect or a pipe in the program)".to_string())
}

/// Why macOS awk (BWK 20200816) reads `args` otherwise than gawk, whose reading [`awk_args_write`]
/// made (`programs`, each with its word's index, and `operands`, the index of the first operand),
/// if it does. BWK takes each word that starts with `-` for an option until `--` or a plain word,
/// reads `-F` and `-v` (the value glued or the next word) and `-f`, and ignores every other one as
/// one word, a long option and `-e` included; its program is the next word and each later word an
/// operand (MEASURED 2026-10-23 and 2026-10-29 on `/usr/bin/awk`: `awk --field-separator
/// 'BEGIN{system("touch m1")}' in.txt` printed `unknown option --field-separator ignored` and made
/// `m1`, `--assign` the same; `awk --source='{print}' 'BEGIN{system("touch m3")}'` made `m3`; `awk
/// -e '{print}' -e key` read `-e` and `key` as files). A line the two read differently is refused,
/// so the program checked is the one either runs.
fn bwk_reads_otherwise(
    args: &[String],
    programs: &[(usize, &str)],
    operands: usize,
) -> Option<String> {
    let mut j = 0;
    while let Some(w) = args.get(j).filter(|w| w.starts_with('-') && w.len() > 1) {
        if w == "--" {
            j += 1;
            break;
        }
        j += if matches!(w.as_str(), "-F" | "-v") {
            2
        } else {
            1
        };
    }
    let agree = match args.get(j) {
        Some(_) => matches!(programs, [(at, _)] if *at == j) && operands == j + 1,
        None => programs.is_empty(),
    };
    (!agree).then(|| {
        let option = args
            .iter()
            .find(|w| (w.starts_with("--") && w.len() > 2) || *w == "-e")
            .map_or("-e", String::as_str);
        format!(
            "awk {option}: macOS awk (BWK) reads its words otherwise than gawk (it ignores \
             every long option and -e, and runs the next word as its program)"
        )
    })
}

fn awk_program_writes(p: &str) -> bool {
    if p.contains(">>") || p.contains("|&") || p.contains('@') || names_awk_word(p, "system") {
        return true;
    }
    let cs: Vec<char> = p.chars().collect();
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut k = 0;
    const START: char = '\n';
    let mut prev: Option<char> = None;
    let mut prev_at = 0;
    let mut depth = 0usize;
    let mut print_at: Option<usize> = None;
    while k < cs.len() {
        let c = cs[k];
        if (c.is_ascii_alphabetic() || c == '_') && !(k > 0 && ident(cs[k - 1])) {
            let start = k;
            while k < cs.len() && ident(cs[k]) {
                k += 1;
            }
            let word: String = cs[start..k].iter().collect();
            if word == "print" || word == "printf" {
                print_at = Some(depth);
            }
            prev = if matches!(
                word.as_str(),
                "print" | "printf" | "return" | "else" | "do" | "case" | "in"
            ) {
                Some(START)
            } else {
                Some(cs[k - 1])
            };
            prev_at = k - 1;
            continue;
        }
        match c {
            '"' => {
                k += 1;
                while k < cs.len() && cs[k] != '"' {
                    k += if cs[k] == '\\' { 2 } else { 1 };
                }
                prev = Some('"');
                prev_at = k;
                k += 1;
                continue;
            }
            '#' => {
                while k < cs.len() && cs[k] != '\n' {
                    k += 1;
                }
                continue;
            }
            '/' if !prev.is_some_and(|p| {
                p.is_ascii_alphanumeric() || matches!(p, '_' | ')' | ']' | '"' | '/' | '.' | '$')
            }) =>
            {
                if let Some(op @ ('+' | '-')) = prev
                    && prev_at > 0
                    && cs[prev_at - 1] == op
                {
                    return true;
                }
                k = past_awk_regex(&cs, k + 1);
                prev = Some('/');
                prev_at = k.saturating_sub(1);
                continue;
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' | '{' | '}' => print_at = None,
            '\n' if !prev.is_some_and(|p| ",\\&|".contains(p)) => {
                print_at = None;
                prev = Some(START);
                k += 1;
                continue;
            }
            '|' => {
                if cs.get(k + 1) == Some(&'|') {
                    k += 2;
                    prev = Some('|');
                    prev_at = k - 1;
                    continue;
                }
                return true;
            }
            '>' => {
                if print_at.is_some_and(|d| depth <= d) {
                    return true;
                }
                if cs.get(k + 1) == Some(&'=') || prev == Some('-') || prev == Some('<') {
                    k += 1;
                    prev = Some('>');
                    prev_at = k - 1;
                    continue;
                }
                let next = cs[k + 1..].iter().find(|d| !d.is_whitespace());
                match next {
                    Some(d) if d.is_ascii_digit() || matches!(d, '$' | '-' | '+' | '.') => {}
                    _ => return true,
                }
            }
            _ => {}
        }
        if !c.is_whitespace() {
            prev = Some(c);
            prev_at = k;
        }
        k += 1;
    }
    false
}

fn past_awk_regex(cs: &[char], mut k: usize) -> usize {
    while k < cs.len() && cs[k] != '/' {
        match cs[k] {
            '\\' => k += 2,
            '[' => {
                k += 1;
                if cs.get(k) == Some(&'^') {
                    k += 1;
                }
                if cs.get(k) == Some(&']') {
                    k += 1;
                }
                while k < cs.len() && cs[k] != ']' {
                    if cs[k] == '[' && matches!(cs.get(k + 1), Some(':' | '.' | '=')) {
                        let close = cs[k + 1];
                        k += 2;
                        while k < cs.len() && !(cs[k] == close && cs.get(k + 1) == Some(&']')) {
                            k += 1;
                        }
                        k += 2;
                        continue;
                    }
                    k += if cs[k] == '\\' { 2 } else { 1 };
                }
                k += 1;
            }
            _ => k += 1,
        }
    }
    k + 1
}

fn names_awk_word(text: &str, word: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(ident)
            && !text[at + word.len()..].chars().next().is_some_and(ident)
    })
}

/// The scripts sed's words `args` (after the program) run: each `-e`/`--expression` value,
/// else its first operand; `Err` naming a script read from a file (`-f`, `--file`).
fn sed_scripts<S: AsRef<str>>(args: &[S]) -> Result<Vec<&str>, String> {
    let mut scripts: Vec<&str> = Vec::new();
    let mut explicit = false;
    let mut k = 0;
    while k < args.len() {
        let w = args[k].as_ref();
        if w == "-f" || w == "--file" || w.starts_with("--file=") || long_option_spells(w, "file") {
            return Err("sed -f (a script file)".to_string());
        }
        if w.starts_with("--") && w.contains('=') && long_option_spells(w, "expression") {
            scripts.push(w.split_once('=').map_or("", |(_, s)| s));
            explicit = true;
            k += 1;
            continue;
        }
        if w == "-e"
            || w == "--expression"
            || (w.starts_with("--") && long_option_spells(w, "expression"))
        {
            if let Some(s) = args.get(k + 1) {
                scripts.push(s.as_ref());
            }
            explicit = true;
            k += 2;
            continue;
        }
        if w.starts_with('-') && !w.starts_with("--") && w.len() > 1 {
            let cluster = &w[1..];
            let mut consumed_next = false;
            for (idx, c) in cluster.char_indices() {
                match c {
                    'f' => return Err("sed -f (a script file)".to_string()),
                    'e' => {
                        let rest = &cluster[idx + 1..];
                        if rest.is_empty() {
                            if let Some(s) = args.get(k + 1) {
                                scripts.push(s.as_ref());
                            }
                            consumed_next = true;
                        } else {
                            scripts.push(rest);
                        }
                        explicit = true;
                        break;
                    }
                    _ => {}
                }
            }
            k += if consumed_next { 2 } else { 1 };
            continue;
        }
        if w == "--line-length"
            || w.starts_with("--line-length=")
            || long_option_spells(w, "line-length")
        {
            k += if w.contains('=') { 1 } else { 2 };
            continue;
        }
        if w == "--" {
            k += 1;
            if !explicit && let Some(s) = args.get(k) {
                scripts.push(s.as_ref());
            }
            break;
        }
        if w.starts_with("--") && w.len() > 2 {
            k += 1;
            continue;
        }
        if !explicit {
            scripts.push(w);
        }
        break;
    }
    Ok(scripts)
}

fn sed_args_write(args: &[String]) -> Option<String> {
    let scripts = match sed_scripts(args) {
        Ok(scripts) => scripts,
        Err(why) => return Some(why),
    };
    if let Some(s) = scripts.iter().find(|s| s.contains(SUBSTITUTION)) {
        return Some(format!("sed: a script this check cannot read ({s})"));
    }
    scripts.iter().find_map(|s| sed_script_writes(s))
}

/// The files sed's words `args` (after the program) read by name in its scripts (`r` and `R`),
/// for the secret check.
fn sed_reads(args: &[&str]) -> Vec<String> {
    let mut reads = Vec::new();
    for script in sed_scripts(args).unwrap_or_default() {
        sed_script_walk(script, &mut reads);
    }
    reads
}

fn sed_script_writes(script: &str) -> Option<String> {
    sed_script_walk(script, &mut Vec::new())
}

/// [`sed_script_writes`], pushing onto `reads` each file the script's `r` and `R` commands read
/// (the rest of the line after the command and its blanks) as it walks.
fn sed_script_walk(script: &str, reads: &mut Vec<String>) -> Option<String> {
    let cs: Vec<char> = script.chars().collect();
    let n = cs.len();
    let unparsed = || Some("sed (unparsed script)".to_string());
    let skip_delimited = |i: &mut usize, d: char| -> bool {
        while *i < n {
            if cs[*i] == '\\' {
                *i += 2;
                continue;
            }
            if cs[*i] == d {
                *i += 1;
                return true;
            }
            *i += 1;
        }
        false
    };
    let skip_regex = |i: &mut usize, d: char| -> bool {
        if d == '[' {
            return skip_delimited(i, d);
        }
        while *i < n {
            match cs[*i] {
                '\\' => {
                    *i += 2;
                    continue;
                }
                '[' => {
                    *i += 1;
                    if cs.get(*i) == Some(&'^') {
                        *i += 1;
                    }
                    if cs.get(*i) == Some(&']') {
                        *i += 1;
                    }
                    while *i < n && cs[*i] != ']' {
                        if cs[*i] == '[' && matches!(cs.get(*i + 1), Some(':' | '.' | '=')) {
                            let close = cs[*i + 1];
                            *i += 2;
                            while *i < n && !(cs[*i] == close && cs.get(*i + 1) == Some(&']')) {
                                *i += 1;
                            }
                            *i += 2;
                            continue;
                        }
                        *i += 1;
                    }
                    *i += 1;
                    continue;
                }
                c if c == d => {
                    *i += 1;
                    return true;
                }
                _ => *i += 1,
            }
        }
        false
    };
    let to_line_end = |i: &mut usize| {
        while *i < n && cs[*i] != '\n' {
            *i += 1;
        }
    };
    let to_command_end = |i: &mut usize| {
        while *i < n && cs[*i] != '\n' && cs[*i] != ';' && cs[*i] != '}' {
            *i += 1;
        }
    };
    let mut i = 0;
    while i < n {
        let c = cs[i];
        if c.is_whitespace() || c == ';' || c == '{' || c == '}' {
            i += 1;
            continue;
        }
        loop {
            if i >= n {
                return unparsed();
            }
            match cs[i] {
                d if d.is_ascii_digit() => {
                    while i < n && (cs[i].is_ascii_digit() || cs[i] == '~') {
                        i += 1;
                    }
                }
                '$' => i += 1,
                '/' => {
                    i += 1;
                    if !skip_regex(&mut i, '/') {
                        return unparsed();
                    }
                    while i < n && matches!(cs[i], 'I' | 'M') {
                        i += 1;
                    }
                }
                '\\' => {
                    let Some(&d) = cs.get(i + 1) else {
                        return unparsed();
                    };
                    i += 2;
                    if !skip_regex(&mut i, d) {
                        return unparsed();
                    }
                    while i < n && matches!(cs[i], 'I' | 'M') {
                        i += 1;
                    }
                }
                _ => break,
            }
            while i < n && cs[i].is_whitespace() {
                i += 1;
            }
            if i < n && cs[i] == ',' {
                i += 1;
                while i < n && cs[i].is_whitespace() {
                    i += 1;
                }
                continue;
            }
            break;
        }
        while i < n && (cs[i].is_whitespace() || cs[i] == '!') {
            i += 1;
        }
        if i >= n {
            return unparsed();
        }
        let cmd = cs[i];
        i += 1;
        match cmd {
            '{' | '}' => {}
            '#' | 'a' | 'i' | 'c' => to_line_end(&mut i),
            'r' | 'R' => {
                while i < n && matches!(cs[i], ' ' | '\t') {
                    i += 1;
                }
                let from = i;
                to_line_end(&mut i);
                if i > from {
                    reads.push(cs[from..i].iter().collect());
                }
            }
            ':' | 'b' | 't' | 'T' | 'v' => to_command_end(&mut i),
            'w' | 'W' => return Some("sed w (writes a file)".to_string()),
            'e' => return Some("sed e (runs a command)".to_string()),
            'q' | 'Q' | 'l' | 'L' => {
                while i < n && (cs[i].is_whitespace() || cs[i].is_ascii_digit()) {
                    i += 1;
                }
            }
            's' => {
                let Some(&d) = cs.get(i) else {
                    return unparsed();
                };
                i += 1;
                if !skip_regex(&mut i, d) || !skip_delimited(&mut i, d) {
                    return unparsed();
                }
                while i < n {
                    match cs[i] {
                        'g' | 'p' | 'i' | 'I' | 'm' | 'M' | '0'..='9' => i += 1,
                        'w' => return Some("sed s///w (writes a file)".to_string()),
                        'e' => return Some("sed s///e (runs a command)".to_string()),
                        _ => break,
                    }
                }
            }
            'y' => {
                let Some(&d) = cs.get(i) else {
                    return unparsed();
                };
                i += 1;
                if !skip_delimited(&mut i, d) || !skip_delimited(&mut i, d) {
                    return unparsed();
                }
            }
            'd' | 'D' | 'p' | 'P' | 'n' | 'N' | 'g' | 'G' | 'h' | 'H' | 'x' | '=' | 'z' | 'F' => {}
            other => return Some(format!("sed {other} (unparsed script)")),
        }
    }
    None
}

/// Split `src` into segments of quote-resolved words with substitutions marked.
pub fn program_words(src: &str) -> Vec<Vec<String>> {
    program_words_nested(src)
        .into_iter()
        .map(|(seg, _)| seg)
        .collect()
}

fn program_words_nested(src: &str) -> Vec<NestedSegment> {
    scan_words(src).segments
}

/// A segment's quote-resolved words and whether it lies inside a substitution.
type NestedSegment = (Vec<String>, bool);

/// A segment's words and the first glob character the shell may expand in
/// them ([`command_words_globbed`]).
pub type GlobbedSegment = (Vec<String>, Option<char>);

/// What [`scan_words`] reads off a line.
struct Scanned {
    /// [`program_words_nested`]'s segments.
    segments: Vec<NestedSegment>,
    /// Beside each segment, the first glob character the shell may expand in
    /// its words (an unquoted, unescaped `*`, `?` or `[`, or one in an
    /// unquoted parameter expansion such as `$*`, `$?` or `${x:-*}`, as
    /// [`unquoted_glob`] counts them) or in a `$(…)` or backquoted
    /// substitution among them, whose output becomes its words.
    globs: Vec<Option<char>>,
    /// The text of every parameter expansion outside single quotes
    /// ([`expanded_parameters`]).
    params: Vec<String>,
}

fn scan_words(src: &str) -> Scanned {
    let chars: Vec<char> = src.chars().collect();
    let mut scan = WordScan {
        chars: &chars,
        i: 0,
        segments: Vec::new(),
        globs: Vec::new(),
        params: Vec::new(),
        seg: Vec::new(),
        cur: String::new(),
        in_word: false,
        nest: 0,
        glob: None,
    };
    scan.run(None);
    Scanned {
        segments: scan.segments,
        globs: scan.globs,
        params: scan.params,
    }
}

/// The text of every parameter expansion `line` holds outside single quotes, a substitution's
/// among them: a name (`HOME`, `1`), a braced body (`x:-*`, `#x`, `!x`), and every name a braced
/// body expands in turn (`TOKEN` of `${x:-$TOKEN}`), the name after zsh's `=`, `^`, `+` or `~`
/// flags among them (`TOKEN` of `$=TOKEN`). The special parameters (`$?`, `$@`, …) are none.
///
/// # Errors
///
/// The line's lexing refusal ([`prelex`]).
pub fn expanded_parameters(line: &str) -> Result<Vec<String>, String> {
    Ok(scan_words(&strip_alarm_idiom(&prelex(line)?)).params)
}

/// [`command_words`], each segment with the first glob character the shell
/// may expand in its own words or in a substitution nested in them
/// (`scan_words`). A glob in one command of a pipeline or a list is never
/// another's words; a substitution's output is its parent's.
pub fn command_words_globbed(line: &str) -> Result<Vec<GlobbedSegment>, String> {
    let Scanned {
        segments, globs, ..
    } = scan_words(&strip_alarm_idiom(&prelex(line)?));
    Ok(segments
        .into_iter()
        .map(|(seg, _)| seg)
        .zip(globs)
        .collect())
}

struct WordScan<'a> {
    chars: &'a [char],
    i: usize,
    segments: Vec<(Vec<String>, bool)>,
    /// Beside each of `segments`, its glob ([`scan_words`]).
    globs: Vec<Option<char>>,
    /// [`Scanned::params`].
    params: Vec<String>,
    seg: Vec<String>,
    cur: String,
    in_word: bool,
    nest: usize,
    /// The current segment's first glob.
    glob: Option<char>,
}

impl WordScan<'_> {
    fn end_word(&mut self) {
        if self.in_word {
            self.seg.push(std::mem::take(&mut self.cur));
            self.in_word = false;
        }
    }
    fn end_seg(&mut self) {
        self.end_word();
        let glob = self.glob.take();
        if !self.seg.is_empty() {
            self.segments
                .push((std::mem::take(&mut self.seg), self.nest > 0));
            self.globs.push(glob);
        }
    }
    fn nested(&mut self, stop: char) {
        let seg = std::mem::take(&mut self.seg);
        let cur = std::mem::take(&mut self.cur);
        let glob = self.glob.take();
        let inner = self.globs.len();
        self.in_word = false;
        self.nest += 1;
        self.run(Some(stop));
        self.nest -= 1;
        self.seg = seg;
        self.cur = cur;
        self.glob = glob.or_else(|| self.globs[inner..].iter().find_map(|g| *g));
        self.cur.push_str(SUBSTITUTION);
        self.in_word = true;
    }
    fn mark_parameter(&mut self) {
        self.i += 1;
        let flags = self.chars[self.i..]
            .iter()
            .take_while(|c| matches!(c, '=' | '^' | '+' | '~'))
            .count();
        if flags > 0
            && self
                .chars
                .get(self.i + flags)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '{'))
        {
            self.i += flags;
        }
        match self.chars.get(self.i) {
            Some('{') => {
                self.i += 1;
                let from = self.i;
                while self.i < self.chars.len() && self.chars[self.i] != '}' {
                    self.i += 1;
                }
                let body: String = self.chars[from..self.i].iter().collect();
                self.params.extend(scan_words(&body).params);
                self.params.push(body);
                if self.i < self.chars.len() {
                    self.i += 1;
                }
            }
            Some(&c) if c.is_ascii_alphanumeric() || c == '_' => {
                let from = self.i;
                while self.i < self.chars.len()
                    && (self.chars[self.i].is_ascii_alphanumeric() || self.chars[self.i] == '_')
                {
                    self.i += 1;
                }
                self.params.push(self.chars[from..self.i].iter().collect());
            }
            Some('@' | '*' | '#' | '?' | '$' | '!' | '-') => self.i += 1,
            _ => {
                self.cur.push('$');
                self.in_word = true;
                return;
            }
        }
        self.cur.push_str(SUBSTITUTION);
        self.in_word = true;
    }

    fn run(&mut self, stop: Option<char>) {
        let mut depth = 0usize;
        while self.i < self.chars.len() {
            let c = self.chars[self.i];
            match c {
                '\\' => {
                    if let Some(&n) = self.chars.get(self.i + 1) {
                        self.cur.push(n);
                        self.i += 2;
                    } else {
                        self.i += 1;
                    }
                    self.in_word = true;
                }
                '\'' => {
                    self.i += 1;
                    while self.i < self.chars.len() && self.chars[self.i] != '\'' {
                        self.cur.push(self.chars[self.i]);
                        self.i += 1;
                    }
                    self.i += 1;
                    self.in_word = true;
                }
                '"' => {
                    self.i += 1;
                    self.in_word = true;
                    while self.i < self.chars.len() {
                        let d = self.chars[self.i];
                        match d {
                            '\\' => {
                                match self.chars.get(self.i + 1) {
                                    Some(&n) if matches!(n, '"' | '\\' | '$' | '`') => {
                                        self.cur.push(n);
                                    }
                                    Some(&n) => {
                                        self.cur.push('\\');
                                        self.cur.push(n);
                                    }
                                    None => {}
                                }
                                self.i += 2;
                            }
                            '"' => {
                                self.i += 1;
                                break;
                            }
                            '$' if self.chars.get(self.i + 1) == Some(&'(') => {
                                self.i += 2;
                                self.nested(')');
                            }
                            '$' => self.mark_parameter(),
                            '`' => {
                                self.i += 1;
                                self.nested('`');
                            }
                            _ => {
                                self.cur.push(d);
                                self.i += 1;
                            }
                        }
                    }
                }
                '$' if self.chars.get(self.i + 1) == Some(&'(') => {
                    self.i += 2;
                    self.nested(')');
                }
                '$' => {
                    let from = self.i + 1;
                    self.mark_parameter();
                    if self.glob.is_none() {
                        self.glob = self.chars[from.min(self.i)..self.i]
                            .iter()
                            .copied()
                            .find(|c| matches!(c, '*' | '?' | '['));
                    }
                }
                '`' if stop == Some('`') => {
                    self.i += 1;
                    self.end_seg();
                    return;
                }
                '`' => {
                    self.i += 1;
                    self.nested('`');
                }
                '(' => {
                    depth += 1;
                    self.end_seg();
                    self.i += 1;
                }
                ')' => {
                    self.i += 1;
                    self.end_seg();
                    if depth == 0 && stop == Some(')') {
                        return;
                    }
                    depth = depth.saturating_sub(1);
                }
                ';' | '|' | '&' | '\n' => {
                    self.end_seg();
                    self.i += 1;
                }
                c if c.is_whitespace() => {
                    self.end_word();
                    self.i += 1;
                }
                '>' | '<' => {
                    if self.in_word
                        && self
                            .cur
                            .chars()
                            .last()
                            .is_some_and(|l| !matches!(l, '>' | '<'))
                        && !self.cur.chars().all(|d| d.is_ascii_digit())
                    {
                        self.end_word();
                    }
                    self.cur.push(c);
                    self.in_word = true;
                    self.i += 1;
                }
                _ => {
                    if matches!(c, '*' | '?' | '[') && self.glob.is_none() {
                        self.glob = Some(c);
                    }
                    self.cur.push(c);
                    self.in_word = true;
                    self.i += 1;
                }
            }
        }
        self.end_seg();
    }
}

/// The first brace expansion bash may perform in `line`, from its `{` to its `}`.
pub fn brace_expansion(line: &str) -> Option<String> {
    let c: Vec<char> = line.chars().collect();
    let mut found = None;
    scan_unquoted(&c, 0, None, &mut |c, i| {
        if found.is_none() && c[i] == '{' && (i == 0 || c[i - 1] != '$') {
            found = brace_close(c, i).map(|end| c[i..=end].iter().collect());
        }
    });
    found
}

/// The first glob character the shell may expand in `line`: an unquoted, unescaped `*`, `?` or `[`.
pub fn unquoted_glob(line: &str) -> Option<char> {
    let c: Vec<char> = line.chars().collect();
    let mut found = None;
    scan_unquoted(&c, 0, None, &mut |c, i| {
        if found.is_none() && matches!(c[i], '*' | '?' | '[') {
            found = Some(c[i]);
        }
    });
    found
}

fn scan_unquoted(
    c: &[char],
    mut i: usize,
    close: Option<char>,
    visit: &mut dyn FnMut(&[char], usize),
) -> usize {
    let mut parens = 0usize;
    while i < c.len() {
        let ch = c[i];
        if Some(ch) == close && (ch != ')' || parens == 0) {
            return i + 1;
        }
        i = match ch {
            '\\' => i + 2,
            '\'' => past_single(c, i + 1, false),
            '$' if c.get(i + 1) == Some(&'\'') => past_single(c, i + 2, true),
            '"' => scan_double(c, i + 1, visit),
            '$' if c.get(i + 1) == Some(&'(') => scan_unquoted(c, i + 2, Some(')'), visit),
            '`' if close != Some('`') => scan_unquoted(c, i + 1, Some('`'), visit),
            '(' => {
                parens += 1;
                i + 1
            }
            ')' => {
                parens = parens.saturating_sub(1);
                i + 1
            }
            _ => {
                visit(c, i);
                i + 1
            }
        };
    }
    c.len()
}

fn scan_double(c: &[char], mut i: usize, visit: &mut dyn FnMut(&[char], usize)) -> usize {
    while i < c.len() {
        i = match c[i] {
            '\\' => i + 2,
            '"' => return i + 1,
            '$' if c.get(i + 1) == Some(&'(') => scan_unquoted(c, i + 2, Some(')'), visit),
            '`' => scan_unquoted(c, i + 1, Some('`'), visit),
            _ => i + 1,
        };
    }
    c.len()
}

const fn past_single(c: &[char], mut i: usize, ansi: bool) -> usize {
    while i < c.len() {
        match c[i] {
            '\\' if ansi => i += 2,
            '\'' => return i + 1,
            _ => i += 1,
        }
    }
    c.len()
}

fn brace_close(c: &[char], open: usize) -> Option<usize> {
    let (mut depth, mut parens, mut sep) = (1usize, 0usize, false);
    let mut skip = |_: &[char], _: usize| {};
    let mut j = open + 1;
    while j < c.len() {
        j = match c[j] {
            '\\' => j + 2,
            '\'' => past_single(c, j + 1, false),
            '$' if c.get(j + 1) == Some(&'\'') => past_single(c, j + 2, true),
            '"' => scan_double(c, j + 1, &mut skip),
            '$' if c.get(j + 1) == Some(&'(') => scan_unquoted(c, j + 2, Some(')'), &mut skip),
            '`' => scan_unquoted(c, j + 1, Some('`'), &mut skip),
            '(' => {
                parens += 1;
                j + 1
            }
            ')' if parens == 0 => return None,
            ')' => {
                parens -= 1;
                j + 1
            }
            ' ' | '\t' | '\n' | ';' | '|' | '&' | '<' | '>' if parens == 0 => return None,
            '{' => {
                depth += 1;
                j + 1
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return sep.then_some(j);
                }
                j + 1
            }
            ',' if depth == 1 => {
                sep = true;
                j + 1
            }
            '.' if depth == 1 && c.get(j + 1) == Some(&'.') => {
                sep = true;
                j + 2
            }
            _ => j + 1,
        };
    }
    None
}

fn network_read(line: &str) -> Option<String> {
    let socket = |t: &str| ["/dev/tcp/", "/dev/udp/"].iter().any(|p| t.starts_with(p));
    let inet = |t: &str| {
        ["/inet/", "/inet4/", "/inet6/"]
            .iter()
            .any(|p| t.contains(p))
    };
    let segments = command_words(line).ok()?;
    for seg in &segments {
        for (k, word) in seg.iter().enumerate() {
            if inet(word) {
                return Some(format!("names a gawk network file: {word}"));
            }
            let op = word.trim_start_matches(|c: char| c.is_ascii_digit());
            let Some(rest) = op.strip_prefix('<') else {
                continue;
            };
            if rest.starts_with('<') {
                continue;
            }
            let rest = rest.strip_prefix('&').unwrap_or(rest);
            let Some(target) = Some(rest)
                .filter(|r| !r.is_empty())
                .or_else(|| seg.get(k + 1).map(String::as_str))
            else {
                continue;
            };
            if socket(target) || target.contains(['$', '`']) {
                return Some(format!(
                    "an input redirect from {} may open a network connection",
                    target.replace(SUBSTITUTION, "$(…)")
                ));
            }
        }
    }
    None
}

fn network_reach(line: &str) -> Option<String> {
    network_read(line)
}

/// Match `pattern` (`*` and `?`, spanning `/`) against the whole `text`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark): (Option<usize>, usize) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Corpus of substitution-bypass command lines and the rules that refuse them.
#[cfg(any(test, feature = "testing"))]
pub const MEASURED_SUBSTITUTION_BYPASSES: &[(&str, &str)] = &[
    ("$(printf 'r''m') -rf /usr", UNREADABLE_SUBSTITUTION),
    ("`printf %s r m` -rf /usr", UNREADABLE_SUBSTITUTION),
    ("$(printf 'r''m') -- /usr", UNREADABLE_SUBSTITUTION),
    ("$(printf 'r''m') 2>/dev/null /usr", UNREADABLE_SUBSTITUTION),
    ("$(printf 'r''m') ls", UNREADABLE_SUBSTITUTION),
    ("ls; $(printf 'r''m') -rf /usr", UNREADABLE_SUBSTITUTION),
    ("$(which rm) -rf /usr", UNREADABLE_SUBSTITUTION),
    ("`echo rm` -rf /usr", UNREADABLE_SUBSTITUTION),
    ("A=1 $(printf 'r''m') -rf /usr", UNREADABLE_SUBSTITUTION),
    (
        "if $(printf 'r''m') -rf /usr; then echo; fi",
        UNREADABLE_SUBSTITUTION,
    ),
    ("do $(printf 'r''m') -rf /usr", UNREADABLE_SUBSTITUTION),
    (
        "2>/dev/null $(printf 'r''m') -rf /usr",
        UNREADABLE_SUBSTITUTION,
    ),
    ("env $(printf 'r''m') -rf /usr", UNREADABLE_SUBSTITUTION),
    (
        "timeout 5 $(printf 'r''m') -rf /usr",
        UNREADABLE_SUBSTITUTION,
    ),
    ("ls | xargs $(printf 'r''m') -rf", UNREADABLE_SUBSTITUTION),
    (
        "ls | xargs $(echo qm | tr q r) -rf /usr",
        UNREADABLE_SUBSTITUTION,
    ),
    ("env $(echo qm | tr q r) -rf /usr", UNREADABLE_SUBSTITUTION),
    (
        "timeout 5 $(echo qm | tr q r) -rf /usr",
        UNREADABLE_SUBSTITUTION,
    ),
    (
        "echo $( $(printf 'r''m') -rf /usr )",
        UNREADABLE_SUBSTITUTION,
    ),
    (
        "echo \"$( $(printf 'r''m') -rf /usr )\"",
        UNREADABLE_SUBSTITUTION,
    ),
    (
        "git branch $(git rev-parse --short HEAD)",
        "git branch $(…) (a word this check cannot read may be a flag or a verb)",
    ),
    (
        "git tag $(date +%s)",
        "git tag $(…) (a word this check cannot read may be a flag or a verb)",
    ),
    ("uniq a.txt $(echo b.txt)", "uniq <output file>"),
];

#[cfg(any(test, feature = "testing"))]
const UNREADABLE_SUBSTITUTION: &str = "a command whose name this check cannot read ($(…))";

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
