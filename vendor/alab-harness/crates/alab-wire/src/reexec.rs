// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A harness's re-exec of itself: a newer build of the harness takes over the
//! running process, with the same pid and the same child, which never notices
//! (aterm's harness design §4.5; HX-08a). Built from muse-alab's re-exec (its
//! text carry over one inherited descriptor, the probe first, the caught signals
//! blocked across `execve`) and kimi-alab's (the child's kernel identity, its
//! schema 2), so each harness drops its own copy.
//!
//! **The steps, in the old image.**
//!
//! 1. **Probe first.** [`probe`] runs `<new exe> --alab-reexec-probe`
//!    ([`PROBE_ARG`]), which must exit 0 within the caller's bound having
//!    printed exactly its [`Protocol`] line and a newline: the new image starts,
//!    and reads the carry this one writes. Only that answer gives a [`Proved`],
//!    and only a [`Proved`] is exec'd into, so a build that was not probed, or
//!    answered anything else, is never exec'd and the old image runs on. The
//!    probe's file is stamped (device, inode, size, modification and change
//!    times) and must be unchanged when the exec comes, or the exec is held
//!    ([`Held::Changed`]).
//! 2. **The child.** [`Identity::of`], when the harness starts its child, reads
//!    the child's kernel identity: pid, parent (this process), group, session,
//!    birth (the kernel's start stamp, kept across `exec`) and the running
//!    image's file. [`Identity::current`], right before the re-exec, reads it
//!    again, requiring the same pid, parent and birth (a child may `exec` its
//!    own image, so the file is read anew).
//! 3. **Signals.** [`block`] blocks the caught signals on the thread that will
//!    exec, before the harness last reads its own wake source (a signal
//!    delivered before the block is there). A `SIGHUP` or `SIGTERM` posted
//!    after the block means the session is ending: [`exec`] holds on it
//!    ([`Held::Signals`]), and the mask comes back, so the harness's handlers
//!    take it. Any other caught signal pending at the `execve` crosses it
//!    pending, and the new image takes it with [`take_pending`] once its
//!    handlers are installed. Between the two, each caught signal is at its
//!    default action (`execve` resets a caught handler), and macOS discards
//!    one whose default is to ignore it as it is posted, blocked or not:
//!    `SIGCHLD`, `SIGURG`, `SIGWINCH`, `SIGIO` and `SIGINFO` ([`UNKEPT`];
//!    Linux keeps them pending). So a vendor that exits then leaves no
//!    `SIGCHLD`, and [`take_pending`] notes each caught one of [`UNKEPT`]
//!    whether or not it is pending.
//! 4. **The carry.** [`exec`] checks the child against the kernel once more,
//!    writes the [`Carry`] as text into a pipe whose read end is the one
//!    descriptor the new image inherits, closes the write end, and runs
//!    `execv(<exe>, [<exe>, --alab-reexec, <fd>])` ([`ARG`]) with this
//!    process's environment. Not `std`'s `Command::exec`, which empties the
//!    signal mask. It returns only when the exec did not happen ([`Held`]);
//!    nothing was handed over then, and the old image carries on.
//!
//! **In the new image.** Its `main` answers `--alab-reexec-probe` by printing
//! [`Protocol::line`] and exiting 0, touching nothing else. Given
//! `--alab-reexec <fd>`, it calls [`receive`], which reads the carry whole from
//! that pipe, closes it, and refuses a carry written by another process or
//! naming a child that is not this process's own (the kernel identity is
//! checked again); a child that ended in between is still its own and still
//! carried, unreaped, and [`receive`] says how the child stands ([`Alive`]).
//! Then it installs its handlers and calls [`take_pending`]; a child that has
//! ended, or a `SIGCHLD` taken, is reaped and its exit judged as the old image
//! would have. It claims again under the same pid: `Link::adopt_with(config,
//! carry.pid, carry.claim()?, now_ms, lookup)` when [`Carry::linked`], else
//! `Link::absent` ([`crate::Link::adopt_with`]). The old image's claim ended
//! with its connections at the `exec` (both are close-on-exec); aterm keeps a
//! claim through a re-exec grace keyed on the claimant's pid and birth, which an
//! `execve` keeps (the interface design, section 2.3).
//!
//! **The carry's text** ([`Carry::encode`]), one fact a line, each field
//! percent-encoded outside a plain set (`%` alone for an empty field):
//!
//! ```text
//! alab-reexec 1 <harness id> <harness's own number>
//! from <version>
//! pid <harness pid>
//! child <pid> <parent> <pgid> <session> <birth> <executable> <device> <inode>
//! vendor <word>                       (optional)
//! account <label> <home | ->          (optional)
//! ladder <streak> <since ms | ->
//! link claims | link absent
//! own <key> <field>…                  (the harness's own, any number, in order)
//! end
//! ```
//!
//! `<birth>` is `mac:<seconds>.<microseconds>` or `linux:<ticks>`. The first
//! line is the [`Protocol`]: this library's [`FORMAT`], the harness id and the
//! harness's own number for its `own` lines, which it raises when it changes
//! them, so a probe of a build that reads other lines fails.
//!
//! **Threads.** A signal mask is a thread's, and [`exec`] sees only what is
//! pending. A caught signal that another thread takes after the harness last
//! reads its wake source is delivered there, not pending, so the exec goes
//! ahead, and the wake source (a self-pipe, a kqueue, a flag) does not cross
//! `execve`: a `SIGTERM` or `SIGHUP` taken so is lost, and the new image runs
//! on. So every other thread of the harness blocks the caught signals for its
//! whole life (a thread starts with the mask of the thread that starts it, so
//! the harness blocks them before it starts any, and unblocks them on the one
//! thread that takes them), or the harness execs only while it has no other
//! thread. A signal every thread blocks stays pending for the process, where
//! [`exec`] sees it.

