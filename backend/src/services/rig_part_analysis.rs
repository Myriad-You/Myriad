//! One-shot visual inventory of a rig's moving parts.
//!
//! A professional rig gives every part that moves on its own (an earring, a
//! hanging charm, a skirt, a cardigan's open front, a ribbon, a tail) its own
//! layer, its own bone and its own swing, tuned by what it is made of. The
//! vision model looks at the picture once during import preview and names
//! those parts: where each is fixed, which bone carries it, how it moves and
//! what it is made of. The result is persisted in playback JSON as
//! `partProfile`; rendering never calls AI and derives the swing from it.
//! The small hanging parts the page will cut out of the layers they share are
//! then looked at again, close up (close_up).

mod close_up;

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    config::ModelTier,
    services::{
        ai_config::get_ai_config_for_tier,
        ai_cost_ledger::record_ai_call_from_attribution,
        image_generation::ImageReference,
        rig_chest_analysis::{parse_json_object, request_estimate},
    },
};

const PROFILE_VERSION: u8 = 1;
const PROMPT_VERSION: &str = "anime25d-part-inventory-v1";
const MAX_PARTS: usize = 10;
const MAX_NAME: usize = 40;
const MAX_MATERIAL: usize = 20;
/// A vision model gets this long to answer.
const INVENTORY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);
/// The picture is sent no larger than this on its long side: a standing
/// figure's composite is thousands of pixels tall, which only slows the model.
const MAX_SIDE: u32 = 1600;

/// The reference scaled down to `MAX_SIDE`, or none when it is small enough or unreadable.
fn smaller(reference: &ImageReference) -> Option<ImageReference> {
    let picture = image::load_from_memory(&reference.bytes).ok()?;
    if picture.width().max(picture.height()) <= MAX_SIDE {
        return None;
    }
    let mut png = std::io::Cursor::new(Vec::new());
    picture
        .resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    ImageReference::new(png.into_inner(), "image/png").ok()
}

/// Bones a part may hang from; sides are the picture's (L is on the left as seen).
const BONES: [&str; 17] = [
    "head",
    "neck",
    "chest",
    "waist",
    "hips",
    "upperArmL",
    "upperArmR",
    "forearmL",
    "forearmR",
    "handL",
    "handR",
    "thighL",
    "thighR",
    "shinL",
    "shinR",
    "footL",
    "footR",
];
/// How a part moves on its own once its bone has carried it.
const MOTIONS: [&str; 4] = ["rigid", "sway", "dangle", "flutter"];
const MATERIALS: [&str; 16] = [
    "hair", "fur", "feather", "silk", "satin", "chiffon", "tulle", "organza", "cotton", "knit",
    "denim", "leather", "ribbon", "chain", "metal", "plastic",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiInventory {
    /// Each part on its own: one a model got wrong does not lose the others.
    #[serde(default)]
    parts: Vec<Value>,
}

#[derive(Debug)]
struct AiPart {
    name: String,
    layer: Option<String>,
    separate: bool,
    bounds: [f32; 4],
    anchor: [f32; 2],
    bone: &'static str,
    motion: &'static str,
    material: String,
    stiffness: f32,
    weight: f32,
}

/// Coordinates normalized as asked, or on the 0–1000 scale some models use.
fn normalized(values: &[f32]) -> Option<Vec<f32>> {
    let most = values.iter().copied().fold(0.0_f32, f32::max);
    let scale = if most <= 1.0 + 1e-3 {
        1.0
    } else if most <= 1000.0 {
        1000.0
    } else {
        return None;
    };
    values
        .iter()
        .map(|v| (v.is_finite() && *v >= 0.0).then(|| (v / scale).min(1.0)))
        .collect()
}

/// A share 0–1, or one a model gave on a 0–10 or 0–100 scale.
fn share(value: f32) -> Option<f32> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some(if value <= 1.0 {
        value
    } else if value <= 10.0 {
        value / 10.0
    } else {
        (value / 100.0).min(1.0)
    })
}

