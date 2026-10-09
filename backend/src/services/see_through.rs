//! Remote client for a See-through Gradio Space: Myriad's own by default, or
//! any other the owner names. A Space that serves `decompose` fits the portrait
//! on a canvas of its own shape; one that does not (the public demo) pads it to
//! a square through `inference`, as before.
//!
//! The model is deliberately never loaded by Myriad. Character pixels leave
//! the host only after the site owner explicitly asks for decomposition, and
//! the Hugging Face credential is resolved only for these outbound requests.

use reqwest::{Client, RequestBuilder, StatusCode, Url, multipart};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{sync::OnceLock, time::Duration};

/// Myriad's copy of the demo, serving `decompose`: used unless the owner names
/// another Space. Calls count against the caller's ZeroGPU quota, not its owner's.
pub const DEFAULT_SPACE: &str = "SomekawaHitomi/see-through-demo";
/// The canvas (width x height) a Space with `decompose` fits portraits on: one
/// shape that bust (3:4) and full-body (9:16) portraits both fill well, at
/// about the 1280x1280 pixels LayerDiff 3D was trained with.
pub const CANVAS: &str = "1088x1664";
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_CONTROL_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_PSD_BYTES: usize = 32 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// Fitting the turn keys runs on the Space's CPU for many minutes.
const KEYFORMS_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const MAX_KEYFORMS_JSON_BYTES: usize = 32 * 1024 * 1024;
/// A ZeroGPU Space sleeps after two idle days and cannot be kept awake; how
/// long a decomposition waits for one to wake, and how often it looks.
const WAKE_TIMEOUT: Duration = Duration::from_secs(8 * 60);
const WAKE_POLL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecomposeOptions {
    pub resolution: u16,
    pub seed: u16,
    pub split_arms_and_legs: bool,
    /// The hair from a pass of its own (the Space's `decompose_hair`): the body
    /// tags run again on the hair's box at the head pass's scale. A Space
    /// without it decomposes plainly.
    pub hair_pass: bool,
}

impl Default for DecomposeOptions {
    fn default() -> Self {
        Self {
            // Request the Space's upper bound so layer edges stay sharp enough
            // for Anime2.5D. 768 remains valid if a caller asks for it.
            resolution: 1280,
            seed: 42,
            // Myriad requires separate left/right rigid arm fragments. Lower
            // limb layers may be returned too, but the upper-body compiler
            // deliberately ignores them.
            split_arms_and_legs: true,
            hair_pass: false,
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
    /// Fitted on `CANVAS` rather than padded to a square.
    pub canvas: bool,
}

/// A Hugging Face Space running See-through, by its `owner/name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Space {
    name: String,
    base: Url,
}

impl Space {
    pub fn parse(name: &str) -> Result<Self, SeeThroughError> {
        let name = name.trim();
        let invalid = || {
            SeeThroughError::InvalidInput(
                "See-through Space must be an owner/name Hugging Face Space id".to_string(),
            )
        };
        let (owner, repo) = name.split_once('/').ok_or_else(invalid)?;
        let part = |value: &str| {
            !value.is_empty()
                && value.len() <= 96
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                && !value.starts_with(['-', '.'])
                && !value.ends_with(['-', '.'])
        };
        if !part(owner) || !part(repo) {
            return Err(invalid());
        }
        // Spaces are served at owner-name.hf.space, lower case, with `_` and `.` as `-`.
        let subdomain = format!("{owner}-{repo}")
            .to_ascii_lowercase()
            .replace(['_', '.'], "-");
        let base = Url::parse(&format!("https://{subdomain}.hf.space")).map_err(|_| invalid())?;
        Ok(Self {
            name: name.to_string(),
            base,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base.as_str().trim_end_matches('/'))
    }

    fn runtime_url(&self) -> String {
        format!("https://huggingface.co/api/spaces/{}/runtime", self.name)
    }
}

/// What a Space's stage on the Hub means for a decomposition about to start.
#[derive(Debug, PartialEq, Eq)]
enum Readiness {
    Running,
    /// Asleep: a request wakes it.
    Asleep,
    /// Building or starting: wait.
    Starting,
    /// Paused by its owner, or broken: waiting will not help.
    Unavailable,
}

fn readiness(stage: &str) -> Readiness {
    match stage {
        "RUNNING" => Readiness::Running,
        "SLEEPING" => Readiness::Asleep,
        "BUILDING" | "RUNNING_BUILDING" | "APP_STARTING" | "RUNNING_APP_STARTING" | "STOPPED" => {
            Readiness::Starting
        }
        _ => Readiness::Unavailable,
    }
}

/// The Space the owner named, or Myriad's own.
pub fn configured_space(config: &crate::config::DynamicConfig) -> Space {
    config
        .see_through_space
        .as_deref()
        .and_then(|name| Space::parse(name).ok())
        .unwrap_or_else(|| Space::parse(DEFAULT_SPACE).expect("the default Space id is valid"))
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
    Upstream {
        stage: &'static str,
        status: u16,
    },
    Rejected,
    InvalidOutput(String),
    /// Still starting after `WAKE_TIMEOUT`.
    Waking,
    /// Paused, failed to build, or crashed: its stage on the Hub.
    SpaceUnavailable(String),
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
            Self::Waking => formatter.write_str("The See-through Space is still starting"),
            Self::SpaceUnavailable(stage) => {
                write!(formatter, "The See-through Space cannot run ({stage})")
            }
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
    space: Space,
}

impl SeeThroughClient {
    pub async fn new(token: String, space: Space) -> Result<Self, SeeThroughError> {
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
        Ok(Self {
            client,
            token,
            space,
        })
    }

