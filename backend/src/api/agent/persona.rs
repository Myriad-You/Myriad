//! Site persona — one personality, owner-writable.

use super::*;
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::site_owner::site_owner_user_id;
use crate::services::{agent::merope, merope_rig};
use axum::http::StatusCode;
use myriad_error::AppError;

mod addressee;
mod onboarding;
mod visual_profile;

pub use addressee::*;
pub use onboarding::*;
use visual_profile::*;

fn persona_store_http(context: &'static str, error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, context, "persona store failed");
    HttpError(AppError::internal(format!("Failed to {context}")))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutPersonaRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub personality: String,
    /// Absent keeps the current portrait, explicit `null` clears it.
    #[serde(default, deserialize_with = "present_option")]
    pub portrait_asset_id: Option<Option<String>>,
    /// Absent preserves the document, explicit `null` clears it.
    #[serde(default, deserialize_with = "present_option")]
    pub persona: Option<Option<Value>>,
    /// Absent preserves the document, explicit `null` clears it.
    #[serde(default, deserialize_with = "present_option")]
    pub visual_profile: Option<Option<Value>>,
}

fn present_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn merope_disabled() -> HttpError {
    HttpError::from((
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "Agent persona is disabled",
            "code": "merope_disabled"
        })),
    ))
}

async fn require_merope_enabled() -> Result<(), HttpError> {
    let enabled = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved();
    if enabled {
        Ok(())
    } else {
        Err(merope_disabled())
    }
}

async fn require_site_owner(claims: &Claims, db: &DatabaseConnection) -> Result<i32, HttpError> {
    let user_id = parse_user_id_with_agent_access(claims, db).await?;
    let owner = site_owner_user_id(db).await.map_err(|error| {
        tracing::error!(%error, "[Agent persona] Failed to resolve site owner");
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json("Failed to resolve site owner")),
        ))
    })?;
    if user_id != owner {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only the site owner can change persona",
                "code": "site_owner_required"
            })),
        )));
    }
    Ok(user_id)
}

/// GET /api/agent/persona
pub async fn get_persona(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let owner = site_owner_user_id(&db).await.ok();
    let is_owner = owner == Some(user_id);
    let persona = merope::get_persona(&db)
        .await
        .map_err(|error| persona_store_http("load persona", error))?;

    let (mood, arousal, mood_revision, activity, do_not_disturb, dnd_start, dnd_end, dnd_active) =
        if merope::is_logged_in_addressee(user_id) {
            match merope::get_or_create_state(&db, user_id).await {
                Ok(state) => (
                    state.mood,
                    state.arousal,
                    state.mood_settled_at.timestamp_millis(),
                    merope::current_activity(&state).to_string(),
                    state.do_not_disturb,
                    state.dnd_start_minute.and_then(merope::format_clock_minute),
                    state.dnd_end_minute.and_then(merope::format_clock_minute),
                    merope::effective_do_not_disturb(&state),
                ),
                Err(_) => (70.0, 48.0, 0, "idle".to_string(), false, None, None, false),
            }
        } else {
            (70.0, 48.0, 0, "idle".to_string(), false, None, None, false)
        };

    let report_count = report_platform_count(&db, user_id).await.unwrap_or(0);

    let Some(persona) = persona else {
        let mut body = json!({
            "name": "Arael",
            "portraitAssetId": null,
            "avatarAssetId": null,
            "hasCustomPersona": false,
            "mood": mood,
            "arousal": arousal,
            "moodRevision": mood_revision,
            "activity": activity,
            "doNotDisturb": do_not_disturb,
            "doNotDisturbActive": dnd_active,
            "dndStart": dnd_start,
            "dndEnd": dnd_end,
            "reportCount": report_count,
        });
        if is_owner {
            body["name"] = json!("");
            body["personality"] = json!("");
        }
        return Ok(Json(body));
    };

    let display_name = if persona.name.trim().is_empty() {
        "Arael"
    } else {
        persona.name.trim()
    };
    let mut body = json!({
        "name": display_name,
        "portraitAssetId": persona.portrait_asset_id,
        // 贴纸头像是站点对外那张脸，和主立绘同一层可见性：能开 Agent 面板的人
        // 都读得到，因为通知图标和头像来源都要用它。
        "avatarAssetId": persona.avatar_asset_id,
        "hasCustomPersona": merope::has_custom_persona(&persona),
        "mood": mood,
        "arousal": arousal,
        "moodRevision": mood_revision,
        "activity": activity,
        "doNotDisturb": do_not_disturb,
        "doNotDisturbActive": dnd_active,
        "dndStart": dnd_start,
        "dndEnd": dnd_end,
        "reportCount": report_count,
    });
    if is_owner {
        body["personality"] = json!(persona.personality);
        body["name"] = json!(persona.name);
        body["persona"] = persona.persona_json.unwrap_or(Value::Null);
        body["visualProfile"] = persona.visual_profile.unwrap_or(Value::Null);
        body["portraitGeneration"] = persona.portrait_generation.unwrap_or(Value::Null);
    }
    Ok(Json(body))
}

/// GET /api/agent/wardrobe/{outfit_id}/face
///
/// Chat overlay playback. Any Agent user may read a saved set's public face.
/// This does not change the worn outfit.
pub async fn get_wardrobe_face(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    crate::api::merope_rig::wardrobe_outfit_face(&db, &outfit_id)
        .await
        .map_err(HttpError::from)
}

