//! Her temporary host's deputies against a real database: kept as issued and
//! read back the same, given on a link once its end keeps up, renewed by the
//! hour, sent again until its end says it took it, and gone with the end or
//! with syncing.

use super::*;
use crate::services::her_log::deputy::{
    self, Deputy, RENEW_EVERY_MICROS, RESEND_EVERY_MICROS, RUNS_MICROS,
};

const HOUR: i64 = 60 * MINUTE;

/// The deputies `out` sends, as sent.
fn deputies_in(out: &[Outgoing]) -> Vec<Deputy> {
    out.iter()
        .filter(|out| out.t == deputy::FRAME)
        .map(|out| {
            assert_eq!(out.to, To::Sender);
            let given: Deputy = serde_json::from_value(out.body["deputy"].clone()).unwrap();
            assert_eq!(
                out.id.as_deref(),
                Some(deputy::frame_id(&given).as_str()),
                "a deputy goes reliably, by an id of its own"
            );
            given
        })
        .collect()
}

/// What the server answers `side`'s vector of `audience`, at `now`.
async fn vector_of(
    db: &DatabaseConnection,
    side: &mut Side,
    audience: &str,
    now: i64,
) -> Vec<Outgoing> {
    let frame = side.vector(audience);
    let device = side.id.clone();
    relay::take(
        db,
        &root(),
        &on(),
        side.user,
        &device,
        Core::Pc,
        &mut side.link,
        &frame,
        now,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_deputy_is_kept_as_issued_and_read_back_the_same() {
    let Some(schema) = schema("her_log_deputy_kept").await else {
        return;
    };
    let db = &schema.db;
    let standing = lease::current(db).await.unwrap();
    let given = deputy::issue(db, "dev_mac", standing.epoch, 2).await.unwrap();
    assert_eq!(
        (given.holder.as_str(), given.under, given.order),
        ("dev_mac", standing.epoch, 2)
    );
    assert_eq!(
        given.until - given.issued.wall_ms as i64 * 1000,
        RUNS_MICROS,
        "a day from its issue"
    );
    assert_eq!(
        deputy::for_holders(db, &["dev_mac", "dev_win"]).await.unwrap(),
        vec![given.clone()]
    );
    assert!(deputy::for_holders(db, &["dev_win"]).await.unwrap().is_empty());
    assert!(deputy::for_holders(db, &[]).await.unwrap().is_empty());
    // Refused before anything is kept.
    assert!(deputy::issue(db, "dev_mac", 0, 1).await.is_err());
    assert!(deputy::issue(db, "dev_mac", 1, 0).await.is_err());

    // Its end said it took it; another end cannot say so for it.
    assert!(!deputy::received(db, "dev_win", &given.issued).await.unwrap());
    assert!(deputy::received(db, "dev_mac", &given.issued).await.unwrap());
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_lease_deputies WHERE received_at IS NOT NULL"
        )
        .await,
        1
    );

    let other = deputy::issue(db, "dev_win", standing.epoch, 1).await.unwrap();
    assert_eq!(deputy::withdraw(db, "dev_mac").await.unwrap(), 1);
    assert_eq!(
        deputy::for_holders(db, &["dev_mac", "dev_win"]).await.unwrap(),
        vec![other]
    );
    deputy::issue(db, "dev_mac", standing.epoch, 2).await.unwrap();
    assert_eq!(deputy::withdraw_all(db).await.unwrap(), 2);
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_lease_deputies").await,
        0
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_link_that_keeps_up_is_given_one_then_one_an_hour() {
    let Some(schema) = schema("her_log_deputy_given").await else {
        return;
    };
    let db = &schema.db;
    let standing = lease::current(db).await.unwrap();
    let settings = on();
    let mut mac = Side::new("dev_mac");
    mac.link = relay::Session::of(&["log2".to_string(), deputy::CAPABILITY.to_string()]);

    // The owner's private talk alone says nothing of her own yet.
    let out = vector_of(db, &mut mac, PERSONAL, T0).await;
    assert!(deputies_in(&out).is_empty());
    let out = vector_of(db, &mut mac, OWN, T0).await;
    let given = deputies_in(&out);
    assert_eq!(given.len(), 1, "once it keeps up with both");
    assert_eq!(
        (given[0].holder.as_str(), given[0].under, given[0].order),
        ("dev_mac", standing.epoch, 1)
    );
    assert_eq!(
        deputy::for_holders(db, &["dev_mac"]).await.unwrap(),
        given,
        "what was sent is what is kept"
    );
    // It says it took it: only that counts it delivered.
    let received = "SELECT COUNT(*) AS n FROM agent_lease_deputies WHERE received_at IS NOT NULL";
    assert_eq!(count(db, received).await, 0);
    let taken = mac.frame(
        "lease.taken",
        None,
        json!({ "of": deputy::FRAME, "id": deputy::frame_id(&given[0]) }),
    );
    let out = relay::take(db, &root(), &settings, OWNER, "dev_mac", Core::Pc, &mut mac.link, &taken, T0)
        .await
        .unwrap();
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(count(db, received).await, 1);
    // Another end cannot say so for it.
    let mut win_link = relay::Session::of(&["log2".to_string(), deputy::CAPABILITY.to_string()]);
    relay::take(db, &root(), &settings, OWNER, "dev_win", Core::Pc, &mut win_link, &taken, T0)
        .await
        .unwrap();
    assert_eq!(count(db, received).await, 1);

    // Again on the same link: not before the hour.
    let out = vector_of(db, &mut mac, OWN, T0 + MINUTE).await;
    assert!(deputies_in(&out).is_empty());
    let renewed = relay::renew(db, &settings, OWNER, "dev_mac", &mut mac.link, T0 + 2 * MINUTE).await;
    assert!(renewed.is_empty());
    // Its heartbeat an hour on gives the next.
    let renewed = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        T0 + RENEW_EVERY_MICROS + 2 * MINUTE,
    )
    .await;
    assert_eq!(deputies_in(&renewed).len(), 1);

    // A second computer that keeps up stands after it (nobody is using
    // either: by id).
    let mut win = Side::new("dev_win");
    win.link = relay::Session::of(&["log2".to_string(), deputy::CAPABILITY.to_string()]);
    vector_of(db, &mut win, PERSONAL, T0 + HOUR).await;
    let out = vector_of(db, &mut win, OWN, T0 + HOUR).await;
    assert_eq!(
        deputies_in(&out)
            .iter()
            .map(|given| (given.holder.as_str(), given.order))
            .collect::<Vec<_>>(),
        [("dev_win", 2)]
    );

    // One that did not list it gets none.
    let mut old = Side::new("dev_old");
    vector_of(db, &mut old, PERSONAL, T0).await;
    let out = vector_of(db, &mut old, OWN, T0).await;
    assert!(deputies_in(&out).is_empty());
    assert!(
        relay::renew(db, &settings, OWNER, "dev_old", &mut old.link, T0 + 2 * HOUR)
            .await
            .is_empty()
    );

    // Syncing off: all of them go, and none is given while it is off.
    lease::sync_off(db, &settings, OWNER, T0 + 2 * HOUR)
        .await
        .unwrap();
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_lease_deputies").await,
        0
    );
    assert!(
        relay::renew(db, &settings, OWNER, "dev_mac", &mut mac.link, T0 + 3 * HOUR)
            .await
            .is_empty()
    );
    schema.drop().await;
}

