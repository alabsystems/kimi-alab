# alab-harness

The half every ALab harness needs, shared. A harness is the program that runs
one vendor's agent CLI (Claude Code, Kimi, Grok, Muse, Codex) inside an aterm
tab and takes duties from aterm's built-in engine. Each harness used to carry
its own aterm client. This repository holds one copy of that client, and of
the policy and account code that comes next.

Remote: none yet. The sibling harness repositories are private, and the tool
that creates repositories here can only make public ones, so this repository
lives on one machine until a private remote can be made.

## Crates

| Crate | State | What it gives |
|---|---|---|
| `alab-wire` | here, v0.1.0 | an aterm client: wire, the contract-4 and contract-3 claim and its grant, link, exit judgement, crash ladder, fake aterm serving contracts 3 and 4 |
| `alab-policy` | to come | Cruise, Land, the done check, NEEDS YOU, the go-ahead reader, approval classification |
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
  contract is the highest of 3 and 4 that `harness policy`'s `contract=` set
  lists (`Contract::choose`). A contract-4 claim names every scope word the
  harness asks for, `version=`, and when known `vendor=`,
  `child=<pid>:<start>` (`ChildProcess`) and `login=`; its reply
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
  policy read at each attempt while it stays down (Decision 18). A harness image that took over a running child from an
  earlier image of itself (a re-exec that kept the child) starts with
  `Link::adopt_with` (Decision 14).
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
  request connections stay.

It is generic. A harness passes one `Config`, which holds its id (for example
`kimi-alab`), its vendor word, the scope words it asks for (any of the eight)
and its version. Under contract 4 every word is claimed and aterm grants what
it knows and the person's cap allows. Under contract 3 only `relaunch` and
`upgrade` are claimed (`Scope::CONTRACT_3`), because aterm's roster rows allow
nothing else and refuse a whole claim naming another word. The
config can also name the environment variables to read in place of aterm's
defaults. The library names no harness of its own. It does no process
supervision: it never forks, execs, signals or reaps a vendor. It reads no
clock (each call takes `now_ms`) and writes no journal (`Link::take_notes`
returns the lines to log). The caller's `now_ms` should stop while the
machine sleeps, as `std::time::Instant` does (`CLOCK_UPTIME_RAW` on macOS,
`CLOCK_MONOTONIC` on Linux; macOS's `CLOCK_MONOTONIC` and any epoch clock
count sleep): the 45 s away budget and the crash ladder's healthy run are
aterm's grace and back-off, which a closed lid neither spends nor serves.

```rust
use alab_wire::{Child, ChildProcess, Config, Link, Scope};

let config = Config::new("kimi-alab", "kimi", &[Scope::Relaunch, Scope::Upgrade, Scope::State], "0.4.0")?;
let child = Child { vendor: Some("2.5.1"), ..Child::default() };
let mut link = Link::launch(config, std::process::id(), child, now_ms());
// After every child start, name the new child by pid and kernel start time.
let started = Child { process: Some(ChildProcess::of(pid)?), ..child };
link.reannounce(started, now_ms());
if link.takes(Scope::State) { /* report what the agent does */ }
for line in link.take_notes() { journal.note(&line); }
```

## How a harness pins it

There is no git dependency, because aterm's `deny.toml` refuses git sources.
Each harness carries one tagged version of this repository in its own tree and
depends on it by path:

```sh
~/alab-harness/tools/pin ~/kimi-alab v0.1.0     # the tag's tree at vendor/alab-harness/, its tree id in vendor/alab-harness.pin
git -C ~/kimi-alab add -f vendor/alab-harness vendor/alab-harness.pin && git -C ~/kimi-alab commit
~/alab-harness/tools/check-pin ~/kimi-alab      # the release check: copy == pin == tag
```

```toml
# the harness's Cargo.toml
[workspace]
members = ["."]
exclude = ["vendor"]          # the copy is its own workspace, never a member of the harness's

[dependencies]
alab-wire = { path = "vendor/alab-harness/crates/alab-wire" }

[dev-dependencies]
alab-wire = { path = "vendor/alab-harness/crates/alab-wire", features = ["fake"] }
```

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

- **`alab-policy`** and **`alab-accounts`**, as the table above says.
- **A remote.** The repository lives on the lab's Mac until its dev tier
  exists; a pin needs only the tag, so no harness waits on it.

## Decisions

Choices the specification of record (aterm's
`docs/DESIGN-harness-interface-2026-11-12.md`, section 2.2 and packages HX-06
and HX-08) leaves open, made here with their reasons. Revisit any of them when
HX-06 lands in aterm and a real reply contradicts it.

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

## Build

Rust is the Trust toolchain (`rust-toolchain.toml`):

```text
targo --unverified test --locked --workspace
targo tippy --locked --workspace --all-targets -- -D warnings
targo fmt --check
```

These gates run on macOS. The Linux code (`kernel`'s `/proc` reader,
`SOCK_CLOEXEC` in `wire`) was type-checked and linted on 2026-11-14 for
`x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` with stable
`cargo clippy --locked --all-targets --all-features --target <t> -- -D warnings`
(clean, with and without `--all-features`). It has never been run.

Apache-2.0 (`LICENSE`).
