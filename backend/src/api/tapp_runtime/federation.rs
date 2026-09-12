//! Role-aware federation feed exposed only through a Tapp runtime grant.
//!
//! Merge/dedupe and item projection live in
//! [`crate::services::tapp_federation_feed`]. This module keeps SQL loaders,
//! interaction enrichment, and HTTP grant checks.

use crate::error::HttpError;
use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, FixedOffset};
use sea_orm::{DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use serde_json::{json, Value};

use crate::services::permission_service::TappPermission;
use crate::services::tapp_federation_feed::{
    dedupe_federation_feed, federation_feed_includes_personal, federation_feed_item,
    merge_federation_feed, FederationFeedRowView,
};

use super::RuntimeGrantContext;

const AP_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";

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

fn db_unavailable() -> HttpError {
    HttpError::from((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(AppError::public_json("Federation feed is unavailable")),
    ))
}

fn feed_item(row: FeedRow) -> Value {
    let timestamp = row.received_at.to_rfc3339();
    let object_id = row
        .content_json
        .as_ref()
        .and_then(crate::federation::interactions::extract_object_id);
    federation_feed_item(FederationFeedRowView {
        activity_id: &row.activity_id,
        activity_type: row.activity_type.as_deref().unwrap_or(""),
        object_type: row.object_type.as_deref(),
        content_preview: row.content_preview.as_deref(),
        content_json: row.content_json.as_ref(),
        object_id: object_id.as_deref(),
        received_at_rfc3339: &timestamp,
        scope: &row.scope,
        actor_url: row.actor_url.as_deref(),
        username: row.username.as_deref(),
        domain: row.domain.as_deref(),
        display_name: row.display_name.as_deref(),
        avatar_url: row.avatar_url.as_deref(),
        is_local: row.is_local.unwrap_or(false),
    })
}

async fn enrich_feed_items(db: &DatabaseConnection, user_id: i32, items: &mut [Value]) {
    let object_ids: Vec<String> = items
        .iter()
        .filter_map(|it| {
            it.get("object_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let Ok(stats_map) =
        crate::federation::interactions::interaction_stats_for_objects(db, user_id, &object_ids)
            .await
    else {
        return;
    };
    for item in items.iter_mut() {
        let Some(oid) = item
            .get("object_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        else {
            continue;
        };
        let Some(st) = stats_map.get(&oid) else {
            continue;
        };
        if let Some(obj) = item.as_object_mut() {
            obj.insert("liked_by_me".into(), json!(st.liked_by_me));
            obj.insert("bookmarked_by_me".into(), json!(st.bookmarked_by_me));
            obj.insert("announced_by_me".into(), json!(st.announced_by_me));
            obj.insert("like_count".into(), json!(st.like_count));
            obj.insert("bookmark_count".into(), json!(st.bookmark_count));
            obj.insert("announce_count".into(), json!(st.announce_count));
            obj.insert("reply_count".into(), json!(st.reply_count));
            obj.insert("is_bookmarked".into(), json!(st.bookmarked_by_me));
        }
    }
}

/// SQL expression: resolved local-user avatar URL when present. Used only for the
/// post author, never the viewer. Authors follow `services::avatar`.
fn local_user_avatar_expr(alias: &str) -> String {
    crate::services::avatar::avatar_snapshot_expr(alias)
}

async fn load_personal_feed(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<Value>, HttpError> {
    let base_url = crate::federation::types::get_base_url().await;
    let base = base_url.trim_end_matches('/').to_string();
    let domain = crate::federation::types::extract_domain(&base_url).unwrap_or_default();
    // Author identity must come from remote_actor (remote/same-instance posts) or
    // the local author row for self-posts. Never fall back to the timeline owner
    // (viewer) when remote_actor.display_name/avatar is missing — that made every
    // post show the viewer's nickname.
    let author_avatar = local_user_avatar_expr("author");
    let peer_avatar = local_user_avatar_expr("peer");
    let sql = format!(
        r#"SELECT t.activity_id, t.activity_type, t.object_type,
                  t.content_preview, t.content_json, t.received_at,
                  COALESCE(
                      ra.actor_url,
                      CASE WHEN author.username IS NOT NULL
                           THEN $2 || '/users/' || author.username
                           ELSE NULL END
                  ) AS actor_url,
                  COALESCE(ra.username, author.username) AS username,
                  COALESCE(
                      NULLIF(ra.domain, ''),
                      CASE WHEN ra.id IS NULL THEN $3 ELSE NULL END
                  ) AS domain,
                  CASE
                      WHEN ra.id IS NOT NULL THEN
                          COALESCE(
                              NULLIF(ra.display_name, ''),
                              NULLIF(peer.display_name, ''),
                              ra.username,
                              peer.username
                          )
                      ELSE
                          COALESCE(NULLIF(author.display_name, ''), author.username)
                  END AS display_name,
                  CASE
                      WHEN ra.id IS NOT NULL THEN
                          COALESCE(
                              NULLIF(ra.avatar_url, ''),
                              CASE
                                  WHEN peer.username IS NOT NULL AND ({peer_avatar}) IS NOT NULL
                                  THEN $2 || '/users/' || peer.username || '/avatar'
                                  ELSE NULL
                              END
                          )
                      ELSE
                          CASE
                              WHEN author.username IS NOT NULL AND ({author_avatar}) IS NOT NULL
                              THEN $2 || '/users/' || author.username || '/avatar'
                              ELSE NULL
                          END
                  END AS avatar_url,
                  'personal'::TEXT AS scope,
                  (ra.id IS NULL) AS is_local
           FROM federation_timeline t
           LEFT JOIN federation_remote_actors ra ON ra.id = t.remote_actor_id
           -- Self-authored timeline rows only (remote_actor_id IS NULL).
           LEFT JOIN users author ON ra.id IS NULL AND author.id = t.user_id
           -- Same-instance peer enrichment for remote_actor stubs (local followees).
           LEFT JOIN users peer ON ra.id IS NOT NULL
               AND ra.username IS NOT NULL
               AND peer.username = ra.username
               AND (
                   ra.actor_url LIKE ($2 || '/users/%')
                   OR ra.domain = $3
               )
           WHERE t.user_id = $1
             AND (t.activity_type IS NULL OR t.activity_type <> 'Like')
           ORDER BY t.received_at DESC
           LIMIT 100"#,
        peer_avatar = peer_avatar,
        author_avatar = author_avatar,
    );
    let rows = FeedRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        [user_id.into(), base.into(), domain.into()],
    ))
    .all(db)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "Failed to load personal federation feed");
        db_unavailable()
    })?;

    let mut items: Vec<Value> = rows.into_iter().map(feed_item).collect();
    enrich_feed_items(db, user_id, &mut items).await;
    Ok(items)
}

