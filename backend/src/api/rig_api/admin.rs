//! The site admin's management of the rig API (`/api/admin/rig-api/*`,
//! behind `admin_middleware`, each handler also an [`AdminClaims`]): tenants
//! and their owners, their keys (issued, shown once, revoked), their own
//! limits, the site's default limits, and what a tenant has used.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use myriad_error::AppError;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::RwLock;

use super::{database_error, key_json, tenant_json};
use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::extract::{AdminClaims, Db};
use crate::services::config_service::ConfigService;
use crate::services::rig_api::keys::{self, IssueError};
use crate::services::rig_api::tenants::{self, MAX_LIMIT, Overrides, TenantError};
use crate::services::rig_api::{Period, quota};

type ApiResult = Result<Json<Value>, HttpError>;

fn tenant_not_found() -> HttpError {
    HttpError(AppError::not_found("Tenant not found"))
}

fn tenant_error(error: TenantError) -> HttpError {
    match error {
        TenantError::OwnerMissing => HttpError(
            AppError::bad_request("Owner account does not exist")
                .with_message("A tenant belongs to an existing site account."),
        ),
        TenantError::InvalidName => HttpError(
            AppError::bad_request("Invalid tenant name").with_message(format!(
                "A tenant's name is 1 to {} characters.",
                tenants::MAX_NAME_CHARS
            )),
        ),
        TenantError::InvalidLimit => invalid_limit(),
        TenantError::Db(error) => database_error("could not write a tenant")(error),
    }
}

fn invalid_limit() -> HttpError {
    HttpError(
        AppError::bad_request("Invalid tenant limit")
            .with_message(format!("A limit is a whole number from 0 to {MAX_LIMIT}.")),
    )
}

