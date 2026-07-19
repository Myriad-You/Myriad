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

/// 根据用户名查询实际的 user_id，避免硬编码 user_id = 1
async fn resolve_user_id(
    db: &DatabaseConnection,
    username: &str,
) -> Result<i32, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
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
                .query_one(Statement::from_sql_and_values(
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

// ==================== 请求/响应类型 ====================

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

// ==================== 辅助函数 ====================

fn validate_ring_type(rt: &str) -> bool {
    [
        "tapp-store",
        "brew-recommend",
        "library-exchange",
        "instance-directory",
    ]
    .contains(&rt)
}

// ==================== Ring CRUD ====================

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
    let gossip_config = json!({
        "fanout": req.fanout.unwrap_or(3),
        "ttl": req.ttl.unwrap_or(5),
        "interval": req.interval.unwrap_or(300)
    });

    db.execute(Statement::from_sql_and_values(
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
        .query_all(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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
    for peer in &peers {
        // 为每个 peer 生成独立的 activity_id，避免 DB 冲突
        let activity_id = generate_activity_id(&base_url);
        let mut leave_activity = leave_base.clone();
        leave_activity["id"] = json!(&activity_id);
        if let Ok(remote) = crate::federation::actor::fetch_remote_actor(db, peer).await {
            if !remote.inbox_url.is_empty() {
                let domain = extract_domain(&remote.inbox_url).unwrap_or_default();
                let act_row = db
                    .query_one(Statement::from_sql_and_values(
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
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at)
                               VALUES ($1, $2, $3, 'pending', NOW())"#,
                            [act_id.into(), remote.inbox_url.into(), domain.into()],
                        ))
                        .await;
                }
            }
        }
    }

    // 删除本地记录
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_ring_memberships WHERE ring_id = $1",
        [ring_id.into()],
    ))
    .await
    .map_err(db_err)?;

    tracing::info!("[Ring] Left ring {}", ring_id);

    Ok(json!({"success": true, "ring_id": ring_id}))
}

// ==================== Peer 管理 ====================

/// 获取 Ring 的 Peer 列表
pub async fn get_peers(
    ring_id: &str,
    db: &DatabaseConnection,
) -> Result<Vec<RingPeer>, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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
    db.execute(Statement::from_sql_and_values(
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
        let act_row = db
            .query_one(Statement::from_sql_and_values(
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
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())"#,
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
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    // 使用子查询原子地从 JSON 数组中移除指定 peer（cast to jsonb for ops）
    let result = db
        .execute(Statement::from_sql_and_values(
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

    tracing::info!("[Ring] Removed peer {} from ring {}", peer_url, ring_id);

    Ok(json!({"success": true, "ring_id": ring_id, "removed_peer": peer_url}))
}

// ==================== Gossip 同步 ====================

/// 触发 Gossip 同步：向随机 fanout 个 peer 推送本地数据
pub async fn trigger_sync(
    ring_id: &str,
    username: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let ring_row = db
        .query_one(Statement::from_sql_and_values(
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

    // 收集本地要同步的数据（根据 ring_type）
    let entries = collect_sync_entries(&ring_type, db).await;

    if entries.is_empty() {
        // 更新 last_sync_at
        let _ = db
            .execute(Statement::from_sql_and_values(
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
                let local_user_id = resolve_user_id(db, username).await?;
                let act_row = db
                    .query_one(Statement::from_sql_and_values(
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
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at)
                               VALUES ($1, $2, $3, 'pending', NOW())"#,
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
        .execute(Statement::from_sql_and_values(
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

/// 收集本地要同步的数据条目
async fn collect_sync_entries(ring_type: &str, db: &DatabaseConnection) -> Vec<serde_json::Value> {
    match ring_type {
        "brew-recommend" => {
            // 收集最近发布到联邦网络的 Brew 内容
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT activity_id, object_json
                       FROM federation_activities
                       WHERE activity_type = 'Create' AND object_type = 'brew-article' AND is_local = true
                       ORDER BY published_at DESC LIMIT 20"#,
                    [],
                ))
                .await
                .unwrap_or_default();
            rows.iter()
                .filter_map(|r| {
                    let obj: serde_json::Value = r.try_get("", "object_json").ok()?;
                    Some(json!({
                        "type": "brew",
                        "activity_id": r.try_get::<String>("", "activity_id").unwrap_or_default(),
                        "data": obj
                    }))
                })
                .collect()
        }
        "tapp-store" => {
            // 收集已发布的 Tapp 内容
            let rows = db
                .query_all(Statement::from_sql_and_values(
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
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT activity_id, object_json
                       FROM federation_activities
                       WHERE activity_type = 'Create' AND object_type = 'library' AND is_local = true
                       ORDER BY published_at DESC LIMIT 20"#,
                    [],
                ))
                .await
                .unwrap_or_default();
            rows.iter()
                .filter_map(|r| {
                    let obj: serde_json::Value = r.try_get("", "object_json").ok()?;
                    Some(json!({
                        "type": "library",
                        "activity_id": r.try_get::<String>("", "activity_id").unwrap_or_default(),
                        "data": obj
                    }))
                })
                .collect()
        }
        "instance-directory" => {
            // 收集已知实例信息
            let rows = db
                .query_all(Statement::from_sql_and_values(
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

// ==================== Inbox 处理（远程 Ring 事件） ====================

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
        .query_one(Statement::from_sql_and_values(
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
        db.execute(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 FROM federation_timeline WHERE activity_id = $1",
                [activity_id_val.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;

        if exists.is_some() {
            continue;
        }

        let _ = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_timeline
                   (user_id, activity_id, activity_type, object_type, content_preview, content_json, received_at)
                   VALUES ($1, $2, 'Create', $3, $4, $5, NOW())"#,
                [
                    first_user.into(),
                    activity_id_val.into(),
                    entry_type.into(),
                    format!("[Ring Sync] {} from {}", entry_type, actor_url_str).into(),
                    data.into(),
                ],
            ))
            .await;
        imported += 1;
    }

    // 更新 last_sync_at 和确保 peer 在列表中
    // known_peers is json (not jsonb); cast for @> / || containment ops
    let _ = db
        .execute(Statement::from_sql_and_values(
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
            .query_one(Statement::from_sql_and_values(
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
                                .query_one(Statement::from_sql_and_values(
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
                                        .execute(Statement::from_sql_and_values(
                                            DatabaseBackend::Postgres,
                                            r#"INSERT INTO federation_delivery_queue
                                               (activity_id, target_inbox, target_domain, status, created_at)
                                               VALUES ($1, $2, $3, 'pending', NOW())"#,
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
    db.execute(Statement::from_sql_and_values(
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
