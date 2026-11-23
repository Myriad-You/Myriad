//! The lines of a stretch gone over, of someone who asked her not to keep
//! something: a job to judge them, written with what the stretch kept.
use super::*;
use crate::services::agent::memory::unified;
use crate::services::agent::merope::asked::tests::asks;
use crate::services::agent::merope::call;

fn stretch(at: chrono::DateTime<chrono::FixedOffset>) -> Payload {
    Payload::Settle {
        present: Audience::private(7),
        from: at,
        last: at + chrono::Duration::minutes(1),
        heard: vec![(at, true)],
    }
}

/// The jobs to judge a stretch, as they wait.
async fn to_judge(db: &DatabaseConnection) -> Vec<(String, Job)> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT record_id, payload FROM runtime_registry WHERE namespace = $1 \
         AND payload->'data'->'NotKept' IS NOT NULL",
        [ASKED_NAMESPACE.into()],
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        (
            row.try_get("", "record_id").unwrap(),
            serde_json::from_value(row.try_get("", "payload").unwrap()).unwrap(),
        )
    })
    .collect()
}

#[tokio::test]
async fn a_stretch_of_someone_who_asked_is_judged_next_until_it_is_done() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(40);
    // Who asked nothing: nothing to judge.
    enqueue(db, "settle", 7, stretch(began), None)
        .await
        .unwrap();
    due(db, "settle").await;
    let claimed = claim(db).await.unwrap().expect("due");
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert!(to_judge(db).await.is_empty());
    asks(db, 7, "不要记和房东吵架的事").await;
    let later = began + chrono::Duration::minutes(20);
    enqueue(db, "settle", 7, stretch(later), None)
        .await
        .unwrap();
    due(db, "settle").await;
    let claimed = claim(db).await.unwrap().expect("due");
    // Not while it is tried again.
    finish(db, &claimed, Err(Failure::Model(call::Failure::Timeout)))
        .await
        .unwrap();
    assert!(to_judge(db).await.is_empty());
    due(db, "settle").await;
    let claimed = claim(db).await.unwrap().expect("due again");
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    let waiting = to_judge(db).await;
    assert_eq!(waiting.len(), 1);
    let (id, job) = &waiting[0];
    assert!(
        matches!(&job.data, Some(Payload::NotKept { from, .. }) if *from == later),
        "{job:?}"
    );
    // Failed, however often and for whatever reason, or with the process
    // gone while it was held: judged again, and kept until it is.
    for tried in 1..=6 {
        due(db, id).await;
        let claimed = claim(db).await.unwrap().expect("still to be judged");
        assert_eq!(&claimed.id, id);
        match tried {
            2 => {}
            3 => finish(db, &claimed, Err(Failure::Model(call::Failure::Quota)))
                .await
                .unwrap(),
            _ => finish(db, &claimed, Err(Failure::Storage)).await.unwrap(),
        }
        assert_eq!(to_judge(db).await.len(), 1, "after try {tried}");
    }
    // Her persona cleared, or the login changed: what they asked still holds.
    forget(db).await.unwrap();
    db.execute_unprepared("UPDATE users SET token_version = token_version + 1 WHERE id = 7")
        .await
        .unwrap();
    due(db, id).await;
    let claimed = claim(db).await.unwrap().expect("still to be judged");
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert!(to_judge(db).await.is_empty());
    schema.drop().await;
}

/// A stretch out of tries is left unread, and its lines judged all the same.
#[tokio::test]
async fn a_stretch_left_unread_is_judged_too() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    asks(db, 7, "不要记和房东吵架的事").await;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(40);
    enqueue(db, "settle", 7, stretch(began), None)
        .await
        .unwrap();
    // The process dies on each try.
    for _ in 0..MAX_ATTEMPTS {
        due(db, "settle").await;
        claim(db).await.unwrap().expect("due");
    }
    due(db, "settle").await;
    let next = claim(db).await.unwrap();
    let waiting = to_judge(db).await;
    assert_eq!(waiting.len(), 1);
    let (id, _) = &waiting[0];
    // At once, or as soon as it is due.
    let next = match next {
        Some(next) => next,
        None => {
            due(db, id).await;
            claim(db).await.unwrap().expect("due")
        }
    };
    assert_eq!(&next.id, id);
    assert!(
        matches!(&next.job.data, Some(Payload::NotKept { from, .. }) if *from == began),
        "{next:?}"
    );
    schema.drop().await;
}

