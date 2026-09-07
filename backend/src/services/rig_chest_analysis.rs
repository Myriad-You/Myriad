//! One-shot visual estimation for Anime2.5D chest secondary motion.
//!
//! AI selects a bounded garment-aware deformation region and support profile
//! once during import preview. The result is persisted in playback JSON;
//! rendering never calls AI.

use myriad_agent_rules::extract_json_object_from_ai_response;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::ModelTier,
    services::{
        ai_config::{get_ai_config_for_tier, AiConfig},
        ai_cost_ledger::record_ai_call_from_attribution,
        analyzer::{openai_chat_completions_url, AiProvider},
        gemini_media,
        http_client::get_long_running_client,
        image_generation::ImageReference,
    },
};

const PROFILE_VERSION: u8 = 2;
const PROMPT_VERSION: &str = "anime25d-chest-dynamics-v4";
const MIN_AI_CONFIDENCE: f32 = 0.25;
const FULL_AI_CONFIDENCE: f32 = 0.58;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MIN_AI_MOTION_SCALE: f32 = 0.22;
const MAX_AI_MOTION_SCALE: f32 = 1.14;
const AI_MOTION_RAMP_START: f32 = 0.1;
const AI_MOTION_RAMP_END: f32 = 0.85;

#[derive(Clone, Copy, Debug)]
struct PlaybackContext {
    width: f32,
    height: f32,
    face_x0: f32,
    face_x1: f32,
    face_y0: f32,
    face_y1: f32,
    neck_x: f32,
    neck_bottom: f32,
    topwear: Option<Rect>,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiChestEstimate {
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
    visible_scale: f32,
    support_scale: f32,
    garment_motion_scale: f32,
    confidence: f32,
}

/// Apply the authoritative visual-profile policy without consulting AI.
pub fn apply_male_policy(playback: &mut Value) -> bool {
    let Some(context) = playback_context(playback) else {
        return false;
    };
    let mut profile = fallback_profile(context);
    profile["enabled"] = json!(false);
    profile["source"] = json!("gender-policy");
    profile["motionScale"] = json!(0.0);
    profile["visibleScale"] = json!(0.0);
    profile["supportScale"] = json!(1.0);
    profile["garmentMotionScale"] = json!(0.0);
    profile["confidence"] = json!(1.0);
    insert_profile(playback, profile)
}

/// Keep a valid AI preview result, otherwise restore deterministic upper-torso geometry.
pub fn ensure_safe_enabled_profile(playback: &mut Value) -> bool {
    let Some(context) = playback_context(playback) else {
        return false;
    };
    if playback
        .get("chestProfile")
        .is_some_and(|profile| reusable_ai_profile(profile, context))
    {
        return true;
    }
    insert_profile(playback, fallback_profile(context))
}

/// Call one configured vision model at most once and persist a safe result.
pub async fn analyze_once_or_fallback(playback: &mut Value, reference: &ImageReference) -> bool {
    let Some(context) = playback_context(playback) else {
        return false;
    };
    if playback
        .get("chestProfile")
        .is_some_and(|profile| reusable_ai_profile(profile, context))
    {
        return true;
    }
    let fallback = fallback_profile(context);
    let Ok(config) = get_ai_config_for_tier(ModelTier::Standard).await else {
        tracing::warn!("chest vision analysis skipped because no Standard AI model is configured");
        return insert_profile(playback, fallback);
    };
    let prompt = analysis_prompt(context);
    let response = request_estimate(&config, reference, &prompt).await;
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
        .and_then(|value| serde_json::from_value::<AiChestEstimate>(value).ok())
        .and_then(|estimate| profile_from_estimate(context, estimate))
        .unwrap_or_else(|| {
            if let Err(error) = &response {
                tracing::warn!(%error, "chest vision analysis failed; using geometry fallback");
            } else {
                tracing::warn!("chest vision response was unusable; using geometry fallback");
            }
            fallback
        });
    insert_profile(playback, profile)
}

fn playback_context(playback: &Value) -> Option<PlaybackContext> {
    let finite = |pointer: &str| {
        playback
            .pointer(pointer)
            .and_then(Value::as_f64)
            .map(|value| value as f32)
            .filter(|value| value.is_finite())
    };
    let width = finite("/pixelCanvas/width")?;
    let height = finite("/pixelCanvas/height")?;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let layers = playback.get("layers")?.as_array()?;
    let topwear = layers
        .iter()
        .find(|layer| layer.get("role").and_then(Value::as_str) == Some("topwear"))
        .and_then(|layer| {
            let number = |name: &str| {
                layer
                    .get(name)
                    .and_then(Value::as_f64)
                    .map(|value| value as f32)
                    .filter(|value| value.is_finite())
            };
            let rect = Rect {
                x: number("x")?,
                y: number("y")?,
                width: number("w")?,
                height: number("h")?,
            };
            (rect.width > 0.0 && rect.height > 0.0).then_some(rect)
        });
    Some(PlaybackContext {
        width,
        height,
        face_x0: finite("/anchors/face/x0")?,
        face_x1: finite("/anchors/face/x1")?,
        face_y0: finite("/anchors/face/y0")?,
        face_y1: finite("/anchors/face/y1")?,
        neck_x: finite("/anchors/neckPivot/x")?,
        neck_bottom: finite("/anchors/neckBottom")?,
        topwear,
    })
}

fn fallback_profile(context: PlaybackContext) -> Value {
    let (center_x, center_y, radius_x, radius_y) = fallback_geometry(context);
    json!({
        "version": PROFILE_VERSION,
        "enabled": true,
        "source": "geometry-fallback",
        "centerX": center_x,
        "centerY": center_y,
        "radiusX": radius_x,
        "radiusY": radius_y,
        "visibleScale": 0.5,
        "motionScale": 1.0,
        "frequencyScale": 1.0,
        "supportScale": 0.45,
        "garmentMotionScale": 0.65,
        "confidence": 0.0,
    })
}

fn profile_from_estimate(context: PlaybackContext, estimate: AiChestEstimate) -> Option<Value> {
    let values = [
        estimate.center_x,
        estimate.center_y,
        estimate.radius_x,
        estimate.radius_y,
        estimate.visible_scale,
        estimate.support_scale,
        estimate.garment_motion_scale,
        estimate.confidence,
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        || estimate.radius_x <= 0.0
        || estimate.radius_y <= 0.0
        || estimate.confidence < MIN_AI_CONFIDENCE
    {
        return None;
    }
    let (fallback_x, fallback_y, fallback_rx, fallback_ry) = fallback_geometry(context);
    let trust_progress = ((estimate.confidence - MIN_AI_CONFIDENCE)
        / (FULL_AI_CONFIDENCE - MIN_AI_CONFIDENCE))
        .clamp(0.0, 1.0);
    let trust_eased = trust_progress * trust_progress * (3.0 - 2.0 * trust_progress);
    let spatial_weight = 0.35 + trust_eased * 0.65;
    let visible_scale = lerp(0.5, estimate.visible_scale, spatial_weight).clamp(0.0, 1.0);
    let mut center_x = estimate.center_x * context.width;
    let mut center_y = estimate.center_y * context.height;
    if let Some(topwear) = context.topwear {
        center_x = center_x.clamp(
            topwear.x + topwear.width * 0.15,
            topwear.x + topwear.width * 0.85,
        );
        let (minimum_y, maximum_y) = chest_vertical_bounds(context, topwear);
        center_y = center_y.clamp(minimum_y, maximum_y);
    }
    center_x = lerp(fallback_x, center_x, spatial_weight);
    center_y = lerp(fallback_y, center_y, spatial_weight);
    center_y = center_y.max(minimum_ai_center_y(context, visible_scale));
    let face_width = (context.face_x1 - context.face_x0).abs().max(1.0);
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    let maximum_radius_x = (face_width * 0.82).min(context.width * 0.32);
    let maximum_radius_y = (face_height * 0.42).min(context.height * 0.22);
    let minimum_radius_x = minimum_ai_radius_x(context, visible_scale).min(maximum_radius_x);
    let minimum_radius_y = minimum_ai_radius_y(context, visible_scale).min(maximum_radius_y);
    let radius_x = (estimate.radius_x * context.width).clamp(minimum_radius_x, maximum_radius_x);
    let radius_y = (estimate.radius_y * context.height).clamp(minimum_radius_y, maximum_radius_y);
    let radius_x = lerp(fallback_rx, radius_x, spatial_weight).max(minimum_radius_x);
    let radius_y = lerp(fallback_ry, radius_y, spatial_weight).max(minimum_radius_y);
    let support_scale = lerp(0.45, estimate.support_scale, spatial_weight).clamp(0.0, 1.0);
    let garment_motion_scale =
        lerp(0.65, estimate.garment_motion_scale, spatial_weight).clamp(0.0, 1.0);
    let motion_scale = motion_scale_from_visible(visible_scale);
    Some(json!({
        "version": PROFILE_VERSION,
        "enabled": true,
        "source": "ai-vision",
        "centerX": center_x,
        "centerY": center_y,
        "radiusX": radius_x,
        "radiusY": radius_y,
        "visibleScale": visible_scale,
        // AI supplies apparent size, not displacement. A smooth conservative
        // ramp prevents small profiles from inheriting near-full motion.
        "motionScale": motion_scale,
        "frequencyScale": 1.10 - visible_scale * 0.20,
        "supportScale": support_scale,
        "garmentMotionScale": garment_motion_scale,
        "confidence": estimate.confidence,
    }))
}

fn fallback_geometry(context: PlaybackContext) -> (f32, f32, f32, f32) {
    let face_width = (context.face_x1 - context.face_x0).abs().max(1.0);
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    let mut center_x = context.neck_x.clamp(0.0, context.width);
    let mut center_y = (context.neck_bottom + face_height * 0.5).clamp(0.0, context.height);
    if let Some(topwear) = context.topwear {
        center_x = center_x.clamp(
            topwear.x + topwear.width * 0.15,
            topwear.x + topwear.width * 0.85,
        );
        let (minimum_y, maximum_y) = chest_vertical_bounds(context, topwear);
        center_y = center_y.clamp(minimum_y, maximum_y);
    }
    (
        center_x,
        center_y,
        (face_width * 0.6).clamp(1.0, context.width * 0.5),
        (face_height * 0.32).clamp(1.0, context.height * 0.22),
    )
}

fn chest_vertical_bounds(context: PlaybackContext, topwear: Rect) -> (f32, f32) {
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    let minimum = (topwear.y + topwear.height * 0.20)
        .max(context.neck_bottom + face_height * 0.12)
        .clamp(0.0, context.height);
    let maximum = (topwear.y + topwear.height * 0.62)
        .min(context.neck_bottom + face_height * 0.78)
        .clamp(minimum, context.height);
    (minimum, maximum)
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn minimum_ai_center_y(context: PlaybackContext, visible_scale: f32) -> f32 {
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    let minimum = (context.neck_bottom + face_height * (0.44 + 0.20 * visible_scale))
        .clamp(0.0, context.height);
    context.topwear.map_or(minimum, |topwear| {
        let (_, maximum) = chest_vertical_bounds(context, topwear);
        minimum.min(maximum)
    })
}

fn minimum_ai_radius_x(context: PlaybackContext, visible_scale: f32) -> f32 {
    let face_width = (context.face_x1 - context.face_x0).abs().max(1.0);
    face_width * (0.48 + 0.25 * visible_scale)
}

fn minimum_ai_radius_y(context: PlaybackContext, visible_scale: f32) -> f32 {
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    face_height * (0.26 + 0.14 * visible_scale)
}

fn motion_scale_from_visible(visible_scale: f32) -> f32 {
    let progress = ((visible_scale - AI_MOTION_RAMP_START)
        / (AI_MOTION_RAMP_END - AI_MOTION_RAMP_START))
        .clamp(0.0, 1.0);
    let eased = progress * progress * (3.0 - 2.0 * progress);
    MIN_AI_MOTION_SCALE + (MAX_AI_MOTION_SCALE - MIN_AI_MOTION_SCALE) * eased
}

fn valid_profile(profile: &Value, context: PlaybackContext, require_enabled: bool) -> bool {
    let number = |name: &str| {
        profile
            .get(name)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
    };
    let source = profile.get("source").and_then(Value::as_str);
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    let (minimum_center_y, maximum_center_y) =
        context.topwear.map_or((0.0, context.height), |topwear| {
            chest_vertical_bounds(context, topwear)
        });
    let maximum_radius_y = (face_height * 0.42).min(context.height * 0.22);
    let ai_spatial_region_is_valid = || {
        let Some(visible_scale) = number("visibleScale").map(|value| value as f32) else {
            return false;
        };
        source != Some("ai-vision")
            || (number("centerY").is_some_and(|value| {
                value + 0.5 >= f64::from(minimum_ai_center_y(context, visible_scale))
            }) && number("radiusX").is_some_and(|value| {
                value + 0.5 >= f64::from(minimum_ai_radius_x(context, visible_scale))
            }) && number("radiusY").is_some_and(|value| {
                value + 0.5 >= f64::from(minimum_ai_radius_y(context, visible_scale))
            }))
    };
    profile.get("version").and_then(Value::as_u64) == Some(u64::from(PROFILE_VERSION))
        && profile.get("enabled").and_then(Value::as_bool) == Some(require_enabled)
        && matches!(source, Some("ai-vision" | "geometry-fallback"))
        && number("centerX").is_some_and(|value| (0.0..=f64::from(context.width)).contains(&value))
        && number("centerY").is_some_and(|value| {
            (f64::from(minimum_center_y)..=f64::from(maximum_center_y)).contains(&value)
        })
        && number("radiusX")
            .is_some_and(|value| (1.0..=f64::from(context.width * 0.5)).contains(&value))
        && number("radiusY")
            .is_some_and(|value| (1.0..=f64::from(maximum_radius_y)).contains(&value))
        && number("visibleScale").is_some_and(|value| (0.0..=1.0).contains(&value))
        && number("motionScale").is_some_and(|value| (0.0..=1.25).contains(&value))
        && number("frequencyScale").is_some_and(|value| (0.75..=1.25).contains(&value))
        && number("supportScale").is_some_and(|value| (0.0..=1.0).contains(&value))
        && number("garmentMotionScale").is_some_and(|value| (0.0..=1.0).contains(&value))
        && number("confidence").is_some_and(|value| (0.0..=1.0).contains(&value))
        && ai_spatial_region_is_valid()
}

fn reusable_ai_profile(profile: &Value, context: PlaybackContext) -> bool {
    valid_profile(profile, context, true)
        && profile.get("source").and_then(Value::as_str) == Some("ai-vision")
}

fn insert_profile(playback: &mut Value, profile: Value) -> bool {
    let Some(object) = playback.as_object_mut() else {
        return false;
    };
    object.insert("chestProfile".to_string(), profile);
    true
}

fn analysis_prompt(context: PlaybackContext) -> String {
    let topwear = context.topwear.map_or_else(
        || "unknown".to_string(),
        |rect| {
            format!(
                "x={:.4}, y={:.4}, width={:.4}, height={:.4}",
                rect.x / context.width,
                rect.y / context.height,
                rect.width / context.width,
                rect.height / context.height,
            )
        },
    );
    format!(
        r#"Task version: {PROMPT_VERSION}
Analyze this front-facing anime character image only for subtle 2D upper-torso secondary motion. Do not classify gender and do not return anatomical labels. Judge the visible construction instead of assuming behavior from a garment category name.

All coordinates must be normalized to the full image: x from left to right, y from top to bottom. Return the center and radii of one conservative ellipse enclosing the left and right upper-torso volumes together. It is a shared pair envelope, not an ellipse around one side, the cleavage, exposed sternum, neckline, or a central ornament. Put its center at the shared volume centroid; radiusX must span the main curved surface on both sides, and radiusY must cover the main vertical volume rather than only its upper edge. Its vertical center must never sit on the collar, necktie, under-bust seam, belt, waistline, or abdomen. Keep the envelope inside the torso, but do not collapse it to a small central patch merely because seams, bands, ornaments, rigid cups, armor, or loose folds overlap the volume; express those restraints through `supportScale` and `garmentMotionScale`. Never include shoulders, arms, sleeves, cape, or hanging accessories.

`visibleScale` is the apparent underlying volume from 0 (flat/minimal) to 1 (very prominent); it is not a cup size. Estimate it from the paired silhouette, curvature, occupied torso width, and projected volume. Do not lower `visibleScale` merely because clothing is tight, structured, layered, armored, or highly supportive: record those restraints only in `supportScale` and `garmentMotionScale`, otherwise the same garment would suppress motion twice. `supportScale` is the visible mechanical restraint from 0 (little restraint and more delayed motion) to 1 (structured, compressed, or effectively locked to the torso). `garmentMotionScale` is how much localized soft-tissue response should remain visible on the outer topwear: use higher values for soft close-fitting material and lower values for loose draped layers, thick structured panels, rigid armor, or heavy occlusion. A close-fitting garment may have both high support and high surface transmission; a rigid garment may have high support but low surface transmission. `confidence` measures whether silhouette, clothing structure, and pose allow a reliable estimate.

Known rig hints: body center x={:.4}; neck bottom y={:.4}; face box x0={:.4}, y0={:.4}, x1={:.4}, y1={:.4}; topwear box {topwear}.

Return exactly one JSON object and no Markdown:
{{"centerX":0.0,"centerY":0.0,"radiusX":0.0,"radiusY":0.0,"visibleScale":0.0,"supportScale":0.0,"garmentMotionScale":0.0,"confidence":0.0}}"#,
        context.neck_x / context.width,
        context.neck_bottom / context.height,
        context.face_x0 / context.width,
        context.face_y0 / context.height,
        context.face_x1 / context.width,
        context.face_y1 / context.height,
    )
}

async fn request_estimate(
    config: &AiConfig,
    reference: &ImageReference,
    prompt: &str,
) -> Result<String, String> {
    let client = get_long_running_client().await;
    let encoded = BASE64.encode(&reference.bytes);
    let (url, request) = match config.provider {
        AiProvider::Gemini => (
            gemini_media::generate_content_url(
                config.base_url.as_deref().unwrap_or_default(),
                &config.model,
            ),
            client
                .post(gemini_media::generate_content_url(
                    config.base_url.as_deref().unwrap_or_default(),
                    &config.model,
                ))
                .header("x-goog-api-key", &config.api_key)
                .json(&json!({
                    "contents": [{ "parts": [
                        { "inlineData": { "mimeType": reference.media_type, "data": encoded } },
                        { "text": prompt }
                    ]}],
                    "generationConfig": { "responseMimeType": "application/json" }
                })),
        ),
        AiProvider::OpenAI => {
            let url = openai_chat_completions_url(config.base_url.as_deref());
            let data_url = format!("data:{};base64,{encoded}", reference.media_type);
            (
                url.clone(),
                client.post(url).bearer_auth(&config.api_key).json(&json!({
                    "model": config.model,
                    "messages": [{
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": data_url } }
                        ]
                    }],
                    "response_format": { "type": "json_object" }
                })),
            )
        }
    };
    let response = tokio::time::timeout(REQUEST_TIMEOUT, request.send())
        .await
        .map_err(|_| "vision request timed out".to_string())?
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("vision response exceeds size limit".to_string());
    }
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&bytes)
            .chars()
            .take(400)
            .collect::<String>();
        return Err(format!(
            "vision provider returned HTTP {status} from {url}: {preview}"
        ));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    match config.provider {
        AiProvider::Gemini => value
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .and_then(|parts| {
                parts
                    .iter()
                    .find_map(|part| part.get("text").and_then(Value::as_str))
            })
            .map(str::to_string)
            .ok_or_else(|| "Gemini vision response contained no text".to_string()),
        AiProvider::OpenAI => openai_text(&value)
            .map(str::to_string)
            .ok_or_else(|| "OpenAI-compatible vision response contained no text".to_string()),
    }
}

