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
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process, kill_process_group};

const HARNESS: &str = env!("CARGO_BIN_EXE_kimi-alab");
const BUILD_A: u64 = 1_000_002_000_003_000_001; // 2.3.1
const BUILD_B: u64 = 1_000_002_000_003_000_002; // 2.3.2
const BUILD_C: u64 = 1_000_002_000_003_000_003; // 2.3.3

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
const FAKE: &str = r#"#!/bin/sh
label='@LABEL@'
rec="$FAKE_REC"
ev="$rec/$label.events"
{ printf '%s' "$label"; for a in "$@"; do printf ' [%s]' "$a"; done; printf '\n'; } >> "$rec/argv"
echo "auto_update=${KIMI_CODE_NO_AUTO_UPDATE-unset} shim=${FAKE_SHIM-unset}" > "$rec/$label.env"
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
sleep 3600 & spid=$!
echo "$$" > "$rec/$label.pid.tmp"; mv "$rec/$label.pid.tmp" "$rec/$label.pid"
if [ -e "$rec/background-work" ]; then
  sh -c 'while [ ! -e "$1" ]; do sleep 1; done' sh "$rec/stop-background" & workpid=$!
fi
if [ "${FAKE_MODE:-session}" = crash ] && [ ! -e "$rec/crashed" ]; then
  : > "$rec/crashed"
  if [ -e "$rec/unlock-before-crash" ]; then rm -f "$lock"; lock=; fi
  sleep 1
  kill -"${FAKE_SIG:-SEGV}" $$
fi
if [ "${FAKE_MODE:-session}" = crashloop ]; then
  sleep 1
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

struct FakeAterm {
    runtime: PathBuf,
    claims: Arc<Mutex<Vec<String>>>,
    sock: PathBuf,
    token: PathBuf,
    restart_tx: std::sync::mpsc::Sender<String>,
}

impl FakeAterm {
    fn restart(&self) {
        self.restart_tx
            .send("restart cause=asked".to_string())
            .unwrap();
    }

    fn restart_account(&self, home: &Path) {
        let home = home
            .to_string_lossy()
            .replace('%', "%25")
            .replace('/', "%2F")
            .replace(' ', "%20");
        self.restart_tx
            .send(format!("restart cause=account to=work home={home}"))
            .unwrap();
    }
}

impl Drop for FakeAterm {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.sock);
        let _ = std::fs::remove_file(&self.token);
    }
}

