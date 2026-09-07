//! Agent web search. TinyFish Search is preferred; Gemini grounding is fallback.
//!
//! Capability IDs stay `ai.webSearch` / `ai.groundingSearch`. Chat Lite does not
//! call this module.

mod gemini;
mod mapping;
mod tinyfish;

use crate::services::agent::ai_process_pure::sanitize_prompt_input;
use crate::services::agent::response_agent::api_key_not_configured;
use crate::services::agent::search_output::{capability_payload, normalize_search_type};
use crate::GLOBAL_DYNAMIC_CONFIG;
use mapping::{
    hits_to_reading_list, hits_to_web_search_results, purpose_for_search_type, summarize_hits,
    urls_needing_fetch, READING_LIST_FETCH_MAX,
};
use serde_json::Value;
use std::collections::HashMap;
use tinyfish::TinyFishSearchRequest;

pub async fn execute_capability(params: &HashMap<String, Value>) -> Result<Value, String> {
    let parsed = parse_web_search_params(params)?;
    let (ai_summary, results) = search_web(
        &parsed.query,
        parsed.search_type,
        parsed.max_results,
        parsed.search_prompt,
    )
    .await?;
    Ok(capability_payload(
        &parsed.query,
        parsed.search_type,
        ai_summary,
        results,
    ))
}

/// Tapp AI Task `operation: search` enters with a JSON value, not Agent params.
pub async fn execute_from_value(input: &Value) -> Result<Value, String> {
    let mut params = HashMap::new();
    match input {
        Value::String(query) => {
            params.insert("query".to_string(), Value::String(query.clone()));
        }
        Value::Object(object) => {
            for (key, value) in object {
                params.insert(key.clone(), value.clone());
            }
        }
        _ => return Err("search input must be a string or object".to_string()),
    }
    execute_capability(&params).await
}

struct ParsedWebSearch<'a> {
    query: String,
    search_type: &'static str,
    max_results: usize,
    search_prompt: Option<&'a str>,
}

fn parse_web_search_params(params: &HashMap<String, Value>) -> Result<ParsedWebSearch<'_>, String> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("Missing query parameter")?;
    let query = sanitize_prompt_input(query);
    if query.is_empty() {
        return Err("Missing query parameter".to_string());
    }
    Ok(ParsedWebSearch {
        query,
        search_type: normalize_search_type(
            params
                .get("searchType")
                .and_then(|v| v.as_str())
                .unwrap_or("general"),
        ),
        max_results: params
            .get("maxResults")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize,
        search_prompt: params.get("searchPrompt").and_then(|v| v.as_str()),
    })
}

pub async fn execute_reading_list(
    query: &str,
    max_items: usize,
    recency_minutes: Option<u32>,
) -> Result<Vec<Value>, String> {
    let query = sanitize_prompt_input(query);
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let (tinyfish_key, gemini) = search_providers().await;
    let gemini_available = gemini.is_some();

    if let Some(api_key) = tinyfish_key {
        let tinyfish =
            search_reading_list_tinyfish(&api_key, &query, max_items, recency_minutes).await;
        match take_tinyfish_seq(tinyfish, gemini_available)? {
            Some(items) => return Ok(items),
            None => {
                tracing::info!("TinyFish reading-list search missed; trying Gemini fallback");
            }
        }
    }

    if let Some((api_key, model)) = gemini {
        return gemini::search_reading_list(&api_key, &model, &query, max_items).await;
    }

    Err(no_search_provider())
}

async fn search_web(
    query: &str,
    search_type: &str,
    max_results: usize,
    search_prompt: Option<&str>,
) -> Result<(String, Vec<Value>), String> {
    let (tinyfish_key, gemini) = search_providers().await;
    let gemini_available = gemini.is_some();

    let safe_query = sanitize_prompt_input(query);
    if safe_query.is_empty() {
        return Err("Missing query parameter".to_string());
    }
    let max_results = max_results.clamp(1, 20);
    let purpose = purpose_for_search_type(search_type, search_prompt);

    if let Some(api_key) = tinyfish_key {
        let tinyfish = tinyfish::search(
            &api_key,
            TinyFishSearchRequest {
                query: &safe_query,
                purpose: purpose.as_deref(),
                domain_type: None,
                recency_minutes: None,
                max_results,
            },
        )
        .await;
        match take_tinyfish_seq(tinyfish, gemini_available)? {
            Some(hits) => {
                let results = hits_to_web_search_results(&hits, max_results);
                let summary = summarize_hits(&hits);
                return Ok((summary, results));
            }
            None => {
                tracing::info!("TinyFish Search missed; trying Gemini fallback");
            }
        }
    }

    if let Some((api_key, model)) = gemini {
        return gemini::search(&api_key, &model, &safe_query, search_type, max_results).await;
    }

    Err(no_search_provider())
}

fn no_search_provider() -> String {
    api_key_not_configured("web search")
}