async fn load_public_feed(db: &DatabaseConnection) -> Result<Vec<Value>, HttpError> {
    // Local author avatar follows the same snapshot ladder as personal feed /
    // federation actor documents — never raw `users.avatar_url` alone (that
    // ignores avatar_source_kind / avatar_resolved_url).
    let local_avatar = local_user_avatar_expr("u");
    let base_url = crate::federation::types::get_base_url().await;
    let base = base_url.trim_end_matches('/').to_string();
    let sql = format!(
        r#"WITH public_items AS (
               SELECT DISTINCT ON (a.activity_id)
                      a.activity_id,
                      a.activity_type,
                      a.object_type,
                      LEFT(COALESCE(
                          a.object_json #>> '{{object,content}}',
                          a.object_json #>> '{{object,source,content}}',
                          a.object_json ->> 'content',
                          a.object_json #>> '{{object,summary}}',
                          a.object_json ->> 'summary'
                      ), 200) AS content_preview,
                      COALESCE(
                          a.object_json -> 'object',
                          a.object_json
                      ) AS content_json,
                      COALESCE(a.received_at, a.published_at) AS received_at,
                      COALESCE(
                          a.object_json ->> 'actor',
                          a.object_json #>> '{{object,attributedTo}}',
                          a.object_json ->> 'attributedTo',
                          ra.actor_url
                      ) AS actor_url,
                      COALESCE(u.username, ra.username) AS username,
                      ra.domain,
                      COALESCE(u.display_name, ra.display_name, u.username, ra.username) AS display_name,
                      COALESCE(
                          CASE
                              WHEN u.username IS NOT NULL AND ({local_avatar}) IS NOT NULL
                              THEN $2 || '/users/' || u.username || '/avatar'
                              ELSE NULL
                          END,
                          ra.avatar_url
                      ) AS avatar_url,
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
                          OR COALESCE((a.object_json #> '{{object,to}}')::JSONB, '[]'::JSONB) ? $1
                          OR COALESCE((a.object_json #> '{{object,cc}}')::JSONB, '[]'::JSONB) ? $1
                      )
                  )
               ORDER BY a.activity_id, COALESCE(a.received_at, a.published_at) DESC
           )
           SELECT *
           FROM public_items
           ORDER BY received_at DESC
           LIMIT 100"#,
        local_avatar = local_avatar,
    );
    let rows = FeedRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        [AP_PUBLIC.into(), base.into()],
    ))
    .all(db)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "Failed to load public federation feed");
        db_unavailable()
    })?;

    Ok(rows.into_iter().map(feed_item).collect())
}