/// The jobs holding what she held of them before to what they asked.
async fn old_to_judge(db: &DatabaseConnection) -> Vec<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT record_id FROM runtime_registry WHERE namespace = $1 \
         AND payload->'data'->'AskedOver' IS NOT NULL",
        [ASKED_NAMESPACE.into()],
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "record_id").unwrap())
    .collect()
}

/// A request written is held to what she held of them before, in the same
/// write, until that is done; anything else written is not.
#[tokio::test]
async fn a_request_is_held_to_what_she_held_of_them_before() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    chat(db, "plain", Audience::private(7)).await;
    due(db, "plain").await;
    let claimed = claim(db).await.unwrap().expect("due");
    finish(db, &claimed, Ok(fact())).await.unwrap();
    assert!(old_to_judge(db).await.is_empty());

    crate::services::agent::merope::chat_lines::tests::session(
        db,
        "his",
        7,
        Some(r#"{"mode":"chat","venue":"qq:42"}"#),
        false,
    )
    .await;
    for venue in ["group:qq:42", "group:qq:43"] {
        let of = crate::services::agent::merope::bits::SOURCE;
        unified::remember_in_venue(db, venue, "学猫叫", r#"{"handle":"猫"}"#, of)
            .await
            .unwrap()
            .unwrap();
    }
    chat(db, "request", Audience::private(7)).await;
    due(db, "request").await;
    let claimed = claim(db).await.unwrap().expect("due");
    let Effect::Chat(mut updates) = fact() else {
        unreachable!()
    };
    updates.updates[0].fact = Some("不要记和房东吵架的事".into());
    updates.asked = [0].into();
    finish(db, &claimed, Ok(Effect::Chat(updates)))
        .await
        .unwrap();
    // In private, and in the group they talk with her in; not in another she
    // holds something of.
    let waiting = old_to_judge(db).await;
    assert_eq!(waiting.len(), 2);
    let groups: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT payload->'data'->'AskedOver'->>'venue' AS venue FROM runtime_registry \
             WHERE namespace = $1 AND payload->'data'->'AskedOver'->>'venue' IS NOT NULL",
            [ASKED_NAMESPACE.into()],
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "venue").unwrap())
        .collect();
    assert_eq!(groups, ["qq:42"]);
    db.execute_unprepared(
        "DELETE FROM runtime_registry WHERE payload->'data'->'AskedOver'->>'venue' = 'qq:42'",
    )
    .await
    .unwrap();
    let waiting = old_to_judge(db).await;
    let id = &waiting[0];
    for tried in 1..=6 {
        due(db, id).await;
        let claimed = claim(db).await.unwrap().expect("still to be judged");
        assert_eq!((&claimed.id, claimed.job.owner), (id, 7));
        if tried != 2 {
            finish(db, &claimed, Err(Failure::Storage)).await.unwrap();
        }
        assert_eq!(old_to_judge(db).await.len(), 1, "after try {tried}");
    }
    forget(db).await.unwrap();
    due(db, id).await;
    let claimed = claim(db).await.unwrap().expect("still to be judged");
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert!(old_to_judge(db).await.is_empty());
    schema.drop().await;
}

