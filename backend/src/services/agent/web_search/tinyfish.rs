//! TinyFish Search + Fetch outbound. Host secret stays in headers only.

use super::mapping::{infer_search_locale, store_fetched_markdown, SearchHit};
use crate::services::agent::external_pure::classify_outbound_fetch;
use crate::services::agent::response_agent::api_key_not_configured;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

const SEARCH_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

const SEARCH_ENDPOINT: &str = "https://api.search.tinyfish.ai";
const FETCH_ENDPOINT: &str = "https://api.fetch.tinyfish.ai";
const SEARCH_BODY_MAX: usize = 256 * 1024;
const FETCH_BODY_MAX: usize = 2 * 1024 * 1024;
const ERROR_BODY_MAX: usize = 64 * 1024;
const FETCH_BATCH: usize = 10;

#[derive(Debug, Deserialize)]
struct TinyFishSearchResponse {
    #[serde(default)]
    results: Vec<TinyFishSearchResult>,
}

#[derive(Debug, Deserialize)]
struct TinyFishSearchResult {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    snippet: Option<String>,
    #[serde(default)]
    site_name: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TinyFishFetchResponse {
    #[serde(default)]
    results: Vec<TinyFishFetchResult>,
}

#[derive(Debug, Deserialize)]
struct TinyFishFetchResult {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    final_url: Option<String>,
    #[serde(default)]
    text: Option<Value>,
}

pub struct TinyFishSearchRequest<'a> {
    pub query: &'a str,
    pub purpose: Option<&'a str>,
    pub domain_type: Option<&'a str>,
    pub recency_minutes: Option<u32>,
    pub max_results: usize,
}

pub(crate) fn apply_search_query(url: &mut reqwest::Url, request: &TinyFishSearchRequest<'_>) {
    let mut pairs = url.query_pairs_mut();
    pairs.append_pair("query", request.query);
    if let Some(purpose) = request.purpose.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.append_pair("purpose", &purpose.chars().take(2000).collect::<String>());
    }
    // Language only. TinyFish auto-resolves location from language; Search
    // docs do not list CN/KR, and an invalid location is HTTP 400.
    let (language, _location) = infer_search_locale(request.query);
    if let Some(language) = language {
        pairs.append_pair("language", language);
    }
    if let Some(domain_type) = request.domain_type {
        pairs.append_pair("domain_type", domain_type);
    }
    if let Some(minutes) = request.recency_minutes.filter(|m| *m > 0) {
        pairs.append_pair("recency_minutes", &minutes.min(5_256_000).to_string());
    }
}

pub async fn search(
    api_key: &str,
    request: TinyFishSearchRequest<'_>,
) -> Result<Vec<SearchHit>, String> {
    let mut url = reqwest::Url::parse(SEARCH_ENDPOINT).map_err(|error| {
        tracing::error!(%error, "TinyFish search URL parse failed");
        "Web search failed".to_string()
    })?;
    apply_search_query(&mut url, &request);

    let client = crate::services::http_client::get_tinyfish_client().await;
    tracing::info!(query_len = request.query.len(), "Calling TinyFish Search");
    let response = client
        .get(url)
        .timeout(SEARCH_REQUEST_TIMEOUT)
        .header("X-API-Key", api_key)
        .send()
        .await
        .map_err(|error| {
            tracing::error!(%error, "TinyFish search request failed");
            classify_outbound_fetch("Web search failed", &error.to_string())
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(map_http_error("Web search failed", status, response).await);
    }

    let body_bytes =
        crate::services::outbound_security::read_limited_body(response, SEARCH_BODY_MAX)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "Failed to read TinyFish search response");
                "Web search failed".to_string()
            })?;
    let parsed: TinyFishSearchResponse = serde_json::from_slice(&body_bytes).map_err(|error| {
        tracing::error!(%error, "Failed to parse TinyFish search response");
        "Web search failed".to_string()
    })?;

    let mut hits = Vec::new();
    for item in parsed.results {
        let Some(hit) = SearchHit::from_search_fields(
            item.title,
            item.url,
            item.snippet,
            item.site_name,
            item.publisher,
            item.date,
        ) else {
            continue;
        };
        hits.push(hit);
        if hits.len() >= request.max_results.max(1) {
            break;
        }
    }
    Ok(hits)
}

