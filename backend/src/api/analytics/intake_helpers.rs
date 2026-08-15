use crate::config::DynamicConfig;
use axum::{
    extract::{ConnectInfo, Request},
    http::{header, StatusCode},
    Json,
};
use chrono::{Duration, Local, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value as SeaValue};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};
use tokio::sync::Mutex;
use tokio::sync::RwLock;

pub(crate) const SITE_PATH: &str = "__site__";
const MAX_PATH_LEN: usize = 128;
const MAX_EVENT_NAME_LEN: usize = 48;
/// Product event dimension (tapp id, platform slug, brew source, …).
const MAX_TARGET_LEN: usize = 64;
const MAX_BATCH_ITEMS: usize = 20;
const MAX_ENGAGEMENT_MS: i64 = 30 * 60 * 1000; // 30 min cap per flush
const MIN_ENGAGEMENT_MS: i64 = 800; // align with client MIN_ENGAGEMENT_MS
pub(crate) const DEFAULT_SUMMARY_DAYS: i64 = 7;
/// Admin summary / AI-usage query window upper bound.
/// Aligned with [`DAILY_RETENTION_DAYS`] (page/event daily aggregates).
/// Distinct-visitor detail is still limited by [`VISITOR_RETENTION_DAYS`].
pub(crate) const MAX_SUMMARY_DAYS: i64 = 365;
pub(crate) const VISITOR_RETENTION_DAYS: i64 = 90;
pub(crate) const DAILY_RETENTION_DAYS: i64 = 365;
/// Collect/pageview posts per client IP per minute (handler-level).
/// Keep at or above middleware `ANALYTICS_WRITE_MAX` so the shared middleware
/// bucket is the primary gate; 36 was below SPA engage+nav bursts.
const RATE_LIMIT_PER_MINUTE: u32 = 90;
const VIEW_DEDUPE_WINDOW: StdDuration = StdDuration::from_secs(3);
const DEFAULT_ANALYTICS_SALT: &str = "myriad-analytics-v1";
pub(crate) const ANALYTICS_BACKUP_FORMAT: &str = "myriad-analytics-backup";
/// Backup schema version (single current format; integrity always required).
pub(crate) const ANALYTICS_BACKUP_VERSION: u32 = 1;

/// Parse `YYYY-MM-DD` day strings used in analytics backups / filters.
pub(crate) fn parse_day_str(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").ok()
}

/// Visitor hash shape: 8–64 ASCII hex digits (matches intake fingerprints).
pub(crate) fn valid_visitor_hash(raw: &str) -> bool {
    let s = raw.trim();
    let len = s.len();
    (8..=64).contains(&len) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Non-negative i64 from a JSON number (rejects negatives and u64 overflow).
pub(crate) fn i64_nonneg(v: Option<&Value>) -> Option<i64> {
    let n = v.and_then(|x| x.as_i64()).or_else(|| {
        v.and_then(|x| x.as_u64())
            .and_then(|u| i64::try_from(u).ok())
    })?;
    if n < 0 {
        None
    } else {
        Some(n)
    }
}
pub(crate) const MAX_IMPORT_PAGE_DAILY: usize = 50_000;
pub(crate) const MAX_IMPORT_VISITOR_SEEN: usize = 200_000;
pub(crate) const MAX_IMPORT_EVENT_DAILY: usize = 50_000;
pub(crate) const MAX_IMPORT_EVENT_VISITOR: usize = 200_000;
pub(crate) const MAX_IMPORT_REFERRER_DAILY: usize = 20_000;
pub(crate) const MAX_IMPORT_COUNTRY_DAILY: usize = 50_000;
pub(crate) const MAX_IMPORT_COUNTRY_VISITOR: usize = 200_000;
/// ISO / CDN header codes we treat as "unknown" (do not store as a country).
const UNKNOWN_COUNTRY_CODES: &[&str] = &["XX", "T1", "A1", "A2", "O1", ""];
const COUNTRY_CACHE_TTL: StdDuration = StdDuration::from_secs(6 * 60 * 60);
const COUNTRY_LOOKUP_TIMEOUT: StdDuration = StdDuration::from_millis(900);
const MAX_COUNTRY_CACHE: usize = 4096;

static RATE_LIMIT: once_cell::sync::Lazy<Arc<Mutex<HashMap<String, (Instant, u32)>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));
static VIEW_DEDUPE: once_cell::sync::Lazy<Arc<Mutex<HashMap<String, Instant>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));
/// Admin summary cache: days → (stored_at, body). Invalidated on write.
/// Cache key is `days:N` or `from..to` (YYYY-MM-DD).
pub(crate) static SUMMARY_CACHE: once_cell::sync::Lazy<
    Arc<Mutex<HashMap<String, (Instant, Value)>>>,
> = once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));
pub(crate) const SUMMARY_CACHE_TTL: StdDuration = StdDuration::from_secs(45);
/// Public visitor-card aggregate (today / all-time / trend). The per-visitor
/// ordinal is **never** cached here — it is looked up per request.
pub(crate) static VISITOR_CARD_CACHE: once_cell::sync::Lazy<Arc<Mutex<Option<(Instant, Value)>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));
/// IP → country (code, name) cache for analytics intake.
static COUNTRY_CACHE: once_cell::sync::Lazy<Arc<Mutex<HashMap<String, (Instant, CountryInfo)>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Debug, Clone)]
pub(crate) struct CountryInfo {
    pub(crate) code: String,
    pub(crate) name: String,
}

/// Production is missing a configured `ANALYTICS_SALT` — collection must not proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnalyticsSaltUnavailable;

