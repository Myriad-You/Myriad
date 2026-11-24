//! 立绘的手势手臂：让生图模型只改一条手臂（挥手、竖食指、按胸口、托腮），其余
//! 保持原样；前端导入时把改动的手臂抠成平时隐藏的一层，举手时从画面下方抬进来。
//! 画好的图先过两道检查：姿态模型在图上找得到这只手，视觉模型认为除了这条手臂
//! 的姿势什么都没改（袖子、护甲最容易被改掉）。不过关就重画，最多三次，仍不过
//! 就不提供这个手势。两道检查看的是同一张图，同时做。姿态模型自己没能看（进程没
//! 起来、卡死、模型文件缺失）不是这张图的错：对同一张图再看一次，仍不行就以「服务
//! 暂时不可用」结束这个手势，不重画（`hand`）。生图请求没被服务商接下（连不上、
//! 连接中途断开、不属于超时的 5xx）时隔几秒再发一次，之前画好的几张不白画。结果
//! 按立绘归档成私有媒体，换了立绘自然失效；手的关键点存在旁边，前端抠层时用它找到
//! 哪一块是手臂。全身形象的手太小，在放大图上取一扇窗口画，画完超分贴回放大图
//! （`figure`）。请求被受理就答复，在后台画；页面读列表看哪些还在画、哪些没画成
//! （`turns`）。

use std::{future::Future, io::Cursor, sync::Arc};

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod figure;
mod hand;
mod sheet;
use hand::{Found, find_pose, in_portrait};
use sheet::{figure_sheet, laid_back};

use super::{
    ApiResult,
    expressions::{current_portrait, portrait_required},
    internal_error,
    master::{MasterSlot, current_master},
    portrait_generation_config_error, portrait_generation_provider_error, require_merope_enabled,
    require_owner,
    turns::Turns,
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

/// 同一个手势同时只画一次：一次要一两分钟，还要花钱。全身的按「套装/手势」占。
static TURNS: Turns = Turns::new();

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

/// The newest drawing of each gesture on `picture`, the gestures being drawn in `scope`, and the ones whose last
/// drawing was not kept, with what went wrong.
async fn listed(db: &DatabaseConnection, picture: &str, scope: &str) -> ApiResult<Json<Value>> {
    // Before the kept ones are read: a gesture kept in between is then among them.
    let (drawing, failed) = TURNS.listed(scope, picture);
    let mut variants = serde_json::Map::new();
    for key in keys() {
        let found = latest_variant(db, picture, &key).await?;
        variants.insert(key, found.unwrap_or(Value::Null));
    }
    Ok(Json(
        json!({ "variants": variants, "drawing": drawing, "failed": failed }),
    ))
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

/// Takes one gesture arm up to be drawn on the worn bust's picture; the list tells what became of it.
pub async fn generate_arm_variant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(key): Path<String>,
) -> ApiResult<Accepted> {
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
    let sheet = std::future::ready(Ok(Arc::new(sheet)));
    taken_up(db, user_id, (gesture, side, portrait), sheet, &key).await
}

/// Takes one gesture arm up to be drawn on a full-body set's picture: on a window of it
/// enlarged, laid back on the enlarged picture its decomposition was cut from.
pub async fn generate_full_body_arm_variant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((outfit_id, key)): Path<(String, String)>,
) -> ApiResult<Accepted> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let Some((gesture, side)) = parse_key(&key) else {
        return Err(unknown_gesture());
    };
    let master = figure_master(&db, &outfit_id).await?;
    let picture = load_picture(&master).await?;
    // Made once the gesture is claimed: the picture may have to be enlarged first, and a second ask for the
    // same gesture is turned away before it pays for that. One sheet for the set's gestures drawn together.
    let of = master.clone();
    let sheet = async move { figure_sheet(&of, &picture).await };
    let scope = format!("{}{key}", in_flight_scope(Some(&outfit_id)));
    taken_up(db, user_id, (gesture, side, master), sheet, &scope).await
}

/// A gesture taken up: answered before it is drawn.
type Accepted = (StatusCode, Json<Value>);

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

/// Takes a gesture up, once at a time under `claim`, and draws it in the background on the sheet `sheet` makes
/// once it is claimed. The page is answered now and reads the list for what became of it, as one reloaded
/// meanwhile does: a browser keeps too few connections to one site to hold one open for each gesture.
async fn taken_up(
    db: DatabaseConnection,
    user_id: i32,
    (gesture, side, picture): (&'static str, &'static str, String),
    sheet: impl Future<Output = ApiResult<Arc<Sheet>>> + Send + 'static,
    claim: &str,
) -> ApiResult<Accepted> {
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    let Some(turn) = TURNS.claim(claim) else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "This gesture is already being drawn",
                "code": "arm_variant_generation_in_progress"
            })),
        ));
    };
    tracing::info!(gesture, side, provider = %config.provider, model = %config.model, "arm variant started");
    turn.run(picture.clone(), async move {
        let sheet = sheet.await?;
        let asked = Asked {
            gesture,
            side,
            portrait: &picture,
        };
        draw_arm_variant(&db, user_id, asked, &config, &sheet).await
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "key": format!("{gesture}-{side}"), "drawing": true })),
    ))
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
) -> ApiResult<()> {
    let Asked {
        gesture,
        side,
        portrait,
    } = asked;
    let (png, found) = drawn(user_id, (gesture, side), config, sheet).await?;
    keep(db, user_id, (gesture, side, portrait), png, found).await
}

