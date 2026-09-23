//! Library item models, paging, platform builders, and short-lived assembly cache.
//!
//! Keep DB I/O in the API layer; this module shapes and caches LibraryItem values.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::services::image_proxy_urls::proxy_image_url;

/// 资料库数据项
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibraryItem {
    pub id: String,
    pub item_type: String, // LIBRARY_ITEM_TYPES
    pub title: String,
    pub cover: Option<String>,
    pub platform: String,
    pub metadata: Value,
}

pub type CachedLibraryItems = Arc<Vec<LibraryItem>>;

struct LibraryAssemblyCache {
    user_id: i32,
    cached_at: Instant,
    items: CachedLibraryItems,
}

static LIBRARY_ASSEMBLY_CACHE: Lazy<RwLock<Option<LibraryAssemblyCache>>> =
    Lazy::new(|| RwLock::new(None));
const LIBRARY_ASSEMBLY_CACHE_TTL: Duration = Duration::from_secs(30);

pub fn cached_library_items(user_id: i32) -> Option<CachedLibraryItems> {
    let cache = LIBRARY_ASSEMBLY_CACHE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.as_ref().and_then(|entry| {
        (entry.user_id == user_id && entry.cached_at.elapsed() < LIBRARY_ASSEMBLY_CACHE_TTL)
            .then(|| Arc::clone(&entry.items))
    })
}

pub fn store_library_items(user_id: i32, items: Vec<LibraryItem>) -> CachedLibraryItems {
    // One pass: prefer_card_cover_url + slim_library_metadata.
    let items = Arc::new(
        items
            .into_iter()
            .map(normalize_library_item_for_client)
            .collect::<Vec<_>>(),
    );
    *LIBRARY_ASSEMBLY_CACHE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(LibraryAssemblyCache {
        user_id,
        cached_at: Instant::now(),
        items: Arc::clone(&items),
    });
    items
}

/// Prefer card-sized covers and drop bulk platform JSON before shipping to clients.
pub fn normalize_library_item_for_client(mut item: LibraryItem) -> LibraryItem {
    if let Some(cover) = item.cover.take() {
        item.cover = Some(prefer_card_cover_url(&cover));
    }
    item.metadata = slim_library_metadata(&item.metadata);
    item
}

/// Prefer Bangumi `/c/` and Netease `param=288y288`; skip Bangumi `/r/{width}/` resize paths.
pub fn prefer_card_cover_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }

    // Already-proxied covers embed the CDN host in ?url= — rewrite upstream first
    // so encoded paths like pic%2Fcover%2Fl%2F are not missed.
    if trimmed.contains("/api/proxy/image") && trimmed.contains("url=") {
        if let Some(preferred) = rewrite_proxied_cover_url(trimmed) {
            return preferred;
        }
        return trimmed.to_string();
    }

    prefer_raw_card_cover_url(trimmed)
}

fn prefer_raw_card_cover_url(trimmed: &str) -> String {
    // Bangumi: legacy /pic/cover/{l|c|m|s|g}/… — rewrite l|g → c.
    // Current API `/r/{width}/pic/cover/l/` — `l` is the source; skip resize paths.
    if trimmed.contains("bgm.tv") || trimmed.contains("lain.bgm") {
        if is_bangumi_resize_cover(trimmed) {
            return trimmed.to_string();
        }
        return trimmed
            .replace("/pic/cover/l/", "/pic/cover/c/")
            .replace("/pic/cover/g/", "/pic/cover/c/");
    }

    // Netease CDN accepts ?param=WxH; cap decode size without another hop.
    if trimmed.contains("music.126.net") || trimmed.contains("music.163.com") {
        return with_netease_card_size(trimmed);
    }

    if let Some(hdslb) = with_bilibili_card_size(trimmed) {
        return hdslb;
    }

    trimmed.to_string()
}

/// Card paint is ~220px. 288 is 1× plus ~20% slack (hover / canvas zoom).
const NETEASE_CARD_PARAM: &str = "288y288";
const NETEASE_CARD_EDGE: u32 = 288;

fn with_netease_card_size(url: &str) -> String {
    if let Some(after) = url.split_once("param=").map(|(_, rest)| rest) {
        let spec = after.split('&').next().unwrap_or(after);
        let mut parts = spec.split('y');
        let width = parts.next().and_then(|v| v.parse::<u32>().ok());
        let height = parts.next().and_then(|v| v.parse::<u32>().ok());
        if let (Some(width), Some(height)) = (width, height) {
            if width <= NETEASE_CARD_EDGE && height <= NETEASE_CARD_EDGE {
                return url.to_string();
            }
            return url.replacen(spec, NETEASE_CARD_PARAM, 1);
        }
    }
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}param={NETEASE_CARD_PARAM}")
}

/// Card paint is ~220px; 2× retina plus ~20% slack. Width-only so CSS object-fit keeps aspect.
const BILIBILI_CARD_WIDTH_SUFFIX: &str = "@528w.webp";

fn host_is_hdslb(url: &str) -> bool {
    let trimmed = url.trim();
    let rest = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| trimmed.strip_prefix("//").unwrap_or(trimmed));
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    host == "hdslb.com" || host.ends_with(".hdslb.com")
}

fn with_bilibili_card_size(url: &str) -> Option<String> {
    if !host_is_hdslb(url) {
        return None;
    }
    let (path, query) = url
        .split_once('?')
        .map(|(path, query)| (path, format!("?{query}")))
        .unwrap_or((url, String::new()));
    if path.contains('@') {
        return None;
    }
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".gif") || lower.ends_with(".svg") {
        return None;
    }
    Some(format!("{path}{BILIBILI_CARD_WIDTH_SUFFIX}{query}"))
}

