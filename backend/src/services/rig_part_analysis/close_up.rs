//! A second, close look at the parts the runtime will cut out of the layers
//! they share (partSplit on the page). The inventory sees the whole figure at
//! once, and for a small hanging piece its box is often off by half its size,
//! or names something that does not hang at all (a brooch pinned flat called
//! an earring). Each such part is looked at again in a close-up of where the
//! inventory put it: placed exactly, or dropped when it is not there.

use std::time::Duration;

use futures::future::join_all;
use serde_json::Value;

use super::{Context, normalized};
use crate::services::{
    ai_config::AiConfig,
    ai_cost_ledger::record_ai_call_from_attribution,
    image_generation::ImageReference,
    rig_chest_analysis::{parse_json_object, request_estimate},
};

const PROMPT_VERSION: &str = "anime25d-part-close-up-v1";
/// A hanging part this small, as a share of the canvas, is cut out of its
/// layer when that layer holds other drawing too (partSplit's own limit).
const CUT_AREA_SHARE: f32 = 0.03;
/// The close-up spans this many times the part's box.
const SPAN: f32 = 3.0;
/// The close-up is sent no larger than this on its long side.
const MAX_SIDE: u32 = 768;
/// Each close look gets this long.
const TIMEOUT: Duration = Duration::from_secs(300);

/// Whether a part will be cut out of the layer it shares, so needs its exact place.
fn cut_out(part: &Value, context: &Context) -> bool {
    let Some(bounds) = bounds_of(part, "box", 4) else {
        return false;
    };
    part.get("motion").and_then(Value::as_str) == Some("dangle")
        && part.get("separate").and_then(Value::as_bool) != Some(true)
        && (bounds[2] - bounds[0]) * (bounds[3] - bounds[1])
            <= CUT_AREA_SHARE * context.width * context.height
}

fn bounds_of(part: &Value, key: &str, count: usize) -> Option<Vec<f32>> {
    let list = part.get(key)?.as_array()?;
    (list.len() == count)
        .then(|| list.iter().map(|v| v.as_f64().map(|v| v as f32)).collect())
        .flatten()
}

/// The close-up around a part's box, in the picture's pixels: `[x0, y0, x1, y1]`.
fn close_up(bounds: &[f32], context: &Context, width: u32, height: u32) -> Option<[u32; 4]> {
    let sx = width as f32 / context.width;
    let sy = height as f32 / context.height;
    let pad = ((bounds[2] - bounds[0]) * sx).max((bounds[3] - bounds[1]) * sy) * (SPAN - 1.0) / 2.0;
    let x0 = (bounds[0] * sx - pad).floor().max(0.0) as u32;
    let y0 = (bounds[1] * sy - pad).floor().max(0.0) as u32;
    let x1 = ((bounds[2] * sx + pad).ceil() as u32).min(width);
    let y1 = ((bounds[3] * sy + pad).ceil() as u32).min(height);
    (x1 > x0 + 8 && y1 > y0 + 8).then_some([x0, y0, x1, y1])
}

/// What a close look found.
#[derive(Debug, PartialEq)]
enum Found {
    /// It is not there: what the inventory saw does not hang.
    Absent,
    /// Its box and anchor, in canvas coordinates.
    At([f32; 4], [f32; 2]),
}

/// A close look's answer, placed back on the canvas.
fn read(
    answer: &Value,
    crop: [u32; 4],
    context: &Context,
    width: u32,
    height: u32,
) -> Option<Found> {
    if !answer.get("present").and_then(Value::as_bool)? {
        return Some(Found::Absent);
    }
    let bounds = normalized(&bounds_of(answer, "box", 4)?)?;
    let anchor = normalized(&bounds_of(answer, "anchor", 2)?)?;
    if !(bounds[2] > bounds[0] && bounds[3] > bounds[1]) {
        return None;
    }
    let to_x =
        |v: f32| (crop[0] as f32 + v * (crop[2] - crop[0]) as f32) * context.width / width as f32;
    let to_y =
        |v: f32| (crop[1] as f32 + v * (crop[3] - crop[1]) as f32) * context.height / height as f32;
    Some(Found::At(
        [
            to_x(bounds[0]),
            to_y(bounds[1]),
            to_x(bounds[2]),
            to_y(bounds[3]),
        ],
        [to_x(anchor[0]), to_y(anchor[1])],
    ))
}

