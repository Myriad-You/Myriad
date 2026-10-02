//! 全身：照穿着这套衣服的半身立绘画出从头到脚的站姿，拆层、导入，存进这套衣服的全身槽。
//! 全身只读写自己的槽，不碰半身的形象，也不碰面板正在播的那份。

use axum::{
    Extension, Json,
    extract::{Multipart, State},
    http::StatusCode,
    response::Response,
};
use myriad_merope::{
    CharacterAssetProfile, build_full_body_asset_contract, build_full_body_portrait_prompt,
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
    decompose::{SeeThroughDecomposeRequest, decompose_master},
    import::{import_rig, preview_rig},
    internal_error,
    master::{MasterProvenance, master_for, master_from_persona},
    not_found,
    package::figure_json,
    portrait_generation_config_error, portrait_generation_provider_error,
    portrait_upload::{persist_uploaded_portrait, read_portrait_upload},
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    models::entities::agent_persona,
    services::{agent::merope, image_generation},
};

/// One full figure at a time: a second request would draw over the first.
static FULL_BODY_SLOT: Semaphore = Semaphore::const_new(1);

fn provenance_changed() -> (StatusCode, Json<Value>) {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "The worn outfit or its portrait changed while the full figure was drawing",
            "code": "character_asset_provenance_changed"
        })),
    )
}