/// What is judged by what they asked waits apart from the rest: what was
/// here before reads all of the rest and none of it. A row that cannot be
/// read (another build's) is passed over and counted, and the rest go on.
#[tokio::test]
async fn it_waits_apart_and_a_row_that_cannot_be_read_holds_up_nothing() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    asks(db, 7, "不要记和房东吵架的事").await;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(40);
    enqueue(db, "settle", 7, stretch(began), None)
        .await
        .unwrap();
    due(db, "settle").await;
    let claimed = claim(db).await.unwrap().expect("due");
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert_eq!(to_judge(db).await.len(), 1);
    let among_the_rest = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM runtime_registry WHERE namespace = $1 \
             AND payload->'data' <> 'null'::jsonb",
            [NAMESPACE.into()],
        ))
        .await
        .unwrap();
    assert!(among_the_rest.is_empty());

    // Another build's, the oldest of those waiting.
    chat(db, "odd", Audience::group("telegram:1", 7)).await;
    db.execute_unprepared(
        "UPDATE runtime_registry SET updated_at = to_timestamp(0), payload = \
           jsonb_set(jsonb_set(jsonb_set(payload, '{data}', '{\"Later\":{\"of\":1}}'), \
             '{ready}', '0'), '{lease}', '0') WHERE record_id = 'odd'",
    )
    .await
    .unwrap();
    chat(db, "after", Audience::group("telegram:2", 7)).await;
    due(db, "after").await;
    let before = passed_over();
    let mut claimed = Vec::new();
    while let Some(claim) = claim(db).await.unwrap() {
        claimed.push(claim);
    }
    let ids: Vec<&str> = claimed.iter().map(|claim| claim.id.as_str()).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert!(ids.contains(&"after") && !ids.contains(&"odd"));
    assert!(passed_over() > before);
    // Come across again: passed over as before.
    assert!(claim(db).await.unwrap().is_none());
    let odd = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT payload->'data'->'Later' IS NOT NULL AS there FROM runtime_registry \
             WHERE record_id = 'odd'",
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(odd.try_get::<bool>("", "there").unwrap(), "left as it is");
    schema.drop().await;
}

async fn number(db: &DatabaseConnection, id: &str, what: &str) -> i64 {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "SELECT (payload->>'{what}')::bigint AS kept FROM runtime_registry \
             WHERE record_id = '{id}'"
        ),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "kept")
    .unwrap()
}

/// The days run out while a line is gone over, as it is not to be given up
/// on: what they ask in it is not let go with it, the next time is to keep
/// that alone.
#[tokio::test]
async fn a_line_is_not_given_up_on_while_it_is_gone_over() {
    use super::super::super::super::memory_jobs::GIVES_UP_AFTER_SECS;
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    chat(db, "line", Audience::group("telegram:1", 7)).await;
    let now = crate::clock::now().timestamp();
    let since = async |since: i64| {
        db.execute_unprepared(&format!(
            "UPDATE runtime_registry SET payload = payload || \
               '{{\"untold_since\": {since}, \"untold_at\": {now}}}' \
             WHERE record_id = 'line'"
        ))
        .await
        .unwrap();
    };
    call::judged_last(now);
    due(db, "line").await;
    since(now - GIVES_UP_AFTER_SECS + 60).await;
    let claimed = claim(db).await.unwrap().expect("due");
    assert!(!claimed.gives_up);
    since(now - GIVES_UP_AFTER_SECS).await;
    let refused = || Err(Failure::NotTold(call::Failure::Refused));
    finish(db, &claimed, refused()).await.unwrap();
    assert!(loaded_data(db, "line").await.is_some());
    due(db, "line").await;
    let claimed = claim(db).await.unwrap().expect("still there");
    assert!(claimed.gives_up);
    finish(db, &claimed, refused()).await.unwrap();
    assert!(loaded_data(db, "line").await.is_none());
    schema.drop().await;
}

/// What she already held, refused the last time too, is asked about again
/// an hour on.
#[tokio::test]
async fn what_is_refused_again_waits_an_hour() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let data = Payload::AskedOver {
        venue: None,
        asked: crate::clock::now().fixed_offset(),
    };
    enqueue(db, "over", 7, data, None).await.unwrap();
    use call::Failure::{Refused, Rejected};
    for (failure, waits) in [
        (Refused, 5),
        (Refused, 3600),
        (Rejected, 30),
        (Refused, 600),
        (Refused, 3600),
    ] {
        due(db, "over").await;
        let claimed = claim(db).await.unwrap().expect("due");
        let now = crate::clock::now().timestamp();
        finish(db, &claimed, Err(Failure::Model(failure)))
            .await
            .unwrap();
        let ready: i64 = number(db, "over", "ready").await;
        assert!(
            (waits - 2..=waits + 2).contains(&(ready - now)),
            "{failure}"
        );
    }
    schema.drop().await;
}

