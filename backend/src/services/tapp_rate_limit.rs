//! Tapp shared rate limiter (tapp_runtime_registry namespace `rate_limit`).
//!
//! Domain implementation lives in services so host-attribution middleware, AI
//! tasks, events, declared-API, and metrics do not own the registry SQL / config
//! tables. The API layer maps [`RateLimitError`] to Axum `(StatusCode, Json)`
//! responses. Route→permission maps live in [`crate::services::tapp_host_attribution`].

use hmac::{Hmac, KeyInit, Mac};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};
use sha2::{Digest, Sha256};

use crate::services::permission_service::TappPermission;
use crate::services::tapp_registry as shared_registry;

const RATE_LIMIT_NAMESPACE: &str = "rate_limit";

#[derive(FromQueryResult)]
struct RateLimitRow {
    count: i64,
    expires_at: i64,
}

#[derive(FromQueryResult)]
struct CountRow {
    count: i64,
}

/// Domain errors for Tapp rate-limit operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitError {
    /// Registry / DB unavailable (fail closed as 503 at the HTTP edge).
    Unavailable,
    /// Subject exceeded the operation window.
    Exceeded {
        retry_after: u64,
        limit: u32,
        remaining: u32,
    },
}

impl RateLimitError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "RATE_LIMITER_UNAVAILABLE",
            Self::Exceeded { .. } => "RATE_LIMIT_EXCEEDED",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Unavailable => "Rate limiter unavailable",
            Self::Exceeded { .. } => "Rate limit exceeded",
        }
    }
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for RateLimitError {}

/// 获取操作的速率限制配置。
///
/// Returns `(limit, window_secs)`. Defaults are coarse per-(subject, tapp,
/// operation) caps for sandboxed traffic; host UI (no runtime grant) does not
/// use these keys. Tune here rather than adding a parallel limiter.
///
/// Host-proxied write classes (enforced via `tapp_host_attribution` after grant
/// validation) intentionally sit in the tens–low hundreds / minute range, with
/// stricter caps for manage/trust and speech synthesis.
pub fn get_rate_limit_config(operation: &str) -> (u32, u64) {
    match operation {
        // AI still cost-bounded; slightly above old 20 so retry/UI double-submit is tolerable.
        "ai.task" => (30, 60),
        "ai.anonymous" => (15, 60),
        operation if operation.starts_with("network.fetch:") => (90, 60),
        "route.anonymous" => (60, 60),
        "route.verify" => (60, 60),
        "route.verify.hour" => (180, 3600),
        "route.fail" => (25, 600),
        "route.fail.site" => (80, 600),
        "platform.write" => (45, 60),
        // Storage autosave / multi-key writes are normal Tapp traffic.
        "storage.set" | "storage.clear" => (180, 60),
        // Host-proxied brew mutations (grant-bearing only).
        "brew.write" => (90, 60),
        "brew.comment" => (90, 60),
        "brew.manage" => (30, 60),
        // Host-proxied federation mutations.
        // post/interact 是高频社交操作（沿用原 federation.write 额度）；
        // channel/room/ring 治理操作低频，与 files 同档。
        "federation.post" => (90, 60),
        "federation.interact" => (90, 60),
        "federation.channel" => (60, 60),
        "federation.room" => (60, 60),
        "federation.ring" => (60, 60),
        "federation.message" => (180, 60),
        "federation.files" => (60, 60),
        "federation.trust" => (20, 60),
        // Host-proxied speech write paths (TTS/ASR POST).
        "speech.tts" => (45, 60),
        "speech.asr" => (45, 60),
        // event.publish default was 200 — keep default high for pub/sub noise.
        _ => (240, 60),
    }
}

/// Map a host-proxied [`TappPermission`] to a coarse rate-limit operation class.
///
/// Returns `None` for pure-read permissions (`brew:read`, `federation:read`)
/// and anything outside brew/federation/speech host proxies — those paths are
/// not subject to this host-attribution limiter. Callers must also skip safe
/// HTTP methods (GET/HEAD/OPTIONS) so e.g. `GET /api/speech/voices` is not
/// counted against `speech.tts`.
pub fn host_write_rate_limit_operation(permission: TappPermission) -> Option<&'static str> {
    match permission {
        TappPermission::BrewWrite => Some("brew.write"),
        TappPermission::BrewComment => Some("brew.comment"),
        TappPermission::BrewManage => Some("brew.manage"),
        TappPermission::FederationPost => Some("federation.post"),
        TappPermission::FederationInteract => Some("federation.interact"),
        TappPermission::FederationChannel => Some("federation.channel"),
        TappPermission::FederationRoom => Some("federation.room"),
        TappPermission::FederationRing => Some("federation.ring"),
        TappPermission::FederationMessage => Some("federation.message"),
        TappPermission::FederationFiles => Some("federation.files"),
        TappPermission::FederationTrust => Some("federation.trust"),
        TappPermission::SpeechTts => Some("speech.tts"),
        TappPermission::SpeechAsr => Some("speech.asr"),
        // Reads and unrelated permissions: no additional host-proxy limit.
        _ => None,
    }
}

