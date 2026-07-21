//! Object interactions: Like, Bookmark, Announce (repost), and helpers for
//! timeline enrichment / inbound AP handling.
//!
//! Storage: `federation_object_interactions` (local user actions).
//! Counts combine local interactions with remote Like/Announce activities.

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::federation::content;
use crate::federation::types::*;

// ==================== Request / response ====================

#[derive(Debug, Deserialize)]
pub struct ObjectIdRequest {
    /// Canonical AP object id / URL (e.g. Note id).
    pub object_id: String,
}

/// Quote-repost (announce) body. Commentary is required (non-empty after trim).
#[derive(Debug, Deserialize)]
pub struct AnnounceRequest {
    /// Canonical AP object id / URL being quote-reposted.
    pub object_id: String,
    /// Required user commentary for the quote-repost.
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InteractionResponse {
    pub success: bool,
    pub object_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    #[serde(default)]
    pub liked_by_me: Option<bool>,
    #[serde(default)]
    pub bookmarked_by_me: Option<bool>,
    #[serde(default)]
    pub announced_by_me: Option<bool>,
    #[serde(default)]
    pub like_count: Option<i64>,
    #[serde(default)]
    pub bookmark_count: Option<i64>,
    #[serde(default)]
    pub announce_count: Option<i64>,
    #[serde(default)]
    pub reply_count: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct BookmarkListResponse {
    pub items: Vec<serde_json::Value>,
    pub total: usize,
}

// ==================== Helpers ====================

fn db_err(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("[interactions] DB error: {}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": format!("Database error: {}", e)})),
    )
}

fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": msg})))
}

/// Extract a canonical object URL/id from an AP object, Create envelope, or string.
pub fn extract_object_id(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
        let t = id.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    // Create / Update envelope
    if let Some(obj) = value.get("object") {
        return extract_object_id(obj);
    }
    None
}

/// Normalize object_id from request body.
fn require_object_id(raw: &str) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let id = raw.trim();
    if id.is_empty() {
        return Err(bad_request("object_id required"));
    }
    if id.len() > 2048 {
        return Err(bad_request("object_id too long"));
    }
    Ok(id.to_string())
}

/// Resolve an AP object (Note/Article) from local DB for repost/reply previews.
async fn resolve_local_object(
    db: &DatabaseConnection,
    object_id: &str,
) -> Option<serde_json::Value> {
    // Prefer Create activity object
    if let Ok(Some(row)) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT object_json FROM federation_activities
               WHERE activity_type = 'Create'
                 AND (
                   object_json #>> '{object,id}' = $1
                   OR object_json ->> 'id' = $1
                 )
               ORDER BY published_at DESC NULLS LAST
               LIMIT 1"#,
            [object_id.into()],
        ))
        .await
    {
        if let Ok(Some(v)) = row.try_get::<Option<serde_json::Value>>("", "object_json") {
            if let Some(obj) = v.get("object").cloned() {
                if obj.is_object() {
                    return Some(obj);
                }
            }
            if v.get("type").and_then(|t| t.as_str()).is_some() && v.get("id").is_some() {
                return Some(v);
            }
        }
    }

    // Fallback: any timeline row
    if let Ok(Some(row)) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT content_json FROM federation_timeline
               WHERE content_json->>'id' = $1
                  OR content_json #>> '{object,id}' = $1
               ORDER BY received_at DESC
               LIMIT 1"#,
            [object_id.into()],
        ))
        .await
    {
        if let Ok(Some(v)) = row.try_get::<Option<serde_json::Value>>("", "content_json") {
            if let Some(obj) = v.get("object").cloned() {
                if obj.is_object()
                    && (obj.get("content").is_some()
                        || obj.get("source").is_some()
                        || obj.get("summary").is_some())
                {
                    return Some(obj);
                }
            }
            if v.is_object() {
                return Some(v);
            }
        }
    }

    None
}

