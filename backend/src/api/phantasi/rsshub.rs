//! RSSHub instance admin for Phantasi sources.
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::Deserialize;
use serde_json::json;

use crate::error::HttpError;
use crate::extract::AdminClaims;
use crate::models::entities::rsshub_instances;
use crate::services::rsshub_service::RsshubService;
use myriad_error::AppError;

use super::helpers::{admin_user_id, phantasi_http_err};

fn rsshub_mutate_error(error: String) -> HttpError {
    if error.starts_with("Failed to ") {
        return phantasi_http_err(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    if error == "Instance not found" {
        return phantasi_http_err(StatusCode::NOT_FOUND, error);
    }
    if error == "Permission denied" || error.starts_with("Only admins ") {
        return phantasi_http_err(StatusCode::FORBIDDEN, error);
    }
    phantasi_http_err(StatusCode::BAD_REQUEST, error)
}

pub(crate) async fn list_rsshub_instances(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let rsshub_service = RsshubService::new(db);
    if let Err(e) = rsshub_service.ensure_default_instances().await {
        tracing::warn!("[RSSHub] Failed to ensure default instances: {}", e);
    }
    match rsshub_service.get_instances(Some(user_id)).await {
        Ok(instances) => {
            let responses: Vec<rsshub_instances::InstanceResponse> =
                instances.into_iter().map(|i| i.into()).collect();
            Ok(Json(json!({ "success": true, "instances": responses })))
        }
        Err(error) => {
            tracing::error!(%error, "Failed to fetch RSSHub instances");
            Err(phantasi_http_err(StatusCode::INTERNAL_SERVER_ERROR, error))
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct AddRsshubInstanceRequest {
    name: String,
    url: String,
    access_key: Option<String>,
    priority: Option<i32>,
}

pub(crate) async fn add_rsshub_instance(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Json(req): Json<AddRsshubInstanceRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let rsshub_service = RsshubService::new(db);
    match rsshub_service
        .add_instance(
            Some(user_id),
            req.name,
            req.url,
            req.access_key,
            req.priority,
            true,
        )
        .await
    {
        Ok(instance) => {
            let response: rsshub_instances::InstanceResponse = instance.into();
            Ok(Json(json!({ "success": true, "instance": response })))
        }
        Err(error) => Err(rsshub_mutate_error(error)),
    }
}

#[derive(Deserialize)]
pub(crate) struct UpdateRsshubInstanceRequest {
    name: Option<String>,
    url: Option<String>,
    access_key: Option<String>,
    priority: Option<i32>,
    enabled: Option<bool>,
}

pub(crate) async fn update_rsshub_instance(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<UpdateRsshubInstanceRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let rsshub_service = RsshubService::new(db);
    match rsshub_service
        .update_instance(
            id,
            Some(user_id),
            req.name,
            req.url,
            req.access_key,
            req.priority,
            req.enabled,
            true,
        )
        .await
    {
        Ok(instance) => {
            let response: rsshub_instances::InstanceResponse = instance.into();
            Ok(Json(json!({ "success": true, "instance": response })))
        }
        Err(error) => Err(rsshub_mutate_error(error)),
    }
}

pub(crate) async fn delete_rsshub_instance(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let rsshub_service = RsshubService::new(db);
    match rsshub_service
        .delete_instance(id, Some(user_id), true)
        .await
    {
        Ok(()) => Ok(Json(json!({ "success": true }))),
        Err(error) => Err(rsshub_mutate_error(error)),
    }
}

pub(crate) async fn health_check_rsshub_instance(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let rsshub_service = RsshubService::new(db.clone());
    let instance = match rsshub_instances::Entity::find_by_id(id).one(&db).await {
        Ok(Some(i)) => i,
        Ok(None) => {
            return Err(HttpError::from((
                StatusCode::NOT_FOUND,
                Json(AppError::fail_json("Instance not found")),
            )));
        }
        Err(error) => {
            tracing::error!(%error, "Failed to find RSSHub instance");
            return Err(phantasi_http_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to find RSSHub instance",
            ));
        }
    };
    match rsshub_service.health_check(&instance).await {
        Ok(response_time) => Ok(Json(json!({
            "success": true,
            "healthy": true,
            "response_time_ms": response_time
        }))),
        Err(e) => Ok(Json(json!({
            "success": true,
            "healthy": false,
            "error": e
        }))),
    }
}

pub(crate) async fn reset_rsshub_instance(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let rsshub_service = RsshubService::new(db);
    match rsshub_service
        .reset_instance_stats(id, Some(user_id), true)
        .await
    {
        Ok(()) => Ok(Json(json!({ "success": true }))),
        Err(error) => Err(rsshub_mutate_error(error)),
    }
}

pub(crate) async fn health_check_all_rsshub_instances(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let rsshub_service = RsshubService::new(db);
    match rsshub_service.check_all_instances(Some(user_id)).await {
        Ok(()) => Ok(Json(
            json!({ "success": true, "message": "Health check completed" }),
        )),
        Err(error) => {
            tracing::error!(%error, "Failed to check RSSHub instances");
            Err(phantasi_http_err(StatusCode::INTERNAL_SERVER_ERROR, error))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rsshub_http_requires_admin() {
        let src = include_str!("rsshub.rs");
        let impl_src = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(!impl_src.contains("get_user_id_from_headers"));
        assert!(!impl_src.contains("get_user_and_admin_status"));
        for name in [
            "list_rsshub_instances",
            "add_rsshub_instance",
            "update_rsshub_instance",
            "delete_rsshub_instance",
            "health_check_rsshub_instance",
            "reset_rsshub_instance",
            "health_check_all_rsshub_instances",
        ] {
            let start = impl_src
                .find(&format!("pub(crate) async fn {name}"))
                .unwrap_or_else(|| panic!("{name}"));
            let body = &impl_src[start..];
            // The extractor type in the signature is the guard; the binding
            // name (`admin` / `_admin`) is irrelevant.
            let signature = &body[..body.find('{').expect("signature")];
            assert!(
                signature.contains(": AdminClaims,"),
                "{name} must require an admin session"
            );
        }
    }
}
