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
