//! 联邦 Ring 管理模块（Phase 5 — Layer 3）
//!
//! 去中心化环网：Tapp 商店发现、Brew 推荐交换、Library 交换圈、实例目录
//! 基于 Gossip 协议进行对等同步，每个节点维护 known_peers 列表

#![allow(dead_code)]

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::federation::types::*;

/// Ensure signing keys before ring enqueue (defense-in-depth).
///
/// Delivery worker remains the universal choke point for already-queued rows;
/// this avoids cold-key races on ring create/add_peer/leave/sync (same trap as
/// room join in production logs).
async fn ensure_keys_before_ring_outbound(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    context: &str,
) {
    if username.trim().is_empty() || user_id <= 0 {
        return;
    }
    if let Err(e) =
        crate::federation::actor::ensure_user_federation_keys(db, user_id, username).await
    {
        tracing::warn!(
            user_id = user_id,
            username = %username,
            context = %context,
            error = %e,
            "Failed to ensure federation keys before ring outbound; delivery may ensure later"
        );
    }
}

/// 根据用户名查询实际的 user_id，避免硬编码 user_id = 1
async fn resolve_user_id(
    db: &DatabaseConnection,
    username: &str,
) -> Result<i32, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE username = $1",
            [username.into()],
        ))
        .await
        .map_err(db_err)?;

    match row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
        Some(uid) => Ok(uid),
        None => {
            // 回退：单用户实例可能用户名不匹配，取第一个用户
            let fallback = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id FROM users ORDER BY id LIMIT 1",
                    [],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "No local users found"})),
                    )
                })?;
            fallback.try_get("", "id").map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e.to_string()})),
                )
            })
        }
    }
}

// 请求/响应类型

/// 创建 / 加入 Ring 请求
#[derive(Debug, Deserialize)]
pub struct CreateRingRequest {
    /// Ring 名称
    pub name: String,
    /// Ring 类型: tapp-store, brew-recommend, library-exchange, instance-directory
    pub ring_type: String,
    /// Gossip fanout（每次传播给几个 peer）
    pub fanout: Option<u32>,
    /// Gossip TTL（跳数）
    pub ttl: Option<u32>,
    /// 同步间隔（秒）
    pub interval: Option<u64>,
    /// Optional brew category filter (brew-recommend only).
    /// When set, only sources/items under this category name are synced.
    /// Accepts either `category` or `brew_category` in JSON.
    #[serde(default, alias = "brew_category")]
    pub category: Option<String>,
}

/// Ring 概要
#[derive(Debug, Serialize)]
pub struct RingSummary {
    pub ring_id: String,
    pub ring_name: Option<String>,
    pub ring_type: String,
    pub peer_count: i64,
    pub last_sync_at: Option<String>,
    pub joined_at: String,
}

/// Ring 详情
#[derive(Debug, Serialize)]
pub struct RingDetail {
    pub ring_id: String,
    pub ring_name: Option<String>,
    pub ring_type: String,
    pub gossip_config: Option<serde_json::Value>,
    pub known_peers: Vec<String>,
    pub last_sync_at: Option<String>,
    pub joined_at: String,
}

/// Peer 信息
#[derive(Debug, Serialize)]
pub struct RingPeer {
    pub actor_url: String,
    pub instance_domain: String,
    pub added_at: String,
}

/// 同步数据载荷
#[derive(Debug, Deserialize)]
pub struct SyncDataRequest {
    /// 携带的数据条目（取决于 ring_type）
    pub entries: Vec<serde_json::Value>,
    /// 来源 peer
    pub origin_peer: Option<String>,
    /// 剩余 TTL
    pub ttl: Option<u32>,
}

/// 添加 Peer 请求
#[derive(Debug, Deserialize)]
pub struct AddPeerRequest {
    /// 远程 Actor URL 或 acct:user@domain / @user@domain / user@domain
    pub peer: String,
}

// 辅助函数

fn validate_ring_type(rt: &str) -> bool {
    [
        "tapp-store",
        "brew-recommend",
        "library-exchange",
        "instance-directory",
    ]
    .contains(&rt)
}

/// Match a brew source `category` field against a single category name.
/// Supports multi-category values (comma-separated), same rules as brew API.
///
/// Patterns: exact | starts with `"name, "` | ends with `", name"` | contains `", name, "`.
pub fn source_category_matches(source_category: &str, category_name: &str) -> bool {
    let cat = category_name.trim();
    if cat.is_empty() {
        return false;
    }
    let sc = source_category.trim();
    if sc.is_empty() {
        return false;
    }
    sc == cat
        || sc.starts_with(&format!("{}, ", cat))
        || sc.ends_with(&format!(", {}", cat))
        || sc.contains(&format!(", {}, ", cat))
}

/// True if source category matches any of the given category names.
pub fn source_matches_any_category(source_category: Option<&str>, categories: &[String]) -> bool {
    let Some(sc) = source_category.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    categories.iter().any(|c| source_category_matches(sc, c))
}

/// Max brew items to push per brew-recommend ring sync.
const BREW_RING_ITEM_LIMIT: u64 = 40;

// Ring CRUD

/// 创建新 Ring（本地节点作为创建者）
pub async fn create_ring(
    _user_id: i32,
    db: &DatabaseConnection,
    req: &CreateRingRequest,
) -> Result<RingDetail, (StatusCode, Json<serde_json::Value>)> {
    if !validate_ring_type(&req.ring_type) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": "Invalid ring_type. Must be one of: tapp-store, brew-recommend, library-exchange, instance-directory"}),
            ),
        ));
    }

    let ring_id = generate_ring_id();
    let mut gossip_config = json!({
        "fanout": req.fanout.unwrap_or(3),
        "ttl": req.ttl.unwrap_or(5),
        "interval": req.interval.unwrap_or(300)
    });
    // Persist optional brew category filter for brew-recommend rings
    if req.ring_type == "brew-recommend" {
        if let Some(cat) = req
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            gossip_config["category"] = json!(cat);
        }
    }

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_ring_memberships
           (ring_id, ring_name, ring_type, gossip_config, known_peers, joined_at)
           VALUES ($1, $2, $3, $4, '[]'::json, NOW())"#,
        [
            ring_id.clone().into(),
            req.name.clone().into(),
            req.ring_type.clone().into(),
            gossip_config.clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    tracing::info!("[Ring] Created ring {} type={}", ring_id, req.ring_type);

    Ok(RingDetail {
        ring_id,
        ring_name: Some(req.name.clone()),
        ring_type: req.ring_type.clone(),
        gossip_config: Some(gossip_config),
        known_peers: vec![],
        last_sync_at: None,
        joined_at: now_iso8601(),
    })
}

