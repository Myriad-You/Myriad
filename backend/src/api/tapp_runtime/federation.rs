//! Role-aware federation feed exposed only through a Tapp runtime grant.

use std::collections::HashSet;

use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, FixedOffset};
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use serde_json::{json, Value};

use crate::services::permission_service::TappPermission;

use super::RuntimeGrantContext;

const AP_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";
const FEED_LIMIT: usize = 100;

#[derive(Debug, FromQueryResult)]
struct FeedRow {
    activity_id: String,
    activity_type: Option<String>,
    object_type: Option<String>,
    content_preview: Option<String>,
    content_json: Option<Value>,
    received_at: DateTime<FixedOffset>,
    actor_url: Option<String>,
    username: Option<String>,
    domain: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
    scope: String,
    is_local: Option<bool>,
}

fn db_unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Federation feed is unavailable"})),
    )
}

fn feed_item(row: FeedRow) -> Value {
    let timestamp = row.received_at.to_rfc3339();
    json!({
        "activity_id": row.activity_id,
        "activity_type": row.activity_type,
        "object_type": row.object_type,
        "content_preview": row.content_preview,
        "content_json": row.content_json,
        "is_read": false,
        "created_at": timestamp,
        "received_at": timestamp,
        "scope": row.scope,
        "actor": {
            "actor_url": row.actor_url,
            "username": row.username,
            "domain": row.domain,
            "display_name": row.display_name,
            "avatar_url": row.avatar_url,
            "is_local": row.is_local.unwrap_or(false),
        },
    })
}

async fn load_personal_feed(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<Value>, (StatusCode, Json<Value>)> {
    let base_url = crate::federation::types::get_base_url().await;
    let rows = FeedRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"SELECT t.activity_id, t.activity_type, t.object_type,
                  t.content_preview, t.content_json, t.received_at,
                  COALESCE(ra.actor_url, CASE WHEN u.username IS NOT NULL THEN $2 || '/users/' || u.username ELSE NULL END) AS actor_url,
                  COALESCE(ra.username, u.username) AS username,
                  ra.domain,
                  COALESCE(ra.display_name, u.username) AS display_name,
                  ra.avatar_url,
                  'personal'::TEXT AS scope,
                  (ra.id IS NULL) AS is_local
           FROM federation_timeline t
           LEFT JOIN federation_remote_actors ra ON ra.id = t.remote_actor_id
           LEFT JOIN users u ON u.id = t.user_id
           WHERE t.user_id = $1
           ORDER BY t.received_at DESC
           LIMIT 100"#,
        [user_id.into(), base_url.trim_end_matches('/').into()],
    ))
    .all(db)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "Failed to load personal federation feed");
        db_unavailable()
    })?;

    Ok(rows.into_iter().map(feed_item).collect())
}

async fn load_public_feed(
    db: &DatabaseConnection,
) -> Result<Vec<Value>, (StatusCode, Json<Value>)> {
    let rows = FeedRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"WITH public_items AS (
               SELECT DISTINCT ON (a.activity_id)
                      a.activity_id,
                      a.activity_type,
                      a.object_type,
                      LEFT(COALESCE(
                          a.object_json #>> '{object,content}',
                          a.object_json #>> '{object,source,content}',
                          a.object_json ->> 'content',
                          a.object_json #>> '{object,summary}',
                          a.object_json ->> 'summary'
                      ), 200) AS content_preview,
                      COALESCE(
                          a.object_json -> 'object',
                          a.object_json
                      ) AS content_json,
                      COALESCE(a.received_at, a.published_at) AS received_at,
                      COALESCE(
                          a.object_json ->> 'actor',
                          a.object_json #>> '{object,attributedTo}',
                          a.object_json ->> 'attributedTo',
                          ra.actor_url
                      ) AS actor_url,
                      COALESCE(u.username, ra.username) AS username,
                      ra.domain,
                      COALESCE(u.display_name, ra.display_name, u.username, ra.username) AS display_name,
                      COALESCE(u.avatar_url, ra.avatar_url) AS avatar_url,
                      'public'::TEXT AS scope,
                      a.is_local AS is_local
               FROM federation_activities a
               LEFT JOIN federation_published_content pc ON pc.activity_id = a.activity_id
               LEFT JOIN users u ON u.id = a.user_id
               LEFT JOIN federation_remote_actors ra ON ra.id = a.remote_actor_id
               WHERE pc.visibility = 'public'
                  OR (
                      a.is_local = false
                      AND (
                          COALESCE((a.object_json -> 'to')::JSONB, '[]'::JSONB) ? $1
                          OR COALESCE((a.object_json -> 'cc')::JSONB, '[]'::JSONB) ? $1
                          OR COALESCE((a.object_json #> '{object,to}')::JSONB, '[]'::JSONB) ? $1
                          OR COALESCE((a.object_json #> '{object,cc}')::JSONB, '[]'::JSONB) ? $1
                      )
                  )
               ORDER BY a.activity_id, COALESCE(a.received_at, a.published_at) DESC
           )
           SELECT *
           FROM public_items
           ORDER BY received_at DESC
           LIMIT 100"#,
        [AP_PUBLIC.into()],
    ))
    .all(db)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "Failed to load public federation feed");
        db_unavailable()
    })?;

    Ok(rows.into_iter().map(feed_item).collect())
}

fn merge_feed(mut personal: Vec<Value>, public: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    personal.retain(|item| {
        item.get("activity_id")
            .and_then(Value::as_str)
            .is_some_and(|id| seen.insert(id.to_string()))
    });
    for item in public {
        let Some(id) = item.get("activity_id").and_then(Value::as_str) else {
            continue;
        };
        if seen.insert(id.to_string()) {
            personal.push(item);
        }
    }
    personal.sort_by(|left, right| {
        let left_time = left
            .get("received_at")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let right_time = right
            .get("received_at")
            .and_then(Value::as_str)
            .unwrap_or_default();
        right_time.cmp(left_time)
    });
    personal.truncate(FEED_LIMIT);
    personal
}

/// GET /api/tapp/federation/feed
///
/// Guests receive public activities only. Authenticated users receive their
/// personal federation timeline merged with the same public activities.
pub async fn get_federation_feed(
    State(db): State<DatabaseConnection>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::FederationRead)?;

    let public = load_public_feed(&db).await?;
    let is_guest = runtime_grant.subject_id() < 0;
    let personal = if is_guest {
        Vec::new()
    } else {
        load_personal_feed(&db, runtime_grant.subject_id()).await?
    };
    let items = merge_feed(personal, public);
    let total = items.len();

    Ok(Json(json!({
        "items": items,
        "total": total,
        "audience": if is_guest { "public" } else { "public+personal" },
    })))
}

#[cfg(test)]
mod tests {
    use super::merge_feed;
    use serde_json::json;

    #[test]
    fn personal_copy_wins_when_public_feed_contains_same_activity() {
        let merged = merge_feed(
            vec![json!({
                "activity_id": "same",
                "received_at": "2026-07-15T10:00:00+00:00",
                "scope": "personal"
            })],
            vec![json!({
                "activity_id": "same",
                "received_at": "2026-07-15T10:00:00+00:00",
                "scope": "public"
            })],
        );

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0]["scope"], "personal");
    }

    #[test]
    fn merged_feed_is_newest_first() {
        let merged = merge_feed(
            vec![json!({
                "activity_id": "older",
                "received_at": "2026-07-14T10:00:00+00:00"
            })],
            vec![json!({
                "activity_id": "newer",
                "received_at": "2026-07-15T10:00:00+00:00"
            })],
        );

        assert_eq!(merged[0]["activity_id"], "newer");
    }
}