/// PUT /api/agent/persona
pub async fn put_persona(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PutPersonaRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = require_site_owner(&claims, &db).await?;
    let transaction = db
        .begin()
        .await
        .map_err(|error| persona_store_http("begin persona save", error))?;
    let previous = merope::get_persona_on(&transaction)
        .await
        .map_err(|error| persona_store_http("load persona", error))?;
    let portrait = match body.portrait_asset_id {
        None => merope::PortraitUpdate::Keep,
        Some(None) => merope::PortraitUpdate::Clear,
        Some(Some(raw)) => match sanitize_portrait_asset_id(&raw) {
            Some(cleaned) if cleaned.is_empty() => merope::PortraitUpdate::Clear,
            Some(cleaned) => merope::PortraitUpdate::Set(cleaned),
            None => {
                return Err(HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Portrait must be a site asset",
                        "code": "portrait_not_site_asset"
                    })),
                )));
            }
        },
    };
    let visual_profile = match body.visual_profile.as_ref() {
        None => merope::JsonDocumentUpdate::Keep,
        Some(None) => merope::JsonDocumentUpdate::Clear,
        Some(Some(value)) => {
            let sanitized = sanitize_visual_profile(value)?;
            merope::JsonDocumentUpdate::Set(merge_visual_profile(
                sanitized,
                previous
                    .as_ref()
                    .and_then(|persona| persona.visual_profile.as_ref()),
            ))
        }
    };
    let effective_visual_profile = match &visual_profile {
        merope::JsonDocumentUpdate::Set(value) => Some(value),
        merope::JsonDocumentUpdate::Clear => None,
        merope::JsonDocumentUpdate::Keep => previous
            .as_ref()
            .and_then(|persona| persona.visual_profile.as_ref()),
    };
    let persona = match body.persona.as_ref() {
        None => merope::JsonDocumentUpdate::Keep,
        Some(None) => merope::JsonDocumentUpdate::Clear,
        Some(Some(value)) => merope::JsonDocumentUpdate::Set(sanitize_structured_persona(
            &body.name,
            value,
            effective_visual_profile,
        )?),
    };
    let contract = merope::PersonaContractUpdate {
        persona,
        visual_profile,
        ..merope::PersonaContractUpdate::default()
    };
    let saved = merope::upsert_persona_on(
        &transaction,
        body.name,
        body.personality,
        portrait,
        contract,
        user_id,
    )
    .await
    .map_err(|error| persona_store_http("save persona", error))?;
    let live_rig = myriad_merope::active_outfit_rig_asset_id(saved.visual_profile.as_ref());
    let live_asset = merope_rig::persist_active_asset(&transaction, live_rig.as_deref())
        .await
        .map_err(|error| persona_store_http("update persona portrait", error))?;
    let portrait_url = match saved.portrait_asset_id.as_deref() {
        Some(url) => Some(
            crate::services::media::normalize_local_url(
                &transaction,
                url,
                &crate::services::media::upgrade::configured_origins().await,
            )
            .await
            .map_err(|error| HttpError(error.into()))?,
        ),
        None => None,
    };
    let avatar_url = match saved.avatar_asset_id.as_deref() {
        Some(url) => Some(
            crate::services::media::normalize_local_url(
                &transaction,
                url,
                &crate::services::media::upgrade::configured_origins().await,
            )
            .await
            .map_err(|error| HttpError(error.into()))?,
        ),
        None => None,
    };
    let saved = merope::rewrite_persona_media_urls(
        &transaction,
        saved,
        portrait_url.clone(),
        avatar_url.clone(),
    )
    .await
    .map_err(|error| persona_store_http("rewrite persona media urls", error))?;
    crate::services::media::bind_persona(
        &transaction,
        portrait_url
            .as_deref()
            .or(saved.portrait_asset_id.as_deref()),
        avatar_url.as_deref().or(saved.avatar_asset_id.as_deref()),
        saved.visual_profile.as_ref(),
        &crate::services::media::upgrade::configured_origins().await,
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    transaction
        .commit()
        .await
        .map_err(|error| persona_store_http("commit persona save", error))?;
    merope_rig::mirror_active_asset(live_asset).await;
    Ok(Json(json!({
        "name": saved.name,
        "personality": saved.personality,
        "persona": saved.persona_json,
        "visualProfile": saved.visual_profile,
        "portraitGeneration": saved.portrait_generation,
        "portraitAssetId": saved.portrait_asset_id,
        "hasCustomPersona": merope::has_custom_persona(&saved),
    })))
}

/// DELETE /api/agent/persona
pub async fn delete_persona(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = require_site_owner(&claims, &db).await?;
    let transaction = db
        .begin()
        .await
        .map_err(|error| persona_store_http("begin persona delete", error))?;
    merope::clear_persona_on(&transaction)
        .await
        .map_err(|error| persona_store_http("delete persona", error))?;
    let cleared_asset = merope_rig::persist_active_asset(&transaction, None)
        .await
        .map_err(|error| persona_store_http("clear persona portrait", error))?;
    crate::services::media::bind_persona(
        &transaction,
        None,
        None,
        None,
        &crate::services::media::upgrade::configured_origins().await,
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    transaction
        .commit()
        .await
        .map_err(|error| persona_store_http("commit persona delete", error))?;
    merope_rig::mirror_active_asset(cleared_asset).await;
    merope::forget_in_memory();
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
#[path = "persona_tests.rs"]
mod tests;
