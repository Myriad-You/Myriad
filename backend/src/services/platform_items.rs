//! Platform filtered-cache item projection for Tapp Platform Read APIs.
//!
//! Smart-filter caches store content under `raw_unknown_content` / `content_analysis`,
//! not always a top-level `items` array. This module projects those shapes into a
//! uniform items[] for Tapps (and keeps write-path hosts free of API imports).
//!
//! Lives in services so agent / future readers do not reach through
//! `api::tapp_runtime::platform` for pure projection.

use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// Smart-filter caches store content under `raw_unknown_content` / `content_analysis`,
/// not a top-level `items` array. Project those shapes into a uniform items[] for Tapps.
/// Prefer existing `items` when present (e.g. tapp-written entries).
pub fn extract_platform_items(data: &Value, platform: &str) -> Vec<Value> {
    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
        if !items.is_empty() {
            return items
                .iter()
                .enumerate()
                .map(|(i, item)| normalize_platform_item(item, platform, i))
                .collect();
        }
    }

    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(raw) = data.get("raw_unknown_content").and_then(|v| v.as_array()) {
        for (i, entry) in raw.iter().enumerate() {
            let item = project_unknown_content(entry, platform, i);
            let key = item_dedupe_key(&item);
            if seen.insert(key) {
                items.push(item);
            }
        }
    }

    if let Some(analysis) = data.get("content_analysis") {
        for item in project_content_analysis(analysis, platform, items.len()) {
            let key = item_dedupe_key(&item);
            if seen.insert(key) {
                items.push(item);
            }
        }
    }

    // 从 `cache/raw/{platform}.json` 补封面 / appid。
    enrich_items_from_raw_cache(platform, &mut items);
    items
}

/// Steam CDN header art from appid (reliable for library picker / share cards).
fn steam_header_image(appid: i64) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg")
}

fn normalize_item_type(raw: &str, platform: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    match t.as_str() {
        "game" | "games" => "game".into(),
        "bangumi" | "anime" | "番剧" => "anime".into(),
        "video" | "videos" => "video".into(),
        "music" | "song" | "songs" => "music".into(),
        "book" | "manga" | "novel" => "book".into(),
        "tv" | "tv_series" | "series" => "tv_series".into(),
        // Bangumi smart_filter label for subject type 6
        "real" => crate::services::library_items::bangumi_real_item_type(None).into(),
        "repo" | "repos" | "repository" => "repo".into(),
        "" if platform.eq_ignore_ascii_case("steam") => "game".into(),
        "" if platform.eq_ignore_ascii_case("bilibili") => "video".into(),
        "" if platform.eq_ignore_ascii_case("netease")
            || platform.eq_ignore_ascii_case("netease_music") =>
        {
            "music".into()
        }
        "" if platform.eq_ignore_ascii_case("github") => "repo".into(),
        "" if platform.eq_ignore_ascii_case("xbox") || platform.eq_ignore_ascii_case("psn") => {
            "game".into()
        }
        other => other.to_string(),
    }
}

/// Merge cover/appid/playtime from the platform raw fetch when filtered items are sparse.
/// Works on existing caches without re-running smart_filter.
fn enrich_items_from_raw_cache(platform: &str, items: &mut [Value]) {
    if items.is_empty() {
        return;
    }
    let slug = platform.to_ascii_lowercase();
    // netease_music catalog → raw/netease.json
    let raw_slug = match slug.as_str() {
        "netease_music" => "netease",
        other => other,
    };
    let raw_path = crate::services::data_paths::platform_raw_file(raw_slug);
    let Ok(content) = std::fs::read_to_string(&raw_path) else {
        return;
    };
    let Ok(raw) = serde_json::from_str::<Value>(&content) else {
        return;
    };

    match slug.as_str() {
        "steam" => enrich_steam_items(items, &raw),
        "bilibili" => enrich_bilibili_items(items, &raw),
        "netease" | "netease_music" => enrich_netease_items(items, &raw),
        "github" => enrich_github_items(items, &raw),
        "xbox" => enrich_xbox_items(items, &raw),
        // bangumi / mal already keep cover in filtered subjects; still fill gaps.
        "bangumi" => enrich_bangumi_items(items, &raw),
        "mal" => enrich_mal_items(items, &raw),
        _ => {}
    }
}

fn enrich_steam_items(items: &mut [Value], raw: &Value) {
    // name (lower) → (appid, playtime_forever)
    let mut by_name: HashMap<String, (i64, i64)> = HashMap::new();
    if let Some(games) = raw.get("games").and_then(|v| v.as_array()) {
        for g in games {
            let name = g
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            let appid = g.get("appid").and_then(|v| v.as_i64()).unwrap_or(0);
            let playtime = g
                .get("playtime_forever")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if appid > 0 {
                by_name.insert(name, (appid, playtime));
            }
        }
    }
    if by_name.is_empty() {
        return;
    }

    for item in items.iter_mut() {
        let title = item
            .get("title")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let Some(&(appid, playtime)) = by_name.get(&title) else {
            continue;
        };

        let has_image = item
            .get("image")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if !has_image {
            let img = steam_header_image(appid);
            item["image"] = json!(img);
            item["cover"] = json!(img);
        }
        // Prefer numeric appid as stable id when current id is synthetic.
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() || id.starts_with("steam_") || id.starts_with("game_") {
            item["id"] = json!(appid.to_string());
        }
        item["type"] = json!("game");
        if let Some(meta) = item.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            meta.entry("appid".to_string())
                .or_insert_with(|| json!(appid.to_string()));
            if playtime > 0 {
                meta.entry("playtime".to_string())
                    .or_insert_with(|| json!(playtime.to_string()));
            }
        }
    }
}