    fn authenticated(&self, request: RequestBuilder) -> RequestBuilder {
        request.bearer_auth(&self.token)
    }

    pub async fn decompose(
        &self,
        image: axum::body::Bytes,
        media_type: &str,
        options: DecomposeOptions,
    ) -> Result<DecomposeOutput, SeeThroughError> {
        let _permit = inference_gate()
            .try_acquire()
            .map_err(|_| SeeThroughError::Busy)?;
        let options = options.validate()?;
        validate_image(&image, media_type)?;

        self.wait_until_running().await?;
        let canvas = self.serves_canvas().await?;
        let hair_pass = canvas && options.hair_pass && self.serves("decompose_hair").await?;
        let remote_path = self.upload(image, media_type).await?;
        let endpoint = if hair_pass {
            "decompose_hair"
        } else if canvas {
            "decompose"
        } else {
            "inference"
        };
        let file = json!({
            "path": remote_path,
            "orig_name": "character.png",
            "meta": { "_type": "gradio.FileData" }
        });
        let data = if hair_pass {
            // Canvas, seed, left/right split, the hair pass on the head's scale.
            json!([
                file,
                CANVAS,
                options.seed,
                options.split_arms_and_legs,
                "head"
            ])
        } else if canvas {
            // Canvas, seed, left/right split, output scale, SDXL size condition.
            json!([
                file,
                CANVAS,
                options.seed,
                options.split_arms_and_legs,
                1,
                "trained"
            ])
        } else {
            json!([
                file,
                options.resolution,
                options.seed,
                options.split_arms_and_legs
            ])
        };
        let event_id = self.start(endpoint, data).await?;
        let output_url = self.await_output(endpoint, &event_id).await?;
        let psd = self.download_psd(output_url).await?;
        Ok(DecomposeOutput {
            psd,
            event_id,
            canvas,
        })
    }

