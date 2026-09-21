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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicListeningRequest {
    pub listened_seconds: u32,
}

fn present_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPersonaRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub extra_requirements: String,
    #[serde(default = "default_signals_language")]
    pub language: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPersonaRequest {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default = "default_signals_language")]
    pub language: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestNameRequest {
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub avoid_name: Option<String>,
    #[serde(default)]
    pub name_style: String,
    #[serde(default = "default_signals_language")]
    pub language: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuggestVisualDesignRequest {
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub visual_requirements: String,
    #[serde(default)]
    pub clothing_style: String,
    #[serde(default)]
    pub keep_character: bool,
    #[serde(default)]
    pub regenerate: bool,
    #[serde(default)]
    pub existing_visual_identity: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSignalsRequest {
    consent: bool,
    #[serde(default = "default_signals_language")]
    language: String,
    #[serde(default)]
    regenerate: bool,
}

fn default_signals_language() -> String {
    "en-US".to_string()
}

fn normalize_signals_language(raw: &str) -> &'static str {
    crate::api::reports::locale::normalize_report_locale(raw)
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

async fn report_platform_count(db: &DatabaseConnection, user_id: i32) -> Result<usize, HttpError> {
    merope::report_dna::count_report_platforms(db, user_id)
        .await
        .map_err(|error| persona_store_http("count persona reports", error))
}

async fn require_persona_reports(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<usize, HttpError> {
    let count = report_platform_count(db, user_id).await?;
    if count < merope::report_dna::MIN_PERSONA_REPORTS {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Need at least 3 platform reports",
                "code": "persona_reports_required",
                "reportCount": count,
                "required": merope::report_dna::MIN_PERSONA_REPORTS,
            })),
        )));
    }
    Ok(count)
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
            crate::services::media::publish_local_url(&transaction, url, &[])
                .await
                .map_err(|error| HttpError(error.into()))?,
        ),
        None => None,
    };
    let avatar_url = match saved.avatar_asset_id.as_deref() {
        Some(url) => Some(
            crate::services::media::publish_local_url(&transaction, url, &[])
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
        &[],
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
    crate::services::media::bind_persona(&transaction, None, None, None, &[])
        .await
        .map_err(|error| HttpError(error.into()))?;
    transaction
        .commit()
        .await
        .map_err(|error| persona_store_http("commit persona delete", error))?;
    merope_rig::mirror_active_asset(cleared_asset).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutAddresseeRequest {
    pub do_not_disturb: Option<bool>,
    pub dnd_start: Option<String>,
    pub dnd_end: Option<String>,
}

fn parse_schedule(
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(Option<i32>, Option<i32>), HttpError> {
    let start = start.map(str::trim).filter(|value| !value.is_empty());
    let end = end.map(str::trim).filter(|value| !value.is_empty());
    match (start, end) {
        (None, None) => Ok((None, None)),
        (Some(start), Some(end)) => {
            let start = merope::parse_clock_minute(start).ok_or_else(|| {
                HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid do-not-disturb start time",
                        "code": "dnd_schedule_invalid"
                    })),
                ))
            })?;
            let end = merope::parse_clock_minute(end).ok_or_else(|| {
                HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid do-not-disturb end time",
                        "code": "dnd_schedule_invalid"
                    })),
                ))
            })?;
            Ok((Some(start), Some(end)))
        }
        _ => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Set both start and end, or clear both",
                "code": "dnd_schedule_incomplete"
            })),
        ))),
    }
}

/// PUT /api/agent/addressee — current speaker only. Never another person's state.
pub async fn put_addressee(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<PutAddresseeRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    if !merope::is_logged_in_addressee(user_id) {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Guests cannot change addressee state",
                "code": "login_required"
            })),
        )));
    }
    let mut state = if let Some(do_not_disturb) = body.do_not_disturb {
        merope::set_do_not_disturb(&db, user_id, do_not_disturb)
            .await
            .map_err(|error| persona_store_http("save addressee", error))?
    } else {
        merope::get_or_create_state(&db, user_id)
            .await
            .map_err(|error| persona_store_http("load addressee", error))?
    };
    if body.dnd_start.is_some() || body.dnd_end.is_some() {
        let (start, end) = parse_schedule(body.dnd_start.as_deref(), body.dnd_end.as_deref())?;
        state = merope::set_dnd_schedule(&db, user_id, start, end)
            .await
            .map_err(|error| persona_store_http("save quiet-hours", error))?;
    }
    Ok(Json(json!({
        "mood": state.mood,
        "arousal": state.arousal,
        "activity": merope::current_activity(&state),
        "doNotDisturb": state.do_not_disturb,
        "doNotDisturbActive": merope::effective_do_not_disturb(&state),
        "dndStart": state.dnd_start_minute.and_then(merope::format_clock_minute),
        "dndEnd": state.dnd_end_minute.and_then(merope::format_clock_minute),
    })))
}

/// POST /api/agent/addressee/music-listening — credit a meaningful block of
/// actual playback. The client reports time, never a mood delta; the backend
/// owns the effect, ceiling and cross-process cooldown.
pub async fn post_music_listening(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<MusicListeningRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    if !merope::is_logged_in_addressee(user_id) {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Guests cannot change addressee mood",
                "code": "login_required"
            })),
        )));
    }
    if body.listened_seconds < merope::MUSIC_LISTENING_MIN_SECS {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Listening block is too short",
                "code": "music_listening_too_short",
                "minimumSeconds": merope::MUSIC_LISTENING_MIN_SECS,
            })),
        )));
    }

    let credit = merope::credit_music_listening(&db, user_id, body.listened_seconds)
        .await
        .map_err(|error| persona_store_http("credit music listening", error))?;
    let after = merope::store::affect_from_state(&credit.state);
    let mood = merope::MoodTransition::from_affect(
        &credit.before,
        &after,
        "music_listening",
        credit
            .state
            .updated_at
            .with_timezone(&chrono::Utc)
            .timestamp_millis(),
    );
    Ok(Json(json!({
        "credited": credit.credited,
        "nextCreditInSeconds": credit.next_credit_in_seconds,
        "mood": mood,
        "activity": merope::current_activity(&credit.state),
    })))
}

