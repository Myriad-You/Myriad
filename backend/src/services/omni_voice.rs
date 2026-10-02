//! Her own voice, from the model she thinks with (see
//! `DynamicConfig::merope_omni_voice`): Lite's Omni model, called directly on
//! DashScope's OpenAI-compatible endpoint, writes her reply and says it in
//! the same stream. The words go where any reply goes; the sound goes to
//! whoever is listening, as it comes.
//!
//! The stream (DashScope's Qwen-Omni guide): text in
//! `choices[0].delta.content`, sound in `choices[0].delta.audio.data` as
//! base64 of 16-bit mono PCM at 24 kHz; `stream_options.include_usage` is
//! required alongside `stream`. OpenRouter carries only the text of this
//! model, so the voice needs the direct endpoint.
//!
//! The key stays here: it is never in an event, a log line or an error.
//!
//! The sound does not ride the agent's event stream: every event there is
//! kept and replayed, and sound would crowd the events out and fill the
//! database. The panel that will play a turn opens its own stream under a
//! token it made, sent with the turn (`customData.voiceOut`); the turn
//! writes the sound there, and closes it at once if it will not speak.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::config::OmniVoice;
use crate::services::analyzer::ImageInput;

/// The sound she makes, as it arrives.
pub const SAMPLE_RATE: u32 = 24_000;

/// Larger than a text stream's: a minute of her voice is about 4 MB of
/// base64.
const MAX_STREAM_BYTES: usize = 24 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
/// A stream that goes quiet this long has stopped.
const IDLE: Duration = Duration::from_secs(30);

/// A turn's voice stream, by the token the panel made: whoever comes
/// first (the panel opening it, or the turn writing to it) finds it made.
struct Stream {
    user_id: i32,
    made: Instant,
    sender: Option<tokio::sync::mpsc::UnboundedSender<axum::body::Bytes>>,
    receiver: Option<tokio::sync::mpsc::UnboundedReceiver<axum::body::Bytes>>,
}

static STREAMS: LazyLock<Mutex<HashMap<String, Stream>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// An end nobody came for in this long is let go.
const UNCLAIMED_FOR: Duration = Duration::from_secs(120);
const TOKEN_CHARS: std::ops::RangeInclusive<usize> = 16..=64;

/// The stream under `token`, made if new: none when the token is not one a
/// panel could have made, or the stream is someone else's.
fn with_stream<T>(token: &str, user_id: i32, take: impl FnOnce(&mut Stream) -> T) -> Option<T> {
    if !TOKEN_CHARS.contains(&token.len())
        || !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let mut streams = STREAMS.lock().ok()?;
    // Kept until they expire, taken or not, so a token pairs only once;
    // an end nobody came for is let go then.
    streams.retain(|_, stream| stream.made.elapsed() < UNCLAIMED_FOR);
    let stream = streams.entry(token.to_string()).or_insert_with(|| {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        Stream {
            user_id,
            made: Instant::now(),
            sender: Some(sender),
            receiver: Some(receiver),
        }
    });
    (stream.user_id == user_id).then(|| take(stream))
}

/// The end the turn writes her voice into: raw 16-bit mono PCM at
/// [`SAMPLE_RATE`]. Dropping it ends the stream.
pub fn voice_out(token: &str, user_id: i32) -> Option<VoiceOut> {
    with_stream(token, user_id, |stream| stream.sender.take())
        .flatten()
        .map(|sender| VoiceOut { sender })
}

/// The end the panel plays from.
pub fn voice_in(
    token: &str,
    user_id: i32,
) -> Option<tokio::sync::mpsc::UnboundedReceiver<axum::body::Bytes>> {
    with_stream(token, user_id, |stream| stream.receiver.take()).flatten()
}

/// Where a turn's voice goes.
pub struct VoiceOut {
    sender: tokio::sync::mpsc::UnboundedSender<axum::body::Bytes>,
}

impl VoiceOut {
    /// A piece of her voice as it came (base64 PCM); false once nobody is
    /// listening.
    pub fn send(&self, base64_pcm: &str) -> bool {
        use base64::Engine as _;
        match base64::engine::general_purpose::STANDARD.decode(base64_pcm.trim()) {
            Ok(pcm) if !pcm.is_empty() => self.sender.send(axum::body::Bytes::from(pcm)).is_ok(),
            Ok(_) => true,
            Err(_) => {
                tracing::warn!("[Voice] a piece of her voice could not be read");
                true
            }
        }
    }
}

