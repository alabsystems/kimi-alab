// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The harness end to end, against a fake `kimi` laid in a scratch atpkg store: argument
//! passthrough, exit status, signal forwarding, stop following, the live move onto a new build
//! with the resume argv, every refusal to move a busy child, and the refusal to run with no
//! vendor installed. The tests that need a terminal run the harness on a real pty.

#![cfg(unix)]

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use alab_wire::Scope;
use alab_wire::fake::{FakeAterm, POLICY_ON};
use rustix::process::{Pid, Signal, kill_process, kill_process_group};

const HARNESS: &str = env!("CARGO_BIN_EXE_kimi-alab");
const BUILD_A: u64 = 1_000_002_000_003_000_001; // 2.3.1
const BUILD_B: u64 = 1_000_002_000_003_000_002; // 2.3.2
const BUILD_C: u64 = 1_000_002_000_003_000_003; // 2.3.3

/// The harness's poll period in these tests (`KIMI_ALAB_POLL_MS`).
const POLL_MS: u64 = 50;
/// The safe point's quiet window in these tests (`KIMI_ALAB_QUIET_MS`).
const QUIET_MS: u64 = 400;
/// The SIGTERM grace a test that abandons a move gives a deaf child (`KIMI_ALAB_TERM_GRACE_MS`).
const TERM_GRACE_MS: u64 = 300;
/// The ceiling for anything the harness does as soon as it can. A test that passes returns the
/// moment its condition holds, so this costs nothing there; it is generous because a loaded
/// machine can leave the harness unscheduled for seconds.
const DEADLINE: Duration = Duration::from_secs(30);

/// A store `kimi` that leads back to the harness: it records its depth, then runs `$HARNESS_BIN`
/// (`exec`ing it, or as a child, per `@RUN@`). It stops by itself at `$CHAIN_CAP` with exit 42,
/// so a harness that fails to refuse can never run away in a test.
const CHAIN: &str = r#"#!/bin/sh
n=${CHAIN_DEPTH:-0}
echo "$n" >> "$FAKE_REC/chain"
if [ "$n" -ge "$CHAIN_CAP" ]; then exit 42; fi
CHAIN_DEPTH=$((n + 1)); export CHAIN_DEPTH
@RUN@ "$HARNESS_BIN" "$@"
"#;

/// The fake vendor CLI: records how it was run, then behaves as `FAKE_MODE` says. In the
/// default `session` mode it holds a Kimi session lock (as kimi 2.3.1's `SessionLockService`
/// writes one) and idles until a signal, ending 143 on SIGTERM and 129 on SIGHUP as Kimi does.
/// In `crash` mode its first run, and in `crashloop` mode every run, crashes once the test lets
/// it ([`Lab::let_run_crash`]): the test waits for what the crash needs instead of a timer.
const FAKE: &str = r#"#!/bin/sh
label='@LABEL@'
rec="$FAKE_REC"
ev="$rec/$label.events"
{ printf '%s' "$label"; for a in "$@"; do printf ' [%s]' "$a"; done; printf '\n'; } >> "$rec/argv"
run=$(($(wc -l < "$rec/argv")))
echo "auto_update=${KIMI_CODE_NO_AUTO_UPDATE-unset} shim=${FAKE_SHIM-unset}" > "$rec/$label.env"
echo "no_harness=${ATERM_NO_HARNESS-unset}" > "$rec/$label.left-out"
printf 'home=%s model=%s key=%s headers=%s\n' "${KIMI_CODE_HOME-default}" "${KIMI_MODEL_NAME+set}" "${KIMI_MODEL_API_KEY+set}" "${KIMI_CODE_CUSTOM_HEADERS+set}" >> "$rec/$label.account"
ulimit -c 0
case "${FAKE_MODE:-session}" in
exit) exit "${FAKE_EXIT:-0}" ;;
selfkill) kill -"${FAKE_SIG:-SEGV}" $$; exit 99 ;;
stop) echo "$$" > "$rec/$label.pid"; kill -STOP $$; echo continued >> "$ev"; exit "${FAKE_EXIT:-5}" ;;
esac
spid=
workpid=
lock=
quit() { if [ -n "$lock" ]; then rm -f "$lock"; fi; if [ -n "$spid" ]; then kill "$spid" 2>/dev/null; fi; if [ -n "$workpid" ]; then kill "$workpid" 2>/dev/null; fi; echo "$1" >> "$ev"; exit "$2"; }
trap 'if [ -n "$FAKE_IGNORE_TERM" ] && [ ! -e "$rec/honor-term" ]; then echo term-ignored >> "$ev"; else quit term 143; fi' TERM
trap 'quit hup 129' HUP
trap 'echo int >> "$ev"' INT
if [ "${FAKE_SESSION:-sess-1}" != none ]; then
  mkdir -p "$KIMI_CODE_HOME/locks/sessions"
  lock="$KIMI_CODE_HOME/locks/sessions/${FAKE_SESSION:-sess-1}.lock"
  printf '{"version":1,"token":"%s:x","pid":%s,"hostname":"%s","startedAt":1,"clientIdentity":"kimi_code_cli"}' "$$" "$$" "$(uname -n)" > "$lock.tmp"
  mv "$lock.tmp" "$lock"
fi
if [ -n "$FAKE_READ" ]; then exec 3<&0; cat <&3 > /dev/null & fi
if [ -e "$rec/background-work" ]; then
  # A shell that starts no process of its own while it waits, so the tree stays the same.
  mkfifo "$rec/background.fifo"
  sh -c 'read x < "$1"' sh "$rec/background.fifo" & workpid=$!
fi
sleep 3600 & spid=$!
# Every process of the idle tree runs before the pid is published; from here on only builtins
# run, apart from a crash's own wait.
echo "$$" > "$rec/$label.pid.tmp"; mv "$rec/$label.pid.tmp" "$rec/$label.pid"
if { [ "${FAKE_MODE:-session}" = crash ] && [ "$run" = 1 ]; } || [ "${FAKE_MODE:-session}" = crashloop ]; then
  while [ ! -e "$rec/crash-$run" ]; do sleep 0.05; done
  if [ -e "$rec/unlock-before-crash" ]; then rm -f "$lock"; lock=; fi
  kill -"${FAKE_SIG:-SEGV}" $$
fi
if [ -n "$FAKE_BURN" ]; then while [ ! -e "$rec/stop-burn" ]; do :; done; fi
if [ -n "$FAKE_DRAW" ]; then while :; do printf .; done; fi
while :; do
  wait "$spid"
  if ! kill -0 "$spid" 2>/dev/null; then sleep 3600 & spid=$!; fi
done
"#;

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A scratch atpkg store, Kimi home and record directory, and the harness process group,
/// which is killed (with every fake under it) when the lab is dropped.
struct Lab {
    root: PathBuf,
    pgid: Option<Pid>,
    pty: Option<Pty>,
}

struct Pty {
    writer: File,
    seen: Arc<Mutex<Vec<u8>>>,
}

/// A fake aterm (alab-wire's, feature `fake`) laid for this harness: kimi-alab's roster row, the
/// policy in effect serving contracts 2, 3 and 4, a quiet session, and no claim. A scenario
/// changes it while the harness runs.
fn fake_aterm(name: &str) -> FakeAterm {
    FakeAterm::for_harness(name, &kimi_alab::aterm::config())
}

/// What aterm answers a read while it shows a dialog: it cannot say now, and asks to be asked
/// again.
const DIALOG_ERR: &str = "ERR aterm is showing a dialog; answer it, then retry";

/// Every claim the harness sent (`meta set supervisor …`), in order, without its `@<sid> `.
fn claims(aterm: &FakeAterm) -> Vec<String> {
    aterm
        .verbs()
        .into_iter()
        .filter(|verb| verb.starts_with("meta set supervisor "))
        .collect()
}

/// How many `harness policy` reads aterm was asked for.
fn policy_reads(aterm: &FakeAterm) -> usize {
    aterm
        .verbs()
        .iter()
        .filter(|verb| *verb == "harness policy")
        .count()
}

impl Lab {
    fn new(tag: &str) -> Self {
        Self::new_in(&std::env::temp_dir(), tag)
    }

    /// A lab beside the harness binary (Cargo's per-target scratch directory), so a store
    /// binary can be a HARD LINK to the harness.
    fn near_the_harness(tag: &str) -> Self {
        Self::new_in(Path::new(env!("CARGO_TARGET_TMPDIR")), tag)
    }