#[tokio::test]
async fn none_is_given_while_her_mind_is_on_a_device() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_deputy_mind_away").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    assert!(mac.epoch.is_some(), "her mind is on the Mac");
    let mut win = Side::new("dev_win");
    win.link = relay::Session::of(&["log2".to_string(), deputy::CAPABILITY.to_string()]);
    vector_of(db, &mut win, PERSONAL, T0).await;
    let out = vector_of(db, &mut win, OWN, T0).await;
    assert!(deputies_in(&out).is_empty());
    schema.drop().await;
}

/// A link whose hello listed `deputy`.
fn deputy_link() -> relay::Session {
    relay::Session::of(&["log2".to_string(), deputy::CAPABILITY.to_string()])
}

/// The ids of the deputies `out` sends, in order.
fn ids_in(out: &[Outgoing]) -> Vec<String> {
    deputies_in(out).iter().map(deputy::frame_id).collect()
}

/// `side` says it took the frame `of` by `id`.
async fn taken(db: &DatabaseConnection, side: &mut Side, of: &str, id: &str, now: i64) {
    let frame = side.frame("lease.taken", None, json!({ "of": of, "id": id }));
    let device = side.id.clone();
    let out = relay::take(
        db,
        &root(),
        &on(),
        side.user,
        &device,
        Core::Pc,
        &mut side.link,
        &frame,
        now,
    )
    .await
    .unwrap();
    assert!(out.is_empty(), "{out:?}");
}

