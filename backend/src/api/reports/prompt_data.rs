//! Pack `SmartFilteredData` for the report prompt, and parse the model JSON.
//!
//! Pretty-print + a hard 12k slice was dropping Look fields that sit late in
//! the blob (GitHub calendar, Discord guilds). Compact JSON, strip display
//! noise, and shrink arrays so the budget keeps evaluation keys.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::services::smart_filter::SmartFilteredData;

/// Compact Data budget. Instruction body stays under 4200; this is the payload.
pub const REPORT_DATA_CHAR_BUDGET: usize = 16_000;

const DROP_KEYS: &[&str] = &[
    "cover",
    "image",
    "avatar",
    "url",
    "channel_url",
    "custom_url",
    "display_image",
    "icon_url",
    "profile_image_url",
    "banner_url",
    "user_avatar",
    "video_id",
    "bvid",
    "video_summary",
    "game_summary",
    "repo_summary",
    "music_summary",
    "collection_summary",
    "post_summary",
    "following_summary",
    "community_summary",
    "gaming_summary",
    "trophy_summary_text",
    "summary",
    "description",
    "unknown_songs",
];

/// Keys Look still needs even when named `description` (X following / GitHub repo).
const KEEP_DESCRIPTION_UNDER: &[&str] = &["following_sample", "recent_repos"];

pub fn platform_report_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "insights", "card_visuals"],
        "properties": {
            "summary": { "type": "string" },
            "insights": {
                "type": "array",
                "items": { "type": "string" }
            },
            "card_visuals": { "type": "object" }
        }
    })
}

pub fn serialize_for_report_prompt(data: &SmartFilteredData) -> Result<String, String> {
    let mut value = serde_json::to_value(data).map_err(|e| e.to_string())?;
    strip_noise(&mut value, "");
    strip_dead_metrics(&mut value);
    slim_unknown_metadata(&mut value);
    filter_private_discord_connections(&mut value);
    compact_github_calendar(&mut value);
    truncate_strings(&mut value, 160, 280);
    cap_arrays(&mut value, &data.platform, CapLevel::Normal);
    let mut encoded = encode(&value)?;
    if over_budget(&encoded) {
        cap_arrays(&mut value, &data.platform, CapLevel::Tight);
        encoded = encode(&value)?;
    }
    if over_budget(&encoded) {
        drop_lowest_priority(&mut value, &data.platform);
        encoded = encode(&value)?;
    }
    if over_budget(&encoded) {
        cap_arrays(&mut value, &data.platform, CapLevel::Emergency);
        truncate_strings(&mut value, 80, 120);
        encoded = encode(&value)?;
    }
    if over_budget(&encoded) {
        cap_arrays(&mut value, &data.platform, CapLevel::Emergency);
        if let Some(obj) = content_obj(&mut value) {
            for key in [
                "following_sample",
                "guilds_preview",
                "recent_repos",
                "recent_games",
                "recent_songs",
                "recent_videos",
                "top_posts",
                "recent_posts",
            ] {
                cap(obj, key, 1);
            }
        }
        truncate_strings(&mut value, 40, 80);
        encoded = encode(&value)?;
    }
    Ok(encoded)
}

#[derive(Deserialize)]
struct AiReportJson {
    summary: String,
    insights: Vec<String>,
    #[serde(default)]
    card_visuals: Value,
}

