//! 立绘的手势手臂：让生图模型只改一条手臂（挥手、竖食指、按胸口、托腮），其余
//! 保持原样；前端导入时把改动的手臂抠成平时隐藏的一层，举手时从画面下方抬进来。
//! 画好的图先过两道检查：姿态模型在图上找得到这只手，视觉模型认为除了这条手臂
//! 的姿势什么都没改（袖子、护甲最容易被改掉）。不过关就重画，最多三次，仍不过
//! 就不提供这个手势。结果按立绘归档成私有媒体，换了立绘自然失效；手的关键点存
//! 在旁边，前端抠层时用它找到哪一块是手臂。

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
    ApiResult,
    expressions::{current_portrait, portrait_required},
    internal_error, portrait_generation_config_error, portrait_generation_provider_error,
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    models::entities::media_assets,
    services::{agent::merope::api::onboarding_ai, image_generation, pose_estimation},
};

/// 与 figure 的 `ARM_VARIANT_GESTURES` 一一对应：手势、交给生图模型的改法、交给检查的说法。
const GESTURES: [(&str, &str, &str); 4] = [
    (
        "wave",
        ": she raises that hand beside her face at shoulder height, the forearm upright, palm \
open toward the viewer, all five fingers spread a little, as in a friendly wave.",
        "the hand raised beside the face, palm open, waving",
    ),
    (
        "point",
        ": she raises that hand to chest height and points her index finger straight up, the \
other fingers curled in, the palm side facing the viewer, as when an idea comes to her.",
        "the hand raised to chest height, index finger pointing up",
    ),
    (
        "chest",
        ": she bends that elbow and rests that hand flat on the middle of her chest, palm \
against her body, fingers together and relaxed, the back of the hand toward the viewer.",
        "the hand resting flat on the middle of her chest",
    ),
    (
        "chin",
        ": she raises that hand to her face and rests the bent index finger lightly under her \
chin, the other fingers loosely curled, as when thinking.",
        "the hand raised to the face, a bent finger under the chin",
    ),
];
/// The arm changed, by the side of the picture the viewer sees it on.
const SIDES: [&str; 2] = ["left", "right"];

/// Always asked for the viewer's left arm: the right one is drawn on the
/// portrait mirrored and flipped back, which the model follows more reliably
/// than "the other arm".
const KEEP: &str = "Edit this exact illustration. Keep the canvas size, framing, the \
character's size and position, the face, expression, eyes, hair, outfit, accessories, colors, \
line style and shading exactly the same, on the same background. The arm on the viewer's right \
side stays exactly as drawn. Change only the arm on the viewer's left side";
const TAIL: &str = " The hand has five fingers with correct anatomy, drawn in the original's \
style. The sleeve keeps its exact design and moves with the arm. Clean finished art.";

/// One drawing and its two checks, then two more tries.
const ATTEMPTS: usize = 3;
/// A hand point the pose model is at least this sure of.
const HAND_SURE: f32 = 0.3;
/// A drawn hand shows at least this many such points.
const HAND_POINTS: usize = 5;

fn arm_prompt(gesture: &str) -> Option<String> {
    GESTURES
        .iter()
        .find(|(name, _, _)| *name == gesture)
        .map(|(_, change, _)| format!("{KEEP}{change}{TAIL}"))
}

/// `wave-left` → ("wave", "left"), for the gestures and sides there are.
fn parse_key(key: &str) -> Option<(&'static str, &'static str)> {
    let (gesture, side) = key.split_once('-')?;
    let gesture = GESTURES.iter().find(|(name, _, _)| *name == gesture)?.0;
    let side = SIDES.into_iter().find(|name| *name == side)?;
    Some((gesture, side))
}

fn keys() -> impl Iterator<Item = String> {
    GESTURES
        .iter()
        .flat_map(|(gesture, _, _)| SIDES.iter().map(move |side| format!("{gesture}-{side}")))
}

/// 同一个手势同时只画一次：一次要一两分钟，还要花钱。
static IN_FLIGHT: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

struct InFlight(String);

