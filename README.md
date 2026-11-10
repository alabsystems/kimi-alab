# kimi-alab

The ALab harness for Moonshot AI's **Kimi Code** CLI (`kimi`).

`kimi-alab` runs the **unmodified** vendor `kimi`, resolved from the atpkg store, as its child
on your terminal. It passes every argument through, follows the child's signals and stops, and
exits with the child's own status. When atpkg installs a newer `kimi` while a session is
running, the harness waits for a safe point and resumes **the same session** on the new build,
and the harness process never exits while it does so. A new harness build likewise crosses a safe
point by `exec`, keeping the harness pid and leaving the vendor child running. It never ships,
forks or patches vendor bytes, and its version is independent of the vendor's.

Inside aterm, atpkg's managed `kimi` twin automatically starts an installed, live `kimi-alab`
as `kimi-alab run -- …`. `--no-harness`, `--no-alab-harness` or `ATERM_NO_HARNESS=1` keeps the
vendor direct, and a missing or unfinished harness falls back to the vendor.

```console
$ kimi-alab --version
kimi-alab <version> (kimi <vendor version>)
$ kimi-alab --yolo            # the vendor binary's own --yolo (no aterm alias), under the harness
$ kimi-alab --alab-which      # what would run, from where, and why
```

## Usage

```
kimi-alab [--alab-no-update] [kimi arguments…]
kimi-alab run -- [kimi arguments…]  atpkg's managed-launch envelope
kimi-alab --version | -V        the harness and kimi versions
kimi-alab --alab-which          what would run, from where, and why
kimi-alab --alab-help           help
```

* Every word after the harness's own leading `--alab-*` flags reaches `kimi` verbatim, in
  order (an empty word included). `--version`/`-V` belong to the harness only when they are
  the single argument; `kimi-alab -m k2 -V` passes `-V` to `kimi`.
* In atpkg's `run --` envelope, every following word is the vendor's, including `--version` and
  words in the `--alab-*` namespace. The envelope requires the literal `--`.
* An unknown `--alab-*` flag in the leading position is refused (exit 2) so a mistyped
  harness flag never reaches the vendor.
* `--alab-no-update` turns the live update off for that run: the store is not watched.

### Where `kimi` comes from

Only from the atpkg store: `<prefix>/store/kimi/current/bin/kimi`, where `current` is the
symlink atpkg flips when it installs a build. Never from `PATH`, and never through the
`agents/` twin shim. The prefix is, in order:

1. `KIMI_ALAB_PKG_PREFIX` (must be absolute);
2. `[packages] prefix` in aterm's config (`$XDG_CONFIG_HOME/aterm/aterm.toml`, else
   `~/.config/aterm/aterm.toml`), as atpkg reads it, refused if it is not under `$HOME`;
3. atpkg's default: `~/Library/Application Support/aterm/pkg` on macOS,
   `~/.local/share/aterm/pkg` elsewhere.

The build must be complete as atpkg marks it (`<build>.ready` reading `ok`) and its
`bin/kimi` a regular executable file. With no installed `kimi` the harness refuses, exit 127:

```
kimi-alab: kimi is not installed in the atpkg store (…/store/kimi/current does not exist);
install it with `aterm pkg install kimi` — kimi-alab never runs a `kimi` found on PATH
```