    fn new_in(base: &Path, tag: &str) -> Self {
        let root = base.join(format!(
            "kimi-alab-it-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["pkg", "rec", "kimi-home", "home"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        Self {
            root,
            pgid: None,
            pty: None,
        }
    }

    fn prefix(&self) -> PathBuf {
        self.root.join("pkg")
    }

    fn rec(&self) -> PathBuf {
        self.root.join("rec")
    }

    fn journal(&self) -> String {
        std::fs::read_to_string(self.root.join("journal.log")).unwrap_or_default()
    }

    /// Lay build `id` (`label`, vendor `version`) in the store, with its marker and records.
    fn lay(&self, id: u64, label: &str, version: &str) {
        self.lay_script(id, &FAKE.replace("@LABEL@", label), version);
    }

    /// Lay build `id` whose `bin/kimi` is the script `text`.
    fn lay_script(&self, id: u64, text: &str, version: &str) {
        let exe = self.lay_dir(id, version).join("bin/kimi");
        std::fs::write(&exe, text).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Lay build `id` whose `bin/kimi` is a hard link to the harness binary itself.
    fn lay_harness(&self, id: u64, version: &str) {
        let exe = self.lay_dir(id, version).join("bin/kimi");
        std::fs::hard_link(HARNESS, &exe).unwrap();
    }

    fn lay_own_image(&self, id: u64) -> PathBuf {
        let dir = self.prefix().join("store/kimi-alab").join(id.to_string());
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let exe = dir.join("bin/kimi-alab");
        std::fs::copy(HARNESS, &exe).unwrap();
        std::fs::write(
            dir.with_file_name(format!("{id}.ready")),
            "ok\ncontents=bin:kimi-alab\n",
        )
        .unwrap();
        exe
    }

    fn make_own_current(&self, id: u64) {
        let pdir = self.prefix().join("store/kimi-alab");
        let tmp = pdir.join(".current.tmp");
        let _ = std::fs::remove_file(&tmp);
        symlink(pdir.join(id.to_string()), &tmp).unwrap();
        std::fs::rename(&tmp, pdir.join("current")).unwrap();
    }

    /// Lay build `id`'s directory and sidecars; the caller writes `bin/kimi`.
    fn lay_dir(&self, id: u64, version: &str) -> PathBuf {
        let dir = self.prefix().join("store/kimi").join(id.to_string());
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let side = |suffix: &str, text: &str| {
            std::fs::write(dir.with_file_name(format!("{id}.{suffix}")), text).unwrap();
        };
        side(
            "vendor",
            &format!("schema = 1\nprogram = \"kimi\"\nversion = \"{version}\"\n"),
        );
        side("shim-env", "KIMI_CODE_NO_AUTO_UPDATE=1\nFAKE_SHIM=on\n");
        side("ready", "ok\ncontents=bin:kimi\n");
        dir
    }

    /// The depths the [`CHAIN`] store binary recorded, one per run of it.
    fn chain(&self) -> Vec<String> {
        std::fs::read_to_string(self.rec().join("chain"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Point `store/kimi/current` at `id`, atomically, as atpkg flips it.
    fn make_current(&self, id: u64) {
        let pdir = self.prefix().join("store/kimi");
        let tmp = pdir.join(".current.tmp");
        let _ = std::fs::remove_file(&tmp);
        symlink(pdir.join(id.to_string()), &tmp).unwrap();
        std::fs::rename(&tmp, pdir.join("current")).unwrap();
    }

    fn command(&self, args: &[&str], fake: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(HARNESS);
        cmd.args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("KIMI_ALAB_PKG_PREFIX", self.prefix())
            .env("KIMI_ALAB_JOURNAL", self.root.join("journal.log"))
            .env("KIMI_CODE_HOME", self.root.join("kimi-home"))
            .env("KIMI_ALAB_POLL_MS", POLL_MS.to_string())
            .env("KIMI_ALAB_QUIET_MS", QUIET_MS.to_string())
            .env("KIMI_ALAB_ATTEST_MS", "15000")
            .env("FAKE_REC", self.rec())
            .process_group(0);
        for (k, v) in fake {
            cmd.env(k, v);
        }
        cmd
    }

    /// Start the harness with no terminal.
    fn spawn(&mut self, args: &[&str], fake: &[(&str, &str)]) -> Child {
        let child = self
            .command(args, fake)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        self.pgid = Some(Pid::from_child(&child));
        child
    }

    /// Start the harness on a fresh pty, as a terminal session would, with standard error sent
    /// to `stderr` instead of the pty when given.
    fn spawn_on_pty_with(
        &mut self,
        args: &[&str],
        fake: &[(&str, &str)],
        stderr: Option<Stdio>,
    ) -> Child {
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
        grantpt(&master).unwrap();
        unlockpt(&master).unwrap();
        let name = ptsname(&master, Vec::new()).unwrap();
        let slave = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(i32::try_from(rustix::fs::OFlags::NOCTTY.bits()).unwrap())
            .open(name.to_str().unwrap())
            .unwrap();
        let master = File::from(master);
        let writer = master.try_clone().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        std::thread::spawn(move || {
            let mut master = master;
            let mut buf = [0u8; 4096];
            while let Ok(n) = master.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        let child = self
            .command(args, fake)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(stderr.unwrap_or_else(|| Stdio::from(slave)))
            .spawn()
            .unwrap();
        self.pgid = Some(Pid::from_child(&child));
        self.pty = Some(Pty { writer, seen });
        child
    }

    fn screen(&self) -> String {
        self.pty
            .as_ref()
            .map(|p| String::from_utf8_lossy(&p.seen.lock().unwrap()).into_owned())
            .unwrap_or_default()
    }

    /// The terminal's keyboard: what is written here is typed on the pty.
    fn keyboard(&self) -> File {
        self.pty.as_ref().unwrap().writer.try_clone().unwrap()
    }

    fn pid_of(&self, label: &str) -> Option<Pid> {
        std::fs::read_to_string(self.rec().join(format!("{label}.pid")))
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
            .and_then(Pid::from_raw)
    }

    fn events(&self, label: &str) -> String {
        std::fs::read_to_string(self.rec().join(format!("{label}.events"))).unwrap_or_default()
    }

    fn argv(&self) -> String {
        std::fs::read_to_string(self.rec().join("argv")).unwrap_or_default()
    }

    fn lock_exists(&self) -> bool {
        self.root
            .join("kimi-home/locks/sessions/sess-1.lock")
            .exists()
    }

    /// Lay A as current, start the harness on a pty with `args`, and wait for A to be idle.
    fn running_a_on_pty(&mut self, args: &[&str], fake: &[(&str, &str)]) -> Child {
        self.running_a_on_pty_with(args, fake, None)
    }

    /// [`Lab::running_a_on_pty`], with standard error sent to `stderr` when given.
    fn running_a_on_pty_with(
        &mut self,
        args: &[&str],
        fake: &[(&str, &str)],
        stderr: Option<Stdio>,
    ) -> Child {
        self.lay(BUILD_A, "A", "2.3.1");
        self.make_current(BUILD_A);
        let child = self.spawn_on_pty_with(args, fake, stderr);
        assert!(
            wait_for(DEADLINE, || self.pid_of("A").is_some()),
            "A never started"
        );
        child
    }

    /// Install B and make it current, then wait until the harness has seen it.
    fn install_b(&self) {
        self.lay(BUILD_B, "B", "2.3.2");
        self.make_current(BUILD_B);
        assert!(
            wait_for(DEADLINE, || self.journal().contains("UPDATE ")),
            "the harness never saw the update; journal:\n{}",
            self.journal()
        );
    }

    /// How many journal lines are `event` notes.
    fn notes(&self, event: &str) -> usize {
        let event = format!(" {event} ");
        self.journal()
            .lines()
            .filter(|l| l.contains(&event))
            .count()
    }

    /// Let run `run` (counted from 1) of a `crash` or `crashloop` fake crash, once the harness
    /// has read that run's session lock: a crash before it has no conversation to resume.
    fn let_run_crash(&self, run: usize) {
        assert!(
            wait_for(DEADLINE, || self.notes("SESSION") >= run),
            "the harness never read run {run}'s session; journal:\n{}",
            self.journal()
        );
        std::fs::write(self.rec().join(format!("crash-{run}")), "").unwrap();
    }

    /// Wait until the harness, `child`, has finished a whole pass of its loop that did its work
    /// after this call. The harness takes a `SIGINT` while a child runs at the top of a pass and
    /// journals it there as `IGNORED`, ahead of that pass's decisions; a second one, sent only
    /// once the first is journalled, is taken at the top of a later pass, which starts only
    /// when the first has ended. Every decision of the first pass is journalled by then.
    fn pass(&self, child: &mut Child) {
        self.ignored_sigint(child);
        self.ignored_sigint(child);
    }

    /// Let `window` of the harness's own clock go by with the harness running, for an absence:
    /// a whole pass first, so a timer a wrong decision would start has started; then `window`;
    /// then a whole pass that decides with at least `window` gone since. What that decision
    /// would journal is in the journal when this returns, however loaded the machine.
    fn pass_after(&self, child: &mut Child, window: Duration) {
        self.pass(child);
        sleep(window);
        self.pass(child);
    }

    /// Send the harness a `SIGINT` and wait until a pass of its loop has taken it.
    fn ignored_sigint(&self, child: &mut Child) {
        let before = self.notes("IGNORED");
        signal(child, Signal::INT);
        assert!(
            wait_for(DEADLINE, || self.notes("IGNORED") > before),
            "the harness never took its SIGINT (status {:?}); journal:\n{}",
            child.try_wait().unwrap(),
            self.journal()
        );
    }
}

/// The milliseconds since the epoch the journal stamped on its first `event` note matching
/// `detail`.
fn stamp(journal: &str, event: &str, detail: &str) -> Option<u128> {
    let event = format!(" {event} ");
    journal
        .lines()
        .find(|l| l.contains(&event) && l.contains(detail))
        .and_then(|l| l.split(' ').next())
        .and_then(|ms| ms.parse().ok())
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::write(self.rec().join("stop-burn"), "");
        if let Some(pgid) = self.pgid {
            let _ = kill_process_group(pgid, Signal::KILL);
        }
        for label in ["A", "B", "C"] {
            if let Some(pid) = self.pid_of(label) {
                let _ = kill_process(pid, Signal::KILL);
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn wait_for(limit: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if cond() {
            return true;
        }
        sleep(Duration::from_millis(25));
    }
    cond()
}

fn wait_exit(child: &mut Child, limit: Duration) -> ExitStatus {
    let end = Instant::now() + limit;
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        assert!(
            Instant::now() < end,
            "the harness did not exit in {limit:?}"
        );
        sleep(Duration::from_millis(25));
    }
}

fn signal(child: &Child, sig: Signal) {
    kill_process(Pid::from_child(child), sig).unwrap();
}

fn stderr_of(child: &mut Child) -> String {
    let mut s = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut s);
    }
    s
}

#[test]
fn refuses_when_the_vendor_is_missing() {
    let mut lab = Lab::new("missing");
    let mut child = lab.spawn(&["--yolo"], &[]);
    let st = wait_exit(&mut child, DEADLINE);
    let err = stderr_of(&mut child);
    assert_eq!(st.code(), Some(127), "stderr: {err}");
    assert!(err.contains("not installed in the atpkg store"), "{err}");
    assert!(err.contains("aterm pkg install kimi"), "{err}");
    assert!(lab.argv().is_empty(), "something ran: {}", lab.argv());
}

#[test]
fn version_names_the_harness_and_the_vendor_version() {
    let version = env!("CARGO_PKG_VERSION");
    let line = |vendor: &str| format!("kimi-alab {version} (kimi {vendor})\n");
    let lab = Lab::new("version");
    let run = |lab: &Lab| {
        let out = lab.command(&["--version"], &[]).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert_eq!(run(&lab), line("not installed"));
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    assert_eq!(run(&lab), line("2.3.1"));
    // The record alone names the version, and only when it names this build: a record that is
    // missing, garbage or names another version says `unknown`, never the version the build id
    // encodes and never the vendor's own answer.
    let record = lab
        .prefix()
        .join("store/kimi")
        .join(format!("{BUILD_A}.vendor"));
    for (why, text) in [
        ("garbage", Some("not a record\n")),
        ("no version", Some("schema = 1\nprogram = \"kimi\"\n")),
        ("another version", Some("version = \"2.3.2\"\n")),
        ("missing", None),
    ] {
        match text {
            Some(text) => std::fs::write(&record, text).unwrap(),
            None => std::fs::remove_file(&record).unwrap(),
        }
        assert_eq!(run(&lab), line("unknown"), "{why}");
    }
    // A build the store does not admit is not the one a run would start.
    lab.lay(BUILD_B, "B", "2.3.2");
    std::fs::write(
        lab.prefix()
            .join("store/kimi")
            .join(format!("{BUILD_B}.ready")),
        "staging\n",
    )
    .unwrap();
    lab.make_current(BUILD_B);
    assert_eq!(run(&lab), line("unknown"), "not ready");
    assert!(
        lab.argv().is_empty(),
        "--version ran the vendor: {}",
        lab.argv()
    );
}

#[test]
fn passes_every_argument_through_and_exits_with_the_childs_code() {
    let mut lab = Lab::new("passthrough");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(
        &["--yolo", "a b", "", "--alab-which", "-V"],
        &[("FAKE_MODE", "exit"), ("FAKE_EXIT", "7")],
    );
    let st = wait_exit(&mut child, DEADLINE);
    assert_eq!(st.code(), Some(7), "stderr: {}", stderr_of(&mut child));
    assert_eq!(lab.argv(), "A [--yolo] [a b] [] [--alab-which] [-V]\n");
    let env = std::fs::read_to_string(lab.rec().join("A.env")).unwrap();
    assert_eq!(
        env.trim(),
        "auto_update=1 shim=on",
        "the child must get the build's shim env"
    );
}

#[test]
fn atpkg_run_envelope_passes_vendor_arguments_verbatim() {
    let mut lab = Lab::new("atpkg-run");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(
        &["run", "--", "--version", "a b", "", "--alab-which"],
        &[("FAKE_MODE", "exit"), ("FAKE_EXIT", "0")],
    );
    let st = wait_exit(&mut child, DEADLINE);
    assert!(st.success(), "stderr: {}", stderr_of(&mut child));
    assert_eq!(lab.argv(), "A [--version] [a b] [] [--alab-which]\n");
}

/// A stand-in `aterm` on `PATH`: it records how it was run (its pid, the self-update re-entry
/// guard it was handed, and its words) in `$FAKE_REC/aterm.calls`, then, under `FAKE_FORWARD`,
/// does what atpkg's `__selfupdate` does with a shape it forwards to the vendor: it runs the
/// launcher again by `exec`, under the same pid, with the words after `--`. Otherwise it exits 7.
const FAKE_ATERM: &str = r#"#!/bin/sh
{ printf 'pid=%s guard=%s' "$$" "${__ATPKG_SELFUPDATE_PID-unset}"; for a in "$@"; do printf ' [%s]' "$a"; done; printf '\n'; } >> "$FAKE_REC/aterm.calls"
if [ -n "$FAKE_FORWARD" ]; then
  while [ $# -gt 0 ] && [ "$1" != "--" ]; do shift; done
  shift
  exec "$HARNESS_BIN" run -- "$@"
fi
exit 7
"#;

impl Lab {
    /// Lay [`FAKE_ATERM`] as `aterm` in a directory of its own, and return the `PATH` that puts
    /// it ahead of the system's.
    fn lay_aterm(&self) -> String {
        let bin = self.root.join("aterm-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let aterm = bin.join("aterm");
        std::fs::write(&aterm, FAKE_ATERM).unwrap();
        std::fs::set_permissions(&aterm, std::fs::Permissions::from_mode(0o755)).unwrap();
        format!("{}:/usr/bin:/bin", bin.display())
    }

    /// What the stand-in `aterm` recorded, one line per run.
    fn aterm_calls(&self) -> String {
        std::fs::read_to_string(self.rec().join("aterm.calls")).unwrap_or_default()
    }

    /// Run the harness with `args` to its end, with no terminal and Kimi exiting at once:
    /// its status, its standard error, and the vendor runs it made (`argv`, emptied first).
    fn launch(&mut self, args: &[&str], fake: &[(&str, &str)]) -> (ExitStatus, String, String) {
        let _ = std::fs::remove_file(self.rec().join("argv"));
        let fake: Vec<(&str, &str)> = [("FAKE_MODE", "exit")]
            .into_iter()
            .chain(fake.iter().copied())
            .collect();
        let mut child = self.spawn(args, &fake);
        let status = wait_exit(&mut child, DEADLINE);
        let stderr = stderr_of(&mut child);
        (status, stderr, self.argv())
    }
}

/// The launcher's yolo block, carried into `run` (atpkg `yolo.rs` `decide`, printed as its
/// `sh` block prints it): a launch through `run --` that names no approval posture gets Kimi's
/// `--auto` in front, a typed `--yolo` becomes `--auto` (dropping Kimi's weaker `-y` ahead of
/// the first word), and either prints the command as run on standard error, byte for byte as
/// atpkg's launcher does. A launch that names its own posture or a management word runs as
/// typed; so does one outside the envelope, which is pure pass-through. `--no-harness` is taken
/// off and Kimi runs in the harness's place with no default, and `ATERM_NO_HARNESS=1` gets none
/// either.
#[test]
fn run_sets_the_yolo_default() {
    let mut lab = Lab::new("yolo-default");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    for (args, ran, said) in [
        (
            &["run", "--"][..],
            "A [--auto]\n",
            "aterm: yolo (default): running 'kimi' '--auto'\n",
        ),
        (
            &["run", "--", "-m", "k2", "it's"],
            "A [--auto] [-m] [k2] [it's]\n",
            "aterm: yolo (default): running 'kimi' '--auto' '-m' 'k2' 'it'\\''s'\n",
        ),
        (
            &["run", "--", "-y", "--yolo", "-m", "k2"],
            "A [--auto] [-m] [k2]\n",
            "aterm: --yolo: running 'kimi' '--auto' '-m' 'k2'\n",
        ),
        (&["run", "--", "--plan"], "A [--plan]\n", ""),
        (&["run", "--", "--session=abc"], "A [--session=abc]\n", ""),
        (&["run", "--", "login"], "A [login]\n", ""),
        (&["run", "--", "--unknown"], "A [--unknown]\n", ""),
        (&["hello"], "A [hello]\n", ""),
        (&["run", "--", "--no-harness", "hello"], "A [hello]\n", ""),
    ] {
        let (status, stderr, argv) = lab.launch(args, &[]);
        assert!(status.success(), "{args:?}: {stderr}");
        assert_eq!(argv, ran, "{args:?}");
        assert_eq!(stderr, said, "{args:?}");
    }
    let left_out = std::fs::read_to_string(lab.rec().join("A.left-out")).unwrap();
    assert_eq!(
        left_out.trim(),
        "no_harness=1",
        "--no-harness reaches Kimi as the variable"
    );
    assert!(
        lab.journal()
            .contains("--no-harness leaves the harness out"),
        "journal:\n{}",
        lab.journal()
    );
    let (status, stderr, argv) = lab.launch(&["run", "--", "hello"], &[("ATERM_NO_HARNESS", "1")]);
    assert!(status.success(), "{stderr}");
    assert_eq!((argv.as_str(), stderr.as_str()), ("A [hello]\n", ""));
}

/// The launcher's sign-in gate, carried into `run`: Kimi has none. Its atpkg row says
/// `signin_gate: None` (aterm `vendor_direct/table.rs`) and `aterm harness signin` serves
/// `--cli grok` alone, so atpkg's launcher runs a signed-out interactive Kimi launch inside aterm
/// as it is, and so does `run`: Kimi starts, with the yolo default, and aterm is never asked to
/// sign anyone in. NEGATIVE CONTROL: the same `aterm` on `PATH` is run for `run -- update`, so a
/// gate that called it would have been seen.
#[test]
fn run_gates_a_signed_out_launch() {
    let mut lab = Lab::new("signin");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let path = lab.lay_aterm();
    let kimi_home = lab.root.join("kimi-home");
    assert_eq!(
        std::fs::read_dir(&kimi_home).unwrap().count(),
        0,
        "signed out: Kimi's home holds no login"
    );
    let inside = [("PATH", path.as_str()), ("ATERM_CHILD", "1")];
    let (status, stderr, argv) = lab.launch(&["run", "--"], &inside);
    assert!(status.success(), "{stderr}");
    assert_eq!(argv, "A [--auto]\n");
    assert_eq!(
        lab.aterm_calls(),
        "",
        "the launch asked aterm for a sign-in"
    );
    let (status, _, argv) = lab.launch(&["run", "--", "update"], &inside);
    assert_eq!(status.code(), Some(7), "the control: aterm on PATH is run");
    assert_eq!(argv, "");
    assert_eq!(
        lab.aterm_calls().lines().count(),
        1,
        "{}",
        lab.aterm_calls()
    );
}

/// The launcher's self-update intercept, carried into `run`: `kimi update` and `kimi upgrade`
/// through `run --` replace the harness with `aterm pkg __selfupdate kimi <prefix> -- <words>`
/// (atpkg's hidden verb, which runs `aterm pkg update kimi`) under the re-entry guard
/// `__ATPKG_SELFUPDATE_PID` set to the harness's own pid, and Kimi never starts. A forward that
/// comes back under that pid (atpkg runs Kimi's own `update --help` by `exec`) runs Kimi with
/// the words and no yolo default. A self-update word that is not the first, or outside the
/// envelope, is Kimi's.
#[test]
fn run_intercepts_self_update() {
    let mut lab = Lab::new("self-update");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let path = lab.lay_aterm();
    let prefix = lab.prefix().display().to_string();
    let on_path = [("PATH", path.as_str()), ("HARNESS_BIN", HARNESS)];
    for word in ["update", "upgrade"] {
        let _ = std::fs::remove_file(lab.rec().join("aterm.calls"));
        let mut child = lab.spawn(&["run", "--", word], &on_path);
        let harness = child.id();
        let status = wait_exit(&mut child, DEADLINE);
        assert_eq!(
            status.code(),
            Some(7),
            "the harness was replaced by aterm: {}",
            stderr_of(&mut child)
        );
        assert_eq!(
            lab.aterm_calls(),
            format!(
                "pid={harness} guard={harness} [pkg] [__selfupdate] [kimi] [{prefix}] [--] [{word}]\n"
            )
        );
        assert_eq!(lab.argv(), "", "Kimi ran for `{word}`");
    }
    let _ = std::fs::remove_file(lab.rec().join("aterm.calls"));
    let forward: Vec<(&str, &str)> = on_path
        .iter()
        .copied()
        .chain([("FAKE_FORWARD", "1")])
        .collect();
    let (status, stderr, argv) = lab.launch(&["run", "--", "update", "--help"], &forward);
    assert!(status.success(), "{stderr}");
    assert_eq!(argv, "A [update] [--help]\n");
    assert_eq!(stderr, "", "no yolo default on a forwarded update");
    assert_eq!(
        lab.aterm_calls().lines().count(),
        1,
        "{}",
        lab.aterm_calls()
    );
    let _ = std::fs::remove_file(lab.rec().join("aterm.calls"));
    for (args, ran) in [
        (
            &["run", "--", "-m", "k2", "update"][..],
            "A [-m] [k2] [update]\n",
        ),
        (&["update"], "A [update]\n"),
        (&["run", "--", "Update"], "A [--auto] [Update]\n"),
    ] {
        let (status, stderr, argv) = lab.launch(args, &on_path);
        assert!(status.success(), "{args:?}: {stderr}");
        assert_eq!(argv, ran, "{args:?}");
    }
    assert_eq!(
        lab.aterm_calls(),
        "",
        "aterm ran for a launch that is Kimi's"
    );
}

/// The shim's environment, carried into `run`: atpkg's launcher exports it only after its
/// hand-off to the harness, so a harness-led Kimi gets it from the harness: the build's
/// `<build>.shim-env` (the entries atpkg's shim for that build exports: here
/// `KIMI_CODE_NO_AUTO_UPDATE=1` and one of the test's own) over whatever the launch's own
/// environment said, and with no sidecar the row's own `KIMI_CODE_NO_AUTO_UPDATE=1`, whether the
/// harness supervises Kimi or runs it in its place.
#[test]
fn run_sets_the_shim_env() {
    let mut lab = Lab::new("shim-env");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let env = |lab: &Lab| {
        std::fs::read_to_string(lab.rec().join("A.env"))
            .unwrap()
            .trim()
            .to_string()
    };
    let launch_env = [("KIMI_CODE_NO_AUTO_UPDATE", "0"), ("FAKE_SHIM", "off")];
    for args in [&["run", "--"][..], &["run", "--", "--no-harness"]] {
        let (status, stderr, _) = lab.launch(args, &launch_env);
        assert!(status.success(), "{args:?}: {stderr}");
        assert_eq!(env(&lab), "auto_update=1 shim=on", "{args:?}");
    }
    std::fs::remove_file(lab.prefix().join(format!("store/kimi/{BUILD_A}.shim-env"))).unwrap();
    for args in [&["run", "--"][..], &["run", "--", "--no-harness"]] {
        let (status, stderr, _) = lab.launch(args, &[]);
        assert!(status.success(), "{args:?}: {stderr}");
        assert_eq!(env(&lab), "auto_update=1 shim=unset", "{args:?}");
    }
}

#[test]
fn dies_by_the_signal_that_killed_the_child() {
    let mut lab = Lab::new("signal-death");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[("FAKE_MODE", "selfkill"), ("FAKE_SIG", "SEGV")]);
    let st = wait_exit(&mut child, DEADLINE);
    assert_eq!(
        st.signal(),
        Some(11),
        "status {st:?}, stderr: {}",
        stderr_of(&mut child)
    );
    assert!(
        !st.core_dumped(),
        "the harness must not dump core for the child's crash"
    );
}

#[test]
fn int_is_ignored_and_term_is_passed_on() {
    let mut lab = Lab::new("term");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[]);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("A").is_some()),
        "A never started"
    );
    signal(&child, Signal::INT);
    signal(&child, Signal::QUIT);
    // The harness journals each signal it keeps from the child on the pass that takes it.
    assert!(
        wait_for(DEADLINE, || {
            let j = lab.journal();
            j.contains("IGNORED signal 2 ") && j.contains("IGNORED signal 3 ")
        }),
        "the harness never took SIGINT and SIGQUIT; journal:\n{}",
        lab.journal()
    );
    assert!(
        child.try_wait().unwrap().is_none(),
        "SIGINT/SIGQUIT to the harness ended it"
    );
    signal(&child, Signal::TERM);
    let st = wait_exit(&mut child, DEADLINE);
    assert_eq!(st.code(), Some(143), "the child's own status after SIGTERM");
    let events = lab.events("A");
    assert!(events.contains("term"), "SIGTERM never reached the child");
    // An absence, looked for once A has ended: had the harness passed SIGINT on, it did so
    // before the SIGTERM, and A's shell runs a pending INT trap before its TERM trap, whose
    // exit ends it; so a forwarded SIGINT is in A's events by now, however late A ran.
    assert!(
        !events.contains("int"),
        "the harness forwarded SIGINT, which belongs to the child; events:\n{events}"
    );
    assert!(!lab.lock_exists(), "the child did not end its session");
}

#[test]
fn hup_is_passed_on() {
    let mut lab = Lab::new("hup");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[]);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("A").is_some()),
        "A never started"
    );
    signal(&child, Signal::HUP);
    let st = wait_exit(&mut child, DEADLINE);
    assert_eq!(st.code(), Some(129));
    assert!(lab.events("A").contains("hup"));
}

#[test]
fn a_child_that_stops_stops_the_harness_and_continue_resumes_both() {
    let mut lab = Lab::new("stop");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[("FAKE_MODE", "stop"), ("FAKE_EXIT", "5")]);
    let harness = Pid::from_child(&child);
    let state = |pid: Pid| {
        let out = Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid.as_raw_nonzero().to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert!(
        wait_for(DEADLINE, || state(harness).starts_with('T')),
        "the harness did not stop with its child (state {:?}); journal:\n{}",
        state(harness),
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    // As a shell's `fg` does: continue the whole job.
    kill_process_group(harness, Signal::CONT).unwrap();
    let st = wait_exit(&mut child, DEADLINE);
    assert_eq!(st.code(), Some(5));
    assert!(lab.events("A").contains("continued"));
}

#[test]
fn moves_onto_a_new_build_after_the_session_lock_disappears() {
    let mut lab = Lab::new("move");
    let mut child = lab.running_a_on_pty(&["--yolo", "--skills-dir", "/sk", "-m", "k2"], &[]);
    let harness = Pid::from_child(&child);
    lab.install_b();
    std::fs::remove_file(lab.root.join("kimi-home/locks/sessions/sess-1.lock")).unwrap();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("B").is_some()),
        "never moved to B; journal:\n{}",
        lab.journal()
    );
    assert!(
        lab.events("A").contains("term"),
        "A was not stopped by SIGTERM"
    );
    assert_eq!(
        lab.argv(),
        "A [--yolo] [--skills-dir] [/sk] [-m] [k2]\nB [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(
        child.try_wait().unwrap().is_none(),
        "the harness exited during the move"
    );
    assert_eq!(Pid::from_child(&child), harness);
    assert!(
        wait_for(DEADLINE, || lab
            .screen()
            .contains("resuming session sess-1")),
        "screen: {:?}",
        lab.screen()
    );
    // B can start, and say so, before the harness journals that it started it.
    let events = ["START", "UPDATE", "ATTEST", "MOVE", "RELAUNCHED"];
    assert!(
        wait_for(DEADLINE, || {
            let j = lab.journal();
            events.iter().all(|event| j.contains(event))
        }),
        "not all of {events:?} in the journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        std::fs::read_to_string(lab.rec().join("B.env"))
            .unwrap()
            .trim(),
        "auto_update=1 shim=on"
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// The fake aterm's session variables with `extra` after them, as the lab's spawn takes them.
fn vars<'a>(
    aterm: &'a [(String, String)],
    extra: &[(&'a str, &'a str)],
) -> Vec<(&'a str, &'a str)> {
    aterm
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .chain(extra.iter().copied())
        .collect()
}

/// The launch claims at contract 4 (the highest aterm serves), before the child starts, and
/// names the child once it runs; aterm attests a safe point and the move announces the new
/// vendor build with the new child.
#[test]
fn aterm_attests_and_reclaims_an_automatic_move() {
    let mut lab = Lab::new("aterm-move");
    let aterm = fake_aterm("aterm-move");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&["--skills-dir", "/sk"], &vars(&env, &[]));
    assert!(
        wait_for(DEADLINE, || claims(&aterm).len() == 2),
        "aterm never proved the claim and its child; journal:\n{}",
        lab.journal()
    );
    lab.install_b();
    assert!(
        wait_for(DEADLINE, || lab.pid_of("B").is_some()),
        "aterm's attestation never moved B; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nB [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(
        wait_for(DEADLINE, || claims(&aterm).len() == 3),
        "the new vendor was not announced: {:?}",
        claims(&aterm)
    );
    let claims = claims(&aterm);
    let version = format!(
        " contract=4 scope=relaunch,upgrade version={} vendor=",
        kimi_alab::cli::VERSION
    );
    assert!(
        claims.iter().all(|claim| claim.contains(&version)),
        "{claims:?}"
    );
    // The launch's claim names no child: none runs yet. Each start names its own.
    assert!(claims[0].ends_with(" vendor=2.3.1"), "{claims:?}");
    let a = lab.pid_of("A").unwrap().as_raw_nonzero();
    let b = lab.pid_of("B").unwrap().as_raw_nonzero();
    assert!(
        claims[1].contains(&format!(" vendor=2.3.1 child={a}:")),
        "{claims:?}"
    );
    assert!(
        claims[2].contains(&format!(" vendor=2.3.2 child={b}:")),
        "{claims:?}"
    );
    assert!(
        lab.journal().contains("CLAIMED holder=kimi-alab@")
            && lab
                .journal()
                .contains(" contract=4 scope=relaunch,upgrade "),
        "journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// No request the harness sent names contract 3: no claim at 3, and no contract-3 roster read.
fn assert_never_contract_3(aterm: &FakeAterm) {
    let verbs = aterm.verbs();
    assert!(
        !verbs
            .iter()
            .any(|verb| verb.contains(" contract=3") || verb == "native-act info"),
        "{verbs:?}"
    );
}

/// Against an aterm from before contract 4 (it serves 2 and 3), no claim is made at all: this
/// build declares contract 4 alone (`alab-harness.toml`, `contracts = [4]`) to alab-wire, whose
/// claim sequence reads the policy and sends nothing more, and Kimi runs in the harness's
/// place. Not even the contract-3 roster guard is asked.
#[test]
fn against_an_aterm_without_contract_4_no_claim_is_made_and_kimi_runs_in_place() {
    let mut lab = Lab::new("aterm-three");
    let aterm = fake_aterm("aterm-three");
    aterm.serve_contracts(&[2, 3]);
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    assert_eq!(lab.pid_of("A"), Some(Pid::from_child(&child)));
    assert!(
        lab.journal()
            .contains("UNCLAIMED policy contract=2,3 declared=4")
            && lab.journal().contains(
                " PASS the vendor runs in the harness's place: policy contract=2,3 declared=4"
            ),
        "journal:\n{}",
        lab.journal()
    );
    assert!(claims(&aterm).is_empty(), "{:?}", aterm.verbs());
    assert_never_contract_3(&aterm);
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// A claim alab-wire makes again on its own clock keeps to contract 4 too: an aterm that
/// answered `ERR` at launch and then serves only 2 and 3 is sent no claim at all while Kimi runs
/// on unclaimed, and once it serves 4 again the claim is made at 4, naming the child that runs.
/// No request ever names contract 3.
/// NEGATIVE CONTROL: the claim at 4 once aterm serves it shows the link was claiming all along.
#[test]
fn a_claim_made_again_never_names_contract_3_and_is_made_at_4_once_served() {
    let mut lab = Lab::new("reclaim-three");
    let aterm = fake_aterm("reclaim-three");
    aterm.set_policy_reply(DIALOG_ERR);
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    assert_ne!(lab.pid_of("A"), Some(Pid::from_child(&child)));
    aterm.serve_contracts(&[2, 3]);
    aterm.clear_policy_reply();
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("UNCLAIMED policy contract=2,3 declared=4")),
        "journal:\n{}",
        lab.journal()
    );
    assert!(claims(&aterm).is_empty(), "{:?}", aterm.verbs());
    assert_eq!(aterm.holder(), None);
    assert!(lab.pid_of("A").is_some());
    aterm.serve_contracts(&[2, 3, 4]);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains(" contract=4 scope=relaunch,upgrade ")),
        "the claim was not made at 4 once aterm served it; journal:\n{}",
        lab.journal()
    );
    let claims = claims(&aterm);
    let a = lab.pid_of("A").unwrap().as_raw_nonzero();
    assert_eq!(claims.len(), 1, "{claims:?}");
    assert!(
        claims[0].contains(" contract=4 ") && claims[0].contains(&format!(" child={a}:")),
        "{claims:?}"
    );
    assert_never_contract_3(&aterm);
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// An aterm that restarts into one serving only 2 and 3 ends the claim at 4, and the claim made
/// again on the link's clock is never sent at 3. Its last word is that it serves no contract
/// this build declares, so a crash then is passed on at once, as with no claim, rather than
/// waiting out the 45 s budget for a claim that cannot come.
#[test]
fn a_crash_after_aterm_drops_contract_4_is_passed_on_at_once_and_3_is_never_claimed() {
    let mut lab = Lab::new("drop-four");
    let aterm = fake_aterm("drop-four");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(&env, &[("FAKE_MODE", "crash"), ("FAKE_SIG", "KILL")]),
    );
    assert!(
        wait_for(DEADLINE, || claims(&aterm).len() == 2),
        "aterm never proved the claim and its child; journal:\n{}",
        lab.journal()
    );
    aterm.restart();
    aterm.serve_contracts(&[2, 3]);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("UNCLAIMED policy contract=2,3 declared=4")),
        "journal:\n{}",
        lab.journal()
    );
    lab.let_run_crash(1);
    let status = wait_exit(&mut child, DEADLINE);
    assert_eq!(status.signal(), Some(9), "status: {status:?}");
    assert!(
        lab.journal()
            .contains("CRASH-PASSED-ON no-claim policy contract=2,3 declared=4")
            && !lab.journal().contains("CRASH-WAIT"),
        "journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.argv(), "A\n");
    assert_eq!(claims(&aterm).len(), 2, "{:?}", aterm.verbs());
    assert_never_contract_3(&aterm);
}

