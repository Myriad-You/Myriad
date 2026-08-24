//! Remote client for the public See-through Gradio Space.
//!
//! The model is deliberately never loaded by Myriad. Character pixels leave
//! the host only after the site owner explicitly asks for decomposition, and
//! the Hugging Face credential is resolved only for these outbound requests.

use reqwest::{multipart, Client, RequestBuilder, StatusCode, Url};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::OnceLock, time::Duration};

pub const SPACE_BASE_URL: &str = "https://24yearsold-see-through-demo.hf.space";
pub const SPACE_NAME: &str = "24yearsold/see-through-demo";
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_CONTROL_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_PSD_BYTES: usize = 32 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecomposeOptions {
    pub resolution: u16,
    pub seed: u16,
    pub split_arms_and_legs: bool,
}

impl Default for DecomposeOptions {
    fn default() -> Self {
        Self {
            // The Space itself recommends 768 for ZeroGPU's free tier.
            resolution: 768,
            seed: 42,
            // Myriad requires separate left/right rigid arm fragments. Lower
            // limb layers may be returned too, but the upper-body compiler
            // deliberately ignores them.
            split_arms_and_legs: true,
        }
    }
}

impl DecomposeOptions {
    pub fn validate(self) -> Result<Self, SeeThroughError> {
        if !(768..=1280).contains(&self.resolution) || self.resolution % 64 != 0 {
            return Err(SeeThroughError::InvalidInput(
                "See-through resolution must be 768–1280 in steps of 64".to_string(),
            ));
        }
        if self.seed > 9999 {
            return Err(SeeThroughError::InvalidInput(
                "See-through seed must be between 0 and 9999".to_string(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug)]
pub struct DecomposeOutput {
    pub psd: Vec<u8>,
    pub event_id: String,
}

#[derive(Debug)]
pub enum SeeThroughError {
    NotConfigured,
    Busy,
    InvalidInput(String),
    Authentication,
    Quota,
    Timeout,
    Transport(String),
    Upstream { stage: &'static str, status: u16 },
    Rejected,
    InvalidOutput(String),
}

impl std::fmt::Display for SeeThroughError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => formatter.write_str("Hugging Face API token is not configured"),
            Self::Busy => formatter.write_str("A See-through decomposition is already running"),
            Self::InvalidInput(message)
            | Self::Transport(message)
            | Self::InvalidOutput(message) => formatter.write_str(message),
            Self::Authentication => formatter.write_str("Hugging Face rejected the API token"),
            Self::Quota => formatter.write_str("Hugging Face ZeroGPU quota is exhausted"),
            Self::Timeout => formatter.write_str("See-through inference timed out"),
            Self::Upstream { stage, status } => {
                write!(formatter, "See-through {stage} returned HTTP {status}")
            }
            Self::Rejected => formatter.write_str(
                "See-through ZeroGPU rejected the job; check the Hugging Face token and quota",
            ),
        }
    }
}

impl std::error::Error for SeeThroughError {}

pub fn validate_hf_token(token: &str) -> Result<String, SeeThroughError> {
    let token = token.trim();
    if token.len() < 20
        || token.len() > 512
        || !token.starts_with("hf_")
        || token.chars().any(char::is_whitespace)
        || !token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(SeeThroughError::InvalidInput(
            "Hugging Face token must be a valid hf_… access token".to_string(),
        ));
    }
    Ok(token.to_string())
}

pub fn configured_hf_token(config: &crate::config::DynamicConfig) -> Option<String> {
    config
        .see_through_hf_token
        .as_deref()
        .and_then(|value| validate_hf_token(value).ok())
        .or_else(|| {
            ["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN"]
                .into_iter()
                .find_map(|key| std::env::var(key).ok())
                .and_then(|value| validate_hf_token(&value).ok())
        })
}

fn inference_gate() -> &'static tokio::sync::Semaphore {
    static GATE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Semaphore::new(1))
}

#[derive(Clone)]
pub struct SeeThroughClient {
    client: Client,
    token: String,
}