fn enrich_bilibili_items(items: &mut [Value], raw: &Value) {
    // title (lower) → cover / season_id / progress / bvid
    let mut by_title: HashMap<String, Value> = HashMap::new();

    if let Some(bangumi) = raw.get("bangumi").and_then(|v| v.as_array()) {
        for b in bangumi {
            let title = b
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if title.is_empty() {
                continue;
            }
            by_title.insert(
                title,
                json!({
                    "kind": "anime",
                    "cover": b.get("cover").and_then(|v| v.as_str()).unwrap_or(""),
                    "season_id": b.get("season_id"),
                    "progress": b.get("progress").and_then(|v| v.as_str()).unwrap_or(""),
                }),
            );
        }
    }

    if let Some(favorites) = raw.get("favorites").and_then(|v| v.as_array()) {
        for fav in favorites {
            if let Some(videos) = fav.get("videos").and_then(|v| v.as_array()) {
                for v in videos {
                    let title = v
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase();
                    if title.is_empty() {
                        continue;
                    }
                    by_title.entry(title).or_insert_with(|| {
                        json!({
                            "kind": "video",
                            "cover": v.get("cover").and_then(|c| c.as_str()).unwrap_or(""),
                            "bvid": v.get("bvid").and_then(|c| c.as_str()).unwrap_or(""),
                            "id": v.get("id"),
                        })
                    });
                }
            }
        }
    }

    if by_title.is_empty() {
        return;
    }

    for item in items.iter_mut() {
        let title = item
            .get("title")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let Some(extra) = by_title.get(&title) else {
            continue;
        };

        let has_image = item
            .get("image")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if !has_image {
            if let Some(cover) = extra.get("cover").and_then(|v| v.as_str()) {
                if !cover.is_empty() {
                    // Prefer https for mixed-content-safe chat cards
                    let cover = if cover.starts_with("http://") {
                        cover.replacen("http://", "https://", 1)
                    } else {
                        cover.to_string()
                    };
                    item["image"] = json!(cover);
                    item["cover"] = json!(cover);
                }
            }
        }

        if let Some(kind) = extra.get("kind").and_then(|v| v.as_str()) {
            let cur = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if cur.is_empty()
                || cur.eq_ignore_ascii_case("item")
                || cur.eq_ignore_ascii_case("bangumi")
                || cur.eq_ignore_ascii_case("video")
            {
                item["type"] = json!(kind);
            }
        }

        // Stable ids
        if let Some(sid) = extra.get("season_id") {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() || id.starts_with("bilibili_") || id.starts_with("anime_") {
                let sid_str = sid
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| sid.as_i64().map(|n| n.to_string()))
                    .or_else(|| sid.as_u64().map(|n| n.to_string()))
                    .unwrap_or_default();
                if !sid_str.is_empty() {
                    item["id"] = json!(sid_str);
                }
            }
        } else if let Some(bvid) = extra.get("bvid").and_then(|v| v.as_str()) {
            if !bvid.is_empty() {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() || id.starts_with("bilibili_") || id.starts_with("video_") {
                    item["id"] = json!(bvid);
                }
            }
        }

        if let Some(meta) = item.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            if let Some(p) = extra.get("progress").and_then(|v| v.as_str()) {
                if !p.is_empty() {
                    meta.entry("progress".to_string())
                        .or_insert_with(|| json!(p));
                }
            }
            if let Some(bvid) = extra.get("bvid").and_then(|v| v.as_str()) {
                if !bvid.is_empty() {
                    meta.entry("bvid".to_string())
                        .or_insert_with(|| json!(bvid));
                }
            }
        }
    }
}

fn prefer_https(url: &str) -> String {
    if url.starts_with("http://") {
        url.replacen("http://", "https://", 1)
    } else {
        url.to_string()
    }
}

fn set_item_image_if_empty(item: &mut Value, url: &str) {
    if url.is_empty() {
        return;
    }
    let has_image = item
        .get("image")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if has_image {
        return;
    }
    let url = prefer_https(url);
    item["image"] = json!(url);
    item["cover"] = json!(url);
}

