// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `kimi-alab`: run Kimi Code from the atpkg store under the ALab harness.

#[cfg(unix)]
use alab_wire::Link;
#[cfg(unix)]
use kimi_alab::{
    aterm,
    cli::{self, Command, NAME},
    env::Env,
    journal::Journal,
    kimi, launch,
    store::{self, Build, Prefix, StoreError},
    supervise,
};

/// The atpkg prefix, and the build it has current (or why there is none).
#[cfg(unix)]
fn resolve(env: &Env) -> Result<(Prefix, Result<Build, StoreError>), StoreError> {
    let prefix = store::resolve_prefix(env)?;
    let build = store::current(&prefix.path, kimi::PROGRAM, kimi::COMMAND);
    Ok((prefix, build))
}

/// `--alab-which`: where everything a run would use comes from. Exits 127 when there is no
/// vendor binary to run.
#[cfg(unix)]
fn which(env: &Env) {
    println!("{} {}", NAME, cli::VERSION);
    let (prefix, build) = match resolve(env) {
        Ok(found) => found,
        Err(e) => {
            println!("prefix    unresolved: {e}");
            std::process::exit(127);
        }
    };
    println!("prefix    {} ({:?})", prefix.path.display(), prefix.source);
    match build {
        Ok(b) => {
            println!(
                "current   store/{}/{} ({} {})",
                kimi::PROGRAM,
                b.id,
                kimi::COMMAND,
                b.version_label()
            );
            println!("binary    {}", b.exe.display());
            let shim: Vec<String> = b.shim_env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            if shim.is_empty() {
                println!("shim env  (none)");
            } else {
                println!("shim env  {}", shim.join(" "));
            }
        }
        Err(e) => {
            println!("current   none: {e}");
            std::process::exit(127);
        }
    }
    let harness: Vec<String> = kimi::HARNESS_ENV
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    println!("harness   {}", harness.join(" "));
    match kimi::home(env) {
        Ok(h) => println!("kimi home {}", h.display()),
        Err(e) => println!("kimi home unusable: {e}"),
    }
    println!(
        "aterm     contract {}, scope={} claimed by {} when the host proves it",
        aterm::config()
            .contracts()
            .iter()
            .map(|contract| contract.number().to_string())
            .collect::<Vec<_>>()
            .join(","),
        alab_wire::claim::scope_words(&aterm::SCOPES),
        alab_wire::claim::holder(aterm::HOLDER_ID, std::process::id())
    );
}

/// Exec the vendor in place when aterm cannot prove this harness's claim, or the launch leaves
/// the harness out, so no second supervisor stands between the tab and Kimi. A launch `env` says
/// leaves the harness out hands the vendor `ATERM_NO_HARNESS=1`, as atpkg's launcher exports it.
#[cfg(unix)]
fn pass_through(
    build: &Build,
    args: &[std::ffi::OsString],
    depth: u32,
    env: &Env,
    journal: &mut Journal,
    why: &str,
) -> ! {
    use std::os::unix::process::CommandExt as _;
    journal.note(
        "PASS",
        &format!("the vendor runs in the harness's place: {why}"),
    );
    let mut cmd = std::process::Command::new(&build.exe);
    cmd.args(args);
    for (key, value) in &build.shim_env {
        cmd.env(key, value);
    }
    for (key, value) in kimi::HARNESS_ENV {
        cmd.env(key, value);
    }
    if aterm::left_out(env) {
        cmd.env(aterm::NO_HARNESS_ENV, "1");
    }
    cmd.env_remove(supervise::PID_ENV);
    cmd.env(supervise::DEPTH_ENV, depth.saturating_add(1).to_string());
    let error = cmd.exec();
    eprintln!("{NAME}: cannot run {}: {error}", build.exe.display());
    std::process::exit(if error.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    });
}

