//! Pure param parsing, text/URL helpers, ranking, and projections for agent data_read.
//!
//! Handlers keep DB/FS/HTTP. This module owns:
//! - brew.read / brew.article param projection
//! - allowWebSearch opt-in parsing
//! - HTML plain-text extraction
//! - fuzzy score / URL cleanup / domain extract
//! - netease keyword category + playlist relevance
//! - platform item since/limit filtering
//! - platform stats projections (bilibili/github/netease/steam)
//! - agent platform item extract (in-memory only; no raw-cache FS enrich)
//! - AI JSON string extract / legacy brew RSS parse / RSSHub radar-rules parse

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::HashMap;

static RE_HTML_TAG: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_WHITESPACE: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"\s+").unwrap());

// ── Scalar param helpers ────────────────────────────────────────────────────

/// Parse a JSON value as optional i32 (integer, unsigned, or numeric string).
pub fn parse_optional_i32(v: &Value) -> Option<i32> {
    if let Some(n) = v.as_i64() {
        return i32::try_from(n).ok();
    }
    if let Some(n) = v.as_u64() {
        return i32::try_from(n).ok();
    }
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i32>().ok())
}

/// Non-empty trimmed string from a JSON value.
pub fn parse_optional_str(v: &Value) -> Option<&str> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// Clamp a limit param to `1..=max`, defaulting when missing.
pub fn parse_limit_clamped(params: &HashMap<String, Value>, default: u64, max: u64) -> usize {
    params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(default)
        .clamp(1, max) as usize
}

// ── brew.read / brew.article ────────────────────────────────────────────────

/// Filters extracted from brew.read params (sourceId / source / sourceName / limit / since).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrewReadFilters {
    pub source_id: Option<i32>,
    pub source_name: Option<String>,
    pub limit: usize,
    pub since: Option<String>,
}

/// Align schema `source` with handler `sourceId` / `sourceName`.
///
/// - `sourceId` (int or numeric string) wins for id
/// - numeric `source` also resolves as id
/// - non-numeric `source` / `sourceName` resolve as name filter
pub fn parse_brew_read_filters(params: &HashMap<String, Value>) -> BrewReadFilters {
    let source_id = params
        .get("sourceId")
        .and_then(parse_optional_i32)
        .or_else(|| params.get("source").and_then(parse_optional_i32));

    let source_name = params
        .get("sourceName")
        .and_then(parse_optional_str)
        .map(|s| s.to_string())
        .or_else(|| {
            // Only treat `source` as a name when it is not a pure integer id
            params
                .get("source")
                .and_then(parse_optional_str)
                .filter(|s| s.parse::<i32>().is_err())
                .map(|s| s.to_string())
        });

    let limit = parse_limit_clamped(params, 50, 200);

    let since = params
        .get("since")
        .and_then(parse_optional_str)
        .map(|s| s.to_string());

    BrewReadFilters {
        source_id,
        source_name,
        limit,
        since,
    }
}

/// Resolve brew.article lookup keys: id (i32), guid/string id, or url/link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrewArticleLookup {
    pub item_id: Option<i32>,
    /// Raw articleId when not purely numeric (guid / link fallback)
    pub article_key: Option<String>,
    pub url: Option<String>,
    pub source_id: Option<i32>,
}

pub fn parse_brew_article_lookup(params: &HashMap<String, Value>) -> BrewArticleLookup {
    let article_id_val = params
        .get("articleId")
        .or_else(|| params.get("itemId"))
        .or_else(|| params.get("id"));

    let item_id = article_id_val.and_then(parse_optional_i32);
    let article_key = article_id_val
        .and_then(parse_optional_str)
        .filter(|_| item_id.is_none())
        .map(|s| s.to_string())
        .or_else(|| {
            // Keep string form of numeric id as guid fallback only when provided as string
            article_id_val
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });

    let url = params
        .get("url")
        .or_else(|| params.get("link"))
        .and_then(parse_optional_str)
        .map(|s| s.to_string());

    let source_id = params.get("sourceId").and_then(parse_optional_i32);

    BrewArticleLookup {
        item_id,
        article_key,
        url,
        source_id,
    }
}

/// Whether an item matches article lookup (id / guid / url).
pub fn article_lookup_matches(
    lookup: &BrewArticleLookup,
    item_id: i32,
    guid: &str,
    link: &str,
) -> bool {
    if let Some(id) = lookup.item_id {
        if item_id == id {
            return true;
        }
    }
    if let Some(ref key) = lookup.article_key {
        if guid == key.as_str() || link == key.as_str() {
            return true;
        }
    }
    if let Some(ref url) = lookup.url {
        if link == url.as_str() || guid == url.as_str() {
            return true;
        }
    }
    false
}

/// Opt-in flag for external/web search fallback (brew.generateReadingList).
///
/// Accepts allowWebSearch / useWebSearch / webSearch / allowExternal / external.
/// Default false — local miss must not force ai.webSearch.
pub fn parse_allow_web_search(params: &HashMap<String, Value>) -> bool {
    const KEYS: &[&str] = &[
        "allowWebSearch",
        "useWebSearch",
        "webSearch",
        "allowExternal",
        "external",
    ];
    for key in KEYS {
        if let Some(v) = params.get(*key) {
            if v.as_bool() == Some(true) {
                return true;
            }
            if matches!(v.as_i64(), Some(1)) || matches!(v.as_u64(), Some(1)) {
                return true;
            }
            if let Some(s) = v.as_str() {
                let s = s.trim().to_lowercase();
                if matches!(s.as_str(), "true" | "1" | "yes" | "on") {
                    return true;
                }
            }
        }
    }
    false
}

// ── Text / URL ──────────────────────────────────────────────────────────────

