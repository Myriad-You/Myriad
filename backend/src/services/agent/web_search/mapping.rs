//! Pure mapping for web-search hits. No I/O.

use serde_json::{json, Value};
use std::collections::HashMap;

pub const READING_LIST_SUMMARY_CHARS: usize = 400;
/// Skip TinyFish Fetch when the search snippet is already a usable summary.
pub const FETCH_SNIPPET_SKIP_CHARS: usize = 120;
/// Cap how many destination pages a reading-list search will Fetch.
pub const READING_LIST_FETCH_MAX: usize = 5;
/// Keep only enough fetched markdown for a summary (not the full page).
pub const FETCH_STORE_CHARS: usize = 800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub site_name: String,
    pub date: Option<String>,
}

impl SearchHit {
    pub fn from_search_fields(
        title: Option<String>,
        url: Option<String>,
        snippet: Option<String>,
        site_name: Option<String>,
        publisher: Option<String>,
        date: Option<String>,
    ) -> Option<Self> {
        let url = url
            .as_deref()
            .map(clean_search_result_url)
            .filter(|u| is_http_url(u))?;
        let title = title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Untitled")
            .to_string();
        let site_name = site_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or(publisher)
            .unwrap_or_else(|| extract_domain_from_url(&url));
        Some(Self {
            title,
            url,
            snippet: snippet.unwrap_or_default(),
            site_name,
            date,
        })
    }
}

pub fn store_fetched_markdown(
    by_url: &mut HashMap<String, String>,
    url: Option<String>,
    final_url: Option<String>,
    text: &str,
) {
    if text.trim().is_empty() {
        return;
    }
    let stored = truncate_summary(text, FETCH_STORE_CHARS);
    for key in [url, final_url].into_iter().flatten() {
        if !key.is_empty() {
            by_url.insert(key, stored.clone());
        }
    }
}

pub fn infer_search_locale(query: &str) -> (Option<&'static str>, Option<&'static str>) {
    if query.chars().any(is_kana) {
        return (Some("ja"), Some("JP"));
    }
    if query
        .chars()
        .any(|c| ('\u{ac00}'..='\u{d7af}').contains(&c))
    {
        return (Some("ko"), Some("KR"));
    }
    // Han-only Japanese (東京都) shares CJK with Chinese. Do not default Han
    // to zh. Send zh only when Chinese function characters are present.
    if query.chars().any(is_cjk) && query.chars().any(is_chinese_particle) {
        return (Some("zh"), Some("CN"));
    }
    (None, None)
}

fn is_kana(c: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&c)
        || ('\u{31f0}'..='\u{31ff}').contains(&c)
        || c == '々'
        || c == 'ヶ'
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{f900}'..='\u{faff}').contains(&c)
}

fn is_chinese_particle(c: char) -> bool {
    matches!(c, '的' | '了' | '吗' | '呢' | '吧' | '这' | '那')
}

pub fn purpose_for_search_type(search_type: &str, search_prompt: Option<&str>) -> Option<String> {
    let custom = search_prompt.map(str::trim).filter(|s| !s.is_empty());
    if let Some(prompt) = custom {
        return Some(prompt.to_string());
    }
    match search_type {
        "rss_source" => {
            Some("Find official RSS or Atom feed URLs, including RSSHub routes".to_string())
        }
        "api_docs" => Some("Find official API documentation".to_string()),
        _ => None,
    }
}

pub fn is_http_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("https://") || trimmed.starts_with("http://")
}

pub fn extract_domain_from_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