impl SeeThroughClient {
    pub async fn new(token: String) -> Result<Self, SeeThroughError> {
        let token = validate_hf_token(&token)?;
        let proxy = crate::services::http_client::ProxyConfig::from_dynamic_config().await;
        let builder = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Myriad-Merope/See-through");
        let client = crate::services::http_client::apply_proxy(builder, &proxy)
            .map_err(|error| SeeThroughError::Transport(error.to_string()))?
            .build()
            .map_err(|error| SeeThroughError::Transport(error.to_string()))?;
        Ok(Self { client, token })
    }

    fn authenticated(&self, request: RequestBuilder) -> RequestBuilder {
        request.bearer_auth(&self.token)
    }

    pub async fn decompose(
        &self,
        image: Vec<u8>,
        media_type: &str,
        options: DecomposeOptions,
    ) -> Result<DecomposeOutput, SeeThroughError> {
        let _permit = inference_gate()
            .try_acquire()
            .map_err(|_| SeeThroughError::Busy)?;
        let options = options.validate()?;
        validate_image(&image, media_type)?;

        let remote_path = self.upload(image, media_type).await?;
        let event_id = self.start_inference(&remote_path, options).await?;
        let output_url = self.await_output(&event_id).await?;
        let psd = self.download_psd(output_url).await?;
        Ok(DecomposeOutput { psd, event_id })
    }

    async fn upload(&self, image: Vec<u8>, media_type: &str) -> Result<String, SeeThroughError> {
        let extension = match media_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => {
                return Err(SeeThroughError::InvalidInput(
                    "See-through accepts PNG, JPEG, or WebP portraits".to_string(),
                ))
            }
        };
        let part = multipart::Part::bytes(image)
            .file_name(format!("character.{extension}"))
            .mime_str(media_type)
            .map_err(|error| SeeThroughError::InvalidInput(error.to_string()))?;
        let response = self
            .authenticated(
                self.client
                    .post(format!("{SPACE_BASE_URL}/gradio_api/upload")),
            )
            .multipart(multipart::Form::new().part("files", part))
            .send()
            .await
            .map_err(transport_error)?;
        let body = successful_body(response, "upload", MAX_CONTROL_RESPONSE_BYTES).await?;
        let paths: Vec<String> = serde_json::from_slice(&body).map_err(|_| {
            SeeThroughError::InvalidOutput(
                "See-through upload returned an invalid response".to_string(),
            )
        })?;
        let path = paths.into_iter().next().ok_or_else(|| {
            SeeThroughError::InvalidOutput(
                "See-through upload did not return a file path".to_string(),
            )
        })?;
        validate_remote_temp_path(&path)?;
        Ok(path)
    }

    async fn start_inference(
        &self,
        remote_path: &str,
        options: DecomposeOptions,
    ) -> Result<String, SeeThroughError> {
        let response = self
            .authenticated(
                self.client
                    .post(format!("{SPACE_BASE_URL}/gradio_api/call/inference")),
            )
            .json(&json!({
                "data": [
                    {
                        "path": remote_path,
                        "orig_name": "character.png",
                        "meta": { "_type": "gradio.FileData" }
                    },
                    options.resolution,
                    options.seed,
                    options.split_arms_and_legs
                ]
            }))
            .send()
            .await
            .map_err(transport_error)?;
        let body = successful_body(response, "submission", MAX_CONTROL_RESPONSE_BYTES).await?;
        let response: EventCreated = serde_json::from_slice(&body).map_err(|_| {
            SeeThroughError::InvalidOutput(
                "See-through submission returned an invalid response".to_string(),
            )
        })?;
        validate_event_id(&response.event_id)?;
        Ok(response.event_id)
    }

    async fn await_output(&self, event_id: &str) -> Result<Url, SeeThroughError> {
        let response = self
            .authenticated(self.client.get(format!(
                "{SPACE_BASE_URL}/gradio_api/call/inference/{event_id}"
            )))
            .send()
            .await
            .map_err(transport_error)?;
        let body = successful_body(response, "event stream", MAX_CONTROL_RESPONSE_BYTES).await?;
        let terminal = parse_terminal_event(&body)?;
        match terminal {
            TerminalEvent::Complete(data) => output_file_url(&data),
            TerminalEvent::Error => Err(SeeThroughError::Rejected),
        }
    }

    async fn download_psd(&self, url: Url) -> Result<Vec<u8>, SeeThroughError> {
        validate_output_url(&url)?;
        let response = self
            .authenticated(self.client.get(url))
            .send()
            .await
            .map_err(transport_error)?;
        let psd = successful_body(response, "PSD download", MAX_PSD_BYTES).await?;
        validate_psd(&psd)?;
        Ok(psd)
    }
}

