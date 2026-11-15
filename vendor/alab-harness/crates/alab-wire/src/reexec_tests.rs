// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The re-exec: the probe, the carry and its child's kernel identity, the
//! caught signals across the exec, and real execs of this test binary (an
//! "image", one ignored test run alone in a process of its own) that claim
//! again under the same pid against the fake aterm.
//!
//! An image is this test binary run as `<binary> reexec::tests::<image>
//! --exact --ignored --nocapture --test-threads=1`, given a scratch directory
//! in `ALAB_REEXEC_TEST_DIR`; alone, it does nothing. An exec into an image
//! passes that lead and `--` before `--alab-reexec <fd>`, which libtest then
//! reads as filters that name no test ([`exec_with`]). libtest runs each test
//! on a thread of its own, so a signal pending on the thread that exec'd is
//! pending on the new image's main thread, not on its test's: an image reads
//! the mask that crossed, and the pending signals are read where they were
//! posted.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::claim::Scope;
use crate::fake::FakeAterm;
use crate::link::Link;

/// The harness under test.
const ID: &str = "example-alab";
/// How long a test waits for an image or a child before it fails.
const HANG: Duration = Duration::from_secs(20);
/// Where an image finds the scratch directory its test made.
const DIR_VAR: &str = "ALAB_REEXEC_TEST_DIR";
/// The signals the images catch.
const CAUGHT: [libc::c_int; 3] = [libc::SIGHUP, libc::SIGINT, libc::SIGTERM];

fn config(version: &str) -> Config {
    Config::new(ID, "example", &[Scope::Relaunch, Scope::Upgrade], version).expect("a config")
}

fn protocol() -> Protocol {
    Protocol::of(&config("0.5.0"), 1)
}

fn now_ms() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch");
    u64::try_from(since.as_millis()).expect("in range")
}

/// A fresh private directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "alab-reexec-{name}-{}-{}.noindex",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// An executable shell script `name` in `dir`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write the script");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make it executable");
    path
}

/// A vendor stand-in: `sleep`, this process's child, killed and reaped when
/// dropped.
struct Vendor(std::process::Child);

impl Vendor {
    fn start() -> Self {
        Self(
            Command::new("sleep")
                .arg("60")
                .stdin(Stdio::null())
                .spawn()
                .expect("start sleep"),
        )
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// Kill it, leaving it unreaped.
    fn end(&mut self) {
        self.0.kill().expect("kill the vendor");
    }

    fn reap(&mut self) {
        self.0.wait().expect("reap the vendor");
    }
}

impl Drop for Vendor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A carry of this process and its child `child`, every optional fact set and
/// fields that need escaping.
fn carry_of(child: Identity) -> Carry {
    Carry {
        from: "0.5.0".to_string(),
        pid: std::process::id(),
        child,
        vendor: Some("1.2.3".to_string()),
        account: Some(Account {
            label: "work-2".to_string(),
            home: Some(PathBuf::from("/Users/_owner/homes/work 2")),
        }),
        ladder: (2, Some(Duration::from_millis(3_500))),
        linked: true,
        own: vec![
            Own {
                key: "arg".to_string(),
                fields: vec![
                    b"a prompt with spaces, 100% and a\nnewline".to_vec(),
                    Vec::new(),
                    vec![0xff, b'x'],
                ],
            },
            Own {
                key: "session".to_string(),
                fields: vec![b"01a148f8-9301-7752".to_vec()],
            },
            Own {
                key: "term-ignored".to_string(),
                fields: Vec::new(),
            },
        ],
    }
}

/// `text` in a pipe whose write end is closed: the read end's number, which
/// [`receive`] takes over.
fn piped(text: &str) -> String {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for the two descriptors pipe(2) writes.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // SAFETY: pipe(2) just returned the write end, owned by nothing else.
    File::from(unsafe { OwnedFd::from_raw_fd(fds[1]) })
        .write_all(text.as_bytes())
        .expect("write the carry");
    fds[0].to_string()
}

/// Whether this thread's mask blocks `sig`.
fn blocked_here(sig: libc::c_int) -> bool {
    let mut set = MaybeUninit::<libc::sigset_t>::zeroed();
    // SAFETY: no change is asked for; `set` receives the current mask.
    let rc = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, std::ptr::null(), set.as_mut_ptr()) };
    assert_eq!(rc, 0);
    // SAFETY: pthread_sigmask filled it (a zeroed set is valid besides).
    let set = unsafe { set.assume_init() };
    // SAFETY: sigismember reads an initialized set.
    unsafe { libc::sigismember(&raw const set, sig) == 1 }
}

/// Post `sig` to this thread alone.
fn raise_here(sig: libc::c_int) {
    // SAFETY: pthread_kill(3) to the calling thread.
    assert_eq!(unsafe { libc::pthread_kill(libc::pthread_self(), sig) }, 0);
}

/// Which signals the images' handler saw, one bit each.
static ARRIVED: AtomicU64 = AtomicU64::new(0);

extern "C" fn arrived(sig: libc::c_int) {
    if let Ok(bit) = u32::try_from(sig)
        && bit < 64
    {
        ARRIVED.fetch_or(1 << bit, Ordering::SeqCst);
    }
}

fn bits(signals: &[libc::c_int]) -> u64 {
    signals.iter().fold(0, |all, &sig| all | (1 << sig))
}

/// Install [`arrived`] for `signals`, process-wide: only in an image.
fn install(signals: &[libc::c_int]) {
    for &sig in signals {
        // SAFETY: an all-zero sigaction is a valid one (no flags, an empty
        // mask); its handler is set next.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = arrived as *const () as libc::sighandler_t;
        // SAFETY: a valid action, and no old one is asked for.
        let rc = unsafe { libc::sigaction(sig, &raw const action, std::ptr::null_mut()) };
        assert_eq!(rc, 0);
    }
}

