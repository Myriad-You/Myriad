//! 立绘：生成、按说明修改。换立绘同一事务里作废旧装配；上传见 `portrait_upload`。

use axum::{Extension, Json, extract::State, http::StatusCode};
use myriad_merope::{
    CharacterAssetProfile, MEROPE_STYLE_REFERENCE_SHA256, MEROPE_VISUAL_SCHOOL_VERSION,
    build_character_asset_contract, build_character_visual_edit_prompt,
    build_character_visual_prompt, character_asset_contract_fingerprint,
};
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    ApiResult, bad_request, internal_error, portrait_generation_config_error,
    portrait_generation_provider_error, require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{agent::merope, image_generation, merope_rig},
};

const MEROPE_STYLE_REFERENCE_BYTES: &[u8] =
    include_bytes!("../../../assets/merope/style-reference.png");

pub(super) fn merope_style_reference()
-> Result<image_generation::ImageReference, image_generation::ImageGenerationError> {
    image_generation::ImageReference::new(
        axum::body::Bytes::from_static(MEROPE_STYLE_REFERENCE_BYTES),
        "image/png",
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePortraitRequest {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    edit: bool,
}

fn sanitize_portrait_adjustment(raw: Option<&str>) -> ApiResult<Option<String>> {
    let Some(text) = raw.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > 2_000 || text.chars().any(char::is_control) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Portrait adjustment is invalid",
                "code": "portrait_adjustment_invalid"
            })),
        ));
    }
    if !myriad_merope::portrait_adjustment_is_within_scope(text) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Portrait adjustments may change only lighting, expression, or frame occupancy",
                "code": "portrait_adjustment_out_of_scope"
            })),
        ));
    }
    Ok(Some(text.to_string()))
}

/// Keep the worn outfit's picture in sync even when optional visual analysis fails.
pub(super) fn bind_worn_portrait(profile: &mut Value, portrait: &str, fingerprint: Option<&str>) {
    myriad_merope::detach_active_outfit_rig(profile);
    let Some(active) = profile
        .get("activeOutfitId")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    if let Some(items) = profile.get_mut("wardrobe").and_then(Value::as_array_mut) {
        for item in items {
            if item.get("id").and_then(Value::as_str) == Some(active.as_str()) {
                if let Some(item) = item.as_object_mut() {
                    item.insert("portraitAssetId".into(), json!(portrait));
                    if let Some(fingerprint) = fingerprint {
                        item.insert("generationFingerprint".into(), json!(fingerprint));
                    }
                }
                break;
            }
        }
    }
}

async fn bind_generated_portrait<C>(
    db: &C,
    user_id: i32,
    portrait: &str,
    fingerprint: &str,
) -> ApiResult<()>
where
    C: sea_orm::ConnectionTrait,
{
    let Some(row) = merope::get_persona_on(db).await.map_err(internal_error)? else {
        return Ok(());
    };
    let Some(mut profile) = row.visual_profile.clone() else {
        return Ok(());
    };
    bind_worn_portrait(&mut profile, portrait, Some(fingerprint));
    merope::upsert_persona_on(
        db,
        row.name,
        row.personality,
        merope::PortraitUpdate::Keep,
        merope::PersonaContractUpdate {
            visual_profile: merope::JsonDocumentUpdate::Set(profile),
            ..Default::default()
        },
        user_id,
    )
    .await
    .map_err(internal_error)?;
    Ok(())
}

/// 生成立绘的外观输入：补齐性别表述，再确认这份上半身设计能拿来生成。
fn portrait_visual_profile(
    persona: Option<&crate::models::entities::agent_persona::Model>,
) -> ApiResult<Value> {
    drawable_visual_profile(persona.and_then(|row| row.visual_profile.clone()))
}

/// A visual profile a portrait can be drawn from: a gender and a complete,
/// usable design that matches it.
pub(super) fn drawable_visual_profile(visual_profile: Option<Value>) -> ApiResult<Value> {
    let visual_profile = {
        let mut profile = visual_profile.unwrap_or_else(|| {
            json!({
                "gender": "unspecified",
                "language": "en-US"
            })
        });
        let gender = profile
            .get("gender")
            .and_then(Value::as_str)
            .unwrap_or("unspecified")
            .to_string();
        let language = profile
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("en-US")
            .to_string();
        if let Some(identity) = profile.get("visualIdentity").cloned() {
            if !identity.is_null() {
                if let Some(fixed) = myriad_merope::ensure_visual_identity_states_gender(
                    &identity, &gender, &language,
                ) {
                    if let Some(root) = profile.as_object_mut() {
                        root.insert("visualIdentity".into(), fixed);
                    }
                }
            }
        }
        profile
    };
    let gender = visual_profile.get("gender").and_then(Value::as_str);
    if !matches!(
        gender,
        Some("female" | "male" | "nonbinary" | "unspecified")
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a gender presentation before generating the portrait",
                "code": "visual_gender_required"
            })),
        ));
    }
    let visual_identity = visual_profile.get("visualIdentity").unwrap_or(&Value::Null);
    if !myriad_merope::upper_body_visual_identity_is_complete(visual_identity) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Confirm an upper-body visual design before generating the portrait",
                "code": "visual_design_required"
            })),
        ));
    }
    if myriad_merope::normalize_visual_identity_for_prompt(visual_identity).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "The upper-body visual design cannot be used for portrait generation",
                "code": "visual_identity_unusable"
            })),
        ));
    }
    if !myriad_merope::visual_identity_matches_gender_presentation(
        visual_identity,
        gender.unwrap_or("unspecified"),
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "The upper-body visual design does not match the chosen gender presentation",
                "code": "visual_gender_mismatch"
            })),
        ));
    }
    Ok(visual_profile)
}