/// A crash that begins while the link is lost to an aterm restarting into one that serves only
/// 2 and 3, before the claim made again has heard that answer, waits for aterm. Once the answer
/// is read, on the link's 5 s clock, the crash is passed on as with no claim, rather than
/// waiting out the 45 s budget for a claim that cannot come and being passed on as
/// `claim-not-proven-again`: the harness exits within `DEADLINE`, well inside that budget.
/// NEGATIVE CONTROL: `CRASH-WAIT` journalled before `UNCLAIMED` shows the crash began before
/// aterm's answer, so it was not the check at the crash that passed it on.
#[test]
fn a_crash_waiting_for_aterm_is_passed_on_once_aterm_answers_it_serves_no_4() {
    let mut lab = Lab::new("wait-four");
    let aterm = fake_aterm("wait-four");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(&env, &[("FAKE_MODE", "crash"), ("FAKE_SIG", "KILL")]),
    );
    assert!(
        wait_for(DEADLINE, || claims(&aterm).len() == 2),
        "aterm never proved the claim and its child; journal:\n{}",
        lab.journal()
    );
    aterm.restart();
    aterm.serve_contracts(&[2, 3]);
    lab.let_run_crash(1);
    let status = wait_exit(&mut child, DEADLINE);
    assert_eq!(status.signal(), Some(9), "status: {status:?}");
    let journal = lab.journal();
    let waited = journal.find(" CRASH-WAIT ");
    let answered = journal.find(" UNCLAIMED policy contract=2,3 declared=4");
    assert!(
        matches!((waited, answered), (Some(w), Some(a)) if w < a),
        "the crash did not wait for aterm's answer; journal:\n{journal}"
    );
    assert!(
        journal.contains("CRASH-PASSED-ON no-claim policy contract=2,3 declared=4")
            && !journal.contains("claim-not-proven-again"),
        "journal:\n{journal}"
    );
    assert_eq!(lab.argv(), "A\n");
    assert_eq!(claims(&aterm).len(), 2, "{:?}", aterm.verbs());
    assert_never_contract_3(&aterm);
}