/// 列出本节点加入的所有 Ring
pub async fn list_rings(
    db: &DatabaseConnection,
) -> Result<Vec<RingSummary>, (StatusCode, Json<serde_json::Value>)> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ring_id, ring_name, ring_type, known_peers, last_sync_at, joined_at
               FROM federation_ring_memberships
               ORDER BY joined_at DESC"#,
            [],
        ))
        .await
        .map_err(db_err)?;

    let mut rings = Vec::new();
    for row in rows {
        let peers: serde_json::Value = row.try_get("", "known_peers").unwrap_or(json!([]));
        let peer_count = peers.as_array().map(|a| a.len() as i64).unwrap_or(0);

        rings.push(RingSummary {
            ring_id: row.try_get("", "ring_id").unwrap_or_default(),
            ring_name: row
                .try_get::<Option<String>>("", "ring_name")
                .unwrap_or(None),
            ring_type: row.try_get("", "ring_type").unwrap_or_default(),
            peer_count,
            last_sync_at: row
                .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_sync_at")
                .ok()
                .flatten()
                .map(|t| t.to_rfc3339()),
            joined_at: row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "joined_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        });
    }

    Ok(rings)
}

/// 获取 Ring 详情
pub async fn get_ring(
    ring_id: &str,
    db: &DatabaseConnection,
) -> Result<RingDetail, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ring_id, ring_name, ring_type, gossip_config, known_peers, last_sync_at, joined_at
               FROM federation_ring_memberships
               WHERE ring_id = $1"#,
            [ring_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Ring not found"})),
            )
        })?;

    let peers_json: serde_json::Value = row.try_get("", "known_peers").unwrap_or(json!([]));
    let known_peers: Vec<String> = peers_json
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(RingDetail {
        ring_id: row.try_get("", "ring_id").unwrap_or_default(),
        ring_name: row
            .try_get::<Option<String>>("", "ring_name")
            .unwrap_or(None),
        ring_type: row.try_get("", "ring_type").unwrap_or_default(),
        gossip_config: row
            .try_get::<Option<serde_json::Value>>("", "gossip_config")
            .unwrap_or(None),
        known_peers,
        last_sync_at: row
            .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_sync_at")
            .ok()
            .flatten()
            .map(|t| t.to_rfc3339()),
        joined_at: row
            .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "joined_at")
            .map(|t| t.to_rfc3339())
            .unwrap_or_default(),
    })
}

/// 离开（删除）Ring
pub async fn leave_ring(
    ring_id: &str,
    username: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 确认 Ring 存在
    let ring_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT known_peers FROM federation_ring_memberships WHERE ring_id = $1",
            [ring_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Ring not found"})),
            )
        })?;

    let peers_json: serde_json::Value = ring_row.try_get("", "known_peers").unwrap_or(json!([]));
    let peers: Vec<String> = peers_json
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // 通知所有 peer 我们要离开
    let local_actor = actor_url(&base_url, username);
    let leave_base = json!({
        "@context": build_context(),
        "type": "myriad:RingLeave",
        "actor": &local_actor,
        "object": {
            "type": "myriad:Ring",
            "id": ring_id
        }
    });

    // 向每个 peer 投递离开通知
    let local_user_id = resolve_user_id(db, username).await?;
    ensure_keys_before_ring_outbound(db, local_user_id, username, "ring_leave").await;
    for peer in &peers {
        // 为每个 peer 生成独立的 activity_id，避免 DB 冲突
        let activity_id = generate_activity_id(&base_url);
        let mut leave_activity = leave_base.clone();
        leave_activity["id"] = json!(&activity_id);
        if let Ok(remote) = crate::federation::actor::fetch_remote_actor(db, peer).await {
            if !remote.inbox_url.is_empty() {
                let domain = extract_domain(&remote.inbox_url).unwrap_or_default();
                let act_row = db
                    .query_one_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_activities
                           (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                           VALUES ($1, $2, 'RingLeave', 'Ring', $3, true, NOW())
                           RETURNING id"#,
                        [activity_id.clone().into(), local_user_id.into(), leave_activity.clone().into()],
                    ))
                    .await
                    .map_err(db_err)?;

                if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
                    let _ = db
                        .execute_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at)
                               VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                            [act_id.into(), remote.inbox_url.into(), domain.into()],
                        ))
                        .await;
                }
            }
        }
    }

    // 删除本地记录
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_ring_memberships WHERE ring_id = $1",
        [ring_id.into()],
    ))
    .await
    .map_err(db_err)?;

    tracing::info!("[Ring] Left ring {}", ring_id);

    Ok(json!({"success": true, "ring_id": ring_id}))
}

// Peer 管理

/// 获取 Ring 的 Peer 列表
pub async fn get_peers(
    ring_id: &str,
    db: &DatabaseConnection,
) -> Result<Vec<RingPeer>, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT known_peers FROM federation_ring_memberships WHERE ring_id = $1",
            [ring_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Ring not found"})),
            )
        })?;

    let peers_json: serde_json::Value = row.try_get("", "known_peers").unwrap_or(json!([]));
    let mut result = Vec::new();
    if let Some(arr) = peers_json.as_array() {
        for v in arr {
            if let Some(url) = v.as_str() {
                let domain = extract_domain(url).unwrap_or_else(|| url.to_string());
                result.push(RingPeer {
                    actor_url: url.to_string(),
                    instance_domain: domain,
                    added_at: String::new(),
                });
            }
        }
    }

    Ok(result)
}

