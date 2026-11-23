//! Whose word it is taken for that a record of hers is gone, against a real
//! database: an end that held her then, and what the owner changes; not a
//! temporary host, an end that did not hold her, or one that had handed her
//! on. What is not taken stays on the site and is live again in her log.

use super::*;
use crate::services::her_log::deputy;
use crate::services::her_log::gone::{self, Refused};
use crate::services::her_log::state;
use myriad_persona_sync::Kind;

const SECOND: i64 = 1_000_000;

fn diary(id: &str, state: &str, content: &str, at: i64) -> Value {
    let body = match state {
        "live" => {
            json!({ "content": content, "source": "chat", "createdAt": at / 1000, "subject": "owner" })
        }
        _ => Value::Null,
    };
    json!({
        "at": at / 1000,
        "format": myriad_persona_sync::FORMAT_VERSION,
        "kind": "diary",
        "id": id,
        "state": state,
        "body": body,
    })
}

/// `side` writes the diary entry `id` gone, as `author` at `epoch`.
fn writes_gone(side: &mut Side, id: &str, author: Author, at: i64) -> Row {
    side.write_body(
        PERSONAL,
        &format!("state:diary:{id}"),
        "state.write",
        author,
        Some(diary(id, "deleted", "", at)),
        at,
    )
}

/// The site with the diary entries `ids`, in her log as hers.
async fn with_diary(prefix: &str, ids: &[&str]) -> Option<crate::db::IsolatedSchema> {
    let schema = schema(prefix).await?;
    for id in ids {
        schema
            .db
            .execute_unprepared(&format!(
                "INSERT INTO agent_diary (id, user_id, content, source, created_at) \
                 VALUES ('{id}', {OWNER}, '{id} 那天', 'chat', now())"
            ))
            .await
            .unwrap();
    }
    let wrote = state::sync(&schema.db, &root(), OWNER).await.unwrap();
    assert_eq!(wrote.written, ids.len());
    Some(schema)
}