/// A restart aterm asked for that waits for aterm (its policy reads answer `ERR`) is refused
/// once aterm, restarted into one that serves only 2 and 3, answers the claim made again so: no
/// claim stands and none can, so it is not left to wait out the restart wait (45 s by default,
/// longer than `DEADLINE`) and is never made later.
#[test]
fn a_restart_waiting_for_aterm_is_refused_once_aterm_answers_it_serves_no_4() {
    let mut lab = Lab::new("rs-four");
    let aterm = fake_aterm("rs-four");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || lab.journal().contains("SESSION")));
    aterm.set_policy_reply(DIALOG_ERR);
    aterm.send_restart("asked");
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("RESTART-WAITS cause=asked aterm did not answer")),
        "the restart did not wait for aterm; journal:\n{}",
        lab.journal()
    );
    aterm.restart();
    aterm.serve_contracts(&[2, 3]);
    aterm.clear_policy_reply();
    assert!(
        wait_for(DEADLINE, || lab.journal().contains(
            "RESTART-REFUSED cause=asked no-claim policy contract=2,3 declared=4"
        )),
        "the restart was not refused on aterm's answer; journal:\n{}",
        lab.journal()
    );
    // An absence: the refused restart is never made later. A restart that waits is asked about
    // again a second after the pass that refused it; that and a whole pass go by, and the
    // harness's decision, a `RESTART` line, is looked for.
    lab.pass_after(
        &mut child,
        Duration::from_millis(alab_wire::link::LATER_RETRY_MS),
    );
    assert_eq!(lab.notes("RESTART"), 0, "journal:\n{}", lab.journal());
    assert_eq!(lab.pid_of("A"), Some(first));
    assert_eq!(lab.argv(), "A\n");
    assert_never_contract_3(&aterm);
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn aterm_relaunches_a_crashed_child_on_the_same_session() {
    let mut lab = Lab::new("cr");
    let aterm = fake_aterm("cr");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &vars(&env, &[("FAKE_MODE", "crash"), ("FAKE_SIG", "KILL")]),
    );
    let first = lab.pid_of("A").unwrap();
    lab.let_run_crash(1);
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the crash was not relaunched; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nA [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(lab.journal().contains("CRASH-RELAUNCH"));
    // The relaunched child can run before the harness announces it.
    assert!(
        wait_for(DEADLINE, || claims(&aterm).len() == 3),
        "{:?}",
        claims(&aterm)
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn aterm_relaunches_after_the_session_lock_disappears_before_a_crash() {
    let mut lab = Lab::new("cr-unlock");
    let aterm = fake_aterm("cr-unlock");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(&env, &[("FAKE_MODE", "crash"), ("FAKE_SIG", "KILL")]),
    );
    let first = lab.pid_of("A").unwrap();
    std::fs::write(lab.rec().join("unlock-before-crash"), "").unwrap();
    lab.let_run_crash(1);
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the crash was not relaunched after the lock disappeared; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
    assert!(lab.journal().contains("CRASH-RELAUNCH"));
    // The relaunched child can run before the harness announces it.
    assert!(
        wait_for(DEADLINE, || claims(&aterm).len() == 3),
        "{:?}",
        claims(&aterm)
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn aterm_restart_event_restarts_the_same_session() {
    let mut lab = Lab::new("rs");
    let aterm = fake_aterm("rs");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&["--skills-dir", "/sk"], &vars(&env, &[]));
    let first = lab.pid_of("A").unwrap();
    aterm.send_restart("asked");
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the restart event did not restart A; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nA [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(lab.journal().contains("RESTART cause=asked"));
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// aterm names a model on a restart from HX-06 on. Starting Kimi Code on the model it ran would
/// answer a model move with the model aterm moved it from, so until the harness can choose
/// Kimi's model on a resume, the restart is refused and the child runs on.
#[test]
fn a_restart_naming_a_model_is_refused_and_the_child_runs_on() {
    let mut lab = Lab::new("rs-model");
    let aterm = fake_aterm("rs-model");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || lab.journal().contains("SESSION")));
    aterm.send_model_restart("model", "kimi-k2-turbo");
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("RESTART-REFUSED cause=model model=kimi-k2-turbo")),
        "the model restart was not refused; journal:\n{}",
        lab.journal()
    );
    lab.pass(&mut child);
    assert_eq!(lab.notes("RESTART"), 0, "journal:\n{}", lab.journal());
    assert_eq!(lab.pid_of("A"), Some(first));
    assert_eq!(lab.argv(), "A\n");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// Lay an account home that shares the default home's sessions and locks, as aterm's pool
/// overlay homes do.
fn account_home(lab: &Lab) -> PathBuf {
    let target = lab.root.join("account home");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(lab.root.join("kimi-home/sessions")).unwrap();
    symlink(lab.root.join("kimi-home/locks"), target.join("locks")).unwrap();
    symlink(lab.root.join("kimi-home/sessions"), target.join("sessions")).unwrap();
    target
}

#[test]
fn aterm_account_event_moves_home_and_drops_credential_overrides() {
    let mut lab = Lab::new("account");
    let aterm = fake_aterm("account");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &vars(
            &env,
            &[
                ("KIMI_MODEL_NAME", "other-account"),
                ("KIMI_MODEL_API_KEY", "test-only-key"),
                ("KIMI_CODE_CUSTOM_HEADERS", "Authorization: test-only"),
            ],
        ),
    );
    let first = lab.pid_of("A").unwrap();
    let target = account_home(&lab);
    aterm.send_account_restart("account", "work", Some(&target));
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the account event did not restart A; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nA [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert_eq!(
        std::fs::read_to_string(lab.rec().join("A.account")).unwrap(),
        format!(
            "home={} model=set key=set headers=set\nhome={} model= key= headers=\n",
            lab.root.join("kimi-home").display(),
            target.display()
        )
    );
    assert!(
        lab.journal()
            .contains("RESTART cause=account to=work: resuming session sess-1")
    );
    // The restarted child can run before the harness announces its account.
    assert!(
        wait_for(DEADLINE, || claims(&aterm)
            .last()
            .is_some_and(|claim| claim.ends_with("login=work"))),
        "{:?}",
        claims(&aterm)
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn harness_reexec_keeps_the_child_pid_and_managed_account() {
    let mut lab = Lab::new("self-exec");
    lab.lay_own_image(1);
    lab.make_own_current(1);
    let aterm = fake_aterm("self-exec");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&["--skills-dir", "/sk"], &vars(&env, &[]));
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || claims(&aterm).len() == 2));
    let target = account_home(&lab);
    aterm.send_account_restart("account", "work", Some(&target));
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first) && claims(&aterm).len() == 3
        }),
        "the account move did not finish; journal:\n{}",
        lab.journal()
    );
    let preserved = lab.pid_of("A").unwrap();

    lab.lay_own_image(2);
    lab.make_own_current(2);
    assert!(
        wait_for(DEADLINE, || {
            lab.journal().contains(" ADOPT ") && claims(&aterm).len() == 4
        }),
        "the new harness image did not adopt the child; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(lab.pid_of("A"), Some(preserved), "the child was replaced");
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nA [--skills-dir] [/sk] [--session] [sess-1]\n",
        "re-exec started no vendor process"
    );
    assert_eq!(
        std::fs::read_to_string(lab.rec().join("A.account")).unwrap(),
        format!(
            "home={} model= key= headers=\nhome={} model= key= headers=\n",
            lab.root.join("kimi-home").display(),
            target.display()
        )
    );
    // The new image's claim names the same child it adopted, on the same account.
    let last = claims(&aterm).pop().unwrap();
    assert!(
        last.contains(&format!(" child={}:", preserved.as_raw_nonzero()))
            && last.ends_with("login=work"),
        "{last}"
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_failed_harness_reexec_leaves_the_old_supervisor_and_child_alive() {
    let mut lab = Lab::new("self-exec-fails");
    lab.lay_own_image(1);
    lab.make_own_current(1);
    let mut child = lab.running_a_on_pty(&[], &[]);
    let vendor = lab.pid_of("A").unwrap();
    let replacement = lab.lay_own_image(2);
    lab.make_own_current(2);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("SELF-UPDATE ")),
        "the harness did not see its update; journal:\n{}",
        lab.journal()
    );
    std::fs::remove_file(replacement).unwrap();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("REEXEC-FAILED")),
        "the failed exec was not journaled; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(lab.pid_of("A"), Some(vendor));
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_crash_streak_climbs_the_ladder_and_a_signal_in_the_pause_ends_the_harness() {
    let mut lab = Lab::new("ladder");
    let aterm = fake_aterm("ladder");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[("FAKE_MODE", "crashloop")]));
    lab.let_run_crash(1);
    lab.let_run_crash(2);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("streak=1 pause-ms=10000")),
        "the second crash did not wait 10 s; journal:\n{}",
        lab.journal()
    );
    assert!(lab.journal().contains("streak=0 pause-ms=1000"));
    // The pause runs: nothing is started for its first seconds, and the harness waits on. The
    // seconds count from when the harness journalled the pause, not from when this test saw
    // it, so the look falls well inside the pause however late the test runs.
    let pause = alab_wire::relaunch::BACKOFF[1];
    let began = stamp(&lab.journal(), "CRASH", "streak=1 pause-ms=10000").unwrap();
    let look = began + pause.as_millis() * 2 / 5;
    while now_ms() < look {
        sleep(Duration::from_millis(25));
    }
    // What is seen and sent here is judged only once it is known to lie inside the pause: a
    // test thread that wakes after the third run began would see that run, not a fault.
    let argv = lab.argv();
    let running = child.try_wait().unwrap().is_none();
    // Ctrl-C while no child runs ends the harness the way the child ended.
    signal(&child, Signal::INT);
    let end = began + pause.as_millis() - u128::from(POLL_MS);
    assert!(
        now_ms() < end,
        "the test ran past the pause, so it could not look inside it; journal:\n{}",
        lab.journal()
    );
    assert_eq!(argv, "A\nA [--session] [sess-1]\n");
    assert!(running);
    let status = wait_exit(&mut child, DEADLINE);
    assert_eq!(status.signal(), Some(11), "status: {status:?}");
    assert!(
        lab.journal().contains("CRASH-PASSED-ON signal 2"),
        "journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
}

