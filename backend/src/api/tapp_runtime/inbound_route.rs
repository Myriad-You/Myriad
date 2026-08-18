//! Public inbound `/tapi/{tappId}/{path}` surface. No Runtime Grant.

use super::common::{check_inbound_anonymous_rate_limit, check_route_verify_rate_limit};
use crate::error::HttpError;
use crate::middleware::client_ip::{client_ip_from_parts, trusted_proxy_headers_enabled};
use crate::models::entities::tapps;
use crate::services::permission_service::TappPermission;
use crate::services::tapp_api_service::{ApiExecutionContext, TappApiService};
use crate::services::tapp_credentials::{self, TappCredentialError};
use crate::services::tapp_declared_api;
use crate::services::tapp_inbound_guard::{self, InboundDenial};
use crate::services::tapp_inbound_route::{
    self, inbound_path, merge_params, normalize_route_method, InboundRouteError,
};
use crate::services::tapp_ownership;
use crate::services::tapp_validation::validate_tapp_id;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::Json;
use myriad_tapp_contract::manifest::TappApiAccess;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::net::SocketAddr;

fn inbound_http_error(error: InboundRouteError) -> HttpError {
    let status =
        StatusCode::from_u16(error.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    HttpError(myriad_error::AppError::new(status, error.code()).with_message(error.message()))
}

fn credential_http_error(error: TappCredentialError) -> HttpError {
    match error {
        TappCredentialError::Missing | TappCredentialError::ReauthorizationRequired => {
            inbound_http_error(InboundRouteError::VerifyInvalid)
        }
        TappCredentialError::InvalidDefinition(_) | TappCredentialError::InvalidValue => HttpError(
            myriad_error::AppError::new(StatusCode::BAD_REQUEST, error.code())
                .with_message(error.message()),
        ),
        TappCredentialError::Encryption | TappCredentialError::Database => HttpError(
            myriad_error::AppError::new(StatusCode::INTERNAL_SERVER_ERROR, error.code())
                .with_message(error.message()),
        ),
    }
}

fn request_client_ip(headers: &HeaderMap, addr: SocketAddr) -> Option<String> {
    client_ip_from_parts(headers, Some(addr.ip()), trusted_proxy_headers_enabled())
        .map(|ip| ip.to_string())
}

/// Other programs never send a site session. Always resolve the public install
/// so a logged-in browser cookie cannot steer `/tapi` onto a private copy.
async fn public_tapp(db: &DatabaseConnection, tapp_id: &str) -> Result<tapps::Model, HttpError> {
    tapp_ownership::find_visible_tapp(db, None, tapp_id)
        .await
        .map_err(|_| inbound_http_error(InboundRouteError::Database))?
        .map(|visible| visible.tapp)
        .ok_or_else(|| inbound_http_error(InboundRouteError::VerifyInvalid))
}

/// GET|POST /tapi/{tapp_id}/{route}
pub async fn execute_inbound_route(
    State(db): State<DatabaseConnection>,
    method: Method,
    headers: HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    Path((tapp_id, route_segment)): Path<(String, String)>,
    uri: Uri,
    body: Bytes,
) -> Result<Json<Value>, HttpError> {
    validate_tapp_id(&tapp_id).map_err(|_| inbound_http_error(InboundRouteError::VerifyInvalid))?;
    let client_ip = request_client_ip(&headers, addr);
    check_inbound_anonymous_rate_limit(&db, client_ip.as_deref(), &tapp_id).await?;
    match tapp_inbound_guard::check_site_inbound_block(&db, client_ip.as_deref())
        .await
        .map_err(|_| inbound_http_error(InboundRouteError::Database))?
    {
        Some(InboundDenial::Blocked { retry_after }) => {
            return Err(inbound_http_error(InboundRouteError::Blocked {
                retry_after,
            }));
        }
        Some(InboundDenial::Paused) | None => {}
    }
    let method_name = normalize_route_method(&method)
        .ok_or_else(|| inbound_http_error(InboundRouteError::VerifyInvalid))?;

    let path = inbound_path(&route_segment);
    let tapp = public_tapp(&db, &tapp_id).await?;
    match tapp_inbound_guard::check_tapp_inbound_access(
        &db,
        tapp.user_id,
        &tapp_id,
        client_ip.as_deref(),
    )
    .await
    .map_err(|_| inbound_http_error(InboundRouteError::Database))?
    {
        Some(InboundDenial::Paused) => {
            return Err(inbound_http_error(InboundRouteError::Paused));
        }
        Some(InboundDenial::Blocked { retry_after }) => {
            return Err(inbound_http_error(InboundRouteError::Blocked {
                retry_after,
            }));
        }
        None => {}
    }
    let cache_key = format!("{}:{}", tapp.user_id, tapp_id);
    let apis = tapp_declared_api::get_tapp_apis(&cache_key, &tapp_id, &tapp.manifest).await;
    let Some((api_name, api_def, route)) = tapp_inbound_route::find_declared_route(&apis, &path)
    else {
        let _ = tapp_inbound_guard::record_verify_failure(
            &db,
            tapp.user_id,
            &tapp_id,
            client_ip.as_deref(),
        )
        .await;
        return Err(inbound_http_error(InboundRouteError::VerifyInvalid));
    };
    if api_def.access != TappApiAccess::Public
        || (api_def.api_type == "builtin"
            && matches!(api_def.builtin.as_deref(), Some("ai:chat" | "ai:generate")))
        || !route.methods.iter().any(|allowed| allowed == method_name)
    {
        let _ = tapp_inbound_guard::record_verify_failure(
            &db,
            tapp.user_id,
            &tapp_id,
            client_ip.as_deref(),
        )
        .await;
        return Err(inbound_http_error(InboundRouteError::VerifyInvalid));
    }

    let credential = tapp_credentials::resolve_credential_by_key(&db, &tapp, &route.verify.key)
        .await
        .map_err(credential_http_error)?;

    let now = chrono::Utc::now().timestamp();
    let query = uri.query();
    let (nonce, _timestamp, _unix) = match tapp_inbound_route::verify_signature(
        route,
        method_name,
        &tapp_id,
        &headers,
        query,
        &body,
        credential.value(),
        now,
    ) {
        Ok(verified) => verified,
        Err(error) => {
            let error = match error {
                InboundRouteError::MethodNotAllowed | InboundRouteError::RouteNotFound => {
                    InboundRouteError::VerifyInvalid
                }
                other => other,
            };
            if matches!(
                error,
                InboundRouteError::VerifyInvalid | InboundRouteError::VerifyExpired
            ) {
                let _ = tapp_inbound_guard::record_verify_failure(
                    &db,
                    tapp.user_id,
                    &tapp_id,
                    client_ip.as_deref(),
                )
                .await;
            }
            return Err(inbound_http_error(error));
        }
    };

    // A valid HMAC is a one-time ticket. Reserve the nonce before later
    // 403/429/DB paths so the same signed request cannot be replayed.
    tapp_inbound_route::consume_nonce(
        &db,
        tapp.user_id,
        &tapp_id,
        &route.verify.key,
        &nonce,
        now,
        route.verify.max_skew_secs,
    )
    .await
    .map_err(inbound_http_error)?;

    if api_def.api_type == "http" {
        let installed = tapp_declared_api::installed_permissions_from_tapp(&tapp);
        if !installed
            .iter()
            .any(|permission| permission == TappPermission::NetworkFetch.as_str())
        {
            return Err(HttpError(
                myriad_error::AppError::new(StatusCode::FORBIDDEN, "TAPP_PERMISSION_NOT_GRANTED")
                    .with_message("Permission 'network:fetch' required"),
            ));
        }
    }

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let params =
        merge_params(query, method_name, &body, content_type).map_err(inbound_http_error)?;

    let declared = tapp_declared_api::declared_settings_from_manifest(&tapp.manifest)
        .map_err(|_| inbound_http_error(InboundRouteError::InvalidParams))?;
    let (outbound_credential, settings) = tokio::try_join!(
        async {
            tapp_credentials::resolve_api_credential(&db, &tapp, api_def)
                .await
                .map_err(credential_http_error)
        },
        async {
            crate::services::tapp_storage::load_declared_setting_values(
                &db,
                tapp.user_id,
                &tapp.tapp_id,
                &declared,
            )
            .await
            .map_err(|_| inbound_http_error(InboundRouteError::Database))
        },
    )?;

    check_route_verify_rate_limit(&db, &tapp_id, &route.verify.key, credential.revision()).await?;

    let context = ApiExecutionContext {
        user_id: -1,
        owner_id: tapp.user_id,
        username: "anonymous".into(),
        is_admin: false,
        client_ip,
        granted_permissions: tapp_declared_api::installed_permissions_from_tapp(&tapp),
        ai_model_tier: tapp_declared_api::ai_model_tier_from_manifest(&tapp.manifest),
        credential: outbound_credential,
        settings,
    };

    let result = TappApiService::execute(&tapp_id, api_name, api_def, Some(params), &context).await;
    if result.success {
        Ok(Json(json!({
            "success": true,
            "data": result.data,
            "cached": result.cached
        })))
    } else {
        Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": result.error })),
        )))
    }
}