/// Netease: fill album cover + song id from liked_songs.
fn enrich_netease_items(items: &mut [Value], raw: &Value) {
    // title|artist → (id, picUrl, album, fee, is_vip)
    let mut by_key: HashMap<String, (String, String, String, Option<i64>, bool)> = HashMap::new();
    let mut by_title: HashMap<String, (String, String, String, Option<i64>, bool)> = HashMap::new();

    let songs = raw
        .get("liked_songs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for s in songs {
        let title = s
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let artist = s
            .get("ar")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let id = s
            .get("id")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default();
        let pic = s
            .get("al")
            .and_then(|al| al.get("picUrl"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let album = s
            .get("al")
            .and_then(|al| al.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let fee = s.get("fee").and_then(|v| v.as_i64()).or_else(|| {
            s.get("privilege")
                .and_then(|p| p.get("fee"))
                .and_then(|v| v.as_i64())
        });
        let is_vip = s
            .get("isVip")
            .and_then(|v| v.as_bool())
            .or_else(|| s.get("is_vip").and_then(|v| v.as_bool()))
            .unwrap_or_else(|| fee.map(|f| f == 1 || f == 4).unwrap_or(false));
        let entry = (id, pic, album, fee, is_vip);
        by_key.insert(format!("{title}|{artist}"), entry.clone());
        by_title.entry(title).or_insert(entry);
    }

    if by_key.is_empty() && by_title.is_empty() {
        return;
    }

    for item in items.iter_mut() {
        let title = item
            .get("title")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let artist = item
            .get("artist")
            .or_else(|| item.get("description"))
            .or_else(|| item.pointer("/metadata/artist"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        // description may be "artist · album" — take first segment
        let artist_key = artist.split('·').next().unwrap_or("").trim();

        let hit = by_key
            .get(&format!("{title}|{artist_key}"))
            .or_else(|| by_title.get(&title));
        let Some((id, pic, album, fee, is_vip)) = hit else {
            continue;
        };

        set_item_image_if_empty(item, pic);
        item["type"] = json!("music");
        if !id.is_empty() {
            let cur = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if cur.is_empty() || cur.starts_with("netease_") || cur.starts_with("music_") {
                item["id"] = json!(id);
            }
        }
        if !album.is_empty() {
            item["album"] = json!(album);
        }
        if let Some(meta) = item.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            if !id.is_empty() {
                meta.entry("id".to_string()).or_insert_with(|| json!(id));
            }
            if !album.is_empty() {
                meta.entry("album".to_string())
                    .or_insert_with(|| json!(album));
            }
            if let Some(f) = fee {
                meta.insert("fee".to_string(), json!(f));
            }
            meta.insert("isVip".to_string(), json!(is_vip));
        }
    }
}

/// GitHub: fill html_url + opengraph image from raw repos.
fn enrich_github_items(items: &mut [Value], raw: &Value) {
    let owner = raw
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut by_name: HashMap<String, Value> = HashMap::new();
    if let Some(repos) = raw.get("repos").and_then(|v| v.as_array()) {
        for r in repos {
            let name = r
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            by_name.insert(name, r.clone());
        }
    }
    if by_name.is_empty() {
        return;
    }

    for item in items.iter_mut() {
        let title = item
            .get("title")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let Some(repo) = by_name.get(&title) else {
            continue;
        };
        item["type"] = json!("repo");
        if let Some(url) = repo
            .get("html_url")
            .or_else(|| repo.get("url"))
            .and_then(|v| v.as_str())
        {
            if item
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                item["url"] = json!(url);
            }
        }
        let og = if !owner.is_empty() {
            format!(
                "https://opengraph.githubassets.com/1/{owner}/{}",
                repo.get("name").and_then(|v| v.as_str()).unwrap_or(&title)
            )
        } else {
            String::new()
        };
        if !og.is_empty() {
            set_item_image_if_empty(item, &og);
        }
        if let Some(desc) = repo.get("description").and_then(|v| v.as_str()) {
            if item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                item["description"] = json!(desc);
            }
        }
        if let Some(meta) = item.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            if let Some(stars) = repo
                .get("stargazers_count")
                .or_else(|| repo.get("stars"))
                .and_then(|v| v.as_i64())
            {
                meta.entry("stars".to_string())
                    .or_insert_with(|| json!(stars));
            }
            if let Some(lang) = repo.get("language").and_then(|v| v.as_str()) {
                meta.entry("language".to_string())
                    .or_insert_with(|| json!(lang));
            }
        }
    }
}

/// Xbox: fill displayImage + titleId from raw achievements.titles.
fn enrich_xbox_items(items: &mut [Value], raw: &Value) {
    let mut by_name: HashMap<String, Value> = HashMap::new();
    let titles = raw
        .pointer("/achievements/titles")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for t in titles {
        let name = t
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        by_name.insert(name, t);
    }
    if by_name.is_empty() {
        return;
    }

    for item in items.iter_mut() {
        let title = item
            .get("title")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let Some(t) = by_name.get(&title) else {
            continue;
        };
        item["type"] = json!("game");
        if let Some(img) = t
            .get("displayImage")
            .or_else(|| t.get("display_image"))
            .and_then(|v| v.as_str())
        {
            set_item_image_if_empty(item, img);
        }
        if let Some(tid) = t
            .get("titleId")
            .or_else(|| t.get("title_id"))
            .or_else(|| t.get("modernTitleId"))
        {
            let tid_str = tid
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| tid.as_i64().map(|n| n.to_string()))
                .or_else(|| tid.as_u64().map(|n| n.to_string()))
                .unwrap_or_default();
            if !tid_str.is_empty() {
                let cur = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if cur.is_empty() || cur.starts_with("xbox_") || cur.starts_with("game_") {
                    item["id"] = json!(tid_str);
                }
            }
        }
    }
}

