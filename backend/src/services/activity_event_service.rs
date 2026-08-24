use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const MAX_STORED_CHANGES: usize = 12;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ActivityChange {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_title: Option<String>,
    /// Cover / icon / avatar URL when the platform snapshot has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(rename = "old", skip_serializing_if = "Option::is_none")]
    pub old_value: Option<Value>,
    #[serde(rename = "new", skip_serializing_if = "Option::is_none")]
    pub new_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Value>,
    pub importance: i16,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ActivityPayload {
    pub event_type: String,
    pub title: String,
    pub changes: Vec<ActivityChange>,
    pub change_count: usize,
    pub importance: i16,
}

/// Convert a platform snapshot transition into one compact, semantic event.
///
/// The raw JSON-path diff remains in `metadata_history`; this payload contains
/// only stable, user-facing concepts. A `suppressed` row deliberately marks a
/// sync whose only changes were volatile provider metadata, preventing the API
/// from mistaking it for an unprocessed legacy record.
pub fn build_activity_payload(
    platform: &str,
    old_data: Option<&Value>,
    new_data: &Value,
    raw_change_count: usize,
) -> ActivityPayload {
    if old_data.is_none() {
        let mut changes = baseline_changes(platform, new_data);
        let total = changes.len();
        changes.truncate(MAX_STORED_CHANGES);
        return ActivityPayload {
            event_type: "imported".to_string(),
            title: platform_label(platform).to_string(),
            importance: 30,
            changes,
            change_count: total,
        };
    }

    let old_data = old_data.expect("checked above");
    let mut changes = match platform {
        "steam" => steam_changes(old_data, new_data),
        "bangumi" => bangumi_changes(old_data, new_data),
        "github" => github_changes(old_data, new_data),
        "bilibili" => bilibili_changes(old_data, new_data),
        "netease" | "netease_music" => netease_changes(old_data, new_data),
        "x" => x_changes(old_data, new_data),
        "xbox" => xbox_changes(old_data, new_data),
        "mal" | "myanimelist" => mal_changes(old_data, new_data),
        "discord" => discord_changes(old_data, new_data),
        "psn" | "playstation" => psn_changes(old_data, new_data),
        _ => vec![ActivityChange {
            kind: "metric_changed".to_string(),
            subject_type: Some("platform".to_string()),
            subject_id: None,
            subject_title: None,
            subject_image: account_image(platform, new_data),
            metric: Some("data_changes".to_string()),
            old_value: None,
            new_value: Some(json!(raw_change_count)),
            delta: None,
            importance: 10,
        }],
    };

    if changes.is_empty() {
        return ActivityPayload {
            event_type: "suppressed".to_string(),
            title: platform_label(platform).to_string(),
            changes,
            change_count: 0,
            importance: 0,
        };
    }

    changes.sort_by(|a, b| {
        b.importance.cmp(&a.importance).then_with(|| {
            a.subject_title
                .as_deref()
                .unwrap_or_default()
                .cmp(b.subject_title.as_deref().unwrap_or_default())
        })
    });
    let total = changes.len();
    let importance = changes
        .iter()
        .map(|change| change.importance)
        .max()
        .unwrap_or(0);
    let title = if total == 1 {
        changes[0]
            .subject_title
            .clone()
            .unwrap_or_else(|| platform_label(platform).to_string())
    } else {
        platform_label(platform).to_string()
    };
    changes.truncate(MAX_STORED_CHANGES);

    ActivityPayload {
        event_type: "updated".to_string(),
        title,
        changes,
        change_count: total,
        importance,
    }
}

pub fn platform_label(platform: &str) -> &str {
    match platform {
        "steam" => "Steam",
        "github" => "GitHub",
        "bilibili" => "Bilibili",
        "youtube" => "YouTube",
        "netease" | "netease_music" => "NetEase Cloud Music",
        "bangumi" => "Bangumi",
        "x" => "X",
        "discord" => "Discord",
        "mal" | "myanimelist" => "MyAnimeList",
        "xbox" => "Xbox",
        "psn" | "playstation" => "PlayStation",
        _ => platform,
    }
}

/// Project a stored semantic change onto the public activity-card contract.
///
/// Activity events deliberately keep internal identifiers and ranking weights
/// for auditing and future presentation work. The public dashboard only needs
/// display labels and scalar before/after values, so no other stored fields are
/// allowed to cross this boundary.
pub fn public_activity_changes(changes: &Value) -> Value {
    let Some(changes) = changes.as_array() else {
        return Value::Array(Vec::new());
    };

    Value::Array(
        changes
            .iter()
            .filter_map(Value::as_object)
            .map(|change| {
                let mut public = serde_json::Map::new();
                for key in ["kind", "subject_title", "metric"] {
                    if let Some(value) = change.get(key).and_then(Value::as_str) {
                        public.insert(key.to_string(), Value::String(value.to_string()));
                    }
                }
                // Only public http(s) media URLs — never leak relative paths or data URIs.
                if let Some(image) = change
                    .get("subject_image")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|url| {
                        let lower = url.to_ascii_lowercase();
                        lower.starts_with("https://") || lower.starts_with("http://")
                    })
                {
                    public.insert(
                        "subject_image".to_string(),
                        Value::String(image.to_string()),
                    );
                }
                for key in ["old", "new", "delta"] {
                    if let Some(value) = change.get(key).filter(|value| {
                        value.is_null()
                            || value.is_boolean()
                            || value.is_number()
                            || value.is_string()
                    }) {
                        public.insert(key.to_string(), value.clone());
                    }
                }
                Value::Object(public)
            })
            .collect(),
    )
}

