// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The link's pure rules, and the link driven against the fake aterm.

use std::time::Instant;

use super::*;
use crate::fake::{FakeAterm, POLICY_ON};
use crate::stream::{self, Incoming};

const ID: &str = "example-alab";
/// How long a scenario waits for the fake's threads before it fails.
const HANG: Duration = Duration::from_secs(20);

fn config() -> Config {
    Config::new(ID, "example", &[Scope::Relaunch, Scope::Upgrade], "0.2.0").expect("a config")
}

fn child() -> Child<'static> {
    Child {
        vendor: Some("2.1.311"),
        login: None,
        process: None,
    }
}

#[test]
fn a_hold_is_read_again_after_the_grace_and_within_five_seconds() {
    assert_eq!(recheck_after_ms(None), RECLAIM_EVERY_MS);
    assert_eq!(recheck_after_ms(Some(Duration::ZERO)), 1_000);
    assert_eq!(recheck_after_ms(Some(Duration::from_millis(1_500))), 2_500);
    assert_eq!(recheck_after_ms(Some(Duration::from_secs(4))), 5_000);
    assert_eq!(recheck_after_ms(Some(Duration::from_secs(119))), 5_000);
    assert_eq!(recheck_after_ms(Some(Duration::MAX)), 5_000);
}

#[test]
fn a_refused_reply_is_two_journal_words() {
    assert!(ok("OK") && ok("OK gen=1") && !ok("OKAY") && !ok("ERR busy") && !ok(""));
    assert_eq!(
        reply_words("ERR busy supervisor=someone-else@77"),
        "reply=ERR:busy"
    );
    assert_eq!(reply_words("ERR"), "reply=ERR");
    assert_eq!(reply_words(""), "reply=(empty)");
}

/// The contracts a config declares unless it says otherwise: 3 and 4.
const BOTH: [Contract; 2] = Config::DEFAULT_CONTRACTS;

#[test]
fn a_policy_admits_a_claim_only_with_a_declared_contract_in_effect() {
    let policy = |contract: &str, effective: &str| {
        Policy::parse(&format!(
            "OK gen=1 effective={effective} contract={contract} relaunch=true upgrade=true \
             upgrade_ask=false human_grace_s=120"
        ))
        .expect("a policy")
    };
    let admits = |policy: &Policy| admits(policy, &BOTH);
    assert_eq!(admits(&policy("2,3", "on")), Ok(()));
    assert_eq!(admits(&policy("4", "on")), Ok(()));
    assert_eq!(admits(&policy("2,3,4", "on")), Ok(()));
    // An aterm that serves no contract the harness declares may serve one
    // after its update: asked again on the clock.
    assert_eq!(
        admits(&policy("2", "on")),
        Err(Failure::Unserved(
            "policy contract=2 declared=3,4".to_string()
        ))
    );
    // A harness that declares `[4]` (D11) is not admitted by `2,3`.
    let four = [Contract::Four];
    assert_eq!(
        super::admits(&policy("2,3", "on"), &four),
        Err(Failure::Unserved(
            "policy contract=2,3 declared=4".to_string()
        ))
    );
    assert_eq!(super::admits(&policy("2,3,4", "on"), &four), Ok(()));
    // Off by a setting a person can switch on again while the session
    // runs, or by a fact of the window that stands.
    for why in ["disabled", "agent-off", "autopilot-off"] {
        assert_eq!(
            admits(&policy("2,3", &format!("off why={why}"))),
            Err(Failure::Off(format!("policy effective=off why={why}")))
        );
    }
    for why in ["unhosted", "headless"] {
        assert_eq!(
            admits(&policy("2,3", &format!("off why={why}"))),
            Err(Failure::Refused(format!("policy effective=off why={why}")))
        );
    }
    // A halt, aterm's own update and a reason not known here pass: the
    // answer stands for now only.
    for why in ["hold", "suspended", "something-new"] {
        assert_eq!(
            admits(&policy("2,3", &format!("off why={why}"))),
            Err(Failure::Later(format!("policy effective=off why={why}")))
        );
    }
    assert_eq!(
        admits(&policy("2,3", "off")),
        Err(Failure::Later("policy effective=off".to_string()))
    );
    // The contract is named first: a host that does not serve it has no
    // policy this library reads.
    assert_eq!(
        admits(&policy("2,5", "off")).unwrap_err().why(),
        "policy contract=2,5 declared=3,4"
    );
}

#[test]
fn a_reply_aterm_could_not_give_now_is_asked_again_and_a_bad_ok_is_refused() {
    assert_eq!(
        unreadable(
            "policy",
            &Policy::parse("ERR aterm is showing a dialog; answer it, then retry").unwrap_err()
        ),
        Failure::Later("policy reply=ERR:aterm".to_string())
    );
    assert_eq!(
        unreadable(
            "status",
            &Status::parse("ERR main thread stalled 6s; retry").unwrap_err()
        ),
        Failure::Later("status reply=ERR:main".to_string())
    );
    // Any other refusal stands: an unknown session, a principal aterm
    // denies, a verb it does not serve.
    for reply in [
        "ERR denied",
        "ERR no such session 3",
        "ERR usage: status",
        "",
    ] {
        assert!(
            matches!(
                unreadable("status", &Status::parse(reply).unwrap_err()),
                Failure::Refused(_)
            ),
            "{reply:?}"
        );
        assert!(
            matches!(
                unreadable("policy", &Policy::parse(reply).unwrap_err()),
                Failure::Refused(_)
            ),
            "{reply:?}"
        );
    }
    assert_eq!(
        unreadable("policy", &Policy::parse("OK effective=on").unwrap_err()),
        Failure::Refused("policy gen=absent".to_string())
    );
    assert_eq!(
        unreadable("status", &Status::parse("OK hold=0").unwrap_err()),
        Failure::Refused("status schema=absent".to_string())
    );
}

#[test]
fn the_claim_line_names_the_holder_the_configs_duties_and_the_vendor_build() {
    let config = config();
    let three = Grant {
        contract: Contract::Three,
        scopes: config.scopes().to_vec(),
        revision: Some(136),
    };
    assert_eq!(
        claimed_line(&three, "example-alab@77", child()),
        "aterm claimed holder=example-alab@77 contract=3 scope=relaunch,upgrade revision=136 \
         vendor=2.1.311"
    );
    let four = Grant {
        contract: Contract::Four,
        scopes: Vec::new(),
        revision: Some(2),
    };
    assert_eq!(
        claimed_line(&four, "example-alab@77", Child::default()),
        "aterm claimed holder=example-alab@77 contract=4 scope=- revision=2 vendor=-"
    );
    assert_eq!(
        claim::claim_verb(&config, Contract::Three, "s-1a", 77, child()).as_deref(),
        Some(
            "@s-1a meta set supervisor example-alab@77 contract=3 scope=relaunch,upgrade \
             version=0.2.0 vendor=2.1.311"
        )
    );
}

#[test]
fn the_proof_needs_the_holder_aterms_proof_and_every_claimed_duty() {
    let status = |fields: &str| Status::parse(&format!("OK schema=1 {fields}")).unwrap();
    let both = [Scope::Relaunch, Scope::Upgrade];
    let holder = "example-alab@77";
    let full =
        status("supervisor=example-alab@77 supervisor_proven=1 supervisor_scope=relaunch,upgrade");
    assert_eq!(proof(&full, holder, &both), Ok(()));
    assert_eq!(proof(&full, holder, &[Scope::Upgrade]), Ok(()));
    for (fields, why) in [
        ("", "unproven supervisor=- supervisor_proven=0"),
        (
            "supervisor=other@9 supervisor_proven=1 supervisor_scope=relaunch,upgrade",
            "unproven supervisor=other@9 supervisor_proven=1",
        ),
        (
            "supervisor=example-alab@77 supervisor_proven=0 supervisor_scope=relaunch,upgrade",
            "unproven supervisor=example-alab@77 supervisor_proven=0",
        ),
        (
            "supervisor=example-alab@77 supervisor_proven=1 supervisor_scope=upgrade",
            "unproven scope=relaunch supervisor_scope=upgrade",
        ),
        (
            "supervisor=example-alab@77 supervisor_proven=1 supervisor_scope=-",
            "unproven scope=relaunch supervisor_scope=-",
        ),
    ] {
        assert_eq!(
            proof(&status(fields), holder, &both),
            Err(Failure::Refused(why.to_string())),
            "{fields:?}"
        );
    }
}

#[test]
fn only_a_timeout_after_the_handshake_is_busy() {
    assert_eq!(
        busy(Failure::Gone("policy timed out".to_string()), true),
        Failure::Later("policy timed out (aterm answered the handshake)".to_string())
    );
    // A reply no client reads, or a connection aterm closed, is not busy.
    let unreadable = Failure::Gone("policy reply has no OK/ERR header".to_string());
    assert_eq!(busy(unreadable.clone(), false), unreadable);
    for kept in [
        Failure::Refused("unrostered".to_string()),
        Failure::Off("policy effective=off why=disabled".to_string()),
        Failure::Later("status reply=ERR:main".to_string()),
    ] {
        assert_eq!(busy(kept.clone(), true), kept);
        assert_eq!(busy(kept.clone(), false), kept);
    }
    // A subscription aterm answered with anything but its ACK stands; one
    // that could not be opened is gone.
    let refused = std::io::Error::new(std::io::ErrorKind::InvalidData, "no ACK");
    assert_eq!(
        events_failure(&refused),
        Failure::Refused("events no ACK".to_string())
    );
    let closed = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed");
    assert_eq!(
        events_failure(&closed),
        Failure::Gone("events closed".to_string())
    );
}

// ─────────────────────────── the link against the fake ───────────────────────────

/// A millisecond clock for a scenario: real time since it began, plus any
/// time a test says has passed.
struct Clock(Instant, u64);

impl Clock {
    fn new() -> Self {
        Self(Instant::now(), 1_000_000)
    }

    fn now(&self) -> u64 {
        self.1 + u64::try_from(self.0.elapsed().as_millis()).unwrap()
    }

    fn pass(&mut self, ms: u64) {
        self.1 += ms;
    }
}

fn launch(fake: &FakeAterm, clock: &Clock) -> Link {
    let config = config();
    let lookup = fake.lookup(config.env());
    Link::launch_with(config, std::process::id(), child(), clock.now(), lookup)
}

/// Drain the link's events until `wanted` holds, waiting on its descriptor.
fn await_events(
    what: &str,
    link: &mut Link,
    clock: &Clock,
    wanted: impl Fn(&Drained, &Link) -> bool,
) -> Drained {
    let until = Instant::now() + HANG;
    loop {
        if let Some(fd) = link.event_fd() {
            let mut probe = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: probe is one initialized poll record for the link's open descriptor.
            let _ = unsafe { libc::poll(&raw mut probe, 1, 50) };
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
        let drained = link.events(child(), clock.now());
        if wanted(&drained, link) {
            return drained;
        }
        assert!(Instant::now() < until, "hang detector: {what}");
    }
}

#[test]
fn outside_aterm_nothing_is_attempted_and_nothing_is_noted() {
    let fake = FakeAterm::for_harness("outside", &config());
    let mut link = Link::launch_with(config(), std::process::id(), child(), 0, |_| None);
    assert!(link.is_absent() && !link.claims() && !link.judges());
    assert!(link.take_notes().is_empty());
    assert_eq!(fake.connections(), 0);
    assert_eq!(
        link.relaunch_word(false, 0),
        Relaunch::Denied("no-claim".to_string())
    );
    assert_eq!(link.safe_point(Duration::ZERO, 0), Verdict::Local);
}

/// Under contract 3 a harness off aterm's roster sends no claim, not even to
/// find out: aterm would store it as a foreign claim taking every duty.
#[test]
fn an_unrostered_harness_reads_and_never_claims() {
    let fake = FakeAterm::scratch("unrostered");
    fake.serve_contracts(&[2, 3]);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_absent());
    assert_eq!(link.take_notes(), ["aterm unclaimed unrostered"]);
    // The guard is a read: nothing was claimed by asking.
    assert_eq!(fake.verbs(), ["harness policy", "native-act info"]);
    assert_eq!(fake.holder(), None);
    assert_eq!(fake.subscriptions(), 0);
}

fn adopt(fake: &FakeAterm, clock: &Clock) -> Link {
    let config = config();
    let lookup = fake.lookup(config.env());
    Link::adopt_with(config, std::process::id(), child(), clock.now(), lookup)
}

/// A harness image that took over a running child from an earlier image of
/// itself has that image's duty: a claim aterm cannot make now, refused or
/// unreachable, keeps the link lost and claimed again on the clock, where a
/// launch would be absent and pass its vendor through.
/// NEGATIVE CONTROL: the same refusals leave a launch absent.
#[test]
fn an_adopted_child_keeps_its_claim_through_any_refusal() {
    let mut clock = Clock::new();
    let fake = FakeAterm::for_harness("adopt", &config());
    let mut link = adopt(&fake, &clock);
    assert!(link.judges() && link.claims(), "{:?}", link.take_notes());
    link.release(clock.now());

    let unrostered = FakeAterm::scratch("adopt-unrostered");
    unrostered.serve_contracts(&[2, 3]);
    assert!(launch(&unrostered, &clock).is_absent());
    let mut link = adopt(&unrostered, &clock);
    assert!(link.is_lost() && link.claims() && !link.judges());
    assert_eq!(link.take_notes(), ["aterm unclaimed unrostered"]);
    assert_eq!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Away("link-lost".to_string())
    );
    // Asked again on the clock, refused again without a second note, and
    // still not absent.
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.is_lost() && link.claims());
    assert!(link.take_notes().is_empty());
    // Once aterm rosters it, the claim is made.
    unrostered.set_row(ID, &Scope::CONTRACT_3);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.judges(), "{:?}", link.take_notes());
    link.release(clock.now());

    let gone = FakeAterm::for_harness("adopt-gone", &config());
    std::fs::remove_file(gone.sock_dir.join("c.sock")).expect("remove the socket");
    assert!(launch(&gone, &clock).is_absent());
    let mut link = adopt(&gone, &clock);
    assert!(link.is_lost() && link.claims());
    assert!(
        link.take_notes()[0].starts_with("aterm unclaimed connect "),
        "an unreachable aterm is noted as at launch"
    );

    // Outside aterm there is nothing to claim, adopted or not.
    let outside = Link::adopt_with(config(), std::process::id(), child(), 0, |_| None);
    assert!(outside.is_absent() && !outside.claims());
}