    /// The Space's `keyforms`: each part of the front decomposition fitted onto
    /// four decompositions of the head turned toward image right and left,
    /// raised and lowered. Returns the keys (JSON) and the front decomposition
    /// with what the turns uncover baked in. Waits its turn behind a running
    /// decomposition rather than refusing.
    /// Keys for the turned decompositions. `pictures` are the five drawings
    /// that were decomposed (front, then as `turned`): the fit matches the
    /// keys against them, not only against the decompositions.
    pub async fn turn_keyforms(
        &self,
        front: Vec<u8>,
        turned: [Vec<u8>; 4],
        pictures: [(Vec<u8>, String); 5],
    ) -> Result<TurnKeyformsOutput, SeeThroughError> {
        let _permit = inference_gate()
            .acquire()
            .await
            .map_err(|_| SeeThroughError::Busy)?;
        for psd in std::iter::once(&front).chain(turned.iter()) {
            validate_psd(psd)?;
        }
        for (picture, media_type) in &pictures {
            if picture.is_empty()
                || picture.len() > MAX_IMAGE_BYTES
                || image_extension(media_type).is_none()
            {
                return Err(SeeThroughError::InvalidInput(
                    "A picture for the turn keys must be a PNG, JPEG, or WebP up to 10 MB"
                        .to_string(),
                ));
            }
        }
        self.wait_until_running().await?;
        let mut files = Vec::with_capacity(10);
        for (index, psd) in std::iter::once(front).chain(turned).enumerate() {
            let path = self
                .upload_file(
                    psd,
                    &format!("decomposition-{index}.psd"),
                    "image/vnd.adobe.photoshop",
                )
                .await?;
            files.push(json!({
                "path": path,
                "orig_name": format!("decomposition-{index}.psd"),
                "meta": { "_type": "gradio.FileData" }
            }));
        }
        for (index, (picture, media_type)) in pictures.into_iter().enumerate() {
            let name = format!(
                "picture-{index}.{}",
                image_extension(&media_type).unwrap_or("png")
            );
            let path = self.upload_file(picture, &name, &media_type).await?;
            files.push(json!({
                "path": path,
                "orig_name": name,
                "meta": { "_type": "gradio.FileData" }
            }));
        }
        let event_id = self.start("keyforms", Value::Array(files)).await?;
        let data = self
            .await_data("keyforms", &event_id, KEYFORMS_TIMEOUT)
            .await?;
        let keys_url = output_file_url_at(&self.space, &data, 0)?;
        let psd_url = output_file_url_at(&self.space, &data, 1)?;
        let keyforms = self
            .download(keys_url, "keyforms download", MAX_KEYFORMS_JSON_BYTES)
            .await?;
        serde_json::from_slice::<Value>(&keyforms).map_err(|_| {
            SeeThroughError::InvalidOutput("See-through returned invalid keyforms".to_string())
        })?;
        let psd = self.download_psd(psd_url).await?;
        Ok(TurnKeyformsOutput { keyforms, psd })
    }

    /// The Space's `check`: whether the fit can key from a decomposition (one
    /// whose face took in the hair at its sides, whose headwear took in the
    /// outfit, or whose ears took in the head cannot). A Space without the
    /// endpoint (an older deployment) passes every decomposition.
    pub async fn check_decomposition(
        &self,
        psd: Vec<u8>,
    ) -> Result<DecompositionCheck, SeeThroughError> {
        validate_psd(&psd)?;
        self.wait_until_running().await?;
        if !self.serves("check").await? {
            return Ok(DecompositionCheck::unchecked());
        }
        let path = self
            .upload_file(psd, "decomposition.psd", "image/vnd.adobe.photoshop")
            .await?;
        let file = json!({
            "path": path,
            "orig_name": "decomposition.psd",
            "meta": { "_type": "gradio.FileData" }
        });
        let event_id = self.start("check", json!([file])).await?;
        let data = self.await_data("check", &event_id, REQUEST_TIMEOUT).await?;
        data.as_array()
            .and_then(|values| values.first())
            .cloned()
            .and_then(|check| serde_json::from_value(check).ok())
            .ok_or_else(|| {
                SeeThroughError::InvalidOutput(
                    "See-through returned an invalid decomposition check".to_string(),
                )
            })
    }

