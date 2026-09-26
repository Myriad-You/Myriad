use crate::config::DynamicConfig;
use axum::{
    Json,
    extract::{ConnectInfo, Request},
    http::{StatusCode, header},
};
use chrono::{Duration, Local, NaiveDate, Utc};
use myriad_error::AppError;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait,
    Value as SeaValue,
};
use serde::Deserialize;
use serde_json::{Value, json};
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
/// Product event dimension (tapp id, platform slug, phantasi source, …).
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
/// bucket is the primary gate.
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
    if n < 0 { None } else { Some(n) }
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
pub(crate) const SUMMARY_CACHE_TTL: StdDuration = StdDuration::from_secs(45);
/// Admin summary and public visitor-card caches. Invalidated on every write.
pub(crate) static SUMMARY_CACHES: once_cell::sync::Lazy<Mutex<SummaryCaches>> =
    once_cell::sync::Lazy::new(|| Mutex::new(SummaryCaches::default()));
/// IP → country (code, name) cache for analytics intake.
static COUNTRY_CACHE: once_cell::sync::Lazy<Arc<Mutex<HashMap<String, (Instant, CountryInfo)>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Debug, Clone)]
pub(crate) struct CountryInfo {
    pub(crate) code: String,
    pub(crate) name: String,
}

/// Production has neither `ANALYTICS_SALT` nor `JWT_SECRET` — collection must not proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnalyticsSaltUnavailable;

/// Summary cache entries plus a generation that every invalidation bumps.
///
/// A summary is computed without holding the lock, so an intake can commit and
/// invalidate while it runs. The computing side reads [`Self::generation`]
/// before querying and stores through `store_*`, which drop the result when
/// the generation has moved: otherwise the stale body would be written back
/// over the invalidation and served for a whole TTL. Bump and clear happen
/// under the same lock as the check-and-store, so there is no window between.
#[derive(Default)]
pub(crate) struct SummaryCaches {
    generation: u64,
    /// `{from}..{to}` (YYYY-MM-DD) → (stored_at, body).
    summary: HashMap<String, (Instant, Value)>,
    /// Public visitor-card aggregate (today / all-time / trend). The
    /// per-visitor ordinal is **never** cached here — it is looked up per request.
    card: Option<(Instant, Value)>,
}

impl SummaryCaches {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn summary(&self, key: &str) -> Option<Value> {
        self.summary
            .get(key)
            .filter(|(at, _)| at.elapsed() < SUMMARY_CACHE_TTL)
            .map(|(_, body)| body.clone())
    }

    /// Stores only if nothing was invalidated since `generation` was read.
    pub(crate) fn store_summary(&mut self, generation: u64, key: String, body: Value) -> bool {
        if generation != self.generation {
            return false;
        }
        self.summary.insert(key, (Instant::now(), body));
        // Keep map small (only a few day windows are ever queried)
        if self.summary.len() > 12 {
            self.summary
                .retain(|_, (at, _)| at.elapsed() < SUMMARY_CACHE_TTL * 2);
        }
        true
    }

    pub(crate) fn card(&self) -> Option<Value> {
        self.card
            .as_ref()
            .filter(|(at, _)| at.elapsed() < SUMMARY_CACHE_TTL)
            .map(|(_, body)| body.clone())
    }

    /// Stores only if nothing was invalidated since `generation` was read.
    pub(crate) fn store_card(&mut self, generation: u64, body: Value) -> bool {
        if generation != self.generation {
            return false;
        }
        self.card = Some((Instant::now(), body));
        true
    }

    pub(crate) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.summary.clear();
        self.card = None;
    }
}