/// A line whose "not to keep" is not told is not let go with its tries:
/// those are not spent on it, however it failed; only answers that were
/// none count toward giving it up, and refused for what it is, only how long
/// it has been.
#[tokio::test]
async fn a_line_not_told_of_is_tried_till_it_is() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    chat(db, "line", Audience::group("telegram:1", 7)).await;
    let failed = [
        call::Failure::Rejected,
        call::Failure::Quota,
        call::Failure::RateLimited,
        call::Failure::Timeout,
        call::Failure::InvalidOutput,
        call::Failure::Refused,
    ];
    for round in 0..3 {
        for (nth, failure) in failed.into_iter().enumerate() {
            due(db, "line").await;
            let claimed = claim(db).await.unwrap().expect("still to be told of");
            assert_eq!(claimed.job.untold, round + u8::from(nth > 4));
            assert_eq!(claimed.job.untold_since.is_some(), round > 0 || nth > 4);
            assert_eq!(claimed.job.attempts, 1, "its tries are not spent");
            finish(db, &claimed, Err(Failure::NotTold(failure)))
                .await
                .unwrap();
        }
    }
    // What comes meanwhile does not shorten the wait.
    let ready = async || {
        registry::get::<Job>(db, NAMESPACE, "line")
            .await
            .unwrap()
            .unwrap()
            .ready
    };
    due(db, "line").await;
    let claimed = claim(db).await.unwrap().expect("still to be told of");
    finish(db, &claimed, Err(Failure::NotTold(call::Failure::Rejected)))
        .await
        .unwrap();
    let waits = ready().await;
    chat(db, "line", Audience::group("telegram:1", 7)).await;
    assert_eq!(ready().await, waits);
    // Told, it is written as ever.
    due(db, "line").await;
    let claimed = claim(db).await.unwrap().expect("still to be told of");
    finish(db, &claimed, Ok(fact())).await.unwrap();
    assert!(loaded_data(db, "line").await.is_none());
    // Not told for as long as it is tried, or refused for days, while other
    // judgments were answered: let go. Not asked at all (the account
    // refused, no answer in time), never; nor with no other answered since
    // (it may be who asks), nor after a day with no such try.
    use super::super::super::super::memory_jobs::{GIVES_UP_AFTER, GIVES_UP_AFTER_SECS};
    use call::Failure::{InvalidOutput, Refused, Rejected, Timeout};
    let now = crate::clock::now().timestamp();
    let days_ago = now - GIVES_UP_AFTER_SECS;
    for (nth, (untold, since, last, answered, failure, goes)) in [
        (GIVES_UP_AFTER, now - 600, now, now, InvalidOutput, true),
        (
            GIVES_UP_AFTER,
            now - 600,
            now,
            now - 900,
            InvalidOutput,
            false,
        ),
        (GIVES_UP_AFTER, now - 600, now, now, Timeout, false),
        (0, days_ago, now, now, Refused, true),
        (0, days_ago, now, days_ago + 1, Refused, true),
        (0, days_ago, now, days_ago - 1, Refused, false),
        (0, days_ago, now, 0, Refused, false),
        (0, days_ago + 60, now, now, Refused, false),
        (0, days_ago, now - 86_460, now, Refused, false),
        (0, days_ago, now, now, Rejected, false),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("another{nth}");
        chat(db, &id, Audience::group(format!("telegram:2{nth}"), 7)).await;
        db.execute_unprepared(&format!(
            "UPDATE runtime_registry SET payload = payload || \
               '{{\"untold\": {untold}, \"untold_since\": {since}, \"untold_at\": {last}}}' \
             WHERE record_id = '{id}'"
        ))
        .await
        .unwrap();
        due(db, &id).await;
        call::judged_last(answered);
        let claimed = claim(db).await.unwrap().expect("due");
        // As it was taken up, whatever comes while it is gone over.
        call::judged_last(now + 1);
        finish(db, &claimed, Err(Failure::NotTold(failure)))
            .await
            .unwrap();
        assert_eq!(
            loaded_data(db, &id).await.is_none(),
            goes,
            "{nth} {failure}"
        );
        if !goes && last < now - 86_400 {
            let began: i64 = number(db, &id, "untold_since").await;
            assert!(began >= now - 1, "asked of anew");
        }
        db.execute_unprepared(&format!(
            "DELETE FROM runtime_registry WHERE record_id = '{id}'"
        ))
        .await
        .unwrap();
    }
    schema.drop().await;
}