/// Contract 4 has no roster list to ask: its proof reads the registry row, a
/// verified record may claim any word, and the seed rows `own-image=` is built
/// from go (HX-11, HX-14). A harness `native-act info` does not list, agy-alab
/// or any harness once the seed rows are deleted, claims at 4 and is judged by
/// the proof `status` reads back.
/// NEGATIVE CONTROL: the same fake at contract 3 is unrostered.
#[test]
fn under_contract_4_own_image_is_not_asked_and_the_proof_decides() {
    let config = config();
    let fake = FakeAterm::scratch("no-own-image");
    fake.set_status("program", Some(config.id()));
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    let holder = config.holder(std::process::id());
    assert!(link.judges(), "{:?}", link.take_notes());
    assert!(!fake.verbs().iter().any(|verb| verb == "native-act info"));
    assert_eq!(
        link.take_notes(),
        [format!(
            "aterm claimed holder={holder} contract=4 scope=relaunch,upgrade revision=1 \
             vendor=2.1.311"
        )]
    );
    link.release(clock.now());

    // aterm does not prove it: the claim is released and the launch absent.
    let unproven = FakeAterm::scratch("no-own-image-unproven");
    unproven.set_proven(false);
    let link = launch(&unproven, &clock);
    assert!(link.is_absent());
    assert_eq!(unproven.holder(), None);
    assert!(
        !unproven
            .verbs()
            .iter()
            .any(|verb| verb == "native-act info")
    );

    let three = FakeAterm::scratch("no-own-image-three");
    three.serve_contracts(&[2, 3]);
    let mut link = launch(&three, &clock);
    assert!(link.is_absent());
    assert_eq!(link.take_notes(), ["aterm unclaimed unrostered"]);
}

/// Contract 3 unchanged: against an aterm from before contract 4, the claim is
/// contract 3's text, its bare `OK` is read back from `status`, and a new
/// child is announced as before.
#[test]
fn a_rostered_launch_claims_reads_its_proof_back_and_subscribes() {
    let config = config();
    let fake = FakeAterm::for_harness("claim", &config);
    fake.serve_contracts(&[2, 3]);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    let holder = config.holder(std::process::id());
    assert!(link.judges() && link.claims() && !link.is_lost());
    assert_eq!(
        link.take_notes(),
        [format!(
            "aterm claimed holder={holder} contract=3 scope=relaunch,upgrade revision=1 \
             vendor=2.1.311"
        )]
    );
    assert_eq!(
        link.grant(),
        Some(&Grant {
            contract: Contract::Three,
            scopes: vec![Scope::Relaunch, Scope::Upgrade],
            revision: Some(1),
        })
    );
    assert!(link.takes(Scope::Relaunch) && !link.takes(Scope::State));
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    assert_eq!(
        fake.verbs(),
        [
            "harness policy".to_string(),
            "native-act info".to_string(),
            format!(
                "meta set supervisor {holder} contract=3 scope=relaunch,upgrade version=0.2.0 \
                 vendor=2.1.311"
            ),
            "status".to_string(),
        ]
    );
    assert_eq!(fake.subscriptions(), 1);
    assert!(link.event_fd().is_some());
    assert_eq!(link.safe_point(Duration::ZERO, clock.now()), Verdict::Safe);
    assert_eq!(link.relaunch_word(false, clock.now()), Relaunch::Permitted);

    // A draft holds by name, and the hold stands without a read until its recheck.
    fake.set_status("composer", Some("draft"));
    let now = clock.now();
    assert_eq!(
        link.safe_point(Duration::ZERO, now),
        Verdict::Hold("wait aterm composer=draft".to_string())
    );
    assert!(link.held_back(now) && !link.held_back(now + RECLAIM_EVERY_MS));
    fake.set_status("composer", Some("empty"));

    // A new child is announced with what it runs.
    link.reannounce(
        Child {
            vendor: Some("2.1.312"),
            login: None,
            process: None,
        },
        clock.now(),
    );
    assert_eq!(link.take_notes(), ["aterm reannounced vendor=2.1.312"]);
    assert_eq!(
        fake.claim_words().and_then(|words| words.vendor).as_deref(),
        Some("2.1.312")
    );

    // Leaving gives the claim back.
    link.release(clock.now());
    assert!(link.is_absent());
    assert_eq!(fake.holder(), None);
}

#[test]
fn a_claim_aterm_does_not_prove_is_released() {
    let fake = FakeAterm::for_harness("unproven", &config());
    fake.set_proven(false);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_absent());
    let notes = link.take_notes();
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].starts_with("aterm unclaimed unproven supervisor=example-alab@"),
        "{notes:?}"
    );
    assert_eq!(fake.holder(), None);
    assert!(
        fake.verbs()
            .last()
            .is_some_and(|verb| verb.starts_with("meta unset supervisor holder=example-alab@"))
    );
}

/// A claim made again on a held link whose proof fails is given back; a
/// release aterm answers with anything but `OK` may leave that claim standing
/// on the kept connection, so the link drops the connection (and aterm the
/// claim bound to it) and is lost, not left believing itself unclaimed.
/// NEGATIVE CONTROL: the same release answered `OK` keeps the connection, and
/// the link notes only that it is unclaimed.
#[test]
fn a_release_aterm_does_not_take_after_a_failed_reclaim_drops_the_connection() {
    for refused in [false, true] {
        let config = config();
        let fake = FakeAterm::for_harness("release", &config);
        let clock = Clock::new();
        let mut link = launch(&fake, &clock);
        assert!(link.judges());
        link.take_notes();
        fake.set_policy(
            &POLICY_ON.replace("effective=on why=enabled", "effective=off why=disabled"),
        );
        await_events("the release", &mut link, &clock, |_, link| !link.judges());
        link.take_notes();
        fake.set_proven(false);
        if refused {
            fake.set_release_reply(Some("ERR busy"));
        }
        let connections = fake.connections();
        fake.set_policy(POLICY_ON);
        await_events("the claim made again", &mut link, &clock, |_, link| {
            fake.verbs()
                .iter()
                .filter(|verb| verb.starts_with("meta unset supervisor"))
                .count()
                == 2
                && (link.is_lost() || !link.claims() || fake.holder().is_none())
        });
        let notes = link.take_notes();
        assert_eq!(notes.len(), 1, "{notes:?}");
        if refused {
            assert!(
                notes[0].starts_with("aterm lost unproven supervisor=example-alab@")
                    && notes[0].ends_with(" (release reply=ERR:busy)"),
                "{notes:?}"
            );
            assert!(link.is_lost() && !link.judges());
            // The claim aterm kept went with the connection it was bound to.
            let until = Instant::now() + HANG;
            while fake.holder().is_some() {
                assert!(Instant::now() < until, "hang detector: the dropped claim");
                std::thread::sleep(Duration::from_millis(5));
            }
        } else {
            assert!(
                notes[0].starts_with("aterm unclaimed unproven supervisor=example-alab@"),
                "{notes:?}"
            );
            assert!(link.claims() && !link.is_lost());
            assert_eq!(fake.holder(), None);
        }
        assert_eq!(fake.connections(), connections);
    }
}

/// kimi-alab's rule: an aterm that answered the handshake but not the claim's
/// reads in time is busy, not away. The launch is lost, not absent, and the
/// claim is made on the clock once aterm answers.
/// NEGATIVE CONTROL: no socket at all is gone, and the launch is absent.
#[test]
fn a_launch_aterm_answers_late_after_the_handshake_is_claimed_again_not_given_up() {
    let fake = FakeAterm::for_harness("busy", &config());
    fake.set_reply_delay(REQUEST_TIMEOUT + Duration::from_millis(300));
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_lost() && !link.claims() && !link.judges());
    let notes = link.take_notes();
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].starts_with("aterm unclaimed policy ")
            && notes[0].ends_with(" (aterm answered the handshake)"),
        "{notes:?}"
    );
    assert_eq!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Away("link-lost".to_string())
    );
    // Not before the clock, and then through the whole sequence.
    fake.set_reply_delay(Duration::ZERO);
    link.tick(child(), clock.now());
    assert!(link.is_lost());
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.judges(), "{:?}", link.take_notes());

    let gone = FakeAterm::for_harness("gone", &config());
    let config = config();
    let lookup = gone.lookup(config.env());
    std::fs::remove_file(gone.sock_dir.join("c.sock")).expect("remove the socket");
    let mut away = Link::launch_with(config, std::process::id(), child(), 0, lookup);
    assert!(away.is_absent());
    let notes = away.take_notes();
    assert!(
        notes[0].starts_with("aterm unclaimed connect "),
        "{notes:?}"
    );
}

#[test]
fn a_restart_aterm_asks_for_is_returned_and_judged_as_asked() {
    let fake = FakeAterm::for_harness("restart", &config());
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.judges());
    fake.send_restart("asked");
    let drained = await_events("a restart event", &mut link, &clock, |drained, _| {
        drained.restart.is_some()
    });
    assert!(drained.drained);
    assert_eq!(
        drained.restart.map(|restart| restart.cause).as_deref(),
        Some("asked")
    );
    // A person typed a moment ago: a crash is theirs, an asked restart is not.
    fake.set_status("human_ms", Some("5"));
    assert_eq!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Denied("person human_ms=5".to_string())
    );
    assert_eq!(link.relaunch_word(true, clock.now()), Relaunch::Permitted);
}

#[test]
fn a_policy_switched_off_releases_the_claim_and_switched_on_claims_again() {
    let config = config();
    let fake = FakeAterm::for_harness("policy", &config);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    link.take_notes();
    fake.set_policy(&POLICY_ON.replace("effective=on why=enabled", "effective=off why=disabled"));
    await_events("the release", &mut link, &clock, |_, link| !link.judges());
    assert_eq!(
        link.take_notes(),
        ["aterm released policy effective=off why=disabled"]
    );
    assert_eq!(fake.holder(), None);
    assert!(
        link.claims(),
        "the link is still held, its claim given back"
    );
    assert_eq!(link.safe_point(Duration::ZERO, clock.now()), Verdict::Local);
    assert_eq!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Denied("no-claim".to_string())
    );
    fake.set_policy(POLICY_ON);
    await_events("the claim again", &mut link, &clock, |_, link| {
        link.judges()
    });
    let holder = config.holder(std::process::id());
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    // The claim again is at contract 4, and its revision is past the release's.
    assert_eq!(
        link.take_notes(),
        [format!(
            "aterm claimed holder={holder} contract=4 scope=relaunch,upgrade revision=3 \
             vendor=2.1.311"
        )]
    );
}

#[test]
fn an_aterm_restart_loses_the_link_keeps_its_last_hold_and_claims_again() {
    let config = config();
    let fake = FakeAterm::for_harness("lost", &config);
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    fake.set_status("agent", Some("working"));
    assert_eq!(
        link.safe_point(Duration::ZERO, clock.now()),
        Verdict::Hold("wait aterm agent=working".to_string())
    );
    link.take_notes();
    fake.restart();
    // aterm's last word was a hold, and it stands over one that never came.
    assert_eq!(
        link.safe_point(Duration::ZERO, clock.now()),
        Verdict::Hold("wait link-lost aterm agent=working".to_string())
    );
    assert!(link.is_lost() && link.claims() && !link.judges());
    let notes = link.take_notes();
    assert!(notes[0].starts_with("aterm lost policy "), "{notes:?}");
    assert_eq!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Away("link-lost".to_string())
    );
    fake.set_status("agent", Some("idle"));
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.judges());
    assert_eq!(link.lost_hold(), None);
    assert_eq!(link.safe_point(Duration::ZERO, clock.now()), Verdict::Safe);
    assert_eq!(fake.subscriptions(), 2);
}