/// Strip HTML tags and decode common entities for plain-text previews.
pub fn extract_plain_text(html: &str) -> String {
    let text = RE_HTML_TAG.replace_all(html, " ");
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"");
    RE_WHITESPACE.replace_all(&text, " ").trim().to_string()
}

/// Whether a string looks like a URL / domain for brew.discover.
///
/// Accepts http(s), `www.`, and short host-like tokens with a dot.
pub fn looks_like_url(s: &str) -> bool {
    let s_lower = s.trim().to_lowercase();
    s_lower.starts_with("http://")
        || s_lower.starts_with("https://")
        || s_lower.starts_with("www.")
        || (s_lower.contains('.') && !s_lower.contains(' ') && s_lower.len() < 100)
}

/// Extract host from a URL for display (strips scheme/www, first path segment).
pub fn extract_domain_from_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or("未知来源")
        .to_string()
}

/// Clean search result URLs: unwrap Google redirects, drop AMP/cache, strip tracking.
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
            } else {
                return format!("https://{cleaned}");
            }
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
            "ref",
            "source",
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
        } else {
            return format!("{base_url}?{}", clean_params.join("&"));
        }
    }

    url.to_string()
}

/// Extract a JSON array from free-form AI text (raw, markdown fences).
pub fn extract_json_array_from_ai_response(text: &str) -> Vec<Value> {
    let json_start = text.find('[');
    let json_end = text.rfind(']');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        if end > start {
            let json_str = &text[start..=end];
            if let Ok(arr) = serde_json::from_str::<Vec<Value>>(json_str) {
                return arr;
            }
        }
    }

    if text.contains("```json") {
        let parts: Vec<&str> = text.split("```json").collect();
        if parts.len() > 1 {
            if let Some(json_part) = parts[1].split("```").next() {
                if let Ok(arr) = serde_json::from_str::<Vec<Value>>(json_part.trim()) {
                    return arr;
                }
            }
        }
    }

    if text.contains("```") {
        let parts: Vec<&str> = text.split("```").collect();
        for part in parts {
            let trimmed = part.trim();
            if trimmed.starts_with('[') {
                if let Ok(arr) = serde_json::from_str::<Vec<Value>>(trimmed) {
                    return arr;
                }
            }
        }
    }

    vec![]
}

// ── Fuzzy search score ──────────────────────────────────────────────────────

/// Fuzzy match score in `0.0..=1.0` (exact > contains > word > char overlap).
pub fn calculate_fuzzy_score(query: &str, target: &str) -> f64 {
    if query == target {
        return 1.0;
    }

    if target.contains(query) {
        let ratio = query.len() as f64 / target.len() as f64;
        return 0.7 + (ratio * 0.3);
    }

    let query_words: Vec<&str> = query.split_whitespace().collect();
    let target_words: Vec<&str> = target.split_whitespace().collect();

    let mut matched_words = 0;
    for qw in &query_words {
        for tw in &target_words {
            if tw.contains(qw) || qw.contains(tw) {
                matched_words += 1;
                break;
            }
        }
    }

    if !query_words.is_empty() {
        let word_ratio = matched_words as f64 / query_words.len() as f64;
        if word_ratio > 0.0 {
            return 0.3 + (word_ratio * 0.4);
        }
    }

    let query_chars: Vec<char> = query.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    let mut matches = 0;
    for qc in &query_chars {
        if target_chars.contains(qc) {
            matches += 1;
        }
    }

    if !query_chars.is_empty() {
        let char_ratio = matches as f64 / query_chars.len() as f64;
        return char_ratio * 0.3;
    }

    0.0
}

// ── Platform item post-filters ──────────────────────────────────────────────

/// Apply RFC3339 `since` filter and optional `limit` to platform item lists.
pub fn filter_items_by_since_and_limit(
    mut items: Vec<Value>,
    since: Option<&str>,
    limit: Option<u64>,
) -> Vec<Value> {
    if let Some(since) = since {
        if let Ok(since_time) = chrono::DateTime::parse_from_rfc3339(since) {
            items.retain(|item| {
                item.get("createdAt")
                    .or_else(|| item.get("created_at"))
                    .or_else(|| item.get("timestamp"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t >= since_time)
                    .unwrap_or(true)
            });
        }
    }
    if let Some(limit) = limit {
        items.truncate(limit as usize);
    }
    items
}

// ── Netease playlist ranking ────────────────────────────────────────────────

/// Map a free-text music intent keyword to a Netease playlist category.
pub fn map_keyword_to_netease_category(keyword: &str) -> String {
    let keyword_lower = keyword.to_lowercase();

    let mappings: Vec<(&[&str], &str)> = vec![
        (
            &["工作", "办公", "专注", "学习", "编程", "coding"],
            "轻音乐",
        ),
        (&["睡眠", "睡前", "入睡", "助眠", "安静"], "轻音乐"),
        (&["放松", "舒缓", "休闲", "轻松"], "轻音乐"),
        (&["运动", "健身", "跑步", "动感", "激情"], "电子"),
        (&["古风", "中国风", "国风"], "古风"),
        (&["摇滚", "rock"], "摇滚"),
        (&["民谣", "folk"], "民谣"),
        (&["电子", "edm", "electronic", "dj"], "电子"),
        (&["说唱", "嘻哈", "rap", "hip-hop"], "说唱"),
        (&["流行", "pop", "热门"], "流行"),
        (&["古典", "classical", "钢琴", "交响"], "古典"),
        (&["爵士", "jazz"], "爵士"),
        (&["蓝调", "blues"], "蓝调"),
        (&["乡村", "country"], "乡村"),
        (&["acg", "动漫", "二次元", "日语"], "ACG"),
        (&["华语", "中文", "国语"], "华语"),
        (&["英文", "欧美", "英语"], "欧美"),
        (&["日语", "日本", "日系"], "日语"),
        (&["韩语", "韩国", "韩流", "kpop"], "韩语"),
    ];

    for (keywords, category) in mappings {
        for k in keywords {
            if keyword_lower.contains(k) {
                return category.to_string();
            }
        }
    }

    keyword.to_string()
}

/// Related/synonym terms for playlist relevance scoring.
pub fn get_related_terms(keyword: &str) -> Vec<&'static str> {
    let term_groups: &[&[&str]] = &[
        &[
            "放松", "轻松", "舒缓", "休息", "休闲", "慵懒", "惬意", "chill",
        ],
        &["安静", "静心", "静谧", "宁静", "平静", "冥想", "禅"],
        &[
            "学习", "阅读", "读书", "看书", "工作", "专注", "集中", "效率", "coding", "编程",
        ],
        &["睡眠", "助眠", "入睡", "晚安", "深夜", "夜晚", "催眠"],
        &["运动", "健身", "跑步", "锻炼", "燃脂", "有氧", "gym"],
        &[
            "轻音乐",
            "纯音乐",
            "器乐",
            "钢琴",
            "吉他",
            "小提琴",
            "无人声",
        ],
        &["治愈", "温暖", "温馨", "舒适", "暖心", "感动"],
        &["伤感", "难过", "悲伤", "失恋", "分手", "孤独", "寂寞"],
        &["欢快", "开心", "快乐", "愉悦", "活力", "元气", "阳光"],
        &["acg", "动漫", "二次元", "日漫", "番剧", "游戏", "anime"],
    ];

    for group in term_groups {
        if group
            .iter()
            .any(|t| keyword.contains(t) || t.contains(keyword))
        {
            return group.iter().filter(|&&t| t != keyword).copied().collect();
        }
    }

    Vec::new()
}

