//! A stretch with more short notes than are gone over at once is gone over
//! in passes, oldest first, each once the one before is written; notes it
//! brought along count among them, and are brought along by it once.

use super::unread::noted_at;
use super::*;

fn from_of(job: &Job) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    match &job.data {
        Some(Payload::Settle { from, .. }) => Some(*from),
        _ => None,
    }
}

/// What going over `claimed` takes up now: no lines were said, so it asks
/// nothing and only says where the rest begins.
async fn rest_of(
    db: &DatabaseConnection,
    claimed: &Claim,
) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let Some(Payload::Settle { present, from, .. }) = &claimed.job.data else {
        panic!("a stretch");
    };
    match crate::services::agent::merope::chat_remember::settle(
        db,
        claimed.job.owner,
        present,
        *from,
        claimed.job.said_from,
    )
    .await
    {
        Ok(Effect::Settle {
            updates,
            settled,
            rest,
        }) => {
            assert!(updates.updates.is_empty() && settled.is_empty());
            assert!(rest.is_some(), "nothing to go on with is no change");
            rest
        }
        Ok(Effect::NoChange) => None,
        _ => panic!("nothing to ask"),
    }
}

fn going_on(
    updates: Vec<ChatMemoryUpdate>,
    notes: Vec<Vec<String>>,
    rest: chrono::DateTime<chrono::FixedOffset>,
) -> Effect {
    let Effect::Settle {
        updates, settled, ..
    } = settled(updates, notes)
    else {
        unreachable!()
    };
    Effect::Settle {
        updates,
        settled,
        rest: Some(rest),
    }
}

#[tokio::test]
async fn a_long_stretch_is_gone_over_in_passes_each_after_the_last_is_written() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    for minutes in 39..=50 {
        noted_at(db, &format!("第{minutes}条"), ago(minutes)).await;
    }
    let first = stretch_from(db, "settle", ago(51)).await;
    let rest = rest_of(db, &first).await;
    assert_eq!(rest, Some(ago(41)), "the oldest nine now");
    let kept = vec![vec![id_of(db, "第50条").await, id_of(db, "第49条").await]];
    finish(
        db,
        &first,
        Ok(going_on(
            vec![lasting("前两条合在一起", &[])],
            kept,
            ago(41),
        )),
    )
    .await
    .unwrap();
    // Written before the next pass is taken up: it sees it among what she
    // knows.
    let all = went(db).await;
    assert_eq!(all["第50条"], holds("前两条合在一起"));
    assert_eq!(all["第41条"], None, "the rest are still notes");
    let job = loaded(db, "settle").await;
    assert_eq!(from_of(&job), Some(ago(41)));
    assert_eq!(
        job.said_from,
        Some(ago(51)),
        "its lines from where they began"
    );
    assert_eq!((job.attempts, job.lease, job.token.is_none()), (0, 0, true));
    assert_eq!(job.outcome.as_deref(), Some("applied"));

    let next = claim(db).await.unwrap().expect("due at once");
    assert_eq!(rest_of(db, &next).await, None, "the last three");
    let kept = vec![vec![id_of(db, "第39条").await]];
    finish(db, &next, Ok(settled(vec![lasting("第39条", &[])], kept)))
        .await
        .unwrap();
    let job = loaded(db, "settle").await;
    assert!(job.data.is_none(), "over");
    assert_eq!(job.said_from, None);
    schema.drop().await;
}

/// Talk that came while a pass was gone over goes with the rest, once
/// it is over.
#[tokio::test]
async fn a_pass_waits_for_talk_that_came_meanwhile() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    let first = stretch_from(db, "settle", ago(51)).await;
    let line = Payload::Settle {
        present: Audience::private(7),
        from: ago(0),
        last: ago(0),
        heard: Vec::new(),
    };
    enqueue(db, "settle", 7, line, None).await.unwrap();
    finish(db, &first, Ok(going_on(vec![], vec![], ago(41))))
        .await
        .unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(from_of(&job), Some(ago(41)));
    assert!(matches!(job.data, Some(Payload::Settle { last, .. }) if last == ago(0)));
    assert!(job.next.is_none());
    assert_eq!(job.outcome.as_deref(), Some("no_change"));
    assert!(claim(db).await.unwrap().is_none(), "they are still talking");
    schema.drop().await;
}