/// An event stream that ends while the request connection stays is reopened
/// on the reclaim clock, and while it cannot be reopened the policy is read at
/// each attempt (Decision 18): nothing else would say that a person turned
/// Autopilot Off, so the claim is given back within one attempt, and made
/// again once the policy is back on, the stream still down. The stream's
/// return is noted and followed by one more read.
#[test]
fn while_the_event_stream_is_down_the_policy_is_read_on_the_clock() {
    let config = config();
    let fake = FakeAterm::for_harness("stream-down", &config);
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.judges());
    link.take_notes();
    fake.set_subscribe_reply(Some("ERR busy"));
    fake.end_events();
    await_events("the stream's end", &mut link, &clock, |_, link| {
        link.event_fd().is_none()
    });
    assert_eq!(link.take_notes(), ["aterm events-lost closed"]);
    assert!(link.judges(), "the claim stands on its request connection");

    let off = POLICY_ON.replace(
        "effective=on why=enabled",
        "effective=off why=autopilot-off",
    );
    fake.set_policy_quietly(&off);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(!link.judges(), "the claim lingers: {:?}", link.take_notes());
    assert_eq!(
        link.take_notes(),
        ["aterm released policy effective=off why=autopilot-off"]
    );
    assert_eq!(fake.holder(), None);

    fake.set_policy_quietly(POLICY_ON);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.judges(), "{:?}", link.take_notes());
    let holder = config.holder(std::process::id());
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    assert!(link.event_fd().is_none(), "the stream is still down");
    assert_eq!(fake.subscriptions(), 1, "every reopen was refused");
    link.take_notes();

    fake.set_subscribe_reply(None);
    let reads = || {
        fake.verbs()
            .iter()
            .filter(|verb| *verb == "harness policy")
            .count()
    };
    let before = reads();
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.event_fd().is_some());
    assert_eq!(link.take_notes(), ["aterm events-reopened"]);
    assert_eq!(
        reads(),
        before + 1,
        "the policy is read once the stream is back"
    );
    assert!(link.judges());
}

/// A policy read a standing claim's event asks for, answered with aterm's
/// dialog `ERR … retry`, is owed: noted once when it begins to be owed, read
/// again on the clock (each answer that stays owed noted no more), and
/// followed once aterm answers it (Decision 18).
#[test]
fn a_policy_read_aterm_owes_on_a_standing_claim_is_noted_once_and_read_again() {
    let config = config();
    let fake = FakeAterm::for_harness("owed", &config);
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    link.take_notes();
    let dialog = "ERR aterm is showing a dialog; answer it, then retry";
    fake.set_policy_quietly(
        &POLICY_ON.replace("effective=on why=enabled", "effective=off why=disabled"),
    );
    fake.set_policy_reply(dialog);
    let reads = || {
        fake.verbs()
            .iter()
            .filter(|verb| *verb == "harness policy")
            .count()
    };
    let before = reads();
    fake.event("harness-policy gen=9");
    await_events("the owed read", &mut link, &clock, |_, _| reads() > before);
    assert_eq!(
        link.take_notes(),
        ["aterm policy-owed policy reply=ERR:aterm"]
    );
    assert!(link.judges(), "the claim stands while aterm owes the read");
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert_eq!(link.take_notes(), Vec::<String>::new(), "noted once");
    assert!(link.judges());
    fake.clear_policy_reply();
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(!link.judges());
    assert_eq!(
        link.take_notes(),
        ["aterm released policy effective=off why=disabled"]
    );
}

/// A lost link whose reclaim aterm answers with no contract this harness can
/// claim at (an aterm replaced across its restart by one serving contract 2
/// alone) lets go of the hold aterm stated before ([`Link::lost_hold`]): that
/// aterm states none on the session, and the harness's own readings decide
/// while the policy is read again on the clock.
/// NEGATIVE CONTROL: a reclaim answered with a policy off for now (a halt)
/// keeps the hold.
#[test]
fn a_lost_link_reclaimed_against_an_aterm_serving_only_contract_2_drops_its_hold() {
    let config = config();
    for (name, unserved) in [("lost-two", true), ("lost-halt", false)] {
        let fake = FakeAterm::for_harness(name, &config);
        let mut clock = Clock::new();
        let mut link = launch(&fake, &clock);
        fake.set_status("agent", Some("working"));
        assert_eq!(
            link.safe_point(Duration::ZERO, clock.now()),
            Verdict::Hold("wait aterm agent=working".to_string())
        );
        fake.restart();
        assert_eq!(
            link.safe_point(Duration::ZERO, clock.now()),
            Verdict::Hold("wait link-lost aterm agent=working".to_string())
        );
        link.take_notes();
        let why = if unserved {
            fake.serve_contracts(&[2]);
            "policy contract=2 declared=3,4"
        } else {
            fake.set_policy(
                &POLICY_ON.replace("effective=on why=enabled", "effective=off why=hold"),
            );
            "policy effective=off why=hold"
        };
        for _ in 0..3 {
            clock.pass(RECLAIM_EVERY_MS);
            link.tick(child(), clock.now());
            assert!(link.is_lost() && link.claims() && !link.judges(), "{name}");
        }
        assert_eq!(
            link.take_notes(),
            [format!("aterm unclaimed {why}")],
            "{name}"
        );
        assert_eq!(fake.holder(), None, "{name}");
        let kept = (!unserved).then(|| "wait link-lost aterm agent=working".to_string());
        assert_eq!(link.lost_hold(), kept, "{name}");
    }
}

#[test]
fn another_holder_taking_the_session_loses_the_link_and_permits_nothing() {
    let config = config();
    let fake = FakeAterm::for_harness("taken", &config);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    link.take_notes();
    // The fake proves nothing; withdrawing its proof is aterm's word that the
    // claim is no longer this harness's to act under.
    fake.set_proven(false);
    assert_eq!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Denied("claim supervisor_proven=0".to_string())
    );
    assert!(link.is_lost());
    assert_eq!(link.take_notes(), ["aterm lost claim supervisor_proven=0"]);
}

/// A launch whose event subscription aterm refuses gives its claim back and
/// ends absent, as the client this was ported from did: claiming and
/// releasing again on the clock would switch aterm's built-in engine between
/// standing down and resuming every five seconds for the session's life.
/// NEGATIVE CONTROL: the same fake with its ACK restored claims and holds.
#[test]
fn a_launch_whose_subscription_aterm_refuses_is_absent_and_never_claims_again() {
    let config = config();
    let holder = config.holder(std::process::id());
    let fake = FakeAterm::for_harness("events-refused", &config);
    fake.set_subscribe_reply(Some("ERR denied"));
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_absent() && !link.claims() && !link.judges());
    let notes = link.take_notes();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].starts_with("aterm unclaimed events "), "{notes:?}");
    assert_eq!(fake.holder(), None);
    let claims = |verbs: &[String]| {
        verbs
            .iter()
            .filter(|verb| verb.starts_with("meta set supervisor "))
            .count()
    };
    let verbs = fake.verbs();
    assert_eq!(claims(&verbs), 1);
    assert_eq!(
        verbs.last(),
        Some(&format!("meta unset supervisor holder={holder}"))
    );
    // Nothing is claimed again on the clock.
    for _ in 0..3 {
        clock.pass(RECLAIM_EVERY_MS);
        link.tick(child(), clock.now());
    }
    assert_eq!(fake.verbs(), verbs);
    assert!(link.take_notes().is_empty());

    fake.set_subscribe_reply(None);
    let held = launch(&fake, &clock);
    assert!(held.judges());
}

/// A reply no client may read is an aterm this client cannot speak to, not a
/// busy one: the launch is absent and the sequence is not run again.
#[test]
fn a_launch_aterm_answers_unreadably_is_absent_not_busy() {
    let fake = FakeAterm::for_harness("unreadable", &config());
    fake.set_policy_reply("garbage with no header");
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_absent());
    let notes = link.take_notes();
    assert!(
        notes.len() == 1 && notes[0].starts_with("aterm unclaimed policy "),
        "{notes:?}"
    );
    assert!(!notes[0].contains("handshake"), "{notes:?}");
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert_eq!(fake.verbs(), ["harness policy"]);
}

/// Every harness aterm rosters can test its unrostered contract-3 path
/// against the default fake: no row stands there until a test adds it.
#[test]
fn a_sibling_harness_is_unrostered_on_the_default_fake() {
    for id in ["kimi-alab", "muse-alab", "grok-alab", "claude-code-alab"] {
        let fake = FakeAterm::scratch("sibling");
        fake.serve_contracts(&[2, 3]);
        let config =
            Config::new(id, "vendor", &Scope::CONTRACT_3, "0.1.0").expect("a sibling config");
        let lookup = fake.lookup(config.env());
        let mut link = Link::launch_with(config, std::process::id(), child(), 0, lookup);
        assert!(link.is_absent(), "{id}");
        assert_eq!(link.take_notes(), ["aterm unclaimed unrostered"], "{id}");
        assert_eq!(fake.holder(), None, "{id}");
    }
}

/// aterm's account pool moves the holder to another account through the
/// link's event stream, label and home intact.
#[test]
fn an_account_move_reaches_the_harness_with_its_account() {
    let fake = FakeAterm::for_harness("account", &config());
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.judges());
    fake.send_account_restart("account", "b", Some(std::path::Path::new("/h")));
    let drained = await_events("an account move", &mut link, &clock, |drained, _| {
        drained.restart.is_some()
    });
    assert_eq!(
        drained.restart,
        Some(Restart {
            cause: "account".to_string(),
            account: Some(crate::Account {
                label: "b".to_string(),
                home: Some("/h".into()),
            }),
            model: None,
        })
    );
}

/// aterm's model move (HX-06) reaches the harness with the model to start
/// the next child on.
#[test]
fn a_model_move_reaches_the_harness_with_its_model() {
    let fake = FakeAterm::for_harness("model", &config());
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.judges());
    fake.send_model_restart("model", "gemini-3-pro");
    let drained = await_events("a model move", &mut link, &clock, |drained, _| {
        drained.restart.is_some()
    });
    assert_eq!(
        drained.restart,
        Some(Restart {
            cause: "model".to_string(),
            account: None,
            model: Some("gemini-3-pro".to_string()),
        })
    );
}

/// A harness with its own warrant for a move (a person asked for it under
/// `upgrade_ask=true`) gets aterm's fresh facts where the safe-point judge
/// holds, and the same release, hold and loss rules as that judge.
#[test]
fn a_fresh_read_gives_the_harness_aterms_facts_to_judge_by_its_own_rule() {
    let config = config();
    let holder = config.holder(std::process::id());
    let fake = FakeAterm::for_harness("read", &config);
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    link.take_notes();

    fake.set_policy(&POLICY_ON.replace("upgrade_ask=false", "upgrade_ask=true"));
    assert_eq!(
        link.safe_point(Duration::ZERO, clock.now()),
        Verdict::Hold("wait aterm upgrade_ask=true".to_string())
    );
    let Reading::Fresh(facts) = link.read(clock.now()) else {
        panic!("a standing claim reads fresh");
    };
    let Facts { policy, status } = *facts;
    assert!(policy.upgrade_ask && policy.admits_claim(&BOTH));
    assert!(status.held_by(&holder));
    assert_eq!(status.hold.as_deref(), Some("0"));
    assert_eq!(status.hand.as_deref(), Some("-"));
    // The facts are read afresh, so the hold the judge stated no longer
    // stands without a read.
    assert!(!link.held_back(clock.now()));

    // A halt is the policy off for now: aterm states a hold, and the claim
    // stands.
    fake.set_status("hold", Some("1"));
    assert_eq!(
        link.read(clock.now()),
        Reading::Hold("wait aterm policy effective=off why=hold".to_string())
    );
    assert!(link.judges());
    fake.set_status("hold", Some("0"));

    // A claim aterm no longer proves is not this harness's: the link is lost.
    fake.set_proven(false);
    assert_eq!(
        link.read(clock.now()),
        Reading::Hold("wait aterm supervisor_proven=0".to_string())
    );
    assert!(link.is_lost());
    assert_eq!(
        link.take_notes(),
        ["aterm lost claim wait aterm supervisor_proven=0"]
    );
    assert_eq!(link.read(clock.now()), Reading::Local);
    fake.set_proven(true);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.judges());

    // A policy switched off releases the claim; the harness's own readings
    // decide.
    link.take_notes();
    fake.set_policy_quietly(
        &POLICY_ON.replace("effective=on why=enabled", "effective=off why=disabled"),
    );
    assert_eq!(link.read(clock.now()), Reading::Local);
    assert_eq!(
        link.take_notes(),
        ["aterm released policy effective=off why=disabled"]
    );
    assert_eq!(fake.holder(), None);

    let mut absent = Link::absent(config, std::process::id());
    assert_eq!(absent.read(clock.now()), Reading::Local);
}

// ─────────────────────────────────── contract 4 ───────────────────────────────────

/// Every duty, so a grant can be seen to be narrower than the claim.
const WIDE: [Scope; 4] = [
    Scope::Relaunch,
    Scope::Upgrade,
    Scope::State,
    Scope::Accounts,
];

fn wide_config() -> Config {
    Config::new(ID, "example", &WIDE, "0.2.0").expect("a config")
}