/// Bangumi: ensure cover/subject_id from raw collections.subject.images.
fn enrich_bangumi_items(items: &mut [Value], raw: &Value) {
    let mut by_title: HashMap<String, Value> = HashMap::new();
    if let Some(cols) = raw.get("collections").and_then(|v| v.as_array()) {
        for c in cols {
            let subject = c.get("subject").cloned().unwrap_or(Value::Null);
            let title = subject
                .get("name")
                .or_else(|| subject.get("name_cn"))
                .or_else(|| c.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            // Also index by Chinese/common name variants
            let title_cn = subject
                .get("name_cn")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if !title.is_empty() {
                by_title.insert(title, c.clone());
            }
            if !title_cn.is_empty() {
                by_title.insert(title_cn, c.clone());
            }
        }
    }
    if by_title.is_empty() {
        return;
    }

    for item in items.iter_mut() {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let Some(c) = by_title.get(&title) else {
            continue;
        };
        let subject = c.get("subject");
        if let Some(cover) = subject
            .and_then(|s| s.get("images"))
            .and_then(|images| {
                images
                    .get("large")
                    .or_else(|| images.get("common"))
                    .or_else(|| images.get("medium"))
            })
            .and_then(|v| v.as_str())
        {
            set_item_image_if_empty(item, cover);
        }
        if let Some(sid) = c
            .get("subject_id")
            .or_else(|| subject.and_then(|s| s.get("id")))
        {
            let sid_str = sid
                .as_i64()
                .map(|n| n.to_string())
                .or_else(|| sid.as_u64().map(|n| n.to_string()))
                .or_else(|| sid.as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            if !sid_str.is_empty() {
                let cur = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if cur.is_empty() || cur.starts_with("bangumi_") {
                    item["id"] = json!(sid_str);
                }
            }
        }
        // `rate` / `ep_status` for media card
        if let Some(meta) = item.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            if let Some(rate) = c.get("rate").and_then(|v| v.as_i64()) {
                if rate > 0 {
                    meta.entry("rate".to_string())
                        .or_insert_with(|| json!(rate));
                }
            }
            if let Some(ep) = c.get("ep_status").and_then(|v| v.as_i64()) {
                meta.entry("ep_status".to_string())
                    .or_insert_with(|| json!(ep));
            }
        }
    }
}

/// MAL: fill cover from node.main_picture when missing.
fn enrich_mal_items(items: &mut [Value], raw: &Value) {
    let mut by_title: HashMap<String, Value> = HashMap::new();
    for key in ["anime_list", "manga_list"] {
        if let Some(list) = raw.get(key).and_then(|v| v.as_array()) {
            for entry in list {
                let node = entry.get("node").cloned().unwrap_or(Value::Null);
                let title = node
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                if title.is_empty() {
                    continue;
                }
                by_title.insert(title, entry.clone());
            }
        }
    }
    if by_title.is_empty() {
        return;
    }

    for item in items.iter_mut() {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let Some(entry) = by_title.get(&title) else {
            continue;
        };
        let node = entry.get("node");
        if let Some(cover) = node
            .and_then(|n| n.get("main_picture"))
            .and_then(|p| p.get("medium").or_else(|| p.get("large")))
            .and_then(|v| v.as_str())
        {
            set_item_image_if_empty(item, cover);
        }
        if let Some(id) = node.and_then(|n| n.get("id")) {
            let id_str = id
                .as_i64()
                .map(|n| n.to_string())
                .or_else(|| id.as_u64().map(|n| n.to_string()))
                .unwrap_or_default();
            if !id_str.is_empty() {
                let cur = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if cur.is_empty() || cur.starts_with("mal_") {
                    item["id"] = json!(id_str);
                }
            }
        }
        if let Some(meta) = item.get_mut("metadata").and_then(|m| m.as_object_mut()) {
            if let Some(ls) = entry.get("list_status") {
                if let Some(score) = ls.get("score").and_then(|v| v.as_i64()) {
                    if score > 0 {
                        meta.entry("score".to_string())
                            .or_insert_with(|| json!(score));
                    }
                }
                if let Some(ep) = ls.get("num_episodes_watched").and_then(|v| v.as_i64()) {
                    meta.entry("num_episodes_watched".to_string())
                        .or_insert_with(|| json!(ep));
                }
            }
        }
    }
}

fn item_dedupe_key(item: &Value) -> String {
    // Prefer title so raw_unknown_content and content_analysis lists don't double-list the same entry.
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !title.is_empty() {
        return title;
    }
    item.get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn first_string(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(s) = v.as_str() {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            } else if let Some(n) = v.as_i64() {
                return Some(n.to_string());
            } else if let Some(n) = v.as_u64() {
                return Some(n.to_string());
            } else if let Some(n) = v.as_f64() {
                return Some(n.to_string());
            }
        }
    }
    None
}