/// The diary as the site has it.
async fn entries(db: &DatabaseConnection) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!("SELECT id, content FROM agent_diary WHERE user_id = {OWNER}"),
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| {
            (
                row.try_get("", "id").unwrap(),
                row.try_get("", "content").unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

/// The newest row of the diary entry `id` in her log here: whose, and what
/// its body says.
async fn newest(db: &DatabaseConnection, id: &str) -> (String, Value) {
    let object = format!("state:diary:{id}");
    let rows = store::rows(db, OWNER, PERSONAL).await.unwrap();
    let row = rows
        .iter()
        .filter(|row| row.object == object)
        .max_by(|a, b| a.hlc.cmp(&b.hlc))
        .unwrap();
    let bodies = store::bodies(db, &root(), OWNER, PERSONAL, &[row])
        .await
        .unwrap();
    (row.origin.clone(), bodies[&row.id].clone())
}

/// A temporary host that had a deputy, an end that never held her at that
/// epoch, one at an epoch no grant is of and one writing alone each write a
/// diary entry gone: the site keeps all four, its log has each live again by
/// a newer row of the site's, which the end that wrote it gone takes back,
/// and nothing is written twice.
#[tokio::test]
async fn what_an_end_not_holding_her_writes_gone_stays_and_is_live_again() {
    let ids = ["d1", "d2", "d3", "d4"];
    let Some(schema) = with_diary("her_log_gone_kept", &ids).await else {
        return;
    };
    let db = &schema.db;
    let standing = lease::current(db).await.unwrap();
    deputy::issue(db, "dev_mac", standing.epoch, 1)
        .await
        .unwrap();
    let before = entries(db).await;
    let ends = [
        ("dev_mac", "d1", standing.epoch),
        ("dev_win", "d2", standing.epoch),
        ("dev_odd", "d3", standing.epoch + 40),
        ("dev_old", "d4", 0),
    ];
    for (index, (device, id, epoch)) in ends.into_iter().enumerate() {
        let mut side = Side::new(device);
        let now = crate::services::her_log::now() + index as i64 * SECOND;
        sync(db, &mut side, PERSONAL, now).await;
        side.epoch = Some(epoch);
        writes_gone(&mut side, id, Author::Her, now + SECOND);
        sync(db, &mut side, PERSONAL, now + SECOND).await;

        assert_eq!(entries(db).await, before, "{device}");
        let (origin, body) = newest(db, id).await;
        assert_eq!(
            (origin.as_str(), &body["state"]),
            (NODE, &json!("live")),
            "{device}"
        );
        assert_eq!(body["body"]["content"], format!("{id} 那天"), "{device}");
        // The end that wrote it gone has it back.
        sync(db, &mut side, PERSONAL, now + SECOND).await;
        let object = format!("state:diary:{id}");
        let last = side
            .log(PERSONAL)
            .into_iter()
            .filter(|row| row.object == object)
            .max_by(|a, b| a.hlc.cmp(&b.hlc))
            .unwrap();
        assert_eq!(last.origin, NODE, "{device}");
    }
    let rows = store::rows(db, OWNER, PERSONAL).await.unwrap().len();
    assert_eq!(state::take_in(db, &root(), OWNER).await.unwrap(), 0);
    let after = state::sync(db, &root(), OWNER).await.unwrap();
    assert_eq!((after.written, after.gone), (0, 0));
    assert_eq!(store::rows(db, OWNER, PERSONAL).await.unwrap().len(), rows);
    schema.drop().await;
}

/// What a temporary host changed before it wrote the entry gone is taken,
/// the going is not: the entry stands as it was changed.
#[tokio::test]
async fn a_change_before_a_going_not_taken_stands() {
    let Some(schema) = with_diary("her_log_gone_changed", &["d1"]).await else {
        return;
    };
    let db = &schema.db;
    let standing = lease::current(db).await.unwrap();
    deputy::issue(db, "dev_mac", standing.epoch, 1)
        .await
        .unwrap();
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    sync(db, &mut mac, PERSONAL, now).await;
    mac.epoch = Some(standing.epoch);
    mac.write_body(
        PERSONAL,
        "state:diary:d1",
        "state.write",
        Author::Her,
        Some(diary("d1", "live", "在外面改的", now + SECOND)),
        now + SECOND,
    );
    writes_gone(&mut mac, "d1", Author::Her, now + 2 * SECOND);
    sync(db, &mut mac, PERSONAL, now + 2 * SECOND).await;

    let changed = vec![("d1".to_string(), "在外面改的".to_string())];
    assert_eq!(entries(db).await, changed);
    let (origin, body) = newest(db, "d1").await;
    assert_eq!((origin.as_str(), &body["state"]), (NODE, &json!("live")));
    assert_eq!(body["body"]["content"], "在外面改的");
    // What the site has of it since is not written over by that change again.
    db.execute_unprepared("UPDATE agent_diary SET content = '站点又改的' WHERE id = 'd1'")
        .await
        .unwrap();
    assert_eq!(state::take_in(db, &root(), OWNER).await.unwrap(), 0);
    assert_eq!(entries(db).await[0].1, "站点又改的");
    schema.drop().await;
}

/// The end holding her writes one gone: it goes here. One it writes gone
/// after it handed her back does not.
#[tokio::test]
async fn what_the_end_holding_her_writes_gone_goes_until_it_hands_her_back() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = with_diary("her_log_gone_holder", &["d1", "d2"]).await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], now).await;
    let epoch = mac.epoch.expect("her mind is on the Mac");
    sync(db, &mut mac, PERSONAL, now).await;
    writes_gone(&mut mac, "d1", Author::Her, now + SECOND);
    sync(db, &mut mac, PERSONAL, now + SECOND).await;
    assert_eq!(
        entries(db).await,
        vec![("d2".to_string(), "d2 那天".to_string())]
    );

    let yielded = mac.frame(
        "lease.yield",
        Some("yield_back"),
        json!({ "epoch": epoch, "through": { PERSONAL: 1 } }),
    );
    exchange(db, &mut mac, vec![yielded], now + 2 * SECOND).await;
    assert!(lease::current(db).await.unwrap().held_by(NODE));
    mac.epoch = Some(epoch);
    writes_gone(&mut mac, "d2", Author::Her, now + 3 * SECOND);
    sync(db, &mut mac, PERSONAL, now + 3 * SECOND).await;
    assert_eq!(
        entries(db).await,
        vec![("d2".to_string(), "d2 那天".to_string())]
    );
    state::sync(db, &root(), OWNER).await.unwrap();
    let (origin, body) = newest(db, "d2").await;
    assert_eq!((origin.as_str(), &body["state"]), (NODE, &json!("live")));
    // The one that went stays gone.
    let (origin, body) = newest(db, "d1").await;
    assert_eq!(
        (origin.as_str(), &body["state"]),
        ("dev_mac", &json!("deleted"))
    );
    schema.drop().await;
}