fn baseline_changes(platform: &str, data: &Value) -> Vec<ActivityChange> {
    let counts: Vec<(&str, usize)> = match platform {
        "steam" => vec![("games_count", array_len(data, "/games"))],
        "bangumi" => vec![("collections_count", array_len(data, "/collections"))],
        "github" => vec![("repositories_count", array_len(data, "/repos"))],
        "bilibili" => vec![
            ("subscriptions_count", array_len(data, "/bangumi")),
            ("collections_count", array_len(data, "/favorites")),
        ],
        "netease" | "netease_music" => {
            vec![("liked_songs_count", array_len(data, "/liked_songs"))]
        }
        "x" => vec![
            ("posts_count", array_len(data, "/tweets")),
            ("following_count", array_len(data, "/following")),
        ],
        "discord" => vec![
            ("servers_count", array_len(data, "/guilds")),
            ("connections_count", array_len(data, "/connections")),
        ],
        "mal" | "myanimelist" => vec![
            ("anime_count", array_len(data, "/anime_list")),
            ("manga_count", array_len(data, "/manga_list")),
        ],
        "xbox" => vec![("games_count", array_len(data, "/achievements/titles"))],
        "psn" | "playstation" => {
            vec![("games_count", array_len(data, "/trophy_titles"))]
        }
        _ => vec![(
            "data_sections_count",
            data.as_object().map(|object| object.len()).unwrap_or(0),
        )],
    };

    let avatar = account_image(platform, data);
    counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(metric, count)| ActivityChange {
            kind: "baseline".to_string(),
            subject_type: Some("platform".to_string()),
            subject_id: None,
            subject_title: None,
            subject_image: avatar.clone(),
            metric: Some(metric.to_string()),
            old_value: None,
            new_value: Some(json!(count)),
            delta: None,
            importance: 30,
        })
        .collect()
}

fn steam_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    // Steam owned-games payload has no cover URLs; derive capsule from appid.
    diff_items(
        &mut changes,
        old,
        new,
        "/games",
        "/appid",
        "game",
        &["/name"],
        &[],
        ImageSource::SteamAppId,
        &[("playtime_minutes", "/playtime_forever", 90)],
    );
    changes
}

fn bangumi_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    diff_items(
        &mut changes,
        old,
        new,
        "/collections",
        "/subject_id",
        "media",
        &["/subject/name_cn", "/subject/name"],
        &["/subject/images"],
        ImageSource::Pointers,
        &[
            ("rating", "/rate", 95),
            ("episodes_progress", "/ep_status", 100),
            ("volumes_progress", "/vol_status", 90),
            ("collection_status", "/type", 80),
        ],
    );
    changes
}

