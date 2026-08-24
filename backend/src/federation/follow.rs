//! 联邦关注管理（Layer 2）
//!
//! 本地用户发起关注远程 Actor、取消关注等操作

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::federation::actor::fetch_remote_actor;
use crate::federation::types::*;

/// 关注远程用户请求
#[derive(Debug, Deserialize)]
pub struct FollowRequest {
    /// 远程 Actor URL 或 acct:user@domain / @user@domain 格式
    pub target: String,
}

/// 关注响应
#[derive(Debug, Serialize)]
pub struct FollowResponse {
    pub status: String,
    pub target_actor: String,
    pub activity_id: String,
}

/// 发起关注远程用户
///
/// POST /api/federation/follow
pub async fn follow_remote(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    target: &str,
) -> Result<FollowResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 解析目标：支持 acct:user@domain / @user@domain 和直接 URL
    let target_url = resolve_actor_reference(target).await?;
    let local_actor = actor_url(&base_url, username);
    if same_actor_url(&target_url, &local_actor) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot follow your own federation actor"})),
        ));
    }

    // 获取远程 Actor 信息
    let remote = fetch_remote_actor(db, &target_url).await.map_err(|e| {
        tracing::warn!(error = %e, "Failed to resolve remote actor");
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Cannot resolve remote actor",
                "code": "remote_actor_unresolved",
            })),
        )
    })?;

    // 检查是否已关注
    let existing = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT id, status FROM federation_follows
               WHERE user_id = $1 AND remote_actor_id = $2 AND direction = 'outgoing'"#,
            [user_id.into(), remote.id.into()],
        ))
        .await
        .map_err(db_err)?;

    if let Some(row) = existing {
        let status: String = row.try_get("", "status").unwrap_or_default();
        if status == "accepted" || status == "pending" {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "Already following or pending", "status": status})),
            ));
        }
    }

    // 构造 Follow Activity
    let activity_id = generate_activity_id(&base_url);

    let follow_activity = serde_json::json!({
        "@context": build_ap_context(),
        "type": "Follow",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&target_url],
        "published": now_iso8601(),
        "object": &target_url
    });

    // 记录 outgoing follow
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_follows (user_id, remote_actor_id, direction, status, activity_id, created_at)
           VALUES ($1, $2, 'outgoing', 'pending', $3, NOW())
           ON CONFLICT (user_id, remote_actor_id, direction) DO UPDATE SET
               status = 'pending', activity_id = $3"#,
        [
            user_id.into(),
            remote.id.into(),
            activity_id.clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // Defense-in-depth: ensure keys before enqueue. Delivery worker is the
    // universal choke point and will also ensure-once if keys are still missing.
    if let Err(e) =
        crate::federation::actor::ensure_user_federation_keys(db, user_id, username).await
    {
        tracing::warn!(
            user_id = user_id,
            username = %username,
            error = %e,
            "Failed to ensure federation keys before Follow enqueue; delivery may ensure later"
        );
    }

    // 存 Activity 记录（完整 Activity JSON，供 delivery 直接发送）
    let act_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Follow', $3, true, NOW())
               RETURNING id"#,
            [
                activity_id.clone().into(),
                user_id.into(),
                follow_activity.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_db_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    let domain = extract_domain(&remote.inbox_url).unwrap_or_default();

    // Same-instance Follow: deliver in-process. HTTP delivery refuses
    // localhost/private inboxes, so without this the followee never records
    // the follower and never emits Accept — initiator stays pending forever
    // while (if HTTP somehow worked one-way) the remote side looks accepted.
    let mut final_status = "pending".to_string();
    if let Some(target_username) = local_username_from_actor_url(&base_url, &target_url) {
        match crate::federation::inbox::deliver_activity_locally(
            db,
            &target_username,
            &follow_activity,
        )
        .await
        {
            Ok(()) => {
                tracing::info!(
                    "📬 Follow delivered locally: {} → {}",
                    username,
                    target_username
                );
                let _ = db
                    .execute_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at, last_attempt_at)
                           VALUES ($1, $2, $3, 'delivered', NOW(), NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                        [
                            act_db_id.into(),
                            remote.inbox_url.clone().into(),
                            domain.clone().into(),
                        ],
                    ))
                    .await;
                // Local auto-Accept flips our outgoing row to accepted immediately.
                if let Ok(Some(row)) = db
                    .query_one_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"SELECT status FROM federation_follows
                           WHERE user_id = $1 AND remote_actor_id = $2 AND direction = 'outgoing'"#,
                        [user_id.into(), remote.id.into()],
                    ))
                    .await
                {
                    let s: String = row.try_get("", "status").unwrap_or_default();
                    if !s.is_empty() {
                        final_status = s;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Local Follow delivery failed ({} → {}): {}; queueing HTTP",
                    username,
                    target_username,
                    e
                );
                db.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                           (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                    [
                        act_db_id.into(),
                        remote.inbox_url.clone().into(),
                        domain.into(),
                    ],
                ))
                .await
                .map_err(db_err)?;
            }
        }
    } else {
        // 远程：入队投递
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
               VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
            [
                act_db_id.into(),
                remote.inbox_url.clone().into(),
                domain.into(),
            ],
        ))
        .await
        .map_err(db_err)?;
        tracing::info!("📤 Follow queued: {} → {}", username, target_url);
    }

    Ok(FollowResponse {
        status: final_status,
        target_actor: target_url,
        activity_id,
    })
}

