//! Where her portrait style comes from (`services::persona_style`): the
//! owner reads which source is in use and the last probe of it, asks for a
//! probe, and points the site at a recipe service or back to this build's
//! own. The token is write-only, bound to the service's origin, and no
//! answer carries it. Reading the status makes no outbound call.

use axum::{Extension, Json, extract::State, http::StatusCode};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::collections::HashMap;

use super::{ApiResult, internal_error, require_merope_enabled, require_owner};
use crate::{
    middleware::auth::Claims,
    services::{config_service::ConfigService, persona_style},
};

const MAX_TOKEN_CHARS: usize = 512;

/// Absent leaves a field as it is; `null` or `""` clears it.
fn present<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePersonaStyleRequest {
    /// The recipe service's base URL; cleared goes back to this build's own.
    #[serde(default, deserialize_with = "present")]
    url: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    token: Option<Option<String>>,
}

fn invalid(code: &'static str, message: &'static str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message, "code": code })),
    )
}

pub async fn get_persona_style(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    Ok(Json(persona_style::status(&dynamic)))
}

/// Asks the configured service what it draws with now; the answer is shown
/// by the status for five minutes.
pub async fn probe_persona_style(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    Ok(Json(persona_style::probe(&dynamic).await))
}

pub async fn update_persona_style(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<UpdatePersonaStyleRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let url = match payload.url {
        None => None,
        Some(raw) => Some(
            persona_style::sanitize_service_url(raw.as_deref().unwrap_or("")).ok_or_else(
                || {
                    invalid(
                        "persona_style_url_invalid",
                        "Use an https URL, or http only to a private or loopback host; no link-local address, credentials, query or fragment",
                    )
                },
            )?,
        ),
    };
    let token = match payload.token {
        None => None,
        Some(raw) => {
            let token = raw.as_deref().unwrap_or("").trim().to_string();
            if token.chars().count() > MAX_TOKEN_CHARS
                || !token.bytes().all(|byte| byte.is_ascii_graphic())
            {
                return Err(invalid(
                    "persona_style_token_invalid",
                    "The token must be printable ASCII without spaces, at most 512 characters",
                ));
            }
            Some(token)
        }
    };
    let current = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_persona_url
        .clone();
    let token =
        persona_style::token_after_url_change(current.as_deref(), url.as_deref(), token.as_deref());
    let mut updates = HashMap::new();
    if let Some(url) = &url {
        updates.insert("merope_persona_url".to_string(), json!(url));
    }
    if let Some(token) = &token {
        updates.insert("merope_persona_token".to_string(), json!(token));
    }
    ConfigService::new(db)
        .update_configs(updates)
        .await
        .map_err(internal_error)?;
    let dynamic = {
        let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
        if let Some(url) = url {
            config.merope_persona_url = Some(url).filter(|url| !url.is_empty());
        }
        if let Some(token) = token {
            config.merope_persona_token = Some(token).filter(|token| !token.is_empty());
        }
        config.clone()
    };
    persona_style::forget_answers();
    Ok(Json(persona_style::status(&dynamic)))
}
