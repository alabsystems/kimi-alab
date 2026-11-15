// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The link's pure rules, and the link driven against the fake aterm.

use std::time::Instant;

use super::*;
use crate::fake::{FakeAterm, POLICY_ON};

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

#[test]
fn a_policy_admits_a_claim_only_with_contract_three_or_four_in_effect() {
    let policy = |contract: &str, effective: &str| {
        Policy::parse(&format!(
            "OK gen=1 effective={effective} contract={contract} relaunch=true upgrade=true \
             upgrade_ask=false human_grace_s=120"
        ))
        .expect("a policy")
    };
    assert_eq!(admits(&policy("2,3", "on")), Ok(()));
    assert_eq!(admits(&policy("4", "on")), Ok(()));
    assert_eq!(admits(&policy("2,3,4", "on")), Ok(()));
    // An aterm that serves no contract this library speaks may serve one
    // after its update: asked again on the clock.
    assert_eq!(
        admits(&policy("2", "on")),
        Err(Failure::Unserved("policy contract=2".to_string()))
    );
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
        "policy contract=2,5"
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
            "policy contract=2"
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
    assert!(policy.upgrade_ask && policy.admits_claim());
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
    assert_eq!(link.take_notes(), ["aterm unclaimed policy contract=2"]);
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
