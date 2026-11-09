use crate::services::retained_cache::RetainedCache;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Serialize;
use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

use crate::services::http_client::{GeminiApiUrl, ProxyConfig};

use super::gemini::{flatten_messages_for_gemini, gemini_stream_deltas};
use super::openai::{
    consume_openai_sse, extract_openai_completion_text, format_openai_compatible_http_error,
    openai_chat_completions_url,
};
use super::schema::JsonMode;
use super::text_protocol;
use super::transport;
use super::types::{
    AiProvider, ChatMessage, Gateway, GeminiContent, GeminiPart, GeminiRequest, GeminiResponse,
    ImageInput, OpenAIMessage, OpenAIRequest, OpenAIResponse, OutputBudget, StreamDelta,
    gateway_of,
};

pub struct AiAnalyzer {
    pub(super) client: Client,
    pub(super) provider: AiProvider,
    pub(super) api_key: String,
    pub(super) model: String,
    pub(super) base_url: Option<String>, // For OpenAI-compatible APIs
    /// Ask the gateway for as little thinking as it allows; see
    /// [`AiAnalyzer::with_light_thinking`].
    pub(super) light_thinking: bool,
    /// Ask the model to think it through; see
    /// [`AiAnalyzer::with_deep_thinking`].
    pub(super) deep_thinking: bool,
    /// Most output a request asks for when it names none; see
    /// [`AiAnalyzer::with_output_cap`].
    pub(super) output_cap: Option<u32>,
}

/// The output limit of a deep-thinking ask that names none: the answer is
/// short, the thinking before it is not.
const DEEP_THINKING_MAX_TOKENS: u32 = 16_000;

/// 进程内记下端点和模型拒绝的请求参数；重启清空。
///
/// 阶梯本身没错，错在它没有记忆：不支持 `json_schema` 的网关上，每一次调用都
/// 要先被拒一次再降级——等于给最弱的那批网关加了一笔常驻的往返税。记下来之后
/// 这笔钱只付一次。
///
/// 只记「请求形状被拒」（4xx，且不含鉴权和限流，见
/// [`ProviderCallFailure::rejected_request`]）。进程内有效：配置换了、网关升级
/// 了，最多一天后重新尝试；容量上限避免配置轮换积累。
static SHAPE_REFUSED: OnceLock<std::sync::RwLock<RetainedCache<String, ()>>> = OnceLock::new();

fn shape_memo() -> &'static std::sync::RwLock<RetainedCache<String, ()>> {
    SHAPE_REFUSED.get_or_init(|| {
        std::sync::RwLock::new(RetainedCache::new(256, Duration::from_secs(24 * 3600)))
    })
}

pub(crate) fn cleanup_shape_memo() {
    if let Some(memo) = SHAPE_REFUSED.get() {
        if let Ok(mut memo) = memo.write() {
            memo.purge_expired();
        }
    }
}

/// 各组参数单独记忆，网关可能只拒其中一组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestShape {
    /// `response_format` / `responseSchema`
    StructuredOutput,
    /// 输出上限 + 那一家的「少想」参数
    Extras,
    /// 只有「少想」参数（[`AiAnalyzer::with_light_thinking`]）
    LightThinking,
    /// 只有「多想」参数（[`AiAnalyzer::with_deep_thinking`]）
    DeepThinking,
}

impl RequestShape {
    fn tag(self) -> &'static str {
        match self {
            Self::StructuredOutput => "structured",
            Self::Extras => "extras",
            Self::LightThinking => "light-thinking",
            Self::DeepThinking => "deep-thinking",
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

/// A failed provider call, keeping the HTTP status so the caller can tell a
/// rejected request shape from a transport or upstream failure.
struct ProviderCallFailure {
    status: Option<reqwest::StatusCode>,
    error: anyhow::Error,
}

impl From<anyhow::Error> for ProviderCallFailure {
    fn from(error: anyhow::Error) -> Self {
        Self::transport(error)
    }
}

/// Provider status preserved through retries and public anyhow boundaries.
#[derive(Debug)]
pub struct ProviderHttpError {
    pub status: reqwest::StatusCode,
    error: anyhow::Error,
}

impl std::fmt::Display for ProviderHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for ProviderHttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}

impl ProviderCallFailure {
    fn into_error(self) -> anyhow::Error {
        match self.status {
            Some(status) => ProviderHttpError {
                status,
                error: self.error,
            }
            .into(),
            None => self.error,
        }
    }

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

    /// 400, 404 (OpenRouter: no endpoint takes these parameters) and 422:
    /// what was asked for is not taken. The account's own refusals (401,
    /// 402 out of credit, 403, 429) and the rest of 4xx (a timeout, a body
    /// too long) say nothing of the shape, and must not be remembered as
    /// one for a day. No status (transport) is not a shape refusal.
    fn rejected_request(&self) -> bool {
        self.status.is_some_and(|status| {
            matches!(
                status,
                reqwest::StatusCode::BAD_REQUEST
                    | reqwest::StatusCode::NOT_FOUND
                    | reqwest::StatusCode::UNPROCESSABLE_ENTITY
            )
        })
    }
}

impl AiAnalyzer {
    pub(super) async fn gemini_url(&self, stream: bool) -> String {
        if let Some(base) = self
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
        {
            let method = if stream {
                "streamGenerateContent?alt=sse"
            } else {
                "generateContent"
            };
            return format!(
                "{}/v1beta/models/{}:{}",
                base.trim_end_matches('/'),
                self.model,
                method
            );
        }
        if stream {
            let base = GeminiApiUrl::get_base().await;
            format!(
                "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
                base, self.model
            )
        } else {
            GeminiApiUrl::generate_content_url(&self.model).await
        }
    }