#[allow(clippy::too_many_lines)]
fn fake_aterm(root: &Path, sid: &str) -> FakeAterm {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    let runtime = root.join("aterm");
    std::fs::create_dir_all(runtime.join("graph")).unwrap();
    let sock = std::env::temp_dir().join(format!(
        "ka-{}-{}.sock",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let token = sock.with_file_name(format!(
        "{}.token",
        sock.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&token, "test-token\n").unwrap();
    std::fs::write(
        runtime.join("graph").join(sid),
        format!("sock {}\n", sock.display()),
    )
    .unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let claims = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&claims);
    let sid = sid.to_string();
    let (restart_tx, restart_rx) = std::sync::mpsc::channel();
    let restart_rx = std::sync::Arc::new(Mutex::new(Some(restart_rx)));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                break;
            };
            let recorded = Arc::clone(&recorded);
            let sid = sid.clone();
            let restart_rx = Arc::clone(&restart_rx);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = stream;
                let mut supervisor = String::from("kimi-alab@0");
                let mut lines = String::new();
                loop {
                    lines.clear();
                    let Ok(read) = reader.read_line(&mut lines) else {
                        break;
                    };
                    if read == 0 {
                        break;
                    }
                    let line = lines.trim_end();
                    if line.starts_with("AUTH ") {
                        assert_eq!(line, "AUTH test-token");
                        continue;
                    }
                    if line == format!("subscribe @{sid} events") {
                        if writeln!(writer, "OK subscribe 1\nsub @{sid} events").is_err() {
                            break;
                        }
                        if let Some(rx) = restart_rx.lock().unwrap().take() {
                            let Ok(event) = rx.recv() else {
                                break;
                            };
                            if writeln!(writer, "EVENT {sid} {event}").is_err() {
                                break;
                            }
                        }
                        continue;
                    }
                    let reply = if line == "version" {
                        "OK fake-aterm".to_string()
                    } else if line == format!("@{sid} harness policy") {
                        "OK gen=1 effective=on why=enabled contract=2,3 relaunch=true upgrade=true upgrade_ask=false human_grace_s=0".to_string()
                    } else if let Some(claim) =
                        line.strip_prefix(&format!("@{sid} meta set supervisor "))
                    {
                        supervisor = claim
                            .split(" contract=3 ")
                            .next()
                            .unwrap_or("kimi-alab@0")
                            .to_string();
                        recorded.lock().unwrap().push(claim.to_string());
                        assert!(claim.contains(" scope=relaunch,upgrade "), "{claim}");
                        "OK".to_string()
                    } else if line == format!("@{sid} status") {
                        format!(
                            "OK schema=1 supervisor={supervisor} supervisor_proven=1 agent=idle composer=empty human_ms=- hand=- hold=0 input=clear need=-"
                        )
                    } else {
                        "ERR unexpected verb".to_string()
                    };
                    if writeln!(writer, "{reply}").is_err() {
                        break;
                    }
                }
            });
        }
    });
    FakeAterm {
        runtime,
        claims,
        sock,
        token,
        restart_tx,
    }
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
            .env("KIMI_ALAB_POLL_MS", "50")
            .env("KIMI_ALAB_QUIET_MS", "400")
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

    fn type_keys(&mut self, keys: &[u8]) {
        if let Some(p) = self.pty.as_mut() {
            let _ = p.writer.write_all(keys);
        }
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
            wait_for(Duration::from_secs(10), || self.pid_of("A").is_some()),
            "A never started"
        );
        child
    }

    /// Install B and make it current, then wait until the harness has seen it.
    fn install_b(&self) {
        self.lay(BUILD_B, "B", "2.3.2");
        self.make_current(BUILD_B);
        assert!(
            wait_for(Duration::from_secs(10), || self
                .journal()
                .contains("UPDATE ")),
            "the harness never saw the update; journal:\n{}",
            self.journal()
        );
    }
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
    let st = wait_exit(&mut child, Duration::from_secs(10));
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
    let st = wait_exit(&mut child, Duration::from_secs(10));
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
    let st = wait_exit(&mut child, Duration::from_secs(10));
    assert!(st.success(), "stderr: {}", stderr_of(&mut child));
    assert_eq!(lab.argv(), "A [--version] [a b] [] [--alab-which]\n");
}