/// 添加 Peer 到 Ring
pub async fn add_peer(
    ring_id: &str,
    username: &str,
    db: &DatabaseConnection,
    req: &AddPeerRequest,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 确认 Ring 存在
    let ring_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT ring_type, known_peers, gossip_config FROM federation_ring_memberships WHERE ring_id = $1",
            [ring_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Ring not found"})),
            )
        })?;

    let peers_json: serde_json::Value = ring_row.try_get("", "known_peers").unwrap_or(json!([]));

    let peer_url = crate::federation::follow::resolve_actor_reference(&req.peer).await?;
    let local_actor = actor_url(&base_url, username);
    if same_actor_url(&peer_url, &local_actor) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot add this instance as its own ring peer"})),
        ));
    }

    // 验证远程 actor 存在
    let remote = crate::federation::actor::fetch_remote_actor(db, &peer_url)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Cannot resolve peer: {}", e)})),
            )
        })?;

    // 检查是否已存在
    if let Some(arr) = peers_json.as_array() {
        if arr.iter().any(|v| v.as_str() == Some(&peer_url)) {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "Peer already in ring"})),
            ));
        }
    }

    // 原子追加到 known_peers，避免并发读-改-写竞争
    // known_peers is json (not jsonb); cast for @> / || containment ops
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_ring_memberships
           SET known_peers = CASE
             WHEN NOT (COALESCE(known_peers, '[]'::json)::jsonb @> $2::jsonb)
             THEN (COALESCE(known_peers, '[]'::json)::jsonb || $2::jsonb)
             ELSE COALESCE(known_peers, '[]'::json)::jsonb
           END
           WHERE ring_id = $1"#,
        [ring_id.into(), json!([&peer_url]).into()],
    ))
    .await
    .map_err(db_err)?;

    // 发送 RingJoin Activity 通知新 peer
    let activity_id = generate_activity_id(&base_url);
    let join_activity = json!({
        "@context": build_context(),
        "type": "myriad:RingJoin",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&peer_url],
        "object": {
            "type": "myriad:Ring",
            "id": ring_id,
            "ringType": ring_row.try_get::<String>("", "ring_type").unwrap_or_default(),
        }
    });

    if !remote.inbox_url.is_empty() {
        let domain = extract_domain(&remote.inbox_url).unwrap_or_default();
        let local_user_id = resolve_user_id(db, username).await?;
        // Matches production log: add_peer enqueues without GET /users/{username}.
        ensure_keys_before_ring_outbound(db, local_user_id, username, "ring_add_peer").await;
        let act_row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'RingJoin', 'Ring', $3, true, NOW())
                   RETURNING id"#,
                [activity_id.clone().into(), local_user_id.into(), join_activity.clone().into()],
            ))
            .await
            .map_err(db_err)?;

        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            let _ = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                    [act_id.into(), remote.inbox_url.into(), domain.into()],
                ))
                .await;
        }
    }

    tracing::info!("[Ring] Added peer {} to ring {}", peer_url, ring_id);

    Ok(json!({"success": true, "ring_id": ring_id, "peer": peer_url}))
}

/// 移除 Peer（原子操作，避免并发读-改-写竞争）
pub async fn remove_peer(
    ring_id: &str,
    peer_url: &str,
    username: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    // 使用子查询原子地从 JSON 数组中移除指定 peer（cast to jsonb for ops）
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_ring_memberships
               SET known_peers = (
                   SELECT COALESCE(jsonb_agg(elem), '[]'::jsonb)
                   FROM jsonb_array_elements(COALESCE(known_peers, '[]'::json)::jsonb) AS elem
                   WHERE elem #>> '{}' != $2
               )
               WHERE ring_id = $1"#,
            [ring_id.into(), peer_url.into()],
        ))
        .await
        .map_err(db_err)?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Ring not found"})),
        ));
    }

    // Notify removed peer so they drop us from known_peers (was local-only)
    let local_actor = actor_url(&base_url, username);
    let local_user_id = resolve_user_id(db, username).await?;
    ensure_keys_before_ring_outbound(db, local_user_id, username, "ring_remove_peer").await;
    if let Ok(remote) = crate::federation::actor::fetch_remote_actor(db, peer_url).await {
        if !remote.inbox_url.is_empty() {
            let activity_id = generate_activity_id(&base_url);
            let leave_activity = json!({
                "@context": build_context(),
                "type": "myriad:RingLeave",
                "id": &activity_id,
                "actor": &local_actor,
                "object": {
                    "type": "myriad:Ring",
                    "id": ring_id,
                    "removedPeer": peer_url
                }
            });
            let domain = extract_domain(&remote.inbox_url).unwrap_or_default();
            if let Ok(Some(act_row)) = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_activities
                       (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                       VALUES ($1, $2, 'RingLeave', 'Ring', $3, true, NOW())
                       RETURNING id"#,
                    [
                        activity_id.into(),
                        local_user_id.into(),
                        leave_activity.into(),
                    ],
                ))
                .await
            {
                if let Ok(act_id) = act_row.try_get::<i32>("", "id") {
                    let _ = db
                        .execute_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at)
                               VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                            [act_id.into(), remote.inbox_url.into(), domain.into()],
                        ))
                        .await;
                }
            }
        }
    }

    tracing::info!("[Ring] Removed peer {} from ring {}", peer_url, ring_id);

    Ok(json!({"success": true, "ring_id": ring_id, "removed_peer": peer_url}))
}

// Gossip 同步