pub fn parse_platform_report_json(raw: &str) -> Result<(String, Vec<String>, Value), String> {
    let candidate =
        extract_json_object(raw).ok_or_else(|| "no JSON object in response".to_string())?;
    let parsed: AiReportJson = serde_json::from_str(candidate).map_err(|e| e.to_string())?;
    if parsed.summary.trim().is_empty() {
        return Err("empty summary".into());
    }
    let visuals = if parsed.card_visuals.is_object() || parsed.card_visuals.is_null() {
        if parsed.card_visuals.is_null() {
            json!({})
        } else {
            parsed.card_visuals
        }
    } else {
        json!({})
    };
    Ok((parsed.summary, parsed.insights, visuals))
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    let unfenced = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let start = unfenced.find('{')?;
    let end = unfenced.rfind('}')?;
    if end > start {
        Some(&unfenced[start..=end])
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum CapLevel {
    Normal,
    Tight,
    Emergency,
}

impl CapLevel {
    fn n(self, normal: usize) -> usize {
        match self {
            Self::Normal => normal,
            Self::Tight => (normal / 2).max(3),
            Self::Emergency => 3,
        }
    }
}

fn content_obj(value: &mut Value) -> Option<&mut Map<String, Value>> {
    value
        .get_mut("content_analysis")
        .and_then(|v| v.as_object_mut())
}

fn encode(value: &Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn over_budget(encoded: &str) -> bool {
    encoded.chars().count() > REPORT_DATA_CHAR_BUDGET
}

fn strip_noise(value: &mut Value, parent: &str) {
    match value {
        Value::Object(map) => {
            let keep_desc = KEEP_DESCRIPTION_UNDER.contains(&parent);
            map.retain(|key, _| {
                if key == "description" && keep_desc {
                    return true;
                }
                !DROP_KEYS.contains(&key.as_str())
            });
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    strip_noise(child, &key);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_noise(item, parent);
            }
        }
        _ => {}
    }
}

fn strip_dead_metrics(value: &mut Value) {
    if let Some(stats) = value
        .pointer_mut("/content_analysis/engagement_stats")
        .and_then(|v| v.as_object_mut())
    {
        stats.remove("liked_posts_count");
    }
}

fn filter_private_discord_connections(value: &mut Value) {
    let Some(obj) = content_obj(value) else {
        return;
    };
    let public_types: Vec<String> = {
        let Some(connections) = obj.get_mut("connections").and_then(|v| v.as_array_mut()) else {
            return;
        };
        connections.retain(|item| item.get("visibility").and_then(Value::as_i64).unwrap_or(0) != 0);
        connections
            .iter()
            .filter_map(|item| item.get("type").and_then(Value::as_str).map(str::to_string))
            .collect()
    };
    let Some(graph) = obj
        .get_mut("identity_graph")
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    if let Some(platforms) = graph
        .get_mut("linked_platforms")
        .and_then(|v| v.as_array_mut())
    {
        platforms.retain(|item| {
            item.as_str()
                .is_some_and(|name| public_types.iter().any(|t| t == name))
        });
    }
    if let Some(check) = graph.get_mut("cross_check").and_then(|v| v.as_object_mut()) {
        for (key, entry) in check.iter_mut() {
            if public_types.iter().any(|t| t == key) {
                continue;
            }
            if let Some(entry) = entry.as_object_mut() {
                entry.insert("discord_linked".into(), json!(false));
                entry.insert("id_match".into(), Value::Null);
                entry.insert("name_match".into(), Value::Null);
            }
        }
    }
}

fn truncate_strings(value: &mut Value, desc_chars: usize, text_chars: usize) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if let Value::String(text) = child {
                    let limit = if key == "text" {
                        text_chars
                    } else if key == "description" {
                        desc_chars
                    } else {
                        continue;
                    };
                    if text.chars().count() > limit {
                        *text = text.chars().take(limit).collect();
                    }
                } else {
                    truncate_strings(child, desc_chars, text_chars);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                truncate_strings(item, desc_chars, text_chars);
            }
        }
        _ => {}
    }
}

fn slim_unknown_metadata(value: &mut Value) {
    let Some(items) = value
        .get_mut("raw_unknown_content")
        .and_then(|v| v.as_array_mut())
    else {
        return;
    };
    for item in items {
        if let Some(meta) = item.get_mut("metadata").and_then(|v| v.as_object_mut()) {
            meta.retain(|key, _| key == "progress");
        }
    }
}

fn compact_github_calendar(value: &mut Value) {
    let Some(obj) = content_obj(value) else {
        return;
    };
    let stats = obj.get_mut("contribution_calendar").and_then(|calendar| {
        let days = calendar.as_array_mut()?;
        let span = days.len();
        let active = days
            .iter()
            .filter(|day| day.get("count").and_then(Value::as_i64).unwrap_or(0) > 0)
            .count();
        days.retain(|day| day.get("count").and_then(Value::as_i64).unwrap_or(0) > 0);
        if days.len() > 180 {
            let skip = days.len() - 180;
            days.drain(..skip);
        }
        Some((span, active))
    });
    if let Some((span, active)) = stats {
        obj.insert("calendar_span_days".into(), json!(span));
        obj.insert("calendar_active_days".into(), json!(active));
    }
}