/// Score playlist JSON against a keyword (name/desc/tags/playCount).
pub fn score_playlist_relevance(playlist: &Value, keyword: &str) -> i32 {
    let mut score = 0;

    if let Some(name) = playlist.get("name").and_then(|n| n.as_str()) {
        let name_lower = name.to_lowercase();
        if name_lower.contains(keyword) {
            score += 100;
        }
        for word in keyword.split_whitespace() {
            if name_lower.contains(word) {
                score += 30;
            }
        }
    }

    if let Some(desc) = playlist.get("description").and_then(|d| d.as_str()) {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains(keyword) {
            score += 50;
        }
        for term in get_related_terms(keyword) {
            if desc_lower.contains(term) {
                score += 15;
            }
        }
    }

    if let Some(tags) = playlist.get("tags").and_then(|t| t.as_array()) {
        for tag in tags {
            if let Some(tag_str) = tag.as_str() {
                if tag_str.to_lowercase().contains(keyword) {
                    score += 80;
                }
                for term in get_related_terms(keyword) {
                    if tag_str.to_lowercase().contains(term) {
                        score += 25;
                    }
                }
            }
        }
    }

    if let Some(play_count) = playlist.get("playCount").and_then(|p| p.as_i64()) {
        score += (play_count / 1_000_000).min(20) as i32;
    }

    score
}

// ── time.info pure projection ───────────────────────────────────────────────

/// Weekday label (zh) for a chrono weekday.
pub fn weekday_zh(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    }
}

/// Project a UTC instant into the wall clock of `timezone`.
///
/// Accepts IANA names (`Asia/Shanghai`), `UTC`/`Z`, `local`, and fixed offsets
/// (`+08:00`, `UTC+8`). Unknown zones fail instead of echoing UTC fields.
pub fn project_time_info(
    now: chrono::DateTime<chrono::Utc>,
    timezone: &str,
) -> Result<Value, String> {
    use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
    use chrono_tz::Tz;
    use std::str::FromStr;

    let label = timezone.trim();
    if label.is_empty() {
        return Err("Missing timezone".to_string());
    }

    fn pack<Z: TimeZone>(now: DateTime<chrono::Utc>, zoned: DateTime<Z>, timezone: &str) -> Value
    where
        Z::Offset: std::fmt::Display,
    {
        json!({
            "datetime": zoned.to_rfc3339(),
            "timestamp": now.timestamp(),
            "timezone": timezone,
            "weekday": weekday_zh(zoned.weekday()),
            "year": zoned.year(),
            "month": zoned.month(),
            "day": zoned.day(),
            "hour": zoned.hour(),
            "minute": zoned.minute()
        })
    }

    if label.eq_ignore_ascii_case("utc") || label.eq_ignore_ascii_case("z") {
        return Ok(pack(now, now, "UTC"));
    }
    if label.eq_ignore_ascii_case("local") {
        return Ok(pack(now, now.with_timezone(&Local), "local"));
    }
    if let Ok(offset) = parse_fixed_offset(label) {
        return Ok(pack(now, now.with_timezone(&offset), label));
    }
    let tz = Tz::from_str(label).map_err(|_| {
        format!("Unknown timezone '{label}': use IANA (Asia/Shanghai), UTC, local, or +08:00")
    })?;
    Ok(pack(now, now.with_timezone(&tz), label))
}