async fn release_portrait_generation_lease(db: &DatabaseConnection, token: &str) {
    if let Err(error) = merope::release_portrait_generation(db, token).await {
        tracing::error!(%error, "failed to release portrait generation lease");
    }
}

async fn cleanup_uncommitted_portrait(
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
        .and_then(|persona| persona.portrait_asset_id)
        .is_some_and(|asset_id| asset_id == persisted.url);
    if is_current {
        return;
    }
    if let Err(error) = image_generation::remove_persisted_generated(db, persisted).await {
        tracing::warn!(%error, url = %persisted.url, "failed to remove uncommitted portrait asset");
    }
}

pub async fn generate_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<GeneratePortraitRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let name = persona
        .as_ref()
        .map(|row| row.name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("Arael");
    let additional_requirements = sanitize_portrait_adjustment(request.prompt.as_deref())?;
    let visual_profile = portrait_visual_profile(persona.as_ref())?;
    let existing_portrait = persona
        .as_ref()
        .and_then(|row| row.portrait_asset_id.as_deref())
        .filter(|url| !url.is_empty());
    let (reference, reference_role) = if request.edit {
        let Some(url) = existing_portrait else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Generate a master portrait before editing it",
                    "code": "portrait_required_for_edit"
                })),
            ));
        };
        if additional_requirements.is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Describe the adjustment before editing the portrait",
                    "code": "portrait_edit_notes_required"
                })),
            ));
        }
        match image_generation::load_local_reference(url).await {
            Ok(reference) => (Some(reference), "existing-portrait-identity-anchor"),
            Err(error) => return Err(portrait_generation_provider_error(error)),
        }
    } else {
        (
            Some(merope_style_reference().map_err(portrait_generation_config_error)?),
            "rendering-technique-only",
        )
    };
    let generation_contract =
        build_character_asset_contract(name, &visual_profile, additional_requirements.as_deref());
    let contract_fingerprint = character_asset_contract_fingerprint(&generation_contract);
    let prompt = if request.edit {
        build_character_visual_edit_prompt(additional_requirements.as_deref().unwrap_or(""))
    } else {
        build_character_visual_prompt(name, &visual_profile, additional_requirements.as_deref())
    };
    let bust = CharacterAssetProfile::Bust.contract();
    let (width, height) = (bust.generation_width, bust.generation_height);
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    tracing::info!(
        provider = %config.provider,
        model = %config.model,
        width,
        height,
        prompt_chars = prompt.chars().count(),
        style_school_named = prompt.contains("miHoYo"),
        visual_school_version = MEROPE_VISUAL_SCHOOL_VERSION,
        reference_role,
        style_reference_sha256 = MEROPE_STYLE_REFERENCE_SHA256,
        lookalike_ban = prompt.contains("找班"),
        "site portrait generation started"
    );
    let generation_token = Uuid::new_v4().to_string();
    let pending = json!({
        "token": generation_token,
        "inputFingerprint": contract_fingerprint,
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    let acquired = merope::acquire_portrait_generation(&db, name, &visual_profile, &pending)
        .await
        .map_err(internal_error)?;
    if !acquired {
        let current = merope::get_persona(&db).await.map_err(internal_error)?;
        let (message, code) = if current.as_ref().is_some_and(|persona| {
            merope::portrait_generation_is_pending(persona.portrait_generation.as_ref())
        }) {
            (
                "A portrait generation is already in progress",
                "portrait_generation_in_progress",
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
        "portrait",
        image_generation::generate_image_with_background(
            &config,
            &prompt,
            width,
            height,
            reference.as_ref(),
            Some(image_generation::ImageBackground::Opaque),
        ),
    )
    .await
    {
        Ok(generated) => generated,
        Err(error) => {
            release_portrait_generation_lease(&db, &generation_token).await;
            return Err(portrait_generation_provider_error(error));
        }
    };
    let actor = match crate::services::media::MediaActor::admin(user_id) {
        Ok(actor) => actor,
        Err(error) => {
            release_portrait_generation_lease(&db, &generation_token).await;
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
        "portrait",
        crate::services::media::MediaExposure::Private,
    )
    .await
    {
        Ok(persisted) => persisted,
        Err(error) => {
            release_portrait_generation_lease(&db, &generation_token).await;
            return Err(bad_request(&error.to_string()));
        }
    };
    let url = persisted.url.clone();
    let portrait_generation = json!({
        "fingerprint": contract_fingerprint,
        "contract": generation_contract,
        "provider": config.provider,
        "model": config.model,
        "referenceRole": reference_role,
    });
    let transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            release_portrait_generation_lease(&db, &generation_token).await;
            cleanup_uncommitted_portrait(&db, &persisted).await;
            return Err(internal_error(error));
        }
    };
    let written = async {
        let public_url = crate::services::media::normalize_local_url(
            &transaction,
            &url,
            &crate::services::media::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        let completed = merope::complete_portrait_generation(
            &transaction,
            name,
            &visual_profile,
            &generation_token,
            &public_url,
            &portrait_generation,
            user_id,
        )
        .await
        .map_err(internal_error)?;
        if !completed {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Character visual inputs changed while the portrait was generating",
                    "code": "character_visual_inputs_changed"
                })),
            ));
        }
        bind_generated_portrait(&transaction, user_id, &public_url, &contract_fingerprint).await?;
        let cleared_asset = merope_rig::persist_active_asset(&transaction, None)
            .await
            .map_err(internal_error)?;
        crate::services::media::bind_persona(
            &transaction,
            Some(&public_url),
            None,
            Some(&visual_profile),
            &crate::services::media::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        Ok((public_url, cleared_asset))
    }
    .await;
    let committed = match written {
        Ok(written) => transaction
            .commit()
            .await
            .map(|()| written)
            .map_err(internal_error),
        Err(error) => {
            let _ = transaction.rollback().await;
            Err(error)
        }
    };
    let (public_url, cleared_asset) = match committed {
        Ok(committed) => committed,
        Err(error) => {
            release_portrait_generation_lease(&db, &generation_token).await;
            cleanup_uncommitted_portrait(&db, &persisted).await;
            return Err(error);
        }
    };
    merope_rig::mirror_active_asset(cleared_asset).await;
    Ok(Json(json!({
        "portraitUrl": public_url,
        "portraitAssetId": public_url,
        "characterAssetContractVersion": CharacterAssetProfile::Bust.contract().contract_version,
        "generationFingerprint": contract_fingerprint,
    })))
}

