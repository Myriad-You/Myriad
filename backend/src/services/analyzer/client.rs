use anyhow::{Context, Result};
use reqwest::Client;
use serde::Serialize;
use std::collections::HashSet;
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
use super::transport;
use super::types::{
    gateway_of, AiProvider, ChatMessage, Gateway, GeminiContent, GeminiPart, GeminiRequest,
    GeminiResponse, OpenAIMessage, OpenAIRequest, OpenAIResponse, OutputBudget, StreamDelta,
};

pub struct AiAnalyzer {
    pub(super) client: Client,
    provider: AiProvider,
    pub(super) api_key: String,
    pub(super) model: String,
    pub(super) base_url: Option<String>, // For OpenAI-compatible APIs
}

/// 进程内记下 (base_url, model, structured|extras) 曾被 `rejected_request`；重启清空。
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

    /// 4xx except 401/403/429. No status (transport) is not a shape refusal.
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

        let mode = if self.refused(RequestShape::StructuredOutput) {
            JsonMode::PromptOnly(schema)
        } else {
            JsonMode::Structured(schema)
        };
        let mut result = self
            .analyze_json_inner(system, prompt, schema_name, mode, None)
            .await;

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
    use super::{require_analyze_prompt, ProviderCallFailure};
    use serde_json::json;

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
        assert!(inner.contains("self.gateway().thinking_off()"));
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
}
