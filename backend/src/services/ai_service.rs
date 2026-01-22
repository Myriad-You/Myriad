//! AI 服务模块
//!
//! 提供与 Gemini AI 的交互功能，用于 Brewlia 词汇注释等 AI 增强功能

use reqwest::Client;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::http_client::{GeminiApiUrl, ProxyConfig};
use crate::GLOBAL_DYNAMIC_CONFIG;

/// AI 服务错误
#[derive(Debug)]
pub enum AiServiceError {
    /// API 密钥未配置
    ApiKeyNotConfigured,
    /// 网络请求失败
    NetworkError(String),
    /// API 返回错误
    ApiError(String),
    /// JSON 解析错误
    ParseError(String),
    /// 速率限制
    RateLimited,
    /// 配额耗尽
    QuotaExceeded,
}

impl std::fmt::Display for AiServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiServiceError::ApiKeyNotConfigured => write!(f, "Gemini API key not configured"),
            AiServiceError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            AiServiceError::ApiError(msg) => write!(f, "API error: {}", msg),
            AiServiceError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            AiServiceError::RateLimited => write!(f, "Rate limited"),
            AiServiceError::QuotaExceeded => write!(f, "Quota exceeded"),
        }
    }
}

impl std::error::Error for AiServiceError {}

/// Gemini API 请求体
#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

/// Gemini API 响应体
#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    error: Option<GeminiError>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContentResponse>,
    #[serde(rename = "finishReason")]
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiContentResponse {
    parts: Option<Vec<GeminiPartResponse>>,
}

#[derive(Debug, Deserialize)]
struct GeminiPartResponse {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiError {
    code: Option<i32>,
    message: Option<String>,
    status: Option<String>,
}

/// AI 服务
pub struct AiService {
    client: Client,
    api_key: String,
    model: String,
}

impl AiService {
    /// 创建 AI 服务实例
    pub async fn new(_db: &DatabaseConnection) -> Result<Self, AiServiceError> {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;

        let api_key = config
            .gemini_api_key
            .clone()
            .filter(|k| !k.is_empty())
            .ok_or(AiServiceError::ApiKeyNotConfigured)?;

        let model = config.gemini_model.clone();

        // 创建 HTTP 客户端（带代理支持）
        let proxy_config = ProxyConfig::from_dynamic_config().await;

        // 设置更长的超时时间，因为 AI 请求可能较慢
        let client = if proxy_config.should_use_proxy() {
            if let Some(proxy_url) = &proxy_config.proxy_url {
                Client::builder()
                    .timeout(Duration::from_secs(120))
                    .connect_timeout(Duration::from_secs(30))
                    .user_agent("Myriad/1.0")
                    .proxy(
                        reqwest::Proxy::all(proxy_url)
                            .map_err(|e| AiServiceError::NetworkError(e.to_string()))?,
                    )
                    .build()
                    .map_err(|e| AiServiceError::NetworkError(e.to_string()))?
            } else {
                Client::builder()
                    .timeout(Duration::from_secs(120))
                    .connect_timeout(Duration::from_secs(30))
                    .user_agent("Myriad/1.0")
                    .build()
                    .map_err(|e| AiServiceError::NetworkError(e.to_string()))?
            }
        } else {
            Client::builder()
                .timeout(Duration::from_secs(120))
                .connect_timeout(Duration::from_secs(30))
                .user_agent("Myriad/1.0")
                .build()
                .map_err(|e| AiServiceError::NetworkError(e.to_string()))?
        };

        Ok(Self {
            client,
            api_key,
            model,
        })
    }

    /// 生成文本
    ///
    /// # Arguments
    /// * `prompt` - 提示文本
    /// * `max_tokens` - 最大输出 Token 数量
    pub async fn generate_text(
        &self,
        prompt: &str,
        max_tokens: Option<i32>,
    ) -> Result<String, AiServiceError> {
        let url = GeminiApiUrl::generate_content_url(&self.model).await;

        let request_body = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: Some(GenerationConfig {
                max_output_tokens: max_tokens,
                temperature: Some(0.7),
            }),
        };

        tracing::debug!("Calling Gemini API: {}", url);

        let response = self
            .client
            .post(&url)
            .query(&[("key", &self.api_key)])
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AiServiceError::NetworkError(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| AiServiceError::NetworkError(e.to_string()))?;

        tracing::debug!("Gemini API response status: {}", status);

        if !status.is_success() {
            // 尝试解析错误
            if let Ok(error_response) = serde_json::from_str::<GeminiResponse>(&body) {
                if let Some(error) = error_response.error {
                    let error_msg = format!(
                        "Gemini API error: {} (code: {:?})",
                        error.message.unwrap_or_default(),
                        error.code
                    );

                    // 检查是否是速率限制或配额问题
                    if error.code == Some(429) {
                        return Err(AiServiceError::RateLimited);
                    }
                    if let Some(status) = &error.status {
                        if status.contains("QUOTA") {
                            return Err(AiServiceError::QuotaExceeded);
                        }
                    }

                    return Err(AiServiceError::ApiError(error_msg));
                }
            }
            return Err(AiServiceError::ApiError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        // 解析成功响应
        let response: GeminiResponse = serde_json::from_str(&body)
            .map_err(|e| AiServiceError::ParseError(format!("Failed to parse response: {}", e)))?;

        // 提取文本
        let text = response
            .candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .and_then(|c| c.parts)
            .and_then(|p| p.into_iter().next())
            .and_then(|p| p.text)
            .ok_or_else(|| AiServiceError::ParseError("No text in response".to_string()))?;

        Ok(text)
    }
}
