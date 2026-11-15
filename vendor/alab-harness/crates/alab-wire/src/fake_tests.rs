// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The fake and the client agree: each test drives the fake through
//! `Locator`, `Connection` and `EventReader` with no harness process, so a
//! harness scenario that fails later is the harness's failure and not a
//! disagreement between the fake and this crate.

use std::os::fd::AsRawFd as _;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use super::*;
use crate::claim::{
    self, Child, ChildProcess, Contract, Grant, Hold, Human, Policy, ReplyError, SafePoint, Scope,
    Status, judge_safe_point,
};
use crate::wire::{Connection, EventReader, EventUpdate, Fields};

/// How long a self-test waits for the fake's threads before it fails.
const HANG: Duration = Duration::from_secs(20);
/// One request's deadline in the self-tests.
const TIMEOUT: Duration = Duration::from_secs(5);
/// The harness under test.
const ID: &str = "example-alab";

fn config(scopes: &[Scope]) -> Config {
    Config::new(ID, "example", scopes, "0.1.0").expect("a config")
}

fn root(name: &str) -> FakeAterm {
    FakeAterm::scratch(name)
}

fn connect(fake: &FakeAterm) -> Connection {
    let socket = fake.locator().socket().expect("the graph names the socket");
    Connection::connect(&socket, TIMEOUT).expect("connect and authenticate")
}

