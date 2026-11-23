//! A private talk's stretch: due once its box is full, and gone over now
//! when another part of her needs it.
use super::*;

fn line(present: Audience, at: chrono::DateTime<chrono::FixedOffset>, answered: bool) -> Payload {
    Payload::Settle {
        present,
        from: at,
        last: at,
        heard: vec![(at, answered)],
    }
}

/// Their lines and her answers, as the talk goes: `exchanges` lines, each
/// answered.
async fn talk(
    db: &DatabaseConnection,
    id: &str,
    present: Audience,
    began: chrono::DateTime<chrono::FixedOffset>,
    exchanges: i64,
) {
    for index in 0..exchanges {
        let at = began + chrono::Duration::seconds(index * 20);
        // As it lands, and again once she has answered.
        enqueue(db, id, 7, line(present.clone(), at, false), None)
            .await
            .unwrap();
        enqueue(db, id, 7, line(present.clone(), at, true), None)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_private_talk_is_gone_over_once_twelve_lines_wait() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(2);
    // Eleven: their sixth line landed, not yet answered.
    talk(db, "settle", Audience::private(7), began, 5).await;
    let sixth = began + chrono::Duration::seconds(100);
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), sixth, false),
        None,
    )
    .await
    .unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(job.data.as_ref().unwrap().waiting(), 11);
    assert!(
        job.ready > job.last_arrival,
        "eleven wait for them to go quiet"
    );
    // The same line held twice is not counted twice.
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), sixth, false),
        None,
    )
    .await
    .unwrap();
    assert_eq!(loaded(db, "settle").await.data.unwrap().waiting(), 11);
    // Her answer to it: twelve, gone over now.
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), sixth, true),
        None,
    )
    .await
    .unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(job.data.as_ref().unwrap().waiting(), 12);
    assert!(job.ready <= job.last_arrival, "twelve: at once");
    let claimed = claim(db).await.unwrap().expect("due");
    // What they say while it is gone over waits for the next, never lost.
    let later = sixth + chrono::Duration::seconds(20);
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), later, false),
        None,
    )
    .await
    .unwrap();
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    let job = loaded(db, "settle").await;
    assert!(matches!(job.data, Some(Payload::Settle { from, .. }) if from == later));
    assert_eq!(job.data.unwrap().waiting(), 1);
    schema.drop().await;
}

/// A group is gone over from its short notes, as before: a full box does
/// not hurry it.
#[tokio::test]
async fn a_group_waits_for_them_to_go_quiet_however_many_lines() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(2);
    talk(db, "group", Audience::group("g1", 7), began, 8).await;
    let job = loaded(db, "group").await;
    assert_eq!(job.data.as_ref().unwrap().waiting(), 16);
    assert!(job.ready > job.last_arrival);
    schema.drop().await;
}

#[tokio::test]
async fn another_part_of_her_hurries_a_talk_only_when_lines_of_theirs_wait() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    // Nothing waiting: nothing to hurry.
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), None);
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(1);
    talk(db, "settle", Audience::private(7), began, 1).await;
    assert!(loaded(db, "settle").await.ready > loaded(db, "settle").await.last_arrival);
    // Another's job is not hurried for this one.
    assert_eq!(hurry(db, "settle", 8).await.unwrap(), None);
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), Some(began));
    let job = loaded(db, "settle").await;
    assert!(job.ready <= job.last_arrival + 1, "due now");
    assert!(still_waiting(db, "settle", began).await.unwrap());
    // Taken up and gone over: no longer waiting; hurrying again finds
    // nothing.
    let claimed = claim(db).await.unwrap().expect("due");
    assert_eq!(
        hurry(db, "settle", 7).await.unwrap(),
        Some(began),
        "being gone over: waited on as it is"
    );
    assert!(still_waiting(db, "settle", began).await.unwrap());
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert!(!still_waiting(db, "settle", began).await.unwrap());
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), None);
    schema.drop().await;
}

/// A stretch that failed keeps its wait before the next try: hurrying it
/// does not try a connection that just dropped again at once.
#[tokio::test]
async fn a_failed_stretch_keeps_its_wait_when_hurried() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(1);
    talk(db, "settle", Audience::private(7), began, 1).await;
    due(db, "settle").await;
    let claimed = claim(db).await.unwrap().expect("due");
    finish(
        db,
        &claimed,
        Err(memory_jobs::Failure::Model(call::Failure::Timeout)),
    )
    .await
    .unwrap();
    let waits = loaded(db, "settle").await.ready;
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), Some(began));
    assert_eq!(loaded(db, "settle").await.ready, waits);
    schema.drop().await;
}