fn openai_text(value: &Value) -> Option<&str> {
    let content = value.pointer("/choices/0/message/content")?;
    if let Some(text) = content.as_str() {
        return Some(text);
    }
    content.as_array()?.iter().find_map(|part| {
        part.get("text")
            .and_then(Value::as_str)
            .or_else(|| part.pointer("/text/value").and_then(Value::as_str))
    })
}

fn parse_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    serde_json::from_str(&extract_json_object_from_ai_response(trimmed)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playback() -> Value {
        json!({
            "pixelCanvas": { "width": 1041, "height": 1388 },
            "anchors": {
                "face": { "x0": 340, "x1": 703, "y0": 170, "y1": 658 },
                "faceScale": 1.09009,
                "neckPivot": { "x": 525.84, "y": 723.45 },
                "neckBottom": 750
            },
            "layers": [{
                "role": "topwear", "x": 234, "y": 637, "w": 613, "h": 697
            }]
        })
    }

    #[test]
    fn fallback_targets_the_upper_torso_instead_of_the_waist() {
        let context = playback_context(&playback()).expect("context");
        let profile = fallback_profile(context);
        assert!((profile["centerY"].as_f64().unwrap() - 994.0).abs() < 0.001);
        assert!((profile["radiusY"].as_f64().unwrap() - 156.16).abs() < 0.001);
        assert!((profile["radiusX"].as_f64().unwrap() - 217.8).abs() < 0.001);
        assert_eq!(profile["motionScale"], 1.0);
        assert_eq!(profile["supportScale"], 0.45);
        assert_eq!(profile["garmentMotionScale"], 0.65);
    }

    #[test]
    fn male_policy_disables_secondary_motion() {
        let mut value = playback();
        assert!(apply_male_policy(&mut value));
        assert_eq!(value["chestProfile"]["enabled"], false);
        assert_eq!(value["chestProfile"]["motionScale"], 0.0);
        assert_eq!(value["chestProfile"]["supportScale"], 1.0);
        assert_eq!(value["chestProfile"]["garmentMotionScale"], 0.0);
        assert_eq!(value["chestProfile"]["source"], "gender-policy");
    }

    #[test]
    fn ai_size_and_garment_map_to_separate_bounded_controls() {
        let context = playback_context(&playback()).expect("context");
        let profile = profile_from_estimate(
            context,
            AiChestEstimate {
                center_x: 0.51,
                center_y: 0.72,
                radius_x: 0.18,
                radius_y: 0.13,
                visible_scale: 0.8,
                support_scale: 0.25,
                garment_motion_scale: 0.9,
                confidence: 0.9,
            },
        )
        .expect("profile");
        assert_eq!(profile["source"], "ai-vision");
        assert!(profile["motionScale"].as_f64().unwrap() > 1.1);
        assert!(profile["motionScale"].as_f64().unwrap() <= 1.14);
        assert!(profile["frequencyScale"].as_f64().unwrap() >= 0.9);
        assert_eq!(profile["supportScale"], 0.25);
        assert!((profile["garmentMotionScale"].as_f64().unwrap() - 0.9).abs() < 1e-6);
        assert!(valid_profile(&profile, context, true));
    }

    #[test]
    fn incomplete_profile_without_garment_dynamics_is_not_reused() {
        let context = playback_context(&playback()).expect("context");
        let mut profile = fallback_profile(context);
        profile["version"] = json!(1);
        profile.as_object_mut().unwrap().remove("supportScale");
        profile
            .as_object_mut()
            .unwrap()
            .remove("garmentMotionScale");
        assert!(!valid_profile(&profile, context, true));
    }

    #[test]
    fn small_ai_size_gets_conservative_continuous_motion() {
        let small = motion_scale_from_visible(0.35);
        let medium = motion_scale_from_visible(0.5);
        let large = motion_scale_from_visible(0.8);
        assert!((small - 0.4585).abs() < 0.001);
        assert!(small < medium);
        assert!(medium < large);
        assert_eq!(motion_scale_from_visible(0.0), MIN_AI_MOTION_SCALE);
        assert_eq!(motion_scale_from_visible(1.0), MAX_AI_MOTION_SCALE);
    }

    #[test]
    fn marginal_confidence_ai_result_is_blended_with_safe_geometry() {
        let context = playback_context(&playback()).expect("context");
        let profile = profile_from_estimate(
            context,
            AiChestEstimate {
                center_x: 0.5,
                center_y: 0.9,
                radius_x: 0.2,
                radius_y: 0.1,
                visible_scale: 0.5,
                support_scale: 0.5,
                garment_motion_scale: 0.6,
                confidence: 0.3,
            },
        )
        .expect("blended profile");
        assert_eq!(profile["source"], "ai-vision");
        assert!(profile["centerY"].as_f64().unwrap() < 1_070.0);
        assert!(profile["centerY"].as_f64().unwrap() > 994.0);
    }

    #[test]
    fn unusably_low_confidence_ai_result_is_rejected() {
        let context = playback_context(&playback()).expect("context");
        assert!(profile_from_estimate(
            context,
            AiChestEstimate {
                center_x: 0.5,
                center_y: 0.7,
                radius_x: 0.2,
                radius_y: 0.1,
                visible_scale: 0.5,
                support_scale: 0.5,
                garment_motion_scale: 0.6,
                confidence: 0.2,
            },
        )
        .is_none());
    }
}
