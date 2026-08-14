//! Short-lived host grants that bind a runtime request to one Tapp instance.
//!
//! Domain storage / rebind / revoke live in [`crate::services::tapp_runtime_grant`].
//! This module owns Axum extractors, HTTP DTO mapping, and revoke side-effects.

use axum::{
    extract::{FromRef, FromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::{
    middleware::auth::Claims,
    services::permission_service::{TappPermission, TappPermissionService, UnknownTappPermission},
    services::tapp_runtime_grant::{
        self, IssuedRuntimeGrant, RuntimeGrant, RuntimeGrantError, RuntimeKind,
    },
};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::common::{current_tapp_user_role, parse_user_id, resolve_accessible_tapp};

pub use crate::services::tapp_runtime_grant::RUNTIME_GRANT_HEADER;

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

/// Validated runtime identity injected into handlers by the Axum extractor.
///
/// Thin HTTP adapter over [`RuntimeGrant`]; permission checks map domain errors
/// to stable Axum JSON contracts.
#[derive(Debug, Clone)]
pub struct RuntimeGrantContext(RuntimeGrant);

impl RuntimeGrantContext {
    pub fn runtime_id(&self) -> &str {
        self.0.runtime_id()
    }

    pub fn tapp_id(&self) -> &str {
        self.0.tapp_id()
    }

    pub fn owner_id(&self) -> i32 {
        self.0.owner_id()
    }

    pub fn subject_id(&self) -> i32 {
        self.0.subject_id()
    }

    pub fn expires_at(&self) -> i64 {
        self.0.expires_at()
    }

    pub fn has(&self, permission: TappPermission) -> bool {
        self.0.has(permission)
    }

    pub fn require(&self, permission: TappPermission) -> Result<(), HttpError> {
        self.0
            .check_permission(permission)
            .map_err(grant_http_error)
    }

    pub fn require_tapp_id(&self, tapp_id: &str) -> Result<(), HttpError> {
        self.0.check_tapp_id(tapp_id).map_err(grant_http_error)
    }
}

pub(crate) async fn active_runtime_grant_count(
    db: &sea_orm::DatabaseConnection,
) -> Result<i64, sea_orm::DbErr> {
    tapp_runtime_grant::active_runtime_grant_count(db).await
}

fn grant_http_error(err: RuntimeGrantError) -> HttpError {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    match &err {
        RuntimeGrantError::PermissionDenied { permission } => HttpError::from((
            status,
            Json(json!({
                "error": "Permission denied",
                "message": format!("Runtime grant is missing '{permission}'"),
                "code": err.code(),
            })),
        )),
        _ => HttpError::from((
            status,
            Json(json!({
                "error": err.message(),
                "code": err.code(),
            })),
        )),
    }
}

pub(crate) async fn validate_runtime_grant(
    db: &DatabaseConnection,
    token: &str,
    claims: &Claims,
) -> Result<RuntimeGrantContext, HttpError> {
    let subject_id = parse_user_id(claims)?;
    let admin_role_revoked = claims.is_admin
        && crate::middleware::auth::ensure_current_admin_on(claims, db)
            .await
            .is_err();
    let role = current_tapp_user_role(db, claims).await;
    tapp_runtime_grant::validate_runtime_grant(db, token, subject_id, admin_role_revoked, role)
        .await
        .map(RuntimeGrantContext)
        .map_err(grant_http_error)
}

impl<S> FromRequestParts<S> for RuntimeGrantContext
where
    S: Send + Sync,
    DatabaseConnection: FromRef<S>,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let db = DatabaseConnection::from_ref(state);
        let claims = parts.extensions.get::<Claims>().ok_or_else(|| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Authentication context is missing" })),
            ))
        })?;
        let token = parts
            .headers
            .get(RUNTIME_GRANT_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                HttpError::from((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "X-Tapp-Runtime-Grant header is required",
                        "code": "RUNTIME_GRANT_REQUIRED"
                    })),
                ))
            })?;
        validate_runtime_grant(&db, token, claims).await
    }
}

/// POST /api/tapps/{tapp_id}/runtime-grants
pub async fn issue_runtime_grant(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(request): Json<IssueRuntimeGrantRequest>,
) -> Result<Json<RuntimeGrantResponse>, HttpError> {
    let subject_id = parse_user_id(&claims)?;
    let tapp = resolve_accessible_tapp(&db, subject_id, &tapp_id).await?;
    let role = current_tapp_user_role(&db, &claims).await;
    let installed_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions.clone()).unwrap_or_default();
    let permissions = {
        let config = dynamic_config.read().await;
        TappPermissionService::filter_permissions_for_role(&config, role, &installed_permissions)
    }
    .map_err(RuntimeGrantError::from)
    .map_err(grant_http_error)?;

    let issued = tapp_runtime_grant::issue_runtime_grant(
        &db,
        subject_id,
        &tapp_id,
        tapp.user_id,
        &request.instance_id,
        request.kind,
        permissions,
    )
    .await
    .map_err(grant_http_error)?;

    Ok(Json(issued_to_response(issued)))
}

fn issued_to_response(issued: IssuedRuntimeGrant) -> RuntimeGrantResponse {
    RuntimeGrantResponse {
        version: 2,
        token: issued.token,
        runtime_id: issued.runtime_id,
        tapp_id: issued.tapp_id,
        owner_id: issued.owner_id,
        subject_id: issued.subject_id,
        instance_id: issued.instance_id,
        kind: issued.kind,
        permissions: issued.permissions,
        expires_at: issued.expires_at.to_rfc3339(),
    }
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
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    let permission = TappPermission::from_str(&request.permission).ok_or_else(|| {
        // Fail-closed, but route the retired-name guidance through the shared
        // replacement hint so `storage` recommends the split permissions.
        let unknown = UnknownTappPermission {
            permission: request.permission.clone(),
        };
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": unknown.message(),
                "code": unknown.code(),
            })),
        )
    })?;
    runtime_grant.require(permission)?;
    Ok(Json(json!({ "authorized": true })))
}

/// DELETE /api/tapps/{tapp_id}/runtime-grants/{runtime_id}
pub async fn revoke_runtime_grant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, runtime_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    let subject_id = parse_user_id(&claims)?;
    let revoked = tapp_runtime_grant::delete_matching_grants(
        &db,
        Some(subject_id),
        Some(&tapp_id),
        Some(&runtime_id),
    )
    .await
    .map_err(grant_http_error)?
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
/// Registry delete is domain; AI/event/data-exchange teardown stays HTTP-adjacent.
pub async fn revoke_tapp_runtime_grants(
    db: &DatabaseConnection,
    subject_id: i32,
    tapp_id: &str,
) -> usize {
    let revoked = tapp_runtime_grant::revoke_tapp_runtime_grants(db, subject_id, tapp_id).await;
    super::ai_tasks::cancel_tapp_ai_tasks(subject_id, tapp_id).await;
    super::events::disconnect_tapp_events(subject_id, tapp_id).await;
    super::data_exchange::cancel_tapp_data_exchanges(subject_id, tapp_id).await;
    revoked
}

/// Revoke all subjects for an installation that is being removed or replaced.
pub async fn revoke_all_tapp_runtime_grants(db: &DatabaseConnection, tapp_id: &str) -> usize {
    let revoked = tapp_runtime_grant::revoke_all_tapp_runtime_grants(db, tapp_id).await;
    super::ai_tasks::cancel_all_tapp_ai_tasks(tapp_id).await;
    super::events::disconnect_all_tapp_events(tapp_id).await;
    super::data_exchange::cancel_all_tapp_data_exchanges(tapp_id).await;
    revoked
}