    /// The Space's `check_turns`: which generated turns redraw the character
    /// beyond what keys of the front drawing can follow, so are worth drawing
    /// again before they are decomposed. `pictures` are the front drawing and
    /// the turned ones (right, left, raised, lowered). A Space without the
    /// endpoint finds none bad.
    pub async fn check_turned_pictures(
        &self,
        front: Vec<u8>,
        pictures: [(Vec<u8>, String); 5],
    ) -> Result<TurnedPicturesCheck, SeeThroughError> {
        validate_psd(&front)?;
        self.wait_until_running().await?;
        if !self.serves("check_turns").await? {
            return Ok(TurnedPicturesCheck::default());
        }
        let path = self
            .upload_file(front, "decomposition-0.psd", "image/vnd.adobe.photoshop")
            .await?;
        let mut files = vec![json!({
            "path": path,
            "orig_name": "decomposition-0.psd",
            "meta": { "_type": "gradio.FileData" }
        })];
        for (index, (picture, media_type)) in pictures.into_iter().enumerate() {
            let name = format!(
                "picture-{index}.{}",
                image_extension(&media_type).unwrap_or("png")
            );
            let path = self.upload_file(picture, &name, &media_type).await?;
            files.push(json!({
                "path": path,
                "orig_name": name,
                "meta": { "_type": "gradio.FileData" }
            }));
        }
        let event_id = self.start("check_turns", Value::Array(files)).await?;
        let data = self
            .await_data("check_turns", &event_id, REQUEST_TIMEOUT)
            .await?;
        data.as_array()
            .and_then(|values| values.first())
            .cloned()
            .and_then(|check| serde_json::from_value(check).ok())
            .ok_or_else(|| {
                SeeThroughError::InvalidOutput(
                    "See-through returned an invalid turned pictures check".to_string(),
                )
            })
    }

    /// The Space's `figure_head`: where a standing figure's head is, framed
    /// as a bust (from its decomposition), and that crop of the picture at a
    /// bust's size, to key as a bust. None when the Space has no such endpoint
    /// or the head is big enough already.
    pub async fn figure_head(
        &self,
        figure: Vec<u8>,
        (picture, media_type): (Vec<u8>, String),
    ) -> Result<Option<FigureHead>, SeeThroughError> {
        validate_psd(&figure)?;
        self.wait_until_running().await?;
        if !self.serves("figure_head").await? {
            return Ok(None);
        }
        let figure = self
            .upload_file(figure, "figure.psd", "image/vnd.adobe.photoshop")
            .await?;
        let name = format!("figure.{}", image_extension(&media_type).unwrap_or("png"));
        let picture = self.upload_file(picture, &name, &media_type).await?;
        let files = json!([
            { "path": figure, "orig_name": "figure.psd", "meta": { "_type": "gradio.FileData" } },
            { "path": picture, "orig_name": name, "meta": { "_type": "gradio.FileData" } },
        ]);
        let event_id = self.start("figure_head", files).await?;
        let data = self
            .await_data("figure_head", &event_id, REQUEST_TIMEOUT)
            .await?;
        let found = data
            .as_array()
            .and_then(|values| values.first())
            .and_then(|found| found.get("box"))
            .cloned()
            .unwrap_or(Value::Null);
        if found.is_null() {
            return Ok(None);
        }
        let crop_box: [i32; 4] = serde_json::from_value(found).map_err(|_| {
            SeeThroughError::InvalidOutput("See-through returned an invalid head box".to_string())
        })?;
        let crop_url = output_file_url_at(&self.space, &data, 1)?;
        let crop = self
            .download(crop_url, "head picture download", MAX_IMAGE_BYTES)
            .await?;
        Ok(Some(FigureHead { crop_box, crop }))
    }