/// Strip Google redirect / AMP / tracking params from a result URL.
pub fn clean_search_result_url(url: &str) -> String {
    let url = url.trim();

    if url.contains("google.com/url?") {
        if let Some(start) = url.find("url=").or_else(|| url.find("q=")) {
            let param_start = start
                + if url[start..].starts_with("url=") {
                    4
                } else {
                    2
                };
            let param_value = &url[param_start..];
            let end = param_value.find('&').unwrap_or(param_value.len());
            let decoded = urlencoding::decode(&param_value[..end]).unwrap_or_default();
            if decoded.starts_with("http") {
                return decoded.to_string();
            }
        }
        return String::new();
    }

    if url.contains("google.com/amp/") || url.contains("/amp/s/") {
        if let Some(amp_pos) = url.find("/amp/s/").or_else(|| url.find("google.com/amp/")) {
            let clean_start = if url[amp_pos..].starts_with("/amp/s/") {
                amp_pos + 7
            } else if let Some(pos) = url[amp_pos..].find("/amp/") {
                amp_pos + pos + 5
            } else {
                return String::new();
            };
            let cleaned = &url[clean_start..];
            if cleaned.starts_with("http") {
                return cleaned.to_string();
            }
            return format!("https://{cleaned}");
        }
        return String::new();
    }

    if url.contains("webcache.googleusercontent.com") {
        return String::new();
    }

    if let Some(query_start) = url.find('?') {
        let base_url = &url[..query_start];
        let query = &url[query_start + 1..];
        let tracking_params = [
            "utm_source",
            "utm_medium",
            "utm_campaign",
            "utm_content",
            "utm_term",
            "fbclid",
            "gclid",
            "mc_cid",
            "mc_eid",
        ];
        let clean_params: Vec<&str> = query
            .split('&')
            .filter(|param| {
                let key = param.split('=').next().unwrap_or("");
                !tracking_params.contains(&key)
            })
            .collect();
        if clean_params.is_empty() {
            return base_url.to_string();
        }
        return format!("{}?{}", base_url, clean_params.join("&"));
    }

    url.to_string()
}