fn github_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    let avatar = account_image("github", new).or_else(|| account_image("github", old));
    // Repos rarely ship a dedicated image; owner avatar still reads better than a blank tile.
    diff_items(
        &mut changes,
        old,
        new,
        "/repos",
        "/name",
        "repository",
        &["/name"],
        &["/owner/avatar_url"],
        ImageSource::Pointers,
        &[
            ("stars", "/stargazers_count", 95),
            ("forks", "/forks_count", 75),
            ("watchers", "/watchers_count", 60),
            ("open_issues", "/open_issues_count", 55),
        ],
    );
    // If repo items lack owner avatars, fall back to the authenticated user avatar.
    if let Some(avatar) = avatar.clone() {
        for change in &mut changes {
            if change.subject_image.is_none() {
                change.subject_image = Some(avatar.clone());
            }
        }
    }
    push_metric(
        &mut changes,
        "account",
        None,
        None,
        avatar.clone(),
        "followers",
        old.pointer("/user/followers"),
        new.pointer("/user/followers"),
        80,
    );
    push_metric(
        &mut changes,
        "account",
        None,
        None,
        avatar.clone(),
        "repositories_count",
        old.pointer("/user/public_repos"),
        new.pointer("/user/public_repos"),
        55,
    );

    let old_contributions = contribution_total(old.pointer("/contribution_calendar"));
    let new_contributions = contribution_total(new.pointer("/contribution_calendar"));
    push_owned_metric(
        &mut changes,
        "account",
        None,
        None,
        avatar,
        "contributions",
        old_contributions.map(|value| json!(value)),
        new_contributions.map(|value| json!(value)),
        65,
    );
    changes
}

fn bilibili_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    diff_items(
        &mut changes,
        old,
        new,
        "/bangumi",
        "/season_id",
        "subscription",
        &["/title"],
        &["/cover"],
        ImageSource::Pointers,
        &[("progress", "/progress", 90)],
    );
    diff_items(
        &mut changes,
        old,
        new,
        "/favorites",
        "/id",
        "collection",
        &["/title"],
        &["/cover"],
        ImageSource::Pointers,
        &[("media_count", "/media_count", 70)],
    );
    changes
}

fn netease_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    let avatar = account_image("netease", new).or_else(|| account_image("netease", old));
    push_count_metric(
        &mut changes,
        "account",
        avatar.clone(),
        "liked_songs_count",
        array_len(old, "/liked_songs"),
        array_len(new, "/liked_songs"),
        90,
    );
    diff_items(
        &mut changes,
        old,
        new,
        "/liked_songs",
        "/id",
        "song",
        &["/name"],
        &["/al/picUrl", "/album/picUrl"],
        ImageSource::Pointers,
        &[],
    );
    for (metric, pointer, importance) in [
        ("followers", "/profile/followeds", 70),
        ("following_count", "/profile/follows", 45),
        ("playlists_count", "/profile/playlistCount", 55),
        ("level", "/profile/level", 65),
    ] {
        push_metric(
            &mut changes,
            "account",
            None,
            None,
            avatar.clone(),
            metric,
            old.pointer(pointer),
            new.pointer(pointer),
            importance,
        );
    }
    changes
}

fn x_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    let avatar = account_image("x", new).or_else(|| account_image("x", old));
    // Deliberately ignore metrics of accounts the user follows. Those values
    // caused the old widget to be dominated by unrelated follower-count noise.
    for (metric, pointer, importance) in [
        ("followers", "/user/public_metrics/followers_count", 90),
        (
            "following_count",
            "/user/public_metrics/following_count",
            55,
        ),
        ("posts_count", "/user/public_metrics/tweet_count", 80),
        ("likes", "/user/public_metrics/like_count", 65),
        ("media_count", "/user/public_metrics/media_count", 55),
    ] {
        push_metric(
            &mut changes,
            "account",
            None,
            None,
            avatar.clone(),
            metric,
            old.pointer(pointer),
            new.pointer(pointer),
            importance,
        );
    }
    changes
}