use std::ffi::{CString, OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd as _, BorrowedFd, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::claim::{self, Account, Child, ChildProcess};
use crate::config::Config;
use crate::relaunch::Ladder;

/// The argument the new image runs with: `<exe> --alab-reexec <fd>`.
pub const ARG: &str = "--alab-reexec";

/// The probe's argument: the image prints its [`Protocol::line`] and exits 0,
/// touching nothing else.
pub const PROBE_ARG: &str = "--alab-reexec-probe";

/// This library's carry format, the second word of every [`Protocol`] line.
pub const FORMAT: u32 = 1;

/// The most carry text crosses: what a fresh pipe takes on macOS and Linux
/// without a reader. A larger carry is a held re-exec, never a hang.
pub const MAX_BYTES: usize = 64 * 1024;

/// The pending signals that hold a re-exec: the session is ending.
pub const HOLDING: [libc::c_int; 2] = [libc::SIGHUP, libc::SIGTERM];

/// The signals whose default action is to ignore them, `SIGCONT` apart:
/// [`take_pending`] notes each caught one whether or not it is pending. From
/// `execve` until the new image installs its handlers, each caught signal is
/// at its default, and macOS discards one of these as it is posted, though
/// blocked; Linux keeps a blocked signal pending whatever its action, and it
/// is noted there too, so a harness acts the same on both. Measured on macOS
/// 2026-11-14: posted after an `execve` that kept them blocked, `SIGCHLD`
/// (a child killed), `SIGWINCH`, `SIGURG`, `SIGIO` and `SIGINFO` were not
/// pending and the killed child was unreaped, while `SIGCONT`, `SIGINT` and
/// `SIGUSR1` were pending; every one pending at the `execve` crossed it.
#[cfg(target_os = "macos")]
pub const UNKEPT: &[libc::c_int] = &[
    libc::SIGCHLD,
    libc::SIGURG,
    libc::SIGWINCH,
    libc::SIGIO,
    libc::SIGINFO,
];

/// The signals whose default action is to ignore them, `SIGCONT` apart:
/// [`take_pending`] notes each caught one whether or not it is pending. Linux
/// keeps a blocked signal pending whatever its action; they are noted here
/// too, because macOS discards one posted between `execve` and the new
/// image's handlers, so a harness acts the same on both.
#[cfg(not(target_os = "macos"))]
pub const UNKEPT: &[libc::c_int] = &[libc::SIGCHLD, libc::SIGURG, libc::SIGWINCH];

/// The most of a probe's output kept.
const MAX_SAID: u64 = 4096;

/// How often a probe is asked whether it exited.
const PROBE_POLL: Duration = Duration::from_millis(10);

/// The longest key of an `own` line.
const KEY_MAX: usize = 32;

/// What a carry's first line and the probe's whole answer say: the format, the
/// harness and the harness's own number for the lines it adds
/// (`alab-reexec <FORMAT> <id> <own>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Protocol {
    line: String,
}

impl Protocol {
    /// `config`'s harness carrying its own lines in the form it numbers `own`.
    /// A harness raises `own` whenever it changes what its `own` lines say.
    #[must_use]
    pub fn of(config: &Config, own: u32) -> Self {
        Self {
            line: format!("alab-reexec {FORMAT} {} {own}", config.id()),
        }
    }

    /// The line, without its newline: what the probe prints, and the carry's
    /// first line.
    #[must_use]
    pub fn line(&self) -> &str {
        &self.line
    }
}

/// A file as it was when probed: device, inode, size, and modification and
/// change times to the nanosecond.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    len: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Stamp {
    fn of(path: &Path) -> io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        Ok(Self {
            device: meta.dev(),
            inode: meta.ino(),
            len: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        })
    }
}

/// A build of the harness whose probe answered this protocol: the only thing
/// [`exec`] takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proved {
    exe: PathBuf,
    stamp: Stamp,
    protocol: Protocol,
}

impl Proved {
    /// The executable proved.
    #[must_use]
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// The protocol it answered.
    #[must_use]
    pub fn protocol(&self) -> &Protocol {
        &self.protocol
    }

    /// `exe` stamped as it is, with no probe: for this crate's tests, whose
    /// test binary cannot answer a probe (libtest reads its arguments).
    #[cfg(test)]
    pub(crate) fn unprobed(exe: &Path, protocol: &Protocol) -> io::Result<Self> {
        Ok(Self {
            exe: exe.to_path_buf(),
            stamp: Stamp::of(exe)?,
            protocol: protocol.clone(),
        })
    }
}

/// Why a probe proved nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unproved {
    /// It could not be started (or its file read), or its path is not
    /// absolute.
    NoExec(String),
    /// It exited otherwise than with 0 (the status).
    Exited(String),
    /// It exited 0 but did not print exactly the protocol line (what it said,
    /// shown escaped and cut).
    Said(String),
    /// It had not exited at the bound; its process group was killed.
    TimedOut(Duration),
    /// Its file changed while it was probed.
    Changed(String),
}

impl fmt::Display for Unproved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoExec(why) => write!(f, "probe-noexec {why}"),
            Self::Exited(status) => write!(f, "probe-exited {status}"),
            Self::Said(said) => write!(f, "probe-said {said}"),
            Self::TimedOut(within) => write!(f, "probe-timeout {}ms", within.as_millis()),
            Self::Changed(why) => write!(f, "probe-changed {why}"),
        }
    }
}

/// Probe the harness build `exe`: `<exe> --alab-reexec-probe`, with this
/// process's environment, no input, its own process group, its error output
/// discarded, and at most `within` to exit. It is proved when it exits 0
/// having printed exactly `protocol`'s line and a newline, and its file is the
/// same before and after. A probe still running at the bound is killed with
/// its group. `exe` must be absolute, as a store path is: a relative one would
/// be found on `PATH` by the probe but in the working directory by its stamp
/// and by [`exec`].
///
/// # Errors
/// Why it is not proved ([`Unproved`]).
pub fn probe(exe: &Path, protocol: &Protocol, within: Duration) -> Result<Proved, Unproved> {
    if !exe.is_absolute() {
        return Err(Unproved::NoExec(format!(
            "{} is not an absolute path",
            exe.display()
        )));
    }
    let noexec = |error: io::Error| Unproved::NoExec(format!("{}: {error}", exe.display()));
    let before = Stamp::of(exe).map_err(noexec)?;
    let mut child = Command::new(exe)
        .arg(PROBE_ARG)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(noexec)?;
    let deadline = Instant::now() + within;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(PROBE_POLL),
            Ok(None) => {
                end_group(&mut child);
                return Err(Unproved::TimedOut(within));
            }
            Err(error) => {
                end_group(&mut child);
                return Err(noexec(error));
            }
        }
    };
    let said = child.stdout.take().map(drain).unwrap_or_default();
    if !status.success() {
        return Err(Unproved::Exited(status.to_string()));
    }
    if said != format!("{}\n", protocol.line()).as_bytes() {
        return Err(Unproved::Said(shown(&said)));
    }
    match Stamp::of(exe) {
        Ok(after) if after == before => Ok(Proved {
            exe: exe.to_path_buf(),
            stamp: before,
            protocol: protocol.clone(),
        }),
        Ok(_) => Err(Unproved::Changed(format!(
            "{} changed while it was probed",
            exe.display()
        ))),
        Err(error) => Err(Unproved::Changed(format!("{}: {error}", exe.display()))),
    }
}