/// What arrives while she speaks.
pub enum OmniDelta {
    /// A piece of what she says, as text.
    Text(String),
    /// A piece of how it sounds: base64 of 16-bit mono PCM at
    /// [`SAMPLE_RATE`].
    Audio(String),
}

/// How a recording rides in a request. DashScope's guide passes base64
/// with this data-URL prefix (not verified against the live API yet).
const HEARD_PREFIX: &str = "data:;base64,";

/// What she is asked: `prompt`, with `images` and what she `heard` (base64
/// WAV) beside it; `aloud`, she answers with her voice too.
fn request_body(
    voice: &OmniVoice,
    prompt: &str,
    images: &[ImageInput],
    heard: Option<&str>,
    aloud: bool,
) -> Value {
    let content = if images.is_empty() && heard.is_none() {
        json!(prompt)
    } else {
        let mut parts = vec![json!({ "type": "text", "text": prompt })];
        parts.extend(images.iter().map(|image| {
            json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{};base64,{}", image.mime, image.base64) }
            })
        }));
        if let Some(heard) = heard {
            parts.push(json!({
                "type": "input_audio",
                "input_audio": { "data": format!("{HEARD_PREFIX}{heard}"), "format": "wav" }
            }));
        }
        Value::Array(parts)
    };
    let mut body = json!({
        "model": voice.model,
        "messages": [{ "role": "user", "content": content }],
        "stream": true,
        "stream_options": { "include_usage": true },
        "modalities": ["text"],
    });
    if aloud {
        let mut audio = json!({ "format": "wav" });
        if !voice.voice.is_empty() {
            audio["voice"] = json!(voice.voice);
        }
        body["modalities"] = json!(["text", "audio"]);
        body["audio"] = audio;
    }
    body
}

/// What someone said, kept by the token the panel will send with the turn
/// (`customData.voiceIn`): base64 WAV, so she hears it as said.
struct Heard {
    user_id: i32,
    made: Instant,
    wav_base64: String,
}

static HEARD: LazyLock<Mutex<HashMap<String, Heard>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// A recording as Omni takes it: under 10 MB of base64.
pub const HEARD_MAX_BASE64: usize = 9 * 1024 * 1024;

/// Keep what `user_id` said under `token`, for their turn.
pub fn keep_heard(token: &str, user_id: i32, wav_base64: String) -> bool {
    if !TOKEN_CHARS.contains(&token.len())
        || !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || wav_base64.len() > HEARD_MAX_BASE64
    {
        return false;
    }
    let Ok(mut heard) = HEARD.lock() else {
        return false;
    };
    heard.retain(|_, kept| kept.made.elapsed() < UNCLAIMED_FOR);
    if heard.contains_key(token) {
        return false;
    }
    heard.insert(
        token.to_string(),
        Heard {
            user_id,
            made: Instant::now(),
            wav_base64,
        },
    );
    true
}

/// What `user_id` said under `token`, once.
pub fn take_heard(token: &str, user_id: i32) -> Option<String> {
    let mut heard = HEARD.lock().ok()?;
    if heard.get(token)?.user_id != user_id {
        return None;
    }
    heard.remove(token).map(|kept| kept.wav_base64)
}

/// What was said in a recording, written down in its own words: for the
/// turn's text (its history and what it brings to mind), while she hears
/// the recording itself.
pub async fn transcribe(voice: &OmniVoice, wav_base64: &str) -> Result<String> {
    const ASK: &str = "Write down exactly what is said in this recording, in the language it is said in. Only the words, nothing else; if nothing is said, write nothing.";
    let text = call(voice, ASK, &[], Some(wav_base64), false, |_| async { true }).await?;
    Ok(text.trim().to_string())
}