/// POST /api/agent/persona/signals
/// Distill spoken personality tags from the owner's latest reports. No visual assets.
pub async fn report_signals(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<ReportSignalsRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = require_site_owner(&claims, &db).await?;
    require_persona_reports(&db, user_id).await?;
    if !request.consent {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Explicit consent is required",
                "code": "consent_required"
            })),
        )));
    }
    let language = normalize_signals_language(&request.language);
    let distilled =
        merope::report_dna::distill_report_dna(&db, user_id, language, request.regenerate)
            .await
            .map_err(distill_error)?;
    Ok(Json(json!({
        "reportCount": distilled.report_count,
        "tags": distilled.tags,
        "aiDistilled": distilled.ai_distilled,
    })))
}

fn distill_error(error: merope::report_dna::DistillReportDnaError) -> HttpError {
    match error {
        merope::report_dna::DistillReportDnaError::Db(error) => {
            persona_store_http("load persona reports", error)
        }
    }
}

/// POST /api/agent/persona/name
/// Strict Lite rolls one given name in the selected style. Tags stay out.
pub async fn suggest_name(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<SuggestNameRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = require_site_owner(&claims, &db).await?;
    require_persona_reports(&db, user_id).await?;
    let language = normalize_signals_language(&body.language);
    match merope::onboarding_ai::suggest_display_name(
        &body.gender,
        body.avoid_name.as_deref(),
        language,
        &body.name_style,
    )
    .await
    {
        Ok(name) => Ok(Json(json!({
            "name": name,
        }))),
        Err(error) => Err(onboarding_generation_error(
            "name",
            "Failed to suggest a name",
            error,
        )),
    }
}

/// POST /api/agent/persona/draft
/// Pro writes a structured character persona. No appearance, room, or clothes.
pub async fn draft_persona(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<DraftPersonaRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = require_site_owner(&claims, &db).await?;
    require_persona_reports(&db, user_id).await?;
    let language = normalize_signals_language(&body.language);
    let tags = merope::report_dna::sanitize_onboarding_tags_for_language(&body.tags, language);
    let name = body.name.trim();
    let display = if name.is_empty() { "Arael" } else { name };
    let persona = match merope::onboarding_ai::suggest_persona(
        display,
        language,
        &tags,
        &body.gender,
        &body.extra_requirements,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Err(onboarding_generation_error(
                "persona",
                "Failed to draft a persona",
                error,
            ));
        }
    };
    Ok(Json(json!({
        "persona": persona,
    })))
}

/// POST /api/agent/persona/import
/// Pro rewrites an owner-supplied write-up into the structured persona fields.
pub async fn import_persona(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ImportPersonaRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = require_site_owner(&claims, &db).await?;
    let language = normalize_signals_language(&body.language);
    let source = body.source.trim();
    if source.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Import source is required",
                "code": "import_source_required"
            })),
        )));
    }
    let name = body.name.trim();
    let display = if name.is_empty() { "Arael" } else { name };
    let persona = match merope::onboarding_ai::import_persona(
        display,
        language,
        &body.gender,
        source,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Err(onboarding_generation_error(
                "persona",
                "Failed to import a persona",
                error,
            ));
        }
    };
    Ok(Json(json!({
        "persona": persona,
    })))
}

/// POST /api/agent/persona/visual-design
/// Pro turns the saved persona into one complete upper-body visual identity.
/// The suggestion is returned for owner review and is not persisted here.
pub async fn suggest_visual_design(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<SuggestVisualDesignRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = require_site_owner(&claims, &db).await?;
    let persona = merope::get_persona(&db)
        .await
        .map_err(|error| persona_store_http("load persona", error))?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Save the persona before designing appearance",
                    "code": "persona_required"
                })),
            ))
        })?;
    let structured = persona.persona_json.as_ref().ok_or_else(|| {
        HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Structured persona is required before visual design",
                "code": "structured_persona_required"
            })),
        ))
    })?;
    if !myriad_merope::persona_draft_is_complete(structured) {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Structured persona is incomplete",
                "code": "persona_contract_invalid"
            })),
        )));
    }
    let language = required_visual_language(&body.language).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a supported interface language before generating the visual design",
                "code": "visual_language_required"
            })),
        ))
    })?;
    let gender = required_visual_gender(&body.gender).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a valid gender presentation before generating the visual design",
                "code": "gender_required"
            })),
        ))
    })?;
    let requirements = myriad_merope::normalize_visual_requirements_for_design_with_gender(
        &sanitize_visual_text(
            &body.visual_requirements,
            myriad_merope::MAX_VISUAL_NOTES_CHARS,
            "visualRequirements",
        )?,
        gender,
    );
    let clothing_style =
        myriad_merope::normalize_clothing_style(&body.clothing_style).ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Choose a clothing style before generating the visual design",
                    "code": "clothing_style_required"
                })),
            ))
        })?;
    let explicit_existing = match body.existing_visual_identity.as_ref() {
        Some(value) => {
            let sanitized =
                myriad_merope::sanitize_upper_body_visual_identity(value).ok_or_else(|| {
                    HttpError::from((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "Existing visual identity is incomplete",
                            "code": "visual_identity_invalid"
                        })),
                    ))
                })?;
            Some(
                myriad_merope::normalize_visual_identity_for_prompt_checked(&sanitized)
                    .map_err(|issue| visual_profile_issue(issue.prefixed("visualIdentity")))?,
            )
        }
        None => None,
    };
    let existing =
        (body.regenerate || body.keep_character).then(|| explicit_existing.unwrap_or(Value::Null));
    if body.keep_character && existing.as_ref().is_none_or(Value::is_null) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "An explicit existing visual identity is required to keep the character",
                "code": "visual_identity_invalid"
            })),
        )));
    }
    let identity = match merope::onboarding_ai::suggest_visual_design(
        persona.name.trim(),
        language,
        structured,
        gender,
        clothing_style,
        &requirements,
        existing.as_ref(),
        body.regenerate,
        body.keep_character,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Err(onboarding_generation_error(
                "visual",
                "Failed to design upper-body appearance",
                error,
            ));
        }
    };
    Ok(Json(json!({
        "visualIdentity": identity,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservePortraitVisualRequest {
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub language: String,
}

/// POST /api/agent/persona/visual-from-portrait
/// Pro reads the stored master portrait into visualIdentity + clothingStyle.
/// Suggestion is returned for the import finish write; nothing is persisted here.
pub async fn observe_visual_from_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ObservePortraitVisualRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = require_site_owner(&claims, &db).await?;
    let language = required_visual_language(&body.language).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a supported interface language before reading the portrait",
                "code": "visual_language_required"
            })),
        ))
    })?;
    let gender = required_visual_gender(&body.gender).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a valid gender presentation before reading the portrait",
                "code": "gender_required"
            })),
        ))
    })?;
    let persona = merope::get_persona(&db)
        .await
        .map_err(|error| persona_store_http("load persona", error))?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Upload a master portrait before reading visual features",
                    "code": "portrait_required"
                })),
            ))
        })?;
    let portrait_url = persona.portrait_asset_id.as_deref().ok_or_else(|| {
        HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Upload a master portrait before reading visual features",
                "code": "portrait_required"
            })),
        ))
    })?;
    // An uploaded portrait lives in the media store (`/media/assets/...`), not the
    // image cache, so read it through the loader that covers both. Reading the
    // cache directly reports an uploaded portrait as missing.
    let image = crate::services::image_generation::load_local_reference(portrait_url)
        .await
        .map_err(|error| {
            tracing::error!(%error, "imported portrait bytes missing");
            HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Uploaded master portrait is missing from storage",
                    "code": "portrait_required"
                })),
            ))
        })?;
    let observed =
        match merope::onboarding_ai::observe_visual_from_portrait(language, gender, &image).await {
            Ok(value) => value,
            Err(error) => {
                return Err(onboarding_generation_error(
                    "visual",
                    "Failed to read visual features from the portrait",
                    error,
                ));
            }
        };
    Ok(Json(json!({
        "visualIdentity": observed.visual_identity,
        "clothingStyle": observed.clothing_style,
    })))
}