/// Kill a probe that outlived its bound, with its process group, and reap it.
fn end_group(child: &mut std::process::Child) {
    if let Ok(group) = libc::pid_t::try_from(child.id())
        && group > 1
    {
        // SAFETY: kill(2) on the probe's own process group (`process_group(0)`
        // made its pgid its pid), sent before the probe is reaped, so the id is
        // still its; never this process's group.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// What waits in an exited probe's output pipe, at most [`MAX_SAID`] bytes,
/// read without blocking: a process the probe left behind holding the pipe
/// cannot hold the harness.
fn drain(out: std::process::ChildStdout) -> Vec<u8> {
    let fd = out.as_raw_fd();
    // SAFETY: fcntl on the pipe this process owns (the child handle's stdout).
    let nonblocking = unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0
    };
    let mut said = Vec::new();
    if nonblocking {
        // A read that would block ends it: what the probe wrote before it
        // exited is there.
        let _ = out.take(MAX_SAID).read_to_end(&mut said);
    }
    said
}

/// `said` for a journal line: escaped, at most 120 characters.
fn shown(said: &[u8]) -> String {
    let text: String = String::from_utf8_lossy(said).escape_debug().collect();
    text.chars().take(120).collect()
}

/// A process's kernel start stamp, kept across `exec` and never shared by two
/// processes under one pid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Birth {
    /// macOS's `pbi_start_tvsec` and `pbi_start_tvusec` (`proc_pidinfo`,
    /// `PROC_PIDTBSDINFO`): exact.
    MacOs {
        /// Whole seconds since the Unix epoch.
        seconds: u64,
        /// Microseconds within that second, below 1 000 000.
        microseconds: u32,
    },
    /// Linux's `starttime`, field 22 of `/proc/<pid>/stat`: clock ticks after
    /// boot, exact (unlike the wall-clock start, which moves with `btime`).
    Linux {
        /// Clock ticks after boot.
        ticks: u64,
    },
}

/// The image file a process runs, as the kernel names it and holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    /// Its absolute path (macOS `proc_pidpath`, Linux `/proc/<pid>/exe`).
    pub path: PathBuf,
    /// The device of the file mapped as the process's image.
    pub device: u64,
    /// That file's inode: the running image's own, even after the path is
    /// replaced.
    pub inode: u64,
}

/// A child's kernel identity (kimi-alab's handoff schema 2): what proves, in
/// the new image, that the pid carried is still the process the old image
/// started and that it is this process's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Its pid, above 1.
    pub pid: u32,
    /// Its parent: the harness process.
    pub parent: u32,
    /// Its process group.
    pub pgid: u32,
    /// Its session.
    pub session: u32,
    /// Its kernel birth.
    pub birth: Birth,
    /// The image it runs.
    pub executable: Executable,
}

/// How a checked child stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alive {
    /// Running or sleeping.
    Running,
    /// Stopped by job control or a tracer.
    Stopped,
    /// Exited, or working its way through `exit`, and not yet reaped: still
    /// this process's to wait for, and never to run its own code again. A
    /// wait that does not hang finds it once it has finished exiting, which
    /// its `SIGCHLD` announces.
    Ended,
}

/// What the kernel says of a process now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Facts {
    parent: u32,
    pgid: Option<u32>,
    session: Option<u32>,
    birth: Birth,
    alive: Alive,
}

impl Facts {
    /// These facts are `identity`'s process: the same parent and birth, and,
    /// unless it ended (the kernel then keeps no session), the same group and
    /// session.
    fn are(&self, identity: &Identity) -> bool {
        self.parent == identity.parent
            && self.birth == identity.birth
            && (self.alive == Alive::Ended
                || (self.pgid == Some(identity.pgid) && self.session == Some(identity.session)))
    }
}

impl Identity {
    /// The kernel identity of this process's live child `pid`, read twice
    /// around its image file so nothing changed in between.
    ///
    /// # Errors
    /// The pid is not above 1, the process is not this process's child, has
    /// ended, or its facts or image cannot be read (on a platform other than
    /// macOS and Linux, never).
    pub fn of(pid: u32) -> Result<Self, String> {
        let this = std::process::id();
        let facts =
            facts(pid).ok_or_else(|| format!("process {pid}'s kernel facts are unreadable"))?;
        if facts.alive == Alive::Ended {
            return Err(format!("process {pid} has ended"));
        }
        if facts.parent != this {
            return Err(format!(
                "process {pid} is not this process's child (its parent is {})",
                facts.parent
            ));
        }
        let identity = Self {
            pid,
            parent: facts.parent,
            pgid: facts
                .pgid
                .ok_or_else(|| format!("process {pid} has no process group"))?,
            session: facts
                .session
                .ok_or_else(|| format!("process {pid} has no session"))?,
            birth: facts.birth,
            executable: executable(pid)
                .ok_or_else(|| format!("process {pid}'s executable is unreadable"))?,
        };
        match identity.check()? {
            Alive::Ended => Err(format!("process {pid} has ended")),
            Alive::Running | Alive::Stopped => Ok(identity),
        }
    }

    /// The same child as it is now, for the carry: [`Identity::of`] its pid,
    /// which must have this identity's parent and birth (the same process);
    /// its group, session and image as they are now.
    ///
    /// # Errors
    /// As [`Identity::of`], or the pid is another process now.
    pub fn current(&self) -> Result<Self, String> {
        let now = Self::of(self.pid)?;
        if now.parent != self.parent || now.birth != self.birth {
            return Err(format!(
                "process {} is not the child captured (another parent or birth)",
                self.pid
            ));
        }
        Ok(now)
    }

    /// Whether this is still this process's own child, as the kernel says now:
    /// its parent is this process, its birth (and while it lives, its group,
    /// session and image file) unchanged, and a wait for it that consumes
    /// nothing finds it. An ended child that is not yet reaped passes as
    /// [`Alive::Ended`], also when it ends while it is checked.
    ///
    /// # Errors
    /// What no longer matches.
    pub fn check(&self) -> Result<Alive, String> {
        let pid = self.pid;
        let this = std::process::id();
        if self.parent != this {
            return Err(format!(
                "child {pid} names parent {}, not this process ({this})",
                self.parent
            ));
        }
        let before = facts(pid)
            .or_else(|| self.ended())
            .ok_or_else(|| format!("child {pid}'s kernel facts are unreadable"))?;
        if !before.are(self) {
            return Err(format!(
                "child {pid}'s parent, birth, group or session is not the one carried"
            ));
        }
        if before.alive != Alive::Ended {
            let Some(now) = executable(pid) else {
                return self
                    .ended()
                    .map(|ended| ended.alive)
                    .ok_or_else(|| format!("child {pid}'s executable is unreadable"));
            };
            if now != self.executable {
                return Err(format!(
                    "child {pid} runs {} ({}:{}), not {} ({}:{})",
                    now.path.display(),
                    now.device,
                    now.inode,
                    self.executable.path.display(),
                    self.executable.device,
                    self.executable.inode
                ));
            }
        }
        if !owns(pid) {
            return Err(format!("child {pid} is not this process's to wait for"));
        }
        let after = facts(pid)
            .or_else(|| self.ended())
            .ok_or_else(|| format!("child {pid}'s kernel facts changed"))?;
        if !after.are(self) {
            return Err(format!("child {pid} changed while it was checked"));
        }
        Ok(after.alive)
    }

    /// After a read of the child failed: its facts read again when it has
    /// ended in the meantime and is still this process's own, unreaped (the
    /// same parent and birth, and a wait finds it). A process that exits
    /// between two of the kernel's answers fails the second: macOS's
    /// `getsid` and `proc_pidpath` answer nothing for an exiting or exited
    /// process, though `proc_pidinfo` does (measured 2026-11-14: without
    /// this read, a check of a child ending under it failed at once), and
    /// Linux's `/proc/<pid>/exe` is unreadable once the task has let go of
    /// its memory, though `/proc/<pid>/stat` is not.
    fn ended(&self) -> Option<Facts> {
        facts(self.pid)
            .filter(|facts| facts.alive == Alive::Ended && facts.are(self) && owns(self.pid))
    }

