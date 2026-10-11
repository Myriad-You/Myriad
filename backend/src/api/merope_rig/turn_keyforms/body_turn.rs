//! 全身转身关键形：照转头的做法，以立绘为参考生成整个人向画面右、左转约 20°
//! 的两张图（脚不动），各拆一次层，在 Space 上把全身拆层的每个身体部件
//! （头整体、脖子、上衣、下装、左右大腿小腿、左右鞋、左右手臂）拟合到两张图
//! 上（`/body_keys`），只留下左右方向相反的那部分位移，放到全身的图上。
//! 重心转移照同样做：重心落在画面右腿、左腿的两张对立式站姿图；腿和鞋两边
//! 不对称（放松的那条弯、承重的直），各用各图的拟合。
//!
//! 和头部的半身流程同时跑（不改任务进度）。图与拆层按全身的图存在缓存里；拟合
//! 每次重做。拿不到身体关键形时，全身照常只带头部关键形；重心关键形只和身体
//! 关键形一起用。这一步的失败不让整个任务失败。

use std::io::Cursor;

use serde_json::Value;

use super::{
    ApiResult, BodyProgress, Stage, TurnCache, decompose_waiting, draw_turn, internal_error,
    see_through_error, update,
};
use crate::services::{image_generation, see_through};

/// Image generators draw a standing figure on a 2:3 canvas; a figure padded to
/// it keeps its size and place instead of being stretched to fill.
const BODY_ASPECT: (u32, u32) = (2, 3);

/// Toward (or onto the leg on) the viewer's right and left: the Space's order, the keys' plus and minus.
const BODY_SIDES: [&str; 2] = ["right", "left"];

/// What the figure is drawn doing either way.
#[derive(Clone, Copy, Debug)]
enum Motion {
    /// The whole body turned about 20°.
    Turn,
    /// The weight on one leg, the other free (contrapposto).
    Weight,
}

impl Motion {
    /// Cached drawings go with it: bump when its prompt or framing changes.
    fn cache(self) -> &'static str {
        match self {
            Self::Turn => "body-v1",
            Self::Weight => "weight-v1",
        }
    }

    /// The Space's mode for `/body_keys`, and the job's name for it.
    fn mode(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Weight => "weight",
        }
    }

    fn prompt(self, side: &str) -> String {
        let other = if side == "right" { "left" } else { "right" };
        match self {
            Self::Turn => format!(
                "Edit this exact illustration. Keep the canvas size, framing, the character's size and \
                 the spot she stands on, the outfit, colors, line style and shading exactly the same, on \
                 the same background. Change only the pose: she turns her whole body about 20 degrees \
                 toward the viewer's {side}, a natural three-quarter standing view, feet staying where \
                 they are; her head turns with her body and her eyes keep looking at the viewer. Every \
                 garment, accessory, hair lock and ornament keeps its exact design, colors and length, \
                 seen from the new angle; her arms keep their pose relative to her body and her hands \
                 keep their shape. Clean finished art in the original's style."
            ),
            Self::Weight => format!(
                "Edit this exact illustration. Keep the canvas size, framing, the character's size, the \
                 outfit, colors, line style and shading exactly the same, on the same background, and \
                 keep her facing the viewer squarely: no turn of the body or head. Change only her stance \
                 into a clear contrapposto: her weight is on the leg on the viewer's {side}, which stands \
                 straight; her hips are pushed toward the viewer's {side}, the hip on that side clearly \
                 higher than the other; the leg on the viewer's {other} relaxes, bending at the knee so \
                 that knee comes slightly forward and inward, its heel raised with only the toes touching \
                 the ground. Both feet keep their exact place on the ground. Her shoulders tilt slightly \
                 the opposite way to her hips, her arms hang as they do, her head stays upright and her \
                 eyes keep looking at the viewer. Every garment, accessory and hair lock keeps its exact \
                 design. Clean finished art in the original's style."
            ),
        }
    }
}

/// What the Space fitted for a figure's body (`{canvas, keyforms, drift, knees}` each).
#[derive(Debug, Default, PartialEq)]
pub(super) struct BodyKeys {
    turn: Option<Value>,
    weight: Option<Value>,
}

