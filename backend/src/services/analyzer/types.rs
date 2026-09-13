use serde::{Deserialize, Serialize};

// Gemini API Structures
#[derive(Debug, Serialize)]
pub(super) struct GeminiRequest {
    pub(super) contents: Vec<GeminiContent>,
    /// Set when structured output or an output budget is present; omitted on ordinary calls.
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    pub(super) generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize)]
pub(super) struct GeminiGenerationConfig {
    #[serde(rename = "responseMimeType")]
    pub(super) response_mime_type: String,
    #[serde(rename = "responseSchema", skip_serializing_if = "Option::is_none")]
    pub(super) response_schema: Option<serde_json::Value>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    pub(super) max_output_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(super) struct GeminiContent {
    pub(super) parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
pub(super) struct GeminiPart {
    pub(super) text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiResponse {
    #[serde(default)]
    pub(super) candidates: Vec<GeminiCandidate>,
    #[serde(default, rename = "promptFeedback")]
    pub(super) prompt_feedback: Option<GeminiPromptFeedback>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiPromptFeedback {
    #[serde(default, rename = "blockReason")]
    pub(super) block_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiCandidate {
    pub(super) content: Option<GeminiCandidateContent>,
    #[serde(default, rename = "finishReason")]
    pub(super) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiCandidateContent {
    #[serde(default)]
    pub(super) parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
pub(super) struct GeminiResponsePart {
    #[serde(default)]
    pub(super) text: String,
}

// OpenAI-compatible API Structures
#[derive(Debug, Serialize)]
pub(super) struct OpenAIRequest {
    pub(super) model: String,
    pub(super) messages: Vec<OpenAIMessage>,
    /// Only set for structured-output requests; omitted on ordinary calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) response_format: Option<serde_json::Value>,
    /// Only set for bounded calls; see [`OutputBudget`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) max_tokens: Option<u32>,
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
pub(super) struct OpenAIMessage {
    pub(super) role: String,
    pub(super) content: String,
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
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAIResponse {
    #[serde(default)]
    pub(super) choices: Vec<OpenAIChoice>,
    /// OpenRouter and some gateways return HTTP 200 with a top-level error object.
    #[serde(default)]
    pub(super) error: Option<OpenAIErrorBody>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAIErrorBody {
    #[serde(default)]
    pub(super) message: Option<String>,
    #[serde(default)]
    pub(super) code: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAIChoice {
    #[serde(default)]
    pub(super) message: OpenAIResponseMessage,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct OpenAIResponseMessage {
    /// Providers may return `null` for content when only reasoning is filled.
    #[serde(default)]
    pub(super) content: Option<String>,
    /// OpenRouter / DeepSeek-style reasoning fields (optional, never required).
    #[serde(default)]
    pub(super) reasoning_content: Option<String>,
    #[serde(default)]
    pub(super) reasoning: Option<String>,
}

/// One piece of a streamed completion: the visible reply, or the hidden chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamDelta {
    Text(String),
    Reasoning(String),
}

pub(super) fn json_token(value: &serde_json::Value) -> Option<&str> {
    value.as_str().filter(|s| !s.is_empty())
}

// AI Provider Enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    Gemini,
    OpenAI,
    OpenAIResponses,
    Anthropic,
}

impl AiProvider {
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.to_lowercase().as_str() {
            "gemini" => Ok(Self::Gemini),
            "openai" | "openrouter" => Ok(Self::OpenAI),
            "openai_responses" => Ok(Self::OpenAIResponses),
            "anthropic" | "anthropic_messages" => Ok(Self::Anthropic),
            _ => Err(anyhow::anyhow!("unsupported AI provider: {s}")),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::OpenAIResponses => "openai_responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }
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
    pub(super) fn thinking_off(self) -> Option<(&'static str, serde_json::Value)> {
        match self {
            Self::OpenRouter => Some(("reasoning", serde_json::json!({ "enabled": false }))),
            Self::OpenAi | Self::Gemini | Self::OpenAiCompatible => None,
        }
    }
}

pub(super) fn gateway_of(provider: AiProvider, base_url: Option<&str>) -> Gateway {
    if provider == AiProvider::Gemini {
        return Gateway::Gemini;
    }
    if provider == AiProvider::Anthropic {
        return Gateway::OpenAiCompatible;
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

#[cfg(test)]
mod tests {
    use super::{
        AiProvider, Gateway, GeminiContent, GeminiPart, GeminiRequest, OpenAIMessage,
        OpenAIRequest, gateway_of,
    };
    use serde_json::json;

    #[test]
    fn ai_provider_rejects_unknown_values() {
        let error = AiProvider::from_str("unknown-provider").unwrap_err();
        assert_eq!(
            error.to_string(),
            "unsupported AI provider: unknown-provider"
        );
    }

    #[test]
    fn the_gateway_is_recovered_from_the_base_url() {
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

    /// 「别思考」只发有把握的那一家，其余留空。
    ///
    /// 各家参数名不同，发一个猜来的比不发更糟：网关不认就是 4xx，多一个来回。
    /// 有了记忆之后代价从「每次」降到「一次」，但那不是乱发的理由——不确定
    /// 就返回 None。
    #[test]
    fn only_a_gateway_we_are_sure_about_gets_a_thinking_switch() {
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
}