pub(crate) async fn invalidate_summary_cache() {
    *VISITOR_CARD_CACHE.lock().await = None;
    SUMMARY_CACHE.lock().await.clear();
}

// ── Request types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PageviewRequest {
    pub path: String,
    #[serde(default)]
    pub vid: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CollectRequest {
    #[serde(default)]
    pub vid: Option<String>,
    #[serde(default)]
    pub items: Vec<CollectItem>,
}

#[derive(Debug, Deserialize)]
pub struct CollectItem {
    /// `pageview` | `engagement` | `event`
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
    /// Engagement duration in ms (capped server-side).
    #[serde(default)]
    pub ms: Option<i64>,
    /// Custom event name (allowlisted shape).
    #[serde(default)]
    pub name: Option<String>,
    /// Optional event dimension (tapp id / platform / source / …).
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    pub days: Option<i64>,
    /// Inclusive start day `YYYY-MM-DD` (custom range; takes precedence with `to`).
    pub from: Option<String>,
    /// Inclusive end day `YYYY-MM-DD`.
    pub to: Option<String>,
}

/// Resolve admin analytics / AI-usage calendar window.
///
/// Prefer explicit `from`+`to` when both parse; otherwise use `days` ending at
/// [`analytics_today`]. Span is clamped to `1..=MAX_SUMMARY_DAYS`.
pub(crate) fn resolve_analytics_window(
    days: Option<i64>,
    from_raw: Option<&str>,
    to_raw: Option<&str>,
) -> (chrono::NaiveDate, chrono::NaiveDate, i64) {
    use chrono::NaiveDate;

    let today = analytics_today();
    let parse = |s: &str| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok();

    if let (Some(from_s), Some(to_s)) = (from_raw, to_raw) {
        if let (Some(mut from), Some(mut to)) = (parse(from_s), parse(to_s)) {
            if from > to {
                std::mem::swap(&mut from, &mut to);
            }
            // Cap end at server-local today (no future buckets).
            if to > today {
                to = today;
            }
            if from > to {
                from = to;
            }
            let mut span = (to - from).num_days() + 1;
            if span > MAX_SUMMARY_DAYS {
                from = to - Duration::days(MAX_SUMMARY_DAYS - 1);
                span = MAX_SUMMARY_DAYS;
            }
            if span < 1 {
                span = 1;
            }
            return (from, to, span);
        }
    }

    let days = days
        .unwrap_or(DEFAULT_SUMMARY_DAYS)
        .clamp(1, MAX_SUMMARY_DAYS);
    let from = today - Duration::days(days - 1);
    (from, today, days)
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Same production gate as router CORS / security headers: `ENVIRONMENT=production`.
pub(crate) fn is_production_environment() -> bool {
    std::env::var("ENVIRONMENT")
        .map(|s| s.trim() == "production")
        .unwrap_or(false)
}

/// Pure salt resolution (unit-testable; no process env / logging).
///
/// - Explicit non-empty `ANALYTICS_SALT` always wins.
/// - Without salt: prefer `JWT_SECRET`-derived material when present (instance-
/// unique; works for compose `ENVIRONMENT=production` + empty ANALYTICS_SALT).
/// - **Production** with neither salt nor JWT → `Err` (never use the shared
/// built-in default in production).
/// - **Development** with neither → built-in default.
pub(crate) fn resolve_analytics_salt(
    env_salt: Option<&str>,
    is_production: bool,
    jwt_secret: Option<&str>,
) -> Result<String, AnalyticsSaltUnavailable> {
    if let Some(s) = env_salt.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(s.to_string());
    }
    if let Some(jwt) = jwt_secret.map(str::trim).filter(|s| !s.is_empty()) {
        // Prefix keeps this material distinct from production salts and from raw JWT use.
        // Production uses a distinct prefix so ops can tell derived vs dedicated salt.
        let prefix = if is_production {
            "myriad-analytics-prod"
        } else {
            "myriad-analytics-dev"
        };
        return Ok(format!("{prefix}|{jwt}"));
    }
    if is_production {
        return Err(AnalyticsSaltUnavailable);
    }
    Ok(DEFAULT_ANALYTICS_SALT.to_string())
}

/// Resolve salt from process env. Logs once on missing salt (warn in dev, error in prod).
pub(crate) fn try_analytics_salt() -> Result<String, AnalyticsSaltUnavailable> {
    static WARNED_DEV: std::sync::Once = std::sync::Once::new();
    static ERR_PROD: std::sync::Once = std::sync::Once::new();

    let env_salt = std::env::var("ANALYTICS_SALT").ok();
    let jwt_secret = std::env::var("JWT_SECRET").ok();
    let production = is_production_environment();
    let result = resolve_analytics_salt(env_salt.as_deref(), production, jwt_secret.as_deref());

    match &result {
        Ok(_)
            if env_salt
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none() =>
        {
            WARNED_DEV.call_once(|| {
                if jwt_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .is_some()
                {
                    if production {
                        tracing::warn!(
                            "ANALYTICS_SALT is unset in production; deriving salt from JWT_SECRET. \
                             Set a dedicated random salt (openssl rand -hex 32) for stable visitor hashes."
                        );
                    } else {
                        tracing::warn!(
                            "ANALYTICS_SALT is unset; deriving a dev salt from JWT_SECRET. \
                             Set a dedicated random salt in production."
                        );
                    }
                } else {
                    tracing::warn!(
                        "ANALYTICS_SALT is unset; using built-in default. \
                         Set a random salt in production so visitor hashes are instance-unique."
                    );
                }
            });
        }
        Err(_) => {
            ERR_PROD.call_once(|| {
                tracing::error!(
                    "ANALYTICS_SALT is unset/empty in production and JWT_SECRET is missing; \
                     analytics collection refused. Set ANALYTICS_SALT (openssl rand -hex 32) \
                     or ensure JWT_SECRET is configured."
                );
            });
        }
        _ => {}
    }

    result
}