fn started(pid: u32, start: u64, vendor: &'static str) -> Child<'static> {
    Child {
        vendor: Some(vendor),
        login: None,
        process: Some(claim::ChildProcess { pid, start }),
    }
}

fn launch_as(fake: &FakeAterm, clock: &Clock, config: Config, child: Child<'_>) -> Link {
    let lookup = fake.lookup(config.env());
    Link::launch_with(config, std::process::id(), child, clock.now(), lookup)
}

/// An aterm that lists contract 4 is claimed at 4: every duty asked for is
/// named, the child too, a word that aterm does not know is dropped, and the
/// harness holds exactly the grant the reply names.
#[test]
fn a_launch_claims_at_contract_4_and_holds_the_grant_aterm_names() {
    let config = wide_config();
    let fake = FakeAterm::for_harness("four", &config);
    // An aterm that does not know `accounts` yet.
    fake.set_known(&[Scope::Relaunch, Scope::Upgrade, Scope::State]);
    let clock = Clock::new();
    let mut link = launch_as(&fake, &clock, config.clone(), started(5151, 77, "2.1.311"));
    let holder = config.holder(std::process::id());
    assert!(link.judges());
    assert_eq!(
        link.take_notes(),
        [format!(
            "aterm claimed holder={holder} contract=4 scope=relaunch,upgrade,state revision=1 \
             vendor=2.1.311"
        )]
    );
    assert_eq!(
        fake.verbs(),
        [
            "harness policy".to_string(),
            format!(
                "meta set supervisor {holder} contract=4 scope=relaunch,upgrade,state,accounts \
                 version=0.2.0 vendor=2.1.311 child=5151:77"
            ),
            "status".to_string(),
        ]
    );
    assert_eq!(
        link.grant(),
        Some(&Grant {
            contract: Contract::Four,
            scopes: vec![Scope::Relaunch, Scope::Upgrade, Scope::State],
            revision: Some(1),
        })
    );
    assert!(link.takes(Scope::State) && !link.takes(Scope::Accounts));
    assert_eq!(fake.child().as_deref(), Some("5151:77"));
    assert_eq!(link.safe_point(Duration::ZERO, clock.now()), Verdict::Safe);
    assert_eq!(link.relaunch_word(false, clock.now()), Relaunch::Permitted);
    link.release(clock.now());
    assert!(link.grant().is_none() && !link.takes(Scope::Relaunch));
}

/// A person's cap narrows the live claim: aterm raises its revision and says
/// `harness-policy`, and the link keeps the narrower grant and the new
/// revision, so a duty withdrawn is no longer acted on. A cap that withholds
/// nothing moves no revision and notes nothing.
#[test]
fn a_cap_narrows_the_held_grant_and_raises_its_revision() {
    let config = config();
    let fake = FakeAterm::for_harness("cap", &config);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    link.take_notes();
    assert_eq!(link.grant().and_then(|grant| grant.revision), Some(1));

    fake.set_cap(Some(&Scope::ALL));
    await_events("the wide cap", &mut link, &clock, |drained, _| {
        drained.drained
    });
    assert_eq!(fake.revision(), 1, "a cap that withholds nothing");
    assert!(link.take_notes().is_empty());
    assert_eq!(link.grant().and_then(|grant| grant.revision), Some(1));

    fake.set_cap(Some(&[Scope::Upgrade]));
    await_events("the narrowing", &mut link, &clock, |_, link| {
        !link.takes(Scope::Relaunch)
    });
    assert_eq!(fake.revision(), 2);
    assert_eq!(
        link.take_notes(),
        ["aterm narrowed scope=upgrade revision=2"]
    );
    assert_eq!(
        link.grant(),
        Some(&Grant {
            contract: Contract::Four,
            scopes: vec![Scope::Upgrade],
            revision: Some(2),
        })
    );
    // The withdrawn duty is aterm's engine's now; the granted one stands.
    assert_eq!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Denied("claim supervisor_scope=upgrade".to_string())
    );
    assert_eq!(link.safe_point(Duration::ZERO, clock.now()), Verdict::Safe);

    // Lifting the cap gives a live claim nothing back by itself.
    fake.set_cap(None);
    await_events("the lifted cap", &mut link, &clock, |drained, _| {
        drained.drained
    });
    assert!(link.take_notes().is_empty());
    assert_eq!(
        link.grant().map(|grant| grant.scopes.clone()),
        Some(vec![Scope::Upgrade])
    );
}

/// An aterm that gives a live claim back what a lifted cap allows shows it in
/// `status`, and its engine stands down for those duties; the held grant
/// follows, of the words the claim asked for, so a crashed child is not left
/// to nobody and `takes` agrees with `relaunch_word`.
/// NEGATIVE CONTROL: a grant filtered to the words already held keeps
/// `relaunch` withdrawn while `relaunch_word` permits it.
#[test]
fn a_grant_aterm_widens_on_a_live_claim_is_taken_up() {
    let config = config();
    let fake = FakeAterm::for_harness("widen", &config);
    fake.set_cap_restores(true);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    link.take_notes();
    fake.set_cap(Some(&[Scope::Upgrade]));
    await_events("the narrowing", &mut link, &clock, |_, link| {
        !link.takes(Scope::Relaunch)
    });
    assert_eq!(
        link.take_notes(),
        ["aterm narrowed scope=upgrade revision=2"]
    );
    fake.set_cap(None);
    await_events("the widening", &mut link, &clock, |_, link| {
        link.takes(Scope::Relaunch)
    });
    assert_eq!(
        link.take_notes(),
        ["aterm widened scope=relaunch,upgrade revision=3"]
    );
    assert_eq!(link.relaunch_word(false, clock.now()), Relaunch::Permitted);
    // Never a word the claim did not ask for, whatever `status` shows.
    assert!(!link.takes(Scope::State));
}

/// A cap that lands between the claim's reply and the `status` read back is
/// aterm's later word: the launch holds the narrower grant at the newer
/// revision rather than refusing a claim aterm stands by.
/// NEGATIVE CONTROL: without the revision check the proof finds `relaunch`
/// missing and the launch is absent for the session.
#[test]
fn a_cap_between_the_claims_reply_and_its_proof_is_the_grant() {
    let config = config();
    let fake = FakeAterm::for_harness("race", &config);
    fake.set_cap_after_next_claim(Some(&[Scope::Upgrade]));
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    let holder = config.holder(std::process::id());
    assert!(link.judges(), "{:?}", link.take_notes());
    assert_eq!(
        link.take_notes(),
        [format!(
            "aterm claimed holder={holder} contract=4 scope=upgrade revision=2 vendor=2.1.311"
        )]
    );
    assert_eq!(
        link.grant(),
        Some(&Grant {
            contract: Contract::Four,
            scopes: vec![Scope::Upgrade],
            revision: Some(2),
        })
    );
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
}

/// A granted word `status` does not show at the reply's own revision is a
/// duty nobody would perform: the claim is released and the launch absent.
#[test]
fn a_granted_word_status_does_not_take_releases_the_contract_4_claim() {
    let fake = FakeAterm::for_harness("unstored", &config());
    fake.set_unstored(&[Scope::Relaunch]);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_absent());
    assert_eq!(
        link.take_notes(),
        ["aterm unclaimed unproven scope=relaunch supervisor_scope=upgrade"]
    );
    assert_eq!(fake.holder(), None);
    assert!(
        fake.verbs()
            .last()
            .is_some_and(|verb| verb.starts_with("meta unset supervisor holder=example-alab@"))
    );
}

/// A `status` aterm cannot give when a policy event comes leaves the grant as
/// it was, and is read again after `LATER_RETRY_MS`, not left until the next
/// event.
#[test]
fn a_status_aterm_cannot_give_on_a_policy_event_is_read_again_soon() {
    let fake = FakeAterm::for_harness("status-later", &config());
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    link.take_notes();
    fake.set_status_reply("ERR main thread stalled 6s; retry");
    fake.set_cap(Some(&[Scope::Upgrade]));
    await_events("the cap's event", &mut link, &clock, |drained, _| {
        drained.drained
    });
    assert!(link.takes(Scope::Relaunch));
    assert!(link.take_notes().is_empty());
    fake.clear_status_reply();
    clock.pass(LATER_RETRY_MS);
    link.tick(child(), clock.now());
    assert!(!link.takes(Scope::Relaunch));
    assert_eq!(
        link.take_notes(),
        ["aterm narrowed scope=upgrade revision=2"]
    );
}

/// The harness sends `child=` again after every child start, by announcing
/// the same claim with the new child; the reply is the grant from then on,
/// so a cap lifted since the claim narrowed is granted back on the next start.
#[test]
fn every_child_start_sends_child_again_and_its_reply_is_the_grant() {
    let config = config();
    let fake = FakeAterm::for_harness("child", &config);
    let clock = Clock::new();
    let mut link = launch_as(&fake, &clock, config.clone(), started(5151, 1, "2.1.311"));
    link.take_notes();
    assert_eq!(fake.child().as_deref(), Some("5151:1"));

    link.reannounce(started(6262, 2, "2.1.312"), clock.now());
    assert_eq!(
        link.take_notes(),
        ["aterm reannounced vendor=2.1.312 child=6262:2 scope=relaunch,upgrade revision=2"]
    );
    assert_eq!(fake.child().as_deref(), Some("6262:2"));
    let holder = config.holder(std::process::id());
    assert_eq!(
        fake.verbs().last().map(String::as_str),
        Some(
            format!(
                "meta set supervisor {holder} contract=4 scope=relaunch,upgrade version=0.2.0 \
                 vendor=2.1.312 child=6262:2"
            )
            .as_str()
        )
    );
    assert_eq!(link.grant().and_then(|grant| grant.revision), Some(2));

    // A child start under a cap is granted only the cap.
    fake.set_cap(Some(&[Scope::Relaunch]));
    await_events("the narrowing", &mut link, &clock, |_, link| {
        !link.takes(Scope::Upgrade)
    });
    link.take_notes();
    link.reannounce(started(7373, 3, "2.1.312"), clock.now());
    assert_eq!(
        link.take_notes(),
        ["aterm reannounced vendor=2.1.312 child=7373:3 scope=relaunch revision=4"]
    );
    // With the cap lifted, the next start's reply grants the duty back.
    fake.set_cap(None);
    await_events("the lifted cap", &mut link, &clock, |drained, _| {
        drained.drained
    });
    link.reannounce(started(8484, 4, "2.1.312"), clock.now());
    assert!(link.takes(Scope::Upgrade) && link.takes(Scope::Relaunch));
    assert_eq!(link.grant().and_then(|grant| grant.revision), Some(5));
    assert_eq!(fake.child().as_deref(), Some("8484:4"));
    link.take_notes();

    // A reply that states no grant is no announcement: the link is lost.
    fake.set_claim_reply("OK revision=9");
    link.reannounce(started(9595, 5, "2.1.312"), clock.now());
    assert!(link.is_lost());
    assert_eq!(link.take_notes(), ["aterm lost reannounce scope=absent"]);
}

/// Contract 3 unchanged: an aterm that lists 3 and not 4 is claimed at 3,
/// whose text has no `child=` and names only `relaunch` and `upgrade` of what
/// the harness asks for. A cap narrows the live contract-3 claim too, and the
/// held grant follows. A harness asking only for contract-4 duties does not
/// claim at 3, and claims at 4 once an updated aterm lists it.
#[test]
fn against_an_aterm_without_contract_4_the_claim_is_contract_3s() {
    let config = wide_config();
    let fake = FakeAterm::for_harness("three", &config);
    fake.serve_contracts(&[3]);
    let clock = Clock::new();
    let mut link = launch_as(&fake, &clock, config.clone(), started(5151, 1, "2.1.311"));
    let holder = config.holder(std::process::id());
    assert!(link.judges());
    assert_eq!(
        fake.verbs()[2],
        format!(
            "meta set supervisor {holder} contract=3 scope=relaunch,upgrade version=0.2.0 \
             vendor=2.1.311"
        )
    );
    assert_eq!(fake.child(), None);
    assert_eq!(
        link.grant()
            .map(|grant| (grant.contract, grant.scopes.clone())),
        Some((Contract::Three, vec![Scope::Relaunch, Scope::Upgrade]))
    );
    // A cap narrows every live claim it covers, a contract-3 claim's too, and
    // the duty it withdrew is no longer this harness's.
    link.take_notes();
    fake.set_cap(Some(&[Scope::Upgrade]));
    await_events("the narrowing", &mut link, &clock, |_, link| {
        !link.takes(Scope::Relaunch)
    });
    assert_eq!(
        link.take_notes(),
        ["aterm narrowed scope=upgrade revision=2"]
    );
    assert_eq!(link.grant().and_then(|grant| grant.revision), Some(2));
    link.release(clock.now());

    let state_only = Config::new(ID, "example", &[Scope::State], "0.2.0").expect("a config");
    let fake = FakeAterm::for_harness("three-state", &state_only);
    fake.serve_contracts(&[2, 3]);
    let mut clock = clock;
    let mut link = launch_as(&fake, &clock, state_only, Child::default());
    assert!(link.is_lost() && !link.claims() && !link.judges());
    assert_eq!(link.take_notes(), ["aterm unclaimed contract=3 scope=-"]);
    assert_eq!(fake.verbs(), ["harness policy"]);
    // Nothing new is noted while it stays so, and nothing is claimed.
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(Child::default(), clock.now());
    assert!(link.is_lost() && link.take_notes().is_empty());
    assert_eq!(fake.holder(), None);
    // aterm is updated and lists contract 4: the harness claims there.
    fake.serve_contracts(&[2, 3, 4]);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(Child::default(), clock.now());
    assert!(link.takes(Scope::State), "{:?}", link.take_notes());
}

/// An aterm that serves no contract this library speaks (contract 2 alone)
/// leaves the launch unclaimed, not absent: the policy is read again on the
/// reclaim clock, and once an update serves 2, 3 and 4 the harness claims at 4
/// (the interface design, section 2.2: a harness claims again after an aterm
/// update).
#[test]
fn an_aterm_serving_only_contract_2_is_claimed_at_4_once_it_serves_4() {
    let config = config();
    let fake = FakeAterm::for_harness("only-two", &config);
    fake.serve_contracts(&[2]);
    let mut clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_lost() && !link.claims() && !link.judges());
    assert_eq!(
        link.take_notes(),
        ["aterm unclaimed policy contract=2 declared=3,4"]
    );
    assert_eq!(
        link.unclaimed(),
        Some(&Failure::Unserved(
            "policy contract=2 declared=3,4".to_string()
        ))
    );
    assert_eq!(fake.verbs(), ["harness policy"]);
    // Nothing is claimed, or noted again, while aterm still serves 2 alone.
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.is_lost() && link.take_notes().is_empty());
    assert_eq!(fake.holder(), None);
    // aterm is updated and serves 2, 3 and 4: the harness claims at 4.
    fake.serve_contracts(&[2, 3, 4]);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.judges(), "{:?}", link.take_notes());
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Four)
    );
    let holder = config.holder(std::process::id());
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    link.release(clock.now());
}