/// While her mind is on another end the site writes none of her rows: what it
/// did not take as gone stays here, and is live again in her log once she is
/// back.
#[tokio::test]
async fn what_stayed_while_she_was_elsewhere_is_live_again_once_she_is_back() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = with_diary("her_log_gone_elsewhere", &["d1"]).await else {
        return;
    };
    let db = &schema.db;
    let (mut mac, mut win) = (Side::new("dev_mac"), Side::new("dev_win"));
    let now = crate::services::her_log::now();
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], now).await;
    let epoch = mac.epoch.expect("her mind is on the Mac");
    sync(db, &mut win, PERSONAL, now).await;
    win.epoch = Some(epoch);
    writes_gone(&mut win, "d1", Author::Her, now + SECOND);
    sync(db, &mut win, PERSONAL, now + SECOND).await;
    assert_eq!(entries(db).await.len(), 1);
    assert_eq!(
        newest(db, "d1").await.0,
        "dev_win",
        "the site writes none of her rows now"
    );
    assert_eq!(state::take_in(db, &root(), OWNER).await.unwrap(), 0);

    let yielded = mac.frame(
        "lease.yield",
        Some("yield_back"),
        json!({ "epoch": epoch, "through": {} }),
    );
    exchange(db, &mut mac, vec![yielded], now + 2 * SECOND).await;
    state::sync(db, &root(), OWNER).await.unwrap();
    let (origin, body) = newest(db, "d1").await;
    assert_eq!((origin.as_str(), &body["state"]), (NODE, &json!("live")));
    assert_eq!(entries(db).await.len(), 1);
    schema.drop().await;
}

/// The owner's word that a diary entry is gone is not taken from another end
/// (there she forgets it: a forget); of her wardrobe it is.
#[tokio::test]
async fn the_owner_says_only_what_the_owner_changes_is_gone() {
    let Some(schema) = with_diary("her_log_gone_owner", &["d1"]).await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    sync(db, &mut mac, PERSONAL, now).await;
    let row = writes_gone(&mut mac, "d1", Author::Owner, now + SECOND);
    sync(db, &mut mac, PERSONAL, now + SECOND).await;
    assert_eq!(entries(db).await.len(), 1);
    let (origin, body) = newest(db, "d1").await;
    assert_eq!((origin.as_str(), &body["state"]), (NODE, &json!("live")));

    let holders = gone::holders(db, &[&row]).await.unwrap();
    for kind in state::KINDS {
        let theirs = matches!(kind, Kind::Persona | Kind::Outfit);
        assert_eq!(
            holders.refuses(&row, kind),
            (!theirs).then_some(Refused::NotTheirs),
            "{kind:?}"
        );
    }
    schema.drop().await;
}

