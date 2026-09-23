//! Phantasi reading stats.
use axum::{Json, extract::State};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::error::HttpError;
use crate::extract::OptionalViewer;

use super::helpers::{get_phantasi_viewer, phantasi_store_http};

// 统计信息

/// 获取统计信息（游客可访问）
/// 游客不计算已读/收藏统计以节约计算
pub(crate) async fn get_stats(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (user_id, is_admin) = get_phantasi_viewer(&viewer, &db).await?;

    let visible_sources = if is_admin {
        "phantasi_sources"
    } else {
        "phantasi_sources WHERE admin_only = FALSE"
    };
    let row = if let Some(uid) = user_id {
        let unread = if is_admin {
            "SELECT COUNT(*)::int FROM phantasi_items i \
             WHERE NOT EXISTS ( \
               SELECT 1 FROM phantasi_user_states s \
               WHERE s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
             )"
        } else {
            "SELECT COUNT(*)::int FROM phantasi_items i \
             INNER JOIN phantasi_sources src ON src.id = i.source_id AND src.admin_only = FALSE \
             WHERE NOT EXISTS ( \
               SELECT 1 FROM phantasi_user_states s \
               WHERE s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
             )"
        };
        let starred = if is_admin {
            "SELECT COUNT(*)::int FROM phantasi_user_states s \
             INNER JOIN phantasi_items i ON i.id = s.item_id \
             WHERE s.user_id = $1 AND s.is_starred = TRUE"
        } else {
            "SELECT 0"
        };
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT \
                    (SELECT COUNT(*)::int FROM {visible_sources}) AS total_sources, \
                    (SELECT COALESCE(SUM(item_count), 0)::int FROM {visible_sources}) AS total_items, \
                    ({unread}) AS unread_count, \
                    ({starred}) AS starred_count"
            ),
            [uid.into()],
        ))
        .await
        .map_err(|error| phantasi_store_http("count reading stats", error))?
        .ok_or_else(|| phantasi_store_http("count reading stats", "empty stats"))?
    } else {
        db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "SELECT \
                    (SELECT COUNT(*)::int FROM {visible_sources}) AS total_sources, \
                    (SELECT COALESCE(SUM(item_count), 0)::int FROM {visible_sources}) AS total_items, \
                    0::int AS unread_count, \
                    0::int AS starred_count"
            ),
        ))
        .await
        .map_err(|error| phantasi_store_http("count sources", error))?
        .ok_or_else(|| phantasi_store_http("count sources", "empty totals"))?
    };
    let total_sources: i32 = row
        .try_get("", "total_sources")
        .map_err(|error| phantasi_store_http("read source count", error))?;
    let total_items: i32 = row
        .try_get("", "total_items")
        .map_err(|error| phantasi_store_http("read item count", error))?;
    let total_unread: i32 = row
        .try_get("", "unread_count")
        .map_err(|error| phantasi_store_http("read unread count", error))?;
    let starred_count: i32 = row
        .try_get("", "starred_count")
        .map_err(|error| phantasi_store_http("read starred count", error))?;

    Ok(Json(json!({
        "success": true,
        "stats": {
            "total_sources": total_sources,
            "total_items": total_items,
            "total_unread": total_unread,
            "total_starred": starred_count,
        }
    })))
}

#[cfg(test)]
mod journal_audit_contracts {
    fn impl_fn<'a>(src: &'a str, name: &str) -> &'a str {
        let start = src
            .find(&format!("pub(crate) async fn {name}"))
            .expect(name);
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .or_else(|| body[1..].find("\n#[cfg(test)]"))
            .map(|index| index + 1)
            .unwrap_or(body.len());
        &body[..end]
    }

    #[test]
    fn get_stats_aggregates_without_materializing_item_ids() {
        let stats = impl_fn(include_str!("reading_stats.rs"), "get_stats");
        assert!(stats.contains("SUM(item_count)"));
        assert!(stats.contains("NOT EXISTS"));
        assert!(stats.contains("get_phantasi_viewer"));
        assert!(
            !stats.contains("phantasi_items::Entity::find()"),
            "get_stats must not load item rows just to count them"
        );
        assert!(
            !stats.contains(".all(&db)"),
            "get_stats must not collect item IDs"
        );
        assert!(
            stats.contains("src.admin_only = FALSE"),
            "unread count must join visible sources"
        );
        assert!(
            stats.contains("phantasi_store_http(\"count sources\"")
                || stats.contains("phantasi_store_http(\"count reading stats\""),
            "get_stats must not turn store failures into zeros"
        );
        assert!(
            !stats.contains("try_join!"),
            "totals/unread/starred must be one snapshot, not parallel round trips"
        );
        assert!(
            !stats.contains(".ok()\n        .flatten()"),
            "get_stats must not swallow query_one_raw errors"
        );
    }
}