pub(crate) async fn invalidate_summary_cache() {
    SUMMARY_CACHES.lock().await.invalidate();
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

/// Same production gate as router CORS: trimmed `ENVIRONMENT=production`.
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

/// Resolve salt from process env. Logs once if `ANALYTICS_SALT` is unset (warn when a salt is still derived; error only if production resolution fails).
pub(crate) fn try_analytics_salt() -> Result<String, AnalyticsSaltUnavailable> {
    static WARNED_DEV: std::sync::Once = std::sync::Once::new();
    static ERR_PROD: std::sync::Once = std::sync::Once::new();

    let env_salt = std::env::var("ANALYTICS_SALT").ok();
    let jwt_secret = crate::middleware::auth::session_secret();
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

/// HTTP 503 when production salt resolution fails closed.
pub(crate) fn salt_unavailable_response() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "success": false,
            "error": "analytics_unavailable",
            "code": "analytics_unavailable",
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

/// The server-local date of a `timestamptz` column, on the same clock as
/// [`analytics_today`]. A bare `::date` would use the database session's
/// time zone instead, and the two disagree whenever the database runs in
/// another zone: between local midnight and the database's, "today" would
/// miss everything that happened so far. The offset is the current one, as
/// for [`analytics_today`].
pub(crate) fn analytics_day_sql(column: &str) -> String {
    let offset = Local::now().offset().local_minus_utc();
    format!("({column} AT TIME ZONE INTERVAL '{offset} seconds')::date")
}

/// Display label: `TZ` if set; else `local` at UTC+0, else `UTC±N`.
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
        "phantasi" => vec!["phantasi".into()],
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

/// Hash visitor identity. Returns `None` when `try_analytics_salt` fails (production with neither `ANALYTICS_SALT` nor `JWT_SECRET`) so callers fail closed without the shared default.
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
/// Same trust gate as client IP: `TRUST_PROXY_HEADERS` plus peer in `TRUST_PROXY_PEERS`
/// (empty list → narrow default loopback+docker0, not all RFC1918). Otherwise return
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
//
// Every "seen set + counter" pair is written by **one statement**: a
// data-modifying CTE inserts into the seen set and the counter upsert adds
// `count(*)` of what that insert actually returned. A failure anywhere rolls
// back both halves, so a visitor can never be marked seen while the counter
// misses them; a concurrent duplicate blocks on the seen key, then hits
// `DO NOTHING`, returns no row and adds 0.

const PAGEVIEW_SQL: &str = r#"
WITH ins AS (
    INSERT INTO analytics_visitor_seen (day, path, visitor_hash)
    VALUES ($1, $2, $3)
    ON CONFLICT (day, path, visitor_hash) DO NOTHING
    RETURNING 1
)
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
SELECT $1, $2, $4, fresh.n, 0, 0
FROM (SELECT count(*)::bigint AS n FROM ins) fresh
WHERE $4 > 0 OR fresh.n > 0
ON CONFLICT (day, path) DO UPDATE SET
  views = analytics_page_daily.views + EXCLUDED.views,
  unique_visitors = analytics_page_daily.unique_visitors + EXCLUDED.unique_visitors
"#;

/// Site-unique bump. Returns the post-increment `unique_visitors` only when
/// this call inserted the seen row; no row when the visitor was already seen.
const SITE_UNIQUE_SQL: &str = r#"
WITH ins AS (
    INSERT INTO analytics_visitor_seen (day, path, visitor_hash)
    VALUES ($1, $2, $3)
    ON CONFLICT (day, path, visitor_hash) DO NOTHING
    RETURNING 1
)
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
SELECT $1, $2, 0, 1, 0, 0 FROM ins
ON CONFLICT (day, path) DO UPDATE SET
  unique_visitors = analytics_page_daily.unique_visitors + 1
RETURNING unique_visitors
"#;

const ENGAGEMENT_SQL: &str = r#"
WITH ins AS (
    INSERT INTO analytics_event_visitor (day, event_name, path, target, visitor_hash)
    VALUES ($1, $2, $3, '', $4)
    ON CONFLICT (day, event_name, path, target, visitor_hash) DO NOTHING
    RETURNING 1
)
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
SELECT $1, $3, 0, 0, $5, fresh.n
FROM (SELECT count(*)::bigint AS n FROM ins) fresh
ON CONFLICT (day, path) DO UPDATE SET
  engagement_ms = analytics_page_daily.engagement_ms + EXCLUDED.engagement_ms,
  engaged_views = analytics_page_daily.engaged_views + EXCLUDED.engaged_views
"#;

const EVENT_SQL: &str = r#"
WITH ins AS (
    INSERT INTO analytics_event_visitor (day, event_name, path, target, visitor_hash)
    VALUES ($1, $2, $3, $4, $5)
    ON CONFLICT (day, event_name, path, target, visitor_hash) DO NOTHING
    RETURNING 1
)
INSERT INTO analytics_event_daily (day, event_name, path, target, count, unique_visitors)
SELECT $1, $2, $3, $4, 1, fresh.n
FROM (SELECT count(*)::bigint AS n FROM ins) fresh
ON CONFLICT (day, event_name, path, target) DO UPDATE SET
  count = analytics_event_daily.count + 1,
  unique_visitors = analytics_event_daily.unique_visitors + EXCLUDED.unique_visitors
"#;

const COUNTRY_SQL: &str = r#"
WITH ins AS (
    INSERT INTO analytics_country_visitor (day, country_code, visitor_hash)
    VALUES ($1, $2, $3)
    ON CONFLICT (day, country_code, visitor_hash) DO NOTHING
    RETURNING 1
)
INSERT INTO analytics_country_daily (day, country_code, country_name, views, unique_visitors)
SELECT $1, $2, $4, $5, fresh.n
FROM (SELECT count(*)::bigint AS n FROM ins) fresh
WHERE $5 > 0 OR fresh.n > 0
ON CONFLICT (day, country_code) DO UPDATE SET
  views = analytics_country_daily.views + EXCLUDED.views,
  unique_visitors = analytics_country_daily.unique_visitors + EXCLUDED.unique_visitors,
  country_name = CASE
    WHEN EXCLUDED.country_name <> '' THEN EXCLUDED.country_name
    ELSE analytics_country_daily.country_name
  END
"#;

pub(super) async fn bump_pageview(
    db: &DatabaseConnection,
    day: NaiveDate,
    path: &str,
    visitor: &str,
    count_view: bool,
) -> Result<(), sea_orm::DbErr> {
    let view_inc: i64 = if count_view { 1 } else { 0 };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        PAGEVIEW_SQL,
        [
            SeaValue::from(day),
            SeaValue::from(path.to_string()),
            SeaValue::from(visitor.to_string()),
            SeaValue::from(view_inc),
        ],
    ))
    .await?;
    Ok(())
}