fn parse_fixed_offset(raw: &str) -> Result<chrono::FixedOffset, String> {
    let s = raw.trim();
    let body = s
        .strip_prefix("UTC")
        .or_else(|| s.strip_prefix("utc"))
        .or_else(|| s.strip_prefix("GMT"))
        .or_else(|| s.strip_prefix("gmt"))
        .unwrap_or(s)
        .trim();
    let (sign, rest) = if let Some(r) = body.strip_prefix('+') {
        (1i32, r)
    } else if let Some(r) = body.strip_prefix('-') {
        (-1i32, r)
    } else {
        return Err(format!("not a fixed offset: {raw}"));
    };
    let rest = rest.trim();
    let (hh, mm) = if let Some((h, m)) = rest.split_once(':') {
        (
            h.parse::<i32>()
                .map_err(|_| format!("Invalid timezone hour in '{raw}'"))?,
            m.parse::<i32>()
                .map_err(|_| format!("Invalid timezone minute in '{raw}'"))?,
        )
    } else {
        let h = rest
            .parse::<i32>()
            .map_err(|_| format!("Invalid timezone hour in '{raw}'"))?;
        (h, 0)
    };
    if !(0..=14).contains(&hh) || !(0..60).contains(&mm) {
        return Err(format!("Timezone offset out of range: '{raw}'"));
    }
    let secs = sign * (hh * 3600 + mm * 60);
    chrono::FixedOffset::east_opt(secs).ok_or_else(|| format!("Invalid timezone offset: '{raw}'"))
}

// ── Platform stats projections ──────────────────────────────────────────────