/// True for Bangumi resize paths `/r/{width}/pic/cover/…` (width is ASCII digits).
fn is_bangumi_resize_cover(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let Some(start) = lower.find("/r/") else {
        return false;
    };
    let rest = &lower[start + 3..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && rest[digits..].starts_with("/pic/cover/")
}

fn rewrite_proxied_cover_url(proxied: &str) -> Option<String> {
    // Support relative `/api/proxy/image?url=…` and absolute forms.
    let query = proxied.split_once('?').map(|(_, q)| q)?;
    let mut upstream: Option<String> = None;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        if key != "url" {
            continue;
        }
        let raw = parts.next().unwrap_or("");
        upstream = Some(urlencoding_decode(raw));
        break;
    }
    let upstream = upstream?;
    let preferred = prefer_raw_card_cover_url(&upstream);
    if preferred == upstream {
        return None;
    }
    // Re-proxy so callers still hit the same-origin image proxy.
    Some(proxy_image_url(&preferred))
}

/// Minimal application/x-www-form-urlencoded decode for proxy `url` values.
fn urlencoding_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = |c: u8| -> Option<u8> {
                    match c {
                        b'0'..=b'9' => Some(c - b'0'),
                        b'a'..=b'f' => Some(c - b'a' + 10),
                        b'A'..=b'F' => Some(c - b'A' + 10),
                        _ => None,
                    }
                };
                if let (Some(a), Some(b)) = (h(bytes[i + 1]), h(bytes[i + 2])) {
                    out.push(char::from(a * 16 + b));
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(char::from(c));
                i += 1;
            }
        }
    }
    out
}

fn pick_str<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| obj.get(*k))
}