    pub(super) fn authenticate(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match (
            self.provider,
            (!self.api_key.trim().is_empty()).then_some(self.api_key.as_str()),
        ) {
            (AiProvider::Anthropic, Some(key)) => request.header("x-api-key", key),
            (AiProvider::OpenAI | AiProvider::OpenAIResponses, Some(key)) => {
                request.bearer_auth(key)
            }
            (AiProvider::Gemini, Some(key)) => request.header("x-goog-api-key", key),
            _ => request,
        }
    }

    /// 对面是哪一家网关。
    pub fn gateway(&self) -> Gateway {
        gateway_of(self.provider, self.base_url.as_deref())
    }

    /// 记忆的键。同一个网关上不同模型的结构化输出支持度可以不一样
    /// （OpenRouter 后面挂着几十家），所以端点和模型都要进键。
    fn capability_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.provider.as_str(),
            self.base_url.as_deref().unwrap_or(""),
            self.model
        )
    }

    fn refused(&self, shape: RequestShape) -> bool {
        let key = format!("{}#{}", self.capability_key(), shape.tag());
        shape_memo()
            .write()
            .is_ok_and(|mut seen| seen.get(&key).is_some())
    }

    fn remember_refusal(&self, shape: RequestShape) {
        let key = format!("{}#{}", self.capability_key(), shape.tag());
        if let Ok(mut seen) = shape_memo().write() {
            if seen.get(&key).is_none() {
                seen.insert(key, ());
                tracing::warn!(
                    model = %self.model,
                    shape = shape.tag(),
                    "[AiAnalyzer] Endpoint refuses this request shape; skipping it from now on"
                );
            }
        }
    }

    /// Cap success JSON to `max_bytes`, then deserialize.
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
        Self::new_with_timeout(
            provider,
            api_key,
            model,
            base_url,
            Duration::from_secs(5 * 60),
        )
        .await
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

        // if proxy is required and build fails, do not silently direct-connect.
        let client = match transport::pooled_client(&proxy_config, request_timeout) {
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
            light_thinking: false,
            deep_thinking: false,
            output_cap: None,
        }
    }

    /// For calls whose output is short (talk, typed judgments): a request
    /// that names no output limit asks for at most `tokens`. OpenRouter
    /// reserves credit for the whole limit a request may use; left unset it
    /// is the model's own maximum (65536 for Gemini 3.8 Flash), and with
    /// credit running low every request is refused although each would
    /// have used a few hundred.
    pub fn with_output_cap(mut self, tokens: u32) -> Self {
        self.output_cap = Some(tokens);
        self
    }

    /// A request body as sent: within the handler's allowance, and capped
    /// as [`AiAnalyzer::with_output_cap`] says where it names no limit.
    fn prepare_body(&self, body: &impl serde::Serialize) -> anyhow::Result<serde_json::Value> {
        let mut body = super::request_budget::prepare(body, self.provider)?;
        if let (Some(cap), AiProvider::OpenAI) = (self.output_cap, self.provider)
            && body
                .get("max_tokens")
                .is_none_or(serde_json::Value::is_null)
            && body
                .get("max_completion_tokens")
                .is_none_or(serde_json::Value::is_null)
            && let Some(object) = body.as_object_mut()
        {
            object.insert("max_tokens".into(), serde_json::json!(cap));
        }
        Ok(body)
    }

    /// For calls where waiting costs more than thinking gains: small typed
    /// judgments, and talk. Sends the gateway's "think little" parameter where
    /// it is known ([`Gateway::light_thinking`]); an endpoint that refuses it
    /// is remembered and asked again without it.
    pub fn with_light_thinking(mut self) -> Self {
        self.light_thinking = true;
        self
    }

    /// The light-thinking parameter to send now, if any.
    fn light_thinking_param(&self) -> Option<(&'static str, serde_json::Value)> {
        if !self.light_thinking || self.refused(RequestShape::LightThinking) {
            return None;
        }
        self.gateway().light_thinking()
    }

    /// For calls where getting it right is worth the wait: reading what
    /// people mean, and what she keeps of it. Sends the provider's "think it
    /// through" parameter where it is known ([`Gateway::deep_thinking`];
    /// Anthropic's own API: adaptive thinking with `output_config.effort`);
    /// an endpoint that
    /// refuses it is remembered and asked again without it. Overrides
    /// [`AiAnalyzer::with_light_thinking`].
    pub fn with_deep_thinking(mut self) -> Self {
        self.deep_thinking = true;
        self.light_thinking = false;
        self
    }

    /// The deep-thinking parameter to send now, if any.
    fn deep_thinking_param(&self) -> Option<(&'static str, serde_json::Value)> {
        if !self.deep_thinking || self.refused(RequestShape::DeepThinking) {
            return None;
        }
        if self.provider == AiProvider::Anthropic {
            return Some(("output_config", serde_json::json!({ "effort": "high" })));
        }
        self.gateway().deep_thinking()
    }

    /// Whichever thinking parameter this analyzer asks for, if any.
    fn thinking_param(&self) -> Option<(&'static str, serde_json::Value)> {
        self.deep_thinking_param()
            .or_else(|| self.light_thinking_param())
    }

    /// POST a streaming OpenAI-compatible body, with the light-thinking
    /// parameter when asked for. An endpoint that refuses the parameter is
    /// remembered and asked once more without it.
    async fn post_openai_stream(&self, mut body: serde_json::Value) -> Result<reqwest::Response> {
        let url = openai_chat_completions_url(self.base_url.as_deref());
        let light = self.light_thinking_param();
        if let (Some(object), Some((key, value))) = (body.as_object_mut(), light.clone()) {
            object.insert(key.to_string(), value);
        }
        let mut response = self.send_openai_stream(&url, &body).await?;
        if let Some((key, _)) = light
            && ProviderCallFailure::http(response.status(), anyhow::anyhow!("refused"))
                .rejected_request()
        {
            if let Some(object) = body.as_object_mut() {
                object.remove(key);
            }
            response = self.send_openai_stream(&url, &body).await?;
            // Only when that was the difference: an image the model cannot
            // take is refused either way.
            if response.status().is_success() {
                self.remember_refusal(RequestShape::LightThinking);
            }
        }
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
            )
            .into_error());
        }
        Ok(response)
    }

    async fn send_openai_stream(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        self.authenticate(self.client.post(url))
            .header("Content-Type", "application/json")
            .json(&self.prepare_body(body)?)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Failed to send streaming request to OpenAI-compatible API (endpoint: {url}, model: {})",
                    self.model
                )
            })
    }

    pub async fn analyze_profile(&self, profile_data: &serde_json::Value) -> Result<String> {
        let prompt = require_analyze_prompt(profile_data)?;
        let input_chars = serde_json::to_string(profile_data)
            .map(|s| s.len())
            .unwrap_or(0);
        let result = match self.provider {
            AiProvider::Gemini => self.analyze_with_gemini(prompt).await,
            AiProvider::OpenAI => self.analyze_with_openai(prompt).await,
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                self.analyze_with_text_protocol("", &[ChatMessage::user(prompt)])
                    .await
            }
        };
        self.note_ledger(input_chars, &result).await;
        result
    }

    /// Best-effort site-wide cost ledger. Writes even without attribution
    /// (`source=internal`). Governed Tapp+scheduler paths suppress this hook.
    async fn note_ledger(&self, input_chars: usize, result: &Result<String, anyhow::Error>) {
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
            Err(error) => {
                #[cfg(test)]
                crate::services::ai_cost_ledger::spent::because(&self.model, error);
                #[cfg(not(test))]
                let _ = error;
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

        let url = self.gemini_url(false).await;

        tracing::info!("🔗 Calling Gemini API (model: {})", self.model);

        let response = self
            .authenticate(self.client.post(&url))
            .json(&self.prepare_body(&request_body)?)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = Self::read_limited_error_text(response).await;
            return Err(ProviderCallFailure::http(
                status,
                anyhow::anyhow!("Gemini API error {}: {}", status, error_text),
            )
            .into_error());
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
            .authenticate(self.client.post(&url))
            .header("Content-Type", "application/json")
            .json(&self.prepare_body(&request_body)?)
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
            return Err(ProviderCallFailure::http(
                status,
                anyhow::anyhow!(format_openai_compatible_http_error(
                    status,
                    &url,
                    &self.model,
                    &error_text,
                )),
            )
            .into_error());
        }

        let openai_response: OpenAIResponse =
            Self::read_limited_json(response, 2 * 1024 * 1024).await?;

        extract_openai_completion_text(&openai_response)
    }

    async fn send_text_protocol(
        &self,
        system: &str,
        messages: &[ChatMessage],
        response_format: Option<serde_json::Value>,
        max_tokens: Option<u32>,
    ) -> std::result::Result<String, ProviderCallFailure> {
        let url = text_protocol::endpoint(self.provider, self.base_url.as_deref());
        let deep = self
            .deep_thinking_param()
            .filter(|_| self.provider == AiProvider::Anthropic);
        // Thinking counts against the output limit: room for it.
        let max_tokens = max_tokens.or(deep.as_ref().map(|_| DEEP_THINKING_MAX_TOKENS));
        let mut body = text_protocol::request_body(
            self.provider,
            &self.model,
            system,
            messages,
            response_format,
            max_tokens,
            false,
        );
        if let (Some(object), Some((key, value))) = (body.as_object_mut(), deep) {
            object.insert(key.to_string(), value);
            // Haiku 5.5 already thinks adaptively by default; models such
            // as Sonnet 4.6 require it to be enabled explicitly.
            object.insert("thinking".into(), serde_json::json!({ "type": "adaptive" }));
        }
        let mut request = self.authenticate(self.client.post(&url));
        if self.provider == AiProvider::Anthropic {
            request = request.header("anthropic-version", "2023-06-01");
        }
        let response = request
            .header("Content-Type", "application/json")
            .json(&self.prepare_body(&body)?)
            .send()
            .await
            .map_err(|error| ProviderCallFailure::transport(error.into()))?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(ProviderCallFailure::http(
                status,
                anyhow::anyhow!("AI provider API returned HTTP {status}"),
            ));
        }
        let body: serde_json::Value = Self::read_limited_json(response, 2 * 1024 * 1024)
            .await
            .map_err(ProviderCallFailure::transport)?;
        text_protocol::response_text(self.provider, &body).map_err(ProviderCallFailure::transport)
    }

    async fn analyze_with_text_protocol(
        &self,
        system: &str,
        messages: &[ChatMessage],
    ) -> Result<String> {
        self.send_text_protocol(system, messages, None, None)
            .await
            .map_err(ProviderCallFailure::into_error)
    }

    /// prompt-only wrapper around `analyze_profile`.
    pub async fn analyze(&self, prompt: &str) -> Result<String> {
        let data = serde_json::json!({ "prompt": prompt });
        self.analyze_profile(&data).await
    }

    /// system + one user message.
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
                    .authenticate(self.client.post(&url))
                    .header("Content-Type", "application/json")
                    .json(&self.prepare_body(&request_body)?)
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
                            Err(ProviderCallFailure::http(
                                status,
                                anyhow::anyhow!(format_openai_compatible_http_error(
                                    status,
                                    &url,
                                    &self.model,
                                    &error_text,
                                )),
                            )
                            .into_error())
                        } else {
                            let openai_response: OpenAIResponse =
                                Self::read_limited_json(response, 2 * 1024 * 1024).await?;
                            extract_openai_completion_text(&openai_response)
                        }
                    }
                    Err(e) => Err(e),
                };
                self.note_ledger(input_chars, &result).await;
                result
            }
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                let input_chars =
                    system.len() + messages.iter().map(|m| m.content.len()).sum::<usize>();
                let result = self.analyze_with_text_protocol(system, &messages).await;
                self.note_ledger(input_chars, &result).await;
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
    /// parameters outright fall back to a plain prompt-only call, so a
    /// non-conforming OpenAI-compatible endpoint does not fail the request.
    ///
    /// Returns model text. This fn does not parse. Structured asks the provider
    /// for JSON; PromptOnly does not.
    pub async fn analyze_json(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String> {
        let input_chars = system.len() + prompt.len();

        let mode = if self.provider == AiProvider::Anthropic
            || self.refused(RequestShape::StructuredOutput)
        {
            JsonMode::PromptOnly(schema)
        } else {
            JsonMode::Structured(schema)
        };
        let deep = self.deep_thinking_param().is_some();
        let light = self.light_thinking_param().is_some();
        let mut result = self
            .analyze_json_inner(system, prompt, schema_name, mode, None)
            .await;

        // Drop a refused thinking parameter before trying without JSON
        // enforcement: rejecting one says nothing about the other.
        if deep
            && result
                .as_ref()
                .is_err_and(ProviderCallFailure::rejected_request)
        {
            self.remember_refusal(RequestShape::DeepThinking);
            result = self
                .analyze_json_inner(system, prompt, schema_name, mode, None)
                .await;
        }

        // The think-little parameter goes first: a refusal of it says
        // nothing about structured output.
        if light
            && result
                .as_ref()
                .is_err_and(ProviderCallFailure::rejected_request)
        {
            self.remember_refusal(RequestShape::LightThinking);
            result = self
                .analyze_json_inner(system, prompt, schema_name, mode, None)
                .await;
        }

        if let Err(ref failure) = result {
            if matches!(mode, JsonMode::Structured(_)) && failure.rejected_request() {
                self.remember_refusal(RequestShape::StructuredOutput);
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

        let result = result.map_err(ProviderCallFailure::into_error);
        crate::services::ai_cost_ledger::as_step(
            schema_name,
            self.note_ledger(input_chars, &result),
        )
        .await;
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
        let mode = if self.provider == AiProvider::Anthropic
            || self.refused(RequestShape::StructuredOutput)
        {
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
            // The same parameter would ride again without the budget.
            if self.light_thinking {
                self.remember_refusal(RequestShape::LightThinking);
            }
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

        let result = result.map_err(ProviderCallFailure::into_error);
        crate::services::ai_cost_ledger::as_step(
            schema_name,
            self.note_ledger(input_chars, &result),
        )
        .await;
        result
    }

    /// Structured JSON, but reasoning deltas are pushed live.
    /// OpenAI-compatible providers that reject `stream` + `response_format`
    /// fall back to the blocking call.
    #[cfg(test)]
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
        if self.provider == AiProvider::Gemini {
            return self.analyze_json(system, prompt, schema_name, schema).await;
        }

        let input_chars = system.len() + prompt.len();
        let streamed = if self.provider == AiProvider::OpenAI {
            self.analyze_json_streaming_openai(system, prompt, schema_name, schema, on_delta)
                .await
        } else {
            self.analyze_json_streaming_protocol(system, prompt, schema_name, schema, on_delta)
                .await
        };
        match streamed {
            Ok(text) => {
                let result = Ok(text);
                self.note_ledger(input_chars, &result).await;
                result
            }
            Err(failure) if failure.rejected_request() => {
                tracing::warn!(
                    model = %self.model,
                    "[AiAnalyzer] Streaming structured output rejected; falling back"
                );
                self.analyze_json(system, prompt, schema_name, schema).await
            }
            Err(failure) => Err(failure.into_error()),
        }
    }

    #[cfg(test)]
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
            .authenticate(self.client.post(&url))
            .header("Content-Type", "application/json")
            .json(&self.prepare_body(&request_body)?)
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

    #[cfg(test)]
    async fn analyze_json_streaming_protocol<F, Fut>(
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
        let prompt = if self.provider == AiProvider::Anthropic {
            JsonMode::PromptOnly(schema).decorate_prompt(prompt)
        } else {
            prompt.to_string()
        };
        let format = (self.provider == AiProvider::OpenAIResponses)
            .then(|| JsonMode::Structured(schema).openai_response_format(schema_name))
            .flatten();
        let messages = vec![ChatMessage::user(prompt)];
        let url = text_protocol::endpoint(self.provider, self.base_url.as_deref());
        let body = text_protocol::request_body(
            self.provider,
            &self.model,
            system,
            &messages,
            format,
            None,
            true,
        );
        let mut request = self.authenticate(self.client.post(&url));
        if self.provider == AiProvider::Anthropic {
            request = request.header("anthropic-version", "2023-06-01");
        }
        let response = request
            .header("Content-Type", "application/json")
            .json(&self.prepare_body(&body)?)
            .send()
            .await
            .map_err(|error| ProviderCallFailure::transport(error.into()))?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(ProviderCallFailure::http(
                status,
                anyhow::anyhow!("AI provider streaming API returned HTTP {status}"),
            ));
        }

        super::sse::consume_text_sse(response, on_delta, self.protocol_stream_extractor())
            .await
            .map_err(ProviderCallFailure::transport)
    }

    fn protocol_stream_extractor(&self) -> fn(&serde_json::Value) -> Vec<StreamDelta> {
        match self.provider {
            AiProvider::Anthropic => {
                |body| text_protocol::stream_deltas(AiProvider::Anthropic, body)
            }
            _ => super::openai::openai_stream_deltas,
        }
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

                let url = self.gemini_url(false).await;
                let response = self
                    .authenticate(self.client.post(&url))
                    .json(&self.prepare_body(&request_body)?)
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
                // 「少想」和输出上限同进同退：它们同属「附加参数」这一类，
                // 被拒时一起丢掉，也一起被记住。没有输出上限时，只有要求
                // 少想的分析器才发（单独记忆，见 `RequestShape::LightThinking`）。
                let mut request_body = serde_json::to_value(request_body)
                    .map_err(|e| ProviderCallFailure::transport(e.into()))?;
                let thinking = if budget.is_some() {
                    self.gateway().light_thinking()
                } else {
                    self.thinking_param()
                };
                if let (Some(object), Some((key, value))) = (request_body.as_object_mut(), thinking)
                {
                    object.insert(key.to_string(), value);
                }

                let url = openai_chat_completions_url(self.base_url.as_deref());
                let response = self
                    .authenticate(self.client.post(&url))
                    .header("Content-Type", "application/json")
                    .json(&self.prepare_body(&request_body)?)
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
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                let (prompt, response_format) = match mode {
                    JsonMode::Structured(schema)
                        if self.provider == AiProvider::OpenAIResponses =>
                    {
                        (
                            prompt.to_string(),
                            JsonMode::Structured(schema).openai_response_format(schema_name),
                        )
                    }
                    JsonMode::Structured(schema) => {
                        (JsonMode::PromptOnly(schema).decorate_prompt(prompt), None)
                    }
                    JsonMode::PromptOnly(schema) => {
                        (JsonMode::PromptOnly(schema).decorate_prompt(prompt), None)
                    }
                };
                self.send_text_protocol(
                    system,
                    &[ChatMessage::user(prompt)],
                    response_format,
                    budget.map(|value| value.max_tokens),
                )
                .await
            }
        }
    }

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
    pub async fn analyze_stream_parts<F, Fut>(&self, prompt: &str, on_delta: F) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        let input_chars = prompt.len();
        let result = self.analyze_stream_inner(prompt, on_delta).await;
        self.note_ledger(input_chars, &result).await;
        result
    }

    /// [`Self::analyze_stream_parts`] with images the model sees alongside
    /// the prompt. Gemini gets `inlineData` parts, OpenAI-compatible endpoints
    /// a content array with data-URL `image_url`s, and Anthropic base64
    /// `image` blocks; other protocols get the text alone. No images: exactly
    /// the text path.
    pub async fn analyze_stream_parts_with_images<F, Fut>(
        &self,
        prompt: &str,
        images: &[ImageInput],
        on_delta: F,
    ) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        if images.is_empty()
            || !matches!(
                self.provider,
                AiProvider::Gemini | AiProvider::OpenAI | AiProvider::Anthropic
            )
        {
            if !images.is_empty() {
                tracing::info!(
                    provider = ?self.provider,
                    "images not supported on this protocol; sending text only"
                );
            }
            return self.analyze_stream_parts(prompt, on_delta).await;
        }
        let input_chars = prompt.len() + images.len() * IMAGE_INPUT_CHARS;
        let result = self.analyze_stream_images(prompt, images, on_delta).await;
        self.note_ledger(input_chars, &result).await;
        result
    }

    /// [`Self::analyze_stream_parts_with_images`] for a conversation: a
    /// system prompt and the messages in order, user / assistant, with
    /// `images` on the last of them. Every protocol takes them as messages
    /// (Gemini as `systemInstruction` and turns). A single user message and
    /// no system prompt: exactly the prompt path.
    pub async fn analyze_stream_messages_with_images<F, Fut>(
        &self,
        system: &str,
        messages: &[ChatMessage],
        images: &[ImageInput],
        on_delta: F,
    ) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        if system.trim().is_empty()
            && let [only] = messages
            && only.role == "user"
        {
            return self
                .analyze_stream_parts_with_images(&only.content, images, on_delta)
                .await;
        }
        let images = if images.is_empty()
            || matches!(
                self.provider,
                AiProvider::Gemini | AiProvider::OpenAI | AiProvider::Anthropic
            ) {
            images
        } else {
            tracing::info!(
                provider = ?self.provider,
                "images not supported on this protocol; sending text only"
            );
            &[]
        };
        let input_chars = system.len()
            + messages
                .iter()
                .map(|message| message.content.len())
                .sum::<usize>()
            + images.len() * IMAGE_INPUT_CHARS;
        let result = self
            .analyze_stream_messages(system, messages, images, on_delta)
            .await;
        self.note_ledger(input_chars, &result).await;
        result
    }

    async fn analyze_stream_messages<F, Fut>(
        &self,
        system: &str,
        messages: &[ChatMessage],
        images: &[ImageInput],
        on_delta: F,
    ) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        match self.provider {
            AiProvider::Gemini => {
                let body = gemini_messages_request(system, messages, images);
                let url = self.gemini_url(true).await;
                let response = self
                    .authenticate(self.client.post(&url))
                    .json(&self.prepare_body(&body)?)
                    .send()
                    .await
                    .context("Failed to send streaming request to Gemini API")?;
                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!("Gemini streaming API error {}: {}", status, error_text),
                    )
                    .into_error());
                }
                super::sse::consume_text_sse(response, on_delta, gemini_stream_deltas).await
            }
            AiProvider::OpenAI => {
                let response = self
                    .post_openai_stream(openai_messages_request(
                        &self.model,
                        system,
                        messages,
                        images,
                    ))
                    .await?;
                consume_openai_sse(response, on_delta).await
            }
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                let url = text_protocol::endpoint(self.provider, self.base_url.as_deref());
                let mut body = text_protocol::request_body(
                    self.provider,
                    &self.model,
                    system,
                    messages,
                    None,
                    None,
                    true,
                );
                if !images.is_empty() {
                    anthropic_images_on_last(&mut body, images);
                }
                let mut request = self.authenticate(self.client.post(url));
                if self.provider == AiProvider::Anthropic {
                    request = request.header("anthropic-version", "2023-06-01");
                }
                let response = request.json(&self.prepare_body(&body)?).send().await?;
                if !response.status().is_success() {
                    let status = response.status();
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!("AI provider streaming API returned HTTP {status}"),
                    )
                    .into_error());
                }
                super::sse::consume_text_sse(response, on_delta, self.protocol_stream_extractor())
                    .await
            }
        }
    }

    async fn analyze_stream_images<F, Fut>(
        &self,
        prompt: &str,
        images: &[ImageInput],
        on_delta: F,
    ) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
        if self.provider == AiProvider::Gemini {
            let body = gemini_image_request(prompt, images);
            let url = self.gemini_url(true).await;
            let response = self
                .authenticate(self.client.post(&url))
                .json(&self.prepare_body(&body)?)
                .send()
                .await
                .context("Failed to send streaming image request to Gemini API")?;
            if !response.status().is_success() {
                let status = response.status();
                let error_text = Self::read_limited_error_text(response).await;
                return Err(ProviderCallFailure::http(
                    status,
                    anyhow::anyhow!("Gemini streaming API error {}: {}", status, error_text),
                )
                .into_error());
            }
            return super::sse::consume_text_sse(response, on_delta, gemini_stream_deltas).await;
        }
        if self.provider == AiProvider::Anthropic {
            let url = text_protocol::endpoint(self.provider, self.base_url.as_deref());
            let response = self
                .authenticate(self.client.post(&url))
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&self.prepare_body(&anthropic_image_request(&self.model, prompt, images))?)
                .send()
                .await
                .context("Failed to send streaming image request to Anthropic API")?;
            if !response.status().is_success() {
                let status = response.status();
                let error_text = Self::read_limited_error_text(response).await;
                return Err(ProviderCallFailure::http(
                    status,
                    anyhow::anyhow!("Anthropic streaming API error {}: {}", status, error_text),
                )
                .into_error());
            }
            return super::sse::consume_text_sse(
                response,
                on_delta,
                self.protocol_stream_extractor(),
            )
            .await;
        }
        let response = self
            .post_openai_stream(openai_image_request(&self.model, prompt, images))
            .await?;
        consume_openai_sse(response, on_delta).await
    }

    async fn analyze_stream_inner<F, Fut>(&self, prompt: &str, on_delta: F) -> Result<String>
    where
        F: FnMut(StreamDelta) -> Fut + Send,
        Fut: Future<Output = bool> + Send,
    {
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

                let url = self.gemini_url(true).await;

                let response = self
                    .authenticate(self.client.post(&url))
                    .json(&self.prepare_body(&request_body)?)
                    .send()
                    .await
                    .context("Failed to send streaming request to Gemini API")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!("Gemini streaming API error {}: {}", status, error_text),
                    )
                    .into_error());
                }

                super::sse::consume_text_sse(response, on_delta, gemini_stream_deltas).await
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
                let response = self
                    .post_openai_stream(serde_json::to_value(request_body)?)
                    .await?;
                consume_openai_sse(response, on_delta).await
            }
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                let url = text_protocol::endpoint(self.provider, self.base_url.as_deref());
                let body = text_protocol::request_body(
                    self.provider,
                    &self.model,
                    "",
                    &[ChatMessage::user(prompt)],
                    None,
                    None,
                    true,
                );
                let mut request = self.authenticate(self.client.post(url));
                if self.provider == AiProvider::Anthropic {
                    request = request.header("anthropic-version", "2023-06-01");
                }
                let response = request.json(&self.prepare_body(&body)?).send().await?;
                if !response.status().is_success() {
                    let status = response.status();
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!("AI provider streaming API returned HTTP {status}"),
                    )
                    .into_error());
                }
                super::sse::consume_text_sse(response, on_delta, self.protocol_stream_extractor())
                    .await
            }
        }
    }
}

