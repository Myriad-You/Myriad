//! A stretch whose answer could not be read: tried again like any failure,
//! and when it still cannot be read, its notes stay notes and go with the
//! next stretch in the same place.

use super::*;

fn from_of(job: &Job) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    match &job.data {
        Some(Payload::Settle { from, .. }) => Some(*from),
        _ => None,
    }
}

/// Unreadable once, read the next time: what it keeps is kept.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn an_unreadable_stretch_is_tried_again() {
    let schema = fixture().await;
    let db = &schema.db;
    note(db, "周末要搬家", unified::SHORT_NOTE_SOURCE).await;
    let before = settling::failures();
    let first = stretch(db, "settle").await;
    finish(
        db,
        &first,
        Err(Failure::Model(call::Failure::InvalidOutput)),
    )
    .await
    .unwrap();
    assert!(loaded(db, "settle").await.data.is_some(), "tried again");
    assert_eq!(went(db).await["周末要搬家"], None, "still a note meanwhile");
    due(db, "settle").await;
    let again = claim(db).await.unwrap().expect("tried again");
    let moving = vec![vec![id_of(db, "周末要搬家").await]];
    finish(
        db,
        &again,
        Ok(settled(vec![lasting("周末要搬家", &[])], moving)),
    )
    .await
    .unwrap();
    assert_eq!(went(db).await["周末要搬家"], holds("周末要搬家"));
    let after = settling::failures();
    assert!(after.unreadable > before.unreadable);
    assert!(after.asked_again > before.asked_again);
    schema.drop().await;
}

/// Unreadable on every try: its notes stay notes, and the next stretch is
/// gone over from where it began, so they are kept for good then.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn notes_of_a_stretch_never_read_go_with_the_next() {
    let schema = fixture().await;
    let db = &schema.db;
    note(db, "周末要搬家", unified::SHORT_NOTE_SOURCE).await;
    let before = settling::failures();
    let began = crate::clock::now().fixed_offset() - chrono::Duration::hours(2);
    let mut claimed = stretch_from(db, "settle", began).await;
    for attempt in 1..=MAX_ATTEMPTS {
        finish(
            db,
            &claimed,
            Err(Failure::Model(call::Failure::InvalidOutput)),
        )
        .await
        .unwrap();
        if attempt < MAX_ATTEMPTS {
            due(db, "settle").await;
            claimed = claim(db).await.unwrap().expect("tried again");
        }
    }
    let job = loaded(db, "settle").await;
    assert!(job.data.is_none(), "not tried a fourth time");
    assert_eq!(job.unread_from, Some(began), "where it began is kept");
    assert_eq!(went(db).await["周末要搬家"], None, "still a note");

    // They talk again later: while lines still come, that stretch waits
    // for them to go quiet as any does; taken up, it begins where the
    // unread one did.
    let later = crate::clock::now().fixed_offset() - chrono::Duration::minutes(1);
    let line = |at| Payload::Settle {
        present: Audience::private(7),
        from: at,
        last: at,
    };
    for _ in 0..2 {
        enqueue(db, "settle", 7, line(later), None).await.unwrap();
    }
    let waiting = loaded(db, "settle").await;
    assert_eq!(from_of(&waiting), Some(later));
    assert!(
        waiting.ready > crate::clock::now().timestamp() + 60,
        "not due at once for the unread stretch"
    );
    assert!(claim(db).await.unwrap().is_none(), "they are still talking");
    due(db, "settle").await;
    let next = claim(db).await.unwrap().expect("over");
    assert_eq!(from_of(&next.job), Some(began));
    assert_eq!(loaded(db, "settle").await.unread_from, None);
    let moving = vec![vec![id_of(db, "周末要搬家").await]];
    finish(
        db,
        &next,
        Ok(settled(vec![lasting("周末要搬家", &[])], moving)),
    )
    .await
    .unwrap();
    assert_eq!(went(db).await["周末要搬家"], holds("周末要搬家"));
    let after = settling::failures();
    assert!(after.unreadable >= before.unreadable + u64::from(MAX_ATTEMPTS));
    assert!(after.left_unread > before.left_unread);
    assert!(after.brought_along > before.brought_along);
    schema.drop().await;
}