/// Stored arrival ordinal for a visitor on `day` ("you are today's Nth visitor").
///
/// `0`/absent means unknown (no number to show). Non-`SITE_PATH` rows never store an ordinal.
/// Callers treat unknown as "no number to show" rather than "visitor #0".
pub(crate) async fn read_visitor_ordinal(
    db: &impl ConnectionTrait,
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
///
/// Seen row and counter are one statement ([`SITE_UNIQUE_SQL`]); the ordinal
/// write shares its transaction, so a failure leaves nothing behind and the
/// next pageview simply retries as a first visit.
pub(super) async fn record_site_unique(
    db: &DatabaseConnection,
    day: NaiveDate,
    visitor: &str,
) -> Result<Option<i64>, sea_orm::DbErr> {
    let txn = db.begin().await?;
    let bumped = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            SITE_UNIQUE_SQL,
            [
                SeaValue::from(day),
                SeaValue::from(SITE_PATH.to_string()),
                SeaValue::from(visitor.to_string()),
            ],
        ))
        .await?;
    let ordinal = match bumped {
        None => read_visitor_ordinal(&txn, day, visitor).await?,
        Some(row) => {
            let ordinal = row
                .try_get::<i64>("", "unique_visitors")
                .ok()
                .filter(|n| *n > 0);
            if let Some(n) = ordinal {
                txn.execute_raw(Statement::from_sql_and_values(
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
            ordinal
        }
    };
    txn.commit().await?;
    Ok(ordinal)
}

/// Internal event name: marks "this visitor already contributed engaged_views
/// for path today". Not shown in public event list.
pub(crate) const ENGAGE_MARKER: &str = "__engage__";

/// First engagement report for (day, path, visitor) → +1 engaged_views.
/// Later soft-flushes only add ms (otherwise avg time and bounce break).
pub(super) async fn bump_engagement(
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
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        ENGAGEMENT_SQL,
        [
            SeaValue::from(day),
            SeaValue::from(ENGAGE_MARKER.to_string()),
            SeaValue::from(path.to_string()),
            SeaValue::from(visitor.to_string()),
            SeaValue::from(ms),
        ],
    ))
    .await?;
    Ok(())
}