/// The bone named, or the nearest one its words name; the chest when none.
fn bone_named(name: &str) -> &'static str {
    if let Some(bone) = BONES.iter().find(|bone| **bone == name) {
        return bone;
    }
    let lower = name.to_ascii_lowercase();
    let side = if lower.contains("left") || lower.ends_with('l') {
        "L"
    } else if lower.contains("right") || lower.ends_with('r') {
        "R"
    } else {
        ""
    };
    let limb = ["forearm", "thigh", "shin", "foot", "hand"]
        .into_iter()
        .find(|limb| lower.contains(limb));
    if let Some(limb) = limb.filter(|_| !side.is_empty()) {
        let wanted = format!("{limb}{side}");
        if let Some(bone) = BONES.iter().find(|bone| **bone == wanted) {
            return bone;
        }
    }
    if lower.contains("hip") || lower.contains("pelvis") {
        "hips"
    } else if lower.contains("waist") || lower.contains("belly") {
        "waist"
    } else if lower.contains("head") || lower.contains("ear") || lower.contains("hair") {
        "head"
    } else if lower.contains("neck") {
        "neck"
    } else {
        "chest"
    }
}

fn motion_named(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    MOTIONS
        .iter()
        .find(|motion| **motion == lower)
        .copied()
        .or(match lower.as_str() {
            "swing" | "hang" | "hanging" | "pendulum" => Some("dangle"),
            "flow" | "trail" | "cloth" => Some("sway"),
            "flap" | "wave" => Some("flutter"),
            "static" | "fixed" | "none" => Some("rigid"),
            _ => None,
        })
}

impl AiPart {
    fn read(value: &Value) -> Option<Self> {
        let numbers = |key: &str, count: usize| -> Option<Vec<f32>> {
            let list = value.get(key)?.as_array()?;
            if list.len() != count {
                return None;
            }
            let found = list
                .iter()
                .map(|v| v.as_f64().map(|v| v as f32))
                .collect::<Option<Vec<_>>>()?;
            normalized(&found)
        };
        let bounds = numbers("box", 4)?;
        let anchor = numbers("anchor", 2)?;
        let material = value.get("material")?.as_str()?.trim().to_ascii_lowercase();
        if material.is_empty() || material.chars().count() > MAX_MATERIAL {
            return None;
        }
        Some(Self {
            name: value.get("name")?.as_str()?.trim().to_string(),
            layer: value
                .get("layer")
                .and_then(Value::as_str)
                .map(str::to_string),
            separate: value
                .get("separate")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            bounds: [bounds[0], bounds[1], bounds[2], bounds[3]],
            anchor: [anchor[0], anchor[1]],
            bone: bone_named(value.get("bone")?.as_str()?),
            motion: motion_named(value.get("motion")?.as_str()?)?,
            material,
            stiffness: share(value.get("stiffness")?.as_f64()? as f32)?,
            weight: share(value.get("weight")?.as_f64()? as f32)?,
        })
    }
}

struct LayerHint {
    name: String,
    role: String,
    bounds: [f32; 4],
}

struct Context {
    width: f32,
    height: f32,
    layers: Vec<LayerHint>,
}

fn context(playback: &Value) -> Option<Context> {
    let width = playback.pointer("/pixelCanvas/width")?.as_f64()? as f32;
    let height = playback.pointer("/pixelCanvas/height")?.as_f64()? as f32;
    if !(width > 0.0 && height > 0.0) {
        return None;
    }
    let layers = playback
        .get("layers")?
        .as_array()?
        .iter()
        .filter_map(|layer| {
            let number = |name: &str| {
                layer
                    .get(name)?
                    .as_f64()
                    .map(|v| v as f32)
                    .filter(|v| v.is_finite())
            };
            let role = layer.get("role")?.as_str()?.to_string();
            // Expression overlays and facial features are not parts that hang.
            if !PART_ROLES.contains(&role.as_str()) {
                return None;
            }
            Some(LayerHint {
                name: layer.get("name")?.as_str()?.to_string(),
                role,
                bounds: [number("x")?, number("y")?, number("w")?, number("h")?],
            })
        })
        .collect();
    Some(Context {
        width,
        height,
        layers,
    })
}

/// Layers a moving part may be drawn in.
const PART_ROLES: [&str; 16] = [
    "front-hair",
    "back-hair",
    "headwear",
    "earwear",
    "eyewear",
    "neckwear",
    "neck",
    "topwear",
    "handwear",
    "bottomwear",
    "legwear",
    "footwear",
    "tail",
    "wings",
    "objects",
    "unknown",
];