/// HTTP response when production has no analytics salt (fail closed).
pub(crate) fn salt_unavailable_response() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "success": false,
            "error": "analytics_unavailable",
        })),
    )
}

/// Calendar day for bucketing = **server process local date**.
///
/// No product-default offset (not +8 / +9 / hard-coded region). Whatever the
/// host or container clock reports via `chrono::Local` is used — typically
/// driven by the process `TZ` env if set, otherwise the OS default.
/// Operators should keep the server clock correct (`timedatectl` / container `TZ`).
pub(crate) fn analytics_today() -> NaiveDate {
    Local::now().date_naive()
}

/// Display label for the process local zone (env `TZ` name, or numeric UTC offset).
pub(crate) fn analytics_tz_label() -> String {
    std::env::var("TZ")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let secs = Local::now().offset().local_minus_utc();
            let hours = secs / 3600;
            if hours == 0 {
                "local".to_string()
            } else {
                format!("UTC{hours:+}")
            }
        })
}

pub fn normalize_path(raw: &str) -> Option<String> {
    let mut path = raw.trim();
    if path.is_empty() {
        return None;
    }
    if let Some(idx) = path.find("://") {
        let rest = &path[idx + 3..];
        path = rest.find('/').map(|i| &rest[i..]).unwrap_or("/");
    }
    if let Some((p, _)) = path.split_once('?') {
        path = p;
    }
    if let Some((p, _)) = path.split_once('#') {
        path = p;
    }
    let path = path.trim();
    if !path.starts_with('/') {
        return None;
    }

    let collapsed: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if collapsed.is_empty() {
        return Some("/".to_string());
    }

    let mapped: Vec<String> = match collapsed[0].to_ascii_lowercase().as_str() {
        "library" => vec!["library".into()],
        "brew" => vec!["brew".into()],
        "reports" => vec!["reports".into()],
        "config" => vec!["config".into()],
        "login" => vec!["login".into()],
        "register" => vec!["register".into()],
        "setup" => vec!["setup".into()],
        "tapps" => {
            if collapsed.len() >= 2 {
                vec!["tapps".into(), ":id".into()]
            } else {
                vec!["tapps".into()]
            }
        }
        "tapp" => {
            if collapsed.len() >= 2 {
                vec!["tapp".into(), ":id".into()]
            } else {
                vec!["tapp".into()]
            }
        }
        other => {
            if other
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && other.len() <= 48
            {
                vec![other.to_string()]
            } else {
                vec!["other".into()]
            }
        }
    };

    let mut out = format!("/{}", mapped.join("/"));
    if out.len() > MAX_PATH_LEN {
        out.truncate(MAX_PATH_LEN);
    }
    if !out
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_' || c == ':')
    {
        return Some("/other".to_string());
    }
    Some(out)
}

pub fn is_valid_vid(raw: &str) -> bool {
    let s = raw.trim();
    let len = s.len();
    (16..=64).contains(&len)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub(crate) fn sha16(material: &str) -> String {
    let digest = Sha256::digest(material.as_bytes());
    hex::encode(&digest[..16])
}

/// Hash visitor identity. Returns `None` when salt is unavailable (production
/// without `ANALYTICS_SALT`) so callers can fail closed without using a shared default.
pub fn resolve_visitor_hash(
    vid: Option<&str>,
    ip: Option<std::net::IpAddr>,
    user_agent: &str,
) -> Option<String> {
    let salt = try_analytics_salt().ok()?;
    if let Some(v) = vid.map(str::trim).filter(|s| is_valid_vid(s)) {
        return Some(sha16(&format!("{salt}|vid1|{v}")));
    }
    let ip_s = ip
        .map(|i| i.to_string())
        .unwrap_or_else(|| "unknown".into());
    let ua: String = user_agent
        .chars()
        .take(180)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    Some(sha16(&format!("{salt}|fp1|{ip_s}|{ua}")))
}

pub(crate) fn is_bot_ua(ua: &str) -> bool {
    let u = ua.to_ascii_lowercase();
    if u.is_empty() {
        return true;
    }
    const NEEDLES: &[&str] = &[
        "bot",
        "spider",
        "crawler",
        "slurp",
        "bingpreview",
        "facebookexternalhit",
        "embedly",
        "headless",
        "phantomjs",
        "selenium",
        "puppeteer",
        "playwright",
        "curl/",
        "wget/",
        "python-requests",
        "go-http-client",
        "scrapy",
        "ahrefs",
        "semrush",
        "yandex",
        "baiduspider",
        "applebot",
        "petalbot",
        "bytespider",
        "gptbot",
        "chatgpt",
        "claudebot",
        "anthropic",
        "ccbot",
    ];
    NEEDLES.iter().any(|n| u.contains(n))
}

/// Event names: lowercase snake / kebab, 2–48 chars, no free-form spam.
/// Reserved: names starting with `__` (internal markers).
pub fn normalize_event_name(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.len() < 2 || s.len() > MAX_EVENT_NAME_LEN {
        return None;
    }
    if s.starts_with("__") {
        return None;
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return None;
    }
    if s.starts_with('-') || s.starts_with('_') {
        return None;
    }
    Some(s)
}

/// Event target dimension: empty string = no dim; else 1–64 safe chars.
/// Allows UUID-ish ids, slugs, hosts: `[a-z0-9._:@+-]`.
pub fn normalize_target(raw: &str) -> String {
    let s: String = raw
        .trim()
        .chars()
        .take(MAX_TARGET_LEN * 2) // pre-cap before filter
        .collect::<String>()
        .to_ascii_lowercase();
    if s.is_empty() {
        return String::new();
    }
    let filtered: String = s
        .chars()
        .filter(|c| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || matches!(*c, '.' | '_' | '-' | ':' | '@' | '+')
        })
        .take(MAX_TARGET_LEN)
        .collect();
    if filtered.is_empty() {
        return String::new();
    }
    filtered
}