/// Author domain for a `federation_activities` row.
///
/// `ra.domain` is authoritative when the actor document was fetched; otherwise
/// fall back to the authority of whichever actor IRI the activity carries, and
/// finally to the local domain for self-authored rows (they have no remote actor).
fn author_domain_expr(local_domain_param: &str) -> String {
    format!(
        r#"lower(COALESCE(
               NULLIF(ra.domain, ''),
               substring(COALESCE(
                   a.object_json ->> 'actor',
                   a.object_json #>> '{{object,attributedTo}}',
                   a.object_json ->> 'attributedTo',
                   ra.actor_url
               ) from '^[a-zA-Z][a-zA-Z0-9+.-]*://([^/]+)'),
               CASE WHEN a.is_local THEN {local_domain_param}::text ELSE NULL END
           ))"#
    )
}

/// Rooms-feed SQL. `$1` = AP Public, `$2` = base URL, `$3` = local domain.
///
/// Split out from the loader so the statement can be inspected without a live
/// connection — it nests three generated fragments and a brace-heavy JSON path
/// syntax, which is exactly the shape that breaks silently at runtime.
fn rooms_feed_sql(local_avatar: &str) -> String {
    format!(
        r#"WITH peer_domains AS ({peer_domains}),
           rooms_items AS (
               SELECT DISTINCT ON (a.activity_id)
                      a.activity_id,
                      a.activity_type,
                      a.object_type,
                      LEFT(COALESCE(
                          a.object_json #>> '{{object,content}}',
                          a.object_json #>> '{{object,source,content}}',
                          a.object_json ->> 'content',
                          a.object_json #>> '{{object,summary}}',
                          a.object_json ->> 'summary'
                      ), 200) AS content_preview,
                      COALESCE(
                          a.object_json -> 'object',
                          a.object_json
                      ) AS content_json,
                      COALESCE(a.received_at, a.published_at) AS received_at,
                      COALESCE(
                          a.object_json ->> 'actor',
                          a.object_json #>> '{{object,attributedTo}}',
                          a.object_json ->> 'attributedTo',
                          ra.actor_url
                      ) AS actor_url,
                      COALESCE(u.username, ra.username) AS username,
                      ra.domain,
                      COALESCE(u.display_name, ra.display_name, u.username, ra.username) AS display_name,
                      COALESCE(
                          CASE
                              WHEN u.username IS NOT NULL AND ({local_avatar}) IS NOT NULL
                              THEN $2 || '/users/' || u.username || '/avatar'
                              ELSE NULL
                          END,
                          ra.avatar_url
                      ) AS avatar_url,
                      'rooms'::TEXT AS scope,
                      a.is_local AS is_local
               FROM federation_activities a
               LEFT JOIN federation_published_content pc ON pc.activity_id = a.activity_id
               LEFT JOIN users u ON u.id = a.user_id
               LEFT JOIN federation_remote_actors ra ON ra.id = a.remote_actor_id
               WHERE a.activity_type IN ('Create', 'Announce')
                 AND (
                     pc.visibility = 'public'
                     OR (
                         a.is_local = false
                         AND (
                             COALESCE((a.object_json -> 'to')::JSONB, '[]'::JSONB) ? $1
                             OR COALESCE((a.object_json -> 'cc')::JSONB, '[]'::JSONB) ? $1
                             OR COALESCE((a.object_json #> '{{object,to}}')::JSONB, '[]'::JSONB) ? $1
                             OR COALESCE((a.object_json #> '{{object,cc}}')::JSONB, '[]'::JSONB) ? $1
                         )
                     )
                 )
                 AND {author_domain} IN (SELECT domain FROM peer_domains)
               ORDER BY a.activity_id, COALESCE(a.received_at, a.published_at) DESC
           )
           SELECT *
           FROM rooms_items
           ORDER BY received_at DESC
           LIMIT 100"#,
        peer_domains = crate::federation::room_peers::room_peer_domains_sql("$3"),
        local_avatar = local_avatar,
        author_domain = author_domain_expr("$3"),
    )
}

/// Public posts authored anywhere on an instance that shares a joined room with
/// this one — the Aro Home feed.
///
/// Scope is the **instance**, not room membership: three instances in one group
/// chat means all three see every user of the other two, whether or not those
/// users ever joined the room. See [`crate::federation::room_peers`].
///
/// Restricted to `Create`/`Announce` so room-protocol activities (`myriad:Room*`,
/// stored in the same table by the room fan-out) never surface as posts.
async fn load_rooms_feed(db: &DatabaseConnection) -> Result<Vec<Value>, HttpError> {
    let base_url = crate::federation::types::get_base_url().await;
    let base = base_url.trim_end_matches('/').to_string();
    let local_domain = crate::federation::types::extract_domain(&base_url)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let rows = FeedRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        rooms_feed_sql(&local_user_avatar_expr("u")),
        [AP_PUBLIC.into(), base.into(), local_domain.into()],
    ))
    .all(db)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "Failed to load room-peer federation feed");
        db_unavailable()
    })?;

    Ok(rows.into_iter().map(feed_item).collect())
}

