// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! THE SUPERVISOR: run the vendor CLI as a child, follow its signals and stops, exit with its
//! status, and move it onto a newer build at a safe point without exiting.
//!
//! **Process shape** (aterm's harness split survey, shape B2): the child is spawned in the
//! harness's own process group on the inherited terminal — no `setpgid`, no pty in between —
//! so the harness leads the terminal's foreground group and the child reads and draws the
//! terminal directly.
//!
//! **Signals.**
//! - `SIGINT`/`SIGQUIT` belong to the child (the terminal sends them to the whole foreground
//!   group, so the child already has them): the harness catches and ignores them. A caught
//!   signal is reset to its default across `exec`, so the child starts with the default
//!   disposition, unlike an ignored one.
//! - `SIGTSTP` is left at its default: a terminal `^Z` stops the harness with the child. When
//!   the child stops on its own (raw-mode TUIs suspend themselves), the harness stops itself too
//!   so the shell sees the job stop, and on `SIGCONT` continues the child.
//! - `SIGHUP`/`SIGTERM` are passed on to the child; the harness then exits with the child's
//!   status once it ends, and never relaunches after one.
//! - `SIGUSR1` is the idle attestation ([`crate::safepoint`]).
//!
//! **Exit.** The child's real status: its exit code, or death by the same signal (re-raised on
//! the harness with core dumps off, so the shell reports it exactly as it would the child's).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, WaitOptions, kill_process, waitpid};
use signal_hook::consts::signal::{
    SIGABRT, SIGBUS, SIGCHLD, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGKILL, SIGQUIT, SIGSEGV, SIGSYS,
    SIGTERM, SIGTRAP, SIGTSTP, SIGUSR1, SIGUSR2,
};

use alab_wire::link::LATER_RETRY_MS;
use alab_wire::relaunch::{AWAY_BUDGET, AwayBudget, ChildEnd, Ladder};
use alab_wire::{Account, ChildProcess, Link, Reading, Relaunch, Restart, Verdict as Word};

use crate::aterm;
use crate::cli::NAME;
use crate::env::Env;
use crate::journal::Journal;
use crate::kimi;
use crate::proc;
use crate::safepoint::{self, Attestation, Knobs, Probe, Verdict};
use crate::store::{self, Build};
use crate::tty;

/// How a harness run ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Exit with this code.
    Exit(i32),
    /// Die by this signal.
    Signal(i32),
}

/// The variable the harness sets, in every process it starts as the vendor, to its own pid. A
/// kimi-alab that finds it naming its parent (or itself: an `exec` in between keeps the pid) was
/// started AS THE VENDOR of a harness — the store's `kimi` leads back to kimi-alab through
/// something the same-file check cannot see (a script that `exec`s it, a copy) — and refuses.
pub const PID_ENV: &str = "KIMI_ALAB_PID";

/// The variable the harness sets, in every process it starts as the vendor, to its own nesting
/// depth plus one. [`PID_ENV`] catches a harness started directly as the vendor; this catches one
/// reached again through a process in between (a store `kimi` that runs kimi-alab without `exec`).
pub const DEPTH_ENV: &str = "KIMI_ALAB_DEPTH";

/// The most harnesses that may run nested in one another. A person starting kimi-alab from a
/// Kimi tool's shell goes one level deeper; a store `kimi` that runs kimi-alab again goes on
/// forever, and stops here.
pub const MAX_DEPTH: u32 = 8;

/// This harness's nesting depth, or why it must not run at all (it then exits 126 before it
/// touches the store). `parent` is this process's parent pid and `me` its own.
///
/// # Errors
/// [`PID_ENV`] names `parent` or `me`; or [`DEPTH_ENV`] is not a number, or is [`MAX_DEPTH`] or
/// more (an unreadable depth is never read as "not nested").
pub fn nesting(env: &Env, parent: u32, me: u32) -> Result<u32, String> {
    let named = env
        .get(PID_ENV)
        .and_then(|v| v.to_str())
        .and_then(|v| v.trim().parse::<u32>().ok());
    if let Some(pid) = named.filter(|p| *p == parent || *p == me) {
        return Err(format!(
            "started as the vendor of {NAME} pid {pid} ({PID_ENV}): the atpkg store's `{}` leads \
             back to {NAME}; refusing to run it again",
            kimi::COMMAND
        ));
    }
    let depth = match env.get(DEPTH_ENV) {
        None => 0,
        Some(v) => v
            .to_str()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .ok_or_else(|| format!("{DEPTH_ENV}={} is not a nesting depth", v.to_string_lossy()))?,
    };
    if depth >= MAX_DEPTH {
        return Err(format!(
            "{DEPTH_ENV}={depth}: {NAME} already runs {depth} deep (at most {MAX_DEPTH}); the atpkg \
             store's `{}` probably runs {NAME} again",
            kimi::COMMAND
        ));
    }
    Ok(depth)
}

/// Write one line to the terminal, best effort. Never `eprintln!` here: it panics when stderr is
/// a broken pipe or a hung-up terminal, and a panic would end the harness with 101 instead of the
/// child's status (or leave a relaunched child without its supervisor).
fn say(line: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{NAME}: {line}");
}

/// Everything a run is configured with.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The atpkg prefix the vendor builds come from.
    pub prefix: PathBuf,
    /// The harness's environment (the child's, before the build's shim env).
    pub env: Env,
    /// The safe-point timing.
    pub knobs: Knobs,
    /// How often the store and the child are looked at.
    pub poll: Duration,
    /// How long the child has to exit after the harness's `SIGTERM` before the move is abandoned.
    pub term_grace: Duration,
    /// How long a restart aterm asked for may wait, from its arrival, for the child's session
    /// lock and for aterm's word before it is refused (alab-wire's [`AWAY_BUDGET`], 45 s).
    pub restart_wait: Duration,
    /// Never move the child onto a new build.
    pub no_update: bool,
    /// This harness's nesting depth ([`nesting`]); its children get one more.
    pub depth: u32,
}

impl Settings {
    /// Settings from `env`: the defaults, with the test and tuning knobs
    /// `KIMI_ALAB_POLL_MS`, `KIMI_ALAB_QUIET_MS`, `KIMI_ALAB_ATTEST_MS`,
    /// `KIMI_ALAB_TERM_GRACE_MS` and `KIMI_ALAB_RESTART_WAIT_MS` read when set.
    pub fn from_env(prefix: PathBuf, env: Env, no_update: bool, depth: u32) -> Self {
        let ms = |name: &str| {
            env.get(name)
                .and_then(|v| v.to_str())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_millis)
        };
        let d = Knobs::defaults();
        let knobs = Knobs::with_quiet(
            ms("KIMI_ALAB_QUIET_MS").unwrap_or(d.quiet),
            ms("KIMI_ALAB_ATTEST_MS").unwrap_or(d.attest_valid),
        );
        let poll = ms("KIMI_ALAB_POLL_MS")
            .unwrap_or(Duration::from_millis(250))
            .clamp(Duration::from_millis(20), Duration::from_secs(5));
        let term_grace = ms("KIMI_ALAB_TERM_GRACE_MS")
            .unwrap_or(Duration::from_secs(30))
            .clamp(Duration::from_millis(100), Duration::from_secs(600));
        let restart_wait = ms("KIMI_ALAB_RESTART_WAIT_MS")
            .unwrap_or(AWAY_BUDGET)
            .clamp(Duration::from_millis(100), Duration::from_secs(3600));
        Self {
            prefix,
            env,
            knobs,
            poll,
            term_grace,
            restart_wait,
            no_update,
            depth,
        }
    }
}

/// The signals the harness catches.
const CAUGHT: &[i32] = &[SIGCHLD, SIGHUP, SIGTERM, SIGINT, SIGQUIT, SIGUSR1, SIGUSR2];

/// Caught signals: a flag per signal, and one self-pipe that wakes the loop.
struct Signals {
    flags: Vec<(i32, Arc<AtomicBool>)>,
    wake: UnixStream,
}

impl Signals {
    fn install() -> std::io::Result<Self> {
        let (wake, tx) = UnixStream::pair()?;
        let mut flags = Vec::new();
        for &sig in CAUGHT {
            let flag = Arc::new(AtomicBool::new(false));
            signal_hook::flag::register(sig, Arc::clone(&flag))?;
            signal_hook::low_level::pipe::register(sig, tx.try_clone()?)?;
            flags.push((sig, flag));
        }
        Ok(Self { flags, wake })
    }