/// The ledger counts characters (about four per token); an image of this
/// size costs roughly a thousand input tokens.
const IMAGE_INPUT_CHARS: usize = 4_000;

fn gemini_image_request(prompt: &str, images: &[ImageInput]) -> serde_json::Value {
    let mut parts = vec![serde_json::json!({ "text": prompt })];
    parts.extend(images.iter().map(|image| {
        serde_json::json!({ "inlineData": { "mimeType": image.mime, "data": image.base64 } })
    }));
    serde_json::json!({ "contents": [{ "parts": parts }] })
}

/// The Messages API takes images as base64 `image` blocks, before the text
/// that asks about them.
fn anthropic_image_request(model: &str, prompt: &str, images: &[ImageInput]) -> serde_json::Value {
    let mut content: Vec<serde_json::Value> = images
        .iter()
        .map(|image| {
            serde_json::json!({
                "type": "image",
                "source": { "type": "base64", "media_type": image.mime, "data": image.base64 }
            })
        })
        .collect();
    content.push(serde_json::json!({ "type": "text", "text": prompt }));
    serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "stream": true,
        "messages": [{ "role": "user", "content": content }],
    })
}

fn openai_image_request(model: &str, prompt: &str, images: &[ImageInput]) -> serde_json::Value {
    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    content.extend(images.iter().map(|image| {
        serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:{};base64,{}", image.mime, image.base64) }
        })
    }));
    serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": content }],
        "stream": true,
    })
}