pub fn summarize_hits(hits: &[SearchHit]) -> String {
    hits.iter()
        .enumerate()
        .map(|(index, hit)| {
            let snippet = if hit.snippet.is_empty() {
                String::new()
            } else {
                format!("\n{}", hit.snippet)
            };
            format!(
                "{}. {} — {}{}",
                index + 1,
                hit.title,
                hit.site_name,
                snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn hits_to_web_search_results(hits: &[SearchHit], max: usize) -> Vec<Value> {
    hits.iter()
        .filter(|hit| is_http_url(&hit.url))
        .take(max)
        .map(|hit| {
            json!({
                "name": hit.title,
                "url": hit.url,
                "description": if hit.snippet.is_empty() {
                    format!("来源: {}", hit.site_name)
                } else {
                    hit.snippet.clone()
                },
                "source": "tinyfish",
                "siteName": hit.site_name,
            })
        })
        .collect()
}

pub fn truncate_summary(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut summary: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    summary.push('…');
    summary
}

pub fn urls_needing_fetch(hits: &[SearchHit], max: usize) -> Vec<String> {
    hits.iter()
        .filter(|hit| is_http_url(&hit.url) && hit.snippet.trim().len() < FETCH_SNIPPET_SKIP_CHARS)
        .take(max)
        .map(|hit| hit.url.clone())
        .collect()
}

pub fn lookup_fetched<'a>(fetched: &'a HashMap<String, String>, url: &str) -> &'a str {
    if let Some(text) = fetched.get(url) {
        return text;
    }
    let trimmed = url.trim_end_matches('/');
    if let Some(text) = fetched.get(trimmed) {
        return text;
    }
    let with_slash = format!("{trimmed}/");
    fetched.get(&with_slash).map(String::as_str).unwrap_or("")
}

pub fn hits_to_reading_list(
    hits: &[SearchHit],
    fetched: &HashMap<String, String>,
    max: usize,
) -> Vec<Value> {
    hits.iter()
        .filter(|hit| is_http_url(&hit.url))
        .take(max)
        .enumerate()
        .filter_map(|(index, hit)| {
            let fetched_text = lookup_fetched(fetched, &hit.url);
            let summary = if fetched_text.trim().len() >= 30 {
                truncate_summary(fetched_text, READING_LIST_SUMMARY_CHARS)
            } else if hit.snippet.trim().len() >= 30 {
                hit.snippet.clone()
            } else {
                String::new()
            };
            normalize_reading_list_item(
                json!({
                    "id": index + 1,
                    "title": hit.title,
                    "link": hit.url,
                    "summary": summary,
                    "sourceName": if hit.site_name.is_empty() {
                        extract_domain_from_url(&hit.url)
                    } else {
                        hit.site_name.clone()
                    },
                    "author": "",
                    "publishedAt": hit.date.clone().unwrap_or_default(),
                    "relevanceReason": "联网搜索结果",
                }),
                index,
                "",
                "联网搜索结果",
            )
        })
        .collect()
}

/// Fill reading-list article defaults. `now` is only used when `publishedAt` is empty.
pub fn normalize_reading_list_item(
    mut item: Value,
    idx: usize,
    now: &str,
    default_reason: &str,
) -> Option<Value> {
    let id = item
        .get("id")
        .and_then(|v| v.as_i64())
        .unwrap_or((idx + 1) as i64);
    item["id"] = json!(id);

    if json_str(&item, "title").is_empty() {
        item["title"] = json!("未知标题");
    }

    let link = json_str(&item, "link").to_string();
    if link.is_empty() {
        return None;
    }
    let cleaned_link = clean_search_result_url(&link);
    if !is_http_url(&cleaned_link) {
        return None;
    }
    item["link"] = json!(cleaned_link);

    if json_str(&item, "sourceName").is_empty() {
        item["sourceName"] = json!(extract_domain_from_url(&cleaned_link));
    }

    let summary_too_short = {
        let summary = json_str(&item, "summary");
        summary.is_empty() || summary.len() < 30
    };
    if summary_too_short {
        let title = json_str(&item, "title").to_string();
        let source = json_str(&item, "sourceName").to_string();
        item["summary"] = json!(article_summary_placeholder(&source, &title));
    }

    if json_str(&item, "publishedAt").is_empty() && !now.is_empty() {
        item["publishedAt"] = json!(now);
    }
    if item.get("author").is_none() {
        item["author"] = json!("");
    }
    if json_str(&item, "relevanceReason").is_empty() {
        item["relevanceReason"] = json!(default_reason);
    }
    item["fromWebSearch"] = json!(true);
    Some(item)
}

fn json_str<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn article_summary_placeholder(source: &str, title: &str) -> String {
    format!(
        "这是一篇来自 {} 的文章：{}。点击阅读原文获取完整内容。",
        source, title
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_detects_cjk_and_kana() {
        assert_eq!(
            infer_search_locale("最新的 Rust 发布"),
            (Some("zh"), Some("CN"))
        );
        assert_eq!(infer_search_locale("最新 Rust 发布"), (None, None));
        assert_eq!(infer_search_locale("東京都"), (None, None));
        assert_eq!(
            infer_search_locale("最新のニュース"),
            (Some("ja"), Some("JP"))
        );
        assert_eq!(infer_search_locale("今日々"), (Some("ja"), Some("JP")));
        assert_eq!(infer_search_locale("오늘의 뉴스"), (Some("ko"), Some("KR")));
        assert_eq!(infer_search_locale("rust release notes"), (None, None));
    }

    #[test]
    fn purpose_uses_search_type_and_custom_prompt() {
        assert!(purpose_for_search_type("rss_source", None)
            .unwrap()
            .contains("RSS"));
        assert_eq!(
            purpose_for_search_type("general", Some("  Find invoices  ")).as_deref(),
            Some("Find invoices")
        );
        assert_eq!(purpose_for_search_type("general", None), None);
    }

    #[test]
    fn mapping_preserves_http_results_and_skips_empty_urls() {
        let hits = vec![
            SearchHit {
                title: "Rust".into(),
                url: "https://www.rust-lang.org/".into(),
                snippet: "A language".into(),
                site_name: "rust-lang.org".into(),
                date: None,
            },
            SearchHit {
                title: "Nope".into(),
                url: String::new(),
                snippet: String::new(),
                site_name: String::new(),
                date: None,
            },
        ];
        let results = hits_to_web_search_results(&hits, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], "Rust");
        assert_eq!(results[0]["source"], "tinyfish");
        assert_eq!(results[0]["url"], "https://www.rust-lang.org/");
    }

    #[test]
    fn reading_list_prefers_fetched_text() {
        let hits = vec![SearchHit {
            title: "Article".into(),
            url: "https://example.com/a".into(),
            snippet: "short".into(),
            site_name: "example.com".into(),
            date: Some("2026-09-01".into()),
        }];
        let mut fetched = HashMap::new();
        fetched.insert(
            "https://example.com/a".into(),
            "This is a long enough fetched body for a reading-list summary.".into(),
        );
        let items = hits_to_reading_list(&hits, &fetched, 3);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["link"], "https://example.com/a");
        assert!(items[0]["summary"]
            .as_str()
            .unwrap()
            .contains("fetched body"));
        assert_eq!(items[0]["fromWebSearch"], true);
    }

    #[test]
    fn lookup_fetched_tolerates_trailing_slash() {
        let mut fetched = HashMap::new();
        fetched.insert(
            "https://example.com/a/".into(),
            "Fetched body that is long enough to use.".into(),
        );
        assert!(lookup_fetched(&fetched, "https://example.com/a").contains("Fetched body"));
        assert!(lookup_fetched(&fetched, "https://example.com/a/").contains("Fetched body"));
        assert!(lookup_fetched(&fetched, "https://example.com/missing").is_empty());
    }

    #[test]
    fn urls_needing_fetch_skips_long_snippets_and_caps() {
        let hits = vec![
            SearchHit {
                title: "Short".into(),
                url: "https://example.com/short".into(),
                snippet: "too short".into(),
                site_name: "example.com".into(),
                date: None,
            },
            SearchHit {
                title: "Long".into(),
                url: "https://example.com/long".into(),
                snippet: "n".repeat(FETCH_SNIPPET_SKIP_CHARS),
                site_name: "example.com".into(),
                date: None,
            },
            SearchHit {
                title: "Also short".into(),
                url: "https://example.com/short-2".into(),
                snippet: "nope".into(),
                site_name: "example.com".into(),
                date: None,
            },
        ];
        let urls = urls_needing_fetch(&hits, 1);
        assert_eq!(urls, vec!["https://example.com/short".to_string()]);
        let urls = urls_needing_fetch(&hits, 5);
        assert_eq!(
            urls,
            vec![
                "https://example.com/short".to_string(),
                "https://example.com/short-2".to_string()
            ]
        );
    }

    #[test]
    fn clean_url_strips_google_redirect_and_utm() {
        let redirected = clean_search_result_url(
            "https://www.google.com/url?q=https%3A%2F%2Fexample.com%2Fpost&sa=U",
        );
        assert_eq!(redirected, "https://example.com/post");
        let tracked = clean_search_result_url("https://example.com/a?utm_source=x&id=1");
        assert_eq!(tracked, "https://example.com/a?id=1");
        let kept = clean_search_result_url("https://example.com/a?source=rss&ref=home");
        assert_eq!(kept, "https://example.com/a?source=rss&ref=home");
    }

    #[test]
    fn normalize_reading_list_item_fills_defaults_and_drops_bad_links() {
        let ok = normalize_reading_list_item(
            json!({
                "title": "",
                "link": "https://www.google.com/url?q=https%3A%2F%2Fexample.com%2Fpost&sa=U",
                "summary": "short"
            }),
            2,
            "2026-09-05T00:00:00Z",
            "AI 联网搜索推荐",
        )
        .expect("cleaned google redirect");
        assert_eq!(ok["id"], 3);
        assert_eq!(ok["title"], "未知标题");
        assert_eq!(ok["link"], "https://example.com/post");
        assert_eq!(ok["sourceName"], "example.com");
        assert_eq!(ok["publishedAt"], "2026-09-05T00:00:00Z");
        assert_eq!(ok["relevanceReason"], "AI 联网搜索推荐");
        assert_eq!(ok["fromWebSearch"], true);
        assert!(ok["summary"].as_str().unwrap().contains("example.com"));

        assert!(
            normalize_reading_list_item(json!({ "title": "x", "link": "" }), 0, "", "r").is_none()
        );
        assert!(normalize_reading_list_item(
            json!({
                "title": "x",
                "link": "https://webcache.googleusercontent.com/search?q=cache:abc"
            }),
            0,
            "",
            "r"
        )
        .is_none());
    }

    #[test]
    fn search_hit_from_fields_cleans_url_and_fills_site() {
        let hit = SearchHit::from_search_fields(
            Some("  Rust  ".into()),
            Some("https://www.google.com/url?q=https%3A%2F%2Fexample.com%2Fpost&sa=U".into()),
            Some("snippet".into()),
            None,
            Some("Example".into()),
            Some("2026-09-01".into()),
        )
        .expect("http url");
        assert_eq!(hit.title, "Rust");
        assert_eq!(hit.url, "https://example.com/post");
        assert_eq!(hit.site_name, "Example");
        assert_eq!(hit.date.as_deref(), Some("2026-09-01"));
        assert!(SearchHit::from_search_fields(None, None, None, None, None, None).is_none());
    }

    #[test]
    fn store_fetched_markdown_indexes_url_and_final_url() {
        let mut by_url = HashMap::new();
        store_fetched_markdown(
            &mut by_url,
            Some("https://example.com/a".into()),
            Some("https://example.com/a/".into()),
            "Fetched body that is long enough to keep.",
        );
        assert!(by_url
            .get("https://example.com/a")
            .unwrap()
            .contains("Fetched body"));
        assert_eq!(
            by_url.get("https://example.com/a"),
            by_url.get("https://example.com/a/")
        );
        store_fetched_markdown(&mut by_url, Some("https://skip".into()), None, "   ");
        assert!(!by_url.contains_key("https://skip"));
    }
}