#[derive(Debug, Deserialize)]
struct EventCreated {
    event_id: String,
}

enum TerminalEvent {
    Complete(Value),
    Error,
}

fn validate_image(image: &[u8], media_type: &str) -> Result<(), SeeThroughError> {
    if image.is_empty() || image.len() > MAX_IMAGE_BYTES {
        return Err(SeeThroughError::InvalidInput(
            "Master portrait is empty or exceeds 10 MB".to_string(),
        ));
    }
    if !matches!(media_type, "image/png" | "image/jpeg" | "image/webp") {
        return Err(SeeThroughError::InvalidInput(
            "Master portrait must be PNG, JPEG, or WebP".to_string(),
        ));
    }
    Ok(())
}

fn validate_remote_temp_path(path: &str) -> Result<(), SeeThroughError> {
    let valid = path.starts_with("/tmp/gradio/")
        && path.len() <= 2048
        && !path.contains('\0')
        && !path.split('/').any(|component| component == "..");
    if valid {
        Ok(())
    } else {
        Err(SeeThroughError::InvalidOutput(
            "See-through upload returned an unsafe file path".to_string(),
        ))
    }
}

fn validate_event_id(event_id: &str) -> Result<(), SeeThroughError> {
    if !event_id.is_empty()
        && event_id.len() <= 128
        && event_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        Ok(())
    } else {
        Err(SeeThroughError::InvalidOutput(
            "See-through returned an invalid event id".to_string(),
        ))
    }
}

fn parse_terminal_event(body: &[u8]) -> Result<TerminalEvent, SeeThroughError> {
    let text = std::str::from_utf8(body).map_err(|_| {
        SeeThroughError::InvalidOutput("See-through event stream is not UTF-8".to_string())
    })?;
    let normalized = text.replace("\r\n", "\n");
    for block in normalized.split("\n\n") {
        let mut event = None;
        let mut data = Vec::new();
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event = Some(value.trim());
            } else if let Some(value) = line.strip_prefix("data:") {
                data.push(value.trim_start());
            }
        }
        match event {
            Some("complete") => {
                let value = serde_json::from_str(&data.join("\n")).map_err(|_| {
                    SeeThroughError::InvalidOutput(
                        "See-through completion payload is invalid".to_string(),
                    )
                })?;
                return Ok(TerminalEvent::Complete(value));
            }
            Some("error") => return Ok(TerminalEvent::Error),
            _ => {}
        }
    }
    Err(SeeThroughError::InvalidOutput(
        "See-through event stream ended without a result".to_string(),
    ))
}

fn output_file_url(data: &Value) -> Result<Url, SeeThroughError> {
    let file = data
        .as_array()
        .and_then(|values| values.first())
        .and_then(Value::as_object)
        .ok_or_else(|| {
            SeeThroughError::InvalidOutput(
                "See-through result did not contain a PSD file".to_string(),
            )
        })?;
    let url = if let Some(url) = file.get("url").and_then(Value::as_str) {
        Url::parse(url).map_err(|_| {
            SeeThroughError::InvalidOutput("See-through returned an invalid PSD URL".to_string())
        })?
    } else if let Some(path) = file.get("path").and_then(Value::as_str) {
        validate_remote_temp_path(path)?;
        Url::parse(&format!("{SPACE_BASE_URL}/gradio_api/file={path}")).map_err(|_| {
            SeeThroughError::InvalidOutput("See-through returned an invalid PSD path".to_string())
        })?
    } else {
        return Err(SeeThroughError::InvalidOutput(
            "See-through result did not expose a PSD download".to_string(),
        ));
    };
    validate_output_url(&url)?;
    Ok(url)
}