pub(crate) fn normalize_country_code(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_uppercase();
    if s.len() != 2 || !s.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if UNKNOWN_COUNTRY_CODES.iter().any(|u| *u == s) {
        return None;
    }
    Some(s)
}

pub(crate) fn normalize_country_name(raw: &str) -> String {
    raw.chars().take(64).collect::<String>().trim().to_string()
}

fn is_private_or_local_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_unspecified()
        }
    }
}

/// Prefer edge CDN country headers (no network); codes are ISO 3166-1 alpha-2.
///
/// Same trust rules as client IP: only honor CDN country headers when
/// `TRUST_PROXY_HEADERS` is on and the peer is trusted (`TRUST_PROXY_PEERS`
/// allowlist, or private/loopback when that list is empty). Otherwise return
/// `None` so callers fall through to IP geo lookup / none. Spoofed
/// `cf-ipcountry` / `x-country-code` from untrusted clients must not pollute
/// country analytics.
pub(crate) fn country_from_headers(
    headers: &axum::http::HeaderMap,
    peer: Option<std::net::IpAddr>,
) -> Option<CountryInfo> {
    use crate::middleware::client_ip::{
        trusted_proxy_headers_enabled, trusted_proxy_peer_allowlist,
    };
    country_from_headers_with_trust(
        headers,
        peer,
        trusted_proxy_headers_enabled(),
        trusted_proxy_peer_allowlist(),
    )
}

/// Testable core: trust gate + CDN country header parse (env-free).
pub(crate) fn country_from_headers_with_trust(
    headers: &axum::http::HeaderMap,
    peer: Option<std::net::IpAddr>,
    trust_proxy_headers: bool,
    allowlist: &[ipnet::IpNet],
) -> Option<CountryInfo> {
    use crate::middleware::client_ip::should_trust_proxy_headers;

    if !should_trust_proxy_headers(peer, trust_proxy_headers, allowlist) {
        return None;
    }

    const KEYS: &[&str] = &[
        "cf-ipcountry",
        "cloudfront-viewer-country",
        "x-vercel-ip-country",
        "x-country-code",
        "x-appengine-country",
    ];
    for key in KEYS {
        if let Some(v) = headers.get(*key).and_then(|h| h.to_str().ok()) {
            if let Some(code) = normalize_country_code(v) {
                return Some(CountryInfo {
                    code: code.clone(),
                    name: code,
                });
            }
        }
    }
    None
}

async fn country_cache_get(ip_key: &str) -> Option<CountryInfo> {
    let mut map = COUNTRY_CACHE.lock().await;
    if let Some((at, info)) = map.get(ip_key) {
        if at.elapsed() < COUNTRY_CACHE_TTL {
            return Some(info.clone());
        }
        map.remove(ip_key);
    }
    None
}

async fn country_cache_put(ip_key: &str, info: CountryInfo) {
    let mut map = COUNTRY_CACHE.lock().await;
    if map.len() >= MAX_COUNTRY_CACHE {
        map.retain(|_, (at, _)| at.elapsed() < COUNTRY_CACHE_TTL);
        while map.len() >= MAX_COUNTRY_CACHE {
            let Some(oldest) = map
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            map.remove(&oldest);
        }
    }
    map.insert(ip_key.to_string(), (Instant::now(), info));
}

/// Resolve visitor country: CDN header → cache → short-timeout IP lookup.
async fn resolve_country(
    ip: Option<std::net::IpAddr>,
    header_country: Option<CountryInfo>,
) -> Option<CountryInfo> {
    if let Some(c) = header_country {
        if let Some(ip) = ip {
            country_cache_put(&ip.to_string(), c.clone()).await;
        }
        return Some(c);
    }
    let ip = ip?;
    if is_private_or_local_ip(ip) {
        return None;
    }
    let ip_key = ip.to_string();
    if let Some(c) = country_cache_get(&ip_key).await {
        return Some(c);
    }

    // Free tier ip-api.com — short timeout so collect never hangs.
    let url = format!(
        "http://ip-api.com/json/{}?fields=status,country,countryCode",
        ip_key
    );
    let client = crate::services::http_client::get_global_client().await;
    let lookup = async {
        let resp = client.get(&url).send().await.ok()?;
        let data: Value = resp.json().await.ok()?;
        if data.get("status").and_then(|s| s.as_str()) != Some("success") {
            return None;
        }
        let code = data
            .get("countryCode")
            .and_then(|v| v.as_str())
            .and_then(normalize_country_code)?;
        let name = data
            .get("country")
            .and_then(|v| v.as_str())
            .map(normalize_country_name)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| code.clone());
        Some(CountryInfo { code, name })
    };

    match tokio::time::timeout(COUNTRY_LOOKUP_TIMEOUT, lookup).await {
        Ok(Some(info)) => {
            country_cache_put(&ip_key, info.clone()).await;
            Some(info)
        }
        _ => None,
    }
}

