//! The site owner's Steam status, cached for everyone who asks: the public
//! presence card polls it and she glances at it, so one fetch serves both.
//! Stale-while-revalidate: a fresh hit is served as is, a stale one served
//! while a single background refresh runs, a cold cache fetched once. The
//! API key stays in the backend; only the status comes back.

use std::sync::atomic::{AtomicBool, Ordering};

use sea_orm::DatabaseConnection;
use serde::Serialize;

use crate::services::fetcher::{PlatformFetcher, SteamUserInfo};
use crate::services::platform_id::PlatformId;

#[derive(Debug, Clone, Serialize)]
pub struct SteamPresenceResponse {
    pub steamid: String,
    pub personaname: String,
    pub avatar: String,
    pub profileurl: String,
    pub personastate: i32,
    pub personastate_label: String,
    pub is_online: bool,
    pub is_in_game: bool,
    pub gameid: Option<String>,
    pub gameextrainfo: Option<String>,
    pub lastlogoff: Option<i64>,
    /// 近两周游玩总时长（分钟）；受频率限制，可能为 None
    pub recent_2weeks_minutes: Option<i32>,
    pub checked_at: String,
}

impl From<SteamUserInfo> for SteamPresenceResponse {
    fn from(info: SteamUserInfo) -> Self {
        let is_in_game = info.gameid.is_some() || info.gameextrainfo.is_some();
        Self {
            steamid: info.steamid,
            personaname: info.personaname,
            // 用高清头像（184px），前端头像框放大后不糊；steamstatic/akamai 走站内代理防盗链
            avatar: {
                let raw = if info.avatarfull.trim().is_empty() {
                    info.avatar.as_str()
                } else {
                    info.avatarfull.as_str()
                };
                crate::services::image_proxy_urls::proxy_image_url(raw)
            },
            profileurl: info.profileurl,
            personastate: info.personastate,
            personastate_label: info.personastate_label,
            is_online: info.personastate != 0 || is_in_game,
            is_in_game,
            gameid: info.gameid,
            gameextrainfo: info.gameextrainfo,
            lastlogoff: info.lastlogoff,
            // 由 handler 按频率限制单独填充
            recent_2weeks_minutes: None,
            checked_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// 近两周时长的服务端缓存：GetRecentlyPlayedGames 是额外一次 Steam 调用，
/// 且是 2 周滚动总量、变化极慢，用 6h 长 TTL 限流，避免每次刷新在线状态都白打一遍。
/// `std::sync::Mutex`: short critical section only; never hold across `.await`.
static RECENT_PLAYTIME_CACHE: std::sync::Mutex<Option<RecentPlaytimeCache>> =
    std::sync::Mutex::new(None);
const RECENT_PLAYTIME_TTL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

struct RecentPlaytimeCache {
    steam_id: String,
    minutes: i32,
    fetched_at: std::time::Instant,
}

/// 读缓存：命中且同一 steam_id 且未过期才返回
fn cached_recent_playtime(steam_id: &str) -> Option<i32> {
    let guard = RECENT_PLAYTIME_CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.steam_id == steam_id && entry.fetched_at.elapsed() < RECENT_PLAYTIME_TTL {
        Some(entry.minutes)
    } else {
        None
    }
}

fn store_recent_playtime(steam_id: &str, minutes: i32) {
    if let Ok(mut guard) = RECENT_PLAYTIME_CACHE.lock() {
        *guard = Some(RecentPlaytimeCache {
            steam_id: steam_id.to_string(),
            minutes,
            fetched_at: std::time::Instant::now(),
        });
    }
}

/// 在线状态的服务端缓存：多访客共享同一份，避免每个访客每次轮询都打一遍
/// GetPlayerSummaries。120s TTL 兼顾在线状态时效性与配额，头像/用户名随之一起缓存。
/// 采用 stale-while-revalidate：Fresh/Stale 命中立刻返回；冷缓存同步拉一次。
/// `std::sync::Mutex`: short critical section only; never hold across `.await`.
static PRESENCE_CACHE: std::sync::Mutex<Option<PresenceCache>> = std::sync::Mutex::new(None);
const PRESENCE_TTL: std::time::Duration = std::time::Duration::from_secs(120);
/// 后台刷新的在飞标记：并发过期时只触发一次刷新
static PRESENCE_REFRESHING: AtomicBool = AtomicBool::new(false);

struct PresenceCache {
    steam_id: String,
    presence: SteamPresenceResponse,
    fetched_at: std::time::Instant,
}

/// 缓存命中态：新鲜（TTL 内）/ 陈旧（已过期但可先用）
pub(crate) enum PresenceHit {
    Fresh(SteamPresenceResponse),
    Stale(SteamPresenceResponse),
}

/// 读缓存：同一 steam_id 才命中，按 TTL 区分新鲜/陈旧
pub(crate) fn read_presence(steam_id: &str) -> Option<PresenceHit> {
    let guard = PRESENCE_CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.steam_id != steam_id {
        return None;
    }
    let presence = entry.presence.clone();
    if entry.fetched_at.elapsed() < PRESENCE_TTL {
        Some(PresenceHit::Fresh(presence))
    } else {
        Some(PresenceHit::Stale(presence))
    }
}

fn store_presence(steam_id: &str, presence: &SteamPresenceResponse) {
    if let Ok(mut guard) = PRESENCE_CACHE.lock() {
        *guard = Some(PresenceCache {
            steam_id: steam_id.to_string(),
            presence: presence.clone(),
            fetched_at: std::time::Instant::now(),
        });
    }
}

/// 实时拉取一次 presence（含近两周时长的 6h 子缓存）并落缓存
pub(crate) async fn fetch_and_store_presence(
    api_key: &str,
    steam_id: &str,
) -> anyhow::Result<SteamPresenceResponse> {
    let fetcher = PlatformFetcher::new().await;
    let info = fetcher.fetch_steam_user(api_key, steam_id).await?;
    let mut presence: SteamPresenceResponse = info.into();

    // 近两周时长：命中缓存直接用，否则受 6h TTL 限流后再打一次 Steam
    presence.recent_2weeks_minutes = match cached_recent_playtime(steam_id) {
        Some(minutes) => Some(minutes),
        None => match fetcher.fetch_steam_recent_playtime(api_key, steam_id).await {
            Ok(minutes) => {
                store_recent_playtime(steam_id, minutes);
                Some(minutes)
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch Steam recent playtime for {}: {}",
                    steam_id,
                    e
                );
                None
            }
        },
    };

    store_presence(steam_id, &presence);
    Ok(presence)
}

/// 后台异步刷新缓存，不阻塞当前响应；并发时靠 in-flight 标记只刷一次
pub(crate) fn trigger_presence_refresh(api_key: String, steam_id: String) {
    if PRESENCE_REFRESHING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        if let Err(e) = fetch_and_store_presence(&api_key, &steam_id).await {
            tracing::warn!(
                "Background Steam presence refresh failed for {}: {}",
                steam_id,
                e
            );
        }
        PRESENCE_REFRESHING.store(false, Ordering::Release);
    });
}

/// Stored, trimmed, non-blank value. Steam credentials are DB-only, like the
/// fetch arm: [`PlatformId::credentials_present`] is the gate for both.
pub(crate) fn stored(value: Option<&String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The site's own Steam status, for the persona: fresh from the cache, else
/// fetched now. `None` when Steam is off, not configured, or unreachable.
/// The key stays on this side; only the status comes back.
pub(crate) async fn site_presence(db: &DatabaseConnection) -> Option<SteamPresenceResponse> {
    let config = crate::services::config_service::ConfigService::new(db.clone())
        .load_config()
        .await
        .ok()
        .unwrap_or_default();
    if PlatformId::Steam.explicit_enabled(&config) == Some(false) {
        return None;
    }
    let api_key = stored(config.steam_api_key.as_ref())?;
    let steam_id = stored(config.steam_id.as_ref())?;
    if let Some(PresenceHit::Fresh(presence)) = read_presence(&steam_id) {
        return Some(presence);
    }
    fetch_and_store_presence(&api_key, &steam_id).await.ok()
}
