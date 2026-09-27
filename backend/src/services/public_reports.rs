//! Whose reports the public sees: the site owner's, or whoever last wrote
//! platform reports when the owner has none. Viewer credentials never pick it.

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};

use crate::models::entities::platform_reports;

/// Public latest owner: `site_owner_user_id` (lowest admin) then user_id 1.
/// Viewer credentials never select public report ownership. Used by `get_latest_report` + catalog, not authed `/api/reports/list`.
pub(crate) async fn public_report_owner_user_id(db: &DatabaseConnection) -> i32 {
    if let Ok(owner_id) = crate::services::site_owner::site_owner_user_id(db).await {
        return owner_id;
    }
    1
}

/// Prefer `preferred` when they have platform reports; otherwise use the user_id
/// that most recently wrote a non-`all` platform report.
///
/// If `preferred` has no non-`all` platform reports, use the user_id that
/// most recently wrote one.
pub(crate) async fn owner_for_public_read(
    db: &DatabaseConnection,
    preferred: i32,
) -> Result<i32, sea_orm::DbErr> {
    let preferred_count = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(preferred))
        .filter(platform_reports::Column::Platform.ne("all"))
        .count(db)
        .await
        .inspect_err(|e| {
            tracing::error!("count platform_reports for owner {}: {}", preferred, e);
        })?;
    if preferred_count > 0 {
        return Ok(preferred);
    }

    let fallback = platform_reports::Entity::find()
        .filter(platform_reports::Column::Platform.ne("all"))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .one(db)
        .await
        .inspect_err(|e| {
            tracing::error!("fallback platform_reports lookup failed: {}", e);
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