fn normalize_platform_item(item: &Value, platform: &str, index: usize) -> Value {
    let obj = item.as_object();
    let title = obj
        .and_then(|o| first_string(o, &["title", "name", "username", "label"]))
        .unwrap_or_else(|| format!("Item {}", index + 1));
    let item_type = normalize_item_type(
        &obj.and_then(|o| first_string(o, &["type", "content_type", "subject_type"]))
            .unwrap_or_else(|| "item".to_string()),
        platform,
    );
    let id = obj
        .and_then(|o| {
            first_string(
                o,
                &[
                    "id",
                    "title_id",
                    "subject_id",
                    "item_id",
                    "appid",
                    "bvid",
                    "season_id",
                ],
            )
        })
        .unwrap_or_else(|| format!("{platform}_{index}"));
    let image = obj
        .and_then(|o| {
            first_string(
                o,
                &[
                    "image",
                    "cover",
                    "display_image",
                    "profile_image_url",
                    "thumbnail",
                    "poster",
                ],
            )
        })
        .or_else(|| {
            item.get("metadata")
                .and_then(|m| m.as_object())
                .and_then(|m| first_string(m, &["image", "cover", "display_image", "thumbnail"]))
        })
        .or_else(|| {
            // Steam：封面缺失时用 appid 拼 CDN header。
            let appid = obj
                .and_then(|o| first_string(o, &["appid"]))
                .or_else(|| {
                    item.get("metadata")
                        .and_then(|m| m.as_object())
                        .and_then(|m| first_string(m, &["appid"]))
                })
                .and_then(|s| s.parse::<i64>().ok());
            appid.filter(|id| *id > 0).map(steam_header_image)
        });
    let metadata = item.get("metadata").cloned().unwrap_or_else(|| {
        // Promote remaining scalar fields into metadata for richer picker/detail.
        let mut meta = Map::new();
        if let Some(o) = obj {
            for (k, v) in o {
                if matches!(
                    k.as_str(),
                    "id" | "title"
                        | "name"
                        | "type"
                        | "content_type"
                        | "subject_type"
                        | "image"
                        | "cover"
                        | "display_image"
                        | "profile_image_url"
                        | "thumbnail"
                        | "poster"
                        | "metadata"
                        | "platform"
                        | "description"
                        | "url"
                        | "createdAt"
                        | "source"
                ) {
                    continue;
                }
                if v.is_string() || v.is_number() || v.is_boolean() {
                    meta.insert(k.clone(), v.clone());
                }
            }
        }
        Value::Object(meta)
    });

    let mut out = json!({
        "id": id,
        "title": title,
        "type": item_type,
        "platform": platform,
        "metadata": metadata,
    });
    if let Some(img) = image {
        let img = if img.starts_with("http://") {
            img.replacen("http://", "https://", 1)
        } else {
            img
        };
        out["image"] = json!(img);
        out["cover"] = out["image"].clone();
    }
    if let Some(desc) = obj.and_then(|o| first_string(o, &["description", "summary", "desc"])) {
        out["description"] = json!(desc);
    }
    if let Some(url) = obj.and_then(|o| first_string(o, &["url", "link", "web_url"])) {
        out["url"] = json!(url);
    }
    // Preserve original fields that callers may already read.
    if let Some(o) = obj {
        for (k, v) in o {
            if out.get(k).is_none() {
                out[k] = v.clone();
            }
        }
    }
    out
}

fn project_unknown_content(entry: &Value, platform: &str, index: usize) -> Value {
    let obj = entry.as_object();
    let title = obj
        .and_then(|o| first_string(o, &["title", "name"]))
        .unwrap_or_else(|| format!("Item {}", index + 1));
    let item_type = normalize_item_type(
        &obj.and_then(|o| first_string(o, &["content_type", "type"]))
            .unwrap_or_else(|| "item".to_string()),
        platform,
    );
    let metadata = entry.get("metadata").cloned().unwrap_or_else(|| json!({}));
    let image = obj
        .and_then(|o| first_string(o, &["image", "cover", "display_image", "profile_image_url"]))
        .or_else(|| {
            metadata.as_object().and_then(|m| {
                first_string(m, &["image", "cover", "display_image", "profile_image_url"])
            })
        })
        .or_else(|| {
            metadata
                .as_object()
                .and_then(|m| first_string(m, &["appid"]))
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|id| *id > 0)
                .map(steam_header_image)
        });
    let id = obj
        .and_then(|o| {
            first_string(
                o,
                &["id", "title_id", "subject_id", "appid", "bvid", "season_id"],
            )
        })
        .or_else(|| {
            metadata
                .as_object()
                .and_then(|m| first_string(m, &["id", "appid", "bvid", "season_id"]))
        })
        .unwrap_or_else(|| format!("{platform}_{}_{}", slugify_fragment(&item_type), index));

    let mut out = json!({
        "id": id,
        "title": title,
        "type": item_type,
        "platform": platform,
        "metadata": metadata,
    });
    if let Some(img) = image {
        let img = if img.starts_with("http://") {
            img.replacen("http://", "https://", 1)
        } else {
            img
        };
        out["image"] = json!(img);
        out["cover"] = out["image"].clone();
    }
    // Promote playtime / progress from metadata.
    if let Some(m) = metadata.as_object() {
        if let Some(pt) = first_string(m, &["playtime", "playtime_forever"]) {
            out["playtime"] = json!(pt);
            out["playtime_minutes"] = json!(pt);
        }
        if let Some(p) = first_string(m, &["progress"]) {
            out["progress"] = json!(p);
        }
        for key in ["appid", "video_id", "bvid", "url", "view_count"] {
            if out.get(key).is_none() {
                if let Some(v) = m.get(key) {
                    out[key] = v.clone();
                }
            }
        }
    }
    if out.get("name").is_none() {
        out["name"] = out["title"].clone();
    }
    if let Some(obj) = obj {
        promote_entry_fields(&mut out, obj);
    }
    out
}

