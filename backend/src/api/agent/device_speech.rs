//! `POST /api/agent/device/tts`: one line of hers read aloud in her host's voice,
//! for a device (`sync_device_auth`: a device token, never a session).
//!
//! A device that shows her (the XR glasses, a phone) reads her lines aloud. It
//! is not to set up a voice of its own: her voice is the one this site was set
//! up with (`speech_source`), as the web reads her (`speechPipelineHost.ts`:
//! the line cut to 150, MP3, 16 kHz, no voice of its own asked for). So this is
//! that same synthesis (`synthesize_standalone_tts`), on the device owner's
//! account in the cost ledger. Where this site has no voice the answer says so
//! (`503 no_voice`) and the device reads the line in its own.
//!
//! The answer is the sound itself, not JSON: a device plays it and keeps
//! nothing. What went wrong is said in words that carry no key and no address
//! of the speech service (they are the shared ones of `standalone_tts`).

use axum::{
    Json,
    extract::Extension,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::Deserialize;
use serde_json::json;

use crate::middleware::sync_device::SyncDevice;
use crate::services::standalone_tts::{TtsApiRequest, synthesize_standalone_tts};

/// The most of a line read at once, in characters: what the web asks for, and
/// what Tencent's synthesis takes.
pub const LINE_MAX_CHARS: usize = 150;

#[derive(Debug, Deserialize)]
pub struct DeviceTtsRequest {
    pub text: String,
}

/// What is wrong with a line before anything is synthesized, as the status and
/// the code a device reads.
fn check(text: &str) -> Result<&str, (StatusCode, &'static str)> {
    let text = text.trim();
    if text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty"));
    }
    if text.chars().count() > LINE_MAX_CHARS {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "too_long"));
    }
    Ok(text)
}

/// The media type of a sound by its first bytes: the providers answer in MP3
/// as asked, Gemini in WAV.
fn sound_type(sound: &[u8]) -> &'static str {
    match sound {
        [b'I', b'D', b'3', ..] => "audio/mpeg",
        [0xFF, second, ..] if second & 0xE0 == 0xE0 => "audio/mpeg",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'A',
            b'V',
            b'E',
            ..,
        ] => "audio/wav",
        [b'O', b'g', b'g', b'S', ..] => "audio/ogg",
        _ => "application/octet-stream",
    }
}

fn refused(status: StatusCode, code: &'static str, error: &str) -> Response {
    (status, Json(json!({ "code": code, "error": error }))).into_response()
}

pub async fn device_tts(
    Extension(device): Extension<SyncDevice>,
    Json(body): Json<DeviceTtsRequest>,
) -> Response {
    let text = match check(&body.text) {
        Ok(text) => text.to_string(),
        Err((status, code)) => return refused(status, code, "This line cannot be read aloud"),
    };
    let probe = crate::services::speech_runtime::speech_probe().await;
    if !probe.tts_enabled {
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "no_voice",
            "This site has no voice set up",
        );
    }
    let request = TtsApiRequest {
        text,
        voice_type: None,
        speed: None,
        volume: None,
        codec: Some("mp3".into()),
        sample_rate: Some(16000),
        emotion: None,
        force_regenerate: false,
    };
    let owner = super::sync::owner_of(&device);
    let answer = crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "speech",
        "tts",
        synthesize_standalone_tts(&request),
    )
    .await;
    let sound = match answer {
        Ok(answer) => answer.audio.and_then(|audio| BASE64.decode(audio).ok()),
        Err(error) => {
            tracing::warn!(device = %device.0.id, "device tts failed: {error}");
            return refused(StatusCode::BAD_GATEWAY, "voice_failed", &error);
        }
    };
    match sound {
        Some(sound) if !sound.is_empty() => {
            ([(header::CONTENT_TYPE, sound_type(&sound))], sound).into_response()
        }
        _ => refused(
            StatusCode::BAD_GATEWAY,
            "voice_failed",
            "Speech service request failed",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_trimmed_and_held_to_what_the_web_reads() {
        assert_eq!(check("  你回来啦。 "), Ok("你回来啦。"));
        assert_eq!(check("   "), Err((StatusCode::BAD_REQUEST, "empty")));
        let longest = "好".repeat(LINE_MAX_CHARS);
        assert_eq!(check(&longest), Ok(longest.as_str()));
        // Counted in characters, not bytes: 150 Chinese characters are 450 bytes.
        assert_eq!(
            check(&"好".repeat(LINE_MAX_CHARS + 1)),
            Err((StatusCode::PAYLOAD_TOO_LARGE, "too_long"))
        );
    }

    #[test]
    fn a_sound_is_typed_by_its_first_bytes() {
        assert_eq!(sound_type(b"ID3\x04\0\0"), "audio/mpeg");
        assert_eq!(sound_type(&[0xFF, 0xFB, 0x90, 0x00]), "audio/mpeg");
        assert_eq!(sound_type(b"RIFF\x24\0\0\0WAVEfmt "), "audio/wav");
        assert_eq!(sound_type(b"OggS\0\x02"), "audio/ogg");
        assert_eq!(sound_type(b"{\"x\":1}"), "application/octet-stream");
        assert_eq!(sound_type(&[]), "application/octet-stream");
    }

    async fn post(
        app: &axum::Router,
        token: Option<&str>,
        text: &str,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/agent/device/tts")
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let body = axum::body::Body::from(json!({ "text": text }).to_string());
        let response = app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// The route over HTTP on a real database (an isolated schema; skipped
    /// without a test database, as `sync`'s device routes are): no token and
    /// a forged one are turned away at the gate, a session is not a device,
    /// and a live device token gets to the line, where a site with no voice
    /// set up says so for the device to read in its own.
    #[tokio::test]
    async fn only_a_live_device_token_reaches_the_voice() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = fixture.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'owner41', true, true)",
        )
        .await
        .unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let state = crate::state::AppState::new(
            db.clone(),
            crate::config::AppConfig::default(),
            crate::config::DynamicConfig::default(),
        );
        let app = axum::Router::new()
            .nest(
                "/api/agent",
                super::super::create_agent_routes(state.clone()),
            )
            .with_state(state);
        let (_, token) = crate::services::persona_sync::devices::add(&db, 41, "Glasses")
            .await
            .unwrap();

        let (status, body) = post(&app, None, "你好").await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("device_token_required"))
        );
        let forged = format!("mys_{}", "A".repeat(43));
        let (status, body) = post(&app, Some(&forged), "你好").await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("device_token_invalid"))
        );
        // Through the gate, the line is looked at before any voice is asked for.
        let (status, body) = post(&app, Some(&token), "   ").await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("empty"))
        );
        let (status, body) = post(&app, Some(&token), &"好".repeat(LINE_MAX_CHARS + 1)).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::PAYLOAD_TOO_LARGE, Some("too_long"))
        );
        // A test site has no speech set up: said, and nothing of the token in it.
        let (status, body) = post(&app, Some(&token), "你好").await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, Some("no_voice"))
        );
        assert!(!body.to_string().contains(&token));
    }
}