/// Not told twice running, it is asked again only once an hour: what it
/// gives is asked for again each time.
#[tokio::test]
async fn a_line_not_told_of_twice_waits_longer() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    chat(db, "line", Audience::group("telegram:1", 7)).await;
    let mut waited = Vec::new();
    for _ in 0..3 {
        due(db, "line").await;
        let claimed = claim(db).await.unwrap().expect("still to be told of");
        finish(db, &claimed, Err(Failure::NotTold(call::Failure::Rejected)))
            .await
            .unwrap();
        let job = registry::get::<Job>(db, NAMESPACE, "line").await.unwrap();
        waited.push(job.unwrap().ready - crate::clock::now().timestamp());
    }
    assert!((590..=600).contains(&waited[0]), "{waited:?}");
    assert!(
        (waited[1..].iter()).all(|wait| (3_590..=3_600).contains(wait)),
        "{waited:?}"
    );
    schema.drop().await;
}

/// Lines joined while they wait do not grow without end: the oldest go.
#[test]
fn lines_joined_while_they_wait_do_not_grow_without_end() {
    let at = crate::clock::now().fixed_offset();
    let said = |nth: i64| Payload::Chat {
        user_text: format!("{nth}{}", "长".repeat(999)),
        reply: String::new(),
        input_at: at + chrono::Duration::seconds(nth),
        present: Audience::private(7),
        turn: TurnContext::default(),
        lines: vec![at + chrono::Duration::seconds(nth)],
        for_good: false,
    };
    let before = super::super::super::super::memory_jobs::folded_away();
    let mut first = said(100);
    for nth in 101..140 {
        first.fold(said(nth));
    }
    let Payload::Chat { user_text, .. } = first else {
        unreachable!()
    };
    assert!(user_text.chars().count() <= 12_000);
    assert!(user_text.contains("139长") && !user_text.contains("100长"));
    assert!(super::super::super::super::memory_jobs::folded_away() >= before + 28);
}

/// One job of each kind judged by what they asked, by name.
async fn one_of_each(db: &DatabaseConnection) -> chrono::DateTime<chrono::FixedOffset> {
    let at = crate::clock::now().fixed_offset() - chrono::Duration::minutes(40);
    let present = Audience::private(7);
    let last = at + chrono::Duration::minutes(1);
    for (id, data) in [
        (
            "not_kept",
            Payload::NotKept {
                present: present.clone(),
                from: at,
                last,
            },
        ),
        (
            "asked_over",
            Payload::AskedOver {
                asked: at,
                venue: None,
            },
        ),
        (
            "adopt",
            Payload::Adopt {
                marker: "onebot:111".into(),
            },
        ),
        (
            "left_unread",
            Payload::LeftUnread {
                present,
                last,
                since: at,
                until: last,
            },
        ),
        (
            "group_heard",
            Payload::GroupHeard {
                venue: "qq:42".into(),
                stretch: vec![(at, "某人".into(), "房东又来吵".into(), false)],
                speakers: vec![("onebot:111".into(), "某人".into())],
                read: crate::services::agent::merope::bits::Read {
                    raw: "{}".into(),
                    at,
                },
            },
        ),
    ] {
        enqueue(db, id, 7, data, None).await.unwrap();
    }
    at
}