/// 触发 Gossip 同步：向随机 fanout 个 peer 推送本地数据
pub async fn trigger_sync(
    ring_id: &str,
    username: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let ring_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT ring_type, gossip_config, known_peers FROM federation_ring_memberships WHERE ring_id = $1",
            [ring_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Ring not found"})),
            )
        })?;

    let ring_type: String = ring_row.try_get("", "ring_type").unwrap_or_default();
    let gossip_config: serde_json::Value = ring_row
        .try_get("", "gossip_config")
        .unwrap_or(json!({"fanout": 3, "ttl": 5}));
    let peers_json: serde_json::Value = ring_row.try_get("", "known_peers").unwrap_or(json!([]));

    let fanout = gossip_config
        .get("fanout")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;
    let ttl = gossip_config
        .get("ttl")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as u32;

    let peers: Vec<String> = peers_json
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if peers.is_empty() {
        return Ok(json!({"success": true, "synced_peers": 0, "message": "No peers to sync with"}));
    }

    let local_user_id = resolve_user_id(db, username).await?;
    ensure_keys_before_ring_outbound(db, local_user_id, username, "ring_sync").await;
    let category_filter = gossip_config
        .get("category")
        .or_else(|| gossip_config.get("brew_category"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // 收集本地要同步的数据（根据 ring_type；brew-recommend 按用户分类）
    let entries = collect_sync_entries(
        &ring_type,
        db,
        local_user_id,
        username,
        category_filter.as_deref(),
    )
    .await;

    if entries.is_empty() {
        // 更新 last_sync_at
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE federation_ring_memberships SET last_sync_at = NOW() WHERE ring_id = $1",
                [ring_id.into()],
            ))
            .await;
        return Ok(json!({"success": true, "synced_peers": 0, "message": "No data to sync"}));
    }

    // 随机选 fanout 个 peer
    let selected: Vec<&String> = if peers.len() <= fanout {
        peers.iter().collect()
    } else {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let mut rng_state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as usize;
        while set.len() < fanout {
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
            set.insert(rng_state % peers.len());
        }
        set.iter().map(|&i| &peers[i]).collect()
    };

    let local_actor = actor_url(&base_url, username);
    let mut synced = 0;

    for peer in &selected {
        let peer_url = match crate::federation::follow::resolve_actor_reference(peer).await {
            Ok(url) => url,
            Err(_) => {
                tracing::warn!("[Ring] Skipping unresolved peer {} during sync", peer);
                continue;
            }
        };
        let activity_id = generate_activity_id(&base_url);
        let sync_activity = json!({
            "@context": build_context(),
            "type": "myriad:RingSync",
            "id": &activity_id,
            "actor": &local_actor,
            "to": [&peer_url],
            "object": {
                "type": "myriad:RingSyncPayload",
                "ring": ring_id,
                "ringType": &ring_type,
                "entries": &entries,
                "ttl": ttl.saturating_sub(1)
            }
        });

        if let Ok(remote) = crate::federation::actor::fetch_remote_actor(db, &peer_url).await {
            if !remote.inbox_url.is_empty() {
                let domain = extract_domain(&remote.inbox_url).unwrap_or_default();
                let act_row = db
                    .query_one_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_activities
                           (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                           VALUES ($1, $2, 'RingSync', 'Ring', $3, true, NOW())
                           RETURNING id"#,
                        [activity_id.clone().into(), local_user_id.into(), sync_activity.clone().into()],
                    ))
                    .await
                    .map_err(db_err)?;

                if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
                    let _ = db
                        .execute_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at)
                               VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                            [act_id.into(), remote.inbox_url.into(), domain.into()],
                        ))
                        .await;
                }
                synced += 1;
            }
        }
    }

    // 更新 last_sync_at
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE federation_ring_memberships SET last_sync_at = NOW() WHERE ring_id = $1",
            [ring_id.into()],
        ))
        .await;

    tracing::info!(
        "[Ring] Synced ring {} to {} peers ({} entries)",
        ring_id,
        synced,
        entries.len()
    );

    Ok(json!({
        "success": true,
        "synced_peers": synced,
        "entries_count": entries.len()
    }))
}

/// Legacy path: last N federated brew-article Creates (manual publish).
async fn collect_legacy_brew_activities(db: &DatabaseConnection) -> Vec<serde_json::Value> {
    // Prefer federation_published_content (stable content_type=brew-article) over
    // object_type on activities (which stores AP type "Article").
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT fpc.activity_id, fa.object_json
               FROM federation_published_content fpc
               LEFT JOIN federation_activities fa ON fa.activity_id = fpc.activity_id
               WHERE fpc.content_type = 'brew-article'
               ORDER BY fpc.published_at DESC
               LIMIT 20"#,
            [],
        ))
        .await
        .unwrap_or_default();

    if !rows.is_empty() {
        return rows
            .iter()
            .filter_map(|r| {
                let activity_id = r.try_get::<String>("", "activity_id").ok()?;
                let obj: serde_json::Value = r
                    .try_get("", "object_json")
                    .unwrap_or(json!({ "type": "Article" }));
                Some(json!({
                    "type": "brew",
                    "activity_id": activity_id,
                    "data": obj
                }))
            })
            .collect();
    }

    // Fallback: older rows may only exist on federation_activities
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT activity_id, object_json, object_type
               FROM federation_activities
               WHERE activity_type = 'Create'
                 AND is_local = true
                 AND (
                   object_type = 'brew-article'
                   OR object_json::text LIKE '%brew-article%'
                 )
               ORDER BY published_at DESC LIMIT 20"#,
            [],
        ))
        .await
        .unwrap_or_default();
    rows.iter()
        .filter_map(|r| {
            let obj: serde_json::Value = r.try_get("", "object_json").ok()?;
            let object_type = r.try_get::<String>("", "object_type").unwrap_or_default();
            let is_brew = object_type == "brew-article"
                || obj.get("mfp:contentType").and_then(|v| v.as_str()) == Some("brew-article")
                || obj
                    .pointer("/object/mfp:contentType")
                    .and_then(|v| v.as_str())
                    == Some("brew-article");
            if !is_brew {
                return None;
            }
            Some(json!({
                "type": "brew",
                "activity_id": r.try_get::<String>("", "activity_id").unwrap_or_default(),
                "data": obj
            }))
        })
        .collect()
}