fn cap_arrays(value: &mut Value, platform: &str, level: CapLevel) {
    if let Some(unknown) = value
        .get_mut("raw_unknown_content")
        .and_then(|v| v.as_array_mut())
    {
        unknown.truncate(level.n(20));
    }
    let Some(obj) = value
        .get_mut("content_analysis")
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    match platform {
        "x" => {
            cap(obj, "following_sample", level.n(40));
            cap(obj, "top_posts", level.n(10));
            cap(obj, "recent_posts", level.n(10));
        }
        "github" => cap(obj, "recent_repos", level.n(16)),
        "youtube" | "bilibili" => cap(obj, "recent_videos", level.n(12)),
        "steam" => cap(obj, "recent_games", level.n(16)),
        "netease" => cap(obj, "recent_songs", level.n(16)),
        "bangumi" | "mal" => {
            cap(obj, "top_rated_subjects", level.n(12));
            cap(obj, "watching_subjects", level.n(12));
            cap(obj, "recent_updates", level.n(8));
        }
        "discord" => {
            cap(obj, "guilds_preview", level.n(12));
            cap(obj, "connections", level.n(12));
        }
        "xbox" | "psn" => {
            cap(obj, "recent_titles", level.n(12));
            cap(obj, "top_completed_titles", level.n(8));
        }
        _ => {}
    }
}

fn cap(obj: &mut Map<String, Value>, key: &str, n: usize) {
    if let Some(Value::Array(items)) = obj.get_mut(key) {
        items.truncate(n);
    }
}

