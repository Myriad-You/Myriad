//! Short-lived host grants that bind a runtime request to one Tapp instance.

use axum::{
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

use crate::{
    middleware::auth::Claims,
    services::permission_service::{TappPermission, TappPermissionService},
    GLOBAL_DYNAMIC_CONFIG,
};

use super::{
    common::{current_tapp_user_role, parse_user_id, resolve_accessible_tapp},
    shared_registry,
};

pub const RUNTIME_GRANT_HEADER: &str = "x-tapp-runtime-grant";
const RUNTIME_GRANT_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_INSTANCE_ID_LENGTH: usize = 100;
const MAX_ACTIVE_GRANTS_PER_SUBJECT: usize = 128;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Page,
    Widget,
    Headless,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueRuntimeGrantRequest {
    pub instance_id: String,
    pub kind: RuntimeKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeRuntimePermissionRequest {
    pub permission: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeGrantResponse {
    pub version: u8,
    pub token: String,
    pub runtime_id: String,
    pub tapp_id: String,
    pub owner_id: i32,
    pub subject_id: i32,
    pub instance_id: String,
    pub kind: RuntimeKind,
    pub permissions: Vec<String>,
    pub expires_at: String,
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

/// Validated runtime identity injected into handlers by the Axum extractor.
#[derive(Debug, Clone)]
pub struct RuntimeGrantContext(StoredRuntimeGrant);

impl RuntimeGrantContext {
    pub fn runtime_id(&self) -> &str {
        &self.0.runtime_id
    }

    pub fn tapp_id(&self) -> &str {
        &self.0.tapp_id
    }

    pub fn owner_id(&self) -> i32 {
        self.0.owner_id
    }

    pub fn subject_id(&self) -> i32 {
        self.0.subject_id
    }

    pub fn expires_at(&self) -> i64 {
        self.0.expires_at
    }

    pub fn has(&self, permission: TappPermission) -> bool {
        self.0
            .permissions
            .iter()
            .any(|value| value == permission.as_str())
    }

    pub fn require(&self, permission: TappPermission) -> Result<(), (StatusCode, Json<Value>)> {
        if self.has(permission) {
            return Ok(());
        }

        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Permission denied",
                "message": format!("Runtime grant is missing '{}'", permission.as_str()),
                "code": "RUNTIME_GRANT_PERMISSION_DENIED"
            })),
        ))
    }

    pub fn require_tapp_id(&self, tapp_id: &str) -> Result<(), (StatusCode, Json<Value>)> {
        if self.tapp_id() == tapp_id {
            Ok(())
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Runtime grant Tapp mismatch",
                    "code": "RUNTIME_GRANT_TAPP_MISMATCH"
                })),
            ))
        }
    }
}

const RUNTIME_GRANT_NAMESPACE: &str = "runtime_grant";

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_token() -> String {
    format!("trg_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn valid_instance_id(instance_id: &str) -> bool {
    !instance_id.is_empty()
        && instance_id.len() <= MAX_INSTANCE_ID_LENGTH
        && instance_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn intersect_current_permissions(issued: &mut Vec<String>, currently_allowed: &[String]) {
    issued.retain(|permission| {
        currently_allowed
            .iter()
            .any(|current| current == permission)
    });
}

pub(crate) async fn validate_runtime_grant(
    token: &str,
    claims: &Claims,
) -> Result<RuntimeGrantContext, (StatusCode, Json<Value>)> {
    let subject_id = parse_user_id(claims)?;
    let hash = token_hash(token);
    let db = shared_registry::database().await.map_err(|error| {
        tracing::error!(%error, "[TAPP] Runtime Grant database unavailable");
        api_error_unavailable()
    })?;
    let mut grant = shared_registry::get::<StoredRuntimeGrant>(&db, RUNTIME_GRANT_NAMESPACE, &hash)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Runtime Grant lookup failed");
            api_error_unavailable()
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Runtime grant is missing, expired or revoked",
                    "code": "INVALID_RUNTIME_GRANT"
                })),
            )
        })?;

    if grant.subject_id != subject_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Runtime grant subject mismatch",
                "code": "RUNTIME_GRANT_SUBJECT_MISMATCH"
            })),
        ));
    }
    if claims.is_admin
        && crate::middleware::auth::ensure_current_admin(claims)
            .await
            .is_err()
    {
        let _ = shared_registry::delete(&db, RUNTIME_GRANT_NAMESPACE, &hash).await;
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Runtime grant administrator role is no longer current",
                "code": "RUNTIME_GRANT_ROLE_CHANGED"
            })),
        ));
    }

    // A grant is a short-lived upper bound, not a frozen authorization fact.
    // Rebind it to the installation that is visible now and intersect its
    // permissions with the current role/config/installation on every request.
    // This closes the window after role demotion, delegation revocation,
    // installation replacement or a public-namespace owner change.
    let tapp = match resolve_accessible_tapp(&db, subject_id, &grant.tapp_id).await {
        Ok(tapp) if tapp.user_id == grant.owner_id => tapp,
        Ok(_) | Err(_) => {
            let _ = shared_registry::delete(&db, RUNTIME_GRANT_NAMESPACE, &hash).await;
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Runtime grant installation scope changed",
                    "code": "INVALID_RUNTIME_GRANT"
                })),
            ));
        }
    };
    let role = current_tapp_user_role(claims).await;
    let installed_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions).unwrap_or_default();
    let currently_allowed = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        TappPermissionService::filter_permissions_for_role(&config, role, &installed_permissions)
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

    Ok(RuntimeGrantContext(grant))
}