    /// The child as a contract-4 claim names it (`child=<pid>:<start>`): on
    /// macOS from the birth, exactly; on Linux read as aterm reads it
    /// ([`ChildProcess::of`]).
    ///
    /// # Errors
    /// The kernel start cannot be read (Linux), or the birth is out of range.
    pub fn process(&self) -> io::Result<ChildProcess> {
        match self.birth {
            Birth::MacOs {
                seconds,
                microseconds,
            } => seconds
                .checked_mul(1_000_000)
                .and_then(|us| us.checked_add(u64::from(microseconds)))
                .map(|start| ChildProcess {
                    pid: self.pid,
                    start,
                })
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "birth out of range")),
            Birth::Linux { .. } => ChildProcess::of(self.pid),
        }
    }
}

/// Whether this process may wait for `pid`: `waitid` with `WNOWAIT` and
/// `WNOHANG`, which consumes no event and does not block.
fn owns(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    let id = libc::id_t::from(pid);
    loop {
        let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `info` is writable `siginfo_t` storage for the call. WNOWAIT
        // leaves every pending event in place; WNOHANG returns at once for a
        // live child.
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                id,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if rc == 0 {
            return true;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return false;
        }
    }
}

/// `PROC_FLAG_INEXIT` (`<sys/proc_info.h>`), which `libc` does not declare:
/// the process is working its way through `exit`.
#[cfg(target_os = "macos")]
const PROC_FLAG_INEXIT: u32 = 4;