    /// Sleep until a signal arrives or `timeout` passes, then drain the pipe.
    fn wait(&mut self, timeout: Duration) {
        let mut buf = [0u8; 64];
        if self
            .wake
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
            .is_ok()
        {
            let _ = self.wake.read(&mut buf);
        }
        if self.wake.set_nonblocking(true).is_ok() {
            while matches!(self.wake.read(&mut buf), Ok(n) if n > 0) {}
            let _ = self.wake.set_nonblocking(false);
        }
    }

    /// The signals that arrived since the last call, in [`CAUGHT`] order.
    fn take(&self) -> Vec<i32> {
        self.flags
            .iter()
            .filter(|(_, f)| f.swap(false, Ordering::SeqCst))
            .map(|(s, _)| *s)
            .collect()
    }
}

/// How the child ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// It exited with this code.
    Code(i32),
    /// A signal killed it.
    Killed(i32),
}

impl Status {
    /// The harness outcome that reports this status unchanged.
    pub fn outcome(self) -> Outcome {
        match self {
            Self::Code(c) => Outcome::Exit(c),
            Self::Killed(s) => Outcome::Signal(s),
        }
    }

    /// The end as `alab-wire` judges it.
    pub fn end(self) -> ChildEnd {
        match self {
            Self::Code(code) => ChildEnd::Exited(code),
            Self::Killed(signal) => ChildEnd::Signaled(signal),
        }
    }

    /// Whether this is how Kimi ends after the harness's `SIGTERM`: its graceful stop's 143, or
    /// death by the signal itself.
    pub fn after_our_term(self) -> bool {
        matches!(self, Self::Code(kimi::TERM_EXIT) | Self::Killed(SIGTERM))
    }

    /// Whether this status, soon after a move started a new build, is that build refusing to
    /// start: any non-zero exit that is not a terminal/job signal's (129 HUP, 130 INT, 131 QUIT,
    /// 143 TERM), a fault/abort, or an untrappable SIGKILL.
    pub fn refused_start(self) -> bool {
        match self {
            Self::Code(c) => c != 0 && !matches!(c, 129 | 130 | 131 | kimi::TERM_EXIT),
            Self::Killed(s) => matches!(
                s,
                SIGABRT | SIGBUS | SIGFPE | SIGILL | SIGKILL | SIGSEGV | SIGSYS | SIGTRAP
            ),
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Code(c) => write!(f, "exit {c}"),
            Self::Killed(s) => write!(f, "signal {s}"),
        }
    }
}

enum Reap {
    Running,
    Stopped,
    /// The child was continued: after the harness's own `SIGCONT`, or after a stop the harness
    /// never saw, as when a whole-job `^Z` stopped the harness with it and the shell's `fg`
    /// continued both (the stop is then no longer reported, only the continue).
    Continued,
    Ended(Status),
}

fn reap(pid: Pid) -> Reap {
    loop {
        match waitpid(
            Some(pid),
            WaitOptions::NOHANG | WaitOptions::UNTRACED | WaitOptions::CONTINUED,
        ) {
            Ok(None) => return Reap::Running,
            Ok(Some((_, st))) if st.stopped() => return Reap::Stopped,
            Ok(Some((_, st))) if st.continued() => return Reap::Continued,
            Ok(Some((_, st))) => {
                if let Some(code) = st.exit_status() {
                    return Reap::Ended(Status::Code(code));
                }
                if let Some(sig) = st.terminating_signal() {
                    return Reap::Ended(Status::Killed(sig));
                }
                return Reap::Ended(Status::Code(1));
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return Reap::Ended(Status::Code(1)),
        }
    }
}

/// A running vendor child.
struct Running {
    _child: Option<Child>,
    pid: Pid,
    build: Build,
    /// The launch's exact words, carried across a harness re-exec.
    argv: Vec<OsString>,
    /// The launch's words as UTF-8, if they all are.
    words: Option<Vec<String>>,
    /// The session most recently proven by this running child's lock.
    session: Option<String>,
    /// The managed account home this child runs on, if aterm selected one.
    account: Option<Account>,
    /// Kernel identity required before this child may cross a harness re-exec.
    identity: Option<proc::ChildIdentity>,
    /// The child's pid and kernel start time as aterm checks a claim's `child=`, when they could
    /// be read.
    process: Option<ChildProcess>,
    started: Instant,
}

const CREDENTIAL_OVERRIDES: [&str; 3] = [
    "KIMI_MODEL_NAME",
    "KIMI_MODEL_API_KEY",
    "KIMI_CODE_CUSTOM_HEADERS",
];

fn apply_account(env: &mut Env, account: Option<&Account>) {
    let Some(account) = account else {
        return;
    };
    for name in CREDENTIAL_OVERRIDES {
        env.remove(name);
    }
    if let Some(home) = &account.home {
        env.set(kimi::HOME_ENV, home.as_os_str());
    } else {
        env.remove(kimi::HOME_ENV);
    }
}

impl Running {
    /// Start `build` with `args`, as the vendor of a harness at nesting depth `depth`. Refuses a
    /// build whose binary is this harness itself, on the first launch and on every relaunch.
    fn start(
        build: Build,
        args: &[OsString],
        depth: u32,
        account: Option<&Account>,
    ) -> std::io::Result<Self> {
        if store::is_this_harness(&build.exe) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{} is this harness itself", build.exe.display()),
            ));
        }
        let mut cmd = Command::new(&build.exe);
        cmd.args(args);
        for (k, v) in &build.shim_env {
            cmd.env(k, v);
        }
        for (k, v) in kimi::HARNESS_ENV {
            cmd.env(k, v);
        }
        if account.is_some() {
            for name in CREDENTIAL_OVERRIDES {
                cmd.env_remove(name);
            }
            if let Some(home) = account.and_then(|account| account.home.as_ref()) {
                cmd.env(kimi::HOME_ENV, home);
            } else {
                cmd.env_remove(kimi::HOME_ENV);
            }
        }
        cmd.env(PID_ENV, std::process::id().to_string());
        cmd.env(DEPTH_ENV, depth.saturating_add(1).to_string());
        let child = cmd.spawn()?;
        let pid = Pid::from_child(&child);
        let identity = proc::capture_child(child.id());
        let process = ChildProcess::of(child.id()).ok();
        let words = args
            .iter()
            .map(|a| a.to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>();
        Ok(Self {
            _child: Some(child),
            pid,
            build,
            argv: args.to_vec(),
            words,
            session: None,
            account: account.cloned(),
            identity,
            process,
            started: Instant::now(),
        })
    }

    fn adopt(state: crate::handoff::State, args: &[OsString]) -> Result<Self, String> {
        let crate::handoff::State {
            pid,
            build,
            session,
            account,
            started_for,
            identity,
            ..
        } = state;
        let raw_pid = u32::try_from(pid).map_err(|_| "the handoff's child pid is invalid")?;
        let identity = match identity {
            Some(expected) => match proc::validate_child(&expected) {
                Ok(proc::ChildState::Live | proc::ChildState::Stopped) => expected,
                Ok(proc::ChildState::Unreaped) => {
                    return Err("the handoff's child has already ended".to_string());
                }
                Err(why) => return Err(why),
            },
            None => proc::capture_child(raw_pid).ok_or(
                "the legacy handoff's child cannot be independently captured as a live child",
            )?,
        };
        let pid = Pid::from_raw(pid).ok_or("the handoff's child pid is invalid")?;
        let words = args
            .iter()
            .map(|a| a.to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>();
        let started = Instant::now()
            .checked_sub(started_for)
            .unwrap_or_else(Instant::now);
        Ok(Self {
            _child: None,
            pid,
            build,
            argv: args.to_vec(),
            words,
            session,
            account,
            identity: Some(identity),
            process: ChildProcess::of(raw_pid).ok(),
            started,
        })
    }

    /// Capture the child's current launcher/exec image immediately before a handoff, while
    /// requiring the original kernel birth so no earlier child replacement is hidden.
    fn handoff_identity(&self) -> Option<proc::ChildIdentity> {
        let current = proc::capture_child(u32::try_from(self.pid.as_raw_nonzero().get()).ok()?)?;
        self.identity
            .as_ref()
            .is_none_or(|original| original.birth == current.birth)
            .then_some(current)
    }

    /// The environment the child runs under.
    fn env(&self, base: &Env) -> Env {
        let mut env = base.overlay(
            self.build
                .shim_env
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .chain(kimi::HARNESS_ENV.iter().copied()),
        );
        apply_account(&mut env, self.account.as_ref());
        env
    }

    /// Why this launch cannot be resumed on another build, if it cannot.
    fn launch_refusal(&self) -> Option<kimi::Refusal> {
        match &self.words {
            None => Some(kimi::Refusal::NotUtf8),
            Some(w) => kimi::carried(w).err(),
        }
    }

    fn signal(&self, sig: Signal) {
        let _ = kill_process(self.pid, sig);
    }

    /// The account label the claim announces for this child.
    fn login(&self) -> Option<&str> {
        self.account.as_ref().map(|account| account.label.as_str())
    }

    /// What the claim says about this child: its build, its account and its process.
    fn claim(&self) -> alab_wire::Child<'_> {
        alab_wire::Child {
            vendor: self.build.version.as_deref(),
            login: self.login(),
            process: self.process,
        }
    }
}

