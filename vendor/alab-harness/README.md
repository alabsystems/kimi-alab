# alab-harness

The half every ALab harness needs, shared. A harness is the program that runs
one vendor's agent CLI (Claude Code, Kimi, Grok, Muse, Codex) inside an aterm
tab and takes duties from aterm's built-in engine. Each harness used to carry
its own aterm client. This repository holds one copy of that client, the
policy deciders aterm's supervisor used to keep to itself, and the account
code that comes next.

Remote: none yet. The sibling harness repositories are private, and the tool
that creates repositories here can only make public ones, so this repository
lives on one machine until a private remote can be made.

## Crates

| Crate | State | What it gives |
|---|---|---|
| `alab-wire` | here since v0.1.0; the record vocabulary since v0.3.0; the stream client and `run` since v0.4.0; the re-exec since v0.6.0; the declared contracts since v0.7.0 | an aterm client: wire, the contract-4 and contract-3 claim under the contracts a harness declares and its grant, link, exit judgement, crash ladder, fake aterm serving contracts 3 and 4 and the stream; the session stream's records, their codec and its client; the relaunch grammar; the harness's re-exec of itself |
| `alab-policy` | here since v0.2.0, in part; sleeps, git-config and shell startup since v0.5.0 | approval classification, the back-off ladder, NEEDS YOU and credential needs, the go-ahead reader, the Land check, the Cruise carry, the press guard, the machine's sleeps inside a turn's done row, the git-config hazard check and Claude Code's shell startup; the turn-end readers and the done check to come |
| `alab-accounts` | to come | the account driver's arguments and JSON types, and the test every driver passes |

### alab-wire

It starts from claude-code-alab's aterm client, which was measured against a
real aterm on 2026-11-13. It keeps kimi-alab's rule that an aterm which
answered the handshake but answers a later read late is busy, not gone. Only
lateness is busy: an unreadable reply, or an event subscription aterm refuses,
ends a launch unclaimed, as claude-code-alab's client did.

- `wire`: finding the session (`Locator`, from the variables the config names),
  the socket and its token. The direct connection (`Connection`) is
  close-on-exec, has one deadline per exchange, and is poisoned by any failed
  exchange. Also the field readers and the event subscription (`EventReader`),
  which needs aterm's real ACK, `OK subscribe 1` and then
  `sub <local> <sid> role=- attention=- user_title=-`.
- `claim`: the claim texts of contracts 3 and 4 and their guards. The
  contract is the highest of those the harness declares that `harness
  policy`'s `contract=` set lists (`Contract::choose`; Decision 33). A
  harness declares 3 and 4 unless `Config::with_contracts` names fewer
  (`Config::contracts`); its record's `contracts = [4]` never claims 3, and
  `claim_verb` makes no text at an undeclared contract. A contract-4 claim
  names every scope word the harness asks for, `version=`, and when known
  `vendor=`, `child=<pid>:<start>` (`ChildProcess`) and `login=`; its reply
  `OK scope=<granted> revision=<n>` is read into a `Grant`, which may be
  narrower than the claim. A contract-3 claim names only `relaunch` and
  `upgrade` of what the harness asks for, and its grant is read back from
  `status`. The guards are the read-only roster check before a contract-3
  claim (`native-act info` must list `own-image=<id>:upgrade`), the policy
  reader and the release. Also the
  `status` reader, the `restart cause=` event with an account move's `to=` and
  `home=` (`Restart::account`) and a model move's `model=` (`Restart::model`),
  the upgrade safe-point judge, and the judge of a move a harness weighed by
  its own readings (`judge_own_move`, under a `Warrant`).
- `kernel`: a child's kernel start time as aterm checks `child=`
  (`kernel::start_us`, `ChildProcess::of`): exact on macOS, within about one
  second on Linux.