/// A stretch whose process went away on every try is let go unread too.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn a_stretch_never_finished_is_left_unread() {
    let schema = fixture().await;
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::hours(2);
    stretch_from(db, "settle", began).await;
    for _ in 1..MAX_ATTEMPTS {
        due(db, "settle").await;
        claim(db).await.unwrap().expect("taken up again");
    }
    due(db, "settle").await;
    assert!(claim(db).await.unwrap().is_none(), "no try left");
    let job = loaded(db, "settle").await;
    assert!(job.data.is_none());
    assert_eq!(job.unread_from, Some(began));
    schema.drop().await;
}

/// Every try of the stretch `claimed` answered unreadably.
async fn never_read(db: &DatabaseConnection, mut claimed: Claim) {
    for attempt in 1..=MAX_ATTEMPTS {
        finish(
            db,
            &claimed,
            Err(Failure::Model(call::Failure::InvalidOutput)),
        )
        .await
        .unwrap();
        if attempt < MAX_ATTEMPTS {
            due(db, "settle").await;
            claimed = claim(db).await.unwrap().expect("tried again");
        }
    }
}

async fn noted_at(db: &DatabaseConnection, text: &str, at: chrono::DateTime<chrono::FixedOffset>) {
    note(db, text, unified::SHORT_NOTE_SOURCE).await;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1 WHERE content = $2",
        [at.into(), text.into()],
    ))
    .await
    .unwrap();
}

/// Never read, brought along by two stretches after it and still not read:
/// its notes are kept for good as they are, each pointing at what keeps it;
/// what came after is gone over on its own, from where it began.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn notes_never_read_are_kept_as_they_are_after_two_more_stretches() {
    let schema = fixture().await;
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    noted_at(db, "周末要搬家", ago(120)).await;
    noted_at(db, "在学吉他", ago(60)).await;
    noted_at(db, "明天要考试", ago(20)).await;
    let before = settling::failures();

    never_read(db, stretch_from(db, "settle", ago(121)).await).await;
    // Brought along once, then twice: still notes.
    for began in [ago(61), ago(30)] {
        let next = stretch_from(db, "settle", began).await;
        assert_eq!(from_of(&next.job), Some(ago(121)));
        never_read(db, next).await;
        if began == ago(61) {
            assert_eq!(went(db).await["周末要搬家"], None, "brought once: a note");
        }
    }
    let all = went(db).await;
    assert_eq!(all["周末要搬家"], holds("周末要搬家"), "kept as it was");
    assert_eq!(all["在学吉他"], holds("在学吉他"), "kept with it");
    assert_eq!(all["明天要考试"], None, "the last stretch's own waits");
    let job = loaded(db, "settle").await;
    assert_eq!(job.unread_from, Some(ago(30)), "brought no further back");
    assert_eq!(job.unread_times, 0);

    // The next stretch goes over only what came after.
    let next = stretch_from(db, "settle", ago(5)).await;
    assert_eq!(from_of(&next.job), Some(ago(30)));
    let exam = vec![vec![id_of(db, "明天要考试").await]];
    finish(
        db,
        &next,
        Ok(settled(vec![lasting("明天要考试", &[])], exam)),
    )
    .await
    .unwrap();
    assert_eq!(went(db).await["明天要考试"], holds("明天要考试"));
    let after = settling::failures();
    assert!(after.kept_unsettled >= before.kept_unsettled + 2);
    schema.drop().await;
}

/// The same when the last stretch to bring them along never finished: its
/// process went away on every try.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn notes_are_kept_as_they_are_when_the_last_stretch_never_finished() {
    let schema = fixture().await;
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    noted_at(db, "周末要搬家", ago(120)).await;
    never_read(db, stretch_from(db, "settle", ago(121)).await).await;
    never_read(db, stretch_from(db, "settle", ago(61)).await).await;
    stretch_from(db, "settle", ago(30)).await;
    for _ in 1..MAX_ATTEMPTS {
        due(db, "settle").await;
        claim(db).await.unwrap().expect("taken up again");
    }
    due(db, "settle").await;
    assert!(claim(db).await.unwrap().is_none(), "no try left");
    assert_eq!(went(db).await["周末要搬家"], holds("周末要搬家"));
    assert_eq!(loaded(db, "settle").await.unread_from, Some(ago(30)));
    schema.drop().await;
}