/// How long after a move's relaunch an immediate non-zero exit or fault counts as the new build
/// refusing to start (falling back onto the previous build).
const STARTUP_GRACE: Duration = Duration::from_secs(15);

/// A move onto a new build, or a restart aterm asked for, under way: `SIGTERM` was sent.
struct Moving {
    argv: Vec<String>,
    session: String,
    from: String,
    target: Build,
    account: Option<Account>,
    /// When the last signal was sent, on a clock that stops while the machine sleeps; time the
    /// child spends stopped does not count either: [`run`] starts it again when the harness
    /// continues a child that stopped alone, and whenever [`reap`] reports a continue.
    sent: Instant,
    /// For a restart aterm asked for, how a child deaf to the `SIGTERM` is ended.
    restart: Option<Escalation>,
}

/// A restart aterm asked for is made whatever the child does with its `SIGTERM`: aterm's own
/// restart act sends `SIGKILL` to a program still running after its grace
/// (`crates/aterm-gui/src/restart_act.rs` `escalate`), and under this harness's `relaunch`
/// claim aterm leaves that to the holder.
struct Escalation {
    cause: String,
    /// How long after the `SIGTERM` the `SIGKILL` follows.
    kill_after: Duration,
    /// Whether the `SIGKILL` was sent.
    killed: bool,
}

impl Escalation {
    /// aterm's grace: 5 s for a stalled program (it reads nothing, so it cannot take the
    /// signal), the act's 30 s deadline otherwise. `KIMI_ALAB_TERM_GRACE_MS` bounds both.
    fn for_event(event: &Restart, term_grace: Duration) -> Self {
        let kill_after = if event.cause == "stall" {
            STALL_KILL_AFTER.min(term_grace)
        } else {
            term_grace
        };
        Self {
            cause: event.cause.clone(),
            kill_after,
            killed: false,
        }
    }
}

/// aterm's grace before a stalled program's `SIGKILL` (`crates/aterm-gui/src/input_stall.rs`
/// `RESTART_GRACE`).
const STALL_KILL_AFTER: Duration = Duration::from_secs(5);

impl Moving {
    /// How long after `sent` the child must have ended.
    fn limit(&self, term_grace: Duration) -> Duration {
        match &self.restart {
            Some(escalation) if !escalation.killed => escalation.kill_after,
            _ => term_grace,
        }
    }

    /// Whether `status` is the end the harness's own signal gave the child, so the move or the
    /// restart goes on.
    fn ended_by_us(&self, status: Status) -> bool {
        status.after_our_term()
            || self
                .restart
                .as_ref()
                .is_some_and(|escalation| escalation.killed && status == Status::Killed(SIGKILL))
    }
}

/// A build a move just started: if it refuses to start within [`STARTUP_GRACE`], the harness
/// marks it abandoned and falls back onto `prev`.
struct Moved {
    prev: Build,
    argv: Vec<String>,
    session: String,
    account: Option<Account>,
    started: Instant,
}

/// A crash being answered: no child runs, and the harness waits for its next step.
struct Crash {
    /// How the child ended; the harness ends the same way when the crash is passed on.
    status: Status,
    /// The conversation the child last proved by its lock.
    session: String,
    /// The resume argv.
    argv: Vec<String>,
    /// When the back-off's pause is over.
    due: Instant,
    /// What the crash waits for now.
    wait: CrashWait,
    /// Time spent waiting for aterm to answer again, in all, for this crash.
    budget: AwayBudget,
    /// Why aterm did not answer, for the journal when the wait runs out.
    away: String,
}

/// What a crash being answered waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrashWait {
    /// The back-off's pause ([`Crash::due`]); aterm is asked again when it is over.
    Pause,
    /// aterm, which was away or answered `ERR`: asked again at this time, once the link is held.
    Away(Instant),
}

/// What one step of a crash being answered decided.
enum Tick {
    /// Nothing yet.
    Wait,
    /// Start the same build on the conversation now.
    Relaunch,
    /// The crash is not answered with a relaunch; the reason.
    PassOn(String),
}

/// A restart aterm asked for that waits: for the child's session lock, or for aterm to answer.
/// It waits at most [`Settings::restart_wait`] from its arrival, whatever it waits for: a restart
/// is made for the moment aterm asked in, and one made much later (the person's first prompt
/// opening a session an hour on) would end the child mid-turn for a cause long gone. aterm asks
/// again while its cause stands (a stall's remedy fires again).
struct Deferred {
    event: Restart,
    /// When it is refused if it still waits.
    expires: Instant,
    /// When aterm is asked again.
    next_ask: Instant,
    /// Why it waits, journalled once.
    why: String,
}

/// The exit code for a binary that could not be started.
fn spawn_failure_code(e: &std::io::Error) -> i32 {
    if e.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

fn hostname() -> String {
    rustix::system::uname()
        .nodename()
        .to_string_lossy()
        .into_owned()
}

fn own_build_id() -> Option<u64> {
    let exe = std::env::current_exe().ok()?;
    let build = exe.parent()?.parent()?;
    if build.parent()?.file_name()? != NAME {
        return None;
    }
    store::parse_build_id(build.file_name()?.to_str()?)
}

fn probe(pid: Pid) -> Probe {
    let tree = proc::table()
        .ok()
        .and_then(|t| proc::tree_of(&t, pid.as_raw_nonzero().get()));
    Probe {
        at: Instant::now(),
        tty: tty::probe(),
        tree,
    }
}

/// The one session the child holds open, named by its Kimi session lock.
fn session_of(env: &Env, tree: Option<&proc::Tree>, host: &str) -> Result<String, String> {
    if !kimi::locks_enabled(env) {
        return Err(format!(
            "{} is off, so the running session cannot be named",
            kimi::SESSION_LOCK_ENV
        ));
    }
    let home = kimi::home(env)?;
    let tree = tree.ok_or("the child's process tree could not be read")?;
    let held = kimi::held_sessions(&home, &tree.pids, host)?;
    match held.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(String::from(
            "the child holds no session lock (no session started yet, or its locks are unavailable)",
        )),
        many => Err(format!(
            "the child holds {} sessions ({})",
            many.len(),
            many.join(", ")
        )),
    }
}

fn direct_session_of(env: &Env, pid: Pid, host: &str) -> Result<String, String> {
    if !kimi::locks_enabled(env) {
        return Err(format!("{} is off", kimi::SESSION_LOCK_ENV));
    }
    let home = kimi::home(env)?;
    let pids = BTreeSet::from([pid.as_raw_nonzero().get()]);
    let held = kimi::held_sessions(&home, &pids, host)?;
    match held.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err("the child holds no direct session lock".to_string()),
        many => Err(format!("the child holds {} direct sessions", many.len())),
    }
}

fn refresh_session(cur: &mut Running, base: &Env, host: &str, journal: &mut Journal) {
    let env = cur.env(base);
    let next = direct_session_of(&env, cur.pid, host).or_else(|_| {
        let tree = proc::table()
            .ok()
            .and_then(|table| proc::tree_of(&table, cur.pid.as_raw_nonzero().get()));
        session_of(&env, tree.as_ref(), host)
    });
    let next = next.ok();
    if next.is_some() && cur.session != next {
        if let Some(session) = &next {
            journal.note("SESSION", &format!("the child holds session {session}"));
        }
        cur.session = next;
    }
}