pub(super) async fn bump_event(
    db: &DatabaseConnection,
    day: NaiveDate,
    name: &str,
    path: &str,
    target: &str,
    visitor: &str,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        EVENT_SQL,
        [
            SeaValue::from(day),
            SeaValue::from(name.to_string()),
            SeaValue::from(path.to_string()),
            SeaValue::from(target.to_string()),
            SeaValue::from(visitor.to_string()),
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

pub(super) async fn bump_country(
    db: &DatabaseConnection,
    day: NaiveDate,
    country: &CountryInfo,
    visitor: &str,
    count_view: bool,
) -> Result<(), sea_orm::DbErr> {
    let view_inc: i64 = if count_view { 1 } else { 0 };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        COUNTRY_SQL,
        [
            SeaValue::from(day),
            SeaValue::from(country.code.clone()),
            SeaValue::from(visitor.to_string()),
            SeaValue::from(country.name.clone()),
            SeaValue::from(view_inc),
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

pub(super) struct IntakeCtx {
    db: DatabaseConnection,
    visitor: String,
    day: NaiveDate,
    country: Option<CountryInfo>,
}

/// `true` when the write landed; a failure is logged, never surfaced.
///
/// Intake is best-effort for the client on purpose. The browser flush treats
/// any 5xx as "retry the whole batch" (`frontend/src/utils/siteAnalytics.ts`),
/// and views / event counts are not idempotent, so failing the request over
/// one side counter would double-count every item that did land. Each write is
/// atomic on its own (see the SQL above), so a logged failure loses exactly
/// that one increment and never leaves a seen row without its count.
fn intake_write_ok<T>(write: &'static str, result: Result<T, sea_orm::DbErr>) -> bool {
    match result {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(write, %error, "analytics intake write failed");
            false
        }
    }
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
                intake_write_ok(
                    "site_unique",
                    record_site_unique(&ctx.db, ctx.day, &ctx.visitor).await,
                );
                let dup = is_duplicate_view(&ctx.visitor, &path).await;
                if intake_write_ok(
                    "pageview",
                    bump_pageview(&ctx.db, ctx.day, &path, &ctx.visitor, !dup).await,
                ) {
                    accepted += 1;
                    if let Some(ref country) = ctx.country {
                        intake_write_ok(
                            "country",
                            bump_country(&ctx.db, ctx.day, country, &ctx.visitor, !dup).await,
                        );
                    }
                }
                if let Some(host) = item.referrer.as_deref().and_then(normalize_referrer_host) {
                    intake_write_ok("referrer", bump_referrer(&ctx.db, ctx.day, &host).await);
                }
            }
            "engagement" => {
                let Some(path) = item.path.as_deref().and_then(normalize_path) else {
                    continue;
                };
                let ms = item.ms.unwrap_or(0);
                if intake_write_ok(
                    "engagement",
                    bump_engagement(&ctx.db, ctx.day, &path, &ctx.visitor, ms).await,
                ) {
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
                if intake_write_ok(
                    "event",
                    bump_event(&ctx.db, ctx.day, &name, &path, &target, &ctx.visitor).await,
                ) {
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
                Json(AppError::fail_json("invalid_body")),
            ))
        })?;
    let body: T = serde_json::from_slice(&bytes).map_err(|_| {
        crate::error::HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("invalid_json")),
        ))
    })?;
    Ok((ip, ua, is_staff, body))
}

// ── Handlers ───────────────────────────────────────────────────────────────

/// Whether first-party visitor collection is enabled (default true).
pub(crate) fn analytics_collection_enabled(config: &DynamicConfig) -> bool {
    config.analytics_enabled
}

type IntakeResponse = (StatusCode, Json<Value>);

/// Body of one intake endpoint: where its `vid` is and how it flattens into
/// [`CollectItem`]s. Validation runs inside [`admit_intake`], after the
/// disabled / staff / bot / rate-limit gates and before the salt.
pub(super) trait IntakeBody: for<'de> Deserialize<'de> {
    fn vid(&self) -> Option<&str>;
    fn into_items(self) -> Result<Vec<CollectItem>, IntakeResponse>;
}

impl IntakeBody for CollectRequest {
    fn vid(&self) -> Option<&str> {
        self.vid.as_deref()
    }

    fn into_items(self) -> Result<Vec<CollectItem>, IntakeResponse> {
        if self.items.is_empty() {
            return Err((StatusCode::BAD_REQUEST, Json(AppError::fail_json("empty"))));
        }
        Ok(self.items)
    }
}

impl IntakeBody for PageviewRequest {
    fn vid(&self) -> Option<&str> {
        self.vid.as_deref()
    }

    fn into_items(self) -> Result<Vec<CollectItem>, IntakeResponse> {
        let Some(path) = normalize_path(&self.path) else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::fail_json("invalid_path")),
            ));
        };
        Ok(vec![CollectItem {
            kind: "pageview".into(),
            path: Some(path),
            referrer: self.referrer,
            ms: None,
            name: None,
            target: None,
        }])
    }
}

fn intake_skipped(reason: &'static str) -> IntakeResponse {
    (
        StatusCode::OK,
        Json(json!({ "success": true, "skipped": reason, "accepted": 0 })),
    )
}

