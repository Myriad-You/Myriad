//! The rig API: what outside developers call, under [`PREFIX`], with a key
//! of their tenant's (`middleware::rig_api_key`), and the site admin's
//! management of tenants, keys and limits under `/api/admin/rig-api`
//! ([`admin`]).
//!
//! The rig API's path space is its own, apart from the site's `/api`: no
//! site session reaches it and no key reaches the site.

pub mod admin;

#[cfg(test)]
mod tests;

use axum::{Json, response::Response};
use myriad_error::AppError;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::middleware::rig_api_key::ApiTenant;
use crate::services::rig_api::admission::Refusal;
use crate::services::rig_api::keys::ApiKey;
use crate::services::rig_api::quota::{self, Limits};
use crate::services::rig_api::tenants::Tenant;

/// Where the rig API's routes are, by version.
pub const PREFIX: &str = "/rig-api/v1";

/// The path space the rig API owns, every version of it.
pub const PATH_SPACE: &str = "/rig-api";

pub(crate) fn database_error(context: &'static str) -> impl FnOnce(sea_orm::DbErr) -> HttpError {
    move |error| {
        tracing::error!(%error, "[rig-api] {context}");
        HttpError(AppError::internal("Database error"))
    }
}

pub(crate) fn tenant_json(tenant: &Tenant, limits: &Limits) -> Value {
    json!({
        "id": tenant.id,
        "ownerUserId": tenant.owner_user_id,
        "name": tenant.name,
        "disabled": tenant.disabled(),
        "disabledAt": tenant.disabled_at,
        "createdAt": tenant.created_at,
        "updatedAt": tenant.updated_at,
        "ownLimits": tenant.overrides,
        "limits": limits,
    })
}

pub(crate) fn key_json(key: &ApiKey) -> Value {
    json!({
        "id": key.id,
        "tenantId": key.tenant_id,
        "name": key.name,
        "prefix": key.prefix,
        "createdByUserId": key.created_by_user_id,
        "createdAt": key.created_at,
        "expiresAt": key.expires_at,
        "revokedAt": key.revoked_at,
        "lastUsedAt": key.last_used_at,
        "live": key.live(crate::clock::now()),
    })
}

/// `GET /rig-api/v1/me`: the tenant the key belongs to, its limits and what
/// it has used of them, this request included.
pub async fn me(
    crate::extract::Db(db): crate::extract::Db,
    caller: ApiTenant,
) -> Result<Json<Value>, HttpError> {
    let usage = quota::usage(&db, &caller.tenant, &caller.limits, quota::Clock::Database)
        .await
        .map_err(database_error("could not read a tenant's usage"))?;
    Ok(Json(json!({
        "tenant": { "id": caller.tenant.id, "name": caller.tenant.name },
        "key": {
            "id": caller.key_id,
            "prefix": caller.key_prefix,
            "expiresAt": caller.key_expires_at,
        },
        "limits": caller.limits,
        "usage": usage,
    })))
}

/// The answer to a job the tenant's quotas do not let start: 429 with a
/// code saying which quota, and `Retry-After` when the quota says when it
/// frees (the period's end). A disabled or missing tenant is refused as its
/// key would be.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the rig API does not take jobs")
)]
pub fn job_refused(refusal: &Refusal) -> Response {
    match refusal {
        Refusal::Concurrency { limit, running } => crate::error::app_error_response(
            AppError::too_many_requests("Too many concurrent jobs")
                .with_message(format!(
                    "This tenant may run {limit} jobs at once; wait for one to end."
                ))
                .with_details(json!({ "limit": limit, "running": running })),
        ),
        Refusal::Period {
            limit,
            used,
            resets_at,
            retry_after_secs,
        } => {
            let mut response = crate::error::app_error_response(
                AppError::too_many_requests("Job quota exhausted")
                    .with_message(match resets_at {
                        Some(at) => format!(
                            "This tenant may start {limit} jobs a period; the next period \
                             begins at {at}."
                        ),
                        None => "This tenant may start no jobs.".to_owned(),
                    })
                    .with_details(json!({
                        "limit": limit,
                        "used": used,
                        "resetsAt": resets_at,
                        "retryAfterSeconds": retry_after_secs,
                    })),
            );
            if let Some(value) = retry_after_secs
                .and_then(|secs| axum::http::HeaderValue::from_str(&secs.to_string()).ok())
            {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, value);
            }
            response
        }
        Refusal::TenantDisabled => crate::error::app_error_response(
            AppError::forbidden("Tenant disabled")
                .with_message("This tenant is disabled; its keys are refused."),
        ),
        Refusal::TenantMissing => {
            crate::error::app_error_response(AppError::unauthorized("Invalid API key"))
        }
    }
}