/// Keeps a profile carried over from the preview, or an empty one.
pub fn ensure_profile(playback: &mut Value) -> bool {
    let Some(context) = context(playback) else {
        return false;
    };
    if playback
        .get("partProfile")
        .is_some_and(|profile| valid_profile(profile, &context))
    {
        return true;
    }
    insert_profile(playback, empty_profile())
}

/// Asks one configured vision model for the rig's moving parts: an
/// `ai-vision` profile, or none when no model is set or its answer is unusable.
/// It runs after the import, off its path, so a slow model only delays it.
pub async fn inventory(playback: &Value, reference: &ImageReference) -> Option<Value> {
    let context = context(playback)?;
    let Ok(config) = get_ai_config_for_tier(ModelTier::Standard).await else {
        tracing::warn!("part inventory skipped because no Standard AI model is configured");
        return None;
    };
    let prompt = prompt(&context);
    let small = smaller(reference);
    let response = tokio::time::timeout(
        INVENTORY_TIMEOUT,
        request_estimate(&config, small.as_ref().unwrap_or(reference), &prompt),
    )
    .await
    .unwrap_or_else(|_| Err("part inventory timed out".to_string()));
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
    let profile = response
        .as_deref()
        .ok()
        .and_then(parse_json_object)
        .and_then(|value| serde_json::from_value::<AiInventory>(value).ok())
        .map(|inventory| profile_from(&context, inventory));
    if profile.is_none() {
        match &response {
            Err(error) => tracing::warn!(%error, "part inventory failed"),
            Ok(raw) => tracing::warn!(
                raw = %raw.chars().take(1500).collect::<String>(),
                "part inventory response was unusable"
            ),
        }
    }
    // What the page will cut out of a shared layer is placed exactly first.
    let mut profile = profile?;
    close_up::look_closer(&mut profile, &context, &config, reference).await;
    Some(profile)
}

/// Whether a playback's part profile is one the vision model gave.
pub fn has_inventory(playback: &Value) -> bool {
    context(playback).is_some_and(|context| {
        playback.get("partProfile").is_some_and(|profile| {
            valid_profile(profile, &context)
                && profile.get("source").and_then(Value::as_str) == Some("ai-vision")
        })
    })
}

fn empty_profile() -> Value {
    json!({ "version": PROFILE_VERSION, "source": "none", "parts": [] })
}

fn insert_profile(playback: &mut Value, profile: Value) -> bool {
    let Some(object) = playback.as_object_mut() else {
        return false;
    };
    object.insert("partProfile".to_string(), profile);
    true
}

/// Normalized AI parts in canvas pixels, the unusable ones dropped.
fn profile_from(context: &Context, inventory: AiInventory) -> Value {
    let names: HashSet<&str> = context
        .layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect();
    let parts: Vec<Value> = inventory
        .parts
        .iter()
        .filter_map(AiPart::read)
        .filter(|part| {
            part.bounds[2] > part.bounds[0]
                && part.bounds[3] > part.bounds[1]
                && !part.name.is_empty()
        })
        .take(MAX_PARTS)
        .map(|part| {
            let layer = part.layer.filter(|layer| names.contains(layer.as_str()));
            json!({
                "name": part.name.trim().chars().take(MAX_NAME).collect::<String>(),
                "separate": part.separate && layer.is_some(),
                "layer": layer,
                "box": [
                    part.bounds[0] * context.width,
                    part.bounds[1] * context.height,
                    part.bounds[2] * context.width,
                    part.bounds[3] * context.height,
                ],
                "anchor": [part.anchor[0] * context.width, part.anchor[1] * context.height],
                "bone": part.bone,
                "motion": part.motion,
                "material": part.material,
                "stiffness": part.stiffness,
                "weight": part.weight,
            })
        })
        .collect();
    json!({ "version": PROFILE_VERSION, "source": "ai-vision", "parts": parts })
}

