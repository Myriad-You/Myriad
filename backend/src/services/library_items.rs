//! Library item models, paging, platform builders, and short-lived assembly cache.
//!
//! Keep DB I/O in the API layer; this module shapes and caches LibraryItem values.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::services::image_proxy_urls::proxy_image_url;

/// 资料库数据项
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibraryItem {
    pub id: String,
    pub item_type: String, // "game", "video", "music"
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
    // One pass before cache: medium covers + slim metadata so every page response
    // (and the assembly cache) stays compact for large libraries / infinite canvas.
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

/// Prefer medium/common assets over large/original for library cards (~184–400 CSS px).
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
    // Bangumi: /pic/cover/{l|c|m|s|g}/… — common is enough for cards.
    if trimmed.contains("bgm.tv") || trimmed.contains("lain.bgm") {
        return trimmed
            .replace("/pic/cover/l/", "/pic/cover/c/")
            .replace("/pic/cover/g/", "/pic/cover/c/");
    }

    // Netease CDN accepts ?param=WxH; cap decode size without another hop.
    if (trimmed.contains("music.126.net") || trimmed.contains("music.163.com"))
        && !trimmed.contains("param=")
    {
        let sep = if trimmed.contains('?') { '&' } else { '?' };
        return format!("{trimmed}{sep}param=300y300");
    }

    trimmed.to_string()
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

/// Keep only fields LibraryGrid / watch-progress / play-song need.
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

#[allow(dead_code)] // 仅测试调用：本仓无生产调用点（编译器已核）。
pub fn apply_library_source_preferences(
    items: Vec<LibraryItem>,
    preferences: &LibrarySourcePreferences,
) -> Vec<LibraryItem> {
    items
        .into_iter()
        .filter(|item| preferences.source_enabled(&item.item_type, &item.platform))
        .collect()
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
    let limit = limit.map(|limit| limit.clamp(1, 200));
    let items: Vec<LibraryItem> = match limit {
        Some(limit) => filtered
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect(),
        None => filtered.into_iter().cloned().collect(),
    };
    let returned = items.len();
    let next = offset + returned;
    let has_more = limit.is_some() && next < total;

    Ok(LibraryPage {
        items,
        total,
        returned,
        offset,
        limit,
        has_more,
        next_offset: has_more.then_some(next),
    })
}

/// Canonical library type for Bangumi subject type codes.
/// Type 6 ("real"/实景) → tv_series when platform looks like TV drama, else video.
/// Shared by Library, report cards, and platform_items normalize.
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
        // Card display is ~184–400 CSS px; prefer common/medium over large.
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
            // medium is typically ~225px; large is ~400+ and overkill for cards.
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
        assert!(items[0]
            .cover
            .as_ref()
            .unwrap()
            .starts_with("/api/proxy/image"));
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
        assert!(slimmed
            .metadata
            .get("subject")
            .and_then(|s| s.get("images"))
            .is_none());
        assert_eq!(slimmed.metadata.get("ep_status"), Some(&json!(3)));
        assert_eq!(slimmed.metadata.pointer("/subject/eps"), Some(&json!(12)));
    }

    #[test]
    fn prefer_card_cover_rewrites_bangumi_large_and_netease_param() {
        assert_eq!(
            prefer_card_cover_url("https://lain.bgm.tv/pic/cover/l/ab.jpg"),
            "https://lain.bgm.tv/pic/cover/c/ab.jpg"
        );
        let netease = prefer_card_cover_url("https://p2.music.126.net/xx.jpg");
        assert!(netease.contains("param=300y300"), "{netease}");
        // Already sized: leave alone.
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
            rewritten_ne.contains("param%3D300y300") || rewritten_ne.contains("param=300y300"),
            "proxied netease: {rewritten_ne}"
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
        assert!(slim
            .pointer("/al/picUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("param=300y300"));
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
        let filtered = apply_library_source_preferences(items, &prefs);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "1");
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
}