impl InFlight {
    fn claim(key: &str) -> Option<Self> {
        let mut running = IN_FLIGHT.lock().unwrap_or_else(|error| error.into_inner());
        running
            .insert(key.to_string())
            .then(|| Self(key.to_string()))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.0);
    }
}

/// 跟着立绘走：换了立绘，旧手臂的键前缀不再匹配，自然失效。
fn producer_prefix(portrait_asset_id: &str, key: &str) -> String {
    let digest = Sha256::digest(portrait_asset_id.as_bytes());
    let portrait: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("merope-arm-variant:{portrait}:{key}:")
}

/// The hand points kept beside each drawing, by its producer key's last part.
fn hand_file(producer_key: &str) -> Option<std::path::PathBuf> {
    let id = producer_key.rsplit(':').next()?;
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| {
        crate::services::data_paths::paths()
            .root
            .join("merope/arm-variants")
            .join(format!("{id}.json"))
    })
}

async fn latest_variant(
    db: &DatabaseConnection,
    portrait_asset_id: &str,
    key: &str,
) -> ApiResult<Option<Value>> {
    let row = media_assets::Entity::find()
        .filter(
            media_assets::Column::ProducerKey.starts_with(producer_prefix(portrait_asset_id, key)),
        )
        .filter(media_assets::Column::State.eq("ready"))
        .order_by_desc(media_assets::Column::CreatedAt)
        .one(db)
        .await
        .map_err(internal_error)?;
    let Some(row) = row else { return Ok(None) };
    let hand = match row.producer_key.as_deref().and_then(hand_file) {
        Some(path) => tokio::fs::read(path)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    Ok(Some(json!({ "url": row.url, "hand": hand })))
}

pub async fn list_arm_variants(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let portrait = current_portrait(&db).await?;
    let mut variants = serde_json::Map::new();
    for key in keys() {
        let found = latest_variant(&db, &portrait, &key).await?;
        variants.insert(key, found.unwrap_or(Value::Null));
    }
    let drawing: Vec<String> = IN_FLIGHT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .cloned()
        .collect();
    Ok(Json(json!({ "variants": variants, "drawing": drawing })))
}

pub async fn generate_arm_variant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let Some((gesture, side)) = parse_key(&key) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Unknown gesture", "code": "unknown_arm_variant" })),
        ));
    };
    let portrait = current_portrait(&db).await?;
    let reference = image_generation::load_local_reference(&portrait)
        .await
        .map_err(|error| {
            tracing::error!(%error, "stored master portrait is unusable for an arm variant");
            portrait_required()
        })?;
    let (width, height) = dimensions(&reference.bytes).ok_or_else(portrait_required)?;
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    let Some(running) = InFlight::claim(&key) else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "This gesture is already being drawn",
                "code": "arm_variant_generation_in_progress"
            })),
        ));
    };
    tracing::info!(gesture, side, provider = %config.provider, model = %config.model, "arm variant started");
    // Drawn and kept on its own: a page reloaded meanwhile does not throw it away.
    tokio::spawn(async move {
        let _running = running;
        let asked = Asked {
            gesture,
            side,
            portrait: &portrait,
            width,
            height,
        };
        draw_arm_variant(&db, user_id, asked, &config, &reference).await
    })
    .await
    .map_err(|error| internal_error(error.to_string()))?
}

/// The gesture asked for: which, on which arm, of which portrait, at its size.
struct Asked<'a> {
    gesture: &'static str,
    side: &'static str,
    portrait: &'a str,
    width: u32,
    height: u32,
}