/// GET /api/tapp/federation/rooms-feed
///
/// Every public post from every user of every instance represented in a group
/// chat this instance has joined, local users included, deduplicated.
///
/// Separate from [`get_federation_feed`] on purpose: that one is public (plus
/// personal when the subject is signed in); this one is neighbourhood public posts.
pub async fn get_federation_rooms_feed(
    State(db): State<DatabaseConnection>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::FederationRead)?;

    let mut items = dedupe_federation_feed(load_rooms_feed(&db).await?);
    // Guests get the same list without like/bookmark state (there is no "me").
    if federation_feed_includes_personal(runtime_grant.subject_id()) {
        enrich_feed_items(&db, runtime_grant.subject_id(), &mut items).await;
    }
    let total = items.len();

    Ok(Json(json!({
        "items": items,
        "total": total,
        "audience": "rooms",
    })))
}

/// GET /api/tapp/federation/feed
///
/// Guests receive public activities only. Authenticated users receive their
/// personal federation timeline merged with the same public activities.
pub async fn get_federation_feed(
    State(db): State<DatabaseConnection>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::FederationRead)?;

    let public = load_public_feed(&db).await?;
    let include_personal = federation_feed_includes_personal(runtime_grant.subject_id());
    let personal = if include_personal {
        load_personal_feed(&db, runtime_grant.subject_id()).await?
    } else {
        Vec::new()
    };
    let mut items = merge_federation_feed(personal, public);
    // Re-enrich after merge so public-only rows also get counts / me-flags.
    if include_personal {
        enrich_feed_items(&db, runtime_grant.subject_id(), &mut items).await;
    }
    let total = items.len();

    Ok(Json(json!({
        "items": items,
        "total": total,
        "audience": if include_personal { "public+personal" } else { "public" },
    })))
}

#[cfg(test)]
mod tests {
    use crate::services::tapp_federation_feed::merge_federation_feed;
    use serde_json::json;

    #[test]
    fn personal_copy_wins_when_public_feed_contains_same_activity() {
        let merged = merge_federation_feed(
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
        let merged = merge_federation_feed(
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

    /// Guards the four clauses that decide what Home shows. Each was checked
    /// against a real Postgres fixture with three instances in one room; losing
    /// any of them fails silently (wrong rows, not an error).
    #[test]
    fn rooms_feed_sql_keeps_its_four_gates() {
        let sql = super::rooms_feed_sql("NULL");

        // 1. Posts only — room-protocol activities live in the same table.
        assert!(sql.contains("a.activity_type IN ('Create', 'Announce')"));
        // 2. Public only — local `followers`/`direct` posts and inbound DMs stay out.
        assert!(sql.contains("pc.visibility = 'public'"));
        assert!(sql.contains("a.object_json -> 'to'"));
        // 3. Scoped to instances sharing a joined room, local instance included.
        assert!(sql.contains("IN (SELECT domain FROM peer_domains)"));
        assert!(sql.contains("WITH peer_domains AS"));
        // 4. One row per activity, newest first.
        assert!(sql.contains("DISTINCT ON (a.activity_id)"));
        assert!(sql.contains("ORDER BY received_at DESC"));
    }

    #[test]
    fn author_domain_falls_back_from_remote_actor_to_iri_to_local() {
        let expr = super::author_domain_expr("$3");
        // A member whose actor document has not been fetched yet still resolves.
        assert!(expr.contains("NULLIF(ra.domain, '')"));
        assert!(expr.contains("a.object_json ->> 'actor'"));
        assert!(expr.contains("attributedTo"));
        // Self-authored rows have no remote actor at all.
        assert!(expr.contains("CASE WHEN a.is_local THEN $3::text"));
    }
}
use myriad_error::AppError;