/// Why this launch makes no claim at all and runs the vendor in its place, before aterm is
/// asked anything: the person left the harness out, or the launch cannot be resumed on another
/// build, so no duty could be performed.
#[cfg(unix)]
fn unclaimable(env: &Env, args: &[std::ffi::OsString]) -> Option<String> {
    if aterm::left_out(env) {
        return Some(format!(
            "{}=1 leaves the harness out",
            aterm::NO_HARNESS_ENV
        ));
    }
    let Some(words) = args
        .iter()
        .map(|word| word.to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
    else {
        return Some("the launch carries a word that is not UTF-8".to_string());
    };
    kimi::carried(&words).err().map(|why| why.to_string())
}

/// The launch's link to aterm (`alab-wire`): the policy, which must serve contract 4 (the only
/// one this build declares, [`aterm::CONTRACTS`]; alab-wire sends no claim otherwise and
/// [`aterm::unserved`] says why), the claim, and its scope read back, made before the first
/// child starts; or, for a child an earlier harness image ran, the same claim kept trying
/// whatever aterm answers. `Err` is why the vendor runs in the harness's place instead.
#[cfg(unix)]
fn claim(
    env: &Env,
    start: &supervise::Start,
    args: &[std::ffi::OsString],
    journal: &mut Journal,
) -> Result<Link, String> {
    let config = aterm::config();
    let me = std::process::id();
    let now = aterm::now_ms();
    let lookup = aterm::lookup(env);
    let fresh = match start {
        supervise::Start::Fresh(build) => build,
        supervise::Start::Adopt(state) => {
            let login = state.account.as_ref().map(|account| account.label.as_str());
            let child = aterm::child(
                state.build.version.as_deref(),
                login,
                u32::try_from(state.pid).ok(),
            );
            let mut link = Link::adopt_with(config, me, child, now, lookup);
            aterm::journal_notes(&mut link, journal);
            return Ok(link);
        }
    };
    if let Some(why) = unclaimable(env, args) {
        return Err(why);
    }
    let child = aterm::child(fresh.version.as_deref(), None, None);
    let mut link = Link::launch_with(config, me, child, now, lookup);
    let last = aterm::journal_notes(&mut link, journal);
    if link.is_absent() {
        return Err(last.unwrap_or_else(|| "aterm made no claim".to_string()));
    }
    if let Some(why) = aterm::unserved(&link) {
        // aterm lists no contract 4 (`UNCLAIMED policy contract=2,3 declared=4`): nothing was
        // claimed, and Kimi runs in the harness's place rather than wait for an aterm update.
        return Err(why.to_string());
    }
    if link.judges() && !aterm::takes_both(&link) {
        let granted = aterm::granted(&link);
        link.release(aterm::now_ms());
        return Err(format!(
            "scope={granted} (the claim needs {})",
            alab_wire::claim::scope_words(&aterm::SCOPES)
        ));
    }
    if link.is_lost() {
        // aterm answered the handshake and then could not say now (an `ERR` that asks to be
        // retried, a reply late past its bound, a policy off for now): it is there, so the
        // child runs supervised and the claim is made again on the link's clock.
        journal.note(
            "UNCLAIMED",
            "the child stays supervised while the claim is tried again every 5 s",
        );
    }
    Ok(link)
}

#[cfg(unix)]
fn fresh_start(prefix: &std::path::Path) -> supervise::Start {
    let build = match store::current(prefix, kimi::PROGRAM, kimi::COMMAND) {
        Ok(build) => build,
        Err(error) => {
            eprintln!("{NAME}: {error}");
            std::process::exit(127);
        }
    };
    if store::is_this_harness(&build.exe) {
        eprintln!(
            "{NAME}: {} is this harness itself; refusing to run it",
            build.exe.display()
        );
        std::process::exit(126);
    }
    supervise::Start::Fresh(build)
}

/// THE LAUNCHER'S BLOCKS, in its order ([`launch`]), for a fresh launch through atpkg's `run --`
/// envelope: a self-update word is handed to aterm's package manager, which replaces this
/// process; Kimi has no sign-in gate; `--no-harness` and `--no-alab-harness` are taken off and
/// leave the harness out (`ATERM_NO_HARNESS=1` in `env`); the yolo block decides the words and
/// prints the command as run. The words to run, and whether a word left the harness out.
#[cfg(unix)]
fn managed_launch(
    env: &mut Env,
    prefix: &std::path::Path,
    args: Vec<std::ffi::OsString>,
) -> (Vec<std::ffi::OsString>, bool) {
    use std::io::Write as _;
    use std::os::unix::process::CommandExt as _;
    if let Some(mut update) = launch::self_update(&args, env, prefix, std::process::id()) {
        let error = update.exec();
        eprintln!(
            "{NAME}: cannot run {}: {error}",
            update.get_program().to_string_lossy()
        );
        std::process::exit(if error.kind() == std::io::ErrorKind::NotFound {
            127
        } else {
            126
        });
    }
    let (args, by_word) = launch::strip_no_harness(args);
    if by_word {
        env.set(aterm::NO_HARNESS_ENV, std::ffi::OsStr::new("1"));
    }
    match launch::yolo(&args, aterm::left_out(env)) {
        Some((run, line)) => {
            // Best effort, as every line of the harness's own: a closed standard error never
            // stops the launch.
            let _ = std::io::stderr().write_all(&line);
            (run, by_word)
        }
        None => (args, by_word),
    }
}

/// Run a fresh session, or adopt the child an earlier harness image carried through re-exec. A
/// fresh `managed` launch (atpkg's `run --`) first gets the launcher's blocks
/// ([`managed_launch`]); an adopted child's words were decided when it started.
#[cfg(unix)]
fn run(env: &Env, no_update: bool, managed: bool, args: Vec<std::ffi::OsString>) -> ! {
    let handed = match kimi_alab::handoff::receive(env) {
        Ok(handed) => handed,
        Err(why) => {
            eprintln!("{NAME}: cannot adopt the harness's child: {why}");
            std::process::exit(126);
        }
    };
    let mut run_env = env.clone();
    run_env.remove(kimi_alab::handoff::FD_ENV);
    // Never run as the vendor of another kimi-alab, and never nest without bound: a store `kimi`
    // that leads back to this harness would otherwise start it again forever.
    let depth = match supervise::nesting(
        &run_env,
        std::os::unix::process::parent_id(),
        std::process::id(),
    ) {
        Ok(d) => d,
        Err(why) => {
            eprintln!("{NAME}: {why}");
            std::process::exit(126);
        }
    };
    let prefix = match store::resolve_prefix(&run_env) {
        Ok(prefix) => prefix,
        Err(error) => {
            eprintln!("{NAME}: {error}");
            std::process::exit(127);
        }
    };
    let (args, by_word) = if managed && handed.is_none() {
        managed_launch(&mut run_env, &prefix.path, args)
    } else {
        (args, false)
    };
    let args = &args[..];
    let env = &run_env;
    let start = if let Some(state) = handed {
        supervise::Start::Adopt(state)
    } else {
        fresh_start(&prefix.path)
    };
    let adopted = matches!(&start, supervise::Start::Adopt(_));
    let mut journal = Journal::open(env);
    if by_word {
        let supervise::Start::Fresh(build) = &start else {
            unreachable!("only a fresh launch reads the launcher's words")
        };
        pass_through(
            build,
            args,
            depth,
            env,
            &mut journal,
            "--no-harness leaves the harness out",
        );
    }
    let link = if aterm::hosted(env, &aterm::config()) {
        if no_update && !adopted {
            let supervise::Start::Fresh(build) = &start else {
                unreachable!()
            };
            pass_through(
                build,
                args,
                depth,
                env,
                &mut journal,
                "--alab-no-update leaves no upgrade duty",
            );
        }
        match claim(env, &start, args, &mut journal) {
            Ok(link) => link,
            Err(why) => {
                let supervise::Start::Fresh(build) = &start else {
                    unreachable!("an adopted child's claim never fails")
                };
                pass_through(build, args, depth, env, &mut journal, &why)
            }
        }
    } else {
        Link::absent(aterm::config(), std::process::id())
    };
    let settings = supervise::Settings::from_env(prefix.path, env.clone(), no_update, depth);
    let outcome = supervise::run(&settings, start, args, link, &mut journal);
    journal.note("EXIT", &format!("{outcome:?}"));
    drop(journal);
    supervise::finish(outcome)
}

#[cfg(unix)]
fn main() {
    let env = Env::from_process();
    let command = match cli::parse(std::env::args_os().skip(1).collect()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    match command {
        Command::Help => print!("{}", cli::help()),
        Command::Version => {
            // The store is read, never the vendor run.
            let current = resolve(&env).and_then(|(_, b)| b);
            println!("{}", cli::version_line(&current));
        }
        Command::Which => which(&env),
        Command::Run {
            no_update,
            managed,
            args,
        } => run(&env, no_update, managed, args),
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("kimi-alab: this platform is not supported (Kimi Code is served on macOS and Linux)");
    std::process::exit(1);
}