fn onboarding_error_body(error: &str, code: &str, message: Option<&str>) -> serde_json::Value {
    let mut body = json!({ "error": error, "code": code });
    if let Some(message) = message.map(str::trim).filter(|value| !value.is_empty()) {
        if message != error {
            body["message"] = json!(message);
        }
    }
    body
}

fn onboarding_generation_error(
    kind: &str,
    failed_message: &str,
    error: merope::onboarding_ai::OnboardingAiError,
) -> HttpError {
    use merope::onboarding_ai::OnboardingAiError;
    tracing::error!(%error, kind, "onboarding generation failed");
    let detail = error.public_detail().map(str::to_string);
    match error {
        OnboardingAiError::AnalyzerUnavailable => HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(onboarding_error_body(
                if kind == "name" {
                    "Lite model is unavailable"
                } else {
                    "Pro model is unavailable"
                },
                if kind == "name" {
                    "lite_unavailable"
                } else {
                    "pro_unavailable"
                },
                None,
            )),
        )),
        OnboardingAiError::LanguageMismatch => HttpError::from((
            StatusCode::BAD_GATEWAY,
            Json(onboarding_error_body(
                "Visual design did not match the interface language",
                "visual_design_language",
                None,
            )),
        )),
        OnboardingAiError::UnusableResponse(_) => {
            let (label, code) = match kind {
                "name" => (
                    "The model returned a name without a usable meaning or script",
                    "name_unusable",
                ),
                "persona" => (
                    "The model returned an unusable persona draft",
                    "persona_unusable",
                ),
                _ => (
                    "The model returned an unusable visual design",
                    "visual_design_unusable",
                ),
            };
            HttpError::from((
                StatusCode::BAD_GATEWAY,
                Json(onboarding_error_body(label, code, detail.as_deref())),
            ))
        }
        OnboardingAiError::ProviderFailed(_) => {
            let code = match kind {
                "name" => "name_suggest_failed",
                "persona" => "persona_draft_failed",
                _ => "visual_design_failed",
            };
            HttpError::from((
                StatusCode::BAD_GATEWAY,
                Json(onboarding_error_body(
                    failed_message,
                    code,
                    detail.as_deref(),
                )),
            ))
        }
    }
}

/// Site assets only. The public face must not be able to point off-site, so a
/// scheme, host, or traversal is refused rather than quietly rewritten.
/// Accepts a same-origin path (`/uploads/face.png`) or a bare asset id.
fn sanitize_portrait_asset_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return Some(String::new());
    }
    if value.len() > 512
        || value.contains(':')
        || value.contains("..")
        || value.starts_with("//")
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let bare_id = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if value.starts_with('/') || bare_id {
        Some(value.to_string())
    } else {
        None
    }
}

fn sanitize_structured_persona(
    name: &str,
    value: &Value,
    visual_profile: Option<&Value>,
) -> Result<Value, HttpError> {
    let language = visual_profile
        .and_then(|profile| profile.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("en-US");
    let fallback = myriad_merope::fallback_persona_draft(name, language, &[]);
    let persona = myriad_merope::sanitize_persona_draft(value, &fallback)
        .filter(myriad_merope::persona_draft_is_complete)
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Structured persona is incomplete",
                    "code": "persona_contract_invalid"
                })),
            ))
        })?;
    Ok(persona)
}

fn required_visual_gender(value: &str) -> Option<&str> {
    match value.trim() {
        gender @ ("female" | "male" | "nonbinary" | "unspecified") => Some(gender),
        _ => None,
    }
}

fn required_visual_language(value: &str) -> Option<&'static str> {
    let language = value.trim();
    if language.is_empty() {
        return None;
    }
    let lower = language.to_ascii_lowercase().replace('_', "-");
    if lower.starts_with("zh-tw")
        || lower.starts_with("zh-hk")
        || lower.starts_with("zh-mo")
        || lower.contains("hant")
    {
        Some("zh-TW")
    } else if lower.starts_with("zh") {
        Some("zh-CN")
    } else if lower.starts_with("ja") {
        Some("ja-JP")
    } else if lower.starts_with("en") {
        Some("en-US")
    } else {
        None
    }
}

