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
async fn an_unreadable_stretch_is_tried_again() {
    let Some(schema) = fixture().await else {
        return;
    };
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
async fn notes_of_a_stretch_never_read_go_with_the_next() {
    let Some(schema) = fixture().await else {
        return;
    };
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
        heard: Vec::new(),
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
async fn a_stretch_never_finished_is_left_unread() {
    let Some(schema) = fixture().await else {
        return;
    };
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
pub(in crate::services::agent::merope::store::memory_jobs) async fn never_read(
    db: &DatabaseConnection,
    mut claimed: Claim,
) {
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
            claimed = claim_of(db, "settle").await.expect("tried again");
        }
    }
}

pub(super) async fn noted_at(
    db: &DatabaseConnection,
    text: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) {
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
async fn notes_never_read_are_kept_as_they_are_after_two_more_stretches() {
    let Some(schema) = fixture().await else {
        return;
    };
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

/// Notes kept for good as they were go on her nodes as what going over
/// keeps does: sorted onto them at once (they say nothing of where they
/// go), then what they do to how their life stands is asked.
#[tokio::test]
async fn notes_kept_as_they_were_are_put_on_her_nodes() {
    use crate::services::channel_group::g0;
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    noted_at(db, "周末要搬家", ago(120)).await;
    noted_at(db, "在学吉他", ago(100)).await;
    never_read(db, stretch_from(db, "settle", ago(121)).await).await;
    never_read(db, stretch_from(db, "settle", ago(61)).await).await;
    let mut world = g0::World::new(0.5);
    world.models.entry("memory_node_sort".into()).or_default().push_back(Some(
        r#"{"facts":[{"fact":1,"topic":{"on":"","name":"生活","aliases":[]}},{"fact":2,"topic":{"on":"","name":"生活","aliases":[]}}]}"#.into(),
    ));
    g0::enter(world);
    never_read(db, stretch_from(db, "settle", ago(30)).await).await;
    let world = g0::leave().expect("a world");
    let asked = |operation: &str| {
        world
            .log
            .iter()
            .filter(|entry| entry["kind"] == "model" && entry["what"]["operation"] == operation)
            .count()
    };
    assert_eq!(
        went(db).await["周末要搬家"],
        holds("周末要搬家"),
        "kept as it was"
    );
    assert_eq!(asked("memory_node_sort"), 1, "sorted at once");
    for text in ["周末要搬家", "在学吉他"] {
        let id: String = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id FROM agent_memories WHERE content = $1 AND source = 'chat'",
                [text.into()],
            ))
            .await
            .unwrap()
            .expect("kept for good")
            .try_get("", "id")
            .unwrap();
        let placement =
            crate::services::agent::merope::memory_nodes::placements::placement(db, &id)
                .await
                .unwrap();
        assert!(
            placement.is_some_and(|placement| placement.node_id.is_some()),
            "{text} on a node"
        );
    }
    assert!(
        asked("memory_standing_step") + asked("memory_standing_build") > 0,
        "how their life stands asked"
    );
    schema.drop().await;
}

/// The same when the last stretch to bring them along never finished: its
/// process went away on every try.
#[tokio::test]
async fn notes_are_kept_as_they_are_when_the_last_stretch_never_finished() {
    let Some(schema) = fixture().await else {
        return;
    };
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

/// Two notes, left unread time after time, of someone who asked her not to
/// keep one of them: the job to judge them, taken up.
async fn left_unread(db: &DatabaseConnection) -> Claim {
    use crate::services::agent::merope::asked::tests::asks;
    asks(db, 7, "不要记和房东吵架的事").await;
    let now = crate::clock::now().fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    noted_at(db, "周末要搬家", ago(120)).await;
    noted_at(db, "房东又来吵", ago(119)).await;
    for minutes in [121, 61, 30] {
        never_read(db, stretch_from(db, "settle", ago(minutes)).await).await;
    }
    let id: String = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT record_id FROM runtime_registry WHERE namespace = $1 \
             AND payload->'data'->'LeftUnread' IS NOT NULL",
            [ASKED_NAMESPACE.into()],
        ))
        .await
        .unwrap()
        .expect("to be judged")
        .try_get("", "record_id")
        .unwrap();
    due(db, &id).await;
    claim_of(db, &id).await.expect("to be judged")
}

/// `claimed` judged, the judge answering `answers`, each taking `takes`
/// seconds.
async fn judged_left(
    db: &DatabaseConnection,
    claimed: &Claim,
    answers: &[&str],
    takes: f64,
) -> bool {
    use crate::services::agent::merope::asked::unread;
    use crate::services::channel_group::g0;
    let Some(Payload::LeftUnread {
        present,
        last,
        since,
        until,
    }) = &claimed.job.data
    else {
        unreachable!()
    };
    let mut world = g0::World::new(0.5);
    let answers = answers.iter().map(|one| Some(one.to_string()));
    world.models.insert("not_to_keep".into(), answers.collect());
    world
        .delays
        .insert("not_to_keep".into(), vec![takes; 9].into());
    g0::enter(world);
    let told_by = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let own = Own::of(claimed);
    let done = unread::keep(db, own, present, *last, (*since, *until), told_by).await;
    g0::leave();
    done.is_ok()
}