async fn waits(db: &DatabaseConnection) -> Vec<String> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT record_id FROM runtime_registry WHERE namespace = $1 ORDER BY record_id",
            [ASKED_NAMESPACE.into()],
        ))
        .await
        .unwrap();
    (rows.iter().map(|row| row.try_get("", "record_id").unwrap())).collect()
}

/// What only holds to what they asked goes on whoever she is by then; what
/// writes something new goes with the her it was for, and with the login.
#[tokio::test]
async fn what_would_write_anew_goes_with_the_her_it_was_for() {
    let holds = ["adopt", "asked_over", "not_kept"];
    // Cleared; another her since; the login changed.
    for (nth, change) in [
        None,
        Some(
            "UPDATE runtime_registry SET payload = jsonb_set(payload, '{her}', \
               '{\"lineage\":\"another\"}')",
        ),
        Some("UPDATE users SET token_version = token_version + 1 WHERE id = 7"),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(schema) = fixture().await else {
            return;
        };
        let db = &schema.db;
        one_of_each(db).await;
        assert_eq!(waits(db).await.len(), 5);
        match change {
            None => forget(db).await.unwrap(),
            Some(sql) => {
                db.execute_unprepared(sql).await.unwrap();
            }
        }
        // Taken up, and failed: only what is stale is let go.
        for id in waits(db).await {
            due(db, &id).await;
        }
        while let Some(claimed) = claim(db).await.unwrap() {
            finish(db, &claimed, Err(Failure::Storage)).await.unwrap();
        }
        assert_eq!(waits(db).await, holds, "change {nth}");
        schema.drop().await;
    }
}

/// A group's stretch put by is kept as long as its words are at hand, and
/// goes with any of them forgotten, whoever it is kept under.
#[tokio::test]
async fn a_groups_stretch_goes_with_its_words() {
    use crate::services::agent::memory::origins;
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let at = one_of_each(db).await;
    let expires: i64 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT expires_at FROM runtime_registry WHERE record_id = 'group_heard'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "expires_at")
        .unwrap();
    assert_eq!(expires, (at + chrono::Duration::hours(6)).timestamp());
    for (said, left) in [("周末去爬山", 5), ("房东又来吵", 4)] {
        let quote = origins::quotes(Some(said)).remove(0);
        forget_holding(db, 8, &quote).await.unwrap();
        assert_eq!(waits(db).await.len(), left, "{said}");
    }
    assert!(!waits(db).await.contains(&"group_heard".to_string()));
    schema.drop().await;
}

async fn loaded_data(db: &DatabaseConnection, id: &str) -> Option<Payload> {
    registry::get::<Job>(db, NAMESPACE, id)
        .await
        .unwrap()
        .and_then(|job| job.data)
}

/// However much waits, what they asked is still held to.
#[tokio::test]
async fn a_full_queue_still_takes_what_is_to_be_judged() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    asks(db, 7, "不要记和房东吵架的事").await;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(40);
    enqueue(db, "settle", 7, stretch(began), None)
        .await
        .unwrap();
    due(db, "settle").await;
    let claimed = claim(db).await.unwrap().expect("due");
    db.execute_unprepared(&format!(
        "INSERT INTO runtime_registry \
           (namespace, record_id, subject_id, owner_id, payload, expires_at, updated_at) \
         SELECT namespace, 'full-' || n, subject_id, owner_id, \
           jsonb_set(payload, '{{ready}}', '9999999999'), expires_at, updated_at \
         FROM runtime_registry, generate_series(1, {CAPACITY}) n WHERE record_id = 'settle'",
    ))
    .await
    .unwrap();
    assert!(
        enqueue(db, "another", 7, stretch(began), None)
            .await
            .is_err(),
        "full"
    );
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert_eq!(to_judge(db).await.len(), 1);
    schema.drop().await;
}