#[test]
fn a_crash_just_after_a_person_typed_is_theirs_and_is_not_relaunched() {
    let mut lab = Lab::new("cr-person");
    let aterm = fake_aterm("cr-person");
    aterm.set_status("human_ms", Some("500"));
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(&env, &[("FAKE_MODE", "crash"), ("FAKE_SIG", "KILL")]),
    );
    lab.let_run_crash(1);
    let status = wait_exit(&mut child, DEADLINE);
    assert_eq!(status.signal(), Some(9), "status: {status:?}");
    assert_eq!(lab.argv(), "A\n");
    assert!(
        lab.journal()
            .contains("CRASH-PASSED-ON person human_ms=500"),
        "journal:\n{}",
        lab.journal()
    );
}

#[test]
fn a_crash_while_aterm_answers_err_waits_and_is_relaunched() {
    let mut lab = Lab::new("cr-err");
    let aterm = fake_aterm("cr-err");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(&env, &[("FAKE_MODE", "crash"), ("FAKE_SIG", "KILL")]),
    );
    let first = lab.pid_of("A").unwrap();
    // A dialog comes up in aterm before the crash: its policy reads answer `ERR` until it goes.
    aterm.set_policy_reply(DIALOG_ERR);
    lab.let_run_crash(1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("CRASH-WAIT")),
        "the crash did not wait for aterm; journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.argv(), "A\n");
    aterm.clear_policy_reply();
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the crash was not relaunched once aterm answered; journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
    assert!(child.try_wait().unwrap().is_none());
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_signal_while_the_crash_waits_for_aterm_ends_the_harness_as_the_child_ended() {
    let mut lab = Lab::new("cr-wait-sig");
    let aterm = fake_aterm("cr-wait-sig");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(&env, &[("FAKE_MODE", "crash"), ("FAKE_SIG", "KILL")]),
    );
    aterm.set_policy_reply(DIALOG_ERR);
    lab.let_run_crash(1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("CRASH-WAIT")),
        "the crash did not wait for aterm; journal:\n{}",
        lab.journal()
    );
    // The harness asks aterm again, is answered `ERR` again, and waits on.
    let asked = policy_reads(&aterm);
    assert!(
        wait_for(DEADLINE, || policy_reads(&aterm) > asked
            || child.try_wait().unwrap().is_some()),
        "the harness never asked aterm again; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none(), "{}", lab.journal());
    signal(&child, Signal::QUIT);
    let status = wait_exit(&mut child, DEADLINE);
    assert_eq!(status.signal(), Some(9), "status: {status:?}");
    assert!(lab.journal().contains("CRASH-PASSED-ON signal 3"));
    assert_eq!(lab.argv(), "A\n");
}

