// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Process facts the safe-point backstops and re-exec handoff read: descendant commands and CPU,
//! plus the owned child's kernel birth, live state and executable identity. Process tables come
//! from `/bin/ps` on macOS and `/proc` on Linux; identity uses bounded native process queries.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

/// Serialize process creation and native identity probes in tests, matching the harness's
/// single-threaded process model.
#[cfg(test)]
pub static PROCESS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One row of the process table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its pid.
    pub pid: i32,
    /// Its parent's pid.
    pub ppid: i32,
    /// Its CPU time so far, user plus system.
    pub cpu: Duration,
    /// Its executable name or path.
    pub command: String,
}

/// A process and everything descending from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    /// The root and its descendants.
    pub pids: BTreeSet<i32>,
    /// Their CPU time so far, summed.
    pub cpu: Duration,
    /// Descendant shells and keepalive processes whose work must not be interrupted.
    pub background: BTreeSet<String>,
}

/// The whole process table.
///
/// # Errors
/// The table could not be read.
pub fn table() -> std::io::Result<Vec<Entry>> {
    if cfg!(target_os = "linux") {
        linux_table()
    } else {
        ps_table()
    }
}

/// A kernel process-start stamp, stable across `exec` and never reused with the same live child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Birth {
    /// macOS's process start time in seconds and microseconds since the epoch.
    MacOs {
        /// Whole epoch seconds.
        seconds: u64,
        /// Microseconds within the start second.
        microseconds: u32,
    },
    /// Linux's start clock ticks since boot (`/proc/<pid>/stat`, field 22).
    Linux(u64),
}

/// The kernel-named executable and its file identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    /// The kernel's absolute image path, with Unix bytes preserved.
    pub path: PathBuf,
    /// The executable's device.
    pub device: u64,
    /// The executable's inode.
    pub inode: u64,
}

/// A complete identity captured from an owned, live child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildIdentity {
    /// The child's pid.
    pub pid: u32,
    /// The supervising parent's pid.
    pub parent: u32,
    /// The child's process group.
    pub pgid: u32,
    /// The child's terminal session.
    pub session: u32,
    /// The kernel birth stamp.
    pub birth: Birth,
    /// The executable observed while the child was alive.
    pub executable: Executable,
}

/// Live child states that may be adopted without consuming a wait event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildState {
    /// A running or sleeping child.
    Live,
    /// A job-control or tracing stop.
    Stopped,
    /// Exited but not reaped; never eligible for live adoption.
    Unreaped,
}

#[derive(Clone, Debug)]
struct KernelIdentity {
    pid: u32,
    parent: u32,
    pgid: Option<u32>,
    session: Option<u32>,
    birth: Birth,
    state: ChildState,
}

impl KernelIdentity {
    fn matches(&self, expected: &ChildIdentity) -> bool {
        self.pid == expected.pid
            && self.parent == expected.parent
            && self.birth == expected.birth
            && (self.state == ChildState::Unreaped
                || (self.pgid == Some(expected.pgid) && self.session == Some(expected.session)))
    }
}

/// Capture complete live process facts without consuming the child's wait event.
#[must_use]
pub fn capture_child(pid: u32) -> Option<ChildIdentity> {
    let before = kernel_identity(pid)?;
    if before.state == ChildState::Unreaped || before.parent != std::process::id() {
        return None;
    }
    let identity = ChildIdentity {
        pid,
        parent: before.parent,
        pgid: before.pgid?,
        session: before.session?,
        birth: before.birth,
        executable: executable(pid)?,
    };
    let after = kernel_identity(pid)?;
    (after.state != ChildState::Unreaped
        && after.matches(&identity)
        && owns_child(pid)
        && executable(pid)? == identity.executable)
        .then_some(identity)
}

/// Validate the same owned child without consuming its pending exit.
///
/// # Errors
/// Names the kernel witness that no longer matches the captured child.
pub fn validate_child(expected: &ChildIdentity) -> Result<ChildState, String> {
    if expected.parent != std::process::id() {
        return Err("the child has a different parent".to_string());
    }
    let before = kernel_identity(expected.pid).ok_or("the child's kernel facts are unreadable")?;
    if !before.matches(expected) {
        return Err("the child's pid, parent, birth, group or session changed".to_string());
    }
    if before.state != ChildState::Unreaped {
        let actual = executable(expected.pid).ok_or("the child's executable is unreadable")?;
        if actual != expected.executable {
            return Err(format!(
                "the child's executable changed: expected {:?}, found {:?}",
                expected.executable, actual
            ));
        }
    }
    let after = kernel_identity(expected.pid).ok_or("the child's kernel facts changed")?;
    if !after.matches(expected) {
        return Err("the child's identity changed during validation".to_string());
    }
    if !owns_child(expected.pid) {
        return Err("the child has no non-consuming wait proof".to_string());
    }
    if after.state != ChildState::Unreaped {
        let actual =
            executable(expected.pid).ok_or("the child's executable changed during validation")?;
        if actual != expected.executable {
            return Err(format!(
                "the child's executable changed during validation: expected {:?}, found {:?}",
                expected.executable, actual
            ));
        }
    }
    Ok(after.state)
}