/// The figure's body turn and weight-shift keys on `picture` (the picture
/// `figure` was decomposed from). The weight shift is made only beside a turn.
#[allow(clippy::too_many_arguments)]
pub(super) async fn body_turn_keys(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    master: &image_generation::ImageReference,
    picture: &image_generation::ImageReference,
    figure: &[u8],
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
) -> BodyKeys {
    let keyed = |motion: Motion| async move {
        match make(
            id, motion, user_id, generation, master, picture, figure, client, cache,
        )
        .await
        {
            Ok(keys) => Some(keys),
            Err((_, axum::Json(error))) => {
                tracing::warn!(job = %id, ?motion, ?error, "body keys could not be made; the figure goes without them");
                update(id, |job| {
                    job.body = Some(BodyProgress {
                        motion: motion.mode(),
                        stage: Stage::Failed,
                        done: 0,
                        total: 1,
                    })
                });
                None
            }
        }
    };
    // The weight shift is the figure's bones' now (Myriad's figureSkeleton):
    // its keys would go unused, so none are drawn for it.
    BodyKeys {
        turn: keyed(Motion::Turn).await,
        weight: None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn make(
    id: &str,
    motion: Motion,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    master: &image_generation::ImageReference,
    picture: &image_generation::ImageReference,
    figure: &[u8],
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
) -> ApiResult<Value> {
    tracing::info!(job = %id, ?motion, "body keys: drawing");
    let report = |stage: Stage, done: u8, total: u8| {
        update(id, |job| {
            job.body = Some(BodyProgress {
                motion: motion.mode(),
                stage,
                done,
                total,
            })
        })
    };
    report(Stage::Generating, 0, 2);
    let (padded, size, place) = padded(&master.bytes)?;
    let padded = image_generation::ImageReference {
        bytes: axum::body::Bytes::from(padded),
        media_type: "image/png".to_string(),
    };
    let mut drawn = Vec::new();
    for side in BODY_SIDES {
        let file = format!("{}-{side}.png", motion.cache());
        let png = match cache.read(&file).await.filter(|png| !png.is_empty()) {
            Some(png) => png,
            None => {
                let png = draw_turn(
                    user_id,
                    generation,
                    &motion.prompt(side),
                    &padded,
                    size.0,
                    size.1,
                )
                .await?;
                let png = cropped(&png, place)?;
                cache.write(&file, &png).await;
                png
            }
        };
        drawn.push(png);
        report(Stage::Generating, drawn.len() as u8, 2);
    }
    tracing::info!(job = %id, ?motion, "body keys: decomposing");
    report(Stage::Decomposing, 0, 2);
    let mut turned = Vec::new();
    for (side, png) in BODY_SIDES.iter().zip(&drawn) {
        let file = format!("{}-{side}.psd", motion.cache());
        let psd = match cache.read(&file).await.filter(|psd| !psd.is_empty()) {
            Some(psd) => psd,
            None => {
                // The head's decompositions run meanwhile; this one waits its turn on the Space.
                let psd = decompose_waiting(
                    client,
                    axum::body::Bytes::from(png.clone()),
                    "image/png",
                    see_through::DecomposeOptions::default(),
                )
                .await?;
                cache.write(&file, &psd).await;
                psd
            }
        };
        turned.push(psd);
        report(Stage::Decomposing, turned.len() as u8, 2);
    }
    tracing::info!(job = %id, ?motion, "body keys: fitting");
    report(Stage::Fitting, 0, 1);
    let [right_psd, left_psd]: [Vec<u8>; 2] = turned.try_into().expect("two sides");
    let [right_png, left_png]: [Vec<u8>; 2] = drawn.try_into().expect("two sides");
    let keys = client
        .body_keys(
            (picture.bytes.to_vec(), picture.media_type.clone()),
            figure.to_vec(),
            [right_psd, left_psd],
            [right_png, left_png],
            motion.mode(),
        )
        .await
        .map_err(see_through_error)?;
    tracing::info!(job = %id, ?motion, "body keys: keyed");
    report(Stage::Done, 1, 1);
    serde_json::from_slice(&keys).map_err(internal_error)
}

/// The master on a white BODY_ASPECT canvas, the canvas' size, and where the master sits on it.
fn padded(bytes: &[u8]) -> ApiResult<(Vec<u8>, (u32, u32), [u32; 4])> {
    let master = image::load_from_memory(bytes)
        .map_err(internal_error)?
        .to_rgba8();
    let (w, h) = master.dimensions();
    let (aw, ah) = BODY_ASPECT;
    let (width, height) = if w * ah < h * aw {
        ((h * aw).div_ceil(ah), h)
    } else {
        (w, (w * ah).div_ceil(aw))
    };
    let (x, y) = ((width - w) / 2, (height - h) / 2);
    let mut canvas = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut canvas, &master, x.into(), y.into());
    Ok((
        png(&image::DynamicImage::ImageRgba8(canvas))?,
        (width, height),
        [x, y, w, h],
    ))
}

/// The master's place cut back out of a drawing on the padded canvas.
fn cropped(bytes: &[u8], [x, y, w, h]: [u32; 4]) -> ApiResult<Vec<u8>> {
    let drawn = image::load_from_memory(bytes).map_err(internal_error)?;
    png(&drawn.crop_imm(x, y, w, h))
}

fn png(image: &image::DynamicImage) -> ApiResult<Vec<u8>> {
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(internal_error)?;
    Ok(out)
}

/// Where each kind of body keys rides in the figure's keys JSON: keys and what the fit found.
const BODY_FIELDS: [(&str, &str); 2] =
    [("bodyKeyforms", "bodyFit"), ("weightKeyforms", "weightFit")];

/// The figure's keys JSON with its body keys beside the head's (`bodyKeyforms`,
/// `weightKeyforms`, and what each fit found in `bodyFit`, `weightFit`), those
/// on the same picture. A figure whose head was not keyed (`null`) gets keys
/// of its own on the body's canvas, the head's empty.
pub(super) fn with_body_keys(keyforms: Vec<u8>, body: Option<&BodyKeys>) -> Vec<u8> {
    let Some(body) = body else { return keyforms };
    let Ok(mut keys) = serde_json::from_slice::<Value>(&keyforms) else {
        return keyforms;
    };
    if keys.is_null() {
        let Some(found) = [&body.turn, &body.weight].into_iter().flatten().next() else {
            return keyforms;
        };
        keys = serde_json::json!({ "canvas": found["canvas"], "keyforms": {} });
    }
    let canvas = keys.get("canvas").cloned();
    let Some(object) = keys.as_object_mut() else {
        return keyforms;
    };
    for (found, (keys_field, fit_field)) in [&body.turn, &body.weight].into_iter().zip(BODY_FIELDS)
    {
        let Some(found) = found else { continue };
        if found.get("canvas") != canvas.as_ref() {
            tracing::warn!(head = ?canvas, body = ?found.get("canvas"), "body keys are on another canvas; left out");
            continue;
        }
        object.insert(keys_field.to_string(), found["keyforms"].clone());
        object.insert(
            fit_field.to_string(),
            serde_json::json!({ "drift": found["drift"], "knees": found["knees"] }),
        );
    }
    serde_json::to_vec(&keys).unwrap_or(keyforms)
}

/// The body motions a figure's keys JSON carries ("turn", "weight"), listed with its archive.
pub(super) fn motions_of(keyforms: &[u8]) -> Vec<String> {
    let Ok(keys) = serde_json::from_slice::<Value>(keyforms) else {
        return Vec::new();
    };
    BODY_FIELDS
        .iter()
        .zip([Motion::Turn, Motion::Weight])
        .filter(|((keys_field, _), _)| keys.get(*keys_field).is_some())
        .map(|(_, motion)| motion.mode().to_string())
        .collect()
}

/// The body keys of a figure's keys JSON, to keep across a refit of the head's.
pub(super) fn body_of(keyforms: &[u8]) -> Option<BodyKeys> {
    let keys: Value = serde_json::from_slice(keyforms).ok()?;
    let read = |(keys_field, fit_field): (&str, &str)| {
        let found = keys.get(keys_field)?.clone();
        let fit = keys.get(fit_field).cloned().unwrap_or(Value::Null);
        Some(serde_json::json!({
            "canvas": keys.get("canvas"),
            "keyforms": found,
            "drift": fit.get("drift"),
            "knees": fit.get("knees"),
        }))
    };
    let [turn, weight] = BODY_FIELDS.map(read);
    (turn.is_some() || weight.is_some()).then_some(BodyKeys { turn, weight })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        png(&image::DynamicImage::ImageRgba8(image)).unwrap()
    }

    #[test]
    fn a_tall_figure_is_padded_sideways_to_two_by_three_and_cut_back() {
        let (bytes, size, place) = padded(&picture(617, 1443)).unwrap();
        assert_eq!(size, (962, 1443));
        assert_eq!(place, [172, 0, 617, 1443]);
        let back = image::load_from_memory(&cropped(&bytes, place).unwrap()).unwrap();
        assert_eq!((back.width(), back.height()), (617, 1443));
        assert_eq!(back.to_rgba8().get_pixel(0, 0).0, [10, 20, 30, 255]);
        let wide = padded(&picture(900, 900)).unwrap();
        assert_eq!((wide.1, wide.2), ((900, 1350), [0, 225, 900, 900]));
    }

    #[test]
    fn body_keys_ride_beside_the_heads_and_survive_a_refit() {
        let head = br#"{"canvas":[1751,4096],"keyforms":{"face":{}}}"#.to_vec();
        let found = |n: i32| {
            serde_json::json!({
                "canvas": [1751, 4096],
                "keyforms": {"topwear": {"plus": n, "minus": -n}},
                "drift": {"topwear": [3, 4]},
                "knees": {"L": 2600},
            })
        };
        let body = BodyKeys {
            turn: Some(found(1)),
            weight: Some(found(2)),
        };
        let merged = with_body_keys(head.clone(), Some(&body));
        let keys: Value = serde_json::from_slice(&merged).unwrap();
        assert_eq!(keys["keyforms"]["face"], serde_json::json!({}));
        assert_eq!(keys["bodyKeyforms"]["topwear"]["plus"], 1);
        assert_eq!(keys["weightKeyforms"]["topwear"]["plus"], 2);
        assert_eq!(keys["bodyFit"]["knees"]["L"], 2600);
        assert_eq!(body_of(&merged).unwrap(), body);
        assert!(body_of(&head).is_none());
        assert_eq!(motions_of(&merged), ["turn", "weight"]);
        assert!(motions_of(&head).is_empty());
        // The body keyed alone: keys of its own, the head's empty.
        let alone: Value =
            serde_json::from_slice(&with_body_keys(b"null".to_vec(), Some(&body))).unwrap();
        assert_eq!(alone["canvas"], serde_json::json!([1751, 4096]));
        assert_eq!(alone["keyforms"], serde_json::json!({}));
        assert_eq!(alone["weightKeyforms"]["topwear"]["plus"], 2);
        let none = BodyKeys {
            turn: None,
            weight: None,
        };
        assert_eq!(with_body_keys(b"null".to_vec(), Some(&none)), b"null");
        let turn_only = BodyKeys {
            turn: Some(found(1)),
            weight: None,
        };
        assert_eq!(
            body_of(&with_body_keys(head.clone(), Some(&turn_only))).unwrap(),
            turn_only
        );
        let elsewhere = serde_json::json!({ "canvas": [1088, 1664], "keyforms": {} });
        let away = BodyKeys {
            turn: Some(elsewhere),
            weight: None,
        };
        let kept: Value =
            serde_json::from_slice(&with_body_keys(head.clone(), Some(&away))).unwrap();
        assert!(kept.get("bodyKeyforms").is_none());
    }

    #[test]
    fn the_weight_shift_is_drawn_as_a_contrapposto_onto_each_side() {
        let right = Motion::Weight.prompt("right");
        assert!(right.contains("weight is on the leg on the viewer's right"));
        assert!(right.contains("the leg on the viewer's left relaxes"));
        assert!(
            Motion::Turn
                .prompt("left")
                .contains("toward the viewer's left")
        );
        assert_ne!(Motion::Turn.cache(), Motion::Weight.cache());
    }
}
