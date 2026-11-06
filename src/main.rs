// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `kimi-alab`: run Kimi Code from the atpkg store under the ALab harness.

#[cfg(unix)]
use kimi_alab::{
    aterm,
    cli::{self, Command, NAME},
    env::Env,
    journal::Journal,
    kimi,
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
        "aterm     contract {} not advertised: {} does not claim supervision (would claim {})",
        aterm::CONTRACT,
        NAME,
        aterm::holder(std::process::id())
    );
}

/// Run the session: resolve the vendor binary from the store (refusing with 127 when there is
/// none), supervise it, and end as it ended.
#[cfg(unix)]
fn run(env: &Env, no_update: bool, args: &[std::ffi::OsString]) -> ! {
    // Never run as the vendor of another kimi-alab, and never nest without bound: a store `kimi`
    // that leads back to this harness would otherwise start it again forever.
    let depth =
        match supervise::nesting(env, std::os::unix::process::parent_id(), std::process::id()) {
            Ok(d) => d,
            Err(why) => {
                eprintln!("{NAME}: {why}");
                std::process::exit(126);
            }
        };
    let (prefix, build) = match resolve(env) {
        Ok((p, Ok(b))) => (p, b),
        Ok((_, Err(e))) | Err(e) => {
            eprintln!("{NAME}: {e}");
            std::process::exit(127);
        }
    };
    // Never exec ourselves, whatever the store holds.
    if store::is_this_harness(&build.exe) {
        eprintln!(
            "{NAME}: {} is this harness itself; refusing to run it",
            build.exe.display()
        );
        std::process::exit(126);
    }
    let mut journal = Journal::open(env);
    let settings = supervise::Settings::from_env(prefix.path, env.clone(), no_update, depth);
    let outcome = supervise::run(&settings, build, args, &mut journal);
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
        Command::Run { no_update, args } => run(&env, no_update, &args),
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("kimi-alab: this platform is not supported (Kimi Code is served on macOS and Linux)");
    std::process::exit(1);
}