fn slugify_fragment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if (c == '-' || c == '_' || c.is_whitespace()) && !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Keys under content_analysis that are narrative summaries, not item lists.
const SKIP_ANALYSIS_KEYS: &[&str] = &[
    "game_summary",
    "gaming_summary",
    "video_summary",
    "repo_summary",
    "collection_summary",
    // Note: `anime_analysis` may be an item array (Agent bilibili path) or a
    // narrative blob; non-array values are already skipped below.
    "genre_analysis",
    "language_distribution",
    "contribution_calendar",
    "subject_type_distribution",
    "collection_type_distribution",
    "tag_distribution",
    "music_summary",
    "tweet_summary",
    "server_summary",
    // X / Xbox narrative or scalar stats (not item arrays)
    "post_summary",
    "following_summary",
    "user_name",
    "user_avatar",
    "engagement_stats",
    "mean_score",
    "days_watched",
    "gamerscore",
    "games_count",
    "achievement_games",
    "completed_games",
    "total_achievements_earned",
    "total_achievements_available",
    "average_completion",
    "hardcore_score",
    "display_gamertag",
    // `artist_analysis` is a nested object; nested lists are not walked here.
];

fn project_content_analysis(analysis: &Value, platform: &str, start_index: usize) -> Vec<Value> {
    let Some(obj) = analysis.as_object() else {
        return Vec::new();
    };

    let mut items = Vec::new();
    let mut index = start_index;

    for (list_key, value) in obj {
        if SKIP_ANALYSIS_KEYS.contains(&list_key.as_str()) {
            continue;
        }
        let Some(arr) = value.as_array() else {
            continue;
        };
        // Skip arrays of pure scalars / date buckets
        let sample = arr.iter().find(|v| v.is_object());
        let Some(sample) = sample else {
            continue;
        };
        let sample_obj = sample.as_object().unwrap();
        // Must look like a content entry (has a display name field)
        if first_string(
            sample_obj,
            &["title", "name", "username", "label", "subject_title"],
        )
        .is_none()
        {
            continue;
        }

        let default_type = list_key
            .trim_start_matches("recent_")
            .trim_start_matches("top_")
            .trim_end_matches("_subjects")
            .trim_end_matches("_titles")
            .trim_end_matches("_games")
            .trim_end_matches("_videos")
            .trim_end_matches("_songs")
            .trim_end_matches("_repos")
            .trim_end_matches("_sample")
            .trim_end_matches('s');
        let default_type = if default_type.is_empty() {
            "item"
        } else {
            default_type
        };

        for entry in arr {
            let Some(entry_obj) = entry.as_object() else {
                continue;
            };
            let title = match first_string(
                entry_obj,
                &["title", "name", "username", "label", "subject_title"],
            ) {
                Some(t) => t,
                None => continue,
            };
            let item_type = normalize_item_type(
                &first_string(entry_obj, &["content_type", "type", "subject_type"])
                    .unwrap_or_else(|| default_type.to_string()),
                platform,
            );
            let id = first_string(
                entry_obj,
                &[
                    "id",
                    "title_id",
                    "subject_id",
                    "item_id",
                    "appid",
                    "bvid",
                    "season_id",
                ],
            )
            .unwrap_or_else(|| format!("{platform}_{}_{}", slugify_fragment(&item_type), index));
            let image = first_string(
                entry_obj,
                &[
                    "image",
                    "cover",
                    "display_image",
                    "profile_image_url",
                    "thumbnail",
                    "poster",
                ],
            )
            .or_else(|| {
                first_string(entry_obj, &["appid"])
                    .and_then(|s| s.parse::<i64>().ok())
                    .or_else(|| entry_obj.get("appid").and_then(|v| v.as_i64()))
                    .filter(|id| *id > 0)
                    .map(steam_header_image)
            });
            let description =
                first_string(entry_obj, &["description", "summary", "desc", "artist"]);

            let mut metadata = Map::new();
            for (k, v) in entry_obj {
                if matches!(
                    k.as_str(),
                    "id" | "title"
                        | "name"
                        | "username"
                        | "type"
                        | "content_type"
                        | "subject_type"
                        | "image"
                        | "cover"
                        | "display_image"
                        | "profile_image_url"
                        | "thumbnail"
                        | "poster"
                        | "description"
                        | "summary"
                ) {
                    continue;
                }
                if v.is_string() || v.is_number() || v.is_boolean() {
                    metadata.insert(k.clone(), v.clone());
                }
            }

            let mut item = json!({
                "id": id,
                "title": title,
                "type": item_type,
                "platform": platform,
                "metadata": metadata,
                "source_list": list_key,
            });
            if let Some(img) = image {
                item["image"] = json!(img);
                item["cover"] = item["image"].clone();
            }
            if let Some(desc) = description {
                item["description"] = json!(desc);
            }
            // Promote common identity/stats fields for Agent + Tapp consumers.
            promote_entry_fields(&mut item, entry_obj);
            items.push(item);
            index += 1;
        }
    }

    items
}