pub(crate) fn normalize_referrer_host(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() || s.len() > 128 {
        return None;
    }
    // Host-like only
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':')
    {
        return None;
    }
    // Strip port for aggregation
    let host = s.split(':').next().unwrap_or(&s);
    if host.is_empty() || host == "localhost" || host.ends_with(".local") {
        return None;
    }
    Some(host.to_string())
}

async fn rate_limited(ip_key: &str) -> bool {
    let mut map = RATE_LIMIT.lock().await;
    let now = Instant::now();
    if map.len() > 8000 {
        map.retain(|_, (t, _)| now.duration_since(*t) < StdDuration::from_secs(120));
    }
    let entry = map.entry(ip_key.to_string()).or_insert((now, 0));
    if now.duration_since(entry.0) >= StdDuration::from_secs(60) {
        *entry = (now, 1);
        return false;
    }
    entry.1 += 1;
    entry.1 > RATE_LIMIT_PER_MINUTE
}

async fn is_duplicate_view(visitor: &str, path: &str) -> bool {
    let key = format!("{visitor}|{path}");
    let mut map = VIEW_DEDUPE.lock().await;
    let now = Instant::now();
    if map.len() > 20000 {
        map.retain(|_, t| now.duration_since(*t) < VIEW_DEDUPE_WINDOW * 2);
    }
    if let Some(prev) = map.get(&key) {
        if now.duration_since(*prev) < VIEW_DEDUPE_WINDOW {
            return true;
        }
    }
    map.insert(key, now);
    false
}

// ── DB writes ──────────────────────────────────────────────────────────────

async fn mark_visitor_seen(
    db: &DatabaseConnection,
    day: NaiveDate,
    path: &str,
    visitor: &str,
) -> Result<bool, sea_orm::DbErr> {
    let insert = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO analytics_visitor_seen (day, path, visitor_hash)
VALUES ($1, $2, $3)
ON CONFLICT (day, path, visitor_hash) DO NOTHING
"#,
            [
                SeaValue::from(day),
                SeaValue::from(path.to_string()),
                SeaValue::from(visitor.to_string()),
            ],
        ))
        .await?;
    Ok(insert.rows_affected() > 0)
}

async fn bump_pageview(
    db: &DatabaseConnection,
    day: NaiveDate,
    path: &str,
    visitor: &str,
    count_view: bool,
) -> Result<(), sea_orm::DbErr> {
    let is_new = mark_visitor_seen(db, day, path, visitor).await?;
    let unique_inc: i64 = if is_new { 1 } else { 0 };
    let view_inc: i64 = if count_view { 1 } else { 0 };
    if view_inc == 0 && unique_inc == 0 {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
VALUES ($1, $2, $3, $4, 0, 0)
ON CONFLICT (day, path) DO UPDATE SET
  views = analytics_page_daily.views + EXCLUDED.views,
  unique_visitors = analytics_page_daily.unique_visitors + EXCLUDED.unique_visitors
"#,
        [
            SeaValue::from(day),
            SeaValue::from(path.to_string()),
            SeaValue::from(view_inc),
            SeaValue::from(unique_inc),
        ],
    ))
    .await?;
    Ok(())
}

/// Stored arrival ordinal for a visitor on `day` ("you are today's Nth visitor").
///
/// `0` means unknown — rows written before the column existed, or non-site paths
/// which never get an ordinal. Callers treat unknown as "no number to show"
/// rather than "visitor #0".
pub(crate) async fn read_visitor_ordinal(
    db: &DatabaseConnection,
    day: NaiveDate,
    visitor: &str,
) -> Result<Option<i64>, sea_orm::DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT ordinal FROM analytics_visitor_seen
WHERE day = $1 AND path = $2 AND visitor_hash = $3
"#,
            [
                SeaValue::from(day),
                SeaValue::from(SITE_PATH.to_string()),
                SeaValue::from(visitor.to_string()),
            ],
        ))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<i64>("", "ordinal").ok())
        .filter(|n| *n > 0))
}

/// Counts the visitor as a site-unique for `day` and returns their arrival
/// ordinal, or `None` when it cannot be determined.
///
/// The ordinal has to be **persisted**, not derived: the number a visitor is
/// shown must not move for the rest of the day, and any rank computed from the
/// hash set drifts upward as later visitors arrive. So the post-increment
/// `unique_visitors` value — which *is* the arrival position — is captured with
/// `RETURNING` and written onto the visitor's row. `RETURNING` on the single
/// counter row is atomic per statement, so concurrent first-visits can never
/// come away holding the same number.
async fn record_site_unique(
    db: &DatabaseConnection,
    day: NaiveDate,
    visitor: &str,
) -> Result<Option<i64>, sea_orm::DbErr> {
    let is_new = mark_visitor_seen(db, day, SITE_PATH, visitor).await?;
    if !is_new {
        return read_visitor_ordinal(db, day, visitor).await;
    }
    let ordinal = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
VALUES ($1, $2, 0, 1, 0, 0)
ON CONFLICT (day, path) DO UPDATE SET
  unique_visitors = analytics_page_daily.unique_visitors + 1
RETURNING unique_visitors
"#,
            [SeaValue::from(day), SeaValue::from(SITE_PATH.to_string())],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "unique_visitors").ok())
        .filter(|n| *n > 0);

    if let Some(n) = ordinal {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE analytics_visitor_seen SET ordinal = $4
WHERE day = $1 AND path = $2 AND visitor_hash = $3
"#,
            [
                SeaValue::from(day),
                SeaValue::from(SITE_PATH.to_string()),
                SeaValue::from(visitor.to_string()),
                SeaValue::from(n),
            ],
        ))
        .await?;
    }
    Ok(ordinal)
}

