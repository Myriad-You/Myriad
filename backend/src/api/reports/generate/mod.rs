//! Platform report generation: HTTP handlers, persist, AI internals, and read enrich.

mod enrich;
mod generate_internal;
mod persist;

pub(crate) use enrich::finalize_public_platform_report;
pub(crate) use generate_internal::{
    anime_status_counts_five, generate_platform_reports_internal, github_contribution_level,
    normalize_steam_player_type,
};
pub(crate) use persist::MAX_CONCURRENT_PLATFORM_REPORTS;

use axum::{extract::State, http::HeaderMap, Extension, Json};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::platform_reports;
use crate::services::smart_filter::SmartFilteredData;
use myriad_error::AppError;

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneratePlatformReportsRequest {
    pub platforms: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlatformReport {
    pub platform: String,
    pub metadata: SmartFilteredData,
    pub summary: String,
    pub insights: Vec<String>,
    #[serde(default)]
    pub card_visuals: Value,
    pub created_at: String,
    /// Host UI locale at generation time (`zh-CN` / `ja-JP` / `en-US`).
    #[serde(default)]
    pub locale: String,
}

/// User id under which public platform reports are stored.
/// Prefer durable site owner so home ReportCards (which always *read* owner
/// reports) find rows written by any admin who generates them.
async fn report_storage_user_id(db: &DatabaseConnection, actor_id: i32) -> i32 {
    match crate::api::profile::site_owner_user_id(db).await {
        Ok(owner_id) => {
            if owner_id != actor_id {
                tracing::info!(
                    "Storing platform reports under site owner {} (actor was {})",
                    owner_id,
                    actor_id
                );
            }
            owner_id
        }
        Err(_) => actor_id,
    }
}

/// 生成平台报告（第一层）
/// POST /api/reports/platform
pub async fn generate_platform_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    Json(req): Json<GeneratePlatformReportsRequest>,
) -> Result<Json<Value>, HttpError> {
    tracing::info!("📊 [ENTRY] generate_platform_reports called");
    tracing::info!("   Platforms: {:?}", req.platforms);
    tracing::info!("   User: {} (ID: {})", claims.username, claims.sub);

    let actor_id = claims.sub.parse::<i32>().map_err(|e| {
        tracing::error!("❌ Failed to parse user_id: {}", e);
        HttpError(AppError::unauthorized("Unauthorized"))
    })?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    let locale = super::locale::locale_from_headers(&headers);
    tracing::info!("   Locale: {:?}", locale);
    let (platform_reports, skipped) =
        generate_platform_reports_internal(&db, user_id, req.platforms.clone(), locale).await;

    // 将跳过原因结构化，便于前端逐平台展示
    let skipped_json: Vec<Value> = skipped
        .iter()
        .map(|(platform, reason)| json!({ "platform": platform, "reason": reason }))
        .collect();

    if platform_reports.is_empty() {
        tracing::warn!(
            "⚠️ No platform reports generated for platforms: {:?}, skipped: {:?}",
            req.platforms,
            skipped
        );
        // 有具体原因时透出首个原因，否则回退到通用文案
        let locale_tag = locale.unwrap_or(super::locale::DEFAULT_AUTO_REGEN_LOCALE);
        let message = skipped
            .first()
            .map(|(_, reason)| reason.clone())
            .unwrap_or_else(|| super::locale::generate_none_message(locale_tag));
        return Ok(Json(json!({
            "success": false,
            "message": message,
            "reports": [],
            "skipped": skipped_json,
            "token_estimate": 0
        })));
    }

    // Token优化：估算每个报告的大小
    let total_tokens: usize = platform_reports
        .iter()
        .map(|r| crate::services::smart_filter::SmartFilter::estimate_token_size(&r.metadata))
        .sum();

    tracing::info!(
        "✅ Generated {} platform reports, estimated tokens: {}",
        platform_reports.len(),
        total_tokens
    );

    Ok(Json(json!({
        "success": true,
        "reports": platform_reports,
        "skipped": skipped_json,
        "token_estimate": total_tokens,
    })))
}

