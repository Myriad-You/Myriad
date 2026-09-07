//! ActivityPub Follow / Accept / Undo / content / Move inbox handlers.

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::actor::RemoteActorInfo;
use crate::federation::types::*;

use super::inbox_err;
use super::local_deliver::{enqueue_delivery, enqueue_delivery_queue, DeliveryMode};

// Activity 处理器

/// Handle ActivityPub Move (domain / account migration).
///
/// Fail-closed unless **all** pass:
/// 1. HTTP Signature already verified (caller)
/// 2. `actor` == signed actor == `object` (old id); `target` present and distinct
/// 3. Fresh fetch of old actor has `movedTo` == target
/// 4. Fresh fetch of new actor has `alsoKnownAs` containing old id
///
/// On accept: re-point local `federation_follows` from old remote actor → new.
pub(crate) async fn handle_move(
    db: &DatabaseConnection,
    signed_actor: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    use crate::federation::move_actor::{
        fetch_actor_document, migrate_follows_old_to_new, verify_move_structure,
        verify_new_actor_also_known_as, verify_old_actor_moved_to,
    };

    let (old_actor, new_actor) = verify_move_structure(activity, signed_actor).map_err(|e| {
        tracing::warn!("Move rejected (structure): {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let old_doc = fetch_actor_document(db, &old_actor)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Move rejected (old actor fetch)");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Cannot fetch old actor for Move verify"})),
            )
        })?;

    verify_old_actor_moved_to(&old_doc, &old_actor, &new_actor).map_err(|e| {
        tracing::warn!("Move rejected (movedTo): {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let new_doc = fetch_actor_document(db, &new_actor)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Move rejected (new actor fetch)");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Cannot fetch new actor for Move verify"})),
            )
        })?;

    verify_new_actor_also_known_as(&new_doc, &new_actor, &old_actor).map_err(|e| {
        tracing::warn!("Move rejected (alsoKnownAs): {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let migrated = migrate_follows_old_to_new(db, &old_actor, &new_actor)
        .await
        .map_err(|e| inbox_err("Move follow migration failed", e))?;

    // Record inbound Move for audit
    let activity_id = activity["id"].as_str().unwrap_or("").to_string();
    if !activity_id.is_empty() {
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                       (activity_id, activity_type, object_type, object_json, is_local, received_at, published_at)
                   VALUES ($1, 'Move', 'Person', $2, false, NOW(), NOW())
                   ON CONFLICT (activity_id) DO NOTHING"#,
                [activity_id.clone().into(), activity.clone().into()],
            ))
            .await;
    }

    tracing::info!(
        old_actor = %old_actor,
        new_actor = %new_actor,
        migrated_follows = migrated,
        "Accepted ActivityPub Move"
    );

    Ok(StatusCode::ACCEPTED)
}