/// What aterm's restart request came to.
#[allow(
    clippy::large_enum_variant,
    reason = "made once and moved rarely; a Box would put every access behind a heap deref the Trust verifier fails closed on"
)]
enum Asked {
    /// The child was sent `SIGTERM`; the restart is under way.
    Moving(Moving),
    /// It was refused, and journalled.
    Refused,
    /// It waits: for the child's session lock, or for aterm to answer.
    Waits { why: String },
}

/// aterm asked the claim holder to restart its child (`restart cause=<cause>`): end the child
/// with `SIGTERM` now (and `SIGKILL` if it does not end, [`Escalation`]), and resume the same
/// session when it has ended. The request is aterm's, or a person's through aterm, so no safe
/// point is waited for, and it is no crash.
///
/// It is made only when the child's lock has named its session, the resume argv can be written,
/// the restart names no model, and one fresh policy and status read permits it
/// (`alab_wire::relaunch::permitted` with `asked`): the claim is this harness's, proven and takes
/// `relaunch`, the policy stands, and nobody holds the session (no halt, no other driver's
/// hand). A request with no session yet, or one aterm could not answer, waits ([`Deferred`]).
/// One is refused at once, whatever else it waits for, while aterm's last answer to a claim made
/// again is that it serves no contract this build declares ([`aterm::unserved`]): no claim
/// stands, and none can until aterm serves one.
fn restart_now(
    cur: &Running,
    link: &mut Link,
    event: &Restart,
    term_grace: Duration,
    journal: &mut Journal,
) -> Asked {
    if let Some(why) = aterm::unserved(link) {
        journal.note(
            "RESTART-REFUSED",
            &format!("cause={} no-claim {why}", event.cause),
        );
        return Asked::Refused;
    }
    let Some(session) = cur.session.clone() else {
        return Asked::Waits {
            why: "no running child's session lock names the conversation".to_string(),
        };
    };
    let words = cur.words.clone().unwrap_or_default();
    let argv = match kimi::resume_argv(&words, &session) {
        Ok(argv) => argv,
        Err(why) => {
            journal.note("RESTART-REFUSED", &format!("cause={} {why}", event.cause));
            return Asked::Refused;
        }
    };
    if let Some(model) = &event.model {
        // Kimi Code would be started on the model it ran, which answers a model move with the
        // model aterm moved it from. Choosing Kimi's model on a resume is not built yet.
        journal.note(
            "RESTART-REFUSED",
            &format!(
                "cause={} model={model}: {NAME} cannot yet start {} on a model aterm names",
                event.cause,
                kimi::COMMAND
            ),
        );
        return Asked::Refused;
    }
    let word = link.relaunch_word(true, aterm::now_ms());
    aterm::journal_notes(link, journal);
    match word {
        Relaunch::Permitted => {}
        Relaunch::Denied(why) => {
            journal.note("RESTART-REFUSED", &format!("cause={} {why}", event.cause));
            return Asked::Refused;
        }
        Relaunch::Away(why) | Relaunch::Later(why) => {
            return Asked::Waits {
                why: format!("aterm did not answer: {why}"),
            };
        }
    }
    let to = event
        .account
        .as_ref()
        .map_or_else(String::new, |account| format!(" to={}", account.label));
    journal.note(
        "RESTART",
        &format!("cause={}{to}: resuming session {session}", event.cause),
    );
    cur.signal(Signal::TERM);
    Asked::Moving(Moving {
        argv,
        session,
        from: cur.build.version_label(),
        target: cur.build.clone(),
        account: event.account.clone().or_else(|| cur.account.clone()),
        sent: Instant::now(),
        restart: Some(Escalation::for_event(event, term_grace)),
    })
}

/// A restart aterm asked for waited [`Settings::restart_wait`] and is refused: journal it with
/// what it last waited for.
fn restart_expired(waiting: &Deferred, why: &str, limit: Duration, journal: &mut Journal) {
    journal.note(
        "RESTART-REFUSED",
        &format!(
            "cause={} not made within {} ms: {why}",
            waiting.event.cause,
            limit.as_millis()
        ),
    );
}

/// A new child was started and is not announced yet: a restart aterm asked for until now was
/// meant for the child before it, so it is passed over and never applied to this one, as
/// `Link::events` says. That is one still waiting ([`Deferred`]), and one aterm sent as that
/// child ended (read in the same wake as its end, or while the next child was being started)
/// and not yet drained.
fn pass_over_earlier(
    cur: &Running,
    link: &mut Link,
    deferred: &mut Option<Deferred>,
    journal: &mut Journal,
) {
    if let Some(waiting) = deferred.take() {
        journal.note(
            "RESTART-PASSED-OVER",
            &format!(
                "cause={} it waited for the child before this one",
                waiting.event.cause
            ),
        );
    }
    let now = aterm::now_ms();
    link.tick(cur.claim(), now);
    let drained = link.events(cur.claim(), now);
    aterm::journal_notes(link, journal);
    if let Some(event) = drained.restart {
        journal.note(
            "RESTART-PASSED-OVER",
            &format!(
                "cause={} it came before this child was announced",
                event.cause
            ),
        );
    }
}

/// Whether the child's end `status` is a crash the harness now answers: nobody passed it `HUP`
/// or `TERM` (`ending`), the end is abnormal (`alab_wire::relaunch::abnormal_exit`), the session
/// has a claim and aterm did not last answer that it serves none of this build's contracts
/// ([`aterm::unserved`]; an answer read after the crash began passes it on in [`crash_tick`]),
/// the child's lock named its session, and the launch can be resumed.
/// `Err(None)` for an end that is the session's own, `Err(Some(why))` for an abnormal end passed
/// on.
fn crash_begins(
    status: Status,
    cur: &Running,
    link: &Link,
    ending: Option<i32>,
) -> Result<(String, Vec<String>), Option<String>> {
    if ending.is_some() || !alab_wire::relaunch::abnormal_exit(status.end()) {
        return Err(None);
    }
    if !link.claims() {
        return Err(Some("no-claim".to_string()));
    }
    if let Some(why) = aterm::unserved(link) {
        // aterm's last word is that it serves no contract this build declares: no claim stands
        // and none is made until it does, so the relaunch duty is not this harness's.
        return Err(Some(format!("no-claim {why}")));
    }
    let Some(session) = cur.session.clone() else {
        return Err(Some(
            "no-session: the child's lock never named its conversation".to_string(),
        ));
    };
    let words = cur.words.clone().unwrap_or_default();
    let argv = kimi::resume_argv(&words, &session)
        .map_err(|why| Some(format!("cannot resume session {session}: {why}")))?;
    Ok((session, argv))
}

/// Ask aterm whether the crashed child may be brought back, and say what follows: a relaunch
/// once the pause is over, a wait, or the crash passed on. aterm away or answering `ERR` is
/// waited for, asked again every [`ASK_AGAIN`] once the link is held, within
/// `alab_wire::relaunch::AWAY_BUDGET` of time actually spent waiting for this crash.
fn crash_judged(crash: &mut Crash, link: &mut Link, journal: &mut Journal) -> Tick {
    let now = Instant::now();
    let ms = aterm::now_ms();
    let word = link.relaunch_word(false, ms);
    aterm::journal_notes(link, journal);
    match word {
        Relaunch::Permitted => {
            crash.budget.answered(ms);
            if now >= crash.due {
                Tick::Relaunch
            } else {
                crash.wait = CrashWait::Pause;
                Tick::Wait
            }
        }
        Relaunch::Denied(why) => Tick::PassOn(why),
        Relaunch::Away(why) | Relaunch::Later(why) => {
            if crash.budget.wait(ms) {
                journal.note(
                    "CRASH-WAIT",
                    &format!("aterm did not answer ({why}); asking again within the 45 s budget"),
                );
            }
            crash.away = why;
            if crash.budget.spent(ms) {
                return Tick::PassOn(format!("claim-not-proven-again {}", crash.away));
            }
            crash.wait = CrashWait::Away(now + ASK_AGAIN);
            Tick::Wait
        }
    }
}

/// One step of a crash being answered, on every pass of the loop while no child runs. It
/// never waits.
///
/// A crash that began while the link was lost, before aterm answered the claim made again that
/// it serves no contract this build declares ([`aterm::unserved`]), is passed on as soon as that
/// answer is read, as [`crash_begins`] passes on one that began after it: no claim can come
/// until aterm serves one, so the wait for aterm would only run out the 45 s budget.
fn crash_tick(crash: &mut Crash, link: &mut Link, journal: &mut Journal) -> Tick {
    if let Some(why) = aterm::unserved(link) {
        return Tick::PassOn(format!("no-claim {why}"));
    }
    let now = Instant::now();
    match crash.wait {
        CrashWait::Pause if now < crash.due => Tick::Wait,
        CrashWait::Away(next) if now < next || link.is_lost() => {
            if crash.budget.spent(aterm::now_ms()) {
                Tick::PassOn(format!("claim-not-proven-again {}", crash.away))
            } else {
                Tick::Wait
            }
        }
        CrashWait::Pause | CrashWait::Away(_) => crash_judged(crash, link, journal),
    }
}

