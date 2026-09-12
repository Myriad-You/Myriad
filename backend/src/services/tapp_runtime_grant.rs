//! Short-lived Tapp runtime grants (registry namespace `runtime_grant`).
//!
//! Domain lives in services. The API layer maps [`RuntimeGrantError`] to Axum
//! responses and owns HTTP extractors + revoke side-effects (AI cancel, event
//! disconnect, …).

use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

use crate::services::permission_service::{
    tapp_permission_replacement_hint, TappPermission, TappPermissionService, UnknownTappPermission,
    UserRole,
};
use crate::services::tapp_ownership;
use crate::services::tapp_registry as shared_registry;

pub const RUNTIME_GRANT_HEADER: &str = "x-tapp-runtime-grant";
const RUNTIME_GRANT_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_INSTANCE_ID_LENGTH: usize = 100;
const MAX_ACTIVE_GRANTS_PER_SUBJECT: usize = 128;
const RUNTIME_GRANT_NAMESPACE: &str = "runtime_grant";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Page,
    Widget,
    Headless,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StoredRuntimeGrant {
    runtime_id: String,
    tapp_id: String,
    owner_id: i32,
    subject_id: i32,
    instance_id: String,
    kind: RuntimeKind,
    permissions: Vec<String>,
    expires_at: i64,
}

/// Validated runtime identity used by handlers after grant rebind.
#[derive(Debug, Clone)]
pub struct RuntimeGrant {
    stored: StoredRuntimeGrant,
}

impl RuntimeGrant {
    pub fn runtime_id(&self) -> &str {
        &self.stored.runtime_id
    }

    pub fn tapp_id(&self) -> &str {
        &self.stored.tapp_id
    }

    pub fn owner_id(&self) -> i32 {
        self.stored.owner_id
    }

    pub fn subject_id(&self) -> i32 {
        self.stored.subject_id
    }

    #[allow(dead_code)]
    pub fn instance_id(&self) -> &str {
        &self.stored.instance_id
    }

    #[allow(dead_code)]
    pub fn kind(&self) -> RuntimeKind {
        self.stored.kind
    }

    pub fn expires_at(&self) -> i64 {
        self.stored.expires_at
    }

    #[allow(dead_code)]
    pub fn permissions(&self) -> &[String] {
        &self.stored.permissions
    }

    pub fn has(&self, permission: TappPermission) -> bool {
        self.stored
            .permissions
            .iter()
            .any(|value| value == permission.as_str())
    }

    pub fn check_permission(&self, permission: TappPermission) -> Result<(), RuntimeGrantError> {
        if self.has(permission) {
            return Ok(());
        }
        Err(RuntimeGrantError::PermissionDenied {
            permission: permission.as_str().to_string(),
        })
    }

    pub fn check_tapp_id(&self, tapp_id: &str) -> Result<(), RuntimeGrantError> {
        if self.tapp_id() == tapp_id {
            Ok(())
        } else {
            Err(RuntimeGrantError::TappMismatch)
        }
    }
}

/// Domain errors for runtime-grant operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeGrantError {
    Unavailable,
    /// Missing, expired, or already revoked token.
    Invalid,
    /// Install owner/visibility no longer matches the grant (same code as Invalid).
    ScopeChanged,
    SubjectMismatch,
    RoleChanged,
    /// Install-state marker: refuse until this install is explicitly re-authorized.
    NeedsReauthorization,
    UnknownPermission {
        permission: String,
    },
    PermissionDenied {
        permission: String,
    },
    TappMismatch,
    InvalidInstanceId,
    LimitExceeded,
}