/// Wait until the handler has seen every signal of `signals`.
fn await_arrived(signals: &[libc::c_int]) {
    let start = Instant::now();
    while ARRIVED.load(Ordering::SeqCst) != bits(signals) {
        assert!(
            start.elapsed() < HANG,
            "arrived {:b}, not {signals:?}",
            ARRIVED.load(Ordering::SeqCst)
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn this_binary() -> PathBuf {
    std::env::current_exe().expect("this test binary")
}

/// The arguments that make this test binary the image `name`.
fn image_args(name: &str) -> Vec<OsString> {
    let filter = format!("reexec::tests::{name}");
    [
        filter.as_str(),
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

/// An exec's lead into the image `name`: its arguments and `--`.
fn lead(name: &str) -> Vec<OsString> {
    let mut lead = image_args(name);
    lead.push(OsString::from("--"));
    lead
}

/// The image `name` working in `dir`, with only `PATH` and `env` set, its
/// output in `dir` and its input a pipe its test holds.
fn image(name: &str, dir: &Path, env: &[(String, String)]) -> Command {
    let mut command = Command::new(this_binary());
    command
        .args(image_args(name))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(DIR_VAR, dir)
        .envs(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .stdin(Stdio::piped())
        .stdout(File::create(dir.join("out")).expect("its output"))
        .stderr(File::create(dir.join("err")).expect("its errors"));
    command
}

/// In an image: its test's directory, or `None` when it was run by hand.
fn image_dir() -> Option<PathBuf> {
    std::env::var_os(DIR_VAR).map(PathBuf::from)
}

/// In an image: the descriptor its `--alab-reexec <fd>` names.
fn reexec_fd() -> OsString {
    let args: Vec<OsString> = std::env::args_os().collect();
    let at = args
        .iter()
        .position(|arg| arg == ARG)
        .expect("--alab-reexec in the arguments");
    args.get(at + 1).expect("its descriptor").clone()
}

/// Write `text` as the file `name` in `dir`, whole or not at all.
fn witness(dir: &Path, name: &str, text: &str) {
    let part = dir.join(format!("{name}.part"));
    fs::write(&part, text).expect("write the witness");
    fs::rename(&part, dir.join(name)).expect("place the witness");
}

/// What the image wrote to its output and errors.
fn output(dir: &Path) -> String {
    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap_or_default();
    format!("stdout:\n{}\nstderr:\n{}", read("out"), read("err"))
}

/// The witness `name` in `dir`, once an image has written it.
fn await_file(dir: &Path, name: &str, image: &mut std::process::Child) -> String {
    let start = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(dir.join(name)) {
            return text;
        }
        if let Ok(Some(status)) = image.try_wait() {
            // It can have written the witness and ended since the read above.
            if let Ok(text) = fs::read_to_string(dir.join(name)) {
                return text;
            }
            panic!("the image ended ({status}) before {name}\n{}", output(dir));
        }
        if start.elapsed() > HANG {
            let _ = image.kill();
            let _ = image.wait();
            panic!("no {name} within {HANG:?}\n{}", output(dir));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The image's exit, within [`HANG`].
fn finish(image: &mut std::process::Child, dir: &Path) -> ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = image.try_wait().expect("wait for the image") {
            return status;
        }
        if start.elapsed() > HANG {
            let _ = image.kill();
            let _ = image.wait();
            panic!("the image did not end within {HANG:?}\n{}", output(dir));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// End and reap `pid`, which must be this process's own child.
fn end_child(pid: u32) {
    let pid = libc::pid_t::try_from(pid).expect("a pid");
    // SAFETY: kill(2) on one positive pid, this image's child.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let mut status = 0;
    // SAFETY: waitpid(2) on that one pid, with a writable status.
    let reaped = unsafe { libc::waitpid(pid, &raw mut status, 0) };
    assert_eq!(reaped, pid, "the child crossed as this process's own");
}

/// In an image: a vendor stand-in, `sleep`, left unwaited here because it
/// crosses the exec and the new image ends and reaps it ([`end_child`]).
fn crossing_vendor() -> u32 {
    Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .spawn()
        .expect("the vendor stand-in")
        .id()
}

/// A probe proves only a build that exits 0 having printed exactly this
/// protocol's line; anything else leaves no [`Proved`], which is the only
/// thing [`exec`] takes. An exec the proof no longer covers, of a carry that
/// is not this process's, of an ended child or onto a file `execve` refuses
/// is held: this process runs on, with its signal mask back.
/// NEGATIVE CONTROL: a build that answers is proved.
#[test]
fn a_failed_probe_keeps_the_old_image() {
    let dir = Scratch::new("probe");
    let line = protocol().line().to_string();
    let within = HANG;
    let right = script(&dir.0, "right", &format!("printf '%s\\n' '{line}'"));
    let proved = probe(&right, &protocol(), within).expect("a build that answers is proved");
    assert_eq!(proved.exe(), right);
    assert_eq!(proved.protocol(), &protocol());

    let said = |name: &str, body: &str| {
        probe(&script(&dir.0, name, body), &protocol(), within).expect_err("not proved")
    };
    assert_eq!(
        said("older", "printf '%s\\n' 'alab-reexec 1 example-alab 0'"),
        Unproved::Said("alab-reexec 1 example-alab 0\\n".to_string()),
        "a build whose own lines are another number"
    );
    assert!(matches!(
        said("twice", &format!("printf '%s\\n%s\\n' '{line}' '{line}'")),
        Unproved::Said(_)
    ));
    assert_eq!(
        said("bare", &format!("printf '%s' '{line}'")),
        Unproved::Said(line.escape_debug().to_string()),
        "no newline"
    );
    assert_eq!(said("silent", "exit 0"), Unproved::Said(String::new()));
    assert!(
        matches!(
            said("harness", "printf 'example-alab 0.5.0\\n'"),
            Unproved::Said(_)
        ),
        "a build that does not know the probe runs as a harness and says otherwise"
    );
    assert!(matches!(
        said("failing", &format!("printf '%s\\n' '{line}'; exit 3")),
        Unproved::Exited(status) if status.contains('3')
    ));
    let other = Protocol::of(
        &Config::new("other-alab", "other", &[Scope::Upgrade], "0.1.0").expect("config"),
        1,
    );
    assert!(
        matches!(probe(&right, &other, within), Err(Unproved::Said(_))),
        "another harness"
    );
    let hung = script(&dir.0, "hung", "exec sleep 30");
    let started = Instant::now();
    assert_eq!(
        probe(&hung, &protocol(), Duration::from_millis(300)),
        Err(Unproved::TimedOut(Duration::from_millis(300)))
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(matches!(
        probe(&dir.0.join("missing"), &protocol(), within),
        Err(Unproved::NoExec(_))
    ));
    // Only an absolute path: a relative one is found on `PATH` by the probe
    // but in the working directory by the stamp and the exec.
    let here = std::env::current_dir().expect("the working directory");
    let up = "../".repeat(here.components().count().saturating_sub(1));
    let relative = PathBuf::from(format!(
        "{up}{}",
        right.strip_prefix("/").expect("absolute").display()
    ));
    assert_eq!(
        fs::canonicalize(&relative).ok(),
        fs::canonicalize(&right).ok(),
        "the same file"
    );
    assert!(
        matches!(
            probe(&relative, &protocol(), within),
            Err(Unproved::NoExec(why)) if why.contains("not an absolute path")
        ),
        "{relative:?}"
    );
    assert!(matches!(
        probe(Path::new("sh"), &protocol(), within),
        Err(Unproved::NoExec(why)) if why.contains("not an absolute path")
    ));
    let plain = dir.0.join("plain");
    fs::write(&plain, format!("#!/bin/sh\nprintf '%s\\n' '{line}'\n")).expect("write");
    assert!(
        matches!(probe(&plain, &protocol(), within), Err(Unproved::NoExec(_))),
        "not executable"
    );

    // The old image is kept, its mask back, whatever holds the exec.
    let mut vendor = Vendor::start();
    let carry = carry_of(Identity::of(vendor.pid()).expect("the child's identity"));
    let held = |proved: &Proved, carry: &Carry| {
        assert!(!blocked_here(libc::SIGUSR2));
        let held = exec(block(&[libc::SIGUSR2]).expect("blocked"), proved, carry);
        assert!(!blocked_here(libc::SIGUSR2), "the mask came back");
        held
    };
    // A build rewritten after its probe is not the build proved.
    fs::write(
        &right,
        format!("#!/bin/sh\nprintf '%s\\n' '{line}'\n# rebuilt\n"),
    )
    .expect("rebuild");
    assert!(matches!(held(&proved, &carry), Held::Changed(why) if why.contains("since its probe")));
    let proved = probe(&right, &protocol(), within).expect("proved again");
    fs::remove_file(&right).expect("remove it");
    assert!(
        matches!(held(&proved, &carry), Held::Changed(_)),
        "a build gone"
    );
    // A carry another process wrote.
    let foreign = Carry {
        pid: carry.pid + 1,
        ..carry.clone()
    };
    assert!(matches!(held(&proved, &foreign), Held::Carry(why) if why.contains("not this one")));
    // A file execve refuses, though nothing changed since it was stamped.
    let refused = Proved::unprobed(&plain, &protocol()).expect("stamped");
    assert!(matches!(held(&refused, &carry), Held::Exec(why) if why.contains("execve")));
    // A carry over the bound is refused before any pipe is made.
    let large = Carry {
        own: vec![Own {
            key: "large".to_string(),
            fields: vec![vec![b'x'; MAX_BYTES]],
        }],
        ..carry.clone()
    };
    let over = format!("over {MAX_BYTES}");
    assert!(matches!(held(&refused, &large), Held::Carry(why) if why.contains(&over)));
    // Text larger than a pipe takes is refused by the pipe, at once: its write
    // end never blocks. 2 MiB is above every pipe's first capacity: 64 KiB on
    // macOS, 16 pages on Linux (1 MiB with 64 KiB pages).
    let (sent, filled) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sent.send(carried_on_pipe(&vec![b'x'; 32 * MAX_BYTES]).map(drop));
    });
    let filled = filled
        .recv_timeout(HANG)
        .expect("a full pipe refuses the carry, never blocks");
    assert!(
        filled
            .as_ref()
            .is_err_and(|why| why.contains("did not fit the pipe")),
        "{filled:?}"
    );
    // A child that has ended is the old image's to reap.
    vendor.end();
    let start = Instant::now();
    while carry.child.check() != Ok(Alive::Ended) {
        assert!(start.elapsed() < HANG, "{:?}", carry.child.check());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(held(&refused, &carry), Held::Child(why) if why.contains("ended")));
    vendor.reap();
    assert!(matches!(held(&refused, &carry), Held::Child(_)), "reaped");
}

/// A child's kernel identity is read from the kernel, its birth being the
/// start aterm checks `child=` by, crosses the carry's text whole, is received
/// from an inherited pipe and checked again there; another birth, another
/// image file, another writer or another parent is refused. An ended child not
/// yet reaped is still carried, a reaped one is not.
#[test]
fn the_carry_round_trips_the_child_and_its_birth() {
    let mut vendor = Vendor::start();
    let pid = vendor.pid();
    let this = std::process::id();
    let identity = Identity::of(pid).expect("the child's identity");
    assert_eq!((identity.pid, identity.parent), (pid, this));
    let start = crate::kernel::start_us(pid).expect("its kernel start");
    let process = identity.process().expect("its claim word");
    assert_eq!(process.pid, pid);
    match identity.birth {
        Birth::MacOs {
            seconds,
            microseconds,
        } => {
            assert_eq!(seconds * 1_000_000 + u64::from(microseconds), start);
            assert_eq!(process.start, start, "macOS's start is exact");
        }
        Birth::Linux { .. } => assert!(process.start.abs_diff(start) <= 1_100_000),
    }
    let meta = fs::metadata(&identity.executable.path).expect("its image's path");
    assert_eq!(
        (identity.executable.device, identity.executable.inode),
        (meta.dev(), meta.ino()),
        "the file the kernel runs: {:?}",
        identity.executable
    );
    assert!(identity.executable.path.ends_with("sleep"));
    assert_eq!(identity.check(), Ok(Alive::Running));
    assert_eq!(identity.current().as_ref(), Ok(&identity));

    let carry = carry_of(identity.clone());
    let text = carry.encode(&protocol()).expect("it encodes");
    assert!(
        text.starts_with(&format!(
            "alab-reexec 1 example-alab 1\nfrom 0.5.0\npid {this}\nchild {pid} {this} "
        )),
        "{text}"
    );
    assert!(
        text.contains("\naccount work-2 /Users/_owner/homes/work%202\n"),
        "{text}"
    );
    assert!(text.contains("\nladder 2 3500\nlink claims\n"), "{text}");
    assert!(
        text.contains(
            "\nown arg a%20prompt%20with%20spaces,%20100%25%20and%20a%0Anewline % %FFx\n"
        ),
        "{text}"
    );
    assert!(text.ends_with("\nown term-ignored\nend\n"), "{text}");
    assert_eq!(Carry::decode(&text, &protocol()).as_ref(), Ok(&carry));
    let bare = Carry {
        vendor: None,
        account: None,
        ladder: (0, None),
        linked: false,
        own: Vec::new(),
        ..carry.clone()
    };
    let bare_text = bare.encode(&protocol()).expect("it encodes");
    assert!(
        bare_text.ends_with("\nladder 0 -\nlink absent\nend\n"),
        "{bare_text}"
    );
    assert_eq!(Carry::decode(&bare_text, &protocol()), Ok(bare));
    let linux = Carry {
        child: Identity {
            birth: Birth::Linux { ticks: 12_345 },
            ..identity.clone()
        },
        account: Some(Account {
            label: "work-2".to_string(),
            home: None,
        }),
        ..carry.clone()
    };
    let linux_text = linux.encode(&protocol()).expect("it encodes");
    assert!(linux_text.contains(" linux:12345 "), "{linux_text}");
    assert!(linux_text.contains("\naccount work-2 -\n"), "{linux_text}");
    assert_eq!(Carry::decode(&linux_text, &protocol()), Ok(linux));
    let claim = carry.claim().expect("the claim's words");
    assert_eq!(
        (claim.vendor, claim.login, claim.process),
        (Some("1.2.3"), Some("work-2"), Some(process))
    );
    assert_eq!(carry.ladder_at(10_000).streak(), 2);
    assert_eq!(
        carry.ladder_at(10_000).since(10_000),
        Some(Duration::from_millis(3_500))
    );

    assert_eq!(
        receive(OsStr::new(&piped(&text)), &protocol()),
        Ok((carry.clone(), Alive::Running)),
        "received from an inherited pipe"
    );
    let reborn = Identity {
        birth: match identity.birth {
            Birth::MacOs {
                seconds,
                microseconds,
            } => Birth::MacOs {
                seconds,
                microseconds: (microseconds + 1) % 1_000_000,
            },
            Birth::Linux { ticks } => Birth::Linux { ticks: ticks + 1 },
        },
        ..identity.clone()
    };
    assert!(reborn.check().is_err(), "another birth is another process");
    assert!(reborn.current().is_err());
    let reborn_text = Carry {
        child: reborn,
        ..carry.clone()
    }
    .encode(&protocol())
    .expect("it encodes");
    assert!(receive(OsStr::new(&piped(&reborn_text)), &protocol()).is_err());
    let elsewhere = Identity {
        executable: Executable {
            inode: identity.executable.inode + 1,
            ..identity.executable.clone()
        },
        ..identity.clone()
    };
    assert!(
        elsewhere
            .check()
            .expect_err("another image")
            .contains("runs")
    );
    let foreign = Carry {
        pid: this + 1,
        child: Identity {
            parent: this + 1,
            ..identity.clone()
        },
        ..carry.clone()
    };
    let foreign_text = foreign.encode(&protocol()).expect("it encodes");
    assert!(
        receive(OsStr::new(&piped(&foreign_text)), &protocol())
            .expect_err("another writer")
            .contains("written by process")
    );
    assert!(
        Identity::of(this).is_err(),
        "a process is not its own child"
    );
    assert!(Identity::of(1).is_err());
    assert!(Identity::of(0).is_err());

    // Received only from an inherited pipe.
    assert!(
        receive(OsStr::new("1"), &protocol()).is_err(),
        "a standard descriptor"
    );
    assert!(receive(OsStr::new("x"), &protocol()).is_err());
    assert!(
        receive(OsStr::new("05"), &protocol()).is_err(),
        "not canonical"
    );
    let file = File::open("/dev/null").expect("open");
    assert!(
        receive(OsStr::new(&file.as_raw_fd().to_string()), &protocol())
            .expect_err("not a pipe")
            .contains("not a pipe")
    );

    vendor.end();
    let start = Instant::now();
    while identity.check() != Ok(Alive::Ended) {
        assert!(start.elapsed() < HANG, "{:?}", identity.check());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        Identity::of(pid).is_err(),
        "an ended child is not captured anew"
    );
    assert!(identity.current().is_err());
    assert_eq!(
        receive(OsStr::new(&piped(&text)), &protocol())
            .map(|(carry, alive)| (carry.child.pid, alive)),
        Ok((pid, Alive::Ended)),
        "but still carried, and said to have ended: the new image reaps it"
    );
    vendor.reap();
    assert!(identity.check().is_err(), "a reaped child is gone");
}

/// A child that ends while it is checked is still this process's own: every
/// check, from running to ended and unreaped, passes, though the kernel stops
/// answering for its session and image file as it exits.
#[test]
fn a_child_that_ends_while_it_is_checked_is_still_its_own() {
    let mut checks = 0u64;
    for round in 0..300 {
        let mut vendor = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .expect("start cat");
        let identity = Identity::of(vendor.id()).expect("its identity");
        drop(vendor.stdin.take());
        let start = Instant::now();
        loop {
            checks += 1;
            match identity.check() {
                Ok(Alive::Ended) => break,
                Ok(Alive::Running | Alive::Stopped) => {}
                Err(why) => panic!("round {round}, check {checks}: {why}"),
            }
            assert!(start.elapsed() < HANG, "round {round}: it never ended");
        }
        vendor.wait().expect("reap cat");
    }
}

/// A carry is read only whole, in the protocol it was written under, each
/// fact once and nothing after `end`, every field in its grammar.
#[test]
fn a_carry_that_is_not_whole_is_refused() {
    let vendor = Vendor::start();
    let carry = carry_of(Identity::of(vendor.pid()).expect("identity"));
    let text = carry.encode(&protocol()).expect("it encodes");
    let refused = |text: &str, what: &str| {
        assert!(Carry::decode(text, &protocol()).is_err(), "{what}: {text}");
    };
    let replaced = |from: &str, to: &str| {
        assert!(text.contains(from), "{from:?} in {text}");
        text.replacen(from, to, 1)
    };
    refused(text.trim_end_matches("end\n"), "no end");
    refused(text.trim_end_matches('\n'), "no newline after end");
    refused(&format!("{text}\n"), "a blank line after end");
    refused(&format!("{text}pid 7\n"), "a line after end");
    refused(
        &replaced("example-alab 1\n", "example-alab 2\n"),
        "another own number",
    );
    refused(
        &replaced("alab-reexec 1 ", "alab-reexec 2 "),
        "another format",
    );
    refused(&replaced("ladder 2 3500\n", ""), "no ladder");
    refused(&replaced("link claims\n", ""), "no link");
    refused(
        &replaced("link claims\n", "link maybe\n"),
        "an unknown link",
    );
    refused(&replaced("from 0.5.0\n", ""), "no version");
    refused(&replaced("from 0.5.0\n", "from 0.5\n"), "not a version");
    refused(
        &replaced("from 0.5.0\n", "from 0.5.0\nfrom 0.5.0\n"),
        "twice",
    );
    let child_line = text
        .lines()
        .find(|line| line.starts_with("child "))
        .expect("a child line");
    refused(&replaced(&format!("{child_line}\n"), ""), "no child");
    let fields: Vec<&str> = child_line.split(' ').collect();
    let with = |at: usize, value: &str| {
        let mut changed = fields.clone();
        changed[at] = value;
        replaced(child_line, &changed.join(" "))
    };
    refused(&with(2, "1"), "a parent that is not the harness");
    refused(&with(1, "1"), "pid 1");
    refused(&with(1, "0042"), "not a canonical decimal");
    refused(
        &with(5, "mac:1700000000.12345"),
        "five digits of microseconds",
    );
    refused(&with(5, "mac:0.000001"), "no seconds");
    refused(&with(5, "linux:0"), "no ticks");
    refused(&with(5, "bsd:1"), "an unknown birth");
    refused(&with(6, "relative/sleep"), "a relative image");
    refused(&with(6, "/bin/%73leep"), "a needless escape");
    refused(&with(6, "/bin/%2g"), "a bad escape");
    refused(&with(6, "/bin/%2f"), "a lowercase escape");
    refused(&with(8, "0"), "no inode");
    refused(
        &replaced("vendor 1.2.3\n", "vendor 1.2.3 x\n"),
        "a vendor of two words",
    );
    refused(
        &replaced("vendor 1.2.3\n", "vendor 1/2\n"),
        "not a vendor word",
    );
    refused(
        &replaced("account work-2 ", "account Work "),
        "not an account label",
    );
    refused(
        &replaced("account work-2 /Users", "account work-2 Users"),
        "a relative home",
    );
    refused(&replaced("own session ", "own Session "), "not a key");
    refused(&replaced("own session ", "own "), "a field for a key");
    refused(
        &replaced("ladder 2 3500\n", "ladder two 3500\n"),
        "not a streak",
    );
    refused(&replaced("pid ", "pid  "), "an empty field");
    refused(
        &replaced("link claims\n", "link claims\nbegun 1\n"),
        "an unknown line",
    );
    assert!(
        Carry::decode(&text, &Protocol::of(&config("0.5.0"), 2)).is_err(),
        "read under another protocol"
    );
    let large = Carry {
        own: vec![Own {
            key: "large".to_string(),
            fields: vec![vec![b'x'; MAX_BYTES]],
        }],
        ..carry.clone()
    };
    assert!(
        large
            .encode(&protocol())
            .expect_err("too large")
            .contains(&format!("over {MAX_BYTES}"))
    );
    let mut huge = text.clone();
    huge.insert_str(
        huge.len() - "end\n".len(),
        &"own pad x\n".repeat(MAX_BYTES / 10),
    );
    refused(&huge, "over the bound");
    let badly = |change: &dyn Fn(&mut Carry), what: &str| {
        let mut bad = carry.clone();
        change(&mut bad);
        assert!(bad.encode(&protocol()).is_err(), "{what}");
    };
    badly(&|c| c.from = "1".to_string(), "a version");
    badly(&|c| c.child.parent += 1, "a parent");
    badly(&|c| c.vendor = Some(String::new()), "a vendor");
    badly(&|c| c.own[0].key = "1st".to_string(), "a key");
    badly(
        &|c| {
            c.account = Some(Account {
                label: "help".to_string(),
                home: None,
            });
        },
        "an account",
    );
}

/// Linux's identity is read from `/proc/<pid>/stat`, on recorded text: the
/// fields counted after the last `)`, a task on its way through `exit`
/// (`PF_EXITING` in field 9) as ended.
#[test]
fn the_linux_identity_reads_state_parent_group_session_and_field_22() {
    let stat = |state: &str| {
        format!(
            "4242 (a) b) {state} 4241 4241 4240 34816 4242 4194304 100 0 0 0 1 2 0 0 20 0 1 0 \
             12345 1000000 200 18446744073709551615"
        )
    };
    let facts = linux_facts(&stat("S"), 4242).expect("facts");
    assert_eq!(
        facts,
        Facts {
            parent: 4241,
            pgid: Some(4241),
            session: Some(4240),
            birth: Birth::Linux { ticks: 12_345 },
            alive: Alive::Running,
        }
    );
    assert_eq!(
        linux_facts(&stat("Z"), 4242).map(|f| f.alive),
        Some(Alive::Ended)
    );
    assert_eq!(
        linux_facts(&stat("T"), 4242).map(|f| f.alive),
        Some(Alive::Stopped)
    );
    assert_eq!(
        linux_facts(&stat("S").replace(" 4194304 ", " 4194308 "), 4242).map(|f| f.alive),
        Some(Alive::Ended),
        "PF_EXITING: on its way out"
    );
    assert_eq!(
        linux_facts(&stat("T").replace(" 4194304 ", " 4194308 "), 4242).map(|f| f.alive),
        Some(Alive::Ended),
        "PF_EXITING, stopped on its way out"
    );
    assert_eq!(
        linux_facts(&stat("X").replace(" 4194304 ", " 4194308 "), 4242),
        None,
        "dead, whatever its flags"
    );
    assert_eq!(
        linux_facts(&stat("S").replace(" 4194304 ", " x "), 4242),
        None,
        "flags that are no number"
    );
    assert_eq!(linux_facts(&stat("X"), 4242), None, "dead");
    assert_eq!(linux_facts(&stat("S"), 4243), None, "another pid");
    assert_eq!(
        linux_facts(&stat("S").replace(" 12345 ", " 0 "), 4242),
        None
    );
    assert_eq!(linux_facts("4242 (a) S 1", 4242), None);
}

/// Signals posted while the caught ones are blocked: a `SIGTERM`, a `SIGHUP`
/// or both hold the exec and reach the handlers once the mask is back; what a
/// new image takes is what was pending; a pending `SIGINT` does not hold it,
/// and the mask crosses the exec into the new image.
/// NEGATIVE CONTROL: the `SIGINT` exec happens.
#[test]
fn a_pending_term_or_hup_holds_the_exec() {
    let dir = Scratch::new("signals");
    let mut image = image("image_signalled", &dir.0, &[])
        .spawn()
        .expect("the old image");
    let pid = image.id().to_string();
    assert_eq!(await_file(&dir.0, "signalled", &mut image), pid);
    let crossed = await_file(&dir.0, "crossed", &mut image);
    assert_eq!(crossed.split(' ').next(), Some(pid.as_str()), "{crossed}");
    let status = finish(&mut image, &dir.0);
    assert!(status.success(), "{status}\n{}", output(&dir.0));
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_signalled() {
    let Some(dir) = image_dir() else {
        return;
    };
    install(&CAUGHT);
    let vendor = crossing_vendor();
    let carry = carry_of(Identity::of(vendor).expect("its identity"));
    let proved = Proved::unprobed(&this_binary(), &protocol()).expect("stamped");
    for posted in [
        &[libc::SIGTERM][..],
        &[libc::SIGHUP],
        &[libc::SIGHUP, libc::SIGTERM],
    ] {
        ARRIVED.store(0, Ordering::SeqCst);
        let blocked = block(&CAUGHT).expect("blocked");
        for &sig in posted {
            raise_here(sig);
        }
        assert_eq!(blocked.pending(), posted);
        assert_eq!(ARRIVED.load(Ordering::SeqCst), 0, "blocked, not delivered");
        let held = exec_with(blocked, &proved, &carry, &lead("image_crossed"));
        assert_eq!(held, Held::Signals(posted.to_vec()));
        await_arrived(posted);
        assert!(
            CAUGHT.iter().all(|&sig| !blocked_here(sig)),
            "the mask is back"
        );
    }
    // The new image's half: what was posted while blocked is taken, and
    // reaches the handler once unblocked.
    ARRIVED.store(0, Ordering::SeqCst);
    let blocked = block(&CAUGHT).expect("blocked");
    raise_here(libc::SIGINT);
    assert_eq!(take_pending(&CAUGHT), [libc::SIGINT]);
    await_arrived(&[libc::SIGINT]);
    drop(blocked);
    // A caught signal that does not end the session crosses.
    ARRIVED.store(0, Ordering::SeqCst);
    let blocked = block(&CAUGHT).expect("blocked");
    raise_here(libc::SIGINT);
    assert_eq!(blocked.pending(), [libc::SIGINT]);
    witness(&dir, "signalled", &std::process::id().to_string());
    let held = exec_with(blocked, &proved, &carry, &lead("image_crossed"));
    panic!("a pending SIGINT held the exec: {held}");
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_crossed() {
    let Some(dir) = image_dir() else {
        return;
    };
    let (carry, alive) = receive(&reexec_fd(), &protocol()).expect("the carry");
    assert_eq!(alive, Alive::Running);
    for sig in CAUGHT {
        assert!(blocked_here(sig), "signal {sig} crossed the exec blocked");
    }
    witness(
        &dir,
        "crossed",
        &format!("{} {}", std::process::id(), carry.child.pid),
    );
    end_child(carry.child.pid);
}

/// The carry never crosses on a standard descriptor, which [`receive`]
/// refuses: with the old image's input closed, `pipe(2)` gives the read end
/// descriptor 0, and [`exec`] moves it above 2.
#[test]
fn a_carry_never_crosses_on_a_standard_descriptor() {
    let dir = Scratch::new("stdin");
    let mut image = image("image_stdin_closed", &dir.0, &[])
        .spawn()
        .expect("the old image");
    let pid = image.id().to_string();
    let crossed = await_file(&dir.0, "crossed", &mut image);
    assert_eq!(crossed.split(' ').next(), Some(pid.as_str()), "{crossed}");
    let status = finish(&mut image, &dir.0);
    assert!(status.success(), "{status}\n{}", output(&dir.0));
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_stdin_closed() {
    let Some(_dir) = image_dir() else {
        return;
    };
    let vendor = crossing_vendor();
    let carry = carry_of(Identity::of(vendor).expect("its identity"));
    let proved = Proved::unprobed(&this_binary(), &protocol()).expect("stamped");
    // SAFETY: closes this image's own input, which nothing here reads.
    assert_eq!(unsafe { libc::close(0) }, 0);
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for the two descriptors pipe(2) writes; both are
    // closed again at once.
    unsafe {
        assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
    assert_eq!(fds[0], 0, "pipe(2) takes the lowest free number");
    let held = exec_with(
        block(&CAUGHT).expect("blocked"),
        &proved,
        &carry,
        &lead("image_crossed"),
    );
    panic!("the exec was held: {held}");
}

/// The signals the noting images catch: [`CAUGHT`], a child's exit and the
/// window's size.
const NOTING: [libc::c_int; 5] = [
    libc::SIGHUP,
    libc::SIGINT,
    libc::SIGTERM,
    libc::SIGCHLD,
    libc::SIGWINCH,
];

/// Between the exec and the new image's handlers each caught signal is at its
/// default action, and macOS discards one whose default is to ignore it as it
/// is posted: a vendor that exits then leaves no `SIGCHLD`, a resize no
/// `SIGWINCH`. The new image is told of both all the same ([`UNKEPT`]), and
/// [`receive`] says how its child stood.
/// NEGATIVE CONTROL: a `SIGINT` posted then (its default ends the process) is
/// pending on both kernels, and reaches the handler.
#[test]
fn a_vendor_exit_after_the_exec_is_noted() {
    let dir = Scratch::new("noted");
    let mut image = image("image_noting", &dir.0, &[])
        .spawn()
        .expect("the old image");
    let pid = image.id();
    let mut taken = vec![libc::SIGINT, libc::SIGCHLD, libc::SIGWINCH];
    taken.sort_unstable();
    assert_eq!(
        await_file(&dir.0, "noted", &mut image),
        format!("{pid} Running {taken:?} reaped"),
        "{}",
        output(&dir.0)
    );
    let status = finish(&mut image, &dir.0);
    assert!(status.success(), "{status}\n{}", output(&dir.0));
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_noting() {
    let Some(_dir) = image_dir() else {
        return;
    };
    install(&NOTING);
    let vendor = crossing_vendor();
    let carry = carry_of(Identity::of(vendor).expect("its identity"));
    let proved = Proved::unprobed(&this_binary(), &protocol()).expect("stamped");
    let held = exec_with(
        block(&NOTING).expect("blocked"),
        &proved,
        &carry,
        &lead("image_noted"),
    );
    panic!("the exec was held: {held}");
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_noted() {
    let Some(dir) = image_dir() else {
        return;
    };
    let (carry, alive) = receive(&reexec_fd(), &protocol()).expect("the carry");
    for sig in NOTING {
        assert!(blocked_here(sig), "signal {sig} crossed the exec blocked");
    }
    // The vendor exits and the window changes before the handlers are in.
    let child = libc::pid_t::try_from(carry.child.pid).expect("a pid");
    // SAFETY: kill(2) on one positive pid, this image's child.
    assert_eq!(unsafe { libc::kill(child, libc::SIGKILL) }, 0);
    let start = Instant::now();
    while carry.child.check() != Ok(Alive::Ended) {
        assert!(start.elapsed() < HANG, "{:?}", carry.child.check());
        std::thread::sleep(Duration::from_millis(5));
    }
    raise_here(libc::SIGWINCH);
    raise_here(libc::SIGINT);
    ARRIVED.store(0, Ordering::SeqCst);
    install(&NOTING);
    let taken = take_pending(&NOTING);
    let start = Instant::now();
    while ARRIVED.load(Ordering::SeqCst) & bits(&[libc::SIGINT]) == 0 {
        assert!(start.elapsed() < HANG, "no SIGINT reached the handler");
        std::thread::sleep(Duration::from_millis(1));
    }
    // Ended is this image's to wait for: it may still be finishing its exit.
    let start = Instant::now();
    let reaped = loop {
        let mut status = 0;
        // SAFETY: waitpid(2) on that one pid, with a writable status.
        let reaped = unsafe { libc::waitpid(child, &raw mut status, libc::WNOHANG) };
        if reaped != 0 || start.elapsed() > HANG {
            break reaped;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    witness(
        &dir,
        "noted",
        &format!(
            "{} {alive:?} {taken:?} {}",
            std::process::id(),
            if reaped == child {
                "reaped"
            } else {
                "unreaped"
            }
        ),
    );
}

/// A real re-exec of a harness holding a contract-4 claim: the new image
/// takes the carry, finds its child still its own, and claims again through
/// `Link::adopt` under the same pid, with its own version and the same child.
#[test]
fn the_new_image_claims_again_under_the_same_pid() {
    let dir = Scratch::new("claims");
    let fake = FakeAterm::for_harness("reexec", &config("0.5.0"));
    let mut image = image("image_before", &dir.0, &fake.env())
        .spawn()
        .expect("the old image");
    let pid = image.id();
    let before = await_file(&dir.0, "before", &mut image);
    let words: Vec<&str> = before.split(' ').collect();
    let [said_pid, child, start] = words[..] else {
        panic!("{before}");
    };
    assert_eq!(said_pid, pid.to_string());
    let after = await_file(&dir.0, "after", &mut image);
    assert_eq!(
        after,
        format!("{pid} {child} 2 3500"),
        "the same process and child"
    );
    assert_eq!(fake.holder(), Some(format!("{ID}@{pid}")));
    let claim = fake.claim_words().expect("a standing claim");
    assert_eq!(claim.contract.as_deref(), Some("4"));
    assert_eq!(
        claim.version.as_deref(),
        Some("0.6.0"),
        "the new image's own version"
    );
    assert_eq!(claim.vendor.as_deref(), Some("1.2.3"));
    assert_eq!(claim.login.as_deref(), Some("work-2"));
    assert_eq!(fake.child(), Some(format!("{child}:{start}")));
    let claims = fake
        .verbs()
        .iter()
        .filter(|verb| verb.starts_with("meta set supervisor"))
        .count();
    assert_eq!(claims, 2, "one claim from each image: {:?}", fake.verbs());
    drop(image.stdin.take());
    let status = finish(&mut image, &dir.0);
    assert!(status.success(), "{status}\n{}", output(&dir.0));
    assert_eq!(await_file(&dir.0, "reaped", &mut image), child);
    assert_eq!(fake.holder(), None, "released");
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_before() {
    let Some(dir) = image_dir() else {
        return;
    };
    let vendor = crossing_vendor();
    let identity = Identity::of(vendor).expect("its identity");
    let process = identity.process().expect("its start");
    let child = Child {
        vendor: Some("1.2.3"),
        login: Some("work-2"),
        process: Some(process),
    };
    let mut link = Link::launch(config("0.5.0"), std::process::id(), child, now_ms());
    assert!(link.judges(), "{:?}", link.take_notes());
    witness(
        &dir,
        "before",
        &format!("{} {} {}", std::process::id(), vendor, process.start),
    );
    let carry = Carry {
        account: Some(Account {
            label: "work-2".to_string(),
            home: None,
        }),
        ..carry_of(identity.current().expect("the child now"))
    };
    let proved = Proved::unprobed(&this_binary(), &protocol()).expect("stamped");
    let held = exec_with(
        block(&CAUGHT).expect("blocked"),
        &proved,
        &carry,
        &lead("image_after"),
    );
    panic!("the exec was held: {held}");
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_after() {
    let Some(dir) = image_dir() else {
        return;
    };
    let (carry, alive) = receive(&reexec_fd(), &protocol()).expect("the carry");
    assert_eq!(carry.pid, std::process::id());
    assert_eq!(alive, Alive::Running);
    assert_eq!(carry.child.check(), Ok(Alive::Running));
    assert_eq!(carry.own, carry_of(carry.child.clone()).own);
    let _ = take_pending(&CAUGHT);
    let mut link = if carry.linked {
        Link::adopt(
            config("0.6.0"),
            carry.pid,
            carry.claim().expect("the claim's words"),
            now_ms(),
        )
    } else {
        Link::absent(config("0.6.0"), carry.pid)
    };
    assert!(link.judges(), "{:?}", link.take_notes());
    witness(
        &dir,
        "after",
        &format!(
            "{} {} {} {}",
            std::process::id(),
            carry.child.pid,
            carry.ladder.0,
            carry.ladder.1.map_or(0, |since| since.as_millis())
        ),
    );
    // The claim stands until the test has read it, and closes this input.
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    link.release(now_ms());
    end_child(carry.child.pid);
    witness(&dir, "reaped", &carry.child.pid.to_string());
}

/// The production path end to end with a build that is not this test binary:
/// probed, exec'd as `<exe> --alab-reexec <fd>`, it reads the carry whole
/// from its one inherited descriptor (the write end did not cross, or its
/// `cat` would never end) under the same pid.
#[test]
fn a_proved_build_reads_the_carry_from_its_one_inherited_descriptor() {
    let dir = Scratch::new("handing");
    let line = protocol().line().to_string();
    script(
        &dir.0,
        "build",
        &format!(
            r#"case "$1" in
--alab-reexec-probe) printf '%s\n' '{line}'; exit 0 ;;
--alab-reexec) ;;
*) exit 2 ;;
esac
case "$2" in ''|*[!0-9]*) exit 3 ;; esac
[ "$#" -eq 2 ] || exit 3
out="${dir_var}"
eval "cat <&$2" > "$out/carry.part" || exit 4
child=$(sed -n 's/^child \([0-9]*\) .*/\1/p' "$out/carry.part")
kill -9 "$child"
echo "$$" > "$out/pid.part"
mv "$out/carry.part" "$out/carry"
mv "$out/pid.part" "$out/pid""#,
            dir_var = DIR_VAR
        ),
    );
    let mut image = image("image_handing", &dir.0, &[])
        .spawn()
        .expect("the old image");
    let pid = image.id();
    let handing = await_file(&dir.0, "handing", &mut image);
    let (said_pid, sent) = handing.split_once('\n').expect("pid, then the carry");
    assert_eq!(said_pid, pid.to_string());
    let got = await_file(&dir.0, "carry", &mut image);
    assert_eq!(got, sent, "the carry, whole");
    let carry = Carry::decode(&got, &protocol()).expect("a whole carry");
    assert_eq!((carry.pid, carry.child.parent), (pid, pid));
    assert_eq!(
        await_file(&dir.0, "pid", &mut image).trim(),
        pid.to_string(),
        "execve kept the pid"
    );
    let status = finish(&mut image, &dir.0);
    assert!(status.success(), "{status}\n{}", output(&dir.0));
}

#[test]
#[ignore = "an image a re-exec test starts; alone it does nothing"]
fn image_handing() {
    let Some(dir) = image_dir() else {
        return;
    };
    let vendor = crossing_vendor();
    let carry = carry_of(Identity::of(vendor).expect("its identity"));
    let proved = probe(&dir.join("build"), &protocol(), HANG).expect("the build is proved");
    let text = carry.encode(proved.protocol()).expect("it encodes");
    witness(&dir, "handing", &format!("{}\n{text}", std::process::id()));
    let held = exec(block(&CAUGHT).expect("blocked"), &proved, &carry);
    panic!("the exec was held: {held}");
}
