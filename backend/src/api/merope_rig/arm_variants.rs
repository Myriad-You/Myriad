//! 立绘的手势手臂：让生图模型只改一条手臂（挥手、竖食指、按胸口、托腮），其余
//! 保持原样；前端导入时把改动的手臂抠成平时隐藏的一层，举手时从画面下方抬进来。
//! 画好的图先过两道检查：姿态模型在图上找得到这只手，视觉模型认为除了这条手臂
//! 的姿势什么都没改（袖子、护甲最容易被改掉）。不过关就重画，最多三次，仍不过
//! 就不提供这个手势。结果按立绘归档成私有媒体，换了立绘自然失效；手的关键点存
//! 在旁边，前端抠层时用它找到哪一块是手臂。全身形象的手太小，在放大图上取一扇
//! 窗口画，画完超分贴回放大图（`figure`）。

use std::{
    collections::BTreeSet,
    future::Future,
    io::Cursor,
    sync::{Arc, Mutex},
};

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod figure;
mod sheet;
use sheet::{figure_sheet, laid_back};

use super::{
    ApiResult,
    expressions::{current_portrait, portrait_required},
    internal_error,
    master::{MasterSlot, current_master},
    portrait_generation_config_error, portrait_generation_provider_error, require_merope_enabled,
    require_owner,
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
/// The 3D arm wears the portrait's sleeve on the upper arm and the gesture's on
/// the forearm, so a band or cuff the drawing moves down the arm shows twice
/// while the hand rises. Without this sentence the model moved a short sleeve's
/// cuff to the wrist (b00); with it, 2 of 2 kept it where the portrait has it.
const SLEEVE: &str = " That sleeve keeps exactly the length it has in the original: every \
band, cuff and hem on it stays on the same part of the arm, as far from the shoulder as it is \
now, and none of them moves down toward the wrist.";
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
        .map(|(_, change, _)| format!("{KEEP}{change}{SLEEVE}{TAIL}"))
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

/// What the pose model found on each drawing (its hand, and every keypoint
/// for the 3D arm), kept beside it by its producer key's last part.
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
    let stored = match row.producer_key.as_deref().and_then(hand_file) {
        Some(path) => tokio::fs::read(path)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    let (hand, keypoints) = stored_points(stored);
    Ok(Some(
        json!({ "url": row.url, "hand": hand, "keypoints": keypoints }),
    ))
}

/// A drawing's kept points: the hand and every keypoint, or (kept before the
/// 3D arm) the hand alone.
fn stored_points(stored: Value) -> (Value, Value) {
    match stored {
        Value::Object(mut found) => (
            found.remove("hand").unwrap_or(Value::Null),
            found.remove("keypoints").unwrap_or(Value::Null),
        ),
        hand => (hand, Value::Null),
    }
}

pub async fn list_arm_variants(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let portrait = current_portrait(&db).await?;
    listed(&db, &portrait, "").await
}

/// A full-body set's gesture arms, drawn on its own picture.
pub async fn list_full_body_arm_variants(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let master = figure_master(&db, &outfit_id).await?;
    listed(&db, &master, &in_flight_scope(Some(&outfit_id))).await
}

/// The newest drawing of each gesture on `picture`, and the gestures being drawn in `scope`.
async fn listed(db: &DatabaseConnection, picture: &str, scope: &str) -> ApiResult<Json<Value>> {
    let mut variants = serde_json::Map::new();
    for key in keys() {
        let found = latest_variant(db, picture, &key).await?;
        variants.insert(key, found.unwrap_or(Value::Null));
    }
    let drawing: Vec<String> = IN_FLIGHT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter_map(|claimed| claimed.strip_prefix(scope))
        .filter(|key| !key.contains('/'))
        .map(str::to_string)
        .collect();
    Ok(Json(json!({ "variants": variants, "drawing": drawing })))
}

/// What the gestures being drawn for a set are claimed under: none for the worn bust.
fn in_flight_scope(outfit_id: Option<&str>) -> String {
    outfit_id.map(|id| format!("{id}/")).unwrap_or_default()
}

/// A full-body set's current master picture (its asset id).
async fn figure_master(db: &DatabaseConnection, outfit_id: &str) -> ApiResult<String> {
    Ok(current_master(db, MasterSlot::FullBody(outfit_id))
        .await?
        .ok_or_else(portrait_required)?
        .asset_id)
}

pub async fn generate_arm_variant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let Some((gesture, side)) = parse_key(&key) else {
        return Err(unknown_gesture());
    };
    let portrait = current_portrait(&db).await?;
    let reference = load_picture(&portrait).await?;
    let (width, height) = dimensions(&reference.bytes).ok_or_else(portrait_required)?;
    let sheet = Sheet {
        reference,
        drawn: (width, height),
        figure: None,
    };
    let sheet = std::future::ready(Ok(sheet));
    drawn_in_turn(db, user_id, (gesture, side, portrait), sheet, &key).await
}