impl RuntimeGrantError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "RUNTIME_REGISTRY_UNAVAILABLE",
            Self::Invalid | Self::ScopeChanged => "INVALID_RUNTIME_GRANT",
            Self::SubjectMismatch => "RUNTIME_GRANT_SUBJECT_MISMATCH",
            Self::RoleChanged => "RUNTIME_GRANT_ROLE_CHANGED",
            Self::NeedsReauthorization => "TAPP_NEEDS_REAUTHORIZATION",
            Self::UnknownPermission { .. } => "UNKNOWN_TAPP_PERMISSION",
            Self::PermissionDenied { .. } => "RUNTIME_GRANT_PERMISSION_DENIED",
            Self::TappMismatch => "RUNTIME_GRANT_TAPP_MISMATCH",
            Self::InvalidInstanceId => "INVALID_RUNTIME_INSTANCE_ID",
            Self::LimitExceeded => "RUNTIME_GRANT_LIMIT_EXCEEDED",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unavailable => "Runtime registry is unavailable".to_string(),
            Self::Invalid => "Runtime grant is missing, expired or revoked".to_string(),
            Self::ScopeChanged => "Runtime grant installation scope changed".to_string(),
            Self::SubjectMismatch => "Runtime grant subject mismatch".to_string(),
            Self::RoleChanged => {
                "Runtime grant administrator role is no longer current".to_string()
            }
            Self::NeedsReauthorization => {
                "Tapp installation requires permission re-authorization".to_string()
            }
            Self::UnknownPermission { permission } => {
                match tapp_permission_replacement_hint(permission) {
                    Some(hint) => {
                        format!("Unknown Tapp permission '{permission}'; {hint}")
                    }
                    None => format!("Unknown Tapp permission '{permission}'"),
                }
            }
            Self::PermissionDenied { permission } => {
                format!("Runtime grant is missing '{permission}'")
            }
            Self::TappMismatch => "Runtime grant Tapp mismatch".to_string(),
            Self::InvalidInstanceId => "Invalid runtime instanceId".to_string(),
            Self::LimitExceeded => "Too many active Tapp runtimes".to_string(),
        }
    }

    /// HTTP status class for the API adapter.
    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Unavailable => 503,
            Self::Invalid | Self::ScopeChanged => 401,
            Self::UnknownPermission { .. } => 409,
            Self::NeedsReauthorization => 409,
            Self::SubjectMismatch
            | Self::RoleChanged
            | Self::PermissionDenied { .. }
            | Self::TappMismatch => 403,
            Self::InvalidInstanceId => 400,
            Self::LimitExceeded => 429,
        }
    }
}

impl std::fmt::Display for RuntimeGrantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for RuntimeGrantError {}

impl From<UnknownTappPermission> for RuntimeGrantError {
    fn from(error: UnknownTappPermission) -> Self {
        Self::UnknownPermission {
            permission: error.permission,
        }
    }
}

/// Result of successfully issuing a runtime grant.
#[derive(Debug, Clone)]
pub struct IssuedRuntimeGrant {
    pub token: String,
    pub runtime_id: String,
    pub tapp_id: String,
    pub owner_id: i32,
    pub subject_id: i32,
    pub instance_id: String,
    pub kind: RuntimeKind,
    pub permissions: Vec<String>,
    pub expires_at: chrono::DateTime<Utc>,
}

pub async fn active_runtime_grant_count(db: &DatabaseConnection) -> Result<i64, sea_orm::DbErr> {
    shared_registry::count_namespace(db, RUNTIME_GRANT_NAMESPACE).await
}