fn valid_profile(profile: &Value, context: &Context) -> bool {
    let within = |value: &Value, limit: f32| {
        value
            .as_f64()
            .is_some_and(|v| v.is_finite() && (-1.0..=f64::from(limit) + 1.0).contains(&v))
    };
    let unit = |value: Option<&Value>| {
        value
            .and_then(Value::as_f64)
            .is_some_and(|v| (0.0..=1.0).contains(&v))
    };
    let names: HashSet<&str> = context
        .layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect();
    profile.get("version").and_then(Value::as_u64) == Some(u64::from(PROFILE_VERSION))
        && matches!(
            profile.get("source").and_then(Value::as_str),
            Some("ai-vision" | "none")
        )
        && profile
            .get("parts")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts.len() <= MAX_PARTS
                    && parts.iter().all(|part| {
                        let bounds = part.get("box").and_then(Value::as_array);
                        let anchor = part.get("anchor").and_then(Value::as_array);
                        part.get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|name| {
                                !name.is_empty() && name.chars().count() <= MAX_NAME
                            })
                            && part.get("layer").is_some_and(|layer| {
                                layer.is_null()
                                    || layer.as_str().is_some_and(|name| names.contains(name))
                            })
                            && bounds.is_some_and(|b| {
                                b.len() == 4
                                    && within(&b[0], context.width)
                                    && within(&b[1], context.height)
                                    && within(&b[2], context.width)
                                    && within(&b[3], context.height)
                            })
                            && anchor.is_some_and(|a| {
                                a.len() == 2
                                    && within(&a[0], context.width)
                                    && within(&a[1], context.height)
                            })
                            && part
                                .get("bone")
                                .and_then(Value::as_str)
                                .is_some_and(|bone| BONES.contains(&bone))
                            && part
                                .get("motion")
                                .and_then(Value::as_str)
                                .is_some_and(|motion| MOTIONS.contains(&motion))
                            && part.get("material").and_then(Value::as_str).is_some_and(
                                |material| {
                                    !material.is_empty() && material.chars().count() <= MAX_MATERIAL
                                },
                            )
                            && unit(part.get("stiffness"))
                            && unit(part.get("weight"))
                    })
            })
}