pub fn analyze_bilibili_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");

    // 获取番剧分析
    let anime_analysis = content_analysis
        .and_then(|v| v.get("anime_analysis"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let anime_count = anime_analysis.len();

    // 统计番剧类型分布
    let mut genre_distribution: HashMap<String, usize> = HashMap::new();
    for anime in &anime_analysis {
        if let Some(genres) = anime.get("genres").and_then(|v| v.as_object()) {
            for genre in genres.keys() {
                *genre_distribution.entry(genre.clone()).or_default() += 1;
            }
        }
    }

    // 获取观看进度统计
    let mut completed = 0;
    let mut watching = 0;
    for anime in &anime_analysis {
        if let Some(progress) = anime.get("progress").and_then(|v| v.as_str()) {
            if progress.contains("已看完") || progress.contains("全部") {
                completed += 1;
            } else {
                watching += 1;
            }
        }
    }

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = content_analysis
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "bilibili",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_anime": anime_count,
            "completed": completed,
            "watching": watching
        },
        "distribution": {
            "by_genre": genre_distribution
        },
        "top_anime": anime_analysis.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析 GitHub 统计数据
pub fn analyze_github_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");

    // 语言分布
    let language_distribution = content_analysis
        .and_then(|v| v.get("language_distribution"))
        .cloned()
        .unwrap_or(json!({}));

    // 仓库数据
    let repositories = content_analysis
        .and_then(|v| v.get("repositories"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let repo_count = repositories.len();

    // 计算总 stars
    let total_stars: u64 = repositories
        .iter()
        .filter_map(|r| r.get("stars").and_then(|v| v.as_u64()))
        .sum();

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = content_analysis
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "github",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_repos": repo_count,
            "total_stars": total_stars
        },
        "distribution": {
            "by_language": language_distribution
        },
        "top_repos": repositories.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析网易云音乐统计数据
pub fn analyze_netease_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");
    let artist_analysis = content_analysis.and_then(|v| v.get("artist_analysis"));

    // 获取 Top 艺术家
    let top_artists = artist_analysis
        .and_then(|v| v.get("top_artists"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 风格分布
    let genre_analysis = artist_analysis
        .and_then(|v| v.get("genre_analysis"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 统计播放次数
    let total_plays: u64 = top_artists
        .iter()
        .filter_map(|a| a.get("play_count").and_then(|v| v.as_u64()))
        .sum();

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let music_summary = content_analysis
        .and_then(|v| v.get("music_summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "netease",
        "username": username,
        "summary": music_summary,
        "statistics": {
            "total_artists": top_artists.len(),
            "total_plays": total_plays,
            "genres_count": genre_analysis.len()
        },
        "distribution": {
            "by_genre": genre_analysis
        },
        "top_artists": top_artists.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析 Steam 游戏统计数据
pub fn analyze_steam_stats(data: &Value) -> Result<Value, String> {
    // 获取游戏列表
    let recent_games = data
        .get("content_analysis")
        .and_then(|v| v.get("recent_games"))
        .and_then(|v| v.as_array());

    // 计算游戏时间分布
    let mut total_playtime: u64 = 0;
    let mut game_count = 0;
    let mut playtime_distribution: HashMap<String, u64> = HashMap::new();
    let mut games_by_time: Vec<(String, u64)> = Vec::new();

    // 从 recent_games 获取数据
    if let Some(games) = recent_games {
        for game in games {
            let name = game
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let playtime = game.get("playtime").and_then(|v| v.as_u64()).unwrap_or(0);

            total_playtime += playtime;
            game_count += 1;
            games_by_time.push((name.to_string(), playtime));
        }
    }

    // 排序（按游戏时间降序）
    games_by_time.sort_by_key(|b| Reverse(b.1));

    // 计算时间段分布
    for (_, playtime) in &games_by_time {
        let hours = *playtime / 60;
        let category = match hours {
            0..=10 => "少于10小时",
            11..=50 => "10-50小时",
            51..=100 => "50-100小时",
            101..=200 => "100-200小时",
            _ => "200小时以上",
        };
        *playtime_distribution
            .entry(category.to_string())
            .or_default() += 1;
    }

    // 获取 Top 10 游戏
    let top_games: Vec<Value> = games_by_time
        .iter()
        .take(10)
        .map(|(name, playtime)| {
            json!({
                "name": name,
                "playtime_minutes": playtime,
                "playtime_hours": *playtime as f64 / 60.0
            })
        })
        .collect();

    // 获取类型分析
    let genre_analysis = data
        .get("content_analysis")
        .and_then(|v| v.get("genre_analysis"))
        .cloned()
        .unwrap_or(json!([]));

    // 获取用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = data
        .get("content_analysis")
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "steam",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_games": game_count,
            "total_playtime_hours": total_playtime as f64 / 60.0
        },
        "distribution": {
            "by_playtime": playtime_distribution,
            "by_genre": genre_analysis
        },
        "top_games": top_games
    }))
}

/// Project platform filtered-cache `content_analysis` into agent `items[]`.
///
/// Pure in-memory projection only (no filesystem / network). Matches the
/// pre-extraction agent handler shapes for steam / bilibili / youtube /
/// bangumi / x / netease / github / discord.
pub fn agent_extract_platform_items(platform: &str, data: &Value) -> Vec<Value> {
    let platform = platform.to_ascii_lowercase();
    match platform.as_str() {
        "steam" => {
            let mut items = Vec::new();
            if let Some(games) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_games"))
                .and_then(|v| v.as_array())
            {
                for game in games {
                    items.push(json!({
                        "type": "game",
                        "name": game.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "playtime_minutes": game.get("playtime").and_then(|v| v.as_u64()).unwrap_or(0),
                        "appid": game.get("appid"),
                        "icon_url": game.get("icon_url")
                    }));
                }
            }
            items
        }
        "bilibili" => {
            let mut items = Vec::new();
            if let Some(anime_list) = data
                .get("content_analysis")
                .and_then(|v| v.get("anime_analysis"))
                .and_then(|v| v.as_array())
            {
                for anime in anime_list {
                    items.push(json!({
                        "type": "anime",
                        "title": anime.get("title").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "progress": anime.get("progress"),
                        "genres": anime.get("genres")
                    }));
                }
            }
            items
        }
        "github" => {
            let mut items = Vec::new();
            if let Some(repos) = data
                .get("content_analysis")
                .and_then(|v| v.get("repositories"))
                .and_then(|v| v.as_array())
            {
                items.extend(repos.clone());
            }
            items
        }
        // Filtered YouTube cache: content_analysis.recent_videos (public uploads)
        "youtube" => {
            let mut items = Vec::new();
            if let Some(videos) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_videos"))
                .and_then(|v| v.as_array())
            {
                for video in videos {
                    let video_id = video.get("video_id").and_then(|v| v.as_str()).unwrap_or("");
                    items.push(json!({
                        "type": "video",
                        "title": video.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled"),
                        "video_id": video_id,
                        "cover": video.get("cover"),
                        "view_count": video.get("view_count"),
                        "like_count": video.get("like_count"),
                        "published_at": video.get("published_at"),
                        "url": video.get("url").cloned().unwrap_or_else(|| {
                            if video_id.is_empty() {
                                Value::Null
                            } else {
                                json!(format!("https://www.youtube.com/watch?v={video_id}"))
                            }
                        }),
                    }));
                }
            }
            items
        }
        "netease" | "netease_music" => {
            let mut items = Vec::new();
            if let Some(artists) = data
                .get("content_analysis")
                .and_then(|v| v.get("artist_analysis"))
                .and_then(|v| v.get("top_artists"))
                .and_then(|v| v.as_array())
            {
                for artist in artists {
                    items.push(json!({
                        "type": "artist",
                        "name": artist.get("name"),
                        "play_count": artist.get("play_count")
                    }));
                }
            }
            items
        }
        // Bangumi / MAL 过滤结果同构：top_rated / watching / recent
        "bangumi" | "mal" => {
            let mut items = Vec::new();
            let content = data.get("content_analysis");
            for key in ["top_rated_subjects", "watching_subjects", "recent_updates"] {
                if let Some(subjects) = content.and_then(|v| v.get(key)).and_then(|v| v.as_array())
                {
                    for subject in subjects {
                        items.push(json!({
                            "type": subject.get("subject_type").and_then(|v| v.as_str()).unwrap_or("subject"),
                            "title": subject.get("title"),
                            "rate": subject.get("rate"),
                            "collection_type": subject.get("collection_type"),
                            "subject_id": subject.get("subject_id"),
                            "platform": platform
                        }));
                    }
                }
            }
            items
        }
        "x" => data
            .get("content_analysis")
            .and_then(|v| v.get("top_posts").or_else(|| v.get("recent_posts")))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "discord" => data
            .get("content_analysis")
            .and_then(|v| v.get("guilds_preview"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => data
            .get("items")
            .or_else(|| data.get("data"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

pub fn extract_json_from_response(response: &str) -> Option<String> {
    // 尝试找到 JSON 代码块
    if let Some(start) = response.find("```json") {
        let content_start = start + 7;
        if let Some(end) = response[content_start..].find("```") {
            return Some(
                response[content_start..content_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // 尝试找到普通代码块
    if let Some(start) = response.find("```") {
        let content_start = start + 3;
        // 跳过可能的语言标识
        let actual_start = response[content_start..]
            .find('\n')
            .map(|n| content_start + n + 1)
            .unwrap_or(content_start);
        if let Some(end) = response[actual_start..].find("```") {
            return Some(
                response[actual_start..actual_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // 尝试直接解析为 JSON（查找 { 和 } 的匹配）
    if let Some(start) = response.find('{') {
        let mut depth = 0;
        let mut end_pos = start;
        for (i, ch) in response[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end_pos > start {
            return Some(response[start..end_pos].to_string());
        }
    }

    None
}

/// 解析 RSS/Atom 内容 (legacy cache helper; DB path preferred for brew.*)
#[allow(dead_code)]
pub fn parse_brew_content(content: &str) -> Vec<Value> {
    let mut items = Vec::new();

    // 简单的正则提取
    let item_pattern = regex::Regex::new(r"(?s)<(?:item|entry)>(.*?)</(?:item|entry)>").ok();
    let title_re = regex::Regex::new(r"<title[^>]*>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</title>").ok();
    let link_re = regex::Regex::new(r#"<link[^>]*(?:href="([^"]+)"[^>]*)?>([^<]*)</link>"#).ok();
    let date_re = regex::Regex::new(r"<(?:pubDate|published|updated)>([^<]+)</").ok();

    if let Some(pattern) = item_pattern {
        for cap in pattern.captures_iter(content) {
            if let Some(item_content) = cap.get(1) {
                let item_str = item_content.as_str();

                let title = title_re
                    .as_ref()
                    .and_then(|r| r.captures(item_str))
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                let link = link_re
                    .as_ref()
                    .and_then(|r| r.captures(item_str))
                    .and_then(|c| c.get(1).or(c.get(2)))
                    .map(|m| m.as_str().to_string());

                let date = date_re
                    .as_ref()
                    .and_then(|r| r.captures(item_str))
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                items.push(json!({
                    "title": title,
                    "link": link,
                    "pubDate": date
                }));
            }
        }
    }

    items
}

pub fn parse_rsshub_radar_rules(content: &str) -> Value {
    let mut routes = Vec::new();

    // radar-rules.js 的格式大致为:
    // module.exports = {
    // 'zhihu.com': { _name: '知乎', daily: [{ title: '日报', ... }] },
    // ...
    // }

    // 使用正则提取域名和路由信息
    let domain_re = regex::Regex::new(r#"'([^']+\.[^']+)':\s*\{"#).unwrap();
    let name_re = regex::Regex::new(r#"_name:\s*['"]([^'"]+)['"]"#).unwrap();
    let route_re = regex::Regex::new(r#"(\w+):\s*\[\s*\{\s*title:\s*['"]([^'"]+)['"]"#).unwrap();
    let target_re = regex::Regex::new(r#"target:\s*['"]([^'"]+)['"]"#).unwrap();

    // 按域名块分割
    let blocks: Vec<&str> = content.split("': {").collect();

    for block in blocks.iter().skip(1) {
        // 提取域名
        let domain = if let Some(prev_part) = blocks.iter().find(|b| !block.starts_with(*b)) {
            // 从前一个块的末尾提取域名
            domain_re
                .captures(prev_part)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .unwrap_or("")
        } else {
            ""
        };

        // 提取名称
        let name = name_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        // 提取路由
        for cap in route_re.captures_iter(block) {
            let _route_key = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            // 提取 target（RSSHub 路径）
            if let Some(target_cap) = target_re.captures(block) {
                let target = target_cap.get(1).map(|m| m.as_str()).unwrap_or("");

                if !target.is_empty() && !name.is_empty() {
                    // 检查是否需要额外参数（路径中包含 :param 且不是可选的）
                    let requires_config = target.contains(":")
                        && !target.contains("?")
                        && target.matches(':').count() > 1;

                    routes.push(json!({
                        "name": format!("{} - {}", name, title),
                        "path": target,
                        "description": format!("{} 的 {} 订阅", name, title),
                        "domain": domain,
                        "requiresConfig": requires_config
                    }));
                }
            }
        }
    }

    json!(routes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_optional_i32_accepts_int_and_numeric_string() {
        assert_eq!(parse_optional_i32(&json!(42)), Some(42));
        assert_eq!(parse_optional_i32(&json!(42u64)), Some(42));
        assert_eq!(parse_optional_i32(&json!("7")), Some(7));
        assert_eq!(parse_optional_i32(&json!("  9  ")), Some(9));
        assert_eq!(parse_optional_i32(&json!("akiday")), None);
        assert_eq!(parse_optional_i32(&json!(null)), None);
    }

    #[test]
    fn brew_read_filters_align_source_and_source_id() {
        let mut params = HashMap::new();
        params.insert("sourceId".into(), json!(3));
        params.insert("limit".into(), json!(10));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.source_id, Some(3));
        assert_eq!(f.limit, 10);
        assert!(f.source_name.is_none());

        let mut params = HashMap::new();
        params.insert("source".into(), json!("12"));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.source_id, Some(12));
        assert!(f.source_name.is_none());

        let mut params = HashMap::new();
        params.insert("source".into(), json!("akiday"));
        let f = parse_brew_read_filters(&params);
        assert!(f.source_id.is_none());
        assert_eq!(f.source_name.as_deref(), Some("akiday"));

        let mut params = HashMap::new();
        params.insert("sourceName".into(), json!("天利"));
        params.insert("sourceId".into(), json!("5"));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.source_id, Some(5));
        assert_eq!(f.source_name.as_deref(), Some("天利"));

        let mut params = HashMap::new();
        params.insert("limit".into(), json!(9999));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.limit, 200); // clamped
    }

    #[test]
    fn brew_article_lookup_id_guid_url() {
        let mut params = HashMap::new();
        params.insert("articleId".into(), json!(101));
        let l = parse_brew_article_lookup(&params);
        assert_eq!(l.item_id, Some(101));
        assert!(l.url.is_none());

        let mut params = HashMap::new();
        params.insert("articleId".into(), json!("guid-abc"));
        let l = parse_brew_article_lookup(&params);
        assert!(l.item_id.is_none());
        assert_eq!(l.article_key.as_deref(), Some("guid-abc"));

        let mut params = HashMap::new();
        params.insert("url".into(), json!("https://example.com/post"));
        params.insert("sourceId".into(), json!(2));
        let l = parse_brew_article_lookup(&params);
        assert_eq!(l.url.as_deref(), Some("https://example.com/post"));
        assert_eq!(l.source_id, Some(2));

        let mut params = HashMap::new();
        params.insert("itemId".into(), json!("55"));
        let l = parse_brew_article_lookup(&params);
        assert_eq!(l.item_id, Some(55));
    }

    #[test]
    fn article_lookup_matches_by_id_guid_or_link() {
        let by_id = BrewArticleLookup {
            item_id: Some(7),
            article_key: None,
            url: None,
            source_id: None,
        };
        assert!(article_lookup_matches(&by_id, 7, "g", "https://x"));
        assert!(!article_lookup_matches(&by_id, 8, "g", "https://x"));

        let by_guid = BrewArticleLookup {
            item_id: None,
            article_key: Some("guid-1".into()),
            url: None,
            source_id: None,
        };
        assert!(article_lookup_matches(
            &by_guid,
            1,
            "guid-1",
            "https://other"
        ));
        assert!(article_lookup_matches(&by_guid, 1, "other", "guid-1"));

        let by_url = BrewArticleLookup {
            item_id: None,
            article_key: None,
            url: Some("https://example.com/a".into()),
            source_id: None,
        };
        assert!(article_lookup_matches(
            &by_url,
            1,
            "x",
            "https://example.com/a"
        ));
        assert!(!article_lookup_matches(
            &by_url,
            1,
            "x",
            "https://example.com/b"
        ));
    }

    #[test]
    fn extract_plain_text_strips_tags() {
        let plain = extract_plain_text("<p>Hello&nbsp;<b>world</b></p>");
        assert_eq!(plain, "Hello world");
    }

    #[test]
    fn allow_web_search_is_opt_in_only() {
        let empty = HashMap::new();
        assert!(!parse_allow_web_search(&empty));

        let mut params = HashMap::new();
        params.insert("allowWebSearch".into(), json!(false));
        assert!(!parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("allowWebSearch".into(), json!(true));
        assert!(parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("useWebSearch".into(), json!("yes"));
        assert!(parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("webSearch".into(), json!(1));
        assert!(parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("external".into(), json!("on"));
        assert!(parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("webSearch".into(), json!("no"));
        assert!(!parse_allow_web_search(&params));
    }

    #[test]
    fn fuzzy_score_ranks_exact_and_partial() {
        assert_eq!(calculate_fuzzy_score("hello", "hello"), 1.0);
        assert!(calculate_fuzzy_score("hell", "hello") > 0.7);
        assert!(calculate_fuzzy_score("foo bar", "foo baz") > 0.3);
        assert!(calculate_fuzzy_score("xyz", "abc") < 0.3);
    }

    #[test]
    fn url_cleanup_and_domain() {
        assert!(looks_like_url("https://example.com/a"));
        assert!(looks_like_url("www.example.com"));
        assert!(looks_like_url("example.com"));
        assert!(!looks_like_url("not a url"));
        assert!(!looks_like_url("has space.com"));
        assert_eq!(
            extract_domain_from_url("https://www.example.com/path"),
            "example.com"
        );

        let cleaned = clean_search_result_url("https://example.com/a?utm_source=x&id=1&fbclid=y");
        assert!(cleaned.contains("id=1"));
        assert!(!cleaned.contains("utm_source"));
        assert!(!cleaned.contains("fbclid"));

        let g = clean_search_result_url(
            "https://www.google.com/url?url=https%3A%2F%2Fexample.com%2Fpage&sa=U",
        );
        assert_eq!(g, "https://example.com/page");
    }

    #[test]
    fn json_array_extract_and_netease_ranking() {
        let arr = extract_json_array_from_ai_response("here is [{\"a\":1},{\"b\":2}] ok");
        assert_eq!(arr.len(), 2);
        let fenced = extract_json_array_from_ai_response("```json\n[1,2,3]\n```");
        assert_eq!(fenced.len(), 3);

        assert_eq!(map_keyword_to_netease_category("助眠音乐"), "轻音乐");
        assert_eq!(map_keyword_to_netease_category("rock night"), "摇滚");

        let pl = json!({
            "name": "专注编程 BGM",
            "description": "coding 工作学习",
            "tags": ["轻音乐", "专注"],
            "playCount": 5_000_000
        });
        assert!(score_playlist_relevance(&pl, "编程") > 50);
        assert!(!get_related_terms("放松").is_empty());
    }

    #[test]
    fn platform_since_and_limit_filter() {
        let items = vec![
            json!({"title": "old", "createdAt": "2020-01-01T00:00:00+00:00"}),
            json!({"title": "new", "createdAt": "2026-06-01T00:00:00+00:00"}),
            json!({"title": "mid", "created_at": "2025-01-01T00:00:00+00:00"}),
        ];
        let filtered =
            filter_items_by_since_and_limit(items, Some("2024-01-01T00:00:00+00:00"), Some(1));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["title"], "new");
    }
    #[test]
    fn platform_stats_projections_and_json_extract() {
        let bili = json!({
            "user_summary": {"username": "u"},
            "content_analysis": {
                "summary": "s",
                "anime_analysis": [
                    {"genres": {"战斗": true}, "progress": "已看完"},
                    {"genres": {"日常": true}, "progress": "12/24"}
                ]
            }
        });
        let out = analyze_bilibili_stats(&bili).unwrap();
        assert_eq!(out["platform"], "bilibili");
        assert_eq!(out["statistics"]["total_anime"], 2);
        assert_eq!(out["statistics"]["completed"], 1);

        let gh = json!({
            "user_summary": {"username": "dev"},
            "content_analysis": {
                "summary": "repos",
                "language_distribution": {"Rust": 3},
                "repositories": [{"stars": 10}, {"stars": 5}]
            }
        });
        let out = analyze_github_stats(&gh).unwrap();
        assert_eq!(out["statistics"]["total_stars"], 15);

        let ne = json!({
            "user_summary": {"username": "m"},
            "content_analysis": {
                "music_summary": "ok",
                "artist_analysis": {
                    "top_artists": [{"name": "A", "play_count": 3}],
                    "genre_analysis": [{"g": 1}]
                }
            }
        });
        let out = analyze_netease_stats(&ne).unwrap();
        assert_eq!(out["statistics"]["total_plays"], 3);

        let st = json!({
            "user_summary": {"username": "s"},
            "content_analysis": {
                "summary": "g",
                "recent_games": [{"name": "X", "playtime": 120}],
                "genre_analysis": []
            }
        });
        let out = analyze_steam_stats(&st).unwrap();
        assert_eq!(out["statistics"]["total_games"], 1);

        let js = extract_json_from_response("prefix ```json\n{\"a\":1}\n``` tail");
        assert!(js.unwrap().contains("\"a\""));
        let obj = extract_json_from_response("noise {\"k\":true} more");
        assert_eq!(obj.as_deref(), Some("{\"k\":true}"));

        let rss = parse_brew_content(
            "<item><title>T</title><link>https://x</link><pubDate>d</pubDate></item>",
        );
        assert_eq!(rss.len(), 1);
        assert_eq!(rss[0]["title"], "T");

        let radar = parse_rsshub_radar_rules("");
        assert!(radar.as_array().unwrap().is_empty());
    }

    #[test]
    fn agent_platform_items_preserves_pre_extraction_field_shapes() {
        // netease artists
        let netease = json!({
            "content_analysis": {
                "artist_analysis": {
                    "top_artists": [{"name": "YOASOBI", "play_count": 12}]
                }
            }
        });
        let items = agent_extract_platform_items("netease", &netease);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], "artist");
        assert_eq!(items[0]["name"], "YOASOBI");
        assert_eq!(items[0]["play_count"], 12);
        assert!(
            items[0].get("title").is_none(),
            "agent shape has name not title"
        );

        // steam: type/name/playtime_minutes/appid/icon_url
        let steam = json!({
            "content_analysis": {
                "recent_games": [{
                    "name": "Hades",
                    "playtime": 900,
                    "appid": 1145360,
                    "icon_url": "https://cdn.example/icon.jpg"
                }]
            }
        });
        let items = agent_extract_platform_items("steam", &steam);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], "game");
        assert_eq!(items[0]["name"], "Hades");
        assert_eq!(items[0]["playtime_minutes"], 900);
        assert_eq!(items[0]["appid"], 1145360);
        assert_eq!(items[0]["icon_url"], "https://cdn.example/icon.jpg");
        assert!(
            items[0].get("title").is_none(),
            "shared Tapp normalize must not rewrite steam agent shape"
        );

        // bilibili anime
        let bili = json!({
            "content_analysis": {
                "anime_analysis": [{
                    "title": "Spy x Family",
                    "progress": "12/12",
                    "genres": {"日常": true}
                }]
            }
        });
        let items = agent_extract_platform_items("bilibili", &bili);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], "anime");
        assert_eq!(items[0]["title"], "Spy x Family");
        assert_eq!(items[0]["progress"], "12/12");
        assert!(items[0]["genres"].is_object());

        // youtube: video_id + constructed url
        let yt = json!({
            "content_analysis": {
                "recent_videos": [{
                    "title": "Sample Upload",
                    "video_id": "dQw4w9WgXcQ",
                    "cover": "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg",
                    "view_count": 1000,
                    "like_count": 50
                }]
            }
        });
        let items = agent_extract_platform_items("youtube", &yt);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], "video");
        assert_eq!(items[0]["video_id"], "dQw4w9WgXcQ");
        assert_eq!(items[0]["title"], "Sample Upload");
        assert_eq!(items[0]["view_count"], 1000);
        assert_eq!(
            items[0]["url"],
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );

        // bangumi subjects
        let bgm = json!({
            "content_analysis": {
                "top_rated_subjects": [{
                    "title": "CLANNAD",
                    "subject_type": "anime",
                    "rate": 9,
                    "collection_type": "collect",
                    "subject_id": 123
                }],
                "watching_subjects": []
            }
        });
        let items = agent_extract_platform_items("bangumi", &bgm);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], "anime");
        assert_eq!(items[0]["title"], "CLANNAD");
        assert_eq!(items[0]["rate"], 9);
        assert_eq!(items[0]["subject_id"], 123);
        assert_eq!(items[0]["platform"], "bangumi");

        // x top_posts
        let x = json!({
            "content_analysis": {
                "top_posts": [{"id": "1", "text": "hello"}]
            }
        });
        let items = agent_extract_platform_items("x", &x);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "1");
        assert_eq!(items[0]["text"], "hello");
    }

    #[test]
    fn project_time_info_uses_supplied_clock_not_hidden_now() {
        use chrono::{TimeZone, Utc};
        let now = Utc.with_ymd_and_hms(2026, 7, 31, 12, 30, 0).unwrap();
        let out = project_time_info(now, "Asia/Shanghai").expect("valid zone");
        assert_eq!(out["timezone"], "Asia/Shanghai");
        assert_eq!(out["year"], 2026);
        assert_eq!(out["month"], 7);
        assert_eq!(out["day"], 31);
        assert_eq!(out["hour"], 20);
        assert_eq!(out["minute"], 30);
        assert_eq!(out["weekday"], "星期五"); // 2026-07-31 20:30 +08 is Friday
        assert_eq!(out["timestamp"], now.timestamp());
        assert!(out["datetime"]
            .as_str()
            .unwrap()
            .starts_with("2026-07-31T20:30:00"));
        assert!(project_time_info(now, "Not/AZone").is_err());
        let utc = project_time_info(now, "UTC").expect("utc");
        assert_eq!(utc["hour"], 12);
        let offset = project_time_info(now, "UTC+8").expect("offset");
        assert_eq!(offset["hour"], 20);
    }
}