fn sanitize_visual_profile(value: &Value) -> Result<Value, HttpError> {
    let source = value.as_object().ok_or_else(|| {
        visual_profile_issue(myriad_merope::VisualProfileIssue::new(
            "visualProfile",
            myriad_merope::VisualProfileReason::NotObject,
        ))
    })?;
    let mut profile = Map::new();
    if let Some(gender) = source.get("gender").and_then(Value::as_str) {
        if !matches!(gender, "female" | "male" | "nonbinary" | "unspecified") {
            return Err(visual_profile_issue(
                myriad_merope::VisualProfileIssue::new(
                    "gender",
                    myriad_merope::VisualProfileReason::Invalid,
                ),
            ));
        }
        profile.insert("gender".into(), json!(gender));
    }
    // 显式 `null` 是「清掉」，和 `visualIdentity` 同一套写法。
    // 没有它的话前端无法清除这个字段：`merge_visual_profile` 会把缺席的键从
    // 旧值补上，于是上一次生成挑的服装风格会一直粘在后来的人设身上。
    if source.get("clothingStyle").is_some_and(Value::is_null) {
        profile.insert("clothingStyle".into(), Value::Null);
    } else if let Some(clothing_style) = source.get("clothingStyle").and_then(Value::as_str) {
        let clothing_style =
            myriad_merope::normalize_clothing_style(clothing_style).ok_or_else(|| {
                visual_profile_issue(myriad_merope::VisualProfileIssue::new(
                    "clothingStyle",
                    myriad_merope::VisualProfileReason::UnknownStyle,
                ))
            })?;
        profile.insert("clothingStyle".into(), json!(clothing_style));
    }
    if let Some(language) = source.get("language").and_then(Value::as_str) {
        profile.insert(
            "language".into(),
            json!(normalize_signals_language(language)),
        );
    }
    for (key, max_chars) in [
        ("extraRequirements", myriad_merope::MAX_VISUAL_NOTES_CHARS),
        (
            "personaExtraRequirements",
            myriad_merope::MAX_VISUAL_NOTES_CHARS,
        ),
    ] {
        if let Some(text) = source.get(key).and_then(Value::as_str) {
            let text = sanitize_visual_text(text, max_chars, key)?;
            profile.insert(key.into(), json!(text));
        }
    }
    if let Some(tags) = source.get("sourceTags").and_then(Value::as_array) {
        let tags = tags
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let tags = merope::report_dna::sanitize_onboarding_tags(&tags);
        profile.insert("sourceTags".into(), json!(tags));
    }
    if let Some(wardrobe) = source.get("wardrobe") {
        if wardrobe.is_null() {
            profile.insert("wardrobe".into(), json!([]));
        } else {
            let items = myriad_merope::sanitize_wardrobe_checked(wardrobe)
                .map_err(|issue| visual_profile_issue(issue.prefixed("wardrobe")))?;
            profile.insert("wardrobe".into(), json!(items));
        }
    }
    if let Some(active) = source.get("activeOutfitId") {
        if active.is_null() {
            profile.insert("activeOutfitId".into(), Value::Null);
        } else {
            let id = match active.as_str().map(str::trim) {
                None => {
                    return Err(visual_profile_issue(
                        myriad_merope::VisualProfileIssue::new(
                            "activeOutfitId",
                            myriad_merope::VisualProfileReason::Invalid,
                        ),
                    ));
                }
                Some("") => {
                    return Err(visual_profile_issue(
                        myriad_merope::VisualProfileIssue::new(
                            "activeOutfitId",
                            myriad_merope::VisualProfileReason::Empty,
                        ),
                    ));
                }
                Some(id) if id.chars().count() > myriad_merope::MAX_WARDROBE_ID_CHARS => {
                    return Err(visual_profile_issue(
                        myriad_merope::VisualProfileIssue::new(
                            "activeOutfitId",
                            myriad_merope::VisualProfileReason::TooLong {
                                max_chars: myriad_merope::MAX_WARDROBE_ID_CHARS,
                            },
                        ),
                    ));
                }
                Some(id) if id.chars().any(char::is_control) => {
                    return Err(visual_profile_issue(
                        myriad_merope::VisualProfileIssue::new(
                            "activeOutfitId",
                            myriad_merope::VisualProfileReason::ControlChar,
                        ),
                    ));
                }
                Some(id) => id,
            };
            profile.insert("activeOutfitId".into(), json!(id));
        }
    }
    if let Some(identity) = source.get("visualIdentity") {
        if identity.is_null() {
            profile.insert("visualIdentity".into(), Value::Null);
            profile.entry("wardrobe".to_string()).or_insert(json!([]));
            profile
                .entry("activeOutfitId".to_string())
                .or_insert(Value::Null);
            drop_stale_active_outfit(&mut profile);
            return Ok(Value::Object(profile));
        }
        let mut sanitized = myriad_merope::sanitize_upper_body_visual_identity_checked(identity)
            .map_err(|issue| visual_profile_issue(issue.prefixed("visualIdentity")))?;
        if let Some(style) = profile
            .get("clothingStyle")
            .and_then(Value::as_str)
            .and_then(myriad_merope::normalize_clothing_style)
            .or_else(|| myriad_merope::clothing_style_of(&sanitized))
        {
            profile.insert("clothingStyle".into(), json!(style));
            myriad_merope::stamp_clothing_style(&mut sanitized, style);
        }
        let mut sanitized = myriad_merope::normalize_visual_identity_for_prompt_checked(&sanitized)
            .map_err(|issue| visual_profile_issue(issue.prefixed("visualIdentity")))?;
        if let Some(gender) = profile.get("gender").and_then(Value::as_str) {
            let language = profile
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("en-US");
            if let Some(fixed) =
                myriad_merope::ensure_visual_identity_states_gender(&sanitized, gender, language)
            {
                sanitized = fixed;
            }
        }
        profile.insert("visualIdentity".into(), sanitized);
    }
    drop_stale_active_outfit(&mut profile);
    Ok(Value::Object(profile))
}