fn eventually(what: &str, mut holds: impl FnMut() -> bool) {
    let until = Instant::now() + HANG;
    while !holds() {
        assert!(Instant::now() < until, "hang detector: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Drain `events` until `wanted` says an update is the one awaited, waiting on
/// the reader's own descriptor as a supervisor's loop will.
fn await_update(
    what: &str,
    events: &mut EventReader,
    wanted: impl Fn(&EventUpdate) -> bool,
) -> EventUpdate {
    let until = Instant::now() + HANG;
    loop {
        let mut probe = libc::pollfd {
            fd: events.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: probe is one initialized poll record for the reader's open descriptor.
        let _ = unsafe { libc::poll(&raw mut probe, 1, 50) };
        let update = events.drain();
        if wanted(&update) {
            return update;
        }
        assert!(
            !update.ended,
            "the event stream ended awaiting {what}: {update:?}"
        );
        assert!(Instant::now() < until, "hang detector: {what}");
    }
}

fn ask(connection: &mut Connection, request: Option<String>) -> String {
    connection
        .request_line(&request.expect("a session ID the client accepts"))
        .expect("a reply")
}

fn me() -> (u32, String) {
    let pid = std::process::id();
    (pid, claim::holder(ID, pid))
}

/// A contract-3 claim of `scopes` by this process.
fn claim(fake: &FakeAterm, connection: &mut Connection, scopes: &[Scope]) -> String {
    claim_at(fake, connection, Contract::Three, scopes, None)
}

/// A claim of `scopes` at `contract` by this process, naming `process` as its
/// child.
fn claim_at(
    fake: &FakeAterm,
    connection: &mut Connection,
    contract: Contract,
    scopes: &[Scope],
    process: Option<ChildProcess>,
) -> String {
    let (pid, _) = me();
    ask(
        connection,
        claim::claim_verb(
            &config(scopes),
            contract,
            &fake.sid,
            pid,
            Child {
                vendor: Some("2.1.311"),
                login: None,
                process,
            },
        ),
    )
}

fn status(fake: &FakeAterm, connection: &mut Connection) -> Status {
    Status::parse(&ask(connection, claim::status_verb(&fake.sid))).expect("a schema-1 status")
}

fn policy(fake: &FakeAterm, connection: &mut Connection) -> Policy {
    Policy::parse(&ask(connection, claim::policy_verb(&fake.sid))).expect("a policy")
}

/// The client finds the socket through the graph, reads the token beside it and
/// authenticates; a wrong token and an unknown session are refused.
#[test]
fn the_fake_aterm_is_found_and_authenticates_this_crates_client() {
    let fake = root("auth");
    assert_eq!(fake.locator().sid(), fake.sid);
    assert_eq!(
        fake.locator().socket().expect("socket"),
        fake.sock_dir.join("c.sock")
    );
    let mut connection = connect(&fake);
    assert_eq!(connection.version(), "version=fake-aterm");
    assert_eq!(fake.connections(), 1);
    // A request must name this session, and an unknown verb is an ordinary error.
    assert_eq!(
        connection.request_line("@s-ffff status").expect("reply"),
        "ERR no such session"
    );
    assert_eq!(
        connection
            .request_line(&format!("@{} look", fake.sid))
            .expect("reply"),
        "ERR unknown verb"
    );
    assert!(!connection.is_poisoned());
    assert_eq!(fake.verbs(), ["look"]);

    let token = fake.sock_dir.join("c.sock.token");
    std::fs::write(&token, "another-token\n").expect("rewrite the token");
    let socket = fake.locator().socket().expect("socket");
    let refused = Connection::connect(&socket, TIMEOUT).expect_err("a wrong token");
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(fake.connections(), 1);
    std::fs::write(&token, format!("{TOKEN}\n")).expect("restore the token");
    drop(connect(&fake));
    assert_eq!(fake.connections(), 2);
}

/// `harness policy` parses, moves with its generation, and a refusal or an older
/// aterm's line is an error and never a policy.
#[test]
fn the_fake_aterms_policy_reads_as_a_policy() {
    let fake = root("policy");
    let mut connection = connect(&fake);
    let on = policy(&fake, &mut connection);
    assert_eq!(on.generation, 1);
    assert!(on.admits_claim() && on.relaunch && on.upgrade && !on.upgrade_ask);
    assert_eq!(on.human_grace, Duration::from_secs(120));

    fake.set_policy(&POLICY_ON.replace("effective=on why=enabled", "effective=off why=hold"));
    let off = policy(&fake, &mut connection);
    assert_eq!(off.generation, 2);
    assert!(off.serves_contract() && !off.effective && !off.admits_claim());

    // A halt is the policy off, `why=hold`, at the same generation.
    fake.set_policy(POLICY_ON);
    fake.set_status("hold", Some("1"));
    let held = policy(&fake, &mut connection);
    assert_eq!(held.generation, 3);
    assert!(!held.effective && held.why.as_deref() == Some("hold"));
    assert!(!held.off_by_configuration());
    fake.set_status("hold", Some("0"));
    assert!(policy(&fake, &mut connection).effective);

    // An aterm that predates contract 3 says nothing of contracts.
    fake.set_policy_reply("OK gen=12 effective=on why=config continue=on land=off questions=on");
    let older = ask(&mut connection, claim::policy_verb(&fake.sid));
    assert_eq!(Policy::parse(&older), Err(ReplyError::Missing("contract")));
    assert_eq!(claim::advertised_contract(&older), None);
    fake.set_policy_reply("ERR denied");
    let denied = ask(&mut connection, claim::policy_verb(&fake.sid));
    assert_eq!(
        Policy::parse(&denied).unwrap_err().to_string(),
        "reply=ERR:denied"
    );
    // An explicit ERR is a reply: the connection stays usable.
    assert!(!connection.is_poisoned());
    assert_eq!(fake.verbs(), ["harness policy"; 6]);
}

/// `native-act info` carries a bare word, reads through the roster guard, and
/// lists exactly the fake's rows: none by default, so no harness id is
/// rostered until a test puts its row there.
#[test]
fn the_fake_aterms_info_reply_reads_through_the_roster_guard() {
    let fake = root("info");
    let mut connection = connect(&fake);
    let empty = ask(&mut connection, claim::info_verb(&fake.sid));
    assert_eq!(empty, info_with_rows(&[]));
    assert!(empty.ends_with(" own-image=-"));
    // The strict reader refuses the bare `native-act`, so this reply must never
    // go through `request_fields`, which would poison the connection.
    assert!(Fields::parse(&empty).is_err());
    // No sibling is rostered by default: kimi-alab, muse-alab and grok-alab
    // can test their unrostered path too.
    for id in [
        ID,
        "kimi-alab",
        "muse-alab",
        "grok-alab",
        "claude-code-alab",
    ] {
        assert!(!claim::rostered(&empty, id), "{id}");
    }

    // Another harness's row is not this harness's.
    fake.set_row("kimi-alab", &Scope::CONTRACT_3);
    let others = ask(&mut connection, claim::info_verb(&fake.sid));
    assert!(others.ends_with(" own-image=kimi-alab:upgrade"));
    assert!(!claim::rostered(&others, ID));
    fake.set_row(ID, &Scope::CONTRACT_3);
    let both = ask(&mut connection, claim::info_verb(&fake.sid));
    assert_eq!(both, info_with_rows(&["kimi-alab", ID]));
    assert!(claim::rostered(&both, ID));
    // As in aterm's `own_image_rows`, only a row allowing upgrade is listed.
    fake.set_row(ID, &[Scope::Relaunch]);
    assert!(!claim::rostered(
        &ask(&mut connection, claim::info_verb(&fake.sid)),
        ID
    ));
    fake.set_row(ID, &Scope::CONTRACT_3);
    fake.set_info("ERR usage: native-act");
    assert!(!claim::rostered(
        &ask(&mut connection, claim::info_verb(&fake.sid)),
        ID
    ));
    fake.clear_info();
    assert!(claim::rostered(
        &ask(&mut connection, claim::info_verb(&fake.sid)),
        ID
    ));
    assert!(!connection.is_poisoned());
    // The guard is a read: nothing was claimed by asking.
    assert_eq!(fake.holder(), None);
    assert_eq!(fake.verbs(), ["native-act info"; 6]);
}

/// A rostered holder's claim is read against its row as aterm reads it: a
/// scope word the row does not allow refuses the whole claim, and an
/// uncontracted claim is refused, with nothing stored either way. Every row of
/// aterm's roster allows relaunch and upgrade only, so `turn-end` is refused
/// though the grammar knows it.
#[test]
fn a_rostered_holders_claim_outside_its_row_is_refused_as_aterm_refuses_it() {
    let fake = FakeAterm::for_harness("row", &config(&[Scope::Relaunch, Scope::Upgrade]));
    let mut connection = connect(&fake);
    let (_, holder) = me();
    let set = |connection: &mut Connection, words: &str| {
        connection
            .request_line(&format!(
                "@{} meta set supervisor {holder} {words}",
                fake.sid
            ))
            .expect("reply")
    };
    for (words, why) in [
        (
            "contract=3 scope=relaunch,turn-end version=0.1.0",
            "a scope word outside the harness's row",
        ),
        (
            "contract=3 scope=walls",
            "a scope word outside the harness's row",
        ),
        (
            "ttl=5000",
            "a rostered harness claims under contract=3 with its scope=",
        ),
    ] {
        assert_eq!(
            set(&mut connection, words),
            format!(
                "ERR usage: meta set supervisor <id>@<pid> contract=3 \
                 scope=<word>[,<word>…] [version=<MAJOR.MINOR.0>] [vendor=<version>]: {why}"
            ),
            "{words}"
        );
        assert_eq!(fake.holder(), None, "{words}");
        assert_eq!(status(&fake, &mut connection).supervisor, None);
    }
    assert_eq!(
        set(&mut connection, "contract=3 scope=relaunch,upgrade"),
        "OK"
    );
    assert!(status(&fake, &mut connection).held_by(&holder));
    // A narrower row refuses what it no longer allows.
    fake.set_row(ID, &[Scope::Relaunch]);
    assert!(
        set(&mut connection, "contract=3 scope=upgrade")
            .ends_with(": a scope word outside the harness's row")
    );
    assert_eq!(set(&mut connection, "contract=3 scope=relaunch"), "OK");
    assert_eq!(
        status(&fake, &mut connection).supervisor_scope,
        ["relaunch"]
    );
}

/// A claim is stored, `status` reads it back proven with its scope, the judge
/// calls a quiet session safe, and a release clears it.
#[test]
fn a_claim_on_the_fake_aterm_reads_back_proven_in_status() {
    let fake = root("claim");
    let mut connection = connect(&fake);
    let (pid, holder) = me();
    let before = status(&fake, &mut connection);
    assert_eq!(before.supervisor, None);
    assert!(!before.held_by(&holder) && before.supervisor_scope.is_empty());
    assert_eq!(before.supervisor_revision, Some(0));

    assert_eq!(claim(&fake, &mut connection, &[Scope::Upgrade]), "OK");
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    let held = status(&fake, &mut connection);
    assert!(held.held_by(&holder));
    assert!(held.takes(Scope::Upgrade) && !held.takes(Scope::Relaunch));
    assert_eq!(held.supervisor_revision, Some(1));
    assert_eq!(held.human, Human::Never);
    let on = policy(&fake, &mut connection);
    assert_eq!(
        judge_safe_point(&on, &held, &holder, Duration::ZERO),
        SafePoint::Safe
    );

    // One field at a time holds, by name.
    fake.set_status("composer", Some("draft"));
    let drafting = status(&fake, &mut connection);
    assert_eq!(
        judge_safe_point(&on, &drafting, &holder, Duration::ZERO),
        SafePoint::Hold(Hold {
            field: "composer",
            value: "draft".to_string(),
            grace_left: None,
        })
    );
    fake.set_status("composer", None);
    let SafePoint::Hold(hold) = judge_safe_point(
        &on,
        &status(&fake, &mut connection),
        &holder,
        Duration::ZERO,
    ) else {
        panic!("a status without composer= holds");
    };
    assert_eq!(hold.to_string(), "wait aterm composer=absent");
    fake.set_status("composer", Some("empty"));

    // A stored claim aterm did not prove is not this harness's to act under.
    fake.set_proven(false);
    let unproven = status(&fake, &mut connection);
    assert!(!unproven.held_by(&holder));
    assert_eq!(unproven.supervisor.as_deref(), Some(holder.as_str()));
    let SafePoint::Hold(hold) = judge_safe_point(&on, &unproven, &holder, Duration::ZERO) else {
        panic!("an unproven claim holds");
    };
    assert_eq!(hold.to_string(), "wait aterm supervisor_proven=0");
    fake.set_proven(true);

    // Claiming again on the same connection widens the scope in place.
    assert_eq!(
        claim(&fake, &mut connection, &[Scope::Upgrade, Scope::Relaunch]),
        "OK"
    );
    let wide = status(&fake, &mut connection);
    assert!(wide.held_by(&holder) && wide.takes(Scope::Relaunch) && wide.takes(Scope::Upgrade));

    assert_eq!(
        ask(
            &mut connection,
            claim::release_verb(&config(&[Scope::Upgrade]), &fake.sid, pid)
        ),
        "OK"
    );
    assert_eq!(fake.holder(), None);
    assert_eq!(status(&fake, &mut connection).supervisor, None);

    let verbs = fake.verbs();
    assert_eq!(verbs.first().map(String::as_str), Some("status"));
    assert_eq!(
        verbs.get(1).map(String::as_str),
        Some(
            format!(
                "meta set supervisor {holder} contract=3 scope=upgrade version=0.1.0 \
                 vendor=2.1.311"
            )
            .as_str()
        )
    );
    assert!(verbs.contains(&format!(
        "meta set supervisor {holder} contract=3 scope=relaunch,upgrade version=0.1.0 \
         vendor=2.1.311"
    )));
    assert_eq!(
        verbs.iter().rev().nth(1).map(String::as_str),
        Some(format!("meta unset supervisor holder={holder}").as_str())
    );
}

/// The settable claim reply refuses without storing, another holder's standing
/// claim answers `ERR busy`, and a release naming someone else changes nothing.
#[test]
fn a_refused_or_contended_claim_on_the_fake_aterm_stores_nothing() {
    let fake = root("refused");
    let mut connection = connect(&fake);
    let (pid, holder) = me();
    let unproven = "ERR unproven: a contract 3 claim needs its own direct connection, the \
                    session's foreground group and a build of its program from atpkg";
    fake.set_claim_reply(unproven);
    assert_eq!(claim(&fake, &mut connection, &[Scope::Upgrade]), unproven);
    assert_eq!(fake.holder(), None);
    assert_eq!(status(&fake, &mut connection).supervisor, None);
    fake.set_claim_reply("OK");

    // An unregistered contract-4 holder is answered `ERR unproven` and
    // nothing is stored (HX-06); contract 3 is not consulted on it.
    fake.set_registered(false);
    assert_eq!(
        claim_at(
            &fake,
            &mut connection,
            Contract::Four,
            &[Scope::Upgrade],
            None
        ),
        crate::fake::UNPROVEN_4
    );
    assert_eq!(fake.holder(), None);
    assert_eq!(status(&fake, &mut connection).supervisor, None);
    fake.set_registered(true);

    // Another program holds the session from its own connection.
    let mut other = connect(&fake);
    assert_eq!(
        other
            .request_line(&format!(
                "@{} meta set supervisor someone-else@77",
                fake.sid
            ))
            .expect("reply"),
        "OK"
    );
    let foreign = status(&fake, &mut connection);
    assert_eq!(foreign.supervisor.as_deref(), Some("someone-else@77"));
    // A claim without `contract=` takes every duty.
    assert!(foreign.takes(Scope::Upgrade) && foreign.takes(Scope::Relaunch));
    assert!(!foreign.held_by(&holder));
    assert_eq!(
        claim(&fake, &mut connection, &[Scope::Upgrade]),
        "ERR busy supervisor=someone-else@77"
    );
    // Releasing by name leaves the other holder's claim alone.
    assert_eq!(
        ask(
            &mut connection,
            claim::release_verb(&config(&[Scope::Upgrade]), &fake.sid, pid)
        ),
        "OK"
    );
    assert_eq!(fake.holder().as_deref(), Some("someone-else@77"));
    drop(other);
    eventually("the other holder's connection closes", || {
        fake.holder().is_none()
    });
    assert_eq!(claim(&fake, &mut connection, &[Scope::Upgrade]), "OK");
    assert!(status(&fake, &mut connection).held_by(&holder));
}

/// A claim is bound to its connection: closing it releases the claim, and
/// another connection reads the session unclaimed.
#[test]
fn closing_the_claims_connection_releases_it_on_the_fake_aterm() {
    let fake = root("drop");
    let (_, holder) = me();
    let mut claimant = connect(&fake);
    let mut observer = connect(&fake);
    assert_eq!(claim(&fake, &mut claimant, &[Scope::Upgrade]), "OK");
    let seen = status(&fake, &mut observer);
    assert!(seen.held_by(&holder));
    let revision = seen.supervisor_revision.expect("revision");

    // The observer going away releases nothing: the claim is the claimant's
    // connection's, not the last one's.
    drop(observer);
    assert_eq!(fake.connections(), 2);
    assert!(status(&fake, &mut claimant).held_by(&holder));

    drop(claimant);
    eventually("the claim goes with its connection", || {
        fake.holder().is_none()
    });
    let mut later = connect(&fake);
    let after = status(&fake, &mut later);
    assert_eq!(after.supervisor, None);
    assert!(!after.supervisor_proven && after.supervisor_scope.is_empty());
    assert!(after.supervisor_revision.expect("revision") > revision);
}

/// A subscription is acknowledged with a numeric tag this crate's strict reader
/// accepts, and events arrive on a descriptor that polls readable.
#[test]
fn events_from_the_fake_aterm_reach_this_crates_reader() {
    let fake = root("events");
    let socket = fake.locator().socket().expect("socket");
    // An event from before the subscription is not replayed.
    fake.event("human");
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe");
    eventually("the subscription is counted", || fake.subscriptions() == 1);
    assert!(!events.drain().requires_refresh());

    fake.event("status");
    let update = await_update("a status event", &mut events, |update| update.activity);
    assert!(!update.policy_changed && update.restart.is_none() && update.fault.is_none());

    fake.set_policy(POLICY_ON);
    let update = await_update("a policy event", &mut events, |update| {
        update.policy_changed
    });
    assert!(update.requires_refresh() && !update.ended);
    // The event says only that the policy moved; the policy is read again.
    let mut connection = connect(&fake);
    assert_eq!(policy(&fake, &mut connection).generation, 2);

    // A kind the reader has no use for is read past.
    fake.event("meta field=title value=-");
    fake.event("turn 7 submitted=1 status=done dur_ms=12 by=-");
    let update = await_update("a turn event", &mut events, |update| update.activity);
    assert!(update.fault.is_none() && !update.ended);

    // A subscription for another session is refused outright.
    assert!(EventReader::connect(&socket, "s-ffff", TIMEOUT).is_err());
    assert_eq!(fake.subscriptions(), 1);
}

/// The restart sender's line parses as a restart with its cause.
#[test]
fn a_restart_event_from_the_fake_aterm_parses() {
    let fake = root("restart-event");
    let socket = fake.locator().socket().expect("socket");
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe");
    fake.send_restart("asked");
    let update = await_update("a restart event", &mut events, |update| {
        update.restart.is_some()
    });
    assert_eq!(
        update.restart.map(|restart| restart.cause).as_deref(),
        Some("asked")
    );
    assert!(update.fault.is_none() && !update.ended && !update.policy_changed);
    fake.send_restart("stall");
    let update = await_update("a second restart event", &mut events, |update| {
        update.restart.is_some()
    });
    assert_eq!(
        update.restart.map(|restart| restart.cause).as_deref(),
        Some("stall")
    );
}

/// An account move from the fake reads whole: the label, and the home
/// percent-encoded on the wire and absolute once read, or none for `-`.
#[test]
fn an_account_move_from_the_fake_aterm_parses_with_its_account() {
    let fake = root("account-event");
    let socket = fake.locator().socket().expect("socket");
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe");
    let home = std::path::Path::new("/Users/_owner/.kimi homes/b");
    fake.send_account_restart("account", "b", Some(home));
    let update = await_update("an account move", &mut events, |update| {
        update.restart.is_some()
    });
    assert_eq!(
        update.restart,
        Some(crate::Restart {
            cause: "account".to_string(),
            account: Some(crate::Account {
                label: "b".to_string(),
                home: Some(home.to_path_buf()),
            }),
            model: None,
        })
    );
    fake.send_account_restart("account-ahead", "work", None);
    let update = await_update("a move to the default home", &mut events, |update| {
        update.restart.is_some()
    });
    let restart = update.restart.expect("a restart");
    assert_eq!(restart.cause, "account-ahead");
    assert_eq!(
        restart.account,
        Some(crate::Account {
            label: "work".to_string(),
            home: None,
        })
    );
}

/// A model move from the fake reads with its model (HX-06:
/// `restart cause=model model=<word>`).
#[test]
fn a_model_move_from_the_fake_aterm_parses_with_its_model() {
    let fake = root("model-event");
    let socket = fake.locator().socket().expect("socket");
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe");
    fake.send_model_restart("model", "opus[1m]");
    let update = await_update("a model move", &mut events, |update| {
        update.restart.is_some()
    });
    assert_eq!(
        update.restart,
        Some(crate::Restart {
            cause: "model".to_string(),
            account: None,
            model: Some("opus[1m]".to_string()),
        })
    );
}

/// An aterm restart drops every connection and the claim; the client sees its
/// request connection fail and its event stream end, finds the socket again
/// through the graph, and claims again.
#[test]
fn restarting_the_fake_aterm_drops_every_connection_and_the_claim() {
    let fake = root("restart");
    let (_, holder) = me();
    let socket = fake.locator().socket().expect("socket");
    let mut connection = connect(&fake);
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe");
    assert_eq!(claim(&fake, &mut connection, &[Scope::Upgrade]), "OK");
    assert!(status(&fake, &mut connection).held_by(&holder));

    fake.restart();
    assert_eq!(fake.holder(), None);
    let request = claim::status_verb(&fake.sid).expect("status verb");
    assert!(connection.request_line(&request).is_err());
    // A failed exchange poisons the connection: nothing more is sent on it.
    assert!(connection.is_poisoned());
    assert!(connection.request_line(&request).is_err());
    let update = await_update("the event stream ends", &mut events, |update| update.ended);
    assert!(update.requires_refresh());
    assert!(events.drain().ended);

    let mut again = connect(&fake);
    assert_eq!(status(&fake, &mut again).supervisor, None);
    assert_eq!(claim(&fake, &mut again, &[Scope::Upgrade]), "OK");
    assert!(status(&fake, &mut again).held_by(&holder));
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe again");
    fake.event("agent");
    assert!(
        await_update("an event after the restart", &mut events, |update| update
            .activity)
        .fault
        .is_none()
    );
    assert_eq!(fake.subscriptions(), 2);
}

/// Ending the event streams drops only the subscriptions: the client's stream
/// ends while its request connection and its claim stay, and a subscription
/// made afterwards is served (or refused, as set) as before.
#[test]
fn ending_the_event_streams_keeps_the_request_connection_and_the_claim() {
    let fake = root("end-events");
    let (_, holder) = me();
    let socket = fake.locator().socket().expect("socket");
    let mut connection = connect(&fake);
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe");
    assert_eq!(claim(&fake, &mut connection, &[Scope::Upgrade]), "OK");

    fake.end_events();
    let update = await_update("the event stream ends", &mut events, |update| update.ended);
    assert!(update.requires_refresh() && update.fault.is_none());
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    assert!(status(&fake, &mut connection).held_by(&holder));

    fake.set_subscribe_reply(Some("ERR busy"));
    assert!(EventReader::connect(&socket, &fake.sid, TIMEOUT).is_err());
    fake.set_subscribe_reply(None);
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe again");
    fake.event("agent");
    assert!(
        await_update("an event on the new stream", &mut events, |update| update
            .activity)
        .fault
        .is_none()
    );
    assert_eq!(fake.subscriptions(), 2);
    assert!(status(&fake, &mut connection).held_by(&holder));
}

/// The fake's subscription ACK is byte for byte the shape a real aterm sent
/// on 2026-11-13: `OK subscribe 1`, then `sub <local> <sid> role=- attention=-
/// user_title=-`. It is read here off the raw socket, not through the client,
/// so a client that grew lenient could not hide a fake that drifted.
#[test]
fn the_fakes_subscribe_ack_is_the_shape_a_real_aterm_sends() {
    let fake = root("ack");
    let socket = fake.locator().socket().expect("socket");
    let mut stream = UnixStream::connect(&socket).expect("connect");
    stream.set_read_timeout(Some(TIMEOUT)).expect("read bound");
    stream
        .write_all(format!("AUTH {TOKEN}\nsubscribe @{} events\n", fake.sid).as_bytes())
        .expect("subscribe");
    let mut reader = BufReader::new(stream);
    let (mut header, mut row) = (String::new(), String::new());
    reader.read_line(&mut header).expect("header");
    reader.read_line(&mut row).expect("row");
    assert_eq!(header, "OK subscribe 1\n");
    assert_eq!(
        row,
        format!("sub 1 {} role=- attention=- user_title=-\n", fake.sid)
    );
    assert_eq!(subscribe_ack(&fake.sid), format!("{header}{row}"));
    // The row's words, as the measured reply spelled them.
    let words: Vec<&str> = row.split_ascii_whitespace().collect();
    assert_eq!(words.len(), 6);
    assert_eq!(&words[..3], ["sub", "1", fake.sid.as_str()]);
    assert_eq!(&words[3..], ["role=-", "attention=-", "user_title=-"]);
    // And this crate's strict reader accepts exactly that.
    assert!(EventReader::connect(&socket, &fake.sid, TIMEOUT).is_ok());
}

/// The fake reads a claim's words as aterm does: contract 3 with known scope
/// words stored in aterm's order, an uncontracted claim taking every duty,
/// and anything else refused with aterm's usage line and nothing stored.
#[test]
fn the_fake_reads_a_claims_words_as_aterm_does() {
    let fake = root("grammar");
    let mut connection = connect(&fake);
    let (pid, holder) = me();
    let set = |connection: &mut Connection, words: &str| {
        connection
            .request_line(&format!(
                "@{} meta set supervisor {holder} {words}",
                fake.sid
            ))
            .expect("reply")
    };
    for (words, why) in [
        (
            "contract=5 scope=relaunch",
            "a scoped claim names a contract aterm serves",
        ),
        ("contract=3", "a v3 claim names its scope="),
        ("contract=3 scope=relaunch,state", "an unknown scope word"),
        (
            "contract=3 scope=relaunch,relaunch",
            "a scope word is named twice",
        ),
        (
            "contract=3 scope=relaunch ttl=5",
            "a v3 claim takes contract=",
        ),
        (
            "contract=3 scope=relaunch bare",
            "a claim word is key=value",
        ),
        (
            "contract=3 contract=3 scope=upgrade",
            "a claim word is named twice",
        ),
    ] {
        let reply = set(&mut connection, words);
        assert!(
            reply.starts_with("ERR usage: meta set supervisor") && reply.contains(why),
            "{words}: {reply}"
        );
        assert_eq!(fake.holder(), None, "{words}");
    }
    assert_eq!(
        set(
            &mut connection,
            "contract=3 scope=upgrade,relaunch version=0.1.0 vendor=2.1.311 login=work"
        ),
        "OK"
    );
    let words = fake.claim_words().expect("a claim");
    assert_eq!(words.holder, holder);
    assert_eq!(words.login.as_deref(), Some("work"));
    let held = status(&fake, &mut connection);
    assert_eq!(held.supervisor_scope, ["relaunch", "upgrade"]);
    assert!(held.held_by(&holder));
    assert_eq!(
        ask(
            &mut connection,
            claim::release_verb(&config(&[Scope::Upgrade]), &fake.sid, pid)
        ),
        "OK"
    );
    // An uncontracted claim takes every duty contract 3 knows.
    assert_eq!(set(&mut connection, "ttl=5000"), "OK");
    let every = status(&fake, &mut connection);
    assert!(Scope::ALL.iter().all(|scope| every.takes(*scope)));
}

/// A fake made for a harness's config is found through that config's own
/// variable names and carries the harness's roster row and program.
#[test]
fn a_fake_for_a_harness_is_found_through_its_config() {
    let config = config(&[Scope::Relaunch, Scope::Upgrade]);
    let fake = FakeAterm::for_harness("config", &config);
    let names = config.env();
    assert!(names.hosted_by(fake.lookup(names)));
    let located = Locator::from_lookup(names, fake.lookup(names)).expect("locator");
    assert_eq!(located, fake.locator());
    let own = EnvNames {
        session: "MY_SID".to_string(),
        runtime_dir: "MY_RUN".to_string(),
        home: "MY_HOME".to_string(),
        hosted: vec![crate::HostedMark {
            name: "MY_HOST".to_string(),
            value: None,
        }],
    };
    assert!(own.hosted_by(fake.lookup(&own)));
    assert!(!names.hosted_by(fake.lookup(&own)));
    assert_eq!(
        Locator::from_lookup(&own, fake.lookup(&own)).expect("locator"),
        fake.locator()
    );
    let mut connection = connect(&fake);
    assert!(claim::rostered(
        &ask(&mut connection, claim::info_verb(&fake.sid)),
        ID
    ));
    // aterm's `program=` is the foreground leader's image, the harness itself,
    // never the vendor word.
    assert!(fake.status_line().contains(" program=example-alab "));
    assert!(!fake.status_line().contains(" program=example "));
    let root = fake.scratch.clone().expect("a scratch root");
    let sock_dir = fake.sock_dir.clone();
    drop(connection);
    drop(fake);
    assert!(!root.exists() && !sock_dir.exists());
}

// ─────────────────────────────────── contract 4 ───────────────────────────────────

/// The fake serves contracts 3 and 4 beside the uncontracted claim, and can
/// be told to serve 3 alone: then a contract-4 claim is refused and the client
/// chooses 3.
#[test]
fn the_fake_serves_contracts_3_and_4_and_can_serve_3_alone() {
    let fake = root("contracts");
    let mut connection = connect(&fake);
    let served = policy(&fake, &mut connection);
    assert_eq!(served.contracts, [2, 3, 4]);
    assert_eq!(served.contract(), Some(Contract::Four));
    fake.serve_contracts(&[3]);
    let three = policy(&fake, &mut connection);
    assert_eq!(three.contracts, [3]);
    assert_eq!(three.contract(), Some(Contract::Three));
    assert!(three.generation > served.generation);
    assert!(
        claim_at(
            &fake,
            &mut connection,
            Contract::Four,
            &[Scope::Relaunch],
            None
        )
        .ends_with(": a scoped claim names a contract aterm serves")
    );
    assert_eq!(fake.holder(), None);
    assert_eq!(claim(&fake, &mut connection, &[Scope::Relaunch]), "OK");
    fake.serve_contracts(&[2, 3, 4]);
    assert_eq!(
        policy(&fake, &mut connection).contract(),
        Some(Contract::Four)
    );
}

/// A contract-4 claim drops a word the fake does not know and grants the rest,
/// with no roster row consulted, and answers with the grant and the claim's
/// revision; `child=` is kept for a test to read. Its grammar refuses a claim
/// without `version=`, a malformed `child=`, and a word named twice; contract
/// 3 still has no `child=`.
#[test]
fn a_contract_4_claim_drops_unknown_words_and_names_its_grant_and_revision() {
    let fake = root("grant");
    let mut connection = connect(&fake);
    let (_, holder) = me();
    let set = |connection: &mut Connection, words: &str| {
        connection
            .request_line(&format!(
                "@{} meta set supervisor {holder} {words}",
                fake.sid
            ))
            .expect("reply")
    };
    assert_eq!(
        set(
            &mut connection,
            "contract=4 scope=relaunch,telepathy,state version=0.1.0 child=5151:99"
        ),
        "OK scope=relaunch,state revision=1"
    );
    assert_eq!(fake.child().as_deref(), Some("5151:99"));
    let held = status(&fake, &mut connection);
    assert!(held.held_by(&holder));
    assert_eq!(held.supervisor_scope, ["relaunch", "state"]);
    assert_eq!(held.supervisor_revision, Some(1));
    assert!(fake.status_line().contains(" supervisor_contract=4 "));

    // An aterm that knows fewer words drops them too, and the client keeps
    // what it asked for of what was granted.
    fake.set_known(&[Scope::Relaunch, Scope::Upgrade]);
    let asked = [
        Scope::Relaunch,
        Scope::Upgrade,
        Scope::State,
        Scope::Accounts,
    ];
    let reply = claim_at(
        &fake,
        &mut connection,
        Contract::Four,
        &asked,
        Some(ChildProcess {
            pid: 6262,
            start: 7,
        }),
    );
    assert_eq!(reply, "OK scope=relaunch,upgrade revision=2");
    assert_eq!(
        Grant::of_reply(&reply, &asked),
        Ok(Grant {
            contract: Contract::Four,
            scopes: vec![Scope::Relaunch, Scope::Upgrade],
            revision: Some(2),
        })
    );
    assert_eq!(fake.child().as_deref(), Some("6262:7"));
    // Nothing known is granted nothing, and the claim still stands.
    assert_eq!(
        set(&mut connection, "contract=4 scope=state version=0.1.0"),
        "OK scope=- revision=3"
    );
    assert_eq!(
        status(&fake, &mut connection).supervisor_scope,
        Vec::<String>::new()
    );
    assert_eq!(fake.child(), None);

    for (words, why) in [
        ("contract=4 scope=relaunch", "a v4 claim names its version="),
        ("contract=4 version=0.1.0", "a v4 claim names its scope="),
        (
            "contract=4 scope=relaunch,relaunch version=0.1.0",
            "a scope word is named twice",
        ),
        (
            "contract=4 scope=relaunch,,upgrade version=0.1.0",
            "an empty scope word",
        ),
        (
            "contract=4 scope=relaunch version=0.1.0 child=5151",
            "child= is <pid>:<start>",
        ),
        (
            "contract=4 scope=relaunch version=0.1.0 child=1:5",
            "child= is <pid>:<start>",
        ),
        (
            "contract=4 scope=relaunch version=0.1.0 child=05:1",
            "child= is <pid>:<start>",
        ),
        (
            "contract=4 scope=relaunch version=0.1.0 child=5:x",
            "child= is <pid>:<start>",
        ),
        (
            "contract=4 scope=relaunch version=0.1.0 ttl=5",
            "a v3 claim takes contract=",
        ),
        (
            "contract=3 scope=relaunch child=5151:1",
            "a v3 claim takes contract= scope= version= vendor= login=",
        ),
    ] {
        let reply = set(&mut connection, words);
        assert!(
            reply.starts_with("ERR usage: meta set supervisor") && reply.contains(why),
            "{words}: {reply}"
        );
    }
    assert_eq!(fake.revision(), 3, "a refused claim moves no revision");
}

/// A person's cap narrows a live contract-4 claim to it, raises the claim's
/// revision and says `harness-policy`; the policy reads `scopes=`; a later
/// claim is granted only the cap; a contract-3 claim is narrowed too; and a
/// lifted cap gives a live claim back nothing unless the fake restores.
#[test]
fn a_cap_narrows_a_live_contract_4_claim_raises_its_revision_and_says_so() {
    let fake = root("cap");
    let socket = fake.locator().socket().expect("socket");
    let mut events = EventReader::connect(&socket, &fake.sid, TIMEOUT).expect("subscribe");
    let mut connection = connect(&fake);
    let both = [Scope::Relaunch, Scope::Upgrade];
    assert_eq!(
        claim_at(&fake, &mut connection, Contract::Four, &both, None),
        "OK scope=relaunch,upgrade revision=1"
    );
    fake.set_cap(Some(&[Scope::Upgrade, Scope::State]));
    await_update("the cap's policy event", &mut events, |update| {
        update.policy_changed
    });
    let capped = status(&fake, &mut connection);
    assert_eq!(capped.supervisor_scope, ["upgrade"]);
    assert_eq!(capped.supervisor_revision, Some(2));
    assert!(
        policy(&fake, &mut connection).generation > 1
            && ask(&mut connection, claim::policy_verb(&fake.sid))
                .contains(" scopes=upgrade,state")
    );
    assert_eq!(
        claim_at(&fake, &mut connection, Contract::Four, &both, None),
        "OK scope=upgrade revision=3"
    );
    // A cap that withholds nothing from the live claim moves no revision.
    fake.set_cap(Some(&Scope::ALL));
    await_update("the wide cap's policy event", &mut events, |update| {
        update.policy_changed
    });
    assert_eq!(fake.revision(), 3);
    // A cap narrows every live claim it covers, a contract-3 claim's too.
    assert_eq!(claim(&fake, &mut connection, &both), "OK");
    assert_eq!(fake.revision(), 4);
    fake.set_cap(Some(&[Scope::Upgrade]));
    await_update("the third cap's policy event", &mut events, |update| {
        update.policy_changed
    });
    let capped = status(&fake, &mut connection);
    assert_eq!(capped.supervisor_scope, ["upgrade"]);
    assert_eq!(capped.supervisor_revision, Some(5));
    // Lifted, it gives nothing back, unless the fake is told aterm restores.
    fake.set_cap(None);
    await_update("the lifted cap's policy event", &mut events, |update| {
        update.policy_changed
    });
    assert_eq!(status(&fake, &mut connection).supervisor_scope, ["upgrade"]);
    fake.set_cap_restores(true);
    fake.set_cap(Some(&Scope::ALL));
    await_update("the restoring cap's policy event", &mut events, |update| {
        update.policy_changed
    });
    let restored = status(&fake, &mut connection);
    assert_eq!(restored.supervisor_scope, ["relaunch", "upgrade"]);
    assert_eq!(restored.supervisor_revision, Some(6));
}
