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
| `SIGINT`, `SIGQUIT` | Ignored by the harness while a child runs: they belong to the child, which already has them from the terminal. While a crashed child is being brought back (no child runs), they end the harness with the crashed child's status. |
| `SIGTSTP` (`^Z`) | Default: the harness stops with the child. When the child stops itself, the harness stops too, and on `SIGCONT` it continues the child. |
| `SIGHUP`, `SIGTERM` | Passed on to the child. The harness ends with the child's status and never relaunches after one. While no child runs, they end the harness with the crashed child's status. |
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

* **An idle attestation**: under a proven aterm `upgrade` claim (contract 4 or 3), a fresh policy and
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
(at least 200, default 10000), `KIMI_ALAB_ATTEST_MS` (default 120000),
`KIMI_ALAB_RESTART_WAIT_MS` (100–3600000, default 45000: how long a restart aterm asked for may
wait before it is refused).

### aterm supervision: proven relaunch and upgrade claims

The link to aterm is the shared ALab library `alab-wire` (crate `crates/alab-wire` of
`alab-harness`; see "Build and test" for how it is pinned). kimi-alab keeps only what is its own
(`src/aterm.rs`): its id `kimi-alab`, its vendor word `kimi`, the duties it claims
(`relaunch,upgrade`), `ATERM_NO_HARNESS`, its clock, and what Kimi Code needs (its account home
`KIMI_CODE_HOME`, its session locks and its process walk). Inside aterm, the harness claims the
session before it spawns Kimi, over the session's authenticated control socket, in this order:

1. `@self harness policy` must serve contract 4 or 3 with `effective=on`; the claim is made at
   the highest of the two it lists (alab-wire chooses). The reply is read strictly: `gen=`,
   `contract=`, `effective=`, `relaunch=`, `upgrade=`, `upgrade_ask=` and `human_grace_s=` must
   each be there once and in their grammar; nothing is defaulted.
2. **The roster guard, under contract 3.** A read-only `@self native-act info` must name
   `kimi-alab:upgrade` in its `own-image=` list (aterm's `ROSTER`). Without the row nothing is
   claimed, not even to find out: aterm stores a contract-3 claim from a holder off its roster as
   a foreign claim that takes every duty from its built-in engine. Contract 4 has no roster to
   ask: aterm proves a holder by its registry row and answers one it cannot prove `ERR unproven`.
3. The claim: `kimi-alab@<pid> contract=<4|3> scope=relaunch,upgrade version=<version>
   [vendor=<vendor version>] [child=<pid>:<start>] [login=<label>]` (`child=` under contract 4
   only). A contract-4 reply states the grant, `OK scope=<granted> revision=<n>`.
4. `status` must read it back as `supervisor=kimi-alab@<pid> supervisor_proven=1` with
   `supervisor_scope` naming every duty granted (under contract 3, both duties claimed). A claim
   that reads otherwise is released (`meta unset supervisor`). A launch whose contract-4 grant is
   narrower than both duties releases it too.
5. `subscribe @self events` on a second connection. aterm acknowledges with `OK subscribe 1` and
   `sub <local id> <sid> role=… attention=… user_title=…`, and its events name the local id
   (`EVENT <local id> restart cause=…`); a line for any other id ends the stream, which is
   reopened on the 5 s clock.

After every child start, the first included, the claim is sent again naming the new child's pid
and kernel start time and its build and account; under contract 4 its reply is the grant from
then on.

A noninteractive launch, `--alab-no-update`, `ATERM_NO_HARNESS=1`, a missing or older control
socket, a policy with supervision off by configuration, a host whose roster lacks the row (under
contract 3), or a claim that is refused, unproven or granted less than both duties makes the
harness **exec the vendor in its place** instead. Kimi then runs under its own name with no
second supervisor. If aterm is there but cannot say now (an `ERR` that asks to be retried, which
it answers at once while it shows a dialog or its main thread is stalled; a reply later than its
bound after the handshake, which is aterm busy, not gone; a policy off for a reason that passes),
or serves no contract the harness can claim at yet, the child runs supervised and the whole claim
sequence is tried again every 5 s. The same 5 s clock claims a lost claim again, and reopens a
lost event stream, whatever else the harness is doing; each of those exchanges waits for aterm
at most 500 ms. `kimi-alab --alab-which` names the scopes it will claim.

**Crash relaunch.** While Kimi runs, the harness follows the session id in its lock. When the
child ends abnormally (a non-zero exit other than 129, 130, 131 and 143, or death by a signal
other than `INT`, `QUIT`, `TERM` and `HUP`) and nobody passed the harness `HUP`/`TERM`, the
crash is answered under aterm's own rule (aterm `crates/aterm-agent/src/harness/relaunch.rs`,
ported in alab-wire's `relaunch` module):

* The terminal's modes are put back at once, with the keyboard, mouse and paste reporting reset
  on both screens, so Ctrl-C reaches the harness while no child runs.
* **The back-off.** The relaunch waits 1 s after the first crash, 10 s after the second, 60 s
  after the third and 10 minutes after every one after. A child that runs 10 minutes after its
  last crash relaunch starts the ladder over. The streak crosses a harness re-exec.
* **Whose exit it was.** One fresh `harness policy` and `status` read at the exit, and another
  at the end of the pause, must show this harness's proven claim taking `relaunch`, the policy
  in effect (a halt reads `effective=off why=hold`), `[harness] relaunch = true`, nobody typing
  within `human_grace_s`, `hold=0`, and no other driver's hand (`hand=lease:…` or
  `hand=turn:<id>:<holder>`). A fact `status` does not state holds. Anything else passes the
  crash on: the harness exits with the child's status and journals `CRASH-PASSED-ON <why>`.
