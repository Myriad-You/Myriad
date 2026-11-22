use super::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

async fn person(db: &DatabaseConnection, name: &str) -> i32 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO users (username) VALUES ($1) RETURNING id",
        [name.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "id")
    .unwrap()
}

/// A thing she did on her own, as her own time keeps it.
async fn did(db: &DatabaseConnection, note: &str, evidence: serde_json::Value) {
    unified::remember_own(
        db,
        note,
        &evidence.to_string(),
        Vec::new(),
        unified::OWN_EXPERIENCE,
    )
    .await
    .unwrap()
    .unwrap();
}

fn song(id: &str, reaction: &str) -> serde_json::Value {
    json!({
        "key": format!("song:netease:{id}"),
        "thing": {
            "kind": "song", "id": id, "source": "netease", "name": "晴天", "artist": "周杰伦",
            "album": "", "cover": "", "durationMs": 240000,
        },
        "reaction": reaction,
    })
}

/// What happens with people, written where it is kept: a big change of
/// her emotion toward someone, and something she remembers of them.
async fn with_people(db: &DatabaseConnection, someone: i32) {
    let affect = |emotion: f64| {
        json!({ "mood": 50.0, "arousal": 50.0, "emotion": emotion, "emotionArousal": 50.0 })
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_affect_events (id, user_id, at, cause, impulse, baseline_mood, \
         baseline_arousal, before, after, created_at) VALUES ('ae-undertone', $1, now(), \
         'user_scold', '{}'::jsonb, 70, 48, $2, $3, now())",
        [someone.into(), affect(50.0).into(), affect(5.0).into()],
    ))
    .await
    .unwrap();
    unified::remember(
        db,
        unified::NewMemory {
            user_id: someone,
            kind: unified::MemoryKind::Fact,
            content: "小林说她最近被老板骂了".into(),
            evidence: Some(song("2", "moved").to_string()),
            speaker: unified::Speaker::User,
            source: "chat",
            audience: unified::Audience::private(someone),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap();
}

/// Her undertone and her own gaps read her own rows only: what happens
/// with people (a scolding, what she knows of them, even a row of theirs
/// shaped like one of her experiences) changes neither, and what is kept
/// is numbers and a date, nothing anyone said or is called.
#[tokio::test]
async fn her_undertone_is_made_of_her_own_days_only() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "her_undertone").await;
    let db = isolated.db.clone();
    did(&db, "这首歌听完一直在脑子里转", song("1", "moved")).await;
    did(&db, "还行", song("3", "fine")).await;
    did(&db, "不太对我胃口", song("4", "not_for_me")).await;
    did(
        &db,
        "猜错了",
        json!({
            "key": "serial:pg-1:1",
            "thing": { "kind": "chapter", "serial": "pg-1", "title": "T", "author": "A", "index": 1, "total": 3 },
            "reaction": "fine",
            "guessed": { "said": "狗是关键", "held": "no", "happened": "不是狗" },
        }),
    )
    .await;
    unified::remember_own(
        &db,
        "想要的「把她的歌听一遍」实现了：听完了",
        &json!({ "want": "把她的歌听一遍", "since": "2026-11-01T00:00:00Z", "ended": "came_true", "how": "听完了" }).to_string(),
        Vec::new(),
        super::super::wants::ENDED,
    )
    .await
    .unwrap();

    let now = crate::clock::now().fixed_offset();
    let since = now - chrono::Duration::hours(1);
    let until = now + chrono::Duration::minutes(1);
    let (pushes, things) = gaps_between(&db, since, until).await;
    let mut moved: Vec<f64> = pushes.iter().map(|push| push.valence).collect();
    moved.sort_by(f64::total_cmp);
    use myriad_merope::undertone::{CAME_TRUE, GUESS, MOVED};
    assert_eq!(moved, [-GUESS, MOVED, CAME_TRUE], "only what landed hard");
    assert_eq!(things, 4);

    let today = super::super::clock::local_now().date_naive();
    let alone = day_of(&db, today).await.unwrap();
    let someone = person(&db, "undertone-someone").await;
    with_people(&db, someone).await;
    assert_eq!(day_of(&db, today).await.unwrap(), alone);
    assert_eq!(gaps_between(&db, since, until).await.0.len(), pushes.len());

    let undertone = Undertone::after(None, today, &alone).unwrap();
    let kept = serde_json::to_string(&undertone).unwrap();
    for word in ["undertone-someone", "小林", "晴天", "周杰伦", "狗"] {
        assert!(!kept.contains(word), "{kept}");
    }
    isolated.drop().await;
}

/// The night folds yesterday once; where her mood settles toward anyone
/// then reads it, her arousal moved and her valence left as it was.
#[tokio::test]
async fn the_night_folds_her_day_and_every_baseline_reads_it() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "her_undertone_night").await;
    let db = isolated.db.clone();
    let someone = person(&db, "undertone-night").await;
    assert_eq!(kept(&db).await, None);
    let before = super::super::store::baseline_toward(&db, someone).await.unwrap();

    // Yesterday she did a few things, one of them moved her.
    for (id, reaction) in [("1", "moved"), ("2", "fine"), ("3", "liked")] {
        did(&db, "听了一首", song(id, reaction)).await;
    }
    let yesterday = super::super::clock::local_now().date_naive().pred_opt().unwrap();
    let (start, _) = super::super::pace::bounds(yesterday).unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1 WHERE user_id IS NULL",
        [(start + chrono::Duration::hours(12)).into()],
    ))
    .await
    .unwrap();

    go_over(&db).await;
    let folded = kept(&db).await.unwrap();
    assert_eq!(folded.day, yesterday);
    assert_eq!(folded.days, 1);
    go_over(&db).await;
    assert_eq!(kept(&db).await, Some(folded), "a night folds a day once");

    // Her undertone goes with her: it is one of her log's kinds, and what
    // a move or an export carries has it.
    use myriad_persona_sync::Kind;
    assert!(crate::services::her_log::state::KINDS.contains(&Kind::Undertone));
    let pictures = crate::services::persona_sync::assets::Pictures::new(&db, someone);
    let carried = crate::services::persona_sync::snapshot::of_kind(
        &db,
        &pictures,
        someone,
        Kind::Undertone,
        None,
        10,
    )
    .await
    .unwrap();
    assert_eq!(carried.len(), 1, "{carried:?}");

    let after = super::super::store::baseline_toward(&db, someone).await.unwrap();
    assert_eq!(after.mood, before.mood, "valence is not opened");
    assert_eq!(
        after,
        myriad_merope::affect::baseline(before, Some(&folded), 0.0)
    );
    isolated.drop().await;
}