/// Draws one gesture arm on a full-body set's picture: on a window of it
/// enlarged, laid back on the enlarged picture its decomposition was cut from.
pub async fn generate_full_body_arm_variant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((outfit_id, key)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let Some((gesture, side)) = parse_key(&key) else {
        return Err(unknown_gesture());
    };
    let master = figure_master(&db, &outfit_id).await?;
    let picture = load_picture(&master).await?;
    // Made once the gesture is claimed: the picture may have to be enlarged first, and a second ask for the
    // same gesture is turned away before it pays for that.
    let sheet = async move { figure_sheet(&picture).await };
    let scope = format!("{}{key}", in_flight_scope(Some(&outfit_id)));
    drawn_in_turn(db, user_id, (gesture, side, master), sheet, &scope).await
}

fn unknown_gesture() -> super::ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "Unknown gesture", "code": "unknown_arm_variant" })),
    )
}

async fn load_picture(asset_id: &str) -> ApiResult<image_generation::ImageReference> {
    image_generation::load_local_reference(asset_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "stored master picture is unusable for an arm variant");
            portrait_required()
        })
}

/// What a gesture is drawn on: the picture handed to the image model at the
/// size it is drawn at, and for a full body where that goes back.
struct Sheet {
    reference: image_generation::ImageReference,
    drawn: (u32, u32),
    /// A full body's window of its enlarged picture, and that picture.
    figure: Option<(figure::Window, Arc<image::DynamicImage>)>,
}

/// Draws a gesture once at a time under `claim` (in the background: a page reloaded meanwhile does not lose it),
/// on the sheet `sheet` makes once it is claimed.
async fn drawn_in_turn(
    db: DatabaseConnection,
    user_id: i32,
    (gesture, side, picture): (&'static str, &'static str, String),
    sheet: impl Future<Output = ApiResult<Sheet>> + Send + 'static,
    claim: &str,
) -> ApiResult<Json<Value>> {
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    let Some(running) = InFlight::claim(claim) else {
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
        let sheet = sheet.await?;
        let asked = Asked {
            gesture,
            side,
            portrait: &picture,
        };
        draw_arm_variant(&db, user_id, asked, &config, &sheet).await
    })
    .await
    .map_err(|error| internal_error(error.to_string()))?
}

/// The gesture asked for: which, on which arm, of which picture.
struct Asked<'a> {
    gesture: &'static str,
    side: &'static str,
    portrait: &'a str,
}

async fn draw_arm_variant(
    db: &DatabaseConnection,
    user_id: i32,
    asked: Asked<'_>,
    config: &image_generation::ImageGenerationConfig,
    sheet: &Sheet,
) -> ApiResult<Json<Value>> {
    let Asked {
        gesture,
        side,
        portrait,
    } = asked;
    let (png, found) = drawn(user_id, (gesture, side), config, sheet).await?;
    keep(db, user_id, (gesture, side, portrait), png, found).await
}