async fn draw_arm_variant(
    db: &DatabaseConnection,
    user_id: i32,
    asked: Asked<'_>,
    config: &image_generation::ImageGenerationConfig,
    reference: &image_generation::ImageReference,
) -> ApiResult<Json<Value>> {
    let Asked {
        gesture,
        side,
        portrait,
        width,
        height,
    } = asked;
    let prompt = arm_prompt(gesture).ok_or_else(|| internal_error("arm prompt missing"))?;
    let words = GESTURES
        .iter()
        .find(|(name, _, _)| *name == gesture)
        .map(|(_, _, words)| *words)
        .unwrap_or_default();
    let mirrored = side == "right";
    let asked_on = if mirrored {
        let flipped = flip(&reference.bytes).ok_or_else(portrait_required)?;
        image_generation::ImageReference::new(flipped, "image/png")
            .map_err(portrait_generation_provider_error)?
    } else {
        reference.clone()
    };
    let mut notes: Vec<String> = Vec::new();
    for attempt in 1..=ATTEMPTS {
        let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
            user_id,
            "merope",
            "arm-variant",
            image_generation::generate_image(config, &prompt, width, height, Some(&asked_on)),
        )
        .await
        .map_err(portrait_generation_provider_error)?;
        let (bytes, _) = image_generation::load_generated_bytes(generated)
            .await
            .map_err(portrait_generation_provider_error)?;
        let Some(png) = (if mirrored {
            flip(&bytes)
        } else {
            to_png(&bytes)
        }) else {
            notes.push("unreadable picture".into());
            continue;
        };
        let Some(hand) = hand_points(&png, (width, height)).await else {
            tracing::info!(
                gesture,
                side,
                attempt,
                "arm variant: no hand found, drawn again"
            );
            notes.push("no hand found".into());
            continue;
        };
        let variant = image_generation::ImageReference::new(png.clone(), "image/png")
            .map_err(portrait_generation_provider_error)?;
        match onboarding_ai::check_arm_variant(reference, &variant, side, words).await {
            Ok(check) if check.same_design && check.gesture_shown => {}
            Ok(check) => {
                tracing::info!(gesture, side, attempt, differences = ?check.differences, "arm variant changed the design or missed the gesture, drawn again");
                notes = check.differences;
                if notes.is_empty() {
                    notes.push(
                        if check.same_design {
                            "gesture not shown"
                        } else {
                            "design changed"
                        }
                        .into(),
                    );
                }
                continue;
            }
            // The check is a guard, not the drawing: without a vision model the drawing stands.
            Err(error) => tracing::warn!(%error, gesture, side, "arm variant check unavailable"),
        }
        return keep(db, user_id, (gesture, side, portrait), png, hand).await;
    }
    Err((
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": "The gesture could not be drawn without changing the design",
            "code": "arm_variant_rejected",
            "differences": notes,
        })),
    ))
}

async fn keep(
    db: &DatabaseConnection,
    user_id: i32,
    (gesture, side, portrait): (&str, &str, &str),
    png: Vec<u8>,
    hand: Vec<[f32; 2]>,
) -> ApiResult<Json<Value>> {
    let key = format!("{gesture}-{side}");
    let (width, height) = dimensions(&png).unwrap_or_default();
    let producer_key = format!(
        "{}{}",
        producer_prefix(portrait, &key),
        uuid::Uuid::new_v4().simple()
    );
    if let Some(path) = hand_file(&producer_key) {
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(internal_error)?;
        }
        tokio::fs::write(&path, serde_json::to_vec(&hand).map_err(internal_error)?)
            .await
            .map_err(internal_error)?;
    }
    let actor = crate::services::media::MediaActor::admin(user_id)
        .map_err(|error| internal_error(error.to_string()))?;
    let persisted = image_generation::persist_generated_with_status(
        db,
        crate::services::media::MediaContext::site(
            actor,
            crate::services::media::MediaSource::Generated,
        )
        .with_producer_key(producer_key),
        image_generation::GeneratedImage {
            source: image_generation::GeneratedImageSource::Inline(png),
            media_type: "image/png".into(),
            width,
            height,
        },
        &format!("arm-variant-{key}"),
        crate::services::media::MediaExposure::Private,
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    Ok(Json(
        json!({ "key": key, "url": persisted.url, "hand": hand }),
    ))
}

fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
}

fn encode(picture: image::DynamicImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    encode(image::load_from_memory(bytes).ok()?)
}

fn flip(bytes: &[u8]) -> Option<Vec<u8>> {
    encode(image::load_from_memory(bytes).ok()?.fliph())
}

