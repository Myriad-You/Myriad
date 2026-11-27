//! 全身套装：衣柜里独立的一套，有自己的立绘、拆层和骨骼。可以照选定的半身那套重画，
//! 也可以直接按设计画；穿着的全身另记一处，面板始终播半身。全身只读写自己那一套，
//! 不碰半身的形象，也不碰面板正在播的那份。

use axum::{
    Extension, Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use myriad_merope::{
    CharacterAssetProfile, FullBodySource, build_full_body_asset_contract_with,
    character_asset_contract_fingerprint,
};
use sea_orm::{
    DatabaseConnection, DatabaseTransaction, EntityTrait, QuerySelect, TransactionTrait,
};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use uuid::Uuid;

use super::{
    ApiResult, bad_request,
    import::{import_rig, preview_rig, spawn_part_inventory},
    internal_error,
    master::{MasterProvenance, MasterSlot, master_for},
    not_found,
    package::figure_json,
    persona_style_error,
    portrait::{DrawingPlan, drawable_visual_profile, style_reference_role},
    portrait_generation_config_error, portrait_generation_provider_error,
    portrait_upload::{persist_uploaded_portrait, read_portrait_upload},
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    models::entities::agent_persona,
    services::{
        agent::merope,
        image_generation::{self, ImageReference},
        persona_style::{self, Drawing as Picture, StyleSource},
    },
};

/// What a full figure is drawn from.
pub(super) enum FullBodyFrom<'a> {
    /// Redrawn from a bust set's master portrait, attached as the anchor.
    Bust {
        portrait: &'a str,
        generation_fingerprint: Option<&'a str>,
        anchor: ImageReference,
    },
    /// Drawn from the design with this set's outfit on, in the recipe's style.
    Design {
        name: &'a str,
        visual_profile: &'a Value,
    },
}

/// A full figure's plan: the recipe's prompt and references (or the bust it
/// is redrawn from), and the contract built from the recipe's provenance.
pub(super) async fn full_body_plan(
    source: &StyleSource,
    from: FullBodyFrom<'_>,
) -> ApiResult<DrawingPlan> {
    let drawing = match &from {
        FullBodyFrom::Bust { .. } => Picture::FullBodyFromBust,
        FullBodyFrom::Design {
            name,
            visual_profile,
        } => Picture::FullBodyDesign {
            name,
            visual_profile,
        },
    };
    let recipe = persona_style::recipe_for(source, drawing)
        .await
        .map_err(persona_style_error)?;
    let prompt = recipe.prompt(drawing);
    let (contract, references, reference_role) = match from {
        FullBodyFrom::Bust {
            portrait,
            generation_fingerprint,
            anchor,
        } => (
            build_full_body_asset_contract_with(
                FullBodySource::Bust {
                    portrait,
                    generation_fingerprint,
                },
                &recipe.rendering,
            ),
            vec![anchor],
            "bust-portrait-identity-anchor",
        ),
        FullBodyFrom::Design {
            name,
            visual_profile,
        } => {
            let references = recipe.style_references(drawing);
            let role = style_reference_role(&references);
            (
                build_full_body_asset_contract_with(
                    FullBodySource::Design {
                        name,
                        visual_profile,
                    },
                    &recipe.rendering,
                ),
                references,
                role,
            )
        }
    };
    let fingerprint = character_asset_contract_fingerprint(&contract);
    Ok(DrawingPlan {
        prompt,
        references,
        contract,
        fingerprint,
        reference_role,
        rendering: recipe.rendering,
        source: recipe.source,
    })
}

/// One full figure at a time: a second request would draw over the first.
static FULL_BODY_SLOT: Semaphore = Semaphore::const_new(1);

/// The set whose full figure is being drawn, while one is: a page opened or
/// reloaded meanwhile shows it being drawn, and takes it up when done.
static DRAWING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

struct Drawing;

impl Drawing {
    fn begin(outfit_id: &str) -> Self {
        *DRAWING.lock().unwrap_or_else(|error| error.into_inner()) = Some(outfit_id.to_owned());
        Self
    }
}

impl Drop for Drawing {
    fn drop(&mut self) {
        *DRAWING.lock().unwrap_or_else(|error| error.into_inner()) = None;
    }
}

/// Whether this set's full figure is being drawn now.
pub async fn full_body_portrait_drawing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let drawing = DRAWING
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_deref()
        == Some(outfit_id.as_str());
    Ok(Json(json!({ "drawing": drawing })))
}

fn set_missing() -> (StatusCode, Json<Value>) {
    not_found("The full-body set is missing")
}

pub(super) fn full_body_set(
    persona: Option<&agent_persona::Model>,
    id: &str,
) -> ApiResult<myriad_merope::FullBodyOutfit> {
    myriad_merope::full_body_outfit(persona.and_then(|row| row.visual_profile.as_ref()), id)
        .ok_or_else(set_missing)
}