#[test]
fn an_asked_restart_is_refused_under_another_drivers_hand() {
    let mut lab = Lab::new("rs-hand");
    let aterm = fake_aterm("rs-hand");
    aterm.set_status("hand", Some("lease:driver%404"));
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || lab.journal().contains("SESSION")));
    aterm.send_restart("asked");
    assert!(
        wait_for(DEADLINE, || lab.journal().contains(
            "RESTART-REFUSED cause=asked held hold=0 hand=lease:driver%25404"
        )),
        "the restart was not refused; journal:\n{}",
        lab.journal()
    );
    // An absence: the refused restart is never made later. A restart that waits is asked about
    // again a second after the pass that refused it; that and a whole pass go by, and the
    // harness's decision, a `RESTART` line, is looked for.
    lab.pass_after(
        &mut child,
        Duration::from_millis(alab_wire::link::LATER_RETRY_MS),
    );
    assert_eq!(lab.notes("RESTART"), 0, "journal:\n{}", lab.journal());
    assert_eq!(lab.pid_of("A"), Some(first));
    assert_eq!(lab.argv(), "A\n");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_restart_while_aterm_answers_err_waits_and_is_made() {
    let mut lab = Lab::new("rs-err");
    let aterm = fake_aterm("rs-err");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || lab.journal().contains("SESSION")));
    aterm.set_policy_reply(DIALOG_ERR);
    aterm.send_restart("asked");
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("RESTART-WAITS cause=asked aterm did not answer")),
        "the restart did not wait for aterm; journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.pid_of("A"), Some(first));
    aterm.clear_policy_reply();
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the restart was not made once aterm answered; journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn an_asked_restart_is_made_under_relaunch_false() {
    let mut lab = Lab::new("rs-limited");
    let aterm = fake_aterm("rs-limited");
    aterm.set_policy(&POLICY_ON.replace("relaunch=true", "relaunch=false"));
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || lab.journal().contains("SESSION")));
    aterm.send_restart("asked");
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the restart aterm asked for was not made; journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_restart_of_a_child_deaf_to_sigterm_is_killed_and_resumed() {
    let mut lab = Lab::new("rs-deaf");
    let aterm = fake_aterm("rs-deaf");
    let env = aterm.env();
    // The child's trap must run, and say it ignored the SIGTERM, before the SIGKILL ends it: a
    // grace a loaded machine cannot outlast, though it is the whole cost of the test.
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &vars(
            &env,
            &[
                ("FAKE_IGNORE_TERM", "1"),
                ("KIMI_ALAB_TERM_GRACE_MS", "3000"),
            ],
        ),
    );
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || lab.journal().contains("SESSION")));
    aterm.send_restart("stall");
    assert!(
        wait_for(DEADLINE, || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the deaf child was not ended and resumed; journal:\n{}",
        lab.journal()
    );
    assert!(lab.journal().contains("RESTART-KILL cause=stall"));
    assert_eq!(
        lab.events("A")
            .lines()
            .filter(|l| *l == "term-ignored")
            .count(),
        1
    );
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nA [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(child.try_wait().unwrap().is_none());
    std::fs::write(lab.rec().join("honor-term"), "").unwrap();
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// Write the session lock `sess-1` for the child `pid`, as Kimi writes one when the person's
/// first prompt opens a session: a child started with `FAKE_SESSION=none` holds none until then.
fn open_session(lab: &Lab, pid: Pid) {
    let dir = lab.root.join("kimi-home/locks/sessions");
    std::fs::create_dir_all(&dir).unwrap();
    let pid = pid.as_raw_nonzero();
    let host = rustix::system::uname()
        .nodename()
        .to_string_lossy()
        .into_owned();
    let tmp = dir.join("sess-1.lock.tmp");
    std::fs::write(
        &tmp,
        format!(
            r#"{{"version":1,"token":"{pid}:x","pid":{pid},"hostname":"{host}","startedAt":1,"clientIdentity":"kimi_code_cli"}}"#
        ),
    )
    .unwrap();
    std::fs::rename(&tmp, dir.join("sess-1.lock")).unwrap();
}

/// A restart aterm asked for that cannot be made (the child holds no session lock yet) waits at
/// most the restart wait (`KIMI_ALAB_RESTART_WAIT_MS`, alab-wire's 45 s `AWAY_BUDGET` by
/// default) from its arrival and is then refused: the session Kimi opens later, with the
/// person's first prompt, is never ended for it. Before, a wait for the session never spent the
/// budget, so the restart was made the moment the lock appeared, however much later.
#[test]
fn a_restart_that_cannot_be_made_in_time_is_refused_and_never_made_later() {
    let mut lab = Lab::new("rs-stale");
    let aterm = fake_aterm("rs-stale");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(
            &env,
            &[
                ("FAKE_SESSION", "none"),
                ("KIMI_ALAB_RESTART_WAIT_MS", "1500"),
            ],
        ),
    );
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || claims(&aterm).len() == 2));
    aterm.send_restart("asked");
    assert!(
        wait_for(DEADLINE, || lab.journal().contains(
            "RESTART-WAITS cause=asked no running child's session lock"
        )),
        "the restart did not wait for the session; journal:\n{}",
        lab.journal()
    );
    sleep(Duration::from_millis(2500));
    open_session(&lab, first);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("SESSION the child holds session sess-1")),
        "the harness never read the late session lock; journal:\n{}",
        lab.journal()
    );
    // An absence: a restart that still waited is asked about again a second after the pass
    // that read the lock; that and a whole pass go by, and a `RESTART` line is looked for.
    lab.pass_after(
        &mut child,
        Duration::from_millis(alab_wire::link::LATER_RETRY_MS),
    );
    assert_eq!(lab.notes("RESTART"), 0, "journal:\n{}", lab.journal());
    assert_eq!(lab.pid_of("A"), Some(first));
    assert_eq!(lab.argv(), "A\n");
    assert!(
        lab.journal().contains(
            "RESTART-REFUSED cause=asked not made within 1500 ms: no running child's session lock"
        ),
        "journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// A restart aterm sends as the child is ending (for a restart under way) was meant for that
/// child: it is passed over, and never applied to the child started after it. The harness is
/// held still while the child ends and aterm's second request comes in, so both reach it in one
/// wake, as they can on a loaded machine.
#[test]
fn a_restart_sent_as_the_child_ends_is_not_applied_to_the_next_child() {
    let mut lab = Lab::new("rs-late");
    let aterm = fake_aterm("rs-late");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(
        &[],
        &vars(
            &env,
            &[
                ("FAKE_IGNORE_TERM", "1"),
                ("KIMI_ALAB_TERM_GRACE_MS", "8000"),
            ],
        ),
    );
    let first = lab.pid_of("A").unwrap();
    assert!(wait_for(DEADLINE, || lab.journal().contains("SESSION")));
    aterm.send_restart("asked");
    assert!(
        wait_for(DEADLINE, || lab.events("A").contains("term-ignored")),
        "A never took the restart's SIGTERM; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::STOP);
    // A ends by a SIGTERM, an end the restart accepts as its own.
    std::fs::write(lab.rec().join("honor-term"), "").unwrap();
    kill_process(first, Signal::TERM).unwrap();
    assert!(
        wait_for(DEADLINE, || lab.events("A").contains("term\n")),
        "A did not end; events:\n{}",
        lab.events("A")
    );
    aterm.send_restart("stall");
    sleep(Duration::from_millis(500));
    signal(&child, Signal::CONT);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("A").is_some_and(|pid| pid != first)),
        "the asked restart was not made; journal:\n{}",
        lab.journal()
    );
    assert!(
        wait_for(DEADLINE, || lab.journal().contains(
            "RESTART-PASSED-OVER cause=stall it came before this child was announced"
        )),
        "journal:\n{}",
        lab.journal()
    );
    let second = lab.pid_of("A").unwrap();
    // An absence: a restart applied to the new child would wait for its lock and be asked about
    // again a second later; that and a whole pass go by, and a second `RESTART` is looked for.
    lab.pass_after(
        &mut child,
        Duration::from_millis(alab_wire::link::LATER_RETRY_MS),
    );
    assert_eq!(lab.notes("RESTART"), 1, "journal:\n{}", lab.journal());
    assert_eq!(lab.pid_of("A"), Some(second));
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_halt_holds_a_move_a_person_attested_until_it_lifts() {
    let mut lab = Lab::new("halt");
    let aterm = fake_aterm("halt");
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    assert!(wait_for(DEADLINE, || claims(&aterm).len() == 2));
    // aterm halts the session: `hold=1`, and the policy reads `effective=off why=hold`.
    aterm.set_status("hold", Some("1"));
    lab.install_b();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("REFUSED wait aterm policy effective=off why=hold")),
        "the halt did not hold the move; journal:\n{}",
        lab.journal()
    );
    assert!(lab.pid_of("B").is_none());
    // The claim was never given back, and moves once the halt lifts.
    assert!(!aterm.verbs().iter().any(|verb| verb.contains("meta unset")));
    aterm.set_status("hold", Some("0"));
    assert!(
        wait_for(DEADLINE, || lab.pid_of("B").is_some()),
        "never moved after the halt lifted; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// A person's cap that narrows the standing contract-4 claim to `relaunch` hands the upgrade duty
/// back to aterm's engine: an update a person attests with `SIGUSR1` under that claim is refused
/// on aterm's `supervisor_scope`, however free the session is, and the claim is kept.
/// NEGATIVE CONTROL: a cap that narrows the claim to `upgrade` instead leaves the duty with the
/// harness, and the same attestation moves the vendor.
#[test]
fn a_move_a_person_attested_needs_the_upgrade_duty_granted() {
    let (journal, moved) = attest_under_cap("capped-move", Scope::Relaunch);
    assert!(
        journal.contains("REFUSED aterm supervisor_scope=relaunch;") && !moved,
        "a narrowed grant did not hold the move; journal:\n{journal}"
    );
    let (journal, moved) = attest_under_cap("capped-kept", Scope::Upgrade);
    assert!(
        moved && !journal.contains("REFUSED"),
        "a grant that kept upgrade did not move; journal:\n{journal}"
    );
}

/// Narrow the standing claim to `cap`, attest an update with `SIGUSR1`, and wait for the move or
/// its refusal: the journal, and whether the vendor moved. The claim is never given back.
fn attest_under_cap(name: &str, cap: Scope) -> (String, bool) {
    let mut lab = Lab::new(name);
    let aterm = fake_aterm(name);
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    assert!(wait_for(DEADLINE, || claims(&aterm).len() == 2));
    aterm.set_cap(Some(&[cap]));
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains(&format!("NARROWED scope={} ", cap.word()))),
        "journal:\n{}",
        lab.journal()
    );
    lab.install_b();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("REFUSED")
            || lab.pid_of("B").is_some()),
        "the attestation was neither refused nor moved; journal:\n{}",
        lab.journal()
    );
    let moved = lab.pid_of("B").is_some();
    assert!(!aterm.verbs().iter().any(|verb| verb.contains("meta unset")));
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
    (lab.journal(), moved)
}

