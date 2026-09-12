// Latest report listing and auto-regen of expired platform reports.

use axum::Json;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::models::entities::platform_reports;
use myriad_error::AppError;

use super::generate::{
    finalize_public_platform_report, generate_platform_reports_internal,
    public_report_owner_user_id, resolve_report_user_id_for_public_read, REPORT_REGEN_IN_FLIGHT,
};

/// 后台重新生成过期的平台报告（只用已有缓存数据调 AI，不重新抓平台）
fn spawn_report_auto_regen(db: DatabaseConnection, user_id: i32, platforms: Vec<String>) {
    let to_run: Vec<String> = {
        let mut in_flight = REPORT_REGEN_IN_FLIGHT.lock().unwrap();
        platforms
            .into_iter()
            .filter(|p| in_flight.insert(format!("{user_id}:{p}")))
            .collect()
    };
    if to_run.is_empty() {
        return;
    }

    tokio::spawn(async move {
        tracing::info!("♻️ Auto-regenerating expired reports: {:?}", to_run);
        let (_reports, skipped) =
            generate_platform_reports_internal(&db, user_id, to_run.clone(), None).await;
        if !skipped.is_empty() {
            tracing::warn!("♻️ Auto-regen skipped some platforms: {:?}", skipped);
        }
        let mut in_flight = REPORT_REGEN_IN_FLIGHT.lock().unwrap();
        for platform in to_run {
            in_flight.remove(&format!("{user_id}:{platform}"));
        }
    });
}

pub async fn get_latest_report(
    crate::extract::Db(db): crate::extract::Db,
) -> Result<Json<Value>, crate::error::HttpError> {
    // Public dashboard: site owner first; else the user_id that most recently wrote a non-`all` report.
    let preferred = public_report_owner_user_id(&db).await;
    let user_id = resolve_report_user_id_for_public_read(&db, preferred).await?;

    // 获取所有单平台报告，保留每个平台最新的一份
    let user_reports = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.ne("all"))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|e| {
            tracing::error!("Database error fetching user reports: {}", e);
            HttpError(AppError::internal("Database error"))
        })?;

    let settings = crate::api::config::load_report_settings(&db).await;
    let now = chrono::Utc::now().naive_utc();

    // 保留每个平台最新的一份报告；按过期设置过滤
    let mut seen = std::collections::HashSet::new();
    let mut platform_reports_list: Vec<Value> = Vec::new();
    let mut expired_platforms: Vec<String> = Vec::new();

    for r in user_reports {
        if !seen.insert(r.platform.clone()) {
            continue;
        }
        let expired_at = r.created_at + chrono::Duration::days(settings.expiry_days);
        let expired = settings.expiry_enabled && expired_at <= now;
        let body = finalize_public_platform_report(&r.platform, r.report);
        if !expired {
            platform_reports_list.push(body);
        } else if settings.auto_regenerate {
            // stale-while-revalidate：先返回旧报告，后台异步重新生成
            expired_platforms.push(r.platform.clone());
            platform_reports_list.push(body);
        }
        // 过期且未开自动重生成：直接隐藏
    }

    if !expired_platforms.is_empty() {
        spawn_report_auto_regen(db.clone(), user_id, expired_platforms);
    }

    // 如果没有任何平台报告
    if platform_reports_list.is_empty() {
        return Ok(Json(json!({
            "success": false,
            "message": "No valid report found",
            "code": "no_valid_report",
        })));
    }

    // Include user_id so clients/debug can verify which account fed home cards.
    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "platform_reports": platform_reports_list,
        "created_at": chrono::Utc::now().to_rfc3339()
    })))
}