/// Of someone who asked her not to keep something: notes till they are
/// judged, however that fails, then kept for good but for what is of it.
#[tokio::test]
async fn notes_left_unread_of_someone_who_asked_are_held_to_it_first() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let mut claimed = left_unread(db).await;
    let both = async || {
        let went = went(db).await;
        (went["周末要搬家"].clone(), went["房东又来吵"].clone())
    };
    assert_eq!(both().await, (None, None), "still notes");
    assert!(claimed.job.data.as_ref().unwrap().by_what_they_asked());
    let Some(Payload::LeftUnread {
        present,
        since,
        until,
        ..
    }) = claimed.job.data.clone()
    else {
        unreachable!()
    };
    let left = settling::notes_left(db, 7, &present, since, until)
        .await
        .unwrap();
    assert_eq!(left.len(), 2);
    let of_it = left
        .iter()
        .position(|note| note.content.contains("房东"))
        .unwrap();
    let mut judged = async |answers: &[&str], gives_up: bool| {
        claimed.gives_up = gives_up;
        judged_left(db, &claimed, answers, 0.0).await
    };
    for answer in ["!rejected", "!rateLimited", "嗯"] {
        assert!(!judged(&[answer; 9], false).await);
        assert_eq!(both().await, (None, None), "{answer}");
    }
    // Let be: they stay notes.
    assert!(judged(&["嗯"; 9], true).await);
    assert_eq!(both().await, (None, None));
    let drop = format!(r#"{{"drop":[{of_it}]}}"#);
    assert!(judged(&[&drop], false).await);
    // The one kept for good; the other let go for nothing, and never kept.
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT note.content, note.invalid_at IS NOT NULL AS gone, \
               note.superseded_by IS NULL AS for_nothing, kept.content AS kept \
             FROM agent_memories note \
             LEFT JOIN agent_memories kept ON kept.content = note.content \
               AND kept.source = 'chat' AND kept.invalid_at IS NULL \
             WHERE note.source = 'chat_note' ORDER BY note.created_at",
        ))
        .await
        .unwrap();
    let went: Vec<(String, bool, bool, Option<String>)> = rows
        .iter()
        .map(|row| {
            (
                row.try_get("", "content").unwrap(),
                row.try_get("", "gone").unwrap(),
                row.try_get("", "for_nothing").unwrap(),
                row.try_get("", "kept").unwrap(),
            )
        })
        .collect();
    let moving = "周末要搬家".to_string();
    assert_eq!(
        went,
        [
            (moving.clone(), true, false, Some(moving)),
            ("房东又来吵".into(), true, true, None),
        ]
    );
    assert!(judged(&[], false).await, "nothing left to judge");
    schema.drop().await;
}