fn validate_output_url(url: &Url) -> Result<(), SeeThroughError> {
    let expected = Url::parse(SPACE_BASE_URL).expect("constant See-through URL is valid");
    let valid = url.scheme() == "https"
        && url.host_str() == expected.host_str()
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().starts_with("/gradio_api/file=")
        && url.fragment().is_none();
    if valid {
        Ok(())
    } else {
        Err(SeeThroughError::InvalidOutput(
            "See-through returned an unsafe PSD download URL".to_string(),
        ))
    }
}

fn validate_psd(bytes: &[u8]) -> Result<(), SeeThroughError> {
    let valid =
        bytes.len() >= 26 && bytes.get(..4) == Some(b"8BPS") && bytes.get(4..6) == Some(&[0, 1]);
    if valid {
        Ok(())
    } else {
        Err(SeeThroughError::InvalidOutput(
            "See-through download is not a valid PSD document".to_string(),
        ))
    }
}

async fn successful_body(
    response: reqwest::Response,
    stage: &'static str,
    max_bytes: usize,
) -> Result<Vec<u8>, SeeThroughError> {
    let status = response.status();
    if !status.is_success() {
        return Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => SeeThroughError::Authentication,
            StatusCode::TOO_MANY_REQUESTS => SeeThroughError::Quota,
            _ => SeeThroughError::Upstream {
                stage,
                status: status.as_u16(),
            },
        });
    }
    crate::services::outbound_security::read_limited_body(response, max_bytes)
        .await
        .map_err(SeeThroughError::Transport)
}

fn transport_error(error: reqwest::Error) -> SeeThroughError {
    if error.is_timeout() {
        SeeThroughError::Timeout
    } else {
        SeeThroughError::Transport(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_space_options_and_token_shape() {
        assert!(DecomposeOptions::default().validate().is_ok());
        assert!(DecomposeOptions {
            resolution: 800,
            ..DecomposeOptions::default()
        }
        .validate()
        .is_err());
        assert!(validate_hf_token("hf_abcdefghijklmnopqrstuvwxyz").is_ok());
        assert!(validate_hf_token("secret").is_err());
        assert!(validate_hf_token("hf_has whitespace in it").is_err());
    }

    #[test]
    fn parses_gradio_complete_event_and_rejects_error_event() {
        let complete = br#"event: heartbeat
data: null

event: complete
data: [{"url":"https://24yearsold-see-through-demo.hf.space/gradio_api/file=/tmp/gradio/a/output.psd"}, {"value":[]}]

"#;
        let TerminalEvent::Complete(data) = parse_terminal_event(complete).unwrap() else {
            panic!("complete event expected")
        };
        let url = output_file_url(&data).unwrap();
        assert_eq!(url.host_str(), Some("24yearsold-see-through-demo.hf.space"));

        assert!(matches!(
            parse_terminal_event(b"event: error\ndata: null\n\n").unwrap(),
            TerminalEvent::Error
        ));
    }

    #[test]
    fn output_download_is_pinned_to_the_space() {
        let malicious = json!([{
            "url": "https://attacker.example/gradio_api/file=/tmp/gradio/a/output.psd"
        }]);
        assert!(output_file_url(&malicious).is_err());
        assert!(validate_remote_temp_path("/tmp/gradio/a/input.png").is_ok());
        assert!(validate_remote_temp_path("/tmp/gradio/../secret").is_err());
    }

    #[test]
    fn validates_psd_signature() {
        let mut bytes = vec![0_u8; 26];
        bytes[..6].copy_from_slice(b"8BPS\0\x01");
        assert!(validate_psd(&bytes).is_ok());
        assert!(validate_psd(b"not a psd").is_err());
    }
}