/// Truncate summary text for ring payload (no secrets / no full content).
fn brew_ring_summary(summary: Option<&str>, content: Option<&str>) -> String {
    let raw = summary
        .filter(|s| !s.trim().is_empty())
        .or(content)
        .unwrap_or("")
        .chars()
        .take(500)
        .collect::<String>();
    // Strip crude HTML tags for a plain summary
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collect brew-recommend ring entries from user brew categories + sources.
/// When the user has no categories, falls back to legacy federated brew-article Creates.
async fn collect_brew_recommend_entries(
    db: &DatabaseConnection,
    user_id: i32,
    _username: &str,
    category_filter: Option<&str>,
) -> Vec<serde_json::Value> {
    // 1) Load user's category names
    let cat_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT name FROM brew_categories WHERE user_id = $1 ORDER BY sort_order ASC, id ASC",
            [user_id.into()],
        ))
        .await
        .unwrap_or_default();

    let mut category_names: Vec<String> = cat_rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "name").ok())
        .filter(|n| !n.trim().is_empty())
        .collect();

    // Optional filter: only that category (must still belong to the user)
    if let Some(filter) = category_filter.map(str::trim).filter(|s| !s.is_empty()) {
        category_names.retain(|n| n == filter);
        if category_names.is_empty() {
            // Filter set but not in user's categories → nothing to sync (not legacy fallback)
            tracing::debug!(
                "[Ring] brew-recommend category filter {:?} not in user {} categories",
                filter,
                user_id
            );
            return vec![];
        }
    }

    if category_names.is_empty() {
        // No user categories → legacy federated Creates so empty categories still work
        return collect_legacy_brew_activities(db).await;
    }

    // 2) Load this user's sources
    let source_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT id, name, category
               FROM brew_sources
               WHERE user_id = $1"#,
            [user_id.into()],
        ))
        .await
        .unwrap_or_default();

    let mut matching_source_ids: Vec<i32> = Vec::new();
    let mut source_meta: std::collections::HashMap<i32, (String, String)> =
        std::collections::HashMap::new();

    for row in &source_rows {
        let id: i32 = match row.try_get("", "id") {
            Ok(v) => v,
            Err(_) => continue,
        };
        let name: String = row.try_get("", "name").unwrap_or_default();
        let category: Option<String> = row.try_get("", "category").ok().flatten();
        if source_matches_any_category(category.as_deref(), &category_names) {
            matching_source_ids.push(id);
            let matched_cats: Vec<String> = category_names
                .iter()
                .filter(|c| {
                    category
                        .as_deref()
                        .map(|sc| source_category_matches(sc, c))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            source_meta.insert(id, (name, matched_cats.join(", ")));
        }
    }

    if matching_source_ids.is_empty() {
        // Categories exist but no sources assigned → empty (not an error)
        return vec![];
    }

    // 3) Recent items from matching sources
    let item_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT bi.id, bi.title, bi.link, bi.summary, bi.content, bi.source_id, bi.published_at,
                      fpc.activity_id AS published_activity_id
               FROM brew_items bi
               LEFT JOIN federation_published_content fpc
                 ON fpc.content_type = 'brew-article'
                AND fpc.content_id = bi.id::text
                AND fpc.user_id = $1
               WHERE bi.source_id = ANY($2)
               ORDER BY bi.published_at DESC
               LIMIT $3"#,
            [
                user_id.into(),
                matching_source_ids.clone().into(),
                (BREW_RING_ITEM_LIMIT as i64).into(),
            ],
        ))
        .await
        .unwrap_or_default();

    let base_url = get_base_url().await;
    let base = base_url.trim_end_matches('/');

    item_rows
        .iter()
        .filter_map(|r| {
            let item_id: i32 = r.try_get("", "id").ok()?;
            let title: String = r.try_get("", "title").unwrap_or_default();
            let link: String = r.try_get("", "link").unwrap_or_default();
            let summary_opt: Option<String> = r.try_get("", "summary").ok().flatten();
            let content_opt: Option<String> = r.try_get("", "content").ok().flatten();
            let source_id: i32 = r.try_get("", "source_id").unwrap_or(0);
            let (source_name, cats_str) = source_meta
                .get(&source_id)
                .cloned()
                .unwrap_or_else(|| (String::new(), String::new()));
            let categories: Vec<String> = if cats_str.is_empty() {
                vec![]
            } else {
                cats_str
                    .split(", ")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect()
            };
            let summary = brew_ring_summary(summary_opt.as_deref(), content_opt.as_deref());

            // Stable activity_id: reuse published federation activity if any
            let activity_id = r
                .try_get::<Option<String>>("", "published_activity_id")
                .ok()
                .flatten()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("{}/brew/ring/{}", base, item_id));

            Some(json!({
                "type": "brew",
                "activity_id": activity_id,
                "data": {
                    "type": "Article",
                    "name": title,
                    "title": title,
                    "url": link,
                    "link": link,
                    "summary": summary,
                    "source": source_name,
                    "categories": categories,
                    "brew_item_id": item_id,
                    "mfp:contentType": "brew-article",
                    "mfp:contentId": item_id.to_string()
                }
            }))
        })
        .collect()
}

