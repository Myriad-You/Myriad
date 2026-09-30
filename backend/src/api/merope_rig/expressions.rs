//! 立绘的表情重绘：让生图模型只改脸，其余保持原样，前端导入时再把眼睛、嘴切成
//! 装配已有的表情替换件。重绘结果按立绘归档成私有媒体，不替换立绘、不动装配。

use std::{collections::BTreeSet, io::Cursor, sync::Mutex};

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    ApiError, ApiResult, internal_error, portrait_generation_config_error,
    portrait_generation_provider_error, require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    models::entities::media_assets,
    services::{agent::merope, image_generation},
};

/// 与前端 `AuthoredExpressionKind` 一一对应。
const KINDS: [&str; 2] = ["cry", "squeeze"];

const KEEP_EVERYTHING_ELSE: &str = "Keep everything else exactly identical: same art style, \
line weight, colours, hair, clothing, pose, framing, background and image size. Do not move \
or redraw anything outside the face.";

fn expression_prompt(kind: &str) -> Option<String> {
    let face = match kind {
        "cry" => {
            "Edit only the facial expression: make her cry softly — eyebrows tilted up at \
the inner ends, eyes slightly narrowed and glossy with tears, a few tear drops on the lower \
lids, mouth a small wavering frown."
        }
        "squeeze" => {
            "Edit only the facial expression: a gentle happy closed-eye smile — both eyes \
closed into soft upward-curved arcs with the lashes along the arc, eyebrows relaxed, mouth \
unchanged."
        }
        _ => return None,
    };
    Some(format!("{face} {KEEP_EVERYTHING_ELSE}"))
}

/// 同一种表情同时只跑一次：一次调用要一两分钟，还要花钱。
static IN_FLIGHT: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

struct InFlight(&'static str);

impl InFlight {
    fn claim(kind: &'static str) -> Option<Self> {
        let mut running = IN_FLIGHT.lock().unwrap_or_else(|error| error.into_inner());
        running.insert(kind).then(|| Self(kind))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(self.0);
    }
}

/// 重绘跟着立绘走：换了立绘，旧重绘的键前缀不再匹配，自然失效。
fn producer_prefix(portrait_asset_id: &str, kind: &str) -> String {
    let digest = Sha256::digest(portrait_asset_id.as_bytes());
    let portrait: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("merope-expression:{portrait}:{kind}:")
}

async fn current_portrait(db: &DatabaseConnection) -> ApiResult<String> {
    merope::get_persona(db)
        .await
        .map_err(internal_error)?
        .and_then(|persona| persona.portrait_asset_id)
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .ok_or_else(portrait_required)
}

fn portrait_required() -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "Generate or upload a master portrait before drawing expressions",
            "code": "portrait_required"
        })),
    )
}

async fn latest_expression(
    db: &DatabaseConnection,
    portrait_asset_id: &str,
    kind: &str,
) -> ApiResult<Option<String>> {
    let row = media_assets::Entity::find()
        .filter(
            media_assets::Column::ProducerKey.starts_with(producer_prefix(portrait_asset_id, kind)),
        )
        .filter(media_assets::Column::State.eq("ready"))
        .order_by_desc(media_assets::Column::CreatedAt)
        .one(db)
        .await
        .map_err(internal_error)?;
    Ok(row.map(|row| row.url))
}

pub async fn list_expressions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let portrait = current_portrait(&db).await?;
    let mut expressions = serde_json::Map::new();
    for kind in KINDS {
        let url = latest_expression(&db, &portrait, kind).await?;
        expressions.insert(kind.to_string(), url.map_or(Value::Null, Value::String));
    }
    Ok(Json(json!({ "expressions": expressions })))
}

pub async fn generate_expression(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(kind): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let Some(kind) = KINDS.into_iter().find(|candidate| *candidate == kind) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Unknown expression", "code": "unknown_expression" })),
        ));
    };
    let prompt =
        expression_prompt(kind).ok_or_else(|| internal_error("expression prompt missing"))?;
    let portrait = current_portrait(&db).await?;
    let reference = image_generation::load_local_reference(&portrait)
        .await
        .map_err(|error| {
            tracing::error!(%error, "stored master portrait is unusable for an expression redraw");
            portrait_required()
        })?;
    // 重绘必须与立绘同尺寸，前端才能逐像素对位。
    let (width, height) = image::ImageReader::new(Cursor::new(reference.bytes.as_ref()))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .ok_or_else(portrait_required)?;
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    let Some(_running) = InFlight::claim(kind) else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "This expression is already being drawn",
                "code": "expression_generation_in_progress"
            })),
        ));
    };
    tracing::info!(
        kind,
        provider = %config.provider,
        model = %config.model,
        width,
        height,
        "portrait expression redraw started"
    );
    let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "expression",
        image_generation::generate_image(&config, &prompt, width, height, Some(&reference)),
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    let actor = crate::services::media::MediaActor::admin(user_id)
        .map_err(|error| internal_error(error.to_string()))?;
    let key = format!(
        "{}{}",
        producer_prefix(&portrait, kind),
        uuid::Uuid::new_v4().simple()
    );
    let persisted = image_generation::persist_generated_with_status(
        &db,
        crate::services::media::MediaContext::site(
            actor,
            crate::services::media::MediaSource::Generated,
        )
        .with_producer_key(key),
        generated,
        &format!("expression-{kind}"),
        crate::services::media::MediaExposure::Private,
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    Ok(Json(json!({ "kind": kind, "url": persisted.url })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_prompt_that_protects_the_rest_of_the_portrait() {
        for kind in KINDS {
            let prompt = expression_prompt(kind).expect(kind);
            assert!(prompt.contains("Edit only the facial expression"));
            assert!(prompt.ends_with(KEEP_EVERYTHING_ELSE));
        }
        assert!(expression_prompt("laugh").is_none());
    }

    #[test]
    fn producer_prefix_is_per_portrait_and_kind() {
        let cry = producer_prefix("/media/assets/a/portrait.png", "cry");
        assert!(cry.starts_with("merope-expression:"));
        assert!(cry.ends_with(":cry:"));
        assert_ne!(cry, producer_prefix("/media/assets/b/portrait.png", "cry"));
        assert_ne!(
            cry,
            producer_prefix("/media/assets/a/portrait.png", "squeeze")
        );
    }

    #[test]
    fn a_kind_is_claimed_once_until_released() {
        let first = InFlight::claim("squeeze").expect("free");
        assert!(InFlight::claim("squeeze").is_none());
        drop(first);
        assert!(InFlight::claim("squeeze").is_some());
    }
}