fn prompt(context: &Context) -> String {
    let layers = context
        .layers
        .iter()
        .map(|layer| {
            format!(
                "- {} ({}): x={:.3}, y={:.3}, width={:.3}, height={:.3}",
                layer.name,
                layer.role,
                layer.bounds[0] / context.width,
                layer.bounds[1] / context.height,
                layer.bounds[2] / context.width,
                layer.bounds[3] / context.height,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"Task version: {PROMPT_VERSION}
This front-facing anime character is being rigged for 2D animation the way a professional Live2D rigger would: every part that moves on its own gets its own layer, hangs from a bone, and swings by what it is made of. List those parts.

Include only parts that would visibly move or swing on their own when the character sways, turns or breathes: hanging or dangling accessories (earrings, charms, pendants, chains, tassels, keychains), ribbons and bow tails, loose cloth panels (an open cardigan or jacket front, a cape, coat tails, a hem or peplum that hangs free, sleeves' loose cuffs), skirts and their layers, tails, wings, ears that are not part of the head's outline, scarves. Do not list the face, eyes, mouth, or the hair as a whole; a long separate lock or a hair ornament that dangles may be listed. Judge from what is drawn, not from a garment's usual name.

For each part give:
- name: a few plain words (English).
- layer: which rig layer below it is drawn in (exact name), or null if none fits.
- separate: true when that layer is the part alone; false when the layer also holds other things (for example an earring drawn into the hair layer).
- box: [x0, y0, x1, y1] tight around the part.
- anchor: [x, y], the point where it is fixed and hangs from.
- bone: the bone that carries the anchor, one of: {bones}. L and R are the left and right of the picture as seen, not the character's.
- motion: "rigid" (moves only with its bone), "sway" (a cloth panel or skirt that trails its bone softly), "dangle" (hangs from one point and swings like a pendulum), "flutter" (very light, long and quick: ribbons, chiffon, feathers).
- material: one of: {materials}.
- stiffness: 0 limp to 1 stiff (organza and leather are stiff, chiffon and ribbons are limp).
- weight: 0 very light to 1 heavy (metal and leather are heavy, tulle and feathers are light).

All coordinates are normalized to the full image: x from left to right, y from top to bottom. List at most {MAX_PARTS} parts, the most visible first.

Rig layers:
{layers}

Return exactly one JSON object and no Markdown:
{{"parts":[{{"name":"","layer":null,"separate":false,"box":[0,0,0,0],"anchor":[0,0],"bone":"chest","motion":"dangle","material":"metal","stiffness":0.5,"weight":0.5}}]}}"#,
        bones = BONES.join(", "),
        materials = MATERIALS.join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playback() -> Value {
        json!({
            "pixelCanvas": { "width": 1000, "height": 2000 },
            "layers": [
                { "name": "front-hair", "role": "front-hair", "x": 300, "y": 50, "w": 400, "h": 400 },
                { "name": "bottomwear", "role": "bottomwear", "x": 300, "y": 900, "w": 400, "h": 300 },
                { "name": "eyelash-L", "role": "eyelash", "x": 400, "y": 300, "w": 50, "h": 20 },
            ],
        })
    }

    #[test]
    fn parts_are_kept_in_canvas_pixels_and_bad_ones_dropped() {
        let context = context(&playback()).unwrap();
        assert_eq!(
            context.layers.len(),
            2,
            "facial features are not part layers"
        );
        let inventory: AiInventory = serde_json::from_value(json!({ "parts": [
            { "name": "cage earring", "layer": "front-hair", "separate": true, "box": [0.4, 0.2, 0.45, 0.3],
              "anchor": [0.42, 0.2], "bone": "head", "motion": "dangle", "material": "metal", "stiffness": 0.8, "weight": 0.7 },
            { "name": "skirt", "layer": "nowhere", "box": [0.3, 0.45, 0.7, 0.6], "anchor": [0.5, 0.45],
              "bone": "hips", "motion": "sway", "material": "cotton", "stiffness": 0.4, "weight": 0.4 },
            { "name": "bad motion", "box": [0.1, 0.1, 0.2, 0.2], "anchor": [0.1, 0.1],
              "bone": "chest", "motion": "spin", "material": "metal", "stiffness": 0.5, "weight": 0.5 },
            { "name": "flipped", "box": [0.3, 0.3, 0.2, 0.4], "anchor": [0.3, 0.3],
              "bone": "chest", "motion": "dangle", "material": "metal", "stiffness": 0.5, "weight": 0.5 },
        ]})).unwrap();
        let profile = profile_from(&context, inventory);
        let parts = profile["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["box"], json!([400.0, 400.0, 450.0, 600.0]));
        assert_eq!(parts[0]["separate"], true);
        assert_eq!(
            parts[1]["layer"],
            Value::Null,
            "an unknown layer name is dropped"
        );
        assert_eq!(parts[1]["separate"], false);
        assert!(valid_profile(&profile, &context));
        let mut carried = playback();
        carried["partProfile"] = profile.clone();
        assert!(ensure_profile(&mut carried));
        assert_eq!(
            carried["partProfile"], profile,
            "a valid preview profile is kept"
        );
        carried["partProfile"]["parts"][0]["bone"] = json!("wing");
        assert!(ensure_profile(&mut carried));
        assert_eq!(
            carried["partProfile"]["source"], "none",
            "an invalid one is replaced"
        );
    }

    #[test]
    fn a_model_s_looser_answers_are_read() {
        let part = AiPart::read(&json!({
            "name": "charm", "box": [400, 200, 450, 300], "anchor": [420, 200],
            "bone": "left thigh", "motion": "swing", "material": "Lace", "stiffness": 4, "weight": 70,
        }))
        .unwrap();
        assert_eq!(
            part.bounds,
            [0.4, 0.2, 0.45, 0.3],
            "0–1000 coordinates are rescaled"
        );
        assert_eq!(part.bone, "thighL");
        assert_eq!(part.motion, "dangle");
        assert_eq!(part.material, "lace");
        assert!((part.stiffness - 0.4).abs() < 1e-6 && (part.weight - 0.7).abs() < 1e-6);
        assert_eq!(bone_named("earring"), "head");
        assert_eq!(bone_named("pelvis"), "hips");
        assert_eq!(bone_named("shoulder"), "chest");
        assert!(AiPart::read(&json!({ "name": "x", "box": [0, 0, 2000, 1], "anchor": [0, 0],
            "bone": "chest", "motion": "sway", "material": "silk", "stiffness": 0.5, "weight": 0.5 })).is_none());
    }
}