/// Best-effort: after new categorized brew items land, trigger brew-recommend ring sync
/// for rings that have peers. Rate-limited to one pass per call (caller batches per tick).
pub async fn maybe_trigger_brew_recommend_sync_for_user(db: &DatabaseConnection, user_id: i32) {
    // Resolve username for trigger_sync
    let username = match db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
    {
        Ok(Some(row)) => row
            .try_get::<String>("", "username")
            .unwrap_or_else(|_| "admin".to_string()),
        _ => return,
    };

    // Rings with at least one peer
    let rings = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ring_id FROM federation_ring_memberships
               WHERE ring_type = 'brew-recommend'
                 AND known_peers IS NOT NULL
                 AND jsonb_array_length(COALESCE(known_peers, '[]'::json)::jsonb) > 0"#,
            [],
        ))
        .await
        .unwrap_or_default();

    if rings.is_empty() {
        return;
    }

    // Only sync if user has categories (otherwise legacy path is publish-driven)
    let has_cats = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM brew_categories WHERE user_id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .ok()
        .flatten()
        .is_some();
    if !has_cats {
        return;
    }

    for row in rings {
        let ring_id: String = match row.try_get("", "ring_id") {
            Ok(id) => id,
            Err(_) => continue,
        };
        match trigger_sync(&ring_id, &username, db).await {
            Ok(res) => {
                tracing::info!(
                    "[Ring] Auto brew-recommend sync ring={} user={}: {}",
                    ring_id,
                    user_id,
                    res
                );
            }
            Err((status, err)) => {
                tracing::warn!(
                    "[Ring] Auto brew-recommend sync failed ring={} user={}: {:?} {:?}",
                    ring_id,
                    user_id,
                    status,
                    err
                );
            }
        }
    }
}

/// 收集本地要同步的数据条目
async fn collect_sync_entries(
    ring_type: &str,
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    category_filter: Option<&str>,
) -> Vec<serde_json::Value> {
    match ring_type {
        "brew-recommend" => {
            collect_brew_recommend_entries(db, user_id, username, category_filter).await
        }
        "tapp-store" => {
            // 收集已发布的 Tapp 内容
            let rows = db
                .query_all_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT activity_id, object_json
                       FROM federation_activities
                       WHERE activity_type = 'Create' AND object_type = 'tapp' AND is_local = true
                       ORDER BY published_at DESC LIMIT 20"#,
                    [],
                ))
                .await
                .unwrap_or_default();
            rows.iter()
                .filter_map(|r| {
                    let obj: serde_json::Value = r.try_get("", "object_json").ok()?;
                    Some(json!({
                        "type": "tapp",
                        "activity_id": r.try_get::<String>("", "activity_id").unwrap_or_default(),
                        "data": obj
                    }))
                })
                .collect()
        }
        "library-exchange" => {
            // Prefer federated Create(library) publishes; fall back to local platform_metadata snapshots
            // so rings have something to gossip even before users explicitly publish.
            let rows = db
                .query_all_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT activity_id, object_json
                       FROM federation_activities
                       WHERE activity_type = 'Create' AND object_type = 'library' AND is_local = true
                       ORDER BY published_at DESC LIMIT 20"#,
                    [],
                ))
                .await
                .unwrap_or_default();
            let mut entries: Vec<serde_json::Value> = rows
                .iter()
                .filter_map(|r| {
                    let obj: serde_json::Value = r.try_get("", "object_json").ok()?;
                    Some(json!({
                        "type": "library",
                        "activity_id": r.try_get::<String>("", "activity_id").unwrap_or_default(),
                        "data": obj
                    }))
                })
                .collect();
            if entries.is_empty() {
                let meta_rows = db
                    .query_all_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"SELECT id, platform_name, fetched_at
                           FROM platform_metadata
                           WHERE user_id = $1
                           ORDER BY fetched_at DESC NULLS LAST, id DESC
                           LIMIT 12"#,
                        [user_id.into()],
                    ))
                    .await
                    .unwrap_or_default();
                for r in meta_rows {
                    let id: i32 = r.try_get("", "id").unwrap_or(0);
                    let platform: String = r.try_get("", "platform_name").unwrap_or_default();
                    if platform.is_empty() {
                        continue;
                    }
                    entries.push(json!({
                        "type": "library",
                        "activity_id": format!("local-library-meta-{}", id),
                        "data": {
                            "type": "Collection",
                            "name": format!("{} library", platform),
                            "summary": format!("Local {} library snapshot", platform),
                            "mfp:contentType": "library",
                            "mfp:platform": platform,
                            "mfp:metadataId": id,
                            "platform": platform,
                        }
                    }));
                }
            }
            entries
        }
        "instance-directory" => {
            // 收集已知实例信息
            let rows = db
                .query_all_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT domain, software, software_version, instance_name, description, trust_level
                       FROM federation_instances
                       ORDER BY last_seen_at DESC NULLS LAST LIMIT 50"#,
                    [],
                ))
                .await
                .unwrap_or_default();
            rows.iter()
                .map(|r| {
                    json!({
                        "type": "instance",
                        "domain": r.try_get::<String>("", "domain").unwrap_or_default(),
                        "software": r.try_get::<Option<String>>("", "software").unwrap_or(None),
                        "version": r.try_get::<Option<String>>("", "software_version").unwrap_or(None),
                        "name": r.try_get::<Option<String>>("", "instance_name").unwrap_or(None),
                        "description": r.try_get::<Option<String>>("", "description").unwrap_or(None),
                        "trust_level": r.try_get::<i16>("", "trust_level").unwrap_or(0)
                    })
                })
                .collect()
        }
        _ => vec![],
    }
}

// Inbox 处理（远程 Ring 事件）