/// A conversation as Gemini takes it: the system prompt as its
/// `systemInstruction`, the messages as turns (hers the model's), `images`
/// after the last one's text.
fn gemini_messages_request(
    system: &str,
    messages: &[ChatMessage],
    images: &[ImageInput],
) -> serde_json::Value {
    let last = messages.len().saturating_sub(1);
    let contents: Vec<serde_json::Value> = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let role = if message.role == "assistant" {
                "model"
            } else {
                "user"
            };
            let mut parts = vec![serde_json::json!({ "text": message.content })];
            if index == last {
                parts.extend(images.iter().map(|image| {
                    serde_json::json!({ "inlineData": { "mimeType": image.mime, "data": image.base64 } })
                }));
            }
            serde_json::json!({ "role": role, "parts": parts })
        })
        .collect();
    let mut body = serde_json::json!({ "contents": contents });
    if !system.trim().is_empty() {
        body["systemInstruction"] = serde_json::json!({ "parts": [{ "text": system }] });
    }
    body
}

/// A conversation on an OpenAI-compatible endpoint: the system prompt
/// first, the messages after it, `images` beside the last one's text.
fn openai_messages_request(
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    images: &[ImageInput],
) -> serde_json::Value {
    let last = messages.len().saturating_sub(1);
    let mut sent = Vec::with_capacity(messages.len() + 1);
    if !system.trim().is_empty() {
        sent.push(serde_json::json!({ "role": "system", "content": system }));
    }
    for (index, message) in messages.iter().enumerate() {
        let role = match message.role.as_str() {
            "assistant" => "assistant",
            "system" => "system",
            _ => "user",
        };
        let content = if index == last && !images.is_empty() {
            let mut content = vec![serde_json::json!({ "type": "text", "text": message.content })];
            content.extend(images.iter().map(|image| {
                serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{};base64,{}", image.mime, image.base64) }
                })
            }));
            serde_json::Value::Array(content)
        } else {
            serde_json::json!(message.content)
        };
        sent.push(serde_json::json!({ "role": role, "content": content }));
    }
    serde_json::json!({ "model": model, "messages": sent, "stream": true })
}