fn api_error_unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "Runtime registry is unavailable",
            "code": "RUNTIME_REGISTRY_UNAVAILABLE"
        })),
    )
}

impl<S> FromRequestParts<S> for RuntimeGrantContext
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let claims = parts.extensions.get::<Claims>().ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Authentication context is missing" })),
            )
        })?;
        let token = parts
            .headers
            .get(RUNTIME_GRANT_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "X-Tapp-Runtime-Grant header is required",
                        "code": "RUNTIME_GRANT_REQUIRED"
                    })),
                )
            })?;
        validate_runtime_grant(token, claims).await
    }
}

/// POST /api/tapps/{tapp_id}/runtime-grants
pub async fn issue_runtime_grant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(request): Json<IssueRuntimeGrantRequest>,
) -> Result<Json<RuntimeGrantResponse>, (StatusCode, Json<Value>)> {
    if !valid_instance_id(&request.instance_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid runtime instanceId",
                "code": "INVALID_RUNTIME_INSTANCE_ID"
            })),
        ));
    }

    let subject_id = parse_user_id(&claims)?;
    let tapp = resolve_accessible_tapp(&db, subject_id, &tapp_id).await?;
    let role = current_tapp_user_role(&claims).await;
    let installed_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions.clone()).unwrap_or_default();
    let permissions = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        TappPermissionService::filter_permissions_for_role(&config, role, &installed_permissions)
    };

    let token = new_token();
    let runtime_id = format!("rt_{}", Uuid::new_v4().simple());
    let expires_at = Utc::now()
        + chrono::Duration::from_std(RUNTIME_GRANT_TTL).expect("runtime grant TTL is valid");
    let grant = StoredRuntimeGrant {
        runtime_id: runtime_id.clone(),
        tapp_id: tapp_id.clone(),
        owner_id: tapp.user_id,
        subject_id,
        instance_id: request.instance_id.clone(),
        kind: request.kind,
        permissions: permissions.clone(),
        expires_at: expires_at.timestamp(),
    };

    let now = Utc::now().timestamp();
    let txn = db.begin().await.map_err(|_| api_error_unavailable())?;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1::BIGINT)",
        vec![(subject_id as i64).into()],
    ))
    .await
    .map_err(|_| api_error_unavailable())?;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND (expires_at <= $2 OR (subject_id = $3 AND tapp_id = $4 AND payload->>'instance_id' = $5))",
        vec![
            RUNTIME_GRANT_NAMESPACE.into(),
            now.into(),
            subject_id.into(),
            tapp_id.clone().into(),
            request.instance_id.clone().into(),
        ],
    ))
    .await
    .map_err(|_| api_error_unavailable())?;
    #[derive(sea_orm::FromQueryResult)]
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
    .map_err(|_| api_error_unavailable())?
    .map_or(0, |row| row.count);
    if count >= MAX_ACTIVE_GRANTS_PER_SUBJECT as i64 {
        txn.rollback().await.ok();
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Too many active Tapp runtimes",
                "code": "RUNTIME_GRANT_LIMIT_EXCEEDED"
            })),
        ));
    }
    let payload = serde_json::to_value(&grant).map_err(|_| api_error_unavailable())?;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO tapp_runtime_registry
            (namespace, record_id, subject_id, owner_id, tapp_id, runtime_id, payload, expires_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())"#,
        vec![
            RUNTIME_GRANT_NAMESPACE.into(),
            token_hash(&token).into(),
            subject_id.into(),
            tapp.user_id.into(),
            tapp_id.clone().into(),
            runtime_id.clone().into(),
            payload.into(),
            expires_at.timestamp().into(),
        ],
    ))
    .await
    .map_err(|_| api_error_unavailable())?;
    txn.commit().await.map_err(|_| api_error_unavailable())?;

    Ok(Json(RuntimeGrantResponse {
        version: 2,
        token,
        runtime_id,
        tapp_id,
        owner_id: tapp.user_id,
        subject_id,
        instance_id: request.instance_id,
        kind: request.kind,
        permissions,
        expires_at: expires_at.to_rfc3339(),
    }))
}

