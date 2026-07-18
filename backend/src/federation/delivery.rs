//! 投递队列服务（Layer 2）
//!
//! 后台任务：从 federation_delivery_queue 取出待投递的 Activity，
//! 签名后发送到目标 inbox，支持指数退避重试。

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use std::time::Duration;

use crate::federation::keys::KeyPair;
use crate::federation::signature::{sign_request, SignatureParams};
use crate::federation::types::*;

/// 投递队列处理器 — 由后台任务驱动
///
/// 每次调用处理一批待投递的 Activity（最多 batch_size 个）
pub async fn process_delivery_queue(
    db: &DatabaseConnection,
    batch_size: u32,
) -> Result<u32, String> {
    // Atomic claim with FOR UPDATE SKIP LOCKED so concurrent workers do not
    // double-deliver the same row. Also reclaims stuck `delivering` rows from
    // crashed workers (#97 behaviour kept).
    let pending = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            // MATERIALIZED keeps FOR UPDATE SKIP LOCKED from being inlined away (PG12+).
            r#"WITH selected AS MATERIALIZED (
                   SELECT id, status AS prev_status, attempts AS prev_attempts
                   FROM federation_delivery_queue
                   WHERE (status = 'pending'
                          AND (next_retry_at IS NULL OR next_retry_at <= NOW()))
                      -- 崩溃恢复：投递中途进程退出会把条目留在 delivering，超时后重新认领
                      OR (status = 'delivering'
                          AND last_attempt_at < NOW() - INTERVAL '10 minutes')
                   ORDER BY created_at ASC
                   LIMIT $1
                   FOR UPDATE SKIP LOCKED
               ),
               claimed AS (
                   UPDATE federation_delivery_queue dq
                   SET status = 'delivering',
                       last_attempt_at = NOW(),
                       attempts = CASE
                           WHEN s.prev_status = 'delivering' THEN s.prev_attempts + 1
                           ELSE s.prev_attempts
                       END
                   FROM selected s
                   WHERE dq.id = s.id
                   RETURNING dq.id, dq.activity_id, dq.target_inbox, dq.target_domain,
                             dq.attempts, dq.max_attempts, s.prev_status
               )
               SELECT c.id, c.activity_id, c.target_inbox, c.target_domain,
                      c.attempts, c.max_attempts, c.prev_status,
                      a.activity_id AS ap_activity_id, a.activity_type, a.object_json, a.user_id
               FROM claimed c
               JOIN federation_activities a ON a.id = c.activity_id"#,
            [(batch_size as i64).into()],
        ))
        .await
        .map_err(|e| format!("Queue claim failed: {}", e))?;

    let mut delivered = 0u32;

    for row in pending {
        let queue_id: i32 = row.try_get("", "id").unwrap_or(0);
        let target_inbox: String = row.try_get("", "target_inbox").unwrap_or_default();
        let target_domain: String = row.try_get("", "target_domain").unwrap_or_default();
        let attempts: i32 = row.try_get("", "attempts").unwrap_or(0);
        let max_attempts: i32 = row.try_get("", "max_attempts").unwrap_or(12);
        let prev_status: String = row.try_get("", "prev_status").unwrap_or_default();
        let _ap_activity_id: String = row.try_get("", "ap_activity_id").unwrap_or_default();
        let activity_type: String = row.try_get("", "activity_type").unwrap_or_default();
        let object_json: serde_json::Value = row.try_get("", "object_json").unwrap_or_default();
        let user_id: i32 = row.try_get("", "user_id").unwrap_or(0);

        // Reclaimed stuck delivering: attempts already incremented in the claim UPDATE
        let reclaim = prev_status == "delivering";
        if reclaim && attempts >= max_attempts {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE federation_delivery_queue SET status = 'dead', error_message = $1, last_attempt_at = NOW() WHERE id = $2",
                    ["Exceeded max attempts after reclaim".into(), queue_id.into()],
                ))
                .await;
            continue;
        }

        // 投递前：目标实例信任策略检查（黑名单等）
        if let Err(reason) = crate::federation::trust::enforce_outbound(db, &target_domain).await {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_delivery_queue
                       SET status = 'dead', error_message = $1, last_attempt_at = NOW()
                       WHERE id = $2"#,
                    [reason.clone().into(), queue_id.into()],
                ))
                .await;
            tracing::warn!(
                "🛑 Delivery blocked by trust policy: target={}, reason={}",
                target_inbox,
                reason
            );
            continue;
        }

        // object_json 已经是完整的 Activity JSON（含 @context/type/id/actor/object），直接发送
        let base_url = get_base_url().await;
        let username = get_username_by_id(db, user_id).await.unwrap_or_default();

        let body_bytes = serde_json::to_vec(&object_json).unwrap_or_default();

        // 获取用户密钥对
        match load_user_keypair(db, user_id).await {
            Ok(keypair) => {
                match deliver_activity(
                    &keypair,
                    &base_url,
                    &username,
                    &target_inbox,
                    &target_domain,
                    &body_bytes,
                )
                .await
                {
                    Ok(()) => {
                        // 投递成功
                        let _ = db
                            .execute(Statement::from_sql_and_values(
                                DatabaseBackend::Postgres,
                                "UPDATE federation_delivery_queue SET status = 'delivered', last_attempt_at = NOW() WHERE id = $1",
                                [queue_id.into()],
                            ))
                            .await;
                        delivered += 1;

                        // 更新实例的 last_success_at，重置 failure_count
                        let _ = db
                            .execute(Statement::from_sql_and_values(
                                DatabaseBackend::Postgres,
                                "UPDATE federation_instances SET last_success_at = NOW(), failure_count = 0 WHERE domain = $1",
                                [target_domain.clone().into()],
                            ))
                            .await;

                        tracing::debug!("📤 Delivered {} to {}", activity_type, target_inbox);
                    }
                    Err(e) => {
                        let new_attempts = attempts + 1;
                        if new_attempts >= max_attempts {
                            // 放弃
                            let _ = db
                                .execute(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_delivery_queue SET status = 'dead', attempts = $1, error_message = $2, last_attempt_at = NOW() WHERE id = $3",
                                    [new_attempts.into(), e.clone().into(), queue_id.into()],
                                ))
                                .await;
                            tracing::warn!(
                                "💀 Delivery dead after {} attempts to {}: {}",
                                new_attempts,
                                target_inbox,
                                e
                            );
                        } else {
                            // 指数退避：2^attempts 秒，最大 86400 秒 (24h)
                            let backoff_secs = std::cmp::min(2i64.pow(new_attempts as u32), 86400);
                            let _ = db
                                .execute(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_delivery_queue SET status = 'pending', attempts = $1, error_message = $2, last_attempt_at = NOW(), next_retry_at = NOW() + make_interval(secs => $4::double precision) WHERE id = $3",
                                    [new_attempts.into(), e.clone().into(), queue_id.into(), backoff_secs.into()],
                                ))
                                .await;

                            // 更新实例 failure_count
                            let _ = db
                                .execute(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_instances SET failure_count = failure_count + 1 WHERE domain = $1",
                                    [target_domain.clone().into()],
                                ))
                                .await;

                            tracing::warn!(
                                "⚠️ Delivery failed (attempt {}/{}), retrying in {}s: {}",
                                new_attempts,
                                max_attempts,
                                backoff_secs,
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to load keypair for user {}: {}", user_id, e);
                // 密钥问题几乎不会自愈；按普通失败计数退避，避免 15s 热循环刷日志
                let new_attempts = attempts + 1;
                if new_attempts >= max_attempts {
                    let _ = db
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'dead', attempts = $1, error_message = $2, last_attempt_at = NOW() WHERE id = $3",
                            [
                                new_attempts.into(),
                                format!("Key load failed: {}", e).into(),
                                queue_id.into(),
                            ],
                        ))
                        .await;
                } else {
                    let backoff_secs = std::cmp::min(2i64.pow(new_attempts as u32), 86400);
                    let _ = db
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'pending', attempts = $1, error_message = $2, last_attempt_at = NOW(), next_retry_at = NOW() + make_interval(secs => $4::double precision) WHERE id = $3",
                            [
                                new_attempts.into(),
                                format!("Key load failed: {}", e).into(),
                                queue_id.into(),
                                backoff_secs.into(),
                            ],
                        ))
                        .await;
                }
            }
        }
    }

    Ok(delivered)
}

