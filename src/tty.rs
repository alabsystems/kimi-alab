// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! When the terminal last carried input and output, read from the device node's own times.
//!
//! The harness never proxies the terminal: the child inherits it. What it can see is the tty
//! device's access time (moved when the child READS a keystroke) and modification time (moved
//! when the child WRITES to the screen). Measured on macOS 27 with a pty pair (2026-10-24): a
//! slave write moved only mtime, a master write moved nothing until the slave read it, and the
//! slave read moved atime, each to the sub-second; `tests/harness.rs` re-measures it. Linux keeps
//! both to 8-second buckets on purpose (`drivers/tty/tty_io.c` `tty_update_time`), so a reading
//! there is only good to [`GRANULARITY`].

use std::io::IsTerminal;
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::process::getpgrp;
use rustix::termios::{
    OptionalActions, QueueSelector, Termios, tcflush, tcgetattr, tcgetpgrp, tcsetattr,
};

/// The terminal's modes when the harness started, restored before a relaunch or after a
/// signal death while the harness's group owns the terminal.
pub struct Modes(Option<Termios>);

impl Modes {
    /// Save standard input's current terminal mode, if it is a terminal.
    pub fn save() -> Self {
        Self(tcgetattr(std::io::stdin()).ok())
    }

    /// Put the saved mode back when this process's group is the foreground group of standard input.
    pub fn restore(&self) {
        let Some(t) = &self.0 else {
            return;
        };
        if foreground() {
            let _ = tcsetattr(std::io::stdin(), OptionalActions::Now, t);
        }
    }
}

/// Whether this process's group is the foreground group of standard input's terminal.
fn foreground() -> bool {
    tcgetpgrp(std::io::stdin()).is_ok_and(|group| group == getpgrp())
}

/// The terminal's keyboard, mouse and paste reporting back to plain, on both screens, written
/// after a child died without undoing its own. Under the kitty keyboard protocol or
/// `modifyOtherKeys` a terminal encodes Ctrl-C as an escape sequence, never the byte the line
/// discipline turns into `SIGINT`, so restoring the line modes alone would leave a dead screen
/// that no key can end. A relaunched child pushes its own modes again.
const PLAIN_KEYBOARD: &[u8] = b"\x1b[=0u\x1b[<99u\x1b[?1049l\x1b[=0u\x1b[<99u\x1b[>4m\
\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1004l\x1b[?2004l\x1b[?25h";

/// Write [`PLAIN_KEYBOARD`] to the terminal the child drew on, best effort, and only while this
/// process's group owns it.
pub fn release_keyboard() {
    use std::io::Write;
    if !foreground() {
        return;
    }
    let wrote = if std::io::stdout().is_terminal() {
        let mut out = std::io::stdout();
        out.write_all(PLAIN_KEYBOARD).and_then(|()| out.flush())
    } else if std::io::stderr().is_terminal() {
        std::io::stderr().write_all(PLAIN_KEYBOARD)
    } else {
        Ok(())
    };
    let _ = wrote;
}

/// Drop what was typed on the terminal and not yet read, while this process's group owns it:
/// keys typed at a dead screen are meant for no shell and no relaunched child.
pub fn discard_input() {
    if foreground() {
        let _ = tcflush(std::io::stdin(), QueueSelector::IFlush);
    }
}

/// How far a reported tty time may lag the event it records.
pub const GRANULARITY: Duration = if cfg!(target_os = "macos") {
    Duration::ZERO
} else {
    Duration::from_secs(8)
};

/// The terminal's last input and output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TtyTimes {
    /// The device's access time: the last keystroke a reader consumed.
    pub input: SystemTime,
    /// The device's modification time: the last write to the screen.
    pub output: SystemTime,
}

/// The times of the first of standard input, output and error that is a terminal, or `None`
/// when none is (or its times cannot be read).
pub fn probe() -> Option<TtyTimes> {
    let fd = if std::io::stdin().is_terminal() {
        std::io::stdin().as_fd().try_clone_to_owned().ok()?
    } else if std::io::stdout().is_terminal() {
        std::io::stdout().as_fd().try_clone_to_owned().ok()?
    } else if std::io::stderr().is_terminal() {
        std::io::stderr().as_fd().try_clone_to_owned().ok()?
    } else {
        return None;
    };
    let meta = std::fs::File::from(fd).metadata().ok()?;
    Some(TtyTimes {
        input: at(meta.atime(), meta.atime_nsec())?,
        output: at(meta.mtime(), meta.mtime_nsec())?,
    })
}

fn at(secs: i64, nsec: i64) -> Option<SystemTime> {
    let secs = u64::try_from(secs).ok()?;
    let nsec = u64::try_from(nsec).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(secs) + Duration::from_nanos(nsec))
}