/// 取消关注远程用户
///
/// POST /api/federation/unfollow
pub async fn unfollow_remote(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    target: &str,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let target_url = resolve_actor_reference(target).await?;

    // 查找关注关系。
    //
    // 精确匹配是快路径（actor_url 有唯一约束）。未命中时按 same_actor_url 再扫一遍
    // 该用户的 outgoing 关注：follow 与 unfollow 都走 resolve_actor_reference，但
    // 用户两次输入的写法可能差一个末尾斜杠或 host 大小写 —— 那样 UI 明明显示
    // 「已关注」，取关却回 404。
    let follow_row = match db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT f.id, f.activity_id, ra.inbox_url, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = 'outgoing' AND ra.actor_url = $2"#,
            [user_id.into(), target_url.clone().into()],
        ))
        .await
        .map_err(db_err)?
    {
        Some(row) => row,
        None => find_outgoing_follow_normalized(db, user_id, &target_url)
            .await
            .map_err(db_err)?
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "Follow relationship not found"})),
                )
            })?,
    };

    let follow_activity_id: String = follow_row.try_get("", "activity_id").unwrap_or_default();
    let inbox: String = follow_row.try_get("", "inbox_url").unwrap_or_default();
    // Delete by the URL form actually stored, not the one the caller typed.
    let stored_actor_url: String = follow_row
        .try_get("", "actor_url")
        .unwrap_or_else(|_| target_url.clone());

    // 构造 Undo(Follow) Activity
    let local_actor = actor_url(&base_url, username);
    let undo_id = generate_activity_id(&base_url);

    let undo_activity = serde_json::json!({
        "@context": build_ap_context(),
        "type": "Undo",
        "id": undo_id,
        "actor": local_actor,
        "object": {
            "type": "Follow",
            "id": follow_activity_id,
            "actor": local_actor,
            "object": target_url
        }
    });

    // 删除本地关注记录
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"DELETE FROM federation_follows
           WHERE user_id = $1 AND direction = 'outgoing'
           AND remote_actor_id IN (SELECT id FROM federation_remote_actors WHERE actor_url = $2)"#,
        [user_id.into(), stored_actor_url.into()],
    ))
    .await
    .map_err(db_err)?;

    // 存 Undo Activity 并入队投递（完整 Activity JSON，供 delivery 直接发送）
    let act_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Undo', $3, true, NOW())
               RETURNING id"#,
            [
                undo_id.clone().into(),
                user_id.into(),
                undo_activity.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_db_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    let domain = extract_domain(&inbox).unwrap_or_default();
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
               VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
            [act_db_id.into(), inbox.into(), domain.into()],
        ))
        .await;

    Ok(json!({"status": "unfollowed", "target": target_url}))
}