/// aterm's word at a move's stop point: its whole safe point when it attested (`by_aterm`), and
/// under a claim at least no halt, no other driver's hand and the claim still this harness's with
/// the upgrade duty when a person attested by hand. A link that stops answering here holds the move, whoever
/// attested; with no claim standing, a person's attestation stands on the harness's own readings.
fn stop_point(link: &mut Link, by_aterm: bool, journal: &mut Journal) -> Result<(), String> {
    let judged = link.judges();
    let holder = link.config().holder(std::process::id());
    let now = aterm::now_ms();
    let word = match (judged, by_aterm) {
        (false, true) => Err("aterm's claim was lost".to_string()),
        (false, false) => Ok(()),
        (true, true) => match link.safe_point(Duration::ZERO, now) {
            Word::Safe => Ok(()),
            Word::Hold(why) => Err(why),
            Word::Local => Err("aterm's claim was lost".to_string()),
        },
        (true, false) => match link.read(now) {
            Reading::Fresh(facts) => aterm::unheld(&facts, &holder),
            Reading::Hold(why) => Err(why),
            Reading::Local if link.is_lost() => Err("aterm's connection failed".to_string()),
            Reading::Local => Ok(()),
        },
    };
    aterm::journal_notes(link, journal);
    word
}

/// How soon aterm is asked again about a crash or an asked restart while it could not answer.
const ASK_AGAIN: Duration = Duration::from_millis(LATER_RETRY_MS);

/// The crash is not answered with a relaunch: journal why, drop whatever was typed at the dead
/// screen (so a parent shell does not run it), and end as the child did.
fn crash_passed_on(crash: &Crash, why: &str, journal: &mut Journal) -> Outcome {
    journal.note(
        "CRASH-PASSED-ON",
        &format!("{why} (the child ended: {})", crash.status),
    );
    tty::discard_input();
    crash.status.outcome()
}

/// The signals that end the harness while no child runs: whoever sends them wants the session
/// over, and a relaunch would fight them.
const fn ends_a_crash(sig: i32) -> bool {
    matches!(sig, SIGINT | SIGQUIT | SIGHUP | SIGTERM)
}

/// How the supervisor obtains its vendor child.
#[allow(
    clippy::large_enum_variant,
    reason = "made once and moved rarely; a Box would put every access behind a heap deref the Trust verifier fails closed on"
)]
pub enum Start {
    /// Spawn this build as a new child.
    Fresh(Build),
    /// Adopt the child preserved by the previous harness image.
    Adopt(crate::handoff::State),
}