/// `limits` for `overrides`, with the site's defaults read once per call.
async fn limits_with(
    db: &sea_orm::DatabaseConnection,
    overrides: Overrides,
    defaults: &mut Option<quota::Limits>,
) -> Result<quota::Limits, HttpError> {
    let defaults = match defaults {
        Some(defaults) => *defaults,
        None => *defaults.insert(
            quota::site_defaults(db)
                .await
                .map_err(database_error("could not read the default limits"))?,
        ),
    };
    Ok(quota::Limits::with(overrides, defaults))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTenant {
    pub owner_user_id: i32,
    pub name: String,
    #[serde(default)]
    pub limits: Overrides,
}

/// `POST /api/admin/rig-api/tenants`.
pub async fn create_tenant(
    Db(db): Db,
    _admin: AdminClaims,
    Json(body): Json<CreateTenant>,
) -> Result<(StatusCode, Json<Value>), HttpError> {
    let tenant = tenants::create(
        &db,
        body.owner_user_id,
        &body.name,
        body.limits,
        crate::clock::now(),
    )
    .await
    .map_err(tenant_error)?;
    let limits = limits_with(&db, tenant.overrides, &mut None).await?;
    tracing::info!(tenant = %tenant.id, owner = tenant.owner_user_id, "[rig-api] tenant created");
    Ok((
        StatusCode::CREATED,
        Json(json!({ "tenant": tenant_json(&tenant, &limits) })),
    ))
}

/// `GET /api/admin/rig-api/tenants`.
pub async fn list_tenants(Db(db): Db, _admin: AdminClaims) -> ApiResult {
    let all = tenants::list(&db)
        .await
        .map_err(database_error("could not list tenants"))?;
    let mut defaults = None;
    let mut out = Vec::with_capacity(all.len());
    for tenant in &all {
        let limits = limits_with(&db, tenant.overrides, &mut defaults).await?;
        out.push(tenant_json(tenant, &limits));
    }
    Ok(Json(json!({ "tenants": out })))
}

/// `GET /api/admin/rig-api/tenants/{id}`: the tenant, its keys (never
/// their secrets) and what it has used.
pub async fn get_tenant(Db(db): Db, Path(id): Path<String>, _admin: AdminClaims) -> ApiResult {
    let tenant = tenants::get(&db, &id)
        .await
        .map_err(database_error("could not read a tenant"))?
        .ok_or_else(tenant_not_found)?;
    let limits = limits_with(&db, tenant.overrides, &mut None).await?;
    let keys = keys::list(&db, &tenant.id)
        .await
        .map_err(database_error("could not list a tenant's keys"))?;
    let usage = quota::usage(&db, &tenant, &limits, crate::clock::now())
        .await
        .map_err(database_error("could not read a tenant's usage"))?;
    Ok(Json(json!({
        "tenant": tenant_json(&tenant, &limits),
        "keys": keys.iter().map(key_json).collect::<Vec<_>>(),
        "usage": usage,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateTenant {
    pub name: Option<String>,
    pub disabled: Option<bool>,
}

/// `PATCH /api/admin/rig-api/tenants/{id}`: rename, disable or enable.
pub async fn update_tenant(
    Db(db): Db,
    Path(id): Path<String>,
    _admin: AdminClaims,
    Json(body): Json<UpdateTenant>,
) -> ApiResult {
    let tenant = tenants::update(
        &db,
        &id,
        body.name.as_deref(),
        body.disabled,
        crate::clock::now(),
    )
    .await
    .map_err(tenant_error)?
    .ok_or_else(tenant_not_found)?;
    let limits = limits_with(&db, tenant.overrides, &mut None).await?;
    Ok(Json(json!({ "tenant": tenant_json(&tenant, &limits) })))
}

/// `PUT /api/admin/rig-api/tenants/{id}/limits`: the tenant's own limits,
/// all of them; one left out or null is the site's default.
pub async fn set_limits(
    Db(db): Db,
    Path(id): Path<String>,
    _admin: AdminClaims,
    Json(body): Json<Overrides>,
) -> ApiResult {
    let tenant = tenants::set_overrides(&db, &id, body, crate::clock::now())
        .await
        .map_err(tenant_error)?
        .ok_or_else(tenant_not_found)?;
    let limits = limits_with(&db, tenant.overrides, &mut None).await?;
    tracing::info!(tenant = %tenant.id, "[rig-api] tenant limits set");
    Ok(Json(json!({ "tenant": tenant_json(&tenant, &limits) })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueKey {
    #[serde(default)]
    pub name: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// `POST /api/admin/rig-api/tenants/{id}/keys`: a new key, in the answer
/// this once (`secret`) and never again. The answer is not to be cached.
pub async fn issue_key(
    Db(db): Db,
    Path(id): Path<String>,
    AdminClaims(claims): AdminClaims,
    Json(body): Json<IssueKey>,
) -> Result<Response, HttpError> {
    let (key, secret) = keys::issue(
        &db,
        &id,
        &body.name,
        body.expires_at,
        claims.durable_user_id(),
        crate::clock::now(),
    )
    .await
    .map_err(|error| match error {
        IssueError::TenantMissing => tenant_not_found(),
        IssueError::ExpiryPast => HttpError(
            AppError::bad_request("Invalid key expiry")
                .with_message("A key's expiry is a time after now."),
        ),
        IssueError::Db(error) => database_error("could not issue a key")(error),
    })?;
    tracing::info!(tenant = %id, key = %key.id, prefix = %key.prefix, "[rig-api] key issued");
    let mut response = (
        StatusCode::CREATED,
        Json(json!({ "key": key_json(&key), "secret": secret })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// `DELETE /api/admin/rig-api/tenants/{id}/keys/{key_id}`: revoke a key.
/// Revoking it again answers the same.
pub async fn revoke_key(
    Db(db): Db,
    Path((id, key_id)): Path<(String, String)>,
    _admin: AdminClaims,
) -> ApiResult {
    let key = keys::revoke(&db, &id, &key_id, crate::clock::now())
        .await
        .map_err(database_error("could not revoke a key"))?
        .ok_or_else(|| HttpError(AppError::not_found("API key not found")))?;
    tracing::info!(tenant = %id, key = %key.id, "[rig-api] key revoked");
    Ok(Json(json!({ "key": key_json(&key) })))
}

/// `GET /api/admin/rig-api/defaults`: the limits a tenant without its own has.
pub async fn get_defaults(Db(db): Db, _admin: AdminClaims) -> ApiResult {
    let defaults = quota::site_defaults(&db)
        .await
        .map_err(database_error("could not read the default limits"))?;
    Ok(Json(json!({ "defaults": defaults })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDefaults {
    pub concurrent_jobs: Option<i32>,
    pub jobs_per_period: Option<i32>,
    pub job_period: Option<Period>,
    pub requests_per_minute: Option<i32>,
}

/// `PUT /api/admin/rig-api/defaults`: the site's default limits; one left
/// out stays as it is. Saved as the `rig_api_*` settings.
pub async fn set_defaults(
    Db(db): Db,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    _admin: AdminClaims,
    Json(body): Json<SetDefaults>,
) -> ApiResult {
    let mut updates = HashMap::new();
    for (key, value) in [
        ("rig_api_concurrent_jobs", body.concurrent_jobs),
        ("rig_api_jobs_per_period", body.jobs_per_period),
        ("rig_api_requests_per_minute", body.requests_per_minute),
    ] {
        if let Some(value) = value {
            if !(0..=MAX_LIMIT).contains(&value) {
                return Err(invalid_limit());
            }
            updates.insert(key.to_owned(), json!(value));
        }
    }
    if let Some(period) = body.job_period {
        updates.insert("rig_api_job_period".to_owned(), json!(period.as_str()));
    }
    let service = ConfigService::new(db.clone());
    if !updates.is_empty() {
        service.update_configs(updates).await.map_err(|error| {
            tracing::error!("[rig-api] could not save the default limits: {error:#}");
            HttpError(AppError::internal("Database error"))
        })?;
        match service.load_config().await {
            Ok(config) => {
                crate::services::config_service::install_dynamic_config(&dynamic_config, config)
                    .await
            }
            Err(error) => tracing::warn!("[rig-api] settings saved but not reloaded: {error:#}"),
        }
    }
    let defaults = quota::site_defaults(&db)
        .await
        .map_err(database_error("could not read the default limits"))?;
    tracing::info!("[rig-api] default limits set");
    Ok(Json(json!({ "defaults": defaults })))
}