#[tokio::test]
async fn a_deputy_not_taken_goes_again_by_the_same_id_until_it_is() {
    let Some(schema) = schema("her_log_deputy_again").await else {
        return;
    };
    let db = &schema.db;
    lease::current(db).await.unwrap();
    let settings = on();
    let mut mac = Side::new("dev_mac");
    mac.link = deputy_link();
    vector_of(db, &mut mac, PERSONAL, T0).await;
    let first = ids_in(&vector_of(db, &mut mac, OWN, T0).await);
    assert_eq!(first.len(), 1);

    // Its lease.taken did not come back: on the same link, not before a
    // while, then the same one again, by its id; and again after that.
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        T0 + RESEND_EVERY_MICROS - 1,
    )
    .await;
    assert!(out.is_empty(), "{out:?}");
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        T0 + RESEND_EVERY_MICROS,
    )
    .await;
    assert_eq!(ids_in(&out), first, "the same one, by the same id");
    let out = vector_of(db, &mut mac, OWN, T0 + RESEND_EVERY_MICROS + 1).await;
    assert!(ids_in(&out).is_empty(), "not again at once");
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        T0 + 2 * RESEND_EVERY_MICROS,
    )
    .await;
    assert_eq!(ids_in(&out), first);

    // Linked again: once it keeps up, that one again first, then a new
    // one as on every link, which replaces it.
    mac.link = deputy_link();
    let at = T0 + 3 * MINUTE;
    vector_of(db, &mut mac, PERSONAL, at).await;
    let out = ids_in(&vector_of(db, &mut mac, OWN, at).await);
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0], first[0], "the one not taken, by its id");
    let newer = out[1].clone();
    assert_ne!(newer, first[0]);
    // Only the newest goes again from now on.
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        at + RESEND_EVERY_MICROS,
    )
    .await;
    assert_eq!(ids_in(&out), [newer.clone()]);

    // Taken: it goes no more, on this link or the next; the one it
    // replaced, never taken, does not go again either.
    taken(
        db,
        &mut mac,
        deputy::FRAME,
        &newer,
        at + RESEND_EVERY_MICROS,
    )
    .await;
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        at + 5 * RESEND_EVERY_MICROS,
    )
    .await;
    assert!(out.is_empty(), "{out:?}");
    mac.link = deputy_link();
    let at = at + 6 * RESEND_EVERY_MICROS;
    vector_of(db, &mut mac, PERSONAL, at).await;
    let out = ids_in(&vector_of(db, &mut mac, OWN, at).await);
    assert_eq!(out.len(), 1, "only the new one of this link: {out:?}");
    assert!(out[0] != newer && out[0] != first[0]);
    schema.drop().await;
}

#[tokio::test]
async fn a_deputy_run_out_or_under_another_epoch_does_not_go_again() {
    let Some(schema) = schema("her_log_deputy_not_again").await else {
        return;
    };
    let db = &schema.db;
    lease::current(db).await.unwrap();
    let settings = on();
    let mut mac = Side::new("dev_mac");
    mac.link = deputy_link();
    vector_of(db, &mut mac, PERSONAL, T0).await;
    let given = deputies_in(&vector_of(db, &mut mac, OWN, T0).await);
    assert_eq!(given.len(), 1);
    let first = deputy::frame_id(&given[0]);
    assert_eq!(
        deputy::unreceived(db, "dev_mac", T0)
            .await
            .unwrap()
            .map(|one| deputy::frame_id(&one)),
        Some(first.clone())
    );

    // Run out: the heartbeat then gives a new one (an hour has gone by) and
    // not the old one again.
    let ran_out = given[0].until;
    assert!(
        deputy::unreceived(db, "dev_mac", ran_out)
            .await
            .unwrap()
            .is_none()
    );
    let out = ids_in(&relay::renew(db, &settings, OWNER, "dev_mac", &mut mac.link, ran_out).await);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_ne!(out[0], first);

    // Another epoch stands: the newest, still running, does not go again
    // under it.
    let before = ran_out - 1;
    assert_eq!(
        deputy::unreceived(db, "dev_mac", before)
            .await
            .unwrap()
            .map(|one| deputy::frame_id(&one)),
        Some(out[0].clone())
    );
    // Away to the Mac and back: she is on the site again, two epochs on.
    let minds = lease::minds_go_to_devices_in_test(true);
    for to in ["dev_mac", NODE] {
        let outcome = lease::request(
            db,
            &settings,
            myriad_persona_log::Request::Force { to: to.to_string() },
            before,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, lease::Outcome::Granted(_)), "{outcome:?}");
    }
    drop(minds);
    assert!(lease::current(db).await.unwrap().held_by(NODE));
    assert!(
        deputy::unreceived(db, "dev_mac", before)
            .await
            .unwrap()
            .is_none()
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_grant_not_taken_goes_again_until_it_is() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_grant_again").await else {
        return;
    };
    let db = &schema.db;
    let settings = on();
    let mut mac = Side::new("dev_mac");
    mac.link = deputy_link();
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    let epoch = mac.epoch.expect("her mind is on the Mac");
    let grants = |out: &[Outgoing]| -> Vec<String> {
        out.iter()
            .filter(|out| out.t == "lease.grant")
            .map(|out| {
                assert_eq!(out.to, To::Sender);
                delivered(out);
                out.id.clone().unwrap()
            })
            .collect()
    };

    // Its lease.taken did not come back: again after a while, by its epoch.
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        T0 + RESEND_EVERY_MICROS - 1,
    )
    .await;
    assert!(grants(&out).is_empty(), "{out:?}");
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        T0 + RESEND_EVERY_MICROS,
    )
    .await;
    assert_eq!(grants(&out), [epoch.to_string()]);
    // Linked again: at once.
    mac.link = deputy_link();
    let at = T0 + RESEND_EVERY_MICROS + 1;
    let out = relay::renew(db, &settings, OWNER, "dev_mac", &mut mac.link, at).await;
    assert_eq!(grants(&out), [epoch.to_string()]);
    // Taken: no more.
    taken(db, &mut mac, "lease.grant", &epoch.to_string(), at).await;
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut mac.link,
        at + 5 * RESEND_EVERY_MICROS,
    )
    .await;
    assert!(grants(&out).is_empty(), "{out:?}");

    // An end that did not list deputy goes by the ack, as before.
    let mut old = Side::new("dev_mac");
    let out = relay::renew(
        db,
        &settings,
        OWNER,
        "dev_mac",
        &mut old.link,
        at + 6 * RESEND_EVERY_MICROS,
    )
    .await;
    assert!(out.is_empty(), "{out:?}");
    schema.drop().await;
}