/// 一键生成所有启用平台的平台报告
/// POST /api/reports/generate-all
pub async fn generate_all_reports(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    let actor_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    // 1. 获取用户启用的所有平台（AppState.dynamic_config，与 GLOBAL_* 同 Arc）
    let config = dynamic_config.read().await;
    let enabled_platforms = [
        (
            "bilibili",
            config
                .bilibili_enabled
                .unwrap_or(config.bilibili_uid.as_ref().is_some()),
        ),
        (
            "steam",
            config
                .steam_enabled
                .unwrap_or(config.steam_api_key.as_ref().is_some()),
        ),
        (
            "github",
            config
                .github_enabled
                .unwrap_or(config.github_username.as_ref().is_some()),
        ),
        (
            "youtube",
            config.youtube_enabled.unwrap_or(
                config.youtube_api_key.as_ref().is_some()
                    && config.youtube_channel_id.as_ref().is_some(),
            ),
        ),
        (
            "netease",
            config
                .netease_enabled
                .unwrap_or(config.netease_user_id.as_ref().is_some()),
        ),
        (
            "bangumi",
            config.bangumi_enabled.unwrap_or(
                config.bangumi_username.as_ref().is_some()
                    || config.bangumi_access_token.as_ref().is_some(),
            ),
        ),
        (
            "x",
            config.x_enabled.unwrap_or(
                config.x_username.as_ref().is_some() && config.x_bearer_token.as_ref().is_some(),
            ),
        ),
        (
            "discord",
            config
                .discord_enabled
                .unwrap_or(config.discord_access_token.as_ref().is_some()),
        ),
        (
            "mal",
            config
                .mal_enabled
                .unwrap_or(config.mal_username.as_ref().is_some()),
        ),
        ("xbox", {
            let has_gamertag = config
                .xbox_gamertag
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("XBOX_GAMERTAG").is_ok();
            let has_key = config
                .openxbl_api_key
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("OPENXBL_API_KEY").is_ok()
                || std::env::var("XBL_API_KEY").is_ok();
            config.xbox_enabled.unwrap_or(has_gamertag && has_key)
        }),
        ("psn", {
            let has_id = config
                .psn_online_id
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("PSN_ONLINE_ID").is_ok();
            let has_npsso = config
                .psn_npsso
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("PSN_NPSSO").is_ok();
            config.psn_enabled.unwrap_or(has_id && has_npsso)
        }),
    ]
    .into_iter()
    .filter(|&(_, enabled)| enabled)
    .map(|(platform, _)| platform.to_string())
    .collect::<Vec<_>>();
    drop(config);

    // 2. 生成平台报告 (使用内部函数，避免序列化开销)
    let locale = super::locale::locale_from_headers(&headers);
    let (platform_reports, skipped) =
        generate_platform_reports_internal(&db, user_id, enabled_platforms, locale).await;
    let skipped_json: Vec<_> = skipped
        .iter()
        .map(|(platform, reason)| json!({ "platform": platform, "reason": reason }))
        .collect();
    if !skipped.is_empty() {
        tracing::warn!("⚠️ generate-all skipped platforms: {:?}", skipped);
    }

    Ok(Json(json!({
        "success": true,
        "platform_reports": platform_reports,
        "skipped": skipped_json,
    })))
}

/// 获取最新的平台报告
/// GET /api/reports/latest
/// Public home / report cards: always return the **site owner's** platform reports
/// (same authority as `/api/user`, library, activities). Do not switch to the
/// viewer's user id when a session cookie is present — logged-in guests would
/// otherwise get empty cards on the owner's dashboard.
/// 过期报告自动重生成的在途去重表（key: "user_id:platform"）
/// In-flight set for regen; `std::sync::Mutex` short critical section only (no await while held).
pub(crate) static REPORT_REGEN_IN_FLIGHT: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Resolve which user's reports the public latest/list endpoints should serve.
/// Prefers durable site owner (`is_owner`); falls back to the legacy owner id 1.
/// Viewer credentials never select public report ownership.
pub(crate) async fn public_report_owner_user_id(db: &DatabaseConnection) -> i32 {
    if let Ok(owner_id) = crate::api::profile::site_owner_user_id(db).await {
        return owner_id;
    }
    1
}

/// Prefer `preferred` when they have platform reports; otherwise use the user_id
/// that most recently wrote a non-`all` platform report.
///
/// Historical generations stored under actor claims (pre-#144) left site-owner
/// home cards empty even though reports exist under another admin id.
pub(crate) async fn resolve_report_user_id_for_public_read(
    db: &DatabaseConnection,
    preferred: i32,
) -> Result<i32, HttpError> {
    let preferred_count = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(preferred))
        .filter(platform_reports::Column::Platform.ne("all"))
        .count(db)
        .await
        .map_err(|e| {
            tracing::error!("count platform_reports for owner {}: {}", preferred, e);
            HttpError(AppError::internal("Database error"))
        })?;
    if preferred_count > 0 {
        return Ok(preferred);
    }

    let fallback = platform_reports::Entity::find()
        .filter(platform_reports::Column::Platform.ne("all"))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("fallback platform_reports lookup failed: {}", e);
            HttpError(AppError::internal("Database error"))
        })?;

    if let Some(row) = fallback {
        if row.user_id != preferred {
            tracing::warn!(
                preferred,
                fallback = row.user_id,
                "Site owner has no platform_reports; serving latest reports from user_id={}",
                row.user_id
            );
        }
        return Ok(row.user_id);
    }
    Ok(preferred)
}