/// Run a fresh or adopted child with the launch words `launch` until the session really ends.
#[allow(clippy::too_many_lines)]
pub fn run(
    settings: &Settings,
    start: Start,
    launch: &[OsString],
    link: Link,
    journal: &mut Journal,
) -> Outcome {
    let mut signals = match Signals::install() {
        Ok(s) => s,
        Err(e) => {
            say(&format!("cannot install signal handlers: {e}"));
            return Outcome::Exit(1);
        }
    };
    let modes = tty::Modes::save();
    let (mut cur, mut ladder, mut abandoned, event) = match start {
        Start::Fresh(first) => {
            let exe = first.exe.clone();
            match Running::start(first, launch, settings.depth, None) {
                Ok(running) => (running, Ladder::new(), BTreeSet::new(), "START"),
                Err(e) => {
                    say(&format!("cannot run {}: {e}", exe.display()));
                    return Outcome::Exit(spawn_failure_code(&e));
                }
            }
        }
        Start::Adopt(state) => {
            let ladder = Ladder::carried(state.crash_streak, state.crash_since, aterm::now_ms());
            let abandoned = state.abandoned.clone();
            match Running::adopt(state, launch) {
                Ok(running) => (running, ladder, abandoned, "ADOPT"),
                Err(why) => {
                    journal.note("ADOPT-FAILED", &why);
                    say(&format!("cannot adopt the running child: {why}"));
                    return Outcome::Exit(126);
                }
            }
        }
    };
    journal.note(
        event,
        &format!(
            "child={} {} {} ({}) interactive={}",
            cur.pid.as_raw_nonzero(),
            kimi::COMMAND,
            cur.build.version_label(),
            cur.build.exe.display(),
            cur.launch_refusal().is_none()
        ),
    );
    let host = hostname();
    let knobs = settings.knobs;
    let mut link = link;
    let mut deferred: Option<Deferred> = None;
    if event == "START" {
        // The claim was made before the child existed; name it now (`child=` under contract 4).
        // An adopted child was named in its claim already.
        pass_over_earlier(&cur, &mut link, &mut deferred, journal);
        link.reannounce(cur.claim(), aterm::now_ms());
        aterm::journal_notes(&mut link, journal);
    }
    let mut claim_tick = Instant::now()
        .checked_sub(settings.poll)
        .unwrap_or_else(Instant::now);
    let mut aterm_attest = false;
    let mut moving: Option<Moving> = None;
    let mut moved: Option<Moved> = None;
    let mut crash: Option<Crash> = None;
    let mut pending: Option<Build> = None;
    let own_id = own_build_id().or_else(|| store::current_id(&settings.prefix, NAME));
    let mut own_pending: Option<Build> = None;
    let mut own_abandoned = BTreeSet::new();
    let mut own_noted_unready: Option<u64> = None;
    let mut noted_unready: Option<u64> = None;
    let mut noted_refusal = false;
    let mut attest: Option<Attestation> = None;
    let mut window: Option<Probe> = None;
    let mut ending: Option<i32> = None;

    loop {
        signals.wait(settings.poll);
        let caught = signals.take();

        // A crash being answered: no child runs.
        if let Some(answering) = crash.as_mut() {
            if let Some(sig) = caught.iter().copied().find(|sig| ends_a_crash(*sig)) {
                return crash_passed_on(answering, &format!("signal {sig}"), journal);
            }
            link.tick(cur.claim(), aterm::now_ms());
            let drained = link.events(cur.claim(), aterm::now_ms());
            aterm::journal_notes(&mut link, journal);
            if let Some(event) = drained.restart {
                journal.note(
                    "RESTART-REFUSED",
                    &format!("cause={} crash-pending: no child runs", event.cause),
                );
            }
            match crash_tick(answering, &mut link, journal) {
                Tick::Wait => continue,
                Tick::PassOn(why) => return crash_passed_on(answering, &why, journal),
                Tick::Relaunch => {}
            }
            // A signal caught since the last look still stops the relaunch it was sent to stop.
            if let Some(sig) = signals.take().into_iter().find(|sig| ends_a_crash(*sig)) {
                return crash_passed_on(answering, &format!("signal {sig}"), journal);
            }
            tty::discard_input();
            modes.restore();
            let streak = ladder.streak().saturating_add(1);
            journal.note(
                "CRASH-RELAUNCH",
                &format!(
                    "resuming session {} after {} streak={streak}",
                    answering.session, answering.status
                ),
            );
            let mv = Moving {
                argv: answering.argv.clone(),
                session: answering.session.clone(),
                from: cur.build.version_label(),
                target: cur.build.clone(),
                account: cur.account.clone(),
                sent: Instant::now(),
                restart: None,
            };
            match relaunch(settings, &cur, &mv, true, &mut abandoned, journal) {
                Ok(next) => {
                    ladder.relaunched(aterm::now_ms());
                    cur = next;
                    pass_over_earlier(&cur, &mut link, &mut deferred, journal);
                    link.reannounce(cur.claim(), aterm::now_ms());
                    aterm::journal_notes(&mut link, journal);
                    crash = None;
                    pending = None;
                    attest = None;
                    aterm_attest = false;
                    window = None;
                    noted_refusal = false;
                    continue;
                }
                Err(outcome) => return outcome,
            }
        }

        for sig in caught {
            match sig {
                SIGHUP | SIGTERM => {
                    ending.get_or_insert(sig);
                    let s = if sig == SIGHUP {
                        Signal::HUP
                    } else {
                        Signal::TERM
                    };
                    cur.signal(s);
                    attest = None;
                    window = None;
                    journal.note(
                        "FORWARD",
                        &format!("signal {sig} passed to the child; the session ends with it"),
                    );
                }
                SIGINT | SIGQUIT => {
                    journal.note("IGNORED", &format!("signal {sig} belongs to the child"));
                }
                SIGUSR1 => {
                    attest = Some(Attestation::now());
                    aterm_attest = false;
                    window = None;
                    journal.note(
                        "ATTEST",
                        if pending.is_some() || own_pending.is_some() {
                            "idle attested; an update is waiting"
                        } else {
                            "idle attested; no update is waiting"
                        },
                    );
                }
                _ => {}
            }
        }

        // The child first: an end already read is never mistaken for a running child below.
        let reaped = reap(cur.pid);
        let still_running = matches!(reaped, Reap::Running | Reap::Continued);
        match reaped {
            Reap::Running => {}
            Reap::Continued => {
                // Time the child spent stopped is not time it had to act on a signal, whoever
                // stopped it: the grace of a SIGTERM or SIGKILL sent starts again.
                if let Some(mv) = moving.as_mut() {
                    mv.sent = Instant::now();
                    journal.note(
                        "CONTINUED",
                        "the child was continued; the grace of the signal sent starts again",
                    );
                }
            }
            Reap::Stopped => {
                journal.note("STOP", "the child stopped; the harness stops with it");
                let _ = signal_hook::low_level::raise(SIGTSTP);
                cur.signal(Signal::CONT);
                // A stopped child acts on no signal but SIGKILL, so its time stopped is not
                // time it had to end: the grace of a SIGTERM or SIGKILL sent starts again.
                if let Some(mv) = moving.as_mut() {
                    mv.sent = Instant::now();
                }
                journal.note("CONT", "continued; the child is continued too");
                continue;
            }
            Reap::Ended(status) => {
                if matches!(status, Status::Killed(_)) {
                    modes.restore();
                }
                journal.note("END", &format!("the child ended: {status}"));
                let Some(mv) = moving.take() else {
                    if let Some(jm) = moved.take()
                        && ending.is_none()
                        && jm.started.elapsed() <= STARTUP_GRACE
                        && status.refused_start()
                    {
                        modes.restore();
                        abandoned.insert(cur.build.id);
                        journal.note(
                            "MOVE-FAILED",
                            &format!(
                                "{} ended ({status}) {} ms after relaunch; falling back to {}",
                                cur.build.version_label(),
                                jm.started.elapsed().as_millis(),
                                jm.prev.version_label()
                            ),
                        );
                        let prev = jm.prev.clone();
                        let back = Moving {
                            argv: jm.argv,
                            session: jm.session,
                            from: cur.build.version_label(),
                            target: jm.prev,
                            account: jm.account,
                            sent: jm.started,
                            restart: None,
                        };
                        match relaunch(settings, &cur, &back, true, &mut abandoned, journal) {
                            Ok(next) => {
                                moved = (next.build.id != prev.id).then(|| Moved {
                                    prev,
                                    argv: back.argv,
                                    session: back.session,
                                    account: cur.account.clone(),
                                    started: Instant::now(),
                                });
                                cur = next;
                                pass_over_earlier(&cur, &mut link, &mut deferred, journal);
                                link.reannounce(cur.claim(), aterm::now_ms());
                                aterm::journal_notes(&mut link, journal);
                                pending = None;
                                attest = None;
                                aterm_attest = false;
                                window = None;
                                noted_refusal = false;
                                continue;
                            }
                            Err(outcome) => return outcome,
                        }
                    }
                    let (session, argv) = match crash_begins(status, &cur, &link, ending) {
                        Ok(found) => found,
                        Err(None) => return status.outcome(),
                        Err(Some(why)) => {
                            journal.note("CRASH-PASSED-ON", &format!("{why} ({status})"));
                            return status.outcome();
                        }
                    };
                    // The crashed child left the terminal in its own modes, in which a key
                    // sends no signal: put the harness's back at once, so Ctrl-C reaches the
                    // harness for as long as no child runs.
                    modes.restore();
                    tty::release_keyboard();
                    if let Some(waiting) = deferred.take() {
                        journal.note(
                            "RESTART-REFUSED",
                            &format!("cause={} child-ended", waiting.event.cause),
                        );
                    }
                    let now = Instant::now();
                    let pause = ladder.crashed(aterm::now_ms());
                    journal.note(
                        "CRASH",
                        &format!(
                            "{status}: session {session} streak={} pause-ms={}",
                            ladder.streak(),
                            pause.as_millis()
                        ),
                    );
                    let mut answering = Crash {
                        status,
                        session,
                        argv,
                        due: now + pause,
                        wait: CrashWait::Pause,
                        budget: AwayBudget::new(),
                        away: String::new(),
                    };
                    match crash_judged(&mut answering, &mut link, journal) {
                        Tick::PassOn(why) => return crash_passed_on(&answering, &why, journal),
                        Tick::Wait | Tick::Relaunch => {}
                    }
                    crash = Some(answering);
                    attest = None;
                    aterm_attest = false;
                    window = None;
                    continue;
                };
                if ending.is_some() || !mv.ended_by_us(status) {
                    say(&format!(
                        "{} ended ({status}) while being moved to a new build; resume it with: {} -r {}",
                        kimi::COMMAND,
                        kimi::COMMAND,
                        mv.session
                    ));
                    return status.outcome();
                }
                modes.restore();
                match relaunch(settings, &cur, &mv, false, &mut abandoned, journal) {
                    Ok(next) => {
                        moved = (next.build.id != cur.build.id).then(|| Moved {
                            prev: cur.build.clone(),
                            argv: mv.argv,
                            session: mv.session,
                            account: cur.account.clone(),
                            started: Instant::now(),
                        });
                        cur = next;
                        pass_over_earlier(&cur, &mut link, &mut deferred, journal);
                        link.reannounce(cur.claim(), aterm::now_ms());
                        aterm::journal_notes(&mut link, journal);
                        pending = None;
                        attest = None;
                        aterm_attest = false;
                        window = None;
                        noted_refusal = false;
                        continue;
                    }
                    Err(outcome) => return outcome,
                }
            }
        }

        if still_running {
            refresh_session(&mut cur, &settings.env, &host, journal);
        }

        link.tick(cur.claim(), aterm::now_ms());
        let drained = link.events(cur.claim(), aterm::now_ms());
        aterm::journal_notes(&mut link, journal);
        if moving.is_none() && ending.is_none() {
            let asked = drained.restart.map(|event| Deferred {
                event,
                expires: Instant::now() + settings.restart_wait,
                next_ask: Instant::now(),
                why: String::new(),
            });
            if asked.is_some() {
                deferred = asked;
            }
            let now = Instant::now();
            let due = deferred
                .as_ref()
                .is_some_and(|waiting| now >= waiting.next_ask);
            if due && let Some(mut waiting) = deferred.take() {
                match restart_now(
                    &cur,
                    &mut link,
                    &waiting.event,
                    settings.term_grace,
                    journal,
                ) {
                    Asked::Moving(mv) => {
                        moving = Some(mv);
                        pending = None;
                        attest = None;
                        aterm_attest = false;
                        window = None;
                    }
                    Asked::Refused => {}
                    Asked::Waits { why } => {
                        if now >= waiting.expires {
                            restart_expired(&waiting, &why, settings.restart_wait, journal);
                        } else {
                            if waiting.why != why {
                                journal.note(
                                    "RESTART-WAITS",
                                    &format!("cause={} {why}", waiting.event.cause),
                                );
                                waiting.why = why;
                            }
                            waiting.next_ask = now + ASK_AGAIN;
                            deferred = Some(waiting);
                        }
                    }
                }
            } else if let Some(waiting) = deferred.as_ref()
                && now >= waiting.expires
            {
                restart_expired(waiting, &waiting.why, settings.restart_wait, journal);
                deferred = None;
            }
        }

        if let Some(mv) = moving.as_mut()
            && mv.sent.elapsed() > mv.limit(settings.term_grace)
        {
            match mv.restart.as_mut() {
                Some(escalation) if !escalation.killed => {
                    journal.note(
                        "RESTART-KILL",
                        &format!(
                            "cause={} {} pid {} did not end within {} ms of SIGTERM; SIGKILL",
                            escalation.cause,
                            kimi::COMMAND,
                            cur.pid.as_raw_nonzero(),
                            escalation.kill_after.as_millis()
                        ),
                    );
                    cur.signal(Signal::KILL);
                    escalation.killed = true;
                    mv.sent = Instant::now();
                }
                Some(escalation) => {
                    journal.note(
                        "RESTART-ABANDONED",
                        &format!(
                            "cause={} {} pid {} still runs {} ms after SIGKILL",
                            escalation.cause,
                            kimi::COMMAND,
                            cur.pid.as_raw_nonzero(),
                            settings.term_grace.as_millis()
                        ),
                    );
                    moving = None;
                }
                None => {
                    journal.note(
                        "MOVE-ABANDONED",
                        &format!(
                            "{} pid {} did not end within {} ms of SIGTERM; the move onto {} is abandoned and not tried again",
                            kimi::COMMAND,
                            cur.pid.as_raw_nonzero(),
                            settings.term_grace.as_millis(),
                            mv.target.version_label()
                        ),
                    );
                    abandoned.insert(mv.target.id);
                    moving = None;
                    pending = None;
                    window = None;
                }
            }
        }

        if moving.is_some() || ending.is_some() || settings.no_update {
            continue;
        }
        if moved
            .as_ref()
            .is_some_and(|jm| jm.started.elapsed() > STARTUP_GRACE)
        {
            moved = None;
        }

        // The store: has atpkg made another build current?
        match store::current_id(&settings.prefix, kimi::PROGRAM) {
            Some(id) if id != cur.build.id && !abandoned.contains(&id) => {
                if pending.as_ref().map(|b| b.id) != Some(id) {
                    match store::current(&settings.prefix, kimi::PROGRAM, kimi::COMMAND) {
                        Ok(b) if b.id == id && store::is_this_harness(&b.exe) => {
                            if noted_unready != Some(id) {
                                journal.note(
                                    "UPDATE-REFUSED",
                                    &format!(
                                        "{} is this harness itself; it is never run as {}",
                                        b.exe.display(),
                                        kimi::COMMAND
                                    ),
                                );
                                noted_unready = Some(id);
                            }
                        }
                        Ok(b) if b.id == id => {
                            journal.note(
                                "UPDATE",
                                &format!(
                                    "{} {} -> {} is current; waiting for a safe point",
                                    kimi::COMMAND,
                                    cur.build.version_label(),
                                    b.version_label()
                                ),
                            );
                            pending = Some(b);
                            window = None;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            if noted_unready != Some(id) {
                                journal.note("UPDATE-UNREADY", &e.to_string());
                                noted_unready = Some(id);
                            }
                        }
                    }
                }
            }
            Some(_) if pending.is_some() => {
                pending = None;
                journal.note("UPDATE-WITHDRAWN", "the running build is current again");
                window = None;
            }
            Some(_) | None => {}
        }
        match store::current_id(&settings.prefix, NAME) {
            Some(id) if Some(id) != own_id && !own_abandoned.contains(&id) => {
                if own_pending.as_ref().map(|b| b.id) != Some(id) {
                    match store::current(&settings.prefix, NAME, NAME) {
                        Ok(build) if build.id == id => {
                            journal.note(
                                "SELF-UPDATE",
                                &format!(
                                    "harness build {} is current; waiting for a safe point",
                                    build.id
                                ),
                            );
                            own_pending = Some(build);
                            window = None;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            if own_noted_unready != Some(id) {
                                journal.note("SELF-UPDATE-UNREADY", &error.to_string());
                                own_noted_unready = Some(id);
                            }
                        }
                    }
                }
            }
            Some(_) if own_pending.is_some() => {
                own_pending = None;
                journal.note(
                    "SELF-UPDATE-WITHDRAWN",
                    "the running image is current again",
                );
                window = None;
            }
            Some(_) | None => {}
        }
        if pending.is_none() && own_pending.is_none() {
            continue;
        }
        if pending.is_some()
            && let Some(why) = cur.launch_refusal()
        {
            if !noted_refusal {
                journal.note(
                    "UPDATE-WAITS",
                    &format!("{why}; the new build runs from the next launch"),
                );
                noted_refusal = true;
            }
            continue;
        }

        if claim_tick.elapsed() >= settings.poll {
            claim_tick = Instant::now();
            let word = link
                .judges()
                .then(|| link.safe_point(Duration::ZERO, aterm::now_ms()));
            aterm::journal_notes(&mut link, journal);
            match word {
                Some(Word::Safe) if attest.is_none() => {
                    attest = Some(Attestation::now());
                    aterm_attest = true;
                    window = None;
                    journal.note("ATTEST", "aterm attested an idle, empty, unheld session");
                }
                // The link was lost or given back: an attestation aterm made no longer stands.
                Some(Word::Local) if aterm_attest => {
                    attest = None;
                    aterm_attest = false;
                    window = None;
                }
                Some(Word::Safe | Word::Hold(_) | Word::Local) | None => {}
            }
        }

        // The safe point.
        let Some(att) = attest else {
            continue;
        };
        let now = Instant::now();
        if !safepoint::fresh(&att, now, &knobs) {
            journal.note(
                "ATTEST-LAPSED",
                "no safe point within the attestation's life",
            );
            attest = None;
            aterm_attest = false;
            window = None;
            continue;
        }
        let reading = probe(cur.pid);
        let start = window.get_or_insert_with(|| reading.clone());
        match safepoint::judge(&att, start, &reading, &knobs) {
            Verdict::Wait => {}
            Verdict::Busy(why) => {
                journal.note(
                    "REFUSED",
                    &format!("busy: {why}; the update waits for a new attestation"),
                );
                attest = None;
                aterm_attest = false;
                window = None;
            }
            Verdict::Safe => {
                // aterm's word at the stop point: its whole safe point when it attested, and
                // under a claim at least no halt, no other driver's hand and the upgrade duty
                // granted when a person did.
                let host_check = stop_point(&mut link, aterm_attest, journal);
                if let Some(target) = own_pending.clone().filter(|_| moved.is_none()) {
                    match host_check {
                        Ok(()) => {
                            let state = crate::handoff::State {
                                pid: cur.pid.as_raw_nonzero().get(),
                                build: cur.build.clone(),
                                session: cur.session.clone(),
                                account: cur.account.clone(),
                                started_for: cur.started.elapsed(),
                                crash_streak: ladder.streak(),
                                crash_since: ladder.since(aterm::now_ms()),
                                abandoned: abandoned.clone(),
                                identity: cur.handoff_identity(),
                            };
                            journal.note(
                                "REEXEC",
                                &format!(
                                    "safe point: replacing the harness image with {}; child {} runs on",
                                    target.exe.display(),
                                    cur.pid.as_raw_nonzero()
                                ),
                            );
                            let error = crate::handoff::exec(&target.exe, &cur.argv, &state);
                            journal.note(
                                "REEXEC-FAILED",
                                &format!("{error}; the old harness and child still run"),
                            );
                            own_abandoned.insert(target.id);
                            own_pending = None;
                        }
                        Err(why) => journal.note(
                            "REFUSED",
                            &format!("{why}; the harness update waits for a new attestation"),
                        ),
                    }
                } else if let Some(target) = pending.as_ref() {
                    let child_env = cur.env(&settings.env);
                    let words = cur.words.clone().unwrap_or_default();
                    let plan = host_check.and_then(|()| {
                        session_of(&child_env, reading.tree.as_ref(), &host)
                            .or_else(|_| {
                                cur.session.clone().ok_or_else(|| {
                                    "no current or most recently proven session lock names the conversation"
                                        .to_string()
                                })
                            })
                            .and_then(|session| {
                                kimi::resume_argv(&words, &session)
                                    .map(|argv| (session, argv))
                                    .map_err(|e| e.to_string())
                            })
                    });
                    match plan {
                        Ok((session, argv)) => {
                            journal.note(
                                "MOVE",
                                &format!(
                                    "safe point: SIGTERM to {} pid {}; then {} {} on {}",
                                    kimi::COMMAND,
                                    cur.pid.as_raw_nonzero(),
                                    kimi::COMMAND,
                                    argv.join(" "),
                                    target.version_label()
                                ),
                            );
                            moving = Some(Moving {
                                argv,
                                session,
                                from: cur.build.version_label(),
                                target: target.clone(),
                                account: cur.account.clone(),
                                sent: Instant::now(),
                                restart: None,
                            });
                            cur.signal(Signal::TERM);
                        }
                        Err(why) => journal.note(
                            "REFUSED",
                            &format!("{why}; the update waits for a new attestation"),
                        ),
                    }
                }
                attest = None;
                aterm_attest = false;
                window = None;
            }
        }
    }
}