pub(crate) fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_token() -> String {
    format!("trg_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub(crate) fn valid_instance_id(instance_id: &str) -> bool {
    !instance_id.is_empty()
        && instance_id.len() <= MAX_INSTANCE_ID_LENGTH
        && instance_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(crate) fn intersect_current_permissions(
    issued: &mut Vec<String>,
    currently_allowed: &[String],
) {
    issued.retain(|permission| {
        currently_allowed
            .iter()
            .any(|current| current == permission)
    });
}

fn map_db_err(error: impl std::fmt::Display) -> RuntimeGrantError {
    tracing::error!(%error, "[TAPP] Runtime Grant database unavailable");
    RuntimeGrantError::Unavailable
}

/// 重新授权 fail-closed gate 的最小纯判定。
///
/// 安装仍标 needs_reauthorization 时拒绝。runtime grant 签发（issue）与逐请求
/// rebind（validate）两条生产路径共用此判定，测试直接覆盖它本身。
pub fn refuse_if_needs_reauthorization(
    needs_reauthorization: bool,
) -> Result<(), RuntimeGrantError> {
    if needs_reauthorization {
        Err(RuntimeGrantError::NeedsReauthorization)
    } else {
        Ok(())
    }
}

/// Validate a bearer grant token and rebind permissions to the current install/role.
///
/// `admin_role_revoked` is true when the JWT still claims admin but the live
/// admin check failed — domain deletes the grant and returns [`RuntimeGrantError::RoleChanged`].
pub async fn validate_runtime_grant(
    db: &DatabaseConnection,
    token: &str,
    subject_id: i32,
    admin_role_revoked: bool,
    role: UserRole,
) -> Result<RuntimeGrant, RuntimeGrantError> {
    let hash = token_hash(token);
    let mut grant = shared_registry::get::<StoredRuntimeGrant>(db, RUNTIME_GRANT_NAMESPACE, &hash)
        .await
        .map_err(map_db_err)?
        .ok_or(RuntimeGrantError::Invalid)?;

    if grant.subject_id != subject_id {
        return Err(RuntimeGrantError::SubjectMismatch);
    }
    if admin_role_revoked {
        let _ = shared_registry::delete(db, RUNTIME_GRANT_NAMESPACE, &hash).await;
        return Err(RuntimeGrantError::RoleChanged);
    }

    // A grant is a short-lived upper bound, not a frozen authorization fact.
    // Rebind it to the installation that is visible now and intersect its
    // permissions with the current role/config/installation on every request.
    let tapp = match tapp_ownership::resolve_accessible_tapp(db, subject_id, &grant.tapp_id).await {
        Ok(tapp) if tapp.user_id == grant.owner_id => tapp,
        Ok(_) | Err(_) => {
            let _ = shared_registry::delete(db, RUNTIME_GRANT_NAMESPACE, &hash).await;
            return Err(RuntimeGrantError::ScopeChanged);
        }
    };
    let installed_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions).unwrap_or_default();
    // Refuse the rebind while the install still needs re-authorization.
    refuse_if_needs_reauthorization(tapp.needs_reauthorization)?;
    let currently_allowed = {
        // Worker refresh intervals are not authorization grace periods. Every
        // request observes committed delegation policy and fails closed on DB errors.
        let config = crate::services::config_service::ConfigService::load_permission_config_on(db)
            .await
            .map_err(map_db_err)?;
        TappPermissionService::filter_permissions_for_role(&config, role, &installed_permissions)?
    };
    intersect_current_permissions(&mut grant.permissions, &currently_allowed);

    tracing::debug!(
        runtime_id = %grant.runtime_id,
        tapp_id = %grant.tapp_id,
        owner_id = grant.owner_id,
        subject_id = grant.subject_id,
        instance_id = %grant.instance_id,
        kind = ?grant.kind,
        "[TAPP] Runtime Grant accepted"
    );

    Ok(RuntimeGrant { stored: grant })
}

/// Issue a new short-lived runtime grant for an already-authorized install.
pub async fn issue_runtime_grant(
    db: &DatabaseConnection,
    subject_id: i32,
    tapp_id: &str,
    owner_id: i32,
    instance_id: &str,
    kind: RuntimeKind,
    permissions: Vec<String>,
) -> Result<IssuedRuntimeGrant, RuntimeGrantError> {
    if !valid_instance_id(instance_id) {
        return Err(RuntimeGrantError::InvalidInstanceId);
    }

    let token = new_token();
    let runtime_id = format!("rt_{}", Uuid::new_v4().simple());
    let expires_at = Utc::now()
        + chrono::Duration::from_std(RUNTIME_GRANT_TTL).expect("runtime grant TTL is valid");
    let grant = StoredRuntimeGrant {
        runtime_id: runtime_id.clone(),
        tapp_id: tapp_id.to_string(),
        owner_id,
        subject_id,
        instance_id: instance_id.to_string(),
        kind,
        permissions: permissions.clone(),
        expires_at: expires_at.timestamp(),
    };

    let now = Utc::now().timestamp();
    let txn = db.begin().await.map_err(map_db_err)?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1::BIGINT)",
        vec![(subject_id as i64).into()],
    ))
    .await
    .map_err(map_db_err)?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND (expires_at <= $2 OR (subject_id = $3 AND tapp_id = $4 AND payload->>'instance_id' = $5))",
        vec![
            RUNTIME_GRANT_NAMESPACE.into(),
            now.into(),
            subject_id.into(),
            tapp_id.to_string().into(),
            instance_id.to_string().into(),
        ],
    ))
    .await
    .map_err(map_db_err)?;
    #[derive(FromQueryResult)]
    struct CountRow {
        count: i64,
    }
    let count = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*)::BIGINT AS count FROM tapp_runtime_registry WHERE namespace = $1 AND subject_id = $2 AND expires_at > $3",
        vec![RUNTIME_GRANT_NAMESPACE.into(), subject_id.into(), now.into()],
    ))
    .one(&txn)
    .await
    .map_err(map_db_err)?
    .map_or(0, |row| row.count);
    if count >= MAX_ACTIVE_GRANTS_PER_SUBJECT as i64 {
        txn.rollback().await.ok();
        return Err(RuntimeGrantError::LimitExceeded);
    }
    let payload = serde_json::to_value(&grant).map_err(map_db_err)?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO tapp_runtime_registry
            (namespace, record_id, subject_id, owner_id, tapp_id, runtime_id, payload, expires_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())"#,
        vec![
            RUNTIME_GRANT_NAMESPACE.into(),
            token_hash(&token).into(),
            subject_id.into(),
            owner_id.into(),
            tapp_id.to_string().into(),
            runtime_id.clone().into(),
            payload.into(),
            expires_at.timestamp().into(),
        ],
    ))
    .await
    .map_err(map_db_err)?;
    txn.commit().await.map_err(map_db_err)?;

    Ok(IssuedRuntimeGrant {
        token,
        runtime_id,
        tapp_id: tapp_id.to_string(),
        owner_id,
        subject_id,
        instance_id: instance_id.to_string(),
        kind,
        permissions,
        expires_at,
    })
}

