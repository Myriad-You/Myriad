//! One-shot visual estimation for Anime2.5D chest secondary motion.
//!
//! AI selects a bounded deformation region once during import preview. The
//! resulting profile is persisted in playback JSON; rendering never calls AI.

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

const PROFILE_VERSION: u8 = 1;
const PROMPT_VERSION: &str = "anime25d-chest-region-v1";
const MIN_AI_CONFIDENCE: f32 = 0.58;
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
    face_scale: f32,
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
    profile["confidence"] = json!(1.0);
    insert_profile(playback, profile)
}

/// Keep a valid preview result, otherwise restore deterministic legacy geometry.
pub fn ensure_safe_enabled_profile(playback: &mut Value) -> bool {
    let Some(context) = playback_context(playback) else {
        return false;
    };
    if playback
        .get("chestProfile")
        .is_some_and(|profile| valid_profile(profile, context, true))
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
        .is_some_and(|profile| valid_profile(profile, context, true))
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
        face_scale: finite("/anchors/faceScale")?,
        neck_x: finite("/anchors/neckPivot/x")?,
        neck_bottom: finite("/anchors/neckBottom")?,
        topwear,
    })
}

fn fallback_profile(context: PlaybackContext) -> Value {
    let face_width = (context.face_x1 - context.face_x0).abs().max(1.0);
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    json!({
        "version": PROFILE_VERSION,
        "enabled": true,
        "source": "geometry-fallback",
        "centerX": context.neck_x.clamp(0.0, context.width),
        // Includes the legacy default bustY=1 offset so profile-aware playback
        // preserves the current neutral placement exactly.
        "centerY": (context.neck_bottom + face_height * 0.6 + 70.0 * context.face_scale)
            .clamp(0.0, context.height),
        "radiusX": (face_width * 0.6).clamp(1.0, context.width * 0.5),
        "radiusY": (face_height * 0.45).clamp(1.0, context.height * 0.5),
        "visibleScale": 0.5,
        "motionScale": 1.0,
        "frequencyScale": 1.0,
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
    let face_width = (context.face_x1 - context.face_x0).abs().max(1.0);
    let face_height = (context.face_y1 - context.face_y0).abs().max(1.0);
    let mut center_x = estimate.center_x * context.width;
    let mut center_y = estimate.center_y * context.height;
    if let Some(topwear) = context.topwear {
        center_x = center_x.clamp(
            topwear.x + topwear.width * 0.15,
            topwear.x + topwear.width * 0.85,
        );
        center_y = center_y.clamp(
            topwear.y + topwear.height * 0.12,
            topwear.y + topwear.height * 0.78,
        );
    }
    let radius_x = (estimate.radius_x * context.width).clamp(
        face_width * 0.30,
        (face_width * 0.82).min(context.width * 0.32),
    );
    let radius_y = (estimate.radius_y * context.height).clamp(
        face_height * 0.16,
        (face_height * 0.58).min(context.height * 0.28),
    );
    let visible_scale = estimate.visible_scale.clamp(0.0, 1.0);
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
        "confidence": estimate.confidence,
    }))
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
    profile.get("version").and_then(Value::as_u64) == Some(u64::from(PROFILE_VERSION))
        && profile.get("enabled").and_then(Value::as_bool) == Some(require_enabled)
        && matches!(source, Some("ai-vision" | "geometry-fallback"))
        && number("centerX").is_some_and(|value| (0.0..=f64::from(context.width)).contains(&value))
        && number("centerY").is_some_and(|value| (0.0..=f64::from(context.height)).contains(&value))
        && number("radiusX")
            .is_some_and(|value| (1.0..=f64::from(context.width * 0.5)).contains(&value))
        && number("radiusY")
            .is_some_and(|value| (1.0..=f64::from(context.height * 0.5)).contains(&value))
        && number("visibleScale").is_some_and(|value| (0.0..=1.0).contains(&value))
        && number("motionScale").is_some_and(|value| (0.0..=1.25).contains(&value))
        && number("frequencyScale").is_some_and(|value| (0.75..=1.25).contains(&value))
        && number("confidence").is_some_and(|value| (0.0..=1.0).contains(&value))
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
Analyze this front-facing anime character image only for 2D rig deformation geometry. Locate the visible upper-torso soft-tissue region that could receive subtle inertial secondary motion. Ignore shoulders, arms, sleeves, cape, collar, medals, armor plates, and hanging ornaments. Do not classify gender and do not return anatomical labels.

All coordinates must be normalized to the full image: x from left to right, y from top to bottom. Return the center and radii of one conservative ellipse. `visibleScale` is a visual size estimate from 0 (flat/minimal) to 1 (very prominent); it is not a cup size. `confidence` measures whether clothing and pose allow a reliable estimate.

Known rig hints: body center x={:.4}; neck bottom y={:.4}; face box x0={:.4}, y0={:.4}, x1={:.4}, y1={:.4}; topwear box {topwear}.

Return exactly one JSON object and no Markdown:
{{"centerX":0.0,"centerY":0.0,"radiusX":0.0,"radiusY":0.0,"visibleScale":0.0,"confidence":0.0}}"#,
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
                    }]
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
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str(&trimmed[start..=end]).ok()
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
    fn fallback_preserves_legacy_neutral_region() {
        let context = playback_context(&playback()).expect("context");
        let profile = fallback_profile(context);
        assert!((profile["centerY"].as_f64().unwrap() - 1119.1063).abs() < 0.001);
        assert!((profile["radiusX"].as_f64().unwrap() - 217.8).abs() < 0.001);
        assert_eq!(profile["motionScale"], 1.0);
    }

    #[test]
    fn male_policy_disables_secondary_motion() {
        let mut value = playback();
        assert!(apply_male_policy(&mut value));
        assert_eq!(value["chestProfile"]["enabled"], false);
        assert_eq!(value["chestProfile"]["motionScale"], 0.0);
        assert_eq!(value["chestProfile"]["source"], "gender-policy");
    }

    #[test]
    fn ai_size_maps_only_to_bounded_motion_and_frequency() {
        let context = playback_context(&playback()).expect("context");
        let profile = profile_from_estimate(
            context,
            AiChestEstimate {
                center_x: 0.51,
                center_y: 0.72,
                radius_x: 0.18,
                radius_y: 0.13,
                visible_scale: 0.8,
                confidence: 0.9,
            },
        )
        .expect("profile");
        assert_eq!(profile["source"], "ai-vision");
        assert!(profile["motionScale"].as_f64().unwrap() > 1.1);
        assert!(profile["motionScale"].as_f64().unwrap() <= 1.14);
        assert!(profile["frequencyScale"].as_f64().unwrap() >= 0.9);
        assert!(valid_profile(&profile, context, true));
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
    fn low_confidence_ai_result_is_rejected() {
        let context = playback_context(&playback()).expect("context");
        assert!(profile_from_estimate(
            context,
            AiChestEstimate {
                center_x: 0.5,
                center_y: 0.7,
                radius_x: 0.2,
                radius_y: 0.1,
                visible_scale: 0.5,
                confidence: 0.3,
            },
        )
        .is_none());
    }
}