#[test]
fn dies_by_the_signal_that_killed_the_child() {
    let mut lab = Lab::new("signal-death");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[("FAKE_MODE", "selfkill"), ("FAKE_SIG", "SEGV")]);
    let st = wait_exit(&mut child, Duration::from_secs(10));
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
        wait_for(Duration::from_secs(10), || lab.pid_of("A").is_some()),
        "A never started"
    );
    signal(&child, Signal::INT);
    signal(&child, Signal::QUIT);
    sleep(Duration::from_millis(400));
    assert!(
        child.try_wait().unwrap().is_none(),
        "SIGINT/SIGQUIT to the harness ended it"
    );
    assert!(
        !lab.events("A").contains("int"),
        "the harness forwarded SIGINT, which belongs to the child"
    );
    signal(&child, Signal::TERM);
    let st = wait_exit(&mut child, Duration::from_secs(10));
    assert_eq!(st.code(), Some(143), "the child's own status after SIGTERM");
    assert!(
        lab.events("A").contains("term"),
        "SIGTERM never reached the child"
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
        wait_for(Duration::from_secs(10), || lab.pid_of("A").is_some()),
        "A never started"
    );
    signal(&child, Signal::HUP);
    let st = wait_exit(&mut child, Duration::from_secs(10));
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
        wait_for(Duration::from_secs(10), || state(harness).starts_with('T')),
        "the harness did not stop with its child (state {:?}); journal:\n{}",
        state(harness),
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    // As a shell's `fg` does: continue the whole job.
    kill_process_group(harness, Signal::CONT).unwrap();
    let st = wait_exit(&mut child, Duration::from_secs(10));
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
        wait_for(Duration::from_secs(10), || lab.pid_of("B").is_some()),
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
        wait_for(Duration::from_secs(5), || lab
            .screen()
            .contains("resuming session sess-1")),
        "screen: {:?}",
        lab.screen()
    );
    let j = lab.journal();
    for event in ["START", "UPDATE", "ATTEST", "MOVE", "RELAUNCHED"] {
        assert!(j.contains(event), "no {event} in the journal:\n{j}");
    }
    assert_eq!(
        std::fs::read_to_string(lab.rec().join("B.env"))
            .unwrap()
            .trim(),
        "auto_update=1 shim=on"
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn aterm_attests_and_reclaims_an_automatic_move() {
    let mut lab = Lab::new("aterm-move");
    let aterm = fake_aterm(&lab.root, "s-beef");
    let runtime = &aterm.runtime;
    let claims = &aterm.claims;
    assert!(runtime.join("graph/s-beef").is_file());
    let xdg = runtime.parent().unwrap().to_string_lossy().into_owned();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &[
            ("TERM_PROGRAM", "aterm"),
            ("ATERM_PARENT_SESSION_ID", "s-beef"),
            ("XDG_RUNTIME_DIR", &xdg),
        ],
    );
    assert!(
        wait_for(Duration::from_secs(10), || claims.lock().unwrap().len()
            == 1),
        "aterm never proved the initial claim; journal:\n{}",
        lab.journal()
    );
    lab.install_b();
    assert!(
        wait_for(Duration::from_secs(10), || lab.pid_of("B").is_some()),
        "aterm's attestation never moved B; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nB [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(
        wait_for(Duration::from_secs(5), || claims.lock().unwrap().len() == 2),
        "the new vendor was not announced: {:?}",
        claims.lock().unwrap()
    );
    let claims = claims.lock().unwrap();
    assert!(
        claims[0].contains(format!("version={} vendor=2.3.1", kimi_alab::cli::VERSION).as_str()),
        "{claims:?}"
    );
    assert!(
        claims[1].contains(format!("version={} vendor=2.3.2", kimi_alab::cli::VERSION).as_str()),
        "{claims:?}"
    );
    drop(claims);
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn aterm_relaunches_a_crashed_child_on_the_same_session() {
    let mut lab = Lab::new("cr");
    let aterm = fake_aterm(&lab.root, "s-beef");
    let runtime = &aterm.runtime;
    let claims = &aterm.claims;
    let xdg = runtime.parent().unwrap().to_string_lossy().into_owned();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &[
            ("TERM_PROGRAM", "aterm"),
            ("ATERM_PARENT_SESSION_ID", "s-beef"),
            ("XDG_RUNTIME_DIR", &xdg),
            ("FAKE_MODE", "crash"),
            ("FAKE_SIG", "KILL"),
        ],
    );
    let first = lab.pid_of("A").unwrap();
    assert!(
        wait_for(Duration::from_secs(10), || {
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
    assert_eq!(claims.lock().unwrap().len(), 2);
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn aterm_relaunches_after_the_session_lock_disappears_before_a_crash() {
    let mut lab = Lab::new("cr-unlock");
    let aterm = fake_aterm(&lab.root, "s-beef");
    let runtime = &aterm.runtime;
    let xdg = runtime.parent().unwrap().to_string_lossy().into_owned();
    let mut child = lab.running_a_on_pty(
        &[],
        &[
            ("TERM_PROGRAM", "aterm"),
            ("ATERM_PARENT_SESSION_ID", "s-beef"),
            ("XDG_RUNTIME_DIR", &xdg),
            ("FAKE_MODE", "crash"),
            ("FAKE_SIG", "KILL"),
        ],
    );
    let first = lab.pid_of("A").unwrap();
    std::fs::write(lab.rec().join("unlock-before-crash"), "").unwrap();
    assert!(
        wait_for(Duration::from_secs(10), || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the crash was not relaunched after the lock disappeared; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
    assert!(lab.journal().contains("CRASH-RELAUNCH"));
    assert_eq!(aterm.claims.lock().unwrap().len(), 2);
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn aterm_restart_event_restarts_the_same_session() {
    let mut lab = Lab::new("rs");
    let aterm = fake_aterm(&lab.root, "s-beef");
    let runtime = &aterm.runtime;
    let xdg = runtime.parent().unwrap().to_string_lossy().into_owned();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &[
            ("TERM_PROGRAM", "aterm"),
            ("ATERM_PARENT_SESSION_ID", "s-beef"),
            ("XDG_RUNTIME_DIR", &xdg),
        ],
    );
    let first = lab.pid_of("A").unwrap();
    aterm.restart();
    assert!(
        wait_for(Duration::from_secs(10), || {
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
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn aterm_account_event_moves_home_and_drops_credential_overrides() {
    let mut lab = Lab::new("account");
    let aterm = fake_aterm(&lab.root, "s-beef");
    let runtime = &aterm.runtime;
    let xdg = runtime.parent().unwrap().to_string_lossy().into_owned();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &[
            ("TERM_PROGRAM", "aterm"),
            ("ATERM_PARENT_SESSION_ID", "s-beef"),
            ("XDG_RUNTIME_DIR", &xdg),
            ("KIMI_MODEL_NAME", "other-account"),
            ("KIMI_MODEL_API_KEY", "test-only-key"),
            ("KIMI_CODE_CUSTOM_HEADERS", "Authorization: test-only"),
        ],
    );
    let first = lab.pid_of("A").unwrap();
    let target = lab.root.join("account home");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(lab.root.join("kimi-home/sessions")).unwrap();
    symlink(lab.root.join("kimi-home/locks"), target.join("locks")).unwrap();
    symlink(lab.root.join("kimi-home/sessions"), target.join("sessions")).unwrap();
    aterm.restart_account(&target);
    assert!(
        wait_for(Duration::from_secs(10), || {
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
    assert!(
        aterm
            .claims
            .lock()
            .unwrap()
            .last()
            .is_some_and(|claim| claim.ends_with("login=work"))
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn harness_reexec_keeps_the_child_pid_and_managed_account() {
    let mut lab = Lab::new("self-exec");
    lab.lay_own_image(1);
    lab.make_own_current(1);
    let aterm = fake_aterm(&lab.root, "s-beef");
    let xdg = aterm
        .runtime
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &[
            ("TERM_PROGRAM", "aterm"),
            ("ATERM_PARENT_SESSION_ID", "s-beef"),
            ("XDG_RUNTIME_DIR", &xdg),
        ],
    );
    let first = lab.pid_of("A").unwrap();
    let target = lab.root.join("account home");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(lab.root.join("kimi-home/sessions")).unwrap();
    symlink(lab.root.join("kimi-home/locks"), target.join("locks")).unwrap();
    symlink(lab.root.join("kimi-home/sessions"), target.join("sessions")).unwrap();
    aterm.restart_account(&target);
    assert!(
        wait_for(Duration::from_secs(10), || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
                && aterm.claims.lock().unwrap().len() == 2
        }),
        "the account move did not finish; journal:\n{}",
        lab.journal()
    );
    let preserved = lab.pid_of("A").unwrap();

    lab.lay_own_image(2);
    lab.make_own_current(2);
    assert!(
        wait_for(Duration::from_secs(10), || {
            lab.journal().contains(" ADOPT ") && aterm.claims.lock().unwrap().len() == 3
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
    assert!(
        aterm
            .claims
            .lock()
            .unwrap()
            .last()
            .is_some_and(|claim| claim.ends_with("login=work"))
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
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
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("SELF-UPDATE ")),
        "the harness did not see its update; journal:\n{}",
        lab.journal()
    );
    std::fs::remove_file(replacement).unwrap();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("REEXEC-FAILED")),
        "the failed exec was not journaled; journal:\n{}",
        lab.journal()
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(lab.pid_of("A"), Some(vendor));
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn a_second_crash_inside_startup_grace_does_not_loop() {
    let mut lab = Lab::new("cl");
    let aterm = fake_aterm(&lab.root, "s-beef");
    let runtime = &aterm.runtime;
    let xdg = runtime.parent().unwrap().to_string_lossy().into_owned();
    let mut child = lab.running_a_on_pty(
        &[],
        &[
            ("TERM_PROGRAM", "aterm"),
            ("ATERM_PARENT_SESSION_ID", "s-beef"),
            ("XDG_RUNTIME_DIR", &xdg),
            ("FAKE_MODE", "crashloop"),
        ],
    );
    let first = lab.pid_of("A").unwrap();
    assert!(
        wait_for(Duration::from_secs(10), || {
            lab.pid_of("A").is_some_and(|pid| pid != first)
        }),
        "the first crash was not relaunched; journal:\n{}",
        lab.journal()
    );
    let status = wait_exit(&mut child, Duration::from_secs(10));
    assert_eq!(status.signal(), Some(11), "status: {status:?}");
    assert_eq!(lab.argv(), "A\nA [--session] [sess-1]\n");
}

#[test]
fn never_moves_without_an_attestation() {
    let mut lab = Lab::new("no-attest");
    let mut child = lab.running_a_on_pty(&[], &[]);
    lab.install_b();
    sleep(Duration::from_millis(1500));
    assert!(lab.pid_of("B").is_none(), "moved with no attestation");
    assert!(
        !lab.events("A").contains("term"),
        "A was stopped with no attestation"
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

/// Attest, then require that the harness refuses with `why` and never touches A.
fn assert_refused(lab: &Lab, child: &Child, why: &str) {
    signal(child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("REFUSED")),
        "no refusal; journal:\n{}",
        lab.journal()
    );
    let j = lab.journal();
    assert!(
        j.contains(why),
        "refused, but not for {why:?}; journal:\n{j}"
    );
    sleep(Duration::from_millis(600));
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
    assert_refused(&lab, &child, "the screen was drawn");
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn never_moves_while_a_person_types() {
    let mut lab = Lab::new("typing");
    let mut child = lab.running_a_on_pty(&[], &[("FAKE_READ", "1")]);
    lab.install_b();
    signal(&child, Signal::USR1);
    for _ in 0..20 {
        lab.type_keys(b"x\n");
        sleep(Duration::from_millis(50));
    }
    assert!(
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("REFUSED")),
        "journal:\n{}",
        lab.journal()
    );
    let j = lab.journal();
    assert!(j.contains("a keystroke was read"), "journal:\n{j}");
    assert!(lab.pid_of("B").is_none(), "moved while a person typed");
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn never_moves_while_the_child_works() {
    let mut lab = Lab::new("working");
    let mut child = lab.running_a_on_pty(&[], &[("FAKE_BURN", "1")]);
    lab.install_b();
    assert_refused(&lab, &child, "of CPU");
    std::fs::write(lab.rec().join("stop-burn"), "").unwrap();
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn never_moves_while_background_work_runs() {
    let mut lab = Lab::new("background-work");
    std::fs::write(lab.rec().join("background-work"), "").unwrap();
    let mut child = lab.running_a_on_pty(&[], &[]);
    lab.install_b();
    assert_refused(&lab, &child, "background work runs under the child: sh");
    std::fs::write(lab.rec().join("stop-background"), "").unwrap();
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn never_moves_a_child_that_holds_no_session() {
    let mut lab = Lab::new("no-session");
    let mut child = lab.running_a_on_pty(&[], &[("FAKE_SESSION", "none")]);
    lab.install_b();
    assert_refused(
        &lab,
        &child,
        "no current or most recently proven session lock",
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn never_moves_without_a_terminal() {
    let mut lab = Lab::new("headless");
    lab.lay(BUILD_A, "A", "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&[], &[]);
    assert!(
        wait_for(Duration::from_secs(10), || lab.pid_of("A").is_some()),
        "A never started"
    );
    lab.install_b();
    assert_refused(&lab, &child, "no terminal");
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn a_one_shot_launch_is_never_moved() {
    let mut lab = Lab::new("one-shot");
    let mut child = lab.running_a_on_pty(&["-p", "hello"], &[]);
    lab.install_b();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("UPDATE-WAITS")),
        "journal:\n{}",
        lab.journal()
    );
    sleep(Duration::from_millis(800));
    assert!(lab.pid_of("B").is_none(), "moved a one-shot launch");
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn no_update_mode_never_moves() {
    let mut lab = Lab::new("no-update");
    let mut child = lab.running_a_on_pty(&["--alab-no-update"], &[]);
    lab.lay(BUILD_B, "B", "2.3.2");
    lab.make_current(BUILD_B);
    sleep(Duration::from_millis(300));
    signal(&child, Signal::USR1);
    sleep(Duration::from_millis(1200));
    assert!(lab.pid_of("B").is_none(), "moved under --alab-no-update");
    assert!(
        !lab.journal().contains("UPDATE "),
        "watched the store under --alab-no-update"
    );
    assert_eq!(lab.argv(), "A\n", "--alab-no-update reached the vendor");
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn refuses_to_run_a_store_binary_that_is_the_harness_itself() {
    let mut lab = Lab::near_the_harness("self");
    lab.lay_harness(BUILD_A, "2.3.1");
    lab.make_current(BUILD_A);
    let mut child = lab.spawn(&["--yolo"], &[]);
    let st = wait_exit(&mut child, Duration::from_secs(10));
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
    let st = wait_exit(&mut child, Duration::from_secs(20));
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
    let st = wait_exit(&mut child, Duration::from_secs(60));
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
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("UPDATE-REFUSED")),
        "journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    sleep(Duration::from_millis(1200));
    let j = lab.journal();
    assert!(!j.contains("MOVE"), "moved onto the harness itself:\n{j}");
    assert!(!lab.events("A").contains("term"), "A was stopped:\n{j}");
    assert_eq!(lab.argv(), "A\n");
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
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
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("UPDATE-REFUSED")),
        "journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab.pid_of("B").is_some()),
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
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
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
        wait_for(Duration::from_secs(10), || lab.pid_of("B").is_some()),
        "never moved to B (status {:?}); journal:\n{}",
        child.try_wait().unwrap(),
        lab.journal()
    );
    sleep(Duration::from_millis(300));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the harness died writing to a broken stderr; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
}

#[test]
fn a_child_that_ignores_sigterm_abandons_the_move_and_runs_on() {
    let mut lab = Lab::new("term-grace");
    let mut child = lab.running_a_on_pty(
        &["--skills-dir", "/sk"],
        &[
            ("FAKE_IGNORE_TERM", "1"),
            ("KIMI_ALAB_TERM_GRACE_MS", "300"),
        ],
    );
    lab.install_b();
    signal(&child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("MOVE-ABANDONED")),
        "never abandoned the move; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.events("A")
            .lines()
            .filter(|l| *l == "term-ignored")
            .count(),
        1,
        "events:\n{}",
        lab.events("A")
    );
    assert!(lab.pid_of("B").is_none());
    assert!(child.try_wait().unwrap().is_none());
    // A second attestation while B is still current never signals A for B again.
    signal(&child, Signal::USR1);
    sleep(Duration::from_millis(700));
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
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("2.3.1 -> 2.3.3 is current")),
        "never saw C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab.pid_of("C").is_some()),
        "never moved to C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
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
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("UPDATE ")),
        "never saw B; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("MOVE-FAILED")),
        "never noted MOVE-FAILED; journal:\n{}",
        lab.journal()
    );
    assert!(
        wait_for(Duration::from_secs(10), || lab.pid_of("A") != Some(first_a)
            && lab.pid_of("A").is_some()),
        "never fell back onto a resumed A; journal:\n{}",
        lab.journal()
    );
    assert_eq!(
        lab.argv(),
        "A [--skills-dir] [/sk]\nB [--skills-dir] [/sk] [--session] [sess-1]\nA [--skills-dir] [/sk] [--session] [sess-1]\n"
    );
    assert!(child.try_wait().unwrap().is_none());
    // A second attestation while B is still current does not try B again.
    signal(&child, Signal::USR1);
    sleep(Duration::from_millis(700));
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
        wait_for(Duration::from_secs(10), || lab
            .journal()
            .contains("2.3.1 -> 2.3.3 is current")),
        "never saw C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::USR1);
    assert!(
        wait_for(Duration::from_secs(10), || lab.pid_of("C").is_some()),
        "never moved to C; journal:\n{}",
        lab.journal()
    );
    signal(&child, Signal::TERM);
    assert_eq!(
        wait_exit(&mut child, Duration::from_secs(10)).code(),
        Some(143)
    );
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