/// attributedTo / actor URL for an object (local resolution).
async fn resolve_object_author(
    db: &DatabaseConnection,
    object_id: &str,
) -> Option<String> {
    if let Some(obj) = resolve_local_object(db, object_id).await {
        if let Some(a) = obj
            .get("attributedTo")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                obj.get("attributedTo")
                    .and_then(|v| v.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        {
            return Some(a);
        }
    }
    None
}

// ==================== Counts / state ====================

/// Batch interaction stats for a list of object ids (for timeline enrichment).
pub async fn interaction_stats_for_objects(
    db: &DatabaseConnection,
    user_id: i32,
    object_ids: &[String],
) -> Result<std::collections::HashMap<String, InteractionStats>, String> {
    use std::collections::HashMap;
    let mut map: HashMap<String, InteractionStats> = HashMap::new();
    if object_ids.is_empty() {
        return Ok(map);
    }

    // Initialize zeros
    for oid in object_ids {
        map.insert(oid.clone(), InteractionStats::default());
    }

    // Local interactions (counts + me flags)
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT object_id, kind, user_id
               FROM federation_object_interactions
               WHERE object_id = ANY($1)"#,
            [object_ids.to_vec().into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    for r in rows {
        let oid: String = r.try_get("", "object_id").unwrap_or_default();
        let kind: String = r.try_get("", "kind").unwrap_or_default();
        let uid: i32 = r.try_get("", "user_id").unwrap_or(0);
        let Some(stats) = map.get_mut(&oid) else {
            continue;
        };
        match kind.as_str() {
            "like" => {
                stats.like_count += 1;
                if uid == user_id {
                    stats.liked_by_me = true;
                }
            }
            "bookmark" => {
                stats.bookmark_count += 1;
                if uid == user_id {
                    stats.bookmarked_by_me = true;
                }
            }
            "announce" => {
                stats.announce_count += 1;
                if uid == user_id {
                    stats.announced_by_me = true;
                }
            }
            _ => {}
        }
    }

    // Remote likes (is_local = false) — avoid double-counting local interactions
    let remote_likes = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT
                 CASE
                   WHEN jsonb_typeof(object_json::jsonb) = 'string'
                     THEN trim(both '"' from object_json::text)
                   ELSE COALESCE(
                     object_json::jsonb ->> 'id',
                     object_json::jsonb #>> '{object,id}'
                   )
                 END AS object_id,
                 COUNT(*)::bigint AS cnt
               FROM federation_activities
               WHERE activity_type = 'Like'
                 AND is_local = false
                 AND (
                   (jsonb_typeof(object_json::jsonb) = 'string'
                      AND trim(both '"' from object_json::text) = ANY($1))
                   OR (object_json::jsonb ->> 'id' = ANY($1))
                   OR (object_json::jsonb #>> '{object,id}' = ANY($1))
                 )
               GROUP BY 1"#,
            [object_ids.to_vec().into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    for r in remote_likes {
        let oid: String = r.try_get("", "object_id").unwrap_or_default();
        let cnt: i64 = r.try_get("", "cnt").unwrap_or(0);
        if let Some(stats) = map.get_mut(&oid) {
            stats.like_count += cnt;
        }
    }

    // Remote announces
    let remote_ann = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT
                 CASE
                   WHEN jsonb_typeof(object_json::jsonb) = 'string'
                     THEN trim(both '"' from object_json::text)
                   ELSE COALESCE(
                     object_json::jsonb ->> 'id',
                     object_json::jsonb #>> '{object,id}'
                   )
                 END AS object_id,
                 COUNT(*)::bigint AS cnt
               FROM federation_activities
               WHERE activity_type = 'Announce'
                 AND is_local = false
                 AND (
                   (jsonb_typeof(object_json::jsonb) = 'string'
                      AND trim(both '"' from object_json::text) = ANY($1))
                   OR (object_json::jsonb ->> 'id' = ANY($1))
                   OR (object_json::jsonb #>> '{object,id}' = ANY($1))
                 )
               GROUP BY 1"#,
            [object_ids.to_vec().into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    for r in remote_ann {
        let oid: String = r.try_get("", "object_id").unwrap_or_default();
        let cnt: i64 = r.try_get("", "cnt").unwrap_or(0);
        if let Some(stats) = map.get_mut(&oid) {
            stats.announce_count += cnt;
        }
    }

    // Reply counts (Create with inReplyTo). Exclude quote-reposts (mfp:kind=repost).
    let replies = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT
                 COALESCE(
                   object_json::jsonb #>> '{object,inReplyTo}',
                   object_json::jsonb ->> 'inReplyTo'
                 ) AS parent_id,
                 COUNT(*)::bigint AS cnt
               FROM federation_activities
               WHERE activity_type = 'Create'
                 AND COALESCE(object_json::jsonb #>> '{object,mfp:kind}', '') <> 'repost'
                 AND COALESCE(object_json::jsonb ->> 'mfp:kind', '') <> 'repost'
                 AND (
                   object_json::jsonb #>> '{object,inReplyTo}' = ANY($1)
                   OR object_json::jsonb ->> 'inReplyTo' = ANY($1)
                 )
               GROUP BY 1"#,
            [object_ids.to_vec().into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    for r in replies {
        let oid: String = r.try_get("", "parent_id").unwrap_or_default();
        let cnt: i64 = r.try_get("", "cnt").unwrap_or(0);
        if let Some(stats) = map.get_mut(&oid) {
            stats.reply_count += cnt;
        }
    }

    Ok(map)
}

#[derive(Debug, Default, Clone)]
pub struct InteractionStats {
    pub liked_by_me: bool,
    pub bookmarked_by_me: bool,
    pub announced_by_me: bool,
    pub like_count: i64,
    pub bookmark_count: i64,
    pub announce_count: i64,
    pub reply_count: i64,
}

async fn stats_for_one(
    db: &DatabaseConnection,
    user_id: i32,
    object_id: &str,
) -> InteractionStats {
    interaction_stats_for_objects(db, user_id, &[object_id.to_string()])
        .await
        .ok()
        .and_then(|m| m.get(object_id).cloned())
        .unwrap_or_default()
}

// ==================== Like ====================

/// POST /api/federation/like
pub async fn like_object(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    object_id_raw: &str,
) -> Result<InteractionResponse, (StatusCode, Json<serde_json::Value>)> {
    let object_id = require_object_id(object_id_raw)?;
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);

    // Idempotent insert
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_object_interactions
                   (user_id, object_id, kind, activity_id, created_at)
               VALUES ($1, $2, 'like', $3, NOW())
               ON CONFLICT (user_id, object_id, kind) DO NOTHING"#,
            [
                user_id.into(),
                object_id.clone().into(),
                activity_id.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    if inserted.rows_affected() == 0 {
        // Already liked — return current state without new activity
        let st = stats_for_one(db, user_id, &object_id).await;
        return Ok(InteractionResponse {
            success: true,
            object_id,
            kind: "like".into(),
            activity_id: None,
            liked_by_me: Some(true),
            bookmarked_by_me: Some(st.bookmarked_by_me),
            announced_by_me: Some(st.announced_by_me),
            like_count: Some(st.like_count),
            bookmark_count: Some(st.bookmark_count),
            announce_count: Some(st.announce_count),
            reply_count: Some(st.reply_count),
        });
    }

    let like_json = json!({
        "@context": build_ap_context(),
        "type": "Like",
        "id": &activity_id,
        "actor": &local_actor,
        "object": &object_id,
        "published": now_iso8601(),
        "to": [AP_PUBLIC],
    });

    let act_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Like', 'Note', $3, true, NOW())
               RETURNING id"#,
            [
                activity_id.clone().into(),
                user_id.into(),
                like_json.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_db_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    // Deliver Like to object author + followers (best-effort)
    if act_db_id > 0 {
        deliver_like_or_announce(db, user_id, act_db_id, &like_json, &object_id).await;
    }

    let st = stats_for_one(db, user_id, &object_id).await;
    Ok(InteractionResponse {
        success: true,
        object_id,
        kind: "like".into(),
        activity_id: Some(activity_id),
        liked_by_me: Some(true),
        bookmarked_by_me: Some(st.bookmarked_by_me),
        announced_by_me: Some(st.announced_by_me),
        like_count: Some(st.like_count),
        bookmark_count: Some(st.bookmark_count),
        announce_count: Some(st.announce_count),
        reply_count: Some(st.reply_count),
    })
}

/// DELETE /api/federation/like
pub async fn unlike_object(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    object_id_raw: &str,
) -> Result<InteractionResponse, (StatusCode, Json<serde_json::Value>)> {
    let object_id = require_object_id(object_id_raw)?;
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT activity_id FROM federation_object_interactions
               WHERE user_id = $1 AND object_id = $2 AND kind = 'like'"#,
            [user_id.into(), object_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    let original_like_id: Option<String> = row
        .and_then(|r| r.try_get::<Option<String>>("", "activity_id").ok().flatten());

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"DELETE FROM federation_object_interactions
           WHERE user_id = $1 AND object_id = $2 AND kind = 'like'"#,
        [user_id.into(), object_id.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    if let Some(like_id) = original_like_id.filter(|s| !s.is_empty()) {
        let undo_id = generate_activity_id(&base_url);
        let undo_json = json!({
            "@context": build_ap_context(),
            "type": "Undo",
            "id": &undo_id,
            "actor": &local_actor,
            "object": {
                "type": "Like",
                "id": &like_id,
                "actor": &local_actor,
                "object": &object_id,
            },
            "published": now_iso8601(),
        });

        if let Ok(Some(act_row)) = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                       (activity_id, user_id, activity_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'Undo', $3, true, NOW())
                   RETURNING id"#,
                [
                    undo_id.clone().into(),
                    user_id.into(),
                    undo_json.clone().into(),
                ],
            ))
            .await
        {
            let act_db_id: i32 = act_row.try_get("", "id").unwrap_or(0);
            if act_db_id > 0 {
                let _ = content::fan_out_to_followers(db, user_id, act_db_id, &undo_json).await;
                deliver_to_object_author(db, act_db_id, &undo_json, &object_id).await;
            }
        }
    }

    let st = stats_for_one(db, user_id, &object_id).await;
    Ok(InteractionResponse {
        success: true,
        object_id,
        kind: "like".into(),
        activity_id: None,
        liked_by_me: Some(false),
        bookmarked_by_me: Some(st.bookmarked_by_me),
        announced_by_me: Some(st.announced_by_me),
        like_count: Some(st.like_count),
        bookmark_count: Some(st.bookmark_count),
        announce_count: Some(st.announce_count),
        reply_count: Some(st.reply_count),
    })
}

// ==================== Bookmark (local-first) ====================

/// POST /api/federation/bookmark
pub async fn bookmark_object(
    user_id: i32,
    db: &DatabaseConnection,
    object_id_raw: &str,
) -> Result<InteractionResponse, (StatusCode, Json<serde_json::Value>)> {
    let object_id = require_object_id(object_id_raw)?;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_object_interactions
               (user_id, object_id, kind, created_at)
           VALUES ($1, $2, 'bookmark', NOW())
           ON CONFLICT (user_id, object_id, kind) DO NOTHING"#,
        [user_id.into(), object_id.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    // Keep timeline flag in sync when a matching row exists
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_timeline
               SET is_bookmarked = true
               WHERE user_id = $1
                 AND (
                   content_json->>'id' = $2
                   OR content_json #>> '{object,id}' = $2
                 )"#,
            [user_id.into(), object_id.clone().into()],
        ))
        .await;

    let st = stats_for_one(db, user_id, &object_id).await;
    Ok(InteractionResponse {
        success: true,
        object_id,
        kind: "bookmark".into(),
        activity_id: None,
        liked_by_me: Some(st.liked_by_me),
        bookmarked_by_me: Some(true),
        announced_by_me: Some(st.announced_by_me),
        like_count: Some(st.like_count),
        bookmark_count: Some(st.bookmark_count),
        announce_count: Some(st.announce_count),
        reply_count: Some(st.reply_count),
    })
}

/// DELETE /api/federation/bookmark
pub async fn unbookmark_object(
    user_id: i32,
    db: &DatabaseConnection,
    object_id_raw: &str,
) -> Result<InteractionResponse, (StatusCode, Json<serde_json::Value>)> {
    let object_id = require_object_id(object_id_raw)?;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"DELETE FROM federation_object_interactions
           WHERE user_id = $1 AND object_id = $2 AND kind = 'bookmark'"#,
        [user_id.into(), object_id.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_timeline
               SET is_bookmarked = false
               WHERE user_id = $1
                 AND (
                   content_json->>'id' = $2
                   OR content_json #>> '{object,id}' = $2
                 )"#,
            [user_id.into(), object_id.clone().into()],
        ))
        .await;

    let st = stats_for_one(db, user_id, &object_id).await;
    Ok(InteractionResponse {
        success: true,
        object_id,
        kind: "bookmark".into(),
        activity_id: None,
        liked_by_me: Some(st.liked_by_me),
        bookmarked_by_me: Some(false),
        announced_by_me: Some(st.announced_by_me),
        like_count: Some(st.like_count),
        bookmark_count: Some(st.bookmark_count),
        announce_count: Some(st.announce_count),
        reply_count: Some(st.reply_count),
    })
}

/// GET /api/federation/bookmarks
pub async fn list_bookmarks(
    user_id: i32,
    db: &DatabaseConnection,
) -> Result<BookmarkListResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let base = base_url.trim_end_matches('/');
    let local_domain = extract_domain(&base_url).unwrap_or_default();

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            // content_json columns are `json`; activity object_json extracts are `jsonb`.
            // COALESCE requires matching types — cast the timeline branch to jsonb.
            r#"SELECT i.object_id, i.created_at,
                      COALESCE(
                        (
                          SELECT t.content_json::jsonb FROM federation_timeline t
                          WHERE t.user_id = i.user_id
                            AND (
                              t.content_json->>'id' = i.object_id
                              OR t.content_json #>> '{object,id}' = i.object_id
                            )
                          ORDER BY t.received_at DESC LIMIT 1
                        ),
                        (
                          SELECT CASE
                                   WHEN a.object_json::jsonb ? 'object'
                                        AND jsonb_typeof(a.object_json::jsonb->'object') = 'object'
                                     THEN a.object_json::jsonb->'object'
                                   ELSE a.object_json::jsonb
                                 END
                          FROM federation_activities a
                          WHERE a.activity_type = 'Create'
                            AND (
                              a.object_json #>> '{object,id}' = i.object_id
                              OR a.object_json ->> 'id' = i.object_id
                            )
                          ORDER BY a.published_at DESC NULLS LAST LIMIT 1
                        )
                      ) AS content_json,
                      (
                        SELECT t.activity_id FROM federation_timeline t
                        WHERE t.user_id = i.user_id
                          AND (
                            t.content_json->>'id' = i.object_id
                            OR t.content_json #>> '{object,id}' = i.object_id
                          )
                        ORDER BY t.received_at DESC LIMIT 1
                      ) AS activity_id,
                      (
                        SELECT t.activity_type FROM federation_timeline t
                        WHERE t.user_id = i.user_id
                          AND (
                            t.content_json->>'id' = i.object_id
                            OR t.content_json #>> '{object,id}' = i.object_id
                          )
                        ORDER BY t.received_at DESC LIMIT 1
                      ) AS activity_type,
                      (
                        SELECT t.object_type FROM federation_timeline t
                        WHERE t.user_id = i.user_id
                          AND (
                            t.content_json->>'id' = i.object_id
                            OR t.content_json #>> '{object,id}' = i.object_id
                          )
                        ORDER BY t.received_at DESC LIMIT 1
                      ) AS object_type,
                      (
                        SELECT t.content_preview FROM federation_timeline t
                        WHERE t.user_id = i.user_id
                          AND (
                            t.content_json->>'id' = i.object_id
                            OR t.content_json #>> '{object,id}' = i.object_id
                          )
                        ORDER BY t.received_at DESC LIMIT 1
                      ) AS content_preview
               FROM federation_object_interactions i
               WHERE i.user_id = $1 AND i.kind = 'bookmark'
               ORDER BY i.created_at DESC
               LIMIT 100"#,
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let mut items = Vec::new();
    let mut object_ids = Vec::new();
    for r in &rows {
        let object_id: String = r.try_get("", "object_id").unwrap_or_default();
        object_ids.push(object_id.clone());
        let created_at = r
            .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
            .ok()
            .map(|t| t.to_rfc3339());
        let content_json = r
            .try_get::<Option<serde_json::Value>>("", "content_json")
            .ok()
            .flatten();
        let preview = r
            .try_get::<Option<String>>("", "content_preview")
            .ok()
            .flatten()
            .or_else(|| {
                content_json.as_ref().and_then(|cj| {
                    cj.pointer("/source/content")
                        .and_then(|v| v.as_str())
                        .or_else(|| cj.get("content").and_then(|v| v.as_str()))
                        .or_else(|| cj.get("summary").and_then(|v| v.as_str()))
                        .map(|s| s.chars().take(200).collect())
                })
            });

        // Best-effort actor from attributedTo
        let attributed = content_json
            .as_ref()
            .and_then(|cj| {
                cj.get("attributedTo")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        let actor = if !attributed.is_empty() {
            let uname = attributed
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let domain = extract_domain(&attributed).unwrap_or_else(|| local_domain.clone());
            json!({
                "actor_url": attributed,
                "username": uname,
                "domain": domain,
                "display_name": uname,
            })
        } else {
            json!({
                "actor_url": "",
                "username": "",
                "domain": local_domain,
            })
        };

        items.push(json!({
            "object_id": object_id,
            "activity_id": r.try_get::<Option<String>>("", "activity_id").ok().flatten()
                .unwrap_or_else(|| format!("bookmark:{}", object_ids.last().unwrap_or(&String::new()))),
            "activity_type": r.try_get::<Option<String>>("", "activity_type").ok().flatten()
                .or(Some("Create".into())),
            "object_type": r.try_get::<Option<String>>("", "object_type").ok().flatten()
                .or(Some("Note".into())),
            "content_preview": preview,
            "content_json": content_json,
            "is_read": true,
            "is_bookmarked": true,
            "bookmarked_by_me": true,
            "created_at": created_at.clone(),
            "received_at": created_at,
            "actor": actor,
            "base_hint": base,
        }));
    }

    // Enrich counts
    if let Ok(stats_map) = interaction_stats_for_objects(db, user_id, &object_ids).await {
        for item in &mut items {
            if let Some(oid) = item.get("object_id").and_then(|v| v.as_str()) {
                if let Some(st) = stats_map.get(oid) {
                    if let Some(obj) = item.as_object_mut() {
                        obj.insert("liked_by_me".into(), json!(st.liked_by_me));
                        obj.insert("bookmarked_by_me".into(), json!(true));
                        obj.insert("announced_by_me".into(), json!(st.announced_by_me));
                        obj.insert("like_count".into(), json!(st.like_count));
                        obj.insert("bookmark_count".into(), json!(st.bookmark_count));
                        obj.insert("announce_count".into(), json!(st.announce_count));
                        obj.insert("reply_count".into(), json!(st.reply_count));
                    }
                }
            }
        }
    }

    let total = items.len();
    Ok(BookmarkListResponse { items, total })
}

// ==================== Announce (quote-repost) ====================

fn escape_html_lite(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Max nested quote depth embedded into a repost card (root + chain).
const MAX_QUOTE_NEST_DEPTH: usize = 3;

fn plain_preview_from_object(obj: &serde_json::Value) -> String {
    obj.pointer("/source/content")
        .and_then(|v| v.as_str())
        .or_else(|| obj.get("content_preview").and_then(|v| v.as_str()))
        .or_else(|| obj.get("content").and_then(|v| v.as_str()))
        .or_else(|| obj.get("summary").and_then(|v| v.as_str()))
        .or_else(|| obj.get("name").and_then(|v| v.as_str()))
        .map(|s| {
            let plain = s
                .replace("<br>", " ")
                .replace("<br/>", " ")
                .replace("<br />", " ");
            let mut out = String::new();
            let mut in_tag = false;
            for c in plain.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => out.push(c),
                    _ => {}
                }
            }
            out.chars().take(200).collect::<String>()
        })
        .unwrap_or_default()
}

fn is_repost_object(obj: &serde_json::Value) -> bool {
    obj.get("mfp:kind")
        .and_then(|v| v.as_str())
        .map(|s| s == "repost")
        .unwrap_or(false)
        || obj
            .get("mfp:contentType")
            .and_then(|v| v.as_str())
            .map(|s| s == "repost")
            .unwrap_or(false)
}

/// Slim preview of the quoted object for local timeline / UI cards.
///
/// Nested quote-reposts are preserved up to [`MAX_QUOTE_NEST_DEPTH`] levels so
/// re-quoting a repost embeds the chain (each level's commentary + author),
/// not only a bare id pointer to the intermediate post.
fn slim_quoted_object(obj: &serde_json::Value) -> serde_json::Value {
    slim_quoted_object_depth(obj, 0)
}

fn slim_quoted_object_depth(obj: &serde_json::Value, depth: usize) -> serde_json::Value {
    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let attributed = obj
        .get("attributedTo")
        .cloned()
        .unwrap_or(json!(null));
    let preview = plain_preview_from_object(obj);
    let kind_repost = is_repost_object(obj);

    let mut slim = json!({
        "id": id,
        "type": obj.get("type").cloned().unwrap_or(json!("Note")),
        "attributedTo": attributed,
        "content_preview": preview,
        "name": obj.get("name").cloned().unwrap_or(json!(null)),
        "summary": obj.get("summary").cloned().unwrap_or(json!(null)),
    });

    if kind_repost {
        slim["mfp:kind"] = json!("repost");
        slim["mfp:contentType"] = json!("repost");
    }

    // Preserve existing nested quote chain (depth-limited).
    if depth < MAX_QUOTE_NEST_DEPTH {
        if let Some(inner) = obj
            .get("mfp:quotedObject")
            .filter(|v| v.is_object())
            .cloned()
            .or_else(|| {
                obj.get("mfp:quotedObject")
                    .and_then(|v| v.as_object())
                    .map(|o| json!(o))
            })
        {
            slim["mfp:quotedObject"] = slim_quoted_object_depth(&inner, depth + 1);
        } else if let Some(inner_id) = obj
            .get("mfp:quotedObjectId")
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("quoteUrl").and_then(|v| v.as_str()))
            .or_else(|| obj.get("inReplyTo").and_then(|v| v.as_str()))
            .filter(|s| !s.is_empty())
        {
            // Intermediate repost without embedded body — keep id so UI can label it.
            slim["mfp:quotedObjectId"] = json!(inner_id);
        }
        if let Some(qid) = obj.get("mfp:quotedObjectId").and_then(|v| v.as_str()) {
            slim["mfp:quotedObjectId"] = json!(qid);
        }
        if let Some(root) = obj.get("mfp:rootQuotedObjectId").and_then(|v| v.as_str()) {
            slim["mfp:rootQuotedObjectId"] = json!(root);
        }
    } else {
        // Cap: point at deepest known root id without further expansion.
        if let Some(root) = obj
            .get("mfp:rootQuotedObjectId")
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("mfp:quotedObjectId").and_then(|v| v.as_str()))
            .or_else(|| obj.get("quoteUrl").and_then(|v| v.as_str()))
        {
            slim["mfp:rootQuotedObjectId"] = json!(root);
            slim["mfp:quoteTruncated"] = json!(true);
        }
    }

    slim
}

/// Resolve root original id when quoting a (possibly nested) repost.
fn root_quoted_object_id(obj: Option<&serde_json::Value>, fallback_id: &str) -> String {
    let Some(obj) = obj else {
        return fallback_id.to_string();
    };
    if let Some(root) = obj.get("mfp:rootQuotedObjectId").and_then(|v| v.as_str()) {
        if !root.is_empty() {
            return root.to_string();
        }
    }
    // Walk embedded chain to the innermost id.
    let mut cur = obj;
    let mut guard = 0;
    while guard < MAX_QUOTE_NEST_DEPTH + 2 {
        if let Some(inner) = cur.get("mfp:quotedObject").filter(|v| v.is_object()) {
            cur = inner;
            guard += 1;
            continue;
        }
        break;
    }
    if is_repost_object(obj) {
        if let Some(qid) = cur
            .get("mfp:quotedObjectId")
            .and_then(|v| v.as_str())
            .or_else(|| cur.get("id").and_then(|v| v.as_str()))
            .filter(|s| !s.is_empty())
        {
            // If we only have the intermediate, prefer explicit quotedObjectId on outer.
            if let Some(outer_q) = obj.get("mfp:quotedObjectId").and_then(|v| v.as_str()) {
                if cur.get("mfp:quotedObject").is_none() {
                    return outer_q.to_string();
                }
            }
            return qid.to_string();
        }
    }
    obj.get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_id)
        .to_string()
}

/// POST /api/federation/announce
///
/// Quote-repost: requires non-empty `content` commentary. Stores a Create Note
/// with `inReplyTo` / `quoteUrl` pointing at the original object, fans out as
/// Create, records `kind=announce` for local counts, and inserts into
/// `federation_published_content` so reposts appear under 已发布.
pub async fn announce_object(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    object_id_raw: &str,
    content_raw: &str,
) -> Result<InteractionResponse, (StatusCode, Json<serde_json::Value>)> {
    let object_id = require_object_id(object_id_raw)?;
    let content = content_raw.trim();
    if content.is_empty() {
        return Err(bad_request("content required for repost"));
    }
    if content.chars().count() > 10_000 {
        return Err(bad_request("content too long (max 10000 chars)"));
    }

    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);
    let note_content_id = format!("repost_{}", uuid::Uuid::new_v4());
    let note_id = format!(
        "{}/notes/{}",
        base_url.trim_end_matches('/'),
        note_content_id
    );

    let inserted = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_object_interactions
                   (user_id, object_id, kind, activity_id, created_at)
               VALUES ($1, $2, 'announce', $3, NOW())
               ON CONFLICT (user_id, object_id, kind) DO NOTHING"#,
            [
                user_id.into(),
                object_id.clone().into(),
                activity_id.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    if inserted.rows_affected() == 0 {
        let st = stats_for_one(db, user_id, &object_id).await;
        return Ok(InteractionResponse {
            success: true,
            object_id,
            kind: "announce".into(),
            activity_id: None,
            liked_by_me: Some(st.liked_by_me),
            bookmarked_by_me: Some(st.bookmarked_by_me),
            announced_by_me: Some(true),
            like_count: Some(st.like_count),
            bookmark_count: Some(st.bookmark_count),
            announce_count: Some(st.announce_count),
            reply_count: Some(st.reply_count),
        });
    }

    // Embed a *copy* of the quoted post (slim + nested chain), not a live pointer-only ref.
    // Re-quoting a repost nests the intermediate commentary under mfp:quotedObject.
    let object_value = resolve_local_object(db, &object_id).await;
    let quoted_slim = object_value
        .as_ref()
        .map(slim_quoted_object)
        .unwrap_or_else(|| json!({ "id": object_id.clone(), "type": "Note" }));
    let root_id = root_quoted_object_id(object_value.as_ref(), &object_id);
    let quote_depth = object_value
        .as_ref()
        .and_then(|o| o.get("mfp:quoteDepth").and_then(|v| v.as_u64()))
        .map(|d| d.saturating_add(1))
        .unwrap_or_else(|| {
            if object_value
                .as_ref()
                .map(is_repost_object)
                .unwrap_or(false)
            {
                1
            } else {
                0
            }
        })
        .min(MAX_QUOTE_NEST_DEPTH as u64);

    let content_html = format!("<p>{}</p>", escape_html_lite(content));
    let published = now_iso8601();

    let mut note = json!({
        "type": "Note",
        "id": &note_id,
        "attributedTo": &local_actor,
        "content": content_html,
        "source": {
            "content": content,
            "mediaType": "text/plain",
        },
        "mediaType": "text/html",
        "published": &published,
        "to": [AP_PUBLIC],
        // Quote the immediate parent (may itself be a repost); root is tracked separately.
        "inReplyTo": &object_id,
        "quoteUrl": &object_id,
        "mfp:kind": "repost",
        "mfp:contentType": "repost",
        "mfp:contentId": &note_content_id,
        "mfp:quotedObjectId": &object_id,
        "mfp:rootQuotedObjectId": &root_id,
        "mfp:quoteDepth": quote_depth,
        "mfp:quotedObject": quoted_slim,
        "attachment": [],
    });
    // Keep a plain content_preview for timeline list UIs (commentary only — not the quote body).
    note["content_preview"] = json!(content.chars().take(200).collect::<String>());

    let create_json = json!({
        "@context": build_context(),
        "type": "Create",
        "id": &activity_id,
        "actor": &local_actor,
        "published": &published,
        "to": [AP_PUBLIC],
        "object": note,
    });

    // Persist as Create / repost activity + published_content (for 已发布 list).
    let act_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Create', 'repost', $3, true, NOW())
               RETURNING id"#,
            [
                activity_id.clone().into(),
                user_id.into(),
                create_json.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_db_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    // Surface under 已发布 (content_type=repost; list_published includes it).
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_published_content
                   (user_id, content_type, content_id, activity_id, visibility, published_at)
               VALUES ($1, 'repost', $2, $3, 'public', NOW())
               ON CONFLICT (content_type, content_id) DO NOTHING"#,
            [
                user_id.into(),
                note_content_id.clone().into(),
                activity_id.clone().into(),
            ],
        ))
        .await;

    // Author timeline: show the quote-repost as a Create Note (user's commentary).
    let preview: Option<String> = Some(content.chars().take(200).collect::<String>());
    let content_for_tl = create_json
        .get("object")
        .cloned()
        .unwrap_or(json!({}));

    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_timeline
                   (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
               SELECT $1, $2, NULL, 'Create', 'repost', $3, $4, NOW()
               WHERE NOT EXISTS (
                   SELECT 1 FROM federation_timeline
                   WHERE user_id = $1 AND activity_id = $2
               )"#,
            [
                user_id.into(),
                activity_id.clone().into(),
                preview.clone().into(),
                content_for_tl.into(),
            ],
        ))
        .await;

    if act_db_id > 0 {
        // Fan-out Create to followers + notify original author.
        let _ = content::fan_out_to_followers(db, user_id, act_db_id, &create_json).await;
        deliver_to_object_author(db, act_db_id, &create_json, &object_id).await;
    }

    let st = stats_for_one(db, user_id, &object_id).await;
    Ok(InteractionResponse {
        success: true,
        object_id,
        kind: "announce".into(),
        activity_id: Some(activity_id),
        liked_by_me: Some(st.liked_by_me),
        bookmarked_by_me: Some(st.bookmarked_by_me),
        announced_by_me: Some(true),
        like_count: Some(st.like_count),
        bookmark_count: Some(st.bookmark_count),
        announce_count: Some(st.announce_count),
        reply_count: Some(st.reply_count),
    })
}

/// POST /api/federation/unannounce — undo quote-repost
pub async fn unannounce_object(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    object_id_raw: &str,
) -> Result<InteractionResponse, (StatusCode, Json<serde_json::Value>)> {
    let object_id = require_object_id(object_id_raw)?;
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT activity_id FROM federation_object_interactions
               WHERE user_id = $1 AND object_id = $2 AND kind = 'announce'"#,
            [user_id.into(), object_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    let original_id: Option<String> = row
        .and_then(|r| r.try_get::<Option<String>>("", "activity_id").ok().flatten());

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"DELETE FROM federation_object_interactions
           WHERE user_id = $1 AND object_id = $2 AND kind = 'announce'"#,
        [user_id.into(), object_id.clone().into()],
    ))
    .await
    .map_err(db_err)?;

    if let Some(ann_id) = original_id.filter(|s| !s.is_empty()) {
        // Remove from local timelines
        let _ = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM federation_timeline WHERE activity_id = $1",
                [ann_id.clone().into()],
            ))
            .await;

        // Load original activity to decide Undo(Announce) vs Delete(Create Note).
        let orig_act = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT activity_type, object_json FROM federation_activities
                   WHERE activity_id = $1 LIMIT 1"#,
                [ann_id.clone().into()],
            ))
            .await
            .ok()
            .flatten();

        let (act_type, object_json): (String, Option<serde_json::Value>) =
            if let Some(r) = orig_act {
                (
                    r.try_get::<String>("", "activity_type")
                        .unwrap_or_else(|_| "Announce".into()),
                    r.try_get::<Option<serde_json::Value>>("", "object_json")
                        .ok()
                        .flatten(),
                )
            } else {
                ("Announce".into(), None)
            };

        let undo_id = generate_activity_id(&base_url);
        let undo_json = if act_type == "Create" {
            // Quote-repost path: Delete the Note we created.
            let note_id = object_json
                .as_ref()
                .and_then(|v| v.pointer("/object/id"))
                .and_then(|v| v.as_str())
                .unwrap_or(ann_id.as_str())
                .to_string();
            json!({
                "@context": build_ap_context(),
                "type": "Delete",
                "id": &undo_id,
                "actor": &local_actor,
                "object": &note_id,
                "published": now_iso8601(),
                "to": [AP_PUBLIC],
            })
        } else {
            // Legacy bare Announce path.
            json!({
                "@context": build_ap_context(),
                "type": "Undo",
                "id": &undo_id,
                "actor": &local_actor,
                "object": {
                    "type": "Announce",
                    "id": &ann_id,
                    "actor": &local_actor,
                    "object": &object_id,
                },
                "published": now_iso8601(),
            })
        };

        if let Ok(Some(act_row)) = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                       (activity_id, user_id, activity_type, object_json, is_local, published_at)
                   VALUES ($1, $2, $3, $4, true, NOW())
                   RETURNING id"#,
                [
                    undo_id.clone().into(),
                    user_id.into(),
                    undo_json
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Undo")
                        .to_string()
                        .into(),
                    undo_json.clone().into(),
                ],
            ))
            .await
        {
            let act_db_id: i32 = act_row.try_get("", "id").unwrap_or(0);
            if act_db_id > 0 {
                let _ = content::fan_out_to_followers(db, user_id, act_db_id, &undo_json).await;
                deliver_to_object_author(db, act_db_id, &undo_json, &object_id).await;
            }
        }
    }

    let st = stats_for_one(db, user_id, &object_id).await;
    Ok(InteractionResponse {
        success: true,
        object_id,
        kind: "announce".into(),
        activity_id: None,
        liked_by_me: Some(st.liked_by_me),
        bookmarked_by_me: Some(st.bookmarked_by_me),
        announced_by_me: Some(false),
        like_count: Some(st.like_count),
        bookmark_count: Some(st.bookmark_count),
        announce_count: Some(st.announce_count),
        reply_count: Some(st.reply_count),
    })
}