/// `images` as base64 `image` blocks before the text of the last message of
/// a Messages API body.
fn anthropic_images_on_last(body: &mut serde_json::Value, images: &[ImageInput]) {
    let Some(last) = body
        .get_mut("messages")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|messages| messages.last_mut())
    else {
        return;
    };
    let text = last["content"].take();
    let mut content: Vec<serde_json::Value> = images
        .iter()
        .map(|image| {
            serde_json::json!({
                "type": "image",
                "source": { "type": "base64", "media_type": image.mime, "data": image.base64 }
            })
        })
        .collect();
    content.push(serde_json::json!({ "type": "text", "text": text }));
    last["content"] = serde_json::Value::Array(content);
}

#[cfg(test)]
mod image_request_tests {
    use super::*;

    fn image() -> ImageInput {
        ImageInput {
            mime: "image/webp".into(),
            base64: "AAAA".into(),
        }
    }

    /// A conversation goes as messages on every protocol: the system prompt
    /// apart, hers as hers, an image beside the last one's text.
    #[test]
    fn a_conversation_goes_as_messages_with_images_on_the_last() {
        let talk = [
            ChatMessage::user("早"),
            ChatMessage::assistant("早啊"),
            ChatMessage::user("看看这个"),
        ];
        let openai = openai_messages_request("m", "你是瞳。", &talk, &[image()]);
        let sent = openai["messages"].as_array().unwrap();
        assert_eq!(
            sent.iter()
                .map(|message| message["role"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["system", "user", "assistant", "user"]
        );
        assert_eq!(sent[0]["content"], "你是瞳。");
        assert_eq!(sent[2]["content"], "早啊");
        assert_eq!(sent[3]["content"][0]["text"], "看看这个");
        assert_eq!(sent[3]["content"][1]["type"], "image_url");
        assert_eq!(openai["stream"], true);

        let gemini = gemini_messages_request("你是瞳。", &talk, &[image()]);
        assert_eq!(gemini["systemInstruction"]["parts"][0]["text"], "你是瞳。");
        let turns = gemini["contents"].as_array().unwrap();
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn["role"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["user", "model", "user"]
        );
        assert_eq!(turns[2]["parts"][1]["inlineData"]["mimeType"], "image/webp");
        assert!(turns[1]["parts"].as_array().unwrap().len() == 1);
        assert!(
            gemini_messages_request("", &talk, &[])
                .get("systemInstruction")
                .is_none()
        );

        let mut anthropic = text_protocol::request_body(
            AiProvider::Anthropic,
            "m",
            "你是瞳。",
            &talk,
            None,
            None,
            true,
        );
        anthropic_images_on_last(&mut anthropic, &[image()]);
        assert_eq!(anthropic["system"], "你是瞳。");
        let sent = anthropic["messages"].as_array().unwrap();
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[1]["content"], "早啊");
        assert_eq!(sent[2]["content"][0]["type"], "image");
        assert_eq!(sent[2]["content"][1]["text"], "看看这个");
    }

    #[tokio::test]
    async fn light_thinking_is_asked_only_when_wanted_and_only_where_known() {
        let router = || {
            AiAnalyzer::new(
                AiProvider::OpenAI,
                String::new(),
                "light-thinking-test".into(),
                Some("https://openrouter.ai/api/v1".into()),
            )
        };
        assert!(router().await.light_thinking_param().is_none());
        assert_eq!(
            router().await.with_light_thinking().light_thinking_param(),
            Some(("reasoning", serde_json::json!({ "effort": "low" })))
        );
        let unknown = AiAnalyzer::new(
            AiProvider::OpenAI,
            String::new(),
            "light-thinking-test".into(),
            Some("https://llm.example/v1".into()),
        )
        .await
        .with_light_thinking();
        assert!(unknown.light_thinking_param().is_none());
    }

    #[tokio::test]
    async fn deep_thinking_is_asked_in_each_providers_own_words() {
        let router = AiAnalyzer::new(
            AiProvider::OpenAI,
            String::new(),
            "deep-thinking-test".into(),
            Some("https://openrouter.ai/api/v1".into()),
        )
        .await
        .with_light_thinking()
        .with_deep_thinking();
        assert_eq!(
            router.thinking_param(),
            Some(("reasoning", serde_json::json!({ "effort": "high" })))
        );
        let anthropic = AiAnalyzer::new(
            AiProvider::Anthropic,
            String::new(),
            "deep-thinking-test".into(),
            None,
        )
        .await;
        assert!(anthropic.deep_thinking_param().is_none());
        assert_eq!(
            anthropic.with_deep_thinking().deep_thinking_param(),
            Some(("output_config", serde_json::json!({ "effort": "high" })))
        );
        let unknown = AiAnalyzer::new(
            AiProvider::OpenAI,
            String::new(),
            "deep-thinking-test".into(),
            Some("https://llm.example/v1".into()),
        )
        .await
        .with_deep_thinking();
        assert!(unknown.thinking_param().is_none());
    }

    #[test]
    fn images_ride_beside_the_prompt_in_each_protocol() {
        let gemini = gemini_image_request("看看这个", &[image()]);
        assert_eq!(gemini["contents"][0]["parts"][0]["text"], "看看这个");
        assert_eq!(
            gemini["contents"][0]["parts"][1]["inlineData"]["mimeType"],
            "image/webp"
        );
        let anthropic = anthropic_image_request("m", "看看这个", &[image()]);
        let blocks = &anthropic["messages"][0]["content"];
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["source"]["media_type"], "image/webp");
        assert_eq!(blocks[0]["source"]["data"], "AAAA");
        assert_eq!(blocks[1]["text"], "看看这个");
        assert_eq!(anthropic["stream"], true);
        let openai = openai_image_request("m", "看看这个", &[image()]);
        let content = &openai["messages"][0]["content"];
        assert_eq!(content[0]["text"], "看看这个");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/webp;base64,AAAA"
        );
        assert_eq!(openai["stream"], true);
        assert!(
            !format!("{:?}", image()).contains("AAAA"),
            "no image data in logs"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderCallFailure, require_analyze_prompt};
    use serde_json::json;