    /// The Space's `figure_keys`: turn keys fitted on a standing figure's head
    /// framed as a bust (`figure_head`), placed on the figure's canvas to move
    /// the figure's own head parts.
    pub async fn figure_keys(
        &self,
        (picture, media_type): (Vec<u8>, String),
        figure: Vec<u8>,
        head_keyforms: Vec<u8>,
        crop_box: [i32; 4],
    ) -> Result<Vec<u8>, SeeThroughError> {
        validate_psd(&figure)?;
        self.wait_until_running().await?;
        let name = format!("figure.{}", image_extension(&media_type).unwrap_or("png"));
        let picture = self.upload_file(picture, &name, &media_type).await?;
        let figure = self
            .upload_file(figure, "figure.psd", "image/vnd.adobe.photoshop")
            .await?;
        let keys = self
            .upload_file(head_keyforms, "keyforms.json", "application/json")
            .await?;
        let [x0, y0, x1, y1] = crop_box;
        let data = json!([
            { "path": picture, "orig_name": name, "meta": { "_type": "gradio.FileData" } },
            { "path": figure, "orig_name": "figure.psd", "meta": { "_type": "gradio.FileData" } },
            { "path": keys, "orig_name": "keyforms.json", "meta": { "_type": "gradio.FileData" } },
            format!("{x0},{y0},{x1},{y1}"),
        ]);
        let event_id = self.start("figure_keys", data).await?;
        let data = self
            .await_data("figure_keys", &event_id, REQUEST_TIMEOUT)
            .await?;
        let keys_url = output_file_url_at(&self.space, &data, 0)?;
        let keyforms = self
            .download(keys_url, "keyforms download", MAX_KEYFORMS_JSON_BYTES)
            .await?;
        serde_json::from_slice::<Value>(&keyforms).map_err(|_| {
            SeeThroughError::InvalidOutput("See-through returned invalid keyforms".to_string())
        })?;
        Ok(keyforms)
    }

    /// Wakes the Space if it sleeps and waits while it builds or starts. A
    /// stage the Hub will not tell (a private Space this token cannot read)
    /// is left to the calls that follow.
    async fn wait_until_running(&self) -> Result<(), SeeThroughError> {
        let deadline = tokio::time::Instant::now() + WAKE_TIMEOUT;
        let mut woken = false;
        loop {
            let Some(stage) = self.stage().await else {
                return Ok(());
            };
            match readiness(&stage) {
                Readiness::Running => return Ok(()),
                Readiness::Unavailable => return Err(SeeThroughError::SpaceUnavailable(stage)),
                Readiness::Asleep if !woken => {
                    woken = true;
                    tracing::info!(space = self.space.name(), "waking the See-through Space");
                    // Any request to a sleeping Space starts it; the answer does not matter.
                    let _ = self
                        .authenticated(self.client.get(self.space.url("/")))
                        .timeout(Duration::from_secs(30))
                        .send()
                        .await;
                }
                Readiness::Asleep | Readiness::Starting => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(SeeThroughError::Waking);
            }
            tokio::time::sleep(WAKE_POLL).await;
        }
    }