/// The pieces in one event of the stream.
fn deltas(event: &Value) -> Vec<OmniDelta> {
    let mut out = Vec::new();
    let Some(delta) = event.pointer("/choices/0/delta") else {
        return out;
    };
    if let Some(text) = delta.get("content").and_then(Value::as_str)
        && !text.is_empty()
    {
        out.push(OmniDelta::Text(text.to_string()));
    }
    if let Some(data) = delta.pointer("/audio/data").and_then(Value::as_str)
        && !data.is_empty()
    {
        out.push(OmniDelta::Audio(data.to_string()));
    }
    out
}

/// Whether the stream says it is done, or failed (a short message that
/// never carries what was sent).
fn finished(event: &Value) -> Option<Result<(), String>> {
    if let Some(error) = event.get("error").filter(|error| !error.is_null()) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Some(Err(message.chars().take(200).collect()));
    }
    event
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .map(|reason| {
            if reason == "error" {
                Err("the provider stopped with an error".to_string())
            } else {
                Ok(())
            }
        })
}

/// She answers `prompt` (with `images`, and what she `heard`, beside it)
/// aloud: each piece of text and sound goes to `on_delta` as it comes, until
/// it returns false. The whole text, when the stream finished.
pub async fn speak<F, Fut>(
    voice: &OmniVoice,
    prompt: &str,
    images: &[ImageInput],
    heard: Option<&str>,
    on_delta: F,
) -> Result<String>
where
    F: FnMut(OmniDelta) -> Fut + Send,
    Fut: Future<Output = bool> + Send,
{
    call(voice, prompt, images, heard, true, on_delta).await
}

async fn call<F, Fut>(
    voice: &OmniVoice,
    prompt: &str,
    images: &[ImageInput],
    heard: Option<&str>,
    aloud: bool,
    mut on_delta: F,
) -> Result<String>
where
    F: FnMut(OmniDelta) -> Fut + Send,
    Fut: Future<Output = bool> + Send,
{
    let client = crate::services::http_client::get_long_running_client().await;
    let url = format!("{}/chat/completions", voice.base_url);
    let response = client
        .post(&url)
        .bearer_auth(&voice.api_key)
        .json(&request_body(voice, prompt, images, heard, aloud))
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("could not reach the voice model"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("the voice model answered HTTP {status}");
    }
    let result = read_stream(response, &mut on_delta).await;
    let (outcome, code) = match &result {
        Ok(_) => ("completed", None),
        Err(_) => ("failed", Some("AI_PROVIDER_ERROR")),
    };
    crate::services::ai_cost_ledger::record_ai_call_from_attribution(
        "openai",
        &voice.model,
        prompt.len() + heard.map_or(0, str::len),
        result.as_ref().map_or(0, String::len),
        outcome,
        code,
    )
    .await;
    result
}