/// Lift frequently-queried fields to the item top-level (Agent search / Tapp cards).
fn promote_entry_fields(item: &mut Value, entry: &Map<String, Value>) {
    const PROMOTE: &[&str] = &[
        "video_id",
        "url",
        "appid",
        "bvid",
        "view_count",
        "like_count",
        "playtime",
        "subject_id",
        "title_id",
        "published_at",
        "language",
        "stars",
        "progress",
    ];
    for key in PROMOTE {
        if item.get(*key).is_none() {
            if let Some(v) = entry.get(*key) {
                if !v.is_null() {
                    item[*key] = v.clone();
                }
            }
        }
    }
    // Agent legacy alias: playtime_minutes
    if item.get("playtime_minutes").is_none() {
        if let Some(p) = entry.get("playtime").or_else(|| item.get("playtime")) {
            item["playtime_minutes"] = p.clone();
        }
    }
    // Agent search often looks for `name`
    if item.get("name").is_none() {
        if let Some(t) = item.get("title").cloned() {
            item["name"] = t;
        }
    }
}

/// Extract items for Agent random-content sampling.
///
/// Prefers legacy raw top-level arrays (`games`, `videos`, …) when present.
/// YouTube and MAL fall back to [`extract_platform_items`].
pub fn extract_platform_items_for_random(platform: &str, data: &Value) -> Vec<Value> {
    let platform = platform.to_ascii_lowercase();
    match platform.as_str() {
        "steam" => data
            .get("games")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "bilibili" => data
            .get("videos")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "github" => data
            .get("repos")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "youtube" => {
            // Prefer raw fetch `videos`; fall back to filtered recent_videos
            let from_raw = data
                .get("videos")
                .or(data.get("items"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if !from_raw.is_empty() {
                from_raw
            } else {
                extract_platform_items(data, "youtube")
            }
        }
        "netease" | "netease_music" => data
            .get("songs")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "bangumi" => data
            .get("collections")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "mal" => {
            let mut items = Vec::new();
            for key in ["anime_list", "manga_list", "items"] {
                if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
                    items.extend(arr.clone());
                }
            }
            if items.is_empty() {
                // filtered cache 走 content_analysis
                return extract_platform_items(data, "mal");
            }
            items
        }
        "x" => data
            .get("tweets")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "discord" => data
            .get("guilds")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => data
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_platform_items, extract_platform_items_for_random};
    use serde_json::json;

    #[test]
    fn extract_prefers_existing_items() {
        let data = json!({
            "items": [{ "id": "a1", "title": "Written Item", "type": "game" }],
            "raw_unknown_content": [{ "content_type": "Game", "title": "Other" }]
        });
        let items = extract_platform_items(&data, "steam");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Written Item");
        assert_eq!(items[0]["platform"], "steam");
    }

    #[test]
    fn extract_projects_raw_unknown_content() {
        let data = json!({
            "platform": "steam",
            "raw_unknown_content": [
                {
                    "content_type": "Game",
                    "title": "Left 4 Dead 2",
                    "metadata": { "playtime": "3324" }
                },
                {
                    "content_type": "Game",
                    "title": "Hades",
                    "metadata": { "playtime": "120" }
                }
            ]
        });
        let items = extract_platform_items(&data, "steam");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["title"], "Left 4 Dead 2");
        assert_eq!(items[0]["type"], "game");
        assert_eq!(items[0]["platform"], "steam");
        assert!(items[0].get("id").and_then(|v| v.as_str()).is_some());
        assert_eq!(items[0]["metadata"]["playtime"], "3324");
    }

    #[test]
    fn extract_promotes_steam_appid_image_from_metadata() {
        let data = json!({
            "raw_unknown_content": [{
                "content_type": "Game",
                "title": "Hades",
                "metadata": { "playtime": "120", "appid": "1145360", "image": "https://cdn.cloudflare.steamstatic.com/steam/apps/1145360/header.jpg" }
            }]
        });
        let items = extract_platform_items(&data, "steam");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "1145360");
        assert_eq!(
            items[0]["image"],
            "https://cdn.cloudflare.steamstatic.com/steam/apps/1145360/header.jpg"
        );
        assert_eq!(items[0]["type"], "game");
    }

    #[test]
    fn extract_projects_content_analysis_lists() {
        let data = json!({
            "platform": "github",
            "raw_unknown_content": [],
            "content_analysis": {
                "repo_summary": "has many repos",
                "recent_repos": [
                    {
                        "name": "Sakurairo",
                        "language": "PHP",
                        "stars": 4024,
                        "description": "A WordPress theme"
                    }
                ],
                "contribution_calendar": [
                    { "date": "2025-07-20", "count": 2 }
                ]
            }
        });
        let items = extract_platform_items(&data, "github");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Sakurairo");
        assert_eq!(items[0]["description"], "A WordPress theme");
        assert_eq!(items[0]["metadata"]["stars"], 4024);
    }

    #[test]
    fn extract_dedupes_raw_and_analysis() {
        let data = json!({
            "raw_unknown_content": [
                { "content_type": "Game", "title": "Hades", "metadata": {} }
            ],
            "content_analysis": {
                "recent_games": [
                    { "name": "Hades", "playtime": 120 },
                    { "name": "Celeste", "playtime": 40 }
                ]
            }
        });
        let items = extract_platform_items(&data, "steam");
        // Hades appears once (from raw), Celeste from analysis
        assert_eq!(items.len(), 2);
        let titles: Vec<&str> = items
            .iter()
            .filter_map(|i| i.get("title").and_then(|v| v.as_str()))
            .collect();
        assert!(titles.contains(&"Hades"));
        assert!(titles.contains(&"Celeste"));
    }

    #[test]
    fn extract_covers_from_bangumi_subjects() {
        let data = json!({
            "content_analysis": {
                "top_rated_subjects": [
                    {
                        "subject_id": 19643,
                        "title": "星之卡比",
                        "subject_type": "game",
                        "rate": 10,
                        "cover": "https://example.com/cover.jpg"
                    }
                ]
            }
        });
        let items = extract_platform_items(&data, "bangumi");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "19643");
        assert_eq!(items[0]["image"], "https://example.com/cover.jpg");
        assert_eq!(items[0]["cover"], "https://example.com/cover.jpg");
        assert_eq!(items[0]["type"], "game");
    }

    #[test]
    fn extract_netease_songs_and_enrich_from_raw_file() {
        // Filtered `content_analysis` shape; no raw file.
        let data = json!({
            "content_analysis": {
                "music_summary": "songs",
                "recent_songs": [
                    { "title": "涙では消せない焔", "artist": "Sound Horizon" }
                ]
            }
        });
        // When raw file is absent, still returns title/artist items.
        let items = extract_platform_items(&data, "netease");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "涙では消せない焔");
        // type normalizes to music (from song list key or platform default)
        let t = items[0]["type"].as_str().unwrap_or("");
        assert!(
            t == "music" || t == "song" || t == "item" || t == "recent_song",
            "type={t}"
        );
    }

    #[test]
    fn extract_github_repos_carry_description() {
        let data = json!({
            "content_analysis": {
                "repo_summary": "1 repo",
                "recent_repos": [{
                    "name": "Sakurairo",
                    "language": "PHP",
                    "stars": 4024,
                    "description": "A WordPress theme",
                    "url": "https://github.com/mirai-mamori/Sakurairo",
                    "image": "https://opengraph.githubassets.com/1/mirai-mamori/Sakurairo"
                }]
            }
        });
        let items = extract_platform_items(&data, "github");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Sakurairo");
        assert_eq!(items[0]["description"], "A WordPress theme");
        assert_eq!(
            items[0]["image"],
            "https://opengraph.githubassets.com/1/mirai-mamori/Sakurairo"
        );
    }

    #[test]
    fn extract_xbox_titles_use_display_image() {
        let data = json!({
            "content_analysis": {
                "gaming_summary": "stats",
                "recent_titles": [{
                    "title_id": "1195776867",
                    "name": "现代战争5",
                    "display_image": "https://images-eds-ssl.xboxlive.com/example.jpg",
                    "progress": 42.0
                }]
            }
        });
        let items = extract_platform_items(&data, "xbox");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "现代战争5");
        assert_eq!(
            items[0]["image"],
            "https://images-eds-ssl.xboxlive.com/example.jpg"
        );
    }

    #[test]
    fn youtube_recent_videos_promote_identity_fields() {
        let data = json!({
            "content_analysis": {
                "video_summary": "sample",
                "recent_videos": [{
                    "title": "Sample Upload One",
                    "video_id": "dQw4w9WgXcQ",
                    "cover": "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg",
                    "view_count": 1000,
                    "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
                }]
            }
        });
        let items = extract_platform_items(&data, "youtube");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], "video");
        assert_eq!(items[0]["video_id"], "dQw4w9WgXcQ");
        assert_eq!(items[0]["view_count"], 1000);
        assert_eq!(items[0]["name"], "Sample Upload One");
        assert!(items[0]["url"]
            .as_str()
            .unwrap_or("")
            .contains("dQw4w9WgXcQ"));
    }

    #[test]
    fn random_extract_prefers_raw_top_level_arrays() {
        let data = json!({
            "games": [{ "name": "Hades" }, { "name": "Celeste" }],
            "items": [{ "title": "ignored when games present" }]
        });
        let items = extract_platform_items_for_random("steam", &data);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "Hades");
    }

    #[test]
    fn random_extract_falls_back_to_shared_projection() {
        let data = json!({
            "content_analysis": {
                "recent_videos": [{
                    "title": "Clip",
                    "video_id": "abc"
                }]
            }
        });
        let items = extract_platform_items_for_random("youtube", &data);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Clip");
    }
}
