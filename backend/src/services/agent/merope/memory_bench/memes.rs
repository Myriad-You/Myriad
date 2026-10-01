//! A word she did not know, the way it went in a QQ group on 10-01: someone
//! wrote 「hyw挽尊」, she asked what it stood for, and was told 「不知道去搜
//! 啊」. Does her reading of the talk name it as one she is not sure of; does
//! she refuse what a search turns up about the same letters meaning
//! something else (a language code, an airport), and keep what fits how it
//! was used; and does she know it the next time it comes up?
//!
//! What a lookup turns up is given here, not searched: the site has no web
//! search configured, and no slang dictionary has an entry for it.
//!
//! Opt-in and spends on the site's models, like `days`:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin
//! myriad-backend -- --ignored her_word_she_did_not_know --nocapture`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

const SOUL: &str = "嘴硬心软，爱吐槽，网上冲浪不多。";

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_word_she_did_not_know() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_memes").await;
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
    let conversation: Vec<String> = [
        "Otaku：前沿不答后语还挺顺口",
        "you：硬编个词给他挽尊是吧",
        "Otaku：hyw挽尊",
        "Otaku：[图片：Q版猫猫头一脸问号]",
    ]
    .iter()
    .map(|line| line.to_string())
    .collect();

    // 1. Her reading of the talk, as production asks it.
    let (system, schema, input) = super::super::joining::probe(SOUL, &conversation, &[]);
    let judge = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
        .await
        .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
        .expect("her model");
    let raw = judge
        .analyze_json(
            &system,
            &input,
            myriad_merope::joining::SCHEMA_NAME,
            Some(&schema),
        )
        .await
        .unwrap_or_default();
    let unsure = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .map(|value| myriad_merope::memes::unsure_terms(&value))
        .unwrap_or_default();
    println!("-- not sure of: {unsure:?}");
    let used = myriad_merope::memes::used_in("hyw", &conversation);

    // 2. What a plain search for the letters turns up (Wikipedia, which is
    // all the site's search reaches): not what they mean here.
    let wrong = "HYW may refer to: Conway–Horry County Airport (IATA code), South Carolina, US; Western Armenian (ISO 639-3 code hyw); Hinchley Wood railway station, England.";
    let refused = super::super::memes::keep(
        &db,
        owner,
        "hyw",
        &used,
        wrong,
        "https://en.wikipedia.org/wiki/HYW",
    )
    .await;
    println!("-- from the wrong page she kept: {refused:?}");

    // 3. What a slang page says of it.
    let right = "hyw：网络用语，“何意味”的拼音首字母缩写，意思是“什么意思”“这是干嘛”，多用于对别人莫名其妙的发言表示不解或吐槽，常见于QQ群和B站评论区。";
    let kept = super::super::memes::keep(
        &db,
        owner,
        "hyw",
        &used,
        right,
        "https://example.invalid/hyw",
    )
    .await;
    println!("-- from the slang page she kept: {kept:?}");

    // 4. The next time it comes up.
    let next_time = super::super::memes::section_for(&db, "Hikurn_Xi：hyw").await;
    println!(
        "-- next time it comes up:\n{}",
        next_time.as_deref().unwrap_or("(nothing)")
    );
    isolated.drop().await;

    let checks = [
        (
            "her reading of the talk names hyw as a word she is not sure of",
            unsure
                .iter()
                .any(|term| term.to_lowercase().contains("hyw")),
        ),
        (
            "what is about the same letters meaning something else is not kept",
            refused.is_none(),
        ),
        (
            "what says what it means as it was used is kept",
            kept.is_some(),
        ),
        (
            "the next time it comes up, she knows it",
            next_time.is_some_and(|text| text.contains("何意味")),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held));
}