/// One gesture drawn and checked (`tried`): the picture (a full body's laid back on its enlarged picture) and
/// what the pose model found on it, in that picture's pixels.
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
    let (prompt, asked_on) = (prompt.as_str(), &asked_on);
    let draw = || async move {
        // A request the provider did not take up is made once more, the place among the drawings kept
        // meanwhile: the tries before it are paid for already.
        let generated = image_generation::once_more_when_unserved(|| {
            crate::services::ai_cost_ledger::with_site_ai_ledger(
                user_id,
                "merope",
                "arm-variant",
                image_generation::generate_image(config, prompt, width, height, Some(asked_on)),
            )
        })
        .await
        .map_err(portrait_generation_provider_error)?;
        let (bytes, _) = image_generation::load_generated_bytes(generated)
            .await
            .map_err(portrait_generation_provider_error)?;
        Ok::<_, super::ApiError>(if mirrored {
            flip(&bytes)
        } else {
            to_png(&bytes)
        })
    };
    let raised = sheet.figure.is_some().then_some(side);
    let look = |drawing: Vec<u8>| async move { find_pose(&drawing, (width, height), raised).await };
    let check = |drawing: Vec<u8>| {
        let original = reference.clone();
        async move {
            let variant = image_generation::ImageReference::new(drawing, "image/png")?;
            let check = onboarding_ai::check_arm_variant(&original, &variant, side, words).await;
            Ok::<_, image_generation::ImageGenerationError>(check)
        }
    };
    let (png, found) = tried((gesture, side), draw, look, check).await?;
    match &sheet.figure {
        None => Ok((png, found)),
        Some((window, enlarged)) => laid_back(enlarged, *window, sheet.drawn, &png, found).await,
    }
}

/// What the vision model made of a drawing: its verdict, or why it gave none (it is unavailable, or the
/// drawing could not be handed to it).
type Checked = Result<
    Result<onboarding_ai::ArmVariantCheck, onboarding_ai::OnboardingAiError>,
    image_generation::ImageGenerationError,
>;