/// What a temporary host changed and then wrote gone, of an entry the site
/// itself let go meanwhile and of one it never had: neither comes back here,
/// nor is written again.
#[tokio::test]
async fn what_the_site_does_not_have_stays_gone() {
    let Some(schema) = with_diary("her_log_gone_absent", &["d1"]).await else {
        return;
    };
    let db = &schema.db;
    let standing = lease::current(db).await.unwrap();
    deputy::issue(db, "dev_mac", standing.epoch, 1)
        .await
        .unwrap();
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    sync(db, &mut mac, PERSONAL, now).await;
    mac.epoch = Some(standing.epoch);
    db.execute_unprepared("DELETE FROM agent_diary WHERE id = 'd1'")
        .await
        .unwrap();
    assert_eq!(state::sync(db, &root(), OWNER).await.unwrap().gone, 1);
    let now = crate::services::her_log::now();
    let mut went = Vec::new();
    for id in ["d1", "d7"] {
        mac.write_body(
            PERSONAL,
            &format!("state:diary:{id}"),
            "state.write",
            Author::Her,
            Some(diary(id, "live", "在外面改的", now + SECOND)),
            now + SECOND,
        );
        went.push(writes_gone(&mut mac, id, Author::Her, now + 2 * SECOND));
    }
    sync(db, &mut mac, PERSONAL, now + 2 * SECOND).await;

    assert!(entries(db).await.is_empty());
    // Settled: their bodies are not opened again each time.
    let took = state::took(db, OWNER).await.unwrap();
    for row in &went {
        assert_eq!(took.get(&row.object), Some(&row.id));
    }
    let rows = store::rows(db, OWNER, PERSONAL).await.unwrap().len();
    assert_eq!(state::take_in(db, &root(), OWNER).await.unwrap(), 0);
    let after = state::sync(db, &root(), OWNER).await.unwrap();
    assert_eq!((after.written, after.gone), (0, 0));
    assert_eq!(store::rows(db, OWNER, PERSONAL).await.unwrap().len(), rows);
    for id in ["d1", "d7"] {
        let (origin, body) = newest(db, id).await;
        assert_eq!(
            (origin.as_str(), &body["state"]),
            ("dev_mac", &json!("deleted")),
            "{id}"
        );
    }
    schema.drop().await;
}

/// She moved on from the Mac to the site as `how` says, the Mac not having
/// handed her over: what it wrote gone before then goes, however late it
/// comes; what it writes gone at its old epoch after that does not (`why`).
async fn moved_on(how: &str, why: Refused) {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = with_diary(&format!("her_log_gone_{how}"), &["d1", "d2"]).await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], now).await;
    let epoch = mac.epoch.expect("her mind is on the Mac");
    sync(db, &mut mac, PERSONAL, now).await;
    let before = writes_gone(&mut mac, "d1", Author::Her, now);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let issued = store::next_rev(db).await.unwrap();
    assert!(before.hlc < issued);
    db.execute_unprepared(&format!(
        "INSERT INTO agent_lease_grants (epoch, holder, issued, how, from_holder, created_at) \
         VALUES ({}, '{NODE}', '{issued}', '{how}', 'dev_mac', now())",
        epoch + 1
    ))
    .await
    .unwrap();
    let now = crate::services::her_log::now();
    let after = writes_gone(&mut mac, "d2", Author::Her, now + SECOND);
    assert!(after.hlc > issued && after.epoch == Some(epoch));
    sync(db, &mut mac, PERSONAL, now + SECOND).await;

    let holders = gone::holders(db, &[&before, &after]).await.unwrap();
    assert_eq!(holders.refuses(&before, Kind::Diary), None, "{how}");
    assert_eq!(holders.refuses(&after, Kind::Diary), Some(why), "{how}");
    assert_eq!(
        entries(db).await,
        vec![("d2".to_string(), "d2 那天".to_string())],
        "{how}"
    );
    state::sync(db, &root(), OWNER).await.unwrap();
    let (origin, body) = newest(db, "d2").await;
    assert_eq!((origin.as_str(), &body["state"]), (NODE, &json!("live")));
    schema.drop().await;
}