// ==================== Delivery helpers ====================

async fn deliver_like_or_announce(
    db: &DatabaseConnection,
    user_id: i32,
    activity_db_id: i32,
    activity_json: &serde_json::Value,
    object_id: &str,
) {
    let act_type = activity_json["type"].as_str().unwrap_or("");
    // Like: deliver to object author only (do not fan-out to followers' home feeds).
    // Announce: fan-out to followers + author (repost should appear on followers' timelines).
    if act_type == "Announce" {
        let _ = content::fan_out_to_followers(db, user_id, activity_db_id, activity_json).await;
    }
    let _ = user_id;
    deliver_to_object_author(db, activity_db_id, activity_json, object_id).await;
}

/// Best-effort: deliver activity to the object's attributedTo inbox.
async fn deliver_to_object_author(
    db: &DatabaseConnection,
    activity_db_id: i32,
    activity_json: &serde_json::Value,
    object_id: &str,
) {
    let Some(author) = resolve_object_author(db, object_id).await else {
        return;
    };

    let base_url = get_base_url().await;
    // Same-instance author: nothing extra needed for counts (local interactions already set).
    // Still skip HTTP delivery queue for local.
    if local_username_from_actor_url(&base_url, &author).is_some() {
        return;
    }

    // Remote author: look up inbox
    let inbox_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT inbox_url, domain FROM federation_remote_actors WHERE actor_url = $1",
            [author.clone().into()],
        ))
        .await
        .ok()
        .flatten();

    let (inbox, domain) = if let Some(row) = inbox_row {
        let inbox: String = row.try_get("", "inbox_url").unwrap_or_default();
        let domain: String = row.try_get("", "domain").unwrap_or_default();
        (inbox, domain)
    } else {
        // Derive shared inbox guess
        let domain = extract_domain(&author).unwrap_or_default();
        let inbox = format!("{}/inbox", author.trim_end_matches('/'));
        (inbox, domain)
    };

    if inbox.is_empty() {
        return;
    }

    // Avoid double-queue if author is already a follower (fan_out already queued)
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
               SELECT $1, $2, $3, 'pending', NOW()
               WHERE NOT EXISTS (
                   SELECT 1 FROM federation_delivery_queue
                   WHERE activity_id = $1 AND target_inbox = $2 AND status = 'pending'
               )"#,
            [
                activity_db_id.into(),
                inbox.into(),
                domain.into(),
            ],
        ))
        .await;

    let _ = activity_json; // activity already stored by caller
}

