//! Start / stop / recent-activity handlers.
//!
//! Branch selection and list projection live in
//! [`crate::services::tapp_lifecycle`]. This module owns Claims, DB mutations,
//! activity upserts, and grant revocation.

use super::{
    current_is_admin, find_admin_user_id, get_admin_user_id, validate_tapp_id, ApiResponse,
};
use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, Set, Statement,
};
use serde::Deserialize;

use crate::middleware::auth::Claims;
use crate::models::entities::{tapp_user_activities, tapps};
use crate::services::tapp_lifecycle::{
    clamp_recent_limit, recent_tapp_item, resolve_start_outcome, resolve_stop_outcome,
    RecentTappItem, StartOutcome, StopOutcome,
};
use crate::services::tapp_ownership::public_install_visible_to_viewer;
use crate::error::HttpError;
use myriad_error::AppError;

/// 启动 Tapp
///
/// 权限模型（private-first，与 list/detail/runtime 一致）：
/// - 主体有同 `tapp_id` 的私有安装时：更新该私有行状态
/// - 否则站点主公开安装：管理员写库；非管理员只记活动（前端会话态）
/// - 普通用户可以启动自己临时安装的 Tapp
///
/// 所有用户启动 Tapp 时都会记录到 tapp_user_activities 表
pub(super) async fn start_tapp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let user_id: i32 = claims.sub.parse().map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))?;
    validate_tapp_id(&tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let admin_id = find_admin_user_id(&db).await?;
    let now = Utc::now().fixed_offset();
    let is_current_admin = current_is_admin(&claims, &db).await;

    let private_tapp = if admin_id != Some(user_id) {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?
    } else {
        None
    };

    let public_tapp = if let Some(admin_id) = admin_id {
        let row = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
        // Admin-only public installs are invisible to non-admins (same as catalog).
        row.filter(|tapp| {
            public_install_visible_to_viewer(&tapp.visibility, is_current_admin)
        })
    } else {
        None
    };

    let public_is_running = public_tapp
        .as_ref()
        .is_some_and(|tapp| matches!(tapp.status, tapps::TappStatus::Running));

    match resolve_start_outcome(
        private_tapp.is_some(),
        public_tapp.is_some(),
        is_current_admin,
        public_is_running,
    ) {
        StartOutcome::MutatePrivate => {
            let tapp = private_tapp.expect("has_private");
            // Refuse startup while the install needs re-authorization.
            // The frontend already blocks this; the backend gate is the server-side
            // half of the same fail-closed contract. Both start branches share the
            // pure decision below.
            refuse_marked_start(tapp.needs_reauthorization)?;
            let mut active: tapps::ActiveModel = tapp.into();
            active.status = Set(tapps::TappStatus::Running);
            active.last_run_at = Set(Some(now));
            active.updated_at = Set(now);
            active
                .update(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?;
            record_user_activity(&db, user_id, &tapp_id, now).await?;
            Ok(Json(ApiResponse::success(())))
        }
        StartOutcome::MutatePublic => {
            let tapp = public_tapp.expect("has_public");
            // Refuse startup while the install needs re-authorization.
            refuse_marked_start(tapp.needs_reauthorization)?;
            let mut active: tapps::ActiveModel = tapp.into();
            active.status = Set(tapps::TappStatus::Running);
            active.last_run_at = Set(Some(now));
            active.updated_at = Set(now);
            active
                .update(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?;
            record_user_activity(&db, user_id, &tapp_id, now).await?;
            Ok(Json(ApiResponse::success(())))
        }
        StartOutcome::RecordActivityOnly => {
            let tapp = public_tapp.expect("has_public");
            refuse_marked_start(tapp.needs_reauthorization)?;
            record_user_activity(&db, user_id, &tapp_id, now).await?;
            Ok(Json(ApiResponse::success(())))
        }
        StartOutcome::Forbidden => Err(HttpError(AppError::forbidden("Forbidden"))),
        StartOutcome::NotFound => Err(HttpError(AppError::not_found("Not found"))),
    }
}

/// 记录用户 Tapp 使用活动
///
/// 使用 upsert 模式：如果记录存在则更新 last_run_at 和 run_count，否则插入新记录
async fn record_user_activity(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), HttpError> {
    // One atomic upsert avoids duplicate-key failures when the same Tapp is
    // started concurrently from multiple tabs or backend replicas.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO tapp_user_activities
               (user_id, tapp_id, last_run_at, run_count)
           VALUES ($1, $2, $3, 1)
           ON CONFLICT (user_id, tapp_id) DO UPDATE SET
               last_run_at = EXCLUDED.last_run_at,
               run_count = tapp_user_activities.run_count + 1"#,
        vec![user_id.into(), tapp_id.into(), now.into()],
    ))
    .await
    .map_err(|_| HttpError(AppError::internal("Database error")))?;
    Ok(())
}

/// 重新授权 start 生命周期闸门的最小纯判定。
///
/// 安装仍标记为需重新授权时不得把 status 置为 Running，也不得把已在跑的
/// 公开安装记成一次成功 start。`MutatePrivate` / `MutatePublic` /
/// `RecordActivityOnly` 共用此判定，测试直接覆盖它本身（不复制逻辑）。
fn refuse_marked_start(needs_reauthorization: bool) -> Result<(), HttpError> {
    if needs_reauthorization {
        Err(HttpError(AppError::conflict(
            "Tapp requires permission re-authorization before it can be started",
        )))
    } else {
        Ok(())
    }
}