/// A config declaring contract 4 alone, as every harness's record does (D11).
fn four_config() -> Config {
    config()
        .with_contracts(&[Contract::Four])
        .expect("a config")
}

/// Whether `verbs` hold a claim under contract 3 or the roster read that
/// comes before one.
fn any_contract_3(verbs: &[String]) -> bool {
    verbs.iter().any(|verb| {
        verb == "native-act info"
            || (verb.starts_with("meta set supervisor ") && !verb.contains(" contract=4 "))
    })
}

/// A harness that declares `[4]` (D11) never claims 3. Against an aterm that
/// serves `2,3` and rosters it, a launch reads the policy and sends nothing
/// more: no roster read, no claim. The link reports the policy unserved
/// (`Link::unclaimed`, and the note), so its caller can pass its vendor
/// through or run it unclaimed, and reads the policy again on the reclaim
/// clock, claiming at 4 once aterm serves 4. An adopted child's link
/// (`Link::adopt_with`) keeps to the same contracts: it gives back the claim
/// its earlier image may have left in aterm's re-exec grace, once, and claims
/// nothing.
/// NEGATIVE CONTROL: the default config, against the same aterm, claims at 3.
#[test]
fn a_link_that_declares_four_never_claims_three() {
    let four = four_config();
    let why = "policy contract=2,3 declared=4";
    let fake = FakeAterm::for_harness("declares-four", &four);
    fake.serve_contracts(&[2, 3]);
    let mut clock = Clock::new();
    let mut link = launch_as(&fake, &clock, four.clone(), child());
    assert!(link.is_lost() && !link.claims() && !link.judges());
    assert_eq!(link.take_notes(), [format!("aterm unclaimed {why}")]);
    assert_eq!(link.unclaimed(), Some(&Failure::Unserved(why.to_string())));
    assert_eq!(fake.verbs(), ["harness policy"]);
    // Read again on the clock, noted once, and nothing past the policy.
    for _ in 0..3 {
        clock.pass(RECLAIM_EVERY_MS);
        link.tick(child(), clock.now());
        assert!(link.is_lost() && !link.claims() && link.grant().is_none());
    }
    assert!(link.take_notes().is_empty());
    assert_eq!(fake.verbs(), ["harness policy"; 4]);
    assert_eq!(fake.holder(), None);
    // aterm serves 4: the claim is made there.
    fake.serve_contracts(&[2, 3, 4]);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Four),
        "{:?}",
        link.take_notes()
    );
    assert_eq!(link.unclaimed(), None);
    let holder = four.holder(std::process::id());
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    assert!(!any_contract_3(&fake.verbs()), "{:?}", fake.verbs());
    link.release(clock.now());

    // An adopted child's link: lost and claimed, never at 3.
    let adopted = FakeAterm::for_harness("adopts-four", &four);
    adopted.serve_contracts(&[2, 3]);
    let lookup = adopted.lookup(four.env());
    let mut link = Link::adopt_with(
        four.clone(),
        std::process::id(),
        child(),
        clock.now(),
        lookup,
    );
    assert!(link.is_lost() && link.claims() && !link.judges());
    assert_eq!(link.take_notes(), [format!("aterm unclaimed {why}")]);
    assert_eq!(link.unclaimed(), Some(&Failure::Unserved(why.to_string())));
    let policy = "harness policy".to_string();
    let release = format!("meta unset supervisor holder={holder}");
    assert_eq!(adopted.verbs(), [policy.clone(), release.clone()]);
    // That answer gave the claim back, so neither a reclaim nor the link's
    // release gives it back again within the grace: the reclaim reads the
    // policy alone.
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.is_lost() && link.grant().is_none());
    assert_eq!(adopted.verbs(), [policy.clone(), release.clone(), policy]);
    assert_eq!(adopted.holder(), None);
    link.release(clock.now());
    assert_eq!(adopted.verbs().len(), 3, "{:?}", adopted.verbs());

    // NEGATIVE CONTROL: declaring 3 and 4, the same aterm is claimed at 3.
    let control = FakeAterm::for_harness("declares-both", &config());
    control.serve_contracts(&[2, 3]);
    let mut link = launch(&control, &clock);
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Three)
    );
    assert!(any_contract_3(&control.verbs()), "{:?}", control.verbs());
    link.release(clock.now());
}

/// A held contract-4 link of a harness that declares `[4]`, lost to an aterm
/// restart into one serving `2,3` (with this harness on its contract-3
/// roster), is claimed again only under 4. Past several reclaim periods aterm
/// hears policy reads and, once, the release of the claim it may have kept
/// through its re-exec grace (Decision 30), and no roster read or claim; once
/// it serves 4 again the link is claimed at 4.
/// NEGATIVE CONTROL: the default config, through the same restart, reclaims at 3.
#[test]
fn a_reclaim_keeps_to_the_declared_contracts() {
    let four = four_config();
    let holder = four.holder(std::process::id());
    let fake = FakeAterm::for_harness("reclaims-four", &four);
    let mut clock = Clock::new();
    let mut link = launch_as(&fake, &clock, four.clone(), child());
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Four)
    );
    fake.restart();
    fake.serve_contracts(&[2, 3]);
    assert!(matches!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Away(_)
    ));
    assert!(link.is_lost() && link.claims());
    link.take_notes();
    let before = fake.verbs().len();
    for _ in 0..4 {
        clock.pass(RECLAIM_EVERY_MS);
        link.tick(child(), clock.now());
        assert!(link.is_lost() && link.claims() && link.grant().is_none());
    }
    let why = "policy contract=2,3 declared=4";
    assert_eq!(link.take_notes(), [format!("aterm unclaimed {why}")]);
    assert_eq!(link.unclaimed(), Some(&Failure::Unserved(why.to_string())));
    let release = format!("meta unset supervisor holder={holder}");
    let policy = "harness policy".to_string();
    assert_eq!(
        fake.verbs()[before..],
        [
            policy.clone(),
            release,
            policy.clone(),
            policy.clone(),
            policy
        ]
    );
    assert_eq!(fake.holder(), None);
    // aterm serves 4 again: the link is claimed at 4.
    fake.serve_contracts(&[2, 3, 4]);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Four),
        "{:?}",
        link.take_notes()
    );
    assert_eq!(fake.holder().as_deref(), Some(holder.as_str()));
    assert!(!any_contract_3(&fake.verbs()), "{:?}", fake.verbs());
    link.release(clock.now());

    // NEGATIVE CONTROL: declaring 3 and 4, the same restart is reclaimed at 3.
    let control = FakeAterm::for_harness("reclaims-both", &config());
    let mut link = launch(&control, &clock);
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Four)
    );
    control.restart();
    control.serve_contracts(&[2, 3]);
    assert!(matches!(
        link.relaunch_word(false, clock.now()),
        Relaunch::Away(_)
    ));
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Three)
    );
    link.release(clock.now());
}

/// aterm answers a contract-4 holder no registry row proves `ERR unproven`
/// and stores nothing (HX-06): the claim is refused, nothing is left to
/// release, and the launch is absent.
#[test]
fn an_unregistered_contract_4_holder_answered_err_unproven_is_refused() {
    let fake = FakeAterm::for_harness("unregistered", &config());
    fake.set_registered(false);
    let clock = Clock::new();
    let mut link = launch(&fake, &clock);
    assert!(link.is_absent());
    assert_eq!(
        link.take_notes(),
        ["aterm unclaimed claim reply=ERR:unproven:"]
    );
    assert_eq!(fake.holder(), None);
    assert!(
        fake.verbs()
            .iter()
            .all(|verb| !verb.starts_with("meta unset supervisor")),
        "{:?}",
        fake.verbs()
    );
}

// ──────────────────────────────── the session stream ────────────────────────────────

/// The caller's wall clock in these scenarios: milliseconds since 1970.
const WALL: u64 = 1_794_000_000_000;

/// A record any stream applies: `title` needs no scope.
fn title() -> Body {
    Body::Title {
        title: Some("the session".to_string()),
        summary: None,
    }
}

/// A `session` record that takes `takes`.
fn session(takes: &[record::Kind]) -> Body {
    Body::Session {
        agent: "example".to_string(),
        version: "0.2.0".to_string(),
        vendor: Some("2.1.311".to_string()),
        account: None,
        conversation: None,
        cwd: None,
        resume: Vec::new(),
        mode: record::Mode::Tui,
        takes: takes.to_vec(),
    }
}