/// 处理 Follow 请求
pub(crate) async fn handle_follow(
    db: &impl ConnectionTrait,
    local_user_id: i32,
    actor_url_str: &str,
    activity: &serde_json::Value,
    follow_remote: Option<&RemoteActorInfo>,
    delivery_mode: DeliveryMode<'_>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // Follow.object 必须就是本次要记录的本地 Actor。
    //
    // 过去只用投递路径（/users/{name}/inbox 或 sharedInbox 的收件人解析）决定
    // local_user_id，从不看 activity.object，于是向 bob 的 inbox POST 一条
    // `Follow{object: ".../users/alice"}` 会给 **bob** 记上一个粉丝，随后
    // 发出的 Accept 里 object 还被重写成 bob 的 Actor。
    let base_url = get_base_url().await;
    let local_username = get_username_by_id(db, local_user_id).await?;
    let local_actor_url = actor_url(&base_url, &local_username);

    let follow_target = crate::federation::audience::object_id(&activity["object"]);
    match follow_target.as_deref() {
        Some(target) if same_actor_url(target, &local_actor_url) => {}
        Some(target) => {
            tracing::warn!(
                actor = %actor_url_str,
                target,
                expected = %local_actor_url,
                "Follow rejected: object does not match the local actor being followed"
            );
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "Follow.object does not match this actor",
                    "expected": local_actor_url,
                    "found": target,
                })),
            ));
        }
        None => {
            tracing::warn!(actor = %actor_url_str, "Follow rejected: missing object");
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Follow.object is required"})),
            ));
        }
    }

    let remote = follow_remote.ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Follow actor preflight was not completed"
            })),
        )
    })?;

    let activity_id = activity["id"].as_str().unwrap_or("").to_string();

    // Record incoming follow. On Postgres, ON CONFLICT DO UPDATE always reports
    // rows_affected >= 1 even when the row was already accepted — so the old
    // `rows_affected == 0` idempotency check never fired and re-enqueued Accept.
    // Pattern: conditional UPDATE + RETURNING; empty result means already accepted.
    let upserted = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_follows (user_id, remote_actor_id, direction, status, activity_id, created_at)
               VALUES ($1, $2, 'incoming', 'accepted', $3, NOW())
               ON CONFLICT (user_id, remote_actor_id, direction) DO UPDATE SET
                   status = 'accepted',
                   activity_id = EXCLUDED.activity_id
               WHERE federation_follows.status IS DISTINCT FROM 'accepted'
               RETURNING id"#,
            [
                local_user_id.into(),
                remote.id.into(),
                activity_id.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    if upserted.is_none() {
        // Already accepted — idempotent 202, no second Accept / enqueue / notify
        tracing::debug!(
            activity_id,
            "Ignoring already-accepted Follow (no second Accept)"
        );
        return Ok(StatusCode::ACCEPTED);
    }

    // 自动发送 Accept（Myriad 个人实例默认自动接受）
    // base_url / local_username / local_actor_url 已在入口处校验 Follow.object 时取得。

    // Embed a minimal Follow object (type/id/actor/object). Full inbound JSON
    // may omit id or carry extra @context noise that confuses remote Accept matching.
    let follow_activity_id = activity["id"].as_str().unwrap_or("").to_string();
    let accept_object = json!({
        "type": "Follow",
        "id": &follow_activity_id,
        "actor": actor_url_str,
        "object": &local_actor_url,
    });

    let accept = Activity {
        context: build_ap_context(),
        activity_type: "Accept".to_string(),
        id: generate_activity_id(&base_url),
        actor: local_actor_url.clone(),
        to: Some(vec![actor_url_str.to_string()]),
        cc: None,
        published: Some(now_iso8601()),
        object: accept_object,
        target: None,
    };

    // In a receipt transaction, queue the outbound Accept in the same DB
    // transaction.  Trusted in-process delivery is retained for the local
    // helper path, where no inbound receipt transaction is open.
    match delivery_mode {
        DeliveryMode::QueueOnly => {
            enqueue_delivery_queue(db, local_user_id, &accept, &remote.inbox_url).await?;
        }
        DeliveryMode::InProcess(local_db) => {
            enqueue_delivery(local_db, local_user_id, &accept, &remote.inbox_url).await?;
        }
    }

    // 新粉丝通知
    let follower_label = crate::federation::notify::actor_label(db, actor_url_str).await;
    crate::federation::notify::notify_new_follower(local_user_id, actor_url_str, &follower_label)
        .await;

    tracing::info!("✅ Follow accepted: {} → {}", actor_url_str, local_username);

    Ok(StatusCode::ACCEPTED)
}

/// 处理 Reject（Room 邀请被拒绝等）
pub(crate) async fn handle_reject(
    db: &impl ConnectionTrait,
    _local_user_id: i32,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let object = &activity["object"];
    let inner_type = object.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if inner_type == "myriad:RoomInvite"
        || inner_type == "myriad:Room"
        || object.get("room").is_some()
    {
        crate::federation::room::handle_room_invite_reject(db, actor_url_str, activity)
            .await
            .map_err(|e| inbox_err("Room invite Reject handling failed", e))?;
    } else {
        tracing::debug!("Ignoring Reject for unsupported object type={}", inner_type);
    }
    Ok(StatusCode::ACCEPTED)
}

/// Extract actor id from an ActivityPub activity.
///
/// Supports string IRI and expanded object `{ "id": "…", "type": "Person" }`.
/// Some peers embed the actor document; treating only strings rejects valid
/// Accept/Follow payloads even when the HTTP Signature keyId is correct.
pub fn extract_activity_actor_id(activity: &serde_json::Value) -> String {
    extract_iri_or_object_id(&activity["actor"])
}

