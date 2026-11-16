//! Quick checks before spending on a run: embeddings and the site's models.

use super::*;

/// `MEROPE_MEMORY_EMBEDDING_MODEL`, when set: recall also by meaning, with
/// this model on Lite's credentials; unset, by words alone as configured.
pub(super) async fn embedding_as_asked() {
    if let Ok(model) = std::env::var("MEROPE_MEMORY_EMBEDDING_MODEL") {
        crate::GLOBAL_DYNAMIC_CONFIG
            .write()
            .await
            .aux_embedding_model = model;
    }
}

/// Before spending on a run: do the site's models answer at all, and if
/// not, what do they say. Prints each model's reply or its error chain.
#[tokio::test]
#[ignore = "calls the site's models once each"]
pub(super) async fn the_models_answer() {
    let config_db = super::super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    embedding_as_asked().await;
    let lite = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        Duration::from_secs(30),
    ))
    .await
    .expect("Lite model");
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
        Duration::from_secs(30),
    ))
    .await
    .expect("judgment model");
    if let Ok(model) = std::env::var("MEROPE_MEMORY_EMBEDDING_MODEL") {
        crate::GLOBAL_DYNAMIC_CONFIG
            .write()
            .await
            .aux_embedding_model = model;
        match crate::services::ai::create_lite_embedding_analyzer_with_timeout(Some(
            Duration::from_secs(30),
        ))
        .await
        {
            Some(embedder) => match embedder.embed(&["猫".to_string()]).await {
                Ok(vectors) => println!("embedding: ok {} dimensions", vectors[0].len()),
                Err(error) => {
                    let chain: Vec<String> = error.chain().map(ToString::to_string).collect();
                    println!("embedding: error {}", chain.join(" <- "));
                }
            },
            None => println!("embedding: not configured"),
        }
    }
    for (name, analyzer) in [("lite", &lite), ("judge", &judge)] {
        match analyzer
            .analyze_with_system("Reply with one word.", "ping")
            .await
        {
            Ok(reply) => println!("{name}: ok {}", reply.chars().take(40).collect::<String>()),
            Err(error) => {
                let chain: Vec<String> = error.chain().map(ToString::to_string).collect();
                println!("{name}: error {}", chain.join(" <- "));
            }
        }
    }
}

/// Which step writes in another language than they spoke: the first
/// `MEROPE_MEMORY_PROBE_SESSIONS` (default 4) sessions of the question
/// `MEROPE_MEMORY_BENCH_ONLY` in `MEROPE_MEMORY_BENCH`, each taken as short
/// notes and gone over the settle way, counting the lines with Chinese in
/// each: the notes, what was kept for good, and the notes kept as they were.
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn which_step_changes_the_language() {
    let path = std::env::var("MEROPE_MEMORY_BENCH").expect("MEROPE_MEMORY_BENCH");
    let id = std::env::var("MEROPE_MEMORY_BENCH_ONLY").expect("question id");
    let count: usize = std::env::var("MEROPE_MEMORY_PROBE_SESSIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4);
    let all: Vec<Question> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let question = all
        .into_iter()
        .find(|question| question.question_id == id)
        .expect("question");
    let config_db = super::super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("judgment model");
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "memory_probe").await;
    let db = isolated.db.clone();
    let user_id = new_user(&db, "probe").await;
    let present = Audience::private(user_id);
    let chinese = |line: &String| line.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    let mut sessions: Vec<(chrono::DateTime<chrono::FixedOffset>, &Vec<Turn>)> = question
        .haystack_dates
        .iter()
        .map(|date| date_of(date))
        .zip(question.haystack_sessions.iter())
        .collect();
    sessions.sort_by_key(|(at, _)| *at);
    for (at, turns) in sessions.into_iter().take(count) {
        let (notes, lasting, passed_over) =
            super::settle::over_time(&db, &judge, user_id, &present, at, turns).await;
        let facts: Vec<String> = lasting.into_iter().map(|(fact, _)| fact).collect();
        println!(
            "{at}: notes {}/{} chinese, lasting {}/{}, kept as noted {}/{}",
            notes.iter().filter(|line| chinese(line)).count(),
            notes.len(),
            facts.iter().filter(|line| chinese(line)).count(),
            facts.len(),
            passed_over.iter().filter(|line| chinese(line)).count(),
            passed_over.len(),
        );
        for line in notes.iter().chain(&facts).filter(|line| chinese(line)) {
            println!("  {line}");
        }
    }
    isolated.drop().await;
}

/// How strongly a question's words name each memory a run kept, by words
/// alone: `MEROPE_MEMORY_PROBE_REPORT` (a bench report), the questions in
/// `MEROPE_MEMORY_BENCH_ONLY`, and `MEROPE_MEMORY_PROBE_MARK` a word marking
/// the lines that hold the answer. Prints each named line's strength
/// against the strongest, in order, so a bar between what bears on it and
/// what shares a common word can be read off. Nothing is called.
#[test]
#[ignore = "reads a bench report given by path"]
fn how_strongly_words_name_what_was_kept() {
    use myriad_agent_rules::memory::lexical;
    let report: Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_PROBE_REPORT").expect("report"))
            .unwrap(),
    )
    .unwrap();
    let ids = std::env::var("MEROPE_MEMORY_BENCH_ONLY").expect("question ids");
    let marks = std::env::var("MEROPE_MEMORY_PROBE_MARK").unwrap_or_default();
    for row in report["rows"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        if !ids.split(',').any(|wanted| wanted == id) {
            continue;
        }
        let question = row["question"].as_str().unwrap();
        let kept: Vec<&str> = row["memories"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let documents: Vec<lexical::Document> = kept
            .iter()
            .map(|text| lexical::Document {
                text,
                concepts: &[],
            })
            .collect();
        let scores = lexical::score_all(question, &documents);
        let strongest = scores
            .iter()
            .filter(|score| score.strong)
            .map(|score| score.value)
            .fold(0.0, f64::max);
        let mut named: Vec<(f64, &str)> = scores
            .iter()
            .zip(&kept)
            .filter(|(score, _)| score.strong)
            .map(|(score, text)| (score.value / strongest, *text))
            .collect();
        named.sort_by(|left, right| right.0.total_cmp(&left.0));
        println!(
            "== {id} {question} ({} named of {})",
            named.len(),
            kept.len()
        );
        for (place, (strength, text)) in named.iter().enumerate().take(30) {
            let mark = marks
                .split(',')
                .any(|mark| !mark.is_empty() && text.to_lowercase().contains(&mark.to_lowercase()));
            println!(
                "{:>3} {strength:.2} {} {}",
                place + 1,
                if mark { "*" } else { " " },
                text.chars().take(90).collect::<String>()
            );
        }
    }
}