pub(crate) fn rate_limit_key(user_id: i32, tapp_id: &str, operation: &str) -> String {
    format!("{user_id}:{tapp_id}:{operation}")
}

pub(crate) fn rate_limit_record_id(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// One-way client-address fingerprint for anonymous rate-limit keys.
/// The source address itself is never persisted in the runtime registry.
pub fn anonymous_subject_fingerprint(value: &str) -> String {
    let secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "myriad-development-anonymous-quota-v1".to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(b"myriad-tapp-anonymous-quota-v1\0");
    mac.update(value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

async fn load_rate_limit_row(
    db: &impl ConnectionTrait,
    record_id: &str,
) -> Result<Option<RateLimitRow>, sea_orm::DbErr> {
    RateLimitRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
SELECT
    COALESCE((payload ->> 'count')::BIGINT, 0) AS count,
    expires_at
FROM tapp_runtime_registry
WHERE namespace = $1 AND record_id = $2
"#,
        vec![RATE_LIMIT_NAMESPACE.into(), record_id.into()],
    ))
    .one(db)
    .await
}

fn map_db_err(error: impl std::fmt::Display) -> RateLimitError {
    tracing::error!(%error, "[TAPP] Shared rate limiter unavailable");
    RateLimitError::Unavailable
}

/// 检查速率限制
pub async fn check_rate_limit(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    operation: &str,
) -> Result<(), RateLimitError> {
    check_rate_limit_key(
        db,
        user_id,
        rate_limit_key(user_id, tapp_id, operation),
        tapp_id,
        operation,
    )
    .await
    .map(|_| ())
}

/// Per inbound-credential limiter. `revision` is the ciphertext hash, so
/// rotation opens a fresh window without persisting the secret.
pub async fn check_route_verify_rate_limit(
    db: &DatabaseConnection,
    tapp_id: &str,
    credential_key: &str,
    revision: &str,
) -> Result<(), RateLimitError> {
    check_rate_limit_key(
        db,
        0,
        format!("route.verify:{tapp_id}:{credential_key}:{revision}"),
        tapp_id,
        "route.verify",
    )
    .await?;
    check_rate_limit_key(
        db,
        0,
        format!("route.verify.hour:{tapp_id}:{credential_key}:{revision}"),
        tapp_id,
        "route.verify.hour",
    )
    .await
    .map(|_| ())
}

/// Anonymous `/tapi` limiter. Separate from AI guest quota.
pub async fn check_inbound_anonymous_rate_limit(
    db: &DatabaseConnection,
    client_ip: Option<&str>,
    tapp_id: &str,
) -> Result<(), RateLimitError> {
    let fingerprint = anonymous_subject_fingerprint(client_ip.unwrap_or("unresolved"));
    check_rate_limit_key(
        db,
        0,
        format!("anonymous:{fingerprint}:{tapp_id}:route.anonymous"),
        tapp_id,
        "route.anonymous",
    )
    .await
    .map(|_| ())
}

/// Coarse anonymous limiter keyed by a one-way client-address fingerprint.
/// The source address itself is never persisted in the runtime registry.
pub async fn check_anonymous_rate_limit(
    db: &DatabaseConnection,
    client_ip: Option<&str>,
    tapp_id: &str,
) -> Result<(), RateLimitError> {
    let fingerprint = anonymous_subject_fingerprint(client_ip.unwrap_or("unresolved"));
    check_rate_limit_key(
        db,
        0,
        format!("anonymous:{fingerprint}:{tapp_id}:ai.anonymous"),
        tapp_id,
        "ai.anonymous",
    )
    .await
    .map(|_| ())
}

async fn check_rate_limit_key(
    db: &DatabaseConnection,
    registry_subject_id: i32,
    key: String,
    tapp_id: &str,
    operation: &str,
) -> Result<u32, RateLimitError> {
    let (limit, window_secs) = get_rate_limit_config(operation);
    let record_id = rate_limit_record_id(&key);
    let transaction = db.begin().await.map_err(map_db_err)?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            vec![format!("tapp_rate_limit:{key}").into()],
        ))
        .await
        .map_err(map_db_err)?;

    let now = chrono::Utc::now().timestamp();
    let current = load_rate_limit_row(&transaction, &record_id)
        .await
        .map_err(map_db_err)?;
    let (count, expires_at) = match current {
        Some(row) if row.expires_at > now => (row.count.max(0) as u32, row.expires_at),
        _ => (0, now.saturating_add(window_secs as i64)),
    };
    let allowed = count < limit;
    let remaining = limit.saturating_sub(count.saturating_add(u32::from(allowed)));
    let reset_in = expires_at.saturating_sub(now) as u64;

    if allowed {
        transaction
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, tapp_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $4, jsonb_build_object('count', $5::BIGINT), $6, NOW())
ON CONFLICT (namespace, record_id) DO UPDATE SET
    subject_id = EXCLUDED.subject_id,
    tapp_id = EXCLUDED.tapp_id,
    payload = EXCLUDED.payload,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW()