#[tokio::test]
async fn what_the_end_she_was_taken_over_from_writes_gone_after_does_not_go() {
    moved_on("takeover", Refused::NotHolder).await;
}

#[tokio::test]
async fn what_the_end_she_was_brought_back_from_writes_gone_after_does_not_go() {
    moved_on("force", Refused::Concurrent).await;
}

/// What the Mac counted when it handed her back goes though its clock was
/// ahead and it comes after.
#[tokio::test]
async fn what_the_end_counted_as_it_handed_her_back_goes_however_late() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = with_diary("her_log_gone_through", &["d1"]).await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], now).await;
    let epoch = mac.epoch.expect("her mind is on the Mac");
    sync(db, &mut mac, PERSONAL, now).await;
    let row = writes_gone(&mut mac, "d1", Author::Her, now + 30 * SECOND);
    let yielded = mac.frame(
        "lease.yield",
        Some("yield_back"),
        json!({ "epoch": epoch, "through": { PERSONAL: 1 } }),
    );
    exchange(db, &mut mac, vec![yielded], now).await;
    let standing = lease::current(db).await.unwrap();
    assert!(standing.held_by(NODE) && row.hlc > standing.issued);
    sync(db, &mut mac, PERSONAL, now + SECOND).await;
    assert!(entries(db).await.is_empty());
    schema.drop().await;
}

/// One gone by a row to take and again by one not to, come together, went:
/// it is not told as kept nor written live; the one beside it, only gone by a
/// row not to take, is.
#[tokio::test]
async fn one_gone_by_a_row_to_take_before_one_not_to_is_not_kept() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = with_diary("her_log_gone_twice", &["d1", "d2", "d3"]).await else {
        return;
    };
    let db = &schema.db;
    let now = crate::services::her_log::now();
    // The line this reads for is logged once before it listens: a line first
    // logged on another test's thread is not heard on this one's.
    let mut win = Side::new("dev_win");
    sync(db, &mut win, PERSONAL, now).await;
    win.epoch = Some(lease::current(db).await.unwrap().epoch);
    writes_gone(&mut win, "d3", Author::Her, now);
    sync(db, &mut win, PERSONAL, now).await;

    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], now).await;
    let epoch = mac.epoch.expect("her mind is on the Mac");
    sync(db, &mut mac, PERSONAL, now).await;
    writes_gone(&mut mac, "d1", Author::Her, now);
    let yielded = mac.frame(
        "lease.yield",
        Some("yield_back"),
        json!({ "epoch": epoch, "through": { PERSONAL: 1 } }),
    );
    exchange(db, &mut mac, vec![yielded], now).await;
    assert!(lease::current(db).await.unwrap().held_by(NODE));
    assert_eq!(entries(db).await.len(), 3, "not sent yet");
    mac.epoch = Some(epoch);
    writes_gone(&mut mac, "d1", Author::Her, now + SECOND);
    writes_gone(&mut mac, "d2", Author::Her, now + SECOND);
    let logged = Logged::start();
    sync(db, &mut mac, PERSONAL, now + SECOND).await;

    let kept = logged.lines("were kept");
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(
        kept[0].contains("records=1") && kept[0].contains("kinds=diary=1"),
        "{kept:?}"
    );
    drop(logged);
    let left = ["d2", "d3"].map(|id| (id.to_string(), format!("{id} 那天")));
    assert_eq!(entries(db).await, left);
    // Once more, and nothing of it comes back.
    state::take_in(db, &root(), OWNER).await.unwrap();
    state::sync(db, &root(), OWNER).await.unwrap();
    assert_eq!(entries(db).await, left);
    let (origin, body) = newest(db, "d1").await;
    assert_eq!(
        (origin.as_str(), &body["state"]),
        ("dev_mac", &json!("deleted"))
    );
    let (origin, body) = newest(db, "d2").await;
    assert_eq!((origin.as_str(), &body["state"]), (NODE, &json!("live")));
    schema.drop().await;
}

