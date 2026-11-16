//! Every short note has somewhere to go once its stretch is gone over: it is
//! still a note, or a memory that is there holds it now. None is let go for
//! nothing.

use super::*;
use crate::services::agent::memory::unified;

#[path = "memory_jobs_unread_tests.rs"]
mod unread;

async fn note(db: &DatabaseConnection, text: &str, source: &'static str) {
    unified::remember(
        db,
        unified::NewMemory {
            user_id: 7,
            kind: unified::MemoryKind::Fact,
            content: text.into(),
            evidence: Some(text.into()),
            speaker: unified::Speaker::User,
            source,
            audience: Audience::private(7),
            importance: 0.6,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .expect("written");
}

async fn id_of(db: &DatabaseConnection, text: &str) -> String {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id FROM agent_memories WHERE content = $1 AND source = 'chat_note'",
        [text.into()],
    ))
    .await
    .unwrap()
    .expect("a note")
    .try_get("", "id")
    .unwrap()
}

fn lasting(fact: &str, supersedes: &[&str]) -> ChatMemoryUpdate {
    ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: Some(fact.into()),
        supersedes: supersedes.iter().map(|text| text.to_string()).collect(),
        evidence: Some(fact.into()),
        concepts: vec![],
    }
}

/// A stretch to go over, claimed.
async fn stretch(db: &DatabaseConnection, id: &str) -> Claim {
    let at = crate::clock::now().fixed_offset() - chrono::Duration::minutes(30);
    stretch_from(db, id, at).await
}

/// A stretch that began `at`, claimed.
async fn stretch_from(
    db: &DatabaseConnection,
    id: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) -> Claim {
    let payload = Payload::Settle {
        present: Audience::private(7),
        from: at,
        last: at,
    };
    enqueue(db, id, 7, payload, None).await.unwrap();
    due(db, id).await;
    claim(db).await.unwrap().expect("a stretch over")
}

fn settled(updates: Vec<ChatMemoryUpdate>, settled: Vec<Vec<String>>) -> Effect {
    Effect::Settle {
        updates: crate::services::agent::merope::chat_remember::ChatMemoryUpdates {
            updates,
            said: vec![],
            put_onto: vec![],
        },
        settled,
    }
}

/// Where each note is now: still a note (`None`), or what holds it and
/// where that came from.
async fn went(db: &DatabaseConnection) -> Went {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT note.content, note.invalid_at IS NULL AS still, \
             kept.content AS kept, kept.source AS kept_source \
             FROM agent_memories note \
             LEFT JOIN agent_memories kept \
               ON kept.id = note.superseded_by AND kept.invalid_at IS NULL \
             WHERE note.source = 'chat_note'"
                .to_string(),
        ))
        .await
        .unwrap();
    rows.iter()
        .map(|row| {
            let content: String = row.try_get("", "content").unwrap();
            if row.try_get::<bool>("", "still").unwrap() {
                return (content, None);
            }
            let kept: Option<String> = row.try_get("", "kept").unwrap();
            let source: Option<String> = row.try_get("", "kept_source").unwrap();
            let held = kept
                .zip(source)
                .unwrap_or_else(|| panic!("{content}: let go with nothing holding it"));
            (content, Some(held))
        })
        .collect()
}

type Went = std::collections::BTreeMap<String, Option<(String, String)>>;

fn holds(text: &str) -> Option<(String, String)> {
    Some((text.into(), "chat".into()))
}

