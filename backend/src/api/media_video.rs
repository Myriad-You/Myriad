//! Admin-only image-to-video jobs. The backend submits, polls and downloads with the site's
//! OpenRouter credential; nothing is stored and the credential never reaches a response.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_must_match_the_provider_format() {
        assert!(valid_job_id("gen-vid-1786552600-abcdefghij0123456789"));
        assert!(!valid_job_id("gen-vid-1786552600-abcdefghij012345678"));
        assert!(!valid_job_id("gen-vid--abcdefghij0123456789"));
        assert!(!valid_job_id("gen-vid-1/../x-abcdefghij0123456789"));
        assert!(!valid_job_id("gen-img-1786552600-abcdefghij0123456789"));
    }

    #[test]
    fn requests_are_bounded_before_any_provider_call() {
        let frame = "data:image/jpeg;base64,/9j/4AAQ".to_string();
        let ok = |job: &VideoJob| validate_job(job).is_ok();
        let job = |prompt: &str, size: &str, duration: u32, first: &str| VideoJob {
            prompt: prompt.to_string(),
            size: size.to_string(),
            duration,
            first_frame: first.to_string(),
            last_frame: None,
            model: None,
        };
        assert!(ok(&job("turn", "1280x720", 4, &frame)));
        assert!(!ok(&job(" ", "1280x720", 4, &frame)));
        assert!(!ok(&job("turn", "1280x720", 3, &frame)));
        assert!(!ok(&job("turn", "1280x720", 16, &frame)));
        assert!(!ok(&job("turn", "4096x4096", 4, &frame)));
        assert!(!ok(&job("turn", "1280", 4, &frame)));
        assert!(!ok(&job(
            "turn",
            "1280x720",
            4,
            "https://example.com/a.png"
        )));
        let mut other = job("turn", "1280x720", 4, &frame);
        other.model = Some("someone/else".to_string());
        assert!(!ok(&other));
    }

    #[test]
    fn provider_errors_never_carry_the_credential() {
        let body = r#"{"error":{"message":"bad key sk-or-secret used"}}"#;
        let shown = provider_message(body, "sk-or-secret");
        assert!(!shown.contains("sk-or-secret"));
        assert!(shown.contains("bad key"));
        assert_eq!(provider_message("<html>", "k"), "");
    }
}

use crate::services::{http_client::get_long_running_client, image_generation};
use crate::{GLOBAL_DYNAMIC_CONFIG, error::HttpError, extract::AdminClaims};
use axum::{
    Json,
    extract::Path,
    http::header,
    response::{IntoResponse, Response},
};
use myriad_error::AppError;
use serde::Deserialize;
use serde_json::{Value, json};

static VIDEO_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const DEFAULT_MODEL: &str = "bytedance/seedance-2.0";
const MAX_VIDEO_BYTES: usize = 256 * 1024 * 1024;

#[derive(Deserialize)]
pub struct VideoJob {
    prompt: String,
    /// "WIDTHxHEIGHT"
    size: String,
    duration: u32,
    /// `data:image/...;base64,` URL
    first_frame: String,
    #[serde(default)]
    last_frame: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

fn valid_job_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("gen-vid-") else {
        return false;
    };
    let Some((stamp, tail)) = rest.split_once('-') else {
        return false;
    };
    !stamp.is_empty()
        && stamp.bytes().all(|b| b.is_ascii_digit())
        && tail.len() == 20
        && tail.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_frame(url: &str) -> bool {
    [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/webp;base64,",
    ]
    .iter()
    .any(|prefix| url.starts_with(prefix))
}

fn validate_job(job: &VideoJob) -> Result<(), HttpError> {
    let bad = |message: &str| Err(HttpError(AppError::bad_request(message.to_string())));
    if job.prompt.trim().is_empty() || job.prompt.chars().count() > 2000 {
        return bad("Use a prompt of 1–2000 characters");
    }
    if !(4..=15).contains(&job.duration) {
        return bad("Duration must be 4–15 seconds");
    }
    let dims = job
        .size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)));
    if !matches!(dims, Some((w, h)) if (256..=1920).contains(&w) && (256..=1920).contains(&h)) {
        return bad("Size must be WIDTHxHEIGHT with each side between 256 and 1920");
    }
    if !is_frame(&job.first_frame) || job.last_frame.as_deref().is_some_and(|f| !is_frame(f)) {
        return bad("Frames must be inline PNG, JPEG or WebP images");
    }
    if job
        .model
        .as_deref()
        .is_some_and(|m| !m.starts_with("bytedance/seedance-"))
    {
        return bad("Unsupported video model");
    }
    Ok(())
}