/// Extract an IRI from a JSON value that may be a string, object with `id`/`href`,
/// or a single-level array of either (ActivityStreams multi-value).
fn extract_iri_or_object_id(value: &serde_json::Value) -> String {
    if let Some(s) = value.as_str() {
        return s.trim().to_string();
    }
    if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
        return id.trim().to_string();
    }
    // AS2 Link objects use `href` rather than `id`.
    if let Some(href) = value.get("href").and_then(|v| v.as_str()) {
        return href.trim().to_string();
    }
    if let Some(arr) = value.as_array() {
        for item in arr {
            if let Some(s) = item.as_str() {
                let t = s.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                let t = id.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
            if let Some(href) = item.get("href").and_then(|v| v.as_str()) {
                let t = href.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    String::new()
}

/// Extract the accepted object id from an Accept activity.
///
/// Supports:
/// - `object`: string activity/object id
/// - `object`: nested `{ "id", "type", ... }` (Follow / ChannelOpen / …)
/// - `object`: AS2 Link `{ "href": "…" }`
/// - `object`: array of the above (first non-empty id)
///
/// Does not walk arbitrary nesting beyond one object / array level.
pub fn extract_accept_object_id(activity: &serde_json::Value) -> String {
    extract_iri_or_object_id(&activity["object"])
}

/// Nested object type for Accept routing (Channel vs Follow).
/// Empty when object is a string id or Link-only. Arrays: first typed element.
fn extract_accept_object_type(activity: &serde_json::Value) -> String {
    let object = &activity["object"];
    if let Some(t) = object.get("type").and_then(|v| v.as_str()) {
        return t.to_string();
    }
    if let Some(arr) = object.as_array() {
        for item in arr {
            if let Some(t) = item.get("type").and_then(|v| v.as_str()) {
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    String::new()
}

/// 处理 Accept（我们发出的 Follow 被接受）
///
/// 授权绑定：状态变更仅在 Accept 的签名 actor 正是该 Channel/Follow 的
/// 远程对端时生效，防止第三方实例伪造他人的 Accept。
/// Actor 比对走 `same_actor_url`（host 大小写 / trailing slash），不依赖 SQL 字节级相等。
pub(crate) async fn handle_accept(
    db: &impl ConnectionTrait,
    local_user_id: i32,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // 签名校验保证了 activity.actor 就是本次请求的签名者
    // (string IRI or expanded object `{id}`; see extract_activity_actor_id)
    let accept_actor = extract_activity_actor_id(activity);
    // Accept.object: string id OR nested Follow/ChannelOpen {id,type,…}
    let inner_type = extract_accept_object_type(activity);
    let follow_id = extract_accept_object_id(activity);

    if inner_type == "myriad:ChannelOpen" || inner_type == "myriad:Channel" {
        // 远程方接受了我们的 Channel 开启请求
        let channel_id = follow_id.as_str(); // object.id 就是 channel_id
        if !channel_id.is_empty() {
            // 先取 pending channel 的远程对端 URL，再在 Rust 侧用 same_actor_url 授权
            let pending = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT ra.actor_url
                       FROM federation_channels c
                       JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
                       WHERE c.channel_id = $1 AND c.user_id = $2 AND c.status = 'pending'"#,
                    [channel_id.into(), local_user_id.into()],
                ))
                .await
                .map_err(db_err)?;

            let authorized = pending
                .as_ref()
                .and_then(|row| row.try_get::<String>("", "actor_url").ok())
                // Align with Follow Accept: host+username case / path form drift.
                .map(|remote_url| same_actor_or_user(&accept_actor, &remote_url))
                .unwrap_or(false);

            if authorized {
                let result = db
                    .execute_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"UPDATE federation_channels
                           SET status = 'accepted', last_activity_at = NOW()
                           WHERE channel_id = $1 AND user_id = $2 AND status = 'pending'"#,
                        [channel_id.into(), local_user_id.into()],
                    ))
                    .await
                    .map_err(db_err)?;

                if result.rows_affected() > 0 {
                    let remote_url = pending
                        .and_then(|row| row.try_get::<String>("", "actor_url").ok())
                        .unwrap_or_default();
                    let remote_label =
                        crate::federation::notify::actor_label(db, &remote_url).await;
                    crate::federation::notify::notify_channel_accepted(
                        local_user_id,
                        channel_id,
                        &remote_label,
                    )
                    .await;
                    // Unlock initiator's live Aro client (composer was pending-locked).
                    // Mirrors handle_channel_accept WS path for myriad:ChannelAccept.
                    crate::federation::ws_gateway::broadcast_to_channel(
                        channel_id,
                        &serde_json::json!({
                            "type": "channel_accepted",
                            "channel_id": channel_id
                        }),
                    )
                    .await;
                    tracing::info!("✅ Channel accepted: {}", channel_id);
                }
            } else {
                tracing::debug!(
                    "Channel accept no-op (not pending, not owner, or actor mismatch): channel={} accept_actor={}",
                    channel_id,
                    accept_actor
                );
            }
        }
    } else {
        // Standard Follow Accept — match Follow object.id, then fallback to a single
        // pending outgoing toward Accept.actor (same_actor_url). Idempotent notify.
        handle_follow_accept(db, local_user_id, &accept_actor, &follow_id, activity).await?;
    }

    Ok(StatusCode::ACCEPTED)
}

/// Loose actor match: exact `same_actor_url`, or same host+port + `/users/{name}`.
///
/// Handles path-capitalization / alias drift between WebFinger-resolved URLs and
/// Accept.actor built from the remote's base_url + preferredUsername.
fn same_actor_or_user(left: &str, right: &str) -> bool {
    if same_actor_url(left, right) {
        return true;
    }
    let l = normalize_actor_url(left);
    let r = normalize_actor_url(right);
    let (l_host, l_user) = split_actor_host_user(&l);
    let (r_host, r_user) = split_actor_host_user(&r);
    !l_host.is_empty()
        && l_host == r_host
        && !l_user.is_empty()
        && l_user.eq_ignore_ascii_case(&r_user)
}

fn split_actor_host_user(normalized: &str) -> (String, String) {
    // normalized form: scheme://host[:port]/users/name
    let Ok(url) = url::Url::parse(normalized) else {
        return (String::new(), String::new());
    };
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{}:{}", h.to_ascii_lowercase(), p),
        (Some(h), None) => h.to_ascii_lowercase(),
        _ => String::new(),
    };
    let path = url.path().trim_end_matches('/');
    let user = path
        .strip_prefix("/users/")
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
        .unwrap_or("")
        .to_string();
    (host, user)
}

/// Same host + username-compatible for Accept authorization under path drift.
///
/// Used when `same_actor_or_user` fails (e.g. Accept.actor is `/@bob` while the
/// stored remote is `/users/bob`) but we must **never** let a different
/// `/users/{name}` on the same host accept someone else's Follow.
fn same_host_username_compatible(accept_actor: &str, remote: &str) -> bool {
    let an = normalize_actor_url(accept_actor);
    let rn = normalize_actor_url(remote);
    let (ah, a_user) = split_actor_host_user(&an);
    let (rh, r_user) = split_actor_host_user(&rn);
    if ah.is_empty() || ah != rh {
        return false;
    }
    // Both standard `/users/{name}`: require same username (case-insensitive).
    // (Normally covered by `same_actor_or_user`; kept for defense-in-depth.)
    if !a_user.is_empty() && !r_user.is_empty() {
        return a_user.eq_ignore_ascii_case(&r_user);
    }
    // Accept uses a non-`/users/` path form (e.g. `/@bob`, `/ap/users/bob`):
    // last path segment (strip leading `@`) must match the stored remote user.
    if a_user.is_empty() && !r_user.is_empty() {
        if let Ok(url) = url::Url::parse(&an) {
            let last = url
                .path()
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_start_matches('@');
            return !last.is_empty() && last.eq_ignore_ascii_case(&r_user);
        }
    }
    false
}

/// Resolve which outgoing Follow an Accept refers to.
///
/// Matching order (safe, no silent multi-pick):
/// 1. Normalized `activity_id` + Accept.actor is remote peer (`same_actor_or_user`)
/// 2. Unique normalized `activity_id` + **same host + username-compatible**
/// (path/alias drift only — never cross-user on the same host, never
/// cross-host username-only)
/// 3. Exactly one **pending** outgoing to Accept.actor via `same_actor_or_user`
///
/// Already-`accepted` rows only yield idempotent success when matched by id
/// (or unique id+host+user). Ambiguous multi-pending → no match.
///
/// Returns `(activity_id, remote_actor_url, already_accepted)`.
pub fn resolve_follow_accept_target(
    follow_id: &str,
    accept_actor: &str,
    candidates: &[(String, String, String)], // activity_id, remote_actor_url, status
) -> Option<(String, String, bool)> {
    let follow_norm = normalize_activity_id(follow_id);
    if !follow_norm.is_empty() {
        // Prefer actor-authorized id match (host+user or same_actor_url)
        for (aid, remote, status) in candidates {
            if same_activity_id(aid, &follow_norm) && same_actor_or_user(accept_actor, remote) {
                return Some((aid.clone(), remote.clone(), status == "accepted"));
            }
        }
        // Unique id match with same host **and** username-compatible path drift.
        // Host-only was insufficient: multi-user instances share a host, and
        // activity ids can leak; a different /users/{name} must not Accept.
        let id_hits: Vec<_> = candidates
            .iter()
            .filter(|(aid, remote, _)| {
                same_activity_id(aid, &follow_norm)
                    && same_host_username_compatible(accept_actor, remote)
            })
            .collect();
        if id_hits.len() == 1 {
            let (aid, remote, status) = id_hits[0];
            tracing::info!(
                follow_id = follow_id,
                activity_id = %aid,
                accept_actor = accept_actor,
                remote = %remote,
                "Follow Accept: unique activity_id + host+user match (actor path drift)"
            );
            return Some((aid.clone(), remote.clone(), status == "accepted"));
        }
        if id_hits.len() > 1 {
            tracing::warn!(
                follow_id = follow_id,
                accept_actor = accept_actor,
                hits = id_hits.len(),
                "Follow Accept ambiguous: multiple activity_id matches on same host+user"
            );
        }
    }

    // Fallback: only pending + same_actor_or_user (same host+user). Never
    // username-only across hosts (same_actor_or_user enforces host).
    let pending_to_actor: Vec<_> = candidates
        .iter()
        .filter(|(_, remote, status)| {
            *status == "pending" && same_actor_or_user(accept_actor, remote)
        })
        .collect();
    if pending_to_actor.len() == 1 {
        let (aid, remote, _) = pending_to_actor[0];
        tracing::info!(
            follow_id = follow_id,
            activity_id = %aid,
            accept_actor = accept_actor,
            "Follow Accept fallback: matched single pending outgoing to Accept actor"
        );
        return Some((aid.clone(), remote.clone(), false));
    }

    if pending_to_actor.len() > 1 {
        tracing::warn!(
            follow_id = follow_id,
            accept_actor = accept_actor,
            candidates = pending_to_actor.len(),
            "Follow Accept fallback ambiguous: multiple pending outgoing to Accept actor"
        );
    }
    None
}

async fn handle_follow_accept(
    db: &impl ConnectionTrait,
    local_user_id: i32,
    accept_actor: &str,
    follow_id: &str,
    activity: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if accept_actor.is_empty() {
        tracing::warn!(
            activity_id = %activity["id"].as_str().unwrap_or(""),
            "Follow Accept missing actor; ignoring"
        );
        return Ok(());
    }

    // Candidate window (not full-table scan):
    // - all pending outgoing for this user (usually few)
    // - accepted rows matching activity_id variants (idempotent re-Accept)
    // - recent accepted (last 40) so normalized id compare can still hit after
    // query/host-case drift without scanning all history
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT f.activity_id, f.status, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON f.remote_actor_id = ra.id
               WHERE f.user_id = $1 AND f.direction = 'outgoing'
                 AND (
                   f.status = 'pending'
                   OR (
                     f.status = 'accepted'
                     AND (
                       ($2 <> '' AND f.activity_id = $2)
                       OR ($2 <> '' AND rtrim(f.activity_id, '/') = rtrim($2::text, '/'))
                       OR ($2 <> '' AND lower(f.activity_id) = lower($2))
                       OR ($2 <> '' AND rtrim(split_part(split_part(f.activity_id, '?', 1), '#', 1), '/')
                           = rtrim(split_part(split_part($2::text, '?', 1), '#', 1), '/'))
                       OR f.id IN (
                         SELECT f2.id FROM federation_follows f2
                         WHERE f2.user_id = $1 AND f2.direction = 'outgoing'
                           AND f2.status = 'accepted'
                         ORDER BY COALESCE(f2.accepted_at, f2.created_at) DESC NULLS LAST
                         LIMIT 40
                       )
                     )
                   )
                 )
               LIMIT 120"#,
            [local_user_id.into(), follow_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let candidates: Vec<(String, String, String)> = rows
        .iter()
        .map(|r| {
            (
                r.try_get::<String>("", "activity_id").unwrap_or_default(),
                r.try_get::<String>("", "actor_url").unwrap_or_default(),
                r.try_get::<String>("", "status").unwrap_or_default(),
            )
        })
        .filter(|(aid, remote, _)| !aid.is_empty() && !remote.is_empty())
        .collect();

    let Some((matched_id, remote_url, already_accepted)) =
        resolve_follow_accept_target(follow_id, accept_actor, &candidates)
    else {
        tracing::warn!(
            follow_id = follow_id,
            accept_actor = accept_actor,
            candidates = candidates.len(),
            "Follow Accept no match (activity_id + fallback failed)"
        );
        return Ok(());
    };

    if already_accepted {
        tracing::debug!(
            activity_id = %matched_id,
            accept_actor = accept_actor,
            "Follow Accept idempotent: already accepted"
        );
        return Ok(());
    }

    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_follows
               SET status = 'accepted', accepted_at = NOW()
               WHERE user_id = $1 AND direction = 'outgoing' AND activity_id = $2
                 AND status IS DISTINCT FROM 'accepted'"#,
            [local_user_id.into(), matched_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    if result.rows_affected() == 0 {
        tracing::debug!(
            activity_id = %matched_id,
            "Follow Accept race: row already accepted"
        );
        return Ok(());
    }

    let label = crate::federation::notify::actor_label(db, &remote_url).await;
    crate::federation::notify::notify_follow_accepted(local_user_id, accept_actor, &label).await;
    tracing::info!(
        activity_id = %matched_id,
        accept_actor = accept_actor,
        follow_object_id = follow_id,
        "✅ Our follow accepted"
    );
    Ok(())
}

/// 处理 Undo（包括 Undo Follow / Like / Announce）
pub(crate) async fn handle_undo(
    db: &impl ConnectionTrait,
    local_user_id: i32,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let inner_type = activity["object"]["type"].as_str().unwrap_or("");

    match inner_type {
        "Follow" => {
            // 远程用户取消关注。
            //
            // 字节级相等和本文件其余地方的 same_actor_url 语义不一致：末尾斜杠或
            // host 大小写一变，取关就静默成功（202）而粉丝关系还留着 —— 对面以为
            // 已经取关，我们这边还在往它的 inbox 投递。快路径保留精确匹配
            // （actor_url 有唯一约束，命中即唯一），未命中时再走规范化兜底。
            let removed = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"DELETE FROM federation_follows
                       WHERE user_id = $1 AND direction = 'incoming'
                       AND remote_actor_id IN (
                           SELECT id FROM federation_remote_actors WHERE actor_url = $2
                       )"#,
                    [local_user_id.into(), actor_url_str.into()],
                ))
                .await
                .map_err(db_err)?;

            let mut removed = removed.rows_affected();
            if removed == 0 {
                removed = delete_incoming_follow_normalized(db, local_user_id, actor_url_str)
                    .await
                    .map_err(db_err)?;
            }

            if removed > 0 {
                tracing::info!(
                    "🔓 Follow removed: {} unfollowed user {}",
                    actor_url_str,
                    local_user_id
                );
            } else {
                tracing::debug!(
                    actor = %actor_url_str,
                    local_user_id,
                    "Undo Follow matched no incoming follow row"
                );
            }
        }
        "Like" | "Announce" => {
            // 被撤销的互动必须由本次签名 Actor 创建 —— 否则任意远端都能
            // 按 activity id 撤销别人的 Like/Announce。
            let inner_actor = extract_activity_actor_id(&activity["object"]);
            if !inner_actor.is_empty() && !same_actor_url(&inner_actor, actor_url_str) {
                tracing::warn!(
                    actor = %actor_url_str,
                    inner_actor = %inner_actor,
                    "Undo rejected: inner activity belongs to a different actor"
                );
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "Undo actor does not own the undone activity",
                        "actor": actor_url_str,
                        "inner_actor": inner_actor,
                    })),
                ));
            }
            crate::federation::interactions::handle_inbound_undo_interaction(
                db,
                local_user_id,
                actor_url_str,
                activity,
            )
            .await
            .map_err(|e| inbox_err("Inbound interaction Undo handling failed", e))?;
        }
        _ => {
            tracing::debug!("Undo for unsupported type: {}", inner_type);
        }
    }

    Ok(StatusCode::ACCEPTED)
}