async fn search_providers() -> (Option<String>, Option<(String, String)>) {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    (
        config.shared_tinyfish_api_key(),
        config.resolve_gemini_grounding(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterTinyFish {
    KeepHits,
    FallbackGemini,
    EmptyOk,
    Fail,
}

fn after_tinyfish(has_hits: bool, errored: bool, gemini_available: bool) -> AfterTinyFish {
    if has_hits {
        return AfterTinyFish::KeepHits;
    }
    if gemini_available {
        return AfterTinyFish::FallbackGemini;
    }
    if errored {
        return AfterTinyFish::Fail;
    }
    AfterTinyFish::EmptyOk
}

fn take_tinyfish<T>(
    result: Result<T, String>,
    is_empty: bool,
    gemini_available: bool,
) -> Result<Option<T>, String> {
    let has_hits = result.is_ok() && !is_empty;
    let errored = result.is_err();
    match after_tinyfish(has_hits, errored, gemini_available) {
        AfterTinyFish::KeepHits | AfterTinyFish::EmptyOk => Ok(Some(result?)),
        AfterTinyFish::FallbackGemini => Ok(None),
        AfterTinyFish::Fail => Err(result.err().unwrap_or_else(|| "Web search failed".into())),
    }
}

fn take_tinyfish_seq<T>(
    result: Result<Vec<T>, String>,
    gemini_available: bool,
) -> Result<Option<Vec<T>>, String> {
    let is_empty = result.as_ref().map(Vec::is_empty).unwrap_or(true);
    take_tinyfish(result, is_empty, gemini_available)
}

async fn search_reading_list_tinyfish(
    api_key: &str,
    query: &str,
    max_items: usize,
    recency_minutes: Option<u32>,
) -> Result<Vec<Value>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let max_items = max_items.clamp(1, 10);
    let hits = tinyfish::search(
        api_key,
        TinyFishSearchRequest {
            query,
            purpose: Some("Find recent news articles and long-form reporting for a reading list"),
            domain_type: Some("news"),
            recency_minutes,
            max_results: max_items,
        },
    )
    .await?;
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let urls = urls_needing_fetch(&hits, READING_LIST_FETCH_MAX.min(max_items));
    let fetched = if urls.is_empty() {
        HashMap::new()
    } else {
        tinyfish::fetch_markdown(
            api_key,
            &urls,
            Some("Extract the article body for a reading-list summary"),
        )
        .await
    };
    Ok(hits_to_reading_list(&hits, &fetched, max_items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tinyfish_hits_are_kept_even_when_gemini_exists() {
        assert_eq!(after_tinyfish(true, false, true), AfterTinyFish::KeepHits);
    }

    #[test]
    fn tinyfish_empty_falls_back_only_when_gemini_exists() {
        assert_eq!(
            after_tinyfish(false, false, true),
            AfterTinyFish::FallbackGemini
        );
        assert_eq!(after_tinyfish(false, false, false), AfterTinyFish::EmptyOk);
    }

    #[test]
    fn tinyfish_error_without_gemini_is_a_failure() {
        assert_eq!(after_tinyfish(false, true, false), AfterTinyFish::Fail);
        assert_eq!(
            after_tinyfish(false, true, true),
            AfterTinyFish::FallbackGemini
        );
    }

    #[test]
    fn take_tinyfish_keeps_empty_when_gemini_is_absent() {
        let kept = take_tinyfish(Ok(Vec::<i32>::new()), true, false).unwrap();
        assert_eq!(kept, Some(Vec::new()));
    }

    #[test]
    fn take_tinyfish_falls_back_when_gemini_exists() {
        let missed = take_tinyfish(Ok(Vec::<i32>::new()), true, true).unwrap();
        assert_eq!(missed, None);
        let failed = take_tinyfish::<Vec<i32>>(Err("boom".into()), true, true).unwrap();
        assert_eq!(failed, None);
    }

    #[test]
    fn take_tinyfish_fail_propagates_the_upstream_error() {
        let err = take_tinyfish::<Vec<i32>>(Err("boom".into()), true, false).unwrap_err();
        assert_eq!(err, "boom");
    }

    #[test]
    fn parse_web_search_params_sanitizes_query_and_clamps_type() {
        let mut params = HashMap::new();
        params.insert("query".into(), json!("  rust\u{0000}lang  "));
        params.insert("searchType".into(), json!("nope"));
        params.insert("maxResults".into(), json!(3));
        let parsed = parse_web_search_params(&params).unwrap();
        assert_eq!(parsed.query, "rustlang");
        assert_eq!(parsed.search_type, "general");
        assert_eq!(parsed.max_results, 3);

        let mut params = HashMap::new();
        params.insert("query".into(), json!("NHK"));
        params.insert("searchType".into(), json!("rss_source"));
        let parsed = parse_web_search_params(&params).unwrap();
        assert_eq!(parsed.search_type, "rss_source");

        let empty = HashMap::new();
        assert!(parse_web_search_params(&empty).is_err());
        let mut params = HashMap::new();
        params.insert("query".into(), json!(" \u{0001} "));
        assert!(parse_web_search_params(&params).is_err());
    }

    #[test]
    fn take_tinyfish_seq_uses_vec_emptiness() {
        let kept = take_tinyfish_seq(Ok(vec![1]), false).unwrap();
        assert_eq!(kept, Some(vec![1]));
        let empty_ok = take_tinyfish_seq(Ok(Vec::<i32>::new()), false).unwrap();
        assert_eq!(empty_ok, Some(Vec::new()));
        let fallback = take_tinyfish_seq(Ok(Vec::<i32>::new()), true).unwrap();
        assert_eq!(fallback, None);
    }
}