/// Find this user's outgoing follow for `target_url` comparing with
/// [`same_actor_url`] rather than byte equality.
///
/// Fallback for the exact-match lookup: scoped to one user's outgoing follows,
/// which is a handful of rows even for heavy accounts.
async fn find_outgoing_follow_normalized(
    db: &DatabaseConnection,
    user_id: i32,
    target_url: &str,
) -> Result<Option<sea_orm::QueryResult>, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT f.id, f.activity_id, ra.inbox_url, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = 'outgoing'"#,
            [user_id.into()],
        ))
        .await?;
    Ok(rows.into_iter().find(|row| {
        row.try_get::<String>("", "actor_url")
            .map(|stored| same_actor_url(&stored, target_url))
            .unwrap_or(false)
    }))
}

/// 解析 Actor 引用：支持直接 Actor URL 或 acct:user@domain / @user@domain / user@domain。
pub async fn resolve_actor_reference(
    reference: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Actor reference is required"})),
        ));
    }

    if trimmed.starts_with("acct:")
        || (trimmed.contains('@')
            && !trimmed.starts_with("http://")
            && !trimmed.starts_with("https://"))
    {
        return resolve_acct_to_url(trimmed).await;
    }

    Ok(trimmed.to_string())
}

/// WebFinger 查询：acct:user@domain → Actor URL
///
/// HTTPS 优先。本地联邦 lab（`MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND=1`）下实例
/// 通常只监听明文 HTTP，因此再回退一次 `http://`——否则 handle 输入会在 TLS
/// 握手阶段失败并被映射成 502，而同一 Actor 的 profile URL（自带 scheme）却能用。
/// 与 [`room::members::fetch_remote_public_room`] 的 scheme 回退保持一致。
async fn resolve_acct_to_url(acct: &str) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let candidates = build_webfinger_url_candidates(acct)?;
    let mut last_err: Option<(StatusCode, Json<serde_json::Value>)> = None;

    for candidate in candidates {
        match webfinger_lookup_once(&candidate).await {
            Ok(actor_url) => return Ok(actor_url),
            // A definite "that instance has no such account" outranks a later
            // transport failure on the fallback scheme — reporting the refused
            // connection instead would hide the answer we actually got.
            Err(err) => {
                let definitive = err.0 == StatusCode::NOT_FOUND;
                last_err = Some(err);
                if definitive {
                    break;
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "WebFinger lookup failed"})),
        )
    }))
}

/// WebFinger 候选 URL：HTTPS 优先，lab 模式下追加 HTTP。
fn build_webfinger_url_candidates(
    acct: &str,
) -> Result<Vec<String>, (StatusCode, Json<serde_json::Value>)> {
    let https_url = build_webfinger_url(acct)?;
    let mut candidates = vec![https_url.clone()];

    if crate::services::outbound_security::federation_lab_private_outbound_enabled() {
        if let Some(rest) = https_url.strip_prefix("https://") {
            candidates.push(format!("http://{}", rest));
        }
    }

    Ok(candidates)
}