pub async fn fetch_markdown(
    api_key: &str,
    urls: &[String],
    purpose: Option<&str>,
) -> HashMap<String, String> {
    let mut by_url = HashMap::new();
    if urls.is_empty() {
        return by_url;
    }
    let client = crate::services::http_client::get_tinyfish_client().await;
    for chunk in urls.chunks(FETCH_BATCH) {
        let mut body = serde_json::json!({
            "urls": chunk,
            "format": "markdown",
            "ttl": 3600
        });
        if let Some(purpose) = purpose.map(str::trim).filter(|s| !s.is_empty()) {
            body["purpose"] = serde_json::Value::String(purpose.to_string());
        }
        tracing::info!(urls = chunk.len(), "Calling TinyFish Fetch");
        let response = match client
            .post(FETCH_ENDPOINT)
            .header("X-API-Key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%error, "TinyFish fetch request failed");
                continue;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let _ = map_http_error("Web fetch failed", status, response).await;
            continue;
        }
        let body_bytes =
            match crate::services::outbound_security::read_limited_body(response, FETCH_BODY_MAX)
                .await
            {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::warn!(error = %error, "Failed to read TinyFish fetch response");
                    continue;
                }
            };
        let parsed: TinyFishFetchResponse = match serde_json::from_slice(&body_bytes) {
            Ok(parsed) => parsed,
            Err(error) => {
                tracing::warn!(%error, "Failed to parse TinyFish fetch response");
                continue;
            }
        };
        for page in parsed.results {
            let text = match page.text {
                Some(Value::String(text)) => text,
                Some(other) => other.to_string(),
                None => continue,
            };
            store_fetched_markdown(&mut by_url, page.url, page.final_url, &text);
        }
    }
    by_url
}

async fn map_http_error(label: &str, status: StatusCode, response: reqwest::Response) -> String {
    let error_bytes =
        crate::services::outbound_security::read_limited_body(response, ERROR_BODY_MAX)
            .await
            .unwrap_or_default();
    let error_text = String::from_utf8_lossy(&error_bytes);
    tracing::error!(status = %status, body = %error_text, "{label}");
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::PAYMENT_REQUIRED {
        return api_key_not_configured("TinyFish");
    }
    classify_outbound_fetch(label, &format!("HTTP {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query_pairs(
        request: TinyFishSearchRequest<'_>,
    ) -> std::collections::HashMap<String, String> {
        let mut url = reqwest::Url::parse(SEARCH_ENDPOINT).unwrap();
        apply_search_query(&mut url, &request);
        url.query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn base_request(query: &str) -> TinyFishSearchRequest<'_> {
        TinyFishSearchRequest {
            query,
            purpose: None,
            domain_type: None,
            recency_minutes: None,
            max_results: 5,
        }
    }

    #[test]
    fn chinese_query_sends_language_not_location() {
        let pairs = query_pairs(base_request("最新的 Rust 发布"));
        assert_eq!(pairs.get("language").map(String::as_str), Some("zh"));
        assert!(!pairs.contains_key("location"));
    }

    #[test]
    fn kanji_only_query_omits_language() {
        let pairs = query_pairs(base_request("東京都"));
        assert!(!pairs.contains_key("language"));
        assert!(!pairs.contains_key("location"));
    }

    #[test]
    fn latin_query_omits_language_and_location() {
        let pairs = query_pairs(base_request("rust release notes"));
        assert!(!pairs.contains_key("language"));
        assert!(!pairs.contains_key("location"));
    }

    #[test]
    fn news_recency_and_purpose_are_query_params() {
        let request = TinyFishSearchRequest {
            query: "openai",
            purpose: Some("Find recent news"),
            domain_type: Some("news"),
            recency_minutes: Some(10_080),
            max_results: 5,
        };
        let pairs = query_pairs(request);
        assert_eq!(pairs.get("domain_type").map(String::as_str), Some("news"));
        assert_eq!(
            pairs.get("recency_minutes").map(String::as_str),
            Some("10080")
        );
        assert_eq!(
            pairs.get("purpose").map(String::as_str),
            Some("Find recent news")
        );
    }

    #[test]
    fn zero_recency_is_omitted() {
        let request = TinyFishSearchRequest {
            query: "openai",
            purpose: None,
            domain_type: None,
            recency_minutes: Some(0),
            max_results: 5,
        };
        let pairs = query_pairs(request);
        assert!(!pairs.contains_key("recency_minutes"));
    }
}
