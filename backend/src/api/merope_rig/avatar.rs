//! 贴纸头像：从确认过的立绘派生 Q 版贴纸。

use axum::{Extension, Json, extract::State, http::StatusCode};
use myriad_merope::{
    MEROPE_STICKER_STYLE_REFERENCE_SHA256, STICKER_AVATAR_CONTRACT_VERSION, STICKER_AVATAR_SIZE,
    build_sticker_avatar_contract, build_sticker_avatar_prompt,
    character_asset_contract_fingerprint,
};
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    ApiError, ApiResult, bad_request, internal_error, portrait_generation_config_error,
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{agent::merope, image_generation},
};

fn sticker_avatar_provider_error(error: image_generation::ImageGenerationError) -> ApiError {
    let code = image_generation::image_generation_failure_code(&error);
    tracing::error!(%error, code, "sticker avatar generation failed");
    (
        if code == "image_provider_rejected" {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::BAD_GATEWAY
        },
        Json(json!({ "error": error.to_string(), "code": code })),
    )
}

async fn release_avatar_generation_lease(db: &DatabaseConnection, token: &str) {
    if let Err(error) = merope::release_avatar_generation(db, token).await {
        tracing::error!(%error, "failed to release sticker avatar generation lease");
    }
}

async fn cleanup_uncommitted_avatar(
    db: &DatabaseConnection,
    persisted: &image_generation::PersistedGeneratedImage,
) {
    if !persisted.created {
        return;
    }
    let is_current = merope::get_persona(db)
        .await
        .ok()
        .flatten()
        .and_then(|persona| persona.avatar_asset_id)
        .is_some_and(|asset_id| asset_id == persisted.url);
    if is_current {
        return;
    }
    if let Err(error) = image_generation::remove_persisted_generated(db, persisted).await {
        tracing::warn!(%error, url = %persisted.url, "failed to remove uncommitted sticker avatar");
    }
}

/// POST /api/merope/rig/avatar
///
/// Derive a Q-sticker from the confirmed master. Master is identity; project
/// logo is style. Both images upload; prompt also pins school/framing/pose/die-cut.
pub async fn generate_sticker_avatar(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let persona = merope::get_persona(&db)
        .await
        .map_err(internal_error)?
        .ok_or_else(portrait_required_for_avatar)?;
    let portrait_asset_id = persona
        .portrait_asset_id
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(portrait_required_for_avatar)?
        .to_string();
    let name = persona.name.trim();
    let visual_profile = persona.visual_profile.clone().unwrap_or(Value::Null);

    let anchor = image_generation::load_local_reference(&portrait_asset_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "stored master portrait is unusable as an avatar anchor");
            (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "The stored master portrait is missing from site storage",
                    "code": "portrait_required"
                })),
            )
        })?;
    let style = crate::services::agent::merope::api::stickers::style_reference()
        .map_err(portrait_generation_config_error)?;

    let contract = build_sticker_avatar_contract(name, &visual_profile, &portrait_asset_id);
    let contract_fingerprint = character_asset_contract_fingerprint(&contract);
    let prompt = build_sticker_avatar_prompt(name, &visual_profile);
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    tracing::info!(
        provider = %config.provider,
        model = %config.model,
        size = STICKER_AVATAR_SIZE,
        prompt_chars = prompt.chars().count(),
        sticker_contract_version = STICKER_AVATAR_CONTRACT_VERSION,
        style_reference_sha256 = MEROPE_STICKER_STYLE_REFERENCE_SHA256,
        "sticker avatar generation started"
    );

    let generation_token = Uuid::new_v4().to_string();
    let pending = json!({
        "token": generation_token,
        "inputFingerprint": contract_fingerprint,
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    // 名字与外观按人设行原样比对：这里不做归一化，锁要和落盘那一步锁同一组值。
    let stored_visual_profile = persona.visual_profile.clone().unwrap_or(Value::Null);
    let acquired = merope::acquire_avatar_generation(
        &db,
        &persona.name,
        &stored_visual_profile,
        &portrait_asset_id,
        &pending,
    )
    .await
    .map_err(internal_error)?;
    if !acquired {
        let current = merope::get_persona(&db).await.map_err(internal_error)?;
        let (message, code) = if current.as_ref().is_some_and(|persona| {
            merope::avatar_generation_is_pending(persona.avatar_generation.as_ref())
        }) {
            (
                "An avatar generation is already in progress",
                "avatar_generation_in_progress",
            )
        } else {
            (
                "Character visual inputs changed before generation started",
                "character_visual_inputs_changed",
            )
        };
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": message, "code": code })),
        ));
    }

    let generated = match crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "sticker-avatar",
        image_generation::generate_image_with_references(
            &config,
            &prompt,
            STICKER_AVATAR_SIZE,
            STICKER_AVATAR_SIZE,
            &[anchor, style],
            Some(image_generation::ImageBackground::Transparent),
        ),
    )
    .await
    {
        Ok(generated) => generated,
        Err(error) => {
            release_avatar_generation_lease(&db, &generation_token).await;
            return Err(sticker_avatar_provider_error(error));
        }
    };
    let actor = match crate::services::media::MediaActor::admin(user_id) {
        Ok(actor) => actor,
        Err(error) => {
            release_avatar_generation_lease(&db, &generation_token).await;
            return Err(internal_error(error.to_string()));
        }
    };
    let persisted = match image_generation::persist_generated_with_status(
        &db,
        crate::services::media::MediaContext::site(
            actor,
            crate::services::media::MediaSource::Generated,
        )
        .with_producer_key(generation_token.clone()),
        generated,
        "avatar",
        crate::services::media::MediaExposure::Private,
    )
    .await
    {
        Ok(persisted) => persisted,
        Err(error) => {
            release_avatar_generation_lease(&db, &generation_token).await;
            return Err(bad_request(&error.to_string()));
        }
    };
    let url = persisted.url.clone();
    let avatar_generation = json!({
        "fingerprint": contract_fingerprint,
        "contract": contract,
        "provider": config.provider,
        "model": config.model,
        "sourcePortraitAssetId": portrait_asset_id,
    });
    let commit = async {
        let transaction = db.begin().await.map_err(internal_error)?;
        let public_url = crate::services::media::normalize_local_url(
            &transaction,
            &url,
            &crate::services::media::upgrade::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        let completed = merope::complete_avatar_generation(
            &transaction,
            &persona.name,
            &stored_visual_profile,
            &portrait_asset_id,
            &generation_token,
            &public_url,
            &avatar_generation,
            user_id,
        )
        .await
        .map_err(internal_error)?;
        if !completed {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "The master portrait changed while the avatar was generating",
                    "code": "character_visual_inputs_changed"
                })),
            ));
        }
        crate::services::media::bind_persona(
            &transaction,
            Some(&portrait_asset_id),
            Some(&public_url),
            Some(&stored_visual_profile),
            &crate::services::media::upgrade::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        transaction.commit().await.map_err(internal_error)?;
        Ok(public_url)
    }
    .await;
    let url = match commit {
        Ok(url) => url,
        Err(error) => {
            release_avatar_generation_lease(&db, &generation_token).await;
            cleanup_uncommitted_avatar(&db, &persisted).await;
            return Err(error);
        }
    };
    Ok(Json(json!({
        "avatarUrl": url,
        "avatarAssetId": url,
        "stickerAvatarContractVersion": STICKER_AVATAR_CONTRACT_VERSION,
        "generationFingerprint": contract_fingerprint,
    })))
}

fn portrait_required_for_avatar() -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "Generate or upload a master portrait before making an avatar",
            "code": "portrait_required"
        })),
    )
}
