//! Writing first after days apart: the last talk with them was four days
//! ago, when they went off to class; since then a chapter of a novel got to
//! her. What she writes should take that talk as from then, not ask about
//! "today's class", and what is hers should be named, not told as details
//! they never read.
//!
//! Opt-in and spends on the site's models, like `days`:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin
//! myriad-backend -- --ignored her_first_words_after_days_apart --nocapture`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

const SOUL: &str = "气质：慢热，熟了以后爱吐槽，嘴硬心软。\n喜好：冷笑话、日系音乐、读小说。\n表达：说话随意，句子短，不爱长篇大论。";

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_first_words_after_days_apart() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_apart").await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', '小满', $1, now())",
        [SOUL.into()],
    ))
    .await
    .unwrap();
    let user_id = super::new_user(&db, "小林").await;
    // Four days ago, the last time they talked.
    let session_id = format!("apart-{user_id}");
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, $3, 0, false, now() - interval '97 hours', now() - interval '96 hours')",
        [session_id.clone().into(), user_id.into(), json!({ "mode": "chat" }).into()],
    ))
    .await
    .unwrap();
    let said = [
        ("user", "你好呀", 97),
        ("assistant", "来啦！你这一天跑哪折腾去了？", 97),
        ("user", "我去上课了", 96),
        (
            "assistant",
            "上课啊，难怪大半天见不着人。今天讲的什么？",
            96,
        ),
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
    // On her own, a chapter that got to her and that she would tell.
    crate::services::agent::memory::unified::remember_own(
        &db,
        "读到这里，我完全被他们那种破釜沉舟的劲儿抓住了。看着主角刮掉胡子准备顶替国王，我心里居然有点激动，想知道他到底能不能瞒天过海。",
        r#"{"key":"serial:pg-95:4","thing":{"kind":"chapter","serial":"pg-95","title":"The prisoner of Zenda","author":"Anthony Hope","index":4,"total":31},"reaction":"moved","tell":true}"#,
        Vec::new(),
        crate::services::agent::memory::unified::OWN_EXPERIENCE,
    )
    .await
    .unwrap();

    let first = super::super::reach::first_words_now(&db, user_id).await;
    isolated.drop().await;
    let line = match &first {
        Some((_, about, line)) => {
            println!("-- she would write first: about {about}\n   {line}");
            line.clone()
        }
        None => {
            println!("-- she would not write first now");
            String::new()
        }
    };
    let brings_up_the_book = ["国王", "读", "书", "章", "小说"]
        .iter()
        .any(|word| line.contains(word));
    let checks = [
        ("she writes first", first.is_some()),
        (
            "the class was days ago, not today",
            !(line.contains("今天") && line.contains("课")),
        ),
        (
            "the book she brings up is named as it is",
            !brings_up_the_book
                || ["曾达", "Zenda", "增达", "Anthony Hope"]
                    .iter()
                    .any(|name| line.contains(name)),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held));
}