/// 启动投递队列后台循环
pub fn spawn_delivery_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            match process_delivery_queue(&db, 20).await {
                Ok(n) if n > 0 => {
                    tracing::info!("📤 Delivery worker: delivered {} activities", n);
                }
                Err(e) => {
                    tracing::error!("Delivery worker error: {}", e);
                }
                _ => {} // 无待投递项，静默
            }
        }
    });
}

// ==================== 实际投递 ====================

/// 投递 Activity 到目标 inbox
async fn deliver_activity(
    keypair: &KeyPair,
    base_url: &str,
    username: &str,
    target_inbox: &str,
    target_domain: &str,
    body: &[u8],
) -> Result<(), String> {
    // 纵深防御：即使 inbox URL 已入库，投递前仍验证不指向内网
    if is_internal_url(target_inbox) {
        return Err(format!(
            "Refusing to deliver to internal URL: {}",
            target_inbox
        ));
    }

    let kid = key_id(base_url, username);

    // Host 头/签名的 host 必须与 URL 一致（含非默认端口），否则对端验签失败；
    // target_domain（不带端口）仅用于信任策略与实例统计。
    let (path, host_header) = match url::Url::parse(target_inbox) {
        Ok(u) => {
            let path = u.path().to_string();
            let host = match (u.host_str(), u.port()) {
                (Some(h), Some(p)) => format!("{}:{}", h, p),
                (Some(h), None) => h.to_string(),
                _ => target_domain.to_string(),
            };
            (path, host)
        }
        Err(_) => ("/inbox".to_string(), target_domain.to_string()),
    };

    let params = SignatureParams {
        key_id: &kid,
        host: &host_header,
        path: &path,
        method: "POST",
        body: Some(body),
    };

    let signed = sign_request(keypair, &params).map_err(|e| format!("Signing failed: {}", e))?;

    let user_agent = format!("Myriad/{} (+{})", env!("CARGO_PKG_VERSION"), base_url);
    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        target_inbox,
        Duration::from_secs(30),
        Some(&user_agent),
    )
    .await?;

    let resp = client
        .post(target_url)
        .header("Host", &host_header)
        .header("Date", &signed.date)
        .header("Digest", &signed.digest.unwrap_or_default())
        .header("Signature", &signed.signature)
        .header("Content-Type", AP_CONTENT_TYPE)
        .body(body.to_vec())
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = resp.status();
    if status.is_success() || status.as_u16() == 202 {
        Ok(())
    } else {
        let body_text = resp.text().await.unwrap_or_default();
        Err(format!(
            "HTTP {}: {}",
            status,
            body_text.chars().take(200).collect::<String>()
        ))
    }
}

// ==================== 辅助函数 ====================

/// 加载用户的密钥对
async fn load_user_keypair(db: &DatabaseConnection, user_id: i32) -> Result<KeyPair, String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT public_key_pem, private_key_encrypted FROM federation_keys WHERE user_id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "No federation keys found for user".to_string())?;

    let pub_pem: String = row.try_get("", "public_key_pem").unwrap_or_default();
    let encrypted: String = row.try_get("", "private_key_encrypted").unwrap_or_default();

    let jwt_secret = {
        let config = crate::GLOBAL_CONFIG.read().await;
        config.jwt_secret.clone()
    };

    KeyPair::from_encrypted(&pub_pem, &encrypted, &jwt_secret)
        .map_err(|e| format!("Key decryption failed: {}", e))
}

async fn get_username_by_id(db: &DatabaseConnection, user_id: i32) -> Result<String, String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "User not found".to_string())?;

    Ok(row.try_get("", "username").unwrap_or_default())
}

async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}