pub async fn generate_full_body_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
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
    let bust = persona
        .as_ref()
        .and_then(master_from_persona)
        .ok_or_else(|| not_found("The worn outfit has no portrait to draw a full figure from"))?;
    let reference = image_generation::load_local_reference(&bust.asset_id)
        .await
        .map_err(portrait_generation_provider_error)?;
    let contract =
        build_full_body_asset_contract(&bust.asset_id, bust.generation_fingerprint.as_deref());
    let fingerprint = character_asset_contract_fingerprint(&contract);
    let prompt = build_full_body_portrait_prompt();
    let full = CharacterAssetProfile::FullBody.contract();
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    tracing::info!(
        provider = %config.provider,
        model = %config.model,
        width = full.generation_width,
        height = full.generation_height,
        "full-body portrait generation started"
    );
    let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "full-body-portrait",
        image_generation::generate_image_with_background(
            &config,
            &prompt,
            full.generation_width,
            full.generation_height,
            Some(&reference),
            Some(image_generation::ImageBackground::Opaque),
        ),
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    let actor = crate::services::media::MediaActor::admin(user_id)
        .map_err(|error| internal_error(error.to_string()))?;
    let persisted = image_generation::persist_generated_with_status(
        &db,
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
        store_full_body_portrait(&db, user_id, &bust, &persisted.url, Some(&fingerprint)).await;
    let public_url = match stored {
        Ok(public_url) => public_url,
        Err(error) => {
            if let Err(cleanup) =
                image_generation::remove_persisted_generated(&db, &persisted).await
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

/// The owner's own full figure for the worn outfit. Like an uploaded bust it
/// has no generation fingerprint; it stays tied to the bust it was put beside.
pub async fn upload_full_body_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let reference = read_portrait_upload(multipart).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let bust = persona
        .as_ref()
        .and_then(master_from_persona)
        .ok_or_else(|| not_found("The worn outfit has no portrait to put a full figure beside"))?;
    let url = persist_uploaded_portrait(&db, user_id, reference).await?;
    let public_url = store_full_body_portrait(&db, user_id, &bust, &url, None).await?;
    Ok(Json(json!({
        "portraitUrl": public_url,
        "generationFingerprint": Value::Null,
        "characterAssetContractVersion": CharacterAssetProfile::FullBody.contract().contract_version,
    })))
}

/// Binds the full figure to the worn outfit, if that outfit still wears the
/// bust it belongs with. The figure's old rig no longer matches it.
async fn store_full_body_portrait(
    db: &DatabaseConnection,
    user_id: i32,
    bust: &MasterProvenance,
    url: &str,
    fingerprint: Option<&str>,
) -> ApiResult<String> {
    let transaction = db.begin().await.map_err(internal_error)?;
    let written = async {
        let row = lock_worn_persona(&transaction).await?;
        if master_from_persona(&row).as_ref() != Some(bust) {
            return Err(provenance_changed());
        }
        let public_url = crate::services::media::normalize_local_url(
            &transaction,
            url,
            &crate::services::media::upgrade::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        let mut profile = row.visual_profile.clone().unwrap_or_else(|| json!({}));
        if !myriad_merope::bind_active_outfit_full_body_portrait(
            &mut profile,
            &public_url,
            fingerprint,
        ) {
            return Err(bad_request("The worn outfit is missing"));
        }
        crate::services::media::bind_persona(
            &transaction,
            row.portrait_asset_id.as_deref(),
            row.avatar_asset_id.as_deref(),
            Some(&profile),
            &crate::services::media::upgrade::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        save_visual_profile(&transaction, row, profile, user_id).await?;
        Ok(public_url)
    }
    .await;
    finish(transaction, written).await
}

/// Binds an imported package to the worn outfit's full figure, if that is
/// still the figure it was compiled from.
async fn bind_full_body_rig(
    db: &DatabaseConnection,
    user_id: i32,
    asset_id: &str,
    expected: &MasterProvenance,
) -> ApiResult<()> {
    let transaction = db.begin().await.map_err(internal_error)?;
    let written = async {
        let row = lock_worn_persona(&transaction).await?;
        if master_for(&row, CharacterAssetProfile::FullBody).as_ref() != Some(expected) {
            return Err(provenance_changed());
        }
        let mut profile = row.visual_profile.clone().unwrap_or_else(|| json!({}));
        if !myriad_merope::bind_active_outfit_full_body_rig(&mut profile, asset_id) {
            return Err(bad_request("The worn outfit is missing"));
        }
        save_visual_profile(&transaction, row, profile, user_id).await
    }
    .await;
    finish(transaction, written).await
}

/// The persona row, locked until the transaction ends so a concurrent
/// portrait or outfit change cannot slip between the check and the write.
async fn lock_worn_persona(transaction: &DatabaseTransaction) -> ApiResult<agent_persona::Model> {
    merope::api::store::lock_persona_on(transaction)
        .await
        .map_err(internal_error)?;
    agent_persona::Entity::find_by_id(merope::api::store::PERSONA_ROW_ID)
        .lock_exclusive()
        .one(transaction)
        .await
        .map_err(internal_error)?
        .ok_or_else(provenance_changed)
}

async fn save_visual_profile(
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

async fn finish<T>(transaction: DatabaseTransaction, written: ApiResult<T>) -> ApiResult<T> {
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

/// The worn outfit's full figure and its package, for the owner's workbench.
pub async fn get_full_body(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let master = persona
        .as_ref()
        .and_then(|row| master_for(row, CharacterAssetProfile::FullBody));
    let asset_id = myriad_merope::active_outfit_full_body(
        persona.as_ref().and_then(|row| row.visual_profile.as_ref()),
    )
    .and_then(|full| full.rig_asset_id);
    figure_json(master, asset_id).await
}

pub async fn decompose_full_body(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SeeThroughDecomposeRequest>,
) -> ApiResult<Response> {
    decompose_master(&db, &claims, CharacterAssetProfile::FullBody, payload).await
}

pub async fn preview_full_body_rig(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    preview_rig(&db, &claims, multipart, CharacterAssetProfile::FullBody).await
}

pub async fn import_full_body_rig(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let imported = import_rig(&db, &claims, multipart, CharacterAssetProfile::FullBody).await?;
    bind_full_body_rig(&db, imported.user_id, &imported.asset_id, &imported.master).await?;
    Ok(Json(
        json!({ "manifest": imported.manifest, "assetId": imported.asset_id }),
    ))
}