/// Delete this user's incoming follow from `actor_url_str`, comparing actor URLs
/// with [`same_actor_url`] instead of byte equality.
///
/// Fallback for the exact-match DELETE: the stored `actor_url` can differ from
/// the Undo's `actor` by trailing slash or host case, and an unfollow that
/// silently keeps the follower is worse than a slightly wider scan (bounded to
/// this user's incoming follows).
async fn delete_incoming_follow_normalized(
    db: &impl ConnectionTrait,
    local_user_id: i32,
    actor_url_str: &str,
) -> Result<u64, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT f.id, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON f.remote_actor_id = ra.id
               WHERE f.user_id = $1 AND f.direction = 'incoming'"#,
            [local_user_id.into()],
        ))
        .await?;

    let mut removed = 0u64;
    for row in rows {
        let (Ok(id), Ok(stored)) = (
            row.try_get::<i32>("", "id"),
            row.try_get::<String>("", "actor_url"),
        ) else {
            continue;
        };
        if !same_actor_url(&stored, actor_url_str) {
            continue;
        }
        removed += db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM federation_follows WHERE id = $1",
                [id.into()],
            ))
            .await?
            .rows_affected();
    }
    Ok(removed)
}

/// 处理内容类 Activity（Create/Update/Delete/Announce/Like）
pub(crate) async fn handle_content_activity(
    db: &impl ConnectionTrait,
    local_user_id: i32,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
    remote: Option<&RemoteActorInfo>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // 签名只证明"请求由 activity.actor 的公钥签出"。要动对象，还得证明这个
    // Actor 有权动它 —— 否则任意联邦实例都能伪造他人内容或删除他人条目。
    //
    // Create/Update：attributedTo 必须是签名 Actor，对象 id 必须同源。
    // Delete：对象通常已压缩成裸 IRI，只能做同源判断，真正的所有权在下面的
    // SQL 里用 remote_actor_id 再收一次。
    // Announce/Like：对象本来就是别人的，不适用。
    let ownership = match activity_type {
        "Create" | "Update" => Some(crate::federation::audience::verify_object_ownership(
            actor_url_str,
            &activity["object"],
        )),
        "Delete" => Some(crate::federation::audience::verify_object_same_origin(
            actor_url_str,
            &activity["object"],
        )),
        _ => None,
    };
    if let Some(Err(e)) = ownership {
        tracing::warn!(
            actor = %actor_url_str,
            activity_type,
            "Content activity rejected by ownership check: {}",
            e
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Object ownership check failed"})),
        ));
    }

    let remote = remote.ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Content actor preflight was not completed"})),
        )
    })?;

    let activity_id = activity["id"].as_str().unwrap_or("").to_string();
    let object_type = activity["object"]["type"].as_str().map(|s| s.to_string());

    // 记录 Activity
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_activities
               (activity_id, remote_actor_id, activity_type, object_type, object_json, is_local, received_at, published_at)
           VALUES ($1, $2, $3, $4, $5, false, NOW(), NOW())
           ON CONFLICT (activity_id) DO NOTHING"#,
        [
            activity_id.into(),
            remote.id.into(),
            activity_type.into(),
            object_type.clone().into(),
            activity["object"].clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // Delete: soft-remove prior Create of the same object from this user's timeline
    if activity_type == "Delete" {
        let object_id = activity["object"]["id"]
            .as_str()
            .or_else(|| activity["object"].as_str())
            .unwrap_or("");
        if !object_id.is_empty() {
            // `remote_actor_id` 约束是关键：没有它，任何持有效签名的远端都能
            // 用任意 object id 删掉目标用户时间线里**别人**的条目。
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"DELETE FROM federation_timeline
                   WHERE user_id = $1
                     AND remote_actor_id = $3
                     AND (
                       content_json->>'id' = $2
                       OR activity_id = $2
                       OR content_json #>> '{object,id}' = $2
                     )"#,
                [local_user_id.into(), object_id.into(), remote.id.into()],
            ))
            .await
            .map_err(db_err)?;
        }
        return Ok(StatusCode::ACCEPTED);
    }

    // Like: record activity only — do not pollute home feed (counts via activities).
    if activity_type == "Like" {
        crate::federation::interactions::handle_inbound_like(
            db,
            local_user_id,
            actor_url_str,
            activity,
        )
        .await;
        return Ok(StatusCode::ACCEPTED);
    }

    // 添加到 Timeline — prefer plain source.content for Note objects
    // Announce / Create / Update land on the feed.
    let preview = timeline_preview_from_object(&activity["object"]);

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
           ON CONFLICT (user_id, activity_id) DO NOTHING"#,
        [
            local_user_id.into(),
            activity["id"].as_str().unwrap_or("").into(),
            remote.id.into(),
            activity_type.into(),
            object_type.into(),
            preview.into(),
            activity["object"].clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    Ok(StatusCode::ACCEPTED)
}