"#,
                vec![
                    RATE_LIMIT_NAMESPACE.into(),
                    record_id.into(),
                    registry_subject_id.into(),
                    tapp_id.to_string().into(),
                    i64::from(count.saturating_add(1)).into(),
                    expires_at.into(),
                ],
            ))
            .await
            .map_err(map_db_err)?;
        transaction.commit().await.map_err(map_db_err)?;
        shared_registry::maybe_cleanup(db).await;
    } else {
        transaction.rollback().await.map_err(map_db_err)?;
    }

    if !allowed {
        tracing::warn!(
            user_id = registry_subject_id,
            tapp_id = tapp_id,
            operation = operation,
            "[TAPP] Rate limit exceeded"
        );
        return Err(RateLimitError::Exceeded {
            retry_after: reset_in,
            limit,
            remaining,
        });
    }
    Ok(count.saturating_add(1))
}

/// Increment a named limit and return the new count. `Exceeded` means the
/// window is already full (used to trip inbound auto-blocks).
pub async fn increment_named_limit(
    db: &DatabaseConnection,
    key: String,
    tapp_id: &str,
    operation: &str,
) -> Result<u32, RateLimitError> {
    check_rate_limit_key(db, 0, key, tapp_id, operation).await
}

/// 获取速率限制状态（只读，不记录）
pub async fn get_rate_limit_status_for(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    operation: &str,
) -> Result<(u32, u32, u64), RateLimitError> {
    let (limit, window_secs) = get_rate_limit_config(operation);
    let key = rate_limit_key(user_id, tapp_id, operation);
    let record_id = rate_limit_record_id(&key);
    let now = chrono::Utc::now().timestamp();
    let Some(row) = load_rate_limit_row(db, &record_id)
        .await
        .map_err(map_db_err)?
    else {
        return Ok((0, limit, window_secs));
    };
    if row.expires_at <= now {
        return Ok((0, limit, 0));
    }
    let used = row.count.clamp(0, i64::from(u32::MAX)) as u32;
    Ok((
        used,
        limit.saturating_sub(used),
        row.expires_at.saturating_sub(now) as u64,
    ))
}

pub async fn get_rate_limiter_active_count(
    db: &DatabaseConnection,
) -> Result<usize, RateLimitError> {
    let row = CountRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT COUNT(*)::BIGINT AS count FROM tapp_runtime_registry WHERE namespace = $1 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT",
        vec![RATE_LIMIT_NAMESPACE.into()],
    ))
    .one(db)
    .await
    .map_err(map_db_err)?;
    Ok(row.map_or(0, |row| row.count.max(0) as usize))
}

#[cfg(test)]
mod tests {
    use super::{
        get_rate_limit_config, host_write_rate_limit_operation, rate_limit_key,
        rate_limit_record_id, RateLimitError,
    };
    use crate::services::permission_service::TappPermission;

    #[test]
    fn rate_limit_keys_are_identity_and_operation_scoped() {
        let base = rate_limit_key(42, "com.example.notes", "storage.set");
        assert_ne!(base, rate_limit_key(43, "com.example.notes", "storage.set"));
        assert_ne!(base, rate_limit_key(42, "com.example.tasks", "storage.set"));
        assert_ne!(base, rate_limit_key(42, "com.example.notes", "ai.task"));
        assert_eq!(rate_limit_record_id(&base).len(), 64);
        assert_eq!(rate_limit_record_id(&base), rate_limit_record_id(&base));
    }