/// Start the resumed session on the build atpkg has current now, else the one the move was
/// decided for, else the old one — each re-checked as a runnable store build first (when
/// `prefer_target` is true for a startup-failure fallback, `mv.target` is tried before
/// `store::current`).
fn relaunch(
    settings: &Settings,
    old: &Running,
    mv: &Moving,
    prefer_target: bool,
    abandoned: &mut BTreeSet<u64>,
    journal: &mut Journal,
) -> Result<Running, Outcome> {
    let args: Vec<OsString> = mv.argv.iter().map(OsString::from).collect();
    let mut candidates: Vec<Build> = Vec::new();
    if !prefer_target
        && let Ok(b) = store::current(&settings.prefix, kimi::PROGRAM, kimi::COMMAND)
        && !abandoned.contains(&b.id)
    {
        candidates.push(b);
    }
    if candidates.iter().all(|c| c.id != mv.target.id) && !abandoned.contains(&mv.target.id) {
        if let Ok(again) = store::build_at(&mv.target.dir, mv.target.id, kimi::COMMAND) {
            candidates.push(again);
        } else {
            abandoned.insert(mv.target.id);
        }
    }
    if prefer_target
        && let Ok(b) = store::current(&settings.prefix, kimi::PROGRAM, kimi::COMMAND)
        && candidates.iter().all(|c| c.id != b.id)
        && !abandoned.contains(&b.id)
    {
        candidates.push(b);
    }
    if candidates.iter().all(|c| c.id != old.build.id)
        && !abandoned.contains(&old.build.id)
        && let Ok(again) = store::build_at(&old.build.dir, old.build.id, kimi::COMMAND)
    {
        candidates.push(again);
    }
    // A candidate that is this harness itself is never run (Running::start refuses it too);
    // skipping it here keeps its "resuming" line off the terminal.
    candidates.retain(|b| {
        let me = store::is_this_harness(&b.exe);
        if me {
            abandoned.insert(b.id);
            journal.note(
                "RELAUNCH-SKIPPED",
                &format!("{} is this harness itself", b.exe.display()),
            );
        }
        !me
    });
    let mut last_err = None;
    for build in candidates {
        let id = build.id;
        let label = build.version_label();
        say(&format!(
            "{} {} -> {label}: resuming session {} on it",
            kimi::PRODUCT,
            mv.from,
            mv.session
        ));
        match Running::start(build, &args, settings.depth, mv.account.as_ref()) {
            Ok(next) => {
                journal.note(
                    "RELAUNCHED",
                    &format!(
                        "child={} {} {label} session {}",
                        next.pid.as_raw_nonzero(),
                        kimi::COMMAND,
                        mv.session
                    ),
                );
                return Ok(next);
            }
            Err(e) => {
                journal.note("RELAUNCH-FAILED", &format!("{label}: {e}"));
                abandoned.insert(id);
                last_err = Some(e);
            }
        }
    }
    let code = last_err.as_ref().map_or(127, spawn_failure_code);
    say(&format!(
        "could not start {} again; resume the session by hand with: {} -r {}",
        kimi::COMMAND,
        kimi::COMMAND,
        mv.session
    ));
    Err(Outcome::Exit(code))
}