#[cfg(target_os = "macos")]
fn facts(pid: u32) -> Option<Facts> {
    let p = libc::pid_t::try_from(pid).ok().filter(|p| *p > 1)?;
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: `info` is a writable buffer of exactly `size` bytes, and
    // `proc_pidinfo` writes at most `size` bytes into it. A nonzero argument
    // answers for an exited, unreaped process too.
    let written = unsafe {
        libc::proc_pidinfo(
            p,
            libc::PROC_PIDTBSDINFO,
            1,
            info.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel filled all `size` bytes, and every bit pattern is a
    // valid `proc_bsdinfo` (integers and byte arrays only); it was zeroed
    // before besides.
    let info = unsafe { info.assume_init() };
    let alive = match info.pbi_status {
        libc::SZOMB => Alive::Ended,
        libc::SSTOP => Alive::Stopped,
        libc::SRUN | libc::SSLEEP => Alive::Running,
        _ => return None,
    };
    let alive = if info.pbi_flags & PROC_FLAG_INEXIT == 0 {
        alive
    } else {
        Alive::Ended
    };
    if info.pbi_pid != pid || info.pbi_ppid == 0 || info.pbi_start_tvsec == 0 {
        return None;
    }
    let microseconds = u32::try_from(info.pbi_start_tvusec)
        .ok()
        .filter(|us| *us < 1_000_000)?;
    let session = if alive == Alive::Ended {
        None
    } else {
        // SAFETY: getsid(2) reads one process's session and touches no memory
        // of ours.
        let sid = unsafe { libc::getsid(p) };
        Some(u32::try_from(sid).ok().filter(|sid| *sid > 0)?)
    };
    Some(Facts {
        parent: info.pbi_ppid,
        pgid: Some(info.pbi_pgid).filter(|pgid| *pgid > 0),
        session,
        birth: Birth::MacOs {
            seconds: info.pbi_start_tvsec,
            microseconds,
        },
        alive,
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn facts(pid: u32) -> Option<Facts> {
    if pid <= 1 {
        return None;
    }
    linux_facts(
        &std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?,
        pid,
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
fn facts(_pid: u32) -> Option<Facts> {
    None
}

/// The kernel's `PF_EXITING` (`<linux/sched.h>`): the task is working its way
/// through `exit`.
#[cfg(any(test, target_os = "linux", target_os = "android"))]
const PF_EXITING: u32 = 4;

/// `/proc/<pid>/stat` read: the state (field 3), parent (4), group (5),
/// session (6), flags (9) and `starttime` (22). The command name (field 2) is
/// in parentheses and may hold spaces and `)`, so the fields are counted after
/// the last `)`.
#[cfg(any(test, target_os = "linux", target_os = "android"))]
fn linux_facts(stat: &str, pid: u32) -> Option<Facts> {
    let (named, rest) = stat.split_once(" (")?;
    if claim::canonical_u64(named)? != u64::from(pid) || pid <= 1 {
        return None;
    }
    let (_, fields) = rest.rsplit_once(") ")?;
    let mut fields = fields.split_ascii_whitespace();
    let state = fields.next()?;
    let positive = |word: &str| word.parse::<u32>().ok().filter(|n| *n > 0);
    let parent = positive(fields.next()?)?;
    let pgid = positive(fields.next()?)?;
    let session = positive(fields.next()?)?;
    let flags = fields.nth(9 - 7)?.parse::<u32>().ok()?;
    let ticks = fields
        .nth(22 - 10)?
        .parse::<u64>()
        .ok()
        .filter(|ticks| *ticks > 0)?;
    let alive = match state {
        "Z" => Alive::Ended,
        "T" | "t" => Alive::Stopped,
        "R" | "S" | "D" | "I" | "P" | "W" | "K" => Alive::Running,
        _ => return None,
    };
    let alive = if flags & PF_EXITING == 0 {
        alive
    } else {
        Alive::Ended
    };
    Some(Facts {
        parent,
        pgid: Some(pgid),
        session: Some(session),
        birth: Birth::Linux { ticks },
        alive,
    })
}

#[cfg(target_os = "macos")]
fn executable(pid: u32) -> Option<Executable> {
    let p = libc::pid_t::try_from(pid).ok().filter(|p| *p > 1)?;
    let mut path = [0u8; 4096];
    let room = u32::try_from(path.len()).ok()?;
    // SAFETY: the kernel is given the whole writable buffer and its size.
    let n = unsafe { libc::proc_pidpath(p, path.as_mut_ptr().cast::<libc::c_void>(), room) };
    let n = usize::try_from(n)
        .ok()
        .filter(|n| *n > 0 && *n < path.len())?;
    let path = path.get(..n)?;
    if path.contains(&0) {
        return None;
    }
    let path = PathBuf::from(OsString::from_vec(path.to_vec()));
    if !path.is_absolute() {
        return None;
    }
    let (device, inode) = image_file(p)?;
    Some(Executable {
        path,
        device,
        inode,
    })
}

/// `struct proc_regioninfo` (`<sys/proc_info.h>`): the fields read here, the
/// others kept as their layout.
#[cfg(target_os = "macos")]
#[repr(C)]
struct RegionInfo {
    _protections: [u32; 4],
    _offset: u64,
    _counts: [u32; 14],
    address: u64,
    size: u64,
}

/// `struct proc_regionwithpathinfo`, which `libc` does not declare.
#[cfg(target_os = "macos")]
#[repr(C)]
struct RegionWithPath {
    info: RegionInfo,
    path: libc::vnode_info_path,
}

/// `PROC_PIDREGIONPATHINFO`, which `libc` does not declare.
#[cfg(target_os = "macos")]
const PROC_PIDREGIONPATHINFO: libc::c_int = 8;

#[cfg(target_os = "macos")]
const _: () = assert!(size_of::<RegionWithPath>() == 1272);

/// The device and inode of the first file-backed region of `pid`'s address
/// space: its image's `__TEXT`, mapped first.
#[cfg(target_os = "macos")]
fn image_file(pid: libc::pid_t) -> Option<(u64, u64)> {
    let size = libc::c_int::try_from(size_of::<RegionWithPath>()).ok()?;
    let mut address = 0u64;
    for _ in 0..64 {
        let mut region = MaybeUninit::<RegionWithPath>::zeroed();
        // SAFETY: `region` is a writable buffer of exactly `size` bytes, the
        // size `PROC_PIDREGIONPATHINFO` writes.
        let n = unsafe {
            libc::proc_pidinfo(
                pid,
                PROC_PIDREGIONPATHINFO,
                address,
                region.as_mut_ptr().cast::<libc::c_void>(),
                size,
            )
        };
        if n != size {
            return None;
        }
        // SAFETY: the kernel filled all `size` bytes, and every bit pattern is
        // valid for these integers and arrays; it was zeroed before besides.
        let region = unsafe { region.assume_init() };
        let stat = &region.path.vip_vi.vi_stat;
        if stat.vst_ino != 0 {
            return Some((u64::from(stat.vst_dev), stat.vst_ino));
        }
        let next = region.info.address.checked_add(region.info.size)?;
        if next <= address {
            return None;
        }
        address = next;
    }
    None
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn executable(pid: u32) -> Option<Executable> {
    let link = PathBuf::from(format!("/proc/{pid}/exe"));
    let path = std::fs::read_link(&link).ok()?;
    // The link's own metadata is the mapped file's, even once its path is
    // replaced.
    let meta = std::fs::metadata(&link).ok()?;
    (path.is_absolute() && meta.ino() != 0).then(|| Executable {
        path,
        device: meta.dev(),
        inode: meta.ino(),
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
fn executable(_pid: u32) -> Option<Executable> {
    None
}

/// One line the harness carries of its own: a key and its fields, as bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Own {
    /// `[a-z][a-z0-9-]*`, at most 32 bytes.
    pub key: String,
    /// Any bytes each, any number.
    pub fields: Vec<Vec<u8>>,
}

/// What crosses the re-exec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carry {
    /// The version of the harness that handed over (a claim's `version=`).
    pub from: String,
    /// The harness process: the old image's pid, which the new image is.
    pub pid: u32,
    /// The child, whose parent is [`Carry::pid`].
    pub child: Identity,
    /// The vendor build the child runs (the claim's `vendor=`), when known.
    pub vendor: Option<String>,
    /// The account the child runs on (the claim's `login=` and its home), for a
    /// harness that manages accounts.
    pub account: Option<Account>,
    /// The crash ladder: relaunches not yet forgiven, and how long before the
    /// exec the last was made ([`Ladder::streak`], [`Ladder::since`]).
    pub ladder: (u32, Option<Duration>),
    /// The old image had a link to aterm (a claim held or being made again):
    /// the new image claims again with [`crate::Link::adopt_with`]. Without
    /// one, it makes none.
    pub linked: bool,
    /// The harness's own lines, in order.
    pub own: Vec<Own>,
}

impl Carry {
    /// The claim the new image makes again for this child: its `vendor=`,
    /// `login=` and `child=` ([`Identity::process`]).
    ///
    /// # Errors
    /// As [`Identity::process`].
    pub fn claim(&self) -> io::Result<Child<'_>> {
        Ok(Child {
            vendor: self.vendor.as_deref(),
            login: self.account.as_ref().map(|account| account.label.as_str()),
            process: Some(self.child.process()?),
        })
    }

    /// The crash ladder rebuilt at `now_ms` on the new image's clock
    /// ([`Ladder::carried`]).
    #[must_use]
    pub fn ladder_at(&self, now_ms: u64) -> Ladder {
        Ladder::carried(self.ladder.0, self.ladder.1, now_ms)
    }

    /// Why this carry cannot be written or was not read whole, if it cannot.
    fn refusal(&self) -> Option<String> {
        let child = &self.child;
        let birth_ok = match child.birth {
            Birth::MacOs {
                seconds,
                microseconds,
            } => seconds > 0 && microseconds < 1_000_000,
            Birth::Linux { ticks } => ticks > 0,
        };
        if !claim::claim_version(&self.from) {
            Some(format!("from {:?} is not a harness version", self.from))
        } else if self.pid <= 1 {
            Some(format!("pid {} is no harness", self.pid))
        } else if child.pid <= 1 || child.pgid == 0 || child.session == 0 || !birth_ok {
            Some(format!("child {} is not a whole identity", child.pid))
        } else if child.parent != self.pid {
            Some(format!(
                "child {} has parent {}, not the harness {}",
                child.pid, child.parent, self.pid
            ))
        } else if !child.executable.path.is_absolute() || child.executable.inode == 0 {
            Some(format!("child {} has no image file", child.pid))
        } else if self
            .vendor
            .as_deref()
            .is_some_and(|vendor| claim::vendor_word(Some(vendor)).is_none())
        {
            Some("vendor is not a claim's vendor= word".to_string())
        } else if self.account.as_ref().is_some_and(|account| {
            claim::login_word(Some(&account.label)).is_none()
                || account
                    .home
                    .as_ref()
                    .is_some_and(|home| !home.is_absolute())
        }) {
            Some("account is not an account label with an absolute home".to_string())
        } else {
            self.own
                .iter()
                .find(|own| !key_ok(&own.key))
                .map(|own| format!("own key {:?} is not a word", own.key))
        }
    }

    /// The carry as text: `protocol`'s line, one line per fact, then `end`
    /// (module documentation).
    ///
    /// # Errors
    /// A field outside its grammar, or text over [`MAX_BYTES`].
    pub fn encode(&self, protocol: &Protocol) -> Result<String, String> {
        if let Some(why) = self.refusal() {
            return Err(why);
        }
        let child = &self.child;
        let mut out = format!("{}\n", protocol.line());
        out.push_str(&format!("from {}\n", self.from));
        out.push_str(&format!("pid {}\n", self.pid));
        out.push_str(&format!(
            "child {} {} {} {} {} {} {} {}\n",
            child.pid,
            child.parent,
            child.pgid,
            child.session,
            birth_word(child.birth),
            pct(child.executable.path.as_os_str().as_bytes()),
            child.executable.device,
            child.executable.inode
        ));
        if let Some(vendor) = &self.vendor {
            out.push_str(&format!("vendor {vendor}\n"));
        }
        if let Some(account) = &self.account {
            out.push_str(&format!(
                "account {} {}\n",
                account.label,
                account
                    .home
                    .as_ref()
                    .map_or_else(|| "-".to_string(), |home| pct(home.as_os_str().as_bytes()))
            ));
        }
        out.push_str(&format!(
            "ladder {} {}\n",
            self.ladder.0,
            self.ladder.1.map_or_else(
                || "-".to_string(),
                |since| u64::try_from(since.as_millis())
                    .unwrap_or(u64::MAX)
                    .to_string()
            )
        ));
        out.push_str(if self.linked {
            "link claims\n"
        } else {
            "link absent\n"
        });
        for own in &self.own {
            out.push_str("own ");
            out.push_str(&own.key);
            for field in &own.fields {
                out.push(' ');
                out.push_str(&pct(field));
            }
            out.push('\n');
        }
        out.push_str("end\n");
        if out.len() > MAX_BYTES {
            return Err(format!("it is {} bytes, over {MAX_BYTES}", out.len()));
        }
        Ok(out)
    }

    /// The carry [`Carry::encode`] wrote under `protocol`: that first line,
    /// every line known, every fact once, nothing after `end`.
    ///
    /// # Errors
    /// What is wrong with it.
    pub fn decode(text: &str, protocol: &Protocol) -> Result<Self, String> {
        if text.len() > MAX_BYTES {
            return Err(format!("it is over {MAX_BYTES} bytes"));
        }
        let mut lines = text.split('\n');
        if lines.next() != Some(protocol.line()) {
            return Err(format!("it does not start {:?}", protocol.line()));
        }
        let (mut from, mut pid, mut child, mut vendor, mut account) =
            (None, None, None, None, None);
        let (mut ladder, mut linked, mut own) = (None, None, Vec::new());
        let mut ended = false;
        for line in lines.by_ref() {
            if line == "end" {
                ended = true;
                break;
            }
            let mut words = line.split(' ');
            let key = words.next().unwrap_or_default();
            let f: Vec<&str> = words.collect();
            let once = |taken: bool| {
                if taken {
                    Err(format!("{key} twice"))
                } else {
                    Ok(())
                }
            };
            match (key, f.as_slice()) {
                ("from", [version]) => {
                    once(from.is_some())?;
                    from = Some((*version).to_string());
                }
                ("pid", [n]) => {
                    once(pid.is_some())?;
                    pid = Some(number(n)?);
                }
                ("child", [p, parent, pgid, session, birth, path, device, inode]) => {
                    once(child.is_some())?;
                    child = Some(Identity {
                        pid: number(p)?,
                        parent: number(parent)?,
                        pgid: number(pgid)?,
                        session: number(session)?,
                        birth: birth_of(birth)?,
                        executable: Executable {
                            path: PathBuf::from(OsString::from_vec(unpct(path)?)),
                            device: number(device)?,
                            inode: number(inode)?,
                        },
                    });
                }
                ("vendor", [word]) => {
                    once(vendor.is_some())?;
                    vendor = Some((*word).to_string());
                }
                ("account", [label, home]) => {
                    once(account.is_some())?;
                    account = Some(Account {
                        label: (*label).to_string(),
                        home: match *home {
                            "-" => None,
                            home => Some(PathBuf::from(OsString::from_vec(unpct(home)?))),
                        },
                    });
                }
                ("ladder", [streak, since]) => {
                    once(ladder.is_some())?;
                    let since = match *since {
                        "-" => None,
                        ms => Some(Duration::from_millis(number(ms)?)),
                    };
                    ladder = Some((number(streak)?, since));
                }
                ("link", [word]) => {
                    once(linked.is_some())?;
                    linked = Some(match *word {
                        "claims" => true,
                        "absent" => false,
                        other => return Err(format!("link {other:?}")),
                    });
                }
                ("own", [key, fields @ ..]) => own.push(Own {
                    key: (*key).to_string(),
                    fields: fields
                        .iter()
                        .map(|field| unpct(field))
                        .collect::<Result<_, _>>()?,
                }),
                _ => return Err(format!("an unknown line {line:?}")),
            }
        }
        if !ended || lines.any(|line| !line.is_empty()) || !text.ends_with("end\n") {
            return Err("it does not end with one `end`".to_string());
        }
        let need = |what: &str| format!("it names no {what}");
        let carry = Self {
            from: from.ok_or_else(|| need("harness version"))?,
            pid: pid.ok_or_else(|| need("harness pid"))?,
            child: child.ok_or_else(|| need("child"))?,
            vendor,
            account,
            ladder: ladder.ok_or_else(|| need("crash ladder"))?,
            linked: linked.ok_or_else(|| need("link"))?,
            own,
        };
        match carry.refusal() {
            Some(why) => Err(why),
            None => Ok(carry),
        }
    }
}

/// An `own` key: `[a-z][a-z0-9-]*`, at most [`KEY_MAX`] bytes.
fn key_ok(key: &str) -> bool {
    key.len() <= KEY_MAX
        && key
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn birth_word(birth: Birth) -> String {
    match birth {
        Birth::MacOs {
            seconds,
            microseconds,
        } => format!("mac:{seconds}.{microseconds:06}"),
        Birth::Linux { ticks } => format!("linux:{ticks}"),
    }
}

fn birth_of(word: &str) -> Result<Birth, String> {
    let unread = || format!("birth {word:?}");
    if let Some(stamp) = word.strip_prefix("mac:") {
        let (seconds, micros) = stamp.split_once('.').ok_or_else(unread)?;
        if micros.len() != 6 || !micros.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(unread());
        }
        Ok(Birth::MacOs {
            seconds: claim::canonical_u64(seconds).ok_or_else(unread)?,
            microseconds: micros.parse().map_err(|_| unread())?,
        })
    } else if let Some(ticks) = word.strip_prefix("linux:") {
        Ok(Birth::Linux {
            ticks: claim::canonical_u64(ticks).ok_or_else(unread)?,
        })
    } else {
        Err(unread())
    }
}

/// A canonical decimal field.
fn number<T: TryFrom<u64>>(field: &str) -> Result<T, String> {
    claim::canonical_u64(field)
        .and_then(|value| T::try_from(value).ok())
        .ok_or_else(|| format!("{field:?} is not a number in range"))
}

/// One field: every byte outside a plain set as `%XX`, so a field holds no
/// space, no newline and no `%` of its own; an empty field is a lone `%`.
fn pct(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || b"/._-+:@,".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    if out.is_empty() {
        out.push('%');
    }
    out
}

/// A field [`pct`] wrote, and only one it could have written.
fn unpct(field: &str) -> Result<Vec<u8>, String> {
    if field == "%" {
        return Ok(Vec::new());
    }
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let hex = field
                .get(i + 1..i + 3)
                .filter(|hex| {
                    hex.bytes()
                        .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
                })
                .ok_or_else(|| format!("a bad escape in {field:?}"))?;
            let value =
                u8::from_str_radix(hex, 16).map_err(|_| format!("a bad escape in {field:?}"))?;
            if value.is_ascii_alphanumeric() || b"/._-+:@,".contains(&value) {
                return Err(format!("a needless escape in {field:?}"));
            }
            out.push(value);
            i += 3;
        } else if byte.is_ascii_alphanumeric() || b"/._-+:@,".contains(&byte) {
            out.push(byte);
            i += 1;
        } else {
            return Err(format!("an unescaped byte in {field:?}"));
        }
    }
    Ok(out)
}

/// The caught signals blocked on this thread for a re-exec, from [`block`].
/// Dropped (the exec did not happen), the thread's earlier mask comes back and
/// whatever arrived meanwhile reaches the harness's handlers. Not `Send`: a
/// mask is a thread's.
pub struct Blocked {
    previous: libc::sigset_t,
    caught: Vec<libc::c_int>,
    _thread: PhantomData<*const ()>,
}

impl fmt::Debug for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Blocked")
            .field("caught", &self.caught)
            .finish_non_exhaustive()
    }
}

/// Block `caught` (the signals the harness has handlers for) on this thread,
/// the one that will exec. Every other thread must block them already (the
/// module's **Threads**), or a holding signal one of them takes is lost.
///
/// # Errors
/// A number that is no signal, or `pthread_sigmask` failed.
pub fn block(caught: &[libc::c_int]) -> io::Result<Blocked> {
    let set = sigset(caught)?;
    let mut previous = MaybeUninit::<libc::sigset_t>::zeroed();
    // SAFETY: `set` is an initialized set; `previous` is writable storage for
    // the old mask.
    let rc =
        unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &raw const set, previous.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc));
    }
    let mut caught = caught.to_vec();
    caught.sort_unstable();
    caught.dedup();
    Ok(Blocked {
        // SAFETY: pthread_sigmask succeeded and wrote the old mask; a zeroed
        // `sigset_t` is valid besides.
        previous: unsafe { previous.assume_init() },
        caught,
        _thread: PhantomData,
    })
}

impl Blocked {
    /// The caught signals posted since the block and waiting for it, in
    /// ascending order.
    #[must_use]
    pub fn pending(&self) -> Vec<libc::c_int> {
        pending_of(&self.caught)
    }
}

impl Drop for Blocked {
    fn drop(&mut self) {
        // SAFETY: `previous` is the mask pthread_sigmask returned on this
        // thread (the type is not `Send`).
        unsafe {
            libc::pthread_sigmask(
                libc::SIG_SETMASK,
                &raw const self.previous,
                std::ptr::null_mut(),
            );
        }
    }
}

/// In the new image, once its handlers are installed: the signals among
/// `caught` that crossed the exec pending (the old image held them blocked),
/// and every one of [`UNKEPT`] among `caught`, pending or not, in ascending
/// order; and `caught` unblocked on this thread, which delivers the pending
/// ones to those handlers. One of [`UNKEPT`] posted between the exec and the
/// handlers may be gone, so its note stands for it: a harness acts on it as
/// on a state to read again (a child's exit, the window's size), and a note
/// with nothing behind it, or a signal both noted and delivered, changes
/// nothing.
#[must_use]
pub fn take_pending(caught: &[libc::c_int]) -> Vec<libc::c_int> {
    let mut taken = pending_of(caught);
    taken.extend(caught.iter().copied().filter(|sig| UNKEPT.contains(sig)));
    taken.sort_unstable();
    taken.dedup();
    if let Ok(set) = sigset(caught) {
        // SAFETY: `set` is an initialized set; no old mask is asked for.
        unsafe {
            libc::pthread_sigmask(libc::SIG_UNBLOCK, &raw const set, std::ptr::null_mut());
        }
    }
    taken
}

fn sigset(signals: &[libc::c_int]) -> io::Result<libc::sigset_t> {
    let mut set = MaybeUninit::<libc::sigset_t>::zeroed();
    // SAFETY: sigemptyset initializes the storage it is given.
    if unsafe { libc::sigemptyset(set.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: sigemptyset initialized it (a zeroed set is valid besides).
    let mut set = unsafe { set.assume_init() };
    for &sig in signals {
        // SAFETY: `set` is an initialized set.
        if unsafe { libc::sigaddset(&raw mut set, sig) } != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{sig} is no signal"),
            ));
        }
    }
    Ok(set)
}