fn eventually(what: &str, mut holds: impl FnMut() -> bool) {
    let until = Instant::now() + HANG;
    while !holds() {
        assert!(Instant::now() < until, "hang detector: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A fake that serves the stream, set up by `setup` before the launch at `now_ms`.
fn served(name: &str, setup: impl FnOnce(&FakeAterm)) -> FakeAterm {
    let fake = FakeAterm::for_harness(name, &config());
    fake.serve_stream(true);
    setup(&fake);
    fake
}

fn launched(fake: &FakeAterm, config: Config, child: Child<'_>, now_ms: u64) -> Link {
    let lookup = fake.lookup(config.env());
    Link::launch_with(config, std::process::id(), child, now_ms, lookup)
}

/// A launch at `now_ms` on a fake that serves the stream, which it opens.
fn streamed_at(name: &str, child: Child<'_>, now_ms: u64) -> (FakeAterm, Link) {
    let fake = served(name, |_| {});
    let link = launched(&fake, config(), child, now_ms);
    assert!(link.streaming(), "the stream opened");
    (fake, link)
}

/// Read the link until `wanted` holds of everything read so far, waiting on the stream's
/// descriptor: the drains gathered.
fn await_stream(
    what: &str,
    link: &mut Link,
    now_ms: u64,
    wanted: impl Fn(&Drained, &Link) -> bool,
) -> Drained {
    let until = Instant::now() + HANG;
    let mut gathered = Drained::default();
    loop {
        if let Some(fd) = link.stream_fd() {
            let mut probe = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: probe is one initialized poll record for the link's open descriptor.
            let _ = unsafe { libc::poll(&raw mut probe, 1, 50) };
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
        let drained = link.events(child(), now_ms);
        gathered.drained |= drained.drained;
        gathered.restart = drained.restart.or(gathered.restart);
        gathered.incoming.extend(drained.incoming);
        gathered.hand = drained.hand.or(gathered.hand);
        gathered.policy = drained.policy.or(gathered.policy);
        gathered.fresh |= drained.fresh;
        gathered.renumbered.extend(drained.renumbered);
        if wanted(&gathered, link) {
            return gathered;
        }
        assert!(Instant::now() < until, "hang detector: {what}");
    }
}

/// Wait until the open's `hand` and `policy` are answered, and their answers acknowledged.
fn settle_open(fake: &FakeAterm, link: &mut Link, now_ms: u64) -> Drained {
    let drained = await_stream("the open's records answered", link, now_ms, |_, link| {
        link.counts().acks >= 2
    });
    assert_eq!(fake.took(), [1, 2]);
    drained
}

/// The harness records the fake applied or dropped, as `(seq, kind)`.
fn received(fake: &FakeAterm) -> Vec<(u64, String)> {
    fake.stream_records()
        .into_iter()
        .map(|record| (record.seq, record.body.word().to_string()))
        .collect()
}

/// Lose the stream by closing it at aterm, and wait until the link knows.
fn close_and_lose(fake: &FakeAterm, link: &mut Link, now_ms: u64) {
    fake.close_stream();
    await_stream("the closed stream", link, now_ms, |_, link| link.is_lost());
}

/// Under a contract-4 grant the link sends `stream` on the claim's connection, which carries
/// records from its `OK`; every read goes on a second request connection; the subscription is
/// kept; the caller's records go on the stream, and the link's own kinds are not the caller's.
#[test]
fn a_link_streams_on_the_claims_connection_and_reads_on_a_second() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("streams", child(), clock.now());
    let claimed = fake.claim_connection().expect("the claim");
    assert_eq!(fake.stream_connection(), Some(claimed));
    assert_eq!(fake.streams_opened(), [claimed]);
    assert_eq!(fake.stream_requests(), 1);
    let notes = link.take_notes();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(
        notes[1],
        "aterm streaming epoch=fake-1 acked=0 in=0 resent=0"
    );
    assert_eq!(link.epoch(), Some("fake-1"));
    let opened = settle_open(&fake, &mut link, clock.now());
    assert!(
        opened.fresh,
        "the first stream holds nothing of the caller's"
    );

    let before = fake.requests().len();
    assert!(matches!(link.read(clock.now()), Reading::Fresh(_)));
    let reads = fake.requests().split_off(before);
    assert_eq!(
        reads
            .iter()
            .map(|(_, verb)| verb.as_str())
            .collect::<Vec<_>>(),
        ["harness policy", "status"]
    );
    assert!(reads.iter().all(|(id, _)| *id != claimed), "{reads:?}");
    assert!(link.event_fd().is_some() && link.stream_fd().is_some());
    assert_eq!(fake.subscriptions(), 1);

    assert_eq!(link.send(title(), WALL, clock.now()), Ok(3));
    eventually("the state record", || fake.acks().len() >= 3);
    assert_eq!(fake.acks()[2], (3, None));
    assert_eq!(received(&fake)[2], (3, "title".to_string()));
    for own in [Body::Beat, Body::Took { of: 1 }, Body::Want, Body::Policy] {
        assert_eq!(
            link.send(own.clone(), WALL, clock.now()),
            Err(SendError::Kind(own.word().to_string()))
        );
    }
    assert_eq!(
        fake.verbs()
            .iter()
            .filter(|verb| verb.starts_with("meta set"))
            .count(),
        1
    );
    link.release(clock.now());
}

/// A stream that closes loses the link; the claim is made again on the reclaim clock and the
/// records aterm did not acknowledge are written again, under their own numbers, from `acked=`.
#[test]
fn a_reconnect_resends_from_acked() {
    let mut clock = Clock::new();
    let (fake, mut link) = streamed_at("resend", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    link.take_notes();
    // aterm takes in the next three records and applies none of them before the stream ends.
    // The fake reads them on its connection's own thread, maybe after the link wrote them all,
    // so it stays deaf until the stream has closed.
    fake.set_stream_deaf(true);
    for seq in 3..=5 {
        assert_eq!(link.send(title(), WALL, clock.now()), Ok(seq));
    }
    close_and_lose(&fake, &mut link, clock.now());
    fake.set_stream_deaf(false);
    assert_eq!(link.take_notes(), ["aterm lost stream closed"]);
    // Kept while lost: written when the stream is back.
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(6));
    assert_eq!(link.epoch(), Some("fake-1"));
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.streaming());
    let notes = link.take_notes();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(
        notes[1],
        "aterm streaming epoch=fake-1 acked=2 in=2 resent=4"
    );
    eventually("the resend", || fake.stream_numbers().0 >= 6);
    assert_eq!(
        received(&fake)[2..6],
        [3, 4, 5, 6].map(|seq| (seq, "title".to_string()))
    );
    assert_eq!(link.counts().resent, 4);
    assert_eq!(fake.streams_opened().len(), 2);
    let again = await_stream("the reopen", &mut link, clock.now(), |_, link| {
        link.counts().acks >= 8
    });
    assert!(!again.fresh && again.renumbered.is_empty(), "{again:?}");
    link.release(clock.now());
}

/// Every number the link gave stays the record's when a stream opens again in the same epoch,
/// a beat between the caller's records included.
#[test]
fn a_seq_stays_the_records_across_a_reopen_in_the_same_epoch() {
    let t = 4_000_000;
    let (fake, mut link) = streamed_at("same-numbers", child(), t);
    settle_open(&fake, &mut link, t);
    fake.set_stream_deaf(true);
    assert_eq!(link.send(title(), WALL, t), Ok(3));
    link.tick(child(), t + BEAT_EVERY_MS);
    assert_eq!(link.send(title(), WALL, t + BEAT_EVERY_MS), Ok(5));
    close_and_lose(&fake, &mut link, t + BEAT_EVERY_MS);
    fake.set_stream_deaf(false);
    link.tick(child(), t + BEAT_EVERY_MS + RECLAIM_EVERY_MS);
    assert!(link.streaming());
    eventually("the resend", || fake.stream_numbers().0 >= 5);
    assert_eq!(
        received(&fake)[2..5],
        [
            (3, "title".to_string()),
            (4, "beat".to_string()),
            (5, "title".to_string())
        ]
    );
    link.release(t + BEAT_EVERY_MS + RECLAIM_EVERY_MS);
}

/// A `beat` goes out every ten seconds of the caller's `now_ms`, from the stream's open, and a
/// late tick sends one; each is stamped from the caller's wall clock.
#[test]
fn a_beat_is_sent_every_ten_seconds_of_the_callers_clock() {
    let t = 5_000_000;
    let (fake, mut link) = streamed_at("beats", child(), t);
    settle_open(&fake, &mut link, t);
    assert_eq!(link.send(title(), WALL, t), Ok(3));
    // Each tick is followed by a record, which the fake acknowledges after any beat it sent.
    let beats_by = |link: &mut Link, now_ms: u64| {
        link.tick(child(), now_ms);
        let seq = link.send(title(), WALL, now_ms).expect("the mark");
        eventually("the mark", || fake.stream_numbers().0 >= seq);
        fake.beats()
    };
    assert_eq!(beats_by(&mut link, t + BEAT_EVERY_MS - 1), 0);
    assert_eq!(beats_by(&mut link, t + BEAT_EVERY_MS), 1);
    assert_eq!(beats_by(&mut link, t + 2 * BEAT_EVERY_MS - 1), 1);
    assert_eq!(beats_by(&mut link, t + 2 * BEAT_EVERY_MS), 2);
    // A tick 45 s late sends one beat, not the four it missed.
    assert_eq!(beats_by(&mut link, t + 65_000), 3);
    assert_eq!(beats_by(&mut link, t + 65_000 + BEAT_EVERY_MS - 1), 3);
    assert_eq!(beats_by(&mut link, t + 65_000 + BEAT_EVERY_MS), 4);
    let beats: Vec<u64> = fake
        .stream_records()
        .into_iter()
        .filter(|record| record.body == Body::Beat)
        .map(|record| record.ms)
        .collect();
    // The caller's last record before each beat was stamped WALL at the tick before it.
    assert_eq!(
        beats,
        [
            WALL + 1,
            WALL + 1,
            WALL + 65_000 - (2 * BEAT_EVERY_MS),
            WALL + 1
        ]
    );
    link.release(t + 80_000);
}

/// Before the caller's first record the link's records are stamped from a wall clock it gave;
/// beats pause while the stream stays, and go again; the link names when it is next due.
#[test]
fn beats_pause_and_are_stamped_from_a_wall_clock_given_before_any_record() {
    let t = 6_000_000;
    let (fake, mut link) = streamed_at("wall", child(), t);
    settle_open(&fake, &mut link, t);
    assert_eq!(link.due_ms(), Some(t + BEAT_EVERY_MS));
    link.set_wall(WALL, t);
    link.set_beating(false, t);
    link.tick(child(), t + 3 * BEAT_EVERY_MS);
    assert_eq!(
        link.due_ms(),
        None,
        "nothing on the clock while beats pause"
    );
    link.set_beating(true, t + 3 * BEAT_EVERY_MS);
    link.tick(child(), t + 3 * BEAT_EVERY_MS);
    eventually("the beat", || fake.beats() == 1);
    assert!(link.streaming() && fake.streaming());
    let beat = fake
        .stream_records()
        .into_iter()
        .find(|record| record.body == Body::Beat)
        .expect("the beat");
    assert_eq!(beat.ms, WALL + 3 * BEAT_EVERY_MS);
    assert_eq!(link.due_ms(), Some(t + 4 * BEAT_EVERY_MS));
    link.release(t + 3 * BEAT_EVERY_MS);
}

/// A `policy` record makes the link read the grant again on the second connection; the
/// subscription's `EVENT harness-policy` and `EVENT restart` are passed over while it streams.
#[test]
fn a_policy_record_rereads_the_grant() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("policy-record", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    link.take_notes();
    let stream = fake.stream_connection().expect("the stream");
    let before = fake.requests().len();
    fake.event("harness-policy gen=9");
    fake.event("restart cause=asked");
    std::thread::sleep(Duration::from_millis(200));
    let mut restart = None;
    for _ in 0..3 {
        restart = restart.or(link.events(child(), clock.now()).restart);
    }
    assert_eq!(restart, None, "EVENT restart is not the stream's");
    assert_eq!(
        fake.requests().len(),
        before,
        "EVENT harness-policy read nothing"
    );

    fake.set_cap(Some(&[Scope::Upgrade]));
    let drained = await_stream("the narrowing", &mut link, clock.now(), |_, link| {
        !link.takes(Scope::Relaunch)
    });
    assert_eq!(drained.policy, Some(3), "the policy record heard");
    assert_eq!(
        link.take_notes(),
        ["aterm narrowed scope=upgrade revision=2"]
    );
    let reads = fake.requests().split_off(before);
    assert_eq!(
        reads
            .iter()
            .map(|(_, verb)| verb.as_str())
            .collect::<Vec<_>>(),
        ["harness policy", "status"],
        "one read for the record and its EVENT"
    );
    assert!(reads.iter().all(|(id, _)| *id != stream));
    eventually("the policy answered", || fake.took().contains(&3));
    link.release(clock.now());
}

/// `release` gives the claim back on the second connection; aterm closes the stream with it,
/// and nothing is kept to send.
#[test]
fn release_goes_on_the_second_connection() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("release", child(), clock.now());
    let stream = fake.stream_connection().expect("the stream");
    let holder = config().holder(std::process::id());
    link.release(clock.now());
    let (id, verb) = fake.requests().pop().expect("the release");
    assert_eq!(verb, format!("meta unset supervisor holder={holder}"));
    assert_ne!(id, stream);
    assert_eq!(fake.holder(), None);
    eventually("the stream closed", || !fake.streaming());
    assert!(link.is_absent() && !link.streaming() && link.epoch().is_none());
    assert_eq!(
        link.send(title(), WALL, clock.now()),
        Err(SendError::NoStream)
    );
}

/// aterm keeps a lost contract-4 claim through its re-exec grace, so a harness that exits while
/// its link is lost gives it back on a connection of its own; and a claim made again inside the
/// grace that aterm refuses gives it back too, so it never outlives the harness's word.
#[test]
fn a_contract_4_claim_kept_through_the_grace_is_released_explicitly() {
    let mut clock = Clock::new();
    let holder = config().holder(std::process::id());
    let release = format!("meta unset supervisor holder={holder}");
    let fake = served("grace-release", |fake| fake.set_reexec_grace(true));
    let mut link = launched(&fake, config(), child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    close_and_lose(&fake, &mut link, clock.now());
    // The fake unbinds the claim on its connection's own thread, once its
    // read ends, which can come after the link found the stream closed.
    eventually("the claim kept through the grace", || fake.in_grace());
    assert!(fake.holder().is_some());
    link.release(clock.now());
    let (id, verb) = fake.requests().pop().expect("the release");
    assert_eq!(verb, release);
    assert!(id > 1, "on a connection of its own");
    assert_eq!(fake.holder(), None);

    let fake = served("grace-refused", |fake| fake.set_reexec_grace(true));
    let mut link = launched(&fake, config(), child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    close_and_lose(&fake, &mut link, clock.now());
    fake.set_policy(&POLICY_ON.replace("effective=on why=enabled", "effective=off why=disabled"));
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.is_lost());
    assert_eq!(
        fake.holder(),
        None,
        "the refused reclaim gave the claim back"
    );
    assert_eq!(
        fake.verbs().iter().filter(|verb| **verb == release).count(),
        1
    );
    let before = fake.requests().len();
    link.release(clock.now());
    assert_eq!(fake.requests().len(), before, "nothing of it stands");
}

/// Every record aterm sends is answered once. The link answers what it acts on (`hand`,
/// `policy`, `restart`), a kind it does not know (`took`, ignored and counted) and what is not
/// the caller's; the caller answers `want` and the kinds its `session` takes, and may refuse one.
#[test]
fn each_aterm_record_is_answered_took_or_refused() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("answers", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    use record::Kind as K;
    assert_eq!(
        link.send(
            session(&[K::Prompt, K::Answer, K::Touch]),
            WALL,
            clock.now()
        ),
        Ok(3)
    );
    eventually("the session applied", || fake.stream_numbers().0 >= 3);
    let prompt = Incoming::Prompt {
        text: "go on".to_string(),
        by: record::By::Person,
    };
    let answer = Incoming::Answer {
        ask: 7,
        option: Some("yes".to_string()),
        text: None,
        by: record::By::Ctl,
        human_seq: 12,
    };
    for body in [
        Body::Hand {
            human_seq: 3,
            composer: record::Composer::Empty,
            hold: false,
        },
        Body::Policy,
        Body::Restart {
            cause: "asked".to_string(),
            account: None,
            model: None,
        },
        Body::Want,
        Body::Prompt {
            text: "go on".to_string(),
            by: record::By::Person,
        },
        Body::Answer {
            ask: 7,
            option: Some("yes".to_string()),
            text: None,
            by: record::By::Ctl,
            human_seq: 12,
        },
        Body::Touch { ask: 7 },
        Body::Interrupt,
        Body::Set {
            model: Some("opus".to_string()),
            effort: None,
            mode: None,
        },
        Body::Unknown {
            k: "later".to_string(),
        },
        Body::Beat,
    ] {
        assert!(fake.send_record(body));
    }
    // The fake's writer thread can send the records after the caller's four (10 to 13, which
    // the link answers itself) after those four were read, and only the link's reads answer
    // them: wait until it has answered all nine of its own, the open's two and these seven.
    let drained = await_stream(
        "every record read",
        &mut link,
        clock.now(),
        |drained, link| {
            let counts = link.counts();
            drained.incoming.len() == 4 && counts.took + counts.declined == 9
        },
    );
    assert_eq!(
        drained.incoming,
        [
            Received {
                of: 6,
                incoming: Incoming::Want
            },
            Received {
                of: 7,
                incoming: prompt
            },
            Received {
                of: 8,
                incoming: answer
            },
            Received {
                of: 9,
                incoming: Incoming::Touch { ask: 7 }
            },
        ]
    );
    assert_eq!(
        drained.restart.map(|restart| restart.cause).as_deref(),
        Some("asked")
    );
    for of in [6, 7, 9] {
        assert_eq!(link.answer(of, None, clock.now()), Ok(()));
    }
    assert_eq!(link.answer(8, Some("ask answered"), clock.now()), Ok(()));
    assert_eq!(
        link.answer(8, None, clock.now()),
        Err(AnswerError::NotAwaited(8))
    );
    assert_eq!(
        link.answer(10, None, clock.now()),
        Err(AnswerError::NotAwaited(10))
    );
    eventually("every answer", || {
        fake.took().len() + fake.refused().len() == 13
    });
    let mut took = fake.took();
    took.sort_unstable();
    assert_eq!(took, [1, 2, 3, 4, 5, 6, 7, 9, 12]);
    let mut refused = fake.refused();
    refused.sort_unstable();
    assert_eq!(
        refused,
        [
            (8, "ask answered".to_string()),
            (10, "not taken".to_string()),
            (11, "not taken".to_string()),
            (13, "harness kind".to_string()),
        ]
    );
    let counts = link.counts();
    assert_eq!((counts.took, counts.declined), (9, 4));
    assert_eq!(
        (
            counts.unknown_kinds,
            counts.hands,
            counts.policies,
            counts.restarts
        ),
        (1, 2, 2, 1)
    );
    // `want` goes to every stream: a link whose caller declared nothing still passes it up.
    let (other, mut bare) = streamed_at("answers-bare", child(), clock.now());
    settle_open(&other, &mut bare, clock.now());
    assert!(other.send_record(Body::Want));
    let drained = await_stream("the want", &mut bare, clock.now(), |drained, _| {
        !drained.incoming.is_empty()
    });
    assert_eq!(drained.incoming[0].of, 3);
    assert_eq!(bare.answer(3, None, clock.now()), Ok(()));
    eventually("the want answered", || other.took() == [1, 2, 3]);
    link.release(clock.now());
    bare.release(clock.now());
}

/// A `restart` record is the claim's [`Restart`], a `hand` record fills the hand fields of
/// [`Link::status`] and is returned as [`Drained::hand`], and a reannounce is a `claim` record,
/// which aterm places as a claim.
#[test]
fn restart_hand_and_reannounce_map_to_restart_status_and_a_claim_record() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("mapped", started(5151, 77, "2.1.311"), clock.now());
    settle_open(&fake, &mut link, clock.now());
    link.take_notes();

    fake.send_model_restart("model", "opus[1m]");
    let drained = await_stream("the restart", &mut link, clock.now(), |drained, _| {
        drained.restart.is_some()
    });
    assert_eq!(
        drained.restart,
        Some(Restart {
            cause: "model".to_string(),
            account: None,
            model: Some("opus[1m]".to_string()),
        })
    );
    assert_eq!(link.relaunch_word(true, clock.now()), Relaunch::Permitted);

    assert!(fake.send_hand(42, record::Composer::Draft, true));
    let drained = await_stream("the hand", &mut link, clock.now(), |_, link| {
        link.status().and_then(|status| status.human_seq) == Some(42)
    });
    assert_eq!(
        drained.hand,
        Some(Hand {
            of: 4,
            human_seq: 42,
            composer: record::Composer::Draft,
            hold: true
        })
    );
    let status = link.status().expect("the status");
    assert_eq!(status.composer.as_deref(), Some("draft"));
    assert_eq!(status.hold.as_deref(), Some("1"));

    let revision = fake.revision();
    link.reannounce(started(6262, 88, "2.1.312"), clock.now());
    let notes = link.take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].starts_with("aterm reannounced vendor=2.1.312 child=6262:88 seq="),
        "{notes:?}"
    );
    eventually("the placement", || {
        fake.child().as_deref() == Some("6262:88")
    });
    let claimed = fake
        .stream_records()
        .into_iter()
        .rev()
        .find(|record| matches!(record.body, Body::Claim { .. }))
        .expect("the claim record");
    assert_eq!(
        claimed.body,
        Body::Claim {
            child: Some(claim::ChildProcess {
                pid: 6262,
                start: 88
            }),
            scope: Some(vec![Scope::Relaunch, Scope::Upgrade]),
            version: Some("0.2.0".to_string()),
            vendor: Some("2.1.312".to_string()),
            login: None,
        }
    );
    assert_eq!(fake.revision(), revision + 1);
    // aterm's `policy` after the placement: the grant is read again at its revision.
    await_stream("the regrant", &mut link, clock.now(), |_, link| {
        link.grant().and_then(|grant| grant.revision) == Some(revision + 1)
    });
    assert!(link.streaming() && link.take_notes().is_empty());
    assert_eq!(
        fake.verbs()
            .iter()
            .filter(|verb| verb.starts_with("meta set supervisor"))
            .count(),
        1,
        "no claim line after the launch's"
    );
    link.release(clock.now());
}