async fn read_stream<F, Fut>(mut response: reqwest::Response, on_delta: &mut F) -> Result<String>
where
    F: FnMut(OmniDelta) -> Fut + Send,
    Fut: Future<Output = bool> + Send,
{
    let mut buffer: Vec<u8> = Vec::new();
    let mut received = 0usize;
    let mut text = String::new();
    let mut done = false;
    loop {
        let chunk = match tokio::time::timeout(IDLE, response.chunk()).await {
            Err(_) => bail!("the voice model went quiet"),
            Ok(chunk) => chunk.context("voice stream read error")?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        received = received.saturating_add(chunk.len());
        if received > MAX_STREAM_BYTES {
            bail!("the voice stream exceeded its size limit");
        }
        buffer.extend_from_slice(&chunk);
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = line.trim_ascii();
            let Some(data) = line.strip_prefix(b"data:").map(<[u8]>::trim_ascii_start) else {
                continue;
            };
            if data == b"[DONE]" {
                return Ok(text);
            }
            let Ok(event) = serde_json::from_slice::<Value>(data) else {
                continue;
            };
            match finished(&event) {
                Some(Err(message)) => bail!("the voice model reported an error: {message}"),
                Some(Ok(())) => done = true,
                None => {}
            }
            for delta in deltas(&event) {
                if let OmniDelta::Text(piece) = &delta {
                    text.push_str(piece);
                }
                if !on_delta(delta).await {
                    return Ok(text);
                }
            }
        }
        if buffer.len() > MAX_LINE_BYTES {
            bail!("the voice stream exceeded its event size limit");
        }
    }
    if done {
        Ok(text)
    } else {
        Err(crate::services::analyzer::StreamCut { partial: text }.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(name: &str) -> OmniVoice {
        OmniVoice {
            base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".into(),
            api_key: "sk-test".into(),
            model: "qwen3.8-omni-flash".into(),
            voice: name.into(),
        }
    }

    #[test]
    fn she_asks_for_text_and_sound_in_one_stream() {
        let body = request_body(&voice("Tina"), "你好", &[], None, true);
        assert_eq!(body["modalities"], json!(["text", "audio"]));
        assert_eq!(body["audio"]["voice"], "Tina");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["messages"][0]["content"], "你好");
        assert!(
            !body.to_string().contains("sk-test"),
            "the key rides in the header only"
        );
        // No voice named: the model's own.
        assert!(
            request_body(&voice(""), "你好", &[], None, true)["audio"]
                .get("voice")
                .is_none()
        );
        let with_image = request_body(
            &voice(""),
            "看看",
            &[ImageInput {
                mime: "image/png".into(),
                base64: "AAAA".into(),
            }],
            None,
            true,
        );
        assert_eq!(with_image["messages"][0]["content"][1]["type"], "image_url");
        // What she heard rides beside the words; writing it down is text only.
        let hearing = request_body(&voice("Tina"), "写下来", &[], Some("UklGRg=="), false);
        let part = &hearing["messages"][0]["content"][1];
        assert_eq!(part["type"], "input_audio");
        assert_eq!(part["input_audio"]["data"], "data:;base64,UklGRg==");
        assert_eq!(hearing["modalities"], json!(["text"]));
        assert!(hearing.get("audio").is_none());
    }

    #[tokio::test]
    async fn her_voice_goes_to_the_panel_that_asked_for_it_and_no_one_else() {
        use base64::Engine as _;
        let token = "turn-voice-token-0001";
        // The turn may come first.
        let out = voice_out(token, 7).expect("made for the turn");
        assert!(voice_out(token, 7).is_none(), "one writer");
        assert!(voice_in(token, 8).is_none(), "not someone else's");
        let mut heard = voice_in(token, 7).expect("the panel's");
        assert!(voice_in(token, 7).is_none(), "one listener");
        let pcm = [1u8, 0, 2, 0];
        assert!(out.send(&base64::engine::general_purpose::STANDARD.encode(pcm)));
        drop(out);
        assert_eq!(heard.recv().await.unwrap().as_ref(), &pcm);
        assert!(heard.recv().await.is_none(), "dropped: the stream ends");
        // A token no panel would make is nothing.
        assert!(voice_out("short", 7).is_none());
        assert!(voice_out("has spaces in it, sixteen+", 7).is_none());
    }

    #[test]
    fn what_was_said_is_kept_for_that_persons_turn_once() {
        let token = "heard-token-000000001";
        assert!(keep_heard(token, 7, "UklGRg==".into()));
        assert!(
            !keep_heard(token, 7, "UklGRg==".into()),
            "one recording a token"
        );
        assert_eq!(take_heard(token, 8), None, "not someone else's");
        assert_eq!(take_heard(token, 7).as_deref(), Some("UklGRg=="));
        assert_eq!(take_heard(token, 7), None, "once");
        assert!(!keep_heard("short", 7, "x".into()));
    }

    #[test]
    fn the_stream_gives_words_and_sound_apart() {
        let event = json!({"choices":[{"delta":{"content":"你好","audio":{"data":"AAEC"}}}]});
        let pieces = deltas(&event);
        assert!(matches!(&pieces[0], OmniDelta::Text(text) if text == "你好"));
        assert!(matches!(&pieces[1], OmniDelta::Audio(data) if data == "AAEC"));
        assert!(deltas(&json!({"choices":[{"delta":{"content":""}}]})).is_empty());
        assert!(deltas(&json!({"usage":{"total_tokens":3}})).is_empty());
        assert_eq!(
            finished(&json!({"choices":[{"finish_reason":"stop","delta":{}}]})),
            Some(Ok(()))
        );
        assert!(matches!(
            finished(&json!({"error":{"message":"bad voice"}})),
            Some(Err(message)) if message == "bad voice"
        ));
    }
}
