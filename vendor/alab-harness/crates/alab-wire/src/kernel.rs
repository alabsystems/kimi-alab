// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! A child's kernel start time, read as aterm reads it to check a contract-4
//! claim's `child=<pid>:<start>` (the interface design, section 2.2; HX-06).
//!
//! `<start>` is microseconds since the Unix epoch ([`ChildProcess`]). aterm
//! keeps a claim's child only when the start it reads for that pid matches:
//!
//! - **macOS**: `proc_pidinfo(pid, PROC_PIDTBSDINFO)`, `pbi_start_tvsec *
//!   1_000_000 + pbi_start_tvusec`. The kernel keeps the start to the
//!   microsecond, so this is exact and aterm compares it exactly.
//! - **Linux**: `btime * 1_000_000 + starttime * 1_000_000 / HZ`, with `btime`
//!   from `/proc/stat`, `starttime` field 22 of `/proc/<pid>/stat` (clock ticks
//!   since boot) and `HZ` from `sysconf(_SC_CLK_TCK)`. `btime` is whole
//!   seconds and `starttime` whole ticks, so two readers can differ; aterm
//!   matches it within one second plus a tick.
//!
//! This is the one process fact the library reads, and only when a harness
//! asks ([`start_us`], [`ChildProcess::of`]); it still forks, execs, signals
//! and reaps nothing.

use std::io;

use crate::claim::ChildProcess;

/// The kernel start time of process `pid`, in microseconds since the Unix
/// epoch, computed as aterm computes it for a `child=` check (module
/// documentation).
///
/// # Errors
/// The pid is 0 or out of range, the process does not exist (or is not
/// readable by this user), the kernel's answer cannot be read, or this
/// platform is neither macOS nor Linux ([`io::ErrorKind::Unsupported`]).
pub fn start_us(pid: u32) -> io::Result<u64> {
    let pid = i32::try_from(pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pid out of range"))?;
    platform_start_us(pid)
}

impl ChildProcess {
    /// The child `pid` as a contract-4 claim names it: its pid and its kernel
    /// start time ([`start_us`]). A harness calls it right after it starts
    /// the child, and passes the result to [`crate::Link::reannounce`].
    ///
    /// # Errors
    /// As [`start_us`].
    pub fn of(pid: u32) -> io::Result<Self> {
        Ok(Self {
            pid,
            start: start_us(pid)?,
        })
    }
}

#[cfg(target_os = "macos")]
fn platform_start_us(pid: i32) -> io::Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>())
        .map_err(|_| io::Error::other("proc_bsdinfo size"))?;
    // SAFETY: `info` is a writable buffer of exactly `size` bytes, and
    // `proc_pidinfo` writes at most `size` bytes into it.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    if written <= 0 {
        return Err(io::Error::last_os_error());
    }
    if written != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "proc_pidinfo answered a short PROC_PIDTBSDINFO",
        ));
    }
    // SAFETY: the kernel filled all `size` bytes, and every bit pattern is a
    // valid `proc_bsdinfo` (integers and byte arrays only); it was zeroed
    // before besides.
    let info = unsafe { info.assume_init() };
    info.pbi_start_tvsec
        .checked_mul(1_000_000)
        .and_then(|us| us.checked_add(info.pbi_start_tvusec))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "start out of range"))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn platform_start_us(pid: i32) -> io::Result<u64> {
    let unreadable = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
    let btime = boot_time(&std::fs::read_to_string("/proc/stat")?)
        .ok_or_else(|| unreadable("/proc/stat has no btime"))?;
    let ticks = start_ticks(&std::fs::read_to_string(format!("/proc/{pid}/stat"))?)
        .ok_or_else(|| unreadable("/proc/<pid>/stat has no starttime"))?;
    // SAFETY: `sysconf` reads a configuration value and touches no memory of
    // ours.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let hz = u64::try_from(hz)
        .ok()
        .filter(|hz| *hz > 0)
        .ok_or_else(|| unreadable("sysconf(_SC_CLK_TCK)"))?;
    linux_start_us(btime, ticks, hz).ok_or_else(|| unreadable("start out of range"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
fn platform_start_us(_pid: i32) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no kernel start reader on this platform",
    ))
}

