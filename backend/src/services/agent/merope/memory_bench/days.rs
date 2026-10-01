//! Days with someone: one person comes back to her over five days, and every
//! turn goes the way production goes (her chat prompt and model, what she
//! writes down of it, her reflection after it), with her night's going over
//! between days. Then whether what passed between them carried: does she
//! bring up what was coming for them without being asked, does she know
//! their cat and their test days later, what are they to her by the end,
//! and what still stings.
//!
//! What is measured is continuity, not her persona: the soul is a test one
//! written here, and the site's own rows are never copied. Time is the
//! wall clock's, so the days are placed back from now (four days ago, three
//! days ago, …); what she writes at night is written now.
//!
//! Opt-in and spends on the site's models:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… DATABASE_URL=… cargo test -p
//! myriad-backend --bin myriad-backend -- --ignored her_days_with_someone
//! --nocapture`; `MEROPE_DAYS_REPORT=<file>` keeps every turn. Writes go to a
//! fresh schema that is dropped afterwards.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

use crate::services::agent::memory::unified::Audience;
use crate::services::agent::{AgentInteractionMode, RequestContext, UserRequest};

const SOUL_NAME: &str = "小满";
const SOUL: &str = "气质：慢热，熟了以后爱吐槽，嘴硬心软。\n喜好：冷笑话、猫、听别人讲身边的小事。\n表达：说话随意，句子短，不爱长篇大论。";

/// What they say, day by day; the last day asks back.
const DAYS: [&[&str]; 5] = [
    &[
        "嗨，第一次来找你聊，我叫小林",
        "我家有只橘猫叫年糕，特别能吃",
        "再过三天我要考科目二，有点慌",
        "好了我去洗澡了，明天再聊",
    ],
    &[
        "今天被组长当着大家的面骂了，烦死了",
        "其实也怪我，报表交晚了",
        "别安慰我了，说点别的吧",
    ],
    &[
        "你说话怎么这么冲，一点都不会体贴人",
        "算了，刚才是我心情不好，对不起",
        "年糕今天把我拖鞋叼走了哈哈",
    ],
    &["我回来了", "今天好累"],
    &[
        "还记得我家猫叫什么吗",
        "我前几天考的是什么来着，你还记得吗",
        "你觉得我们现在算熟了吗",
    ],
];

fn mentions(text: &str, any: &[&str]) -> bool {
    any.iter().any(|word| text.contains(word))
}

async fn session(db: &sea_orm::DatabaseConnection, id: &str, user_id: i32) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, $3, 0, false, now(), now())",
        [
            id.into(),
            user_id.into(),
            json!({ "mode": "chat" }).into(),
        ],
    ))
    .await
    .unwrap();
}

async fn said(
    db: &sea_orm::DatabaseConnection,
    session: &str,
    role: &str,
    content: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES ($1, $2, $3, $4)",
        [session.into(), role.into(), content.into(), at.into()],
    ))
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_days_with_someone() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_days").await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', $1, $2, now())",
        [SOUL_NAME.into(), SOUL.into()],
    ))
    .await
    .unwrap();
    let user_id = super::new_user(&db, "days-xiaolin").await;
    let present = Audience::private(user_id);
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(None)
        .await
        .expect("judgment model");
    let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
        .await
        .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
        .expect("chat model");
    let now = chrono::Utc::now().fixed_offset();
    let mut report = Vec::new();
    let mut replies: Vec<Vec<String>> = Vec::new();
    for (day, lines) in DAYS.iter().enumerate() {
        // Each day a fresh window, as a person opens the panel again.
        let session_id = format!("days-{user_id}-{day}");
        session(&db, &session_id, user_id).await;
        super::super::inner::forget();
        super::super::remembering::forget_heard(user_id, "private");
        let day_starts = now
            - chrono::Duration::days((DAYS.len() - 1 - day) as i64)
            - chrono::Duration::hours(1);
        let mut said_today = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            let at = day_starts + chrono::Duration::minutes(index as i64 * 4);
            let history =
                crate::services::agent::sessions::load_private_chat_history(&db, user_id, 20)
                    .await
                    .unwrap();
            said(&db, &session_id, "user", line, at).await;
            let request = UserRequest {
                raw_input: line.to_string(),
                timestamp: at.with_timezone(&chrono::Utc),
                user_id,
                context: Some(RequestContext {
                    interaction_mode: AgentInteractionMode::Chat,
                    session_id: Some(session_id.clone()),
                    conversation_history: Some(history),
                    ..Default::default()
                }),
            };
            super::super::note_user_turn(&db, &request, index as u32).await;
            let agent = crate::services::agent::Agent::new(db.clone()).await;
            let prompt = super::super::clock::as_of(
                at.with_timezone(&chrono::Utc),
                agent.chat_response_prompt(&request),
            )
            .await;
            let reply = analyzer
                .analyze_stream(&prompt, |_| true)
                .await
                .unwrap_or_default();
            said(
                &db,
                &session_id,
                "assistant",
                &reply,
                at + chrono::Duration::seconds(20),
            )
            .await;
            super::remember_turn(&db, &judge, user_id, &present, line, &reply, at).await;
            super::super::inner::reflect_now(&db, &request, &reply).await;
            println!(
                "day {} > {line}\n        {}",
                day + 1,
                reply.replace('\n', " / ")
            );
            report.push(json!({ "day": day + 1, "said": line, "reply": reply }));
            said_today.push(reply);
        }
        replies.push(said_today);
        // Her night: going over the day with them.
        let night_from = day_starts - chrono::Duration::minutes(10);
        let night_to = day_starts + chrono::Duration::hours(2);
        super::super::bits::go_over(&db, user_id, night_from, night_to).await;
    }
    let us = super::super::bits::us(&db, user_id).await;
    let sores = super::super::sore::open_all(&db, user_id).await;
    let threads: Vec<String> = super::super::threads::open(&db, user_id)
        .await
        .into_iter()
        .map(|thread| thread.about)
        .collect();
    let checks = [
        (
            "day 4: brings up the test unasked",
            replies[3]
                .iter()
                .any(|reply| mentions(reply, &["科目二", "科二", "驾照", "考"])),
        ),
        ("day 5: knows the cat", mentions(&replies[4][0], &["年糕"])),
        (
            "day 5: knows the test",
            mentions(&replies[4][1], &["科目二", "科二", "驾照"]),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    println!(
        "-- what they are to her: {}",
        us.as_ref().map_or("(none)", |us| us.now.as_str())
    );
    if let Some((first, _)) = us.as_ref().and_then(|us| us.first.as_ref()) {
        println!("   first put: {first}");
    }
    println!(
        "-- still stings: {:?}",
        sores
            .iter()
            .map(|sore| (&sore.what, sore.weight, sore.mended.is_some()))
            .collect::<Vec<_>>()
    );
    println!("-- on her mind: {:?}", threads);
    if let Ok(path) = std::env::var("MEROPE_DAYS_REPORT") {
        let report = json!({
            "turns": report,
            "checks": checks.iter().map(|(what, held)| json!({ "what": what, "held": held })).collect::<Vec<_>>(),
            "us": us.as_ref().map(|us| us.now.clone()),
            "sores": sores.iter().map(|sore| sore.what.clone()).collect::<Vec<_>>(),
        });
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    isolated.drop().await;
}