/// 单次 WebFinger 请求：JRD → rel=self 的 Actor URL
async fn webfinger_lookup_once(
    webfinger_url: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    // 防止 SSRF：验证 WebFinger URL 不指向内网
    if is_internal_url(webfinger_url) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot resolve internal domains"})),
        ));
    }

    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        webfinger_url,
        std::time::Duration::from_secs(10),
        None,
    )
    .await
    .map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot resolve unsafe domains"})),
        )
    })?;

    // Some instances only answer `application/json` (or negotiate poorly on an
    // unknown Accept); RFC 7033 §10.2 names jrd+json, so offer both.
    let resp = client
        .get(target_url)
        .header("Accept", "application/jrd+json, application/json;q=0.9")
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(
                webfinger_url = %webfinger_url,
                error = %e,
                "WebFinger request failed"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "WebFinger lookup failed",
                    "code": "webfinger_failed",
                })),
            )
        })?;

    let status = resp.status();
    if !status.is_success() {
        // 404/410 是「那个实例上没有这个账号」，不是网关故障。以前一律 502，
        // 用户看到的是含糊的 Bad Gateway，而真正该说的是「handle 拼错了 / 对方
        // 实例不认这个账号」。其余非 2xx 仍然算上游异常。
        let mapped = if matches!(status, StatusCode::NOT_FOUND | StatusCode::GONE) {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_GATEWAY
        };
        tracing::warn!(
            webfinger_url = %webfinger_url,
            status = status.as_u16(),
            "WebFinger returned non-success status"
        );
        return Err((
            mapped,
            Json(json!({
                "error": if mapped == StatusCode::NOT_FOUND {
                    "No such account at that instance"
                } else {
                    "WebFinger lookup failed"
                },
                "code": if mapped == StatusCode::NOT_FOUND {
                    "webfinger_not_found"
                } else {
                    "webfinger_failed"
                },
            })),
        ));
    }

    // JRD 文档很小；64KB 上限防止恶意/异常实例撑爆内存
    let body = crate::services::outbound_security::read_limited_body(resp, 64 * 1024)
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "WebFinger response too large or unreadable"})),
            )
        })?;
    let wf: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        // Almost always an SPA index.html: the reverse proxy did not route
        // /.well-known/webfinger to the backend. Say so instead of "invalid".
        tracing::warn!(
            webfinger_url = %webfinger_url,
            "WebFinger response was not JSON (is /.well-known/webfinger routed to the backend?)"
        );
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": "WebFinger is not available on that host",
                "code": "webfinger_unavailable",
            })),
        )
    })?;

    // 找到 rel=self, type=application/activity+json 的链接
    let links = wf["links"].as_array().ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": "WebFinger lookup failed",
                "code": "webfinger_failed",
            })),
        )
    })?;

    for link in links {
        let rel = link["rel"].as_str().unwrap_or("");
        let ltype = link["type"].as_str().unwrap_or("");
        if rel == "self" && (ltype == AP_CONTENT_TYPE || ltype.contains("activity+json")) {
            if let Some(href) = link["href"].as_str() {
                return Ok(href.to_string());
            }
        }
    }

    Err((
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": "WebFinger lookup failed",
            "code": "webfinger_failed",
        })),
    ))
}

fn build_webfinger_url(acct: &str) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let trimmed = acct.trim();
    let without_scheme = trimmed.strip_prefix("acct:").unwrap_or(trimmed);
    let stripped = without_scheme.strip_prefix('@').unwrap_or(without_scheme);
    let parts: Vec<&str> = stripped.splitn(2, '@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid acct format"})),
        ));
    }

    let domain = parts[1].trim();
    if domain.is_empty()
        || domain.contains('/')
        || domain.contains('?')
        || domain.contains('#')
        || domain.contains('@')
        || domain.chars().any(char::is_whitespace)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid acct domain"})),
        ));
    }

    let resource = format!("acct:{}", stripped);
    url::Url::parse_with_params(
        &format!("https://{}/.well-known/webfinger", domain),
        &[("resource", resource.as_str())],
    )
    .map(|u| u.to_string())
    .map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid acct domain"})),
        )
    })
}

// 辅助函数