/// Which of `signals` are pending on this thread or the process, ascending.
fn pending_of(signals: &[libc::c_int]) -> Vec<libc::c_int> {
    let mut set = MaybeUninit::<libc::sigset_t>::zeroed();
    // SAFETY: `set` is writable storage sigpending fills.
    if unsafe { libc::sigpending(set.as_mut_ptr()) } != 0 {
        return Vec::new();
    }
    // SAFETY: sigpending filled it (a zeroed set is valid besides).
    let set = unsafe { set.assume_init() };
    let mut pending: Vec<libc::c_int> = signals
        .iter()
        .copied()
        // SAFETY: sigismember reads an initialized set.
        .filter(|&sig| unsafe { libc::sigismember(&raw const set, sig) } == 1)
        .collect();
    pending.sort_unstable();
    pending.dedup();
    pending
}

/// Why [`exec`] did not happen. The old image runs on, with its mask back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Held {
    /// A `SIGHUP` or `SIGTERM` was posted while the caught signals were
    /// blocked: the session is ending. They reached the harness's handlers
    /// when the mask came back; these are they, ascending.
    Signals(Vec<libc::c_int>),
    /// The build proved is not the file at its path now, or cannot be read:
    /// probe it again.
    Changed(String),
    /// The child is not the one the carry names, or has ended (the old image
    /// reaps it).
    Child(String),
    /// The carry names another process, cannot be written, or did not fit
    /// its pipe.
    Carry(String),
    /// `execve` failed.
    Exec(String),
}