/// The provider's own error text, with the credential scrubbed in case it is ever echoed.
fn provider_message(body: &str, api_key: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            let error = value.get("error")?;
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let message = if api_key.is_empty() {
        message
    } else {
        message.replace(api_key, "[redacted]")
    };
    message.chars().take(300).collect()
}

/// (api key, base url) of the site's OpenRouter credential.
async fn openrouter() -> Result<(String, String), HttpError> {
    let dynamic = GLOBAL_DYNAMIC_CONFIG.read().await;
    let from_image = image_generation::config_from_dynamic(&dynamic)
        .ok()
        .filter(|config| config.provider == "openrouter" && !config.api_key.trim().is_empty())
        .map(|config| (config.api_key, config.base_url));
    from_image
        .or_else(|| {
            dynamic
                .shared_openrouter_api_key()
                .map(|key| (key, "https://openrouter.ai/api/v1".to_string()))
        })
        .ok_or_else(|| HttpError(AppError::bad_request("Video generation is not configured")))
}

async fn provider_json(
    request: reqwest::RequestBuilder,
    api_key: &str,
    what: &'static str,
) -> Result<Value, HttpError> {
    let response = request
        .bearer_auth(api_key)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(error = %error.without_url(), what, "video provider unreachable");
            HttpError(AppError::bad_gateway("Video provider is unreachable"))
        })?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let message = provider_message(&body, api_key);
        tracing::warn!(%status, what, %message, "video provider refused the request");
        return Err(HttpError(AppError::bad_gateway(format!(
            "Video provider refused the request ({status}) {message}"
        ))));
    }
    serde_json::from_str(&body).map_err(|_| {
        HttpError(AppError::bad_gateway(
            "Video provider sent an invalid reply",
        ))
    })
}

fn job_view(value: &Value) -> Value {
    json!({
        "id": value.get("id"),
        "status": value.get("status"),
        "error": value.get("error"),
        "usage": value.get("usage"),
    })
}

pub async fn create_job(
    _admin: AdminClaims,
    Json(input): Json<VideoJob>,
) -> Result<Json<Value>, HttpError> {
    validate_job(&input)?;
    let _permit = VIDEO_SLOTS.try_acquire().map_err(|_| {
        HttpError(AppError::service_unavailable(
            "Video generation is busy; please try again",
        ))
    })?;
    let (api_key, base_url) = openrouter().await?;
    let frame = |url: &str, kind: &str| json!({ "type": "image_url", "image_url": { "url": url }, "frame_type": kind });
    let mut frames = vec![frame(&input.first_frame, "first_frame")];
    if let Some(last) = input.last_frame.as_deref() {
        frames.push(frame(last, "last_frame"));
    }
    let body = json!({
        "model": input.model.as_deref().unwrap_or(DEFAULT_MODEL),
        "prompt": input.prompt.trim(),
        "size": input.size,
        "duration": input.duration,
        "generate_audio": false,
        "frame_images": frames,
    });
    let client = get_long_running_client().await;
    let reply = provider_json(
        client.post(format!("{base_url}/videos")).json(&body),
        &api_key,
        "submit",
    )
    .await?;
    Ok(Json(job_view(&reply)))
}

pub async fn job_status(
    _admin: AdminClaims,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    if !valid_job_id(&id) {
        return Err(HttpError(AppError::bad_request("Invalid job id")));
    }
    let (api_key, base_url) = openrouter().await?;
    let client = get_long_running_client().await;
    let reply = provider_json(
        client.get(format!("{base_url}/videos/{id}")),
        &api_key,
        "status",
    )
    .await?;
    Ok(Json(job_view(&reply)))
}

pub async fn job_content(
    _admin: AdminClaims,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    if !valid_job_id(&id) {
        return Err(HttpError(AppError::bad_request("Invalid job id")));
    }
    let (api_key, base_url) = openrouter().await?;
    let client = get_long_running_client().await;
    let response = client
        .get(format!("{base_url}/videos/{id}/content?index=0"))
        .bearer_auth(&api_key)
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(error = %error.without_url(), "video download failed");
            HttpError(AppError::bad_gateway("Video provider is unreachable"))
        })?;
    let status = response.status();
    if !status.is_success() {
        let message = provider_message(&response.text().await.unwrap_or_default(), &api_key);
        tracing::warn!(%status, %message, "video download refused");
        return Err(HttpError(AppError::bad_gateway(format!(
            "Video is not available ({status}) {message}"
        ))));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| HttpError(AppError::bad_gateway("Unable to load generated video")))?;
    if bytes.is_empty() || bytes.len() > MAX_VIDEO_BYTES {
        return Err(HttpError(AppError::bad_gateway(
            "Unable to load generated video",
        )));
    }
    Ok(([(header::CONTENT_TYPE, "video/mp4")], bytes).into_response())
}
