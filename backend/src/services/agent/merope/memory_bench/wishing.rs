//! Something of hers toward someone: they told her they have been getting
//! into anime songs; on her own she heard a Reol song that got to her. Going
//! over the day with them at night, does she come to want to show them it,
//! and is that a reason to write to them first, without telling it in a
//! way that is not hers?
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
    let start = chrono::Utc::now().fixed_offset() - chrono::Duration::hours(31);
    let end = chrono::Utc::now().fixed_offset() - chrono::Duration::hours(28);
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