/// Internal event name: marks "this visitor already contributed engaged_views
/// for path today". Not shown in public event list.
pub(crate) const ENGAGE_MARKER: &str = "__engage__";

async fn bump_engagement(
    db: &DatabaseConnection,
    day: NaiveDate,
    path: &str,
    visitor: &str,
    ms: i64,
) -> Result<(), sea_orm::DbErr> {
    let ms = ms.clamp(0, MAX_ENGAGEMENT_MS);
    if ms < MIN_ENGAGEMENT_MS {
        return Ok(());
    }
    // First engagement report for (day, path, visitor) → +1 engaged_views.
    // Later soft-flushes only add ms (otherwise avg time and bounce break).
    let insert = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO analytics_event_visitor (day, event_name, path, target, visitor_hash)
VALUES ($1, $2, $3, '', $4)
ON CONFLICT (day, event_name, path, target, visitor_hash) DO NOTHING
"#,
            [
                SeaValue::from(day),
                SeaValue::from(ENGAGE_MARKER.to_string()),
                SeaValue::from(path.to_string()),
                SeaValue::from(visitor.to_string()),
            ],
        ))
        .await?;
    let engaged_inc: i64 = if insert.rows_affected() > 0 { 1 } else { 0 };

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
VALUES ($1, $2, 0, 0, $3, $4)
ON CONFLICT (day, path) DO UPDATE SET
  engagement_ms = analytics_page_daily.engagement_ms + EXCLUDED.engagement_ms,
  engaged_views = analytics_page_daily.engaged_views + EXCLUDED.engaged_views
"#,
        [
            SeaValue::from(day),
            SeaValue::from(path.to_string()),
            SeaValue::from(ms),
            SeaValue::from(engaged_inc),
        ],
    ))
    .await?;
    Ok(())
}

async fn bump_event(
    db: &DatabaseConnection,
    day: NaiveDate,
    name: &str,
    path: &str,
    target: &str,
    visitor: &str,
) -> Result<(), sea_orm::DbErr> {
    let insert = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO analytics_event_visitor (day, event_name, path, target, visitor_hash)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (day, event_name, path, target, visitor_hash) DO NOTHING
"#,
            [
                SeaValue::from(day),
                SeaValue::from(name.to_string()),
                SeaValue::from(path.to_string()),
                SeaValue::from(target.to_string()),
                SeaValue::from(visitor.to_string()),
            ],
        ))
        .await?;
    let unique_inc: i64 = if insert.rows_affected() > 0 { 1 } else { 0 };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO analytics_event_daily (day, event_name, path, target, count, unique_visitors)
VALUES ($1, $2, $3, $4, 1, $5)
ON CONFLICT (day, event_name, path, target) DO UPDATE SET
  count = analytics_event_daily.count + 1,
  unique_visitors = analytics_event_daily.unique_visitors + EXCLUDED.unique_visitors
"#,
        [
            SeaValue::from(day),
            SeaValue::from(name.to_string()),
            SeaValue::from(path.to_string()),
            SeaValue::from(target.to_string()),
            SeaValue::from(unique_inc),
        ],
    ))
    .await?;
    Ok(())
}

async fn bump_referrer(
    db: &DatabaseConnection,
    day: NaiveDate,
    host: &str,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO analytics_referrer_daily (day, host, count)
VALUES ($1, $2, 1)
ON CONFLICT (day, host) DO UPDATE SET
  count = analytics_referrer_daily.count + 1
"#,
        [SeaValue::from(day), SeaValue::from(host.to_string())],
    ))
    .await?;
    Ok(())
}

async fn bump_country(
    db: &DatabaseConnection,
    day: NaiveDate,
    country: &CountryInfo,
    visitor: &str,
    count_view: bool,
) -> Result<(), sea_orm::DbErr> {
    let view_inc: i64 = if count_view { 1 } else { 0 };

    // First (day, country, visitor) → +1 unique_visitors
    let insert = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO analytics_country_visitor (day, country_code, visitor_hash)
VALUES ($1, $2, $3)
ON CONFLICT (day, country_code, visitor_hash) DO NOTHING
"#,
            [
                SeaValue::from(day),
                SeaValue::from(country.code.clone()),
                SeaValue::from(visitor.to_string()),
            ],
        ))
        .await?;
    let unique_inc: i64 = if insert.rows_affected() > 0 { 1 } else { 0 };
    if view_inc == 0 && unique_inc == 0 {
        return Ok(());
    }

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO analytics_country_daily (day, country_code, country_name, views, unique_visitors)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (day, country_code) DO UPDATE SET
  views = analytics_country_daily.views + EXCLUDED.views,
  unique_visitors = analytics_country_daily.unique_visitors + EXCLUDED.unique_visitors,
  country_name = CASE
    WHEN EXCLUDED.country_name <> '' THEN EXCLUDED.country_name
    ELSE analytics_country_daily.country_name
  END
"#,
        [
            SeaValue::from(day),
            SeaValue::from(country.code.clone()),
            SeaValue::from(country.name.clone()),
            SeaValue::from(view_inc),
            SeaValue::from(unique_inc),
        ],
    ))
    .await?;
    Ok(())
}

