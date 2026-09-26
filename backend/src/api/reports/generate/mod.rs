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

use axum::{Extension, Json, extract::State, http::HeaderMap};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::platform_reports;
use crate::services::platform_id::PlatformId;
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
    /// Host UI locale at generation time (`zh-CN` / `zh-TW` / `en-US` / `ja-JP` / `ko-KR` / `fr-FR` / `de-DE`).
    #[serde(default)]
    pub locale: String,
}

/// User id under which public platform reports are stored.
/// Prefer durable site owner so public latest (owner-preferred, then latest
/// non-`all` writer) can find the rows.
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

/// 生成平台报告
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

    let actor_id = claims.durable_user_id().ok_or_else(|| {
        tracing::error!("Failed to parse user_id from subject");
        HttpError(AppError::unauthorized("Unauthorized"))
    })?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    let locale = match super::locale::locale_from_headers(&headers) {
        Some(tag) => Some(tag),
        None => super::locale::locale_from_user(&db, user_id).await,
    };
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
            "code": "no_platform_reports",
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

/// generate-all 的平台集合：[`PlatformId::enabled`]（显式开关，否则凭据是否齐备），
/// 与 Agent 接通判定、刷新闸门同源。
pub(crate) fn enabled_report_platforms(config: &DynamicConfig) -> Vec<String> {
    PlatformId::ALL
        .into_iter()
        .filter(|id| id.enabled(config))
        .map(|id| id.slug().to_string())
        .collect()
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
        .subject_id()
        .ok_or_else(|| HttpError(AppError::unauthorized("Unauthorized")))?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    // 1. 获取用户启用的所有平台（AppState.dynamic_config，与 GLOBAL_* 同 Arc）
    let config = dynamic_config.read().await;
    let enabled_platforms = enabled_report_platforms(&config);
    drop(config);

    // generate_platform_reports_internal (same persist path as single-platform).
    let locale = match super::locale::locale_from_headers(&headers) {
        Some(tag) => Some(tag),
        None => super::locale::locale_from_user(&db, user_id).await,
    };
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

/// Reports being generated right now, by `(user_id, platform)`. Every entry
/// point (manual, generate-all, auto-regeneration) goes through
/// [`ReportGeneration::claim`], so one report is never generated (and paid
/// for) twice at once. The Mutex is never held across an await.
static REPORT_GENERATIONS: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashSet<(i32, String)>>,
> = once_cell::sync::Lazy::new(Default::default);

/// Ownership of one in-flight report generation; released on drop, including
/// when the generating task panics or is cancelled.
pub(crate) struct ReportGeneration {
    key: (i32, String),
}

impl ReportGeneration {
    pub(crate) fn claim(user_id: i32, platform: &str) -> Option<Self> {
        let key = (user_id, platform.to_string());
        let claimed = REPORT_GENERATIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone());
        // Build the guard only on success and only after the lock is released:
        // a guard dropped on the failure path would release someone else's claim.
        claimed.then(|| Self { key })
    }
}

impl Drop for ReportGeneration {
    fn drop(&mut self) {
        REPORT_GENERATIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}

/// Public latest owner: `site_owner_user_id` (lowest admin) then user_id 1.
pub(crate) use crate::services::public_reports::public_report_owner_user_id;

/// [`crate::services::public_reports::owner_for_public_read`], as an HTTP error.
pub(crate) async fn resolve_report_user_id_for_public_read(
    db: &DatabaseConnection,
    preferred: i32,
) -> Result<i32, HttpError> {
    crate::services::public_reports::owner_for_public_read(db, preferred)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))
}

#[cfg(test)]
mod report_generation_tests {
    use super::ReportGeneration;

    #[test]
    fn one_generation_per_report_released_even_on_panic() {
        let user = -73_001;
        let first = ReportGeneration::claim(user, "steam").expect("free");
        assert!(ReportGeneration::claim(user, "steam").is_none());
        assert!(ReportGeneration::claim(user, "github").is_some());
        drop(first);

        let task = std::thread::spawn(move || {
            let _held = ReportGeneration::claim(user, "steam").expect("free again");
            panic!("generation failed");
        });
        assert!(task.join().is_err());
        assert!(ReportGeneration::claim(user, "steam").is_some());
    }

    #[test]
    fn every_entry_point_generates_through_the_claim() {
        let internal = include_str!("generate_internal.rs");
        assert!(internal.contains("ReportGeneration::claim(user_id, &platform)"));
        let auto = include_str!("../latest_and_list.rs");
        assert!(!auto.contains("IN_FLIGHT"));
    }
}
