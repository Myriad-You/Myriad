//! 运行上下文 API
//!
//! Domain payload builders live in [`crate::services::tapp_context`]. This
//! module only resolves Claims, DB profile rows, platform lists, and filesystem
//! cache mtimes before calling pure builders.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    Extension, Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::middleware::auth::{ensure_current_admin_on, Claims};
use crate::services::tapp_api_service::{ApiExecutionContext, TappApiService};
use crate::services::tapp_context::{
    context_app_payload, context_system_payload, context_user_payload, idle_navigation_context,
    idle_player_context,
};

use super::common::get_available_platforms;
use super::runtime_grant::RuntimeGrantContext;

/// Host UI locale from `X-Myriad-Locale` or `Accept-Language` (not hard-coded zh-CN).
fn locale_from_headers(headers: &HeaderMap) -> String {
    if let Some(v) = headers
        .get("x-myriad-locale")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 32)
    {
        return v.to_string();
    }
    if let Some(al) = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|h| h.to_str().ok())
    {
        // Take first tag: "en-US,en;q=0.9" → "en-US"
        let tag = al
            .split(',')
            .next()
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !tag.is_empty() && tag.len() <= 32 {
            return tag.to_string();
        }
    }
    "en-US".to_string()
}

/// Host timezone from `X-Myriad-Timezone` (IANA), default UTC.
fn timezone_from_headers(headers: &HeaderMap) -> String {
    if let Some(v) = headers
        .get("x-myriad-timezone")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 64 && !s.contains(['\n', '\r', ' ']))
    {
        return v.to_string();
    }
    "UTC".to_string()
}

/// GET /api/tapp/context/app
pub async fn get_context_app(
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    _runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    tracing::debug!("[TAPP] get_context_app - User: {}", claims.username);

    let config = dynamic_config.read().await;
    let platforms = get_available_platforms().await;
    let ai_enabled = config.gemini_api_key.is_some() || config.openai_api_key.is_some();
    drop(config);

    Ok(Json(context_app_payload(
        env!("CARGO_PKG_VERSION"),
        ai_enabled,
        &platforms,
        &locale_from_headers(&headers),
    )))
}

/// GET /api/tapp/context/user
pub async fn get_context_user(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    _runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    tracing::debug!("[TAPP] get_context_user - User: {}", claims.username);

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let connected_platforms = get_available_platforms().await;
    let is_current_admin = claims.is_admin && ensure_current_admin_on(&claims, &db).await.is_ok();
    let mut display_name: Option<String> = None;
    let mut avatar_url: Option<String> = None;

    if user_id > 0 {
        // 与 /api/auth/me 共用 services::avatar 的阶梯，两处不再各抄一份
        let sql = format!(
            r#"SELECT u.display_name, {avatar} AS avatar_url
               FROM users u
               WHERE u.id = $1
               LIMIT 1"#,
            avatar = crate::services::avatar::avatar_snapshot_expr("u"),
        );
        if let Ok(Some(row)) = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                [user_id.into()],
            ))
            .await
        {
            display_name = row.try_get("", "display_name").ok();
            // tapp 沙箱同样只该拿到可显示地址（防盗链直链在 iframe 里一样裂）
            avatar_url = crate::services::avatar::proxied_avatar(
                row.try_get::<Option<String>>("", "avatar_url").ok().flatten(),
            );
        }
    }

    Ok(Json(context_user_payload(
        user_id,
        &claims.username,
        display_name,
        avatar_url,
        is_current_admin,
        &connected_platforms,
        &locale_from_headers(&headers),
        &timezone_from_headers(&headers),
    )))
}

/// GET /api/tapp/context/player
pub async fn get_context_player(
    Extension(claims): Extension<Claims>,
    _runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    tracing::debug!("[TAPP] get_context_player - User: {}", claims.username);
    Ok(Json(idle_player_context()))
}

/// GET /api/tapp/context/navigation
pub async fn get_context_navigation(
    Extension(claims): Extension<Claims>,
    _runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    tracing::debug!("[TAPP] get_context_navigation - User: {}", claims.username);
    Ok(Json(idle_navigation_context()))
}

/// GET /api/tapp/context/system
pub async fn get_context_system(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    _runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    tracing::debug!("[TAPP] get_context_system - User: {}", claims.username);

    let db_connected = db.ping().await.is_ok();
    let platforms = get_available_platforms().await;
    let cache_dir = std::path::Path::new("cache/platforms");

    let mut last_fetch: HashMap<String, Option<String>> = HashMap::new();
    let futures: Vec<_> = platforms
        .iter()
        .map(|platform| {
            let file = cache_dir.join(format!("{}_filtered.json", platform));
            async move {
                if file.exists() {
                    if let Ok(metadata) = tokio::fs::metadata(&file).await {
                        if let Ok(modified) = metadata.modified() {
                            let datetime: chrono::DateTime<chrono::Utc> = modified.into();
                            return Some(datetime.to_rfc3339());
                        }
                    }
                }
                None
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;
    for (platform, result) in platforms.iter().zip(results) {
        last_fetch.insert(platform.to_string(), result);
    }

    Ok(Json(context_system_payload(
        db_connected,
        env!("CARGO_PKG_VERSION"),
        &last_fetch,
    )))
}

/// GET /api/tapp/context/geo
pub async fn get_context_geo(
    _runtime_grant: RuntimeGrantContext,
    headers: axum::http::HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> Result<Json<Value>, HttpError> {
    use crate::api::tapp_store::{TappApiAccess, TappApiDef};

    let client_ip = crate::middleware::client_ip::client_ip_from_parts(
        &headers,
        Some(addr.ip()),
        crate::middleware::client_ip::trusted_proxy_headers_enabled(),
    )
    .map(|ip| ip.to_string())
    .unwrap_or_else(|| addr.ip().to_string());

    tracing::debug!("[TAPP] get_context_geo for IP: {}", client_ip);

    let context = ApiExecutionContext {
        user_id: -1,
        owner_id: 0,
        username: "guest".to_string(),
        is_admin: false,
        client_ip: Some(client_ip),
        granted_permissions: vec![],
        ai_model_tier: None,
        credential: None,
    };

    let geo_api = TappApiDef {
        access: TappApiAccess::Public,
        api_type: "builtin".to_string(),
        endpoint: None,
        method: "GET".to_string(),
        headers: None,
        credential: None,
        body_mode: myriad_tapp_contract::manifest::TappHttpBodyMode::Json,
        body: None,
        builtin: Some("geo".to_string()),
        inject: None,
        cache_ttl: 300,
        spoof: None,
        description: Some("Get client geolocation".to_string()),
    };

    let result = TappApiService::execute("system", "geo", &geo_api, None, &context).await;

    if result.success {
        Ok(Json(
            json!({ "success": true, "data": result.data, "cached": result.cached }),
        ))
    } else {
        Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": result.error })),
        )))
    }
}