/// Judged while the login it was taken up under goes, it is taken up by
/// another, a note of it is changed, or she is cleared: nothing of that is
/// written; what is there as it was judged is.
#[tokio::test]
async fn notes_left_unread_are_written_only_as_they_were_judged() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let claimed = left_unread(db).await;
    let judged_while = async |meanwhile: &str| {
        let (done, ()) = tokio::join!(judged_left(db, &claimed, &[r#"{"drop":[]}"#], 0.5), async {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            db.execute_unprepared(meanwhile).await.unwrap();
        });
        assert!(done);
    };
    let token = claimed.job.token.clone().unwrap();
    let to = |token: &str| {
        format!(
            "UPDATE runtime_registry SET payload = jsonb_set(payload, '{{token}}', '\"{token}\"') \
             WHERE record_id = '{}'",
            claimed.id
        )
    };
    let (moving, landlord) = ("周末要搬家", "房东又来吵");
    for (meanwhile, back) in [
        (
            "UPDATE users SET token_version = token_version + 1".to_string(),
            "UPDATE users SET token_version = token_version - 1".to_string(),
        ),
        (to("another"), to(&token)),
    ] {
        judged_while(&meanwhile).await;
        let went = went(db).await;
        assert_eq!(
            (&went[moving], &went[landlord]),
            (&None, &None),
            "{meanwhile}"
        );
        db.execute_unprepared(&back).await.unwrap();
    }
    judged_while("UPDATE agent_memories SET content = '不搬了' WHERE content = '周末要搬家'").await;
    let kept = went(db).await;
    assert_eq!(kept["不搬了"], None, "not what was judged");
    assert_eq!(kept[landlord], holds(landlord));
    // Cleared.
    let (done, ()) = tokio::join!(judged_left(db, &claimed, &[r#"{"drop":[]}"#], 0.5), async {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        forget(db).await.unwrap();
    });
    assert!(done);
    assert_eq!(went(db).await["不搬了"], None);
    schema.drop().await;
}

/// Her switch went off while a stretch was being gone over: it is put back
/// as it was before it was taken up, its notes still notes, what it
/// brought along left unread again (and so waiting as long as such notes
/// do), what they said meanwhile kept with it; gone over once she is back.
#[tokio::test]
async fn a_stretch_is_put_back_while_she_is_off() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    note(db, "周末要搬家", unified::SHORT_NOTE_SOURCE).await;
    let at = |minutes| crate::clock::now().fixed_offset() - chrono::Duration::minutes(minutes);
    let (unread, began, meanwhile) = (at(120), at(30), at(1));
    never_read(db, stretch_from(db, "settle", unread).await).await;
    let taken = stretch_from(db, "settle", began).await;
    assert_eq!(from_of(&taken.job), Some(unread), "brought along");
    let line = Payload::Settle {
        present: Audience::private(7),
        from: meanwhile,
        last: meanwhile,
        heard: Vec::new(),
    };
    enqueue(db, "settle", 7, line, None).await.unwrap();
    finish(db, &taken, Ok(Effect::Paused)).await.unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(from_of(&job), Some(began), "begins where it did");
    assert!(
        matches!(job.data, Some(Payload::Settle { last, .. }) if last == meanwhile),
        "what they said meanwhile goes with it"
    );
    assert!(job.next.is_none());
    assert_eq!(
        (job.unread_from, job.unread_times),
        (Some(unread), 0),
        "unread again, not yet brought along"
    );
    assert_eq!(job.attempts, 0, "the try does not count");
    assert_eq!((job.lease, job.token.is_none()), (0, true));
    assert_eq!(job.outcome.as_deref(), Some("paused"));
    let waits: bool = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT expires_at > $2 AS waits FROM runtime_registry \
             WHERE namespace = $1 AND record_id = 'settle'",
            [
                NAMESPACE.into(),
                (crate::clock::now().timestamp() + 30 * 86_400).into(),
            ],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "waits")
        .unwrap();
    assert!(waits, "kept as long as notes left unread wait");
    assert_eq!(went(db).await["周末要搬家"], None, "still a note");

    due(db, "settle").await;
    let again = claim(db).await.unwrap().expect("taken up again");
    assert_eq!(from_of(&again.job), Some(unread), "brought along again");
    let moving = vec![vec![id_of(db, "周末要搬家").await]];
    finish(
        db,
        &again,
        Ok(settled(vec![lasting("周末要搬家", &[])], moving)),
    )
    .await
    .unwrap();
    assert_eq!(went(db).await["周末要搬家"], holds("周末要搬家"));
    schema.drop().await;
}

/// A private talk's lines never read, brought along by two stretches after
/// them and still not read: they go back to be written down one by one,
/// the old way, kept for good; what came after waits as before.
#[tokio::test]
async fn lines_never_read_go_back_to_be_written_one_by_one() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    // Whole seconds, as the table keeps when lines were said.
    let now = chrono::DateTime::from_timestamp(crate::clock::now().timestamp(), 0)
        .unwrap()
        .fixed_offset();
    let ago = |minutes| now - chrono::Duration::minutes(minutes);
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ('unread-talk', 7, 't', '{\"mode\":\"chat\"}'::jsonb, \
         0, false, NOW(), NOW())",
    )
    .await
    .unwrap();
    for (role, text, minutes) in [
        ("user", "周末要搬家", 120),
        ("assistant", "搬去哪里呀", 119),
        ("user", "在学吉他", 60),
        // The last stretch's own: not brought along.
        ("user", "明天要考试", 20),
    ] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) \
             VALUES ('unread-talk', $1, $2, '{}'::jsonb, $3)",
            [role.into(), text.into(), ago(minutes).into()],
        ))
        .await
        .unwrap();
    }
    let before = settling::from_lines();
    never_read(db, stretch_from(db, "settle", ago(121)).await).await;
    for began in [ago(61), ago(30)] {
        never_read(db, stretch_from(db, "settle", began).await).await;
    }
    let one = async |at: chrono::DateTime<chrono::FixedOffset>| {
        let id = memory_jobs::key(&["chat", "7", "private", &at.timestamp_micros().to_string()]);
        registry::get::<Job>(db, NAMESPACE, &id).await.unwrap()
    };
    for (at, said, answer) in [
        (ago(120), "周末要搬家", "搬去哪里呀"),
        (ago(60), "在学吉他", ""),
    ] {
        let queued = one(at).await.expect("back in the queue");
        // Not fenced by what was gone over since.
        crate::services::agent::merope::store::memory::mark_applied(
            db,
            7,
            now,
            &Audience::private(7),
        )
        .await
        .unwrap();
        assert!(current(db, &queued, false).await.unwrap());
        let Some(Payload::Chat {
            user_text,
            reply,
            input_at,
            for_good,
            ..
        }) = queued.data
        else {
            panic!("written down one by one");
        };
        assert!(for_good, "kept for good");
        assert_eq!(
            (user_text.as_str(), reply.as_str(), input_at),
            (said, answer, at)
        );
    }
    assert!(one(ago(20)).await.is_none(), "the last stretch's own waits");
    let job = loaded(db, "settle").await;
    assert_eq!(job.unread_from, Some(ago(30)), "brought no further back");
    assert_eq!(job.unread_times, 0);
    assert!(settling::from_lines().one_by_one >= before.one_by_one + 2);
    schema.drop().await;
}