/// A gesture drawn by `draw` (None: a picture that cannot be read) and checked, up to ATTEMPTS times, until a
/// drawing passes both checks: `look` finds its hand, `check` finds nothing but the arm's pose changed. A
/// drawing that fails either is drawn again. A failure of `draw` or of `look` itself ends the gesture with
/// that failure and nothing more is drawn or checked: neither says anything about the drawing.
async fn tried<F, D, L, C>(
    (gesture, side): (&str, &str),
    mut draw: impl FnMut() -> D,
    look: impl Fn(Vec<u8>) -> L,
    check: impl Fn(Vec<u8>) -> C,
) -> ApiResult<(Vec<u8>, F)>
where
    D: Future<Output = ApiResult<Option<Vec<u8>>>>,
    L: Future<Output = ApiResult<Option<F>>>,
    C: Future<Output = Checked> + Send + 'static,
{
    let mut notes: Vec<String> = Vec::new();
    for attempt in 1..=ATTEMPTS {
        let Some(png) = draw().await? else {
            notes.push("unreadable picture".into());
            continue;
        };
        // Both checks look at this drawing and at nothing of each other's, so neither waits for the other.
        let checks = both_checks(look(png.clone()), check(png.clone()));
        let Some((found, check)) = checks.await? else {
            tracing::info!(
                gesture,
                side,
                attempt,
                "arm variant: no hand found, drawn again"
            );
            notes.push("no hand found".into());
            continue;
        };
        match check.map_err(portrait_generation_provider_error)? {
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
        return Ok((png, found));
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

/// A drawing's two checks at once. `second` is begun now and heard only once `first` has found what it looks
/// for: when it finds nothing the drawing is drawn again without waiting, when it could not look the gesture
/// ends with its failure, and either way `second` ends on its own, unheard (its call is still counted where
/// calls are counted).
async fn both_checks<F, S: Send + 'static>(
    first: impl Future<Output = ApiResult<Option<F>>>,
    second: impl Future<Output = S> + Send + 'static,
) -> ApiResult<Option<(F, S)>> {
    let second = tokio::spawn(second);
    let Some(found) = first.await? else {
        return Ok(None);
    };
    let checked = second
        .await
        .map_err(|error| internal_error(error.to_string()))?;
    Ok(Some((found, checked)))
}

async fn keep(
    db: &DatabaseConnection,
    user_id: i32,
    (gesture, side, portrait): (&str, &str, &str),
    png: Vec<u8>,
    Found { hand, keypoints }: Found,
) -> ApiResult<()> {
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
    image_generation::persist_generated_with_status(
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
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        let first = TURNS.claim("chin-right").expect("free");
        assert!(TURNS.claim("chin-right").is_none());
        drop(first);
        assert!(TURNS.claim("chin-right").is_some());
    }

    /// A check that never answers fails its test instead of hanging it.
    async fn within_a_while<T>(work: impl Future<Output = T>) -> T {
        tokio::time::timeout(std::time::Duration::from_secs(20), work)
            .await
            .expect("the checks did not finish")
    }

    #[tokio::test]
    async fn the_second_check_is_begun_before_the_first_has_answered() {
        let (begun, heard) = tokio::sync::oneshot::channel::<()>();
        // The first answers only once the second has begun: one after the other, it never would.
        let first = async move {
            let found = heard.await.ok().map(|()| "hand");
            Ok::<_, super::super::ApiError>(found)
        };
        let second = async move {
            let _ = begun.send(());
            "same design"
        };
        let both = within_a_while(both_checks(first, second)).await;
        assert_eq!(both.unwrap(), Some(("hand", "same design")));
    }

    #[tokio::test]
    async fn no_hand_found_does_not_wait_for_the_second_check_which_still_ends() {
        let (release, held) = tokio::sync::oneshot::channel::<()>();
        let (ended, end) = tokio::sync::oneshot::channel::<()>();
        // Held until the drawing has been sent back to be drawn again.
        let second = async move {
            let _ = held.await;
            let _ = ended.send(());
        };
        let first = async { Ok::<_, super::super::ApiError>(None::<&str>) };
        let both = within_a_while(both_checks(first, second)).await;
        assert!(matches!(both, Ok(None)));
        release.send(()).unwrap();
        // Not given up half way: its call is made and counted to its end.
        assert!(within_a_while(end).await.is_ok());
    }

    /// The vision model finding nothing but the pose changed.
    fn passed() -> Checked {
        Ok(Ok(onboarding_ai::ArmVariantCheck {
            same_design: true,
            gesture_shown: true,
            differences: Vec::new(),
        }))
    }

    /// A gesture tried with every drawing readable and passed by the vision model, the pose model answering
    /// each look with `seen`: what became of it, how many drawings were made, and how many checks were begun.
    async fn tried_seeing(
        seen: impl Fn(usize) -> ApiResult<Option<&'static str>>,
    ) -> (ApiResult<(Vec<u8>, &'static str)>, usize, usize) {
        let (drawings, checks) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let draw = || {
            let drawing = drawings.fetch_add(1, Ordering::SeqCst) + 1;
            async move { Ok::<_, super::super::ApiError>(Some(vec![drawing as u8])) }
        };
        let look = |drawing: Vec<u8>| {
            let seen = seen(usize::from(drawing[0]));
            async move { seen }
        };
        let check = |_drawing: Vec<u8>| {
            checks.fetch_add(1, Ordering::SeqCst);
            async { passed() }
        };
        let became = within_a_while(tried(("wave", "right"), draw, look, check)).await;
        (
            became,
            drawings.load(Ordering::SeqCst),
            checks.load(Ordering::SeqCst),
        )
    }

    #[tokio::test]
    async fn a_pose_model_that_could_not_look_ends_the_gesture_without_another_drawing() {
        let (became, drawings, checks) = tried_seeing(|_| Err(hand::pose_unavailable())).await;
        let (status, Json(body)) = became.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["code"], "service_unavailable");
        // The one drawing made, and the one check begun beside the look that failed: no other is paid for.
        assert_eq!((drawings, checks), (1, 1));
    }

    #[tokio::test]
    async fn a_drawing_with_no_hand_found_is_drawn_again_up_to_the_last_try() {
        let (became, drawings, checks) = tried_seeing(|_| Ok(None)).await;
        let (status, Json(body)) = became.unwrap_err();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "arm_variant_rejected");
        assert_eq!(body["differences"], json!(vec!["no hand found"; ATTEMPTS]));
        assert_eq!((drawings, checks), (ATTEMPTS, ATTEMPTS));
        // The hand found on the second drawing: that one is kept, and no third is made.
        let (became, drawings, checks) =
            tried_seeing(|drawing| Ok((drawing == 2).then_some("hand"))).await;
        assert_eq!(became.unwrap(), (vec![2], "hand"));
        assert_eq!((drawings, checks), (2, 2));
    }
}