pub async fn generate_full_body_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let _slot = FULL_BODY_SLOT.try_acquire().map_err(|_| {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "A full figure is already being drawn",
                "code": "portrait_generation_in_progress"
            })),
        )
    })?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let set = full_body_set(persona.as_ref(), &outfit_id)?;
    let visual_profile = persona.as_ref().and_then(|row| row.visual_profile.as_ref());
    // Redrawn from the bust set chosen when it was made, while that set has a
    // portrait; otherwise drawn from the design with this set's outfit on.
    let bust = set
        .reference_outfit_id
        .as_deref()
        .and_then(|reference| myriad_merope::wardrobe_bust_portrait(visual_profile, reference));
    let name = persona
        .as_ref()
        .map(|row| row.name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("Arael");
    let anchor = match &bust {
        Some((portrait, _)) => Some(
            image_generation::load_local_reference(portrait)
                .await
                .map_err(portrait_generation_provider_error)?,
        ),
        None => None,
    };
    let dressed = match bust {
        Some(_) => None,
        None => Some(drawable_visual_profile(visual_profile.and_then(
            |profile| myriad_merope::visual_profile_wearing(profile, &outfit_id),
        ))?),
    };
    let full = CharacterAssetProfile::FullBody.contract();
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    // The image provider first: no recipe is asked for a picture that
    // cannot be drawn.
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    let style_source = persona_style::configured(&dynamic).map_err(persona_style_error)?;
    let from = match (&bust, anchor, &dressed) {
        (Some((portrait, fingerprint)), Some(anchor), _) => FullBodyFrom::Bust {
            portrait,
            generation_fingerprint: fingerprint.as_deref(),
            anchor,
        },
        (_, _, Some(dressed)) => FullBodyFrom::Design {
            name,
            visual_profile: dressed,
        },
        _ => return Err(internal_error("full-body source missing")),
    };
    let plan = full_body_plan(&style_source, from).await?;
    tracing::info!(
        provider = %config.provider,
        model = %config.model,
        width = full.generation_width,
        height = full.generation_height,
        from_bust = bust.is_some(),
        style_source = plan.source.as_str(),
        style_recipe = plan.rendering.recipe.as_deref().unwrap_or("builtin"),
        visual_school_version = %plan.rendering.visual_school_version,
        reference_role = plan.reference_role,
        "full-body portrait generation started"
    );
    let DrawingPlan {
        prompt,
        references,
        fingerprint,
        ..
    } = plan;
    // Drawn and stored on its own: a page reloaded or closed meanwhile does
    // not throw the drawing away, and finds it being drawn (`DRAWING`).
    let drawing = Drawing::begin(&outfit_id);
    tokio::spawn(async move {
        let _slot = _slot;
        let _drawing = drawing;
        draw_full_body(
            &db,
            user_id,
            &outfit_id,
            &config,
            &prompt,
            &references,
            fingerprint,
        )
        .await
    })
    .await
    .map_err(|error| internal_error(error.to_string()))?
}

async fn draw_full_body(
    db: &DatabaseConnection,
    user_id: i32,
    outfit_id: &str,
    config: &image_generation::ImageGenerationConfig,
    prompt: &str,
    references: &[ImageReference],
    fingerprint: String,
) -> ApiResult<Json<Value>> {
    let full = CharacterAssetProfile::FullBody.contract();
    let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "full-body-portrait",
        image_generation::generate_image_with_references(
            config,
            prompt,
            full.generation_width,
            full.generation_height,
            references,
            Some(image_generation::ImageBackground::Opaque),
        ),
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    let actor = crate::services::media::MediaActor::admin(user_id)
        .map_err(|error| internal_error(error.to_string()))?;
    let persisted = image_generation::persist_generated_with_status(
        db,
        crate::services::media::MediaContext::site(
            actor,
            crate::services::media::MediaSource::Generated,
        )
        .with_producer_key(format!("merope-full-body:{}", Uuid::new_v4())),
        generated,
        "portrait",
        crate::services::media::MediaExposure::Private,
    )
    .await
    .map_err(|error| bad_request(&error.to_string()))?;
    let stored =
        store_full_body_portrait(db, user_id, outfit_id, &persisted.url, Some(&fingerprint)).await;
    let public_url = match stored {
        Ok(public_url) => public_url,
        Err(error) => {
            if let Err(cleanup) = image_generation::remove_persisted_generated(db, &persisted).await
            {
                tracing::warn!(error = %cleanup, url = %persisted.url, "failed to remove uncommitted full-body portrait");
            }
            return Err(error);
        }
    };
    Ok(Json(json!({
        "portraitUrl": public_url,
        "generationFingerprint": fingerprint,
        "characterAssetContractVersion": full.contract_version,
    })))
}

/// The owner's own picture for a full-body set. Like an uploaded bust it has
/// no generation fingerprint.
pub async fn upload_full_body_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    full_body_set(persona.as_ref(), &outfit_id)?;
    let reference = read_portrait_upload(multipart).await?;
    let upright = super::upright::screen_upload(&reference).await?;
    let url = persist_uploaded_portrait(&db, user_id, reference).await?;
    let public_url = store_full_body_portrait(&db, user_id, &outfit_id, &url, None).await?;
    Ok(Json(json!({
        "portraitUrl": public_url,
        "uprightReasons": upright,
        "generationFingerprint": Value::Null,
        "characterAssetContractVersion": CharacterAssetProfile::FullBody.contract().contract_version,
    })))
}

