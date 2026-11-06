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
    SIGABRT, SIGBUS, SIGCHLD, SIGFPE, SIGHUP, SIGILL, SIGINT, SIGQUIT, SIGSEGV, SIGSYS, SIGTERM,
    SIGTRAP, SIGTSTP, SIGUSR1, SIGUSR2,
};

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
    /// Never move the child onto a new build.
    pub no_update: bool,
    /// This harness's nesting depth ([`nesting`]); its children get one more.
    pub depth: u32,
}

impl Settings {
    /// Settings from `env`: the defaults, with the test and tuning knobs
    /// `KIMI_ALAB_POLL_MS`, `KIMI_ALAB_QUIET_MS`, `KIMI_ALAB_ATTEST_MS` and
    /// `KIMI_ALAB_TERM_GRACE_MS` read when set.
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
        Self {
            prefix,
            env,
            knobs,
            poll,
            term_grace,
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

    /// Whether this is how Kimi ends after the harness's `SIGTERM`: its graceful stop's 143, or
    /// death by the signal itself.
    pub fn after_our_term(self) -> bool {
        matches!(self, Self::Code(kimi::TERM_EXIT) | Self::Killed(SIGTERM))
    }

    /// Whether this status, soon after a move started a new build, is that build refusing to
    /// start: any non-zero exit that is not a terminal/job signal's (129 HUP, 130 INT, 131 QUIT,
    /// 143 TERM), or a fault/abort signal.
    pub fn refused_start(self) -> bool {
        match self {
            Self::Code(c) => c != 0 && !matches!(c, 129 | 130 | 131 | kimi::TERM_EXIT),
            Self::Killed(s) => matches!(
                s,
                SIGABRT | SIGBUS | SIGFPE | SIGILL | SIGSEGV | SIGSYS | SIGTRAP
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
            Ok(Some((_, st))) if st.continued() => {}
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
    _child: Child,
    pid: Pid,
    build: Build,
    /// The launch's words as UTF-8, if they all are.
    words: Option<Vec<String>>,
}

impl Running {
    /// Start `build` with `args`, as the vendor of a harness at nesting depth `depth`. Refuses a
    /// build whose binary is this harness itself, on the first launch and on every relaunch.
    fn start(build: Build, args: &[OsString], depth: u32) -> std::io::Result<Self> {
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
        cmd.env(PID_ENV, std::process::id().to_string());
        cmd.env(DEPTH_ENV, depth.saturating_add(1).to_string());
        let child = cmd.spawn()?;
        let pid = Pid::from_child(&child);
        let words = args
            .iter()
            .map(|a| a.to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>();
        Ok(Self {
            _child: child,
            pid,
            build,
            words,
        })
    }

    /// The environment the child runs under.
    fn env(&self, base: &Env) -> Env {
        base.overlay(
            self.build
                .shim_env
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .chain(kimi::HARNESS_ENV.iter().copied()),
        )
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
}

/// How long after a move's relaunch an immediate non-zero exit or fault counts as the new build
/// refusing to start (falling back onto the previous build).
const STARTUP_GRACE: Duration = Duration::from_secs(15);

/// A move onto a new build, under way: `SIGTERM` was sent at a safe point.
struct Moving {
    argv: Vec<String>,
    session: String,
    from: String,
    target: Build,
    sent: Instant,
}

/// A build a move just started: if it refuses to start within [`STARTUP_GRACE`], the harness
/// marks it abandoned and falls back onto `prev`.
struct Moved {
    prev: Build,
    argv: Vec<String>,
    session: String,
    started: Instant,
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

/// Run `first` with the launch words `launch` until the session really ends.
#[allow(clippy::too_many_lines)]
pub fn run(
    settings: &Settings,
    first: Build,
    launch: &[OsString],
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
    let exe = first.exe.clone();
    let mut cur = match Running::start(first, launch, settings.depth) {
        Ok(r) => r,
        Err(e) => {
            say(&format!("cannot run {}: {e}", exe.display()));
            return Outcome::Exit(spawn_failure_code(&e));
        }
    };
    journal.note(
        "START",
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
    let mut moving: Option<Moving> = None;
    let mut moved: Option<Moved> = None;
    let mut abandoned = BTreeSet::new();
    let mut pending: Option<Build> = None;
    let mut noted_unready: Option<u64> = None;
    let mut noted_refusal = false;
    let mut attest: Option<Attestation> = None;
    let mut window: Option<Probe> = None;
    let mut ending: Option<i32> = None;

    loop {
        signals.wait(settings.poll);
        for sig in signals.take() {
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
                    window = None;
                    journal.note(
                        "ATTEST",
                        if pending.is_some() {
                            "idle attested; an update is waiting"
                        } else {
                            "idle attested; no update is waiting"
                        },
                    );
                }
                _ => {}
            }
        }

        match reap(cur.pid) {
            Reap::Running => {}
            Reap::Stopped => {
                journal.note("STOP", "the child stopped; the harness stops with it");
                let _ = signal_hook::low_level::raise(SIGTSTP);
                cur.signal(Signal::CONT);
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
                            sent: jm.started,
                        };
                        match relaunch(settings, &cur, &back, true, &mut abandoned, journal) {
                            Ok(next) => {
                                moved = (next.build.id != prev.id).then(|| Moved {
                                    prev,
                                    argv: back.argv,
                                    session: back.session,
                                    started: Instant::now(),
                                });
                                cur = next;
                                pending = None;
                                attest = None;
                                window = None;
                                noted_refusal = false;
                                continue;
                            }
                            Err(outcome) => return outcome,
                        }
                    }
                    return status.outcome();
                };
                if ending.is_some() || !status.after_our_term() {
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
                            started: Instant::now(),
                        });
                        cur = next;
                        pending = None;
                        attest = None;
                        window = None;
                        noted_refusal = false;
                        continue;
                    }
                    Err(outcome) => return outcome,
                }
            }
        }

        if let Some(mv) = &moving
            && mv.sent.elapsed() > settings.term_grace
        {
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
            _ => {
                if pending.take().is_some() {
                    journal.note("UPDATE-WITHDRAWN", "the running build is current again");
                    window = None;
                }
            }
        }
        let Some(target) = pending.as_ref() else {
            continue;
        };
        if let Some(why) = cur.launch_refusal() {
            if !noted_refusal {
                journal.note(
                    "UPDATE-WAITS",
                    &format!("{why}; the new build runs from the next launch"),
                );
                noted_refusal = true;
            }
            continue;
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
                window = None;
            }
            Verdict::Safe => {
                let child_env = cur.env(&settings.env);
                let words = cur.words.clone().unwrap_or_default();
                let plan =
                    session_of(&child_env, reading.tree.as_ref(), &host).and_then(|session| {
                        kimi::resume_argv(&words, &session)
                            .map(|argv| (session, argv))
                            .map_err(|e| e.to_string())
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
                            sent: Instant::now(),
                        });
                        cur.signal(Signal::TERM);
                    }
                    Err(why) => journal.note(
                        "REFUSED",
                        &format!("{why}; the update waits for a new attestation"),
                    ),
                }
                attest = None;
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
        match Running::start(build, &args, settings.depth) {
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
        assert!(!Status::Killed(SIGKILL_FOR_TEST).after_our_term());
    }

