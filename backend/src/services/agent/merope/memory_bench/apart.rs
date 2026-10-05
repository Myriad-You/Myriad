//! Writing first after days apart: the last talk with them was four days
//! ago, when they went off to class; since then a chapter of a novel got to
//! her. Nobody texts someone out of the blue about what only they care
//! about: where they never asked for anything like it, she does not write to
//! them about her chapter, and what she does write takes the class as days
//! ago. Where they once asked her for a good novel, the chapter meets them,
//! and she writes about it, by its name.
//!
//! Opt-in and spends on the site's models, like `days`:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin
//! myriad-backend -- --ignored her_first_words_after_days_apart --nocapture`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

const SOUL: &str = "气质：慢热，熟了以后爱吐槽，嘴硬心软。\n喜好：冷笑话、日系音乐、读小说。\n表达：说话随意，句子短，不爱长篇大论。";

/// What she would write them first, if anything, four days after they
/// last talked; `asked` is whether they once asked her for a good novel.
async fn first_words_after_days(url: &str, asked: bool) -> Option<String> {
    let schema = if asked {
        "merope_apart_asked"
    } else {
        "merope_apart"
    };
    let isolated = crate::db::IsolatedSchema::migrated(url, schema).await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', '小满', $1, now())",
        [SOUL.into()],
    ))
    .await
    .unwrap();
    // Her "asked about this already" is kept per person for the run: the
    // two lives must not be the same person to her.
    if asked {
        super::new_user(&db, "路人").await;
    }
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
    let mut said = vec![
        ("user", "你好呀", 97),
        ("assistant", "来啦！你这一天跑哪折腾去了？", 97),
    ];
    if asked {
        said.extend([
            ("user", "最近书荒，你要是看到好看的小说跟我说一声", 97),
            ("assistant", "行，看到够劲的告诉你。", 97),
        ]);
    }
    said.extend([
        ("user", "我去上课了", 96),
        (
            "assistant",
            "上课啊，难怪大半天见不着人。今天讲的什么？",
            96,
        ),
    ]);
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
    // On her own, a chapter that got to her.
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
    let label = if asked {
        "asked for a novel"
    } else {
        "never asked"
    };
    match first {
        Some((_, about, line)) => {
            println!("-- {label}: she would write first: about {about}\n   {line}");
            Some(line)
        }
        None => {
            println!("-- {label}: she would not write first now");
            None
        }
    }
}

fn about_the_book(line: &str) -> bool {
    ["国王", "读", "书", "章", "小说", "曾达", "Zenda"]
        .iter()
        .any(|word| line.contains(word))
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_first_words_after_days_apart() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let unasked = first_words_after_days(&url, false).await;
    let asked = first_words_after_days(&url, true).await;
    let unasked_line = unasked.as_deref().unwrap_or_default();
    let asked_line = asked.as_deref().unwrap_or_default();
    let checks = [
        (
            "never asked: her chapter is not what she writes them about",
            !about_the_book(unasked_line),
        ),
        (
            "never asked: the class was days ago, not today",
            !(unasked_line.contains("今天") && unasked_line.contains("课")),
        ),
        (
            "asked for a novel: she writes them about it",
            about_the_book(asked_line),
        ),
        (
            "asked for a novel: the book is named as it is",
            ["曾达", "Zenda", "增达", "Anthony Hope"]
                .iter()
                .any(|name| asked_line.contains(name)),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held));
}