/// The one gate chain for every intake endpoint, in order: body parse (with
/// staff detection) → collection disabled → staff → bot UA → per-IP rate
/// limit → body validation → visitor salt → country. `Err` is the response
/// to send as-is.
pub(super) async fn admit_intake<T: IntakeBody>(
    dynamic_config: &RwLock<DynamicConfig>,
    db: &DatabaseConnection,
    peer: SocketAddr,
    request: Request,
) -> Result<(IntakeCtx, Vec<CollectItem>), IntakeResponse> {
    let header_country = country_from_headers(request.headers(), Some(peer.ip()));
    let (ip, ua, is_staff, body) = parse_json_body::<T>(db, request).await.map_err(|e| {
        let status = StatusCode::from_u16(e.0.status_u16()).unwrap_or(StatusCode::BAD_REQUEST);
        (status, Json(e.0.to_json()))
    })?;

    if !analytics_collection_enabled(&*dynamic_config.read().await) {
        return Err(intake_skipped("disabled"));
    }
    if is_staff {
        return Err(intake_skipped("staff"));
    }
    if is_bot_ua(&ua) {
        return Err(intake_skipped("bot"));
    }

    let ip_key = ip
        .map(|i| i.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    if rate_limited(&ip_key).await {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(AppError::fail_json("rate_limited")),
        ));
    }

    let vid = body.vid().map(str::to_owned);
    let items = body.into_items()?;
    let Some(visitor) = resolve_visitor_hash(vid.as_deref(), ip, &ua) else {
        return Err(salt_unavailable_response());
    };
    let ctx = IntakeCtx {
        db: db.clone(),
        visitor,
        day: analytics_today(),
        country: resolve_country(ip, header_country).await,
    };
    Ok((ctx, items))
}

/// Write the admitted items, then invalidate caches and maybe prune.
async fn run_intake(ctx: &IntakeCtx, items: &[CollectItem]) -> usize {
    let accepted = process_items(ctx, items).await;
    if accepted > 0 {
        invalidate_summary_cache().await;
    }
    maybe_prune(&ctx.db).await;
    accepted
}

/// POST /api/analytics/collect — preferred batch endpoint.
pub async fn collect(
    axum::extract::State(dynamic_config): axum::extract::State<Arc<RwLock<DynamicConfig>>>,
    crate::extract::Db(db): crate::extract::Db,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> IntakeResponse {
    let (ctx, items) =
        match admit_intake::<CollectRequest>(&dynamic_config, &db, peer, request).await {
            Ok(admitted) => admitted,
            Err(response) => return response,
        };
    let accepted = run_intake(&ctx, &items).await;
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
) -> IntakeResponse {
    let (ctx, items) =
        match admit_intake::<PageviewRequest>(&dynamic_config, &db, peer, request).await {
            Ok(admitted) => admitted,
            Err(response) => return response,
        };
    let accepted = run_intake(&ctx, &items).await;
    let path = items.first().and_then(|item| item.path.clone());
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
) -> Result<i64, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
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
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "analytics distinct count produced no row".to_string())?;
    row.try_get("", "n").map_err(|error| error.to_string())
}

pub(crate) async fn sum_all_time_page_views(db: &DatabaseConnection) -> Result<i64, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT COALESCE(SUM(views), 0)::bigint AS views
FROM analytics_page_daily WHERE path <> $1
"#,
            [SeaValue::from(SITE_PATH.to_string())],
        ))
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "analytics all-time views produced no row".to_string())?;
    row.try_get("", "views").map_err(|error| error.to_string())
}

/// Percent change for 环比 tiles. `None` when previous is 0 and current > 0
/// (undefined baseline — UI shows "新" / new).
pub(crate) fn pct_change(current: i64, previous: i64) -> Option<f64> {
    if previous == 0 {
        if current == 0 { Some(0.0) } else { None }
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

    #[test]
    fn summary_and_visitor_card_propagate_query_errors() {
        let src = include_str!("admin_api.rs");
        let summary = src
            .split("pub(crate) async fn build_analytics_summary")
            .nth(1)
            .and_then(|rest| rest.split("pub(crate) fn vid_from_query").next())
            .expect("build_analytics_summary");
        assert!(summary.contains("Err(error) => return analytics_db_error(error)"));
        assert!(summary.contains("tokio::try_join!"));
        let card = src
            .split("pub(crate) async fn visitor_card_aggregate")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn get_visitor_card").next())
            .expect("visitor_card_aggregate");
        assert!(card.contains("analytics_rows_try!"));
        assert!(card.contains("analytics_count_try!"));
        assert!(card.contains("Result<Value, (StatusCode, Json<Value>)"));
    }
}
