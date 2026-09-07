//! 设置徽章和 Brew 阅读共用的 GitHub 仓库摘要。
//!
//! 出站走数据平台同一套：[`GitHubApiUrl`]（可改 API 基址）+ 全局 HTTP 客户端
//! （代理）+ 可选 `github_token`。浏览器不再直打 api.github.com。

use crate::error::HttpError;
use crate::services::fetcher::{GithubRepoSummary, PlatformFetcher};
use crate::services::http_client::GitHubApiUrl;
use crate::GLOBAL_DYNAMIC_CONFIG;
use axum::extract::Query;
use axum::Json;
use myriad_error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const STAR_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const ERROR_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
pub struct RepoQuery {
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoResponse {
    pub stars: Option<i64>,
    pub forks: Option<i64>,
    pub description: Option<String>,
    pub language: Option<String>,
}

impl RepoResponse {
    fn empty() -> Self {
        Self {
            stars: None,
            forks: None,
            description: None,
            language: None,
        }
    }

    fn from_summary(summary: GithubRepoSummary) -> Self {
        Self {
            stars: Some(summary.stars),
            forks: Some(summary.forks),
            description: summary.description,
            language: summary.language,
        }
    }
}

struct CacheEntry {
    body: RepoResponse,
    ok: bool,
    fetched_at: Instant,
}

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(owner: &str, repo: &str) -> String {
    format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    )
}

/// GitHub owner / repo 段：字母数字开头结尾，中间可有 `.` `_` `-`。
pub fn valid_repo_segment(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 100 {
        return false;
    }
    if value.contains("..") {
        return false;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return false;
    }
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn read_cache(key: &str) -> Option<RepoResponse> {
    let Ok(guard) = cache().lock() else {
        return None;
    };
    let entry = guard.get(key)?;
    let ttl = if entry.ok { STAR_TTL } else { ERROR_TTL };
    if entry.fetched_at.elapsed() >= ttl {
        return None;
    }
    Some(entry.body.clone())
}

fn write_cache(key: String, body: RepoResponse, ok: bool) {
    let Ok(mut guard) = cache().lock() else {
        return;
    };
    guard.insert(
        key,
        CacheEntry {
            body,
            ok,
            fetched_at: Instant::now(),
        },
    );
}

pub async fn get_repo(Query(query): Query<RepoQuery>) -> Result<Json<RepoResponse>, HttpError> {
    let owner = query.owner.trim();
    let repo = query.repo.trim().trim_end_matches(".git");
    if !valid_repo_segment(owner) || !valid_repo_segment(repo) {
        return Err(HttpError(AppError::bad_request(
            "Invalid GitHub repository",
        )));
    }

    let key = cache_key(owner, repo);
    if let Some(body) = read_cache(&key) {
        return Ok(Json(body));
    }

    let token = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config
            .github_token
            .clone()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };

    let fetcher = PlatformFetcher::new().await;
    let (body, ok) = match fetcher
        .fetch_github_repo_summary(owner, repo, token.as_deref())
        .await
    {
        Ok(summary) => (RepoResponse::from_summary(summary), true),
        Err(error) => {
            let api_base = GitHubApiUrl::get_api_base().await;
            tracing::warn!(
                owner,
                repo,
                %error,
                %api_base,
                "GitHub repo summary fetch failed"
            );
            (RepoResponse::empty(), false)
        }
    };
    write_cache(key, body.clone(), ok);
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::fetcher::parse_github_repo_summary;
    use serde_json::json;

    #[test]
    fn valid_repo_segment_accepts_github_names() {
        assert!(valid_repo_segment("852wa"));
        assert!(valid_repo_segment("Anime2.5DRig"));
        assert!(valid_repo_segment("ollama"));
        assert!(!valid_repo_segment(""));
        assert!(!valid_repo_segment(".hidden"));
        assert!(!valid_repo_segment("foo/bar"));
        assert!(!valid_repo_segment(".."));
        assert!(!valid_repo_segment("foo..bar"));
    }

    #[test]
    fn parse_github_repo_summary_reads_card_fields() {
        let summary = parse_github_repo_summary(&json!({
            "stargazers_count": 12,
            "forks_count": 3,
            "description": "  layered portrait  ",
            "language": "TypeScript",
            "html_url": "https://github.com/example/repo",
        }))
        .expect("summary");
        assert_eq!(summary.stars, 12);
        assert_eq!(summary.forks, 3);
        assert_eq!(summary.description.as_deref(), Some("layered portrait"));
        assert_eq!(summary.language.as_deref(), Some("TypeScript"));
    }

    #[test]
    fn parse_github_repo_summary_requires_stars() {
        assert!(parse_github_repo_summary(&json!({ "forks_count": 1 })).is_err());
    }
}