fn xbox_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    diff_items(
        &mut changes,
        old,
        new,
        "/achievements/titles",
        "/titleId",
        "game",
        &["/name"],
        &["/displayImage"],
        ImageSource::Pointers,
        &[
            ("achievements_count", "/achievement/currentAchievements", 95),
            ("gamerscore", "/achievement/currentGamerscore", 90),
            ("progress_percent", "/achievement/progressPercentage", 80),
        ],
    );
    changes
}

fn mal_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    for (pointer, subject_type, progress_pointer) in [
        ("/anime_list", "anime", "/list_status/num_episodes_watched"),
        ("/manga_list", "manga", "/list_status/num_chapters_read"),
    ] {
        diff_items(
            &mut changes,
            old,
            new,
            pointer,
            "/node/id",
            subject_type,
            &["/node/title"],
            &["/node/main_picture"],
            ImageSource::Pointers,
            &[
                ("progress", progress_pointer, 100),
                ("rating", "/list_status/score", 90),
                ("collection_status", "/list_status/status", 80),
            ],
        );
    }
    changes
}

fn discord_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    push_count_metric(
        &mut changes,
        "account",
        None,
        "servers_count",
        array_len(old, "/guilds"),
        array_len(new, "/guilds"),
        65,
    );
    push_count_metric(
        &mut changes,
        "account",
        None,
        "connections_count",
        array_len(old, "/connections"),
        array_len(new, "/connections"),
        55,
    );
    changes
}

fn psn_changes(old: &Value, new: &Value) -> Vec<ActivityChange> {
    let mut changes = Vec::new();
    diff_items(
        &mut changes,
        old,
        new,
        "/trophy_titles",
        "/npCommunicationId",
        "game",
        &["/trophyTitleName"],
        &["/trophyTitleIconUrl"],
        ImageSource::Pointers,
        &[("progress_percent", "/progress", 85)],
    );
    for (metric, pointer, importance) in [
        ("trophy_level", "/trophy_summary/trophyLevel", 80),
        ("progress_percent", "/trophy_summary/progress", 75),
    ] {
        push_metric(
            &mut changes,
            "account",
            None,
            None,
            None,
            metric,
            old.pointer(pointer),
            new.pointer(pointer),
            importance,
        );
    }
    changes
}

#[derive(Clone, Copy)]
enum ImageSource {
    /// Use JSON pointers on the item (string URL or image-size map).
    Pointers,
    /// Steam library art derived from numeric/string appid.
    SteamAppId,
}

#[allow(clippy::too_many_arguments)]
fn diff_items(
    changes: &mut Vec<ActivityChange>,
    old_data: &Value,
    new_data: &Value,
    array_pointer: &str,
    id_pointer: &str,
    subject_type: &str,
    title_pointers: &[&str],
    image_pointers: &[&str],
    image_source: ImageSource,
    metrics: &[(&str, &str, i16)],
) {
    let old_items = index_items(old_data, array_pointer, id_pointer);
    let new_items = index_items(new_data, array_pointer, id_pointer);
    let mut ids: HashSet<String> = old_items.keys().cloned().collect();
    ids.extend(new_items.keys().cloned());
    let mut ids: Vec<String> = ids.into_iter().collect();
    ids.sort();

    for id in ids {
        match (old_items.get(&id), new_items.get(&id)) {
            (None, Some(new_item)) => {
                let image = resolve_item_image(image_source, &id, new_item, image_pointers);
                push_item_change(
                    changes,
                    "item_added",
                    subject_type,
                    &id,
                    item_title(new_item, title_pointers),
                    image,
                    75,
                );
            }
            (Some(old_item), None) => {
                let image = resolve_item_image(image_source, &id, old_item, image_pointers);
                push_item_change(
                    changes,
                    "item_removed",
                    subject_type,
                    &id,
                    item_title(old_item, title_pointers),
                    image,
                    45,
                );
            }
            (Some(old_item), Some(new_item)) => {
                let title = item_title(new_item, title_pointers)
                    .or_else(|| item_title(old_item, title_pointers));
                let image = resolve_item_image(image_source, &id, new_item, image_pointers)
                    .or_else(|| resolve_item_image(image_source, &id, old_item, image_pointers));
                for (metric, pointer, importance) in metrics {
                    push_metric(
                        changes,
                        subject_type,
                        Some(id.clone()),
                        title.clone(),
                        image.clone(),
                        metric,
                        old_item.pointer(pointer),
                        new_item.pointer(pointer),
                        *importance,
                    );
                }
            }
            (None, None) => {}
        }
    }
}