async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_webfinger_url_encodes_acct_resource() {
        let url = build_webfinger_url("alice@example.com").unwrap();
        assert_eq!(
            url,
            "https://example.com/.well-known/webfinger?resource=acct%3Aalice%40example.com"
        );
    }

    #[test]
    fn build_webfinger_url_accepts_domain_port() {
        let url = build_webfinger_url("acct:alice@example.com:8443").unwrap();
        assert_eq!(
            url,
            "https://example.com:8443/.well-known/webfinger?resource=acct%3Aalice%40example.com%3A8443"
        );
    }

    #[test]
    fn build_webfinger_url_accepts_display_handle() {
        let url = build_webfinger_url("@alice@example.com").unwrap();
        assert_eq!(
            url,
            "https://example.com/.well-known/webfinger?resource=acct%3Aalice%40example.com"
        );
    }

    #[tokio::test]
    async fn webfinger_candidates_https_only_without_lab_flag() {
        let _guard = crate::services::outbound_security::tests_lab_env_lock().await;
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        let candidates = build_webfinger_url_candidates("alice@example.com").unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].starts_with("https://example.com/"));
    }

    #[tokio::test]
    async fn webfinger_candidates_add_http_fallback_in_lab() {
        let _guard = crate::services::outbound_security::tests_lab_env_lock().await;
        std::env::remove_var("ENVIRONMENT");
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");
        let candidates = build_webfinger_url_candidates("alice@127.0.0.1:1103").unwrap();
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        assert_eq!(candidates.len(), 2);
        assert!(candidates[0].starts_with("https://127.0.0.1:1103/"));
        assert!(candidates[1].starts_with("http://127.0.0.1:1103/"));
        // Both must carry the same encoded acct resource.
        assert!(candidates[1].contains("resource=acct%3Aalice%40127.0.0.1%3A1103"));
    }

    #[test]
    fn build_webfinger_url_rejects_invalid_domain() {
        assert!(build_webfinger_url("acct:alice@example.com/path").is_err());
        assert!(build_webfinger_url("acct:alice@example.com?x=1").is_err());
        assert!(build_webfinger_url("acct:alice@").is_err());
    }

    #[test]
    fn build_webfinger_url_rejects_domain_whitespace() {
        assert!(build_webfinger_url("alice@ex ample.com").is_err());
        assert!(
            build_webfinger_url("alice@example.com ").is_ok()
                || build_webfinger_url(" alice@example.com ").is_ok()
        );
        // domain internal whitespace must fail
        assert!(build_webfinger_url("alice@exam	ple.com").is_err());
    }

    #[test]
    fn build_webfinger_url_rejects_hash_and_at_in_domain() {
        assert!(build_webfinger_url("alice@example.com#x").is_err());
        assert!(build_webfinger_url("alice@ex@ample.com").is_err());
    }

    #[test]
    fn build_webfinger_url_trims_outer_whitespace() {
        let url = build_webfinger_url("  alice@example.com  ").unwrap();
        assert!(url.starts_with("https://example.com/.well-known/webfinger"));
        assert!(url.contains("resource=acct%3Aalice%40example.com"));
    }

    #[test]
    fn build_webfinger_url_rejects_empty_user_or_domain() {
        assert!(build_webfinger_url("@example.com").is_err());
        assert!(build_webfinger_url("alice@").is_err());
        assert!(build_webfinger_url("alice").is_err());
        assert!(build_webfinger_url("").is_err());
    }

    #[test]
    fn build_webfinger_url_rejects_empty_local_or_domain() {
        assert!(build_webfinger_url("@example.com").is_err());
        assert!(build_webfinger_url("alice@").is_err());
        assert!(build_webfinger_url("alice").is_err());
        assert!(build_webfinger_url("").is_err());
        assert!(build_webfinger_url("   ").is_err());
        assert!(build_webfinger_url("acct:@example.com").is_err());
    }

    #[test]
    fn build_webfinger_url_rejects_whitespace_in_domain() {
        assert!(build_webfinger_url("alice@exam ple.com").is_err());
        assert!(build_webfinger_url("alice@example.com#frag").is_err());
        assert!(build_webfinger_url("alice@example.com@evil").is_err());
    }

    #[test]
    fn r53_build_webfinger_url_basic_acct() {
        let url = build_webfinger_url("alice@example.com").unwrap();
        assert!(url.starts_with("https://example.com/.well-known/webfinger?"));
        assert!(url.contains("resource=acct%3Aalice%40example.com"));
    }

    #[test]
    fn r54_build_webfinger_url_rejects_path_in_domain() {
        assert!(build_webfinger_url("alice@example.com/x").is_err());
        assert!(build_webfinger_url("alice@exam ple.com").is_err());
    }

    #[test]
    fn r55_build_webfinger_url_accepts_at_prefix() {
        let url = build_webfinger_url("@bob@remote.example").unwrap();
        assert!(url.contains("remote.example"));
        assert!(url.contains("acct%3Abob%40remote.example"));
    }
}