/// Plain timeline preview from an AP object (Note prefers source.content).
fn timeline_preview_from_object(object: &serde_json::Value) -> Option<String> {
    object
        .pointer("/source/content")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("content").and_then(|v| v.as_str()))
        .or_else(|| object.get("summary").and_then(|v| v.as_str()))
        .or_else(|| object.get("content_preview").and_then(|v| v.as_str()))
        .or_else(|| object.get("mfp:contentPreview").and_then(|v| v.as_str()))
        .or_else(|| object.get("name").and_then(|v| v.as_str()))
        .map(|s| {
            let plain = s
                .replace("<p>", "")
                .replace("</p>", "")
                .replace("<br>", " ")
                .replace("<br/>", " ")
                .replace("<br />", " ")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
                .replace("&quot;", "\"");
            plain.chars().take(200).collect::<String>()
        })
        .filter(|s| !s.trim().is_empty())
}

/// 留存群邻实例的公开帖，即使本地没有任何人关注作者。
///
/// 共享收件箱此前只做粉丝分发：没有本地粉丝的公开帖验签通过后就被丢弃，连
/// `federation_activities` 都不落。群邻语义要的正是这批 —— 同群不同实例、
/// 互相没关注的用户，他们的帖子必须留存，Aro 首页才有东西可查。
///
/// 闸门层层收紧：签名与信任策略在 `post_shared_inbox` 已过；
/// `may_distribute_to_followers` 挡掉定向给个人的活动；本函数再加两道 ——
/// 必须寻址到 Public，且作者 domain 必须是群邻。不是群邻的实例照旧丢弃，
/// 共享收件箱不因此变成开放中继。
///
/// 只写 `federation_activities`，不写 `federation_timeline`：后者是「订阅」
/// 语义，按关注关系投递，群邻帖子进去会污染每个人的订阅页。
pub(crate) async fn record_room_peer_activity<C: ConnectionTrait>(
    db: &C,
    remote_actor_id: i32,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let activity_id = activity["id"].as_str().unwrap_or("");
    if activity_id.is_empty() {
        return Ok(());
    }

    // `may_distribute_to_followers` also passes activities addressed only to the
    // author's own followers collection. Those are legitimate for follower fan-out
    // but have no business being retained here: the rooms feed only ever surfaces
    // Public-addressed rows, so storing them would be retention without a reader.
    let addressed_to_public = crate::federation::audience::collect_recipients(activity)
        .iter()
        .any(|r| crate::federation::audience::is_public_address(r));
    if !addressed_to_public {
        return Ok(());
    }

    let base_url = get_base_url().await;
    let local_domain = extract_domain(&base_url).unwrap_or_default();
    let actor_domain = extract_domain(actor_url_str).unwrap_or_default();
    match crate::federation::room_peers::is_room_peer_domain(db, &local_domain, &actor_domain).await
    {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(e) => {
            // 判定失败按「不是群邻」处理：宁可首页少一条，也不放行未经判定的来源。
            tracing::warn!(
                actor = %actor_url_str,
                error = %e,
                "Room-peer check failed; dropping public activity from shared inbox"
            );
            return Ok(());
        }
    }

    let object_type = activity["object"]["type"].as_str().map(|s| s.to_string());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_activities
               (activity_id, remote_actor_id, activity_type, object_type, object_json, is_local, received_at, published_at)
           VALUES ($1, $2, $3, $4, $5, false, NOW(), NOW())
           ON CONFLICT (activity_id) DO NOTHING"#,
        [
            activity_id.into(),
            remote_actor_id.into(),
            activity_type.into(),
            object_type.into(),
            activity["object"].clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    Ok(())
}

/// 将共享收件箱的活动分发给所有关注该 Actor 的本地用户
pub(crate) async fn distribute_to_followers(
    db: &impl ConnectionTrait,
    remote_actor_id: i32,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // Likes are recorded in federation_activities only — not home feed.
    if activity_type == "Like" {
        return Ok(());
    }

    let activity_id_str = activity["id"].as_str().unwrap_or("");
    let object_type = activity["object"]["type"].as_str().map(|s| s.to_string());
    let preview = timeline_preview_from_object(&activity["object"]);

    // 批量 INSERT — 一次 SQL 分发到所有关注者的时间线，避免 N+1
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           SELECT f.user_id, $1, $2, $3, $4, $5, $6, NOW()
           FROM federation_follows f
           WHERE f.remote_actor_id = $2 AND f.direction = 'outgoing' AND f.status = 'accepted'
           -- 去重由 (user_id, activity_id) 唯一索引保证。原先的 NOT EXISTS
           -- 是先查后插：同一条活动并发送达时，两次扇出可以同时通过检查。
           ON CONFLICT (user_id, activity_id) DO NOTHING"#,
        [
            activity_id_str.into(),
            remote_actor_id.into(),
            activity_type.into(),
            object_type.into(),
            preview.into(),
            activity["object"].clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    Ok(())
}

async fn get_username_by_id(
    db: &impl ConnectionTrait,
    user_id: i32,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
        })?;

    Ok(row.try_get("", "username").unwrap_or_default())
}

#[cfg(test)]
mod tests;