/// 处理收到的 RingJoin Activity（远程实例请求加入我们的 Ring 或通知我们加入他们的）
pub async fn handle_ring_join(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let ring_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing ring id")?;
    let _ring_type = object
        .get("ringType")
        .and_then(|v| v.as_str())
        .unwrap_or("instance-directory");

    // 检查本地是否已有这个 Ring
    let existing = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT known_peers FROM federation_ring_memberships WHERE ring_id = $1",
            [ring_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if let Some(row) = existing {
        // Ring 已存在，原子追加 actor 到 peers（避免并发竞争）
        let peers: serde_json::Value = row.try_get("", "known_peers").unwrap_or(json!([]));
        if let Some(arr) = peers.as_array() {
            if arr.iter().any(|v| v.as_str() == Some(actor_url_str)) {
                // 已存在，跳过
                tracing::debug!("[Ring] Peer {} already in ring {}", actor_url_str, ring_id);
                return Ok(());
            }
        }
        // known_peers is json (not jsonb); cast for @> / || containment ops
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_ring_memberships
               SET known_peers = CASE
                 WHEN NOT (COALESCE(known_peers, '[]'::json)::jsonb @> $2::jsonb)
                 THEN (COALESCE(known_peers, '[]'::json)::jsonb || $2::jsonb)
                 ELSE COALESCE(known_peers, '[]'::json)::jsonb
               END
               WHERE ring_id = $1"#,
            [ring_id.into(), json!([actor_url_str]).into()],
        ))
        .await
        .map_err(|e| e.to_string())?;
    } else {
        // 未知 Ring — 不自动创建，仅记录日志
        // 安全考量：自动加入任何远程 Ring 会允许恶意实例注入数据到本地 timeline
        tracing::warn!(
            "[Ring] Received RingJoin for unknown ring {} from {}, ignoring (auto-join disabled)",
            ring_id,
            actor_url_str
        );
        return Ok(());
    }

    tracing::info!(
        "[Ring] Received RingJoin for {} from {}",
        ring_id,
        actor_url_str
    );
    Ok(())
}

/// Build a readable timeline preview for a ring sync entry.
fn ring_entry_content_preview(
    entry_type: &str,
    data: &serde_json::Value,
    actor_url_str: &str,
) -> String {
    let title = data
        .get("title")
        .or_else(|| data.get("name"))
        .or_else(|| data.pointer("/object/name"))
        .or_else(|| data.pointer("/object/title"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(t) = title {
        let link = data
            .get("link")
            .or_else(|| data.get("url"))
            .or_else(|| data.pointer("/object/url"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        return match link {
            Some(u) => format!("{} — {}", t, u),
            None => t.to_string(),
        };
    }
    format!("[Ring Sync] {} from {}", entry_type, actor_url_str)
}

/// 处理收到的 RingSync Activity（Gossip 数据推送）
pub async fn handle_ring_sync(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let ring_id = object
        .get("ring")
        .and_then(|v| v.as_str())
        .ok_or("Missing ring id")?;
    let _ring_type = object
        .get("ringType")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let entries = object
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let ttl = object.get("ttl").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // 确保 Ring 存在
    let ring_exists = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT ring_id FROM federation_ring_memberships WHERE ring_id = $1",
            [ring_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if ring_exists.is_none() {
        tracing::warn!(
            "[Ring] Received sync for unknown ring {}, ignoring",
            ring_id
        );
        return Ok(());
    }

    // 获取本地用户 ID（用于 timeline 和转发）
    let first_user: i32 = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users ORDER BY id LIMIT 1",
            [],
        ))
        .await
        .map_err(|e| e.to_string())?
        .and_then(|r| r.try_get("", "id").ok())
        .unwrap_or(1);

    // 处理收到的条目 — 存入 Timeline
    let mut imported = 0;
    for entry in &entries {
        let entry_type = entry
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let data = entry.get("data").cloned().unwrap_or(json!(null));
        let activity_id_val = entry
            .get("activity_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if activity_id_val.is_empty() {
            continue;
        }

        // 去重：检查是否已有此 activity
        let exists = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 FROM federation_timeline WHERE activity_id = $1",
                [activity_id_val.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;

        if exists.is_some() {
            continue;
        }

        // Prefer human-readable title/name for brew (and similar) entries
        let preview = ring_entry_content_preview(entry_type, &data, actor_url_str);

        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_timeline
                   (user_id, activity_id, activity_type, object_type, content_preview, content_json, received_at)
                   VALUES ($1, $2, 'Create', $3, $4, $5, NOW())
                   ON CONFLICT (user_id, activity_id) DO NOTHING"#,
                [
                    first_user.into(),
                    activity_id_val.into(),
                    entry_type.into(),
                    preview.into(),
                    data.into(),
                ],
            ))
            .await;
        imported += 1;
    }

    // 更新 last_sync_at 和确保 peer 在列表中
    // known_peers is json (not jsonb); cast for @> / || containment ops
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_ring_memberships
               SET last_sync_at = NOW(),
                   known_peers = CASE
                     WHEN NOT (COALESCE(known_peers, '[]'::json)::jsonb @> $2::jsonb)
                     THEN (COALESCE(known_peers, '[]'::json)::jsonb || $2::jsonb)
                     ELSE COALESCE(known_peers, '[]'::json)::jsonb
                   END
               WHERE ring_id = $1"#,
            [ring_id.into(), json!([actor_url_str]).into()],
        ))
        .await;

    tracing::info!(
        "[Ring] Sync from {} to ring {}: {} entries ({} imported), ttl={}",
        actor_url_str,
        ring_id,
        entries.len(),
        imported,
        ttl
    );

    // Gossip 转发（如果 TTL > 0 且有新数据导入，继续传播给其他 peer）
    if ttl > 0 && imported > 0 {
        // 获取本地已知 peer 列表，排除发送方
        let ring_row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT known_peers, gossip_config FROM federation_ring_memberships WHERE ring_id = $1",
                [ring_id.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;

        if let Some(row) = ring_row {
            let local_peers: serde_json::Value =
                row.try_get("", "known_peers").unwrap_or(json!([]));
            let config: serde_json::Value = row.try_get("", "gossip_config").unwrap_or(json!({}));
            let fanout = config.get("fanout").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

            let forward_peers: Vec<String> = local_peers
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .filter(|p| p != actor_url_str) // 不回传给发送方
                        .collect()
                })
                .unwrap_or_default();

            if !forward_peers.is_empty() {
                // 只转发新导入的条目
                let new_entries: Vec<&serde_json::Value> = entries.iter().take(imported).collect();
                let base_url = get_base_url().await;

                // 选取最多 fanout 个 peer
                let targets: Vec<&String> = forward_peers.iter().take(fanout).collect();

                for target in &targets {
                    let fwd_activity_id = generate_activity_id(&base_url);
                    let fwd_activity = json!({
                        "@context": build_context(),
                        "type": "myriad:RingSync",
                        "id": &fwd_activity_id,
                        "actor": actor_url_str,
                        "to": [target],
                        "object": {
                            "type": "myriad:RingSyncPayload",
                            "ring": ring_id,
                            "ringType": _ring_type,
                            "entries": &new_entries,
                            "ttl": ttl - 1
                        }
                    });

                    if let Ok(remote) =
                        crate::federation::actor::fetch_remote_actor(db, target).await
                    {
                        if !remote.inbox_url.is_empty() {
                            let domain = extract_domain(&remote.inbox_url).unwrap_or_default();
                            let act_row = db
                                .query_one_raw(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    r#"INSERT INTO federation_activities
                                       (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                                       VALUES ($1, $2, 'RingSync', 'Ring', $3, false, NOW())
                                       RETURNING id"#,
                                    [fwd_activity_id.into(), first_user.into(), fwd_activity.into()],
                                ))
                                .await;

                            if let Ok(Some(r)) = act_row {
                                if let Ok(act_id) = r.try_get::<i32>("", "id") {
                                    let _ = db
                                        .execute_raw(Statement::from_sql_and_values(
                                            DatabaseBackend::Postgres,
                                            r#"INSERT INTO federation_delivery_queue
                                               (activity_id, target_inbox, target_domain, status, created_at)
                                               VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                                            [act_id.into(), remote.inbox_url.into(), domain.into()],
                                        ))
                                        .await;
                                }
                            }
                        }
                    }
                }
                tracing::info!(
                    "[Ring] Forwarded gossip for ring {} to {} peers, ttl={}",
                    ring_id,
                    targets.len(),
                    ttl - 1
                );
            }
        }
    }

    Ok(())
}