/// Delete grants matching subject/tapp/runtime filters. Returns deleted count.
pub async fn delete_matching_grants(
    db: &DatabaseConnection,
    subject_id: Option<i32>,
    tapp_id: Option<&str>,
    runtime_id: Option<&str>,
) -> Result<usize, RuntimeGrantError> {
    let n = shared_registry::delete_matching(
        db,
        RUNTIME_GRANT_NAMESPACE,
        subject_id,
        tapp_id,
        runtime_id,
    )
    .await
    .map_err(map_db_err)?;
    Ok(n as usize)
}

/// Revoke every active runtime for a Tapp subject (registry only).
pub async fn revoke_tapp_runtime_grants(
    db: &DatabaseConnection,
    subject_id: i32,
    tapp_id: &str,
) -> usize {
    delete_matching_grants(db, Some(subject_id), Some(tapp_id), None)
        .await
        .unwrap_or_else(|error| {
            tracing::error!(%error, "[TAPP] Failed to revoke shared runtime grants");
            0
        })
}

/// Revoke all subjects for an installation that is being removed or replaced (registry only).
pub async fn revoke_all_tapp_runtime_grants(db: &DatabaseConnection, tapp_id: &str) -> usize {
    delete_matching_grants(db, None, Some(tapp_id), None)
        .await
        .unwrap_or_else(|error| {
            tracing::error!(%error, "[TAPP] Failed to revoke shared runtime grants");
            0
        })
}

#[cfg(test)]
mod tests {
    use super::{
        intersect_current_permissions, refuse_if_needs_reauthorization, token_hash,
        valid_instance_id, RuntimeGrantError, RuntimeKind,
    };

    #[test]
    fn runtime_instance_ids_are_bounded_and_path_neutral() {
        assert!(valid_instance_id("page_abcd-1234.widget"));
        assert!(!valid_instance_id(""));
        assert!(!valid_instance_id("../other"));
        assert!(!valid_instance_id(&"x".repeat(101)));
    }