/// A row written alone (epoch 0) is no row of an epoch the end held, though
/// the end holds her as it comes.
#[tokio::test]
async fn what_the_end_holding_her_wrote_gone_alone_does_not_go() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = with_diary("her_log_gone_alone", &["d1"]).await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], now).await;
    assert!(mac.epoch.is_some(), "her mind is on the Mac");
    sync(db, &mut mac, PERSONAL, now).await;
    mac.epoch = Some(0);
    let row = writes_gone(&mut mac, "d1", Author::Her, now + SECOND);
    let grants = lease::history(db).await.unwrap();
    assert_eq!(
        myriad_persona_log::lease::standing(&row, &grants, &[]),
        myriad_persona_log::lease::Stand::Normal
    );
    let holders = gone::holders(db, &[&row]).await.unwrap();
    assert_eq!(holders.refuses(&row, Kind::Diary), Some(Refused::NotHolder));
    sync(db, &mut mac, PERSONAL, now + SECOND).await;
    assert_eq!(entries(db).await.len(), 1);
    schema.drop().await;
}

/// An outfit the owner took away on another end goes from her wardrobe here,
/// the rest of it stays, and nothing is written of it again.
#[tokio::test]
async fn an_outfit_the_owner_took_away_on_another_end_goes() {
    let Some(schema) = schema("her_log_gone_outfit").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared(
        r#"INSERT INTO agent_persona
             (id, name, personality, zone, field_revs, persona_json, visual_profile, updated_at)
           VALUES ('site', 'Merope', '安静', 'Asia/Tokyo', '{}', '{}'::jsonb,
             '{"activeOutfitId":"default","wardrobe":[
               {"id":"default","clothingStyle":"everyday","outfit":{"top":"白衬衫"}},
               {"id":"coat","name":"大衣","clothingStyle":"everyday","outfit":{"top":"大衣"}}]}'::jsonb,
             now())"#,
    )
    .await
    .unwrap();
    state::sync(db, &root(), OWNER).await.unwrap();
    let mut mac = Side::new("dev_mac");
    let now = crate::services::her_log::now();
    sync(db, &mut mac, Owner::OWN, now).await;
    let object = "state:outfit:coat";
    assert!(mac.log(Owner::OWN).iter().any(|row| row.object == object));
    mac.write_body(
        Owner::OWN,
        object,
        "state.write",
        Author::Owner,
        Some(json!({
            "at": now / 1000,
            "format": myriad_persona_sync::FORMAT_VERSION,
            "kind": "outfit",
            "id": "coat",
            "state": "deleted",
            "body": null,
        })),
        now + SECOND,
    );
    sync(db, &mut mac, Owner::OWN, now + SECOND).await;

    let left: String = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT string_agg(item->>'id', ' ') AS left FROM agent_persona, \
             jsonb_array_elements(visual_profile->'wardrobe') AS item",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "left")
        .unwrap();
    assert_eq!(left, "default");
    let of_it = |rows: Vec<Row>| rows.iter().filter(|row| row.object == object).count();
    let rows = of_it(store::rows(db, OWNER, Owner::OWN).await.unwrap());
    state::sync(db, &root(), OWNER).await.unwrap();
    assert_eq!(
        of_it(store::rows(db, OWNER, Owner::OWN).await.unwrap()),
        rows
    );
    schema.drop().await;
}

#[test]
fn what_went_is_counted_by_kind_without_an_id() {
    let objects = [
        "state:diary:d1",
        "state:affect_event:x",
        "state:diary:d2",
        "persona:name",
    ];
    assert_eq!(
        gone::by_kind(objects.into_iter()),
        "affect_event=1 diary=2 name=1"
    );
}