* **aterm away.** If the claim's connection fails, or aterm answers `ERR`, the crash waits: aterm
  is asked again every second once the link is held, for at most 45 s of time actually spent
  waiting for this crash, and the crash is then passed on (`claim-not-proven-again`).
* `INT`, `QUIT`, `HUP` or `TERM` during the pause or the wait ends the harness with the crashed
  child's status, including one caught just before the relaunch would start. What was typed at
  the dead screen is discarded before the relaunch and before a crash is passed on, so neither
  the resumed Kimi nor a parent shell reads it.
* The relaunch resumes the session with the carried flags and `--session <id>` on the same build
  when it is still usable.

**Restarts aterm asks for.** aterm's explicit, stall and account restart acts publish
`restart cause=<why>` on the session's event stream for a proven relaunch claimant instead of
signalling its process group. The harness checks one fresh policy and status read: its proven
claim taking `relaunch`, the policy in effect, `hold=0` and no other driver's hand. The person's
grace is not read (whoever asked is the person), and `[harness] relaunch = false`, which limits
crash relaunches, does not limit an asked restart, as in aterm. A restart that cannot be made yet
(aterm answered `ERR`, the link was lost, or the child's lock names no session) waits at most
45 s from its arrival, whatever it waits for, and is then refused (`RESTART-REFUSED cause=… not
made within 45000 ms: …`). A restart that arrives before a new child is announced (as the last
one ends, or while the next starts) was meant for the child that ended and is passed over
(`RESTART-PASSED-OVER`), as is one that still waited when that child ended. The harness sends
Kimi `SIGTERM`, and `SIGKILL` if
it is still running 5 s later for a `stall` restart or 30 s later otherwise
(`KIMI_ALAB_TERM_GRACE_MS` bounds both), as aterm's own act does. When the child has ended by
either signal, the session is resumed with the carried flags and `--session <id>`, on the build
atpkg has current at that moment, else the same build. A restart is no crash and does not move
the back-off.

aterm's account pool can also publish `restart cause=account[-ahead] to=<label> home=<pct>`.
The harness applies the selected `KIMI_CODE_HOME`, removes `KIMI_MODEL_NAME`,
`KIMI_MODEL_API_KEY` and `KIMI_CODE_CUSTOM_HEADERS`, and resumes the same session on that
login. Pool overlay homes share sessions, locks and folder trust with the default home, so no
conversation data is copied and a credential renewal can resume in place. The selected account
stays on later crash and upgrade relaunches. Each successful child start re-announces
`login=<label>` in the scoped claim, which is how aterm distinguishes an applied account move
from an event merely queued. A lost-login renewal uses the same handoff; no Claude-style `/login`
dialog is driven.

**A halt holds.** Under a standing claim, an update a person attested with `SIGUSR1` still needs
aterm to read no halt, no other driver's hand and the upgrade duty still granted to this harness
at the stop point; aterm's own attestation needs its whole safe point. A halt never releases the claim: the move waits and goes ahead under the
same claim once the halt lifts.

## Build and test

Rust here is the Trust toolchain (`rust-toolchain.toml`: `channel = "trust"`).

The aterm client is `alab-wire`, built by path from the copy of alab-harness `v0.1.0` this
repository carries at `vendor/alab-harness/` (`vendor/alab-harness/crates/alab-wire`), pinned
by that tag's git tree id in `vendor/alab-harness.pin` (decision 37). A clone builds alone: no
sibling checkout and no git dependency. The copy is its own workspace (`exclude = ["vendor"]`)
and is never edited here. alab-harness's `tools/pin <this checkout> <tag>` replaces it whole,
then `git add -f vendor/alab-harness vendor/alab-harness.pin` and a commit take it (`-f`: the
copy is whole even where this repository ignores a name), and its `tools/check-pin <this
checkout>` is the release check (copy, pin and tag the same tree). To move to a newer
alab-harness, pin its tag the same way.

```sh
targo --unverified build --release        # target/release/kimi-alab
targo --unverified test --locked           # unit + integration tests
targo tippy --locked --all-targets -- -D warnings
targo fmt --check
```

`tests/pin.rs` holds the half of that check a checkout alone can: the committed
`vendor/alab-harness` is the tree the pin names, and the working copy changes none of it, an
ignored stray file included.

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

The aterm scenarios run the real harness binary against alab-wire's fake aterm (feature `fake`):
a Unix socket that answers as aterm does, with the strict policy line serving contracts 2, 3 and
4, `native-act info` with a roster, contract 4's grant and contract 3's bare `OK`, the claim read
back with its scope, the subscription acknowledged with a local id, and restart events. A test
can serve only contracts 2 and 3, take the row off the roster, grant fewer words, cap the claim
between its reply and its read, halt the session, put a driver's hand or a person's typing on
it, turn `relaunch` off, delay every reply, or make policy reads answer `ERR`. The scenarios
cover the claim at contract 4 and at contract 3 with each child named after it starts, the
roster and scope refusals under both contracts, a launch that meets an `ERR` and one aterm
answers late (busy, not gone), the move under a claim and under a halt, crash relaunch with the
back-off ladder, a crash a person owns, a crash waiting out aterm's `ERR` and a signal during
that wait, and the restart events (asked, account, a model move refused, refused under a
driver's hand, made under `relaunch = false`, and a deaf child escalated to `SIGKILL`). How the
wire, the claim, the link and the ladder behave alone is alab-wire's own tests' to show,
events for another subscription's id among them.

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
* alab-harness `crates/alab-wire` (README Decisions, `docs/HX-06-aterm-map-2026-11-13.md`): the
  aterm client, contracts 3 and 4, and the exit judgement and crash ladder ported from aterm.
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
   claim (contract 4 or 3), or an idle attestation (`SIGUSR1`) elsewhere, plus the harness's tty,
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
9. **A move or restart goes on only after the harness's own signal ended the child.** If the
   child ends any other way during a move (a crash, an exit code other than 143), or
   `HUP`/`TERM` reached the harness, that ending is the session's real end. The harness exits
   with it and prints `kimi -r <id>`. For a restart aterm asked for, the harness's own `SIGKILL`
   (decision 28) is such a signal too. A crash outside a move is answered by decision 24.
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
    backstops still apply. Any refusal at launch execs the vendor, and a failed connection holds
    the move, never leaving a second supervisor of the same duty. The typing scopes remain
    unclaimed until their own implementations and tests land. Revised by decision 25.
24. **Crash relaunch is aterm's own rule, with aterm's back-off (2026-11-13; aterm
    `crates/aterm-agent/src/harness/relaunch.rs`, `on_exit`, `BACKOFF`, `HEALTHY`).** Only an
    abnormal end nobody signalled qualifies, and only with the session id most recently proven by
    the child's lock. It waits 1 s, 10 s, 60 s, then 600 s for each crash in a streak, and a
    10-minute run forgives the streak. A fresh policy and status read at the exit and again at the
    end of the pause must show the proven claim taking `relaunch`, the policy in effect, the
    switch on, and nobody else's exit: a person who typed within `human_grace_s`, a halt, another
    driver's hand, or a fact `status` does not state each pass the crash on. Before, one crash
    was relaunched at once and a second within 15 s ended the session, so a child that crashed
    every 16 s was relaunched forever with no pause, and an exit just after a person typed (or
    under a halt or a driver's lease) was brought back over them. alab-wire's `relaunch` module
    (formerly `src/relaunch.rs`, decision 33) keeps aterm's order with three stricter differences,
    named in its module text.

25. **The roster guard, and the scope read back (2026-11-13; aterm
    `crates/aterm-agent/src/harness/contract.rs` `ROSTER`, `crates/aterm-gui/src/native_act.rs`
    `own_image_rows`).** The harness asks `native-act info` before it claims and claims only when
    `own-image=` names `kimi-alab:upgrade`; a proven claim must also read back with
    `supervisor_scope` naming `relaunch` and `upgrade`, or it is released. Before, the claim was
    sent straight after the policy read: on an aterm serving contract 3 without the row, it was
    stored as a foreign claim taking every duty, and nothing checked which duties a proven claim
    took. The policy reply is read strictly (no field defaulted), so a host that does not say
    whether it permits a duty has not permitted it.
26. **An `ERR` from aterm is "cannot say now", not "no" (2026-11-13).** aterm answers a policy or
    status read with `ERR` at once while it shows a dialog or its main thread is stalled
    (`crates/aterm-gui/src/control_media.rs` `main_thread_refusal`). At launch that keeps the
    child supervised and the claim is tried again every 5 s; for a crash or an asked restart it is
    waited for like a lost connection. Only an answer that was read and refuses passes a crash on.
27. **The wait for aterm is a budget, not a deadline (2026-11-13).** One crash waits for aterm at
    most 45 s (aterm's own grace for a claim whose connection went away), counted only while it
    waits, so a second loss at the end of a 60 s or 600 s pause still gets what is left instead of
    nothing.
28. **An asked restart is made whatever the child does with its `SIGTERM` (2026-11-13; aterm
    `crates/aterm-gui/src/restart_act.rs` `escalate`).** Under a `relaunch` claim aterm hands its
    restart act to the holder and arms no `SIGKILL` of its own, so the harness escalates as the
    act would: 5 s for a `stall` restart (a stalled program reads nothing, so it cannot take the
    signal), 30 s otherwise. Before, a deaf child kept running and the restart was abandoned.
    The update move keeps its single `SIGTERM` (decision 22): a busy `kimi` is never stopped.
29. **An asked restart refuses a held session; `relaunch = false` does not limit it (2026-11-13;
    aterm `crates/aterm-gui/src/input_stall.rs` `other_hand`, `harness_host.rs`).** aterm's own
    restart act refuses `halted`, `driver-turn` and `driver-lease` whoever asks, and skips that
    check for a session whose claim takes `relaunch`, so the harness checks `hold=` and `hand=`
    itself. aterm's host relaunches an asked exit whatever the switch, so the harness does too.
    A halt (`hold=1`, which the policy reports as `effective=off why=hold`) holds every act and
    never releases the claim.
30. **No child, no keyboard taken (2026-11-13).** When the child dies, the terminal's saved modes
    and plain keyboard reporting come back at once, before any wait, and `INT`, `QUIT`, `HUP` and
    `TERM` end the harness with the crashed child's status for as long as no child runs, a signal
    caught just before the relaunch included. Keys typed at the dead screen are discarded, so a
    resumed Kimi never receives them as a prompt and a parent shell never runs them.
31. **aterm's silence never holds the loop (2026-11-13).** The launch opens its connections and
    makes its first claim with 2 s per socket operation; every exchange after that, a claim made
    again from the loop included, has 500 ms (claude-code-alab's bounds). Before, one silent
    aterm held the loop that reaps the child and passes on its signals for 5 s per operation.
    An exchange that outlives its bound drops the connection, so the claim is made again later.
    A launch whose first claim aterm answers the handshake of and then answers late is aterm
    busy, not away (aterm answers through its main thread and says `ERR` only after a 5 s
    stall, never while it launches): the child runs supervised and is claimed again on the 5 s
    clock, as for an `ERR`, instead of running unsupervised for the whole session.
32. **Time stopped is not grace (2026-11-13).** The grace after the harness's own `SIGTERM` or
    `SIGKILL` runs on a clock that stops while the machine sleeps, and starts again whenever the
    child is continued, since a stopped child acts on no signal but `SIGKILL`: when the harness
    continues a child that stopped alone, and when a whole-job `^Z` stopped the harness with it
    and the shell's `fg` continued both (the kernel then reports only the continue). Before, a child stopped past the grace was killed or its move abandoned the moment
    it was continued.
33. **The aterm client is the shared library `alab-wire` (2026-11-14; aterm tracker HX-09).**
    `src/link.rs`, `src/relaunch.rs` and all but kimi-alab's own part of `src/aterm.rs` are gone:
    the wire, the claim and its guards, the link held, lost and claimed again, the exit judgement,
    the crash ladder and the 45 s budget are alab-wire's, and their unit tests with them. The
    integration tests run the harness against alab-wire's fake aterm instead of their own. What
    stays here is this harness's: its config (`kimi-alab`, `kimi`, `relaunch,upgrade`),
    `ATERM_NO_HARNESS`, the clock the link runs on (decision 35), the claim's child (`child=<pid>:<start>` read with
    alab-wire's `ChildProcess::of`), the check of a move a person attested by hand, the journal
    events made from the link's notes (`aterm claimed …` is `CLAIMED …`), and everything Kimi
    Code needs. alab-wire is built from the vendored copy of alab-harness `v0.1.0`, pinned by
    its tree id (decision 37); the port first took it by path from a sibling checkout. The
    decisions above stand, carried by alab-wire: busy is not gone after the handshake (31), the
    2 s and 500 ms bounds (31), the roster guard before a contract-3 claim (25), the scope read
    back (25), the crash back-off and its budget (24, 27). The grace restarted on a continue
    (32) is the supervisor's and stays here.
34. **What changed with alab-wire (2026-11-14).** Each follows alab-wire's decisions, recorded
    there.
    * **Contract 4 is preferred.** The claim is made at the highest contract aterm's policy lists
      (4, else 3). Under contract 4 there is no roster guard (aterm proves the holder by its
      registry row and answers an unproven one `ERR unproven`), the reply states the grant, and
      every child start is announced with `child=<pid>:<start>`. A launch granted less than both
      duties releases the claim and passes the vendor through, as decision 25 does under
      contract 3; a cap that narrows a standing claim later only narrows what the harness does,
      since every relaunch and safe point is judged on `status`'s scope.
    * **Only an `ERR` that asks to be retried is "cannot say now".** aterm's dialog and stall
      refusals end in `retry`; any other `ERR` (an unknown session, a refused principal) is a
      refusal. Decision 26 read every `ERR` as "cannot say now".
    * **A launch not yet claimed has no duty to a crash.** A launch whose claim aterm could not
      make yet runs its child supervised and claims on the clock; a crash before that claim is
      made is passed on (`no-claim`), since nothing took the relaunch duty from aterm's engine.
      Before, it waited up to 45 s for a claim. A child adopted across a harness re-exec keeps its
      claim through any refusal (alab-wire `Link::adopt_with`): the earlier image's duty passes on.
    * **A restart naming a model is refused** (`RESTART-REFUSED cause=… model=…`): Kimi Code
      would be started on the model it ran, answering a model move with the model it moved from.
      Choosing Kimi's model on a resume is not built yet. Before, the restart's `model=` made the
      whole event unreadable and it was ignored.
    * **Journal wording.** Link lines are the link's own (`CLAIMED holder=… contract=4
      scope=relaunch,upgrade revision=… vendor=…`, `UNCLAIMED <why>`, `LOST <why>`,
      `REANNOUNCED …`); refusals quote aterm's words journal-safe (`hand=lease:driver%25404`);
      a move a person attested under a halt is refused with aterm's hold (`wait aterm policy
      effective=off why=hold`).
35. **A person's attestation needs the upgrade duty, and the link's clock stops in sleep
    (2026-11-14; review of the HX-09 port).**
    * **The hand path is judged on `status`'s scope.** Under contract 4 a claim can stand with
      less than both duties: a person's cap narrows it, a reclaim after a lost link accepts a
      narrower grant, and an adopted child keeps whatever grant it gets. A claim without
      `upgrade` has handed that duty back to aterm's engine, so an update a person attests with
      `SIGUSR1` under it is refused (`REFUSED aterm reads supervisor_scope=relaunch; …`), as
      aterm's own attestation already was by alab-wire's safe-point judge. Under contract 3 such
      a claim was always given back, so the old client never met it.
    * **The link's clock is the one `Instant` reads:** `CLOCK_UPTIME_RAW` on macOS,
      `CLOCK_MONOTONIC` on Linux, both stopped while the machine sleeps. The port first read
      `CLOCK_MONOTONIC` on macOS too, which counts sleep: a closed lid spent the 45 s budget a
      crash or a deferred restart waits for aterm (passing the crash on without asking aterm
      again) and counted as the 600 s healthy run that resets the crash ladder. It is a system
      clock, not one the process starts, so it reads the same on both sides of a re-exec;
      the crash streak crosses one as a `Duration` (`Ladder::since`/`carried`) either way.

36. **Three fixes carried from the review of agy-alab's port (2026-11-14; agy-alab 20be436,
    alab-harness 9e8bc55).** kimi-alab's port shared each defect.
    * **A restart aterm asked for waits at most 45 s from its arrival** (`Settings::restart_wait`,
      alab-wire's `AWAY_BUDGET`; `KIMI_ALAB_RESTART_WAIT_MS` for the tests), whatever it waits
      for, and is then refused (`RESTART-REFUSED cause=… not made within … ms: <why>`). Before,
      a wait for the child's session lock never spent the away budget, so a restart sent to a
      fresh Kimi was made the moment the person's first prompt opened a session, however much
      later, ending Kimi mid-turn for a cause long gone. A restart is for the moment aterm asked
      in; aterm asks again while its cause stands. The total bound replaces the away budget the
      deferred restart carried, which it subsumes; a crash keeps its away budget (decision 27).
    * **A restart that arrives before a new child is announced is passed over**
      (`pass_over_earlier`, before every announcement: the first start, a crash relaunch, a
      move's or restart's relaunch, a startup fallback). The loop relaunches as soon as it reaps
      the child, so an event read in the same wake as the end, or one sent while the next child
      was being started, was drained on the next pass and applied to the new child; aterm sent it
      for the child that ended (`Link::events`). A restart still waiting when a child is replaced
      by a move is passed over too: it waited for the child before.
    * **A move a person attested is judged by alab-wire** (`claim::judge_own_move` with
      `Warrant::Person`), replacing the hand-written `unheld`: the claim still this harness's and
      proven, `supervisor_scope` with `upgrade`, no halt, and no other driver's hand. The person's
      word stands over the policy's `upgrade` and `upgrade_ask` switches. The upgrade-duty check
      decision 35 added is alab-wire's scope check, made under either warrant, so it stands
      (`a_hand_attested_move_needs_the_upgrade_duty_granted`, unit and end to end). Journal words
      are alab-wire's `aterm <field>=<value>`: `REFUSED aterm supervisor_scope=relaunch; …` where
      decision 35 quoted `aterm reads supervisor_scope=relaunch`, and a lost claim is named before
      a halt.
37. **alab-wire is the vendored alab-harness `v0.1.0`, pinned by tree id (2026-11-14; alab-harness
    Decision 19, aterm tracker HX-08/HX-09).** alab-harness tagged `v0.1.0` before HX-06 lands (its
    Decision 19): the link already claims at contract 3 where aterm serves only that, so a pinned
    harness runs on the aterm that is installed, and what HX-06's measurement finds ships as
    `v0.1.x`. It is not a git dependency, because aterm's `deny.toml` refuses git sources.
    alab-harness's `tools/pin` copies the tag's tree byte for byte to `vendor/alab-harness/` and
    records its git tree id in `vendor/alab-harness.pin` (`tag=v0.1.0`,
    `tree=f3b718df6a78a54397c48f2db0c164448741dbcb`); both `alab-wire` entries in `Cargo.toml` name
    `vendor/alab-harness/crates/alab-wire`. A tree id is equal exactly when every path, mode and
    byte is, so one comparison holds the whole copy: `tests/pin.rs` holds the committed copy to the
    pin, and alab-harness's `tools/check-pin`, the release check, also holds the pin to the tag. The
    copy is excluded from this workspace, ships in the public snapshot (`publish/manifest.txt` lists
    `vendor`) so a public clone builds alone, and is never edited here: a defect in alab-wire is
    fixed in alab-harness, tagged `v0.1.x` and taken by pinning again. Before, `Cargo.toml` named
    `../alab-harness/crates/alab-wire`, so no clone built without that checkout beside it, and the
    plan was a git tag dependency on `v0.1.0` once alab-harness had a remote (decision 33).
    * **The workspace lists no `members = ["."]`,** unlike the example in alab-harness's README.
      Cargo reads an explicit member `.` as every path under the root, `vendor/` included, and an
      explicit member overrides `exclude`, so it took the vendored crate for a member of this
      workspace and failed to inherit `workspace.package.publish`. With the root package its only,
      implicit, member, `exclude = ["vendor"]` holds.
    * **The tag carries four alab-wire changes made after decision 36 was tested** (alab-harness
      9e8bc55): a release aterm will not take ends the connection (c6db9af), a test build may
      shorten the away budget (97aff86), the policy is read on the clock while the event stream is
      down (d64ada4), and placeholder homes in two test fixtures (de80de5). This harness's tests
      pass against them unchanged.

## Verification status

First measured 2026-10-24 on a clean clone of commit bd8547a, with the gate exactly as
`publish/config.sh` spells it, run step by step as the publication engine runs it (rustup `trust`
toolchain, targo 0.10.0+g41d799797). The advisory Trust check, unverified tests, lint and
formatting were rerun on this change on 2026-11-13, with targo straight from the atpkg store
(this Mac has no rustup, so `rustup run trust` is dropped from each step). The unverified tests,
lint and formatting were rerun with `--locked` on 2026-11-14 after the aterm client moved to
`alab-wire` (decision 33), again after decisions 35 and 36, and again with alab-wire built from
the vendored v0.1.0 (decision 37); the Trust rows were not rerun on those changes:

| Step | Result |
|---|---|
| `targo trust check --allow-l0-gaps` (verified, advisory lane) | exit 3, `Gate: COULD-NOT-RUN [advisory lane, exit 3]`. **0 refuted.** 652 functions: 255 rows proved, 416 runtime-checked, 498 unknown. |
| `targo --unverified test` (no proof claim) | exit 0: 72 unit, 53 integration and 1 pin test passed |
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
excused, and the tests that pin the fixed behaviour are in `src/proc.rs` and `src/store.rs`. The
first run on the 2026-11-13 supervision change refuted 35 more: array indexing by an unbounded
slot in the strict policy parser, and reads and writes through `Box`ed enum payloads, which the
verifier fails closed on. The parser now reads a keyed map, and the three enums keep their
payloads inline (clippy's `large_enum_variant` is allowed on each, with that reason).

**Linux.** The Linux code (the `/proc` process table and identity, the XDG journal path) was
type-checked and linted on 2026-11-14 for `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`, cross-checked from macOS with stable `cargo clippy --locked
--all-targets --target <target> -- -D warnings` (clippy pedantic, tests included): exit 0 on
both. It has never been run: no test above ran on Linux. The Linux arm of the link's clock
(decision 35, `CLOCK_MONOTONIC` through rustix, the reading the port already made) was not
cross-linted again: this Mac has no stable cargo, and the Trust toolchain has no Linux standard
library to check against.

## License

Licensed under either of Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT license
(`LICENSE-MIT`), at your option.