/// End this process as `outcome` says: exit with the code, or die by the signal (with core
/// dumps off, so the harness never leaves a core of its own for the child's crash).
pub fn finish(outcome: Outcome) -> ! {
    match outcome {
        Outcome::Exit(code) => std::process::exit(code),
        Outcome::Signal(sig) => {
            let _ = rustix::process::setrlimit(
                rustix::process::Resource::Core,
                rustix::process::Rlimit {
                    current: Some(0),
                    maximum: rustix::process::getrlimit(rustix::process::Resource::Core).maximum,
                },
            );
            let _ = signal_hook::low_level::emulate_default_handler(sig);
            std::process::exit(sig.saturating_add(128))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_report_unchanged() {
        assert_eq!(Status::Code(7).outcome(), Outcome::Exit(7));
        assert_eq!(Status::Killed(11).outcome(), Outcome::Signal(11));
    }

    #[test]
    fn only_kimis_term_endings_continue_a_move() {
        assert!(Status::Code(143).after_our_term());
        assert!(Status::Killed(SIGTERM).after_our_term());
        assert!(!Status::Code(0).after_our_term());
        assert!(!Status::Code(129).after_our_term());
        assert!(!Status::Killed(SIGKILL).after_our_term());
    }

    #[test]
    fn refused_start_catches_non_signal_exits_and_faults() {
        assert!(Status::Code(1).refused_start());
        assert!(Status::Code(126).refused_start());
        assert!(Status::Code(127).refused_start());
        assert!(Status::Killed(SIGABRT).refused_start());
        assert!(Status::Killed(SIGSEGV).refused_start());
        assert!(Status::Killed(SIGKILL).refused_start());
        assert!(!Status::Code(0).refused_start());
        assert!(!Status::Code(129).refused_start());
        assert!(!Status::Code(130).refused_start());
        assert!(!Status::Code(131).refused_start());
        assert!(!Status::Code(143).refused_start());
        assert!(!Status::Killed(SIGTERM).refused_start());
        assert!(!Status::Killed(SIGHUP).refused_start());
    }

    #[test]
    fn knobs_read_from_the_env_and_clamp() {
        let env = Env::from_pairs([
            ("KIMI_ALAB_QUIET_MS", "300"),
            ("KIMI_ALAB_ATTEST_MS", "5000"),
            ("KIMI_ALAB_POLL_MS", "1"),
            ("KIMI_ALAB_TERM_GRACE_MS", "50"),
            ("KIMI_ALAB_RESTART_WAIT_MS", "1500"),
        ]);
        let s = Settings::from_env("/p".into(), env, false, 0);
        assert_eq!(s.knobs.quiet, Duration::from_millis(300));
        assert_eq!(s.knobs.attest_valid, Duration::from_secs(5));
        assert_eq!(s.poll, Duration::from_millis(20));
        assert_eq!(s.term_grace, Duration::from_millis(100));
        assert_eq!(s.restart_wait, Duration::from_millis(1500));
        let s = Settings::from_env(
            "/p".into(),
            Env::from_pairs([("KIMI_ALAB_RESTART_WAIT_MS", "18446744073709551615")]),
            false,
            0,
        );
        assert_eq!(s.restart_wait, Duration::from_secs(3600));
        let s = Settings::from_env("/p".into(), Env::default(), true, 3);
        assert_eq!(s.knobs, Knobs::defaults());
        assert_eq!(s.term_grace, Duration::from_secs(30));
        assert_eq!(s.restart_wait, AWAY_BUDGET);
        assert_eq!(AWAY_BUDGET, Duration::from_secs(45));
        assert!(s.no_update);
        assert_eq!(s.depth, 3);
    }

    #[test]
    fn a_harness_started_as_a_harnesss_vendor_refuses() {
        // Not nested at all, or nested under an unrelated process (a Kimi tool's shell).
        assert_eq!(nesting(&Env::default(), 77, 78), Ok(0));
        assert_eq!(
            nesting(
                &Env::from_pairs([(PID_ENV, "500"), (DEPTH_ENV, "1")]),
                77,
                78
            ),
            Ok(1)
        );
        // Its parent is the harness that started it as the vendor, directly or via `exec`.
        let err = nesting(
            &Env::from_pairs([(PID_ENV, "77"), (DEPTH_ENV, "1")]),
            77,
            78,
        )
        .unwrap_err();
        assert!(err.contains("leads back"), "{err}");
        assert!(nesting(&Env::from_pairs([(PID_ENV, " 78 ")]), 77, 78).is_err());
    }

    #[test]
    fn nesting_stops_at_the_depth_limit_and_an_unreadable_depth_refuses() {
        let depth = |v: &str| nesting(&Env::from_pairs([(DEPTH_ENV, v)]), 77, 78);
        assert_eq!(depth("7"), Ok(MAX_DEPTH - 1));
        assert!(depth("8").is_err());
        assert!(depth("4294967295").is_err());
        for bad in ["", "x", "-1", "4294967296"] {
            assert!(depth(bad).is_err(), "{bad:?}");
        }
    }
}