/// Under contract 4 aterm may grant less than the claim asked for. A launch granted less than
/// both duties gives the claim back and Kimi runs in the harness's place.
#[test]
fn a_claim_granted_less_than_both_duties_is_released_and_kimi_runs_in_place() {
    let mut lab = Lab::new("scope");
    let aterm = fake_aterm("scope");
    aterm.set_known(&[Scope::Upgrade]);
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    assert_eq!(lab.pid_of("A"), Some(Pid::from_child(&child)));
    assert!(
        lab.journal()
            .contains("scope=upgrade (the claim needs relaunch,upgrade)"),
        "journal:\n{}",
        lab.journal()
    );
    assert!(
        aterm
            .verbs()
            .iter()
            .any(|verb| verb.starts_with("meta unset supervisor holder=kimi-alab@")),
        "the claim was not given back: {:?}",
        aterm.verbs()
    );
    assert_eq!(aterm.holder(), None);
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn an_err_from_aterm_at_launch_keeps_the_child_supervised_and_claims_it_later() {
    let mut lab = Lab::new("launch-err");
    let aterm = fake_aterm("launch-err");
    aterm.set_policy_reply(DIALOG_ERR);
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    assert_ne!(lab.pid_of("A"), Some(Pid::from_child(&child)));
    assert!(
        lab.journal().contains("UNCLAIMED policy reply=ERR:aterm"),
        "journal:\n{}",
        lab.journal()
    );
    aterm.clear_policy_reply();
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains(" contract=4 scope=relaunch,upgrade ")),
        "the claim was not made once aterm answered; journal:\n{}",
        lab.journal()
    );
    // Claimed on the clock, with the child that runs already named.
    let claims = claims(&aterm);
    let a = lab.pid_of("A").unwrap().as_raw_nonzero();
    assert_eq!(claims.len(), 1, "{claims:?}");
    assert!(claims[0].contains(&format!(" child={a}:")), "{claims:?}");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// A launch whose first claim aterm answers the handshake of and then answers late is aterm
/// busy, not away: the child runs supervised and is claimed again on the clock.
#[test]
fn a_launch_aterm_answers_late_is_busy_not_gone() {
    let mut lab = Lab::new("launch-busy");
    let aterm = fake_aterm("launch-busy");
    aterm.set_reply_delay(Duration::from_millis(900));
    let env = aterm.env();
    let mut child = lab.running_a_on_pty(&[], &vars(&env, &[]));
    assert_ne!(lab.pid_of("A"), Some(Pid::from_child(&child)));
    assert!(
        lab.journal().contains("(aterm answered the handshake)"),
        "journal:\n{}",
        lab.journal()
    );
    aterm.set_reply_delay(Duration::ZERO);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains(" contract=4 scope=relaunch,upgrade ")),
        "the claim was not made once aterm answered in time; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn never_moves_without_an_attestation() {
    let mut lab = Lab::new("no-attest");
    let mut child = lab.running_a_on_pty(&[], &[]);
    lab.install_b();
    // An absence: with no attestation, nothing moves. A move needs one quiet window, which
    // goes by with the harness running; then its decision, a `MOVE` line, is looked for.
    lab.pass_after(&mut child, Duration::from_millis(QUIET_MS));
    assert_eq!(
        lab.notes("MOVE"),
        0,
        "moved with no attestation; journal:\n{}",
        lab.journal()
    );
    assert!(lab.pid_of("B").is_none(), "moved with no attestation");
    assert!(
        !lab.events("A").contains("term"),
        "A was stopped with no attestation"
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// Attest, then require that the harness refuses with `why` and never touches A.
fn assert_refused(lab: &Lab, child: &mut Child, why: &str) {
    signal(child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("REFUSED")),
        "no refusal; journal:\n{}",
        lab.journal()
    );
    let j = lab.journal();
    assert!(
        j.contains(why),
        "refused, but not for {why:?}; journal:\n{j}"
    );
    // An absence: a refusal ends the attestation, and the harness never retries on its own.
    // A retry under the same attestation is refused again for the same reason, at once for a
    // busy child and after a quiet window otherwise; that window goes by with the harness
    // running, and the refusals are counted.
    lab.pass_after(child, Duration::from_millis(QUIET_MS));
    assert_eq!(
        lab.notes("REFUSED"),
        1,
        "retried under the refused attestation; journal:\n{}",
        lab.journal()
    );
    assert_eq!(lab.notes("MOVE"), 0, "journal:\n{}", lab.journal());
    assert!(
        lab.pid_of("B").is_none(),
        "moved a busy child; journal:\n{}",
        lab.journal()
    );
    assert!(!lab.events("A").contains("term"), "stopped a busy child");
}

