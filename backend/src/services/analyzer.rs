// AI analysis service using Google Gemini API or OpenAI-compatible API
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

use crate::services::http_client::{GeminiApiUrl, ProxyConfig};

// Gemini API Structures
#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    /// Only set for structured-output requests; omitted otherwise so ordinary
    /// calls keep their exact previous request body.
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: String,
    #[serde(rename = "responseSchema", skip_serializing_if = "Option::is_none")]
    response_schema: Option<serde_json::Value>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default, rename = "promptFeedback")]
    prompt_feedback: Option<GeminiPromptFeedback>,
}

#[derive(Debug, Deserialize)]
struct GeminiPromptFeedback {
    #[serde(default, rename = "blockReason")]
    block_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiCandidateContent>,
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidateContent {
    #[serde(default)]
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    #[serde(default)]
    text: String,
}

// OpenAI-compatible API Structures
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    /// Only set for structured-output requests; omitted otherwise so ordinary
    /// calls keep their exact previous request body.
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
    /// Only set for bounded calls; see [`OutputBudget`].
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// 一次「答案很短」的调用的输出上限。
///
/// **这里没有「关掉思考」的参数，是有意的。** 各家的开关不是同一个东西：
/// OpenRouter 用 `reasoning: { enabled: false }`，OpenAI 用 `reasoning_effort`
/// （取值随模型代际变，GPT-5 的 `minimal` 在别处不成立），Gemini 用
/// `thinkingConfig.thinkingBudget`（而 `0` 只在允许关闭的型号上合法），火山
/// 用 `thinking: { type: "disabled" }`。
///
/// 发一个猜来的参数比不发更糟：网关不认就是 4xx，触发
/// [`AiAnalyzer::analyze_json_short`] 的重试阶梯，每次多跑一个来回——本来是
/// 为了变快，结果更慢，而且思考照样没关掉。要做就得按 `base_url` 认出具体
/// 网关再发对应的那一个（`openai_chat_completions_url` 已有这么认的先例）。
#[derive(Debug, Clone, Copy)]
pub struct OutputBudget {
    /// 输出上限。要留得下一整轮思考——多数网关把思考 token 也算进这个额度。
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

/// OpenAI-compatible chat role/content pair for multi-turn analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    #[allow(dead_code)]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    #[serde(default)]
    choices: Vec<OpenAIChoice>,
    /// OpenRouter and some gateways return HTTP 200 with a top-level error object.
    #[serde(default)]
    error: Option<OpenAIErrorBody>,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorBody {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    #[serde(default)]
    message: OpenAIResponseMessage,
}

#[derive(Debug, Deserialize, Default)]
struct OpenAIResponseMessage {
    /// Providers may return `null` for content when only reasoning is filled.
    #[serde(default)]
    content: Option<String>,
    /// OpenRouter / DeepSeek-style reasoning fields (optional, never required).
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

/// Format a failed OpenAI-compatible HTTP response for user-facing errors.
///
/// The transport is OpenAI-compatible (OpenRouter / DeepSeek / custom gateways),
/// not necessarily official OpenAI — keep the wording accurate so region blocks
/// and wrong models are not misread as "using OpenAI".
fn format_openai_compatible_http_error(
    status: reqwest::StatusCode,
    endpoint: &str,
    model: &str,
    body: &str,
) -> String {
    let body = body.trim();
    let region_blocked = body
        .to_ascii_lowercase()
        .contains("not available in your region")
        || body.contains("\"code\":403")
        || body.contains("\"code\": 403");

    let mut msg = format!(
        "OpenAI-compatible API error {status} (endpoint: {endpoint}, model: {model}): {body}"
    );

    if region_blocked {
        msg.push_str(
            " — this is a provider geo-restriction on the server egress IP (common with OpenRouter for Claude / Grok / GPT / Gemini), not a wrong provider switch. Switch to a region-available model, or enable an outbound proxy in settings.",
        );
    }

    msg
}

/// Extract assistant text from an OpenAI-compatible chat completion body.
///
/// Never panics: returns clear anyhow errors for playground 502 paths.
fn extract_openai_completion_text(response: &OpenAIResponse) -> Result<String> {
    if let Some(error) = &response.error {
        let message = error
            .message
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("OpenAI-compatible provider returned an error object");
        let code = error
            .code
            .as_ref()
            .map(|c| match c {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .filter(|s| !s.is_empty() && s != "null");
        return match code {
            Some(code) => Err(anyhow::anyhow!(
                "OpenAI-compatible API error (code {code}): {message}"
            )),
            None => Err(anyhow::anyhow!("OpenAI-compatible API error: {message}")),
        };
    }

    let message = response
        .choices
        .first()
        .map(|choice| &choice.message)
        .ok_or_else(|| anyhow::anyhow!("OpenAI-compatible API returned no choices"))?;

    if let Some(content) = message
        .content
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Ok(content.to_string());
    }

    // Prefer non-empty content; fall back to common reasoning fields when
    // providers put the payload only in reasoning_* (still never panic).
    for candidate in [
        message.reasoning_content.as_deref(),
        message.reasoning.as_deref(),
    ] {
        if let Some(text) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(text.to_string());
        }
    }

    Err(anyhow::anyhow!(
        "OpenAI-compatible API returned empty message content"
    ))
}

/// One piece of a streamed completion: the visible reply, or the hidden chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamDelta {
    Text(String),
    Reasoning(String),
}

fn json_token(value: &serde_json::Value) -> Option<&str> {
    value.as_str().filter(|s| !s.is_empty())
}

/// OpenAI-compatible chat.completion.chunk → text / reasoning deltas.
///
/// Grok / DeepSeek / OpenRouter put the thinking trace on `delta.reasoning_content`
/// (sometimes `delta.reasoning`). The visible answer stays on `delta.content`.
fn reasoning_text_from_value(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = json_token(value) {
        return Some(text.to_string());
    }
    if let Some(obj) = value.as_object() {
        if let Some(text) = json_token(&obj["content"]).or_else(|| json_token(&obj["text"])) {
            return Some(text.to_string());
        }
    }
    None
}

pub fn openai_stream_deltas(json: &serde_json::Value) -> Vec<StreamDelta> {
    if let Some(kind) = json.get("type").and_then(|v| v.as_str()) {
        if kind == "response.reasoning_text.delta"
            || kind == "response.reasoning_summary_text.delta"
        {
            if let Some(text) = reasoning_text_from_value(&json["delta"]) {
                return vec![StreamDelta::Reasoning(text)];
            }
        }
        if kind == "response.output_text.delta" {
            if let Some(text) = json_token(&json["delta"]) {
                return vec![StreamDelta::Text(text.to_string())];
            }
        }
    }

    let Some(delta) = json.pointer("/choices/0/delta") else {
        return Vec::new();
    };
    let mut out = Vec::new();

    if let Some(text) = reasoning_text_from_value(&delta["reasoning_content"])
        .or_else(|| reasoning_text_from_value(&delta["reasoning"]))
    {
        out.push(StreamDelta::Reasoning(text));
    } else if let Some(details) = delta.get("reasoning_details").and_then(|v| v.as_array()) {
        let mut joined = String::new();
        for item in details {
            if let Some(text) = json_token(&item["text"]).or_else(|| json_token(&item["content"])) {
                joined.push_str(text);
            }
        }
        if !joined.is_empty() {
            out.push(StreamDelta::Reasoning(joined));
        }
    }

    if let Some(text) = json_token(&delta["content"]) {
        out.push(StreamDelta::Text(text.to_string()));
    }
    out
}

async fn consume_openai_sse<F, Fut>(
    mut response: reqwest::Response,
    mut on_delta: F,
) -> Result<String>
where
    F: FnMut(StreamDelta) -> Fut + Send,
    Fut: Future<Output = bool> + Send,
{
    let mut full_text = String::new();
    let mut buffer = String::new();
    loop {
        let chunk = response.chunk().await.context("Stream read error")?;
        match chunk {
            Some(bytes) => buffer.push_str(&String::from_utf8_lossy(&bytes)),
            None => break,
        }
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                return Ok(full_text);
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            for delta in openai_stream_deltas(&json) {
                if let StreamDelta::Text(ref content) = delta {
                    full_text.push_str(content);
                }
                if !on_delta(delta).await {
                    return Ok(full_text);
                }
                tokio::task::yield_now().await;
            }
        }
    }
    Ok(full_text)
}

/// Gemini SSE chunk → text / thought-part deltas.
pub fn gemini_stream_deltas(json: &serde_json::Value) -> Vec<StreamDelta> {
    let Some(parts) = json
        .pointer("/candidates/0/content/parts")
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for part in parts {
        let Some(text) = json_token(&part["text"]) else {
            continue;
        };
        if part.get("thought").and_then(|v| v.as_bool()) == Some(true) {
            out.push(StreamDelta::Reasoning(text.to_string()));
        } else {
            out.push(StreamDelta::Text(text.to_string()));
        }
    }
    out
}

// AI Provider Enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    Gemini,
    OpenAI,
}

