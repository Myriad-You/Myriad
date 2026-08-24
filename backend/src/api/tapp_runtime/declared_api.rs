//! Tapp API 声明系统
//!
//! Catalog/cache/install binding: [`crate::services::tapp_declared_api`].
//! Execution: [`crate::services::tapp_api_service`]. This module owns grant
//! checks, rate limits, client-IP extraction, and Axum DTO mapping.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::middleware::auth::{ensure_current_admin_on, Claims};
use crate::services::permission_service::{TappPermission, UserRole};
use crate::services::tapp_api_service::{ApiExecutionContext, TappApiService};
use crate::services::tapp_credentials::{self, TappCredentialError};
use crate::services::tapp_declared_api::{self, DeclaredApiError};
use crate::services::tapp_ownership::TappAccessError;

use super::common::check_rate_limit;
use super::runtime_grant::RuntimeGrantContext;

fn declared_http_error(err: DeclaredApiError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    match &err {
        DeclaredApiError::Access(access) => {
            // Preserve ownership adapter body shape used by resolve_accessible_tapp.
            match access {
                TappAccessError::PermissionNotGranted { .. } => (
                    status,
                    Json(json!({
                        "error": access.error_code(),
                        "message": access.message(),
                        "code": "TAPP_PERMISSION_NOT_GRANTED"
                    })),
                ),
                TappAccessError::AccessDenied { .. } => (
                    status,
                    Json(json!({
                        "error": access.error_code(),
                        "message": access.message(),
                    })),
                ),
                TappAccessError::Database | TappAccessError::NoAdmin => {
                    (status, Json(json!({ "error": access.error_code() })))
                }
            }
        }
        DeclaredApiError::GrantScopeChanged | DeclaredApiError::UnknownPermission { .. } => (
            status,
            Json(json!({
                "error": err.message(),
                "code": err.code(),
            })),
        ),
        DeclaredApiError::ApiNotFound { .. } => (status, Json(json!({ "error": err.message() }))),
        DeclaredApiError::InvalidUser => (status, Json(json!({ "error": err.message() }))),
    }
}