#[test]
fn never_moves_while_the_child_draws() {
    let mut lab = Lab::new("drawing");
    let mut child = lab.running_a_on_pty(&[], &[("FAKE_DRAW", "1")]);
    lab.install_b();
    assert_refused(&lab, &mut child, "the screen was drawn");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn never_moves_while_a_person_types() {
    let mut lab = Lab::new("typing");
    // A keystroke must be read inside the quiet window after the attestation. A typist that
    // does not get the CPU for a whole window would let the move through, so the window here
    // is long; the refusal still comes at the first keystroke read after the attestation.
    let mut child =
        lab.running_a_on_pty(&[], &[("FAKE_READ", "1"), ("KIMI_ALAB_QUIET_MS", "10000")]);
    lab.install_b();
    // The person types from before the attestation until the harness refuses.
    let typing = Arc::new(AtomicBool::new(true));
    let typist = {
        let typing = Arc::clone(&typing);
        let mut keys = lab.keyboard();
        std::thread::spawn(move || {
            while typing.load(Ordering::SeqCst) {
                let _ = keys.write_all(b"x\n");
                sleep(Duration::from_millis(20));
            }
        })
    };
    signal(&child, Signal::USR1);
    let refused = wait_for(DEADLINE, || lab.journal().contains("REFUSED"));
    typing.store(false, Ordering::SeqCst);
    typist.join().unwrap();
    assert!(refused, "journal:\n{}", lab.journal());
    let j = lab.journal();
    assert!(j.contains("a keystroke was read"), "journal:\n{j}");
    assert!(lab.pid_of("B").is_none(), "moved while a person typed");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn never_moves_while_the_child_works() {
    let mut lab = Lab::new("working");
    let mut child = lab.running_a_on_pty(&[], &[("FAKE_BURN", "1")]);
    lab.install_b();
    assert_refused(&lab, &mut child, "of CPU");
    std::fs::write(lab.rec().join("stop-burn"), "").unwrap();
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn never_moves_while_background_work_runs() {
    let mut lab = Lab::new("background-work");
    std::fs::write(lab.rec().join("background-work"), "").unwrap();
    let mut child = lab.running_a_on_pty(&[], &[]);
    lab.install_b();
    assert_refused(&lab, &mut child, "background work runs under the child: sh");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn never_moves_a_child_that_holds_no_session() {
    let mut lab = Lab::new("no-session");
    let mut child = lab.running_a_on_pty(&[], &[("FAKE_SESSION", "none")]);
    lab.install_b();
    assert_refused(
        &lab,
        &mut child,
        "no current or most recently proven session lock",
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn never_moves_without_a_terminal() {
    let mut lab = Lab::new("headless");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[]);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("A").is_some()),
        "A never started"
    );
    lab.install_b();
    assert_refused(&lab, &mut child, "no terminal");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_one_shot_launch_is_never_moved() {
    let mut lab = Lab::new("one-shot");
    let mut child = lab.running_a_on_pty(&["-p", "hello"], &[]);
    lab.install_b();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("UPDATE-WAITS")
            && lab.notes("ATTEST") == 1),
        "journal:\n{}",
        lab.journal()
    );
    // An absence: a one-shot launch waits out the update. A move needs one quiet window, which
    // goes by with the harness running; then its decision, a `MOVE` line, is looked for.
    lab.pass_after(&mut child, Duration::from_millis(QUIET_MS));
    assert_eq!(
        lab.notes("MOVE"),
        0,
        "moved a one-shot launch; journal:\n{}",
        lab.journal()
    );
    assert!(lab.pid_of("B").is_none(), "moved a one-shot launch");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn no_update_mode_never_moves() {
    let mut lab = Lab::new("no-update");
    let mut child = lab.running_a_on_pty(&["--alab-no-update"], &[]);
    lab.lay(BUILD_B, "B", "2.3.2");
    lab.make_current(BUILD_B);
    // Absences: B is never noticed, and nothing moves. The store would be looked at on every
    // pass, so a whole pass goes by before it is attested idle; then a move would need one
    // quiet window, which goes by with the harness running, before its `MOVE` is looked for.
    lab.pass(&mut child);
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.notes("ATTEST") == 1),
        "the attestation was not taken; journal:\n{}",
        lab.journal()
    );
    lab.pass_after(&mut child, Duration::from_millis(QUIET_MS));
    assert_eq!(lab.notes("MOVE"), 0, "journal:\n{}", lab.journal());
    assert!(lab.pid_of("B").is_none(), "moved under --alab-no-update");
    assert!(
        !lab.journal().contains("UPDATE "),
        "watched the store under --alab-no-update"
    );
    assert_eq!(lab.argv(), "A\n", "--alab-no-update reached the vendor");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn refuses_to_run_a_store_binary_that_is_the_harness_itself() {
    let mut lab = Lab::near_the_harness("self");
    lab.lay_harness(BUILD_A, "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&["--yolo"], &[]);
    let st = wait_exit(&mut child, DEADLINE);
    let err = stderr_of(&mut child);
    assert_eq!(st.code(), Some(126), "stderr: {err}");
    assert!(err.contains("is this harness itself"), "{err}");
    assert_eq!(err.lines().count(), 1, "the harness ran again: {err}");
}

#[test]
fn a_store_kimi_that_execs_the_harness_is_refused_at_once() {
    let mut lab = Lab::new("chain-exec");
    lab.lay_script(BUILD_A, &CHAIN.replace("@RUN@", "exec"), "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[("HARNESS_BIN", HARNESS), ("CHAIN_CAP", "4")]);
    let st = wait_exit(&mut child, DEADLINE);
    let err = stderr_of(&mut child);
    assert_eq!(
        lab.chain(),
        ["0"],
        "the harness ran itself as its own vendor; stderr: {err}"
    );
    assert_eq!(st.code(), Some(126), "stderr: {err}");
    assert!(err.contains("leads back to kimi-alab"), "{err}");
}

#[test]
fn a_store_kimi_that_runs_the_harness_as_a_child_stops_at_the_depth_limit() {
    let mut lab = Lab::new("chain-child");
    lab.lay_script(BUILD_A, &CHAIN.replace("@RUN@", ""), "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[("HARNESS_BIN", HARNESS), ("CHAIN_CAP", "20")]);
    let st = wait_exit(&mut child, 2 * DEADLINE);
    let err = stderr_of(&mut child);
    let depths: Vec<String> = (0..8).map(|n| n.to_string()).collect();
    assert_eq!(
        lab.chain(),
        depths,
        "the chain did not stop at kimi-alab's depth limit; stderr: {err}"
    );
    assert_eq!(st.code(), Some(126), "stderr: {err}");
    assert!(err.contains("KIMI_ALAB_DEPTH=8"), "{err}");
}

#[test]
fn an_update_to_the_harness_itself_is_never_taken() {
    let mut lab = Lab::near_the_harness("update-self");
    let mut child = lab.running_a_on_pty(&[], &[]);
    lab.lay_harness(BUILD_B, "2.3.2");
    lab.make_current(BUILD_B);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("UPDATE-REFUSED")),
        "journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    // An absence: nothing moves onto the harness. The attestation is taken, and a move would
    // need one quiet window, which goes by with the harness running, before it is looked for.
    assert!(
        wait_for(DEADLINE, || lab.notes("ATTEST") == 1),
        "the attestation was not taken; journal:\n{}",
        lab.journal()
    );
    lab.pass_after(&mut child, Duration::from_millis(QUIET_MS));
    let j = lab.journal();
    assert!(!j.contains("MOVE"), "moved onto the harness itself:\n{j}");
    assert!(!lab.events("A").contains("term"), "A was stopped:\n{j}");
    assert_eq!(lab.argv(), "A\n");
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_relaunch_skips_a_current_build_that_is_the_harness_itself() {
    let mut lab = Lab::near_the_harness("relaunch-self");
    let mut child = lab.running_a_on_pty(&["--skills-dir", "/sk"], &[]);
    lab.install_b();
    // After the move to B is decided on, atpkg flips `current` to a build that IS the harness.
    lab.lay_harness(BUILD_C, "2.3.3");
    lab.make_current(BUILD_C);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("UPDATE-REFUSED")),
        "journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("B").is_some()),
        "never resumed on B; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nB [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    let j = lab.journal();
    assert!(j.contains("RELAUNCH-SKIPPED"), "journal:\n{j}");
    assert!(
        child.try_wait().unwrap().is_none(),
        "the harness exited during the move:\n{j}"
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_broken_stderr_never_ends_the_harness_during_a_move() {
    let mut lab = Lab::new("broken-stderr");
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = lab.running_a_on_pty_with(&[], &[], Some(Stdio::from(writer)));
    lab.install_b();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("B").is_some()),
        "never moved to B (status {:?}); journal:\n{}",
        child.try_wait().unwrap(),
        lab.journal()
    );
    // The harness says it resumes the session on its stderr before it starts B, and journals
    // `RELAUNCHED` after; once that is journalled and it has gone round its loop again, the
    // write to the broken stderr is behind it.
    assert!(
        wait_for(DEADLINE, || lab.journal().contains(" RELAUNCHED ")
            || child.try_wait().unwrap().is_some()),
        "the move was never journalled; journal:\n{}",
        lab.journal()
    );
    assert!(
        child.try_wait().unwrap().is_none(),
        "the harness died writing to a broken stderr; journal:\n{}",
        lab.journal()
    );
    lab.pass(&mut child);
    assert!(
        child.try_wait().unwrap().is_none(),
        "the harness died writing to a broken stderr; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_child_that_ignores_sigterm_abandons_the_move_and_runs_on() {
    let mut lab = Lab::new("term-grace");
    let term_grace = TERM_GRACE_MS.to_string();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &[
            ("FAKE_IGNORE_TERM", "1"),
            ("KIMI_ALAB_TERM_GRACE_MS", &term_grace),
        ],
    );
    lab.install_b();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("MOVE-ABANDONED")),
        "never abandoned the move; journal:\n{}",
        lab.journal()
    );
    let ignored = || {
        lab.events("A")
            .lines()
            .filter(|l| *l == "term-ignored")
            .count()
    };
    // A's trap can run after the harness has given up on it.
    assert!(
        wait_for(DEADLINE, || ignored() >= 1),
        "A never took its SIGTERM; events:\n{}",
        lab.events("A")
    );
    assert_eq!(ignored(), 1, "events:\n{}", lab.events("A"));
    assert!(lab.pid_of("B").is_none());
    assert!(child.try_wait().unwrap().is_none());
    // A second attestation while B is still current never signals A for B again. An absence:
    // the attestation is taken, and a second try would be decided one quiet window later,
    // which goes by with the harness running; then that decision, a `MOVE` line, is counted.
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.notes("ATTEST") == 2),
        "the second attestation was not taken; journal:\n{}",
        lab.journal()
    );
    lab.pass_after(&mut child, Duration::from_millis(QUIET_MS));
    assert_eq!(
        lab.notes("MOVE"),
        1,
        "moved again onto abandoned build B; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.events("A")
            .lines()
            .filter(|l| *l == "term-ignored")
            .count(),
        1,
        "signalled again for abandoned build B; journal:\n{}",
        lab.journal()
    );
    // A newer build C re-arms the update and moves cleanly once A honors SIGTERM.
    std::fs::write(lab.rec().join("honor-term"), "").unwrap();
    lab.lay(BUILD_C, "C", "2.3.3");
    lab.make_current(BUILD_C);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("2.3.1 -> 2.3.3 is current")),
        "never saw C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("C").is_some()),
        "never moved to C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

#[test]
fn a_move_onto_a_build_that_refuses_to_start_falls_back_and_abandons_it() {
    let mut lab = Lab::new("refused-start");
    let mut child = lab.running_a_on_pty(&["--skills-dir", "/sk"], &[]);
    let first_a = lab.pid_of("A").unwrap();
    let broken_b = "#!/bin/sh\n{ printf 'B'; for a in \"$@\"; do printf ' [%s]' \"$a\"; done; printf '\\n'; } >> \"$FAKE_REC/argv\"\nexit 1\n";
    lab.lay_script(BUILD_B, broken_b, "2.3.2");
    lab.make_current(BUILD_B);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("UPDATE ")),
        "never saw B; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.journal().contains("MOVE-FAILED")),
        "never noted MOVE-FAILED; journal:\n{}",
        lab.journal()
    );
    assert!(
        wait_for(DEADLINE, || lab.pid_of("A") != Some(first_a)
            && lab.pid_of("A").is_some()),
        "never fell back onto a resumed A; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nB [--skills-dir] [/sk] [--session] [sess-1]\nA [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(child.try_wait().unwrap().is_none());
    // A second attestation while B is still current does not try B again. An absence: the
    // attestation is taken, and a second try would be decided one quiet window later, which
    // goes by with the harness running; then that decision, a `MOVE` line, is counted.
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.notes("ATTEST") == 2),
        "the second attestation was not taken; journal:\n{}",
        lab.journal()
    );
    lab.pass_after(&mut child, Duration::from_millis(QUIET_MS));
    assert_eq!(
        lab.notes("MOVE"),
        1,
        "tried abandoned build B again; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.argv().lines().count(),
        3,
        "tried abandoned build B again; argv:\n{}\njournal:\n{}",
        lab.argv(),
        lab.journal()
    );
    // A newer build C re-arms the update and moves cleanly.
    lab.lay(BUILD_C, "C", "2.3.3");
    lab.make_current(BUILD_C);
    assert!(
        wait_for(DEADLINE, || lab
            .journal()
            .contains("2.3.1 -> 2.3.3 is current")),
        "never saw C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(DEADLINE, || lab.pid_of("C").is_some()),
        "never moved to C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(wait_exit(&mut child, DEADLINE).code(), Some(143));
}

/// The measurement the safe point stands on: on this OS, a pty's slave times move as
/// `src/tty.rs` says (output moves mtime only; input moves atime only once it is read).
#[test]
fn tty_times_move_as_the_safe_point_assumes() {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    if !cfg!(target_os = "macos") {
        return; // Linux keeps tty times to 8 s buckets; the safe point allows for it.
    }
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let path = Path::new(name.to_str().unwrap()).to_path_buf();
    let mut slave = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(i32::try_from(rustix::fs::OFlags::NOCTTY.bits()).unwrap())
        .open(&path)
        .unwrap();
    let mut master = File::from(master);
    let times = || {
        let m = std::fs::metadata(&path).unwrap();
        (m.accessed().unwrap(), m.modified().unwrap())
    };
    let (a0, m0) = times();
    sleep(Duration::from_millis(200));
    slave.write_all(b"out\n").unwrap();
    let mut buf = [0u8; 64];
    let _ = master.read(&mut buf).unwrap();
    let (a1, m1) = times();
    assert!(
        m1 > m0 && a1 == a0,
        "a slave write must move mtime only: {a0:?} {m0:?} -> {a1:?} {m1:?}"
    );
    sleep(Duration::from_millis(200));
    master.write_all(b"k\n").unwrap();
    sleep(Duration::from_millis(100));
    let (a2, m2) = times();
    assert!(
        a2 == a1 && m2 == m1,
        "queued input must move neither time before it is read: {a1:?} {m1:?} -> {a2:?} {m2:?}"
    );
    let _ = slave.read(&mut buf).unwrap();
    let (a3, m3) = times();
    assert!(
        a3 > a2 && m3 == m2,
        "a slave read must move atime only: {a2:?} {m2:?} -> {a3:?} {m3:?}"
    );
}