    #[test]
    fn runtime_grant_hashes_are_stable_and_non_reversible() {
        let hash = token_hash("trg_secret");
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, token_hash("trg_secret"));
        assert_ne!(hash, token_hash("trg_other"));
    }

    #[test]
    fn runtime_grant_permissions_only_shrink_after_issuance() {
        let mut issued = vec![
            "platform:read".to_string(),
            "storage:read".to_string(),
            "network:fetch".to_string(),
        ];
        let current = vec!["platform:read".to_string(), "ai:chat".to_string()];

        intersect_current_permissions(&mut issued, &current);

        assert_eq!(issued, vec!["platform:read"]);
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            RuntimeGrantError::Unavailable.code(),
            "RUNTIME_REGISTRY_UNAVAILABLE"
        );
        assert_eq!(RuntimeGrantError::Invalid.code(), "INVALID_RUNTIME_GRANT");
        assert_eq!(
            RuntimeGrantError::ScopeChanged.code(),
            "INVALID_RUNTIME_GRANT"
        );
        assert_eq!(
            RuntimeGrantError::ScopeChanged.message(),
            "Runtime grant installation scope changed"
        );
        assert_eq!(
            RuntimeGrantError::SubjectMismatch.code(),
            "RUNTIME_GRANT_SUBJECT_MISMATCH"
        );
        assert_eq!(
            RuntimeGrantError::RoleChanged.code(),
            "RUNTIME_GRANT_ROLE_CHANGED"
        );
        assert_eq!(
            RuntimeGrantError::NeedsReauthorization.code(),
            "TAPP_NEEDS_REAUTHORIZATION"
        );
        assert_eq!(
            RuntimeGrantError::NeedsReauthorization.message(),
            "Tapp installation requires permission re-authorization"
        );
        assert_eq!(RuntimeGrantError::NeedsReauthorization.status_hint(), 409);
        assert_eq!(
            RuntimeGrantError::UnknownPermission {
                permission: "legacy:unknown".into()
            }
            .code(),
            "UNKNOWN_TAPP_PERMISSION"
        );
        assert_eq!(
            RuntimeGrantError::PermissionDenied {
                permission: "storage:read".into()
            }
            .code(),
            "RUNTIME_GRANT_PERMISSION_DENIED"
        );
        assert_eq!(
            RuntimeGrantError::TappMismatch.code(),
            "RUNTIME_GRANT_TAPP_MISMATCH"
        );
        assert_eq!(
            RuntimeGrantError::InvalidInstanceId.code(),
            "INVALID_RUNTIME_INSTANCE_ID"
        );
        assert_eq!(
            RuntimeGrantError::LimitExceeded.code(),
            "RUNTIME_GRANT_LIMIT_EXCEEDED"
        );
    }

    #[test]
    fn reauthorization_gate_refuses_marked_installs_only() {
        // L2: production issue/validate paths share this pure decision; the
        // test exercises the exact helper the runtime calls.
        assert_eq!(
            refuse_if_needs_reauthorization(true).unwrap_err(),
            RuntimeGrantError::NeedsReauthorization
        );
        assert!(refuse_if_needs_reauthorization(false).is_ok());
    }

    #[test]
    fn runtime_kind_serde_is_lowercase() {
        let json = serde_json::to_string(&RuntimeKind::Headless).unwrap();
        assert_eq!(json, "\"headless\"");
        let back: RuntimeKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RuntimeKind::Headless);
    }

    #[test]
    fn status_hints_match_http_contract() {
        assert_eq!(RuntimeGrantError::Unavailable.status_hint(), 503);
        assert_eq!(RuntimeGrantError::Invalid.status_hint(), 401);
        assert_eq!(RuntimeGrantError::LimitExceeded.status_hint(), 429);
        assert_eq!(RuntimeGrantError::InvalidInstanceId.status_hint(), 400);
        assert_eq!(
            RuntimeGrantError::PermissionDenied {
                permission: "x".into()
            }
            .status_hint(),
            403
        );
    }

    #[test]
    fn unknown_permission_message_uses_shared_replacement_hint() {
        let storage = RuntimeGrantError::UnknownPermission {
            permission: "storage".into(),
        };
        let message = storage.message();
        assert!(message.contains("'storage'"), "{message}");
        assert!(message.contains("storage:read"), "{message}");
        assert!(message.contains("storage:write"), "{message}");
        assert!(message.contains("reinstall"), "{message}");
        // 仍保持 fail-closed 语义与通用错误码。
        assert_eq!(storage.code(), "UNKNOWN_TAPP_PERMISSION");

        let generic = RuntimeGrantError::UnknownPermission {
            permission: "legacy:unknown".into(),
        };
        assert_eq!(
            generic.message(),
            "Unknown Tapp permission 'legacy:unknown'"
        );
    }
}