fn drop_lowest_priority(value: &mut Value, platform: &str) {
    if let Some(unknown) = value.get_mut("raw_unknown_content") {
        *unknown = json!([]);
    }
    let Some(obj) = value
        .get_mut("content_analysis")
        .and_then(|v| v.as_object_mut())
    else {
        return;
    };
    let drop_keys: &[&str] = match platform {
        "x" => &["recent_posts"],
        "github" => &["contribution_calendar"],
        "discord" => &["connections"],
        "bangumi" | "mal" => &["recent_updates"],
        _ => &[],
    };
    for key in drop_keys {
        obj.remove(*key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x_data(following: usize, posts: usize, post_text_len: usize) -> SmartFilteredData {
        let filler = "字".repeat(post_text_len);
        let following_sample: Vec<Value> = (0..following)
            .map(|i| {
                json!({
                    "username": format!("acc{i}"),
                    "name": format!("Account {i}"),
                    "description": "circle",
                    "follower_count": 10,
                    "verified": false,
                    "profile_image_url": "https://example.com/f.jpg"
                })
            })
            .collect();
        let recent_posts: Vec<Value> = (0..posts)
            .map(|i| {
                json!({
                    "id": format!("{i}"),
                    "text": filler,
                    "like_count": 0,
                    "retweet_count": 0,
                    "reply_count": 0,
                    "impression_count": 0
                })
            })
            .collect();
        serde_json::from_value(json!({
            "platform": "x",
            "user_summary": {
                "username": "demo",
                "user_id": "1",
                "level": null,
                "stats": {
                    "follower_count": 10,
                    "following_count": 20,
                    "total_content": posts
                }
            },
            "content_analysis": {
                "post_summary": "machine",
                "user_name": "Demo",
                "user_avatar": "https://example.com/a.jpg",
                "engagement_stats": {
                    "total_posts": posts,
                    "total_likes_received": 1,
                    "total_retweets_received": 0,
                    "total_replies_received": 0,
                    "total_impressions": 0,
                    "liked_posts_count": 0
                },
                "following_summary": "machine",
                "following_sample": following_sample,
                "recent_posts": recent_posts,
                "top_posts": [],
                "language_distribution": {}
            },
            "raw_unknown_content": []
        }))
        .expect("x fixture")
    }

    #[test]
    fn compact_payload_keeps_following_when_posts_are_huge() {
        let data = x_data(8, 40, 400);
        let pretty = serde_json::to_string_pretty(&data).unwrap();
        let packed = serialize_for_report_prompt(&data).unwrap();
        assert!(packed.len() < pretty.len());
        assert!(packed.chars().count() <= REPORT_DATA_CHAR_BUDGET);
        assert!(packed.contains("acc0"));
        assert!(!packed.contains("https://example.com"));
        assert!(!packed.contains("post_summary"));
        assert!(!packed.contains("liked_posts_count"));
        serde_json::from_str::<Value>(&packed).unwrap();
    }

    #[test]
    fn github_calendar_keeps_density_counts() {
        let days: Vec<Value> = (0..10)
            .map(|i| json!({ "date": format!("2026-01-{:02}", i + 1), "count": i % 3 }))
            .collect();
        let data: SmartFilteredData = serde_json::from_value(json!({
            "platform": "github",
            "user_summary": {
                "username": "dev",
                "user_id": "1",
                "level": null,
                "stats": { "follower_count": null, "following_count": null, "total_content": 1 }
            },
            "content_analysis": {
                "repo_summary": "machine",
                "language_distribution": { "Rust": 2 },
                "recent_repos": [{ "name": "app", "stars": 3, "description": "x" }],
                "contribution_calendar": days,
                "public_repos": 4
            },
            "raw_unknown_content": []
        }))
        .expect("github fixture");
        let packed: Value =
            serde_json::from_str(&serialize_for_report_prompt(&data).unwrap()).unwrap();
        assert_eq!(packed["content_analysis"]["calendar_span_days"], 10);
        assert_eq!(packed["content_analysis"]["calendar_active_days"], 6);
        assert_eq!(
            packed["content_analysis"]["contribution_calendar"]
                .as_array()
                .unwrap()
                .len(),
            6
        );
    }

    #[test]
    fn discord_private_connections_stay_out_of_prompt() {
        let data: SmartFilteredData = serde_json::from_value(json!({
            "platform": "discord",
            "user_summary": {
                "username": "n",
                "user_id": "1",
                "level": null,
                "stats": { "follower_count": null, "following_count": null, "total_content": 0 }
            },
            "content_analysis": {
                "community_summary": "machine",
                "profile": {
                    "display_name": "n",
                    "username": "n",
                    "badges": [],
                    "mfa_enabled": false
                },
                "guild_stats": {
                    "guild_count": 1,
                    "owned_guild_count": 0,
                    "admin_guild_count": 0,
                    "manage_guild_count": 0,
                    "total_member_reach": 0,
                    "total_online_reach": 0,
                    "community_guild_count": 0,
                    "partnered_or_verified_count": 0
                },
                "guilds_preview": [{
                    "id": "1",
                    "name": "room",
                    "owner": false,
                    "permissions_highlight": [],
                    "feature_highlight": []
                }],
                "connections": [
                    { "type": "steam", "name": "public", "id": "1", "verified": true, "visibility": 1 },
                    { "type": "github", "name": "secret", "id": "2", "verified": true, "visibility": 0 }
                ],
                "identity_graph": {
                    "linked_platforms": ["steam", "github"],
                    "cross_check": {
                        "github": { "discord_linked": true, "myriad_configured": false }
                    }
                }
            },
            "raw_unknown_content": []
        }))
        .expect("discord fixture");
        let packed: Value =
            serde_json::from_str(&serialize_for_report_prompt(&data).unwrap()).unwrap();
        let names: Vec<&str> = packed["content_analysis"]["connections"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["name"].as_str())
            .collect();
        assert_eq!(names, ["public"]);
        let platforms: Vec<&str> = packed["content_analysis"]["identity_graph"]["linked_platforms"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(platforms, ["steam"]);
        assert_eq!(
            packed["content_analysis"]["identity_graph"]["cross_check"]["github"]["discord_linked"],
            false
        );
    }

    #[test]
    fn parse_accepts_fenced_json_and_rejects_prose() {
        let raw = "```json\n{\"summary\":\"画像\",\"insights\":[\"a\"],\"card_visuals\":{}}\n```";
        let (summary, insights, visuals) = parse_platform_report_json(raw).unwrap();
        assert_eq!(summary, "画像");
        assert_eq!(insights, vec!["a".to_string()]);
        assert!(visuals.is_object());
        assert!(parse_platform_report_json("观看偏好：番剧很多").is_err());
        assert!(parse_platform_report_json("{\"insights\":[]}").is_err());
    }
}
