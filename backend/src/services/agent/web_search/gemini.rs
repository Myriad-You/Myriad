//! Gemini Google Search grounding fallback.

use super::mapping::{
    clean_search_result_url, extract_domain_from_url, is_http_url, normalize_reading_list_item,
};
use crate::services::agent::ai_process_pure::sanitize_prompt_input;
use crate::services::agent::data_read_pure::extract_json_array_from_ai_response;
use serde_json::{json, Value};
use std::collections::HashMap;

struct GroundingGen {
    temperature: f64,
    max_output_tokens: u32,
    json_mime: bool,
}

pub async fn search(
    api_key: &str,
    model: &str,
    query: &str,
    search_type: &str,
    max_results: usize,
) -> Result<(String, Vec<Value>), String> {
    let safe_query = sanitize_prompt_input(query);
    let prompt = grounding_search_prompt(search_type, &safe_query, max_results);

    tracing::info!(
        "Calling Gemini Grounding Search for query length={}",
        safe_query.len()
    );
    let (ai_text, response_json) = grounding_generate(
        api_key,
        model,
        &prompt,
        GroundingGen {
            temperature: 0.1,
            max_output_tokens: 2048,
            json_mime: false,
        },
    )
    .await?;

    let mut results = extract_json_array_from_ai_response(&ai_text);
    if results.is_empty() {
        for (uri, title) in grounding_web_pages(&response_json) {
            if results.len() >= max_results {
                break;
            }
            let url = clean_search_result_url(&uri);
            if !is_http_url(&url) {
                continue;
            }
            results.push(json!({
                "name": title,
                "url": url,
                "description": format!("Source: {}", title),
                "source": "google_search"
            }));
        }
    }

    if results.is_empty() && !ai_text.is_empty() {
        results.push(json!({
            "name": "Search results",
            "description": ai_text,
            "source": "gemini_grounding"
        }));
    }

    Ok((ai_text, results))
}

pub async fn search_reading_list(
    api_key: &str,
    model: &str,
    query: &str,
    max_items: usize,
) -> Result<Vec<Value>, String> {
    let query = sanitize_prompt_input(query);
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let search_prompt = grounding_reading_list_prompt(&query, max_items);

    tracing::info!(
        query_len = query.len(),
        "[AI Web Search] Searching for reading list content"
    );
    let (ai_text, response_json) = grounding_generate(
        api_key,
        model,
        &search_prompt,
        GroundingGen {
            temperature: 0.2,
            max_output_tokens: 8192,
            json_mime: true,
        },
    )
    .await?;

    let mut results = extract_json_array_from_ai_response(&ai_text);
    if results.is_empty() {
        tracing::info!("[AI Web Search] Trying to extract from grounding metadata");
        let snippets = grounding_support_snippets(&response_json);
        for (idx, (uri, title)) in grounding_web_pages(&response_json)
            .into_iter()
            .enumerate()
            .take(max_items)
        {
            let title = if title.is_empty() {
                "Untitled".to_string()
            } else {
                title
            };
            let summary = snippets
                .get(&idx.to_string())
                .map(|parts| parts.join(" "))
                .filter(|s| s.len() > 20)
                .unwrap_or_else(|| {
                    format!("Article from {}: {}", extract_domain_from_url(&uri), &title)
                });
            results.push(json!({
                "title": title,
                "link": uri,
                "summary": summary,
                "sourceName": extract_domain_from_url(&uri),
            }));
        }
        tracing::info!(
            results = results.len(),
            "[AI Web Search] Extracted from grounding metadata"
        );
    }

    let now = chrono::Utc::now().to_rfc3339();
    let results: Vec<Value> = results
        .into_iter()
        .enumerate()
        .filter_map(|(idx, item)| normalize_reading_list_item(item, idx, &now, "AI web search"))
        .take(max_items)
        .collect();

    tracing::info!(results = results.len(), "[AI Web Search] Search completed");
    Ok(results)
}