    async fn stage(&self) -> Option<String> {
        let response = self
            .authenticated(self.client.get(self.space.runtime_url()))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = crate::services::outbound_security::read_limited_body(
            response,
            MAX_CONTROL_RESPONSE_BYTES,
        )
        .await
        .ok()?;
        let runtime: Value = serde_json::from_slice(&body).ok()?;
        runtime
            .get("stage")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// Whether the Space serves `decompose`, Myriad's canvas endpoint.
    async fn serves_canvas(&self) -> Result<bool, SeeThroughError> {
        self.serves("decompose").await
    }

    /// Whether the Space serves the named endpoint.
    async fn serves(&self, endpoint: &str) -> Result<bool, SeeThroughError> {
        let response = self
            .authenticated(self.client.get(self.space.url("/gradio_api/info")))
            .send()
            .await
            .map_err(transport_error)?;
        let body =
            successful_body(response, "endpoint listing", MAX_CONTROL_RESPONSE_BYTES).await?;
        let info: Value = serde_json::from_slice(&body).map_err(|_| {
            SeeThroughError::InvalidOutput("See-through endpoint listing is invalid".to_string())
        })?;
        Ok(serves_endpoint(&info, endpoint))
    }

    async fn upload(
        &self,
        image: axum::body::Bytes,
        media_type: &str,
    ) -> Result<String, SeeThroughError> {
        let extension = image_extension(media_type).ok_or_else(|| {
            SeeThroughError::InvalidInput(
                "See-through accepts PNG, JPEG, or WebP portraits".to_string(),
            )
        })?;
        // Hand the shared buffer to the request body; no copy of the portrait.
        let length = image.len() as u64;
        let part = multipart::Part::stream_with_length(reqwest::Body::from(image), length)
            .file_name(format!("character.{extension}"))
            .mime_str(media_type)
            .map_err(|error| SeeThroughError::InvalidInput(error.to_string()))?;
        let response = self
            .authenticated(self.client.post(self.space.url("/gradio_api/upload")))
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

    async fn upload_file(
        &self,
        bytes: Vec<u8>,
        name: &str,
        media_type: &str,
    ) -> Result<String, SeeThroughError> {
        let length = bytes.len() as u64;
        let part = multipart::Part::stream_with_length(reqwest::Body::from(bytes), length)
            .file_name(name.to_string())
            .mime_str(media_type)
            .map_err(|error| SeeThroughError::InvalidInput(error.to_string()))?;
        let response = self
            .authenticated(self.client.post(self.space.url("/gradio_api/upload")))
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

    /// The finished call's output values, waiting up to `timeout` for them.
    async fn await_data(
        &self,
        endpoint: &str,
        event_id: &str,
        timeout: Duration,
    ) -> Result<Value, SeeThroughError> {
        let response = self
            .authenticated(
                self.client.get(
                    self.space
                        .url(&format!("/gradio_api/call/{endpoint}/{event_id}")),
                ),
            )
            .timeout(timeout)
            .send()
            .await
            .map_err(transport_error)?;
        let body = successful_body(response, "event stream", MAX_CONTROL_RESPONSE_BYTES).await?;
        match parse_terminal_event(&body)? {
            TerminalEvent::Complete(data) => Ok(data),
            TerminalEvent::Error => Err(SeeThroughError::Rejected),
        }
    }

    async fn download(
        &self,
        url: Url,
        stage: &'static str,
        limit: usize,
    ) -> Result<Vec<u8>, SeeThroughError> {
        validate_output_url(&self.space, &url)?;
        let response = self
            .authenticated(self.client.get(url))
            .send()
            .await
            .map_err(transport_error)?;
        successful_body(response, stage, limit).await
    }

    async fn start(&self, endpoint: &str, data: Value) -> Result<String, SeeThroughError> {
        let response = self
            .authenticated(
                self.client
                    .post(self.space.url(&format!("/gradio_api/call/{endpoint}"))),
            )
            .json(&json!({ "data": data }))
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

    async fn await_output(&self, endpoint: &str, event_id: &str) -> Result<Url, SeeThroughError> {
        let response = self
            .authenticated(
                self.client.get(
                    self.space
                        .url(&format!("/gradio_api/call/{endpoint}/{event_id}")),
                ),
            )
            .send()
            .await
            .map_err(transport_error)?;
        let body = successful_body(response, "event stream", MAX_CONTROL_RESPONSE_BYTES).await?;
        let terminal = parse_terminal_event(&body)?;
        match terminal {
            TerminalEvent::Complete(data) => output_file_url(&self.space, &data),
            TerminalEvent::Error => Err(SeeThroughError::Rejected),
        }
    }

    async fn download_psd(&self, url: Url) -> Result<Vec<u8>, SeeThroughError> {
        validate_output_url(&self.space, &url)?;
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

/// Turn keys measured on the Space, and the front decomposition they go with.
pub struct TurnKeyformsOutput {
    /// `{ canvas: [w, h], keyforms: { family: { plus, minus, up?, down? } }, fit, baked }`.
    pub keyforms: Vec<u8>,
    pub psd: Vec<u8>,
}

/// A standing figure's head framed as a bust (figure_head).
pub struct FigureHead {
    /// x0, y0, x1, y1 in the figure picture's pixels; may run past its edges.
    pub crop_box: [i32; 4],
    /// That box of the picture at a bust's size (PNG), white past the edges.
    pub crop: Vec<u8>,
}

/// Which generated turns to draw again before decomposing them.
#[derive(Debug, Default, Deserialize)]
pub struct TurnedPicturesCheck {
    /// The sides ("plus", "minus", "up", "down") to draw again.
    #[serde(default)]
    pub bad: Vec<String>,
    /// Per side, how far a smooth warp of the front drawing stays from it (px).
    #[serde(default)]
    pub residual: std::collections::BTreeMap<String, f64>,
}

/// Whether the fit can key from a decomposition, and why not.
#[derive(Debug, Deserialize)]
pub struct DecompositionCheck {
    pub ok: bool,
    #[serde(default)]
    pub faults: Vec<String>,
    /// How far past its limits the decomposition is at worst: over 1 when faulty.
    #[serde(default)]
    pub badness: Option<f64>,
}

impl DecompositionCheck {
    fn unchecked() -> Self {
        Self {
            ok: true,
            faults: Vec::new(),
            badness: None,
        }
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

fn image_extension(media_type: &str) -> Option<&'static str> {
    match media_type {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
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

/// Whether a Gradio `/gradio_api/info` listing names `endpoint`.
fn serves_endpoint(info: &Value, endpoint: &str) -> bool {
    info.get("named_endpoints")
        .and_then(Value::as_object)
        .is_some_and(|named| named.contains_key(&format!("/{endpoint}")))
}

fn output_file_url(space: &Space, data: &Value) -> Result<Url, SeeThroughError> {
    output_file_url_at(space, data, 0)
}

fn output_file_url_at(space: &Space, data: &Value, index: usize) -> Result<Url, SeeThroughError> {
    let file = data
        .as_array()
        .and_then(|values| values.get(index))
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
        Url::parse(&space.url(&format!("/gradio_api/file={path}"))).map_err(|_| {
            SeeThroughError::InvalidOutput("See-through returned an invalid PSD path".to_string())
        })?
    } else {
        return Err(SeeThroughError::InvalidOutput(
            "See-through result did not expose a PSD download".to_string(),
        ));
    };
    validate_output_url(space, &url)?;
    Ok(url)
}

fn validate_output_url(space: &Space, url: &Url) -> Result<(), SeeThroughError> {
    let valid = url.scheme() == "https"
        && url.host_str() == space.base.host_str()
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

    /// The public demo Myriad's Space was copied from: square `inference` only.
    const PUBLIC_DEMO_SPACE: &str = "24yearsold/see-through-demo";

    #[test]
    fn validates_space_options_and_token_shape() {
        assert_eq!(DecomposeOptions::default().resolution, 1280);
        assert!(DecomposeOptions::default().validate().is_ok());
        assert!(
            DecomposeOptions {
                resolution: 800,
                ..DecomposeOptions::default()
            }
            .validate()
            .is_err()
        );
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
        let demo = Space::parse(PUBLIC_DEMO_SPACE).unwrap();
        let url = output_file_url(&demo, &data).unwrap();
        assert_eq!(url.host_str(), Some("24yearsold-see-through-demo.hf.space"));

        assert!(matches!(
            parse_terminal_event(b"event: error\ndata: null\n\n").unwrap(),
            TerminalEvent::Error
        ));
    }

    #[test]
    fn output_download_is_pinned_to_the_space() {
        let demo = Space::parse(PUBLIC_DEMO_SPACE).unwrap();
        let malicious = json!([{
            "url": "https://attacker.example/gradio_api/file=/tmp/gradio/a/output.psd"
        }]);
        assert!(output_file_url(&demo, &malicious).is_err());
        // Another Space's file is not this Space's output either.
        let other = json!([{
            "url": "https://somekawahitomi-see-through-demo.hf.space/gradio_api/file=/tmp/gradio/a/output.psd"
        }]);
        assert!(output_file_url(&demo, &other).is_err());
        let own = Space::parse(DEFAULT_SPACE).unwrap();
        assert!(output_file_url(&own, &other).is_ok());
        assert!(validate_remote_temp_path("/tmp/gradio/a/input.png").is_ok());
        assert!(validate_remote_temp_path("/tmp/gradio/../secret").is_err());
    }

    #[test]
    fn a_space_id_names_its_host() {
        let own = Space::parse(" SomekawaHitomi/see-through-demo ").unwrap();
        assert_eq!(own.name(), "SomekawaHitomi/see-through-demo");
        assert_eq!(
            own.url("/gradio_api/info"),
            "https://somekawahitomi-see-through-demo.hf.space/gradio_api/info"
        );
        assert_eq!(
            Space::parse("Some_One/see.through").unwrap().url(""),
            "https://some-one-see-through.hf.space"
        );
        for invalid in [
            "",
            "no-slash",
            "a/b/c",
            "owner/",
            "/name",
            "evil.com/x?y",
            "a/-b",
        ] {
            assert!(Space::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn the_canvas_endpoint_is_used_only_where_the_space_serves_it() {
        let demo = json!({ "named_endpoints": { "/inference": {} }, "unnamed_endpoints": {} });
        let own =
            json!({ "named_endpoints": { "/inference": {}, "/decompose": {}, "/refine": {} } });
        assert!(!serves_endpoint(&demo, "decompose"));
        assert!(serves_endpoint(&own, "decompose"));
        assert!(!serves_endpoint(&json!({}), "decompose"));
    }

    #[test]
    fn the_turned_pictures_check_reads_as_the_space_answers_it() {
        let answer = json!({
            "residual": { "plus": 4.3, "minus": 2.09, "up": 3.36, "down": 2.22 },
            "bad": ["plus"]
        });
        let check: TurnedPicturesCheck = serde_json::from_value(answer).unwrap();
        assert_eq!(check.bad, ["plus"]);
        assert_eq!(check.residual["minus"], 2.09);
        // A Space without the check finds none bad.
        assert!(TurnedPicturesCheck::default().bad.is_empty());
    }

    #[test]
    fn a_sleeping_space_is_woken_and_a_starting_one_waited_for() {
        assert_eq!(readiness("RUNNING"), Readiness::Running);
        assert_eq!(readiness("SLEEPING"), Readiness::Asleep);
        for starting in [
            "BUILDING",
            "RUNNING_BUILDING",
            "APP_STARTING",
            "RUNNING_APP_STARTING",
        ] {
            assert_eq!(readiness(starting), Readiness::Starting, "{starting}");
        }
        for broken in [
            "PAUSED",
            "BUILD_ERROR",
            "RUNTIME_ERROR",
            "CONFIG_ERROR",
            "NO_APP_FILE",
            "DELETING",
        ] {
            assert_eq!(readiness(broken), Readiness::Unavailable, "{broken}");
        }
        assert_eq!(
            Space::parse(DEFAULT_SPACE).unwrap().runtime_url(),
            "https://huggingface.co/api/spaces/SomekawaHitomi/see-through-demo/runtime"
        );
    }

    #[test]
    fn validates_psd_signature() {
        let mut bytes = vec![0_u8; 26];
        bytes[..6].copy_from_slice(b"8BPS\0\x01");
        assert!(validate_psd(&bytes).is_ok());
        assert!(validate_psd(b"not a psd").is_err());
    }
}