/// POST /api/tapps/{tapp_id}/runtime-grants/authorize
///
/// Browser-hosted capabilities (for example media control and speech) use this
/// endpoint immediately before acting. The Runtime Grant extractor rebinds the
/// token to the current installation, role and delegation config on every call.
pub async fn authorize_runtime_permission(
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
    Json(request): Json<AuthorizeRuntimePermissionRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    let permission = TappPermission::from_str(&request.permission).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Unknown Tapp permission",
                "code": "UNKNOWN_TAPP_PERMISSION"
            })),
        )
    })?;
    runtime_grant.require(permission)?;
    Ok(Json(json!({ "authorized": true })))
}

/// DELETE /api/tapps/{tapp_id}/runtime-grants/{runtime_id}
pub async fn revoke_runtime_grant(
    Extension(claims): Extension<Claims>,
    Path((tapp_id, runtime_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let subject_id = parse_user_id(&claims)?;
    let db = shared_registry::database()
        .await
        .map_err(|_| api_error_unavailable())?;
    let revoked = shared_registry::delete_matching(
        &db,
        RUNTIME_GRANT_NAMESPACE,
        Some(subject_id),
        Some(&tapp_id),
        Some(&runtime_id),
    )
    .await
    .map_err(|_| api_error_unavailable())?
        > 0;
    if revoked {
        super::ai_tasks::cancel_runtime_ai_tasks(&runtime_id).await;
        super::events::disconnect_runtime_events(&runtime_id).await;
        super::agent_interactions::disconnect_runtime_interactions(&runtime_id).await;
        super::data_exchange::cancel_runtime_data_exchanges(subject_id, &tapp_id, &runtime_id)
            .await;
    }
    Ok(Json(json!({
        "success": true,
        "revoked": revoked
    })))
}

/// Revoke every active runtime for a Tapp subject, used by stop/uninstall flows.
pub async fn revoke_tapp_runtime_grants(subject_id: i32, tapp_id: &str) -> usize {
    let revoked = match shared_registry::database().await {
        Ok(db) => shared_registry::delete_matching(
            &db,
            RUNTIME_GRANT_NAMESPACE,
            Some(subject_id),
            Some(tapp_id),
            None,
        )
        .await
        .unwrap_or(0) as usize,
        Err(error) => {
            tracing::error!(%error, "[TAPP] Failed to revoke shared runtime grants");
            0
        }
    };
    super::ai_tasks::cancel_tapp_ai_tasks(subject_id, tapp_id).await;
    super::events::disconnect_tapp_events(subject_id, tapp_id).await;
    super::data_exchange::cancel_tapp_data_exchanges(subject_id, tapp_id).await;
    revoked
}

/// Revoke all subjects for an installation that is being removed or replaced.
pub async fn revoke_all_tapp_runtime_grants(tapp_id: &str) -> usize {
    let revoked = match shared_registry::database().await {
        Ok(db) => shared_registry::delete_matching(
            &db,
            RUNTIME_GRANT_NAMESPACE,
            None,
            Some(tapp_id),
            None,
        )
        .await
        .unwrap_or(0) as usize,
        Err(error) => {
            tracing::error!(%error, "[TAPP] Failed to revoke shared runtime grants");
            0
        }
    };
    super::ai_tasks::cancel_all_tapp_ai_tasks(tapp_id).await;
    super::events::disconnect_all_tapp_events(tapp_id).await;
    super::data_exchange::cancel_all_tapp_data_exchanges(tapp_id).await;
    revoked
}

#[cfg(test)]
mod tests {
    use super::{intersect_current_permissions, token_hash, valid_instance_id};

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
            "storage".to_string(),
            "network:fetch".to_string(),
        ];
        let current = vec!["platform:read".to_string(), "ai:chat".to_string()];

        intersect_current_permissions(&mut issued, &current);

        assert_eq!(issued, vec!["platform:read"]);
    }
}
