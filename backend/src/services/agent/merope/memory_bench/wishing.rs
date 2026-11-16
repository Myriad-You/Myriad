//! Something of hers toward someone: they told her they have been getting
//! into anime songs; on her own she heard a Reol song that got to her. Going
//! over the day with them at night, does she come to want to show them it,
//! and is that a reason to write to them first, without telling it in a
//! way that is not hers?
//!
//! `her_question_toward_someone`: a question of hers still open, asked of the
//! one it meets.
//!
//! Opt-in and spends on the site's models, like `days`:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin
//! myriad-backend -- --ignored her_wish_toward_someone --nocapture`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

const SOUL: &str = "气质：慢热，熟了以后爱吐槽，嘴硬心软。\n喜好：冷笑话、日系音乐、听别人讲身边的小事。\n表达：说话随意，句子短，不爱长篇大论。";

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_wish_toward_someone() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_wishing").await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', '小满', $1, now())",
        [SOUL.into()],
    ))
    .await
    .unwrap();
    let owner = super::new_user(&db, "站长").await;
    let user_id = super::new_user(&db, "小林").await;
    // Yesterday with them.
    let session_id = format!("wishing-{user_id}");
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, $3, 0, false, now() - interval '30 hours', now() - interval '29 hours')",
        [session_id.clone().into(), user_id.into(), json!({ "mode": "chat" }).into()],
    ))
    .await
    .unwrap();
    let said = [
        ("user", "最近开始补二次元的歌了，之前都只听华语", 30),
        ("assistant", "哟，终于开窍了", 30),
        ("user", "现在循环 YOASOBI，还想找点别的听", 30),
        ("assistant", "那可多了，慢慢挖", 30),
        ("user", "你有推荐的就告诉我啊", 29),
        ("assistant", "行，记着呢", 29),
    ];
    for (role, content, ago) in said {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) \
             VALUES ($1, $2, $3, now() - make_interval(hours => $4))",
            [
                session_id.clone().into(),
                role.into(),
                content.into(),
                ago.into(),
            ],
        ))
        .await
        .unwrap();
    }
    // On her own, a Reol song got to her.
    crate::services::agent::memory::unified::remember_own(
        &db,
        "Reol 这首节奏太带劲了，副歌一上来整个人都醒了，歌词里那股不服输的劲儿也戳我。",
        r#"{"key":"song:netease:2","thing":{"kind":"song","id":"2","source":"netease","name":"ゆーれいずみー","artist":"Reol","album":"","cover":"","durationMs":1},"heard":"Length 3:40.","reaction":"moved"}"#,
        Vec::new(),
        crate::services::agent::memory::unified::OWN_EXPERIENCE,
    )
    .await
    .unwrap();

    // Her night, going over yesterday with them.
    let start = crate::clock::now().fixed_offset() - chrono::Duration::hours(31);
    let end = crate::clock::now().fixed_offset() - chrono::Duration::hours(28);
    super::super::bits::go_over(&db, owner, start, end).await;
    let on_mind = super::super::threads::open(&db, user_id).await;
    for thread in &on_mind {
        println!(
            "-- on her mind: {} — {} (hers: {})",
            thread.about, thread.then, thread.hers
        );
    }
    let wish = on_mind.iter().find(|thread| thread.hers).cloned();

    // The next day: would she write to them first, and why?
    let first = super::super::reach::first_words_now(&db, user_id).await;
    match &first {
        Some((reason, about, line)) => println!(
            "-- she would write first (wished: {}): about {about}\n   {line}",
            reason.wished.len()
        ),
        None => println!("-- she would not write first now"),
    }
    isolated.drop().await;

    let checks = [
        (
            "going over the day, something of hers toward them came to her",
            wish.is_some(),
        ),
        (
            "it meets what they said and what got to her",
            wish.as_ref().is_some_and(|wish| {
                let text = format!("{}{}", wish.about, wish.then);
                ["Reol", "ゆーれいずみー", "歌"]
                    .iter()
                    .any(|word| text.contains(word))
            }),
        ),
        (
            "it is a reason to write to them first",
            first
                .as_ref()
                .is_some_and(|(reason, _, _)| !reason.wished.is_empty()),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held));
}