#[cfg(test)]
mod rig_invalidation_tests {
    /// 取一个 handler 的函数体：从 `fn <name>(` 到下一个顶层 `\npub ` 之前。
    fn body_of<'a>(source: &'a str, name: &str) -> &'a str {
        let at = source
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("{name} not found"));
        let rest = &source[at..];
        let end = rest.find("\npub ").unwrap_or(rest.len());
        &rest[..end]
    }

    /// 写 `portrait_asset_id` 的入口必须在同一次事务里 persist_active_asset；PUT 同步穿着套的 live pointer，不是一律作废。
    ///
    /// 四个 portrait writer 必须在人设同一次事务里 persist_active_asset；`/active` 仍从该 persona 快照取穿着套，不读 configuration mirror。
    #[test]
    fn every_portrait_writer_clears_the_active_rig() {
        let rig = include_str!("portrait.rs");
        let upload = include_str!("portrait_upload.rs");
        let persona = include_str!("../agent/persona.rs");
        for (file, source, name) in [
            ("merope_rig/portrait.rs", rig, "generate_portrait"),
            ("merope_rig/portrait_upload.rs", upload, "upload_portrait"),
            ("agent/persona.rs", persona, "put_persona"),
            ("agent/persona.rs", persona, "delete_persona"),
        ] {
            assert!(
                body_of(source, name).contains("persist_active_asset"),
                "{file}::{name} 改了主图却没作废旧 Rig"
            );
        }
    }

    /// 作废必须落在事务里，并且提交后镜像出去。
    ///
    /// Split write without a txn can leave a stale package; `/active` drops it
    /// and serves portrait-only. Clear must still be transactional.
    #[test]
    fn upload_portrait_clears_the_rig_transactionally() {
        let body = body_of(include_str!("portrait_upload.rs"), "upload_portrait");
        assert!(body.contains("db.begin()"), "upload_portrait 必须开事务");
        assert!(
            body.contains("persist_active_asset(&transaction, None)"),
            "作废必须和人设写入同一个事务"
        );
        assert!(
            body.contains("transaction.rollback()"),
            "失败路径必须回滚，不能留下新主图配旧 Rig"
        );
        assert!(
            body.contains("mirror_active_asset(cleared_asset)"),
            "提交后要把清空后的 asset 镜像出去，和 generate_portrait 一致"
        );
    }
}

#[cfg(test)]
#[path = "portrait_tests.rs"]
mod tests;