/// The points of the hand the drawing shows most surely, in the portrait's
/// pixels (`size`); none when no hand is drawn well enough to find.
async fn hand_points(png: &[u8], size: (u32, u32)) -> Option<Vec<[f32; 2]>> {
    let pose = match pose_estimation::estimate(png).await {
        Ok(pose) => pose,
        Err(error) => {
            tracing::warn!(%error, "arm variant: the pose model could not look");
            return None;
        }
    };
    surest_hand(&pose.keypoints, (pose.width, pose.height), size)
}

/// COCO-WholeBody's two hands are points 91–111 and 112–132.
fn surest_hand(
    keypoints: &[[f32; 3]],
    drawn: (u32, u32),
    size: (u32, u32),
) -> Option<Vec<[f32; 2]>> {
    let hands = [keypoints.get(91..112)?, keypoints.get(112..133)?];
    let sure = |hand: &[[f32; 3]]| hand.iter().map(|point| point[2]).sum::<f32>();
    let best = if sure(hands[0]) >= sure(hands[1]) {
        hands[0]
    } else {
        hands[1]
    };
    let scale = (
        size.0 as f32 / drawn.0.max(1) as f32,
        size.1 as f32 / drawn.1.max(1) as f32,
    );
    let points: Vec<[f32; 2]> = best
        .iter()
        .filter(|point| point[2] >= HAND_SURE)
        .map(|point| [point[0] * scale.0, point[1] * scale.1])
        .collect();
    (points.len() >= HAND_POINTS).then_some(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gesture_on_either_side_has_a_key_and_a_prompt() {
        let all: Vec<String> = keys().collect();
        assert_eq!(all.len(), 8);
        for key in &all {
            let (gesture, _) = parse_key(key).expect(key);
            let prompt = arm_prompt(gesture).unwrap();
            assert!(prompt.starts_with(KEEP) && prompt.ends_with(TAIL));
        }
        assert!(parse_key("juggle-left").is_none() && parse_key("wave-up").is_none());
        assert!(parse_key("wave").is_none());
    }

    #[test]
    fn producer_prefix_is_per_portrait_and_gesture() {
        let wave = producer_prefix("/media/assets/a/portrait.png", "wave-left");
        assert!(wave.starts_with("merope-arm-variant:") && wave.ends_with(":wave-left:"));
        assert_ne!(
            wave,
            producer_prefix("/media/assets/b/portrait.png", "wave-left")
        );
        assert_ne!(
            wave,
            producer_prefix("/media/assets/a/portrait.png", "wave-right")
        );
    }

    #[test]
    fn hand_points_are_kept_only_beside_a_key_this_module_made() {
        let path = hand_file("merope-arm-variant:00ff:wave-left:0123abcd").unwrap();
        assert!(path.ends_with("merope/arm-variants/0123abcd.json"));
        assert!(hand_file("merope-arm-variant:00ff:wave-left:../../etc").is_none());
        assert!(hand_file("merope-arm-variant:00ff:wave-left:").is_none());
    }

    #[test]
    fn the_surer_hand_is_taken_in_the_portrait_s_pixels() {
        let mut keypoints = vec![[0.0, 0.0, 0.0]; 133];
        for point in &mut keypoints[112..133] {
            *point = [100.0, 50.0, 0.9];
        }
        for point in &mut keypoints[91..95] {
            *point = [10.0, 10.0, 0.95];
        }
        let hand = surest_hand(&keypoints, (500, 500), (1000, 1000)).unwrap();
        assert_eq!(hand.len(), 21);
        assert_eq!(hand[0], [200.0, 100.0]);
        // Four sure points are not a hand.
        assert!(surest_hand(&keypoints[..112], (500, 500), (1000, 1000)).is_none());
        let mut few = vec![[0.0, 0.0, 0.0]; 133];
        for point in &mut few[91..95] {
            *point = [10.0, 10.0, 0.95];
        }
        assert!(surest_hand(&few, (500, 500), (1000, 1000)).is_none());
    }

    #[test]
    fn a_gesture_is_claimed_once_until_released() {
        let first = InFlight::claim("chin-right").expect("free");
        assert!(InFlight::claim("chin-right").is_none());
        drop(first);
        assert!(InFlight::claim("chin-right").is_some());
    }
}