/// A row refused for good, on the server or on the computer, keeps its
/// place there: the count of its side goes on past it, it is not sent again
/// on every link, and the computer is seen to keep up, so it is given its
/// deputy. Here the same node is in the partner's private talk on one
/// computer and in her own audience on the other; each side that knows it
/// in one refuses its row in the other (`audience_changed`).
#[tokio::test]
async fn a_row_refused_for_good_keeps_its_place_and_its_end_keeps_up() {
    let Some(schema) = schema("her_log_deputy_refused").await else {
        return;
    };
    let db = &schema.db;
    let mut win = Side::new("dev_win");
    let mut mac = Side::new("dev_mac");
    mac.link = relay::Session::of(&["log2".to_string(), deputy::CAPABILITY.to_string()]);
    win.write(PERSONAL, "node:z", "node.noted", Author::Owner, Some("in the talk"), T0);
    sync(db, &mut win, PERSONAL, T0).await;
    mac.write(OWN, "node:z", "node.noted", Author::Owner, Some("in her own"), T0);

    // The server knows node:z in the talk: the Mac's row in her own is
    // refused, and kept as its place.
    sync(db, &mut mac, OWN, T0 + MINUTE).await;
    let own = store::holdings(db, OWNER, OWN).await.unwrap();
    assert_eq!(own.vector.get("dev_mac"), Some(&1), "{own:?}");
    assert!(own.beyond.is_empty(), "{own:?}");
    // The Mac knows it in her own: the talk's row is refused there.
    sync(db, &mut mac, PERSONAL, T0 + 2 * MINUTE).await;
    let talk = store::holdings(db, OWNER, PERSONAL).await.unwrap();
    assert_eq!(vector::holdings(&mac.log(PERSONAL)).unwrap().vector, talk.vector);
    assert_eq!(vector::holdings(&mac.log(OWN)).unwrap().vector, own.vector);
    // Neither folds anything of the row it refused.
    let mac_talk = String::from_utf8(mac.fold(PERSONAL, &[])).unwrap();
    assert!(!mac_talk.contains("node:z"), "{mac_talk}");
    let server_own = String::from_utf8(server_fold(db, OWN, &[]).await).unwrap();
    assert!(!server_own.contains("node:z"), "{server_own}");

    // Keeping up with both, it is given its deputy.
    vector_of(db, &mut mac, PERSONAL, T0 + 3 * MINUTE).await;
    let out = vector_of(db, &mut mac, OWN, T0 + 3 * MINUTE).await;
    assert_eq!(deputies_in(&out).len(), 1, "{out:?}");
    schema.drop().await;
}