/// 停止 Tapp
///
/// 权限模型（private-first，与 list/detail/runtime 一致）：
/// - 主体有同 `tapp_id` 的私有安装时：更新该私有行状态并吊销 grant
/// - 否则站点主公开安装：管理员写库；非管理员只吊销自身 grant（不改公开行）
/// - 普通用户可以停止自己临时安装的 Tapp
pub(super) async fn stop_tapp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let user_id: i32 = claims.sub.parse().map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))?;
    validate_tapp_id(&tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let admin_id = find_admin_user_id(&db).await?;
    let is_current_admin = current_is_admin(&claims, &db).await;

    let private_tapp = if admin_id != Some(user_id) {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?
    } else {
        None
    };

    let public_tapp = if let Some(admin_id) = admin_id {
        let row = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
        row.filter(|tapp| {
            public_install_visible_to_viewer(&tapp.visibility, is_current_admin)
        })
    } else {
        None
    };

    match resolve_stop_outcome(
        private_tapp.is_some(),
        public_tapp.is_some(),
        is_current_admin,
    ) {
        StopOutcome::MutatePrivate => {
            let tapp = private_tapp.expect("has_private");
            let now = Utc::now().fixed_offset();
            let mut active: tapps::ActiveModel = tapp.into();
            active.status = Set(tapps::TappStatus::Installed);
            active.updated_at = Set(now);
            active
                .update(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?;
            crate::api::tapp_runtime::revoke_tapp_runtime_grants(&db, user_id, &tapp_id).await;
            Ok(Json(ApiResponse::success(())))
        }
        StopOutcome::MutatePublic => {
            let tapp = public_tapp.expect("has_public");
            let now = Utc::now().fixed_offset();
            let mut active: tapps::ActiveModel = tapp.into();
            active.status = Set(tapps::TappStatus::Installed);
            active.updated_at = Set(now);
            active
                .update(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?;
            crate::api::tapp_runtime::revoke_tapp_runtime_grants(&db, user_id, &tapp_id).await;
            Ok(Json(ApiResponse::success(())))
        }
        StopOutcome::RevokeOnly => {
            crate::api::tapp_runtime::revoke_tapp_runtime_grants(&db, user_id, &tapp_id).await;
            Ok(Json(ApiResponse::success(())))
        }
        StopOutcome::NotFound => Err(HttpError(AppError::not_found("Not found"))),
    }
}

/// 获取最近使用的 Tapp 查询参数
#[derive(Debug, Deserialize)]
pub(super) struct GetRecentTappsQuery {
    /// 返回的最大数量，默认 10
    #[serde(default = "default_recent_limit")]
    limit: i32,
}

fn default_recent_limit() -> i32 {
    10
}

/// 获取当前用户最近使用的 Tapp 列表
///
/// 从 tapp_user_activities 表中获取，按 last_run_at 降序排列
pub(super) async fn get_recent_tapps(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<GetRecentTappsQuery>,
) -> Result<Json<ApiResponse<Vec<RecentTappItem>>>, HttpError> {
    let user_id: i32 = claims.sub.parse().map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))?;
    let limit = clamp_recent_limit(query.limit);

    // 获取用户活动记录
    let activities = tapp_user_activities::Entity::find()
        .filter(tapp_user_activities::Column::UserId.eq(user_id))
        .order_by_desc(tapp_user_activities::Column::LastRunAt)
        .all(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    // Guests cannot start a Tapp and therefore have no activity rows. Return
    // an honest empty result without requiring a configured site owner.
    if activities.is_empty() {
        return Ok(Json(ApiResponse::success(Vec::new())));
    }

    let admin_id = get_admin_user_id(&db).await?;

    // 获取管理员的所有 Tapp（用于查找 Tapp 详情）
    let admin_tapps = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(admin_id))
        .all(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    // 获取用户自己的临时 Tapp
    let user_tapps = if user_id != admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .all(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?
    } else {
        Vec::new()
    };

    let is_admin = current_is_admin(&claims, &db).await;

    // 合并 Tapp 列表，建立 tapp_id -> tapp 映射（private wins for same id）
    let mut tapp_map: std::collections::HashMap<String, &tapps::Model> =
        std::collections::HashMap::new();
    for tapp in &admin_tapps {
        if !public_install_visible_to_viewer(&tapp.visibility, is_admin) {
            continue;
        }
        tapp_map.insert(tapp.tapp_id.clone(), tapp);
    }
    for tapp in &user_tapps {
        tapp_map.entry(tapp.tapp_id.clone()).or_insert(tapp);
    }

    // 构建响应
    let mut result: Vec<RecentTappItem> = Vec::new();
    for activity in activities {
        if result.len() >= limit {
            break;
        }

        if let Some(tapp) = tapp_map.get(&activity.tapp_id) {
            result.push(recent_tapp_item(
                &activity.tapp_id,
                &tapp.name,
                tapp.icon.clone(),
                tapp.theme_color.clone(),
                &tapp.manifest,
                activity.last_run_at.to_rfc3339(),
                activity.run_count,
            ));
        }
        // 如果 Tapp 已被卸载，跳过该记录
    }

    Ok(Json(ApiResponse::success(result)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_gate_refuses_marked_installs_with_conflict() {
        // L2: all three start success branches (MutatePrivate / MutatePublic /
        // RecordActivityOnly) call this shared pure decision; the test covers
        // the exact branch the handlers execute for a marked vs unmarked install.
        let err = refuse_marked_start(true).expect_err("marked install must be refused");
        assert_eq!(err.0.status_u16(), 409);
        assert!(err
            .0
            .to_string()
            .contains("permission re-authorization"));
        assert!(refuse_marked_start(false).is_ok(), "unmarked install starts");
    }
}