/// Notes brought along count among the nine. Those a pass could not take
/// go on with the stretch and are brought along by it once: its next pass
/// left unread, they wait as brought once, from where it stopped; brought
/// along by the stretch after it too and still unread, they are kept as
/// they are.
#[tokio::test]
async fn notes_brought_along_go_on_in_passes_brought_along_once() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    for minutes in 120..=130 {
        noted_at(db, &format!("旧{minutes}"), ago(minutes)).await;
    }
    noted_at(db, "新10", ago(10)).await;
    super::unread::never_read(db, stretch_from(db, "settle", ago(131)).await).await;
    let taken = stretch_from(db, "settle", ago(11)).await;
    assert_eq!(from_of(&taken.job), Some(ago(131)), "brought along");
    let rest = rest_of(db, &taken).await;
    assert_eq!(rest, Some(ago(121)), "nine of what it brought along now");
    finish(db, &taken, Ok(going_on(vec![], vec![], ago(121))))
        .await
        .unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(from_of(&job), Some(ago(121)));
    assert_eq!(job.unread_from, None);

    due(db, "settle").await;
    let next = claim(db).await.unwrap().expect("the rest");
    assert_eq!(
        from_of(&next.job),
        Some(ago(121)),
        "not brought along again"
    );
    super::unread::never_read(db, next).await;
    let job = loaded(db, "settle").await;
    assert!(job.data.is_none());
    assert_eq!(
        (job.unread_from, job.unread_times),
        (Some(ago(121)), 1),
        "what was gone over is not brought again; brought once so far"
    );
    assert_eq!(went(db).await["旧121"], None, "still a note");

    // Brought along by the next stretch too, and still unread: kept as
    // they are, the first pass's notes not among them.
    super::unread::never_read(db, stretch_from(db, "settle", ago(5)).await).await;
    let all = went(db).await;
    assert_eq!(all["旧121"], holds("旧121"), "kept as it was");
    assert_eq!(all["新10"], holds("新10"));
    assert_eq!(
        all["旧122"], None,
        "gone over by the first pass: left to it"
    );
    schema.drop().await;
}

/// A pass that takes all the stretch brought along: what is left is the
/// stretch's own, and left unread it waits as its own, not brought along.
#[tokio::test]
async fn once_what_was_brought_along_is_gone_over_the_rest_is_the_stretchs_own() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    noted_at(db, "旧", ago(120)).await;
    for minutes in 1..=10 {
        noted_at(db, &format!("新{minutes}"), ago(minutes)).await;
    }
    super::unread::never_read(db, stretch_from(db, "settle", ago(121)).await).await;
    let taken = stretch_from(db, "settle", ago(11)).await;
    let rest = rest_of(db, &taken).await;
    assert_eq!(rest, Some(ago(2)));
    finish(db, &taken, Ok(going_on(vec![], vec![], ago(2))))
        .await
        .unwrap();
    due(db, "settle").await;
    super::unread::never_read(db, claim(db).await.unwrap().expect("the rest")).await;
    let job = loaded(db, "settle").await;
    assert_eq!((job.unread_from, job.unread_times), (Some(ago(2)), 0));
    schema.drop().await;
}

/// Her switch went off during a later pass of a stretch that brought notes
/// along: put back as it was, what it brought along and has not gone over
/// left unread again from where the pass began; taken up again, it goes on
/// from there, brought along as many times as before.
#[tokio::test]
async fn a_later_pass_put_back_goes_on_where_it_was() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    for minutes in 120..=130 {
        noted_at(db, &format!("旧{minutes}"), ago(minutes)).await;
    }
    super::unread::never_read(db, stretch_from(db, "settle", ago(131)).await).await;
    let taken = stretch_from(db, "settle", ago(11)).await;
    finish(db, &taken, Ok(going_on(vec![], vec![], ago(121))))
        .await
        .unwrap();
    due(db, "settle").await;
    let next = claim(db).await.unwrap().expect("the rest");
    finish(db, &next, Ok(Effect::Paused)).await.unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(from_of(&job), Some(ago(11)), "its own from where it began");
    assert_eq!((job.unread_from, job.unread_times), (Some(ago(121)), 0));
    assert_eq!(job.said_from, Some(ago(131)), "its lines still from there");
    due(db, "settle").await;
    let again = claim(db).await.unwrap().expect("taken up again");
    assert_eq!(from_of(&again.job), Some(ago(121)));
    super::unread::never_read(db, again).await;
    let job = loaded(db, "settle").await;
    assert_eq!((job.unread_from, job.unread_times), (Some(ago(121)), 1));
    schema.drop().await;
}

/// Brought along the second time and its later pass never read: what it
/// brought along and has not gone over is kept as it is, and only that;
/// its own notes wait.
#[tokio::test]
async fn a_later_pass_never_read_keeps_what_was_brought_along_twice() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    for minutes in 120..=130 {
        noted_at(db, &format!("旧{minutes}"), ago(minutes)).await;
    }
    noted_at(db, "新10", ago(10)).await;
    super::unread::never_read(db, stretch_from(db, "settle", ago(131)).await).await;
    super::unread::never_read(db, stretch_from(db, "settle", ago(100)).await).await;
    let taken = stretch_from(db, "settle", ago(11)).await;
    assert_eq!(
        from_of(&taken.job),
        Some(ago(131)),
        "brought the second time"
    );
    assert_eq!(rest_of(db, &taken).await, Some(ago(121)));
    finish(db, &taken, Ok(going_on(vec![], vec![], ago(121))))
        .await
        .unwrap();
    due(db, "settle").await;
    super::unread::never_read(db, claim(db).await.unwrap().expect("the rest")).await;
    let all = went(db).await;
    assert_eq!(all["旧121"], holds("旧121"), "kept as it was");
    assert_eq!(all["旧120"], holds("旧120"));
    assert_eq!(
        all["旧122"], None,
        "gone over by the first pass: left to it"
    );
    assert_eq!(all["新10"], None, "its own waits");
    let job = loaded(db, "settle").await;
    assert_eq!((job.unread_from, job.unread_times), (Some(ago(11)), 0));
    schema.drop().await;
}