impl AiProvider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" | "openrouter" => Self::OpenAI,
            _ => Self::Gemini,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Gemini => "gemini",
        }
    }
}

pub struct AiAnalyzer {
    client: Client,
    provider: AiProvider,
    api_key: String,
    model: String,
    base_url: Option<String>, // For OpenAI-compatible APIs
}

/// 对面是哪一家网关。
///
/// `AiProvider::from_str` 把 `"openai"` 和 `"openrouter"` 压成同一个值，身份在
/// 那一步就丢了，analyzer 手里只剩 `base_url`。而结构化输出的支持度、关思考的
/// 参数名，都是按网关分的——所以这里把身份从 base_url 认回来
/// （`openai_chat_completions_url` 早就在按 base_url 认 openrouter）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gateway {
    OpenAi,
    OpenRouter,
    Gemini,
    /// 自建或未知的 OpenAI 兼容端点。只用共通参数，不发任何一家的方言。
    OpenAiCompatible,
}

impl Gateway {
    /// 这一家「别思考」怎么说。`None` = 不确定就什么都不发。
    ///
    /// 只写有把握的那一个：OpenRouter 的统一参数是
    /// `reasoning: { enabled: false }`，而它也是本仓库的默认出口
    /// （默认 Lite 模型 `openai/gpt-oss-20b:free` 就挂在这儿）。
    ///
    /// 其余三家**故意留空**：
    /// - OpenAI 用 `reasoning_effort`，取值随模型代际变（GPT-5 的 `minimal`
    ///   在 o 系上不成立），发错就是 4xx；
    /// - Gemini 用 `thinkingConfig.thinkingBudget`，而 `0` 只在允许关闭的
    ///   型号上合法，Pro 系的下限不是 0；
    /// - 自建端点根本不知道后面是谁。
    ///
    /// 这些留空是 `uncertain`，不是「不需要」。要补的话补在这里，一处即可。
    fn thinking_off(self) -> Option<(&'static str, serde_json::Value)> {
        match self {
            Self::OpenRouter => Some(("reasoning", serde_json::json!({ "enabled": false }))),
            Self::OpenAi | Self::Gemini | Self::OpenAiCompatible => None,
        }
    }
}

fn gateway_of(provider: AiProvider, base_url: Option<&str>) -> Gateway {
    if provider == AiProvider::Gemini {
        return Gateway::Gemini;
    }
    let host = base_url
        .map(str::trim)
        .map(|url| {
            url.trim_start_matches("https://")
                .trim_start_matches("http://")
        })
        .unwrap_or("");
    if host.starts_with("openrouter.ai") {
        Gateway::OpenRouter
    } else if host.is_empty() || host.starts_with("api.openai.com") {
        Gateway::OpenAi
    } else {
        Gateway::OpenAiCompatible
    }
}

/// 记住某个 (端点, 模型) 拒过结构化输出。
///
/// 阶梯本身没错，错在它没有记忆：不支持 `json_schema` 的网关上，每一次调用都
/// 要先被拒一次再降级——等于给最弱的那批网关加了一笔常驻的往返税。记下来之后
/// 这笔钱只付一次。
///
/// 只记「请求形状被拒」（4xx，且不含鉴权和限流，见
/// [`ProviderCallFailure::rejected_request`]）。进程内有效：配置换了、网关升级
/// 了，重启就重新试一次，不需要另造失效机制。
static SHAPE_REFUSED: OnceLock<std::sync::RwLock<HashSet<String>>> = OnceLock::new();

fn shape_memo() -> &'static std::sync::RwLock<HashSet<String>> {
    SHAPE_REFUSED.get_or_init(Default::default)
}

/// 记忆分两类，因为它们是两组不同的参数，网关可能只拒其中一组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestShape {
    /// `response_format` / `responseSchema`
    StructuredOutput,
    /// 输出上限 + 那一家的「别思考」参数
    Extras,
}

impl RequestShape {
    fn tag(self) -> &'static str {
        match self {
            Self::StructuredOutput => "structured",
            Self::Extras => "extras",
        }
    }
}

fn require_analyze_prompt(profile_data: &serde_json::Value) -> Result<&str> {
    profile_data
        .get("prompt")
        .and_then(|p| p.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("analyze_profile requires a non-empty prompt"))
}

/// Flatten multi-turn messages into a single Gemini-compatible prompt.
fn flatten_messages_for_gemini(system: &str, messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();
    if !system.trim().is_empty() {
        parts.push(format!("SYSTEM:\n{system}"));
    }
    for message in messages {
        let label = match message.role.as_str() {
            "assistant" => "ASSISTANT",
            "system" => "SYSTEM",
            _ => "USER",
        };
        parts.push(format!("{label}:\n{}", message.content));
    }
    parts.join("\n\n")
}

/// How a JSON request is expressed to the provider.
#[derive(Clone, Copy)]
enum JsonMode<'a> {
    /// Use the provider's native structured-output parameters.
    Structured(Option<&'a serde_json::Value>),
    /// Compatibility fallback for endpoints that reject those parameters: no
    /// structured-output fields, with the JSON contract restated in the prompt
    /// the way the pre-existing call sites did it.
    PromptOnly(Option<&'a serde_json::Value>),
}

impl JsonMode<'_> {
    fn decorate_prompt(self, prompt: &str) -> String {
        let Self::PromptOnly(schema) = self else {
            return prompt.to_string();
        };
        let mut text = prompt.to_string();
        text.push_str("\n\nReturn one valid JSON value only, without Markdown fences.");
        if let Some(schema) = schema {
            text.push_str(" The JSON value must satisfy this schema:\n");
            text.push_str(&schema.to_string());
        }
        text
    }

    fn gemini_generation_config(
        self,
        budget: Option<OutputBudget>,
    ) -> Option<GeminiGenerationConfig> {
        let structured = match self {
            Self::Structured(schema) => Some(schema.and_then(gemini_response_schema)),
            Self::PromptOnly(_) => None,
        };
        // 关思考不依赖结构化输出：退回 prompt-only 时预算还在。
        match (structured, budget) {
            (None, None) => None,
            (structured, budget) => Some(GeminiGenerationConfig {
                response_mime_type: if structured.is_some() {
                    "application/json".to_string()
                } else {
                    "text/plain".to_string()
                },
                response_schema: structured.flatten(),
                max_output_tokens: budget.map(|b| b.max_tokens),
            }),
        }
    }

    fn openai_response_format(self, schema_name: &str) -> Option<serde_json::Value> {
        match self {
            // `strict` stays false: strict mode forbids free-form objects, and
            // planner step `params` is exactly that. Non-strict still guarantees
            // syntactically valid JSON and guides the shape.
            Self::Structured(Some(schema)) => Some(serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name,
                    "schema": schema,
                    "strict": false,
                },
            })),
            Self::Structured(None) => Some(serde_json::json!({ "type": "json_object" })),
            Self::PromptOnly(_) => None,
        }
    }
}