/// One gesture drawn and checked, up to ATTEMPTS times: the picture (a full body's laid back on its enlarged
/// picture) and what the pose model found on it, in that picture's pixels.
async fn drawn(
    user_id: i32,
    (gesture, side): (&'static str, &'static str),
    config: &image_generation::ImageGenerationConfig,
    sheet: &Sheet,
) -> ApiResult<(Vec<u8>, Found)> {
    let reference = &sheet.reference;
    let (width, height) = sheet.drawn;
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
        let raised = sheet.figure.is_some().then_some(side);
        let Some(found) = find_pose(&png, (width, height), raised).await else {
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
        return match &sheet.figure {
            None => Ok((png, found)),
            Some((window, enlarged)) => {
                laid_back(enlarged, *window, sheet.drawn, &png, found).await
            }
        };
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
    Found { hand, keypoints }: Found,
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
        let stored = json!({ "hand": hand, "keypoints": keypoints });
        tokio::fs::write(&path, serde_json::to_vec(&stored).map_err(internal_error)?)
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
        json!({ "key": key, "url": persisted.url, "hand": hand, "keypoints": keypoints }),
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

/// What the pose model found on a drawing, in the portrait's pixels: the
/// points of the hand it shows most surely (the cut seeds there), and every
/// keypoint with its score (the 3D arm is measured from them).
struct Found {
    hand: Vec<[f32; 2]>,
    keypoints: Vec<[f32; 3]>,
}

/// None when no hand is drawn well enough to find.
/// `raised` names a full body's gesturing side: both its hands are drawn, and the surer may be the hanging one
/// (a bust's hanging hand is off the picture).
async fn find_pose(png: &[u8], size: (u32, u32), raised: Option<&str>) -> Option<Found> {
    let pose = match pose_estimation::estimate(png).await {
        Ok(pose) => pose,
        Err(error) => {
            tracing::warn!(%error, "arm variant: the pose model could not look");
            return None;
        }
    };
    let drawn = (pose.width, pose.height);
    Some(Found {
        hand: match raised {
            Some(side) => sided_hand(&pose.keypoints, side, drawn, size)?,
            None => surest_hand(&pose.keypoints, drawn, size)?,
        },
        keypoints: in_portrait(&pose.keypoints, drawn, size),
    })
}

/// Keypoints found on a drawing `drawn` large, moved onto the portrait's `size`.
fn in_portrait(keypoints: &[[f32; 3]], drawn: (u32, u32), size: (u32, u32)) -> Vec<[f32; 3]> {
    let scale = (
        size.0 as f32 / drawn.0.max(1) as f32,
        size.1 as f32 / drawn.1.max(1) as f32,
    );
    keypoints
        .iter()
        .map(|&[x, y, score]| [x * scale.0, y * scale.1, score])
        .collect()
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
    hand_on_portrait(best, drawn, size)
}

/// The hand on the picture's `side` of a figure facing the viewer: its own right (112–132) on the left.
fn sided_hand(
    keypoints: &[[f32; 3]],
    side: &str,
    drawn: (u32, u32),
    size: (u32, u32),
) -> Option<Vec<[f32; 2]>> {
    let hand = if side == "left" {
        keypoints.get(112..133)?
    } else {
        keypoints.get(91..112)?
    };
    hand_on_portrait(hand, drawn, size)
}

fn hand_on_portrait(
    best: &[[f32; 3]],
    drawn: (u32, u32),
    size: (u32, u32),
) -> Option<Vec<[f32; 2]>> {
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
            assert!(prompt.contains(SLEEVE));
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
    fn a_figure_s_gesture_hand_is_the_one_on_its_side_however_sure_the_other() {
        let mut keypoints = vec![[0.0, 0.0, 0.0]; 133];
        // Its own left hand, hanging on the picture's right, found surer than the raised one.
        for point in &mut keypoints[91..112] {
            *point = [400.0, 450.0, 0.95];
        }
        for point in &mut keypoints[112..133] {
            *point = [100.0, 50.0, 0.6];
        }
        let left = sided_hand(&keypoints, "left", (500, 500), (1000, 1000)).unwrap();
        assert_eq!(left[0], [200.0, 100.0]);
        let right = sided_hand(&keypoints, "right", (500, 500), (1000, 1000)).unwrap();
        assert_eq!(right[0], [800.0, 900.0]);
    }

    #[test]
    fn every_keypoint_is_kept_in_the_portrait_s_pixels_with_its_score() {
        let keypoints = vec![[100.0, 50.0, 0.9], [0.0, 400.0, 0.1]];
        assert_eq!(
            in_portrait(&keypoints, (500, 1000), (1000, 1000)),
            vec![[200.0, 50.0, 0.9], [0.0, 400.0, 0.1]]
        );
    }

    #[test]
    fn points_kept_before_the_3d_arm_are_the_hand_alone() {
        let hand = json!([[1.0, 2.0], [3.0, 4.0]]);
        assert_eq!(stored_points(hand.clone()), (hand.clone(), Value::Null));
        let keypoints = json!([[1.0, 2.0, 0.5]]);
        assert_eq!(
            stored_points(json!({ "hand": hand, "keypoints": keypoints })),
            (hand, keypoints)
        );
        assert_eq!(stored_points(Value::Null), (Value::Null, Value::Null));
    }

    #[test]
    fn a_gesture_is_claimed_once_until_released() {
        let first = InFlight::claim("chin-right").expect("free");
        assert!(InFlight::claim("chin-right").is_none());
        drop(first);
        assert!(InFlight::claim("chin-right").is_some());
    }
}