fn credential_http_error(error: TappCredentialError) -> HttpError {
    use myriad_error::AppError;

    let status = match error {
        TappCredentialError::InvalidDefinition(_) | TappCredentialError::InvalidValue => {
            StatusCode::BAD_REQUEST
        }
        TappCredentialError::Missing | TappCredentialError::ReauthorizationRequired => {
            StatusCode::CONFLICT
        }
        TappCredentialError::Encryption | TappCredentialError::Database => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    HttpError(AppError::new(status, error.code()).with_message(error.message()))
}

/// Tapp 更新/卸载时使缓存失效（path-stable re-export of services domain).
pub async fn invalidate_tapp_apis_cache(tapp_id: &str) {
    tapp_declared_api::invalidate_tapp_apis_cache(tapp_id).await;
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TappApiCallRequest {
    pub params: Option<Value>,
}

/// POST /api/tapp/{tapp_id}/api/{api_name}
pub async fn execute_tapp_api(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    headers: axum::http::HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Path((tapp_id, api_name)): Path<(String, String)>,
    Json(body): Json<TappApiCallRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    tracing::debug!(
        "[TAPP API] Execute {} for tapp {} by user {}",
        api_name,
        tapp_id,
        claims.username
    );

    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| declared_http_error(DeclaredApiError::InvalidUser))?;

    // 1. Resolve the same private-first installation bound into the Runtime Grant.
    let tapp = tapp_declared_api::resolve_declared_api_tapp(
        &db,
        user_id,
        &tapp_id,
        runtime_grant.owner_id(),
    )
    .await
    .map_err(declared_http_error)?;

    // 2. 解析 manifest 中的 APIs（带缓存）
    let manifest_cache_key = format!("{}:{}", tapp.user_id, tapp_id);
    let apis =
        tapp_declared_api::get_tapp_apis(&manifest_cache_key, &tapp_id, &tapp.manifest).await;

    let api_def =
        tapp_declared_api::require_api_def(&apis, &api_name).map_err(declared_http_error)?;
    if api_def.api_type == "http" {
        // Public/protected controls the audience only. Every server-side
        // outbound request remains a network capability and must be present in
        // the live Runtime Grant after current role/config revalidation.
        runtime_grant.require(TappPermission::NetworkFetch)?;
        check_rate_limit(&db, user_id, &tapp_id, &format!("network.fetch:{api_name}")).await?;
    }
    if api_def.api_type == "builtin" {
        match api_def.builtin.as_deref() {
            Some("ai:chat") => runtime_grant.require(TappPermission::AiChat)?,
            Some("ai:generate") => runtime_grant.require(TappPermission::AiGenerate)?,
            _ => {}
        }
    }

    // 3. 读取安装时授权；下面还会按调用者当前角色动态过滤。
    let installed_permissions = tapp_declared_api::installed_permissions_from_tapp(&tapp);

    // 4. 获取客户端 IP
    let client_ip = crate::middleware::client_ip::client_ip_from_parts(
        &headers,
        Some(addr.ip()),
        crate::middleware::client_ip::trusted_proxy_headers_enabled(),
    )
    .map(|ip| ip.to_string());

    // 5. 确定用户角色
    let is_current_admin = claims.is_admin && ensure_current_admin_on(&claims, &db).await.is_ok();
    let role = if is_current_admin {
        UserRole::Admin
    } else if user_id < 0 {
        UserRole::Guest
    } else {
        UserRole::User
    };
    let granted_permissions =
        tapp_declared_api::filter_granted_permissions(installed_permissions, role)
            .await
            .map_err(declared_http_error)?;

    // Resolve host-only credential material only after determining that this
    // caller is part of the API's declared audience. This avoids turning
    // missing/re-authorization errors into a credential-state oracle.
    let caller_may_invoke = match api_def.access {
        crate::api::tapp_store::TappApiAccess::Public => true,
        crate::api::tapp_store::TappApiAccess::Protected => user_id >= 0,
        crate::api::tapp_store::TappApiAccess::Manager => {
            user_id == tapp.user_id || is_current_admin
        }
    };
    let credential = if caller_may_invoke {
        tapp_credentials::resolve_api_credential(&db, &tapp, api_def)
            .await
            .map_err(credential_http_error)?
    } else {
        None
    };

    let settings = if caller_may_invoke {
        let declared = tapp_declared_api::declared_settings_from_manifest(&tapp.manifest).map_err(
            |error| {
                HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "error": error
                    })),
                ))
            },
        )?;
        crate::services::tapp_storage::load_declared_setting_values(
            &db,
            tapp.user_id,
            &tapp.tapp_id,
            &declared,
        )
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to load Tapp settings"
                })),
            ))
        })?
    } else {
        std::collections::BTreeMap::new()
    };

    // 6. 构建执行上下文
    let context = ApiExecutionContext {
        user_id,
        owner_id: tapp.user_id,
        username: claims.username.clone(),
        is_admin: is_current_admin,
        client_ip,
        granted_permissions,
        ai_model_tier: tapp_declared_api::ai_model_tier_from_manifest(&tapp.manifest),
        credential,
        settings,
    };

    // 7. 执行 API
    let result = TappApiService::execute(&tapp_id, &api_name, api_def, body.params, &context).await;

    if result.success {
        Ok(Json(
            json!({ "success": true, "data": result.data, "cached": result.cached }),
        ))
    } else {
        Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": result.error })),
        )))
    }
}

/// GET /api/tapp/{tapp_id}/apis
pub async fn list_tapp_apis(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    tracing::debug!(
        "[TAPP API] List APIs for tapp {} by user {}",
        tapp_id,
        claims.username
    );

    let user_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| declared_http_error(DeclaredApiError::InvalidUser))?;
    let tapp = tapp_declared_api::resolve_declared_api_tapp(
        &db,
        user_id,
        &tapp_id,
        runtime_grant.owner_id(),
    )
    .await
    .map_err(declared_http_error)?;

    let manifest_cache_key = format!("{}:{}", tapp.user_id, tapp_id);
    let apis =
        tapp_declared_api::get_tapp_apis(&manifest_cache_key, &tapp_id, &tapp.manifest).await;
    let api_list = tapp_declared_api::list_api_summaries(&apis);

    Ok(Json(json!({ "success": true, "apis": api_list })))
}