async fn grounding_generate(
    api_key: &str,
    model: &str,
    prompt: &str,
    gen: GroundingGen,
) -> Result<(String, Value), String> {
    let mut generation_config = json!({
        "temperature": gen.temperature,
        "maxOutputTokens": gen.max_output_tokens,
    });
    if gen.json_mime {
        generation_config["responseMimeType"] = json!("application/json");
    }
    let request_body = json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "tools": [{
            "google_search": {}
        }],
        "generationConfig": generation_config
    });

    let url = crate::services::http_client::GeminiApiUrl::generate_content_url(model).await;
    let client = crate::services::http_client::get_gemini_grounding_client().await;
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("x-goog-api-key", api_key)
        .json(&request_body)
        .send()
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "Gemini API request failed");
            "AI generation failed".to_string()
        })?;

    const GEMINI_MAX_BODY: usize = 2 * 1024 * 1024;
    if !response.status().is_success() {
        let status = response.status();
        let error_bytes =
            crate::services::outbound_security::read_limited_body(response, 64 * 1024)
                .await
                .unwrap_or_default();
        let error_text = String::from_utf8_lossy(&error_bytes);
        tracing::error!(status = %status, body = %error_text, "Gemini API error");
        return Err("AI generation failed".to_string());
    }

    let body_bytes =
        crate::services::outbound_security::read_limited_body(response, GEMINI_MAX_BODY)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "Failed to read Gemini response");
                "AI generation failed".to_string()
            })?;
    let response_json: Value = serde_json::from_slice(&body_bytes).map_err(|error| {
        tracing::error!(error = %error, "Failed to parse Gemini response");
        "AI generation failed".to_string()
    })?;

    Ok((extract_ai_text(&response_json).to_string(), response_json))
}