/// A failed provider call, keeping the HTTP status so the caller can tell a
/// rejected request shape from a transport or upstream failure.
struct ProviderCallFailure {
    status: Option<reqwest::StatusCode>,
    error: anyhow::Error,
}

impl ProviderCallFailure {
    fn http(status: reqwest::StatusCode, error: anyhow::Error) -> Self {
        Self {
            status: Some(status),
            error,
        }
    }

    fn transport(error: anyhow::Error) -> Self {
        Self {
            status: None,
            error,
        }
    }

    /// Whether the endpoint refused the request as written. Structured-output
    /// parameters are the only thing that path adds, so a retry without them is
    /// worth one round trip. Auth and rate-limit failures are excluded — they
    /// would fail identically the second time.
    fn rejected_request(&self) -> bool {
        self.status.is_some_and(|status| {
            status.is_client_error()
                && !matches!(
                    status,
                    reqwest::StatusCode::UNAUTHORIZED
                        | reqwest::StatusCode::FORBIDDEN
                        | reqwest::StatusCode::TOO_MANY_REQUESTS
                )
        })
    }
}

/// Keys Gemini accepts inside `responseSchema` (an OpenAPI 3.0 subset).
/// Anything else — `additionalProperties`, `$schema`, `default`, `const` — is
/// rejected with a 400, so unknown keys are dropped rather than forwarded.
const GEMINI_SCHEMA_KEYS: &[&str] = &[
    "type",
    "format",
    "description",
    "nullable",
    "enum",
    "items",
    "properties",
    "required",
    "minItems",
    "maxItems",
];

/// Translate a JSON Schema into Gemini's `responseSchema` dialect.
///
/// Returns `None` when the schema cannot be expressed, in which case the caller
/// keeps `responseMimeType` (still guaranteeing valid JSON) and drops the
/// schema. The main inexpressible case is a free-form object: Gemini rejects an
/// `OBJECT` node without `properties`, which is how open maps like a planner
/// step's `params` are declared.
fn gemini_response_schema(schema: &serde_json::Value) -> Option<serde_json::Value> {
    let map = schema.as_object()?;

    let declared_type = map.get("type").and_then(serde_json::Value::as_str);
    if declared_type == Some("object")
        && map
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_none_or(|properties| properties.is_empty())
    {
        return None;
    }

    let mut translated = serde_json::Map::new();
    for (key, value) in map {
        if !GEMINI_SCHEMA_KEYS.contains(&key.as_str()) {
            continue;
        }
        let translated_value = match key.as_str() {
            // Gemini's Type enum is upper case (STRING / OBJECT / ARRAY / ...).
            "type" => serde_json::Value::String(value.as_str()?.to_uppercase()),
            "items" => gemini_response_schema(value)?,
            "properties" => {
                let mut properties = serde_json::Map::new();
                for (name, property) in value.as_object()? {
                    properties.insert(name.clone(), gemini_response_schema(property)?);
                }
                serde_json::Value::Object(properties)
            }
            _ => value.clone(),
        };
        translated.insert(key.clone(), translated_value);
    }
    Some(serde_json::Value::Object(translated))
}

pub(crate) fn openai_chat_completions_url(base_url: Option<&str>) -> String {
    let base_url = base_url
        .unwrap_or("https://api.openai.com/v1")
        .trim()
        .trim_end_matches('/');

    if base_url.ends_with("/chat/completions") {
        return base_url.to_string();
    }

    // OpenRouter documents its OpenAI-compatible API under /api/v1. Accepting
    // the site root here makes a common settings mistake safe without changing
    // the semantics of custom OpenAI-compatible endpoints.
    if matches!(base_url, "https://openrouter.ai" | "http://openrouter.ai") {
        return format!("{base_url}/api/v1/chat/completions");
    }

    // The official OpenAI host is the other common root-only value.
    if matches!(base_url, "https://api.openai.com" | "http://api.openai.com") {
        return format!("{base_url}/v1/chat/completions");
    }

    format!("{base_url}/chat/completions")
}

impl AiAnalyzer {
    /// 对面是哪一家网关。
    pub fn gateway(&self) -> Gateway {
        gateway_of(self.provider, self.base_url.as_deref())
    }

    /// 记忆的键。同一个网关上不同模型的结构化输出支持度可以不一样
    /// （OpenRouter 后面挂着几十家），所以端点和模型都要进键。
    fn capability_key(&self) -> String {
        format!("{}|{}", self.base_url.as_deref().unwrap_or(""), self.model)
    }

    fn refused(&self, shape: RequestShape) -> bool {
        let key = format!("{}#{}", self.capability_key(), shape.tag());
        shape_memo().read().is_ok_and(|seen| seen.contains(&key))
    }

    fn remember_refusal(&self, shape: RequestShape) {
        let key = format!("{}#{}", self.capability_key(), shape.tag());
        if let Ok(mut seen) = shape_memo().write() {
            if seen.insert(key) {
                tracing::warn!(
                    model = %self.model,
                    shape = shape.tag(),
                    "[AiAnalyzer] Endpoint refuses this request shape; skipping it from now on"
                );
            }
        }
    }

