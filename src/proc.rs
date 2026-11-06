// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Process facts the safe-point backstops read: which processes descend from the child, and how
//! much CPU they have used. Read from `/bin/ps` on macOS and `/proc` on Linux — absolute paths,
//! never `PATH`, and only while an update is waiting for a safe point.

use std::collections::BTreeSet;
use std::time::Duration;

/// One row of the process table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its pid.
    pub pid: i32,
    /// Its parent's pid.
    pub ppid: i32,
    /// Its CPU time so far, user plus system.
    pub cpu: Duration,
}

/// A process and everything descending from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    /// The root and its descendants.
    pub pids: BTreeSet<i32>,
    /// Their CPU time so far, summed.
    pub cpu: Duration,
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

/// The tree rooted at `root` in `table`, or `None` when `root` is not in it.
pub fn tree_of(table: &[Entry], root: i32) -> Option<Tree> {
    let root_entry = table.iter().find(|e| e.pid == root)?;
    let mut pids = BTreeSet::from([root]);
    let mut cpu = root_entry.cpu;
    let mut grew = true;
    while grew {
        grew = false;
        for e in table {
            if e.pid != root && !pids.contains(&e.pid) && pids.contains(&e.ppid) {
                pids.insert(e.pid);
                cpu += e.cpu;
                grew = true;
            }
        }
    }
    Some(Tree { pids, cpu })
}

fn ps_table() -> std::io::Result<Vec<Entry>> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "pid=,ppid=,time="])
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

/// Rows of `ps -A -o pid=,ppid=,time=`; a row that does not parse is skipped.
pub fn parse_ps(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let pid = f.next()?.parse().ok()?;
            let parent = f.next()?.parse().ok()?;
            let cpu = parse_ps_time(f.next()?)?;
            Some(Entry {
                pid,
                ppid: parent,
                cpu,
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
    let (_, rest) = stat.rsplit_once(')')?;
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
            "  1 0 1:00.00\n 10 1 0:01.00\n 11 10 0:00.50\n 12 11 0:00.25\n 20 1 0:09.00\nbad row\n",
        );
        let tree = tree_of(&t, 10).unwrap();
        assert_eq!(tree.pids, BTreeSet::from([10, 11, 12]));
        assert_eq!(tree.cpu, Duration::from_millis(1750));
        assert_eq!(tree_of(&t, 99), None);
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
                cpu: Duration::from_secs(3)
            })
        );
    }

    #[test]
    fn the_live_table_holds_this_process() {
        let t = table().unwrap();
        let me = i32::try_from(std::process::id()).unwrap();
        assert!(
            t.iter().any(|e| e.pid == me),
            "the table does not list this test process"
        );
    }
}