/// `btime` (the boot time, whole seconds since the epoch) from `/proc/stat`.
#[cfg(any(test, target_os = "linux", target_os = "android"))]
fn boot_time(stat: &str) -> Option<u64> {
    stat.lines()
        .find_map(|line| line.strip_prefix("btime "))
        .and_then(|value| value.trim().parse().ok())
}

/// Field 22 of `/proc/<pid>/stat` (`starttime`, clock ticks after boot). The
/// command name (field 2) is in parentheses and may hold spaces and `)`, so
/// the fields are counted after the last `)`, which is field 3's start.
#[cfg(any(test, target_os = "linux", target_os = "android"))]
fn start_ticks(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_ascii_whitespace().nth(22 - 3)?.parse().ok()
}

/// `btime * 1_000_000 + ticks * 1_000_000 / hz`, aterm's Linux start.
#[cfg(any(test, target_os = "linux", target_os = "android"))]
fn linux_start_us(btime: u64, ticks: u64, hz: u64) -> Option<u64> {
    let after_boot = u128::from(ticks) * 1_000_000 / u128::from(hz);
    u64::try_from(u128::from(btime) * 1_000_000 + after_boot).ok()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;

    fn now_us() -> u64 {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after the epoch");
        u64::try_from(since.as_micros()).expect("in range")
    }

    /// aterm's Linux reader allows one second plus a tick; macOS is exact.
    const SLACK_US: u64 = 1_100_000;

    /// The test's own process started in the past, within the last day.
    #[test]
    fn this_process_started_in_the_past_within_a_day() {
        let start = start_us(std::process::id()).expect("our own start");
        let now = now_us();
        assert!(start > 0);
        assert!(start <= now + SLACK_US, "start {start} after now {now}");
        let day = u64::try_from(Duration::from_secs(86_400).as_micros()).expect("a day");
        assert!(
            start + day >= now,
            "start {start} more than a day before {now}"
        );
        assert_eq!(
            ChildProcess::of(std::process::id()).expect("our own child word"),
            ChildProcess {
                pid: std::process::id(),
                start,
            }
        );
    }

    /// A process started between two clock reads has its start between them,
    /// and keeps that start for as long as it lives.
    #[test]
    fn a_new_child_started_between_two_reads_of_the_clock() {
        let before = now_us();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("start sleep");
        let after = now_us();
        let start = start_us(child.id());
        let again = start_us(child.id());
        let _ = child.kill();
        let _ = child.wait();
        let start = start.expect("the child's start");
        assert!(
            start + SLACK_US >= before && start <= after + SLACK_US,
            "start {start} outside [{before}, {after}]"
        );
        assert_eq!(again.expect("read again"), start);
    }

    #[test]
    fn no_process_has_no_start() {
        assert_eq!(
            start_us(0).map_err(|error| error.kind()),
            Err(io::ErrorKind::InvalidInput)
        );
        assert_eq!(
            start_us(u32::MAX).map_err(|error| error.kind()),
            Err(io::ErrorKind::InvalidInput)
        );
        // Far above any pid a kernel hands out.
        assert!(start_us(0x7fff_fff0).is_err());
    }

    /// The Linux arithmetic and its two `/proc` readers, on recorded text.
    #[test]
    fn the_linux_start_reads_btime_and_field_22() {
        let stat = "cpu  1 2 3\nintr 9\nctxt 4\nbtime 1763000000\nprocesses 7\n";
        assert_eq!(boot_time(stat), Some(1_763_000_000));
        assert_eq!(boot_time("cpu 1\n"), None);
        // `comm` holds a space and a `)`; starttime is 12345 ticks.
        let pid_stat = "4242 (a) b) S 1 4242 4242 34816 4242 4194304 100 0 0 0 1 2 0 0 \
                        20 0 1 0 12345 1000000 200 18446744073709551615";
        assert_eq!(start_ticks(pid_stat), Some(12_345));
        assert_eq!(start_ticks("4242 (a) S 1"), None);
        assert_eq!(start_ticks("no parenthesis"), None);
        assert_eq!(
            linux_start_us(1_763_000_000, 12_345, 100),
            Some(1_763_000_123_450_000)
        );
        assert_eq!(linux_start_us(10, 1, 3), Some(10_333_333));
    }
}