impl fmt::Display for Held {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signals(signals) => write!(f, "held-signals {signals:?}"),
            Self::Changed(why) => write!(f, "held-changed {why}"),
            Self::Child(why) => write!(f, "held-child {why}"),
            Self::Carry(why) => write!(f, "held-carry {why}"),
            Self::Exec(why) => write!(f, "held-exec {why}"),
        }
    }
}

/// Hand this process over to the build `proved`, `carry` crossing on an
/// inherited pipe: `<exe> --alab-reexec <fd>`, with this process's
/// environment and the caught signals `blocked` still blocked. Returns only
/// when the exec did not happen, with why; nothing was handed over then,
/// `blocked` is dropped (the mask comes back) and this image carries on.
#[must_use]
pub fn exec(blocked: Blocked, proved: &Proved, carry: &Carry) -> Held {
    exec_with(blocked, proved, carry, &[])
}

/// [`exec`] with `lead` before [`ARG`]: this crate's tests exec their own
/// test binary, whose libtest must read its filter first.
fn exec_with(blocked: Blocked, proved: &Proved, carry: &Carry, lead: &[OsString]) -> Held {
    let this = std::process::id();
    if carry.pid != this {
        return Held::Carry(format!(
            "it names process {}, not this one ({this})",
            carry.pid
        ));
    }
    match Stamp::of(&proved.exe) {
        Ok(now) if now == proved.stamp => {}
        Ok(_) => {
            return Held::Changed(format!("{} changed since its probe", proved.exe.display()));
        }
        Err(error) => return Held::Changed(format!("{}: {error}", proved.exe.display())),
    }
    match carry.child.check() {
        Ok(Alive::Running | Alive::Stopped) => {}
        Ok(Alive::Ended) => return Held::Child(format!("child {} has ended", carry.child.pid)),
        Err(why) => return Held::Child(why),
    }
    let text = match carry.encode(&proved.protocol) {
        Ok(text) => text,
        Err(why) => return Held::Carry(why),
    };
    let read = match carried_on_pipe(text.as_bytes()) {
        Ok(read) => read,
        Err(why) => return Held::Carry(why),
    };
    let fd = read.as_raw_fd();
    let mut words = vec![proved.exe.as_os_str().to_os_string()];
    words.extend(lead.iter().cloned());
    words.push(OsString::from(ARG));
    words.push(OsString::from(fd.to_string()));
    let Ok(argv) = words
        .iter()
        .map(|word| CString::new(word.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
    else {
        return Held::Exec(format!(
            "{} is not a path execve takes",
            proved.exe.display()
        ));
    };
    let mut pointers: Vec<*const libc::c_char> = argv.iter().map(|word| word.as_ptr()).collect();
    pointers.push(std::ptr::null());
    // The read end crosses: its close-on-exec goes last of all, so no process
    // this one starts meanwhile inherits it.
    // SAFETY: fcntl on the descriptor this function owns.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } < 0 {
        return Held::Carry(format!("its pipe: {}", io::Error::last_os_error()));
    }
    let holding: Vec<libc::c_int> = HOLDING
        .into_iter()
        .filter(|sig| blocked.caught.contains(sig))
        .collect();
    let posted = pending_of(&holding);
    if !posted.is_empty() {
        drop(read);
        drop(blocked);
        return Held::Signals(posted);
    }
    // SAFETY: `argv[0]` and every pointer are NUL-terminated strings alive for
    // the call, and the list ends with a null pointer; execv(3) keeps this
    // process's environment and its signal mask.
    unsafe {
        libc::execv(argv[0].as_ptr(), pointers.as_ptr());
    }
    let error = io::Error::last_os_error();
    drop(read);
    drop(blocked);
    Held::Exec(format!("execve {}: {error}", proved.exe.display()))
}

/// A pipe holding `text`, its write end closed: the read end, above the
/// standard descriptors and close-on-exec until the exec clears it.
fn carried_on_pipe(text: &[u8]) -> Result<OwnedFd, String> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for the two descriptors pipe(2) writes.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(format!("no pipe: {}", io::Error::last_os_error()));
    }
    // SAFETY: pipe(2) just returned these two descriptors, owned by nothing
    // else.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    // pipe(2) takes the lowest free numbers, so with a standard descriptor
    // closed the read end is one, which `receive` refuses: it moves above 2.
    let read = if read.as_raw_fd() > 2 {
        read
    } else {
        // SAFETY: fcntl on a descriptor this function owns.
        let moved = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        let error = io::Error::last_os_error();
        drop(read);
        if moved < 0 {
            return Err(format!("its pipe: {error}"));
        }
        // SAFETY: fcntl just returned this new descriptor, owned by nothing
        // else.
        unsafe { OwnedFd::from_raw_fd(moved) }
    };
    // Both close-on-exec; the write end never blocks, so a carry larger than
    // the pipe holds is a re-exec that does not happen, never a hang.
    // SAFETY: fcntl on descriptors this function owns.
    let set = unsafe {
        let (r, w) = (read.as_raw_fd(), write.as_raw_fd());
        let flags = libc::fcntl(w, libc::F_GETFL);
        libc::fcntl(r, libc::F_SETFD, libc::FD_CLOEXEC) >= 0
            && libc::fcntl(w, libc::F_SETFD, libc::FD_CLOEXEC) >= 0
            && flags >= 0
            && libc::fcntl(w, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0
    };
    if !set {
        return Err(format!("its pipe: {}", io::Error::last_os_error()));
    }
    File::from(write)
        .write_all(text)
        .map_err(|error| format!("it ({} bytes) did not fit the pipe: {error}", text.len()))?;
    Ok(read)
}