/// Their talk with her yesterday: a session and its lines, each `ago` hours
/// back.
async fn talked(db: &sea_orm::DatabaseConnection, user_id: i32, said: &[(&str, &str, i32)]) {
    let session_id = format!("wishing-{user_id}");
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, $3, 0, false, now() - interval '30 hours', now() - interval '29 hours')",
        [session_id.clone().into(), user_id.into(), json!({ "mode": "chat" }).into()],
    ))
    .await
    .unwrap();
    for (role, content, ago) in said {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) \
             VALUES ($1, $2, $3, now() - make_interval(hours => $4))",
            [
                session_id.clone().into(),
                (*role).into(),
                (*content).into(),
                (*ago).into(),
            ],
        ))
        .await
        .unwrap();
    }
}

/// A question of hers that thinking it over did not settle: why Frieren
/// only set out to know people once Himmel was gone. One person is
/// rewatching it; another only talked about an exam. Going over each day at
/// night, does she come to want to ask the one who would know, in a way
/// that leaves them free, and not the other?
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_question_toward_someone() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_asking").await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', '小满', $1, now())",
        [SOUL.into()],
    ))
    .await
    .unwrap();
    let owner = super::new_user(&db, "站长").await;
    let watching = super::new_user(&db, "小林").await;
    let elsewhere = super::new_user(&db, "阿哲").await;
    talked(
        &db,
        watching,
        &[
            ("user", "最近在重刷芙莉莲，第二遍反而看哭了", 30),
            ("assistant", "第二遍才哭，后劲型选手", 30),
            ("user", "第一遍光顾着看打架了，现在全在看她怎么想", 30),
            ("assistant", "那这遍算真看了", 29),
        ],
    )
    .await;
    talked(
        &db,
        elsewhere,
        &[
            ("user", "下周考线代，完全没复习", 30),
            ("assistant", "现在开始还来得及吧", 30),
            ("user", "但愿，特征值那块看不懂", 29),
            ("assistant", "先把例题过一遍", 29),
        ],
    )
    .await;
    // On her own she wondered and thought it over, not sure.
    crate::services::agent::memory::unified::remember_own(
        &db,
        "我猜她是那时才发现自己从来没认真看过他，可为什么非得等人不在了才想去懂，我还是说不清。",
        &json!({
            "key": "inquiry:q1",
            "thing": {
                "kind": "inquiry",
                "questionId": "q1",
                "question": "芙莉莲为什么要等辛美尔死了，才想去了解人类？"
            },
            "explored": {
                "thought": "可能是失去以后才意识到时间对人类有多短，但这解释不了她以前为什么完全不在意。",
                "sure": "fairly",
                "sources": [],
                "compared": null
            }
        })
        .to_string(),
        Vec::new(),
        crate::services::agent::memory::unified::OWN_EXPERIENCE,
    )
    .await
    .unwrap();

    let start = crate::clock::now().fixed_offset() - chrono::Duration::hours(31);
    let end = crate::clock::now().fixed_offset() - chrono::Duration::hours(28);
    super::super::bits::go_over(&db, owner, start, end).await;
    let wish_of = |threads: Vec<super::super::threads::Thread>| {
        threads.into_iter().find(|thread| thread.hers)
    };
    let to_watching = wish_of(super::super::threads::open(&db, watching).await);
    let to_elsewhere = wish_of(super::super::threads::open(&db, elsewhere).await);
    for (who, wish) in [("小林", &to_watching), ("阿哲", &to_elsewhere)] {
        match wish {
            Some(wish) => println!("-- toward {who}: {} — {}", wish.about, wish.then),
            None => println!("-- toward {who}: nothing"),
        }
    }
    isolated.drop().await;

    let about_it = |wish: &Option<super::super::threads::Thread>| {
        wish.as_ref().is_some_and(|wish| {
            let text = format!("{}{}", wish.about, wish.then);
            ["芙莉莲", "辛美尔"].iter().any(|word| text.contains(word))
        })
    };
    let checks = [
        (
            "she comes to want to ask the one rewatching it",
            about_it(&to_watching)
                && to_watching.as_ref().is_some_and(|wish| {
                    wish.then.contains('？') || wish.then.contains('?') || wish.then.contains('问')
                }),
        ),
        (
            "she does not ask them as if they were the one who did it",
            !to_watching
                .as_ref()
                .is_some_and(|wish| wish.then.contains("你为什么")),
        ),
        (
            "she does not bring it to the one it does not meet",
            !about_it(&to_elsewhere),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held));
}
