//! Her temporary host's deputies against a real database: kept as issued and
//! read back the same, given on a link once its end keeps up, renewed by the
//! hour, and gone with the end or with syncing.

use super::*;
use crate::services::her_log::deputy::{self, Deputy, RENEW_EVERY_MICROS, RUNS_MICROS};

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