/// 处理收到的 RingLeave Activity
pub async fn handle_ring_leave(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let ring_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing ring id")?;

    // 原子地从 known_peers 中移除（与 remove_peer 一致；cast json → jsonb）
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_ring_memberships
           SET known_peers = (
               SELECT COALESCE(jsonb_agg(elem), '[]'::jsonb)
               FROM jsonb_array_elements(COALESCE(known_peers, '[]'::json)::jsonb) AS elem
               WHERE elem #>> '{}' != $2
           )
           WHERE ring_id = $1"#,
        [ring_id.into(), actor_url_str.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    tracing::info!("[Ring] Peer {} left ring {}", actor_url_str, ring_id);
    Ok(())
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_category_matches_exact() {
        assert!(source_category_matches("技术", "技术"));
        assert!(source_category_matches(" tech ", "tech"));
        assert!(!source_category_matches("技术", "科技"));
        assert!(!source_category_matches("", "技术"));
        assert!(!source_category_matches("技术", ""));
    }

    #[test]
    fn source_category_matches_multi_comma() {
        // "友情链接, 技术" contains "技术" and "友情链接"
        assert!(source_category_matches("友情链接, 技术", "技术"));
        assert!(source_category_matches("友情链接, 技术", "友情链接"));
        assert!(source_category_matches("技术, 科技, 生活", "科技")); // middle
        assert!(source_category_matches("技术, 科技, 生活", "技术")); // start
        assert!(source_category_matches("技术, 科技, 生活", "生活")); // end
        assert!(!source_category_matches("技术, 科技", "生活"));
        // substring that is not a full category token should not match
        assert!(!source_category_matches("科学技术", "技术"));
    }

    #[test]
    fn source_matches_any_category_none_and_some() {
        let cats = vec!["技术".to_string(), "生活".to_string()];
        assert!(!source_matches_any_category(None, &cats));
        assert!(!source_matches_any_category(Some(""), &cats));
        assert!(!source_matches_any_category(Some("  "), &cats));
        assert!(source_matches_any_category(Some("技术"), &cats));
        assert!(source_matches_any_category(Some("友情链接, 生活"), &cats));
        assert!(!source_matches_any_category(Some("游戏"), &cats));
        // empty categories list → no match
        assert!(!source_matches_any_category(Some("技术"), &[]));
    }

    #[test]
    fn brew_ring_summary_strips_html_and_limits() {
        let s = brew_ring_summary(Some("<p>Hello <b>world</b></p>"), None);
        assert_eq!(s, "Hello world");
        let long = "x".repeat(600);
        let s2 = brew_ring_summary(Some(&long), None);
        assert_eq!(s2.chars().count(), 500);
        let s3 = brew_ring_summary(None, Some("<div>from content</div>"));
        assert_eq!(s3, "from content");
        let s4 = brew_ring_summary(None, None);
        assert_eq!(s4, "");
    }

    #[test]
    fn ring_entry_content_preview_prefers_title() {
        let data = json!({
            "title": "My Brew Post",
            "link": "https://example.com/a"
        });
        let p = ring_entry_content_preview("brew", &data, "https://peer/users/a");
        assert!(p.contains("My Brew Post"));
        assert!(p.contains("https://example.com/a"));

        let data2 = json!({ "name": "Named Only" });
        let p2 = ring_entry_content_preview("brew", &data2, "https://peer/users/a");
        assert_eq!(p2, "Named Only");

        let data3 = json!({});
        let p3 = ring_entry_content_preview("brew", &data3, "https://peer/users/a");
        assert!(p3.contains("Ring Sync"));
        assert!(p3.contains("brew"));
    }

    #[test]
    fn create_ring_request_deserializes_category_aliases() {
        let a: CreateRingRequest =
            serde_json::from_str(r#"{"name":"r","ring_type":"brew-recommend","category":"技术"}"#)
                .unwrap();
        assert_eq!(a.category.as_deref(), Some("技术"));

        let b: CreateRingRequest = serde_json::from_str(
            r#"{"name":"r","ring_type":"brew-recommend","brew_category":"生活"}"#,
        )
        .unwrap();
        assert_eq!(b.category.as_deref(), Some("生活"));

        let c: CreateRingRequest =
            serde_json::from_str(r#"{"name":"r","ring_type":"tapp-store"}"#).unwrap();
        assert!(c.category.is_none());
    }
}