/// Each way a note can go once its stretch is gone over: put together into
/// a fact, kept word for word, passed over and kept as it is, left to the
/// moment, already known, kept in a fact that could not be written, and a
/// stretch that failed on the way and was gone over again.
#[tokio::test]
async fn every_note_has_somewhere_to_go_once_its_stretch_is_gone_over() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    note(db, "在学日语", "chat").await;
    for text in [
        "养了一只猫叫年糕",
        "下周去大阪出差",
        "喜欢喝乌龙茶",
        "今天有点累",
        "在学日语。",
        "住在新加坡",
    ] {
        note(db, text, unified::SHORT_NOTE_SOURCE).await;
    }
    let mut ids = Vec::new();
    for text in [
        "养了一只猫叫年糕",
        "下周去大阪出差",
        "喜欢喝乌龙茶",
        "在学日语。",
        "住在新加坡",
    ] {
        ids.push(vec![id_of(db, text).await]);
    }
    let over = stretch(db, "settle").await;
    let effect = settled(
        vec![
            // Put together: other words than the note's.
            lasting("家里有只橘猫叫年糕", &[]),
            // Kept word for word.
            lasting("下周去大阪出差", &[]),
            // Passed over: kept for good as it is.
            lasting("喜欢喝乌龙茶", &[]),
            // Already known for good, in those words.
            lasting("在学日语", &[]),
            // What it changes is already gone: nothing is written.
            lasting("搬到了新加坡", &["住在马来西亚"]),
        ],
        ids,
    );
    finish(db, &over, Ok(effect)).await.unwrap();
    assert_eq!(
        went(db).await,
        Went::from([
            ("今天有点累".to_string(), None),
            ("下周去大阪出差".to_string(), holds("下周去大阪出差")),
            ("住在新加坡".to_string(), None),
            ("在学日语。".to_string(), holds("在学日语")),
            ("养了一只猫叫年糕".to_string(), holds("家里有只橘猫叫年糕")),
            ("喜欢喝乌龙茶".to_string(), holds("喜欢喝乌龙茶")),
        ])
    );

    // A stretch that failed on the way is tried again: its notes wait.
    note(db, "想养一只狗", unified::SHORT_NOTE_SOURCE).await;
    let failed = stretch(db, "failed").await;
    finish(db, &failed, Err(Failure::Model(call::Failure::Timeout)))
        .await
        .unwrap();
    assert!(loaded(db, "failed").await.data.is_some(), "tried again");
    let waiting = went(db).await;
    assert_eq!(waiting["想养一只狗"], None, "still a note");
    due(db, "failed").await;
    let again = claim(db).await.unwrap().expect("tried again");
    let dog = vec![vec![id_of(db, "想养一只狗").await]];
    finish(
        db,
        &again,
        Ok(settled(vec![lasting("想养一只狗", &[])], dog)),
    )
    .await
    .unwrap();
    let all = went(db).await;
    assert_eq!(all["想养一只狗"], holds("想养一只狗"));
    assert_eq!(all.len(), 7);
    schema.drop().await;
}

/// An answer keeping more for good than one stretch keeps
/// (`chat_remember::MAX_LASTING`): the facts past it are not kept, and the
/// notes only they were put together from are passed over, so kept as they
/// are, as going over keeps any note it passes over. None is let go.
#[tokio::test]
async fn notes_of_a_fact_past_the_most_kept_are_kept_as_they_are() {
    use crate::services::channel_group::g0;
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let said = [
        "我在学吉他",
        "周末要搬家",
        "养了一只猫",
        "下周去大阪出差",
        "最近在夜跑",
        "喜欢喝乌龙茶",
        "妹妹下个月结婚",
    ];
    assert_eq!(said.len(), myriad_merope::chat_remember::MAX_LASTING + 1);
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(10);
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ('settle-talk', 7, 't', '{\"mode\":\"chat\"}'::jsonb, \
         0, false, NOW(), NOW())",
    )
    .await
    .unwrap();
    for text in said {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) \
             VALUES ('settle-talk', 'user', $1, '{}'::jsonb, NOW())",
            [text.into()],
        ))
        .await
        .unwrap();
        note(db, text, unified::SHORT_NOTE_SOURCE).await;
    }
    // Each fact from one note, said otherwise than the note.
    let lasting: Vec<serde_json::Value> = said
        .iter()
        .enumerate()
        .map(|(index, text)| {
            serde_json::json!({
                "fact": format!("他说{text}"), "evidence": text, "from": [index],
                "concepts": [], "whenFor": [], "topic": null,
                "happened": null, "state": null, "open": null,
            })
        })
        .collect();
    let answer = serde_json::json!({
        "lasting": lasting, "replaces": [], "moment": [], "known": [],
    });
    // The last fact holds on its own: only the most kept leaves it out.
    let theirs = said.join("\n");
    let parse = |answer: &serde_json::Value| {
        myriad_merope::chat_remember::parse_settle(&answer.to_string(), &theirs, &[], said.len(), 0)
            .expect("read")
            .0
    };
    let alone = serde_json::json!({
        "lasting": [lasting.last()], "replaces": [], "moment": [], "known": [],
    });
    assert_eq!(parse(&alone)[0].from, [said.len() - 1]);
    let kept = parse(&answer);
    assert_eq!(kept.len(), myriad_merope::chat_remember::MAX_LASTING);
    assert_eq!(kept.last().unwrap().from, [said.len() - 2]);
    let taken = stretch_from(db, "settle", began).await;
    let mut world = g0::World::new(0.5);
    world
        .models
        .entry("chat_settle".into())
        .or_default()
        .push_back(Some(answer.to_string()));
    g0::enter(world);
    let effect =
        crate::services::agent::merope::chat_remember::settle(db, 7, &Audience::private(7), began)
            .await;
    g0::leave();
    let effect = effect.unwrap_or_else(|_| panic!("gone over"));
    finish(db, &taken, Ok(effect)).await.unwrap();
    let all = went(db).await;
    assert_eq!(all.len(), said.len());
    for text in &said[..myriad_merope::chat_remember::MAX_LASTING] {
        assert_eq!(all[*text], holds(&format!("他说{text}")), "{text}");
    }
    assert_eq!(
        all["妹妹下个月结婚"],
        holds("妹妹下个月结婚"),
        "past the most kept: kept as it is"
    );
    schema.drop().await;
}