async fn maybe_prune(db: &DatabaseConnection) {
    if !Utc::now().timestamp_subsec_nanos().is_multiple_of(100) {
        return;
    }
    let today = analytics_today();
    let visitor_cutoff = today - Duration::days(VISITOR_RETENTION_DAYS);
    let daily_cutoff = today - Duration::days(DAILY_RETENTION_DAYS);
    for sql in [
        "DELETE FROM analytics_visitor_seen WHERE day < $1",
        "DELETE FROM analytics_event_visitor WHERE day < $1",
        "DELETE FROM analytics_country_visitor WHERE day < $1",
        "DELETE FROM analytics_page_daily WHERE day < $1",
        "DELETE FROM analytics_event_daily WHERE day < $1",
        "DELETE FROM analytics_referrer_daily WHERE day < $1",
        "DELETE FROM analytics_country_daily WHERE day < $1",
    ] {
        let cutoff = if sql.contains("visitor") {
            visitor_cutoff
        } else {
            daily_cutoff
        };
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                [SeaValue::from(cutoff)],
            ))
            .await;
    }
}

// ── Shared intake ──────────────────────────────────────────────────────────

struct IntakeCtx {
    db: DatabaseConnection,
    visitor: String,
    day: NaiveDate,
    country: Option<CountryInfo>,
}

async fn process_items(ctx: &IntakeCtx, items: &[CollectItem]) -> usize {
    let mut accepted = 0usize;
    for item in items.iter().take(MAX_BATCH_ITEMS) {
        let kind = item.kind.trim().to_ascii_lowercase();
        match kind.as_str() {
            "pageview" => {
                let Some(path) = item.path.as_deref().and_then(normalize_path) else {
                    continue;
                };
                let _ = record_site_unique(&ctx.db, ctx.day, &ctx.visitor).await;
                let dup = is_duplicate_view(&ctx.visitor, &path).await;
                if bump_pageview(&ctx.db, ctx.day, &path, &ctx.visitor, !dup)
                    .await
                    .is_ok()
                {
                    accepted += 1;
                    if let Some(ref country) = ctx.country {
                        let _ = bump_country(&ctx.db, ctx.day, country, &ctx.visitor, !dup).await;
                    }
                }
                if let Some(host) = item.referrer.as_deref().and_then(normalize_referrer_host) {
                    let _ = bump_referrer(&ctx.db, ctx.day, &host).await;
                }
            }
            "engagement" => {
                let Some(path) = item.path.as_deref().and_then(normalize_path) else {
                    continue;
                };
                let ms = item.ms.unwrap_or(0);
                if bump_engagement(&ctx.db, ctx.day, &path, &ctx.visitor, ms)
                    .await
                    .is_ok()
                {
                    accepted += 1;
                }
            }
            "event" => {
                let Some(name) = item.name.as_deref().and_then(normalize_event_name) else {
                    continue;
                };
                let path = item
                    .path
                    .as_deref()
                    .and_then(normalize_path)
                    .unwrap_or_else(|| "/".to_string());
                let target = item
                    .target
                    .as_deref()
                    .map(normalize_target)
                    .unwrap_or_default();
                if bump_event(&ctx.db, ctx.day, &name, &path, &target, &ctx.visitor)
                    .await
                    .is_ok()
                {
                    accepted += 1;
                }
            }
            _ => {}
        }
    }
    accepted
}

async fn parse_json_body<T: for<'de> Deserialize<'de>>(
    db: &sea_orm::DatabaseConnection,
    request: Request,
) -> Result<(Option<std::net::IpAddr>, String, bool, T), crate::error::HttpError> {
    // Staff JWT (admin or site owner) → drop self-traffic even if client flag is spoofed.
    // Matches FE usePageViewTracker (isAdmin || isOwner).
    let is_staff = crate::middleware::auth::authenticate_optional_request(request.headers(), db)
        .await
        .map_err(|response| {
            crate::error::HttpError(myriad_error::AppError::from_status_u16(
                response.status().as_u16(),
                "Invalid authentication state",
            ))
        })?
        .map(|claims| claims.is_admin || claims.is_owner)
        .unwrap_or(false);
    let ip = crate::middleware::client_ip::extract_client_ip(&request);
    let ua = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = axum::body::to_bytes(request.into_body(), 32 * 1024)
        .await
        .map_err(|_| {
            crate::error::HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({ "success": false, "error": "invalid_body" })),
            ))
        })?;
    let body: T = serde_json::from_slice(&bytes).map_err(|_| {
        crate::error::HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "invalid_json" })),
        ))
    })?;
    Ok((ip, ua, is_staff, body))
}

// ── Handlers ───────────────────────────────────────────────────────────────

/// Whether first-party visitor collection is enabled (default true).
pub(crate) fn analytics_collection_enabled(config: &DynamicConfig) -> bool {
    config.analytics_enabled
}