- `link`: the claim sequence and the `Link` state machine. The sequence is
  policy, roster (contract 3 only), claim at the chosen contract, then
  reading the proof and every granted scope word back from `status`, then
  subscribing. `Link` covers held, lost and absent. The harness acts only on
  `Link::grant` (or `Link::takes`). A `harness-policy` event on a standing
  claim reads `status`, and the held grant and its revision follow what it
  shows of the words the claim asked for (`aterm narrowed scope=…
  revision=…` when a person's cap withdrew a word). After every child
  start the harness calls `Link::reannounce` with the new child, which sends
  the claim again with the new `child=`; its reply is the grant from then on.
  `Link::safe_point` judges an upgrade by aterm's rule; `Link::read` hands a
  harness the fresh policy and status pair to judge a move by its own (one a
  person asked for under `upgrade_ask=true`).
  Connecting may take 2 s and each request 500 ms. A lost claim is claimed
  again every 5 s, and an event stream that ended is reopened every 5 s, the
  policy read at each attempt while it stays down (Decision 18). Every claim
  (launch, adoption, reclaim, a policy switched back on) keeps to the
  config's declared contracts; a policy listing none of them is
  `Failure::Unserved`, read again every 5 s, and `Link::unclaimed` gives a
  lost link's last answer so the caller can pass its vendor through instead
  (Decision 33). A harness image that took over a running child from an
  earlier image of itself (a re-exec that kept the child, `reexec`) starts
  with `Link::adopt_with` (Decision 14).
  From v0.4.0 a contract-4 grant opens the session stream (Decisions 22 to
  28, 30 and 31): `stream @<sid> v=1` on the claim's connection, which
  carries records from its `OK`, a second request connection for every
  read, the release and `Link::request`, and the subscription kept.
  `Link::send` sends the harness's records, paced under aterm's budget,
  kept until aterm acknowledges them and resent from `acked=` under their
  numbers when a stream opens again (`Drained::fresh` and
  `Drained::renumbered` say when aterm forgot them); a `beat` goes every
  10 s of `now_ms` (`Link::set_beating` pauses it); `Link::reannounce`
  sends a `claim` record. Every record aterm sends is answered `took` or
  `refused` once: a `restart` is returned as `Drained::restart`, a `hand` as
  `Drained::hand` and kept in `Link::status`, a `policy` as
  `Drained::policy` and followed by a fresh grant read, and `want` and the
  kinds the caller's `session` takes as `Drained::incoming`, which the
  caller answers with `Link::answer`. `ERR busy streams` keeps everything
  above as it was and asks again 5 s later; any other reply but `OK` keeps
  the v0.1 path and asks again on the next claim's connection.
  `Link::stream_fd` is the stream's descriptor and `Link::due_ms` the time
  of the link's next step, for the harness's wait. `Config::with_stream(false)`
  keeps a contract-4 link on the v0.1 path.
- `record` (v0.3.0): the session stream's records, the vocabulary a harness
  and aterm send each other once a contract-4 claim opens a stream:
  `Record { seq, ms, body }`, a `Body` for each kind in the design's two
  tables and `Unknown { k }`, `Kind::needs` (the scope table) and
  `Body::admitted`, `stream_request` (`stream @<sid> v=1`) and
  `StreamReply` (`OK stream v=1 epoch= acked= in=`), and the stream's
  numbers (`LINE_MAX`, `BEAT_EVERY_MS`, `RATE_PER_S` and the rest).
  `Body::admitted` takes aterm's act table for an act taken by a rule. Its
  codec is `Record::line` and `Record::read`, a bounded JSON writer and
  reader written here (Decision 20). The module documentation lists every
  kind's fields. The `Link` opens the stream (v0.4.0).
- `stream` (v0.4.0): the stream's client half, which `Link` drives: the
  line transport on the claim's connection, the outbox that numbers the
  harness's records, writes them at `PACE_BURST` and `PACE_PER_S` and keeps
  them until aterm acknowledges them (`RESEND_MAX` records and
  `RESEND_BYTES_MAX` bytes of the caller's, `OWN_MAX` of the link's own),
  and what aterm's records become (`Received`, `Incoming`, `Hand`);
  `SendError`, `AnswerError` and `Counts`.
- `run` (v0.4.0): the words aterm types to relaunch a harness on its
  conversation, `run [--account <label>] [--home <dir>] [--model <word>]
  -- <resume…>` (`Run::parse`, `Run::args`), with the claim's `login=` and
  model-word rules (Decision 29).
- `reexec` (v0.6.0): the harness's re-exec of itself onto a newer build of
  itself, with the same pid and the same child (Decision 32). `probe` runs
  `<exe> --alab-reexec-probe`, which must exit 0 within the caller's bound
  having printed exactly the `Protocol` line (`alab-reexec 1 <id> <own>`);
  only that answer gives a `Proved`, the one thing `exec` takes, and its file
  must be unchanged at the exec. `Identity::of` reads the child's kernel
  identity when it starts (pid, parent, group, session, birth and the running
  image's file) and `Identity::current` again before the exec. `block` holds
  the caught signals on the thread that execs; `exec` writes the `Carry` into
  a pipe whose read end is the one descriptor the new image inherits and runs
  `<exe> --alab-reexec <fd>`, and returns only when it did not happen
  (`Held`), a pending `SIGHUP` or `SIGTERM` among the reasons. The new image's
  `receive` reads the carry, refuses one another process wrote or whose child
  is not its own, and says how the child stands (`Alive`, an ended one the new
  image's to reap); `take_pending` hands it the signals that crossed, and
  notes each caught one of `UNKEPT` (`SIGCHLD`, `SIGWINCH`, …) whether or not
  it is pending, because macOS discards those between the exec and the new
  image's handlers. It claims again under the same pid with
  `Link::adopt_with(config, carry.pid,
  carry.claim()?, now_ms, lookup)`. The carry holds the child, the claim's
  `vendor=` and account, the crash ladder and whether the old image was
  linked; anything else is the harness's own `own` lines.
- `relaunch`: aterm's exit judgement (`on_exit`, `permitted`, `abnormal_exit`,
  and `driver_hand` for a harness judging a move a person attested by its own
  rule) and the crash ladder (1 s, 10 s, 60 s, then 10 min, forgiven after a
  healthy 10 min run), which a harness that replaces its own image carries
  across the `exec` (`Ladder::since`, `Ladder::carried`). Also the 45 s budget
  for waiting on aterm.
- `fake` (cargo feature `fake`): a fake aterm a harness can use in its own
  tests. It serves the uncontracted claim and contracts 3 and 4
  (`contract=2,3,4`); `FakeAterm::serve_contracts(&[3])` makes it an aterm
  without contract 4. Its roster starts empty; `FakeAterm::for_harness` adds
  the harness's row allowing relaunch and upgrade, as every row of aterm's
  does, and a rostered holder's contract-3 claim naming any other scope word
  is refused as aterm refuses it. A contract-4 claim drops a word the fake
  does not know (`FakeAterm::set_known`), is answered with its grant and the
  claim's revision, and keeps its `child=` for a test to read
  (`FakeAterm::child`). `FakeAterm::set_cap` is a person's cap: it narrows a
  live contract-3 or contract-4 claim, raises its revision and sends
  `harness-policy`. `FakeAterm::set_registered(false)` makes a contract-4
  holder unregistered, answered `ERR unproven` with nothing stored, and
  `FakeAterm::send_model_restart` sends `restart cause=<why> model=<word>`.
  `FakeAterm::end_events` ends every event stream while the claim and the
  request connections stay. From v0.4.0, `FakeAterm::serve_stream(true)`
  serves `stream @<sid> v=1` on the connection that holds a contract-4
  claim (it answers `ERR unknown verb`, an older aterm's answer, until
  then): an `ack` for each record with `dropped=` `applied`, `bound`,
  `rate`, `scope` or `refused:<word>`, a gap that closes the stream, a
  `claim` record placed as a claim, the epoch and `acked=` and `in=` kept
  per claimant until `FakeAterm::restart`, `hand` and `policy` on every
  open, `policy` on every policy move, `restart` beside every restart
  event, and the `took` and `refused` it reads back. Its knobs are the
  reply to `stream`, the rate budget, a drop for the next record, a deaf
  stream, whether it sends records of its own and applies `claim` records
  (an HX-15b1 alone), the re-exec grace and its end, closing the stream,
  and any record or raw line to send (Decision 28).

It is generic. A harness passes one `Config`, which holds its id (for example
`kimi-alab`), its vendor word, the scope words it asks for (any of the eight),
its version and the contracts it declares (`Config::with_contracts`; 3 and 4
by default until HX-14b). Under contract 4 every word is claimed and aterm
grants what it knows and the person's cap allows. Under contract 3 only
`relaunch` and `upgrade` are claimed (`Scope::CONTRACT_3`), because aterm's
roster rows allow nothing else and refuse a whole claim naming another word. The
config can also name the environment variables to read in place of aterm's
defaults. The library names no harness of its own. It does no process
supervision: it never forks, execs, signals or reaps a vendor. The one program
it starts is a harness build's probe, and the one it execs is that build, in
the harness's own process (`reexec`). It reads no
clock (each call takes `now_ms`) and writes no journal (`Link::take_notes`
returns the lines to log). The caller's `now_ms` should stop while the
machine sleeps, as `std::time::Instant` does (`CLOCK_UPTIME_RAW` on macOS,
`CLOCK_MONOTONIC` on Linux; macOS's `CLOCK_MONOTONIC` and any epoch clock
count sleep): the 45 s away budget and the crash ladder's healthy run are
aterm's grace and back-off, which a closed lid neither spends nor serves.

```rust
use alab_wire::{Child, ChildProcess, Config, Link, Scope, SendError};
use alab_wire::record::{Body, State};

let config = Config::new("kimi-alab", "kimi", &[Scope::Relaunch, Scope::Upgrade, Scope::State], "0.4.0")?;
let child = Child { vendor: Some("2.5.1"), ..Child::default() };
let mut link = Link::launch(config, std::process::id(), child, now_ms());
// After every child start, name the new child by pid and kernel start time.
let started = Child { process: Some(ChildProcess::of(pid)?), ..child };
link.reannounce(started, now_ms());
if link.takes(Scope::State) {
    let state = Body::State { state: State::Working, composer: None };
    match link.send(state, wall_ms(), now_ms()) {
        Ok(_) | Err(SendError::Full) => {}
        Err(SendError::NoStream) => { /* the v0.1 path: meta set harness_* */ }
        Err(other) => journal.note(&other.to_string()),
    }
}
// Each pass: wait on link.event_fd() and link.stream_fd(), until link.due_ms(), then
link.tick(started, now_ms());
let drained = link.events(started, now_ms());
if drained.fresh { /* send the session record and the current state again */ }
for received in drained.incoming {
    let refused = harness.act_on(&received.incoming); // None, or Some(why)
    let _ = link.answer(received.of, refused.as_deref(), now_ms());
}
for line in link.take_notes() { journal.note(&line); }
```

### alab-policy

The deciders aterm's supervisor runs to choose what to type, approve or wait
for: whether a shell command only reads (`classify`), how long to back off
(`ladder`), what a turn's last message asks of a person (`needs`,
`credential_need`, `go_ahead`), whether Land may land on a reply's defense
(`land_check`), what Cruise types when a turn ends while the agent's own
background work runs (`cruise_carry`) and how an approving keystroke is bound
to the box that was judged (`guard`), with the word lists and text helpers
they share (`words`, `text`, `tab_id`, `hash`, `token`). Tracker package HX-21 moved them from aterm,
which links this crate in place of its copies, so aterm and every harness
give the same answers.

HX-21c added the pure halves of the deciders that judge what aterm probes:
how much of the span a turn's done row counts the machine slept through
(`sleeps`), what a git read on a line would load and run (`git_config`), and
what Claude Code's Bash tool's shell starts with (`claude_code::shell_startup`,
whose `ShellStartup::assemble` takes every reading as a `StartupInputs`).
Claude Code's own facts sit in `claude_code`, and a decider asks an agent's
facts through the `port::Agent` trait each harness implements. aterm keeps the
live halves, which read files, start git or read the clock, and hands their
readings in.

A decider takes its readings as data and returns its answer. The crate has no
dependencies, no `unsafe` code and no input or output: its `clippy.toml`
refuses the std calls that reach files, processes, the environment, the
network, the clock, sleep and another thread's channel, which aterm's census
refused in these files before they moved. A caller passes what it read and
the time it means.

Each module is aterm's code at the revision in the newest `Moved-From:` or
`Synced-To:` commit trailer. A move is two commits. The first copies aterm's
code unchanged: a whole file keeps aterm's blob (its `git hash-object` equals
aterm's), and an item cut such as `words`, `git_config` or `claude_code`
repeats aterm's lines at the file and line ranges its copy commit names. The
second fits it: it changes `use` lines, visibility, docs and `cfg`s, and puts
in the seams the HX-21 design fixes, such as `cruise_carry::CarryTiming`,
`port::Agent` and `claude_code::shell_startup::StartupInputs`, each named in
its commit message. A whole file then resyncs by a three-way merge, and an
item cut item by item against the ranges its copy commit names.
`src/lib.rs` lists where each module came from. Feature `testing` opens the
items aterm's own tests read across the crate boundary, such as
`go_ahead::testing`'s table and corpora; turn it on only in
`[dev-dependencies]`.

## How a harness pins it

There is no git dependency, because aterm's `deny.toml` refuses git sources.
Each harness carries one tagged version of this repository in its own tree and
depends on it by path:

```sh
~/alab-harness/tools/pin ~/kimi-alab v0.2.0     # the tag's tree at vendor/alab-harness/, its tree id in vendor/alab-harness.pin
git -C ~/kimi-alab add -f vendor/alab-harness vendor/alab-harness.pin && git -C ~/kimi-alab commit
~/alab-harness/tools/check-pin ~/kimi-alab      # the release check: copy == pin == tag
```

```toml
# the harness's Cargo.toml
[workspace]
exclude = ["vendor"]          # the copy is its own workspace, never a member of the harness's

[dependencies]
alab-wire = { path = "vendor/alab-harness/crates/alab-wire" }

[dev-dependencies]
alab-wire = { path = "vendor/alab-harness/crates/alab-wire", features = ["fake"] }
```

The `[workspace]` table names no `members`, so the root package is its only
member. With an explicit `members = ["."]` beside the `exclude`, the copy's
crates fail to load (`error inheriting 'publish' from workspace root manifest's
'workspace.package.publish'`); the four ported harnesses found it.

A git tree id is equal exactly when every path, mode and byte is, so one
comparison holds the whole copy: `check-pin` compares the committed copy's tree
with the pin and the pin with the tag here, and refuses any change in the
working copy, an ignored stray file included. A harness also runs the pin's
first half as a test of its own (`tests/pin.rs`), which needs only its own
checkout. The harness's `publish/manifest.txt` lists `vendor`, so a public
clone builds alone. To move to a newer tag, run `tools/pin` again and commit.
aterm takes the same kind of copy at its own `vendor/alab-harness` once it
links a crate from here, checked by aterm-forge's provenance check rather than
a test (a submodule would need a URL every clone can reach). Like any harness
repository, it has dev, staging and release tiers.

## Still to come

- **The rest of `alab-policy`**: turn-end's vendor-neutral readers, the
  open-work reader and the done check among them (HX-21d).
- **`alab-accounts`**, as the table above says.
- **A remote.** The repository lives on the lab's Mac until its dev tier
  exists; a pin needs only the tag, so no harness waits on it.

## Decisions

Choices the specification of record (aterm's
`docs/DESIGN-harness-interface-2026-11-12.md`, sections 2.2 and 2.3 and
packages HX-06, HX-08 and HX-15) leaves open, made here with their reasons.
Revisit any of them when HX-06 lands in aterm and a real reply contradicts
it, and the stream's when aterm first answers `stream` (HX-15b1).

**Contract 4 (2026-11-13).**

1. **`<start>` is microseconds since the Unix epoch, a canonical decimal**
   (`ChildProcess { pid, start }`). The design says "kernel start time" and
   names no unit or spelling. A claim word cannot hold aterm's other start
   spelling (`lstart`, which has spaces), and microseconds tell apart two
   processes given one pid within a second. The library reads it as aterm
   checks it (revised 2026-11-13, `ChildProcess::of`, `kernel::start_us`): on
   macOS `proc_pidinfo` `PROC_PIDTBSDINFO`, `pbi_start_tvsec * 1e6 +
   pbi_start_tvusec`, exact; on Linux `btime * 1e6 + starttime * 1e6 / HZ`
   from `/proc/stat` and field 22 of `/proc/<pid>/stat`, `HZ` from
   `sysconf(_SC_CLK_TCK)`. The Linux value is whole seconds plus whole ticks
   and cannot match exactly, so aterm matches it within about one second (one
   second plus a tick). Before, each harness read the kernel itself; one
   reader in the library means each harness computes what aterm compares. It
   uses `libc` alone, and still forks, execs, signals and reaps nothing.
2. **The re-send of `child=` is the same `meta set supervisor` line**, at the
   claim's contract, through `Link::reannounce`, called after every child
   start. The design says the harness "sends it again" and names no verb;
   the claim line already replaces the same holder's claim, as the
   contract-3 `vendor=` announcement did. Its reply is the grant from then on,
   so a duty a lifted cap allows again comes back at the next child start.
3. **A changed grant is read from `status`.** `EVENT harness-policy` carries
   only `gen=`, and the design says to read again. On a policy event the link
   reads `status` and keeps `supervisor_scope` (the words of it the claim
   asked for) and `supervisor_revision`, which it takes to be the reply's
   `revision=` (both are the claim cell's). The grant follows `status` both
   ways (revised 2026-11-13): the design says a cap narrows live claims and
   does not say a lift leaves them narrowed, and aterm's engine stands down
   for every word `status` shows, so a grant that refused a word `status`
   shows would leave that duty to nobody. A `status` at an older revision
   than the grant's is passed over, one aterm cannot give now is read again
   after `LATER_RETRY_MS`, and a word the claim did not ask for is never
   taken. At launch, a contract-4 `status` at a newer revision than the
   claim's reply (a cap that landed between the two) is the grant, not a
   failed proof.
4. **A cap covers contract-3 claims too** (revised 2026-11-13). The design
   says a cap narrows "every live claim it covers", so the link follows
   `status` on policy events whatever the claim's contract, and the fake's
   cap narrows contract-3 claims as well.
5. **An empty grant stands.** `scope=-` (or `scope=`) grants no duty, and the
   claim is kept rather than released: it still names the child, which aterm
   uses to decide when its own engine may type.
6. **The roster guard is a contract-3 rule** (revised 2026-11-13, twice). Before a
   contract-3 claim the library asks `native-act info` for
   `own-image=<id>:upgrade`, since aterm stores a contract-3 claim from a
   holder off its roster as a foreign claim taking every duty. aterm builds
   that list from the compiled roster rows that allow `upgrade`, which
   contract 4 replaces with the registry row ("There is no allow list per
   harness", section 2.2; HX-11 reads the registry row "in place of
   native-act info's own-image= list"; HX-14 deletes the seed rows). A
   contract-4 claim is therefore made without it. aterm answers a contract-4
   holder no registry row proves `ERR unproven` and stores nothing (HX-06,
   `contract.rs` `read_in`), rather than storing it unproven; like any reply
   that is not `OK`, the link reads that as `Failure::Refused`, with nothing to
   release: a launch is absent, and a reclaim after a loss stays lost and asks
   again on the clock. The `status` read back still proves what aterm did
   store, and a claim it shows unproven is released at once. Kept
   under contract 4, the guard would keep agy-alab (no seed row) from ever
   claiming, and every harness once the seed rows go. When aterm serves a
   wire read of a harness's registry row, the contract-4 guard reads that.
7. **`Config` takes any of the eight words.** The contract is not known until
   a claim is made, so the config holds what the harness can do; contract 3
   names the part inside `relaunch,upgrade`, and a harness with none of those
   makes no contract-3 claim (`aterm unclaimed contract=3 scope=-`). That is
   `Failure::Unserved` (Decision 12), not a refusal (revised 2026-11-13): the
   policy is read again every 5 s, so after an aterm update that lists
   contract 4 the harness claims there (section 2.2: a harness process
   outlives an aterm update and claims again afterwards).
   `ConfigError::Scope`, `REQUIRED_CONTRACT` and `may_claim` are gone.
8. **A standing claim keeps its contract.** The contract is chosen at each
   claim (launch, a reclaim after a loss, a claim after a policy on again),
   not switched on a policy event: aterm drops a contract only across its own
   update, which ends the connection and so the claim.
9. **A contract-4 reply is read strictly where it matters.** `scope=` and
   `revision=` must each appear once and `revision=` must be a canonical
   decimal, or the claim is released (at a reannounce, the link is lost). A
   granted word this library does not know, or did not ask for, is passed
   over.
10. **Journal lines.** `aterm claimed holder=… contract=<n> scope=<granted>
    revision=<n> vendor=…`; `aterm narrowed scope=<granted> revision=<n>`
    (and `aterm widened …` or `aterm regranted …` when `status` gives words
    back);
    under contract 4, `aterm reannounced vendor=… child=<pid:start> scope=…
    revision=<n>`; `aterm events-lost <why>` and `aterm events-reopened` for
    the event stream; `aterm policy-owed <why>` when a standing claim's policy
    read begins to be owed (Decision 18).
11. **The fake.** It lists `contract=2,3,4` (the uncontracted claim is still
    served) and refuses a claim at a contract its policy does not list. Its
    contract-4 refusal texts are its own, since aterm's are not designed: it
    refuses a claim without `version=`, a `child=` that is not
    `<pid>:<start>` with a pid above 1, an empty or repeated scope word, and
    any other claim key. It consults no roster row under contract 4, and keeps
    any well-formed `child=` without a descent check. A cap shows in the
    policy as `scopes=` (section 2.4), moves the generation with a
    `harness-policy` event even when it narrows nothing, raises a revision
    only when it withholds a word, and gives a live claim nothing back when
    lifted unless `FakeAterm::set_cap_restores` says so. Three more knobs
    stage what the link must survive: `set_cap_after_next_claim` (a cap
    between a claim's reply and its `status` read), `set_unstored` (a
    reply granting a word the claim cell does not take) and
    `set_status_reply` (a `status` refusal). The uncontracted claim takes all
    eight words. `set_registered(false)` answers a contract-4 claim
    `ERR unproven: …` (the text after the colon is the fake's) and stores
    nothing; `set_proven(false)` still stores a claim that `status` shows
    unproven, for a proof aterm withdraws later.
12. **A policy listing no contract this library speaks is
    `Failure::Unserved`** (2026-11-13, revised the same day). An aterm serving
    contract 2 alone (or a `contract=` set without 3 and 4) used to end the
    launch for the session. An aterm update can serve 3 or 4 while the harness
    runs, and the design says a harness claims again after an update (section
    2.2), so the policy is read again every 5 s and the claim is made at the
    highest contract it then lists. A standing claim is kept through such a
    policy, as through a `Later` one, rather than released: aterm drops a
    contract only across its own update, which ends the connection and the
    claim with it. So is a contract-3 policy for a harness asking for no
    contract-3 duty (Decision 7). It is its own failure, not `Later`, for the
    lost link: a reclaim answered `Later` (a halt, a retry, a timeout) keeps
    the hold aterm stated before the loss (`Link::lost_hold`), since that
    aterm may still mean it, but an aterm that serves no contract this harness
    can claim at states no hold on the session, so `Unserved` drops it as a
    refusal does, and the harness's own readings decide. As `Later`, an aterm
    replaced across its restart by one serving contract 2 alone (a rollback)
    would have held the session's every move for the rest of its life.
    Since v0.7.0 the policy is read against the contracts the harness
    declares, not every one this library speaks (Decision 33).
13. **A restart names its model in `model=`** (2026-11-13). aterm sends
    `restart cause=model model=<word>` (HX-06; never the model in `to=`, which
    stays an account cause's), and may name a model beside any cause.
    `Restart::model` holds it, for the harness to start the next child on.
    A model word is what aterm's model identifier check accepts
    (`harness/upgrade.rs` `is_model_id`: 1 to 256 bytes of ASCII
    alphanumerics and `-_.:/@[]`, any first byte), except a word starting
    with `-`, which a vendor would read as an option (revised 2026-11-13: it
    was 128 bytes with an alphanumeric first, aterm's narrower `valid_model`,
    so a restart naming a longer model or one starting `.`, `_`, `@`, `:`, `/`
    or `[` was read as no restart and aterm's stall remedy would have killed
    the harness). Every `valid_model` word passes too. An empty, repeated or
    other `model=` makes the restart unreadable, as a malformed `to=` does:
    after the widening, only a model aterm itself would refuse to put on a
    command line is left out, and a harness starting the child on the same
    model would answer a model move with the model it moved from. Other
    unknown words are still passed over. `cause=model` without `model=` is a
    restart on the same model.
14. **A re-exec carries the duty and the ladder** (2026-11-14, found porting
    kimi-alab, HX-09). A harness that replaces its own executable with `exec`
    and keeps its child (kimi-alab does, at a safe point) loses its claim at
    the `exec` (both sockets are close-on-exec), but not its duty: the child
    runs and nothing else supervises it. `Link::adopt_with` (and `adopt`) is
    `launch_with` for that image: `claims()` is true from the start, and a
    claim aterm cannot make now, refused or unreachable, keeps the link lost
    and claimed again every 5 s rather than absent, since an adopted child
    cannot be passed through to the vendor as a launch can. The crash streak
    crosses the `exec` too, or a crash loop could reset its back-off by
    updating the harness: `Ladder::since(now_ms)` is what the old image hands
    on and `Ladder::carried(streak, since, now_ms)` what the new one rebuilds,
    on whatever clock it keeps; a `since` longer than that clock has run reads
    as its start, which never forgives early. `relaunch::driver_hand` is
    public, so a harness checking a move a person attested by hand on
    `Link::read`'s facts refuses another driver's hand by aterm's rule rather
    than its own copy of it.
15. **A move judged by the harness's own readings has one rule here**
    (2026-11-14, found porting agy-alab, HX-12). `Link::read` hands a harness
    aterm's facts for a move it judges itself, and each harness wrote the same
    check on them (kimi-alab's `unheld` for a person's `SIGUSR1`, agy-alab's
    for its own safe point). `claim::judge_own_move(policy, status, holder,
    warrant)` is that check: the claim still this holder's, proven, with
    `upgrade`; no halt (`hold=0`, policy `why=` not `hold`); `hand=` naming no
    other driver (absent holds); and under `Warrant::Harness`, where nobody said
    when, `upgrade=true` and `upgrade_ask=false`. `Warrant::Person` skips those
    two switches, since the person is the one `upgrade_ask` waits for. aterm's
    `agent=`, `composer=`, `human_ms=`, `input=` and `need=` are left to the
    harness's own readings, which see the vendor directly. It answers a
    `SafePoint`, so a hold journals as `wait aterm <field>=<value>`.
16. **A release aterm does not take ends the connection** (2026-11-14, found
    porting grok-alab, HX-10). A claim the guarded sequence made and then
    gives back (a `status` that does not prove it, a contract-4 reply that
    cannot be read) used to be released with its reply ignored. At launch the
    connection is dropped either way, but a claim made again on a held link
    (a policy on again) keeps its connection, so a release answered with
    anything but `OK` left a claim aterm may still store on a link that noted
    itself unclaimed and asked again every 5 s on that same connection.
    Such a release, or one not answered, now fails the sequence as
    `Failure::Gone` (`<why> (release reply=ERR:…)`): the link is lost, drops
    the connection (aterm drops the claim bound to it) and claims again on a
    new one, as `Claimed::release` already did for a policy switched off. At
    launch that is absent, not lost, unless the release timed out after the
    handshake (then busy, and lost as any late step is): an aterm that will not give
    back a claim it just took is not one to keep claiming against. The fake
    stages it with `FakeAterm::set_release_reply`.
17. **A harness's test build may shorten the away budget** (2026-11-14, found
    porting muse-alab, HX-13). `AwayBudget` was fixed at aterm's 45 s, so a
    harness could not show in its own end-to-end tests that a crash is passed
    on once aterm stays away past it without waiting 45 s per scenario
    (muse-alab had such a test, against its own client, at 1.5 s).
    `AwayBudget::within(total)` is the same accounting over a shorter total;
    `AwayBudget::new()` (and `Default`) stays `AWAY_BUDGET`. Only a debug-only
    knob of a harness's test build may pass anything else: a release build
    answers aterm with aterm's grace.

18. **While the event stream is down, the policy is read on the clock**
    (2026-11-14, found reviewing the grok-alab port, HX-10). A standing claim
    reads `harness policy` on its `harness-policy` event, and on a clock only
    while a read is owed or the claim is given back. When the stream ended,
    the link read the policy once and tried to reopen the stream every 5 s;
    if that kept failing while the request connection still answered, nothing
    read the policy again until the stream came back, so a person's Autopilot
    Off (which a session's own Autopilot word announces to no link) left the
    claim standing and every act on it held, rather than given back. grok-alab
    read it every 5 s while aterm refused its subscription (its Decision 20).
    Now each failed reopen also reads the policy, so it is read every 5 s
    until the stream is back, and once more when it is. Second: a policy read
    a standing claim's event asked for, answered `ERR … retry`, is owed and
    read again every 5 s (as before), and is now noted once when it begins to
    be owed (`aterm policy-owed <why>`), as grok-alab's own client journaled
    it; a harness's journal otherwise showed nothing between the event and
    the change it waited for. The fake stages a stream that ends alone with
    `FakeAterm::end_events`. No public API changed beyond that knob.
19. **`v0.1.0` is tagged before HX-06 lands** (2026-11-14). The earlier plan
    tagged once aterm served contract 4 and the claim was measured against it.
    That held every harness on a path to a sibling checkout, so no port could
    reach its main or its package. The link already claims at contract 3 where
    aterm serves only that (decision 12), which aterm does today for the seeded
    harnesses, so a pinned harness runs on the aterm that is installed. What
    HX-06's measurement finds is fixed here and taken as `v0.1.x` by running
    `tools/pin` again: fixing forward and bumping, as aterm's
    `docs/OWNER_RULINGS.md` row of 2026-11-12 "Fixing forward" asks. The tracker
    still closes HX-08 only after HX-06, as its dependency says.

**The session stream (2026-11-14).**

20. **The record vocabulary** (HX-15a). `alab_wire::record` is the `Record`
    type that section 2.3, "The session stream", hands to this crate ("the
    full field list is the `Record` type of the `alab-wire` crate"): every
    kind of its "Harness to aterm" and "aterm to harness" tables, each with
    the fields of its "Carries" column, and the scope table as
    `Kind::needs`. It is written first, before either side streams, so
    aterm (HX-15b1, from a pinned copy of the tag) and the `Link` (HX-15k)
    run one parser and one list of kinds. The JSON is written by hand,
    bounded (256 KiB a line, nesting 8, refused before parsing past the
    bound) and total (never a panic): this crate depends on `libc` alone,
    and aterm's `serde` is its own first-party crate under that name, so a
    serde edge from crates.io fits neither side. The module is portable:
    it names no platform item, no I/O and no clock, and reaches only the
    claim: its `Scope`, `ChildProcess`, `Account` and `ReplyError`, and
    its word rules (`canonical_u64`, `claim_version`, `vendor_word`,
    `login_word`, `model_word` and the restart's cause), so a record's
    words are the claim's. A test reads its source to keep it so, and
    refuses a relative path, which would reach past that list. The claim
    reaches no transport, but it does reach the wire's text helpers
    (`Fields`, `percent_decode`, `session_id`) and the config. aterm takes
    alab-wire for Unix only and turns a record into its own types at once,
    but the vocabulary should not be what keeps a Windows harness out:
    when one comes, `lib.rs` gates the transport, the wire's text helpers
    move out of it, the claim's import follows them, and this module
    compiles unchanged. `ms` is passed in, as every time
    here is: the crate reads no clock, and `ms` is only shown (aterm stamps
    each record when it arrives and orders by `seq`).
21. **What the record vocabulary decides where section 2.3 is silent**
    (2026-11-14, revised the same day after review).
    - An `ack`'s `seq` is the harness record it acknowledges, as the design
      spells it; an ack is not counted in aterm's `in` and not answered.
      The HX-15 plan's D27 says a harness answers each of aterm's records
      with `took` or `refused`; `ack` is the one it does not answer (a
      `took` naming an ack's `seq` would name a harness record as aterm's),
      so the Link of HX-15k answers every aterm record but `ack`. `took`
      and `refused` are harness records with their own `seq`; the `in` they
      answer is `of`, and `StreamReply`'s `in=` is `sent`.
    - Every line carries `k`, `seq` (from 1) and `ms`, and `k` is a word;
      the design's short examples, `{"k":"beat"}`, `{"k":"ack","seq":N}`
      and `{"k":"took","in":N}`, are not lines. HX-15b1, which rewrites the
      design's stream lines, gives them `seq` and `ms`.
    - `title`, absent from the scope table, needs only the stream: a title
      is a description claim, ranked by its author (section 2.3), not a
      duty taken from aterm's engine, and under `state` a claim granted only
      `relaunch,upgrade` would have every title dropped. The table should
      gain the row.
    - "The scope of the act it replaces" is aterm's to resolve, never the
      record's to name. An act taken by a rule carries the rule, the
      `<act>` of `meta set harness_act <act>`, and `Body::admitted` takes
      aterm's act table (`acts::act_scope`) for it: a rule the table does
      not name is admitted by no grant. The first version had the act name
      its own scope, so any act read as admitted under any scope its claim
      held. `wait` (`harness_activity`) needs `state` and `held`
      (`harness_held`) `turn-end`; `need` and `clear` take any of
      `approvals`, `questions` or `walls` (`harness_needs`), and `land`
      takes `turn-end` (`harness_land`).
    - Each record that replaces a `meta set harness_*` line carries that
      line's words, so aterm writes it as it writes the line (D31 of the
      plan). `act` is one of three shapes: `wait` with its `reason` (the
      eight words `harness_activity` takes) and `wait_ms`, absent for no
      bound (`wait_ms=-`); `held` with `same-reply` or `no-work`, absent
      for the lift; or any other act, by its `rule`. `need` names the need
      aterm's `mark need` raises (`review`, `decision` or `help`), its
      entry and 1 to 32 needs (`harness_needs`); `clear` names the `need`
      record it ends, and aterm lifts what that record raised. `land`
      `held` carries `since_ms` and `nudges` and, when Land read the
      yield, its legs, open words, the five repository counts and `left`;
      `landed` carries its commit; `open` lifts a hold. A field that
      belongs to another shape refuses the line, and the Rust types
      (`Did`, `Landing`) hold only what each shape can say.
    - A closed enum where the design or aterm lists the words; a bounded
      word where the vendor or aterm's loop states the value (the wall
      kind, an account's state, a limit's name, an effort, a permission
      mode, Land's legs and open words). A wall kind aterm does not know
      is applied as a bare `wall` (the plan's D13), so a newer kind
      reaches an older aterm rather than refusing its line. A turn ends
      `done`, `failed`, `interrupted` or `walled` (its kind the `wall`
      record's), as aterm's own turn readers end one; a tool call or a
      subagent ends `done`, `failed` or `interrupted`; a compaction is
      `started` or `done`, as aterm's compact step, which never fails. A
      turn that starts names who opened it. `asked`'s `reply` is
      `approve`, `deny` or `answer`, the words aterm logs it under.
    - An account is an account label, the claim's `login=` (an account
      pool's `ac-` key is one). `usage` and `account` name theirs, required:
      aterm takes a reading only for the account the pool assigned to the
      tab, and one with no account could not be checked. `session` names
      the account it runs on, as the `login=` it replaces. `claim` carries
      `vendor` and `login` beside `child`, the scope and `version`, each in
      the grammar of the claim word aterm builds from it (D7), so the
      Link's `reannounce` (HX-15k) is one `claim` record and a renewal on
      the stream keeps `supervisor_vendor=` and the claim's `login=`
      current. The child is the claim's `ChildProcess`.
    - A `restart` is read by the claim's `Restart` rules, so every record
      that reads maps to a `Restart`: a cause of 1 to 64 letters, digits,
      `-` and `_`; `to` and `home` together, only for `account` or
      `account-ahead`, `home` absolute or `-` for the vendor's default
      (`Body::Restart` holds the claim's `Account`); a model word. `set`
      and a model on offer name model words too, so no model starts with
      `-`.
    - The top-effort key is `key`, whether it is on, and not its word:
      aterm keeps Claude Code's word as one constant
      (`aterm_primer::CLAUDE_TOP_EFFORT_KEY`) and its export scan refuses
      the word written out. The band (HX-16b) draws aterm's constant for
      the row's vendor where `key` is true.
    - `prompt` and `answer` name `person`, `ctl` or `peer:<tab>` in `by`,
      the senders outside the harness (`By::outside`), as the design lists
      them.
    - A record names another by its `seq` (`asked`, `answer`, `touch`,
      `clear`), and a turn by the harness's own count (`turn`, which
      `msg`, `tool` and `diff` name). `asked` carries its ask's kind, so
      its scope reads without state. An `answer` carries exactly one of
      `option` and `text`. An `ask` says when it will answer as
      `will_in_ms` after the record, because aterm times by its own
      arrival stamp.
    - A field this library does not know is counted at every depth; a
      record of an unknown kind counts none of its fields. A known field
      named twice, of the wrong type or out of its grammar refuses the
      line, and `null` reads as absent. A `claim`'s narrowed scope and a
      `session`'s `takes` name each word once, known or not. The scope
      drops and counts a well-formed unknown word, as aterm drops one, and
      refuses a malformed one; a scope of unknown words alone reads as a
      narrowing to nothing this library knows, as `[]` does, and aterm,
      which refuses an empty `scope=`, refuses that claim. `takes` keeps
      only kinds aterm sends.
    - A string holds raw none of the characters aterm's hidden-character
      table names (`aterm-types` `hidden_char.rs`): the writer escapes
      each one and the reader refuses one raw. No escape the writer makes
      is longer than what it read, so a line that reads writes back within
      the bound, equal; and the writer refuses any record the reader would
      refuse. The writer stops once its line passes the bound, so a record
      too long to send costs no more than the bound to refuse.
    - `LINE_MAX` counts every byte `Record::read` is passed: the line
      before its newline, a `\r` among them. HX-15b1's reader bounds those
      bytes and strips nothing else.
    - `StreamReply::parse` takes its words in any order and passes over a
      `key=value` it does not know, as this crate's other readers do, and
      reads `v=` as the `STREAM_VERSION` the request asked for.
22. **The Link asks for the stream on the claim's connection first**
    (HX-15k, the plan's D24; 2026-11-14). Once a contract-4 claim is proven
    and subscribed, the link sends `stream @<sid> v=1` on the connection
    that holds the claim, and opens the second request connection only after
    `OK`. The other order would connect and close a connection against every
    aterm before HX-15b1. Any reply but `OK` is silent: nothing is noted, so
    a harness on an older aterm journals what v0.1 journalled. The plan asks
    again every `RECLAIM_EVERY_MS`; this asks again on that clock only when
    the reply asks to be retried (`ERR busy streams`, or an `ERR` ending in
    `retry`), and keeps what was kept for the stream. Any other `ERR` (`ERR
    unknown verb`) is asked again only on the next claim's connection, and
    what was kept is given up, noted `aterm stream-given-up records=<n>`: an
    aterm gains the verb only by an update or a restart, which ends the
    connection, so asking every 5 s on it was a request aterm refuses for
    the session's life. An exchange that fails or a second connection that
    cannot be opened loses the link: the claim's connection is then of no
    use. An `OK` that does not read as the stream's (`StreamReply`, or a
    count above `SEQ_MAX`) loses it once, noted `aterm stream-off stream
    <why>`, and the link never asks again, so a broken aterm does not make
    it claim every 5 s. Contract 3 never asks, and nor does a config with
    `Config::with_stream(false)`, for a harness that is not ready to stream.
23. **What is kept, and resent** (D8). Every harness record is numbered from
    1 with no gaps and kept until aterm acknowledges it or a later one. The
    caller's are bounded, 256 records and 4 MiB of lines (`RESEND_MAX`,
    `RESEND_BYTES_MAX`): past either `Link::send` refuses the record,
    unnumbered (`SendError::Full`), so the caller decides what to drop. The
    link's own records (`beat`, `claim`, `took`, `refused`) are not refused,
    since the stream's life depends on them, but past 128 kept (`OWN_MAX`)
    aterm is reading nothing, and the stream is lost, `aterm lost stream
    own-queue=128`. Records are written at four fifths of aterm's budget,
    400 at once and 40 a second (`PACE_BURST`, `PACE_PER_S`, D10), on the
    caller's `now_ms`: 256 and 128 make everything kept fit one burst, so a
    stream opened again writes it whole and aterm drops none of it for the
    rate. A stream that opens gives up what is at or below its `acked=` and
    writes the rest again under the numbers each record had: a `claim`
    record is superseded by the claim placed before every open, and an
    answer to a stream whose `in` aterm no longer holds names a record it
    does not know, so each is written as a `beat` under its number, which
    keeps the caller's numbers (an `answer` names an `ask` by its `seq`).
    Only when aterm's `acked=` went back (a new epoch, or a claim aterm
    forgot when its re-exec grace ended) are the caller's records numbered
    again from `acked+1`, the link's own dropped, and each move returned in
    `Drained::renumbered`; `Drained::fresh` (also the first stream) says
    aterm holds nothing the caller sent, so its `session` and `state` go
    again. A record sent while the link is lost after a stream is kept for
    the next one; `SendError::NoStream` comes only when no stream will
    resend it.
24. **Every record aterm sends is answered once** (D27). The link answers
    what it acts on: `hand` and `policy` `took`; `restart` `took` when the
    grant takes `relaunch` (returned as `Drained::restart`), otherwise
    `refused` "no relaunch grant". The caller answers the rest with
    `Link::answer`, since only it knows whether it took the prompt or the
    answer: `want`, which aterm sends to every stream, always reaches it,
    and `prompt`, `answer`, `touch`, `interrupt` and `set` reach it when the
    last `session` record the caller sent lists the kind in `takes`, the
    list aterm gates its sending on; otherwise they are `refused` "not
    taken". More than 64 left unanswered (`AWAITED_MAX`) refuses the oldest
    "unanswered", so a caller that ignores them does not keep aterm's
    ladder waiting forever. A kind this library does not know is answered
    `took` and counted (`Counts::unknown_kinds`), as the design's rule for
    an unknown `k` is "ignored and counted, never refused"; an unknown field
    is counted too. A harness kind sent by aterm stays `refused` "harness
    kind": that is no unknown `k`, it is aterm's bug. A line that cannot be
    read is scanned loosely for its `k` and `seq`: an unreadable `ack` moves
    no number (it is not aterm's record), a line with a `seq` is refused
    with the reading's fault, and one without is passed over, so an
    unreadable line never hides aterm's next record. The first of a stream
    is noted, `aterm stream-unreadable k=<kind> in=<n> <why>`. A record at or below
    the last one heard was answered and is passed over; a gap is accepted,
    since aterm never resends. A count above `SEQ_MAX`, or an `ack` of a
    record never written, loses the stream: none of the numbers can wrap.
25. **Times on the stream.** `beat` goes first at the stream's open plus
    `BEAT_EVERY_MS` of the caller's `now_ms`, then every `BEAT_EVERY_MS`; a
    late tick sends one beat, not the ones it missed. `Link::set_beating`
    pauses beats while the stream and the claim stay, as a harness that
    hangs looks to aterm's watchdog. A record's `ms` is a wall clock, which
    `now_ms` is not (it should stop in sleep, above), and the crate reads no
    clock: `Link::send` takes the caller's `ms`, and the link's own records
    are stamped with the caller's last `ms` (or the one it gave with
    `Link::set_wall`) moved on by `now_ms` since, or 0 before either. `ms`
    is only shown. `Link::due_ms` names the next beat, paced record, ask,
    reclaim or owed read, and 0 while a line read waits.
26. **`reannounce` on a stream is a `claim` record** (D7). It carries the
    child, every scope word the claim asks for, the version, and the vendor
    and login when they are words, as the claim line would. aterm answers
    it with an `ack` and, when the grant moved, a `policy` record, on which
    the link reads the grant again, so the grant follows aterm's word
    rather than a reply (the plan's D24 replaces Decision 2 for a streaming
    link). An `ack` naming `dropped=rate` sends the claim record again; any
    other `dropped=` loses the link, as a refused claim line does in v0.1.
    It notes `aterm reannounced vendor=<v> child=<pid:start> seq=<n>`.
27. **While a stream is open** the subscription is kept. Once the stream
    has brought one of aterm's own records, `EVENT restart` and `EVENT
    harness-policy` are passed over, and so is the subscription's loss,
    which hides neither: the stream carries both. Until then they are
    honoured, so an aterm with HX-15b1 and not HX-15c, which opens streams
    and sends no record, is still heard; the subscription is also drained
    before the stream is asked for, so a restart sent before the open is not
    lost. That covers what a link can see. It cannot see that such an aterm
    acknowledges `claim` records without placing them (the plan's D9):
    `status` names no claimed child, and a renewal in place moves no
    revision, so nothing could check it. HX-15b1 and HX-15b2 must ship in
    one aterm release. A policy read aterm owed or a released claim is
    still read on the clock (Decision 18). Every read and the release go on
    the second connection; the release closes the stream (D23). A `hand`
    record fills the hand fields of the last `status` read (`Link::status`),
    the newest `human_seq` winning; only `hand`, `policy`, `restart` and the
    subscription's activity have a hold read again before its time, not
    acknowledgements. A stream that ends, or a write it does not take within
    `REQUEST_TIMEOUT`, loses the link, `aterm lost stream <why>`, and the
    claim is made again at once, or `RECLAIM_EVERY_MS` after the stream
    opened when it ended sooner. The first drop of each reason on a stream
    is noted, `aterm stream-dropped seq=<n> k=<kind> dropped=<word>`, and
    counted (`Link::counts`).
28. **The fake serves the stream on request.** `FakeAterm::serve_stream`
    turns it on; until then `stream` is answered `ERR unknown verb`, so
    every earlier test is a test of the v0.1 path against an aterm before
    HX-15b1. It opens only on the connection that holds a contract-4 claim,
    with a full rate budget at each open, and writes each stream's lines
    from a thread of its own, so a harness that does not read never stalls
    the fake. Without `FakeAterm::set_reexec_grace(true)` a claim still goes
    with its connection; with it a contract-4 claim stays, unbound, until
    the next claim or `FakeAterm::end_grace`, which forgets its `acked=`
    and `in=`. `acked=` and `in=` are otherwise kept per claimant until
    `FakeAterm::restart`, a new epoch. `set_stream_own_records(false)` and
    `set_stream_applies_claims(false)` make it an HX-15b1 alone. A record's
    scope is judged with no act table, so an `act` by a rule is dropped
    `scope`. Every placement from a `claim` record moves the revision, so
    every one is followed by `policy`. Its subscription starts at the
    events pushed before its ACK, which fixes a race in which an event
    pushed as the client read the ACK was never sent.
29. **The relaunch grammar** (`run`, D21). `--` is always there and may be
    followed by nothing, as atpkg's twin envelope passes `run -- "$@"`.
    `--home` is refused without `--account`: a home comes only from a pool
    assignment, which names its account. `--home` is an absolute path, and
    `-` is refused, since leaving `--home` out already says the vendor's
    default. A flag's value never starts with `--`, so `--account --` is a
    missing value. `--flag=value` is an unknown flag: one spelling, as aterm
    types it. The resume arguments pass through as given.
30. **A contract-4 claim is given back explicitly** (the plan's D28). aterm
    keeps a contract-4 claim for `REEXEC_GRACE_MS`, 45 s, after its
    connection closes, and the next claim from the same process renews it.
    So a harness whose link is lost and which exits would leave its claim
    standing for 45 s, with aterm's watchdog counting it: `Link::release`
    on a lost link sends `meta unset supervisor` on a connection of its own
    within the grace, and a claim made again inside the grace that aterm
    refuses (policy off, unserved) releases it too. A release aterm does not
    take or answer marks the claim unreleased, so a loss after it still
    counts the grace and gives the claim back.
31. **What the caller decides on a stream.** `Link::request` sends a
    session-addressed request on the held request connection, the second
    while a stream is open, for the acts the stream does not carry (`meta
    set harness_*`, D24); it refuses a claim, a release, `stream`,
    `subscribe`, a leading `@` and control characters, since `meta set
    supervisor` on the second connection would bind the claim to it and
    close the stream. `Link::answer` answers each record the caller was
    given; `Link::set_beating` and `Link::set_wall` are above.

**The re-exec (2026-11-14).**

32. **One re-exec, here** (HX-08a; plan `hx-proofs.md` D8). muse-alab and
    kimi-alab each had one, and agy-alab, then grok-alab, take this one
    before they delete theirs. Where the plan is silent:
    - **No exec without a probe of that file.** `probe` and `exec` are apart,
      because the probe comes before the safe point is re-read (muse-alab's
      order), but `exec` takes only the `Proved` a probe returned, and holds
      (`Held::Changed`) when the file's device, inode, size, modification or
      change time moved since: a build replaced after its probe is probed
      again, never exec'd unproved. `probe` takes only an absolute path (a
      store path is one): a bare name is found on `PATH` by the probe but in
      the working directory by its stamp and by `execv`, so the build probed
      and the build exec'd could differ.
    - **The protocol line is `alab-reexec <FORMAT> <harness id> <own>`**: this
      library's carry format (1), the harness, and the harness's own number
      for its `own` lines. A change on either side fails the probe, so an old
      image never execs into a build that reads another carry.
    - **`<exe> --alab-reexec <fd>`, the descriptor in argv** (muse-alab's), not
      kimi-alab's `KIMI_ALAB_EXEC_FD` beside `run -- <args>`: the environment
      crosses untouched, so there is nothing to strip before the vendor's next
      start, and the child's launch words are the harness's to carry in its
      own lines.
    - **A pipe, not an unlinked file** (muse-alab's, not kimi-alab's): nothing
      touches the disk. A fresh pipe takes 64 KiB without a reader on macOS
      (measured 2026-11-14: one non-blocking write of 65 536 bytes fit, of
      65 537 wrote 65 536) and on Linux by default, so the carry is bounded at
      `MAX_BYTES` (64 KiB) and a larger one is held (`Held::Carry`), never a
      hang. The write end is closed before the exec and the new image reads
      without blocking, so a writer some other process inherited by mistake
      cannot hold it. The read end is moved above 2 (`F_DUPFD_CLOEXEC`):
      `pipe(2)` takes the lowest free numbers, so a harness whose input is
      closed got descriptor 0 (measured), which `receive` refuses as a
      standard one, after the exec, where nothing can hold.
    - **The core lines are what this crate's own types need**: the child's
      identity, the claim's `vendor=` and account (so the new image's claim
      is the same claim, `Carry::claim`), the crash ladder (`Ladder::carried`,
      Decision 14), whether the old image was linked (muse-alab's `link`;
      `Link::absent` without), and the old image's version. Everything else a
      harness carries (its build, launch words, session lock, probes) is its
      `own` lines, bytes it reads back itself. The child's start is not a
      line: it is the birth.
    - **kimi-alab's schema-2 identity, with two changes.** The image file is
      the running image's own (macOS: the first file-backed region's vnode,
      `PROC_PIDREGIONPATHINFO`, with `proc_regionwithpathinfo` declared here as
      `libc` does not; Linux: `/proc/<pid>/exe`'s metadata), and is not also
      required to be the file its path names now: kimi-alab required that,
      which refused a child whose vendor build was replaced in place while it
      ran on. The birth is the kernel's raw stamp (macOS seconds and
      microseconds, Linux ticks after boot), not `kernel::start_us`, because
      Linux's `btime` moves with the wall clock; `Identity::process` gives the
      claim's `child=` from it. kimi-alab's schema 1, which carried no
      identity, is not read: stale compatibility.
    - **An ended child holds the exec, and is still carried.** `exec` holds
      (`Held::Child`) so the old image reaps it and judges the exit; a child
      that ends between the exec and `receive` is still the new image's,
      unreaped, so `receive` accepts it and returns it as `Alive::Ended`
      (kimi-alab's adoption refused it, leaving nobody to judge that exit).
      `receive` refuses a carry another process wrote (the pid is the proof
      of the same process) and checks the child against the kernel again.
    - **Only `SIGHUP` and `SIGTERM` hold** (`HOLDING`): the session is ending.
      muse-alab also held on `SIGCONT` through its own wake source; here every
      other caught signal pending at the exec crosses pending and the new
      image takes it first (`take_pending`).
    - **What macOS drops after the exec is noted all the same** (`UNKEPT`).
      `execve` resets each caught handler to the default, and until the new
      image installs its own, macOS discards a blocked signal whose default
      is to ignore it as it is posted. Measured 2026-11-14: after an exec
      that kept them blocked, a killed child's `SIGCHLD`, `SIGWINCH`,
      `SIGURG`, `SIGIO` and `SIGINFO` were not pending and the child sat
      unreaped, while `SIGCONT`, `SIGINT` and `SIGUSR1` were; all eight
      pending at the exec crossed. Linux keeps a blocked signal pending.
      So `take_pending` notes each caught one of `UNKEPT` (macOS's five,
      Linux's `SIGCHLD`, `SIGURG` and `SIGWINCH`) whether or not it is
      pending, on both kernels, and the harness acts on the note as on a
      state to read again: a note with nothing behind it costs one
      `waitpid` or one size read. `Alive::Ended` (from `receive`) and a
      noted `SIGCHLD` both send the new image to reap the child.
    - **The block is the calling thread's** (`pthread_sigmask`; `Blocked` is
      not `Send`), and a held exec gives the mask back, so the harness's
      handlers take what arrived. Every other thread must already block the
      caught signals, or the harness execs only while it has no other
      thread: a `SIGTERM` or `SIGHUP` another thread takes after the last
      read of the wake source is delivered, not pending, so `exec` goes
      ahead, and the wake source does not cross `execve`, so the signal is
      lost. A signal every thread blocks stays pending for the process,
      where `exec` sees it. The four harnesses start no thread outside
      their tests (grep, 2026-11-14).
    - **The stream needs no line.** aterm keys a claimant and its re-exec
      grace on the pid and birth, which `execve` keeps, so once HX-15k's
      `Link` streams, the new image's stream open reads `acked=` and numbers
      on from it; what the old image had not had acknowledged is not carried.
    - **Tested by exec'ing this crate's test binary.** An image is one ignored
      test run alone in a process of its own; an exec into one passes libtest's
      filter and `--` before `--alab-reexec <fd>` (`exec_with`, the one test
      seam, with `Proved::unprobed`, since libtest cannot answer a probe).
      `the_new_image_claims_again_under_the_same_pid` execs a harness holding
      a contract-4 claim on the fake aterm and reads the claim again from the
      same `<id>@<pid>` at the new version and the same `child=`, and
      `a_proved_build_reads_the_carry_from_its_one_inherited_descriptor` runs
      the production path whole into a probed shell build.
      `a_vendor_exit_after_the_exec_is_noted` kills the child and raises
      `SIGWINCH` in the new image before its handlers are in, and reads both
      from `take_pending` (on macOS only the notes carry them).

**The declared contracts (2026-11-14).**

33. **A harness claims only the contracts it declares** (HX-08b; plan
    `hx-proofs.md` D11). The claim sequence chose the highest contract both
    sides list, 4 else 3, on the first claim and on every reclaim, so a
    harness whose record says `contracts = [4]` claimed 3 from an aterm
    serving `2,3`: at a launch aterm first answered "later", and after a
    link lost to an aterm restart into an older one. agy-alab wrapped the
    link in a policy read of its own and a reclaim gate (its Decision 46);
    it is done once here instead. Where the plan is silent:
    - **`Config::with_contracts(&[Contract])` declares them**, kept once each
      in ascending order. The default, `Config::DEFAULT_CONTRACTS`, is
      `[3, 4]`, every contract this library speaks, until HX-14b deletes
      contract 3. An empty list is `ConfigError::NoContract`, as no scope is
      `NoScope`: such a harness could never claim.
    - **One place chooses.** `Contract::choose(declared, served)` and
      `Policy::contract(declared)` take the declared list, and `link::admits`
      reads the policy against `Config::contracts`, so the launch, the
      adopted image (`Link::adopt_with`), every reclaim on the clock and a
      claim after a policy switched back on all keep to it. `claim_verb`
      returns `None` for an undeclared contract as a second fence.
      `Policy::admits_claim` takes the list too. `Policy::serves_contract`
      keeps its meaning (a contract this library speaks): it is read only by
      the judges of a claim that already stands, which was made under a
      declared contract, and aterm changes the contracts it serves only
      across its own update, which ends that claim.
    - **None served is `Failure::Unserved`, as before.** The reason is
      `policy contract=<served> declared=<declared>` (`aterm unclaimed policy
      contract=2,3 declared=4`), for the default config too (`policy
      contract=2 declared=3,4`): the line says which side lacks what. As for
      contract 2 alone (Decision 12), the launch is lost and unclaimed, the
      policy is read again every 5 s with nothing more sent, a standing claim
      is kept, and the claim is made at the highest declared contract once
      aterm serves one. Inside the re-exec grace an attempt answered so still
      gives back the claim aterm may have kept (Decision 30), once: nothing
      stands after, so the grace ends with that answer at an adoption as on
      a reclaim (`Failure::gives_back`). An adopted image used to keep its
      grace through a refusal and give the claim back again at its first
      reclaim and its release.
    - **`Link::unclaimed` gives the caller the answer.** It returns the lost
      link's last claim failure, so a harness that would rather run its
      vendor in its own place than wait for an aterm update (agy-alab passes
      through on an unserved policy at launch) reads `Failure::Unserved`
      there, with nothing claimed to give back, while one that runs its
      vendor unclaimed (kimi-alab, grok-alab) needs no change. The lost
      state keeps the failure itself in place of its text.
    - **Tested against the fake's `2,3`.**
      `a_link_that_declares_four_never_claims_three` launches and adopts
      against it (policy reads and the grace's one release only: no roster
      read and no claim at 3, then 4 once served),
      `a_reclaim_keeps_to_the_declared_contracts` holds 4
      through an aterm restart into `2,3`, and
      `the_default_contracts_are_three_and_four` pins the default; each link
      test has the default config claim at 3 in the same scene as its
      control.

## Build

Rust is the Trust toolchain (`rust-toolchain.toml`):

```text
targo --unverified test --locked --workspace --all-features
targo tippy --locked --workspace --all-targets --all-features -- -D warnings
targo fmt --check
```

These gates run on macOS. The Linux code (`kernel`'s `/proc` reader,
`SOCK_CLOEXEC` in `wire`, and `reexec`'s `/proc/<pid>/stat` and
`/proc/<pid>/exe` identity reader) was type-checked and linted on 2026-11-14
for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` with stable
`cargo clippy --locked -p alab-wire --all-targets --all-features --target <t> -- -D warnings`
(clean, with and without `--all-features`; stable 1.96 for `reexec`). It has
never been run.

Apache-2.0 (`LICENSE`).