/// In the new image: the carry the old one wrote into descriptor `fd_arg`
/// (`--alab-reexec <fd>`), read whole and the descriptor closed, and how its
/// child stands. Refused when the descriptor is not an inherited pipe, the
/// text is not a whole carry under `protocol`, it was written by another
/// process, or its child is not this process's own ([`Identity::check`]). An
/// ended, unreaped child is, and comes as [`Alive::Ended`]: it is the new
/// image's to reap, and its exit to judge.
///
/// # Errors
/// Why there is nothing to take over.
pub fn receive(fd_arg: &OsStr, protocol: &Protocol) -> Result<(Carry, Alive), String> {
    let fd: RawFd = fd_arg
        .to_str()
        .and_then(claim::canonical_u64)
        .and_then(|fd| RawFd::try_from(fd).ok())
        .filter(|fd| *fd > 2)
        .ok_or_else(|| format!("{fd_arg:?} names no inherited descriptor"))?;
    // SAFETY: F_GETFD only queries the descriptor number.
    if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
        return Err(format!("descriptor {fd} is not open"));
    }
    // SAFETY: the descriptor is open (just checked) and nothing closes it while
    // it is borrowed here: its type is read through a duplicate, which std
    // closes.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let is_pipe = borrowed
        .try_clone_to_owned()
        .ok()
        .map(File::from)
        .and_then(|file| file.metadata().ok())
        .is_some_and(|meta| meta.file_type().is_fifo());
    if !is_pipe {
        return Err(format!("descriptor {fd} is not a pipe"));
    }
    // SAFETY: the old image handed this descriptor over to this process and
    // nothing else here owns it; it is closed when the file drops.
    let file = unsafe { File::from_raw_fd(fd) };
    // The old image wrote all of it and closed its end before the exec, so all
    // of it is there: read without blocking, so a writer some other process
    // inherited by mistake cannot hold the new image.
    // SAFETY: fcntl on the descriptor `file` owns.
    let nonblocking = unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) >= 0
            && flags >= 0
            && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0
    };
    if !nonblocking {
        return Err(format!("descriptor {fd}: {}", io::Error::last_os_error()));
    }
    let mut bytes = Vec::new();
    let bound = u64::try_from(MAX_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    match file.take(bound).read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(error) => return Err(format!("the carry is unreadable: {error}")),
    }
    let text = String::from_utf8(bytes).map_err(|_| "the carry is not UTF-8".to_string())?;
    let carry = Carry::decode(&text, protocol)?;
    let this = std::process::id();
    if carry.pid != this {
        return Err(format!(
            "the carry was written by process {}, not this one ({this})",
            carry.pid
        ));
    }
    let alive = carry.child.check()?;
    Ok((carry, alive))
}

#[cfg(test)]
#[path = "reexec_tests.rs"]
mod tests;