// ==================== Inbound handling ====================

/// Record inbound Like without polluting the home timeline.
/// Call after federation_activities insert.
pub async fn handle_inbound_like(
    _db: &DatabaseConnection,
    _local_user_id: i32,
    actor_url_str: &str,
    activity: &serde_json::Value,
) {
    let object_id = extract_object_id(&activity["object"]).unwrap_or_default();
    tracing::debug!(
        "👍 Inbound Like from {} on {}",
        actor_url_str,
        object_id
    );
    // Counts derived from federation_activities (is_local=false).
}

/// Undo Like / Announce from remote.
pub async fn handle_inbound_undo_interaction(
    db: &DatabaseConnection,
    local_user_id: i32,
    activity: &serde_json::Value,
) {
    let inner = &activity["object"];
    let inner_type = inner["type"].as_str().unwrap_or("");
    let inner_id = inner["id"].as_str().unwrap_or("");

    match inner_type {
        "Like" if !inner_id.is_empty() => {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "DELETE FROM federation_activities WHERE activity_id = $1 AND is_local = false",
                    [inner_id.into()],
                ))
                .await;
        }
        "Announce" if !inner_id.is_empty() => {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "DELETE FROM federation_timeline WHERE user_id = $1 AND activity_id = $2",
                    [local_user_id.into(), inner_id.into()],
                ))
                .await;
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "DELETE FROM federation_activities WHERE activity_id = $1 AND is_local = false",
                    [inner_id.into()],
                ))
                .await;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_object_id_from_string() {
        assert_eq!(
            extract_object_id(&json!("https://ex.com/notes/1")).as_deref(),
            Some("https://ex.com/notes/1")
        );
    }

    #[test]
    fn extract_object_id_from_note() {
        assert_eq!(
            extract_object_id(&json!({"type": "Note", "id": "https://ex.com/notes/2"}))
                .as_deref(),
            Some("https://ex.com/notes/2")
        );
    }

    #[test]
    fn extract_object_id_from_create_envelope() {
        assert_eq!(
            extract_object_id(&json!({
                "type": "Create",
                "object": {"type": "Note", "id": "https://ex.com/notes/3"}
            }))
            .as_deref(),
            Some("https://ex.com/notes/3")
        );
    }

    #[test]
    fn slim_quoted_preserves_nested_repost_chain() {
        let root = json!({
            "id": "https://ex.com/notes/root",
            "type": "Note",
            "attributedTo": "https://ex.com/users/alice",
            "content": "original post"
        });
        let mid = json!({
            "id": "https://ex.com/notes/mid",
            "type": "Note",
            "attributedTo": "https://ex.com/users/bob",
            "source": { "content": "bob's commentary" },
            "mfp:kind": "repost",
            "mfp:quotedObjectId": "https://ex.com/notes/root",
            "mfp:rootQuotedObjectId": "https://ex.com/notes/root",
            "mfp:quotedObject": root,
        });
        let slim = slim_quoted_object(&mid);
        assert_eq!(slim["id"], "https://ex.com/notes/mid");
        assert_eq!(slim["mfp:kind"], "repost");
        assert!(
            slim["content_preview"]
                .as_str()
                .unwrap_or("")
                .contains("bob")
        );
        let nested = &slim["mfp:quotedObject"];
        assert_eq!(nested["id"], "https://ex.com/notes/root");
        assert!(
            nested["content_preview"]
                .as_str()
                .unwrap_or("")
                .contains("original")
        );
        assert_eq!(
            root_quoted_object_id(Some(&mid), "fallback"),
            "https://ex.com/notes/root"
        );
    }

    #[test]
    fn slim_quoted_caps_deep_nesting() {
        // Build a chain deeper than MAX_QUOTE_NEST_DEPTH.
        let mut cur = json!({
            "id": "https://ex.com/notes/0",
            "type": "Note",
            "content": "level 0",
            "attributedTo": "https://ex.com/users/u0",
        });
        for i in 1..=5 {
            cur = json!({
                "id": format!("https://ex.com/notes/{i}"),
                "type": "Note",
                "source": { "content": format!("level {i}") },
                "attributedTo": format!("https://ex.com/users/u{i}"),
                "mfp:kind": "repost",
                "mfp:quotedObject": cur,
                "mfp:rootQuotedObjectId": "https://ex.com/notes/0",
            });
        }
        let slim = slim_quoted_object(&cur);
        // Walk embedded chain; should stop without panicking and mark truncation at leaf.
        let mut node = &slim;
        let mut depth = 0;
        while let Some(inner) = node.get("mfp:quotedObject").filter(|v| v.is_object()) {
            node = inner;
            depth += 1;
            assert!(depth <= MAX_QUOTE_NEST_DEPTH + 1);
        }
        assert!(depth <= MAX_QUOTE_NEST_DEPTH);
    }

    #[test]
    fn extract_object_id_rejects_blank_and_accepts_nested() {

        assert!(extract_object_id(&serde_json::json!("")).is_none());
        assert!(extract_object_id(&serde_json::json!("   ")).is_none());
        assert_eq!(
            extract_object_id(&serde_json::json!({
                "type": "Create",
                "object": {"type": "Note", "id": "https://a.example/notes/1"}
            })).as_deref(),
            Some("https://a.example/notes/1")
        );

    }

    #[test]
    fn extract_object_id_from_update_envelope() {
        assert_eq!(
            extract_object_id(&serde_json::json!({
                "type": "Update",
                "object": {"id": "https://a.example/notes/9", "type": "Note"}
            })).as_deref(),
            Some("https://a.example/notes/9")
        );

    }

    #[test]
    fn extract_object_id_trims_string_id() {
        assert_eq!(
            extract_object_id(&serde_json::json!("  https://a.example/notes/1  ")).as_deref(),
            Some("https://a.example/notes/1")
        );

    }
}