The child gets the build's `<build>.shim-env` (admitted under atpkg's own rules) and
`KIMI_CODE_NO_AUTO_UPDATE=1`, as atpkg's shim gives it, plus two marks of the harness's own:
`KIMI_ALAB_PID` (the harness's pid) and `KIMI_ALAB_DEPTH` (its nesting depth plus one).

### It never runs itself

The harness never runs itself as the vendor:

* A store binary that **is** the harness (the same device and inode) is never run. On the
  first launch the harness refuses with exit 126. An update to such a build is never taken
  (`UPDATE-REFUSED` in the journal), and a relaunch skips it (`RELAUNCH-SKIPPED`) for the next
  candidate.
* A harness started **as the vendor of a kimi-alab** (`KIMI_ALAB_PID` names its parent or
  itself) exits 126 before it touches the store. That is a store `kimi` that leads back to
  `kimi-alab` through a script that `exec`s it, or a copy of it.
* A harness that would run **8 or more deep** (`KIMI_ALAB_DEPTH`), or whose mark is not a
  number, exits 126 the same way, so a store `kimi` that runs `kimi-alab` as a child goes no
  deeper. Starting `kimi-alab` from a Kimi tool's shell is one level deeper and still runs.

### Signals and exit status

The child runs in the harness's process group on the inherited terminal (no pty in between).

| Signal to the harness | What happens |
|---|---|
| `SIGINT`, `SIGQUIT` | Ignored by the harness. They belong to the child, which already has them from the terminal. |
| `SIGTSTP` (`^Z`) | Default: the harness stops with the child. When the child stops itself, the harness stops too, and on `SIGCONT` it continues the child. |
| `SIGHUP`, `SIGTERM` | Passed on to the child. The harness ends with the child's status and never relaunches after one. |
| `SIGUSR1` | The idle attestation (see below). |

The harness exits with the child's exit code, or dies by the signal that killed the child (with
core dumps off, so the harness leaves no core of its own for the child's crash). A `kimi` that
cannot be started at all gives 127 (not found) or 126, and so does a refusal to run itself
(126). The harness's own lines on the terminal are best effort: a broken pipe or a hung-up
terminal on standard error never ends the harness or a move.

### The live update

The harness checks `store/kimi/current` every 250 ms (one `readlink`). When it names a
different complete build, the update **waits** for a safe point. When the safe point comes:

1. The harness reads which session the child has open from Kimi's own session lock:
   `<KIMI_CODE_HOME or ~/.kimi-code>/locks/sessions/<id>.lock`, which must be held by the
   child or one of its descendants on this host. If no lock or several locks match, the
   harness refuses.
2. It sends the child `SIGTERM`, which is Kimi's graceful stop: the session is closed, the
   terminal restored, and Kimi exits 143.
3. When the child ends that way (143, or killed by `SIGTERM`) and nobody sent the harness
   `HUP`/`TERM` in between, the harness starts the build that is current at that moment with
   the resume argv: the carried launch flags, then `--session <id>`. It prints
   `kimi-alab: Kimi Code 2.3.1 -> 2.3.2: resuming session <id> on it`. If no build will start,
   it exits and prints `kimi -r <id>` so you can resume the session by hand.

The harness also watches `store/kimi-alab/current`. At the same kind of safe point it `exec`s the
new harness image in the same pid **without stopping or replacing the vendor child**. The child
pid, vendor build, session, selected account, crash-loop state and kernel identity cross on an
inherited, unlinked regular file; the current launch words cross in argv. Before the handoff the
harness captures the child's current executable and original kernel birth; the new image refuses
adoption unless parentage, birth, process group, session, live state and executable still match.
A legacy handoff without identity is admitted only by independently capturing its direct live
child. The old claim connection closes on `exec`, and the new image claims the same aterm session
again. If the replacement cannot be executed, `REEXEC-FAILED` is journaled and the old harness
and child keep running.

**The safe point needs both halves:**

* **An idle attestation**: under a proven aterm contract-3 `upgrade` claim, a fresh policy and
  status read says `agent=idle`, `composer=empty` or `placeholder`, upgrades are enabled without
  asking, and nobody is typing or holding the session. aterm's evidence is checked again at the
  actual stop point. Outside aterm (or as an explicit override), `kill -USR1 <kimi-alab pid>`
  vouches for the same facts. An attestation is good for 120 s.
* **The harness's own backstops**, over 10 s after the attestation: no keystroke was read and
  nothing was drawn on the terminal since the attestation (the tty's own access and modify
  times), no process started or ended under `kimi`, no descendant shell or background keepalive
  runs (the vendor's direct `caffeinate` excepted), and the `kimi` tree used under 2% CPU.

If any reading fails or shows activity, the child counts as busy. That attestation ends, and the
harness waits for a new one; it never retries on its own. A busy `kimi` is never stopped. The
harness also never moves a launch that is not an interactive session (`-p`, `--output-format`,
`--version`, `--help`, a subcommand, a positional word), a launch with a flag the harness does not
know, a run with no terminal, or a run with `KIMI_CODE_SESSION_LOCK` off. In every one of these
cases the new build simply runs from the next launch.

Every decision is written to a one-line-per-event journal (`START`, `UPDATE`, `ATTEST`,
`REFUSED … busy: <why>`, `MOVE`, `RELAUNCHED`, `END`, `EXIT`, …):
`~/Library/Logs/kimi-alab/journal.log` on macOS, `$XDG_STATE_HOME/kimi-alab/journal.log` (else
`~/.local/state/…`) elsewhere. `KIMI_ALAB_JOURNAL=<absolute path>` moves it, `off` disables
it. It rotates at 4 MiB.

Tuning (and testing) knobs: `KIMI_ALAB_POLL_MS` (20–5000, default 250), `KIMI_ALAB_QUIET_MS`
(at least 200, default 10000), `KIMI_ALAB_ATTEST_MS` (default 120000).

### aterm supervision: proven relaunch and upgrade claims

Inside aterm, the harness reads `@self harness policy` over the session's authenticated control
socket and, when the host serves contract 3 with supervision effective, claims
`kimi-alab@<pid> contract=3 scope=relaunch,upgrade version=<version> [vendor=<vendor version>]`
before it spawns Kimi. It keeps that connection open, requires `status` to confirm
`supervisor_proven=1`, reconnects if the connection is lost, and re-announces the vendor after
every move.

A noninteractive launch, `--alab-no-update`, `ATERM_NO_HARNESS=1`, a missing/older control socket,
a refused or unproven claim, or a policy with supervision off makes the harness **exec the vendor
in its place** instead. Kimi then runs under its own name with no second supervisor. aterm's
built-in Kimi policy retains turn-end, approvals, questions and walls. `kimi-alab --alab-which`
names the scopes it will claim.

While Kimi runs, the harness follows the session id in its lock. After an abnormal exit or fault,
a fresh proven claim and `[harness] relaunch = true` let it resume that session with the carried
flags and `--session <id>`, on the same build when it remains usable. A second crash within the
15-second startup grace ends with the child's status instead of looping. Clean exits, intentional
`HUP`/`TERM` endings, and crashes without a proven session id or relaunch claim keep their original
status.

aterm's explicit, stall and memory-pressure restart acts publish `restart cause=<why>` on the
session's event stream for a proven relaunch claimant instead of signalling its process group. The
harness subscribes on a second authenticated connection, rechecks its claim and relaunch policy,
and resumes the locked session on the same build with the carried flags and `--session <id>`. It
sends Kimi one graceful `SIGTERM`; if that move cannot be proven or the child's exit is not the
expected one, the harness reports the refusal or child's status rather than killing the group or
restarting unconditionally. A lost event connection is reopened with the current control socket.

aterm's account pool can also publish `restart cause=account[-ahead] to=<label> home=<pct>`.
The harness applies the selected `KIMI_CODE_HOME`, removes `KIMI_MODEL_NAME`,
`KIMI_MODEL_API_KEY` and `KIMI_CODE_CUSTOM_HEADERS`, and resumes the same session on that
login. Pool overlay homes share sessions, locks and folder trust with the default home, so no
conversation data is copied and a credential renewal can resume in place. The selected account
stays on later crash and upgrade relaunches. Each successful child start re-announces
`login=<label>` in the scoped claim, which is how aterm distinguishes an applied account move
from an event merely queued. A lost-login renewal uses the same handoff; no Claude-style `/login`
dialog is driven.

## Build and test

Rust here is the Trust toolchain (`rust-toolchain.toml`: `channel = "trust"`).

```sh
targo --unverified build --release        # target/release/kimi-alab
targo --unverified test                    # unit + integration tests
targo tippy --all-targets -- -D warnings
targo fmt --check
```

The integration tests (`tests/harness.rs`) lay a fake `kimi` in a scratch atpkg store and run
the real harness binary, on a real pty where the test needs a terminal. They cover argument
passthrough, exit codes and death by signal, `INT`/`QUIT` ignored, `TERM`/`HUP` forwarded,
stop and continue, the move onto a new build with the resume argv, and every refusal: no
attestation, drawing, typing, CPU work, no session lock, no terminal, a one-shot launch,
`--alab-no-update`. They also cover the refusal when no vendor is installed, `--version`, every
way the harness could run itself (a store binary that is the harness, a store `kimi` that
`exec`s it or runs it as a child, an update to it, a relaunch candidate that is it), a move
with a broken standard error, and a check that the OS's tty times move as the safe point
assumes.

## Rulings this follows

* "we need every harness (like codex-alab) to be in their own repos with their own versions
  starting from v0.1.0 written in Rust and distributed via atpkg"
* "we do need grok and kimi"
* "I am making repos for claude code harness, muse harness, etc. we also need a staging repo
  for each of these"
* "for the harnesses, there is nothing preventing updating the underlying ai cli underneith
  the harness as the harness is running as a program in aterm"
* "in claude code harness with continue or land on YOU CANNOT BLOCK ON USER FEEDBACK" /
  "THE HARNESS MUST DO THIS STOP ASKING THE USER"
* "codex-alab is the codex harness but because we have the source code, we can build it versus
  laying a harness". Kimi Code is the other case: a harness laid over the vendor binary.

## Sources

Read, not guessed:

* aterm 5e2c6d0c28 `crates/aterm-agent/src/harness/relaunch.rs:2512-2545` (`resume_argv`,
  `pool_line`): aterm resumes Kimi as `--session <id>`, after the carried flags. This is the
  resume grammar ported here. aterm has no Kimi relaunch lane
  (`harness/lanes.rs:51`, `Program::Kimi => Lanes::NONE`).
* aterm `crates/aterm-agent/src/harness/upgrade_agy.rs:44-200`: the carried-argv shape (every
  flag has a fate, an unknown flag refuses).
* aterm `crates/atpkg/src/vendor_direct/table.rs:659-735` (the `kimi` row), `store.rs`
  (`current`, `.ready`), `shim_env.rs`: the store layout and admission rules ported in
  `src/store.rs`.
* aterm `crates/aterm-phase/src/kimi.rs`: the measured spinner/idle grammar and composer reader.
  aterm publishes those as `agent=` and `composer=`; the harness still applies its own terminal,
  process-tree and CPU backstops before stopping a child.
* Kimi Code 2.3.1 (MoonshotAI/kimi-code, tag `@moonshot-ai/kimi-code@2.3.1`, MIT):
  `apps/kimi-code/src/cli/commands.ts` (flags, subcommands), `src/tui/kimi-tui.ts`
  (1050-1053 and 2916-2930: resume flags override persisted state; 1170-1201: `SIGTERM`
  exits 143, `SIGHUP` 129), `src/cli/run-shell.ts:248` (`kimi -r <id>`),
  `packages/agent-core-v2/…/sessionLockService.ts` (the session lock),
  `…/bootstrap.ts:168-174` (the home directory).

## Decisions

Choices made without the owner, each fail-closed and reversible:

1. **A harness, not a fork.** Kimi Code's source is public, but the ruling for kimi is a
   harness laid over the vendor binary. The harness spawns the store's `kimi` unchanged; it
   never copies, patches or redistributes it.
2. **Store only, never `PATH`.** A `kimi` found on `PATH` might be the atpkg shim, a global npm
   install or this harness itself. The harness reads `store/kimi/current` and refuses (127) when
   it is missing. It also refuses to exec a store binary that is the harness itself (same
   device and inode), on every launch and relaunch (decision 16). A config prefix outside
   `$HOME` is refused rather than followed.
3. **Pure passthrough, no rewriting.** atpkg's `kimi` launcher applies aterm's `--yolo` alias
   (it rewrites `--yolo` to Kimi's `--auto`) and yolo-by-default (aterm
   `crates/atpkg/src/yolo.rs`). The harness rewrites and injects nothing: `kimi-alab --yolo`
   passes Kimi's own `--yolo`, exactly as the vendor binary defines it. Passing every argument
   through unchanged is the harness's contract, and an alias layer would be a second policy
   that the resume rules would have to track.
4. **Follows `current` either way.** A rollback (atpkg pointing `current` at an older build) is
   followed like an upgrade. The harness runs what atpkg has made current; it never compares
   versions itself.
5. **The safe point is attested, not inferred.** The harness inherits the terminal and cannot
   read the screen itself. A move needs aterm's fresh idle/composer attestation under a proven
   contract-3 claim, or an idle attestation (`SIGUSR1`) elsewhere, plus the harness's tty,
   process-tree and CPU backstops. Without an attestation the update waits for the next launch.
   **This never blocks on a person.** The session keeps running on its build, nothing prompts,
   and nobody is asked.
6. **A busy reading ends the attestation.** The harness does not retry on its own. A new
   attestation must come, so a flapping child cannot be stopped by persistence.
7. **The session comes from Kimi's lock, and ambiguity refuses.** The session id is the one
   lock held by the child's process tree on this host (lock `version` 1). If zero or several
   locks match, or `KIMI_CODE_SESSION_LOCK` is off, the harness does not move the child.
8. **Mode, model and agent flags are dropped on resume.** `--yolo/--yes/--auto-approve`,
   `--auto`, `--plan`, `-m/--model`, `--agent`, `--agent-file` and the session-selection
   flags (`-S/--session`, `-r/--resume`, `-c/--continue`, `-C`) are dropped. Kimi persists
   these per session, and the flags would override what the person changed during the
   session (kimi-tui.ts 1050-1053, 2916-2930). `--skills-dir` and `--add-dir` are carried.
   An unknown flag refuses the move.
9. **Relaunch only after our own `SIGTERM` ended the child.** If the child ends any other way
   during a move (a crash, an exit code other than 143), or `HUP`/`TERM` reached the harness,
   that ending is the session's real end. The harness exits with it and prints
   `kimi -r <id>`. This release never relaunches a crashed child.
10. **The candidate order for the relaunch** is the build current at that moment, then the build
    the move was decided for, then the old build. Each is re-checked as a complete store build,
    so a withdrawn or half-written build is skipped.
11. **`KIMI_CODE_NO_AUTO_UPDATE=1` is always set.** The harness and atpkg own updates. Kimi's
    own updater stages a new binary beside the running one (`<exe dir>/.staging`) and re-execs
    it. That would change a store build in place behind atpkg and hide the update from the
    harness.
12. **`--version` succeeds without a vendor.** It prints
    `kimi-alab <version> (kimi not installed)` (or `(kimi unknown)`, decision 18) and exits
    0, because it reports the harness's version. Only a run refuses.
13. **`unsafe` is confined to descriptor ownership and native child-identity queries.** The crate
    sets `unsafe_code = "deny"`; documented exceptions take sole ownership of the inherited
    handoff descriptor and call `waitid`/`proc_pidinfo`/`proc_pidpath` for the kernel proof a
    handoff needs. Every other syscall goes through `rustix` and `signal-hook`. Trust's verified
    lanes stop at an unmodeled FFI call, so an in-crate FFI call could never pass them.
14. **The publication gate is Trust's advisory lane** (`publish/config.sh`). The measured
    results are below. The publication engine admits `targo --unverified test` only beside a
    verified `targo trust check`, and the strict lane does not pass yet.
15. **License `Apache-2.0 OR MIT`**, the ALab harness convention (aterm's dual license).
16. **Never run itself, by any route (review, 2026-10-24).** The same-file check alone saw only
    a store binary that IS the harness, and only on the first launch: a store `kimi` that
    `exec`s `kimi-alab` (a script, a shim) started the harness again without end (measured
    before the fix with a script that stopped itself at depth 4: the harness ran itself at
    every level), and a relaunch could run the harness as the vendor. Now every spawn,
    update and relaunch candidate gets the same-file check, and the child carries
    `KIMI_ALAB_PID`/`KIMI_ALAB_DEPTH` (the pair muse-alab and grok-alab carry, with the same
    limit of 8). A harness started as a harness's vendor, or 8 deep, exits 126 before
    it touches the store. The cost: a person cannot nest `kimi-alab` 8 deep, and a stale
    `KIMI_ALAB_PID` that happens to name the parent refuses; the message names the variable.
17. **The harness never panics on the terminal path.** Its own lines go to standard error
    best effort. `eprintln!` panicked on a broken pipe, which ended a move with exit 101 after
    Kimi was already stopped, measured before the fix.
18. **`--version` names Kimi's version only from its install record, and otherwise says why
    (2026-10-27; aterm `docs/DESIGN-harness-repos-2026-10-24.md` §5 and D43).** The line is
    `kimi-alab <version> (kimi <vendor version>)`. The vendor version is the `version` that
    the `current` build's `<build>.vendor` record names, and only when it names that build's
    own id (`store::build_version`; the run, the journal and the relaunch read the same
    field). With no `current` link the line says `(kimi not installed)`. For every other
    reason it says `(kimi unknown)`: no readable store, a build the store does not admit, or
    a record that is missing, garbage or names another version. Before, a store that could
    not be read printed `not installed`, and a missing record printed the version the build
    id encodes, a guess the record did not back. `--version` never runs `kimi`; the tests
    check the fake's argv record. The claim will carry the same two facts, `version=` and,
    only when known, `vendor=`.
19. **No git tag before a release (2026-10-27).** The version is `[workspace.package] version`
    and the `--version` line. A `v<version>` tag marks a released version only: the
    publication engine's `pub tag` pushes it once `pub promote` has proved the release
    public, and a promote stops at an occupied dev tag that names another commit. A hand tag
    now would either block that release or name a release that never happened, so the
    repository carries no tag until the owner directs the first promote. `pub tag` adds it
    then.
20. **A packed build prints the version its pack signs (2026-10-27; aterm
    `docs/DESIGN-harness-repos-2026-10-24.md` D46).** `atpkg-publish pack` mints
    `MAJOR.MINOR.0+g<sha9>` for the commit it builds and hands it to the build as
    `ALAB_HARNESS_VERSION`; `cli::VERSION` is that value, else the package's `MAJOR.MINOR.0`.
    The `--version` line, `--alab-which` and the claim's `version=` read it, so a packed binary
    says what its signed manifest says, and a hand build says the bare line, as before. A
    value that is not this package's line alone or followed by `+g` and 9 to 40 lowercase hex
    digits (`cli::is_pack_version`) fails the build, so a wrong one is never printed.
    Reversible: unset, it is `CARGO_PKG_VERSION`.
21. **Its first signed pack waits for the owner before any index pins it (2026-10-31; aterm
    `docs/DESIGN-harness-repos-2026-10-24.md` D50).** atpkg build 7, packed from `ad6ca04` by
    `atpkg-publish pack --upload` and signed by roster machine `incumbent-head`, is staged as
    the dev repository's prerelease `atpkg-kimi-alab-7`. No public index pins it: the pin's
    `--upload` mirrors the pack to `alabsystems/kimi-alab` first, which does not exist, and
    creating it and the promote that fills it are the owner's. Until then a machine runs it as
    a dev link. Reversible: the prerelease is deleted, or later builds supersede it.
22. **Fall back and abandon a moved build that fails on startup, and enforce a 30 s `SIGTERM`
    move grace (2026-11-03; aterm `docs/DESIGN-harness-repos-2026-10-24.md` §4.3 step 6).** If
    a child does not exit within 30 s (`KIMI_ALAB_TERM_GRACE_MS`) of the move's `SIGTERM`, the
    move is abandoned and that target build is not signalled again. Once a move starts the new
    build, if that build exits non-zero (excluding signal-derived codes `129`, `130`, `131`,
    `143`) or dies by a crash signal within the 15 s startup grace before holding the session
    lock, the harness restores the saved terminal modes, marks the target build abandoned,
    journals `MOVE-FALLBACK`, and resumes the session preferring the previous working build
    before `current`.
23. **Claim only the duties this build proves, and pass through otherwise (2026-11-06).** The
    harness claims `relaunch,upgrade` under contract 3. It starts no child until aterm confirms the
    claim as proven, and every host-driven move requires a fresh idle/composer read both when the
    attestation opens and immediately before `SIGTERM`; the local terminal, process-tree and CPU
    backstops still apply. Any startup or connection failure execs the vendor or holds the move,
    never leaves a second supervisor of the same duty. The typing scopes remain unclaimed until
    their own implementations and tests land.
24. **Crash relaunch is proven from the running child and bounded (2026-11-06).** Only an abnormal
    exit or fault qualifies, and only with a fresh proven relaunch claim and the session id most
    recently proven by the running child's lock. A second crash within 15 s ends with the child's
    status rather than forming a relaunch loop; a stable child re-arms relaunch after that grace.
    Clean and intentional exits never take this branch.

## Verification status

First measured 2026-10-24 on a clean clone of commit bd8547a, with the gate exactly as
`publish/config.sh` spells it, run step by step as the publication engine runs it (rustup `trust`
toolchain, targo 0.10.0+g41d799797). The advisory Trust check, unverified tests, lint and
formatting were rerun on this change on 2026-11-09:

| Step | Result |
|---|---|
| `targo trust check --allow-l0-gaps` (verified, advisory lane) | exit 3, `Gate: COULD-NOT-RUN [advisory lane, exit 3]`. **0 refuted.** 542 functions: 215 rows proved, 366 runtime-checked, 453 unknown. |
| `targo --unverified test` (no proof claim) | exit 0: 65 unit and 35 integration tests passed |
| `targo tippy -- -D warnings` (clippy pedantic) | exit 0 |
| `targo fmt --check` | exit 0 |
| `targo trust check` (strict lane, not the gate) | exit 1 on the same unknown rows; 0 refuted (measured on 64fd067, not rerun since) |

Read this honestly. **The crate is not proved.** Some rows are proved, but the verified lane
as a whole gives no verdict. Its unknown rows are std and dependency boundaries the verifier does not model (error discards,
FFI, path and process semantics, unsupported MIR). The runtime-checked rows are calls into
std whose bodies are not in the proof bundle. The publication engine admits exactly this
no-verdict exit 3 with zero refutations when every other step passes. A refutation, a
compile error or a failing test still fails the gate. The verifier's first runs refuted 23
rows: unchecked arithmetic in the `ps`/`/proc` parsers, the vendor build-id decoder and the
exit-code fold. Each was fixed in the code (checked arithmetic, `NonZeroU64` divisors), not
excused, and the tests that pin the fixed behaviour are in `src/proc.rs` and `src/store.rs`.

## License

Licensed under either of Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT license
(`LICENSE-MIT`), at your option.