fn slim_nested_object(value: &Value, keys: &[&str]) -> Option<Value> {
    let obj = value.as_object()?;
    let mut out = serde_json::Map::new();
    for key in keys {
        if let Some(v) = obj.get(*key) {
            out.insert((*key).to_string(), v.clone());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}

fn slim_artist_list(value: &Value) -> Value {
    match value {
        Value::Array(arr) => Value::Array(
            arr.iter()
                .filter_map(|entry| {
                    if let Some(s) = entry.as_str() {
                        return Some(json!(s));
                    }
                    let name = entry
                        .as_object()
                        .and_then(|o| o.get("name"))
                        .and_then(|n| n.as_str())?;
                    Some(json!({ "name": name }))
                })
                .collect(),
        ),
        Value::String(s) => json!(s),
        other => other.clone(),
    }
}

fn slim_album(value: &Value) -> Value {
    match value {
        Value::Object(obj) => {
            let mut out = serde_json::Map::new();
            if let Some(name) = pick_str(obj, &["name"]) {
                out.insert("name".into(), name.clone());
            }
            if let Some(pic) = pick_str(obj, &["picUrl", "pic_url", "cover"]) {
                if let Some(s) = pic.as_str() {
                    out.insert("picUrl".into(), json!(prefer_card_cover_url(s)));
                } else {
                    out.insert("picUrl".into(), pic.clone());
                }
            }
            Value::Object(out)
        }
        Value::String(s) => json!(s),
        other => other.clone(),
    }
}

/// Keep FLAT_KEYS plus slim artist lists (display, links, progress, playback).
pub fn slim_library_metadata(metadata: &Value) -> Value {
    let Some(obj) = metadata.as_object() else {
        return metadata.clone();
    };

    let mut out = serde_json::Map::new();

    // Flat scalars used for display, links, progress, playback.
    const FLAT_KEYS: &[&str] = &[
        "id",
        "name",
        "url",
        "link",
        "web_url",
        "html_url",
        "short_link",
        "short_link_v2",
        "share_url",
        "appid",
        "season_id",
        "bvid",
        "aid",
        "subject_id",
        "media_type",
        "video_id",
        "full_name",
        "playtime_forever",
        "rate",
        "score",
        "artist",
        "dt",
        "duration",
        "fee",
        "isVip",
        "is_vip",
        "type",
        "status",
        "progress",
        "ep_status",
        "vol_status",
        "num_episodes_watched",
        "num_chapters_read",
        "num_volumes_read",
        "num_episodes",
        "num_chapters",
        "num_volumes",
        "platform",
    ];
    for key in FLAT_KEYS {
        if let Some(v) = obj.get(*key) {
            out.insert((*key).to_string(), v.clone());
        }
    }

    if let Some(ar) = obj.get("ar") {
        out.insert("ar".into(), slim_artist_list(ar));
    }
    if let Some(artists) = obj.get("artists") {
        out.insert("artists".into(), slim_artist_list(artists));
    }
    if let Some(al) = obj.get("al") {
        out.insert("al".into(), slim_album(al));
    }
    if let Some(album) = obj.get("album") {
        out.insert("album".into(), slim_album(album));
    }

    if let Some(privilege) = obj.get("privilege") {
        if let Some(fee) = privilege.get("fee") {
            out.insert("privilege".into(), json!({ "fee": fee }));
        }
    }

    if let Some(ls) = slim_nested_object(
        obj.get("list_status").unwrap_or(&Value::Null),
        &[
            "score",
            "status",
            "num_episodes_watched",
            "num_chapters_read",
            "num_volumes_read",
        ],
    ) {
        out.insert("list_status".into(), ls);
    }

    if let Some(subject) = slim_nested_object(
        obj.get("subject").unwrap_or(&Value::Null),
        &["id", "url", "eps", "volumes", "platform", "name", "name_cn"],
    ) {
        out.insert("subject".into(), subject);
    }

    if let Some(node) = slim_nested_object(
        obj.get("node").unwrap_or(&Value::Null),
        &[
            "id",
            "url",
            "title",
            "num_episodes",
            "num_chapters",
            "num_volumes",
        ],
    ) {
        out.insert("node".into(), node);
    }

    if let Some(owner) = slim_nested_object(obj.get("owner").unwrap_or(&Value::Null), &["login"]) {
        out.insert("owner".into(), owner);
    }

    Value::Object(out)
}

pub fn invalidate_library_assembly_cache() {
    *LIBRARY_ASSEMBLY_CACHE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

pub const LIBRARY_SOURCE_PREFERENCES_KEY: &str = "library_source_preferences";
pub const LIBRARY_ITEM_TYPES: [&str; 6] = ["game", "video", "music", "anime", "tv_series", "book"];
pub const LIBRARY_PLATFORMS: [&str; 5] = ["Steam", "Bilibili", "Bangumi", "Netease", "MyAnimeList"];

fn empty_library_type_counts() -> HashMap<String, usize> {
    LIBRARY_ITEM_TYPES
        .iter()
        .map(|item_type| ((*item_type).to_string(), 0))
        .collect()
}

/// Preference-filtered per-type totals. Does not clone item payloads.
pub fn count_library_items_by_type(
    items: &[LibraryItem],
    preferences: Option<&LibrarySourcePreferences>,
) -> HashMap<String, usize> {
    let mut counts = empty_library_type_counts();
    for item in items {
        let allowed = preferences
            .map(|preferences| preferences.source_enabled(&item.item_type, &item.platform))
            .unwrap_or(true);
        if !allowed {
            continue;
        }
        if let Some(slot) = counts.get_mut(&item.item_type) {
            *slot += 1;
        }
    }
    counts
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryLayout {
    #[default]
    List,
    Canvas,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibrarySourcePreferences {
    #[serde(default)]
    pub layout: LibraryLayout,
    #[serde(default = "default_library_source_categories")]
    pub categories: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct LibrarySourceOption {
    pub source: String,
    pub count: usize,
}

pub fn default_library_source_categories() -> HashMap<String, Vec<String>> {
    HashMap::from([
        (
            "game".to_string(),
            vec!["Steam".to_string(), "Bangumi".to_string()],
        ),
        (
            "video".to_string(),
            vec!["Bilibili".to_string(), "Bangumi".to_string()],
        ),
        (
            "music".to_string(),
            vec!["Netease".to_string(), "Bangumi".to_string()],
        ),
        (
            "anime".to_string(),
            vec![
                "Bangumi".to_string(),
                "Bilibili".to_string(),
                "MyAnimeList".to_string(),
            ],
        ),
        (
            "tv_series".to_string(),
            vec!["Bangumi".to_string(), "Bilibili".to_string()],
        ),
        (
            "book".to_string(),
            vec!["Bangumi".to_string(), "MyAnimeList".to_string()],
        ),
    ])
}

impl Default for LibrarySourcePreferences {
    fn default() -> Self {
        Self {
            layout: LibraryLayout::default(),
            categories: default_library_source_categories(),
        }
    }
}

impl LibrarySourcePreferences {
    pub fn normalized(mut self) -> Self {
        let defaults = default_library_source_categories();
        let mut normalized = HashMap::new();

        for item_type in LIBRARY_ITEM_TYPES {
            let sources = self
                .categories
                .remove(item_type)
                .unwrap_or_else(|| defaults.get(item_type).cloned().unwrap_or_default());
            normalized.insert(item_type.to_string(), normalize_platform_list(sources));
        }

        self.categories = normalized;
        self
    }

    pub fn enabled_sources_for(&self, item_type: &str) -> Vec<String> {
        self.categories.get(item_type).cloned().unwrap_or_else(|| {
            default_library_source_categories()
                .get(item_type)
                .cloned()
                .unwrap_or_default()
        })
    }

    pub fn source_enabled(&self, item_type: &str, platform: &str) -> bool {
        let platform = canonical_library_platform(platform);
        self.enabled_sources_for(item_type).contains(&platform)
    }
}

pub fn canonical_library_platform(platform: &str) -> String {
    let trimmed = platform.trim();
    let key = platform
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();

    match key.as_str() {
        "steam" => "Steam".to_string(),
        "bilibili" | "bili" => "Bilibili".to_string(),
        "bangumi" | "bgm" => "Bangumi".to_string(),
        "x" | "twitter" | "xtwitter" => "X".to_string(),
        "netease" | "neteasemusic" | "neteasecloudmusic" => "Netease".to_string(),
        "mal" | "myanimelist" => "MyAnimeList".to_string(),
        "xbox" => "Xbox".to_string(),
        "psn" | "playstation" => "PlayStation".to_string(),
        _ => trimmed.to_string(),
    }
}

pub fn normalize_platform_list(sources: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for source in sources {
        let platform = canonical_library_platform(&source);
        if !platform.is_empty() && seen.insert(platform.clone()) {
            normalized.push(platform);
        }
    }

    normalized
}

pub fn collect_library_source_options(
    items: &[LibraryItem],
) -> HashMap<String, Vec<LibrarySourceOption>> {
    let mut counts: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for item in items {
        let item_type = item.item_type.clone();
        let platform = canonical_library_platform(&item.platform);
        *counts
            .entry(item_type)
            .or_default()
            .entry(platform)
            .or_insert(0) += 1;
    }

    let platform_order = |source: &str| {
        LIBRARY_PLATFORMS
            .iter()
            .position(|candidate| candidate == &source)
            .unwrap_or(usize::MAX)
    };

    let mut options = HashMap::new();
    for item_type in LIBRARY_ITEM_TYPES {
        let mut source_options = counts
            .remove(item_type)
            .unwrap_or_default()
            .into_iter()
            .map(|(source, count)| LibrarySourceOption { source, count })
            .collect::<Vec<_>>();
        source_options.sort_by_key(|option| platform_order(&option.source));
        options.insert(item_type.to_string(), source_options);
    }

    options
}

#[derive(Debug)]
pub struct LibraryPage {
    pub items: Vec<LibraryItem>,
    pub total: usize,
    pub returned: usize,
    pub offset: usize,
    pub limit: Option<usize>,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

/// Same as the frontend `LIBRARY_PAGE_SIZE`. Missing `limit` used to dump the whole library.
pub const LIBRARY_DEFAULT_PAGE_LIMIT: usize = 120;

/// Filter by item type before slicing, so a typed page can never become a false empty state.
pub fn paginate_library_items(
    items: &[LibraryItem],
    preferences: Option<&LibrarySourcePreferences>,
    item_type: Option<&str>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<LibraryPage, &'static str> {
    if let Some(item_type) = item_type {
        if !LIBRARY_ITEM_TYPES.contains(&item_type) {
            return Err("Invalid library item type");
        }
    }

    let filtered = items
        .iter()
        .filter(|item| {
            preferences
                .map(|preferences| preferences.source_enabled(&item.item_type, &item.platform))
                .unwrap_or(true)
        })
        .filter(|item| {
            item_type
                .map(|item_type| item.item_type == item_type)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let total = filtered.len();
    let offset = offset.unwrap_or(0).min(total);
    let limit = limit.unwrap_or(LIBRARY_DEFAULT_PAGE_LIMIT).clamp(1, 200);
    let items: Vec<LibraryItem> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();
    let returned = items.len();
    let next = offset + returned;
    let has_more = next < total;

    Ok(LibraryPage {
        items,
        total,
        returned,
        offset,
        limit: Some(limit),
        has_more,
        next_offset: has_more.then_some(next),
    })
}

/// Canonical library type for Bangumi subject type codes.
/// Type 6 ("real"/实景) → tv_series when platform looks like TV drama, else video.
/// Called from `append_bangumi_library_items`. Reports use `bangumi_label_to_library_type`;
/// `platform_items` uses `bangumi_real_item_type`.
pub fn bangumi_library_item_type(subject_type: i64, platform: Option<&str>) -> &'static str {
    match subject_type {
        1 => "book",
        2 => "anime",
        3 => "music",
        4 => "game",
        6 => bangumi_real_item_type(platform),
        _ => "video",
    }
}

/// Map Bangumi "real" / type-6 to library vocabulary (tv_series | video).
pub fn bangumi_real_item_type(platform: Option<&str>) -> &'static str {
    let platform = platform.unwrap_or_default();
    if platform.contains("TV")
        || platform.contains("剧")
        || platform.contains("Drama")
        || platform.contains("电视剧")
        || platform.eq_ignore_ascii_case("tv")
    {
        "tv_series"
    } else {
        "video"
    }
}

/// Map smart_filter label (`real` / `anime` / …) to library item type.
pub fn bangumi_label_to_library_type(label: &str, platform: Option<&str>) -> &'static str {
    match label.trim().to_ascii_lowercase().as_str() {
        "book" => "book",
        "anime" => "anime",
        "music" => "music",
        "game" => "game",
        "real" | "tv_series" | "tv" => bangumi_real_item_type(platform),
        "video" => "video",
        _ => "video",
    }
}

/// Assemble library items from per-platform data. `take` hands over one
/// platform's owned value (DB metadata map or raw cache object), so entries are
/// moved into `LibraryItem.metadata` instead of cloned.
pub fn assemble_library_items(mut take: impl FnMut(&str) -> Option<Value>) -> Vec<LibraryItem> {
    let mut items = Vec::new();
    if let Some(steam) = take("steam") {
        append_steam_library_items(&mut items, steam);
    }
    if let Some(bilibili) = take("bilibili") {
        append_bilibili_library_items(&mut items, bilibili);
    }
    if let Some(netease) = take("netease") {
        append_netease_library_items(&mut items, netease);
    }
    if let Some(bangumi) = take("bangumi") {
        append_bangumi_library_items(&mut items, &bangumi);
    }
    if let Some(mal) = take("mal") {
        append_mal_library_items(&mut items, &mal);
    }
    items
}

/// Move an array field out of `value`; `None` when absent or not an array.
fn take_array(value: &mut Value, key: &str) -> Option<Vec<Value>> {
    match value.get_mut(key).map(Value::take) {
        Some(Value::Array(items)) => Some(items),
        _ => None,
    }
}

fn append_steam_library_items(library_items: &mut Vec<LibraryItem>, mut steam: Value) {
    let Some(games) = take_array(&mut steam, "games") else {
        return;
    };
    let total = games.len();
    for game in games {
        let (Some(appid), Some(name)) = (
            game.get("appid").and_then(|a| a.as_i64()),
            game.get("name").and_then(|n| n.as_str()).map(str::to_string),
        ) else {
            continue;
        };
        library_items.push(LibraryItem {
            id: format!("steam_game_{}", appid),
            item_type: "game".to_string(),
            title: name,
            cover: Some(format!(
                "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
                appid
            )),
            platform: "Steam".to_string(),
            metadata: game,
        });
    }
    tracing::info!("✓ Loaded {} Steam games", total);
}

/// Bilibili 追番/追剧与收藏夹视频。
fn append_bilibili_library_items(library_items: &mut Vec<LibraryItem>, mut bilibili: Value) {
    if let Some(bangumi) = take_array(&mut bilibili, "bangumi") {
        let total = bangumi.len();
        for mut item in bangumi {
            let (Some(season_id), Some(title), Some(cover)) = (
                item.get("season_id").and_then(|s| s.as_i64()),
                item.get("title").and_then(|t| t.as_str()).map(str::to_string),
                item.get("cover").and_then(|c| c.as_str()).map(proxy_image_url),
            ) else {
                continue;
            };
            // 根据season_type判断类型
            // 1=番剧(动画), 2=电视剧, 3=纪录片, 4=国创, 5=电影
            let season_type = item
                .get("season_type")
                .and_then(|s| s.as_i64())
                .unwrap_or(1);
            let item_type = match season_type {
                1 | 4 => "anime", // 番剧和国创归类为anime
                2 => "tv_series", // 电视剧
                3 | 5 => "video", // 纪录片和电影保持为video
                _ => "anime",     // 默认为anime
            };
            if let Some(obj) = item.as_object_mut() {
                obj.insert(
                    "url".to_string(),
                    json!(format!(
                        "https://www.bilibili.com/bangumi/play/ss{}",
                        season_id
                    )),
                );
            }
            library_items.push(LibraryItem {
                id: format!("bilibili_bangumi_{}", season_id),
                item_type: item_type.to_string(),
                title,
                cover: Some(cover),
                platform: "Bilibili".to_string(),
                metadata: item,
            });
        }
        tracing::info!("✓ Loaded {} Bilibili bangumi", total);
    }

    if let Some(favorites) = take_array(&mut bilibili, "favorites") {
        let folders = favorites.len();
        for mut folder in favorites {
            let Some(videos) = take_array(&mut folder, "videos") else {
                continue;
            };
            for mut video in videos {
                let (Some(bvid), Some(title), Some(cover)) = (
                    video.get("bvid").and_then(|b| b.as_str()).map(str::to_string),
                    video.get("title").and_then(|t| t.as_str()).map(str::to_string),
                    video.get("cover").and_then(|c| c.as_str()).map(proxy_image_url),
                ) else {
                    continue;
                };
                if let Some(obj) = video.as_object_mut() {
                    obj.insert(
                        "url".to_string(),
                        json!(format!("https://www.bilibili.com/video/{}", bvid)),
                    );
                }
                library_items.push(LibraryItem {
                    id: format!("bilibili_video_{}", bvid),
                    item_type: "video".to_string(),
                    title,
                    cover: Some(cover),
                    platform: "Bilibili".to_string(),
                    metadata: video,
                });
            }
        }
        tracing::info!("✓ Loaded {} Bilibili favorite folders", folders);
    }
}

/// 网易云 liked_songs；按歌曲 ID 去重，防止分片合并时产生重复歌曲。
fn append_netease_library_items(library_items: &mut Vec<LibraryItem>, mut netease: Value) {
    let Some(songs) = take_array(&mut netease, "liked_songs") else {
        return;
    };
    let total = songs.len();
    let mut seen_song_ids = HashSet::new();
    let mut added_count = 0;
    for mut song in songs {
        let (Some(id), Some(name)) = (
            song.get("id").and_then(|i| i.as_i64()),
            song.get("name").and_then(|n| n.as_str()).map(str::to_string),
        ) else {
            continue;
        };
        if !seen_song_ids.insert(id) {
            continue;
        }
        // 提取封面 - 支持多种字段格式，并通过代理
        let cover = song
            .get("al")
            .or_else(|| song.get("album"))
            .and_then(|al| {
                al.get("picUrl")
                    .or_else(|| al.get("pic_url"))
                    .or_else(|| al.get("cover"))
            })
            .and_then(|p| p.as_str())
            .map(proxy_image_url);
        // 规范化metadata确保包含所有必要字段
        if let Some(obj) = song.as_object_mut() {
            // 确保有ar字段（艺术家数组）
            if !obj.contains_key("ar") && !obj.contains_key("artists") {
                obj.insert("ar".to_string(), json!([]));
            }
            // 确保有al字段（专辑信息）
            if !obj.contains_key("al") && !obj.contains_key("album") {
                obj.insert("al".to_string(), json!({"name": ""}));
            }
            // 确保有dt字段（时长毫秒）
            if !obj.contains_key("dt") && !obj.contains_key("duration") {
                obj.insert("dt".to_string(), json!(0));
            }
        }
        library_items.push(LibraryItem {
            id: format!("netease_song_{}", id),
            item_type: "music".to_string(),
            title: name,
            cover,
            platform: "Netease".to_string(),
            metadata: song,
        });
        added_count += 1;
    }
    tracing::info!(
        "✓ Loaded {} Netease songs (deduplicated from {})",
        added_count,
        total
    );
}

pub fn append_bangumi_library_items(library_items: &mut Vec<LibraryItem>, bangumi_data: &Value) {
    let Some(collections) = bangumi_data.get("collections").and_then(|c| c.as_array()) else {
        return;
    };

    let mut added = 0usize;
    for collection in collections {
        let subject = collection.get("subject").unwrap_or(collection);
        let subject_id = collection
            .get("subject_id")
            .and_then(|v| v.as_i64())
            .or_else(|| subject.get("id").and_then(|v| v.as_i64()));
        let Some(subject_id) = subject_id else {
            continue;
        };

        let title = subject
            .get("name_cn")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| subject.get("name").and_then(|v| v.as_str()))
            .unwrap_or("Unknown");
        let subject_type = collection
            .get("subject_type")
            .and_then(|v| v.as_i64())
            .or_else(|| subject.get("type").and_then(|v| v.as_i64()))
            .unwrap_or(0);
        let subject_platform = subject.get("platform").and_then(|v| v.as_str());
        let item_type = bangumi_library_item_type(subject_type, subject_platform);
        // Prefer common, then medium, then large, then small.
        let cover = subject
            .get("images")
            .and_then(|images| {
                images
                    .get("common")
                    .or_else(|| images.get("medium"))
                    .or_else(|| images.get("large"))
                    .or_else(|| images.get("small"))
            })
            .and_then(|v| v.as_str())
            .map(|url| proxy_image_url(&prefer_card_cover_url(url)));

        let mut metadata = collection.clone();
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "url".to_string(),
                json!(format!("https://bgm.tv/subject/{}", subject_id)),
            );
            obj.insert(
                "platform".to_string(),
                json!(subject_platform.unwrap_or("Bangumi")),
            );
        }

        library_items.push(LibraryItem {
            id: format!("bangumi_subject_{}", subject_id),
            item_type: item_type.to_string(),
            title: title.to_string(),
            cover,
            platform: "Bangumi".to_string(),
            metadata,
        });
        added += 1;
    }

    tracing::info!("✓ Loaded {} Bangumi collection items", added);
}

pub fn append_mal_library_items(library_items: &mut Vec<LibraryItem>, mal_data: &Value) {
    let mut added = 0usize;

    let mut append_list = |list_key: &str, path_kind: &str, item_type: &str| {
        let Some(list) = mal_data.get(list_key).and_then(|v| v.as_array()) else {
            return;
        };
        for entry in list {
            let node = entry.get("node").unwrap_or(entry);
            let subject_id = node.get("id").and_then(|v| v.as_i64());
            let Some(subject_id) = subject_id else {
                continue;
            };
            let title = node
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            // Prefer main_picture.medium over large.
            let cover = node
                .pointer("/main_picture/medium")
                .or_else(|| node.pointer("/main_picture/large"))
                .and_then(|v| v.as_str())
                .map(|url| proxy_image_url(&prefer_card_cover_url(url)));

            let list_status = entry.get("list_status");
            // Flatten fields used by LibraryGrid (parity with Bangumi `rate` / `progress`)
            let rate = list_status
                .and_then(|s| s.get("score"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let status = list_status
                .and_then(|s| s.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let progress = if path_kind == "anime" {
                list_status
                    .and_then(|s| s.get("num_episodes_watched"))
                    .and_then(|v| v.as_i64())
                    .map(|n| {
                        let total = node
                            .get("num_episodes")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        if total > 0 {
                            format!("{}/{}", n, total)
                        } else if n > 0 {
                            format!("{}", n)
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default()
            } else {
                list_status
                    .and_then(|s| s.get("num_chapters_read"))
                    .and_then(|v| v.as_i64())
                    .map(|chapters| {
                        let volumes = list_status
                            .and_then(|s| s.get("num_volumes_read"))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        if volumes > 0 {
                            format!("{}/{}", chapters, volumes)
                        } else if chapters > 0 {
                            format!("{}", chapters)
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default()
            };

            let mut metadata = entry.clone();
            if let Some(obj) = metadata.as_object_mut() {
                obj.insert(
                    "url".to_string(),
                    json!(format!(
                        "https://myanimelist.net/{}/{}",
                        path_kind, subject_id
                    )),
                );
                obj.insert("platform".to_string(), json!("MyAnimeList"));
                obj.insert("rate".to_string(), json!(rate));
                if !status.is_empty() {
                    obj.insert("status".to_string(), json!(status));
                }
                if !progress.is_empty() {
                    obj.insert("progress".to_string(), json!(progress));
                    // Book cards also read ep_status/vol_status (Bangumi shape)
                    if path_kind == "manga" {
                        if let Some(chapters) = list_status
                            .and_then(|s| s.get("num_chapters_read"))
                            .and_then(|v| v.as_i64())
                        {
                            obj.insert("ep_status".to_string(), json!(chapters));
                        }
                        if let Some(volumes) = list_status
                            .and_then(|s| s.get("num_volumes_read"))
                            .and_then(|v| v.as_i64())
                        {
                            obj.insert("vol_status".to_string(), json!(volumes));
                        }
                    }
                }
            }

            library_items.push(LibraryItem {
                id: format!("mal_{}_{}", path_kind, subject_id),
                item_type: item_type.to_string(),
                title: title.to_string(),
                cover,
                platform: "MyAnimeList".to_string(),
                metadata,
            });
            added += 1;
        }
    };

    append_list("anime_list", "anime", "anime");
    append_list("manga_list", "manga", "book");

    tracing::info!("✓ Loaded {} MyAnimeList list items", added);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn db_and_raw_sources_assemble_identically() {
        let data = json!({
            "steam": { "games": [{ "appid": 10, "name": "Game" }, { "appid": 11 }] },
            "bilibili": {
                "bangumi": [{ "season_id": 5, "title": "Show", "cover": "https://i0.hdslb.com/a.jpg", "season_type": 2 }],
                "favorites": [{ "videos": [{ "bvid": "BV1", "title": "Clip", "cover": "https://i0.hdslb.com/b.jpg" }] }]
            },
            "netease": { "liked_songs": [
                { "id": 1, "name": "Song", "al": { "picUrl": "https://p1.music.126.net/c.jpg" } },
                { "id": 1, "name": "Song again" },
                { "id": 2, "name": "Bare" }
            ] },
            "github": { "repos": [] }
        });
        let mut map: HashMap<String, Value> = data
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let from_db = assemble_library_items(|platform| map.remove(platform));
        let mut raw = data;
        let from_raw =
            assemble_library_items(|platform| raw.as_object_mut().and_then(|d| d.remove(platform)));
        let shape = |items: &[LibraryItem]| {
            items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(shape(&from_db), shape(&from_raw));
        let ids: Vec<&str> = from_db.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "steam_game_10",
                "bilibili_bangumi_5",
                "bilibili_video_BV1",
                "netease_song_1",
                "netease_song_2"
            ]
        );
        assert_eq!(from_db[1].item_type, "tv_series");
        assert_eq!(
            from_db[1].metadata["url"],
            json!("https://www.bilibili.com/bangumi/play/ss5")
        );
        assert_eq!(from_db[3].title, "Song");
        assert!(from_db[3].cover.is_some());
        assert_eq!(from_db[4].metadata["ar"], json!([]));
        assert_eq!(from_db[4].metadata["dt"], json!(0));
        assert!(assemble_library_items(|_| None).is_empty());
    }

    #[test]
    fn canonical_platform_aliases() {
        assert_eq!(canonical_library_platform("bili"), "Bilibili");
        assert_eq!(canonical_library_platform("myanimelist"), "MyAnimeList");
        assert_eq!(canonical_library_platform("bgm"), "Bangumi");
        assert_eq!(canonical_library_platform("Steam"), "Steam");
    }

    #[test]
    fn bangumi_type_mapping() {
        assert_eq!(bangumi_library_item_type(2, None), "anime");
        assert_eq!(bangumi_library_item_type(4, None), "game");
        assert_eq!(bangumi_library_item_type(6, Some("TV")), "tv_series");
        assert_eq!(bangumi_library_item_type(6, Some("movie")), "video");
    }

    #[test]
    fn append_bangumi_builds_items_with_proxy_cover() {
        let mut items = Vec::new();
        let data = json!({
            "collections": [{
                "subject_id": 1,
                "subject_type": 2,
                "ep_status": 3,
                "type": 3,
                "subject": {
                    "id": 1,
                    "name": "Test",
                    "name_cn": "测试",
                    "type": 2,
                    "eps": 12,
                    "images": {
                        "large": "https://lain.bgm.tv/pic/cover/l/1.jpg",
                        "common": "https://lain.bgm.tv/pic/cover/c/1.jpg",
                        "summary": "huge text that should not ship"
                    }
                },
                "comment": "long review body that should be dropped"
            }]
        });
        append_bangumi_library_items(&mut items, &data);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_type, "anime");
        assert_eq!(items[0].platform, "Bangumi");
        assert!(
            items[0]
                .cover
                .as_ref()
                .unwrap()
                .starts_with("/api/proxy/image")
        );
        // Prefer common over large when both exist.
        assert!(
            items[0]
                .cover
                .as_ref()
                .unwrap()
                .contains("pic%2Fcover%2Fc%2F")
                || items[0].cover.as_ref().unwrap().contains("/pic/cover/c/"),
            "cover should use common size: {}",
            items[0].cover.as_ref().unwrap()
        );

        let slimmed = normalize_library_item_for_client(items[0].clone());
        assert!(slimmed.metadata.get("comment").is_none());
        assert!(
            slimmed
                .metadata
                .get("subject")
                .and_then(|s| s.get("images"))
                .is_none()
        );
        assert_eq!(slimmed.metadata.get("ep_status"), Some(&json!(3)));
        assert_eq!(slimmed.metadata.pointer("/subject/eps"), Some(&json!(12)));
    }

    #[test]
    fn append_bangumi_keeps_api_v0_resize_common() {
        let mut items = Vec::new();
        let data = json!({
            "collections": [{
                "subject_id": 1,
                "subject_type": 2,
                "subject": {
                    "id": 1,
                    "name_cn": "测试",
                    "type": 2,
                    "images": {
                        "large": "https://lain.bgm.tv/pic/cover/l/1.jpg",
                        "common": "https://lain.bgm.tv/r/400/pic/cover/l/1.jpg"
                    }
                }
            }]
        });
        append_bangumi_library_items(&mut items, &data);
        let cover = items[0].cover.as_ref().unwrap();
        assert!(cover.starts_with("/api/proxy/image"), "{cover}");
        assert!(
            cover.contains("r%2F400%2Fpic%2Fcover%2Fl%2F") || cover.contains("/r/400/pic/cover/l/"),
            "must keep resize common, not rewrite l→c: {cover}"
        );
        assert!(
            !cover.contains("pic%2Fcover%2Fc%2F") && !cover.contains("/pic/cover/c/"),
            "must not rewrite /r/400/…/l/ to /c/: {cover}"
        );
        let slimmed = normalize_library_item_for_client(items[0].clone());
        let slim_cover = slimmed.cover.as_ref().unwrap();
        assert!(
            slim_cover.contains("r%2F400%2Fpic%2Fcover%2Fl%2F")
                || slim_cover.contains("/r/400/pic/cover/l/"),
            "client normalize must keep resize common: {slim_cover}"
        );
    }

    #[test]
    fn prefer_card_cover_rewrites_bangumi_large_and_netease_param() {
        assert_eq!(
            prefer_card_cover_url("https://lain.bgm.tv/pic/cover/l/ab.jpg"),
            "https://lain.bgm.tv/pic/cover/c/ab.jpg"
        );
        // Current API common/medium/grid: /r/{width}/pic/cover/l/ — leave the `l`.
        assert_eq!(
            prefer_card_cover_url("https://lain.bgm.tv/r/400/pic/cover/l/ab.jpg"),
            "https://lain.bgm.tv/r/400/pic/cover/l/ab.jpg"
        );
        assert_eq!(
            prefer_card_cover_url("https://lain.bgm.tv/r/800/pic/cover/l/ab.jpg"),
            "https://lain.bgm.tv/r/800/pic/cover/l/ab.jpg"
        );
        let proxied_resize = proxy_image_url("https://lain.bgm.tv/r/400/pic/cover/l/ab.jpg");
        assert_eq!(prefer_card_cover_url(&proxied_resize), proxied_resize);
        let netease = prefer_card_cover_url("https://p2.music.126.net/xx.jpg");
        assert!(netease.contains("param=288y288"), "{netease}");
        assert_eq!(
            prefer_card_cover_url("https://p2.music.126.net/xx.jpg?param=400y400"),
            "https://p2.music.126.net/xx.jpg?param=288y288"
        );
        // Already at or below the card edge: leave alone.
        assert_eq!(
            prefer_card_cover_url("https://p2.music.126.net/xx.jpg?param=200y200"),
            "https://p2.music.126.net/xx.jpg?param=200y200"
        );

        // Proxied large Bangumi → re-proxy common.
        let proxied = proxy_image_url("https://lain.bgm.tv/pic/cover/l/ab.jpg");
        let rewritten = prefer_card_cover_url(&proxied);
        assert!(
            rewritten.contains("pic%2Fcover%2Fc%2F") || rewritten.contains("/pic/cover/c/"),
            "proxied rewrite: {rewritten}"
        );

        // Proxied Netease without param → re-proxy with param.
        let proxied_ne = proxy_image_url("https://p2.music.126.net/xx.jpg");
        let rewritten_ne = prefer_card_cover_url(&proxied_ne);
        assert!(
            rewritten_ne.contains("param%3D288y288") || rewritten_ne.contains("param=288y288"),
            "proxied netease: {rewritten_ne}"
        );

        assert_eq!(
            prefer_card_cover_url("https://i0.hdslb.com/bfs/bangumi/image/x.jpg"),
            "https://i0.hdslb.com/bfs/bangumi/image/x.jpg@528w.webp"
        );
        assert_eq!(
            prefer_card_cover_url("https://i2.hdslb.com/bfs/archive/c.jpg?spm=1"),
            "https://i2.hdslb.com/bfs/archive/c.jpg@528w.webp?spm=1"
        );
        let already = "https://i0.hdslb.com/bfs/archive/c.jpg@672w_378h_1c.webp";
        assert_eq!(prefer_card_cover_url(already), already);
        assert_eq!(
            prefer_card_cover_url("https://i0.hdslb.com/bfs/face/a.gif"),
            "https://i0.hdslb.com/bfs/face/a.gif"
        );
        assert_eq!(
            prefer_card_cover_url("https://hdslb.com.evil.com/bfs/archive/c.jpg"),
            "https://hdslb.com.evil.com/bfs/archive/c.jpg"
        );
        let proxied_bili = proxy_image_url("https://i0.hdslb.com/bfs/bangumi/image/x.jpg");
        let rewritten_bili = prefer_card_cover_url(&proxied_bili);
        assert!(
            rewritten_bili.contains("x.jpg%40528w.webp")
                || rewritten_bili.contains("x.jpg@528w.webp"),
            "proxied bilibili: {rewritten_bili}"
        );
    }

    #[test]
    fn slim_metadata_keeps_play_and_progress_fields() {
        let fat = json!({
            "id": 42,
            "name": "Song",
            "fee": 1,
            "ar": [{ "id": 9, "name": "Artist", "tns": [] }],
            "al": { "id": 1, "name": "Album", "picUrl": "https://p2.music.126.net/a.jpg" },
            "dt": 180000,
            "privilege": { "fee": 1, "maxBr": 999, "st": 0 },
            "alias": ["drop me"],
        });
        let slim = slim_library_metadata(&fat);
        assert_eq!(slim.get("id"), Some(&json!(42)));
        assert_eq!(slim.get("fee"), Some(&json!(1)));
        assert_eq!(slim.pointer("/ar/0/name"), Some(&json!("Artist")));
        assert!(slim.pointer("/ar/0/tns").is_none());
        assert_eq!(slim.pointer("/privilege/fee"), Some(&json!(1)));
        assert!(slim.pointer("/privilege/maxBr").is_none());
        assert!(slim.get("alias").is_none());
        assert!(
            slim.pointer("/al/picUrl")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .contains("param=288y288")
        );
    }

    #[test]
    fn apply_preferences_filters_disabled_sources() {
        let items = vec![
            LibraryItem {
                id: "1".into(),
                item_type: "game".into(),
                title: "A".into(),
                cover: None,
                platform: "Steam".into(),
                metadata: json!({}),
            },
            LibraryItem {
                id: "2".into(),
                item_type: "game".into(),
                title: "B".into(),
                cover: None,
                platform: "Bangumi".into(),
                metadata: json!({}),
            },
        ];
        let prefs = LibrarySourcePreferences {
            categories: HashMap::from([("game".into(), vec!["Steam".into()])]),
            ..LibrarySourcePreferences::default()
        }
        .normalized();
        let page = paginate_library_items(&items, Some(&prefs), None, None, None).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, "1");

        let counts = count_library_items_by_type(&items, Some(&prefs));
        assert_eq!(counts.get("game").copied(), Some(1));
        assert_eq!(counts.values().copied().sum::<usize>(), 1);
    }

    #[test]
    fn typed_pagination_filters_before_slicing() {
        let items: Vec<LibraryItem> = (0..500)
            .map(|index| LibraryItem {
                id: index.to_string(),
                item_type: if index >= 470 { "music" } else { "game" }.into(),
                title: index.to_string(),
                cover: None,
                platform: "Test".into(),
                metadata: json!({}),
            })
            .collect();

        let page = paginate_library_items(&items, None, Some("music"), Some(0), Some(20)).unwrap();
        assert_eq!(page.total, 30);
        assert_eq!(page.returned, 20);
        assert!(page.items.iter().all(|item| item.item_type == "music"));
        assert!(page.has_more);
        assert_eq!(page.next_offset, Some(20));
    }

    #[test]
    fn pagination_clamps_limit_and_rejects_unknown_type() {
        let item = LibraryItem {
            id: "1".into(),
            item_type: "game".into(),
            title: "A".into(),
            cover: None,
            platform: "Steam".into(),
            metadata: json!({}),
        };
        let items = vec![item.clone()];
        let page = paginate_library_items(&items, None, None, None, Some(999)).unwrap();
        assert_eq!(page.limit, Some(200));
        assert!(paginate_library_items(&[item], None, Some("unknown"), None, None).is_err());
    }

    #[test]
    fn missing_limit_defaults_to_a_page_instead_of_dumping() {
        let items: Vec<LibraryItem> = (0..300)
            .map(|index| LibraryItem {
                id: index.to_string(),
                item_type: "game".into(),
                title: index.to_string(),
                cover: None,
                platform: "Test".into(),
                metadata: json!({}),
            })
            .collect();
        let page = paginate_library_items(&items, None, None, None, None).unwrap();
        assert_eq!(page.limit, Some(LIBRARY_DEFAULT_PAGE_LIMIT));
        assert_eq!(page.returned, LIBRARY_DEFAULT_PAGE_LIMIT);
        assert!(page.has_more);
        assert_eq!(page.next_offset, Some(LIBRARY_DEFAULT_PAGE_LIMIT));
    }

    #[test]
    fn assembly_cache_reuses_arc_and_invalidates() {
        invalidate_library_assembly_cache();
        let cached = store_library_items(
            42,
            vec![LibraryItem {
                id: "1".into(),
                item_type: "game".into(),
                title: "A".into(),
                cover: None,
                platform: "Steam".into(),
                metadata: json!({}),
            }],
        );
        let hit = cached_library_items(42).expect("cache hit");
        assert!(Arc::ptr_eq(&cached, &hit));
        assert!(cached_library_items(7).is_none());
        invalidate_library_assembly_cache();
        assert!(cached_library_items(42).is_none());
    }

    #[test]
    fn type_counts_skip_disabled_sources_and_unknown_types() {
        let items = vec![
            LibraryItem {
                id: "1".into(),
                item_type: "game".into(),
                title: "A".into(),
                cover: None,
                platform: "Steam".into(),
                metadata: json!({}),
            },
            LibraryItem {
                id: "2".into(),
                item_type: "game".into(),
                title: "B".into(),
                cover: None,
                platform: "Bangumi".into(),
                metadata: json!({}),
            },
            LibraryItem {
                id: "3".into(),
                item_type: "music".into(),
                title: "C".into(),
                cover: None,
                platform: "Netease".into(),
                metadata: json!({}),
            },
            LibraryItem {
                id: "4".into(),
                item_type: "podcast".into(),
                title: "D".into(),
                cover: None,
                platform: "Netease".into(),
                metadata: json!({}),
            },
        ];
        let prefs = LibrarySourcePreferences {
            categories: HashMap::from([("game".into(), vec!["Steam".into()])]),
            ..LibrarySourcePreferences::default()
        }
        .normalized();

        let counts = count_library_items_by_type(&items, Some(&prefs));
        assert_eq!(counts.get("game").copied(), Some(1));
        assert_eq!(counts.get("music").copied(), Some(1));
        assert!(!counts.contains_key("podcast"));
        assert_eq!(counts.values().copied().sum::<usize>(), 2);
    }
}