/// Stores a full-body set's picture. Its old rig no longer matches it.
async fn store_full_body_portrait(
    db: &DatabaseConnection,
    user_id: i32,
    outfit_id: &str,
    url: &str,
    fingerprint: Option<&str>,
) -> ApiResult<String> {
    let transaction = db.begin().await.map_err(internal_error)?;
    let written = async {
        let row = lock_persona(&transaction).await?;
        let public_url = crate::services::media::normalize_local_url(
            &transaction,
            url,
            &crate::services::media::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        let mut profile = row.visual_profile.clone().unwrap_or_else(|| json!({}));
        if !myriad_merope::bind_full_body_outfit_portrait(
            &mut profile,
            outfit_id,
            &public_url,
            fingerprint,
        ) {
            return Err(set_missing());
        }
        crate::services::media::bind_persona(
            &transaction,
            row.portrait_asset_id.as_deref(),
            row.avatar_asset_id.as_deref(),
            Some(&profile),
            &crate::services::media::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        save_visual_profile(&transaction, row, profile, user_id).await?;
        Ok(public_url)
    }
    .await;
    finish(transaction, written).await
}

/// Binds a package to its full-body set, if that set's picture is still the
/// one it was compiled from and, when a rewrite names one, its rig is still
/// the one rewritten.
pub(super) async fn bind_full_body_rig(
    db: &DatabaseConnection,
    user_id: i32,
    outfit_id: &str,
    asset_id: &str,
    expected: &MasterProvenance,
    expected_rig: Option<&str>,
) -> ApiResult<()> {
    let transaction = db.begin().await.map_err(internal_error)?;
    let written = async {
        let row = lock_persona(&transaction).await?;
        if let Some(rig) = expected_rig {
            let current = full_body_set(Some(&row), outfit_id)?.rig_asset_id;
            if current.as_deref() != Some(rig) {
                return Err(super::package::rig_revision_conflict());
            }
        }
        if master_for(&row, MasterSlot::FullBody(outfit_id)).as_ref() != Some(expected) {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "The full-body set's picture changed before its rig was saved",
                    "code": "character_asset_provenance_changed"
                })),
            ));
        }
        let mut profile = row.visual_profile.clone().unwrap_or_else(|| json!({}));
        if !myriad_merope::bind_full_body_outfit_rig(&mut profile, outfit_id, asset_id) {
            return Err(set_missing());
        }
        save_visual_profile(&transaction, row, profile, user_id).await
    }
    .await;
    finish(transaction, written).await
}

/// The persona row, locked until the transaction ends so a concurrent
/// portrait or outfit change cannot slip between the check and the write.
pub(super) async fn lock_persona(
    transaction: &DatabaseTransaction,
) -> ApiResult<agent_persona::Model> {
    merope::api::store::lock_persona_on(transaction)
        .await
        .map_err(internal_error)?;
    agent_persona::Entity::find_by_id(merope::api::store::PERSONA_ROW_ID)
        .lock_exclusive()
        .one(transaction)
        .await
        .map_err(internal_error)?
        .ok_or_else(set_missing)
}

pub(super) async fn save_visual_profile(
    transaction: &DatabaseTransaction,
    row: agent_persona::Model,
    profile: Value,
    user_id: i32,
) -> ApiResult<()> {
    merope::upsert_persona_on(
        transaction,
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

pub(super) async fn finish<T>(
    transaction: DatabaseTransaction,
    written: ApiResult<T>,
) -> ApiResult<T> {
    match written {
        Ok(value) => {
            transaction.commit().await.map_err(internal_error)?;
            Ok(value)
        }
        Err(error) => {
            let _ = transaction.rollback().await;
            Err(error)
        }
    }
}

/// A full-body set's picture and its package, for the owner's workbench.
pub async fn get_full_body(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let set = full_body_set(persona.as_ref(), &outfit_id)?;
    let master = persona
        .as_ref()
        .and_then(|row| master_for(row, MasterSlot::FullBody(&outfit_id)));
    figure_json(master, set.rig_asset_id).await
}

pub async fn preview_full_body_rig(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    preview_rig(&db, &claims, multipart, MasterSlot::FullBody(&outfit_id)).await
}

pub async fn import_full_body_rig(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let imported = import_rig(&db, &claims, multipart, MasterSlot::FullBody(&outfit_id)).await?;
    bind_full_body_rig(
        &db,
        imported.user_id,
        &outfit_id,
        &imported.asset_id,
        &imported.master,
        None,
    )
    .await?;
    spawn_part_inventory(&db, &imported, Some(outfit_id.clone()));
    Ok(Json(
        json!({ "manifest": imported.manifest, "assetId": imported.asset_id }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_shows_as_drawn_until_its_drawing_ends() {
        let drawing = Drawing::begin("w-test");
        assert_eq!(DRAWING.lock().unwrap().as_deref(), Some("w-test"));
        drop(drawing);
        assert_eq!(DRAWING.lock().unwrap().as_deref(), None);
    }
}