/// An aterm that answers `stream` with an `ERR` keeps the v0.1 path, unnoted: the
/// subscription's restarts, claim lines on the claim's connection. An `ERR` an aterm process
/// gives for its life (`ERR unknown verb`) is not asked again on that connection, only on the
/// next claim's; `ERR busy streams` is asked again on it five seconds later, and the stream
/// opens once aterm serves it.
#[test]
fn a_refused_stream_keeps_the_v0_1_path_and_asks_again() {
    let t = 7_000_000;
    let fake = FakeAterm::for_harness("refused-stream", &config());
    let mut link = launched(&fake, config(), child(), t);
    assert!(link.judges() && !link.streaming());
    assert_eq!(fake.stream_requests(), 1);
    let notes = link.take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("aterm claimed "), "{notes:?}");
    assert_eq!(link.send(title(), WALL, t), Err(SendError::NoStream));
    assert!(link.stream_fd().is_none() && link.epoch().is_none());

    for pass in 1..=3 {
        link.tick(child(), t + pass * RECLAIM_EVERY_MS);
    }
    assert_eq!(
        fake.stream_requests(),
        1,
        "not asked again on that connection"
    );
    assert!(link.take_notes().is_empty());

    fake.send_restart("asked");
    let clock = Clock::new();
    let drained = await_events("the EVENT restart", &mut link, &clock, |drained, _| {
        drained.restart.is_some()
    });
    assert_eq!(
        drained.restart.map(|restart| restart.cause).as_deref(),
        Some("asked")
    );
    let claimed = fake.claim_connection().expect("the claim");
    link.reannounce(started(5151, 78, "2.1.311"), t + RECLAIM_EVERY_MS);
    let (id, verb) = fake.requests().pop().expect("the claim line");
    assert!(verb.starts_with("meta set supervisor ") && verb.ends_with(" child=5151:78"));
    assert_eq!(id, claimed);

    // An aterm updated in its place ends the connection; the next claim asks again.
    fake.serve_stream(true);
    fake.set_stream_reply(Some("ERR busy streams"));
    fake.restart();
    eventually("the link lost", || {
        let _ = link.read(t + 3 * RECLAIM_EVERY_MS);
        link.is_lost()
    });
    link.tick(child(), t + 4 * RECLAIM_EVERY_MS);
    assert!(link.judges() && !link.streaming());
    assert_eq!(fake.stream_requests(), 2);
    link.tick(child(), t + 5 * RECLAIM_EVERY_MS - 1);
    assert_eq!(fake.stream_requests(), 2);
    fake.set_stream_reply(None);
    link.tick(child(), t + 5 * RECLAIM_EVERY_MS);
    assert_eq!(
        fake.stream_requests(),
        3,
        "busy is asked again on the same connection"
    );
    assert!(link.streaming());
    assert_eq!(fake.stream_connection(), fake.claim_connection());
    let notes = link.take_notes();
    assert_eq!(
        notes.last().map(String::as_str),
        Some("aterm streaming epoch=fake-2 acked=0 in=0 resent=0"),
        "{notes:?}"
    );
    link.release(t + 5 * RECLAIM_EVERY_MS);
}

/// `ERR busy streams` keeps what the stream would resend and what the caller takes: the next
/// stream resends the records from `acked=` and passes up the kinds the caller's `session`
/// named, with nothing given up.
#[test]
fn a_busy_stream_keeps_the_kept_records_and_what_the_caller_takes() {
    let mut clock = Clock::new();
    let (fake, mut link) = streamed_at("busy", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    use record::Kind as K;
    assert_eq!(link.send(session(&[K::Prompt]), WALL, clock.now()), Ok(3));
    eventually("the session applied", || fake.stream_numbers().0 >= 3);
    fake.set_stream_deaf(true);
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(4));
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(5));
    fake.set_stream_reply(Some("ERR busy streams"));
    close_and_lose(&fake, &mut link, clock.now());
    fake.set_stream_deaf(false);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.judges() && !link.streaming());
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(6), "still kept");
    fake.set_stream_reply(None);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.streaming());
    eventually("the resend", || fake.stream_numbers().0 >= 6);
    assert_eq!(
        received(&fake)[3..6],
        [4, 5, 6].map(|seq| (seq, "title".to_string()))
    );
    assert!(fake.send_record(Body::Prompt {
        text: "go on".to_string(),
        by: record::By::Ctl,
    }));
    let drained = await_stream("the prompt", &mut link, clock.now(), |drained, _| {
        !drained.incoming.is_empty()
    });
    assert!(matches!(
        drained.incoming[0].incoming,
        Incoming::Prompt { .. }
    ));
    assert!(
        link.take_notes()
            .iter()
            .all(|note| !note.contains("given-up")),
        "nothing given up"
    );
    assert_eq!(link.counts().given_up, 0);
    link.release(clock.now());
}

/// A claim aterm forgot when its re-exec grace ended is a new claimant in the same epoch: the
/// link's answers of the old stream are dropped, the caller's records numbered from 1 and each
/// move reported, and the caller told to send its `session` and `state` again.
#[test]
fn a_claim_forgotten_at_the_grace_end_is_a_fresh_stream_and_reports_each_new_number() {
    let mut clock = Clock::new();
    let fake = served("forgotten", |fake| fake.set_reexec_grace(true));
    let mut link = launched(&fake, config(), child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    fake.set_stream_deaf(true);
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(3));
    assert!(fake.send_record(Body::Want));
    let drained = await_stream("the want", &mut link, clock.now(), |drained, _| {
        !drained.incoming.is_empty()
    });
    assert_eq!(drained.incoming[0].of, 3);
    assert_eq!(link.answer(3, None, clock.now()), Ok(()));
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(5));
    close_and_lose(&fake, &mut link, clock.now());
    fake.set_stream_deaf(false);
    eventually("the claim kept through the grace", || fake.in_grace());
    fake.end_grace();
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.streaming());
    let drained = await_stream("the new stream", &mut link, clock.now(), |_, _| {
        fake.stream_numbers().0 >= 2 && fake.took().len() >= 4
    });
    assert!(drained.fresh);
    assert_eq!(drained.renumbered, [(3, 1), (5, 2)]);
    assert_eq!(
        fake.took(),
        [1, 2, 1, 2],
        "no answer of the old stream names the new stream's records"
    );
    let applied: Vec<(u64, String)> = received(&fake).into_iter().skip(2).collect();
    assert_eq!(
        applied,
        [
            (1, "title".to_string()),
            (2, "title".to_string()),
            (3, "took".to_string()),
            (4, "took".to_string())
        ],
        "the new stream's records: the caller's, then the answers to its open"
    );
    link.release(clock.now());
}

/// The caller's records wait for aterm's acknowledgement up to the bound; past it a record is
/// refused, not numbered, while the link's own records still go.
#[test]
fn a_full_resend_queue_refuses_the_callers_record() {
    let t = 9_000_000;
    let (fake, mut link) = streamed_at("full", child(), t);
    settle_open(&fake, &mut link, t);
    fake.set_stream_deaf(true);
    for seq in 3..3 + stream::RESEND_MAX as u64 {
        assert_eq!(link.send(title(), WALL, t), Ok(seq));
    }
    assert_eq!(link.send(title(), WALL, t), Err(SendError::Full));
    let written = link.counts().written;
    link.tick(child(), t + BEAT_EVERY_MS);
    assert!(link.streaming());
    assert_eq!(link.counts().written, written + 1, "the beat went");
    link.release(t + BEAT_EVERY_MS);
}

/// The link writes at its pace under aterm's budget of 500 at once and 50 a second, so a full
/// queue written again on a reconnect is applied whole, never `dropped=rate`; a `claim` record
/// aterm drops for the rate all the same is sent again, not taken as refused.
#[test]
fn a_full_resend_keeps_under_aterms_budget_and_a_claim_dropped_for_the_rate_is_sent_again() {
    let mut clock = Clock::new();
    let (fake, mut link) = streamed_at("paced", started(5151, 77, "2.1.311"), clock.now());
    settle_open(&fake, &mut link, clock.now());
    fake.set_stream_deaf(true);
    for _ in 0..stream::RESEND_MAX {
        link.send(title(), WALL, clock.now()).expect("kept");
    }
    close_and_lose(&fake, &mut link, clock.now());
    fake.set_stream_deaf(false);
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    let resent = stream::RESEND_MAX as u64 + 2;
    eventually("the resend", || fake.stream_numbers().0 >= resent);
    assert!(
        fake.acks().iter().all(|(_, dropped)| dropped.is_none()),
        "nothing dropped for the rate"
    );
    assert_eq!(link.counts().rate, 0);

    fake.drop_next_record(Dropped::Rate);
    link.reannounce(started(6262, 88, "2.1.312"), clock.now());
    eventually("the placement", || {
        link.tick(child(), clock.now());
        fake.child().as_deref() == Some("6262:88")
    });
    assert!(link.streaming() && !link.is_lost());
    assert_eq!(link.counts().rate, 1);
    link.release(clock.now());
}