fn push_item_change(
    changes: &mut Vec<ActivityChange>,
    kind: &str,
    subject_type: &str,
    subject_id: &str,
    subject_title: Option<String>,
    subject_image: Option<String>,
    importance: i16,
) {
    changes.push(ActivityChange {
        kind: kind.to_string(),
        subject_type: Some(subject_type.to_string()),
        subject_id: Some(subject_id.to_string()),
        subject_title,
        subject_image,
        metric: None,
        old_value: None,
        new_value: None,
        delta: None,
        importance,
    });
}

#[allow(clippy::too_many_arguments)]
fn push_metric(
    changes: &mut Vec<ActivityChange>,
    subject_type: &str,
    subject_id: Option<String>,
    subject_title: Option<String>,
    subject_image: Option<String>,
    metric: &str,
    old_value: Option<&Value>,
    new_value: Option<&Value>,
    importance: i16,
) {
    push_owned_metric(
        changes,
        subject_type,
        subject_id,
        subject_title,
        subject_image,
        metric,
        old_value.cloned(),
        new_value.cloned(),
        importance,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_owned_metric(
    changes: &mut Vec<ActivityChange>,
    subject_type: &str,
    subject_id: Option<String>,
    subject_title: Option<String>,
    subject_image: Option<String>,
    metric: &str,
    old_value: Option<Value>,
    new_value: Option<Value>,
    importance: i16,
) {
    if old_value == new_value || new_value.is_none() {
        return;
    }
    let delta = numeric_delta(old_value.as_ref(), new_value.as_ref());
    let kind = if metric.contains("progress") {
        "progress_changed"
    } else {
        "metric_changed"
    };
    changes.push(ActivityChange {
        kind: kind.to_string(),
        subject_type: Some(subject_type.to_string()),
        subject_id,
        subject_title,
        subject_image,
        metric: Some(metric.to_string()),
        old_value,
        new_value,
        delta,
        importance,
    });
}

fn push_count_metric(
    changes: &mut Vec<ActivityChange>,
    subject_type: &str,
    subject_image: Option<String>,
    metric: &str,
    old_count: usize,
    new_count: usize,
    importance: i16,
) {
    push_owned_metric(
        changes,
        subject_type,
        None,
        None,
        subject_image,
        metric,
        Some(json!(old_count)),
        Some(json!(new_count)),
        importance,
    );
}

fn numeric_delta(old_value: Option<&Value>, new_value: Option<&Value>) -> Option<Value> {
    let old = old_value?;
    let new = new_value?;
    if let (Some(old), Some(new)) = (old.as_i64(), new.as_i64()) {
        return Some(json!(new - old));
    }
    if let (Some(old), Some(new)) = (old.as_u64(), new.as_u64()) {
        let delta = i128::from(new) - i128::from(old);
        if let Ok(delta) = i64::try_from(delta) {
            return Some(json!(delta));
        }
    }
    let delta = new.as_f64()? - old.as_f64()?;
    serde_json::Number::from_f64(delta).map(Value::Number)
}

fn contribution_total(value: Option<&Value>) -> Option<i64> {
    value?.as_array().map(|days| {
        days.iter()
            .filter_map(|day| day.get("count").and_then(Value::as_i64))
            .sum()
    })
}

fn array_len(data: &Value, pointer: &str) -> usize {
    data.pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn index_items<'a>(
    data: &'a Value,
    array_pointer: &str,
    id_pointer: &str,
) -> HashMap<String, &'a Value> {
    data.pointer(array_pointer)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| scalar_string(item.pointer(id_pointer)?).map(|id| (id, item)))
                .collect()
        })
        .unwrap_or_default()
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn item_title(item: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        item.pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn resolve_item_image(
    source: ImageSource,
    id: &str,
    item: &Value,
    image_pointers: &[&str],
) -> Option<String> {
    match source {
        ImageSource::SteamAppId => steam_app_image(id),
        ImageSource::Pointers => image_pointers
            .iter()
            .find_map(|pointer| item.pointer(pointer).and_then(http_url_from_value)),
    }
}

fn steam_app_image(appid: &str) -> Option<String> {
    let appid = appid.trim();
    if appid.is_empty() || !appid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Portrait library art crops cleanly in a square/rounded tile.
    Some(format!(
        "https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg"
    ))
}

fn account_image(platform: &str, data: &Value) -> Option<String> {
    let pointers: &[&str] = match platform {
        "steam" => &["/user/avatarfull", "/user/avatar"],
        "github" => &["/user/avatar_url"],
        "bangumi" => &[
            "/user/avatar/large",
            "/user/avatar/medium",
            "/user/avatar/small",
        ],
        "bilibili" => &["/user/face"],
        "netease" | "netease_music" => &["/profile/avatarUrl"],
        "x" => &["/user/profile_image_url"],
        "xbox" => &["/profile/displayPicRaw", "/profile/gamerpic"],
        "mal" | "myanimelist" => &["/user/picture"],
        "psn" | "playstation" => &["/profile/avatarUrl", "/profile/avatar"],
        _ => &[],
    };
    pointers
        .iter()
        .find_map(|pointer| data.pointer(pointer).and_then(http_url_from_value))
        .map(|url| {
            // X serves `_normal` 48px thumbs by default; prefer a larger crop when present.
            if platform == "x" {
                url.replacen("_normal.", ".", 1)
            } else {
                url
            }
        })
}

fn http_url_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => sanitize_http_url(raw),
        Value::Object(map) => {
            // Bangumi / MAL style size maps: prefer compact tiles for the activity list.
            for key in [
                "grid", "medium", "common", "small", "large", "url", "default",
            ] {
                if let Some(url) = map
                    .get(key)
                    .and_then(Value::as_str)
                    .and_then(sanitize_http_url)
                {
                    return Some(url);
                }
            }
            // Nested thumbnail maps (YouTube-style { medium: { url } }).
            for key in ["medium", "default", "high", "standard"] {
                if let Some(url) = map
                    .get(key)
                    .and_then(|entry| entry.get("url"))
                    .and_then(Value::as_str)
                    .and_then(sanitize_http_url)
                {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn sanitize_http_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if let Some(rest) = trimmed.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        trimmed.to_string()
    };
    let lower = candidate.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        Some(candidate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_playtime_becomes_semantic_change() {
        let old = json!({"games": [{"appid": 10, "name": "Example", "playtime_forever": 60}]});
        let new = json!({"games": [{"appid": 10, "name": "Example", "playtime_forever": 135}]});
        let payload = build_activity_payload("steam", Some(&old), &new, 1);

        assert_eq!(payload.event_type, "updated");
        assert_eq!(payload.title, "Example");
        assert_eq!(
            payload.changes[0].metric.as_deref(),
            Some("playtime_minutes")
        );
        assert_eq!(payload.changes[0].delta, Some(json!(75)));
        assert_eq!(
            payload.changes[0].subject_image.as_deref(),
            Some("https://cdn.cloudflare.steamstatic.com/steam/apps/10/library_600x900.jpg")
        );
    }

    #[test]
    fn x_following_account_noise_is_suppressed() {
        let old = json!({
            "user": {"public_metrics": {"followers_count": 1}},
            "following": [{"id": "a", "public_metrics": {"followers_count": 10}}]
        });
        let new = json!({
            "user": {"public_metrics": {"followers_count": 1}},
            "following": [{"id": "a", "public_metrics": {"followers_count": 11}}]
        });
        let payload = build_activity_payload("x", Some(&old), &new, 1);

        assert_eq!(payload.event_type, "suppressed");
        assert!(payload.changes.is_empty());
    }

    #[test]
    fn bangumi_global_popularity_noise_is_suppressed() {
        let old = json!({"collections": [{
            "subject_id": 1,
            "rate": 8,
            "subject": {"name": "Example", "collection_total": 10}
        }]});
        let new = json!({"collections": [{
            "subject_id": 1,
            "rate": 8,
            "subject": {"name": "Example", "collection_total": 11}
        }]});
        let payload = build_activity_payload("bangumi", Some(&old), &new, 1);

        assert_eq!(payload.event_type, "suppressed");
    }

    #[test]
    fn bangumi_progress_carries_cover_image() {
        let old = json!({"collections": [{
            "subject_id": 1,
            "ep_status": 1,
            "subject": {
                "name": "Example",
                "images": {
                    "grid": "https://lain.bgm.tv/r/100/pic/cover/l/example.jpg",
                    "large": "https://lain.bgm.tv/pic/cover/l/example.jpg"
                }
            }
        }]});
        let new = json!({"collections": [{
            "subject_id": 1,
            "ep_status": 2,
            "subject": {
                "name": "Example",
                "images": {
                    "grid": "https://lain.bgm.tv/r/100/pic/cover/l/example.jpg",
                    "large": "https://lain.bgm.tv/pic/cover/l/example.jpg"
                }
            }
        }]});
        let payload = build_activity_payload("bangumi", Some(&old), &new, 1);

        assert_eq!(payload.event_type, "updated");
        assert_eq!(
            payload.changes[0].subject_image.as_deref(),
            Some("https://lain.bgm.tv/r/100/pic/cover/l/example.jpg")
        );
    }

    #[test]
    fn initial_import_is_a_compact_baseline() {
        let data = json!({"liked_songs": [{"id": 1}, {"id": 2}]});
        let payload = build_activity_payload("netease", None, &data, 400);

        assert_eq!(payload.event_type, "imported");
        assert_eq!(payload.change_count, 1);
        assert_eq!(
            payload.changes[0].metric.as_deref(),
            Some("liked_songs_count")
        );
        assert_eq!(payload.changes[0].new_value, Some(json!(2)));
    }

    #[test]
    fn public_changes_expose_only_display_fields_and_scalar_values() {
        let stored = json!([{
            "kind": "metric_changed",
            "subject_type": "repository",
            "subject_id": "private-internal-id",
            "subject_title": "Example",
            "metric": "stars",
            "old": 1,
            "new": 2,
            "delta": 1,
            "importance": 95,
            "internal_note": "must not leak"
        }, {
            "kind": "metric_changed",
            "metric": "data_changes",
            "new": {"raw": "object values are not public"}
        }]);

        assert_eq!(
            public_activity_changes(&stored),
            json!([{
                "kind": "metric_changed",
                "subject_title": "Example",
                "metric": "stars",
                "old": 1,
                "new": 2,
                "delta": 1
            }, {
                "kind": "metric_changed",
                "metric": "data_changes"
            }])
        );
    }

    #[test]
    fn public_changes_keep_http_subject_images_only() {
        let stored = json!([{
            "kind": "metric_changed",
            "subject_title": "Example",
            "subject_image": "https://cdn.example/cover.jpg",
            "metric": "stars",
            "old": 1,
            "new": 2,
            "delta": 1
        }, {
            "kind": "metric_changed",
            "subject_image": "javascript:alert(1)",
            "metric": "stars",
            "new": 1
        }]);

        assert_eq!(
            public_activity_changes(&stored),
            json!([{
                "kind": "metric_changed",
                "subject_title": "Example",
                "subject_image": "https://cdn.example/cover.jpg",
                "metric": "stars",
                "old": 1,
                "new": 2,
                "delta": 1
            }, {
                "kind": "metric_changed",
                "metric": "stars",
                "new": 1
            }])
        );
    }
}