    #[test]
    fn host_write_permissions_map_to_operation_classes() {
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewWrite),
            Some("brew.write")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewComment),
            Some("brew.comment")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewManage),
            Some("brew.manage")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationPost),
            Some("federation.post")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationInteract),
            Some("federation.interact")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationChannel),
            Some("federation.channel")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationRoom),
            Some("federation.room")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationRing),
            Some("federation.ring")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationMessage),
            Some("federation.message")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationFiles),
            Some("federation.files")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationTrust),
            Some("federation.trust")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::SpeechTts),
            Some("speech.tts")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::SpeechAsr),
            Some("speech.asr")
        );
    }

    #[test]
    fn host_read_permissions_are_not_rate_limited() {
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewRead),
            None
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationRead),
            None
        );
        // Unrelated capabilities stay outside the host-proxy limiter.
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::PlatformWrite),
            None
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::AiGenerate),
            None
        );
    }

    #[test]
    fn host_write_rate_limit_defaults_are_sensible() {
        // (limit, window_secs) — tens–low hundreds / minute; manage/trust/speech stricter.
        assert_eq!(get_rate_limit_config("brew.write"), (90, 60));
        assert_eq!(get_rate_limit_config("brew.comment"), (90, 60));
        assert_eq!(get_rate_limit_config("brew.manage"), (30, 60));
        assert_eq!(get_rate_limit_config("federation.post"), (90, 60));
        assert_eq!(get_rate_limit_config("federation.interact"), (90, 60));
        assert_eq!(get_rate_limit_config("federation.channel"), (60, 60));
        assert_eq!(get_rate_limit_config("federation.room"), (60, 60));
        assert_eq!(get_rate_limit_config("federation.ring"), (60, 60));
        assert_eq!(get_rate_limit_config("federation.message"), (180, 60));
        assert_eq!(get_rate_limit_config("federation.files"), (60, 60));
        assert_eq!(get_rate_limit_config("federation.trust"), (20, 60));
        assert_eq!(get_rate_limit_config("speech.tts"), (45, 60));
        assert_eq!(get_rate_limit_config("speech.asr"), (45, 60));

        // Stricter classes stay below chatty ones.
        assert!(get_rate_limit_config("brew.manage").0 < get_rate_limit_config("brew.write").0);
        assert!(
            get_rate_limit_config("federation.trust").0
                < get_rate_limit_config("federation.message").0
        );
        // 治理域（channel/room/ring）比高频社交域（post/interact）更严。
        assert!(
            get_rate_limit_config("federation.channel").0
                < get_rate_limit_config("federation.post").0
        );
        assert!(
            get_rate_limit_config("federation.ring").0
                < get_rate_limit_config("federation.interact").0
        );

        assert_eq!(get_rate_limit_config("ai.task"), (30, 60));
        assert_eq!(get_rate_limit_config("platform.write"), (45, 60));
        assert_eq!(get_rate_limit_config("storage.set"), (180, 60));
    }

    #[test]
    fn network_fetch_prefix_uses_dedicated_cap() {
        assert_eq!(get_rate_limit_config("network.fetch:weather"), (90, 60));
        assert_eq!(get_rate_limit_config("ai.anonymous"), (15, 60));
        assert_eq!(get_rate_limit_config("route.anonymous"), (60, 60));
        assert_eq!(get_rate_limit_config("route.verify"), (60, 60));
        assert_eq!(get_rate_limit_config("route.verify.hour"), (180, 3600));
        assert_eq!(get_rate_limit_config("route.fail"), (25, 600));
        assert_eq!(get_rate_limit_config("route.fail.site"), (80, 600));
        assert!(
            get_rate_limit_config("route.verify.hour").0
                < get_rate_limit_config("route.verify").0 * 60
        );
        assert_eq!(get_rate_limit_config("event.publish"), (240, 60));
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            RateLimitError::Unavailable.code(),
            "RATE_LIMITER_UNAVAILABLE"
        );
        assert_eq!(
            RateLimitError::Exceeded {
                retry_after: 12,
                limit: 20,
                remaining: 0,
            }
            .code(),
            "RATE_LIMIT_EXCEEDED"
        );
    }
}