/// Observe the same owned child without consuming its pending exit.
#[must_use]
pub fn observe_child(expected: &ChildIdentity) -> Option<ChildState> {
    validate_child(expected).ok()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(unsafe_code)]
fn owns_child(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 1 {
        return false;
    }
    loop {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `info` is writable siginfo_t storage. WNOWAIT preserves every pending child
        // event; WNOHANG prevents waiting for a live child to exit.
        let rc = unsafe {
            libc::waitid(
                libc::P_PID,
                pid.cast_unsigned(),
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if rc < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return rc == 0;
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn owns_child(_pid: u32) -> bool {
    false
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn kernel_identity(pid: u32) -> Option<KernelIdentity> {
    let p = libc::pid_t::try_from(pid).ok().filter(|p| *p > 1)?;
    let mut bytes = [0u8; std::mem::size_of::<libc::proc_bsdinfo>()];
    let size = libc::c_int::try_from(bytes.len()).ok()?;
    // SAFETY: `bytes` has exactly the capacity PROC_PIDTBSDINFO requires. A short result is
    // rejected before fields are decoded. Nonzero arg includes pending exits.
    let n = unsafe {
        libc::proc_pidinfo(
            p,
            libc::PROC_PIDTBSDINFO,
            1,
            bytes.as_mut_ptr().cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let u32_at = |offset: usize| {
        let end = offset.checked_add(4)?;
        Some(u32::from_ne_bytes(bytes.get(offset..end)?.try_into().ok()?))
    };
    let u64_at = |offset: usize| {
        let end = offset.checked_add(8)?;
        Some(u64::from_ne_bytes(bytes.get(offset..end)?.try_into().ok()?))
    };
    let state = match u32_at(std::mem::offset_of!(libc::proc_bsdinfo, pbi_status))? {
        libc::SZOMB => ChildState::Unreaped,
        libc::SSTOP => ChildState::Stopped,
        libc::SRUN | libc::SSLEEP => ChildState::Live,
        _ => return None,
    };
    let session = if state == ChildState::Unreaped {
        None
    } else {
        // SAFETY: getsid is a read-only query and reports errors as a negative pid.
        Some(
            u32::try_from(unsafe { libc::getsid(p) })
                .ok()
                .filter(|s| *s > 0)?,
        )
    };
    let actual_pid = u32_at(std::mem::offset_of!(libc::proc_bsdinfo, pbi_pid))?;
    let parent = u32_at(std::mem::offset_of!(libc::proc_bsdinfo, pbi_ppid))?;
    let group = u32_at(std::mem::offset_of!(libc::proc_bsdinfo, pbi_pgid))?;
    let seconds = u64_at(std::mem::offset_of!(libc::proc_bsdinfo, pbi_start_tvsec))?;
    if actual_pid != pid || parent == 0 || seconds == 0 {
        return None;
    }
    let microseconds = u64_at(std::mem::offset_of!(libc::proc_bsdinfo, pbi_start_tvusec))
        .and_then(|value| u32::try_from(value).ok())
        .filter(|us| *us < 1_000_000)?;
    Some(KernelIdentity {
        pid,
        parent,
        pgid: (group > 0).then_some(group),
        session,
        birth: Birth::MacOs {
            seconds,
            microseconds,
        },
        state,
    })
}

#[cfg(target_os = "linux")]
fn kernel_identity(pid: u32) -> Option<KernelIdentity> {
    if pid <= 1 {
        return None;
    }
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_linux_identity(&text, pid)
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_identity(text: &str, pid: u32) -> Option<KernelIdentity> {
    let (named, rest) = text.split_once(" (")?;
    if named.parse::<u32>().ok()? != pid || pid <= 1 {
        return None;
    }
    let (_, fields) = rest.rsplit_once(") ")?;
    let mut fields = fields.split_ascii_whitespace();
    let state = match fields.next()? {
        "Z" => ChildState::Unreaped,
        "T" | "t" => ChildState::Stopped,
        "R" | "S" | "D" | "I" | "P" => ChildState::Live,
        _ => return None,
    };
    let positive = |word: &str| word.parse::<u32>().ok().filter(|n| *n > 0);
    let parent = positive(fields.next()?)?;
    let group = positive(fields.next()?)?;
    let session = positive(fields.next()?)?;
    let start = fields
        .nth(15)?
        .parse::<u64>()
        .ok()
        .filter(|ticks| *ticks > 0)?;
    Some(KernelIdentity {
        pid,
        parent,
        pgid: Some(group),
        session: Some(session),
        birth: Birth::Linux(start),
        state,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn kernel_identity(_pid: u32) -> Option<KernelIdentity> {
    None
}

#[cfg(target_os = "linux")]
fn executable(pid: u32) -> Option<Executable> {
    use std::os::unix::fs::MetadataExt as _;
    let link = PathBuf::from(format!("/proc/{pid}/exe"));
    let path = std::fs::read_link(&link).ok()?;
    let meta = std::fs::metadata(link).ok()?;
    (path.is_absolute() && meta.is_file() && meta.ino() != 0).then_some(Executable {
        path,
        device: meta.dev(),
        inode: meta.ino(),
    })
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn executable(pid: u32) -> Option<Executable> {
    use std::os::unix::{ffi::OsStringExt as _, fs::MetadataExt as _};
    let pid = libc::pid_t::try_from(pid).ok().filter(|p| *p > 1)?;
    let mut path = [0u8; 4096];
    // SAFETY: the kernel receives the full writable path buffer and its capacity.
    let n = unsafe {
        libc::proc_pidpath(
            pid,
            path.as_mut_ptr().cast(),
            u32::try_from(path.len()).ok()?,
        )
    };
    if n <= 0 || usize::try_from(n).ok()? >= path.len() {
        return None;
    }
    let end = path.iter().position(|b| *b == 0)?;
    let path = PathBuf::from(std::ffi::OsString::from_vec(path[..end].to_vec()));
    let meta = std::fs::metadata(&path).ok()?;
    if !path.is_absolute() || !meta.is_file() || meta.ino() == 0 {
        return None;
    }
    let (device, inode) = macos_image_file(pid)?;
    (device == meta.dev() && inode == meta.ino()).then_some(Executable {
        path,
        device,
        inode,
    })
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn macos_image_file(pid: libc::pid_t) -> Option<(u64, u64)> {
    const REGION_SIZE: usize = 96 + std::mem::size_of::<libc::vnode_info_path>();
    const _: () = assert!(REGION_SIZE == 1272);
    let mut address = 0u64;
    for _ in 0..64 {
        let mut bytes = [0u8; REGION_SIZE];
        // SAFETY: bytes has exactly the size requested by PROC_PIDREGIONPATHINFO (flavor 8).
        let n = unsafe {
            libc::proc_pidinfo(
                pid,
                8,
                address,
                bytes.as_mut_ptr().cast(),
                i32::try_from(REGION_SIZE).ok()?,
            )
        };
        if usize::try_from(n).ok()? != REGION_SIZE {
            return None;
        }
        let u64_at = |at: usize| {
            let end = at.checked_add(8)?;
            Some(u64::from_ne_bytes(bytes.get(at..end)?.try_into().ok()?))
        };
        let inode = u64_at(104)?;
        if inode != 0 {
            let device = u32::from_ne_bytes(bytes.get(96..100)?.try_into().ok()?);
            return Some((u64::from(device), inode));
        }
        let next = u64_at(80)?.checked_add(u64_at(88)?)?;
        if next <= address {
            return None;
        }
        address = next;
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn executable(_pid: u32) -> Option<Executable> {
    None
}

fn command_name(command: &str) -> Option<&str> {
    std::path::Path::new(command)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
}

fn is_background(command: &str) -> bool {
    command_name(command).is_some_and(|name| {
        matches!(
            name,
            "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh" | "tcsh" | "caffeinate"
        )
    })
}

/// The tree rooted at `root` in `table`, or `None` when `root` is not in it.
pub fn tree_of(table: &[Entry], root: i32) -> Option<Tree> {
    let root_entry = table.iter().find(|e| e.pid == root)?;
    let mut pids = BTreeSet::from([root]);
    let mut cpu = root_entry.cpu;
    let mut background = BTreeSet::new();
    let mut grew = true;
    while grew {
        grew = false;
        for e in table {
            if e.pid != root && !pids.contains(&e.pid) && pids.contains(&e.ppid) {
                if is_background(&e.command)
                    && !(e.ppid == root && command_name(&e.command) == Some("caffeinate"))
                {
                    background.insert(command_name(&e.command)?.to_string());
                }
                pids.insert(e.pid);
                cpu += e.cpu;
                grew = true;
            }
        }
    }
    Some(Tree {
        pids,
        cpu,
        background,
    })
}

fn ps_table() -> std::io::Result<Vec<Entry>> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,time=,comm="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "/bin/ps exited {}",
            out.status
        )));
    }
    Ok(parse_ps(&String::from_utf8_lossy(&out.stdout)))
}

/// Rows of `ps -A -o pid=,ppid=,time=,comm=`; a row that does not parse is skipped.
pub fn parse_ps(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let pid = f.next()?.parse().ok()?;
            let parent = f.next()?.parse().ok()?;
            let cpu = parse_ps_time(f.next()?)?;
            let command = f.collect::<Vec<_>>().join(" ");
            Some(Entry {
                pid,
                ppid: parent,
                cpu,
                command,
            })
        })
        .collect()
}

/// A `ps` `time` value: `[D-][[H:]M:]S[.frac]` (macOS prints `MMM:SS.cc`).
pub fn parse_ps_time(s: &str) -> Option<Duration> {
    let (days, clock) = match s.split_once('-') {
        Some((d, rest)) => (d.parse::<u64>().ok()?, rest),
        None => (0, s),
    };
    let fields: Vec<&str> = clock.split(':').collect();
    if fields.is_empty() || fields.len() > 3 {
        return None;
    }
    let (secs_field, rest) = fields.split_last()?;
    let mut whole = days.checked_mul(86_400)?;
    // `rest` is `[M]` or `[H, M]`: walk it from the minutes up.
    for (unit, f) in [60u64, 3600].iter().zip(rest.iter().rev()) {
        let n: u64 = f.parse().ok()?;
        whole = whole.checked_add(n.checked_mul(*unit)?)?;
    }
    let (s_int, s_frac) = match secs_field.split_once('.') {
        Some((a, b)) => (a, b),
        None => (*secs_field, ""),
    };
    whole = whole.checked_add(s_int.parse::<u64>().ok()?)?;
    let mut nanos: u64 = 0;
    if !s_frac.is_empty() {
        if !s_frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let missing = 9u32.checked_sub(u32::try_from(s_frac.len()).ok()?)?;
        nanos = s_frac
            .parse::<u64>()
            .ok()?
            .checked_mul(10u64.checked_pow(missing)?)?;
    }
    Duration::from_secs(whole).checked_add(Duration::from_nanos(nanos))
}

fn linux_table() -> std::io::Result<Vec<Entry>> {
    let ticks = rustix::param::clock_ticks_per_second().max(1);
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let Ok(entry) = entry else { continue };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if let Some(e) = parse_proc_stat(pid, &stat, ticks) {
            out.push(e);
        }
    }
    Ok(out)
}

/// One `/proc/<pid>/stat` line: the fields after the last `)` are state, ppid, …, utime (14th
/// field overall) and stime (15th), in clock ticks.
pub fn parse_proc_stat(pid: i32, stat: &str, ticks_per_sec: u64) -> Option<Entry> {
    let (named, rest) = stat.rsplit_once(')')?;
    let command = named.split_once('(')?.1.to_string();
    let f: Vec<&str> = rest.split_whitespace().collect();
    let parent = f.get(1)?.parse().ok()?;
    let utime: u64 = f.get(11)?.parse().ok()?;
    let stime: u64 = f.get(12)?.parse().ok()?;
    let ticks = utime.checked_add(stime)?;
    let per_sec = std::num::NonZeroU64::new(ticks_per_sec)?;
    let nanos = (ticks % per_sec).checked_mul(1_000_000_000)? / per_sec;
    let cpu = Duration::from_secs(ticks / per_sec).checked_add(Duration::from_nanos(nanos))?;
    Some(Entry {
        pid,
        ppid: parent,
        cpu,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_times_parse_in_every_shape() {
        assert_eq!(parse_ps_time("0:00.46"), Some(Duration::from_millis(460)));
        assert_eq!(
            parse_ps_time("1396:18.81"),
            Some(Duration::from_millis(1396 * 60_000 + 18_810))
        );
        assert_eq!(parse_ps_time("01:02:03"), Some(Duration::from_secs(3723)));
        assert_eq!(
            parse_ps_time("2-01:00:00"),
            Some(Duration::from_secs(2 * 86_400 + 3600))
        );
        assert_eq!(parse_ps_time("7"), Some(Duration::from_secs(7)));
        for bad in ["", "x", "1:2:3:4", "0:00.4a", "0:00.1234567891"] {
            assert_eq!(parse_ps_time(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn out_of_range_readings_are_unreadable_not_a_panic() {
        for huge in [
            "18446744073709551615:00",
            "1:18446744073709551615:00",
            "213503982334602-00:00:00",
            "18446744073709551616",
        ] {
            assert_eq!(parse_ps_time(huge), None, "{huge:?}");
        }
        let line = "1 (k) S 0 1 1 0 -1 0 0 0 0 0 18446744073709551615 1 0 0 20 0 1 0 1 1 1";
        assert_eq!(parse_proc_stat(1, line, 100), None);
        let line = "1 (k) S 0 1 1 0 -1 0 0 0 0 0 250 50 0 0 20 0 1 0 1 1 1";
        assert_eq!(parse_proc_stat(1, line, 0), None);
    }

    #[test]
    fn the_tree_holds_every_descendant_and_their_cpu() {
        let t = parse_ps(
            "  1 0 1:00.00 /bin/kimi\n 10 1 0:01.00 /bin/kimi\n 11 10 0:00.50 /bin/sleep\n 12 11 0:00.25 /bin/sleep\n 20 1 0:09.00 /bin/other\nbad row\n",
        );
        let tree = tree_of(&t, 10).unwrap();
        assert_eq!(tree.pids, BTreeSet::from([10, 11, 12]));
        assert_eq!(tree.cpu, Duration::from_millis(1750));
        assert!(tree.background.is_empty());
        assert_eq!(tree_of(&t, 99), None);
    }

    #[test]
    fn descendant_shells_are_background_except_the_vendors_own_keepalive() {
        let t = parse_ps(
            " 10 1 0:01.00 /bin/kimi\n 11 10 0:00.00 /bin/sh\n 12 11 0:00.00 /usr/bin/caffeinate\n 13 10 0:00.00 /usr/bin/caffeinate\n",
        );
        let tree = tree_of(&t, 10).unwrap();
        assert_eq!(
            tree.background,
            BTreeSet::from(["caffeinate".to_string(), "sh".to_string()])
        );
    }

    #[test]
    fn proc_stat_reads_ppid_and_cpu_past_a_tricky_name() {
        let line =
            "4242 (kimi (x) y) S 4241 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 1 0 1 1 1";
        assert_eq!(
            parse_proc_stat(4242, line, 100),
            Some(Entry {
                pid: 4242,
                ppid: 4241,
                cpu: Duration::from_secs(3),
                command: "kimi (x) y".to_string(),
            })
        );
    }

    #[test]
    fn linux_identity_reads_birth_group_session_and_state() {
        let line = "4242 (kimi (x) y) S 4241 4243 4244 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 1 0 98765 1 1";
        let identity = parse_linux_identity(line, 4242).unwrap();
        assert_eq!(identity.parent, 4241);
        assert_eq!(identity.pgid, Some(4243));
        assert_eq!(identity.session, Some(4244));
        assert_eq!(identity.birth, Birth::Linux(98765));
        assert_eq!(identity.state, ChildState::Live);
    }

    #[test]
    fn a_live_childs_identity_is_observed_without_reaping_it() {
        let _process = PROCESS_TEST_LOCK.lock().unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let identity = capture_child(child.id()).unwrap();
        assert_eq!(identity.parent, std::process::id());
        assert_eq!(observe_child(&identity), Some(ChildState::Live));
        let mut wrong_parent = identity.clone();
        wrong_parent.parent = wrong_parent.parent.saturating_add(1);
        assert_eq!(observe_child(&wrong_parent), None);
        let mut wrong_birth = identity.clone();
        wrong_birth.birth = Birth::Linux(1);
        assert_eq!(observe_child(&wrong_birth), None);
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn the_live_table_holds_this_process() {
        let _process = PROCESS_TEST_LOCK.lock().unwrap();
        let t = table().unwrap();
        let me = i32::try_from(std::process::id()).unwrap();
        assert!(
            t.iter().any(|e| e.pid == me),
            "the table does not list this test process"
        );
    }
}
