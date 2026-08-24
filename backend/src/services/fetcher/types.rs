// Platform data fetching service
use serde::{Deserialize, Serialize};

pub struct PlatformFetcher {
    pub(crate) client: reqwest::Client,
}

// Bilibili 数据结构
#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliUserInfo {
    pub mid: i64,
    pub name: String,
    pub face: String,
    pub sign: String,
    pub level: i32,
    pub following: i64,
    pub follower: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliFavorite {
    pub id: i64,
    pub title: String,
    pub cover: String,
    pub intro: String,
    pub media_count: i32,
    pub fav_state: i32,
    pub videos: Vec<BilibiliFavoriteVideo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliFavoriteVideo {
    pub bvid: String,
    pub title: String,
    pub cover: String,
    pub intro: String,
    pub upper_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliBangumi {
    pub season_id: i64,
    pub title: String,
    pub cover: String,
    pub season_type: i32, // 1: 动画, 2: 电影, 3: 纪录片, 4: 国创, 5: 电视剧
    pub progress: String,
    pub badge: String,
}

// Steam 数据结构
#[derive(Debug, Serialize, Deserialize)]
pub struct SteamUserInfo {
    pub steamid: String,
    pub personaname: String,
    pub profileurl: String,
    pub avatar: String,
    pub avatarfull: String,
    pub timecreated: Option<i64>,
    pub communityvisibilitystate: Option<i32>,
    pub personastate: i32,
    pub personastate_label: String,
    pub lastlogoff: Option<i64>,
    pub gameid: Option<String>,
    pub gameextrainfo: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SteamGame {
    pub appid: i64,
    pub name: String,
    pub playtime_forever: i32, // 分钟
    pub playtime_2weeks: Option<i32>,
    pub img_icon_url: String,
    pub img_logo_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SteamWishlistItem {
    pub appid: i64,
    pub name: String,
    pub capsule: String,
    pub review_score: i32,
    pub review_desc: String,
    pub priority: i32,
}

pub(crate) const BANGUMI_API_BASE: &str = "https://api.bgm.tv";
pub(crate) const DEFAULT_BANGUMI_USER_AGENT: &str = "myriad/Myriad";

pub(crate) fn steam_persona_state_label(state: i32) -> &'static str {
    match state {
        0 => "offline",
        1 => "online",
        2 => "busy",
        3 => "away",
        4 => "snooze",
        5 => "looking_to_trade",
        6 => "looking_to_play",
        _ => "unknown",
    }
}

pub(crate) fn optional_json_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
}