    /// Cap Gemini response bodies (success + error) to avoid unbounded buffers.
    async fn read_limited_json<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        max_bytes: usize,
    ) -> anyhow::Result<T> {
        let bytes = crate::services::outbound_security::read_limited_body(response, max_bytes)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        serde_json::from_slice(&bytes).context("Failed to parse Gemini JSON")
    }

    async fn read_limited_error_text(response: reqwest::Response) -> String {
        crate::services::outbound_security::read_limited_body(response, 64 * 1024)
            .await
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_else(|_| "Unknown error".to_string())
    }

    pub async fn new(
        provider: AiProvider,
        api_key: String,
        model: String,
        base_url: Option<String>,
    ) -> Self {
        Self::new_with_timeout(provider, api_key, model, base_url, Duration::from_secs(120)).await
    }

    /// 与 [`AiAnalyzer::new`] 相同，但允许长任务（如 Tapp Playground 生成）
    /// 指定更长的单次请求超时。
    pub async fn new_with_timeout(
        provider: AiProvider,
        api_key: String,
        model: String,
        base_url: Option<String>,
        request_timeout: Duration,
    ) -> Self {
        let proxy_config = ProxyConfig::from_dynamic_config().await;

        let builder = Client::builder()
            .timeout(request_timeout)
            .connect_timeout(Duration::from_secs(30))
            .user_agent("Myriad/1.0");

        // MYR-019: if proxy is required and build fails, do not silently direct-connect.
        let client = match crate::services::http_client::apply_proxy(builder, &proxy_config)
            .and_then(|b| b.build())
        {
            Ok(client) => client,
            Err(e) if crate::services::http_client::proxy_is_required(&proxy_config) => {
                tracing::error!(
                    %e,
                    proxy_url = ?proxy_config.proxy_url.as_deref(),
                    "AiAnalyzer: configured outbound proxy failed to build; refusing \
                     silent direct-connect (MYR-019 fail-closed)"
                );
                panic!(
                    "AiAnalyzer: configured outbound proxy failed to build \
                     (fail-closed, no direct bypass): {e}"
                );
            }
            Err(e) => {
                tracing::error!(
                    %e,
                    "AiAnalyzer client build failed with proxy disabled; using direct client"
                );
                Client::builder()
                    .timeout(request_timeout)
                    .connect_timeout(Duration::from_secs(30))
                    .user_agent("Myriad/1.0")
                    .build()
                    .unwrap_or_else(|_| Client::new())
            }
        };
        Self {
            client,
            provider,
            api_key,
            model,
            base_url,
        }
    }

    pub async fn analyze_profile(&self, profile_data: &serde_json::Value) -> Result<String> {
        let prompt = require_analyze_prompt(profile_data)?;
        let input_chars = serde_json::to_string(profile_data)
            .map(|s| s.len())
            .unwrap_or(0);
        let result = match self.provider {
            AiProvider::Gemini => self.analyze_with_gemini(prompt).await,
            AiProvider::OpenAI => self.analyze_with_openai(prompt).await,
        };
        self.note_ledger(input_chars, &result, "analyze").await;
        result
    }

    /// Best-effort site-wide cost ledger. Writes even without attribution
    /// (`source=internal`). Governed Tapp+scheduler paths suppress this hook.
    async fn note_ledger(
        &self,
        input_chars: usize,
        result: &Result<String, anyhow::Error>,
        operation: &str,
    ) {
        let _ = operation;
        match result {
            Ok(text) => {
                crate::services::ai_cost_ledger::record_ai_call_from_attribution(
                    self.provider.as_str(),
                    &self.model,
                    input_chars,
                    text.len(),
                    "completed",
                    None,
                )
                .await;
            }
            Err(_) => {
                crate::services::ai_cost_ledger::record_ai_call_from_attribution(
                    self.provider.as_str(),
                    &self.model,
                    input_chars,
                    0,
                    "failed",
                    Some("AI_PROVIDER_ERROR"),
                )
                .await;
            }
        }
    }

    async fn analyze_with_gemini(&self, prompt: &str) -> Result<String> {
        let request_body = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: None,
        };

        let url = GeminiApiUrl::generate_content_url(&self.model).await;

        tracing::info!("🔗 Calling Gemini API (model: {})", self.model);

        let response = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = Self::read_limited_error_text(response).await;
            return Err(anyhow::anyhow!(
                "Gemini API error {}: {}",
                status,
                error_text
            ));
        }

        let gemini_response: GeminiResponse =
            Self::read_limited_json(response, 2 * 1024 * 1024).await?;

        // 检测 prompt 级别的安全过滤
        if let Some(ref feedback) = gemini_response.prompt_feedback {
            if let Some(ref reason) = feedback.block_reason {
                return Err(anyhow::anyhow!(
                    "Gemini blocked the request (reason: {})",
                    reason
                ));
            }
        }

        // 检测 candidate 级别的安全过滤（有 candidate 但无 content）
        if let Some(candidate) = gemini_response.candidates.first() {
            if candidate.content.is_none() {
                let reason = candidate.finish_reason.as_deref().unwrap_or("UNKNOWN");
                return Err(anyhow::anyhow!(
                    "Gemini blocked the response (finishReason: {})",
                    reason
                ));
            }
        }

        let analysis = gemini_response
            .candidates
            .first()
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.first())
            .map(|p| p.text.clone())
            .unwrap_or_else(|| "No analysis generated".to_string());

        Ok(analysis)
    }

    async fn analyze_with_openai(&self, prompt: &str) -> Result<String> {
        let request_body = OpenAIRequest {
            max_tokens: None,
            model: self.model.clone(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            response_format: None,
        };

        let url = openai_chat_completions_url(self.base_url.as_deref());

        tracing::info!(
            "🔗 Calling OpenAI-compatible API: {} (model: {})",
            url,
            self.model
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Failed to send request to OpenAI-compatible API (endpoint: {url}, model: {})",
                    self.model
                )
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = Self::read_limited_error_text(response).await;
            return Err(anyhow::anyhow!(format_openai_compatible_http_error(
                status,
                &url,
                &self.model,
                &error_text,
            )));
        }

        let openai_response: OpenAIResponse =
            Self::read_limited_json(response, 2 * 1024 * 1024).await?;

        extract_openai_completion_text(&openai_response)
    }

    /// 简单的分析方法（用于 Tapp API）
    pub async fn analyze(&self, prompt: &str) -> Result<String> {
        let data = serde_json::json!({ "prompt": prompt });
        self.analyze_profile(&data).await
    }

    /// 带系统提示的分析方法（用于 Tapp API）
    pub async fn analyze_with_system(&self, system: &str, prompt: &str) -> Result<String> {
        self.analyze_with_messages(system, vec![ChatMessage::user(prompt.to_string())])
            .await
    }

    /// Multi-turn analysis: system prompt + ordered role/content messages.
    ///
    /// OpenAI-compatible providers receive true multi-turn `messages`.
    /// Gemini flattens the conversation into a single prompt (no native multi-turn
    /// system+history support in this client).
    pub async fn analyze_with_messages(
        &self,
        system: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<String> {
        match self.provider {
            AiProvider::Gemini => {
                // analyze_profile notes the ledger once.
                let full_prompt = flatten_messages_for_gemini(system, &messages);
                let data = serde_json::json!({ "prompt": full_prompt });
                self.analyze_profile(&data).await
            }
            AiProvider::OpenAI => {
                let input_chars =
                    system.len() + messages.iter().map(|m| m.content.len()).sum::<usize>();
                let mut openai_messages = Vec::with_capacity(messages.len() + 1);
                if !system.trim().is_empty() {
                    openai_messages.push(OpenAIMessage {
                        role: "system".to_string(),
                        content: system.to_string(),
                    });
                }
                for message in messages {
                    let role = match message.role.as_str() {
                        "assistant" => "assistant",
                        "system" => "system",
                        _ => "user",
                    };
                    openai_messages.push(OpenAIMessage {
                        role: role.to_string(),
                        content: message.content,
                    });
                }

                let request_body = OpenAIRequest {
                    model: self.model.clone(),
                    messages: openai_messages,
                    response_format: None,
                    max_tokens: None,
                };

                let url = openai_chat_completions_url(self.base_url.as_deref());

                let response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to send request to OpenAI-compatible API (endpoint: {url}, model: {})",
                            self.model
                        )
                    });

                let result = match response {
                    Ok(response) => {
                        if !response.status().is_success() {
                            let status = response.status();
                            let error_text = Self::read_limited_error_text(response).await;
                            Err(anyhow::anyhow!(format_openai_compatible_http_error(
                                status,
                                &url,
                                &self.model,
                                &error_text,
                            )))
                        } else {
                            let openai_response: OpenAIResponse =
                                Self::read_limited_json(response, 2 * 1024 * 1024).await?;
                            extract_openai_completion_text(&openai_response)
                        }
                    }
                    Err(e) => Err(e),
                };
                self.note_ledger(input_chars, &result, "chat").await;
                result
            }
        }
    }

    /// Ask the provider for JSON using its native structured-output parameters.
    ///
    /// The alternative — telling the model "return one valid JSON value only" in
    /// the prompt and then scraping the first `{` to the last `}` out of free
    /// text — fails whenever the model wraps its answer in prose or a Markdown
    /// fence, and the caller cannot tell a malformed answer from a deliberate
    /// one. Both providers can enforce JSON at the API level instead:
    ///
    /// - OpenAI-compatible: `response_format`, carrying `json_schema` when a
    ///   schema is supplied and `json_object` otherwise.
    /// - Gemini: `generationConfig.responseMimeType`, plus `responseSchema`
    ///   when the schema is expressible in its dialect.
    ///
    /// `schema` is advisory: a schema that cannot be translated (Gemini) is
    /// dropped while JSON enforcement is kept. Gateways that reject the
    /// parameters outright fall back to a plain prompt-only call, so an older or
    /// non-conforming OpenAI-compatible endpoint degrades to previous behaviour
    /// rather than failing the request.
    ///
    /// The returned string is still parsed by the caller — JSON validity is
    /// enforced, matching the caller's own type is not.
    pub async fn analyze_json(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String> {
        let input_chars = system.len() + prompt.len();

        let mut result = self
            .analyze_json_inner(
                system,
                prompt,
                schema_name,
                JsonMode::Structured(schema),
                None,
            )
            .await;

        if let Err(ref failure) = result {
            if failure.rejected_request() {
                tracing::warn!(
                    provider = self.provider.as_str(),
                    model = %self.model,
                    status = ?failure.status,
                    "[AiAnalyzer] Endpoint rejected structured output; retrying prompt-only"
                );
                result = self
                    .analyze_json_inner(
                        system,
                        prompt,
                        schema_name,
                        JsonMode::PromptOnly(schema),
                        None,
                    )
                    .await;
            }
        }

        let result = result.map_err(|failure| failure.error);
        self.note_ledger(input_chars, &result, "structured").await;
        result
    }

    /// 短结构化调用：限制输出、要求不要思考。
    ///
    /// 退化阶梯有三级，因为「关思考」和「结构化输出」是两组不同的参数，
    /// 网关可能只认其中一组：
    ///
    /// 1. 预算 + 结构化
    /// 2. 只结构化（网关不认预算参数时）
    /// 3. prompt-only（网关连结构化也不认，与 [`Self::analyze_json`] 的末级一致）
    ///
    /// 每一级只在上一级被判定为「请求形状被拒」时才走——鉴权失败和限流不重试。
    pub async fn analyze_json_short(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        schema: Option<&serde_json::Value>,
        budget: OutputBudget,
    ) -> Result<String> {
        let input_chars = system.len() + prompt.len();

        // 这个端点拒过什么，就别再每次去撞一遍。
        let mode = if self.refused(RequestShape::StructuredOutput) {
            JsonMode::PromptOnly(schema)
        } else {
            JsonMode::Structured(schema)
        };
        let extras = if self.refused(RequestShape::Extras) {
            None
        } else {
            Some(budget)
        };

        let mut result = self
            .analyze_json_inner(system, prompt, schema_name, mode, extras)
            .await;

        if extras.is_some()
            && result
                .as_ref()
                .is_err_and(ProviderCallFailure::rejected_request)
        {
            self.remember_refusal(RequestShape::Extras);
            result = self
                .analyze_json_inner(system, prompt, schema_name, mode, None)
                .await;
        }

        if matches!(mode, JsonMode::Structured(_))
            && result
                .as_ref()
                .is_err_and(ProviderCallFailure::rejected_request)
        {
            self.remember_refusal(RequestShape::StructuredOutput);
            result = self
                .analyze_json_inner(
                    system,
                    prompt,
                    schema_name,
                    JsonMode::PromptOnly(schema),
                    None,
                )
                .await;
        }

        let result = result.map_err(|failure| failure.error);
        self.note_ledger(input_chars, &result, "structured-short")
            .await;
        result
    }

    /// Structured JSON, but reasoning deltas are pushed live.
    ///
    /// Planner used to wait for the whole `analyze_json` body, then dump one
    /// `reasoning` field — that is why the bubble saw a single package.
    /// OpenAI-compatible providers that reject `stream` + `response_format`
    /// fall back to the blocking call.
    pub async fn analyze_json_streaming<F, Fut>(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        schema: Option<&serde_json::Value>,
        on_delta: F,
    ) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        if self.provider != AiProvider::OpenAI {
            return self.analyze_json(system, prompt, schema_name, schema).await;
        }

        let input_chars = system.len() + prompt.len();
        let streamed = self
            .analyze_json_streaming_openai(system, prompt, schema_name, schema, on_delta)
            .await;
        match streamed {
            Ok(text) => {
                let result = Ok(text);
                self.note_ledger(input_chars, &result, "structured-stream")
                    .await;
                result
            }
            Err(failure) if failure.rejected_request() => {
                tracing::warn!(
                    model = %self.model,
                    "[AiAnalyzer] Streaming structured output rejected; falling back"
                );
                self.analyze_json(system, prompt, schema_name, schema).await
            }
            Err(failure) => Err(failure.error),
        }
    }

    async fn analyze_json_streaming_openai<F, Fut>(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        schema: Option<&serde_json::Value>,
        on_delta: F,
    ) -> std::result::Result<String, ProviderCallFailure>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        let mode = JsonMode::Structured(schema);
        let mut messages = Vec::with_capacity(2);
        if !system.trim().is_empty() {
            messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: system.to_string(),
            });
        }
        messages.push(OpenAIMessage {
            role: "user".to_string(),
            content: mode.decorate_prompt(prompt),
        });

        let mut request_body = serde_json::to_value(OpenAIRequest {
            model: self.model.clone(),
            messages,
            response_format: mode.openai_response_format(schema_name),
            max_tokens: None,
        })
        .map_err(|e| ProviderCallFailure::transport(e.into()))?;
        if let Some(obj) = request_body.as_object_mut() {
            obj.insert("stream".to_string(), serde_json::json!(true));
        }

        let url = openai_chat_completions_url(self.base_url.as_deref());
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| ProviderCallFailure::transport(e.into()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = Self::read_limited_error_text(response).await;
            return Err(ProviderCallFailure::http(
                status,
                anyhow::anyhow!(format_openai_compatible_http_error(
                    status,
                    &url,
                    &self.model,
                    &error_text,
                )),
            ));
        }

        consume_openai_sse(response, on_delta)
            .await
            .map_err(ProviderCallFailure::transport)
    }

    async fn analyze_json_inner(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        mode: JsonMode<'_>,
        budget: Option<OutputBudget>,
    ) -> std::result::Result<String, ProviderCallFailure> {
        match self.provider {
            AiProvider::Gemini => {
                // This client has no native multi-turn support for Gemini; the
                // system prompt is folded in exactly as `analyze_with_messages`
                // does, so behaviour matches the non-structured path.
                let text = flatten_messages_for_gemini(
                    system,
                    &[ChatMessage::user(mode.decorate_prompt(prompt))],
                );

                let request_body = GeminiRequest {
                    contents: vec![GeminiContent {
                        parts: vec![GeminiPart { text }],
                    }],
                    generation_config: mode.gemini_generation_config(budget),
                };

                let url = GeminiApiUrl::generate_content_url(&self.model).await;
                let response = self
                    .client
                    .post(&url)
                    .header("x-goog-api-key", &self.api_key)
                    .json(&request_body)
                    .send()
                    .await
                    .map_err(|e| ProviderCallFailure::transport(e.into()))?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!("Gemini API error {}: {}", status, error_text),
                    ));
                }

                let gemini_response: GeminiResponse =
                    Self::read_limited_json(response, 2 * 1024 * 1024)
                        .await
                        .map_err(ProviderCallFailure::transport)?;

                if let Some(reason) = gemini_response
                    .prompt_feedback
                    .as_ref()
                    .and_then(|feedback| feedback.block_reason.as_ref())
                {
                    return Err(ProviderCallFailure::transport(anyhow::anyhow!(
                        "Gemini blocked the request (reason: {})",
                        reason
                    )));
                }

                gemini_response
                    .candidates
                    .first()
                    .and_then(|c| c.content.as_ref())
                    .and_then(|c| c.parts.first())
                    .map(|p| p.text.clone())
                    .ok_or_else(|| {
                        let reason = gemini_response
                            .candidates
                            .first()
                            .and_then(|c| c.finish_reason.as_deref())
                            .unwrap_or("UNKNOWN");
                        ProviderCallFailure::transport(anyhow::anyhow!(
                            "Gemini returned no content (finishReason: {})",
                            reason
                        ))
                    })
            }
            AiProvider::OpenAI => {
                let mut messages = Vec::with_capacity(2);
                if !system.trim().is_empty() {
                    messages.push(OpenAIMessage {
                        role: "system".to_string(),
                        content: system.to_string(),
                    });
                }
                messages.push(OpenAIMessage {
                    role: "user".to_string(),
                    content: mode.decorate_prompt(prompt),
                });

                let request_body = OpenAIRequest {
                    model: self.model.clone(),
                    messages,
                    response_format: mode.openai_response_format(schema_name),
                    max_tokens: budget.map(|b| b.max_tokens),
                };
                // 「别思考」和输出上限同进同退：它们同属「附加参数」这一类，
                // 被拒时一起丢掉，也一起被记住。
                let mut request_body = serde_json::to_value(request_body)
                    .map_err(|e| ProviderCallFailure::transport(e.into()))?;
                if budget.is_some() {
                    if let (Some(object), Some((key, value))) =
                        (request_body.as_object_mut(), self.gateway().thinking_off())
                    {
                        object.insert(key.to_string(), value);
                    }
                }

                let url = openai_chat_completions_url(self.base_url.as_deref());
                let response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                    .map_err(|e| ProviderCallFailure::transport(e.into()))?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!(format_openai_compatible_http_error(
                            status,
                            &url,
                            &self.model,
                            &error_text,
                        )),
                    ));
                }

                let openai_response: OpenAIResponse =
                    Self::read_limited_json(response, 2 * 1024 * 1024)
                        .await
                        .map_err(ProviderCallFailure::transport)?;
                extract_openai_completion_text(&openai_response)
                    .map_err(ProviderCallFailure::transport)
            }
        }
    }

    // TODO: Add more analysis methods
    // pub async fn generate_summary(&self, profiles: Vec<serde_json::Value>) -> Result<String>
    // pub async fn extract_skills(&self, profile_data: &serde_json::Value) -> Result<Vec<String>>

    /// 流式分析（逐 token 返回可见回复）
    ///
    /// 只把 `content` 交给回调。思考链走 [`Self::analyze_stream_parts`]。
    /// 回调返回 `false` 可提前终止流。
    pub async fn analyze_stream<F>(&self, prompt: &str, mut on_token: F) -> Result<String>
    where
        F: FnMut(&str) -> bool + Send,
    {
        self.analyze_stream_parts(prompt, |delta| {
            let keep = match &delta {
                StreamDelta::Text(text) => on_token(text),
                StreamDelta::Reasoning(_) => true,
            };
            async move { keep }
        })
        .await
    }

    /// 流式分析，思考链和正文分开回调。返回值仍只是可见回复。
    ///
    /// 回调是 async：调用方必须 `send().await` 把这一截交给 SSE，
    /// 不能 `try_send` 塞进有界队列再一次性倒出去。
    pub async fn analyze_stream_parts<F, Fut>(&self, prompt: &str, on_delta: F) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        let input_chars = prompt.len();
        let result = self.analyze_stream_inner(prompt, on_delta).await;
        self.note_ledger(input_chars, &result, "stream").await;
        result
    }

    async fn analyze_stream_inner<F, Fut>(&self, prompt: &str, mut on_delta: F) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        let mut full_text = String::new();
        match self.provider {
            AiProvider::Gemini => {
                let request_body = GeminiRequest {
                    contents: vec![GeminiContent {
                        parts: vec![GeminiPart {
                            text: prompt.to_string(),
                        }],
                    }],
                    generation_config: None,
                };

                let base_url = crate::services::http_client::GeminiApiUrl::get_base().await;
                let url = format!(
                    "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
                    base_url, self.model
                );

                let mut response = self
                    .client
                    .post(&url)
                    .header("x-goog-api-key", &self.api_key)
                    .json(&request_body)
                    .send()
                    .await
                    .context("Failed to send streaming request to Gemini API")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(anyhow::anyhow!(
                        "Gemini streaming API error {}: {}",
                        status,
                        error_text
                    ));
                }

                let mut buffer = String::new();

                loop {
                    let chunk = response.chunk().await.context("Stream read error")?;
                    match chunk {
                        Some(bytes) => buffer.push_str(&String::from_utf8_lossy(&bytes)),
                        None => break,
                    }

                    // Parse SSE lines: "data: {...}\n\n"
                    while let Some(pos) = buffer.find("\n\n") {
                        let event_block = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        for line in event_block.lines() {
                            let line = line.trim();
                            if let Some(data) = line.strip_prefix("data: ") {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                    for delta in gemini_stream_deltas(&json) {
                                        if let StreamDelta::Text(ref content) = delta {
                                            full_text.push_str(content);
                                        }
                                        if !on_delta(delta).await {
                                            return Ok(full_text);
                                        }
                                        tokio::task::yield_now().await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            AiProvider::OpenAI => {
                #[derive(Serialize)]
                struct OpenAIStreamRequest {
                    model: String,
                    messages: Vec<OpenAIMessage>,
                    stream: bool,
                }

                let request_body = OpenAIStreamRequest {
                    model: self.model.clone(),
                    messages: vec![OpenAIMessage {
                        role: "user".to_string(),
                        content: prompt.to_string(),
                    }],
                    stream: true,
                };

                let url = openai_chat_completions_url(self.base_url.as_deref());

                let response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to send streaming request to OpenAI-compatible API (endpoint: {url}, model: {})",
                            self.model
                        )
                    })?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(anyhow::anyhow!(format_openai_compatible_http_error(
                        status,
                        &url,
                        &self.model,
                        &error_text,
                    )));
                }

                return consume_openai_sse(response, on_delta).await;
            }
        }

        Ok(full_text)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_openai_completion_text, flatten_messages_for_gemini,
        format_openai_compatible_http_error, gemini_response_schema, gemini_stream_deltas,
        openai_chat_completions_url, openai_stream_deltas, require_analyze_prompt, ChatMessage,
        GeminiContent, GeminiPart, GeminiRequest, JsonMode, OpenAIMessage, OpenAIRequest,
        OpenAIResponse, OutputBudget, ProviderCallFailure, StreamDelta,
    };
    use serde_json::json;

    fn openai_body(mode: JsonMode<'_>, budget: Option<OutputBudget>) -> serde_json::Value {
        serde_json::to_value(OpenAIRequest {
            model: "m".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            response_format: mode.openai_response_format("s"),
            max_tokens: budget.map(|b| b.max_tokens),
        })
        .expect("serialize")
    }

    /// 没有预算的调用，报文必须和加这个功能之前一模一样。
    #[test]
    fn an_unbudgeted_call_sends_no_new_fields() {
        let body = openai_body(JsonMode::PromptOnly(None), None);
        let object = body.as_object().expect("object");
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            // serde_json 这里是 BTreeMap，键按字典序。
            vec!["messages", "model"],
            "普通调用的报文不该多出字段"
        );

        let config = JsonMode::PromptOnly(None).gemini_generation_config(None);
        assert!(
            config.is_none(),
            "没有结构化也没有预算时不该发 generationConfig"
        );
    }

    #[test]
    fn a_budgeted_call_caps_output_and_nothing_else() {
        let budget = OutputBudget { max_tokens: 2048 };
        let body = openai_body(JsonMode::Structured(None), Some(budget));
        assert_eq!(body["max_tokens"], json!(2048));
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
        // 关思考的参数各家不同，猜一个发出去会换来 4xx 和一次多余的重试。
        // 要加就得先按 base_url 认出网关。
        assert!(body.get("reasoning_effort").is_none());

        let config = JsonMode::Structured(None)
            .gemini_generation_config(Some(budget))
            .expect("generationConfig");
        let config = serde_json::to_value(config).expect("serialize");
        assert_eq!(config["maxOutputTokens"], json!(2048));
        assert_eq!(config["responseMimeType"], json!("application/json"));
        assert!(config.get("thinkingConfig").is_none());
    }

    /// 上限和结构化输出是两组参数，网关可能只认一组。退回 prompt-only 之后
    /// 上限还得在，否则退化路径上又变成没有上限。
    #[test]
    fn dropping_structured_output_keeps_the_output_cap() {
        let budget = OutputBudget { max_tokens: 2048 };
        let config = JsonMode::PromptOnly(None)
            .gemini_generation_config(Some(budget))
            .expect("generationConfig");
        let config = serde_json::to_value(config).expect("serialize");
        assert_eq!(config["maxOutputTokens"], json!(2048));
        assert_eq!(config["responseMimeType"], json!("text/plain"));
        assert!(config.get("responseSchema").is_none());
    }

    #[test]
    fn the_gateway_is_recovered_from_the_base_url() {
        use super::{gateway_of, AiProvider, Gateway};
        // AiProvider 把 openai 和 openrouter 压成同一个值，身份只能从 base_url 认。
        assert_eq!(
            gateway_of(AiProvider::OpenAI, Some("https://openrouter.ai/api/v1")),
            Gateway::OpenRouter
        );
        assert_eq!(
            gateway_of(AiProvider::OpenAI, Some("https://api.openai.com/v1")),
            Gateway::OpenAi
        );
        // 没配 base_url 就是官方端点。
        assert_eq!(gateway_of(AiProvider::OpenAI, None), Gateway::OpenAi);
        // 自建端点归到「兼容」：只发共通参数，不发任何一家的方言。
        assert_eq!(
            gateway_of(AiProvider::OpenAI, Some("https://llm.example.com/v1")),
            Gateway::OpenAiCompatible
        );
        // provider 是 gemini 时 base_url 不参与判断。
        assert_eq!(gateway_of(AiProvider::Gemini, None), Gateway::Gemini);
    }

    /// 预算被拒 → 去掉预算重试 → 还被拒 → prompt-only。三级，不能少。
    #[test]
    fn the_short_call_has_a_three_step_ladder() {
        let source = include_str!("analyzer.rs");
        let body = source
            .split("pub async fn analyze_json_short(")
            .nth(1)
            .and_then(|rest| rest.split("\n    /// ").next())
            .expect("analyze_json_short body");
        assert_eq!(
            body.matches("analyze_json_inner(").count(),
            3,
            "短调用的退化阶梯应当是三级"
        );
        assert_eq!(
            body.matches("rejected_request").count(),
            2,
            "每一级都只在请求形状被拒时才降级"
        );
        // 阶梯要有记忆：拒过一次之后直接跳过那一档，否则不支持的网关每一次
        // 调用都要先撞一次墙。两类分开记，因为网关可能只拒其中一组。
        assert!(body.contains("self.refused(RequestShape::StructuredOutput)"));
        assert!(body.contains("self.refused(RequestShape::Extras)"));
        assert!(body.contains("remember_refusal(RequestShape::Extras)"));
        assert!(body.contains("remember_refusal(RequestShape::StructuredOutput)"));
    }

    /// 「别思考」只发有把握的那一家，其余留空。
    ///
    /// 各家参数名不同，发一个猜来的比不发更糟：网关不认就是 4xx，多一个来回。
    /// 有了记忆之后代价从「每次」降到「一次」，但那不是乱发的理由——不确定
    /// 就返回 None。
    #[test]
    fn only_a_gateway_we_are_sure_about_gets_a_thinking_switch() {
        use super::Gateway;
        assert_eq!(
            Gateway::OpenRouter.thinking_off(),
            Some(("reasoning", json!({ "enabled": false })))
        );
        for unsure in [Gateway::OpenAi, Gateway::Gemini, Gateway::OpenAiCompatible] {
            assert!(
                unsure.thinking_off().is_none(),
                "{unsure:?} 的参数没核实过，不该发"
            );
        }
    }

    /// 关思考跟着输出上限走：被拒时一起丢，不会只丢一半。
    #[test]
    fn the_thinking_switch_rides_with_the_budget() {
        let source = include_str!("analyzer.rs");
        let inner = source
            .split("async fn analyze_json_inner(")
            .nth(1)
            .and_then(|rest| rest.split("\n    // TODO").next())
            .expect("analyze_json_inner body");
        // 只在带预算的那一档附上，跟着一起丢。
        assert!(inner.contains("if budget.is_some()"));
        assert!(inner.contains("self.gateway().thinking_off()"));
    }

    /// 只在「请求形状被拒」时记忆——鉴权失败和限流不是能力问题，记下来会让
    /// 一次配错的 key 永久关掉这个端点的结构化输出。
    #[test]
    fn the_memo_is_keyed_on_endpoint_and_model_and_only_on_shape_refusals() {
        let source = include_str!("analyzer.rs");
        let memo = source
            .split("fn remember_refusal(")
            .nth(1)
            .and_then(|rest| rest.split("\n    /// ").next())
            .expect("memo writer");
        assert!(memo.contains("capability_key()"));

        let key = source
            .split("fn capability_key(")
            .nth(1)
            .and_then(|rest| rest.split("\n    fn ").next())
            .expect("capability key");
        // 同一个网关后面可以挂着几十家模型，支持度不一样。
        assert!(key.contains("base_url"));
        assert!(key.contains("self.model"));

        // rejected_request 已经排除了鉴权和限流，记忆挂在它后面即可。
        assert!(source.contains("StatusCode::UNAUTHORIZED"));
        assert!(source.contains("StatusCode::TOO_MANY_REQUESTS"));
    }

    fn closed_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "score": { "type": "number", "minimum": 0.0 },
                "tags": { "type": "array", "items": { "type": "string" } },
                "kind": { "type": "string", "enum": ["a", "b"] }
            },
            "required": ["summary"],
            "additionalProperties": false
        })
    }

    #[test]
    fn analyze_profile_requires_a_non_empty_prompt() {
        assert_eq!(
            require_analyze_prompt(&json!({ "prompt": "write the card" })).unwrap(),
            "write the card"
        );
        assert!(require_analyze_prompt(&json!({})).is_err());
        assert!(require_analyze_prompt(&json!({ "prompt": "" })).is_err());
        assert!(require_analyze_prompt(&json!({ "summary": "raw profile" })).is_err());
    }

    // Structured output — Gemini schema dialect

    #[test]
    fn gemini_schema_uppercases_types_and_drops_unsupported_keys() {
        let translated = gemini_response_schema(&closed_schema()).expect("closed schema");
        assert_eq!(translated["type"], "OBJECT");
        assert_eq!(translated["properties"]["summary"]["type"], "STRING");
        assert_eq!(translated["properties"]["tags"]["type"], "ARRAY");
        assert_eq!(translated["properties"]["tags"]["items"]["type"], "STRING");
        assert_eq!(translated["required"], json!(["summary"]));
        // Gemini rejects these outright.
        assert!(translated.get("additionalProperties").is_none());
        assert!(translated["properties"]["score"].get("minimum").is_none());
        // enum is part of the accepted subset.
        assert_eq!(translated["properties"]["kind"]["enum"], json!(["a", "b"]));
    }

    #[test]
    fn gemini_schema_rejects_free_form_objects() {
        // Gemini has no way to express an OBJECT without properties, which is
        // how a planner step's open `params` map is declared. The whole schema
        // must be dropped rather than silently losing the field.
        assert!(gemini_response_schema(&json!({ "type": "object" })).is_none());
        assert!(gemini_response_schema(&json!({
            "type": "object",
            "properties": {}
        }))
        .is_none());
        assert!(gemini_response_schema(&json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "params": { "type": "object" } }
                    }
                }
            }
        }))
        .is_none());
    }

    #[test]
    fn gemini_schema_rejects_non_object_roots() {
        assert!(gemini_response_schema(&json!("string")).is_none());
        assert!(gemini_response_schema(&json!([1, 2])).is_none());
    }

    // Structured output — request bodies

    #[test]
    fn structured_mode_sets_json_mime_and_schema_for_gemini() {
        let schema = closed_schema();
        let config = JsonMode::Structured(Some(&schema))
            .gemini_generation_config(None)
            .expect("structured mode configures generation");
        assert_eq!(config.response_mime_type, "application/json");
        assert_eq!(
            config.response_schema.expect("translatable schema")["type"],
            "OBJECT"
        );
    }

    #[test]
    fn untranslatable_schema_still_enforces_json_on_gemini() {
        let schema = json!({ "type": "object", "properties": { "p": { "type": "object" } } });
        let config = JsonMode::Structured(Some(&schema))
            .gemini_generation_config(None)
            .expect("json mime is kept");
        assert_eq!(config.response_mime_type, "application/json");
        assert!(config.response_schema.is_none());
    }

    #[test]
    fn openai_structured_mode_uses_json_schema_non_strict() {
        let schema = closed_schema();
        let format = JsonMode::Structured(Some(&schema))
            .openai_response_format("planner_output")
            .expect("response_format is set");
        assert_eq!(format["type"], "json_schema");
        assert_eq!(format["json_schema"]["name"], "planner_output");
        // strict mode forbids free-form objects, which planner params require.
        assert_eq!(format["json_schema"]["strict"], json!(false));
        assert_eq!(format["json_schema"]["schema"], schema);
    }

    #[test]
    fn openai_falls_back_to_json_object_without_a_schema() {
        let format = JsonMode::Structured(None)
            .openai_response_format("x")
            .expect("json mode is still requested");
        assert_eq!(format, json!({ "type": "json_object" }));
    }

    #[test]
    fn prompt_only_mode_sends_no_structured_parameters() {
        let schema = closed_schema();
        let mode = JsonMode::PromptOnly(Some(&schema));
        assert!(mode.gemini_generation_config(None).is_none());
        assert!(mode.openai_response_format("x").is_none());
    }

    #[test]
    fn prompt_only_mode_restates_the_contract_in_the_prompt() {
        let schema = closed_schema();
        let decorated = JsonMode::PromptOnly(Some(&schema)).decorate_prompt("do the thing");
        assert!(decorated.starts_with("do the thing"));
        assert!(decorated.contains("Return one valid JSON value only"));
        assert!(decorated.contains("\"summary\""));

        // Structured mode leaves the prompt untouched; the API enforces it.
        assert_eq!(
            JsonMode::Structured(Some(&schema)).decorate_prompt("do the thing"),
            "do the thing"
        );
    }

    #[test]
    fn ordinary_requests_omit_the_structured_fields_entirely() {
        // Existing call sites must serialize byte-identically to before.
        let gemini = serde_json::to_value(GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart {
                    text: "hi".to_string(),
                }],
            }],
            generation_config: None,
        })
        .expect("serialize");
        assert!(gemini.get("generationConfig").is_none());

        let openai = serde_json::to_value(OpenAIRequest {
            model: "gpt-x".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            response_format: None,
            max_tokens: None,
        })
        .expect("serialize");
        assert!(openai.get("response_format").is_none());
    }

    // Structured output — compatibility fallback

    #[test]
    fn rejected_request_triggers_the_prompt_only_retry() {
        for status in [
            reqwest::StatusCode::BAD_REQUEST,
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        ] {
            let failure = ProviderCallFailure::http(status, anyhow::anyhow!("nope"));
            assert!(failure.rejected_request(), "{status} should retry plain");
        }
    }

    #[test]
    fn auth_rate_limit_and_transport_failures_do_not_retry() {
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::BAD_GATEWAY,
        ] {
            let failure = ProviderCallFailure::http(status, anyhow::anyhow!("nope"));
            assert!(
                !failure.rejected_request(),
                "{status} would fail identically on retry"
            );
        }
        assert!(!ProviderCallFailure::transport(anyhow::anyhow!("timeout")).rejected_request());
    }

    #[test]
    fn openai_compatible_error_does_not_claim_official_openai() {
        let msg = format_openai_compatible_http_error(
            reqwest::StatusCode::FORBIDDEN,
            "https://openrouter.ai/api/v1/chat/completions",
            "x-ai/grok-4.5",
            r#"{"error":{"message":"This model is not available in your region.","code":403}}"#,
        );
        assert!(msg.contains("OpenAI-compatible API error"));
        assert!(!msg.starts_with("OpenAI API error"));
        assert!(msg.contains("x-ai/grok-4.5"));
        assert!(msg.contains("openrouter.ai"));
        assert!(msg.contains("geo-restriction"));
    }

    #[test]
    fn flattens_multi_turn_messages_for_gemini() {
        let flat = flatten_messages_for_gemini(
            "rules",
            &[
                ChatMessage::user("first"),
                ChatMessage::assistant("reply"),
                ChatMessage::user("second"),
            ],
        );
        assert!(flat.starts_with("SYSTEM:\nrules"));
        assert!(flat.contains("USER:\nfirst"));
        assert!(flat.contains("ASSISTANT:\nreply"));
        assert!(flat.contains("USER:\nsecond"));
    }

    #[test]
    fn normalizes_openai_compatible_chat_urls() {
        assert_eq!(
            openai_chat_completions_url(None),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://api.openai.com")),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://openrouter.ai/")),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://openrouter.ai/api/v1")),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://gateway.example.com/v1/chat/completions")),
            "https://gateway.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_response_deserializes_null_content() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "step by step: final answer HERE"
                }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let text = extract_openai_completion_text(&parsed).expect("fallback to reasoning");
        assert!(text.contains("final answer HERE"));
    }

    #[test]
    fn openai_response_top_level_error_is_err() {
        let raw = r#"{
            "error": {
                "message": "Provider returned error",
                "code": "model_not_found"
            },
            "choices": []
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let err = extract_openai_completion_text(&parsed).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Provider returned error"));
        assert!(msg.contains("model_not_found"));
    }

    #[test]
    fn openai_response_empty_content_without_reasoning_is_err() {
        let raw = r#"{
            "choices": [{
                "message": { "role": "assistant", "content": null }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let err = extract_openai_completion_text(&parsed).unwrap_err();
        assert!(format!("{err:#}").contains("empty message content"));
    }

    #[test]
    fn openai_response_prefers_non_empty_content() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "content": "  visible payload  ",
                    "reasoning_content": "hidden chain"
                }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let text = extract_openai_completion_text(&parsed).expect("content");
        assert_eq!(text, "visible payload");
    }

    #[test]
    fn openai_stream_reads_reasoning_content_separately_from_answer() {
        let chunk = json!({
            "choices": [{
                "delta": {
                    "reasoning_content": "let me count",
                    "content": "4"
                }
            }]
        });
        assert_eq!(
            openai_stream_deltas(&chunk),
            vec![
                StreamDelta::Reasoning("let me count".to_string()),
                StreamDelta::Text("4".to_string()),
            ]
        );
    }

    #[test]
    fn openai_stream_reads_reasoning_object_and_responses_events() {
        let object = json!({
            "choices": [{ "delta": { "reasoning": { "content": "energy first" } } }]
        });
        assert_eq!(
            openai_stream_deltas(&object),
            vec![StreamDelta::Reasoning("energy first".to_string())]
        );

        let responses = json!({
            "type": "response.reasoning_summary_text.delta",
            "delta": "then impact speed"
        });
        assert_eq!(
            openai_stream_deltas(&responses),
            vec![StreamDelta::Reasoning("then impact speed".to_string())]
        );
    }

    #[test]
    fn openai_stream_reads_reasoning_alias_and_skips_empty() {
        let reasoning_only = json!({
            "choices": [{ "delta": { "reasoning": "scratch" } }]
        });
        assert_eq!(
            openai_stream_deltas(&reasoning_only),
            vec![StreamDelta::Reasoning("scratch".to_string())]
        );

        let empty = json!({ "choices": [{ "delta": { "content": "" } }] });
        assert!(openai_stream_deltas(&empty).is_empty());
    }

    #[test]
    fn openai_stream_joins_reasoning_details() {
        let chunk = json!({
            "choices": [{
                "delta": {
                    "reasoning_details": [
                        { "text": "step " },
                        { "content": "two" }
                    ]
                }
            }]
        });
        assert_eq!(
            openai_stream_deltas(&chunk),
            vec![StreamDelta::Reasoning("step two".to_string())]
        );
    }

    #[test]
    fn gemini_stream_marks_thought_parts_as_reasoning() {
        let chunk = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "thinking aloud", "thought": true },
                        { "text": "hello" }
                    ]
                }
            }]
        });
        assert_eq!(
            gemini_stream_deltas(&chunk),
            vec![
                StreamDelta::Reasoning("thinking aloud".to_string()),
                StreamDelta::Text("hello".to_string()),
            ]
        );
    }
}
