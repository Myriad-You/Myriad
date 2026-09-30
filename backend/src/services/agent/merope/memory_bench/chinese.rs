//! The Chinese memory set: that it reads as the bench needs, and her memory on it.

use super::*;

/// The Chinese set (tests/merope/memory-zh.json): her kind of chat, in the
/// shape the bench reads, every type covered, each answer's turn marked.
#[test]
pub(super) fn the_chinese_memory_set_reads_as_the_bench_needs() {
    let questions: Vec<Question> = serde_json::from_str(include_str!(
        "../../../../../../tests/merope/memory-zh.json"
    ))
    .unwrap();
    let types: std::collections::BTreeSet<&str> = questions
        .iter()
        .map(|question| question.question_type.as_str())
        .collect();
    assert_eq!(types.len(), 6);
    for question in &questions {
        assert_eq!(
            question.haystack_dates.len(),
            question.haystack_sessions.len()
        );
        for date in &question.haystack_dates {
            date_of(date);
        }
        let marked = question
            .haystack_sessions
            .iter()
            .flatten()
            .any(|turn| turn.has_answer);
        assert_eq!(
            marked,
            !question.question_id.ends_with("_abs"),
            "{}",
            question.question_id
        );
    }
}

/// Whether an embedding model finds what was said by what it means in
/// Chinese, as her conversations mostly are: everything the people of the
/// Chinese set said is one pool, and each question should bring its answer
/// lines into the first five, by meaning and by words. `MEROPE_MEMORY_BENCH=
/// tests/merope/memory-zh.json MEROPE_MEMORY_EMBEDDING_MODEL=<model>`.
#[tokio::test]
#[ignore = "spends on the site's embedding model; see the module docs"]
pub(super) async fn meaning_in_chinese() {
    const FIRST: usize = 5;
    let questions: Vec<Question> = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MEROPE_MEMORY_BENCH").expect("questions")).unwrap(),
    )
    .unwrap();
    let config_db = super::super::super::semantic_eval::load_configured_lite().await;
    config_db.close().await.ok();
    embedding_as_asked().await;
    let embedder = crate::services::ai::create_lite_embedding_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    .expect("an embedding model");
    let pool: Vec<(String, String, bool)> = questions
        .iter()
        .flat_map(|question| {
            question
                .haystack_sessions
                .iter()
                .flatten()
                .filter(|turn| turn.role == "user")
                .map(move |turn| {
                    (
                        question.question_id.clone(),
                        turn.content.clone(),
                        turn.has_answer,
                    )
                })
        })
        .collect();
    let lines: Vec<String> = pool.iter().map(|(_, line, _)| line.clone()).collect();
    let started = std::time::Instant::now();
    let vectors = embedder.embed(&lines).await.expect("pool embedded");
    let pool_ms = started.elapsed().as_millis();
    let asked: Vec<String> = questions.iter().map(|q| q.question.clone()).collect();
    let started = std::time::Instant::now();
    let asked_vectors = embedder.embed(&asked).await.expect("questions embedded");
    let asked_ms = started.elapsed().as_millis();
    let documents: Vec<crate::services::agent::memory::lexical::Document> = lines
        .iter()
        .map(|text| crate::services::agent::memory::lexical::Document {
            text,
            concepts: &[],
        })
        .collect();
    let (mut by_meaning, mut by_words, mut needed) = (0, 0, 0);
    for (question, asked) in questions.iter().zip(&asked_vectors) {
        let answers: Vec<usize> = pool
            .iter()
            .enumerate()
            .filter(|(_, (id, _, answer))| *id == question.question_id && *answer)
            .map(|(index, _)| index)
            .collect();
        if answers.is_empty() {
            continue;
        }
        let first = |scores: Vec<f64>| {
            let mut order: Vec<usize> = (0..scores.len()).collect();
            order.sort_by(|left, right| scores[*right].total_cmp(&scores[*left]));
            order.into_iter().take(FIRST).collect::<Vec<_>>()
        };
        let meaning = first(
            vectors
                .iter()
                .map(|vector| crate::services::agent::memory::meaning::cosine(asked, vector))
                .collect(),
        );
        let words = first(
            crate::services::agent::memory::lexical::score_all(&question.question, &documents)
                .into_iter()
                .map(|score| if score.strong { score.value } else { 0.0 })
                .collect(),
        );
        let found = |first: &[usize]| answers.iter().filter(|index| first.contains(index)).count();
        println!(
            "{} {} meaning {}/{} words {}/{}",
            question.question_type,
            question.question_id,
            found(&meaning),
            answers.len(),
            found(&words),
            answers.len()
        );
        by_meaning += found(&meaning);
        by_words += found(&words);
        needed += answers.len();
    }
    println!(
        "pool {} lines, {} dimensions; answer lines in the first {FIRST}: meaning {by_meaning}/{needed}, \
         words {by_words}/{needed}; embedding {pool_ms}ms for the pool, {asked_ms}ms for {} questions",
        lines.len(),
        vectors.first().map_or(0, Vec::len),
        asked.len()
    );
}