    /// 只在请求没写上限时补上；写了的不动，也不给别的协议补字段。
    #[tokio::test]
    async fn the_output_cap_fills_only_an_unnamed_limit() {
        use super::{AiAnalyzer, AiProvider};
        let analyzer = |provider| async move {
            AiAnalyzer::new(provider, "fixture".into(), "fixture".into(), None)
                .await
                .with_output_cap(8192)
        };
        let openai = analyzer(AiProvider::OpenAI).await;
        let unnamed = json!({"model":"fixture","messages":[]});
        assert_eq!(openai.prepare_body(&unnamed).unwrap()["max_tokens"], 8192);
        let named = json!({"model":"fixture","messages":[],"max_tokens":300});
        assert_eq!(openai.prepare_body(&named).unwrap()["max_tokens"], 300);
        let completion = json!({"model":"fixture","messages":[],"max_completion_tokens":300});
        assert!(
            openai
                .prepare_body(&completion)
                .unwrap()
                .get("max_tokens")
                .is_none()
        );
        let gemini = analyzer(AiProvider::Gemini).await;
        assert!(
            gemini
                .prepare_body(&unnamed)
                .unwrap()
                .get("max_tokens")
                .is_none()
        );
    }

    /// 预算被拒 → 去掉预算重试 → 还被拒 → prompt-only。三级，不能少。
    #[test]
    fn the_short_call_has_a_three_step_ladder() {
        let source = include_str!("client.rs");
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

    /// 关思考跟着输出上限走：被拒时一起丢，不会只丢一半。
    #[test]
    fn the_thinking_switch_rides_with_the_budget() {
        let source = include_str!("client.rs");
        let inner = source
            .split("async fn analyze_json_inner(")
            .nth(1)
            .and_then(|rest| rest.split("\n    pub async fn analyze_stream").next())
            .expect("analyze_json_inner body");
        // 只在带预算的那一档附上，跟着一起丢。
        assert!(inner.contains("if budget.is_some()"));
        assert!(inner.contains("self.gateway().light_thinking()"));
    }

    /// 只在「请求形状被拒」时记忆——鉴权失败和限流不是能力问题，记下来会让
    /// 一次配错的 key 永久关掉这个端点的结构化输出。
    #[test]
    fn the_memo_is_keyed_on_endpoint_and_model_and_only_on_shape_refusals() {
        let source = include_str!("client.rs");
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
    fn auth_credit_rate_limit_and_transport_failures_do_not_retry() {
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::PAYMENT_REQUIRED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::REQUEST_TIMEOUT,
            reqwest::StatusCode::PAYLOAD_TOO_LARGE,
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
}

#[cfg(test)]
mod failure_contract_tests {
    use super::*;

    #[test]
    fn public_errors_keep_http_status_and_source_through_context() {
        let error = ProviderCallFailure::http(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            anyhow::anyhow!("upstream unavailable"),
        )
        .into_error()
        .context("outer call");
        let typed = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<ProviderHttpError>())
            .unwrap();
        assert_eq!(typed.status, reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            error
                .chain()
                .any(|cause| cause.to_string() == "upstream unavailable")
        );
    }
}