fn drop_stale_active_outfit(profile: &mut Map<String, Value>) {
    let Some(id) = profile.get("activeOutfitId").and_then(Value::as_str) else {
        return;
    };
    let known = profile
        .get("wardrobe")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("id").and_then(Value::as_str) == Some(id))
        });
    if !known {
        profile.insert("activeOutfitId".into(), Value::Null);
    }
}

fn finish_wardrobe(mut profile: Value, previous: Option<&Map<String, Value>>) -> Value {
    myriad_merope::ensure_default_wardrobe(&mut profile, previous);
    myriad_merope::reconcile_wardrobe_rigs(&mut profile, previous);
    if let Some(map) = profile.as_object_mut() {
        drop_stale_active_outfit(map);
    }
    profile
}

fn merge_visual_profile(incoming: Value, previous: Option<&Value>) -> Value {
    let Some(previous) = previous.and_then(Value::as_object) else {
        return finish_wardrobe(incoming, None);
    };
    let Some(target) = incoming.as_object() else {
        return incoming;
    };
    let mut merged = target.clone();
    let identity_context_changed = ["gender", "clothingStyle"]
        .iter()
        .any(|key| merged.get(*key).is_some() && merged.get(*key) != previous.get(*key));
    let identity_cleared = merged.get("visualIdentity").is_some_and(Value::is_null);
    for key in [
        "visualIdentity",
        "sourceTags",
        "personaExtraRequirements",
        "clothingStyle",
        "wardrobe",
        "activeOutfitId",
    ] {
        if key == "visualIdentity" && identity_context_changed {
            continue;
        }
        if (key == "wardrobe" || key == "activeOutfitId") && identity_cleared {
            continue;
        }
        if merged.get(key).is_none() {
            if let Some(value) = previous.get(key) {
                merged.insert(key.to_string(), value.clone());
            }
        }
    }
    finish_wardrobe(Value::Object(merged), Some(previous))
}

fn sanitize_visual_text(value: &str, max_chars: usize, field: &str) -> Result<String, HttpError> {
    let value = value.trim();
    if value.chars().count() > max_chars {
        return Err(visual_profile_issue(
            myriad_merope::VisualProfileIssue::new(
                field,
                myriad_merope::VisualProfileReason::TooLong { max_chars },
            ),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(visual_profile_issue(
            myriad_merope::VisualProfileIssue::new(
                field,
                myriad_merope::VisualProfileReason::ControlChar,
            ),
        ));
    }
    Ok(value.to_string())
}

fn visual_profile_issue(issue: myriad_merope::VisualProfileIssue) -> HttpError {
    tracing::warn!(
        field = %issue.field,
        reason = issue.reason.as_str(),
        "visual profile rejected"
    );
    HttpError::from((
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "Visual profile is invalid",
            "code": "visual_profile_invalid",
            "message": issue.message(),
        })),
    ))
}

#[cfg(test)]
mod tests {
    /// 导入的人设不该继承上一次生成留下的视觉痕迹。
    ///
    /// `merge_visual_profile` 会把缺席键从旧值补上，导入必须把这四个键显式写空：
    /// visualIdentity / clothingStyle / sourceTags / personaExtraRequirements。
    #[test]
    fn an_imported_persona_inherits_nothing_from_a_generated_one() {
        let previous = json!({
            "gender": "female",
            "language": "zh-CN",
            "clothingStyle": "uniform",
            "visualIdentity": {"character": {"faceDesign": "生成出来的脸"}},
            "sourceTags": ["生成链选的词条"],
            "personaExtraRequirements": "生成链填的补充",
            "wardrobe": [{ "id": "w-old", "clothingStyle": "uniform" }],
            "activeOutfitId": "w-old"
        });
        // 本测试的 merge 输入（不是完整 OnboardingWizard 提交）。
        let incoming = json!({
            "gender": "female",
            "language": "zh-CN",
            "visualIdentity": null,
            "clothingStyle": null,
            "sourceTags": [],
            "personaExtraRequirements": ""
        });
        let merged = merge_visual_profile(
            sanitize_visual_profile(&incoming).expect("import payload is valid"),
            Some(&previous),
        );

        assert_eq!(merged["visualIdentity"], Value::Null);
        assert_eq!(merged["clothingStyle"], Value::Null);
        assert_eq!(merged["sourceTags"], json!([]));
        assert_eq!(merged["personaExtraRequirements"], json!(""));
        assert_eq!(merged["wardrobe"], json!([]));
        assert_eq!(merged["activeOutfitId"], Value::Null);
        // 身份留着：性别是导入页自己填的，不是继承来的。
        assert_eq!(merged["gender"], json!("female"));
    }

    fn test_outfit() -> Value {
        json!({
            "upperBodySilhouette": "窄肩与清晰领口，胸像轮廓紧凑，左右袖片伸入画面",
            "outfitConstruction": "水手领内搭叠短外套，领巾形成胸前主形，结构止于高腰",
            "sleeveArmDesign": "宽松袖口包住局部前臂，左右形状不完全对称，手可以不出现",
            "materialPlan": "哑光布料为主，丝带带柔和光泽，金属与宝石只用于小面积焦点",
            "heroAccessory": "左侧星形发夹与胸前星形扣形成一次呼应",
            "paletteHint": "粉色头发，淡紫与白为主体，深紫压边，少量金色点缀",
            "motif": "星轨与小型鸟笼，集中在发饰和胸前，不铺满服装"
        })
    }