/// POST /api/analytics/collect — preferred batch endpoint.
pub async fn collect(
    axum::extract::State(dynamic_config): axum::extract::State<Arc<RwLock<DynamicConfig>>>,
    crate::extract::Db(db): crate::extract::Db,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let header_country = country_from_headers(request.headers(), Some(peer.ip()));
    let (ip, ua, is_staff, body) = match parse_json_body::<CollectRequest>(&db, request).await {
        Ok(v) => v,
        Err(e) => {
            let status = StatusCode::from_u16(e.0.status_u16()).unwrap_or(StatusCode::BAD_REQUEST);
            return (status, Json(e.0.to_json()));
        }
    };

    if !analytics_collection_enabled(&*dynamic_config.read().await) {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "skipped": "disabled", "accepted": 0 })),
        );
    }

    if is_staff {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "skipped": "staff", "accepted": 0 })),
        );
    }

    if is_bot_ua(&ua) {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "skipped": "bot", "accepted": 0 })),
        );
    }

    let ip_key = ip
        .map(|i| i.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    if rate_limited(&ip_key).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "success": false, "error": "rate_limited" })),
        );
    }

    if body.items.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "empty" })),
        );
    }

    let Some(visitor) = resolve_visitor_hash(body.vid.as_deref(), ip, &ua) else {
        return salt_unavailable_response();
    };
    let day = analytics_today();
    let country = resolve_country(ip, header_country).await;
    let ctx = IntakeCtx {
        db: db.clone(),
        visitor,
        day,
        country,
    };
    let accepted = process_items(&ctx, &body.items).await;
    if accepted > 0 {
        invalidate_summary_cache().await;
    }
    maybe_prune(&db).await;

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "accepted": accepted,
        })),
    )
}

/// POST /api/analytics/pageview — single pageview (compat).
pub async fn record_pageview(
    axum::extract::State(dynamic_config): axum::extract::State<Arc<RwLock<DynamicConfig>>>,
    crate::extract::Db(db): crate::extract::Db,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let header_country = country_from_headers(request.headers(), Some(peer.ip()));
    let (ip, ua, is_staff, body) = match parse_json_body::<PageviewRequest>(&db, request).await {
        Ok(v) => v,
        Err(e) => {
            let status = StatusCode::from_u16(e.0.status_u16()).unwrap_or(StatusCode::BAD_REQUEST);
            return (status, Json(e.0.to_json()));
        }
    };

    if !analytics_collection_enabled(&*dynamic_config.read().await) {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "skipped": "disabled" })),
        );
    }

    if is_staff {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "skipped": "staff" })),
        );
    }

    if is_bot_ua(&ua) {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "skipped": "bot" })),
        );
    }

    let ip_key = ip
        .map(|i| i.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    if rate_limited(&ip_key).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "success": false, "error": "rate_limited" })),
        );
    }

    let Some(path) = normalize_path(&body.path) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "invalid_path" })),
        );
    };

    let Some(visitor) = resolve_visitor_hash(body.vid.as_deref(), ip, &ua) else {
        return salt_unavailable_response();
    };
    let day = analytics_today();
    let country = resolve_country(ip, header_country).await;
    let items = vec![CollectItem {
        kind: "pageview".into(),
        path: Some(path.clone()),
        referrer: body.referrer,
        ms: None,
        name: None,
        target: None,
    }];
    let ctx = IntakeCtx {
        db: db.clone(),
        visitor,
        day,
        country,
    };
    let accepted = process_items(&ctx, &items).await;
    if accepted > 0 {
        invalidate_summary_cache().await;
    }
    maybe_prune(&db).await;

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "path": path,
            "accepted": accepted,
        })),
    )
}

pub(crate) async fn count_distinct_site(
    db: &DatabaseConnection,
    from: NaiveDate,
    to: NaiveDate,
) -> i64 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT COUNT(DISTINCT visitor_hash)::bigint AS n
FROM analytics_visitor_seen
WHERE day >= $1 AND day <= $2 AND path = $3
"#,
        [
            SeaValue::from(from),
            SeaValue::from(to),
            SeaValue::from(SITE_PATH.to_string()),
        ],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|r| r.try_get::<i64>("", "n").ok())
    .unwrap_or(0)
}

/// Pageviews in `[from, to]` excluding the site-wide rollup path.
pub(crate) async fn sum_page_views(db: &DatabaseConnection, from: NaiveDate, to: NaiveDate) -> i64 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT COALESCE(SUM(views), 0)::bigint AS n
FROM analytics_page_daily
WHERE day >= $1 AND day <= $2 AND path <> $3
"#,
        [
            SeaValue::from(from),
            SeaValue::from(to),
            SeaValue::from(SITE_PATH.to_string()),
        ],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|r| r.try_get::<i64>("", "n").ok())
    .unwrap_or(0)
}

/// Percent change for 环比 tiles. `None` when previous is 0 and current > 0
/// (undefined baseline — UI shows "新" / new).
pub(crate) fn pct_change(current: i64, previous: i64) -> Option<f64> {
    if previous == 0 {
        if current == 0 {
            Some(0.0)
        } else {
            None
        }
    } else {
        Some(((current - previous) as f64 / previous as f64) * 100.0)
    }
}

/// `day` | `week` | `month` | `period` — labels for range 环比 on FE.
pub(crate) fn compare_range_kind(days: i64) -> &'static str {
    match days {
        1 => "day",
        7 => "week",
        30 => "month",
        _ => "period",
    }
}

/// One metric delta for admin summary / AI usage `compare` objects.
pub(crate) fn metric_delta(current: i64, previous: i64) -> serde_json::Value {
    json!({
        "current": current,
        "previous": previous,
        "pct": pct_change(current, previous),
    })
}

#[cfg(test)]
mod compare_tests {
    use super::{compare_range_kind, pct_change};

    #[test]
    fn pct_change_cases() {
        assert_eq!(pct_change(0, 0), Some(0.0));
        assert_eq!(pct_change(10, 0), None);
        assert_eq!(pct_change(120, 100), Some(20.0));
        assert_eq!(pct_change(80, 100), Some(-20.0));
    }

    #[test]
    fn range_kind_labels() {
        assert_eq!(compare_range_kind(1), "day");
        assert_eq!(compare_range_kind(7), "week");
        assert_eq!(compare_range_kind(30), "month");
        assert_eq!(compare_range_kind(14), "period");
    }
}