    #[test]
    fn refused_start_catches_non_signal_exits_and_faults() {
        assert!(Status::Code(1).refused_start());
        assert!(Status::Code(126).refused_start());
        assert!(Status::Code(127).refused_start());
        assert!(Status::Killed(SIGABRT).refused_start());
        assert!(Status::Killed(SIGSEGV).refused_start());
        assert!(!Status::Code(0).refused_start());
        assert!(!Status::Code(129).refused_start());
        assert!(!Status::Code(130).refused_start());
        assert!(!Status::Code(131).refused_start());
        assert!(!Status::Code(143).refused_start());
        assert!(!Status::Killed(SIGTERM).refused_start());
        assert!(!Status::Killed(SIGHUP).refused_start());
        assert!(!Status::Killed(SIGKILL_FOR_TEST).refused_start());
    }

    const SIGKILL_FOR_TEST: i32 = 9;

    #[test]
    fn knobs_read_from_the_env_and_clamp() {
        let env = Env::from_pairs([
            ("KIMI_ALAB_QUIET_MS", "300"),
            ("KIMI_ALAB_ATTEST_MS", "5000"),
            ("KIMI_ALAB_POLL_MS", "1"),
            ("KIMI_ALAB_TERM_GRACE_MS", "50"),
        ]);
        let s = Settings::from_env("/p".into(), env, false, 0);
        assert_eq!(s.knobs.quiet, Duration::from_millis(300));
        assert_eq!(s.knobs.attest_valid, Duration::from_secs(5));
        assert_eq!(s.poll, Duration::from_millis(20));
        assert_eq!(s.term_grace, Duration::from_millis(100));
        let s = Settings::from_env("/p".into(), Env::default(), true, 3);
        assert_eq!(s.knobs, Knobs::defaults());
        assert_eq!(s.term_grace, Duration::from_secs(30));
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