fn grounding_reading_list_prompt(query: &str, max_items: usize) -> String {
    format!(
        r#"You are a reading assistant. The user wants articles about "{query}".

Task: use Google Search to find related news/articles, then return a reading list.

Output:
1. Return {max_items} most relevant articles
2. Pure JSON array only — no extra prose, no markdown
3. Each item must have:
   - "id": number starting at 1
   - "title": full article title (string, do not truncate)
   - "link": the real article URL (must be the article page, not a Google result or redirect; must start with https:// or http://)
   - "summary": 150–300 characters covering the main points so the reader need not open the page
   - "sourceName": site name (string)
   - "author": author (string; "" if unknown)
   - "publishedAt": ISO 8601 datetime (string, e.g. "2026-01-10T12:00:00Z")
   - "relevanceReason": one sentence on why it is worth reading

Write title, summary, and relevanceReason in the same language as the query.

Selection:
- Prefer reputable publishers
- Must be highly relevant to "{query}"
- Prefer recent pieces
- Skip paywalls and login walls
- Skip aggregator/search pages; article pages only

link field:
- Must be a directly openable article URL
- No Google AMP (google.com/amp/...)
- No search-result wrappers (google.com/url?...)
- Tracking params may be dropped; keep the main path

Example:
[{{"id":1,"title":"Full article title","link":"https://www.example.com/news/article-123","summary":"This article covers... (150–300 chars)","sourceName":"Example News","author":"Jane Doe","publishedAt":"2026-01-10T12:00:00Z","relevanceReason":"why it is relevant"}}]

Search now and return the JSON array:"#,
        query = query,
        max_items = max_items
    )
}

fn grounding_search_prompt(search_type: &str, query: &str, max_results: usize) -> String {
    match search_type {
        "rss_source" => format!(
            "搜索「{}」的 RSS 或 Atom 订阅源地址。\n\
            要求：\n\
            1. 返回可直接访问的 RSS/Atom feed URL\n\
            2. 优先返回官方 RSS 源\n\
            3. 也可以返回 RSSHub (rsshub.app) 提供的路由\n\
            4. 最多返回 {} 个结果\n\n\
            请以 JSON 数组格式返回，每个元素包含：\n\
            - name: 源名称\n\
            - url: RSS/Atom feed URL\n\
            - description: 简要说明\n\
            - source: 来源（official/rsshub/third-party）",
            query, max_results
        ),
        "api_docs" => format!(
            "搜索「{}」的官方 API 文档链接。最多返回 {} 个结果。\n\
            以 JSON 数组格式返回，每个元素包含：name, url, description",
            query, max_results
        ),
        _ => format!(
            "搜索关于「{}」的信息，最多返回 {} 个相关结果。\n\
            以 JSON 数组格式返回结果。",
            query, max_results
        ),
    }
}

fn first_candidate(response: &Value) -> Option<&Value> {
    response
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
}

fn extract_ai_text(response: &Value) -> &str {
    first_candidate(response)
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
}

fn grounding_metadata(response: &Value) -> Option<&Value> {
    first_candidate(response).and_then(|c| c.get("groundingMetadata"))
}

fn grounding_web_pages(response: &Value) -> Vec<(String, String)> {
    let Some(chunks) = grounding_metadata(response)
        .and_then(|m| m.get("groundingChunks"))
        .and_then(|c| c.as_array())
    else {
        return Vec::new();
    };
    chunks
        .iter()
        .filter_map(|chunk| {
            let web = chunk.get("web")?;
            let uri = web.get("uri").and_then(|u| u.as_str()).unwrap_or("");
            if uri.is_empty() {
                return None;
            }
            let title = web.get("title").and_then(|t| t.as_str()).unwrap_or("");
            Some((uri.to_string(), title.to_string()))
        })
        .collect()
}

fn grounding_support_snippets(response: &Value) -> HashMap<String, Vec<String>> {
    let mut snippets = HashMap::new();
    let Some(supports) = grounding_metadata(response)
        .and_then(|m| m.get("groundingSupports"))
        .and_then(|s| s.as_array())
    else {
        return snippets;
    };
    for support in supports {
        let Some(segment) = support
            .get("segment")
            .and_then(|s| s.get("text"))
            .and_then(|t| t.as_str())
        else {
            continue;
        };
        let Some(indices) = support
            .get("groundingChunkIndices")
            .and_then(|i| i.as_array())
        else {
            continue;
        };
        for idx in indices {
            if let Some(idx_num) = idx.as_u64() {
                snippets
                    .entry(idx_num.to_string())
                    .or_default()
                    .push(segment.to_string());
            }
        }
    }
    snippets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_grounding() -> Value {
        json!({
            "candidates": [{
                "content": { "parts": [{ "text": "found it" }] },
                "groundingMetadata": {
                    "groundingChunks": [
                        { "web": { "uri": "https://a.example/post", "title": "A" } },
                        { "web": { "uri": "https://b.example/post", "title": "B" } },
                        { "web": { "uri": "", "title": "skip" } }
                    ],
                    "groundingSupports": [{
                        "segment": { "text": "A long enough snippet for the first article." },
                        "groundingChunkIndices": [0]
                    }]
                }
            }]
        })
    }

    #[test]
    fn extract_ai_text_reads_first_candidate() {
        assert_eq!(extract_ai_text(&sample_grounding()), "found it");
        assert_eq!(extract_ai_text(&json!({})), "");
    }

    #[test]
    fn grounding_web_pages_skips_empty_uri() {
        let pages = grounding_web_pages(&sample_grounding());
        assert_eq!(
            pages,
            vec![
                ("https://a.example/post".into(), "A".into()),
                ("https://b.example/post".into(), "B".into()),
            ]
        );
    }

    #[test]
    fn grounding_support_snippets_group_by_chunk() {
        let snippets = grounding_support_snippets(&sample_grounding());
        assert_eq!(
            snippets.get("0").map(Vec::as_slice),
            Some(["A long enough snippet for the first article.".to_string()].as_slice())
        );
        assert!(snippets.get("1").is_none());
    }

    #[test]
    fn search_prompt_mentions_query_and_rsshub() {
        let rss = grounding_search_prompt("rss_source", "NHK", 3);
        assert!(rss.contains("NHK"));
        assert!(rss.contains("RSSHub"));
        let general = grounding_search_prompt("general", "rustc", 5);
        assert!(general.contains("rustc"));
        assert!(!general.contains("RSSHub"));
    }

    #[test]
    fn reading_list_prompt_mentions_query_and_count() {
        let prompt = grounding_reading_list_prompt("openai", 4);
        assert!(prompt.contains("openai"));
        assert!(prompt.contains('4'));
    }
}