fn prompt(part: &Value) -> String {
    let name = part
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("hanging part");
    let material = part
        .get("material")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!(
        r#"Task version: {PROMPT_VERSION}
This is a close-up from a picture of an anime character. A rigger is about to cut one hanging part out of the drawing so it can swing on its own: "{name}" (made of {material}), which should hang from one point somewhere in this close-up.

If such a part is drawn here, hanging free, give a tight box around all of it, with what it hangs by (a string, a chain, ribbon tails), and the point it hangs from. If what is here does not hang (for example a brooch or a pattern lying flat on the cloth), or nothing like it is here, say it is not present.

Coordinates are normalized to this close-up: x from left to right, y from top to bottom, 0 to 1.
Return exactly one JSON object and no Markdown: {{"present":true,"box":[0,0,0,0],"anchor":[0,0]}} or {{"present":false}}"#
    )
}

/// The close-up's pixels, as sent.
fn picture_of(picture: &image::DynamicImage, crop: [u32; 4]) -> Option<ImageReference> {
    let mut close = picture.crop_imm(crop[0], crop[1], crop[2] - crop[0], crop[3] - crop[1]);
    if close.width().max(close.height()) > MAX_SIDE {
        close = close.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let mut png = std::io::Cursor::new(Vec::new());
    close.write_to(&mut png, image::ImageFormat::Png).ok()?;
    ImageReference::new(png.into_inner(), "image/png").ok()
}

/// Looks again, close up, at each part that will be cut out of the layer it
/// shares; places it exactly, or drops it when it is not there. A look that
/// fails leaves its part as the inventory gave it.
pub(super) async fn look_closer(
    profile: &mut Value,
    context: &Context,
    config: &AiConfig,
    reference: &ImageReference,
) {
    let Ok(picture) = image::load_from_memory(&reference.bytes) else {
        return;
    };
    let (width, height) = (picture.width(), picture.height());
    let Some(parts) = profile.get("parts").and_then(Value::as_array) else {
        return;
    };
    let looks = parts
        .iter()
        .enumerate()
        .filter(|(_, part)| cut_out(part, context))
        .filter_map(|(index, part)| {
            let crop = close_up(&bounds_of(part, "box", 4)?, context, width, height)?;
            Some((index, crop, picture_of(&picture, crop)?, prompt(part)))
        })
        .collect::<Vec<_>>();
    let answers = join_all(
        looks
            .into_iter()
            .map(|(index, crop, close, prompt)| async move {
                let response =
                    tokio::time::timeout(TIMEOUT, request_estimate(config, &close, &prompt))
                        .await
                        .unwrap_or_else(|_| Err("part close-up timed out".to_string()));
                record_ai_call_from_attribution(
                    config.provider.as_str(),
                    &config.model,
                    prompt.len(),
                    response.as_ref().map_or(0, String::len),
                    if response.is_ok() {
                        "completed"
                    } else {
                        "failed"
                    },
                    response.as_ref().err().map(|_| "AI_PROVIDER_ERROR"),
                )
                .await;
                let found = response
                    .as_deref()
                    .ok()
                    .and_then(parse_json_object)
                    .and_then(|answer| read(&answer, crop, context, width, height));
                if found.is_none() {
                    tracing::warn!(
                        index,
                        "part close-up gave nothing usable; kept as inventoried"
                    );
                }
                (index, found)
            }),
    )
    .await;
    apply(profile, answers);
}

/// Places or drops the parts looked at again.
fn apply(profile: &mut Value, answers: Vec<(usize, Option<Found>)>) {
    let Some(parts) = profile.get_mut("parts").and_then(Value::as_array_mut) else {
        return;
    };
    let mut absent = Vec::new();
    for (index, found) in answers {
        match (found, parts.get_mut(index)) {
            (Some(Found::At(bounds, anchor)), Some(part)) => {
                part["box"] = serde_json::json!(bounds);
                part["anchor"] = serde_json::json!(anchor);
            }
            (Some(Found::Absent), Some(_)) => absent.push(index),
            _ => {}
        }
    }
    absent.sort_unstable();
    for index in absent.into_iter().rev() {
        let gone = parts.remove(index);
        tracing::info!(name = ?gone.get("name"), "part close-up found nothing hanging there; dropped");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn context() -> Context {
        Context {
            width: 1000.0,
            height: 2000.0,
            layers: Vec::new(),
        }
    }

    #[test]
    fn only_small_hanging_parts_drawn_with_others_are_looked_at_again() {
        let part = |motion: &str, separate: bool, bounds: [f32; 4]| json!({ "motion": motion, "separate": separate, "box": bounds });
        assert!(cut_out(
            &part("dangle", false, [400.0, 500.0, 450.0, 600.0]),
            &context()
        ));
        assert!(
            !cut_out(
                &part("dangle", true, [400.0, 500.0, 450.0, 600.0]),
                &context()
            ),
            "alone in its layer, it is not cut"
        );
        assert!(
            !cut_out(
                &part("sway", false, [400.0, 500.0, 450.0, 600.0]),
                &context()
            ),
            "cloth sways in place"
        );
        assert!(
            !cut_out(&part("dangle", false, [0.0, 0.0, 600.0, 600.0]), &context()),
            "too big to be a hanging piece"
        );
    }

    #[test]
    fn a_close_up_spans_three_times_the_part_in_the_pictures_pixels() {
        // The picture is half the canvas: the box 400–450 × 500–600 is 200–225 × 250–300 there.
        let crop = close_up(&[400.0, 500.0, 450.0, 600.0], &context(), 500, 1000).unwrap();
        assert_eq!(crop, [150, 200, 275, 350]);
        assert_eq!(
            close_up(&[0.0, 0.0, 20.0, 20.0], &context(), 500, 1000).unwrap()[0..2],
            [0, 0],
            "clamped to the picture"
        );
    }

    #[test]
    fn its_answer_is_placed_back_on_the_canvas_or_drops_the_part() {
        let crop = [150, 200, 275, 350];
        let found = read(
            &json!({ "present": true, "box": [0.2, 0.0, 0.6, 1.0], "anchor": [0.4, 0.0] }),
            crop,
            &context(),
            500,
            1000,
        );
        assert_eq!(
            found,
            Some(Found::At([350.0, 400.0, 450.0, 700.0], [400.0, 400.0]))
        );
        let thousand = read(
            &json!({ "present": true, "box": [200, 0, 600, 1000], "anchor": [400, 0] }),
            crop,
            &context(),
            500,
            1000,
        );
        assert_eq!(thousand, found, "on the 0–1000 scale too");
        assert_eq!(
            read(&json!({ "present": false }), crop, &context(), 500, 1000),
            Some(Found::Absent)
        );
        assert_eq!(
            read(
                &json!({ "present": true, "box": [0.5, 0.5, 0.5, 0.9], "anchor": [0.5, 0.5] }),
                crop,
                &context(),
                500,
                1000
            ),
            None
        );

        let mut profile = json!({ "parts": [{ "name": "brooch" }, { "name": "charm", "box": [0, 0, 1, 1], "anchor": [0, 0] }, { "name": "skirt" }] });
        apply(
            &mut profile,
            vec![
                (0, Some(Found::Absent)),
                (1, Some(Found::At([1.0, 2.0, 3.0, 4.0], [2.0, 2.0]))),
            ],
        );
        let parts = profile["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2, "what does not hang is dropped");
        assert_eq!(parts[0]["name"], "charm");
        assert_eq!(parts[0]["box"], json!([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(parts[1]["name"], "skirt");
    }
}