/// An aterm restart ends the stream; the new instance is a new epoch, whose `acked=` starts
/// again, so the records the old one did not acknowledge are numbered from 1, each move is
/// reported, and the link's answers to the old one's records are given up.
#[test]
fn an_aterm_restart_is_a_new_epoch_and_the_kept_records_are_numbered_from_one() {
    let mut clock = Clock::new();
    let (fake, mut link) = streamed_at("epoch", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    fake.set_stream_deaf(true);
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(3));
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(4));
    fake.restart();
    fake.set_stream_deaf(false);
    await_stream("the restart", &mut link, clock.now(), |_, link| {
        link.is_lost()
    });
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    assert!(link.streaming());
    assert_eq!(link.epoch(), Some("fake-2"));
    let notes = link.take_notes();
    assert_eq!(
        notes.last().map(String::as_str),
        Some("aterm streaming epoch=fake-2 acked=0 in=0 resent=2"),
        "{notes:?}"
    );
    eventually("the resend", || fake.stream_numbers().0 >= 2);
    assert_eq!(
        fake.stream_records()
            .into_iter()
            .filter(|record| record.body == title())
            .map(|record| record.seq)
            .collect::<Vec<_>>(),
        [1, 2],
        "the old epoch never applied 3 and 4"
    );
    // The new epoch's records are heard from 1 again.
    let drained = await_stream("the new epoch's records", &mut link, clock.now(), |_, _| {
        fake.took().iter().filter(|seq| **seq <= 2).count() == 4
    });
    assert!(drained.fresh);
    assert_eq!(drained.renumbered, [(3, 1), (4, 2)]);
    link.release(clock.now());
}

/// Each `dropped=` reason is counted and noted once a stream; a `claim` record aterm refuses
/// loses the link, as a refused claim line does.
#[test]
fn a_dropped_record_is_counted_and_noted_once_and_a_dropped_claim_loses_the_link() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("dropped", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    link.take_notes();
    let wall = || Body::Wall {
        kind: "usage-session".to_string(),
        resets_ms: None,
    };
    assert_eq!(link.send(wall(), WALL, clock.now()), Ok(3));
    assert_eq!(link.send(wall(), WALL, clock.now()), Ok(4));
    eventually("the walls", || fake.acks().len() == 4);
    fake.drop_next_record(Dropped::Rate);
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(5));
    await_stream("the drops", &mut link, clock.now(), |_, link| {
        link.counts().acks == 5
    });
    let counts = link.counts();
    assert_eq!((counts.scope, counts.rate), (2, 1));
    assert_eq!(
        link.take_notes(),
        [
            "aterm stream-dropped seq=3 k=wall dropped=scope",
            "aterm stream-dropped seq=5 k=title dropped=rate"
        ]
    );
    assert!(link.streaming());

    fake.set_claim_reply("ERR busy supervisor=other@9");
    link.reannounce(started(5151, 79, "2.1.311"), clock.now());
    await_stream("the refused claim", &mut link, clock.now(), |_, link| {
        link.is_lost()
    });
    let notes = link.take_notes();
    assert_eq!(
        notes.last().map(String::as_str),
        Some("aterm lost reannounce dropped=refused:busy"),
        "{notes:?}"
    );
}

/// A contract-3 claim never asks for the stream, and neither does a config that does not
/// stream: the v0.1 path is all there is.
#[test]
fn a_contract_3_claim_or_a_config_without_the_stream_never_asks_for_it() {
    let t = 11_000_000;
    let fake = served("three", |fake| fake.serve_contracts(&[2, 3]));
    let mut link = launched(&fake, config(), child(), t);
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Three)
    );
    for pass in 1..=3 {
        link.tick(child(), t + pass * RECLAIM_EVERY_MS);
    }
    assert_eq!(fake.stream_requests(), 0);
    assert!(!link.streaming());
    assert_eq!(link.send(title(), WALL, t), Err(SendError::NoStream));
    link.release(t + 4 * RECLAIM_EVERY_MS);

    let fake = served("no-stream", |_| {});
    let mut link = launched(&fake, config().with_stream(false), child(), t);
    assert_eq!(
        link.grant().map(|grant| grant.contract),
        Some(Contract::Four)
    );
    for pass in 1..=3 {
        link.tick(child(), t + pass * RECLAIM_EVERY_MS);
    }
    assert_eq!(fake.stream_requests(), 0);
    assert!(link.judges() && !link.streaming());
    assert_eq!(link.send(title(), WALL, t), Err(SendError::NoStream));
    link.release(t + 4 * RECLAIM_EVERY_MS);
}

/// Against an aterm that opens streams and sends no record of its own (HX-15b1 without
/// HX-15c), the subscription's restart and policy events are still heard; and an `EVENT
/// restart` aterm sent before the stream opened is not lost to the open.
#[test]
fn events_are_heard_until_the_stream_brings_one_of_aterms_records() {
    let clock = Clock::new();
    let fake = served("b1-alone", |fake| fake.set_stream_own_records(false));
    let mut link = launched(&fake, config(), child(), clock.now());
    assert!(link.streaming());
    fake.send_restart("asked");
    let drained = await_events("the EVENT restart", &mut link, &clock, |drained, _| {
        drained.restart.is_some()
    });
    assert_eq!(
        drained.restart.map(|restart| restart.cause).as_deref(),
        Some("asked")
    );
    let before = fake.requests().len();
    fake.set_cap(Some(&[Scope::Upgrade]));
    await_events("the EVENT harness-policy", &mut link, &clock, |_, link| {
        !link.takes(Scope::Relaunch)
    });
    assert!(fake.requests().len() > before, "the policy was read");
    link.release(clock.now());

    let t = 12_000_000;
    let fake = served("event-before-open", |fake| {
        fake.set_stream_reply(Some("ERR busy streams"));
    });
    let mut link = launched(&fake, config(), child(), t);
    assert!(!link.streaming());
    fake.send_restart("asked");
    let fd = link.event_fd().expect("the subscription");
    let mut probe = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: probe is one initialized poll record for the link's open descriptor.
    let ready = unsafe { libc::poll(&raw mut probe, 1, 5_000) };
    assert_eq!(ready, 1, "the event arrived");
    fake.set_stream_reply(None);
    link.tick(child(), t + RECLAIM_EVERY_MS);
    assert!(link.streaming());
    let drained = await_stream(
        "the open's records",
        &mut link,
        t + RECLAIM_EVERY_MS,
        |_, link| link.counts().acks >= 2,
    );
    assert_eq!(
        drained.restart.map(|restart| restart.cause).as_deref(),
        Some("asked"),
        "the restart sent before the open"
    );
    link.release(t + RECLAIM_EVERY_MS);
}

/// An `OK` to `stream` that does not read (here one naming a count past [`stream::SEQ_MAX`])
/// loses the link once and ends the stream for its life: the claim is made again and kept on
/// the v0.1 path, never asked for the stream again, so it does not churn.
#[test]
fn an_ok_that_does_not_read_ends_the_stream_for_the_links_life() {
    let t = 13_000_000;
    let fake = served("bad-ok", |fake| {
        fake.set_stream_reply(Some(
            "OK stream v=1 epoch=x acked=18446744073709551615 in=0",
        ));
    });
    let mut link = launched(&fake, config(), child(), t);
    assert!(link.is_lost());
    let notes = link.take_notes();
    assert!(
        notes
            .iter()
            .any(|note| note.starts_with("aterm stream-off ")),
        "{notes:?}"
    );
    for pass in 1..=3 {
        link.tick(child(), t + pass * RECLAIM_EVERY_MS);
    }
    assert!(link.judges() && !link.streaming());
    assert_eq!(fake.stream_requests(), 1);
    link.release(t + 4 * RECLAIM_EVERY_MS);
}

/// Numbers past [`stream::SEQ_MAX`] from aterm, or an acknowledgement of a record never
/// written, lose the stream; nothing panics or wraps, and the claim is made again.
#[test]
fn protocol_faults_from_aterm_lose_the_stream_and_never_wrap() {
    let mut clock = Clock::new();
    let (fake, mut link) = streamed_at("faults", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    for line in [
        r#"{"k":"ack","seq":18446744073709551615,"ms":0}"#,
        r#"{"k":"want","seq":18446744073709551615,"ms":0}"#,
        r#"{"k":"ack","seq":40,"ms":0}"#,
    ] {
        link.take_notes();
        assert!(fake.send_line(line));
        await_stream(line, &mut link, clock.now(), |_, link| link.is_lost());
        let notes = link.take_notes();
        assert!(notes[0].starts_with("aterm lost stream "), "{notes:?}");
        clock.pass(RECLAIM_EVERY_MS);
        link.tick(child(), clock.now());
        assert!(link.streaming());
    }
    assert!(fake.send_line(r#"{"k":"want","seq":9223372036854775807,"ms":0}"#));
    assert!(fake.send_line("not json"));
    let drained = await_stream("the last want", &mut link, clock.now(), |_, link| {
        link.counts().unreadable == 1
    });
    assert_eq!(drained.incoming[0].of, stream::SEQ_MAX);
    assert!(link.streaming());
    link.release(clock.now());
}

/// A line that does not read is refused when its `seq` can be found, but an `ack` that does not
/// read moves no number: aterm's next record is still heard and answered.
#[test]
fn an_unreadable_ack_moves_no_number_and_the_next_record_is_answered() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("unreadable", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    assert!(fake.send_line(r#"{"k":"ack","seq":"3","ms":0}"#));
    assert!(fake.send_record(Body::Want));
    assert!(fake.send_line(r#"{"k":"want","seq":4,"ms":"x"}"#));
    let drained = await_stream("the want", &mut link, clock.now(), |_, link| {
        link.counts().unreadable == 2
    });
    assert_eq!(drained.incoming.len(), 1);
    assert_eq!(drained.incoming[0].of, 3);
    assert_eq!(link.counts().repeated, 0);
    eventually("the refusal", || !fake.refused().is_empty());
    assert_eq!(fake.refused()[0].0, 4);
    link.release(clock.now());
}

/// Only a record that can end a hold (`hand`, `policy`, `restart`) has `status` read again: the
/// acknowledgements of the harness's own records do not.
#[test]
fn acknowledgements_do_not_end_a_hold() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("hold", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    fake.set_status("hold", Some("1"));
    assert!(matches!(
        link.safe_point(Duration::ZERO, clock.now()),
        Verdict::Hold(_)
    ));
    assert!(link.held_back(clock.now()));
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(3));
    await_stream("the ack", &mut link, clock.now(), |_, link| {
        link.counts().acks >= 3
    });
    link.tick(child(), clock.now());
    assert!(link.held_back(clock.now()));
    assert!(fake.send_hand(9, record::Composer::Empty, false));
    await_stream("the hand", &mut link, clock.now(), |drained, _| {
        drained.hand.is_some()
    });
    assert!(!link.held_back(clock.now()));
    link.release(clock.now());
}

/// The link's own records past [`stream::OWN_MAX`] unacknowledged lose the stream: aterm sends
/// records and acknowledges none of the answers.
#[test]
fn own_records_past_their_bound_lose_the_stream() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("own-bound", child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    link.take_notes();
    fake.set_stream_deaf(true);
    for _ in 0..=stream::OWN_MAX {
        assert!(fake.send_record(Body::Interrupt));
    }
    await_stream("the loss", &mut link, clock.now(), |_, link| link.is_lost());
    assert_eq!(
        link.take_notes(),
        [format!("aterm lost stream own-queue={}", stream::OWN_MAX)]
    );
}

/// `request` sends a session-addressed request on the held request connection, the second one
/// while a stream is open, and refuses the requests that are the link's own.
#[test]
fn request_goes_on_the_second_connection_and_refuses_the_links_own_verbs() {
    let clock = Clock::new();
    let (fake, mut link) = streamed_at("request", child(), clock.now());
    let stream = fake.stream_connection().expect("the stream");
    assert_eq!(
        link.request("meta set harness_act word=x", clock.now()),
        Ok("ERR unknown verb".to_string())
    );
    let (id, verb) = fake.requests().pop().expect("the request");
    assert_eq!(verb, "meta set harness_act word=x");
    assert_ne!(id, stream);
    let before = fake.requests().len();
    for verb in [
        "meta set supervisor x@1 contract=4",
        "meta  unset supervisor",
        "stream @s v=1",
        "subscribe @s events",
        "@s status",
        "status\nstatus",
        "",
    ] {
        assert!(
            matches!(link.request(verb, clock.now()), Err(Failure::Refused(_))),
            "{verb:?}"
        );
    }
    assert_eq!(fake.requests().len(), before);
    assert!(link.streaming());
    link.release(clock.now());
}

/// Each stream's rate budget is full at its open, as aterm's is a stream's (D10).
#[test]
fn each_stream_opens_with_a_full_rate_budget() {
    let mut clock = Clock::new();
    let fake = served("budget", |fake| fake.set_stream_rate(3, 0));
    let mut link = launched(&fake, config(), child(), clock.now());
    settle_open(&fake, &mut link, clock.now());
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(3));
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(4));
    await_stream("the drop", &mut link, clock.now(), |_, link| {
        link.counts().acks >= 4
    });
    assert_eq!(link.counts().rate, 1);
    close_and_lose(&fake, &mut link, clock.now());
    clock.pass(RECLAIM_EVERY_MS);
    link.tick(child(), clock.now());
    await_stream("the new open", &mut link, clock.now(), |_, link| {
        link.counts().acks >= 6
    });
    // The open's answers are 5 and 6, and fit the new budget with this record.
    assert_eq!(link.send(title(), WALL, clock.now()), Ok(7));
    await_stream("the title", &mut link, clock.now(), |_, link| {
        link.counts().acks >= 7
    });
    assert_eq!(link.counts().rate, 1, "the new stream had its budget");
    link.release(clock.now());
}