    #[test]
    fn wardrobe_is_kept_on_partial_visual_saves_and_cleared_with_identity() {
        let item = json!({
            "id": "w-urban",
            "clothingStyle": "urban",
            "outfit": test_outfit()
        });
        let previous = json!({
            "gender": "female",
            "clothingStyle": "urban",
            "wardrobe": [item],
            "activeOutfitId": "w-urban"
        });
        let kept = merge_visual_profile(
            sanitize_visual_profile(&json!({ "gender": "female" })).unwrap(),
            Some(&previous),
        );
        assert_eq!(kept["wardrobe"][0]["id"], "w-urban");
        assert_eq!(kept["activeOutfitId"], "w-urban");

        let cleared = merge_visual_profile(
            sanitize_visual_profile(&json!({
                "gender": "female",
                "visualIdentity": null
            }))
            .unwrap(),
            Some(&previous),
        );
        assert_eq!(cleared["wardrobe"], json!([]));
        assert_eq!(cleared["activeOutfitId"], Value::Null);
    }

    #[test]
    fn empty_wardrobe_with_identity_becomes_the_default_outfit() {
        let identity = json!({
            "character": {
                "faceDesign": "成熟的鹅蛋脸与自然眉形",
                "eyeDesign": "金色多层虹膜与克制高光",
                "hairShape": "银灰齐颌短发与偏分刘海",
                "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓"
            },
            "outfit": test_outfit()
        });
        let profile = merge_visual_profile(
            sanitize_visual_profile(&json!({
                "gender": "female",
                "clothingStyle": "urban",
                "visualIdentity": identity,
                "wardrobe": []
            }))
            .expect("identity can seed the default outfit"),
            None,
        );
        assert_eq!(
            profile["wardrobe"][0]["id"],
            myriad_merope::DEFAULT_WARDROBE_ID
        );
        assert_eq!(
            profile["activeOutfitId"],
            myriad_merope::DEFAULT_WARDROBE_ID
        );
        assert!(profile["wardrobe"][0].get("name").is_none());

        let restored = merge_visual_profile(
            sanitize_visual_profile(&json!({
                "gender": "female",
                "clothingStyle": "idol",
                "visualIdentity": {
                    "character": identity["character"],
                    "outfit": test_outfit()
                },
                "wardrobe": [{
                    "id": "w-new",
                    "clothingStyle": "idol",
                    "outfit": test_outfit()
                }],
                "activeOutfitId": "w-new"
            }))
            .expect("other outfits stay valid"),
            Some(&profile),
        );
        assert_eq!(
            restored["wardrobe"][0]["id"],
            myriad_merope::DEFAULT_WARDROBE_ID
        );
        assert_eq!(restored["wardrobe"][1]["id"], "w-new");
        assert_eq!(restored["activeOutfitId"], "w-new");

        let mut later_outfit = test_outfit();
        later_outfit["outfitConstruction"] =
            json!("敞开领口内搭叠短风衣，胸前只有一条结构线，止于高腰");
        let replaced = merge_visual_profile(
            sanitize_visual_profile(&json!({
                "gender": "female",
                "clothingStyle": "idol",
                "visualIdentity": {
                    "character": identity["character"],
                    "outfit": later_outfit
                },
                "wardrobe": []
            }))
            .expect("empty wardrobe is valid before merge"),
            Some(&profile),
        );
        assert_eq!(
            replaced["wardrobe"][0]["id"],
            myriad_merope::DEFAULT_WARDROBE_ID
        );
        assert_eq!(replaced["wardrobe"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            replaced["wardrobe"][0]["outfit"]["outfitConstruction"],
            profile["wardrobe"][0]["outfit"]["outfitConstruction"]
        );
    }

    /// 显式 `null` 才是清除。少了这一条，前端根本没有办法清掉这个字段。
    #[test]
    fn an_explicit_null_clears_the_clothing_style() {
        let cleared = sanitize_visual_profile(&json!({ "clothingStyle": null }))
            .expect("null is a valid clear");
        assert_eq!(cleared["clothingStyle"], Value::Null);

        let kept = sanitize_visual_profile(&json!({ "clothingStyle": "uniform" }))
            .expect("a real style still normalizes");
        assert_eq!(kept["clothingStyle"], json!("uniform"));

        // 乱填仍然是 400，不会被 null 分支放过去。
        assert!(sanitize_visual_profile(&json!({ "clothingStyle": "not-a-style" })).is_err());
    }

    fn visual_profile_error_body(value: &Value) -> Value {
        sanitize_visual_profile(value)
            .expect_err("invalid visual profile")
            .0
            .to_json()
    }

    #[test]
    fn visual_profile_error_names_the_failing_field() {
        let clothing = visual_profile_error_body(&json!({ "clothingStyle": "not-a-style" }));
        assert_eq!(clothing["code"], "visual_profile_invalid");
        assert_eq!(clothing["error"], "Visual profile is invalid");
        assert_eq!(
            clothing["message"],
            "clothingStyle is not a known clothing style"
        );

        let gender = visual_profile_error_body(&json!({ "gender": "unknown" }));
        assert_eq!(gender["message"], "gender is invalid");

        let mut identity = json!({
            "faceDesign": "成熟的鹅蛋脸与自然眉形",
            "eyeDesign": "金色多层虹膜与克制高光",
            "hairShape": "银灰齐颌短发与偏分刘海",
            "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
            "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
            "outfitConstruction": "高领内搭叠短外套并止于高腰",
            "sleeveArmDesign": "左右袖片携局部前臂进入画面",
            "materialPlan": "哑光布料、银色金属与小面积宝石",
            "heroAccessory": "左胸星轨扣饰",
            "paletteHint": "雾蓝为主、银白为辅、金色点缀",
            "motif": "单一星轨弧线集中在胸前"
        });
        identity
            .as_object_mut()
            .expect("identity")
            .remove("eyeDesign");
        let missing = visual_profile_error_body(&json!({ "visualIdentity": identity }));
        assert_eq!(missing["message"], "visualIdentity.eyeDesign is empty");

        let extra = visual_profile_error_body(&json!({
            "extraRequirements": "a".repeat(myriad_merope::MAX_VISUAL_NOTES_CHARS + 1)
        }));
        assert_eq!(
            extra["message"],
            format!(
                "extraRequirements exceeds {} characters",
                myriad_merope::MAX_VISUAL_NOTES_CHARS
            )
        );

        let wardrobe = visual_profile_error_body(&json!({
            "wardrobe": [{ "id": "w-a" }]
        }));
        assert_eq!(wardrobe["message"], "wardrobe.0.clothingStyle is empty");
    }

    use super::*;

    #[test]
    fn wardrobe_face_is_readable_by_agent_users_and_does_not_wear() {
        let source = include_str!("persona.rs");
        let getter = source
            .split("/// GET /api/agent/wardrobe/{outfit_id}/face")
            .nth(1)
            .expect("wardrobe face")
            .split("/// PUT /api/agent/persona")
            .next()
            .expect("getter body");
        assert!(getter.contains("parse_user_id_with_agent_access"));
        assert!(getter.contains("wardrobe_outfit_face"));
        assert!(!getter.contains("require_site_owner"));
        assert!(!getter.contains("upsert_persona"));
        assert!(!getter.contains("persist_active_asset"));
        let routes = include_str!("routes.rs");
        assert!(routes.contains("/wardrobe/{outfit_id}/face"));
        assert!(routes.contains("get_wardrobe_face"));
    }

    #[test]
    fn put_persona_points_the_live_rig_at_the_worn_outfit() {
        let source = include_str!("persona.rs");
        let put = source
            .split("/// PUT /api/agent/persona")
            .nth(1)
            .expect("PUT persona")
            .split("/// DELETE /api/agent/persona")
            .next()
            .expect("PUT body");
        assert!(
            put.contains("active_outfit_rig_asset_id"),
            "PUT must point the live rig at the worn outfit instead of wiping every saved package"
        );
        assert!(put.contains("persist_active_asset"));
    }

    #[test]
    fn get_persona_returns_arousal_on_both_bodies() {
        let get = include_str!("persona.rs")
            .split("/// PUT /api/agent/persona")
            .next()
            .expect("GET persona");
        assert_eq!(
            get.matches("\"arousal\": arousal").count(),
            2,
            "empty fallback and saved persona GET must both return arousal"
        );
    }

    #[test]
    fn visual_design_requires_an_explicit_valid_gender() {
        assert_eq!(required_visual_gender("female"), Some("female"));
        assert_eq!(required_visual_gender(" male "), Some("male"));
        assert_eq!(required_visual_gender("nonbinary"), Some("nonbinary"));
        assert_eq!(required_visual_gender("unspecified"), Some("unspecified"));
        assert_eq!(required_visual_gender(""), None);
        assert_eq!(required_visual_gender("invalid"), None);
    }

    #[test]
    fn visual_design_requires_an_explicit_supported_language() {
        assert_eq!(required_visual_language("zh-CN"), Some("zh-CN"));
        assert_eq!(required_visual_language("zh-TW"), Some("zh-TW"));
        assert_eq!(required_visual_language("zh-HK"), Some("zh-TW"));
        assert_eq!(required_visual_language(" ja-JP "), Some("ja-JP"));
        assert_eq!(required_visual_language("en-US"), Some("en-US"));
        assert_eq!(required_visual_language(""), None);
        assert_eq!(required_visual_language("fr-FR"), None);
    }

    #[test]
    fn draft_tags_use_onboarding_sanitize() {
        let tags = merope::report_dna::sanitize_onboarding_tags(&[
            "  夜战  ".into(),
            "夜战".into(),
            "喜欢独立游戏".into(),
        ]);
        assert_eq!(tags, vec!["夜战".to_string(), "喜欢独立游戏".to_string()]);
    }

    #[test]
    fn portrait_accepts_site_assets_only() {
        assert_eq!(
            sanitize_portrait_asset_id(" /uploads/face.png "),
            Some("/uploads/face.png".to_string())
        );
        assert_eq!(
            sanitize_portrait_asset_id("asset_1-2.png"),
            Some("asset_1-2.png".to_string())
        );
        assert_eq!(sanitize_portrait_asset_id(""), Some(String::new()));
        assert_eq!(
            sanitize_portrait_asset_id("https://cdn.example.com/a.png"),
            None
        );
        assert_eq!(sanitize_portrait_asset_id("//cdn.example.com/a.png"), None);
        assert_eq!(sanitize_portrait_asset_id("javascript:alert(1)"), None);
        assert_eq!(sanitize_portrait_asset_id("/javascript:alert(1)"), None);
        assert_eq!(sanitize_portrait_asset_id("/uploads/../secret"), None);
        assert_eq!(sanitize_portrait_asset_id("face 1.png"), None);
    }

    #[test]
    fn absent_portrait_keeps_and_null_clears() {
        let keep: PutPersonaRequest =
            serde_json::from_value(json!({ "name": "瞳", "personality": "认真" })).expect("keep");
        assert!(keep.portrait_asset_id.is_none());
        assert!(keep.persona.is_none());
        assert!(keep.visual_profile.is_none());

        let clear: PutPersonaRequest =
            serde_json::from_value(json!({ "name": "瞳", "portraitAssetId": null }))
                .expect("clear");
        assert_eq!(clear.portrait_asset_id, Some(None));

        let clear_contracts: PutPersonaRequest = serde_json::from_value(json!({
            "name": "瞳",
            "persona": null,
            "visualProfile": null
        }))
        .expect("clear contracts");
        assert_eq!(clear_contracts.persona, Some(None));
        assert_eq!(clear_contracts.visual_profile, Some(None));

        let set: PutPersonaRequest =
            serde_json::from_value(json!({ "name": "瞳", "portraitAssetId": "/a.png" }))
                .expect("set");
        assert_eq!(set.portrait_asset_id, Some(Some("/a.png".to_string())));
    }

    #[test]
    fn visual_profile_keeps_generation_inputs_separate_from_spoken_persona() {
        let profile = sanitize_visual_profile(&json!({
            "gender": "nonbinary",
            "language": "zh-Hans",
            "clothingStyle": "fantasy",
            "extraRequirements": "金色眼睛",
            "visualIdentity": {
                "faceDesign": "成熟的鹅蛋脸与自然眉形",
                "eyeDesign": "金色多层虹膜与克制高光",
                "hairShape": "银灰齐颌短发与偏分刘海",
                "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
                "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                "outfitConstruction": "高领内搭叠短外套并止于高腰",
                "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                "materialPlan": "哑光布料、银色金属与小面积宝石",
                "heroAccessory": "左胸星轨扣饰",
                "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                "motif": "单一星轨弧线集中在胸前"
            }
        }))
        .expect("valid profile");
        assert_eq!(profile["language"], "zh-CN");
        assert_eq!(profile["clothingStyle"], "fantasy");
        assert_eq!(profile["extraRequirements"], "金色眼睛");
        assert!(profile["visualIdentity"].get("character").is_some());
        assert!(profile["visualIdentity"].get("outfit").is_some());
        assert_eq!(
            profile["visualIdentity"]["outfit"]["clothingStyle"],
            "fantasy"
        );
        assert_eq!(
            profile["visualIdentity"]["character"]["faceDesign"],
            "中性。成熟的鹅蛋脸与自然眉形"
        );
        assert!(myriad_merope::upper_body_visual_identity_is_complete(
            &profile["visualIdentity"]
        ));

        let kept = merge_visual_profile(
            sanitize_visual_profile(&json!({
                "gender": "nonbinary",
                "language": "zh-CN",
                "sourceTags": [" 慢热 ", "慢热", "嘴硬心软"]
            }))
            .expect("partial profile"),
            Some(&profile),
        );
        assert_eq!(kept["gender"], "nonbinary");
        assert_eq!(kept["sourceTags"], json!(["慢热", "嘴硬心软"]));
        assert_eq!(kept["visualIdentity"], profile["visualIdentity"]);
        assert_eq!(kept["clothingStyle"], "fantasy");
        assert!(kept.get("extraRequirements").is_none());
        assert!(kept.get("personaExtraRequirements").is_none());
    }

    #[test]
    fn visual_profile_explicit_clears_survive_merge_and_context_changes_drop_identity() {
        let previous = json!({
            "gender": "female",
            "language": "zh-CN",
            "clothingStyle": "fantasy",
            "extraRequirements": "金色眼睛",
            "sourceTags": ["慢热"],
            "personaExtraRequirements": "话少",
            "visualIdentity": {
                "character": {
                    "faceDesign": "紧凑柔和的鹅蛋脸与自然眉形",
                    "eyeDesign": "中等偏大的金色多层虹膜与克制高光",
                    "hairShape": "银灰齐颌短发与偏分刘海",
                    "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓"
                },
                "outfit": {
                    "clothingStyle": "fantasy",
                    "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                    "outfitConstruction": "高领内搭叠短外套并止于高腰",
                    "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                    "materialPlan": "哑光布料、银色金属与小面积宝石",
                    "heroAccessory": "左胸星轨扣饰",
                    "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                    "motif": "单一星轨弧线集中在胸前"
                }
            }
        });
        let cleared = merge_visual_profile(
            sanitize_visual_profile(&json!({
                "gender": "female",
                "language": "zh-CN",
                "clothingStyle": "fantasy",
                "extraRequirements": "",
                "sourceTags": [],
                "personaExtraRequirements": "",
                "visualIdentity": null
            }))
            .expect("explicit clears"),
            Some(&previous),
        );
        assert_eq!(cleared["extraRequirements"], "");
        assert_eq!(cleared["sourceTags"], json!([]));
        assert_eq!(cleared["personaExtraRequirements"], "");
        assert!(cleared["visualIdentity"].is_null());

        let changed_gender = merge_visual_profile(
            sanitize_visual_profile(&json!({
                "gender": "male",
                "language": "zh-CN"
            }))
            .expect("changed context"),
            Some(&previous),
        );
        assert!(changed_gender.get("visualIdentity").is_none());
        assert_eq!(changed_gender["clothingStyle"], "fantasy");
    }

    #[test]
    fn visual_profile_copies_outfit_clothing_style_to_root() {
        let profile = sanitize_visual_profile(&json!({
            "gender": "female",
            "language": "zh-CN",
            "visualIdentity": {
                "character": {
                    "faceDesign": "成熟的鹅蛋脸与自然眉形",
                    "eyeDesign": "金色多层虹膜与克制高光",
                    "hairShape": "银灰齐颌短发与偏分刘海",
                    "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓"
                },
                "outfit": {
                    "clothingStyle": "japanese",
                    "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                    "outfitConstruction": "高领内搭叠短外套并止于高腰",
                    "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                    "materialPlan": "哑光布料、银色金属与小面积宝石",
                    "heroAccessory": "左胸星轨扣饰",
                    "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                    "motif": "单一星轨弧线集中在胸前"
                }
            }
        }))
        .expect("modular profile");
        assert_eq!(profile["clothingStyle"], "japanese");
        assert_eq!(
            profile["visualIdentity"]["outfit"]["clothingStyle"],
            "japanese"
        );
        assert_eq!(
            myriad_merope::character_module(&profile["visualIdentity"]).unwrap()["faceDesign"],
            "女性化。成熟的鹅蛋脸与自然眉形"
        );
    }

    #[test]
    fn visual_profile_keeps_persona_seeds() {
        let profile = sanitize_visual_profile(&json!({
            "gender": "male",
            "language": "en-US",
            "personaExtraRequirements": "quieter with strangers",
            "sourceTags": ["Night owl", "Clear boundaries"]
        }))
        .expect("seeds");
        assert_eq!(
            profile["personaExtraRequirements"],
            "quieter with strangers"
        );
        assert_eq!(
            profile["sourceTags"],
            json!(["Night owl", "Clear boundaries"])
        );

        let persona = sanitize_structured_persona(
            "瞳",
            &json!({
                "summary": "安静但对新事物有持续好奇心",
                "temperament": ["安静", "好奇"],
                "likes": ["雨声"],
                "drives": ["理解彼此"],
                "socialStyle": "先听，再回应。",
                "speechStyle": "简洁但温和。"
            }),
            Some(&profile),
        )
        .expect("valid persona");
        assert_eq!(persona["summary"], "安静但对新事物有持续好奇心");
        assert!(persona.get("gender").is_none());
    }
}
